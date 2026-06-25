from __future__ import annotations

import ast
from pathlib import Path

import pandas as pd
import pytest

from parangsae.research.regime import (
    CLAIM_BOUNDARY,
    REGIME_ROW_COLUMNS,
    build_regime_report,
)


def trending_frame(rows: int = 220, *, zero_volume: bool = False) -> pd.DataFrame:
    idx = pd.date_range("2024-01-01", periods=rows, freq="1h", tz="UTC")
    closes = [100 + i * 0.25 + ((i % 12) - 6) * 0.03 for i in range(rows)]
    volumes = [0.0 if zero_volume else 100.0 + (i % 10) for i in range(rows)]
    return pd.DataFrame(
        {
            "Open": [value - 0.1 for value in closes],
            "High": [value + 1.0 for value in closes],
            "Low": [value - 1.0 for value in closes],
            "Close": closes,
            "Volume": volumes,
        },
        index=idx,
    )


def test_trending_market_gets_trend_capable_protocol():
    report = build_regime_report(trending_frame())
    latest = report.summary["latest"]
    assert latest["regime_label"] in {"normal_trend", "high_vol_trend"}
    assert latest["protocol_action"] in {"allow_trend", "reduce_risk"}
    assert latest["risk_multiplier"] > 0


def test_zero_volume_maps_to_liquidity_stress_pause():
    report = build_regime_report(trending_frame(zero_volume=True))
    latest = report.summary["latest"]
    assert latest["regime_label"] == "liquidity_stress"
    assert latest["protocol_action"] == "pause_trend"
    assert latest["risk_multiplier"] == 0.0


def test_short_frame_is_insufficient_data_without_exception():
    report = build_regime_report(trending_frame(rows=12))
    assert set(report.rows["regime_label"]) == {"insufficient_data"}
    assert report.summary["protocol_counts"] == {"pause_trend": 12}
    assert report.summary["claim_boundary"] == CLAIM_BOUNDARY


def test_unsorted_frame_is_rejected_instead_of_silently_reordered():
    frame = trending_frame(rows=12).iloc[::-1]
    with pytest.raises(ValueError, match="monotonic increasing"):
        build_regime_report(frame)


def test_summary_includes_segment_counts_and_claim_boundary():
    frame = trending_frame(rows=140)
    segments = pd.Series(["train"] * 100 + ["holdout_paper"] * 40, index=frame.index)
    report = build_regime_report(frame, segments=segments)
    assert set(report.summary["by_segment"]) == {"holdout_paper", "train"}
    assert report.summary["by_segment"]["train"]["rows"] == 100
    assert report.summary["claim_boundary"] == CLAIM_BOUNDARY


def test_regime_features_are_causal_prefix_invariant_under_future_shock():
    prefix = trending_frame(rows=180)
    future = trending_frame(rows=24)
    future.index = pd.date_range(prefix.index[-1] + pd.Timedelta(hours=1), periods=24, freq="1h")
    future.index = future.index.tz_convert("UTC")
    future["Close"] = future["Close"] * 5
    future["High"] = future["Close"] + 20
    future["Low"] = future["Close"] - 20
    future["Volume"] = 1.0

    prefix_segments = pd.Series(["prefix"] * len(prefix), index=prefix.index)
    extended = pd.concat([prefix, future])
    extended_segments = pd.Series(
        ["prefix"] * len(prefix) + ["future"] * len(future),
        index=extended.index,
    )
    prefix_report = build_regime_report(prefix, segments=prefix_segments)
    extended_report = build_regime_report(extended, segments=extended_segments)

    pd.testing.assert_frame_equal(
        prefix_report.rows[REGIME_ROW_COLUMNS],
        extended_report.rows.iloc[: len(prefix)][REGIME_ROW_COLUMNS],
        check_dtype=False,
        check_exact=False,
        rtol=1e-12,
        atol=1e-12,
    )


def test_regime_and_reporting_import_boundaries_do_not_enter_live_scope():
    forbidden_import_roots = {"ccxt", "parangsae.execution"}
    for path in [
        Path("src/parangsae/research/regime.py"),
        Path("src/parangsae/reporting/real_run_report.py"),
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
