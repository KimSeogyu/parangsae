from __future__ import annotations

import pandas as pd
import pytest

from parangsae.research.backtest_engine import (
    BacktestResult,
    CANONICAL_BROKER,
    CANONICAL_STRATEGY,
    MeaningGateError,
    StrategySettings,
    assert_meaningful_run,
    broker_settings_dict,
    default_risk_candidate_grid,
    risk_adjusted_score_v1,
    run_risk_candidate_search,
    split_chronological,
    split_train_validation_holdout,
    strategy_settings_dict,
)
from parangsae.core.contracts import ProductFamily


def test_canonical_strategy_and_broker_settings_are_fixed():
    assert strategy_settings_dict() == {
        "fast_sma": 12,
        "slow_sma": 48,
        "atr_window": 14,
        "stop_atr": 2.5,
        "take_profit_atr": 4.0,
        "risk_fraction": 0.95,
        "allow_short": True,
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
    source = __import__("pathlib").Path("src/parangsae/research/backtest_engine.py").read_text()
    assert "FractionalBacktest" in source
    assert "fractional_unit=broker_settings.fractional_unit" in source
    assert source.count("overlay=False") >= 3


def test_three_way_split_is_chronological_and_non_overlapping():
    idx = pd.date_range("2024-01-01", periods=1000, freq="1h", tz="UTC")
    frame = pd.DataFrame({"Open": 1, "High": 2, "Low": 1, "Close": 1.5, "Volume": 1}, index=idx)
    split = split_train_validation_holdout(frame)
    assert split.metadata["chronological_split_verified"] is True
    assert split.train.index[-1] < split.validation.index[0]
    assert split.validation.index[-1] < split.holdout.index[0]
    assert len(split.train) == 500
    assert len(split.validation) == 200
    assert len(split.holdout) == 300


def test_risk_adjusted_score_v1_formula_is_frozen():
    train = {"Sharpe_Ratio": 1.0}
    validation = {"Sharpe_Ratio": 0.8, "Max_Drawdown_pct": -10.0, "closed_trades": 45}
    expected = 0.8 - abs(-10.0) / 25 - abs(1.0 - 0.8) * 0.25 - max(0, 45 - 40) * 0.01
    assert risk_adjusted_score_v1(train, validation) == expected


def test_spot_risk_candidate_grid_is_long_only_and_conservative():
    grid = default_risk_candidate_grid(ProductFamily.SPOT_CRYPTO, short_allowed=False)
    assert grid
    assert all(not settings.allow_short for settings in grid)
    assert max(settings.risk_fraction for settings in grid) <= 0.40


def test_risk_candidate_grid_uses_central_policy_direction_input():
    grid = default_risk_candidate_grid(ProductFamily.CRYPTO_PERP, short_allowed=True)
    assert grid
    assert all(settings.allow_short for settings in grid)


def test_risk_candidate_selection_uses_validation_not_holdout(monkeypatch):
    idx = pd.date_range("2024-01-01", periods=1000, freq="1h", tz="UTC")
    frame = pd.DataFrame({"Open": 1, "High": 2, "Low": 1, "Close": 1.5, "Volume": 1}, index=idx)
    low_validation = StrategySettings(10, 50, 14, 2.0, 3.0, 0.25, allow_short=False)
    high_validation = StrategySettings(20, 80, 14, 2.0, 3.0, 0.25, allow_short=False)
    calls: list[tuple[str, int]] = []

    def fake_backtest(frame, *, label, strategy_settings, broker_settings):
        del frame, broker_settings
        calls.append((label, strategy_settings.fast_sma))
        if label.endswith("_train"):
            sharpe = 0.5
        elif label.endswith("_validation"):
            sharpe = 0.1 if strategy_settings.fast_sma == 10 else 1.0
        else:
            assert label == "holdout_paper"
            assert strategy_settings.fast_sma == 20
            sharpe = -9.0
        metrics = {
            "label": label,
            "closed_trades": 2,
            "Sharpe_Ratio": sharpe,
            "Max_Drawdown_pct": -5.0,
            "Return_pct": sharpe,
        }
        return BacktestResult(
            label,
            metrics,
            pd.DataFrame({"PnL": [1.0]}),
            pd.DataFrame({"Equity": [10000.0]}, index=pd.date_range("2024-01-01", periods=1, tz="UTC")),
        )

    monkeypatch.setattr("parangsae.research.backtest_engine.run_strategy_backtest", fake_backtest)
    result = run_risk_candidate_search(
        frame,
        product_family=ProductFamily.SPOT_CRYPTO,
        short_allowed=False,
        candidate_grid=(low_validation, high_validation),
    )
    assert result.selected.settings.fast_sma == 20
    assert result.meaning["holdout_used_for_selection"] is False
    assert calls[-1] == ("holdout_paper", 20)
