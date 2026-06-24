from __future__ import annotations

import pandas as pd
import pytest

from packages.research.backtest_engine import (
    CANONICAL_BROKER,
    CANONICAL_STRATEGY,
    MeaningGateError,
    assert_meaningful_run,
    broker_settings_dict,
    split_chronological,
    strategy_settings_dict,
)


def test_canonical_strategy_and_broker_settings_are_fixed():
    assert strategy_settings_dict() == {
        "fast_sma": 12,
        "slow_sma": 48,
        "atr_window": 14,
        "stop_atr": 2.5,
        "take_profit_atr": 4.0,
        "risk_fraction": 0.95,
    }
    assert broker_settings_dict() == {
        "cash": 10000.0,
        "commission": 0.0004,
        "spread": 0.0002,
        "margin": 1.0,
        "hedging": False,
        "exclusive_orders": True,
        "trade_on_close": False,
        "finalize_trades": True,
        "fractional_unit": 0.00000001,
    }
    assert CANONICAL_STRATEGY.risk_fraction == 0.95
    assert CANONICAL_BROKER.margin == 1.0


def test_strict_meaning_gate_passes_only_with_required_bars_and_trades():
    passed = assert_meaningful_run(
        total_bars=720,
        train_bars=480,
        holdout_bars=240,
        backtest_closed_trades=3,
        holdout_closed_trades=2,
    )
    assert passed["status"] == "passed"
    with pytest.raises(MeaningGateError) as exc:
        assert_meaningful_run(
            total_bars=720,
            train_bars=480,
            holdout_bars=240,
            backtest_closed_trades=2,
            holdout_closed_trades=2,
        )
    assert exc.value.failed_gate == "backtest_closed_trades"


def test_split_chronological_fails_when_train_and_holdout_overlap():
    idx = pd.date_range("2024-01-01", periods=10, freq="1h", tz="UTC")
    frame = pd.DataFrame({"Open": 1, "High": 2, "Low": 1, "Close": 1.5, "Volume": 1}, index=idx)
    train, holdout, split_index = split_chronological(frame, train_fraction=0.7)
    assert split_index == 7
    assert train.index[-1] < holdout.index[0]


def test_engine_uses_fractional_backtest_and_non_overlay_indicators():
    source = __import__("pathlib").Path("packages/research/backtest_engine.py").read_text()
    assert "FractionalBacktest" in source
    assert "fractional_unit=broker_settings.fractional_unit" in source
    assert source.count("overlay=False") >= 3
