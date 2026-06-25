from __future__ import annotations

import pandas as pd
import pytest

from packages.research.forward_paper import (
    EQUITY_COLUMNS,
    POSITIONS_COLUMNS,
    TRADES_COLUMNS,
    ForwardPaperConfig,
    exposure_frame_from_reports,
    run_forward_paper,
    simulate_forward_positions,
)
from packages.research.regime import RegimeReport
from packages.research.signals import SignalReport


def frame() -> pd.DataFrame:
    idx = pd.date_range("2024-01-01T00:00:00Z", periods=3, freq="1h")
    return pd.DataFrame(
        {
            "Open": [100.0, 110.0, 121.0],
            "High": [102.0, 112.0, 123.0],
            "Low": [99.0, 109.0, 120.0],
            "Close": [100.0, 110.0, 121.0],
            "Volume": [10.0, 10.0, 10.0],
        },
        index=idx,
    )


def signal_report(index: pd.DatetimeIndex) -> SignalReport:
    records = []
    by_ts = [0.60, 0.30, 0.0]
    for ts, direction in zip(index, by_ts, strict=True):
        for role, score in {
            "direction": direction,
            "entry": 0.0,
            "risk": 0.0,
            "exit": 0.0,
        }.items():
            records.append({"Timestamp": ts, "role": role, "weighted_score": score})
    rows = pd.DataFrame.from_records(records).set_index("Timestamp")
    return SignalReport(rows=rows, summary={})


def regime_report(index: pd.DatetimeIndex) -> RegimeReport:
    rows = pd.DataFrame(
        {
            "regime_label": ["normal_trend", "normal_trend", "normal_trend"],
            "risk_multiplier": [1.0, 1.0, 1.0],
        },
        index=index,
    )
    return RegimeReport(rows=rows, summary={})


def test_exposure_formula_and_prior_bar_application() -> None:
    data = frame()
    config = ForwardPaperConfig(entry_threshold=0.20, exit_threshold=0.05, fee_rate=0.0, slippage_bps=0.0)
    exposure = exposure_frame_from_reports(data, regime_report(data.index), signal_report(data.index), config=config)
    assert exposure["raw_score"].tolist() == pytest.approx([0.27, 0.135, 0.0])
    assert exposure["target_exposure"].tolist() == pytest.approx([1.0, 1.0, 0.0])
    assert exposure["applied_exposure"].tolist() == pytest.approx([0.0, 1.0, 1.0])


def test_costs_and_funding_events_are_event_aligned() -> None:
    data = frame()
    config = ForwardPaperConfig(entry_threshold=0.20, exit_threshold=0.05, fee_rate=0.0, slippage_bps=0.0)
    exposure = exposure_frame_from_reports(data, regime_report(data.index), signal_report(data.index), config=config)
    funding = pd.DataFrame(
        {"funding_rate": [0.001, -0.001]},
        index=pd.DatetimeIndex(["2024-01-01T01:00:00Z", "2024-01-01T02:00:00Z"]),
    )
    positions, equity, trades, metrics = simulate_forward_positions(
        data,
        exposure,
        funding_frame=funding,
        config=config,
    )
    assert list(positions.columns) == POSITIONS_COLUMNS
    assert list(equity.columns) == EQUITY_COLUMNS
    assert list(trades.columns) == TRADES_COLUMNS
    assert positions.loc[pd.Timestamp("2024-01-01T01:00:00Z"), "funding_cost"] > 0
    assert positions.loc[pd.Timestamp("2024-01-01T02:00:00Z"), "funding_cost"] < 0
    assert metrics["closed_trades"] == 1
    assert metrics["live_orders_submitted"] is False
    assert metrics["api_keys_required"] is False


def test_run_forward_paper_smoke_schema() -> None:
    result = run_forward_paper(frame(), config=ForwardPaperConfig(funding_source="none"), funding_frame=None)
    assert list(result.positions.columns) == POSITIONS_COLUMNS
    assert list(result.equity.columns) == EQUITY_COLUMNS
    assert list(result.trades.columns) == TRADES_COLUMNS
    assert result.metrics["run_type"] == "forward-paper-run"
    assert result.metrics["claim_boundary"].startswith("forward-paper-run")
