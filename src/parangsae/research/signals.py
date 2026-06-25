from __future__ import annotations

from dataclasses import asdict, dataclass, replace
from typing import Any

import pandas as pd

SIGNAL_SCHEMA_VERSION = 1
SIGNAL_ROW_COLUMNS = [
    "signal_id",
    "asset",
    "timeframe",
    "category",
    "family",
    "role",
    "lookback",
    "segment",
    "regime",
    "value",
    "direction",
    "confidence",
    "expiry_bars",
    "normalized_value",
    "weight",
    "weighted_score",
    "description",
]
SIGNAL_CSV_COLUMNS = ["Timestamp", *SIGNAL_ROW_COLUMNS]
SIGNAL_JSON_TOP_LEVEL_KEYS = [
    "schema_version",
    "method",
    "config",
    "catalog",
    "weights",
    "claim_boundary",
    "summary",
]
SCHEMA_VERSION_POLICY = (
    "schema_version 1 is frozen for the v1 signal report contract; breaking "
    "changes to columns, keys, scoring semantics, or computed category meanings "
    "require a schema_version bump and migration note"
)
CLAIM_BOUNDARY = (
    "alpha signal factory scores are causal OHLCV research diagnostics for screening "
    "and review, not trading advice, live-fill proof, model alpha proof, or a profit guarantee"
)
EXECUTION_BOUNDARY = (
    "SignalReport summary decisions are diagnostic labels only; execution, paper, "
    "or live-order code must not consume them as order intents without a separately "
    "approved strategy adapter and validation protocol"
)

FINAL_SCORE_WEIGHTS = {
    "direction_score": 0.45,
    "entry_score": 0.25,
    "risk_score": 0.20,
    "exit_score": 0.10,
}

SIGNAL_CATALOG: dict[str, Any] = {
    "computed_categories": {
        "trend": ["ma_distance", "ma_slope", "ma_stack"],
        "momentum": ["roc", "rsi"],
        "mean_reversion": ["zscore"],
        "volatility": ["atr_pct", "realized_vol_ratio"],
        "volume": ["volume_z_confirmation"],
        "structure": ["donchian_breakout"],
        "regime": ["regime_permission_bridge"],
        "exit": ["close_below_sma20", "roc_decay"],
    },
    "catalog_only_categories": {
        "derivatives": {
            "status": "not_computed_without_data_source",
            "examples": ["funding_rate", "open_interest", "basis", "long_short_ratio"],
        },
        "orderbook_orderflow": {
            "status": "not_computed_without_data_source",
            "examples": ["book_imbalance", "spread", "cvd", "taker_buy_sell"],
        },
        "on_chain": {
            "status": "not_computed_without_data_source",
            "examples": ["exchange_inflow", "mvrv", "nvt", "active_addresses"],
        },
        "options": {
            "status": "not_computed_without_data_source",
            "examples": ["iv_rank", "put_call_ratio", "skew", "gamma_exposure"],
        },
        "macro_events": {
            "status": "not_computed_without_data_source",
            "examples": ["rates", "dxy", "cpi_fomc_window", "liquidity"],
        },
        "sentiment_news": {
            "status": "not_computed_without_data_source",
            "examples": ["news_score", "social_volume", "fear_greed", "etf_flows"],
        },
        "execution": {
            "status": "not_computed_without_data_source",
            "examples": ["estimated_slippage", "market_depth", "limit_vs_market"],
        },
    },
}


@dataclass(frozen=True)
class SignalFactoryConfig:
    ma_windows: tuple[int, ...] = (20, 60, 200)
    ma_slope_lag: int = 5
    roc_windows: tuple[int, ...] = (20, 60)
    rsi_window: int = 14
    zscore_window: int = 20
    atr_window: int = 14
    realized_vol_fast_bars: int = 24
    realized_vol_slow_bars: int = 168
    volume_z_window: int = 60
    donchian_windows: tuple[int, ...] = (20, 60)
    expiry_bars: int = 3
    asset: str = "BTCUSDT"
    timeframe: str = "1h"


@dataclass(frozen=True)
class SignalReport:
    rows: pd.DataFrame
    summary: dict[str, Any]
    config: SignalFactoryConfig = SignalFactoryConfig()

    def to_json_payload(self) -> dict[str, Any]:
        return {
            "schema_version": SIGNAL_SCHEMA_VERSION,
            "method": {
                "name": "causal_ohlcv_alpha_signal_factory_v1",
                "causal_no_lookahead": True,
                "inputs": ["Open", "High", "Low", "Close", "Volume"],
                "rows_schema": SIGNAL_CSV_COLUMNS,
                "schema_version_policy": SCHEMA_VERSION_POLICY,
                "execution_boundary": EXECUTION_BOUNDARY,
                "duplicate_control": (
                    "scores are first normalized per signal, then summarized by role/family so "
                    "multiple indicators in one family cannot dominate only by count"
                ),
            },
            "config": asdict(self.config),
            "catalog": SIGNAL_CATALOG,
            "weights": {
                "final_score": FINAL_SCORE_WEIGHTS,
                "row_weight_semantics": "weighted_score = normalized_value * weight",
                "role_score_semantics": "role scores average family-level normalized weighted scores",
            },
            "claim_boundary": CLAIM_BOUNDARY,
            "summary": self.summary,
        }


def build_signal_report(
    frame: pd.DataFrame,
    *,
    segments: pd.Series | list[str] | tuple[str, ...] | None = None,
    regimes: pd.Series | list[str] | tuple[str, ...] | None = None,
    asset: str | None = None,
    timeframe: str | None = None,
    config: SignalFactoryConfig | None = None,
) -> SignalReport:
    if config is None:
        cfg = SignalFactoryConfig(asset=asset or "BTCUSDT", timeframe=timeframe or "1h")
    else:
        cfg = replace(
            config,
            asset=config.asset if asset is None else asset,
            timeframe=config.timeframe if timeframe is None else timeframe,
        )
    rows = generate_signals(frame, segments=segments, regimes=regimes, config=cfg)
    return SignalReport(rows=rows, summary=summarize_signals(rows), config=cfg)


def generate_signals(
    frame: pd.DataFrame,
    *,
    segments: pd.Series | list[str] | tuple[str, ...] | None = None,
    regimes: pd.Series | list[str] | tuple[str, ...] | None = None,
    config: SignalFactoryConfig = SignalFactoryConfig(),
) -> pd.DataFrame:
    data = _normalize_ohlcv(frame)
    segment_series = _aligned_labels(data, segments, default="research", label="segments")
    regime_series = _aligned_labels(data, regimes, default="unknown", label="regimes")

    close = data["Close"].astype(float)
    high = data["High"].astype(float)
    low = data["Low"].astype(float)
    volume = data["Volume"].astype(float)
    returns = close.pct_change()

    features: dict[str, pd.Series] = {}
    for window in config.ma_windows:
        sma = close.rolling(window, min_periods=1).mean()
        features[f"sma_{window}"] = sma
        features[f"sma_distance_{window}"] = (close - sma) / _nonzero(sma)
        features[f"sma_slope_{window}"] = (sma - sma.shift(config.ma_slope_lag)) / _nonzero(sma.shift(config.ma_slope_lag))
    for window in config.roc_windows:
        features[f"roc_{window}"] = close.pct_change(window)

    features["rsi"] = _rsi(close, config.rsi_window)
    rolling_mean = close.rolling(config.zscore_window, min_periods=2).mean()
    rolling_std = close.rolling(config.zscore_window, min_periods=2).std()
    features["zscore"] = (close - rolling_mean) / rolling_std.where(rolling_std > 0)
    features["atr_pct"] = _atr_pct(high, low, close, config.atr_window)
    vol_fast = returns.rolling(config.realized_vol_fast_bars, min_periods=2).std()
    vol_slow = returns.rolling(config.realized_vol_slow_bars, min_periods=2).std()
    features["realized_vol_ratio"] = vol_fast / vol_slow.where(vol_slow > 0)
    volume_mean = volume.rolling(config.volume_z_window, min_periods=2).mean()
    volume_std = volume.rolling(config.volume_z_window, min_periods=2).std()
    features["volume_z"] = (volume - volume_mean) / volume_std.where(volume_std > 0)
    for window in config.donchian_windows:
        features[f"donchian_high_{window}"] = high.rolling(window, min_periods=1).max().shift(1)
        features[f"donchian_low_{window}"] = low.rolling(window, min_periods=1).min().shift(1)

    records: list[dict[str, Any]] = []
    for timestamp in data.index:
        segment = str(segment_series.loc[timestamp])
        regime = str(regime_series.loc[timestamp])
        prefix = f"{config.asset}_{config.timeframe}"
        for window in config.ma_windows:
            distance = _number(features[f"sma_distance_{window}"].loc[timestamp])
            records.append(
                _record(
                    timestamp,
                    signal_id=f"{prefix}_TREND_CLOSE_VS_SMA{window}",
                    config=config,
                    category="trend",
                    family="trend_ma",
                    role="direction",
                    lookback=window,
                    segment=segment,
                    regime=regime,
                    value=distance,
                    direction=_sign(distance),
                    confidence=_clip(abs(distance) / 0.02),
                    weight=0.7,
                    description=f"Close distance to SMA{window}; positive is trend-up evidence.",
                )
            )
            slope = _number(features[f"sma_slope_{window}"].loc[timestamp])
            records.append(
                _record(
                    timestamp,
                    signal_id=f"{prefix}_TREND_SMA{window}_SLOPE", 
                    config=config,
                    category="trend",
                    family="trend_ma",
                    role="direction",
                    lookback=window,
                    segment=segment,
                    regime=regime,
                    value=slope,
                    direction=_sign(slope),
                    confidence=_clip(abs(slope) / 0.01),
                    weight=0.5,
                    description=f"SMA{window} slope over {config.ma_slope_lag} bars.",
                )
            )

        stack_value = _ma_stack_value(close.loc[timestamp], features, timestamp)
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_TREND_MA20_60_200_STACK",
                config=config,
                category="trend",
                family="trend_structure",
                role="direction",
                lookback=max(config.ma_windows),
                segment=segment,
                regime=regime,
                value=stack_value,
                direction=_sign(stack_value),
                confidence=abs(stack_value),
                weight=0.8,
                description="Close/SMA20/SMA60/SMA200 bullish or bearish stack state.",
            )
        )

        for window in config.roc_windows:
            roc = _number(features[f"roc_{window}"].loc[timestamp])
            records.append(
                _record(
                    timestamp,
                    signal_id=f"{prefix}_MOMENTUM_ROC{window}",
                    config=config,
                    category="momentum",
                    family="momentum_roc",
                    role="direction",
                    lookback=window,
                    segment=segment,
                    regime=regime,
                    value=roc,
                    direction=_sign(roc),
                    confidence=_clip(abs(roc) / 0.05),
                    weight=0.6,
                    description=f"{window}-bar rate of change.",
                )
            )

        rsi = _number(features["rsi"].loc[timestamp], default=50.0)
        rsi_entry_direction = 1 if rsi < 35 else -1 if rsi > 65 else 0
        rsi_entry_confidence = _clip((35 - rsi) / 15) if rsi < 35 else _clip((rsi - 65) / 15) if rsi > 65 else 0.0
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_MOMENTUM_RSI{config.rsi_window}_STATE",
                config=config,
                category="momentum",
                family="momentum_rsi",
                role="entry",
                lookback=config.rsi_window,
                segment=segment,
                regime=regime,
                value=rsi,
                direction=rsi_entry_direction,
                confidence=rsi_entry_confidence,
                weight=0.5,
                description="RSI oversold/overbought state; oversold supports entry, overbought warns against chasing.",
            )
        )
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_RISK_RSI{config.rsi_window}_OVERHEAT",
                config=config,
                category="momentum",
                family="momentum_rsi",
                role="risk",
                lookback=config.rsi_window,
                segment=segment,
                regime=regime,
                value=rsi,
                direction=-1 if rsi > 70 else 0,
                confidence=_clip((rsi - 70) / 15) if rsi > 70 else 0.0,
                weight=0.4,
                description="RSI above 70 is overheating risk, not a standalone short signal.",
            )
        )

        zscore = _number(features["zscore"].loc[timestamp])
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_MEAN_REVERSION_ZSCORE{config.zscore_window}",
                config=config,
                category="mean_reversion",
                family="mean_reversion_zscore",
                role="entry",
                lookback=config.zscore_window,
                segment=segment,
                regime=regime,
                value=zscore,
                direction=1 if zscore < -1.5 else -1 if zscore > 1.5 else 0,
                confidence=_clip((abs(zscore) - 1.5) / 1.5),
                weight=0.6,
                description="Close z-score vs rolling mean/std; extremes are mean-reversion diagnostics.",
            )
        )

        atr_pct = _number(features["atr_pct"].loc[timestamp])
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_VOLATILITY_ATR{config.atr_window}_PCT",
                config=config,
                category="volatility",
                family="volatility_state",
                role="risk",
                lookback=config.atr_window,
                segment=segment,
                regime=regime,
                value=atr_pct,
                direction=-1 if atr_pct > 0.03 else 1 if 0 < atr_pct < 0.01 else 0,
                confidence=_clip(atr_pct / 0.03) if atr_pct > 0.03 else _clip((0.01 - atr_pct) / 0.01) if 0 < atr_pct < 0.01 else 0.0,
                weight=0.7,
                description="ATR/Close risk state; high ATR reduces permission.",
            )
        )
        vol_ratio = _number(features["realized_vol_ratio"].loc[timestamp])
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_VOLATILITY_REALIZED_RATIO_{config.realized_vol_fast_bars}_{config.realized_vol_slow_bars}",
                config=config,
                category="volatility",
                family="volatility_state",
                role="risk",
                lookback=config.realized_vol_slow_bars,
                segment=segment,
                regime=regime,
                value=vol_ratio,
                direction=-1 if vol_ratio > 1.5 else 1 if 0 < vol_ratio < 0.8 else 0,
                confidence=_clip((vol_ratio - 1.5) / 1.5) if vol_ratio > 1.5 else _clip((0.8 - vol_ratio) / 0.8) if 0 < vol_ratio < 0.8 else 0.0,
                weight=0.7,
                description="Fast/slow realized volatility ratio; expansion reduces risk permission.",
            )
        )

        volume_z = _number(features["volume_z"].loc[timestamp])
        ret = _number(returns.loc[timestamp])
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_VOLUME_Z{config.volume_z_window}_CONFIRMATION",
                config=config,
                category="volume",
                family="volume_confirmation",
                role="entry",
                lookback=config.volume_z_window,
                segment=segment,
                regime=regime,
                value=volume_z,
                direction=_sign(ret) if volume_z > 1 else 0,
                confidence=_clip(volume_z / 3) if volume_z > 1 else 0.0,
                weight=0.5,
                description="Volume z-score confirms the current return direction only when elevated.",
            )
        )

        for window in config.donchian_windows:
            previous_high = features[f"donchian_high_{window}"].loc[timestamp]
            previous_low = features[f"donchian_low_{window}"].loc[timestamp]
            if pd.isna(previous_high) or pd.isna(previous_low):
                breakout_value = 0.0
            elif close.loc[timestamp] > previous_high:
                breakout_value = 1.0
            elif close.loc[timestamp] < previous_low:
                breakout_value = -1.0
            else:
                breakout_value = 0.0
            records.append(
                _record(
                    timestamp,
                    signal_id=f"{prefix}_STRUCTURE_DONCHIAN{window}_BREAK", 
                    config=config,
                    category="structure",
                    family="structure_breakout",
                    role="direction",
                    lookback=window,
                    segment=segment,
                    regime=regime,
                    value=breakout_value,
                    direction=_sign(breakout_value),
                    confidence=abs(breakout_value),
                    weight=0.8,
                    description=f"Close breaks prior {window}-bar high/low Donchian channel.",
                )
            )

        regime_value = _regime_score(regime)
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_REGIME_PERMISSION_BRIDGE",
                config=config,
                category="regime",
                family="regime_bridge",
                role="risk",
                lookback=0,
                segment=segment,
                regime=regime,
                value=regime_value,
                direction=_sign(regime_value),
                confidence=abs(regime_value),
                weight=1.0,
                description="Regime protocol bridge; negative values reduce or pause signal permission.",
            )
        )

        sma20_distance = _number(features["sma_distance_20"].loc[timestamp]) if "sma_distance_20" in features else 0.0
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_EXIT_CLOSE_BELOW_SMA20",
                config=config,
                category="exit",
                family="exit_trailing",
                role="exit",
                lookback=20,
                segment=segment,
                regime=regime,
                value=sma20_distance,
                direction=-1 if sma20_distance < 0 else 1 if sma20_distance > 0 else 0,
                confidence=_clip(abs(sma20_distance) / 0.02),
                weight=0.5,
                description="Close vs SMA20 trailing/exit pressure diagnostic.",
            )
        )
        roc20 = _number(features.get("roc_20", pd.Series(0.0, index=data.index)).loc[timestamp])
        records.append(
            _record(
                timestamp,
                signal_id=f"{prefix}_EXIT_ROC20_DECAY",
                config=config,
                category="exit",
                family="exit_momentum",
                role="exit",
                lookback=20,
                segment=segment,
                regime=regime,
                value=roc20,
                direction=-1 if roc20 < 0 else 1 if roc20 > 0 else 0,
                confidence=_clip(abs(roc20) / 0.03),
                weight=0.4,
                description="20-bar ROC decay or continuation as exit pressure diagnostic.",
            )
        )

    rows = pd.DataFrame.from_records(records)
    if rows.empty:
        return pd.DataFrame(columns=SIGNAL_ROW_COLUMNS, index=data.index[:0])
    rows = rows.set_index("Timestamp")
    rows.index = pd.DatetimeIndex(rows.index).tz_convert("UTC")
    return rows[SIGNAL_ROW_COLUMNS]


def summarize_signals(rows: pd.DataFrame) -> dict[str, Any]:
    if rows.empty:
        return {
            "total_rows": 0,
            "first_timestamp": None,
            "last_timestamp": None,
            "latest": None,
            "category_counts": {},
            "family_counts": {},
            "role_counts": {},
            "duplicate_family_counts": {},
            "by_segment": {},
            "claim_boundary": CLAIM_BOUNDARY,
            "execution_boundary": EXECUTION_BOUNDARY,
        }
    by_segment: dict[str, Any] = {}
    for segment, group in rows.groupby("segment", sort=True):
        by_segment[str(segment)] = {
            "rows": int(len(group)),
            "category_counts": _counts(group["category"]),
            "family_counts": _counts(group["family"]),
            "latest": _latest_summary(group),
        }
    return {
        "total_rows": int(len(rows)),
        "first_timestamp": _timestamp(rows.index[0]),
        "last_timestamp": _timestamp(rows.index[-1]),
        "latest": _latest_summary(rows),
        "category_counts": _counts(rows["category"]),
        "family_counts": _counts(rows["family"]),
        "role_counts": _counts(rows["role"]),
        "duplicate_family_counts": _duplicate_family_counts(rows),
        "by_segment": by_segment,
        "claim_boundary": CLAIM_BOUNDARY,
        "execution_boundary": EXECUTION_BOUNDARY,
    }


def _latest_summary(rows: pd.DataFrame) -> dict[str, Any]:
    latest_ts = rows.index[-1]
    latest_rows = rows.loc[[latest_ts]] if not isinstance(rows.loc[latest_ts], pd.Series) else rows.loc[[latest_ts]]
    role_scores = _role_scores(latest_rows)
    final_score = _clip_signed(
        sum(role_scores[key] * weight for key, weight in FINAL_SCORE_WEIGHTS.items())
    )
    risk_score = role_scores["risk_score"]
    if risk_score <= -0.55:
        decision = "risk_off"
    elif final_score >= 0.35:
        decision = "long_bias"
    elif final_score <= -0.35:
        decision = "short_bias"
    else:
        decision = "hold"
    return {
        "timestamp": _timestamp(latest_ts),
        "segment": str(latest_rows["segment"].iloc[-1]),
        "regime": str(latest_rows["regime"].iloc[-1]),
        **role_scores,
        "final_score": final_score,
        "decision": decision,
        "decision_boundary": EXECUTION_BOUNDARY,
        "signal_count": int(len(latest_rows)),
        "family_count": int(latest_rows["family"].nunique()),
        "top_positive": _top_rows(latest_rows, ascending=False),
        "top_negative": _top_rows(latest_rows, ascending=True),
        "claim_boundary": CLAIM_BOUNDARY,
    }


def _role_scores(rows: pd.DataFrame) -> dict[str, float]:
    scores: dict[str, float] = {}
    for role in ("direction", "entry", "risk", "exit"):
        role_rows = rows[rows["role"] == role]
        if role_rows.empty:
            scores[f"{role}_score"] = 0.0
            continue
        family_scores = []
        for _, group in role_rows.groupby("family", sort=True):
            denominator = group["weight"].abs().sum()
            score = 0.0 if denominator == 0 else float(group["weighted_score"].sum() / denominator)
            family_scores.append(_clip_signed(score))
        scores[f"{role}_score"] = _clip_signed(sum(family_scores) / len(family_scores))
    return scores


def _top_rows(rows: pd.DataFrame, *, ascending: bool) -> list[dict[str, Any]]:
    selected = rows.sort_values("weighted_score", ascending=ascending).head(5)
    return [
        {
            "signal_id": str(row["signal_id"]),
            "category": str(row["category"]),
            "family": str(row["family"]),
            "role": str(row["role"]),
            "normalized_value": float(row["normalized_value"]),
            "weighted_score": float(row["weighted_score"]),
        }
        for _, row in selected.iterrows()
    ]


def _normalize_ohlcv(frame: pd.DataFrame) -> pd.DataFrame:
    required = ["Open", "High", "Low", "Close", "Volume"]
    missing = [column for column in required if column not in frame.columns]
    if missing:
        raise ValueError(f"missing OHLCV columns for signal report: {missing}")
    data = frame.loc[:, required].copy()
    if not isinstance(data.index, pd.DatetimeIndex):
        raise ValueError("signal report requires a DatetimeIndex")
    if data.index.tz is None:
        data.index = data.index.tz_localize("UTC")
    else:
        data.index = data.index.tz_convert("UTC")
    if not data.index.is_monotonic_increasing:
        raise ValueError("signal report requires monotonic increasing timestamps")
    return data


def _aligned_labels(
    frame: pd.DataFrame,
    labels: pd.Series | list[str] | tuple[str, ...] | None,
    *,
    default: str,
    label: str,
) -> pd.Series:
    if labels is None:
        return pd.Series([default] * len(frame), index=frame.index, dtype="object")
    if isinstance(labels, pd.Series):
        aligned = labels.reindex(frame.index)
        if aligned.isna().any():
            raise ValueError(f"{label} series must cover every signal frame timestamp")
        return aligned.astype(str)
    if len(labels) != len(frame):
        raise ValueError(f"{label} length must match signal frame length")
    return pd.Series(list(labels), index=frame.index, dtype="object")


def _record(
    timestamp: pd.Timestamp,
    *,
    signal_id: str,
    config: SignalFactoryConfig,
    category: str,
    family: str,
    role: str,
    lookback: int,
    segment: str,
    regime: str,
    value: float,
    direction: int,
    confidence: float,
    weight: float,
    description: str,
) -> dict[str, Any]:
    normalized_value = _clip_signed(direction * _clip(confidence))
    return {
        "Timestamp": timestamp,
        "signal_id": signal_id,
        "asset": config.asset,
        "timeframe": config.timeframe,
        "category": category,
        "family": family,
        "role": role,
        "lookback": int(lookback),
        "segment": segment,
        "regime": regime,
        "value": float(value),
        "direction": int(direction),
        "confidence": float(_clip(confidence)),
        "expiry_bars": int(config.expiry_bars),
        "normalized_value": float(normalized_value),
        "weight": float(weight),
        "weighted_score": float(normalized_value * weight),
        "description": description,
    }


def _rsi(close: pd.Series, window: int) -> pd.Series:
    delta = close.diff()
    gain = delta.clip(lower=0).rolling(window, min_periods=2).mean()
    loss = (-delta.clip(upper=0)).rolling(window, min_periods=2).mean()
    rs = gain / loss.where(loss > 0)
    rsi = 100 - (100 / (1 + rs))
    rsi = rsi.mask((loss == 0) & (gain > 0), 100.0)
    rsi = rsi.mask((loss == 0) & (gain == 0), 50.0)
    return rsi.fillna(50.0)


def _atr_pct(high: pd.Series, low: pd.Series, close: pd.Series, window: int) -> pd.Series:
    prev_close = close.shift(1)
    true_range = pd.concat(
        [(high - low).abs(), (high - prev_close).abs(), (low - prev_close).abs()],
        axis=1,
    ).max(axis=1)
    atr = true_range.rolling(window, min_periods=1).mean()
    return atr / _nonzero(close)


def _nonzero(series: pd.Series) -> pd.Series:
    return series.where(series != 0)


def _ma_stack_value(close_value: float, features: dict[str, pd.Series], timestamp: pd.Timestamp) -> float:
    try:
        sma20 = float(features["sma_20"].loc[timestamp])
        sma60 = float(features["sma_60"].loc[timestamp])
        sma200 = float(features["sma_200"].loc[timestamp])
        close_float = float(close_value)
    except KeyError:
        return 0.0
    if any(pd.isna(value) for value in (close_float, sma20, sma60, sma200)):
        return 0.0
    if close_float > sma20 > sma60 > sma200:
        return 1.0
    if close_float < sma20 < sma60 < sma200:
        return -1.0
    return 0.0


def _regime_score(regime: str) -> float:
    return {
        "normal_trend": 0.6,
        "high_vol_trend": 0.2,
        "chop": -0.4,
        "high_vol_chop": -0.7,
        "liquidity_stress": -1.0,
        "insufficient_data": -0.8,
        "unknown": 0.0,
    }.get(regime, 0.0)


def _counts(values: pd.Series) -> dict[str, int]:
    raw = values.value_counts(sort=False).to_dict()
    return {str(key): int(raw[key]) for key in sorted(raw)}


def _duplicate_family_counts(rows: pd.DataFrame) -> dict[str, int]:
    counts = rows.groupby(["Timestamp", "family"]).size().groupby("family").max().to_dict()
    return {str(key): int(counts[key]) for key in sorted(counts)}


def _timestamp(value: Any) -> str:
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return ts.isoformat().replace("+00:00", "Z")


def _number(value: Any, *, default: float = 0.0) -> float:
    if pd.isna(value):
        return default
    return float(value)


def _sign(value: float) -> int:
    if pd.isna(value):
        return 0
    if value > 0:
        return 1
    if value < 0:
        return -1
    return 0


def _clip(value: float) -> float:
    if pd.isna(value):
        return 0.0
    return max(0.0, min(1.0, float(value)))


def _clip_signed(value: float) -> float:
    if pd.isna(value):
        return 0.0
    return max(-1.0, min(1.0, float(value)))
