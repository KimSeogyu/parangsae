from __future__ import annotations

import json
import math
import random
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterable, Literal, cast

import pandas as pd

from parangsae.marketdata.binance_funding import FundingRateBundle
from parangsae.marketdata.binance_vision import MarketDataBundle
from parangsae.research.loop_artifacts import prepare_loop_output_dir
from parangsae.research.null_baselines import (
    DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT,
    deterministic_random_seed,
    inverse_signal_position,
    lagged_signal_position,
)

ALPHA_SIGNAL_FACTORY_SCHEMA_VERSION = 1
ALPHA_SIGNAL_FACTORY_LOOP_SLUG = "alpha-signal-factory-composition-harness-v1"
ALPHA_SIGNAL_FACTORY_NAMESPACE = "alpha_signal_factory_composition_harness_only_not_readiness"
ALPHA_SIGNAL_FACTORY_DECISION = "SIGNAL_FACTORY_INFRA_ONLY"
READINESS_BASELINE_VERDICT = "NOT_READY"
DEFAULT_SIGNAL_GENERATION_SEED = 20260627
DEFAULT_MAX_DEPTH = 3
DEFAULT_DECISION_LAG_BARS = 1
FORWARD_HORIZONS = (8, 24, 72)
DUPLICATE_CORRELATION_THRESHOLD = 0.95
CLAIM_BOUNDARY = (
    "ALPHA_SIGNAL_FACTORY_COMPOSITION_HARNESS_V1 is paper-only signal-generation, "
    "composition, duplicate-control, and null-comparison infrastructure. Generated "
    "signals are hypotheses, not proven edges, not a trading strategy, not trading "
    "advice, not profitability evidence, not readiness or live readiness, and not "
    "permission for live/testnet/account/API-key/order/fill use."
)
LEAKAGE_BOUNDARY = (
    "Signals are generated from public OHLCV/funding data available at or before "
    "the bar timestamp, use rolling historical windows only, and are shifted by at "
    "least one decision lag before forward-return label comparison."
)
ALLOWED_SHORTLIST_CATEGORIES = (
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEGATIVE_CONTROL_CANDIDATE",
)
FORBIDDEN_OUTPUT_LABELS = {"READY", "TRADE", "LIVE_READY", "PROFITABLE", "DEPLOY", "CAPITAL_ALLOCATE"}
NULL_BASELINES_USED = (
    "zero_signal_cash_null",
    "random_sign_signal",
    "lagged_signal",
    "inverse_signal",
    "shuffled_timestamp_signal",
    "buy_and_hold_same_window",
    "frozen_tsmom_negative_control",
)
ALPHA_FACTORY_REQUIRED_FILES = (
    "alpha_signal_factory.json",
    "signal_catalog.jsonl",
    "signal_diagnostics.csv",
    "signal_family_summary.csv",
    "signal_correlation_clusters.csv",
    "null_baseline_comparison.csv",
    "shortlist.csv",
    "failed_gate_rollup.csv",
    "report.md",
    "commands.txt",
    "changed_files.txt",
)
ALPHA_FACTORY_KNOWN_FILES = set(ALPHA_FACTORY_REQUIRED_FILES)

SignalDecision = Literal[
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEGATIVE_CONTROL_CANDIDATE",
]


@dataclass(frozen=True)
class SignalSpec:
    signal_id: str
    signal_family: str
    base_inputs: tuple[str, ...]
    operators: tuple[dict[str, Any], ...]
    lookback_bars: int
    lag_bars: int
    max_depth: int
    uses_funding: bool
    uses_volume: bool
    uses_future_data: bool
    deterministic_seed: int
    claim_boundary: str = CLAIM_BOUNDARY

    def __post_init__(self) -> None:
        if self.lag_bars < 1:
            raise ValueError("SignalSpec.lag_bars must be at least 1 to prevent same-bar label leakage")
        if self.max_depth < 1 or self.max_depth > DEFAULT_MAX_DEPTH:
            raise ValueError("SignalSpec.max_depth must be between 1 and 3")
        if len(self.operators) > self.max_depth:
            raise ValueError("SignalSpec operators exceed max_depth")
        if self.uses_future_data:
            raise ValueError("SignalSpec may not use future data")

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> SignalSpec:
        return cls(
            signal_id=str(payload["signal_id"]),
            signal_family=str(payload["signal_family"]),
            base_inputs=tuple(str(item) for item in payload["base_inputs"]),
            operators=tuple(dict(item) for item in payload["operators"]),
            lookback_bars=int(payload["lookback_bars"]),
            lag_bars=int(payload["lag_bars"]),
            max_depth=int(payload["max_depth"]),
            uses_funding=bool(payload["uses_funding"]),
            uses_volume=bool(payload["uses_volume"]),
            uses_future_data=bool(payload["uses_future_data"]),
            deterministic_seed=int(payload["deterministic_seed"]),
            claim_boundary=str(payload.get("claim_boundary", CLAIM_BOUNDARY)),
        )


@dataclass(frozen=True)
class SignalEvaluationResult:
    payload: dict[str, Any]
    catalog: list[dict[str, Any]]
    diagnostics: list[dict[str, Any]]
    family_summary: list[dict[str, Any]]
    clusters: list[dict[str, Any]]
    null_comparison: list[dict[str, Any]]
    shortlist: list[dict[str, Any]]
    failed_gates: list[dict[str, Any]]


def default_alpha_signal_factory_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{ALPHA_SIGNAL_FACTORY_LOOP_SLUG}"


def generate_signal_specs(*, seed: int = DEFAULT_SIGNAL_GENERATION_SEED, max_depth: int = DEFAULT_MAX_DEPTH) -> list[SignalSpec]:
    if max_depth < 1 or max_depth > DEFAULT_MAX_DEPTH:
        raise ValueError("max_depth must be between 1 and 3")
    specs: list[SignalSpec] = []

    def add(
        signal_id: str,
        family: str,
        base_inputs: Iterable[str],
        operators: Iterable[dict[str, Any]],
        lookback: int,
        *,
        uses_funding: bool = False,
        uses_volume: bool = False,
    ) -> None:
        operator_tuple = tuple(dict(item) for item in operators)
        depth = min(max(1, len(operator_tuple)), max_depth)
        if len(operator_tuple) > max_depth:
            return
        specs.append(
            SignalSpec(
                signal_id=signal_id,
                signal_family=family,
                base_inputs=tuple(base_inputs),
                operators=operator_tuple,
                lookback_bars=int(lookback),
                lag_bars=DEFAULT_DECISION_LAG_BARS,
                max_depth=depth,
                uses_funding=uses_funding,
                uses_volume=uses_volume,
                uses_future_data=False,
                deterministic_seed=deterministic_random_seed(signal_id, seed_base=seed),
            )
        )

    add("price_return.close_to_close_return.lag1", "price_return", ("Close",), ({"op": "delta_pct"},), 1)
    add("price_return.log_return.lag1", "price_return", ("Close",), ({"op": "log_return"},), 1)
    add("price_return.open_close_return.lag1", "price_return", ("Open", "Close"), ({"op": "open_close_return"},), 1)

    for lookback in (8, 24, 72):
        add(f"momentum.rolling_return.lb{lookback}", "momentum", ("Close",), ({"op": "rolling_return", "window": lookback},), lookback)
        add(
            f"momentum.breakout_position.lb{lookback}",
            "momentum",
            ("High", "Low", "Close"),
            ({"op": "breakout_position", "window": lookback},),
            lookback,
        )
    for lookback in (24, 72):
        add(
            f"momentum.distance_from_moving_average.lb{lookback}",
            "momentum",
            ("Close",),
            ({"op": "distance_from_moving_average", "window": lookback},),
            lookback,
        )
        add(
            f"mean_reversion.zscore_price_deviation.lb{lookback}",
            "mean_reversion",
            ("Close",),
            ({"op": "zscore", "window": lookback}, {"op": "negate"}),
            lookback,
        )
        add(
            f"mean_reversion.range_reversion_proxy.lb{lookback}",
            "mean_reversion",
            ("High", "Low", "Close"),
            ({"op": "range_reversion_proxy", "window": lookback},),
            lookback,
        )
    for lookback in (8, 24):
        add(
            f"mean_reversion.rolling_return_reversal.lb{lookback}",
            "mean_reversion",
            ("Close",),
            ({"op": "rolling_return", "window": lookback}, {"op": "negate"}),
            lookback,
        )

    for lookback in (24, 72):
        add(
            f"volatility.realized_volatility.lb{lookback}",
            "volatility",
            ("Close",),
            ({"op": "realized_volatility", "window": lookback},),
            lookback,
        )
        add(
            f"volatility.volatility_change.lb{lookback}",
            "volatility",
            ("Close",),
            ({"op": "realized_volatility", "window": lookback}, {"op": "delta"}),
            lookback,
        )
    add("volatility.range_volatility.lb24", "volatility", ("High", "Low", "Close"), ({"op": "range_volatility", "window": 24},), 24)
    add(
        "volatility.volatility_zscore.lb72",
        "volatility",
        ("Close",),
        ({"op": "realized_volatility", "window": 24}, {"op": "zscore", "window": 72}),
        72,
    )

    for lookback in (24, 72):
        add(f"volume.volume_zscore.lb{lookback}", "volume_liquidity", ("Volume",), ({"op": "zscore", "window": lookback},), lookback, uses_volume=True)
    add(
        "volume.dollar_volume_proxy.lb24",
        "volume_liquidity",
        ("Close", "Volume"),
        ({"op": "dollar_volume_proxy"}, {"op": "zscore", "window": 24}),
        24,
        uses_volume=True,
    )
    add(
        "volume.volume_return_interaction.lb24",
        "volume_liquidity",
        ("Close", "Volume"),
        ({"op": "volume_return_interaction", "window": 24},),
        24,
        uses_volume=True,
    )

    funding_ops = {
        "funding_rate": ({"op": "funding_rate"},),
        "funding_zscore": ({"op": "funding_rate"}, {"op": "zscore", "window": 24}),
        "funding_change": ({"op": "funding_rate"}, {"op": "delta"}),
        "funding_extreme_flag": ({"op": "funding_extreme_flag", "window": 72},),
        "funding_sign": ({"op": "funding_rate"}, {"op": "sign"}),
        "funding_abs_level": ({"op": "funding_rate"}, {"op": "abs"}),
    }
    for name, operators in funding_ops.items():
        add(f"funding.{name}.lb72", "funding", ("funding_rate",), operators, 72, uses_funding=True)

    add(
        "interaction.momentum_gated_by_volatility.lb24",
        "interaction",
        ("Close",),
        ({"op": "rolling_return", "window": 24}, {"op": "gate_low_volatility", "window": 24}),
        24,
    )
    add(
        "interaction.mean_reversion_gated_by_low_volatility.lb24",
        "interaction",
        ("High", "Low", "Close"),
        ({"op": "range_reversion_proxy", "window": 24}, {"op": "gate_low_volatility", "window": 24}),
        24,
    )
    add(
        "interaction.funding_extreme_gated_by_realized_volatility.lb72",
        "interaction",
        ("funding_rate", "Close"),
        ({"op": "funding_extreme_flag", "window": 72}, {"op": "gate_high_volatility", "window": 24}),
        72,
        uses_funding=True,
    )
    add(
        "interaction.volume_shock_gated_by_return_direction.lb24",
        "interaction",
        ("Close", "Volume"),
        ({"op": "volume_zscore", "window": 24}, {"op": "gate_return_direction"}),
        24,
        uses_volume=True,
    )
    add(
        "interaction.funding_negative_momentum.lb24",
        "interaction",
        ("funding_rate", "Close"),
        ({"op": "rolling_return", "window": 24}, {"op": "gate_negative_funding"}),
        24,
        uses_funding=True,
    )
    return specs


def evaluate_alpha_signal_factory(
    *,
    market_bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None = None,
    seed: int = DEFAULT_SIGNAL_GENERATION_SEED,
    max_depth: int = DEFAULT_MAX_DEPTH,
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> SignalEvaluationResult:
    frame = _normalize_ohlcv(market_bundle.frame)
    funding = _align_public_funding(frame.index, funding_bundle.frame if funding_bundle is not None else None)
    specs = generate_signal_specs(seed=seed, max_depth=max_depth)
    signals: dict[str, pd.Series] = {}
    diagnostics: list[dict[str, Any]] = []
    label_returns = _forward_return_labels(frame, horizons=FORWARD_HORIZONS)
    regime_labels = _regime_labels(frame, funding)
    for spec in specs:
        raw_signal = build_signal_series(spec, frame=frame, funding=funding)
        decision_signal = lag_signal_for_labels(raw_signal, lag_bars=spec.lag_bars)
        signals[spec.signal_id] = decision_signal
        diagnostics.append(
            _diagnostics_for_signal(
                spec,
                raw_signal=raw_signal,
                decision_signal=decision_signal,
                forward_returns=label_returns,
                regime_labels=regime_labels,
            )
        )
    clusters = duplicate_correlation_clusters(signals)
    cluster_by_signal = {row["signal_id"]: row for row in clusters}
    for row in diagnostics:
        cluster = cluster_by_signal.get(str(row["signal_id"]), {})
        row["duplicate_cluster_id"] = cluster.get("duplicate_cluster_id")
        row["max_abs_correlation_to_existing_signal"] = cluster.get("max_abs_correlation_to_existing_signal")
        row["is_redundant_duplicate"] = cluster.get("is_redundant_duplicate", False)
    null_rows = _null_comparison_rows(signals=signals, specs=specs, forward_returns=label_returns, seed=seed)
    family_summary = _family_summary_rows(diagnostics)
    shortlist = _shortlist_rows(family_summary)
    failed_gates = _failed_gate_rows(diagnostics=diagnostics, family_summary=family_summary, clusters=clusters)
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    killed_count = sum(1 for row in shortlist if row["shortlist_category"] == "KILL_SIGNAL")
    watch_count = sum(1 for row in shortlist if str(row["shortlist_category"]).startswith("WATCH"))
    effective_breadth = len({row["duplicate_cluster_id"] for row in clusters if not bool(row.get("is_redundant_duplicate"))})
    payload = {
        "schema_version": ALPHA_SIGNAL_FACTORY_SCHEMA_VERSION,
        "loop_id": f"{datetime.now(UTC).strftime('%Y%m%d')}-{ALPHA_SIGNAL_FACTORY_LOOP_SLUG}",
        "loop_namespace": ALPHA_SIGNAL_FACTORY_NAMESPACE,
        "loop_type": "validation_infrastructure",
        "status": "IMPLEMENTED",
        "suite_decision": ALPHA_SIGNAL_FACTORY_DECISION,
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "signal_generation_seed": int(seed),
        "max_depth": int(max_depth),
        "generated_signal_count": len(specs),
        "evaluated_signal_count": len(diagnostics),
        "killed_signal_count": int(killed_count),
        "duplicate_signal_count": int(duplicate_count),
        "watch_family_count": int(watch_count),
        "effective_signal_breadth": int(effective_breadth),
        "families_evaluated": sorted({spec.signal_family for spec in specs}),
        "horizons_evaluated": list(FORWARD_HORIZONS),
        "null_baselines_used": list(NULL_BASELINES_USED),
        "leakage_boundary": LEAKAGE_BOUNDARY,
        "claim_boundary": CLAIM_BOUNDARY,
        "safety_gates": {
            "no_live_orders_submitted": True,
            "no_testnet_orders_submitted": True,
            "no_account_data_used": True,
            "no_api_key_used": True,
            "no_private_order_or_fill_data_used": True,
            "live_orders_submitted": False,
            "testnet_orders_submitted": False,
            "api_keys_required_for_research": False,
            "account_or_fill_data_used": False,
            "profit_or_live_ready_claim": False,
            "live_trading_path_introduced": False,
        },
        "tsmom_state_changed": False,
        "holdout_retune_used": False,
        "forbidden_outputs_absent": True,
        "selection_boundary": {
            "shortlist_is_strategy": False,
            "shortlist_is_readiness": False,
            "holdout_used_for_selection": False,
            "weight_optimization_performed": False,
            "threshold_or_horizon_retune_performed": False,
        },
        "dataset_provenance": _dataset_provenance(market_bundle, funding_bundle),
        "summary": {
            "signal_count": len(specs),
            "diagnostic_metric_only": True,
            "strategy_pnl_optimization": False,
            "shortlist_categories": list(ALLOWED_SHORTLIST_CATEGORIES),
            "forbidden_outputs": sorted(FORBIDDEN_OUTPUT_LABELS),
        },
        "validation_commands": list(commands),
        "changed_files": list(changed_files),
        "evidence_artifacts": list(ALPHA_FACTORY_REQUIRED_FILES),
        "next_loop": "Use WATCH_* families only as candidates for a separate pre-registered paper-only falsification loop.",
    }
    return SignalEvaluationResult(
        payload=payload,
        catalog=[spec.to_dict() for spec in specs],
        diagnostics=diagnostics,
        family_summary=family_summary,
        clusters=clusters,
        null_comparison=null_rows,
        shortlist=shortlist,
        failed_gates=failed_gates,
    )


def build_signal_series(spec: SignalSpec, *, frame: pd.DataFrame, funding: pd.Series | None = None) -> pd.Series:
    if spec.uses_future_data:
        raise ValueError(f"signal {spec.signal_id} uses future data")
    data = _normalize_ohlcv(frame)
    close = _series(data["Close"]).astype(float)
    current = pd.Series(0.0, index=data.index, dtype=float)
    for operator in spec.operators:
        op = str(operator["op"])
        window = int(operator.get("window", spec.lookback_bars))
        if op == "delta_pct":
            current = close.pct_change()
        elif op == "log_return":
            current = _safe_log(close / close.shift(1))
        elif op == "open_close_return":
            current = _series(data["Close"]).astype(float) / _series(data["Open"]).astype(float) - 1.0
        elif op == "rolling_return":
            current = close / close.shift(window) - 1.0
        elif op == "breakout_position":
            high = _series(_series(data["High"]).astype(float).rolling(window, min_periods=window).max())
            low = _series(_series(data["Low"]).astype(float).rolling(window, min_periods=window).min())
            current = _series(_safe_ratio(close - low, _series(high - low)) * 2.0 - 1.0)
        elif op == "distance_from_moving_average":
            mean = _series(close.rolling(window, min_periods=window).mean())
            current = _safe_ratio(_series(close - mean), mean)
        elif op == "zscore":
            current = rolling_zscore(_series(current if not bool(_series(current).eq(0.0).all()) else close), window=window)
        elif op == "range_reversion_proxy":
            high = _series(_series(data["High"]).astype(float).rolling(window, min_periods=window).max())
            low = _series(_series(data["Low"]).astype(float).rolling(window, min_periods=window).min())
            current = _series(-(_safe_ratio(_series(close - low), _series(high - low)) * 2.0 - 1.0))
        elif op == "realized_volatility":
            current = _series(close.pct_change().rolling(window, min_periods=window).std())
        elif op == "range_volatility":
            current = _series(_safe_ratio(_series(_series(data["High"]).astype(float) - _series(data["Low"]).astype(float)), close).rolling(
                window,
                min_periods=window,
            ).mean())
        elif op == "volume_zscore":
            current = rolling_zscore(_series(data["Volume"]).astype(float), window=window)
        elif op == "dollar_volume_proxy":
            current = close * _series(data["Volume"]).astype(float)
        elif op == "volume_return_interaction":
            volume_z = rolling_zscore(_series(data["Volume"]).astype(float), window=window)
            current = _series(volume_z * _series(close.pct_change().apply(_safe_sign)))
        elif op == "funding_rate":
            current = _funding_or_zero(data.index, funding)
        elif op == "funding_extreme_flag":
            funding_series = _funding_or_zero(data.index, funding)
            z = rolling_zscore(funding_series, window=window)
            current = _series(_series(z.abs() >= 1.5).astype(float) * _series(z.apply(_safe_sign)))
        elif op == "delta":
            current = _series(current).diff()
        elif op == "sign":
            current = _series(current).apply(_safe_sign)
        elif op == "abs":
            current = _series(current).abs()
        elif op == "negate":
            current = -current
        elif op == "delay":
            current = _series(current).shift(int(operator.get("bars", 1)))
        elif op == "rolling_mean":
            current = _series(current).rolling(window, min_periods=window).mean()
        elif op == "rolling_std":
            current = _series(current).rolling(window, min_periods=window).std()
        elif op == "rank":
            current = rolling_rank_last(current, window=window)
        elif op == "winsorize":
            limit = float(operator.get("limit", 3.0))
            current = _series(current).clip(lower=-limit, upper=limit)
        elif op == "decay_linear":
            current = decay_linear(current, window=window)
        elif op == "ewma":
            current = _series(current).ewm(span=window, adjust=False, min_periods=window).mean()
        elif op == "volatility_scale":
            vol = _series(close.pct_change().rolling(window, min_periods=window).std())
            current = _safe_ratio(_series(current), vol)
        elif op == "clip":
            current = _series(current).clip(lower=float(operator.get("lower", -1.0)), upper=float(operator.get("upper", 1.0)))
        elif op == "gate_low_volatility":
            current = current * _volatility_gate(close, window=window, low=True)
        elif op == "gate_high_volatility":
            current = current * _volatility_gate(close, window=window, low=False)
        elif op == "gate_positive_funding":
            current = current * (_funding_or_zero(data.index, funding) > 0).astype(float)
        elif op == "gate_negative_funding":
            current = current * (_funding_or_zero(data.index, funding) < 0).astype(float)
        elif op == "gate_extreme_funding":
            current = current * (rolling_zscore(_funding_or_zero(data.index, funding), window=72).abs() >= 1.5).astype(float)
        elif op == "gate_volume_spike":
            current = current * (rolling_zscore(_series(data["Volume"]).astype(float), window=window).abs() >= 1.5).astype(float)
        elif op == "gate_return_direction":
            current = _series(current) * _series(close.pct_change().apply(_safe_sign))
        elif op == "gate_sideways_range":
            current = current * _sideways_gate(close, window=window)
        else:
            raise ValueError(f"unsupported signal operator: {op}")
    return _series(_series(current).replace([math.inf, -math.inf], pd.NA)).astype(float)


def apply_unary_operator(series: pd.Series, operator: str, *, lookback_bars: int) -> pd.Series:
    if lookback_bars <= 0:
        raise ValueError("lookback_bars must be positive; full-sample normalization is forbidden")
    data = _series(series).astype(float)
    if operator == "zscore":
        return rolling_zscore(data, window=lookback_bars)
    if operator == "delay":
        return _series(data.shift(lookback_bars))
    if operator == "delta":
        return _series(data.diff())
    if operator == "rolling_mean":
        return _series(data.rolling(lookback_bars, min_periods=lookback_bars).mean())
    if operator == "rolling_std":
        return _series(data.rolling(lookback_bars, min_periods=lookback_bars).std())
    if operator == "rank":
        return rolling_rank_last(data, window=lookback_bars)
    if operator == "winsorize":
        return _series(data.clip(lower=-3.0, upper=3.0))
    if operator == "sign":
        return _series(data.apply(_safe_sign))
    if operator == "abs":
        return data.abs()
    if operator == "negate":
        return -data
    if operator == "decay_linear":
        return decay_linear(data, window=lookback_bars)
    if operator == "ewma":
        return _series(data.ewm(span=lookback_bars, adjust=False, min_periods=lookback_bars).mean())
    if operator == "clip":
        return _series(data.clip(lower=-1.0, upper=1.0))
    raise ValueError(f"unsupported unary operator: {operator}")


def apply_binary_operator(left: pd.Series, right: pd.Series, operator: str, *, lookback_bars: int = 24) -> pd.Series:
    lhs = _series(left).astype(float)
    rhs = _series(right).astype(float).reindex(lhs.index)
    if operator == "add":
        return lhs + rhs
    if operator == "subtract":
        return lhs - rhs
    if operator == "multiply" or operator == "interaction":
        return lhs * rhs
    if operator == "ratio_safe":
        return _safe_ratio(lhs, rhs)
    if operator == "rolling_corr":
        return _series(lhs.rolling(lookback_bars, min_periods=lookback_bars).corr(rhs))
    if operator == "spread":
        return lhs - rhs
    if operator == "min":
        return _series(pd.concat([lhs, rhs], axis=1).min(axis=1))
    if operator == "max":
        return _series(pd.concat([lhs, rhs], axis=1).max(axis=1))
    raise ValueError(f"unsupported binary operator: {operator}")


def rolling_zscore(series: pd.Series, *, window: int) -> pd.Series:
    if window <= 1:
        raise ValueError("rolling zscore requires window > 1; full-sample normalization is forbidden")
    data = _series(series).astype(float)
    mean = _series(data.rolling(window, min_periods=window).mean())
    std = _series(data.rolling(window, min_periods=window).std())
    return _safe_ratio(_series(data - mean), std)


def rolling_rank_last(series: pd.Series, *, window: int) -> pd.Series:
    if window <= 1:
        raise ValueError("rolling rank requires window > 1")
    data = _series(series).astype(float)
    return _series(data.rolling(window, min_periods=window).apply(_last_value_rank, raw=False))


def decay_linear(series: pd.Series, *, window: int) -> pd.Series:
    if window <= 1:
        raise ValueError("linear decay requires window > 1")
    data = _series(series).astype(float)
    weights = [float(item + 1) for item in range(window)]
    weight_sum = sum(weights)

    def apply(values: pd.Series) -> float:
        if len(values) != window:
            return math.nan
        return float(sum(float(v) * w for v, w in zip(values, weights, strict=True)) / weight_sum)

    return _series(data.rolling(window, min_periods=window).apply(apply, raw=False))


def lag_signal_for_labels(signal: pd.Series, *, lag_bars: int = DEFAULT_DECISION_LAG_BARS) -> pd.Series:
    if lag_bars < 1:
        raise ValueError("lag_bars must be at least 1 to prevent same-bar label leakage")
    return _series(_series(signal).shift(lag_bars))


def deterministic_random_sign_signal(signal: pd.Series, evidence_key: str, *, seed_base: int = DEFAULT_SIGNAL_GENERATION_SEED) -> tuple[pd.Series, int]:
    seed = deterministic_random_seed(evidence_key, seed_base=seed_base)
    rng = random.Random(seed)
    base = _series(signal).astype(float).abs()
    signs = pd.Series([1.0 if rng.random() >= 0.5 else -1.0 for _ in range(len(base))], index=base.index, dtype=float)
    return base * signs, seed


def shuffled_timestamp_signal(signal: pd.Series, evidence_key: str, *, seed_base: int = DEFAULT_SIGNAL_GENERATION_SEED) -> tuple[pd.Series, int]:
    seed = deterministic_random_seed(f"shuffle-{evidence_key}", seed_base=seed_base)
    rng = random.Random(seed)
    values = list(_series(signal).astype(float).fillna(0.0))
    rng.shuffle(values)
    return pd.Series(values, index=signal.index, dtype=float), seed


def duplicate_correlation_clusters(signals: dict[str, pd.Series], *, threshold: float = DUPLICATE_CORRELATION_THRESHOLD) -> list[dict[str, Any]]:
    signal_ids = list(signals)
    clusters: list[set[str]] = []
    max_corr: dict[str, float] = {signal_id: 0.0 for signal_id in signal_ids}
    for signal_id in signal_ids:
        assigned = False
        current = _series(signals[signal_id]).astype(float)
        for cluster in clusters:
            representative = next(iter(cluster))
            corr = _abs_corr(current, _series(signals[representative]).astype(float))
            max_corr[signal_id] = max(max_corr[signal_id], corr)
            max_corr[representative] = max(max_corr[representative], corr)
            if corr >= threshold:
                cluster.add(signal_id)
                assigned = True
                break
        if not assigned:
            clusters.append({signal_id})
    rows: list[dict[str, Any]] = []
    cluster_id_by_signal: dict[str, str] = {}
    for idx, cluster in enumerate(clusters, start=1):
        cluster_id = f"C{idx:03d}"
        for signal_id in cluster:
            cluster_id_by_signal[signal_id] = cluster_id
    for signal_id in signal_ids:
        cluster_id = cluster_id_by_signal[signal_id]
        cluster_members = sorted(item for item, cid in cluster_id_by_signal.items() if cid == cluster_id)
        rows.append(
            {
                "signal_id": signal_id,
                "duplicate_cluster_id": cluster_id,
                "cluster_size": len(cluster_members),
                "cluster_members": ";".join(cluster_members),
                "is_redundant_duplicate": signal_id != cluster_members[0],
                "max_abs_correlation_to_existing_signal": max_corr[signal_id],
                "duplicate_threshold": threshold,
            }
        )
    return rows


def write_alpha_signal_factory_artifacts(
    output_dir: str | Path,
    result: SignalEvaluationResult,
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_alpha_signal_factory_output_dir(output_dir, overwrite=overwrite)
    payload = dict(result.payload)
    if out.name.endswith(ALPHA_SIGNAL_FACTORY_LOOP_SLUG):
        payload["loop_id"] = out.name
    _write_json(out / "alpha_signal_factory.json", payload)
    _atomic_write_text(
        out / "signal_catalog.jsonl",
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in result.catalog),
    )
    pd.DataFrame(result.diagnostics).to_csv(out / "signal_diagnostics.csv", index=False)
    pd.DataFrame(result.family_summary).to_csv(out / "signal_family_summary.csv", index=False)
    pd.DataFrame(result.clusters).to_csv(out / "signal_correlation_clusters.csv", index=False)
    pd.DataFrame(result.null_comparison).to_csv(out / "null_baseline_comparison.csv", index=False)
    pd.DataFrame(result.shortlist).to_csv(out / "shortlist.csv", index=False)
    pd.DataFrame(result.failed_gates).to_csv(out / "failed_gate_rollup.csv", index=False)
    _atomic_write_text(out / "report.md", render_alpha_signal_factory_report(payload, result))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    return out


def prepare_alpha_signal_factory_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="alpha-signal-factory-composition-run",
        known_files=ALPHA_FACTORY_KNOWN_FILES,
        overwrite=overwrite,
    )


def render_alpha_signal_factory_report(payload: dict[str, Any], result: SignalEvaluationResult) -> str:
    top_shortlist = [row for row in result.shortlist if str(row["shortlist_category"]).startswith("WATCH")]
    duplicate_rows = [row for row in result.clusters if bool(row.get("is_redundant_duplicate"))]
    failed = result.failed_gates[:20]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Purpose",
        "Build a deterministic paper-only Alpha Signal Factory composition harness that generates, composes, diagnoses, duplicate-controls, and null-compares signal hypotheses.",
        "",
        "## Claim boundary",
        f"- suite_decision: {payload['suite_decision']}",
        f"- loop_namespace: {payload['loop_namespace']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- claim_boundary: {payload['claim_boundary']}",
        "- Shortlists are paper-only research queues; they are not strategies, order intents, readiness, or profitability claims.",
        "",
        "## Safety boundary",
        f"- no_live_orders_submitted: {payload['safety_gates']['no_live_orders_submitted']}",
        f"- no_testnet_orders_submitted: {payload['safety_gates']['no_testnet_orders_submitted']}",
        f"- no_account_data_used: {payload['safety_gates']['no_account_data_used']}",
        f"- no_api_key_used: {payload['safety_gates']['no_api_key_used']}",
        f"- no_private_order_or_fill_data_used: {payload['safety_gates']['no_private_order_or_fill_data_used']}",
        f"- holdout_retune_used: {payload['holdout_retune_used']}",
        f"- tsmom_state_changed: {payload['tsmom_state_changed']}",
        "",
        "## Signal generation",
        f"- signal_generation_seed: {payload['signal_generation_seed']}",
        f"- max_depth: {payload['max_depth']}",
        f"- generated_signal_count: {payload['generated_signal_count']}",
        f"- evaluated_signal_count: {payload['evaluated_signal_count']}",
        f"- families_evaluated: {', '.join(payload['families_evaluated'])}",
        f"- horizons_evaluated: {', '.join(str(item) for item in payload['horizons_evaluated'])}",
        "",
        "## Duplicate and null controls",
        f"- duplicate_signal_count: {payload['duplicate_signal_count']}",
        f"- effective_signal_breadth: {payload['effective_signal_breadth']}",
        f"- null_baselines_used: {', '.join(payload['null_baselines_used'])}",
        f"- redundant duplicate rows: {len(duplicate_rows)}",
        "",
        "## Shortlist",
        f"- watch_family_count: {payload['watch_family_count']}",
        f"- killed_signal_count: {payload['killed_signal_count']}",
    ]
    for row in top_shortlist[:10]:
        lines.append(
            f"- {row['signal_family']}: {row['shortlist_category']} ({row['rationale']})"
        )
    lines.extend(
        [
            "",
            "## Failed gates / unclaimed checks",
        ]
    )
    if failed:
        for row in failed:
            lines.append(f"- {row['gate']}: {row['status']} ({row['reason']})")
    else:
        lines.append("- none")
    lines.extend(
        [
            "",
            "## What cannot be claimed",
            "- live readiness",
            "- profitability",
            "- execution readiness",
            "- permission to use real money",
            "- restoration of TSMOM active development",
            "- that any generated signal is a durable edge",
            "",
            "## Next loop",
            payload["next_loop"],
            "",
        ]
    )
    return "\n".join(lines)


def _diagnostics_for_signal(
    spec: SignalSpec,
    *,
    raw_signal: pd.Series,
    decision_signal: pd.Series,
    forward_returns: dict[int, pd.Series],
    regime_labels: pd.Series,
) -> dict[str, Any]:
    signal = _series(decision_signal).astype(float)
    non_null = signal.dropna()
    row: dict[str, Any] = {
        "signal_id": spec.signal_id,
        "signal_family": spec.signal_family,
        "signal_count": 1,
        "non_null_observation_count": int(len(non_null)),
        "coverage_ratio": _safe_div(len(non_null), len(signal)),
        "mean_signal": _finite_or_none(non_null.mean()) if not non_null.empty else None,
        "std_signal": _finite_or_none(non_null.std()) if len(non_null) > 1 else None,
        "signal_autocorrelation": _autocorr_or_none(non_null),
        "turnover_proxy": _finite_or_none(signal.diff().abs().mean()),
        "missing_ratio": 1.0 - _safe_div(len(non_null), len(signal)),
        "duplicate_cluster_id": None,
        "max_abs_correlation_to_existing_signal": None,
        "forward_return_ic_8h": None,
        "forward_return_ic_24h": None,
        "forward_return_ic_72h": None,
        "rank_ic_8h": None,
        "rank_ic_24h": None,
        "rank_ic_72h": None,
        "hit_rate_by_signal_sign": None,
        "decay_profile": "{}",
        "regime_breakdown": "{}",
        "null_comparison_status": "evaluated_by_family_rows",
        "leakage_check_status": "passed_shifted_by_decision_lag",
        "lag_bars": spec.lag_bars,
        "uses_future_data": spec.uses_future_data,
        "claim_boundary": spec.claim_boundary,
    }
    decay: dict[str, float | None] = {}
    for horizon, labels in forward_returns.items():
        ic = _series_corr(signal, labels)
        rank_ic = _series_corr(signal.rank(), labels.rank())
        row[f"forward_return_ic_{horizon}h"] = ic
        row[f"rank_ic_{horizon}h"] = rank_ic
        decay[str(horizon)] = ic
    primary_labels = forward_returns[24]
    aligned = pd.concat([signal.rename("signal"), primary_labels.rename("forward")], axis=1).dropna()
    if not aligned.empty:
        signed = _series(aligned["signal"]).apply(_safe_sign) * _series(aligned["forward"]).apply(_safe_sign)
        row["hit_rate_by_signal_sign"] = _finite_or_none((signed > 0).mean())
    row["decay_profile"] = json.dumps(decay, sort_keys=True)
    row["regime_breakdown"] = json.dumps(_regime_breakdown(signal, primary_labels, regime_labels), sort_keys=True)
    if spec.uses_future_data or spec.lag_bars < 1:
        row["leakage_check_status"] = "failed"
    if raw_signal.equals(decision_signal):
        row["leakage_check_status"] = "failed_unlagged_signal"
    return row


def _null_comparison_rows(
    *,
    signals: dict[str, pd.Series],
    specs: list[SignalSpec],
    forward_returns: dict[int, pd.Series],
    seed: int,
) -> list[dict[str, Any]]:
    specs_by_family: dict[str, list[SignalSpec]] = {}
    for spec in specs:
        specs_by_family.setdefault(spec.signal_family, []).append(spec)
    rows: list[dict[str, Any]] = []
    for family, family_specs in sorted(specs_by_family.items()):
        family_signal = _mean_signal([signals[spec.signal_id] for spec in family_specs])
        for horizon, labels in forward_returns.items():
            signal_ic = _series_corr(family_signal, labels)
            baseline_candidates: list[tuple[str, pd.Series | None, str, int | None, bool]] = []
            zero = pd.Series(0.0, index=family_signal.index, dtype=float)
            random_signal, random_seed = deterministic_random_sign_signal(family_signal, f"{family}-{horizon}", seed_base=seed)
            shuffled, shuffled_seed = shuffled_timestamp_signal(family_signal, f"{family}-{horizon}", seed_base=seed)
            baseline_candidates.extend(
                [
                    ("zero_signal_cash_null", zero, "available", None, False),
                    ("random_sign_signal", random_signal, "available", random_seed, False),
                    ("lagged_signal", lagged_signal_position(family_signal, lag_bars=max(2, DEFAULT_DECISION_LAG_BARS + horizon // 8)), "available", None, False),
                    ("inverse_signal", inverse_signal_position(family_signal), "available", None, False),
                    ("shuffled_timestamp_signal", shuffled, "available", shuffled_seed, False),
                    ("buy_and_hold_same_window", None, "unavailable_constant_position_no_signal_ic", None, False),
                    (
                        "frozen_tsmom_negative_control",
                        None,
                        "available_summary_only" if DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT.exists() else "unavailable_missing_artifact",
                        None,
                        False,
                    ),
                ]
            )
            for baseline_name, baseline_signal, status, deterministic_seed, uses_current_bar in baseline_candidates:
                baseline_ic = _series_corr(baseline_signal, labels) if baseline_signal is not None else None
                rows.append(
                    {
                        "signal_family": family,
                        "baseline_name": baseline_name,
                        "horizon_hours": horizon,
                        "signal_ic": signal_ic,
                        "baseline_ic": baseline_ic,
                        "signal_beats_baseline": _beats_baseline(signal_ic, baseline_ic),
                        "baseline_status": status,
                        "deterministic_seed": deterministic_seed,
                        "baseline_uses_current_bar_signal": uses_current_bar,
                        "sample_size": int(len(pd.concat([family_signal, labels], axis=1).dropna())),
                        "holdout_retune_used": False,
                        "forward_return_label_overlap": False,
                        "claim_boundary": CLAIM_BOUNDARY,
                    }
                )
    return rows


def _family_summary_rows(diagnostics: list[dict[str, Any]]) -> list[dict[str, Any]]:
    frame = pd.DataFrame(diagnostics)
    rows: list[dict[str, Any]] = []
    for family, group in frame.groupby("signal_family", sort=True):
        group_frame = _frame(group)
        duplicate_count = int(_series(group_frame.get("is_redundant_duplicate", pd.Series(False, index=group_frame.index))).fillna(False).astype(bool).sum())
        rows.append(
            {
                "signal_family": str(family),
                "signal_count": int(len(group_frame)),
                "non_null_observation_count_median": _finite_or_none(_series(group_frame["non_null_observation_count"]).median()),
                "coverage_ratio_median": _finite_or_none(_series(group_frame["coverage_ratio"]).median()),
                "duplicate_signal_count": duplicate_count,
                "effective_signal_breadth": int(len(group_frame) - duplicate_count),
                "max_abs_ic_8h": _max_abs_column(group_frame, "forward_return_ic_8h"),
                "max_abs_ic_24h": _max_abs_column(group_frame, "forward_return_ic_24h"),
                "max_abs_ic_72h": _max_abs_column(group_frame, "forward_return_ic_72h"),
                "leakage_fail_count": int((_series(group_frame["leakage_check_status"]) != "passed_shifted_by_decision_lag").sum()),
                "null_comparison_status": "evaluated",
                "claim_boundary": CLAIM_BOUNDARY,
            }
        )
    return rows


def _shortlist_rows(family_summary: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for row in family_summary:
        signal_count = int(row["signal_count"])
        duplicate_count = int(row["duplicate_signal_count"])
        max_ic = max(
            abs(float(row[item] or 0.0)) for item in ("max_abs_ic_8h", "max_abs_ic_24h", "max_abs_ic_72h")
        )
        if int(row["leakage_fail_count"]) > 0:
            category: SignalDecision = "KILL_SIGNAL"
            rationale = "leakage check failed"
        elif duplicate_count >= signal_count and signal_count > 0:
            category = "DUPLICATE_SIGNAL"
            rationale = "all variants are duplicate-correlated"
        elif duplicate_count > signal_count / 2:
            category = "DUPLICATE_SIGNAL"
            rationale = "duplicates dominate family breadth"
        elif max_ic >= 0.03:
            category = "WATCH_DIAGNOSTIC_ONLY"
            rationale = "diagnostic IC survives as a paper-only hypothesis for another falsification loop"
        else:
            category = "KILL_SIGNAL"
            rationale = "weak diagnostic IC and no null-robust edge claim"
        rows.append(
            {
                "signal_family": row["signal_family"],
                "shortlist_category": category,
                "max_abs_ic": max_ic,
                "effective_signal_breadth": row["effective_signal_breadth"],
                "rationale": rationale,
                "is_strategy": False,
                "is_readiness": False,
                "claim_boundary": CLAIM_BOUNDARY,
            }
        )
    for forbidden in FORBIDDEN_OUTPUT_LABELS:
        if any(item["shortlist_category"] == forbidden for item in rows):
            raise ValueError(f"forbidden shortlist output emitted: {forbidden}")
    return rows


def _failed_gate_rows(
    *,
    diagnostics: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    leakage_fail = [row for row in diagnostics if row["leakage_check_status"] != "passed_shifted_by_decision_lag"]
    if leakage_fail:
        rows.append({"gate": "leakage_check_status", "status": "failed", "reason": f"{len(leakage_fail)} signals failed leakage checks"})
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    if duplicate_count:
        rows.append({"gate": "duplicate_correlation_control", "status": "watch", "reason": f"{duplicate_count} redundant signal variants marked"})
    no_watch = all(row["max_abs_ic_24h"] in (None, 0.0) for row in family_summary)
    if no_watch:
        rows.append({"gate": "null_robustness", "status": "unclaimed", "reason": "no family has nonzero 24h diagnostic IC"})
    rows.append({"gate": "readiness_boundary", "status": "passed", "reason": "readiness_baseline_verdict remains NOT_READY"})
    rows.append({"gate": "tsmom_state_unchanged", "status": "passed", "reason": "TSMOM artifacts are read-only negative-control references only"})
    return rows


def _normalize_ohlcv(frame: pd.DataFrame) -> pd.DataFrame:
    data = frame.copy()
    if not isinstance(data.index, pd.DatetimeIndex):
        raise ValueError("market frame index must be a DatetimeIndex")
    if data.index.tz is None:
        data.index = data.index.tz_localize("UTC")
    else:
        data.index = data.index.tz_convert("UTC")
    missing = [column for column in ("Open", "High", "Low", "Close", "Volume") if column not in data.columns]
    if missing:
        raise ValueError(f"market frame missing OHLCV columns: {missing}")
    return _frame(data.sort_index().loc[:, ["Open", "High", "Low", "Close", "Volume"]].astype(float))


def _align_public_funding(index: pd.Index, funding_frame: pd.DataFrame | None) -> pd.Series | None:
    if funding_frame is None or funding_frame.empty:
        return None
    funding = funding_frame.copy()
    if not isinstance(funding.index, pd.DatetimeIndex):
        raise ValueError("funding frame index must be a DatetimeIndex")
    if funding.index.tz is None:
        funding.index = funding.index.tz_localize("UTC")
    else:
        funding.index = funding.index.tz_convert("UTC")
    rates = _series(pd.to_numeric(funding["funding_rate"], errors="coerce")).sort_index()
    aligned = rates.reindex(pd.DatetimeIndex(index), method="ffill").fillna(0.0)
    return _series(aligned.shift(1).fillna(0.0))


def _forward_return_labels(frame: pd.DataFrame, *, horizons: Iterable[int]) -> dict[int, pd.Series]:
    close = _series(frame["Close"]).astype(float)
    return {int(horizon): _series(close.shift(-int(horizon)) / close - 1.0) for horizon in horizons}


def _regime_labels(frame: pd.DataFrame, funding: pd.Series | None) -> pd.Series:
    close = _series(frame["Close"]).astype(float)
    vol = close.pct_change().rolling(24, min_periods=24).std()
    vol_median = _finite_float(vol.median(), default=0.0)
    market = pd.Series("sideways", index=frame.index, dtype="object")
    ret_72 = close / close.shift(72) - 1.0
    market.loc[ret_72 > 0.03] = "bull"
    market.loc[ret_72 < -0.03] = "bear"
    vol_label = pd.Series("normal-vol", index=frame.index, dtype="object")
    vol_label.loc[vol <= vol_median] = "low-vol"
    vol_label.loc[vol > vol_median] = "high-vol"
    funding_label = pd.Series("funding-unavailable", index=frame.index, dtype="object")
    if funding is not None:
        funding_label.loc[funding > 0] = "positive-funding"
        funding_label.loc[funding < 0] = "negative-funding"
        funding_label.loc[funding == 0] = "zero-funding"
    return _series(market.astype(str) + "/" + vol_label.astype(str) + "/" + funding_label.astype(str))


def _dataset_provenance(market_bundle: MarketDataBundle, funding_bundle: FundingRateBundle | None) -> dict[str, Any]:
    frame = _normalize_ohlcv(market_bundle.frame)
    funding_quality = funding_bundle.quality if funding_bundle is not None else None
    return {
        "source": "public_binance_vision_ohlcv_and_optional_public_funding",
        "symbol": market_bundle.symbol,
        "interval": market_bundle.interval,
        "months": list(market_bundle.months),
        "timezone": "UTC",
        "row_count": int(len(frame)),
        "first_timestamp": _timestamp(cast(pd.Timestamp, frame.index[0])),
        "last_timestamp": _timestamp(cast(pd.Timestamp, frame.index[-1])),
        "normalized_ohlcv_sha256": market_bundle.normalized_sha256,
        "closed_bar_only": bool(market_bundle.quality.get("closed_bar_only", True)),
        "funding_source": "public-binance" if funding_bundle is not None else "none",
        "funding_quality": funding_quality,
        "funding_normalized_sha256": funding_bundle.normalized_sha256 if funding_bundle is not None else None,
    }


def _volatility_gate(close: pd.Series, *, window: int, low: bool) -> pd.Series:
    vol = _series(close).pct_change().rolling(window, min_periods=window).std()
    median = vol.rolling(max(window * 3, window + 1), min_periods=window).median()
    return _series((vol <= median) if low else (vol > median)).astype(float)


def _sideways_gate(close: pd.Series, *, window: int) -> pd.Series:
    returns = _series(close) / _series(close).shift(window) - 1.0
    return _series((returns.abs() <= 0.02).astype(float))


def _funding_or_zero(index: pd.Index, funding: pd.Series | None) -> pd.Series:
    if funding is None:
        return pd.Series(0.0, index=index, dtype=float)
    return _series(funding.reindex(index).fillna(0.0)).astype(float)


def _regime_breakdown(signal: pd.Series, labels: pd.Series, regime_labels: pd.Series) -> dict[str, dict[str, Any]]:
    data = pd.concat([signal.rename("signal"), labels.rename("forward"), regime_labels.rename("regime")], axis=1).dropna()
    out: dict[str, dict[str, Any]] = {}
    for regime, group in data.groupby("regime", sort=True):
        gf = _frame(group)
        out[str(regime)] = {
            "count": int(len(gf)),
            "ic": _series_corr(_series(gf["signal"]), _series(gf["forward"])),
        }
    return out


def _mean_signal(signals: list[pd.Series]) -> pd.Series:
    if not signals:
        return pd.Series(dtype=float)
    frame = pd.concat([_series(signal).astype(float) for signal in signals], axis=1)
    return _series(frame.mean(axis=1))


def _safe_ratio(numerator: pd.Series, denominator: pd.Series) -> pd.Series:
    num = _series(numerator).astype(float)
    den = _series(denominator).astype(float).replace(0.0, pd.NA)
    return _series(num / den).replace([math.inf, -math.inf], pd.NA)


def _safe_log(series: pd.Series) -> pd.Series:
    data = _series(series).astype(float)
    return _series(data.where(data > 0).apply(lambda value: math.log(float(value)) if pd.notna(value) else math.nan))


def _safe_sign(value: Any) -> float:
    if pd.isna(value):
        return 0.0
    number = float(value)
    if number > 0:
        return 1.0
    if number < 0:
        return -1.0
    return 0.0


def _last_value_rank(values: pd.Series) -> float:
    clean = _series(values).dropna()
    if clean.empty:
        return math.nan
    ranks = clean.rank(pct=True)
    return float(ranks.iloc[-1] * 2.0 - 1.0)


def _series_corr(left: pd.Series, right: pd.Series) -> float | None:
    data = pd.concat([_series(left).rename("left"), _series(right).rename("right")], axis=1).dropna()
    if len(data) < 3:
        return None
    if _finite_float(_series(data["left"]).std()) == 0.0 or _finite_float(_series(data["right"]).std()) == 0.0:
        return None
    corr = _series(data["left"]).corr(_series(data["right"]))
    return _finite_or_none(corr)


def _autocorr_or_none(series: pd.Series) -> float | None:
    data = _series(series).dropna()
    if len(data) < 3 or _finite_float(data.std()) == 0.0:
        return None
    return _finite_or_none(data.autocorr(lag=1))


def _abs_corr(left: pd.Series, right: pd.Series) -> float:
    corr = _series_corr(left, right)
    return 0.0 if corr is None else abs(float(corr))


def _beats_baseline(signal_ic: float | None, baseline_ic: float | None) -> bool | None:
    if signal_ic is None or baseline_ic is None:
        return None
    return abs(signal_ic) > abs(baseline_ic)


def _max_abs_column(frame: pd.DataFrame, column: str) -> float | None:
    values = _series(pd.to_numeric(frame[column], errors="coerce")).dropna().abs()
    if values.empty:
        return None
    return _finite_or_none(values.max())


def _finite_or_none(value: Any) -> float | None:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(number):
        return None
    return number


def _finite_float(value: Any, *, default: float = 0.0) -> float:
    number = _finite_or_none(value)
    return default if number is None else number


def _safe_div(numerator: int | float, denominator: int | float) -> float:
    return 0.0 if float(denominator) == 0.0 else float(numerator) / float(denominator)


def _timestamp(value: pd.Timestamp) -> str:
    timestamp = value.tz_localize("UTC") if value.tzinfo is None else value.tz_convert("UTC")
    return timestamp.isoformat().replace("+00:00", "Z")


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    _atomic_write_text(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def _atomic_write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(text)
    tmp.replace(path)


def _series(value: Any) -> pd.Series:
    return cast(pd.Series, value)


def _frame(value: Any) -> pd.DataFrame:
    return cast(pd.DataFrame, value)
