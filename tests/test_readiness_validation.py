from __future__ import annotations

import ast
import inspect
import json
from pathlib import Path

import pandas as pd

from packages.core.contracts import ProductFamily
from packages.reporting.real_run_report import build_manifest, prepare_output_dir, write_readiness_success_report
from packages.research import readiness as readiness_module
from packages.research.backtest_engine import BacktestResult, StrategySettings
from packages.research.readiness import (
    READINESS_ABLATION_COLUMNS,
    READINESS_JSON_TOP_LEVEL_KEYS,
    READINESS_REGIME_COLUMNS,
    READINESS_SUCCESS_FILES,
    READINESS_VERDICTS,
    READINESS_WINDOWS_COLUMNS,
    ReadinessConfig,
    build_readiness_report,
    execution_slippage_proxy_bps,
    funding_drag_pct,
    generate_walk_forward_windows,
    slippage_drag_pct,
)


def synthetic_frame(rows: int = 300) -> pd.DataFrame:
    index = pd.date_range("2024-01-01", periods=rows, freq="1h", tz="UTC")
    close = pd.Series([100 + i * 0.05 + ((i % 24) - 12) * 0.08 for i in range(rows)], index=index)
    return pd.DataFrame(
        {
            "Open": close.shift(1).fillna(close.iloc[0]),
            "High": close + 1.5,
            "Low": close - 1.5,
            "Close": close,
            "Volume": 1000 + (pd.Series(range(rows), index=index) % 31) * 7,
        },
        index=index,
    )


def small_config() -> ReadinessConfig:
    return ReadinessConfig(train_bars=100, validation_bars=50, holdout_bars=50, step_bars=50)


def research_depth_config() -> ReadinessConfig:
    return ReadinessConfig(
        train_bars=100,
        validation_bars=50,
        holdout_bars=50,
        step_bars=50,
        min_research_grade_total_input_bars=300,
        min_research_grade_window_count=3,
        min_research_grade_candidate_count=3,
        min_research_grade_candidate_window_evaluations=9,
        min_research_grade_bars_processed_estimate=1650,
    )


def fake_backtest(
    frame: pd.DataFrame,
    *,
    label: str,
    strategy_settings: StrategySettings = StrategySettings(),
    broker_settings=None,
) -> BacktestResult:
    del broker_settings
    base = 1.0 + strategy_settings.fast_sma / 100
    if "validation" in label:
        return_pct = base + 1.0
    elif "stress" in label:
        return_pct = base + 0.5
    elif "holdout" in label:
        return_pct = base + 0.8
    else:
        return_pct = base
    entry_a = frame.index[min(10, len(frame) - 1)]
    entry_b = frame.index[max(0, len(frame) - 10)]
    trades = pd.DataFrame(
        {
            "EntryTime": [entry_a, entry_b],
            "ExitTime": [frame.index[min(15, len(frame) - 1)], frame.index[-1]],
            "PnL": [12.0, -3.0],
            "ReturnPct": [0.4, -0.1],
        }
    )
    equity = pd.DataFrame(
        {"Equity": [10000.0, 10000.0 + return_pct], "DrawdownPct": [0.0, -1.0]},
        index=pd.DatetimeIndex([frame.index[0], frame.index[-1]]),
    )
    return BacktestResult(
        label=label,
        metrics={
            "label": label,
            "closed_trades": 2,
            "Return_pct": return_pct,
            "Sharpe_Ratio": 1.1,
            "Max_Drawdown_pct": -1.2,
            "Buy_&_Hold_Return_pct": 0.25,
            "Exposure_Time_pct": 50.0,
        },
        trades=trades,
        equity=equity,
    )


def one_candidate_grid() -> tuple[StrategySettings, ...]:
    return (StrategySettings(fast_sma=8, slow_sma=21, atr_window=14, stop_atr=2.0, take_profit_atr=3.0, risk_fraction=0.5, allow_short=False),)


def test_walk_forward_golden_windows() -> None:
    windows = generate_walk_forward_windows(synthetic_frame(300), small_config())
    assert [window.start for window in windows] == [0, 50, 100]
    assert [window.window_id for window in windows] == ["W001", "W002", "W003"]
    for window in windows:
        assert window.train.index[-1] < window.validation.index[0]
        assert window.validation.index[-1] < window.holdout.index[0]
        assert len(window.train) == 100
        assert len(window.validation) == 50
        assert len(window.holdout) == 50


def test_funding_and_slippage_golden_math() -> None:
    assert funding_drag_pct(bars=100, exposure_time_pct=50, funding_rate_per_8h=0.0001) == 0.0625
    assert slippage_drag_pct(closed_trades=4, slippage_proxy_bps=5) == 0.4
    proxy = execution_slippage_proxy_bps(synthetic_frame(60), closed_trades=2, config=small_config())
    assert proxy >= small_config().stress_slippage_proxy_bps
    high_volume = synthetic_frame(60)
    low_volume = high_volume.copy()
    low_volume["Volume"] = high_volume["Volume"] * 0.01
    assert execution_slippage_proxy_bps(low_volume, closed_trades=2, config=small_config()) > execution_slippage_proxy_bps(
        high_volume,
        closed_trades=2,
        config=small_config(),
    )


def test_readiness_report_schema_gates_and_formula_contract(monkeypatch) -> None:
    monkeypatch.setattr(readiness_module, "run_strategy_backtest", fake_backtest)
    report = build_readiness_report(
        synthetic_frame(300),
        product_family=ProductFamily.SPOT_CRYPTO,
        short_allowed=False,
        config=small_config(),
        candidate_grid=one_candidate_grid(),
    )
    payload = report.to_json_payload()
    assert set(payload) == set(READINESS_JSON_TOP_LEVEL_KEYS)
    assert payload["schema_version"] == 2
    assert payload["summary"]["verdict"] in READINESS_VERDICTS
    assert payload["compute_profile"]["candidate_count"] == 3
    assert payload["compute_profile"]["window_count"] == 3
    assert payload["compute_profile"]["candidate_window_evaluations"] == 9
    assert payload["compute_profile"]["total_strategy_evaluations"] == 24
    assert payload["compute_profile"]["elapsed_seconds_role"].startswith("diagnostic only")
    assert payload["validation_depth"]["grade"] == "prototype"
    assert payload["validation_depth"]["research_grade"] is False
    assert payload["validation_depth"]["gate"] == "validation_depth_research_grade"
    assert "validation_depth_research_grade" in payload["summary"]["failed_gates"]
    assert list(report.windows.columns) == READINESS_WINDOWS_COLUMNS
    assert list(report.ablation.columns) == READINESS_ABLATION_COLUMNS
    assert list(report.regime.columns) == READINESS_REGIME_COLUMNS
    assert payload["artifact_schemas"]["readiness_windows.csv"] == READINESS_WINDOWS_COLUMNS
    assert payload["baselines"]["cash_null_return_pct"] == 0.0
    assert payload["baselines"]["buy_hold_source"] in {"backtesting_metric", "close_to_close_fallback"}
    assert set(payload["duplicate_risk"]) == {
        "max_raw_signals_per_family_per_timestamp",
        "high_corr_family_pair_count",
        "high_corr_threshold",
        "high_corr_pairs",
    }
    thresholds = payload["summary"]["gate_thresholds"]
    assert thresholds["min_windows"] == 3
    assert thresholds["min_holdout_trades_each"] == 1
    assert thresholds["max_high_corr_family_pairs"] == 6
    assert thresholds["max_selected_candidate_switch_rate"] == 0.75
    assert thresholds["min_regime_labels"] == 2
    assert thresholds["stress_return_floor_pct"] == -15.0
    assert thresholds["validation_depth_research_grade"] == payload["validation_depth"]["thresholds"]
    assert payload["overfit_checks"]["selected_candidate_switch_rate"] == 0.0
    assert payload["overfit_checks"]["candidate_registry_version"] == 1
    assert payload["overfit_checks"]["selection_uses_holdout"] is False
    assert "untouched future/OOS" in payload["overfit_checks"]["post_report_change_policy"]
    assert payload["lookahead_checks"]["passed"] is True
    assert payload["lookahead_checks"]["ablation_prefix_invariant"] is True
    assert payload["method"]["selection_uses_holdout"] is False
    assert payload["method"]["live_orders_submitted"] is False
    assert payload["method"]["api_keys_required"] is False
    for row in payload["walk_forward"]["rows"]:
        assert row["cash_null_return_pct"] == 0.0
        assert row["beats_cash_null"] == (row["stress_holdout_return_pct"] > 0.0)
        assert row["beats_buy_hold"] == (row["stress_holdout_return_pct"] > row["base_buy_hold_return_pct"])
        expected_slippage = slippage_drag_pct(closed_trades=2, slippage_proxy_bps=row["stress_slippage_proxy_bps"])
        assert expected_slippage >= 0.2
    json.dumps(payload)


def test_readiness_insufficient_windows_still_emits_v2_depth_payload(monkeypatch) -> None:
    monkeypatch.setattr(readiness_module, "run_strategy_backtest", fake_backtest)
    report = build_readiness_report(
        synthetic_frame(120),
        config=small_config(),
        candidate_grid=one_candidate_grid(),
    )
    payload = report.to_json_payload()
    assert payload["schema_version"] == 2
    assert set(payload) == set(READINESS_JSON_TOP_LEVEL_KEYS)
    assert payload["summary"]["verdict"] == "BLOCKED"
    assert payload["summary"]["failed_gates"] == ["min_windows", "validation_depth_research_grade"]
    assert payload["compute_profile"]["total_input_bars"] == 120
    assert payload["compute_profile"]["window_count"] == 0
    assert payload["compute_profile"]["candidate_count"] == 3
    assert payload["validation_depth"]["grade"] == "smoke"
    assert payload["validation_depth"]["research_grade"] is False
    assert payload["validation_depth"]["observed"]["window_count"] == 0
    assert payload["lookahead_checks"]["status"] == "blocked"


def test_readiness_depth_gate_can_pass_when_workload_thresholds_are_met(monkeypatch) -> None:
    monkeypatch.setattr(readiness_module, "run_strategy_backtest", fake_backtest)
    report = build_readiness_report(
        synthetic_frame(300),
        config=research_depth_config(),
        candidate_grid=one_candidate_grid(),
    )
    payload = report.to_json_payload()
    assert payload["validation_depth"]["grade"] == "research_grade"
    assert payload["validation_depth"]["research_grade"] is True
    assert payload["validation_depth"]["failed_depth_checks"] == []
    assert "validation_depth_research_grade" not in payload["summary"]["failed_gates"]


def test_readiness_selector_static_contract_excludes_holdout() -> None:
    signature = inspect.signature(readiness_module._select_candidate)
    assert "holdout" not in signature.parameters
    source = inspect.getsource(readiness_module._select_candidate)
    assert "holdout" not in source
    assert "train" in signature.parameters
    assert "validation" in signature.parameters


def test_readiness_report_uses_observed_public_funding_metadata(monkeypatch) -> None:
    monkeypatch.setattr(readiness_module, "run_strategy_backtest", fake_backtest)
    frame = synthetic_frame(300)
    funding = pd.DataFrame(
        {
            "symbol": "BTCUSDT",
            "funding_time_ms": [int(ts.timestamp() * 1000) for ts in frame.index[::8]],
            "funding_rate": [0.0002 for _ in frame.index[::8]],
            "mark_price": [100.0 for _ in frame.index[::8]],
        },
        index=frame.index[::8],
    )
    report = build_readiness_report(
        frame,
        config=small_config(),
        candidate_grid=one_candidate_grid(),
        funding_frame=funding,
        funding_metadata={"source_url": "https://fapi.binance.com/fapi/v1/fundingRate"},
    )
    payload = report.to_json_payload()
    assert payload["funding_scenarios"]["source"] == "public_binance_usdm_funding_history"
    assert payload["funding_scenarios"]["rows"] == len(funding)
    assert payload["method"]["data_coverage"]["derivatives_funding"]["status"] == "validated_public_data"
    assert payload["method"]["data_coverage"]["orderbook_orderflow"]["status"] == "blocked_requires_provider"
    assert all(row["stress_funding_rate_per_8h"] == 0.0002 for row in payload["walk_forward"]["rows"])
    assert "account-specific funding cashflows" in " ".join(payload["live_blockers"])


def test_readiness_success_artifact_contract(monkeypatch, tmp_path) -> None:
    monkeypatch.setattr(readiness_module, "run_strategy_backtest", fake_backtest)
    frame = synthetic_frame(300)
    report = build_readiness_report(frame, config=small_config(), candidate_grid=one_candidate_grid())
    manifest = build_manifest(bundle=None, symbol="BTCUSDT", interval="1h", months=("2024-01",), split_index=None)
    manifest.update({"row_count": len(frame), "first_timestamp": frame.index[0].isoformat(), "last_timestamp": frame.index[-1].isoformat()})
    out = prepare_output_dir(output_dir=tmp_path / "readiness", run_prefix="readiness-run")
    metrics = write_readiness_success_report(output_dir=out, manifest=manifest, readiness_report=report)
    assert {path.name for path in out.iterdir()} == READINESS_SUCCESS_FILES
    assert metrics["verdict"] in READINESS_VERDICTS
    assert list(pd.read_csv(out / "readiness_windows.csv").columns) == READINESS_WINDOWS_COLUMNS
    assert list(pd.read_csv(out / "readiness_ablation.csv").columns) == READINESS_ABLATION_COLUMNS
    assert list(pd.read_csv(out / "readiness_regime.csv").columns) == READINESS_REGIME_COLUMNS
    readiness_json = json.loads((out / "readiness.json").read_text())
    assert set(readiness_json) == set(READINESS_JSON_TOP_LEVEL_KEYS)
    manifest_json = json.loads((out / "manifest.json").read_text())
    assert manifest_json["readiness_schema_version"] == 2
    assert readiness_json["compute_profile"]["elapsed_seconds_role"].startswith("diagnostic only")
    assert readiness_json["validation_depth"]["gate"] == "validation_depth_research_grade"
    report_text = (out / "report.md").read_text()
    assert "Destructive Readiness Validation Report" in report_text
    assert "readiness verdict" in report_text
    assert "Validation depth / compute profile" in report_text
    assert "diagnostic only" in report_text
    assert "funding source" in report_text
    assert "execution proxy source" in report_text
    assert "Data-source coverage" in report_text
    assert "Live blockers" in report_text
    assert "not a profit guarantee" in report_text
    assert "No Binance API key was used" in report_text


def test_readiness_research_static_source_has_no_live_order_or_credential_path() -> None:
    for path in [Path("packages/research/readiness.py"), Path("packages/reporting/real_run_report.py")]:
        source = path.read_text()
        tree = ast.parse(source)
        assert not any(isinstance(node, ast.Import) and any(alias.name == "ccxt" for alias in node.names) for node in ast.walk(tree))
        assert "create_order" not in source
        assert "BINANCE_API_KEY" not in source
        assert "BINANCE_API_SECRET" not in source
    cli_source = Path("apps/cli/quantctl.py").read_text()
    readiness_slice = cli_source[cli_source.index("def cmd_readiness_run") : cli_source.index("def cmd_kill_switch")]
    assert "ccxt" not in readiness_slice
    assert "create_order" not in readiness_slice
    assert "BINANCE_API_KEY" not in readiness_slice
    assert "BINANCE_API_SECRET" not in readiness_slice
