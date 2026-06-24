# pyright: reportAttributeAccessIssue=false, reportArgumentType=false
from __future__ import annotations

import math
from dataclasses import asdict, dataclass
from typing import Any

import numpy as np
import pandas as pd
from backtesting import Strategy
from backtesting.lib import FractionalBacktest, crossover


@dataclass(frozen=True)
class StrategySettings:
    fast_sma: int = 12
    slow_sma: int = 48
    atr_window: int = 14
    stop_atr: float = 2.5
    take_profit_atr: float = 4.0
    risk_fraction: float = 0.95


@dataclass(frozen=True)
class BrokerSettings:
    cash: float = 10000.0
    commission: float = 0.0004
    spread: float = 0.0002
    margin: float = 1.0
    hedging: bool = False
    exclusive_orders: bool = True
    trade_on_close: bool = False
    finalize_trades: bool = True
    fractional_unit: float = 0.00000001


@dataclass(frozen=True)
class MeaningGateConfig:
    min_total_bars: int = 720
    min_train_bars: int = 480
    min_holdout_bars: int = 168
    min_backtest_closed_trades: int = 3
    min_holdout_closed_trades: int = 2


@dataclass(frozen=True)
class BacktestResult:
    label: str
    metrics: dict[str, Any]
    trades: pd.DataFrame
    equity: pd.DataFrame


class MeaningGateError(RuntimeError):
    def __init__(self, failed_gate: str, reason: str, observed: dict[str, Any]) -> None:
        super().__init__(reason)
        self.failed_gate = failed_gate
        self.reason = reason
        self.observed = observed


CANONICAL_STRATEGY = StrategySettings()
CANONICAL_BROKER = BrokerSettings()
MEANING_GATES = MeaningGateConfig()


def _sma(values: np.ndarray, window: int) -> np.ndarray:
    return pd.Series(values).rolling(window).mean().to_numpy()


def _atr(high: np.ndarray, low: np.ndarray, close: np.ndarray, window: int) -> np.ndarray:
    high_s = pd.Series(high, dtype="float64")
    low_s = pd.Series(low, dtype="float64")
    close_s = pd.Series(close, dtype="float64")
    prev_close = close_s.shift(1)
    ranges = pd.concat(
        [
            high_s - low_s,
            (high_s - prev_close).abs(),
            (low_s - prev_close).abs(),
        ],
        axis=1,
    ).max(axis=1)
    return ranges.rolling(window).mean().to_numpy()


class SmaAtrBracketStrategy(Strategy):
    fast_sma = CANONICAL_STRATEGY.fast_sma
    slow_sma = CANONICAL_STRATEGY.slow_sma
    atr_window = CANONICAL_STRATEGY.atr_window
    stop_atr = CANONICAL_STRATEGY.stop_atr
    take_profit_atr = CANONICAL_STRATEGY.take_profit_atr
    risk_fraction = CANONICAL_STRATEGY.risk_fraction

    def init(self) -> None:
        self.fast = self.I(_sma, self.data.Close, self.fast_sma, overlay=False)
        self.slow = self.I(_sma, self.data.Close, self.slow_sma, overlay=False)
        self.atr = self.I(_atr, self.data.High, self.data.Low, self.data.Close, self.atr_window, overlay=False)

    def next(self) -> None:
        price = float(self.data.Close[-1])
        atr = float(self.atr[-1])
        if not all(math.isfinite(v) for v in (price, atr, float(self.fast[-1]), float(self.slow[-1]))):
            return
        if atr <= 0:
            return
        if crossover(self.fast, self.slow):
            self.buy(
                size=self.risk_fraction,
                sl=max(price - self.stop_atr * atr, 1e-8),
                tp=price + self.take_profit_atr * atr,
            )
        elif crossover(self.slow, self.fast):
            self.sell(
                size=self.risk_fraction,
                sl=price + self.stop_atr * atr,
                tp=max(price - self.take_profit_atr * atr, 1e-8),
            )


def _jsonable(value: Any) -> Any:
    if isinstance(value, (np.integer,)):
        return int(value)
    if isinstance(value, (np.floating,)):
        value = float(value)
    if isinstance(value, float):
        if math.isnan(value) or math.isinf(value):
            return None
        return value
    if isinstance(value, pd.Timestamp):
        if value.tzinfo is None:
            value = value.tz_localize("UTC")
        else:
            value = value.tz_convert("UTC")
        return value.isoformat().replace("+00:00", "Z")
    if isinstance(value, pd.Timedelta):
        return str(value)
    if isinstance(value, np.ndarray):
        return [_jsonable(v) for v in value.tolist()]
    return value


def stats_to_metrics(stats: pd.Series, *, label: str) -> dict[str, Any]:
    keys = [
        "Start",
        "End",
        "Duration",
        "Exposure Time [%]",
        "Equity Final [$]",
        "Equity Peak [$]",
        "Return [%]",
        "Buy & Hold Return [%]",
        "Return (Ann.) [%]",
        "Volatility (Ann.) [%]",
        "CAGR [%]",
        "Sharpe Ratio",
        "Sortino Ratio",
        "Calmar Ratio",
        "Max. Drawdown [%]",
        "Avg. Drawdown [%]",
        "# Trades",
        "Win Rate [%]",
        "Best Trade [%]",
        "Worst Trade [%]",
        "Avg. Trade [%]",
        "Profit Factor",
        "Expectancy [%]",
        "SQN",
    ]
    metrics: dict[str, Any] = {"label": label}
    for key in keys:
        if key in stats:
            normalized = key.replace(" ", "_").replace("[%]", "pct").replace("[$]", "usd")
            normalized = normalized.replace(".", "").replace("#", "count").replace("__", "_").strip("_")
            metrics[normalized] = _jsonable(stats[key])
    metrics["closed_trades"] = int(stats.get("# Trades", 0) or 0)
    return metrics


def run_strategy_backtest(
    frame: pd.DataFrame,
    *,
    label: str,
    strategy_settings: StrategySettings = CANONICAL_STRATEGY,
    broker_settings: BrokerSettings = CANONICAL_BROKER,
) -> BacktestResult:
    bt = FractionalBacktest(
        frame,
        SmaAtrBracketStrategy,
        cash=broker_settings.cash,
        commission=broker_settings.commission,
        spread=broker_settings.spread,
        margin=broker_settings.margin,
        hedging=broker_settings.hedging,
        exclusive_orders=broker_settings.exclusive_orders,
        trade_on_close=broker_settings.trade_on_close,
        finalize_trades=broker_settings.finalize_trades,
        fractional_unit=broker_settings.fractional_unit,
    )
    stats = bt.run(**asdict(strategy_settings))
    trades = stats["_trades"].copy()
    equity = stats["_equity_curve"].copy()
    return BacktestResult(label=label, metrics=stats_to_metrics(stats, label=label), trades=trades, equity=equity)


def split_chronological(frame: pd.DataFrame, train_fraction: float = 0.70) -> tuple[pd.DataFrame, pd.DataFrame, int]:
    if not 0 < train_fraction < 1:
        raise ValueError("train_fraction must be between 0 and 1")
    split_index = int(len(frame) * train_fraction)
    train = frame.iloc[:split_index].copy()
    holdout = frame.iloc[split_index:].copy()
    if train.empty or holdout.empty:
        raise MeaningGateError(
            "split_integrity",
            "train and holdout segments must both contain rows",
            {"total_bars": len(frame), "split_index": split_index},
        )
    if train.index[-1] >= holdout.index[0]:
        raise MeaningGateError(
            "split_integrity",
            "train segment must end strictly before holdout begins",
            {
                "train_end": train.index[-1].isoformat(),
                "holdout_start": holdout.index[0].isoformat(),
                "split_index": split_index,
            },
        )
    return train, holdout, split_index


def assert_meaningful_run(
    *,
    total_bars: int,
    train_bars: int,
    holdout_bars: int,
    backtest_closed_trades: int,
    holdout_closed_trades: int,
    gates: MeaningGateConfig = MEANING_GATES,
) -> dict[str, Any]:
    observed = {
        "total_bars": total_bars,
        "train_bars": train_bars,
        "holdout_bars": holdout_bars,
        "backtest_closed_trades": backtest_closed_trades,
        "holdout_closed_trades": holdout_closed_trades,
        "required": asdict(gates),
    }
    checks = [
        ("total_bars", total_bars, gates.min_total_bars),
        ("train_bars", train_bars, gates.min_train_bars),
        ("holdout_bars", holdout_bars, gates.min_holdout_bars),
        ("backtest_closed_trades", backtest_closed_trades, gates.min_backtest_closed_trades),
        ("holdout_closed_trades", holdout_closed_trades, gates.min_holdout_closed_trades),
    ]
    for name, actual, minimum in checks:
        if actual < minimum:
            raise MeaningGateError(name, f"{name} {actual} below required minimum {minimum}", observed)
    return {"status": "passed", **observed}


def strategy_settings_dict(settings: StrategySettings = CANONICAL_STRATEGY) -> dict[str, Any]:
    return asdict(settings)


def broker_settings_dict(settings: BrokerSettings = CANONICAL_BROKER) -> dict[str, Any]:
    return asdict(settings)
