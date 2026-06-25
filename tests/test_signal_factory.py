from __future__ import annotations

import ast
from pathlib import Path

import pandas as pd
import pytest

from packages.research.regime import build_regime_report
from packages.research.signals import (
    CLAIM_BOUNDARY,
    EXECUTION_BOUNDARY,
    SIGNAL_CSV_COLUMNS,
    SIGNAL_JSON_TOP_LEVEL_KEYS,
    SignalFactoryConfig,
    build_signal_report,
)


def trending_frame(rows: int = 220) -> pd.DataFrame:
    idx = pd.date_range("2024-01-01", periods=rows, freq="1h", tz="UTC")
    closes = [100 + i * 0.2 + ((i % 9) - 4) * 0.04 for i in range(rows)]
    return pd.DataFrame(
        {
            "Open": [value - 0.1 for value in closes],
            "High": [value + 1.0 for value in closes],
            "Low": [value - 1.0 for value in closes],
            "Close": closes,
            "Volume": [100.0 + (i % 7) * 5 for i in range(rows)],
        },
        index=idx,
    )


def signal_report_for(frame: pd.DataFrame):
    segments = pd.Series(["train"] * len(frame), index=frame.index, dtype="object")
    regime = build_regime_report(frame, segments=segments)
    return build_signal_report(
        frame,
        segments=segments,
        regimes=regime.rows["regime_label"],
        asset="BTCUSDT",
        timeframe="1h",
    )


def test_signal_report_schema_and_base_fields():
    report = signal_report_for(trending_frame())
    assert list(report.rows.reset_index().columns) == SIGNAL_CSV_COLUMNS
    assert set(report.to_json_payload()) == set(SIGNAL_JSON_TOP_LEVEL_KEYS)
    assert report.to_json_payload()["schema_version"] == 1
    assert report.to_json_payload()["method"]["rows_schema"] == SIGNAL_CSV_COLUMNS
    row = report.rows.iloc[-1]
    assert row["signal_id"].startswith("BTCUSDT_1h_")
    assert row["asset"] == "BTCUSDT"
    assert row["timeframe"] == "1h"
    assert row["expiry_bars"] == 3
    assert -1 <= row["normalized_value"] <= 1
    assert -1 <= report.summary["latest"]["final_score"] <= 1
    assert report.summary["latest"]["decision"] in {"long_bias", "short_bias", "hold", "risk_off"}
    assert report.summary["latest"]["decision_boundary"] == EXECUTION_BOUNDARY
    assert report.summary["claim_boundary"] == CLAIM_BOUNDARY
    assert report.summary["execution_boundary"] == EXECUTION_BOUNDARY
    assert report.to_json_payload()["method"]["execution_boundary"] == EXECUTION_BOUNDARY


def test_signal_report_preserves_config_asset_timeframe_without_overrides():
    frame = trending_frame(rows=40)
    config = SignalFactoryConfig(asset="ETHUSDT", timeframe="4h", expiry_bars=5)
    report = build_signal_report(frame, config=config)
    row = report.rows.iloc[-1]
    assert row["signal_id"].startswith("ETHUSDT_4h_")
    assert row["asset"] == "ETHUSDT"
    assert row["timeframe"] == "4h"
    assert row["expiry_bars"] == 5
    assert report.to_json_payload()["config"]["asset"] == "ETHUSDT"
    assert report.to_json_payload()["config"]["timeframe"] == "4h"


def test_catalog_marks_non_ohlcv_categories_catalog_only():
    payload = signal_report_for(trending_frame()).to_json_payload()
    catalog_only = payload["catalog"]["catalog_only_categories"]
    for category in ["derivatives", "orderbook_orderflow", "on_chain", "options", "macro_events", "sentiment_news", "execution"]:
        assert catalog_only[category]["status"] == "not_computed_without_data_source"
    assert "trend" in payload["catalog"]["computed_categories"]


def test_duplicate_family_counts_expose_raw_count_dominance_risk():
    report = signal_report_for(trending_frame())
    counts = report.summary["duplicate_family_counts"]
    assert counts["trend_ma"] > 1
    latest = report.summary["latest"]
    assert latest["family_count"] < latest["signal_count"]


def test_missing_and_unsorted_frames_are_rejected():
    with pytest.raises(ValueError, match="missing OHLCV columns"):
        build_signal_report(pd.DataFrame({"Close": [1.0]}, index=pd.date_range("2024-01-01", periods=1, tz="UTC")))
    with pytest.raises(ValueError, match="monotonic increasing"):
        signal_report_for(trending_frame(rows=40).iloc[::-1])


def test_incomplete_segment_or_regime_labels_are_rejected():
    frame = trending_frame(rows=40)
    missing = pd.Series(["train"] * 10, index=frame.index[:10])
    with pytest.raises(ValueError, match="segments series must cover"):
        build_signal_report(frame, segments=missing)
    with pytest.raises(ValueError, match="regimes series must cover"):
        build_signal_report(frame, regimes=missing)


def test_signal_features_are_causal_prefix_invariant_under_future_shock():
    prefix = trending_frame(rows=180)
    future = trending_frame(rows=24)
    future.index = pd.date_range(prefix.index[-1] + pd.Timedelta(hours=1), periods=24, freq="1h", tz="UTC")
    future["Close"] = future["Close"] * 6
    future["High"] = future["Close"] + 30
    future["Low"] = future["Close"] - 30
    future["Volume"] = 1.0

    prefix_report = signal_report_for(prefix)
    extended = pd.concat([prefix, future])
    extended_segments = pd.Series(["train"] * len(prefix) + ["future"] * len(future), index=extended.index)
    extended_regime = build_regime_report(extended, segments=extended_segments)
    extended_report = build_signal_report(
        extended,
        segments=extended_segments,
        regimes=extended_regime.rows["regime_label"],
        asset="BTCUSDT",
        timeframe="1h",
    )

    pd.testing.assert_frame_equal(
        prefix_report.rows,
        extended_report.rows.loc[prefix_report.rows.index.unique()],
        check_dtype=False,
        check_exact=False,
        rtol=1e-12,
        atol=1e-12,
    )


def test_signal_reporting_import_boundaries_do_not_enter_live_scope():
    forbidden_import_roots = {"ccxt", "packages.execution"}
    for path in [
        Path("packages/research/signals.py"),
        Path("packages/reporting/real_run_report.py"),
    ]:
        source = path.read_text()
        tree = ast.parse(source)
        imports = []
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                imports.extend(alias.name for alias in node.names)
            elif isinstance(node, ast.ImportFrom) and node.module:
                imports.append(node.module)
        assert not any(
            imported == root or imported.startswith(f"{root}.")
            for imported in imports
            for root in forbidden_import_roots
        )
        assert "os.getenv" not in source
        assert "os.environ" not in source
        assert "BINANCE_API_KEY" not in source
        assert "BINANCE_API_SECRET" not in source


def test_execution_package_does_not_import_signal_report_decisions():
    for path in Path("packages/execution").glob("*.py"):
        source = path.read_text()
        tree = ast.parse(source)
        imports = []
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                imports.extend(alias.name for alias in node.names)
            elif isinstance(node, ast.ImportFrom) and node.module:
                imports.append(node.module)
        assert "packages.research.signals" not in imports
        assert "SignalReport" not in source
        assert "signal_summary" not in source
