from __future__ import annotations

import inspect
from pathlib import Path

import pandas as pd
import pytest

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_funding import (
    FundingDownloadRecord,
    FundingRateBundle,
    normalized_funding_sha256,
    validate_funding_frame,
)
from parangsae.marketdata.binance_vision import (
    DownloadRecord,
    MarketDataBundle,
    normalized_ohlcv_sha256,
    validate_ohlcv,
)
from parangsae.research.data_depth import (
    DataDepthConfig,
    build_data_depth_loop_payload,
    default_data_depth_months,
    prepare_data_depth_output_dir,
    render_data_depth_report,
    write_data_depth_loop_artifacts,
)
from parangsae.research.readiness import READINESS_VERDICTS


def synthetic_ohlcv(rows: int = 300) -> pd.DataFrame:
    index = pd.date_range("2024-01-01", periods=rows, freq="1h", tz="UTC")
    close = pd.Series([100 + i * 0.02 + (i % 7) * 0.1 for i in range(rows)], index=index)
    frame = pd.DataFrame(
        {
            "Open": close.shift(1).fillna(close.iloc[0]),
            "High": close + 1.0,
            "Low": close - 1.0,
            "Close": close,
            "Volume": 1000 + (pd.Series(range(rows), index=index) % 17) * 5,
        },
        index=index,
    )
    frame.attrs["closed_bar_only"] = True
    frame.attrs["open_bar_included"] = False
    frame.attrs["close_time_policy"] = (
        "binance_monthly_close_time_equals_open_time_plus_1h_minus_1ms"
    )
    return frame


def market_bundle(frame: pd.DataFrame) -> MarketDataBundle:
    quality = validate_ohlcv(frame)
    record = DownloadRecord(
        url="https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-01.zip",
        cache_path="data/binance-vision/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-01.zip",
        sha256="a" * 64,
        http_status=200,
        etag="fixture",
        last_modified="Mon, 01 Jan 2024 00:00:00 GMT",
        downloaded_at="2026-06-25T00:00:00Z",
        from_cache=True,
        bytes=123,
    )
    return MarketDataBundle(
        symbol="BTCUSDT",
        interval="1h",
        months=("2024-01",),
        frame=frame,
        downloads=(record,),
        normalized_sha256=normalized_ohlcv_sha256(frame),
        quality=quality,
    )


def funding_bundle_for(frame: pd.DataFrame) -> FundingRateBundle:
    index = pd.DatetimeIndex([frame.index[0], frame.index[8], frame.index[-1]])
    funding = pd.DataFrame(
        {
            "symbol": ["BTCUSDT", "BTCUSDT", "BTCUSDT"],
            "funding_time_ms": [int(ts.timestamp() * 1000) for ts in index],
            "funding_rate": [0.0001, -0.0002, 0.0003],
            "mark_price": [100.0, 101.0, 102.0],
        },
        index=index,
    )
    quality = validate_funding_frame(funding, symbol="BTCUSDT")
    record = FundingDownloadRecord(
        url="https://fapi.binance.com/fapi/v1/fundingRate",
        cache_path="data/binance-funding/futures/um/fundingRate/BTCUSDT/fixture.json",
        sha256="b" * 64,
        http_status=200,
        downloaded_at="2026-06-25T00:00:00Z",
        from_cache=True,
        rows=len(funding),
        params={"symbol": "BTCUSDT"},
    )
    return FundingRateBundle(
        symbol="BTCUSDT",
        frame=funding,
        downloads=(record,),
        normalized_sha256=normalized_funding_sha256(funding),
        quality=quality,
    )


def data_depth_config() -> DataDepthConfig:
    return DataDepthConfig(
        train_bars=100,
        validation_bars=50,
        holdout_bars=50,
        step_bars=50,
        min_research_grade_total_input_bars=300,
        min_research_grade_window_count=3,
    )


def test_default_data_depth_months_cover_two_year_public_archive_window() -> None:
    months = default_data_depth_months()
    assert months[0] == "2024-01"
    assert months[-1] == "2025-12"
    assert len(months) == 24


def test_data_depth_payload_records_provenance_safety_and_unclaimed_strategy_gates() -> None:
    frame = synthetic_ohlcv(300)
    bundle = market_bundle(frame)
    funding_bundle = funding_bundle_for(frame)
    payload = build_data_depth_loop_payload(
        bundle=bundle,
        funding_bundle=funding_bundle,
        funding_metadata={"source": "fixture_public_binance"},
        config=data_depth_config(),
        commands=["uv run quantctl data-depth-run --fixture-like"],
        changed_files=["src/parangsae/research/data_depth.py"],
    )

    assert payload["safety_gates"]["live_orders_submitted"] is False
    assert payload["safety_gates"]["testnet_orders_submitted"] is False
    assert payload["safety_gates"]["api_keys_required_for_research"] is False
    assert payload["safety_gates"]["holdout_used_for_selection"] is False
    assert payload["data_scope"]["row_count"] == 300
    assert payload["data_scope"]["closed_bar_only"] is True
    assert payload["data_scope"]["open_bar_included"] is False
    assert payload["schema_version"] == 2
    assert "verdict" not in payload
    assert "loop_verdict" not in payload
    assert payload["data_depth_loop_verdict"] == "WATCH"
    assert payload["data_depth_loop_verdict"] not in READINESS_VERDICTS
    assert payload["data_depth_loop_verdict_namespace"] == "data_depth_only_not_readiness"
    assert payload["readiness_baseline_verdict"] == "NOT_READY"
    assert payload["data_depth_contract"]["min_research_grade_total_input_bars"] == 300
    assert payload["data_scope"]["volume_policy"] == "strictly_positive_volume_required"
    assert payload["data_scope"]["zero_volume_bars"] == 0
    assert payload["data_scope"]["duplicate_policy"] == {"duplicates": 0, "duplicates_allowed": False}
    assert len(payload["data_scope"]["normalized_ohlcv_sha256"]) == 64
    assert payload["funding_coverage"]["source"] == "public_binance_usdm_funding_history"
    assert payload["funding_coverage"]["causal_alignment_checked"] is True
    assert payload["funding_coverage"]["account_cashflows_validated"] is False
    assert payload["validation_depth"]["data_only_research_grade_depth_passed"] is True
    assert payload["validation_depth"]["strategy_grid_depth_unclaimed"] is True
    data_gates = payload["validation_depth"]["data_depth_gates"]
    assert data_gates["minimum_sample_depth"]["passed"] is True
    assert data_gates["minimum_sample_depth"]["total_input_bars"]["observed"] == 300
    assert data_gates["minimum_sample_depth"]["projected_window_count"]["observed"] == 3
    assert data_gates["walk_forward_coverage"]["passed"] is True
    strategy_gates = payload["validation_depth"]["strategy_performance_gates_unclaimed"]
    for gate_name in [
        "median_stress_return_beats_cash_null",
        "selected_candidate_switch_rate",
        "funding_aware_net_return",
        "max_drawdown_abs_pct",
        "tail_loss_cvar_pct",
    ]:
        assert strategy_gates[gate_name]["passed"] is False
        assert strategy_gates[gate_name]["status"] == "blocked_until_strategy_validation"
        assert gate_name in payload["failed_gates"]
    assert "candidate_count" in payload["failed_gates"]
    assert "candidate_window_evaluations" in payload["failed_gates"]
    assert payload["decision"] == "PROMOTE_TO_NEXT_LOOP"

    datasets = payload["dataset_provenance"]
    assert [dataset["dataset"] for dataset in datasets] == ["ohlcv", "funding"]
    assert datasets[0]["normalized_sha256"] == payload["data_scope"]["normalized_ohlcv_sha256"]
    assert datasets[0]["no_key_public_data"] is True
    assert datasets[0]["account_or_signed_data_used"] is False
    assert datasets[1]["normalized_sha256"] == payload["funding_coverage"]["normalized_funding_sha256"]
    assert datasets[1]["causal_alignment_checked"] is True

    coverage = payload["coverage_analysis"]
    assert len(coverage["walk_forward_windows"]) == 3
    assert coverage["walk_forward_windows"][0]["window_id"] == "W001"
    assert coverage["walk_forward_windows"][0]["train_bars"] == 100
    assert coverage["walk_forward_windows"][0]["validation_bars"] == 50
    assert coverage["walk_forward_windows"][0]["holdout_bars"] == 50
    vol = coverage["volatility_regimes"]
    assert (
        vol["low_count"]
        + vol["normal_count"]
        + vol["high_count"]
        + vol["insufficient_history_count"]
        == 300
    )
    stress = coverage["stress_periods"]
    assert stress["context_only_not_strategy_pnl"] is True
    assert stress["stress_bar_count"] >= stress["high_vol_bar_count"]
    funding_regimes = coverage["funding_regimes"]
    assert funding_regimes["positive_count"] == 2
    assert funding_regimes["negative_count"] == 1
    assert funding_regimes["account_cashflows_validated"] is False

    report = render_data_depth_report(payload)
    assert "data_depth_loop_verdict: WATCH" in report
    assert "WATCH is data-depth-only" in report
    assert "not a readiness verdict" in report
    assert "readiness baseline verdict: NOT_READY" in report
    assert "strategy_performance_gates_unclaimed" in report


def test_data_depth_payload_does_not_claim_closed_bars_without_quality_metadata() -> None:
    frame = synthetic_ohlcv(300)
    bundle = market_bundle(frame)
    legacy_bundle = MarketDataBundle(
        symbol=bundle.symbol,
        interval=bundle.interval,
        months=bundle.months,
        frame=bundle.frame,
        downloads=bundle.downloads,
        normalized_sha256=bundle.normalized_sha256,
        quality={
            key: value
            for key, value in bundle.quality.items()
            if key not in {"closed_bar_only", "open_bar_included", "close_time_policy"}
        },
    )
    payload = build_data_depth_loop_payload(
        bundle=legacy_bundle,
        config=data_depth_config(),
    )
    assert payload["data_scope"]["closed_bar_only"] is False
    assert payload["data_scope"]["open_bar_included"] is True
    assert payload["data_scope"]["close_time_policy"] is None


def test_data_depth_artifact_writer_contract_and_safe_overwrite(tmp_path: Path) -> None:
    payload = build_data_depth_loop_payload(
        bundle=market_bundle(synthetic_ohlcv(300)),
        config=data_depth_config(),
        commands=["uv run pytest tests/test_data_depth_loop.py -q"],
        changed_files=["tests/test_data_depth_loop.py"],
    )
    output_dir = tmp_path / "20260625-data-depth-research-grade"
    written = write_data_depth_loop_artifacts(output_dir, payload)
    assert written == output_dir
    assert sorted(path.name for path in output_dir.iterdir()) == [
        "changed_files.txt",
        "commands.txt",
        "loop.json",
        "report.md",
    ]
    with pytest.raises(FileExistsError, match="already exists"):
        prepare_data_depth_output_dir(output_dir)
    (output_dir / "unexpected.txt").write_text("do not delete\n")
    with pytest.raises(FileExistsError, match="unknown files"):
        write_data_depth_loop_artifacts(output_dir, payload, overwrite=True)


def test_data_depth_command_path_has_no_live_or_credential_surface() -> None:
    source = "\n".join(
        [
            inspect.getsource(quantctl.cmd_data_depth_run),
            inspect.getsource(build_data_depth_loop_payload),
        ]
    )
    forbidden = [
        "ccxt",
        "create_order",
        "BINANCE_API_KEY",
        "BINANCE_API_SECRET",
        "RuntimeLiveContext",
        "LiveTradingGuard",
    ]
    for token in forbidden:
        assert token not in source
