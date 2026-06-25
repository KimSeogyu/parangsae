from __future__ import annotations

import json
import os
from argparse import Namespace
from pathlib import Path
from typing import Any, cast

import pandas as pd
import pytest

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_funding import FundingRateBundle
from parangsae.marketdata.binance_vision import MarketDataBundle
from parangsae.research.readiness import READINESS_CANDIDATE_REGISTRY, READINESS_CANDIDATE_REGISTRY_VERSION
from parangsae.research.tsmom import (
    TSMOM_ALLOWED_LOOP_VERDICTS,
    TSMOM_LOOP_VERDICT_NAMESPACE,
    TsmomCandidateSettings,
    TsmomLoopConfig,
    align_public_funding_to_decision_bars,
    build_tsmom_decision_frame,
    build_tsmom_loop_payload,
    default_tsmom_output_dir,
    render_tsmom_report,
    tsmom_candidate_grid,
    write_tsmom_loop_artifacts,
)


def datetime_index(index: pd.Index) -> pd.DatetimeIndex:
    return pd.DatetimeIndex(index)


def timestamp_at(index: pd.Index, position: int) -> pd.Timestamp:
    return cast(pd.Timestamp, pd.Timestamp(cast(Any, index[position])))


def synthetic_frame(rows: int = 600) -> pd.DataFrame:
    index = pd.date_range("2024-01-01", periods=rows, freq="1h", tz="UTC")
    trend = pd.Series(range(rows), index=index, dtype=float) * 0.03
    cycle = pd.Series([((i % 48) - 24) * 0.04 for i in range(rows)], index=index)
    close = 100 + trend + cycle
    return pd.DataFrame(
        {
            "Open": close.shift(1).fillna(close.iloc[0]),
            "High": close + 1.5,
            "Low": close - 1.5,
            "Close": close,
            "Volume": 1000 + (pd.Series(range(rows), index=index) % 37) * 11,
        },
        index=index,
    )


def funding_frame(index: pd.Index, rate: float = 0.0001) -> pd.DataFrame:
    funding_index = datetime_index(index)[::8]
    return pd.DataFrame(
        {
            "symbol": "BTCUSDT",
            "funding_time_ms": [int(timestamp_at(funding_index, offset).timestamp() * 1000) for offset in range(len(funding_index))],
            "funding_rate": [rate for _ in funding_index],
            "mark_price": [100.0 for _ in funding_index],
        },
        index=funding_index,
    )


def market_bundle(frame: pd.DataFrame) -> MarketDataBundle:
    return MarketDataBundle(
        symbol="BTCUSDT",
        interval="1h",
        months=("2024-01",),
        frame=frame,
        downloads=(),
        normalized_sha256="ohlcv-sha",
        quality={
            "first_timestamp": timestamp_at(frame.index, 0).isoformat().replace("+00:00", "Z"),
            "last_timestamp": timestamp_at(frame.index, -1).isoformat().replace("+00:00", "Z"),
            "closed_bar_only": True,
            "open_bar_included": False,
            "duplicates": 0,
            "expected_gap_seconds": 3600,
            "gap_tolerance_seconds": 0,
        },
    )


def funding_bundle(frame: pd.DataFrame) -> FundingRateBundle:
    return FundingRateBundle(
        symbol="BTCUSDT",
        frame=frame,
        downloads=(),
        normalized_sha256="funding-sha",
        quality={
            "rows": len(frame),
            "first_timestamp": timestamp_at(frame.index, 0).isoformat().replace("+00:00", "Z"),
            "last_timestamp": timestamp_at(frame.index, -1).isoformat().replace("+00:00", "Z"),
            "symbol": "BTCUSDT",
        },
    )


def small_config() -> TsmomLoopConfig:
    return TsmomLoopConfig(
        train_bars=120,
        validation_bars=60,
        holdout_bars=60,
        step_bars=60,
        min_research_grade_candidate_window_evaluations=10_000,
        min_research_grade_bars_processed_estimate=10_000_000,
    )


def test_tsmom_candidate_grid_is_frozen_and_readiness_registry_is_unchanged() -> None:
    grid = tsmom_candidate_grid()
    assert len(grid) == 48
    assert grid[0] == TsmomCandidateSettings(168, 168, 0.25, 0.50, 0.00025, False)
    assert grid[-1] == TsmomCandidateSettings(672, 336, 0.40, 0.80, 0.00050, False)
    assert READINESS_CANDIDATE_REGISTRY_VERSION == 1
    assert "TSMOM_VOL_TARGET_FUNDING_AWARE_V1" not in READINESS_CANDIDATE_REGISTRY["families"]


def test_funding_alignment_is_causal_and_not_backfilled() -> None:
    index = pd.date_range("2024-01-01", periods=6, freq="1h", tz="UTC")
    funding = pd.DataFrame(
        {
            "symbol": ["BTCUSDT", "BTCUSDT"],
            "funding_time_ms": [
                int(timestamp_at(index, 2).timestamp() * 1000),
                int(timestamp_at(index, 4).timestamp() * 1000),
            ],
            "funding_rate": [0.001, 0.002],
            "mark_price": [100.0, 100.0],
        },
        index=pd.DatetimeIndex([index[2], index[4]]),
    )
    aligned = align_public_funding_to_decision_bars(index, funding)
    assert aligned.tolist() == [0.0, 0.0, 0.0, 0.001, 0.001, 0.002]


def test_tsmom_decision_frame_is_prefix_invariant_for_future_shocks() -> None:
    frame = synthetic_frame(900)
    funding = funding_frame(frame.index, rate=0.0001)
    settings = TsmomCandidateSettings(24, 24, 0.25, 0.50, 0.00025, False)
    prefix = frame.iloc[:500].copy()
    shocked = frame.copy()
    shocked.iloc[500:, shocked.columns.get_loc("Close")] *= 7
    shocked.iloc[500:, shocked.columns.get_loc("High")] *= 7
    shocked.iloc[500:, shocked.columns.get_loc("Low")] *= 7
    prefix_decisions = build_tsmom_decision_frame(prefix, settings=settings, funding_frame=funding.loc[: prefix.index[-1]])
    shocked_decisions = build_tsmom_decision_frame(shocked, settings=settings, funding_frame=funding).loc[prefix.index]
    pd.testing.assert_series_equal(prefix_decisions["momentum"], shocked_decisions["momentum"])
    pd.testing.assert_series_equal(prefix_decisions["realized_volatility"], shocked_decisions["realized_volatility"])
    pd.testing.assert_series_equal(prefix_decisions["risk_multiplier"], shocked_decisions["risk_multiplier"])
    pd.testing.assert_series_equal(prefix_decisions["position"], shocked_decisions["position"])


def test_tsmom_loop_payload_records_hard_gates_and_claim_boundary() -> None:
    frame = synthetic_frame(420)
    payload = build_tsmom_loop_payload(
        bundle=market_bundle(frame),
        funding_bundle=None,
        config=small_config(),
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    assert "verdict" not in payload
    assert "loop_verdict" not in payload
    assert "decision" not in payload
    assert payload["tsmom_loop_decision"] in {"ITERATE_ON_SAME_HYPOTHESIS", "PROMOTE_TO_NEXT_LOOP"}
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["tsmom_loop_verdict_namespace"] == TSMOM_LOOP_VERDICT_NAMESPACE
    assert payload["tsmom_loop_verdict"] in {"WATCH", "BLOCKED"}
    assert payload["loop_contract"]["allowed_loop_verdicts"] == list(TSMOM_ALLOWED_LOOP_VERDICTS)
    assert payload["loop_contract"]["readiness_verdict"] is False
    assert payload["safety_gates"] == {
        "live_orders_submitted": False,
        "testnet_orders_submitted": False,
        "api_keys_required_for_research": False,
        "account_or_fill_data_used": False,
        "holdout_used_for_selection": False,
        "profit_or_live_ready_claim": False,
    }
    expected_gates = {
        "candidate_count",
        "candidate_window_evaluations",
        "bars_processed_estimate",
        "median_stress_return_beats_cash_null",
        "selected_candidate_switch_rate",
        "funding_aware_net_return",
        "max_drawdown_abs_pct",
        "tail_loss_cvar_pct",
    }
    assert set(payload["hard_gates"]) == expected_gates
    assert "funding_aware_net_return" in payload["failed_gates"]
    assert payload["hard_gates"]["funding_aware_net_return"]["status"] in {"failed", "unclaimed"}
    assert payload["validation_commands"] == ["uv run pytest tests/test_tsmom_loop.py -q"]
    assert payload["expected_failure_modes"]
    assert "median stress return" in payload["expected_failure_modes"][0]
    assert payload["evidence_artifacts"] == [
        "loop.json",
        "report.md",
        "commands.txt",
        "changed_files.txt",
        "tsmom_windows.csv",
        "tsmom_candidates.csv",
    ]
    assert payload["dataset_provenance"][0]["account_or_signed_data_used"] is False
    assert "not live readiness" in payload["claim_boundary"]
    report = render_tsmom_report(payload)
    assert "## TSMOM loop decision" in report
    assert "## Decision" not in report
    assert "tsmom_loop_decision" in report
    assert "funding_aware_net_return: UNCLAIMED" in report


def test_tsmom_artifact_writer_refuses_to_overwrite_readiness_artifacts(tmp_path: Path) -> None:
    readiness_dir = tmp_path / "readiness-run-20260625T053627Z"
    readiness_dir.mkdir()
    (readiness_dir / "readiness.json").write_text("{}\n")
    (readiness_dir / "manifest.json").write_text(json.dumps({"run_type": "readiness-run"}) + "\n")
    payload = build_tsmom_loop_payload(bundle=market_bundle(synthetic_frame(300)), config=small_config())
    with pytest.raises(FileExistsError, match="readiness"):
        write_tsmom_loop_artifacts(readiness_dir, payload, overwrite=True)


def test_tsmom_cli_writes_paper_only_loop_artifacts_without_credentials(monkeypatch, tmp_path: Path) -> None:
    frame = synthetic_frame(360)
    funding = funding_bundle(funding_frame(frame.index))
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **kwargs: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **kwargs: funding)
    monkeypatch.setattr(quantctl, "funding_metadata_for_report", lambda bundle: {"source": "public_binance_usdm_funding_history", "rows": len(bundle.frame)})
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"tsmom-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    output_dir = tmp_path / "reports" / "loops" / "20260625-tsmom-vol-target-funding-aware-v1"
    args = Namespace(
        symbol="BTCUSDT",
        interval="1h",
        months="2024-01",
        cache_dir=str(tmp_path / "cache"),
        funding_source="public-binance",
        funding_cache_dir=str(tmp_path / "funding"),
        reports_root=str(tmp_path / "reports"),
        output_dir=str(output_dir),
        overwrite=False,
        command=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_file=["src/parangsae/research/tsmom.py"],
    )
    assert quantctl.cmd_tsmom_run(args) == 0
    loop_json = json.loads((output_dir / "loop.json").read_text())
    assert loop_json["readiness_baseline_verdict"] == "NOT_READY"
    assert loop_json["tsmom_loop_verdict_namespace"] == TSMOM_LOOP_VERDICT_NAMESPACE
    assert loop_json["safety_gates"]["live_orders_submitted"] is False
    assert (output_dir / "report.md").exists()
    assert (output_dir / "commands.txt").read_text().strip() == "uv run pytest tests/test_tsmom_loop.py -q"
    assert default_tsmom_output_dir(root=tmp_path / "reports" / "loops", today="20260625") == output_dir
