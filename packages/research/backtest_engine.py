# pyright: reportAttributeAccessIssue=false, reportArgumentType=false
from __future__ import annotations

import math
from dataclasses import asdict, dataclass
from typing import Any

import numpy as np
import pandas as pd
from backtesting import Strategy
from backtesting.lib import FractionalBacktest, crossover

from packages.core.contracts import ProductFamily


@dataclass(frozen=True)
class StrategySettings:
    fast_sma: int = 12
    slow_sma: int = 48
    atr_window: int = 14
    stop_atr: float = 2.5
    take_profit_atr: float = 4.0
    risk_fraction: float = 0.95
    allow_short: bool = True


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
class RiskMeaningGateConfig:
    min_total_bars: int = 720
    min_train_bars: int = 480
    min_validation_bars: int = 168
    min_holdout_bars: int = 168
    min_train_closed_trades: int = 1
    min_validation_closed_trades: int = 1
    min_holdout_closed_trades: int = 1
    max_validation_drawdown_abs_pct: float = 35.0


@dataclass(frozen=True)
class BacktestResult:
    label: str
    metrics: dict[str, Any]
    trades: pd.DataFrame
    equity: pd.DataFrame


@dataclass(frozen=True)
class ThreeWaySplit:
    train: pd.DataFrame
    validation: pd.DataFrame
    holdout: pd.DataFrame
    metadata: dict[str, Any]


@dataclass(frozen=True)
class CandidateEvaluation:
    candidate_id: str
    settings: StrategySettings
    train: BacktestResult
    validation: BacktestResult
    score: float
    eligible: bool
    rejection_reasons: tuple[str, ...]

    def to_record(self) -> dict[str, Any]:
        return {
            "candidate_id": self.candidate_id,
            "eligible": self.eligible,
            "score": self.score,
            "rejection_reasons": ";".join(self.rejection_reasons),
            **{f"settings_{key}": value for key, value in asdict(self.settings).items()},
            "train_sharpe": self.train.metrics.get("Sharpe_Ratio"),
            "validation_sharpe": self.validation.metrics.get("Sharpe_Ratio"),
            "train_max_drawdown_pct": self.train.metrics.get("Max_Drawdown_pct"),
            "validation_max_drawdown_pct": self.validation.metrics.get("Max_Drawdown_pct"),
            "train_return_pct": self.train.metrics.get("Return_pct"),
            "validation_return_pct": self.validation.metrics.get("Return_pct"),
            "train_closed_trades": self.train.metrics.get("closed_trades"),
            "validation_closed_trades": self.validation.metrics.get("closed_trades"),
        }


@dataclass(frozen=True)
class RiskRunResult:
    product_family: ProductFamily
    split: ThreeWaySplit
    candidates: tuple[CandidateEvaluation, ...]
    selected: CandidateEvaluation
    holdout: BacktestResult
    meaning: dict[str, Any]


class MeaningGateError(RuntimeError):
    def __init__(self, failed_gate: str, reason: str, observed: dict[str, Any]) -> None:
        super().__init__(reason)
        self.failed_gate = failed_gate
        self.reason = reason
        self.observed = observed


CANONICAL_STRATEGY = StrategySettings()
CANONICAL_BROKER = BrokerSettings()
MEANING_GATES = MeaningGateConfig()
RISK_MEANING_GATES = RiskMeaningGateConfig()
RISK_SELECTION_OBJECTIVE = "risk_adjusted_score_v1"
RISK_SELECTION_FORMULA = (
    "validation_sharpe - abs(validation_max_drawdown_pct)/25 "
    "- abs(train_sharpe-validation_sharpe)*0.25 "
    "- max(0, validation_closed_trades-40)*0.01"
)


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
    allow_short = CANONICAL_STRATEGY.allow_short

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
            if self.allow_short:
                self.sell(
                    size=self.risk_fraction,
                    sl=price + self.stop_atr * atr,
                    tp=max(price - self.take_profit_atr * atr, 1e-8),
                )
            elif self.position:
                self.position.close()


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


def _index_iso(frame: pd.DataFrame, position: int) -> str | None:
    if frame.empty:
        return None
    value = frame.index[position]
    return value.isoformat().replace("+00:00", "Z") if hasattr(value, "isoformat") else str(value)


def split_train_validation_holdout(
    frame: pd.DataFrame,
    *,
    train_fraction: float = 0.50,
    validation_fraction: float = 0.20,
) -> ThreeWaySplit:
    if not 0 < train_fraction < 1:
        raise ValueError("train_fraction must be between 0 and 1")
    if not 0 < validation_fraction < 1:
        raise ValueError("validation_fraction must be between 0 and 1")
    if train_fraction + validation_fraction >= 1:
        raise ValueError("train_fraction + validation_fraction must be less than 1")
    train_end = int(len(frame) * train_fraction)
    validation_end = int(len(frame) * (train_fraction + validation_fraction))
    train = frame.iloc[:train_end].copy()
    validation = frame.iloc[train_end:validation_end].copy()
    holdout = frame.iloc[validation_end:].copy()
    if train.empty or validation.empty or holdout.empty:
        raise MeaningGateError(
            "split_integrity",
            "train, validation, and holdout segments must all contain rows",
            {
                "total_bars": len(frame),
                "train_end_index": train_end,
                "validation_end_index": validation_end,
            },
        )
    if not (train.index[-1] < validation.index[0] < validation.index[-1] < holdout.index[0]):
        raise MeaningGateError(
            "split_integrity",
            "chronological segments must be strictly ordered and non-overlapping",
            {
                "train_end": _index_iso(train, -1),
                "validation_start": _index_iso(validation, 0),
                "validation_end": _index_iso(validation, -1),
                "holdout_start": _index_iso(holdout, 0),
            },
        )
    metadata = {
        "chronological_split_verified": True,
        "total_bars": len(frame),
        "train_bars": len(train),
        "validation_bars": len(validation),
        "holdout_bars": len(holdout),
        "train_end_index": train_end,
        "validation_end_index": validation_end,
        "train_start": _index_iso(train, 0),
        "train_end": _index_iso(train, -1),
        "validation_start": _index_iso(validation, 0),
        "validation_end": _index_iso(validation, -1),
        "holdout_start": _index_iso(holdout, 0),
        "holdout_end": _index_iso(holdout, -1),
    }
    return ThreeWaySplit(train=train, validation=validation, holdout=holdout, metadata=metadata)


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


def _metric_as_float(metrics: dict[str, Any], key: str, default: float = 0.0) -> float:
    value = metrics.get(key)
    try:
        result = float(value)
    except (TypeError, ValueError):
        return default
    return result if math.isfinite(result) else default


def risk_adjusted_score_v1(train_metrics: dict[str, Any], validation_metrics: dict[str, Any]) -> float:
    train_sharpe = _metric_as_float(train_metrics, "Sharpe_Ratio")
    validation_sharpe = _metric_as_float(validation_metrics, "Sharpe_Ratio")
    validation_drawdown = _metric_as_float(validation_metrics, "Max_Drawdown_pct")
    validation_closed_trades = int(validation_metrics.get("closed_trades", 0) or 0)
    return (
        validation_sharpe
        - abs(validation_drawdown) / 25
        - abs(train_sharpe - validation_sharpe) * 0.25
        - max(0, validation_closed_trades - 40) * 0.01
    )


def _candidate_rejection_reasons(
    *,
    train: BacktestResult,
    validation: BacktestResult,
    gates: RiskMeaningGateConfig,
) -> tuple[str, ...]:
    reasons: list[str] = []
    if train.metrics["closed_trades"] < gates.min_train_closed_trades:
        reasons.append("train_closed_trades_below_minimum")
    if validation.metrics["closed_trades"] < gates.min_validation_closed_trades:
        reasons.append("validation_closed_trades_below_minimum")
    validation_drawdown_raw = validation.metrics.get("Max_Drawdown_pct")
    try:
        validation_drawdown = float(validation_drawdown_raw)
    except (TypeError, ValueError):
        validation_drawdown = math.nan
    if not math.isfinite(validation_drawdown):
        reasons.append("validation_drawdown_not_finite")
    elif abs(validation_drawdown) > gates.max_validation_drawdown_abs_pct:
        reasons.append("validation_drawdown_above_gate")
    return tuple(reasons)


def default_risk_candidate_grid(
    product_family: ProductFamily,
    *,
    short_allowed: bool,
) -> tuple[StrategySettings, ...]:
    del product_family  # Direction authority is supplied by central product policy.
    return (
        StrategySettings(12, 48, 14, 2.0, 3.0, 0.25, allow_short=short_allowed),
        StrategySettings(12, 48, 14, 2.5, 4.0, 0.25, allow_short=short_allowed),
        StrategySettings(18, 72, 14, 2.0, 3.0, 0.25, allow_short=short_allowed),
        StrategySettings(18, 72, 14, 2.5, 4.0, 0.40, allow_short=short_allowed),
        StrategySettings(24, 96, 14, 2.5, 4.0, 0.25, allow_short=short_allowed),
        StrategySettings(24, 96, 14, 3.0, 5.0, 0.40, allow_short=short_allowed),
    )


def _candidate_id(index: int, settings: StrategySettings) -> str:
    direction = "short-ok" if settings.allow_short else "long-only"
    return f"c{index:03d}-f{settings.fast_sma}-s{settings.slow_sma}-{direction}"


def assert_risk_meaningful_run(
    *,
    split: ThreeWaySplit,
    candidate_count: int,
    train_closed_trades: int,
    validation_closed_trades: int,
    holdout_closed_trades: int,
    gates: RiskMeaningGateConfig = RISK_MEANING_GATES,
) -> dict[str, Any]:
    observed = {
        "status": "passed",
        "holdout_used_for_selection": False,
        "candidate_count": candidate_count,
        "selection_objective": RISK_SELECTION_OBJECTIVE,
        "selection_formula": RISK_SELECTION_FORMULA,
        "minimum_trade_gates_passed": True,
        **split.metadata,
        "train_closed_trades": train_closed_trades,
        "validation_closed_trades": validation_closed_trades,
        "holdout_closed_trades": holdout_closed_trades,
        "required": asdict(gates),
    }
    checks = [
        ("total_bars", split.metadata["total_bars"], gates.min_total_bars),
        ("train_bars", split.metadata["train_bars"], gates.min_train_bars),
        ("validation_bars", split.metadata["validation_bars"], gates.min_validation_bars),
        ("holdout_bars", split.metadata["holdout_bars"], gates.min_holdout_bars),
        ("candidate_count", candidate_count, 1),
        ("train_closed_trades", train_closed_trades, gates.min_train_closed_trades),
        ("validation_closed_trades", validation_closed_trades, gates.min_validation_closed_trades),
        ("holdout_closed_trades", holdout_closed_trades, gates.min_holdout_closed_trades),
    ]
    for name, actual, minimum in checks:
        if actual < minimum:
            raise MeaningGateError(name, f"{name} {actual} below required minimum {minimum}", observed)
    return observed


def run_risk_candidate_search(
    frame: pd.DataFrame,
    *,
    product_family: ProductFamily = ProductFamily.SPOT_CRYPTO,
    short_allowed: bool,
    candidate_grid: tuple[StrategySettings, ...] | None = None,
    broker_settings: BrokerSettings = CANONICAL_BROKER,
    gates: RiskMeaningGateConfig = RISK_MEANING_GATES,
) -> RiskRunResult:
    split = split_train_validation_holdout(frame)
    candidates_settings = candidate_grid or default_risk_candidate_grid(
        product_family,
        short_allowed=short_allowed,
    )
    if not candidates_settings:
        raise MeaningGateError("candidate_count", "candidate grid must not be empty", split.metadata)
    evaluations: list[CandidateEvaluation] = []
    for index, settings in enumerate(candidates_settings, start=1):
        train = run_strategy_backtest(
            split.train,
            label=f"{_candidate_id(index, settings)}_train",
            strategy_settings=settings,
            broker_settings=broker_settings,
        )
        validation = run_strategy_backtest(
            split.validation,
            label=f"{_candidate_id(index, settings)}_validation",
            strategy_settings=settings,
            broker_settings=broker_settings,
        )
        reasons = _candidate_rejection_reasons(train=train, validation=validation, gates=gates)
        evaluations.append(
            CandidateEvaluation(
                candidate_id=_candidate_id(index, settings),
                settings=settings,
                train=train,
                validation=validation,
                score=risk_adjusted_score_v1(train.metrics, validation.metrics),
                eligible=not reasons,
                rejection_reasons=reasons,
            )
        )
    eligible = [candidate for candidate in evaluations if candidate.eligible]
    if not eligible:
        raise MeaningGateError(
            "candidate_selection",
            "no candidate passed risk selection gates",
            {
                **split.metadata,
                "candidate_count": len(evaluations),
                "candidates": [candidate.to_record() for candidate in evaluations],
            },
        )
    selected = sorted(
        eligible,
        key=lambda candidate: (
            candidate.score,
            -abs(_metric_as_float(candidate.validation.metrics, "Max_Drawdown_pct")),
            candidate.validation.metrics.get("closed_trades", 0),
        ),
        reverse=True,
    )[0]
    holdout = run_strategy_backtest(
        split.holdout,
        label="holdout_paper",
        strategy_settings=selected.settings,
        broker_settings=broker_settings,
    )
    meaning = assert_risk_meaningful_run(
        split=split,
        candidate_count=len(evaluations),
        train_closed_trades=selected.train.metrics["closed_trades"],
        validation_closed_trades=selected.validation.metrics["closed_trades"],
        holdout_closed_trades=holdout.metrics["closed_trades"],
        gates=gates,
    )
    return RiskRunResult(
        product_family=product_family,
        split=split,
        candidates=tuple(evaluations),
        selected=selected,
        holdout=holdout,
        meaning=meaning,
    )


def strategy_settings_dict(settings: StrategySettings = CANONICAL_STRATEGY) -> dict[str, Any]:
    return asdict(settings)


def broker_settings_dict(settings: BrokerSettings = CANONICAL_BROKER) -> dict[str, Any]:
    return asdict(settings)
