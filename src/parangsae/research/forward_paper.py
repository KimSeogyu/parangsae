from __future__ import annotations

import math
from dataclasses import asdict, dataclass
from typing import Any, cast

import pandas as pd

from parangsae.research.regime import RegimeReport, build_regime_report
from parangsae.research.signals import SignalReport, build_signal_report

FORWARD_PAPER_SCHEMA_VERSION = 1
POSITIONS_COLUMNS = [
    "close",
    "raw_score",
    "regime_label",
    "regime_risk_multiplier",
    "target_exposure",
    "applied_exposure",
    "position_notional",
    "bar_return_pct",
    "gross_pnl",
    "fee_cost",
    "slippage_cost",
    "funding_cost",
    "equity",
    "drawdown_pct",
]
POSITIONS_CSV_COLUMNS = ["Timestamp", *POSITIONS_COLUMNS]
EQUITY_COLUMNS = [
    "equity",
    "drawdown_pct",
    "applied_exposure",
    "target_exposure",
    "bar_return_pct",
    "gross_pnl",
    "total_cost",
]
EQUITY_CSV_COLUMNS = ["Timestamp", *EQUITY_COLUMNS]
TRADES_COLUMNS = [
    "entry_time",
    "exit_time",
    "entry_price",
    "exit_price",
    "entry_exposure",
    "exit_exposure",
    "gross_pnl",
    "fee_cost",
    "slippage_cost",
    "funding_cost",
    "net_pnl",
    "return_pct",
    "bars_held",
    "exit_reason",
]
CLAIM_BOUNDARY = (
    "forward-paper-run is public-market-data paper accounting for operator review; "
    "it is not exchange matching, live-fill proof, live readiness, trading advice, or a profit guarantee"
)
PAPER_SEMANTICS = (
    "close-to-close bar accounting with prior-bar exposure, configurable fee/slippage, "
    "and public funding events when supplied"
)


@dataclass(frozen=True)
class ForwardPaperConfig:
    initial_cash: float = 10000.0
    fee_rate: float = 0.0004
    slippage_bps: float = 5.0
    entry_threshold: float = 0.20
    exit_threshold: float = 0.05
    max_exposure: float = 1.0
    funding_source: str = "public-binance"


@dataclass(frozen=True)
class ForwardPaperResult:
    positions: pd.DataFrame
    equity: pd.DataFrame
    trades: pd.DataFrame
    metrics: dict[str, Any]
    regime_report: RegimeReport
    signal_report: SignalReport
    config: ForwardPaperConfig

    def to_json_payload(self) -> dict[str, Any]:
        return {
            "schema_version": FORWARD_PAPER_SCHEMA_VERSION,
            "config": asdict(self.config),
            "metrics": self.metrics,
            "artifact_schemas": {
                "positions.csv": POSITIONS_CSV_COLUMNS,
                "equity.csv": EQUITY_CSV_COLUMNS,
                "trades.csv": TRADES_COLUMNS,
            },
            "claim_boundary": CLAIM_BOUNDARY,
            "paper_semantics": PAPER_SEMANTICS,
        }


def run_forward_paper(
    frame: pd.DataFrame,
    *,
    config: ForwardPaperConfig | None = None,
    funding_frame: pd.DataFrame | None = None,
    symbol: str = "BTCUSDT",
    interval: str = "1h",
) -> ForwardPaperResult:
    cfg = config or ForwardPaperConfig()
    _validate_config(cfg)
    data = _normalize_ohlcv(frame)
    funding = _normalize_funding_frame(funding_frame)
    regime_report = build_regime_report(data)
    signal_report = build_signal_report(
        data,
        regimes=_as_series(regime_report.rows["regime_label"]),
        asset=symbol,
        timeframe=interval,
    )
    score_frame = exposure_frame_from_reports(data, regime_report, signal_report, config=cfg)
    positions, equity, trades, metrics = simulate_forward_positions(
        data,
        score_frame,
        funding_frame=funding,
        config=cfg,
    )
    metrics.update(
        {
            "schema_version": FORWARD_PAPER_SCHEMA_VERSION,
            "symbol": symbol,
            "interval": interval,
            "live_orders_submitted": False,
            "api_keys_required": False,
            "claim_boundary": CLAIM_BOUNDARY,
            "paper_semantics": PAPER_SEMANTICS,
            "latest_regime": regime_report.summary.get("latest"),
            "latest_signal": signal_report.summary.get("latest"),
            "signal_to_exposure_formula": {
                "raw_score": "0.45*direction + 0.25*entry + 0.20*risk - 0.10*max(exit,0)",
                "target": "long/flat thresholded target scaled by regime risk multiplier",
                "causality": "applied_exposure[t] = target_exposure[t-1]",
            },
        }
    )
    return ForwardPaperResult(positions, equity, trades, metrics, regime_report, signal_report, cfg)


def exposure_frame_from_reports(
    frame: pd.DataFrame,
    regime_report: RegimeReport,
    signal_report: SignalReport,
    *,
    config: ForwardPaperConfig,
) -> pd.DataFrame:
    data = _normalize_ohlcv(frame)
    role_scores = _role_scores_by_timestamp(signal_report.rows)
    aligned = pd.DataFrame(index=data.index)
    for column in ("direction_score", "entry_score", "risk_score", "exit_score"):
        aligned[column] = _as_series(role_scores.get(column, pd.Series(0.0, index=data.index)).reindex(data.index).fillna(0.0))
    exit_pressure = _as_series(aligned["exit_score"]).clip(lower=0.0)
    aligned["raw_score"] = _as_series(
        0.45 * _as_series(aligned["direction_score"])
        + 0.25 * _as_series(aligned["entry_score"])
        + 0.20 * _as_series(aligned["risk_score"])
        - 0.10 * exit_pressure
    ).clip(lower=-1.0, upper=1.0)
    regimes = _as_frame(regime_report.rows.reindex(data.index))
    aligned["regime_label"] = _as_series(regimes["regime_label"]).astype(str)
    aligned["regime_risk_multiplier"] = (
        _numeric_series(regimes["risk_multiplier"]).fillna(0.0).clip(lower=0.0, upper=config.max_exposure)
    )
    targets: list[float] = []
    previous_target = 0.0
    for _, row in aligned.iterrows():
        raw_score = _float_value(row["raw_score"])
        if raw_score >= config.entry_threshold:
            base_exposure = 1.0
        elif raw_score <= config.exit_threshold:
            base_exposure = 0.0
        else:
            base_exposure = previous_target
        target = _clamp(base_exposure * _float_value(row["regime_risk_multiplier"]), 0.0, config.max_exposure)
        targets.append(target)
        previous_target = target
    aligned["target_exposure"] = pd.Series(targets, index=aligned.index)
    aligned["applied_exposure"] = _as_series(aligned["target_exposure"]).shift(1).fillna(0.0)
    return aligned


def simulate_forward_positions(
    frame: pd.DataFrame,
    exposure_frame: pd.DataFrame,
    *,
    funding_frame: pd.DataFrame | None,
    config: ForwardPaperConfig,
) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame, dict[str, Any]]:
    data = _normalize_ohlcv(frame)
    scores = _as_frame(exposure_frame.reindex(data.index).copy())
    close = _as_series(data["Close"]).astype(float)
    returns = _as_series(close.pct_change().fillna(0.0))
    equity_value = float(config.initial_cash)
    peak_equity = equity_value
    previous_target = 0.0
    previous_timestamp: pd.Timestamp | None = None
    position_records: list[dict[str, Any]] = []
    trade_records: list[dict[str, Any]] = []
    active_trade: dict[str, Any] | None = None
    total_turnover = 0.0
    for raw_timestamp in data.index:
        timestamp = _as_timestamp(raw_timestamp)
        target = _float_value(scores.loc[timestamp, "target_exposure"])
        applied = _float_value(scores.loc[timestamp, "applied_exposure"])
        bar_return = _float_value(returns.loc[timestamp])
        gross_pnl = equity_value * applied * bar_return
        equity_after_gross = equity_value + gross_pnl
        turnover_notional = abs(target - previous_target) * equity_after_gross
        fee_cost = turnover_notional * float(config.fee_rate)
        slippage_cost = turnover_notional * float(config.slippage_bps) / 10000.0
        funding_cost = _funding_cost_for_bar(
            funding_frame,
            previous_timestamp=previous_timestamp,
            current_timestamp=timestamp,
            equity_after_gross=equity_after_gross,
            applied_exposure=applied,
        )
        equity_value = equity_after_gross - fee_cost - slippage_cost - funding_cost
        peak_equity = max(peak_equity, equity_value)
        drawdown_pct = 0.0 if peak_equity <= 0 else (equity_value / peak_equity - 1.0) * 100.0
        total_turnover += turnover_notional

        if previous_target == 0.0 and target > 0.0:
            active_trade = _new_trade(timestamp, _float_value(close.loc[timestamp]), target)
        if active_trade is not None:
            _accumulate_trade(
                active_trade,
                gross_pnl=gross_pnl,
                fee_cost=fee_cost,
                slippage_cost=slippage_cost,
                funding_cost=funding_cost,
            )
        if active_trade is not None and previous_target > 0.0 and target == 0.0:
            trade_records.append(
                _close_trade(
                    active_trade,
                    exit_time=timestamp,
                    exit_price=_float_value(close.loc[timestamp]),
                    exit_exposure=previous_target,
                    exit_reason="target_flat",
                )
            )
            active_trade = None

        position_records.append(
            {
                "Timestamp": timestamp,
                "close": _float_value(close.loc[timestamp]),
                "raw_score": _float_value(scores.loc[timestamp, "raw_score"]),
                "regime_label": str(scores.loc[timestamp, "regime_label"]),
                "regime_risk_multiplier": _float_value(scores.loc[timestamp, "regime_risk_multiplier"]),
                "target_exposure": target,
                "applied_exposure": applied,
                "position_notional": equity_value * target,
                "bar_return_pct": bar_return * 100.0,
                "gross_pnl": gross_pnl,
                "fee_cost": fee_cost,
                "slippage_cost": slippage_cost,
                "funding_cost": funding_cost,
                "equity": equity_value,
                "drawdown_pct": drawdown_pct,
            }
        )
        previous_target = target
        previous_timestamp = timestamp

    if active_trade is not None:
        last_timestamp = _as_timestamp(data.index[-1])
        trade_records.append(
            _close_trade(
                active_trade,
                exit_time=last_timestamp,
                exit_price=_float_value(close.loc[last_timestamp]),
                exit_exposure=previous_target,
                exit_reason="open_at_end",
            )
        )

    positions = _as_frame(pd.DataFrame.from_records(position_records).set_index("Timestamp"))
    positions = _as_frame(positions.loc[:, POSITIONS_COLUMNS])
    equity = _as_frame(
        positions.assign(
            total_cost=_as_series(positions["fee_cost"])
            + _as_series(positions["slippage_cost"])
            + _as_series(positions["funding_cost"])
        ).loc[:, EQUITY_COLUMNS]
    )
    trades = _as_frame(pd.DataFrame.from_records(trade_records, columns=TRADES_COLUMNS))
    metrics = _metrics(
        positions,
        trades,
        initial_cash=float(config.initial_cash),
        total_turnover=total_turnover,
        funding_frame=funding_frame,
        funding_source=config.funding_source,
    )
    return positions, equity, trades, metrics


def _role_scores_by_timestamp(rows: pd.DataFrame) -> dict[str, pd.Series]:
    if rows.empty:
        return {role: pd.Series(dtype="float64") for role in ("direction_score", "entry_score", "risk_score", "exit_score")}
    data = rows.copy()
    data["weighted_score"] = _numeric_series(data["weighted_score"]).fillna(0.0)
    scores: dict[str, pd.Series] = {}
    for role in ("direction", "entry", "risk", "exit"):
        role_rows = _as_frame(data[_as_series(data["role"]) == role])
        scores[f"{role}_score"] = (
            _as_series(role_rows.groupby(level=0)["weighted_score"].mean()) if not role_rows.empty else pd.Series(dtype="float64")
        )
    return scores


def _funding_cost_for_bar(
    funding_frame: pd.DataFrame | None,
    *,
    previous_timestamp: pd.Timestamp | None,
    current_timestamp: pd.Timestamp,
    equity_after_gross: float,
    applied_exposure: float,
) -> float:
    if funding_frame is None or funding_frame.empty or previous_timestamp is None or applied_exposure == 0.0:
        return 0.0
    window = _as_frame(funding_frame.loc[(funding_frame.index > previous_timestamp) & (funding_frame.index <= current_timestamp)])
    if window.empty:
        return 0.0
    rates = _numeric_series(window["funding_rate"]).dropna()
    return float(equity_after_gross * applied_exposure * _float_value(rates.sum()))


def _normalize_funding_frame(frame: pd.DataFrame | None) -> pd.DataFrame | None:
    if frame is None:
        return None
    data = frame.copy()
    if not isinstance(data.index, pd.DatetimeIndex):
        if "funding_time" not in data.columns:
            raise ValueError("funding frame requires a DatetimeIndex or funding_time column")
        data.index = pd.to_datetime(data["funding_time"], utc=True)
    if data.index.tz is None:
        data.index = data.index.tz_localize("UTC")
    else:
        data.index = data.index.tz_convert("UTC")
    if "funding_rate" not in data.columns:
        raise ValueError("funding frame requires funding_rate column")
    data = data.sort_index()
    data["funding_rate"] = _numeric_series(data["funding_rate"])
    if _bool_value(_as_series(data["funding_rate"]).isna().any()):
        raise ValueError("funding_rate contains non-numeric values")
    return data


def _new_trade(timestamp: pd.Timestamp, price: float, exposure: float) -> dict[str, Any]:
    return {
        "entry_time": _timestamp(timestamp),
        "entry_price": float(price),
        "entry_exposure": float(exposure),
        "gross_pnl": 0.0,
        "fee_cost": 0.0,
        "slippage_cost": 0.0,
        "funding_cost": 0.0,
        "bars_held": 0,
    }


def _accumulate_trade(
    trade: dict[str, Any],
    *,
    gross_pnl: float,
    fee_cost: float,
    slippage_cost: float,
    funding_cost: float,
) -> None:
    trade["gross_pnl"] += float(gross_pnl)
    trade["fee_cost"] += float(fee_cost)
    trade["slippage_cost"] += float(slippage_cost)
    trade["funding_cost"] += float(funding_cost)
    trade["bars_held"] += 1


def _close_trade(
    trade: dict[str, Any],
    *,
    exit_time: pd.Timestamp,
    exit_price: float,
    exit_exposure: float,
    exit_reason: str,
) -> dict[str, Any]:
    costs = float(trade["fee_cost"] + trade["slippage_cost"] + trade["funding_cost"])
    net_pnl = float(trade["gross_pnl"] - costs)
    entry_notional = float(trade["entry_price"] * max(trade["entry_exposure"], 1e-12))
    return_pct = 0.0 if entry_notional == 0.0 else net_pnl / entry_notional * 100.0
    return {
        "entry_time": trade["entry_time"],
        "exit_time": _timestamp(exit_time),
        "entry_price": float(trade["entry_price"]),
        "exit_price": float(exit_price),
        "entry_exposure": float(trade["entry_exposure"]),
        "exit_exposure": float(exit_exposure),
        "gross_pnl": float(trade["gross_pnl"]),
        "fee_cost": float(trade["fee_cost"]),
        "slippage_cost": float(trade["slippage_cost"]),
        "funding_cost": float(trade["funding_cost"]),
        "net_pnl": net_pnl,
        "return_pct": return_pct,
        "bars_held": int(trade["bars_held"]),
        "exit_reason": exit_reason,
    }


def _metrics(
    positions: pd.DataFrame,
    trades: pd.DataFrame,
    *,
    initial_cash: float,
    total_turnover: float,
    funding_frame: pd.DataFrame | None,
    funding_source: str,
) -> dict[str, Any]:
    final_equity = _float_value(_as_series(positions["equity"]).iloc[-1])
    closed = _as_frame(trades[_as_series(trades["exit_reason"]) != "open_at_end"]) if not trades.empty else trades
    wins = _as_frame(closed[_as_series(closed["net_pnl"]) > 0]) if not closed.empty else closed
    return {
        "status": "success",
        "run_type": "forward-paper-run",
        "initial_cash": initial_cash,
        "final_equity": final_equity,
        "return_pct": (final_equity / initial_cash - 1.0) * 100.0,
        "max_drawdown_pct": _float_value(_as_series(positions["drawdown_pct"]).min()),
        "bars": int(len(positions)),
        "exposure_time_pct": _float_value(_as_series(_as_series(positions["applied_exposure"]) > 0).mean() * 100.0),
        "turnover_notional": float(total_turnover),
        "fee_cost_total": _float_value(_as_series(positions["fee_cost"]).sum()),
        "slippage_cost_total": _float_value(_as_series(positions["slippage_cost"]).sum()),
        "funding_cost_total": _float_value(_as_series(positions["funding_cost"]).sum()),
        "funding_source": funding_source,
        "funding_rows": int(len(funding_frame)) if funding_frame is not None else 0,
        "closed_trades": int(len(closed)),
        "trade_rows": int(len(trades)),
        "win_rate_pct": None if closed.empty else float(len(wins) / len(closed) * 100.0),
        "open_position": _float_value(_as_series(positions["target_exposure"]).iloc[-1]),
        "live_orders_submitted": False,
        "api_keys_required": False,
    }


def _validate_config(config: ForwardPaperConfig) -> None:
    if config.initial_cash <= 0:
        raise ValueError("initial_cash must be positive")
    if config.fee_rate < 0:
        raise ValueError("fee_rate must be non-negative")
    if config.slippage_bps < 0:
        raise ValueError("slippage_bps must be non-negative")
    if config.max_exposure < 0 or config.max_exposure > 1:
        raise ValueError("max_exposure must be between 0 and 1")
    if config.exit_threshold > config.entry_threshold:
        raise ValueError("exit_threshold must be <= entry_threshold")


def _normalize_ohlcv(frame: pd.DataFrame) -> pd.DataFrame:
    data = frame.copy()
    if not isinstance(data.index, pd.DatetimeIndex):
        raise ValueError("frame index must be a DatetimeIndex")
    if data.index.tz is None:
        data.index = data.index.tz_localize("UTC")
    else:
        data.index = data.index.tz_convert("UTC")
    missing = [column for column in ("Open", "High", "Low", "Close", "Volume") if column not in data.columns]
    if missing:
        raise ValueError(f"missing OHLCV columns: {missing}")
    data = data.sort_index()
    return _as_frame(data.loc[:, ["Open", "High", "Low", "Close", "Volume"]].astype(float))


def _clamp(value: float, lower: float, upper: float) -> float:
    if not math.isfinite(value):
        return lower
    return min(max(float(value), lower), upper)


def _timestamp(value: pd.Timestamp) -> str:
    timestamp = value.tz_localize("UTC") if value.tzinfo is None else value.tz_convert("UTC")
    return timestamp.isoformat().replace("+00:00", "Z")


def _as_frame(value: Any) -> pd.DataFrame:
    return cast(pd.DataFrame, value)


def _as_series(value: Any) -> pd.Series:
    return cast(pd.Series, value)


def _as_timestamp(value: Any) -> pd.Timestamp:
    return cast(pd.Timestamp, value)


def _numeric_series(value: Any) -> pd.Series:
    return cast(pd.Series, pd.to_numeric(value, errors="coerce"))


def _float_value(value: Any) -> float:
    return float(value)


def _bool_value(value: Any) -> bool:
    return bool(value)
