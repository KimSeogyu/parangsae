from __future__ import annotations

from dataclasses import asdict, dataclass
from math import sqrt
from typing import Any

import pandas as pd

REGIME_SCHEMA_VERSION = 1
REGIME_ROW_COLUMNS = [
    "segment",
    "close",
    "return_1h",
    "sma_fast",
    "sma_slow",
    "sma_spread_pct",
    "atr_pct",
    "realized_vol_24h",
    "realized_vol_168h",
    "vol_ratio",
    "whipsaw_24h",
    "amihud_24h",
    "illiquidity_z",
    "regime_label",
    "protocol_action",
    "risk_multiplier",
    "protocol_reason",
]
REGIME_CSV_COLUMNS = ["Timestamp", *REGIME_ROW_COLUMNS]

CLAIM_BOUNDARY = (
    "regime protocol is a causal risk/permission diagnostic for the canonical research "
    "harness, not an alpha signal, live-fill proof, or profit guarantee"
)
SCHEMA_VERSION_POLICY = (
    "schema_version 1 is frozen for the v1 report contract; breaking changes to "
    "columns, labels, or protocol semantics require a schema_version bump and an "
    "explicit migration note"
)

PAPER_AXES = [
    "standard_null_baseline",
    "markov_switching_regimes",
    "change_point_detection",
    "volatility_correlation_models",
    "trend_vs_chop",
    "liquidity_funding_stress",
    "overfit_and_multiple_testing_control",
]

PROTOCOLS = {
    "insufficient_data": {
        "protocol_action": "pause_trend",
        "risk_multiplier": 0.0,
        "protocol_reason": "not enough causal history for the regime gate",
    },
    "liquidity_stress": {
        "protocol_action": "pause_trend",
        "risk_multiplier": 0.0,
        "protocol_reason": "liquidity/volume stress overrides trend permission",
    },
    "high_vol_trend": {
        "protocol_action": "reduce_risk",
        "risk_multiplier": 0.5,
        "protocol_reason": "trend is present but volatility is elevated",
    },
    "high_vol_chop": {
        "protocol_action": "pause_trend",
        "risk_multiplier": 0.0,
        "protocol_reason": "high volatility without stable trend permission",
    },
    "normal_trend": {
        "protocol_action": "allow_trend",
        "risk_multiplier": 1.0,
        "protocol_reason": "trend permission with non-stressed volatility/liquidity",
    },
    "chop": {
        "protocol_action": "pause_trend",
        "risk_multiplier": 0.0,
        "protocol_reason": "range/chop regime; trend-following is not permitted by this gate",
    },
}


@dataclass(frozen=True)
class RegimeThresholds:
    min_history_bars: int = 96
    sma_fast_bars: int = 24
    sma_slow_bars: int = 96
    atr_bars: int = 14
    realized_vol_fast_bars: int = 24
    realized_vol_slow_bars: int = 168
    whipsaw_bars: int = 24
    amihud_bars: int = 24
    illiquidity_z_bars: int = 168
    trend_spread_pct: float = 0.005
    chop_spread_pct: float = 0.003
    high_vol_ratio: float = 1.5
    high_atr_pct: float = 0.03
    chop_whipsaw_count: int = 4
    liquidity_illiquidity_z: float = 3.0


@dataclass(frozen=True)
class RegimeReport:
    rows: pd.DataFrame
    summary: dict[str, Any]
    thresholds: RegimeThresholds = RegimeThresholds()

    def to_json_payload(self) -> dict[str, Any]:
        return {
            "schema_version": REGIME_SCHEMA_VERSION,
            "method": {
                "name": "causal_ohlcv_regime_protocol_v1",
                "causal_no_lookahead": True,
                "inputs": ["Open", "High", "Low", "Close", "Volume"],
                "rows_schema": REGIME_CSV_COLUMNS,
                "schema_version_policy": SCHEMA_VERSION_POLICY,
                "volatility_scaling": {
                    "realized_vol_24h": "rolling 24h return std scaled by sqrt(24)",
                    "realized_vol_168h": "rolling 168h return std scaled by sqrt(168)",
                    "vol_ratio": "unscaled rolling 24h return std divided by unscaled rolling 168h return std",
                },
            },
            "thresholds": asdict(self.thresholds),
            "protocols": PROTOCOLS,
            "paper_axes": PAPER_AXES,
            "claim_boundary": CLAIM_BOUNDARY,
            "summary": self.summary,
        }


def build_regime_report(
    frame: pd.DataFrame,
    *,
    segments: pd.Series | list[str] | tuple[str, ...] | None = None,
    thresholds: RegimeThresholds = RegimeThresholds(),
) -> RegimeReport:
    rows = classify_regimes(frame, segments=segments, thresholds=thresholds)
    return RegimeReport(rows=rows, summary=summarize_regimes(rows), thresholds=thresholds)


def classify_regimes(
    frame: pd.DataFrame,
    *,
    segments: pd.Series | list[str] | tuple[str, ...] | None = None,
    thresholds: RegimeThresholds = RegimeThresholds(),
) -> pd.DataFrame:
    data = _normalize_ohlcv(frame)
    segment_series = _segments_for(data, segments)
    close = data["Close"].astype(float)
    high = data["High"].astype(float)
    low = data["Low"].astype(float)
    volume = data["Volume"].astype(float)
    prev_close = close.shift(1)

    returns = close.pct_change()
    sma_fast = close.rolling(thresholds.sma_fast_bars, min_periods=1).mean()
    sma_slow = close.rolling(thresholds.sma_slow_bars, min_periods=1).mean()
    sma_spread_pct = (sma_fast - sma_slow) / close.replace(0, pd.NA)

    true_range = pd.concat(
        [(high - low).abs(), (high - prev_close).abs(), (low - prev_close).abs()],
        axis=1,
    ).max(axis=1)
    atr = true_range.rolling(thresholds.atr_bars, min_periods=1).mean()
    atr_pct = atr / close.replace(0, pd.NA)

    rolling_vol_24h = returns.rolling(thresholds.realized_vol_fast_bars, min_periods=2).std()
    rolling_vol_168h = returns.rolling(thresholds.realized_vol_slow_bars, min_periods=2).std()
    realized_vol_24h = rolling_vol_24h * sqrt(thresholds.realized_vol_fast_bars)
    realized_vol_168h = rolling_vol_168h * sqrt(thresholds.realized_vol_slow_bars)
    vol_ratio = rolling_vol_24h / rolling_vol_168h.replace(0, pd.NA)

    spread_sign = sma_spread_pct.map(_sign)
    previous_sign = spread_sign.shift(1)
    whipsaw = (
        spread_sign.ne(previous_sign)
        & spread_sign.ne(0)
        & previous_sign.ne(0)
        & previous_sign.notna()
    )
    whipsaw_24h = (
        whipsaw.astype(float).rolling(thresholds.whipsaw_bars, min_periods=1).sum()
    )

    dollar_volume = close * volume
    amihud = returns.abs() / dollar_volume.where(dollar_volume > 0)
    amihud_24h = amihud.rolling(thresholds.amihud_bars, min_periods=2).mean()
    amihud_mean = amihud_24h.rolling(thresholds.illiquidity_z_bars, min_periods=2).mean()
    amihud_std = amihud_24h.rolling(thresholds.illiquidity_z_bars, min_periods=2).std()
    illiquidity_z = (amihud_24h - amihud_mean) / amihud_std.where(amihud_std > 0)

    rows = pd.DataFrame(
        {
            "segment": segment_series,
            "close": close,
            "return_1h": returns,
            "sma_fast": sma_fast,
            "sma_slow": sma_slow,
            "sma_spread_pct": sma_spread_pct,
            "atr_pct": atr_pct,
            "realized_vol_24h": realized_vol_24h,
            "realized_vol_168h": realized_vol_168h,
            "vol_ratio": vol_ratio,
            "whipsaw_24h": whipsaw_24h,
            "amihud_24h": amihud_24h,
            "illiquidity_z": illiquidity_z,
        },
        index=data.index,
    )

    labels: list[str] = []
    for offset, (_, row) in enumerate(rows.iterrows(), start=1):
        labels.append(_label_for_row(row, volume.iloc[offset - 1], offset, thresholds))
    rows["regime_label"] = labels
    rows["protocol_action"] = [PROTOCOLS[label]["protocol_action"] for label in labels]
    rows["risk_multiplier"] = [PROTOCOLS[label]["risk_multiplier"] for label in labels]
    rows["protocol_reason"] = [PROTOCOLS[label]["protocol_reason"] for label in labels]
    return rows[REGIME_ROW_COLUMNS]


def summarize_regimes(rows: pd.DataFrame) -> dict[str, Any]:
    if rows.empty:
        return {
            "total_rows": 0,
            "first_timestamp": None,
            "last_timestamp": None,
            "latest": None,
            "label_counts": {},
            "protocol_counts": {},
            "by_segment": {},
            "claim_boundary": CLAIM_BOUNDARY,
        }
    by_segment: dict[str, Any] = {}
    for segment, group in rows.groupby("segment", sort=True):
        by_segment[str(segment)] = {
            "rows": int(len(group)),
            "label_counts": _counts(group["regime_label"]),
            "protocol_counts": _counts(group["protocol_action"]),
            "latest": _latest_row_summary(group),
        }
    return {
        "total_rows": int(len(rows)),
        "first_timestamp": _timestamp(rows.index[0]),
        "last_timestamp": _timestamp(rows.index[-1]),
        "latest": _latest_row_summary(rows),
        "label_counts": _counts(rows["regime_label"]),
        "protocol_counts": _counts(rows["protocol_action"]),
        "by_segment": by_segment,
        "claim_boundary": CLAIM_BOUNDARY,
    }


def _normalize_ohlcv(frame: pd.DataFrame) -> pd.DataFrame:
    required = ["Open", "High", "Low", "Close", "Volume"]
    missing = [column for column in required if column not in frame.columns]
    if missing:
        raise ValueError(f"missing OHLCV columns for regime report: {missing}")
    data = frame.loc[:, required].copy()
    if not isinstance(data.index, pd.DatetimeIndex):
        raise ValueError("regime report requires a DatetimeIndex")
    if data.index.tz is None:
        data.index = data.index.tz_localize("UTC")
    else:
        data.index = data.index.tz_convert("UTC")
    if not data.index.is_monotonic_increasing:
        raise ValueError("regime report requires monotonic increasing timestamps")
    return data


def _segments_for(
    frame: pd.DataFrame,
    segments: pd.Series | list[str] | tuple[str, ...] | None,
) -> pd.Series:
    if segments is None:
        return pd.Series(["research"] * len(frame), index=frame.index, dtype="object")
    if isinstance(segments, pd.Series):
        aligned = segments.reindex(frame.index)
        if aligned.isna().any():
            raise ValueError("segments series must cover every regime frame timestamp")
        return aligned.astype(str)
    if len(segments) != len(frame):
        raise ValueError("segments length must match regime frame length")
    return pd.Series(list(segments), index=frame.index, dtype="object")


def _label_for_row(
    row: pd.Series,
    volume: float,
    history_count: int,
    thresholds: RegimeThresholds,
) -> str:
    if history_count < thresholds.min_history_bars:
        return "insufficient_data"
    if volume <= 0 or _at_least(row["illiquidity_z"], thresholds.liquidity_illiquidity_z):
        return "liquidity_stress"
    high_vol = _at_least(row["vol_ratio"], thresholds.high_vol_ratio) or _at_least(
        row["atr_pct"], thresholds.high_atr_pct
    )
    trending = abs(_number(row["sma_spread_pct"])) >= thresholds.trend_spread_pct
    choppy = (
        abs(_number(row["sma_spread_pct"])) <= thresholds.chop_spread_pct
        or _number(row["whipsaw_24h"]) >= thresholds.chop_whipsaw_count
    )
    if high_vol and trending:
        return "high_vol_trend"
    if high_vol:
        return "high_vol_chop"
    if trending and not choppy:
        return "normal_trend"
    return "chop"


def _counts(values: pd.Series) -> dict[str, int]:
    raw = values.value_counts(sort=False).to_dict()
    return {str(key): int(raw[key]) for key in sorted(raw)}


def _latest_row_summary(rows: pd.DataFrame) -> dict[str, Any]:
    row = rows.iloc[-1]
    return {
        "timestamp": _timestamp(rows.index[-1]),
        "segment": str(row["segment"]),
        "regime_label": str(row["regime_label"]),
        "protocol_action": str(row["protocol_action"]),
        "risk_multiplier": float(row["risk_multiplier"]),
        "protocol_reason": str(row["protocol_reason"]),
        "sma_spread_pct": _optional_float(row["sma_spread_pct"]),
        "atr_pct": _optional_float(row["atr_pct"]),
        "vol_ratio": _optional_float(row["vol_ratio"]),
        "illiquidity_z": _optional_float(row["illiquidity_z"]),
    }


def _timestamp(value: Any) -> str:
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return ts.isoformat().replace("+00:00", "Z")


def _optional_float(value: Any) -> float | None:
    if pd.isna(value):
        return None
    return float(value)


def _number(value: Any) -> float:
    return 0.0 if pd.isna(value) else float(value)


def _at_least(value: Any, threshold: float) -> bool:
    return False if pd.isna(value) else float(value) >= threshold


def _sign(value: Any) -> int:
    if pd.isna(value):
        return 0
    number = float(value)
    if number > 0:
        return 1
    if number < 0:
        return -1
    return 0
