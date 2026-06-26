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
    REGIME_ONLY_TSMOM_FREEZE_NEGATIVE_CONTROL,
    REGIME_ONLY_TSMOM_FALSIFICATION_NAMESPACE,
    TSMOM_ALLOWED_LOOP_VERDICTS,
    TSMOM_DIAGNOSTIC_VERDICT_NAMESPACE,
    TSMOM_FAMILY_DECISION_NAMESPACE,
    TSMOM_FAMILY_DECISION_REGIME_ONLY_WATCH,
    TSMOM_GROSS_EDGE_VERDICT_NAMESPACE,
    TSMOM_LOOP_VERDICT_NAMESPACE,
    TSMOM_RISK_VERDICT_NAMESPACE,
    TsmomCandidateSettings,
    TsmomLoopConfig,
    align_public_funding_to_decision_bars,
    build_regime_only_tsmom_falsification_payload,
    build_tsmom_candidate_family_decision_payload,
    build_tsmom_diagnostic_payload,
    build_tsmom_decision_frame,
    build_tsmom_gross_edge_loop_payload,
    build_tsmom_loop_payload,
    build_tsmom_risk_loop_payload,
    default_regime_only_tsmom_falsification_output_dir,
    default_tsmom_diagnostic_output_dir,
    default_tsmom_family_decision_output_dir,
    default_tsmom_gross_edge_output_dir,
    default_tsmom_output_dir,
    default_tsmom_risk_output_dir,
    render_tsmom_report,
    tsmom_candidate_grid,
    write_regime_only_tsmom_falsification_artifacts,
    write_tsmom_candidate_family_decision_artifacts,
    write_tsmom_diagnostic_artifacts,
    write_tsmom_gross_edge_loop_artifacts,
    write_tsmom_loop_artifacts,
    write_tsmom_risk_loop_artifacts,
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


def test_tsmom_diagnostic_cli_writes_namespaced_artifacts_without_credentials(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    frame = synthetic_frame(360)
    funding = funding_bundle(funding_frame(frame.index))
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **kwargs: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **kwargs: funding)
    monkeypatch.setattr(
        quantctl,
        "funding_metadata_for_report",
        lambda bundle: {"source": "public_binance_usdm_funding_history", "rows": len(bundle.frame)},
    )
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"tsmom-diagnostic-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    output_dir = tmp_path / "reports" / "loops" / "20260626-tsmom-stress-switch-funding-diagnostic-v1"
    parser = quantctl.build_parser()
    parsed = parser.parse_args(
        [
            "tsmom-diagnostic-run",
            "--symbol",
            "BTCUSDT",
            "--interval",
            "1h",
            "--months",
            "2024-01",
            "--cache-dir",
            str(tmp_path / "cache"),
            "--funding-source",
            "public-binance",
            "--funding-cache-dir",
            str(tmp_path / "funding"),
            "--reports-root",
            str(tmp_path / "reports"),
            "--output-dir",
            str(output_dir),
            "--command",
            "uv run pytest tests/test_tsmom_loop.py -q",
            "--changed-file",
            "src/parangsae/research/tsmom.py",
        ]
    )
    assert parsed.func is quantctl.cmd_tsmom_diagnostic_run
    assert quantctl.cmd_tsmom_diagnostic_run(parsed) == 0
    cli_payload = json.loads(capsys.readouterr().out)
    assert "verdict" not in cli_payload
    assert "loop_verdict" not in cli_payload
    assert cli_payload["tsmom_diagnostic_loop_verdict_namespace"] == TSMOM_DIAGNOSTIC_VERDICT_NAMESPACE
    assert cli_payload["readiness_baseline_verdict"] == "NOT_READY"
    assert cli_payload["live_orders_submitted"] is False
    assert cli_payload["testnet_orders_submitted"] is False
    diagnostic = json.loads((output_dir / "diagnostic.json").read_text())
    assert "verdict" not in diagnostic
    assert "loop_verdict" not in diagnostic
    assert "decision" not in diagnostic
    assert diagnostic["readiness_baseline_verdict"] == "NOT_READY"
    assert diagnostic["tsmom_diagnostic_loop_verdict_namespace"] == TSMOM_DIAGNOSTIC_VERDICT_NAMESPACE
    assert diagnostic["safety_gates"]["account_or_fill_data_used"] is False
    assert diagnostic["safety_gates"]["api_keys_required_for_research"] is False
    assert "API-key" in diagnostic["claim_boundary"]
    assert (output_dir / "failed_windows.csv").exists()
    assert (output_dir / "candidate_switches.csv").exists()
    assert (output_dir / "funding_decomposition.csv").exists()
    assert default_tsmom_diagnostic_output_dir(
        root=tmp_path / "reports" / "loops",
        today="20260626",
    ) == output_dir


def test_tsmom_diagnostic_payload_is_namespaced_and_preserves_failed_gate_boundary() -> None:
    frame = synthetic_frame(420)
    funding = funding_bundle(funding_frame(frame.index))
    payload = build_tsmom_diagnostic_payload(
        bundle=market_bundle(frame),
        funding_bundle=funding,
        config=small_config(),
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    assert "verdict" not in payload
    assert "loop_verdict" not in payload
    assert "decision" not in payload
    assert payload["tsmom_diagnostic_loop_verdict_namespace"] == TSMOM_DIAGNOSTIC_VERDICT_NAMESPACE
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["loop_contract"]["readiness_verdict"] is False
    assert payload["source_tsmom_loop"]["readiness_baseline_verdict"] == "NOT_READY"
    assert set(payload["analyzed_failed_gates"]) == {
        "median_stress_return_beats_cash_null",
        "selected_candidate_switch_rate",
        "funding_aware_net_return",
    }
    assert payload["source_tsmom_loop"]["failed_gates"]
    assert payload["funding_decomposition"]
    assert payload["candidate_switches"]
    assert payload["diagnostic_summary"]["stress_return_distribution"]["count"] > 0
    assert payload["diagnostic_summary"]["candidate_switch_summary"]["switch_rate"] >= 0.0
    assert all(value is False for value in payload["safety_gates"].values())
    assert "not readiness" in payload["claim_boundary"]
    assert "not a profit guarantee" in payload["claim_boundary"]


def test_tsmom_diagnostic_artifacts_are_generated_and_report_is_safe(tmp_path: Path) -> None:
    payload = build_tsmom_diagnostic_payload(
        bundle=market_bundle(synthetic_frame(420)),
        funding_bundle=None,
        config=small_config(),
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    output_dir = tmp_path / "reports" / "loops" / "20260626-tsmom-stress-switch-funding-diagnostic-v1"
    written = write_tsmom_diagnostic_artifacts(output_dir, payload)
    assert written == output_dir
    expected = {
        "diagnostic.json",
        "report.md",
        "failed_windows.csv",
        "candidate_switches.csv",
        "funding_decomposition.csv",
        "commands.txt",
        "changed_files.txt",
    }
    assert {path.name for path in output_dir.iterdir()} == expected
    diagnostic = json.loads((output_dir / "diagnostic.json").read_text())
    assert "verdict" not in diagnostic
    assert "loop_verdict" not in diagnostic
    assert diagnostic["readiness_baseline_verdict"] == "NOT_READY"
    assert "funding_aware_net_return" in diagnostic["analyzed_failed_gates"]
    report = (output_dir / "report.md").read_text()
    assert "diagnostic-only, not readiness" in report
    assert "not permission for live orders" in report
    assert "profit guarantee" in report
    assert "## Funding-aware net return diagnostic" in report
    assert "## Candidate switch diagnostic" in report
    assert "live_orders_submitted: false" in report
    assert "testnet_orders_submitted: false" in report


def test_tsmom_risk_loop_payload_records_pre_registration_and_preserves_boundaries() -> None:
    frame = synthetic_frame(420)
    funding = funding_bundle(funding_frame(frame.index, rate=0.0002))
    payload = build_tsmom_risk_loop_payload(
        bundle=market_bundle(frame),
        funding_bundle=funding,
        config=small_config(),
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    assert "verdict" not in payload
    assert "loop_verdict" not in payload
    assert "decision" not in payload
    assert payload["tsmom_risk_loop_verdict_namespace"] == TSMOM_RISK_VERDICT_NAMESPACE
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["loop_contract"]["readiness_verdict"] is False
    assert "api_keys" in payload["pre_registration"]["forbidden_inputs"]
    assert "holdout_for_parameter_selection" in payload["pre_registration"]["forbidden_inputs"]
    assert payload["pre_registration"]["candidate_risk_rules"]["near_tie_sticky_selection"]["score_margin"] == 0.05
    assert payload["source_failed_gates_preserved"] == payload["source_tsmom_failed_gates"]
    assert set(payload["source_hard_gates"]) == {
        "candidate_count",
        "candidate_window_evaluations",
        "bars_processed_estimate",
        "median_stress_return_beats_cash_null",
        "selected_candidate_switch_rate",
        "funding_aware_net_return",
        "max_drawdown_abs_pct",
        "tail_loss_cvar_pct",
    }
    assert set(payload["source_failed_hard_gates"]) == set(payload["source_tsmom_failed_gates"])
    assert set(payload["risk_evaluation_gates"]) == {
        "candidate_count",
        "candidate_window_evaluations",
        "bars_processed_estimate",
        "median_stress_return_beats_cash_null",
        "selected_candidate_switch_rate",
        "funding_aware_net_return",
        "max_drawdown_abs_pct",
        "tail_loss_cvar_pct",
    }
    assert payload["candidate_switch_stability"]
    assert payload["funding_throttle_decomposition"]
    assert payload["regime_breakdown"]
    assert payload["switch_stability_summary"]["stable"]["switch_rate"] <= payload["switch_stability_summary"]["baseline"][
        "switch_rate"
    ]
    first_switch_row = payload["candidate_switch_stability"][0]
    assert "top_vs_runner_up_score_gap" in first_switch_row
    assert "top_vs_previous_stable_score_gap" in first_switch_row
    assert "sticky_selection_triggered" in first_switch_row
    assert "sticky_selection_eligible" in first_switch_row
    assert any(row["funding_throttle_bars"] > 0 for row in payload["funding_throttle_decomposition"])
    assert all(value is False for value in payload["safety_gates"].values())
    assert "not permission for live orders or testnet orders" in payload["claim_boundary"]
    assert "no account, API-key" in payload["claim_boundary"]


def test_tsmom_risk_loop_output_is_deterministic_for_rows() -> None:
    frame = synthetic_frame(420)
    funding = funding_bundle(funding_frame(frame.index, rate=0.0002))
    first = build_tsmom_risk_loop_payload(bundle=market_bundle(frame), funding_bundle=funding, config=small_config())
    second = build_tsmom_risk_loop_payload(bundle=market_bundle(frame), funding_bundle=funding, config=small_config())
    assert first["candidate_switch_stability"] == second["candidate_switch_stability"]
    assert first["funding_throttle_decomposition"] == second["funding_throttle_decomposition"]
    assert first["regime_breakdown"] == second["regime_breakdown"]


def test_tsmom_risk_loop_artifacts_are_generated_and_report_is_safe(tmp_path: Path) -> None:
    frame = synthetic_frame(420)
    payload = build_tsmom_risk_loop_payload(
        bundle=market_bundle(frame),
        funding_bundle=funding_bundle(funding_frame(frame.index, rate=0.0002)),
        config=small_config(),
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    output_dir = tmp_path / "reports" / "loops" / "20260626-tsmom-switch-stability-funding-throttle-risk-v1"
    written = write_tsmom_risk_loop_artifacts(output_dir, payload)
    assert written == output_dir
    expected = {
        "risk_loop.json",
        "report.md",
        "candidate_switch_stability.csv",
        "funding_throttle_decomposition.csv",
        "regime_breakdown.csv",
        "commands.txt",
        "changed_files.txt",
    }
    assert {path.name for path in output_dir.iterdir()} == expected
    risk_loop = json.loads((output_dir / "risk_loop.json").read_text())
    assert "verdict" not in risk_loop
    assert "loop_verdict" not in risk_loop
    assert "decision" not in risk_loop
    assert risk_loop["loop_id"] == "20260626-tsmom-switch-stability-funding-throttle-risk-v1"
    assert risk_loop["readiness_baseline_verdict"] == "NOT_READY"
    assert risk_loop["tsmom_risk_loop_verdict_namespace"] == TSMOM_RISK_VERDICT_NAMESPACE
    report = (output_dir / "report.md").read_text()
    assert "## Switch stability" in report
    assert "## Funding throttle comparison" in report
    assert "not permission for live orders or testnet orders" in report
    assert "live_trading_path_introduced: false" in report


def test_tsmom_risk_cli_writes_artifacts_without_credentials(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    frame = synthetic_frame(360)
    funding = funding_bundle(funding_frame(frame.index, rate=0.0002))
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **kwargs: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **kwargs: funding)
    monkeypatch.setattr(
        quantctl,
        "funding_metadata_for_report",
        lambda bundle: {"source": "public_binance_usdm_funding_history", "rows": len(bundle.frame)},
    )
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"tsmom-risk-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    output_dir = tmp_path / "reports" / "loops" / "20260626-tsmom-switch-stability-funding-throttle-risk-v1"
    parsed = quantctl.build_parser().parse_args(
        [
            "tsmom-risk-run",
            "--symbol",
            "BTCUSDT",
            "--interval",
            "1h",
            "--months",
            "2024-01",
            "--cache-dir",
            str(tmp_path / "cache"),
            "--funding-source",
            "public-binance",
            "--funding-cache-dir",
            str(tmp_path / "funding"),
            "--reports-root",
            str(tmp_path / "reports"),
            "--output-dir",
            str(output_dir),
            "--command",
            "uv run pytest tests/test_tsmom_loop.py -q",
            "--changed-file",
            "src/parangsae/research/tsmom.py",
        ]
    )
    assert parsed.func is quantctl.cmd_tsmom_risk_run
    assert quantctl.cmd_tsmom_risk_run(parsed) == 0
    cli_payload = json.loads(capsys.readouterr().out)
    assert "verdict" not in cli_payload
    assert "loop_verdict" not in cli_payload
    assert cli_payload["tsmom_risk_loop_verdict_namespace"] == TSMOM_RISK_VERDICT_NAMESPACE
    assert cli_payload["readiness_baseline_verdict"] == "NOT_READY"
    assert cli_payload["live_orders_submitted"] is False
    assert cli_payload["testnet_orders_submitted"] is False
    risk_loop = json.loads((output_dir / "risk_loop.json").read_text())
    assert risk_loop["safety_gates"]["api_keys_required_for_research"] is False
    assert risk_loop["safety_gates"]["account_or_fill_data_used"] is False
    assert risk_loop["safety_gates"]["gates_relaxed"] is False
    assert (output_dir / "candidate_switch_stability.csv").exists()
    assert (output_dir / "funding_throttle_decomposition.csv").exists()
    assert (output_dir / "regime_breakdown.csv").exists()
    assert default_tsmom_risk_output_dir(root=tmp_path / "reports" / "loops", today="20260626") == output_dir


def test_tsmom_gross_edge_loop_payload_records_schema_and_claim_boundary() -> None:
    frame = synthetic_frame(420)
    funding = funding_bundle(funding_frame(frame.index, rate=0.0002))
    payload = build_tsmom_gross_edge_loop_payload(
        bundle=market_bundle(frame),
        funding_bundle=funding,
        config=small_config(),
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    assert "verdict" not in payload
    assert "loop_verdict" not in payload
    assert "decision" not in payload
    assert payload["tsmom_gross_edge_loop_verdict_namespace"] == TSMOM_GROSS_EDGE_VERDICT_NAMESPACE
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["source_tsmom_loop_verdict"] in {"WATCH", "BLOCKED"}
    assert payload["loop_contract"]["readiness_verdict"] is False
    assert "api_keys" in payload["pre_registration"]["forbidden_inputs"]
    assert "holdout_for_parameter_selection" in payload["pre_registration"]["forbidden_inputs"]
    assert payload["source_failed_gates_preserved"] == payload["source_tsmom_failed_gates"]
    assert set(payload["gross_edge_evaluation_gates"]) == {
        "median_gross_return_beats_cash_null",
        "gross_edge_hit_rate",
        "median_gross_beats_random_sign_baseline",
        "median_gross_beats_lagged_signal_baseline",
        "median_gross_beats_inverse_signal_baseline",
        "median_gross_beats_buy_and_hold_baseline",
        "source_failed_gates_preserved",
    }
    assert payload["gross_edge_decomposition"]
    assert payload["regime_edge_breakdown"]
    assert payload["null_baseline_comparison"]
    assert payload["failed_edge_windows"]
    first_row = payload["gross_edge_decomposition"][0]
    assert "gross_return_before_costs_pct" in first_row
    assert "net_return_after_funding_slippage_stress_pct" in first_row
    assert "difference_vs_cash_null_pct" in first_row
    assert "direction_correctness_rate" in first_row
    assert "funding_drag_regime" in first_row
    assert "trend_regime" in first_row
    baselines = {row["baseline_name"] for row in payload["null_baseline_comparison"]}
    assert baselines == {"cash_null", "random_sign", "lagged_signal", "inverse_signal", "buy_and_hold"}
    assert all(value is False for value in payload["safety_gates"].values())
    assert "not permission for live orders or testnet orders" in payload["claim_boundary"]
    assert "no live, testnet, account, API-key" in payload["claim_boundary"]


def test_tsmom_gross_edge_regime_classification_and_null_comparison_are_present() -> None:
    frame = synthetic_frame(420)
    payload = build_tsmom_gross_edge_loop_payload(
        bundle=market_bundle(frame),
        funding_bundle=funding_bundle(funding_frame(frame.index, rate=0.0002)),
        config=small_config(),
    )
    regime_axes = {row["regime_axis"] for row in payload["regime_edge_breakdown"]}
    assert {"market_regime", "volatility_regime", "funding_regime", "funding_drag_regime", "trend_regime"}.issubset(
        regime_axes
    )
    assert payload["gross_edge_summary"]["window_count"] == len(payload["gross_edge_decomposition"])
    by_baseline = payload["null_robustness_summary"]["by_baseline"]
    assert by_baseline["random_sign"]["window_count"] == payload["gross_edge_summary"]["window_count"]
    assert by_baseline["cash_null"]["median_baseline_return_pct"] == 0.0
    assert all(row["deterministic_seed"] is not None for row in payload["null_baseline_comparison"] if row["baseline_name"] == "random_sign")


def test_tsmom_gross_edge_loop_output_is_deterministic_for_rows() -> None:
    frame = synthetic_frame(420)
    funding = funding_bundle(funding_frame(frame.index, rate=0.0002))
    first = build_tsmom_gross_edge_loop_payload(bundle=market_bundle(frame), funding_bundle=funding, config=small_config())
    second = build_tsmom_gross_edge_loop_payload(bundle=market_bundle(frame), funding_bundle=funding, config=small_config())
    assert first["gross_edge_decomposition"] == second["gross_edge_decomposition"]
    assert first["regime_edge_breakdown"] == second["regime_edge_breakdown"]
    assert first["null_baseline_comparison"] == second["null_baseline_comparison"]
    assert first["failed_edge_windows"] == second["failed_edge_windows"]


def test_tsmom_gross_edge_artifacts_are_generated_report_is_safe_and_overwrite_guarded(tmp_path: Path) -> None:
    frame = synthetic_frame(420)
    payload = build_tsmom_gross_edge_loop_payload(
        bundle=market_bundle(frame),
        funding_bundle=funding_bundle(funding_frame(frame.index, rate=0.0002)),
        config=small_config(),
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    output_dir = tmp_path / "reports" / "loops" / "20260626-tsmom-gross-edge-regime-null-robustness-v1"
    written = write_tsmom_gross_edge_loop_artifacts(output_dir, payload)
    assert written == output_dir
    expected = {
        "gross_edge_loop.json",
        "report.md",
        "gross_edge_decomposition.csv",
        "regime_edge_breakdown.csv",
        "null_baseline_comparison.csv",
        "failed_edge_windows.csv",
        "commands.txt",
        "changed_files.txt",
    }
    assert {path.name for path in output_dir.iterdir()} == expected
    gross_edge_loop = json.loads((output_dir / "gross_edge_loop.json").read_text())
    assert "verdict" not in gross_edge_loop
    assert "loop_verdict" not in gross_edge_loop
    assert "decision" not in gross_edge_loop
    assert gross_edge_loop["loop_id"] == "20260626-tsmom-gross-edge-regime-null-robustness-v1"
    assert gross_edge_loop["readiness_baseline_verdict"] == "NOT_READY"
    report = (output_dir / "report.md").read_text()
    assert "## Gross edge summary" in report
    assert "## Null robustness" in report
    assert "not permission for live orders or testnet orders" in report
    assert "live_trading_path_introduced: false" in report
    with pytest.raises(FileExistsError, match="output directory already exists"):
        write_tsmom_gross_edge_loop_artifacts(output_dir, payload)
    assert default_tsmom_gross_edge_output_dir(root=tmp_path / "reports" / "loops", today="20260626") == output_dir


def test_tsmom_gross_edge_cli_writes_artifacts_without_credentials(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    frame = synthetic_frame(360)
    funding = funding_bundle(funding_frame(frame.index, rate=0.0002))
    monkeypatch.setattr(quantctl, "load_market_data_bundle", lambda **kwargs: market_bundle(frame))
    monkeypatch.setattr(quantctl, "fetch_funding_rate_history", lambda **kwargs: funding)
    monkeypatch.setattr(
        quantctl,
        "funding_metadata_for_report",
        lambda bundle: {"source": "public_binance_usdm_funding_history", "rows": len(bundle.frame)},
    )
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"tsmom-gross-edge-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    output_dir = tmp_path / "reports" / "loops" / "20260626-tsmom-gross-edge-regime-null-robustness-v1"
    parsed = quantctl.build_parser().parse_args(
        [
            "tsmom-gross-edge-run",
            "--symbol",
            "BTCUSDT",
            "--interval",
            "1h",
            "--months",
            "2024-01",
            "--cache-dir",
            str(tmp_path / "cache"),
            "--funding-source",
            "public-binance",
            "--funding-cache-dir",
            str(tmp_path / "funding"),
            "--reports-root",
            str(tmp_path / "reports"),
            "--output-dir",
            str(output_dir),
            "--command",
            "uv run pytest tests/test_tsmom_loop.py -q",
            "--changed-file",
            "src/parangsae/research/tsmom.py",
        ]
    )
    assert parsed.func is quantctl.cmd_tsmom_gross_edge_run
    assert quantctl.cmd_tsmom_gross_edge_run(parsed) == 0
    cli_payload = json.loads(capsys.readouterr().out)
    assert "verdict" not in cli_payload
    assert "loop_verdict" not in cli_payload
    assert cli_payload["tsmom_gross_edge_loop_verdict_namespace"] == TSMOM_GROSS_EDGE_VERDICT_NAMESPACE
    assert cli_payload["readiness_baseline_verdict"] == "NOT_READY"
    assert cli_payload["live_orders_submitted"] is False
    assert cli_payload["testnet_orders_submitted"] is False
    gross_edge_loop = json.loads((output_dir / "gross_edge_loop.json").read_text())
    assert gross_edge_loop["safety_gates"]["api_keys_required_for_research"] is False
    assert gross_edge_loop["safety_gates"]["account_or_fill_data_used"] is False
    assert gross_edge_loop["safety_gates"]["gates_relaxed"] is False
    assert (output_dir / "gross_edge_decomposition.csv").exists()
    assert (output_dir / "regime_edge_breakdown.csv").exists()
    assert (output_dir / "null_baseline_comparison.csv").exists()
    assert (output_dir / "failed_edge_windows.csv").exists()


def test_tsmom_candidate_family_decision_payload_rolls_up_prior_artifacts_and_preserves_boundaries() -> None:
    payload = build_tsmom_candidate_family_decision_payload(
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    assert "verdict" not in payload
    assert "loop_verdict" not in payload
    assert "decision" not in payload
    assert payload["tsmom_candidate_family_decision_namespace"] == TSMOM_FAMILY_DECISION_NAMESPACE
    assert payload["tsmom_candidate_family_decision"] == TSMOM_FAMILY_DECISION_REGIME_ONLY_WATCH
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["source_tsmom_loop_verdict"] == "WATCH"
    assert payload["tsmom_risk_loop_verdict"] == "WATCH"
    assert payload["tsmom_gross_edge_loop_verdict"] == "WATCH"
    assert payload["loop_contract"]["readiness_verdict"] is False
    assert "api_keys" in payload["pre_registration"]["forbidden_inputs"]
    assert "new_parameter_optimization" in payload["pre_registration"]["forbidden_inputs"]
    assert "median_stress_return_beats_cash_null" in {row["failed_gate"] for row in payload["failed_gate_rollup"]}
    assert "median_gross_return_pct" in {row["metric_name"] for row in payload["evidence_rollup"]}
    assert payload["evidence_summary"]["median_gross_return_pct"] < 0.0
    assert payload["evidence_summary"]["gross_edge_hit_rate"] < 0.5
    assert payload["evidence_summary"]["null_baselines_matching_or_beating_tsmom"]
    assert all(value is False for value in payload["safety_gates"].values())
    assert "research-governance loop" in payload["claim_boundary"]
    assert "not strategy optimization" in payload["claim_boundary"]
    assert "no live, testnet, account, API-key" in payload["claim_boundary"]


def test_tsmom_candidate_family_decision_gates_select_regime_only_not_broad_watch() -> None:
    payload = build_tsmom_candidate_family_decision_payload()
    gates = payload["decision_gates"]
    assert gates["readiness_boundary_not_live_ready"]["passed"] is True
    assert gates["active_keep_watch_gross_edge"]["passed"] is False
    assert gates["active_keep_watch_null_robustness"]["passed"] is False
    assert gates["regime_only_watch_exception"]["passed"] is True
    assert gates["negative_control_utility"]["passed"] is True
    assert gates["failed_gates_preserved"]["passed"] is True
    assert payload["tsmom_candidate_family_decision"] == "REGIME_ONLY_WATCH"
    assert "removed from broad active development" in payload["candidate_queue_status"]
    qualified = [row for row in payload["regime_exception_review"] if row["qualifies_for_regime_only_watch"]]
    assert qualified
    assert {f"{row['regime_axis']}={row['regime_label']}" for row in qualified} >= {
        "market_regime=sideways",
        "volatility_regime=low-vol",
    }


def test_tsmom_candidate_family_decision_artifacts_are_generated_and_report_is_safe(tmp_path: Path) -> None:
    payload = build_tsmom_candidate_family_decision_payload(
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    output_dir = tmp_path / "reports" / "loops" / "20260626-tsmom-candidate-family-decision-v1"
    written = write_tsmom_candidate_family_decision_artifacts(output_dir, payload)
    assert written == output_dir
    expected = {
        "candidate_family_decision.json",
        "report.md",
        "evidence_rollup.csv",
        "failed_gate_rollup.csv",
        "regime_exception_review.csv",
        "decision_rationale.md",
        "commands.txt",
        "changed_files.txt",
    }
    assert {path.name for path in output_dir.iterdir()} == expected
    decision = json.loads((output_dir / "candidate_family_decision.json").read_text())
    assert "verdict" not in decision
    assert "loop_verdict" not in decision
    assert "decision" not in decision
    assert decision["loop_id"] == "20260626-tsmom-candidate-family-decision-v1"
    assert decision["readiness_baseline_verdict"] == "NOT_READY"
    assert decision["tsmom_candidate_family_decision"] == "REGIME_ONLY_WATCH"
    report = (output_dir / "report.md").read_text()
    assert "## Decision gates" in report
    assert "readiness_baseline_verdict: NOT_READY" in report
    assert "not trading advice" in report
    rationale = (output_dir / "decision_rationale.md").read_text()
    assert "broad KEEP_WATCH did not pass" in rationale
    with pytest.raises(FileExistsError, match="output directory already exists"):
        write_tsmom_candidate_family_decision_artifacts(output_dir, payload)
    assert default_tsmom_family_decision_output_dir(root=tmp_path / "reports" / "loops", today="20260626") == output_dir


def test_tsmom_candidate_family_decision_cli_writes_artifacts_without_credentials(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"tsmom-family-decision-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    output_dir = tmp_path / "reports" / "loops" / "20260626-tsmom-candidate-family-decision-v1"
    parsed = quantctl.build_parser().parse_args(
        [
            "tsmom-family-decision-run",
            "--reports-root",
            str(tmp_path / "reports"),
            "--output-dir",
            str(output_dir),
            "--command",
            "uv run pytest tests/test_tsmom_loop.py -q",
            "--changed-file",
            "src/parangsae/research/tsmom.py",
        ]
    )
    assert parsed.func is quantctl.cmd_tsmom_family_decision_run
    assert quantctl.cmd_tsmom_family_decision_run(parsed) == 0
    cli_payload = json.loads(capsys.readouterr().out)
    assert "verdict" not in cli_payload
    assert "loop_verdict" not in cli_payload
    assert cli_payload["tsmom_candidate_family_decision_namespace"] == TSMOM_FAMILY_DECISION_NAMESPACE
    assert cli_payload["tsmom_candidate_family_decision"] == "REGIME_ONLY_WATCH"
    assert cli_payload["readiness_baseline_verdict"] == "NOT_READY"
    assert cli_payload["live_orders_submitted"] is False
    assert cli_payload["testnet_orders_submitted"] is False
    decision = json.loads((output_dir / "candidate_family_decision.json").read_text())
    assert decision["safety_gates"]["api_keys_required_for_research"] is False
    assert decision["safety_gates"]["account_or_fill_data_used"] is False
    assert decision["safety_gates"]["gates_relaxed"] is False
    assert (output_dir / "evidence_rollup.csv").exists()
    assert (output_dir / "failed_gate_rollup.csv").exists()
    assert (output_dir / "regime_exception_review.csv").exists()


def test_tsmom_candidate_family_decision_fails_fast_on_wrong_prior_artifact(tmp_path: Path) -> None:
    wrong = tmp_path / "wrong.json"
    wrong.write_text(json.dumps({"tsmom_risk_loop_verdict": "WATCH"}) + "\n")
    with pytest.raises(ValueError, match="missing required key"):
        build_tsmom_candidate_family_decision_payload(source_loop_artifact=wrong)


def test_regime_only_tsmom_falsification_payload_preserves_boundaries_and_marks_negative_control() -> None:
    payload = build_regime_only_tsmom_falsification_payload(
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    assert "verdict" not in payload
    assert "loop_verdict" not in payload
    assert "decision" not in payload
    assert payload["regime_only_tsmom_falsification_namespace"] == REGIME_ONLY_TSMOM_FALSIFICATION_NAMESPACE
    assert payload["regime_only_tsmom_falsification_decision"] == REGIME_ONLY_TSMOM_FREEZE_NEGATIVE_CONTROL
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["source_candidate_family_decision"] == "REGIME_ONLY_WATCH"
    assert payload["slice_definition"]["parameter_retune_performed"] is False
    assert payload["slice_definition"]["holdout_reselection_performed"] is False
    assert payload["summary"]["slice_window_count"] == 3
    assert payload["summary"]["total_window_count"] == 31
    assert payload["summary"]["median_gross_return_pct"] > 0.0
    assert payload["summary"]["median_net_return_pct"] < 0.0
    assert payload["summary"]["simpler_trend_baseline_status"] == "unavailable_unclaimed"
    assert payload["decision_gates"]["regime_slice_sample_size"]["passed"] is False
    assert payload["decision_gates"]["median_gross_return_positive"]["passed"] is True
    assert payload["decision_gates"]["cost_adjusted_net_return_beats_cash_null"]["passed"] is False
    assert payload["decision_gates"]["baseline_null_robustness"]["passed"] is False
    assert payload["decision_gates"]["simpler_trend_baseline_available"]["status"] == "unclaimed"
    assert all(value is False for value in payload["safety_gates"].values())
    assert "does not optimize parameters" in payload["claim_boundary"]
    assert "no live, testnet, account, API-key" in payload["claim_boundary"]


def test_regime_only_tsmom_falsification_rollup_contains_slice_costs_and_nulls() -> None:
    payload = build_regime_only_tsmom_falsification_payload()
    row_types = {row["row_type"] for row in payload["evidence_rollup"]}
    assert {"summary", "slice_window", "null_baseline"}.issubset(row_types)
    windows = [row for row in payload["evidence_rollup"] if row["row_type"] == "slice_window"]
    assert {row["window_id"] for row in windows} == {"W022", "W023", "W027"}
    assert all(row["market_regime"] == "sideways" for row in windows)
    assert all(row["volatility_regime"] == "low-vol" for row in windows)
    baselines = {row["baseline_name"] for row in payload["evidence_rollup"] if row["row_type"] == "null_baseline"}
    assert {"cash_null", "buy_and_hold", "random_sign", "lagged_signal", "inverse_signal", "simpler_trend_baseline"}.issubset(baselines)
    lagged = payload["summary"]["null_summary"]["lagged_signal"]
    assert lagged["baseline_beats_or_matches_tsmom_count"] == 2
    assert lagged["median_tsmom_minus_baseline_return_pct"] < 0.0
    failed_names = {row["failed_gate"] for row in payload["failed_gate_rollup"]}
    assert "median_stress_return_beats_cash_null" in failed_names
    assert "cost_adjusted_net_return_beats_cash_null" in failed_names


def test_regime_only_tsmom_falsification_artifacts_are_generated_and_overwrite_guarded(tmp_path: Path) -> None:
    payload = build_regime_only_tsmom_falsification_payload(
        commands=["uv run pytest tests/test_tsmom_loop.py -q"],
        changed_files=["src/parangsae/research/tsmom.py"],
    )
    output_dir = tmp_path / "reports" / "loops" / "20260626-regime-only-tsmom-falsification-v1"
    written = write_regime_only_tsmom_falsification_artifacts(output_dir, payload)
    assert written == output_dir
    expected = {
        "decision.json",
        "report.md",
        "evidence_rollup.csv",
        "failed_gate_rollup.csv",
        "commands.txt",
        "changed_files.txt",
    }
    assert {path.name for path in output_dir.iterdir()} == expected
    decision = json.loads((output_dir / "decision.json").read_text())
    assert "verdict" not in decision
    assert "loop_verdict" not in decision
    assert "decision" not in decision
    assert decision["loop_id"] == "20260626-regime-only-tsmom-falsification-v1"
    assert decision["readiness_baseline_verdict"] == "NOT_READY"
    report = (output_dir / "report.md").read_text()
    assert "## Regime slice evidence" in report
    assert "readiness_baseline_verdict: NOT_READY" in report
    assert "permission to use real money" in report
    with pytest.raises(FileExistsError, match="output directory already exists"):
        write_regime_only_tsmom_falsification_artifacts(output_dir, payload)
    assert default_regime_only_tsmom_falsification_output_dir(
        root=tmp_path / "reports" / "loops",
        today="20260626",
    ) == output_dir


def test_regime_only_tsmom_falsification_cli_writes_artifacts_without_credentials(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key: str, default: str | None = None) -> str | None:
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"tsmom-regime-only-falsification-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    output_dir = tmp_path / "reports" / "loops" / "20260626-regime-only-tsmom-falsification-v1"
    parsed = quantctl.build_parser().parse_args(
        [
            "tsmom-regime-only-falsification-run",
            "--reports-root",
            str(tmp_path / "reports"),
            "--output-dir",
            str(output_dir),
            "--command",
            "uv run pytest tests/test_tsmom_loop.py -q",
            "--changed-file",
            "src/parangsae/research/tsmom.py",
        ]
    )
    assert parsed.func is quantctl.cmd_tsmom_regime_only_falsification_run
    assert quantctl.cmd_tsmom_regime_only_falsification_run(parsed) == 0
    cli_payload = json.loads(capsys.readouterr().out)
    assert "verdict" not in cli_payload
    assert "loop_verdict" not in cli_payload
    assert cli_payload["regime_only_tsmom_falsification_namespace"] == REGIME_ONLY_TSMOM_FALSIFICATION_NAMESPACE
    assert cli_payload["regime_only_tsmom_falsification_decision"] == REGIME_ONLY_TSMOM_FREEZE_NEGATIVE_CONTROL
    assert cli_payload["readiness_baseline_verdict"] == "NOT_READY"
    assert cli_payload["live_orders_submitted"] is False
    assert cli_payload["testnet_orders_submitted"] is False
    decision = json.loads((output_dir / "decision.json").read_text())
    assert decision["safety_gates"]["api_keys_required_for_research"] is False
    assert decision["safety_gates"]["account_or_fill_data_used"] is False
    assert decision["safety_gates"]["strategy_parameters_retuned"] is False
    assert decision["safety_gates"]["gates_relaxed"] is False
    assert (output_dir / "evidence_rollup.csv").exists()
    assert (output_dir / "failed_gate_rollup.csv").exists()
