from __future__ import annotations

import json
import math
import random
import warnings
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterable, Literal, cast

import numpy as np
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
ALPHA_SIGNAL_DAG_SCHEMA_VERSION = 2
ALPHA_SIGNAL_DAG_LOOP_SLUG = "alpha-signal-dag-combinatorial-expansion-v2"
ALPHA_SIGNAL_FACTORY_NAMESPACE = "alpha_signal_factory_composition_harness_only_not_readiness"
ALPHA_SIGNAL_DAG_NAMESPACE = "alpha_signal_dag_combinatorial_expansion_v2_only_not_readiness"
ALPHA_SIGNAL_FACTORY_DECISION = "SIGNAL_FACTORY_INFRA_ONLY"
ALPHA_SIGNAL_DAG_DECISION = "SIGNAL_DAG_INFRA_ONLY"
READINESS_BASELINE_VERDICT = "NOT_READY"
DEFAULT_SIGNAL_GENERATION_SEED = 20260627
DEFAULT_SIGNAL_DAG_GENERATION_SEED = 20260628
DEFAULT_MAX_DEPTH = 3
DEFAULT_SIGNAL_DAG_MAX_DEPTH = 4
DEFAULT_SIGNAL_DAG_CANDIDATE_BUDGET = 2000
HARD_SIGNAL_DAG_CANDIDATE_BUDGET = 10000
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
SIGNAL_DAG_CLAIM_BOUNDARY = (
    "ALPHA_SIGNAL_DAG_COMBINATORIAL_EXPANSION_V2 is paper-only combinatorial Signal DAG "
    "generation, diagnostic evaluation, duplicate-control, multiple-testing warning, and "
    "null-comparison infrastructure. Generated DAG signals are hypotheses, not proven "
    "edges, not a trading strategy, not trading advice, not profitability evidence, not "
    "readiness or live readiness, and not permission for live/testnet/account/API-key/order/fill use."
)
MULTIPLE_TESTING_BOUNDARY = (
    "This loop generates many signal hypotheses and therefore has high data-snooping and "
    "false-discovery risk. It may only produce paper-only family follow-up queues; it cannot "
    "claim edge without a separate future pre-registered OOS falsification protocol."
)
ALLOWED_SHORTLIST_CATEGORIES = (
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEGATIVE_CONTROL_CANDIDATE",
)
DAG_ALLOWED_SHORTLIST_CATEGORIES = ALLOWED_SHORTLIST_CATEGORIES + ("NEEDS_OOS_FALSIFICATION",)
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
DAG_NULL_BASELINES_USED = NULL_BASELINES_USED[:5] + (
    "family_permuted_signal",
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
ALPHA_DAG_REQUIRED_FILES = (
    "alpha_signal_dag_expansion.json",
    "signal_dag_catalog.jsonl",
    "signal_diagnostics.csv",
    "signal_family_summary.csv",
    "signal_correlation_clusters.csv",
    "null_baseline_comparison.csv",
    "multiple_testing_summary.csv",
    "shortlist.csv",
    "failed_gate_rollup.csv",
    "report.md",
    "commands.txt",
    "changed_files.txt",
)
ALPHA_DAG_KNOWN_FILES = set(ALPHA_DAG_REQUIRED_FILES)

SignalDecision = Literal[
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEGATIVE_CONTROL_CANDIDATE",
]
DAGSignalDecision = Literal[
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEGATIVE_CONTROL_CANDIDATE",
    "NEEDS_OOS_FALSIFICATION",
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


@dataclass(frozen=True)
class SignalDagSpec:
    signal_id: str
    dag_id: str
    signal_family: str
    root_operator: str
    nodes: tuple[dict[str, Any], ...]
    edges: tuple[dict[str, str], ...]
    base_inputs: tuple[str, ...]
    operator_sequence: tuple[str, ...]
    depth: int
    max_depth: int
    lookback_bars: int
    lag_bars: int
    uses_price: bool
    uses_volume: bool
    uses_funding: bool
    uses_regime_gate: bool
    uses_future_data: bool
    generation_seed: int
    generation_rule_id: str
    complexity_score: float
    claim_boundary: str = SIGNAL_DAG_CLAIM_BOUNDARY

    def __post_init__(self) -> None:
        if self.lag_bars < 1:
            raise ValueError("SignalDagSpec.lag_bars must be at least 1 to prevent same-bar label leakage")
        if self.max_depth < 1 or self.max_depth > DEFAULT_SIGNAL_DAG_MAX_DEPTH:
            raise ValueError("SignalDagSpec.max_depth must be between 1 and 4")
        if self.depth < 1 or self.depth > self.max_depth:
            raise ValueError("SignalDagSpec.depth must be between 1 and max_depth")
        if len(self.operator_sequence) != self.depth:
            raise ValueError("SignalDagSpec.operator_sequence length must equal depth")
        if self.uses_future_data:
            raise ValueError("SignalDagSpec may not use future data")
        if not self.nodes:
            raise ValueError("SignalDagSpec requires nodes")
        if not self.base_inputs:
            raise ValueError("SignalDagSpec requires base inputs")

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> SignalDagSpec:
        return cls(
            signal_id=str(payload["signal_id"]),
            dag_id=str(payload["dag_id"]),
            signal_family=str(payload["signal_family"]),
            root_operator=str(payload["root_operator"]),
            nodes=tuple(dict(item) for item in payload["nodes"]),
            edges=tuple(dict(item) for item in payload["edges"]),
            base_inputs=tuple(str(item) for item in payload["base_inputs"]),
            operator_sequence=tuple(str(item) for item in payload["operator_sequence"]),
            depth=int(payload["depth"]),
            max_depth=int(payload["max_depth"]),
            lookback_bars=int(payload["lookback_bars"]),
            lag_bars=int(payload["lag_bars"]),
            uses_price=bool(payload["uses_price"]),
            uses_volume=bool(payload["uses_volume"]),
            uses_funding=bool(payload["uses_funding"]),
            uses_regime_gate=bool(payload["uses_regime_gate"]),
            uses_future_data=bool(payload["uses_future_data"]),
            generation_seed=int(payload["generation_seed"]),
            generation_rule_id=str(payload["generation_rule_id"]),
            complexity_score=float(payload["complexity_score"]),
            claim_boundary=str(payload.get("claim_boundary", SIGNAL_DAG_CLAIM_BOUNDARY)),
        )


@dataclass(frozen=True)
class SignalDagExpansionResult:
    payload: dict[str, Any]
    catalog: list[dict[str, Any]]
    diagnostics: list[dict[str, Any]]
    family_summary: list[dict[str, Any]]
    clusters: list[dict[str, Any]]
    null_comparison: list[dict[str, Any]]
    multiple_testing_summary: list[dict[str, Any]]
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
    data = _coerce_float_series(series)
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
    if operator == "volatility_scale":
        return _safe_ratio(data, _series(data.diff().rolling(lookback_bars, min_periods=lookback_bars).std()))
    if operator == "rolling_min":
        return _series(data.rolling(lookback_bars, min_periods=lookback_bars).min())
    if operator == "rolling_max":
        return _series(data.rolling(lookback_bars, min_periods=lookback_bars).max())
    if operator == "rolling_quantile_position":
        rolling_min = _series(data.rolling(lookback_bars, min_periods=lookback_bars).min())
        rolling_max = _series(data.rolling(lookback_bars, min_periods=lookback_bars).max())
        return _series(_safe_ratio(_series(data - rolling_min), _series(rolling_max - rolling_min)) * 2.0 - 1.0)
    raise ValueError(f"unsupported unary operator: {operator}")


def apply_binary_operator(left: pd.Series, right: pd.Series, operator: str, *, lookback_bars: int = 24) -> pd.Series:
    lhs = _coerce_float_series(left)
    rhs = _coerce_float_series(right).reindex(lhs.index)
    if operator == "add":
        return lhs + rhs
    if operator == "subtract":
        return lhs - rhs
    if operator == "multiply" or operator == "interaction":
        return lhs * rhs
    if operator == "ratio_safe":
        return _safe_ratio(lhs, rhs)
    if operator == "rolling_corr":
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", RuntimeWarning)
            return _series(lhs.rolling(lookback_bars, min_periods=lookback_bars).corr(rhs))
    if operator == "rolling_beta_proxy":
        covariance = _series(lhs.rolling(lookback_bars, min_periods=lookback_bars).cov(rhs))
        variance = _series(rhs.rolling(lookback_bars, min_periods=lookback_bars).var())
        return _safe_ratio(covariance, variance)
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
    return _series(data.rolling(window, min_periods=window).apply(_last_value_rank_array, raw=True))


def decay_linear(series: pd.Series, *, window: int) -> pd.Series:
    if window <= 1:
        raise ValueError("linear decay requires window > 1")
    data = _series(series).astype(float)
    weights = [float(item + 1) for item in range(window)]
    weight_sum = sum(weights)

    def apply(values: np.ndarray) -> float:
        if len(values) != window:
            return math.nan
        if not bool(np.isfinite(values).all()):
            return math.nan
        return float(np.dot(values.astype(float), np.array(weights, dtype=float)) / weight_sum)

    return _series(data.rolling(window, min_periods=window).apply(apply, raw=True))


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
    num = _coerce_float_series(numerator)
    den = _coerce_float_series(denominator).replace(0.0, math.nan)
    return _series(num / den).replace([math.inf, -math.inf], math.nan)


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


def _last_value_rank_array(values: np.ndarray) -> float:
    clean = values[np.isfinite(values)]
    if len(clean) == 0:
        return math.nan
    last = values[-1]
    if not math.isfinite(float(last)):
        return math.nan
    less = float(np.sum(clean < last))
    equal = float(np.sum(clean == last))
    percentile_rank = (less + 0.5 * equal) / float(len(clean))
    return float(percentile_rank * 2.0 - 1.0)


def _series_corr(left: pd.Series, right: pd.Series) -> float | None:
    lhs = _coerce_float_series(left)
    rhs = _coerce_float_series(right.reindex(lhs.index))
    lhs_array = lhs.to_numpy(dtype=float)
    rhs_array = rhs.to_numpy(dtype=float)
    mask = np.isfinite(lhs_array) & np.isfinite(rhs_array)
    if int(mask.sum()) < 3:
        return None
    lhs_clean = lhs_array[mask]
    rhs_clean = rhs_array[mask]
    lhs_centered = lhs_clean - float(np.mean(lhs_clean))
    rhs_centered = rhs_clean - float(np.mean(rhs_clean))
    denominator = math.sqrt(float(np.dot(lhs_centered, lhs_centered)) * float(np.dot(rhs_centered, rhs_centered)))
    if denominator == 0.0:
        return None
    corr = float(np.dot(lhs_centered, rhs_centered) / denominator)
    return _finite_or_none(corr)


def _autocorr_or_none(series: pd.Series) -> float | None:
    data = _series(series).dropna()
    if len(data) < 3 or _finite_float(data.std()) == 0.0:
        return None
    return _finite_or_none(data.autocorr(lag=1))


def _abs_corr(left: pd.Series, right: pd.Series) -> float:
    lhs = _coerce_float_series(left).to_numpy(dtype=float)
    rhs = _coerce_float_series(right.reindex(left.index)).to_numpy(dtype=float)
    mask = np.isfinite(lhs) & np.isfinite(rhs)
    if int(mask.sum()) < 3:
        return 0.0
    lhs_clean = lhs[mask]
    rhs_clean = rhs[mask]
    lhs_centered = lhs_clean - float(np.mean(lhs_clean))
    rhs_centered = rhs_clean - float(np.mean(rhs_clean))
    denominator = math.sqrt(float(np.dot(lhs_centered, lhs_centered)) * float(np.dot(rhs_centered, rhs_centered)))
    if denominator == 0.0:
        return 0.0
    corr = float(np.dot(lhs_centered, rhs_centered) / denominator)
    if not math.isfinite(corr):
        return 0.0
    return abs(corr)


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


def _coerce_float_series(value: Any) -> pd.Series:
    return _series(pd.to_numeric(_series(value).replace([math.inf, -math.inf], math.nan), errors="coerce")).astype(float)


def _frame(value: Any) -> pd.DataFrame:
    return cast(pd.DataFrame, value)


def default_alpha_signal_dag_expansion_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{ALPHA_SIGNAL_DAG_LOOP_SLUG}"


def generate_signal_dag_specs(
    *,
    seed: int = DEFAULT_SIGNAL_DAG_GENERATION_SEED,
    max_depth: int = DEFAULT_SIGNAL_DAG_MAX_DEPTH,
    candidate_budget: int = DEFAULT_SIGNAL_DAG_CANDIDATE_BUDGET,
    include_funding: bool = True,
    include_volume: bool = True,
) -> list[SignalDagSpec]:
    if max_depth < 1 or max_depth > DEFAULT_SIGNAL_DAG_MAX_DEPTH:
        raise ValueError("max_depth must be between 1 and 4")
    if candidate_budget < 1 or candidate_budget > HARD_SIGNAL_DAG_CANDIDATE_BUDGET:
        raise ValueError("candidate_budget must be between 1 and 10000")
    base_inputs = [
        "close",
        "returns",
        "log_returns",
        "rolling_range",
        "realized_volatility",
        "open",
        "high",
        "low",
    ]
    if include_volume:
        base_inputs.extend(["volume", "dollar_volume_proxy"])
    if include_funding:
        base_inputs.extend(["funding_rate", "funding_change", "funding_zscore", "funding_extreme_flag"])
    unary_ops = [
        "delay",
        "delta",
        "rolling_mean",
        "rolling_std",
        "zscore",
        "rank",
        "winsorize",
        "sign",
        "abs",
        "negate",
        "clip",
        "ewma",
        "decay_linear",
        "volatility_scale",
        "rolling_min",
        "rolling_max",
        "rolling_quantile_position",
    ]
    binary_ops = [
        "add",
        "subtract",
        "multiply",
        "ratio_safe",
        "spread",
        "interaction",
        "rolling_corr",
        "rolling_beta_proxy",
        "min",
        "max",
    ]
    templates = [
        ("momentum_x_volatility", "momentum", "returns", "realized_volatility", "interaction", "gate_high_volatility"),
        ("momentum_x_funding", "funding", "returns", "funding_rate", "interaction", "gate_positive_funding"),
        ("mean_reversion_x_low_vol", "mean_reversion", "rolling_range", "realized_volatility", "subtract", "gate_low_volatility"),
        ("mean_reversion_x_funding_extreme", "mean_reversion", "returns", "funding_extreme_flag", "multiply", "gate_extreme_funding"),
        ("volume_shock_x_return_direction", "volume_liquidity", "volume", "returns", "interaction", "gate_volume_spike"),
        ("breakout_x_volume_confirmation", "momentum", "close", "volume", "rolling_corr", "gate_volume_spike"),
        ("funding_extreme_x_volatility", "funding", "funding_extreme_flag", "realized_volatility", "interaction", "gate_high_volatility"),
        ("funding_change_x_return_reversal", "funding", "funding_change", "returns", "subtract", "gate_negative_funding"),
        ("range_position_x_realized_volatility", "volatility", "rolling_range", "realized_volatility", "ratio_safe", "gate_range_bound"),
        ("carry_proxy_x_trend_filter", "funding", "funding_rate", "returns", "multiply", "gate_trending_proxy"),
    ]
    templates = [item for item in templates if include_funding or "funding" not in {item[0], item[2], item[3], item[5]}]
    templates = [item for item in templates if include_volume or "volume" not in {item[0], item[2], item[3], item[5]}]
    lookbacks = (8, 24, 72)
    specs: list[SignalDagSpec] = []
    serial = 0

    def add_spec(
        *,
        family: str,
        generation_rule_id: str,
        primary: str,
        secondary: str | None,
        operators: list[dict[str, Any]],
        lookback: int,
    ) -> None:
        nonlocal serial
        if len(specs) >= candidate_budget:
            return
        if len(operators) > max_depth:
            return
        if secondary is not None and secondary not in base_inputs:
            return
        if primary not in base_inputs:
            return
        if any(str(op["op"]) in {"gate_positive_funding", "gate_negative_funding", "gate_extreme_funding"} for op in operators) and not include_funding:
            return
        if any(str(op["op"]) == "gate_volume_spike" for op in operators) and not include_volume:
            return
        serial += 1
        base_tuple = (primary,) if secondary is None else (primary, secondary)
        operator_sequence = tuple(str(op["op"]) for op in operators)
        dag_id = f"dag{serial:05d}"
        signal_id = f"dag.{family}.{generation_rule_id}.lb{lookback}.{dag_id}"
        nodes: list[dict[str, Any]] = []
        edges: list[dict[str, str]] = []
        nodes.append({"node_id": "base_0", "type": "base", "input": primary})
        if secondary is not None:
            nodes.append({"node_id": "base_1", "type": "base", "input": secondary})
        previous = "base_0"
        for idx, operator in enumerate(operators, start=1):
            node_id = f"op_{idx}"
            op_name = str(operator["op"])
            node = {"node_id": node_id, "type": _operator_node_type(op_name), **operator}
            nodes.append(node)
            edges.append({"source": previous, "target": node_id})
            if node["type"] == "binary" and secondary is not None:
                edges.append({"source": "base_1", "target": node_id})
            previous = node_id
        complexity = float(len(operators) + len(base_tuple) * 0.25 + sum(0.5 for op in operator_sequence if op.startswith("gate_")))
        specs.append(
            SignalDagSpec(
                signal_id=signal_id,
                dag_id=dag_id,
                signal_family=family,
                root_operator=operator_sequence[-1],
                nodes=tuple(nodes),
                edges=tuple(edges),
                base_inputs=base_tuple,
                operator_sequence=operator_sequence,
                depth=len(operators),
                max_depth=max_depth,
                lookback_bars=lookback,
                lag_bars=DEFAULT_DECISION_LAG_BARS,
                uses_price=any(item in {"close", "open", "high", "low", "returns", "log_returns", "rolling_range", "realized_volatility"} for item in base_tuple),
                uses_volume=any("volume" in item for item in base_tuple) or any(str(op["op"]) == "gate_volume_spike" for op in operators),
                uses_funding=any("funding" in item for item in base_tuple) or any("funding" in str(op["op"]) for op in operators),
                uses_regime_gate=any(str(op["op"]).startswith("gate_") for op in operators),
                uses_future_data=False,
                generation_seed=deterministic_random_seed(signal_id, seed_base=seed),
                generation_rule_id=generation_rule_id,
                complexity_score=complexity,
            )
        )

    for lookback in lookbacks:
        for base in base_inputs:
            family = _family_for_base_input(base)
            for unary in unary_ops:
                add_spec(
                    family=family,
                    generation_rule_id=f"unary_{unary}_{base}",
                    primary=base,
                    secondary=None,
                    operators=[_operator_payload(unary, lookback)],
                    lookback=lookback,
                )
                add_spec(
                    family=family,
                    generation_rule_id=f"unary_stack_{unary}_zscore_{base}",
                    primary=base,
                    secondary=None,
                    operators=[_operator_payload(unary, lookback), _operator_payload("zscore", lookback)],
                    lookback=lookback,
                )
        for template_name, family, primary, secondary, binary, gate in templates:
            for unary in ("zscore", "rank", "sign", "decay_linear"):
                ops = [_operator_payload(binary, lookback), _operator_payload(gate, lookback), _operator_payload(unary, lookback)]
                add_spec(
                    family=family,
                    generation_rule_id=f"template_{template_name}_{unary}",
                    primary=primary,
                    secondary=secondary,
                    operators=ops,
                    lookback=lookback,
                )
                if max_depth >= 4:
                    add_spec(
                        family=family,
                        generation_rule_id=f"template_{template_name}_{unary}_clip",
                        primary=primary,
                        secondary=secondary,
                        operators=ops + [_operator_payload("clip", lookback)],
                        lookback=lookback,
                    )
        for left, right in _deterministic_base_pairs(base_inputs, limit=24):
            family = _family_for_base_input(left if _family_for_base_input(left) != "price_return" else right)
            for binary in binary_ops:
                add_spec(
                    family=family,
                    generation_rule_id=f"binary_{binary}_{left}_{right}",
                    primary=left,
                    secondary=right,
                    operators=[_operator_payload(binary, lookback)],
                    lookback=lookback,
                )
                if max_depth >= 2:
                    add_spec(
                        family=family,
                        generation_rule_id=f"binary_{binary}_zscore_{left}_{right}",
                        primary=left,
                        secondary=right,
                        operators=[_operator_payload(binary, lookback), _operator_payload("zscore", lookback)],
                        lookback=lookback,
                    )
        if len(specs) >= candidate_budget:
            break
    return specs[:candidate_budget]


def evaluate_alpha_signal_dag_expansion(
    *,
    market_bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None = None,
    seed: int = DEFAULT_SIGNAL_DAG_GENERATION_SEED,
    max_depth: int = DEFAULT_SIGNAL_DAG_MAX_DEPTH,
    candidate_budget: int = DEFAULT_SIGNAL_DAG_CANDIDATE_BUDGET,
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> SignalDagExpansionResult:
    frame = _normalize_ohlcv(market_bundle.frame)
    funding = _align_public_funding(frame.index, funding_bundle.frame if funding_bundle is not None else None)
    specs = generate_signal_dag_specs(
        seed=seed,
        max_depth=max_depth,
        candidate_budget=candidate_budget,
        include_funding=funding_bundle is not None,
        include_volume="Volume" in frame.columns,
    )
    label_returns = _forward_return_labels(frame, horizons=FORWARD_HORIZONS)
    regime_labels = _regime_labels(frame, funding)
    base_series = _dag_base_series(frame, funding)
    signals: dict[str, pd.Series] = {}
    diagnostics: list[dict[str, Any]] = []
    for spec in specs:
        raw_signal = _build_signal_dag_series_prepared(spec, frame=frame, funding=funding, base_series=base_series)
        decision_signal = lag_signal_for_labels(raw_signal, lag_bars=spec.lag_bars)
        signals[spec.signal_id] = decision_signal
        diagnostics.append(
            _dag_diagnostics_for_signal(
                spec,
                raw_signal=raw_signal,
                decision_signal=decision_signal,
                forward_returns=label_returns,
                regime_labels=regime_labels,
            )
        )
    clusters = duplicate_correlation_clusters_for_dags(signals, specs)
    cluster_by_signal = {row["signal_id"]: row for row in clusters}
    for row in diagnostics:
        cluster = cluster_by_signal.get(str(row["signal_id"]), {})
        row["duplicate_cluster_id"] = cluster.get("duplicate_cluster_id")
        row["max_abs_correlation_to_existing_signal"] = cluster.get("max_abs_correlation_to_existing_signal")
        row["effective_independent_signal"] = not bool(cluster.get("is_redundant_duplicate", False))
    null_rows = _dag_null_comparison_rows(signals=signals, specs=specs, forward_returns=label_returns, seed=seed, clusters=clusters)
    family_summary = _dag_family_summary_rows(diagnostics, null_rows)
    multiple_testing = _multiple_testing_summary_rows(diagnostics, family_summary, null_rows, clusters)
    shortlist = _dag_shortlist_rows(family_summary)
    failed_gates = _dag_failed_gate_rows(diagnostics=diagnostics, family_summary=family_summary, clusters=clusters, multiple_testing=multiple_testing)
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    killed_count = sum(1 for row in shortlist if row["shortlist_category"] == "KILL_SIGNAL")
    watch_count = sum(1 for row in shortlist if str(row["shortlist_category"]).startswith("WATCH") or row["shortlist_category"] == "NEEDS_OOS_FALSIFICATION")
    effective_breadth = len({row["duplicate_cluster_id"] for row in clusters if not bool(row.get("is_redundant_duplicate"))})
    duplicate_ratio = _safe_div(duplicate_count, len(clusters))
    payload = {
        "schema_version": ALPHA_SIGNAL_DAG_SCHEMA_VERSION,
        "loop_id": f"{datetime.now(UTC).strftime('%Y%m%d')}-{ALPHA_SIGNAL_DAG_LOOP_SLUG}",
        "loop_namespace": ALPHA_SIGNAL_DAG_NAMESPACE,
        "loop_type": "validation_infrastructure",
        "status": "IMPLEMENTED",
        "suite_decision": ALPHA_SIGNAL_DAG_DECISION,
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "generation_seed": int(seed),
        "candidate_budget": int(candidate_budget),
        "max_depth": int(max_depth),
        "generated_signal_count": len(specs),
        "evaluated_signal_count": len(diagnostics),
        "killed_signal_count": int(killed_count),
        "duplicate_signal_count": int(duplicate_count),
        "watch_family_count": int(watch_count),
        "effective_signal_breadth": int(effective_breadth),
        "duplicate_ratio": duplicate_ratio,
        "families_evaluated": sorted({spec.signal_family for spec in specs}),
        "operator_families_used": _operator_families_used(specs),
        "horizons_evaluated": list(FORWARD_HORIZONS),
        "null_baselines_used": list(DAG_NULL_BASELINES_USED),
        "multiple_testing_boundary": MULTIPLE_TESTING_BOUNDARY,
        "leakage_boundary": LEAKAGE_BOUNDARY,
        "claim_boundary": SIGNAL_DAG_CLAIM_BOUNDARY,
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
        "selection_boundary": {
            "candidate_generation_used_forward_returns": False,
            "candidate_generation_selected_by_performance": False,
            "shortlist_is_strategy": False,
            "shortlist_is_readiness": False,
            "weight_optimization_performed": False,
            "threshold_or_horizon_retune_performed": False,
            "holdout_used_for_selection": False,
        },
        "data_insufficient_reason": None if len(specs) >= candidate_budget else "bounded grammar or unavailable funding/volume generated fewer candidates than requested budget",
        "dataset_provenance": _dataset_provenance(market_bundle, funding_bundle),
        "summary": {
            "diagnostic_metric_only": True,
            "strategy_pnl_optimization": False,
            "shortlist_categories": list(DAG_ALLOWED_SHORTLIST_CATEGORIES),
            "forbidden_outputs": sorted(FORBIDDEN_OUTPUT_LABELS),
            "cannot_claim_edge_due_to_multiple_testing": True,
        },
        "validation_commands": list(commands),
        "changed_files": list(changed_files),
        "evidence_artifacts": list(ALPHA_DAG_REQUIRED_FILES),
        "next_loop": "Use any WATCH/NEEDS_OOS family only as input to a separate pre-registered paper-only OOS falsification loop.",
    }
    return SignalDagExpansionResult(
        payload=payload,
        catalog=[spec.to_dict() for spec in specs],
        diagnostics=diagnostics,
        family_summary=family_summary,
        clusters=clusters,
        null_comparison=null_rows,
        multiple_testing_summary=multiple_testing,
        shortlist=shortlist,
        failed_gates=failed_gates,
    )


def build_signal_dag_series(spec: SignalDagSpec, *, frame: pd.DataFrame, funding: pd.Series | None = None) -> pd.Series:
    if spec.uses_future_data:
        raise ValueError(f"signal DAG {spec.signal_id} uses future data")
    data = _normalize_ohlcv(frame)
    base = _dag_base_series(data, funding)
    return _build_signal_dag_series_prepared(spec, frame=data, funding=funding, base_series=base)


def _build_signal_dag_series_prepared(
    spec: SignalDagSpec,
    *,
    frame: pd.DataFrame,
    funding: pd.Series | None,
    base_series: dict[str, pd.Series],
) -> pd.Series:
    values: dict[str, pd.Series] = {}
    for node in spec.nodes:
        node_id = str(node["node_id"])
        node_type = str(node["type"])
        if node_type == "base":
            values[node_id] = base_series[str(node["input"])]
            continue
        parent_ids = [edge["source"] for edge in spec.edges if edge["target"] == node_id]
        if not parent_ids:
            raise ValueError(f"DAG node has no parent: {node_id}")
        left = values[parent_ids[0]]
        op = str(node["op"])
        window = int(node.get("window", spec.lookback_bars))
        if node_type == "binary":
            if len(parent_ids) < 2:
                raise ValueError(f"binary DAG node has fewer than two parents: {node_id}")
            right = values[parent_ids[1]]
            values[node_id] = apply_binary_operator(left, right, op, lookback_bars=window)
        elif node_type == "gate":
            values[node_id] = apply_gate_operator(left, op, frame=frame, funding=funding, lookback_bars=window)
        else:
            values[node_id] = apply_unary_operator(left, op, lookback_bars=window)
    root_id = str(spec.nodes[-1]["node_id"])
    return _series(pd.to_numeric(values[root_id].replace([math.inf, -math.inf], math.nan), errors="coerce")).astype(float)


def apply_gate_operator(
    series: pd.Series,
    operator: str,
    *,
    frame: pd.DataFrame,
    funding: pd.Series | None = None,
    lookback_bars: int = 24,
) -> pd.Series:
    data = _normalize_ohlcv(frame)
    current = _series(series).astype(float)
    close = _series(data["Close"]).astype(float)
    if operator == "gate_low_volatility":
        return current * _volatility_gate(close, window=lookback_bars, low=True)
    if operator == "gate_high_volatility":
        return current * _volatility_gate(close, window=lookback_bars, low=False)
    if operator == "gate_positive_funding":
        return current * (_funding_or_zero(data.index, funding) > 0).astype(float)
    if operator == "gate_negative_funding":
        return current * (_funding_or_zero(data.index, funding) < 0).astype(float)
    if operator == "gate_extreme_funding":
        return current * (rolling_zscore(_funding_or_zero(data.index, funding), window=max(lookback_bars, 2)).abs() >= 1.5).astype(float)
    if operator == "gate_volume_spike":
        return current * (rolling_zscore(_series(data["Volume"]).astype(float), window=max(lookback_bars, 2)).abs() >= 1.5).astype(float)
    if operator == "gate_range_bound":
        return current * _sideways_gate(close, window=lookback_bars)
    if operator == "gate_trending_proxy":
        trend = (close / close.shift(lookback_bars) - 1.0).abs()
        threshold = trend.rolling(max(lookback_bars * 3, lookback_bars + 1), min_periods=lookback_bars).median()
        return current * (trend > threshold).astype(float)
    raise ValueError(f"unsupported gate operator: {operator}")


def duplicate_correlation_clusters_for_dags(
    signals: dict[str, pd.Series],
    specs: list[SignalDagSpec],
    *,
    threshold: float = DUPLICATE_CORRELATION_THRESHOLD,
) -> list[dict[str, Any]]:
    spec_by_id = {spec.signal_id: spec for spec in specs}
    signal_ids = list(signals)
    if not signal_ids:
        return []
    common_index = signals[signal_ids[0]].index
    matrix_frame = pd.DataFrame(
        {signal_id: _coerce_float_series(signals[signal_id]).reindex(common_index) for signal_id in signal_ids},
        index=common_index,
    )
    matrix = matrix_frame.to_numpy(dtype=float)
    finite = np.isfinite(matrix)
    counts = finite.sum(axis=0)
    sums = np.where(finite, matrix, 0.0).sum(axis=0)
    means = np.divide(sums, counts, out=np.zeros_like(sums, dtype=float), where=counts > 0)
    centered = np.where(finite, matrix - means, 0.0)
    norms = np.sqrt(np.sum(centered * centered, axis=0))
    valid = norms > 0.0
    normalized = np.zeros_like(centered, dtype=float)
    normalized[:, valid] = centered[:, valid] / norms[valid]
    corr_matrix = np.abs(normalized.T @ normalized)
    corr_matrix[~np.isfinite(corr_matrix)] = 0.0
    np.fill_diagonal(corr_matrix, 0.0)

    max_corr = {signal_id: float(corr_matrix[idx].max()) if bool(valid[idx]) else 0.0 for idx, signal_id in enumerate(signal_ids)}
    signal_index = {signal_id: idx for idx, signal_id in enumerate(signal_ids)}
    clusters: list[set[str]] = []
    constant_cluster: set[str] = set()
    for signal_id in signal_ids:
        idx = signal_index[signal_id]
        if not bool(valid[idx]):
            constant_cluster.add(signal_id)
            continue
        assigned = False
        for cluster in clusters:
            representative = _dag_cluster_representative(cluster, spec_by_id)
            representative_idx = signal_index[representative]
            if float(corr_matrix[idx, representative_idx]) >= threshold:
                cluster.add(signal_id)
                assigned = True
                break
        if not assigned:
            clusters.append({signal_id})
    if constant_cluster:
        clusters.append(constant_cluster)

    rows: list[dict[str, Any]] = []
    cluster_id_by_signal: dict[str, str] = {}
    representative_by_cluster: dict[str, str] = {}
    for idx, cluster in enumerate(clusters, start=1):
        cluster_id = f"C{idx:04d}"
        representative = _dag_cluster_representative(cluster, spec_by_id)
        representative_by_cluster[cluster_id] = representative
        for signal_id in cluster:
            cluster_id_by_signal[signal_id] = cluster_id
    for signal_id in signal_ids:
        cluster_id = cluster_id_by_signal[signal_id]
        cluster_members = sorted(item for item, cid in cluster_id_by_signal.items() if cid == cluster_id)
        representative = representative_by_cluster[cluster_id]
        spec = spec_by_id[signal_id]
        rows.append(
            {
                "signal_id": signal_id,
                "dag_id": spec.dag_id,
                "signal_family": spec.signal_family,
                "duplicate_cluster_id": cluster_id,
                "cluster_size": len(cluster_members),
                "cluster_members": ";".join(cluster_members),
                "representative_signal_id": representative,
                "representative_rule": "lowest_complexity_then_depth_then_lexicographic_signal_id",
                "is_redundant_duplicate": signal_id != representative,
                "max_abs_correlation_to_existing_signal": max_corr[signal_id],
                "duplicate_threshold": threshold,
                "complexity_score": spec.complexity_score,
                "depth": spec.depth,
                "constant_or_all_nan_signal": not bool(valid[signal_index[signal_id]]),
                "correlation_engine": "single_matrix_deterministic_nan_to_zero_centered",
            }
        )
    return rows


def write_alpha_signal_dag_expansion_artifacts(
    output_dir: str | Path,
    result: SignalDagExpansionResult,
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_alpha_signal_dag_expansion_output_dir(output_dir, overwrite=overwrite)
    payload = dict(result.payload)
    if out.name.endswith(ALPHA_SIGNAL_DAG_LOOP_SLUG):
        payload["loop_id"] = out.name
    _write_json(out / "alpha_signal_dag_expansion.json", payload)
    _atomic_write_text(
        out / "signal_dag_catalog.jsonl",
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in result.catalog),
    )
    pd.DataFrame(result.diagnostics).to_csv(out / "signal_diagnostics.csv", index=False)
    pd.DataFrame(result.family_summary).to_csv(out / "signal_family_summary.csv", index=False)
    pd.DataFrame(result.clusters).to_csv(out / "signal_correlation_clusters.csv", index=False)
    pd.DataFrame(result.null_comparison).to_csv(out / "null_baseline_comparison.csv", index=False)
    pd.DataFrame(result.multiple_testing_summary).to_csv(out / "multiple_testing_summary.csv", index=False)
    pd.DataFrame(result.shortlist).to_csv(out / "shortlist.csv", index=False)
    pd.DataFrame(result.failed_gates).to_csv(out / "failed_gate_rollup.csv", index=False)
    _atomic_write_text(out / "report.md", render_alpha_signal_dag_expansion_report(payload, result))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    return out


def prepare_alpha_signal_dag_expansion_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="alpha-signal-dag-expansion-run",
        known_files=ALPHA_DAG_KNOWN_FILES,
        overwrite=overwrite,
    )


def render_alpha_signal_dag_expansion_report(payload: dict[str, Any], result: SignalDagExpansionResult) -> str:
    watch = [row for row in result.shortlist if row["shortlist_category"] in {"WATCH_FAMILY_ONLY", "WATCH_DIAGNOSTIC_ONLY", "NEEDS_OOS_FALSIFICATION"}]
    duplicate_rows = [row for row in result.clusters if bool(row.get("is_redundant_duplicate"))]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Purpose",
        "Build a bounded deterministic paper-only combinatorial Signal DAG expansion harness for alpha-signal hypothesis generation, duplicate pruning, null comparison, and multiple-testing-aware family summaries.",
        "",
        "## Claim boundary",
        f"- suite_decision: {payload['suite_decision']}",
        f"- loop_namespace: {payload['loop_namespace']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- claim_boundary: {payload['claim_boundary']}",
        f"- multiple_testing_boundary: {payload['multiple_testing_boundary']}",
        "- Shortlists are paper-only research queues; they are not strategies, order intents, readiness, profitability claims, or live/testnet permission.",
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
        "## DAG generation",
        f"- generation_seed: {payload['generation_seed']}",
        f"- candidate_budget: {payload['candidate_budget']}",
        f"- max_depth: {payload['max_depth']}",
        f"- generated_signal_count: {payload['generated_signal_count']}",
        f"- evaluated_signal_count: {payload['evaluated_signal_count']}",
        f"- families_evaluated: {', '.join(payload['families_evaluated'])}",
        f"- operator_families_used: {', '.join(payload['operator_families_used'])}",
        "",
        "## Duplicate and null controls",
        f"- duplicate_signal_count: {payload['duplicate_signal_count']}",
        f"- effective_signal_breadth: {payload['effective_signal_breadth']}",
        f"- duplicate_ratio: {payload['duplicate_ratio']}",
        f"- null_baselines_used: {', '.join(payload['null_baselines_used'])}",
        f"- redundant duplicates marked: {len(duplicate_rows)}",
        "",
        "## Multiple-testing warning",
        "- cannot_claim_edge_due_to_multiple_testing: True",
        "- approximate_false_discovery_warning: many generated DAG hypotheses inflate false-discovery risk; any apparent IC requires a separate future OOS falsification protocol.",
        "",
        "## Paper-only shortlist",
    ]
    if watch:
        for row in watch:
            lines.append(f"- {row['signal_family']}: {row['shortlist_category']} ({row['rationale']})")
    else:
        lines.append("- no WATCH/NEEDS_OOS family emitted")
    lines.extend(["", "## Failed gates / unclaimed checks"])
    for row in result.failed_gates:
        lines.append(f"- {row['gate']}: {row['status']} ({row['reason']})")
    lines.extend(
        [
            "",
            "## What cannot be claimed",
            "- live readiness",
            "- profitability",
            "- execution readiness",
            "- permission to use real money",
            "- restoration of TSMOM active development",
            "- that any generated DAG signal is a durable edge",
            "- that any shortlist family is tradable without a separate pre-registered paper-only OOS falsification loop",
            "",
            "## Next loop",
            payload["next_loop"],
            "",
        ]
    )
    return "\n".join(lines)


def _operator_payload(operator: str, lookback: int) -> dict[str, Any]:
    payload: dict[str, Any] = {"op": operator, "window": int(max(lookback, 2))}
    if operator == "delay":
        payload["bars"] = 1
    if operator == "clip":
        payload["lower"] = -1.0
        payload["upper"] = 1.0
    if operator == "winsorize":
        payload["limit"] = 3.0
    return payload


def _operator_node_type(operator: str) -> str:
    if operator.startswith("gate_"):
        return "gate"
    if operator in {"add", "subtract", "multiply", "ratio_safe", "spread", "interaction", "rolling_corr", "rolling_beta_proxy", "min", "max"}:
        return "binary"
    return "unary"


def _deterministic_base_pairs(base_inputs: list[str], *, limit: int) -> list[tuple[str, str]]:
    pairs: list[tuple[str, str]] = []
    priority = {
        "returns": 0,
        "log_returns": 1,
        "realized_volatility": 2,
        "funding_rate": 3,
        "funding_change": 4,
        "funding_extreme_flag": 5,
        "volume": 6,
        "dollar_volume_proxy": 7,
        "rolling_range": 8,
        "close": 9,
    }
    for idx, left in enumerate(base_inputs):
        for right in base_inputs[idx + 1 :]:
            pairs.append((left, right))
    return sorted(pairs, key=lambda item: (min(priority.get(item[0], 99), priority.get(item[1], 99)), item[0], item[1]))[:limit]


def _family_for_base_input(base_input: str) -> str:
    if "funding" in base_input:
        return "funding"
    if "volume" in base_input:
        return "volume_liquidity"
    if base_input in {"realized_volatility", "rolling_range", "high", "low"}:
        return "volatility"
    if base_input in {"returns", "log_returns"}:
        return "momentum"
    return "price_return"


def _dag_base_series(frame: pd.DataFrame, funding: pd.Series | None) -> dict[str, pd.Series]:
    close = _series(frame["Close"]).astype(float)
    high = _series(frame["High"]).astype(float)
    low = _series(frame["Low"]).astype(float)
    volume = _series(frame["Volume"]).astype(float)
    returns = _series(close.pct_change())
    funding_series = _funding_or_zero(frame.index, funding)
    return {
        "close": close,
        "open": _series(frame["Open"]).astype(float),
        "high": high,
        "low": low,
        "volume": volume,
        "returns": returns,
        "log_returns": _safe_log(close / close.shift(1)),
        "rolling_range": _safe_ratio(_series(high - low), close),
        "realized_volatility": _series(returns.rolling(24, min_periods=24).std()),
        "dollar_volume_proxy": _series(close * volume),
        "funding_rate": funding_series,
        "funding_change": _series(funding_series.diff()),
        "funding_zscore": rolling_zscore(funding_series, window=72),
        "funding_extreme_flag": _series((rolling_zscore(funding_series, window=72).abs() >= 1.5).astype(float) * rolling_zscore(funding_series, window=72).apply(_safe_sign)),
    }


def _dag_diagnostics_for_signal(
    spec: SignalDagSpec,
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
        "dag_id": spec.dag_id,
        "signal_family": spec.signal_family,
        "depth": spec.depth,
        "complexity_score": spec.complexity_score,
        "non_null_observation_count": int(len(non_null)),
        "coverage_ratio": _safe_div(len(non_null), len(signal)),
        "missing_ratio": 1.0 - _safe_div(len(non_null), len(signal)),
        "mean_signal": _finite_or_none(non_null.mean()) if not non_null.empty else None,
        "std_signal": _finite_or_none(non_null.std()) if len(non_null) > 1 else None,
        "signal_autocorrelation": _autocorr_or_none(non_null),
        "turnover_proxy": _finite_or_none(signal.diff().abs().mean()),
        "max_abs_signal": _finite_or_none(non_null.abs().max()) if not non_null.empty else None,
        "duplicate_cluster_id": None,
        "max_abs_correlation_to_existing_signal": None,
        "effective_independent_signal": True,
        "forward_return_ic_8h": None,
        "forward_return_ic_24h": None,
        "forward_return_ic_72h": None,
        "rank_ic_8h": None,
        "rank_ic_24h": None,
        "rank_ic_72h": None,
        "hit_rate_by_signal_sign_8h": None,
        "hit_rate_by_signal_sign_24h": None,
        "hit_rate_by_signal_sign_72h": None,
        "decay_profile": "{}",
        "regime_breakdown": "{}",
        "null_comparison_status": "evaluated_by_family_and_representative_rows",
        "leakage_check_status": "passed_shifted_by_decision_lag",
        "failure_reason": None,
        "lag_bars": spec.lag_bars,
        "uses_future_data": spec.uses_future_data,
        "claim_boundary": spec.claim_boundary,
    }
    decay: dict[str, float | None] = {}
    regime_breakdown: dict[str, Any] = {}
    for horizon, labels in forward_returns.items():
        ic = _series_corr(signal, labels)
        rank_ic = _series_corr(signal.rank(), labels.rank())
        row[f"forward_return_ic_{horizon}h"] = ic
        row[f"rank_ic_{horizon}h"] = rank_ic
        decay[str(horizon)] = ic
        aligned = pd.concat([signal.rename("signal"), labels.rename("forward")], axis=1).dropna()
        if not aligned.empty:
            signed = _series(aligned["signal"]).apply(_safe_sign) * _series(aligned["forward"]).apply(_safe_sign)
            row[f"hit_rate_by_signal_sign_{horizon}h"] = _finite_or_none((signed > 0).mean())
        if horizon == 24:
            regime_breakdown = _regime_breakdown(signal, labels, regime_labels)
    row["decay_profile"] = json.dumps(decay, sort_keys=True)
    row["regime_breakdown"] = json.dumps(regime_breakdown, sort_keys=True)
    if spec.uses_future_data or spec.lag_bars < 1:
        row["leakage_check_status"] = "failed"
        row["failure_reason"] = "future_data_or_unlagged_spec"
    if len(non_null) < 30:
        row["failure_reason"] = row["failure_reason"] or "insufficient_non_null_observations"
    return row


def _dag_null_comparison_rows(
    *,
    signals: dict[str, pd.Series],
    specs: list[SignalDagSpec],
    forward_returns: dict[int, pd.Series],
    seed: int,
    clusters: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    specs_by_family: dict[str, list[SignalDagSpec]] = {}
    for spec in specs:
        specs_by_family.setdefault(spec.signal_family, []).append(spec)
    representative_ids = sorted({str(row["representative_signal_id"]) for row in clusters})
    rows: list[dict[str, Any]] = []
    for family, family_specs in sorted(specs_by_family.items()):
        family_signal = _mean_signal([signals[spec.signal_id] for spec in family_specs])
        rows.extend(_dag_baseline_rows_for_signal(family_signal, family=family, signal_id="family_mean", level="family", forward_returns=forward_returns, seed=seed))
    for signal_id in representative_ids[: min(len(representative_ids), 200)]:
        spec = next(item for item in specs if item.signal_id == signal_id)
        rows.extend(
            _dag_baseline_rows_for_signal(
                signals[signal_id],
                family=spec.signal_family,
                signal_id=signal_id,
                level="cluster_representative",
                forward_returns=forward_returns,
                seed=seed,
            )
        )
    return rows


def _dag_baseline_rows_for_signal(
    signal: pd.Series,
    *,
    family: str,
    signal_id: str,
    level: str,
    forward_returns: dict[int, pd.Series],
    seed: int,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for horizon, labels in forward_returns.items():
        signal_ic = _series_corr(signal, labels)
        random_signal, random_seed = deterministic_random_sign_signal(signal, f"dag-{family}-{signal_id}-{horizon}", seed_base=seed)
        shuffled, shuffled_seed = shuffled_timestamp_signal(signal, f"dag-{family}-{signal_id}-{horizon}", seed_base=seed)
        family_permuted, permuted_seed = shuffled_timestamp_signal(signal, f"family-permuted-{family}-{horizon}", seed_base=seed + 17)
        candidates: list[tuple[str, pd.Series | None, str, int | None, bool]] = [
            ("zero_signal_cash_null", pd.Series(0.0, index=signal.index, dtype=float), "available", None, False),
            ("buy_and_hold_same_window", None, "unavailable_constant_position_no_signal_ic", None, False),
            ("random_sign_signal", random_signal, "available", random_seed, False),
            ("lagged_signal", lagged_signal_position(signal, lag_bars=max(2, DEFAULT_DECISION_LAG_BARS + horizon // 8)), "available", None, False),
            ("inverse_signal", inverse_signal_position(signal), "available", None, False),
            ("shuffled_timestamp_signal", shuffled, "available", shuffled_seed, False),
            ("family_permuted_signal", family_permuted, "available", permuted_seed, False),
            (
                "frozen_tsmom_negative_control",
                None,
                "available_summary_only" if DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT.exists() else "unavailable_missing_artifact",
                None,
                False,
            ),
        ]
        for baseline_name, baseline_signal, status, deterministic_seed_value, uses_current_bar in candidates:
            baseline_ic = _series_corr(baseline_signal, labels) if baseline_signal is not None else None
            rows.append(
                {
                    "comparison_level": level,
                    "signal_family": family,
                    "signal_id": signal_id,
                    "baseline_name": baseline_name,
                    "horizon_hours": horizon,
                    "signal_ic": signal_ic,
                    "baseline_ic": baseline_ic,
                    "signal_beats_baseline": _beats_baseline(signal_ic, baseline_ic),
                    "baseline_status": status,
                    "deterministic_seed": deterministic_seed_value,
                    "baseline_uses_current_bar_signal": uses_current_bar,
                    "sample_size": int(len(pd.concat([signal, labels], axis=1).dropna())),
                    "holdout_retune_used": False,
                    "forward_return_label_overlap": False,
                    "claim_boundary": SIGNAL_DAG_CLAIM_BOUNDARY,
                }
            )
    return rows


def _dag_family_summary_rows(diagnostics: list[dict[str, Any]], null_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    frame = pd.DataFrame(diagnostics)
    rows: list[dict[str, Any]] = []
    for family, group in frame.groupby("signal_family", sort=True):
        group_frame = _frame(group)
        duplicate_count = int((~_series(group_frame["effective_independent_signal"]).fillna(False).astype(bool)).sum())
        ic_values = _family_ic_values(group_frame)
        best_raw_ic = max((abs(item) for item in ic_values), default=0.0)
        median_ic = _finite_or_none(pd.Series(ic_values, dtype=float).median()) if ic_values else None
        null_family = [row for row in null_rows if row["signal_family"] == family and row["comparison_level"] == "family"]
        null_beat_rows = [row for row in null_family if row["signal_beats_baseline"] is True]
        rows.append(
            {
                "signal_family": str(family),
                "raw_signal_count": int(len(group_frame)),
                "duplicate_signal_count": duplicate_count,
                "duplicate_ratio": _safe_div(duplicate_count, len(group_frame)),
                "effective_signal_breadth": int(len(group_frame) - duplicate_count),
                "family_level_best_raw_ic": _finite_or_none(best_raw_ic),
                "family_level_median_ic": median_ic,
                "family_level_hit_rate": _finite_or_none(_series(group_frame["hit_rate_by_signal_sign_24h"]).dropna().mean()),
                "null_baselines_beaten_count": len(null_beat_rows),
                "null_baseline_comparison_count": len(null_family),
                "leakage_fail_count": int((_series(group_frame["leakage_check_status"]) != "passed_shifted_by_decision_lag").sum()),
                "cannot_claim_edge_due_to_multiple_testing": True,
                "claim_boundary": SIGNAL_DAG_CLAIM_BOUNDARY,
            }
        )
    return rows


def _multiple_testing_summary_rows(
    diagnostics: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
    null_rows: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    total_generated = len(diagnostics)
    total_null = len(null_rows)
    effective = len({row["duplicate_cluster_id"] for row in clusters if not bool(row.get("is_redundant_duplicate"))})
    rows: list[dict[str, Any]] = [
        {
            "scope": "overall",
            "total_generated_signal_count": total_generated,
            "total_evaluated_signal_count": total_generated,
            "total_null_comparisons": total_null,
            "family_level_best_raw_ic": max((float(row["family_level_best_raw_ic"] or 0.0) for row in family_summary), default=0.0),
            "family_level_median_ic": _finite_or_none(pd.Series([row["family_level_median_ic"] for row in family_summary], dtype=float).median()) if family_summary else None,
            "family_level_hit_rate": _finite_or_none(pd.Series([row["family_level_hit_rate"] for row in family_summary], dtype=float).mean()) if family_summary else None,
            "duplicate_adjusted_signal_count": effective,
            "effective_signal_breadth": effective,
            "approximate_false_discovery_warning": "many generated DAG hypotheses inflate false-discovery risk",
            "selection_bias_warning": "shortlist is diagnostic only and must not be treated as selected alpha",
            "cannot_claim_edge_due_to_multiple_testing": True,
            "claim_boundary": SIGNAL_DAG_CLAIM_BOUNDARY,
        }
    ]
    for row in family_summary:
        rows.append(
            {
                "scope": str(row["signal_family"]),
                "total_generated_signal_count": row["raw_signal_count"],
                "total_evaluated_signal_count": row["raw_signal_count"],
                "total_null_comparisons": row["null_baseline_comparison_count"],
                "family_level_best_raw_ic": row["family_level_best_raw_ic"],
                "family_level_median_ic": row["family_level_median_ic"],
                "family_level_hit_rate": row["family_level_hit_rate"],
                "duplicate_adjusted_signal_count": row["effective_signal_breadth"],
                "effective_signal_breadth": row["effective_signal_breadth"],
                "approximate_false_discovery_warning": "family-level apparent IC is unvalidated due to multiple testing",
                "selection_bias_warning": "representatives are chosen by deterministic complexity rule, not performance",
                "cannot_claim_edge_due_to_multiple_testing": True,
                "claim_boundary": SIGNAL_DAG_CLAIM_BOUNDARY,
            }
        )
    return rows


def _dag_shortlist_rows(family_summary: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for row in family_summary:
        duplicate_ratio = float(row["duplicate_ratio"])
        best_ic = abs(float(row["family_level_best_raw_ic"] or 0.0))
        if int(row["leakage_fail_count"]) > 0:
            category: DAGSignalDecision = "KILL_SIGNAL"
            rationale = "leakage check failed"
        elif duplicate_ratio >= 0.8:
            category = "DUPLICATE_SIGNAL"
            rationale = "duplicates dominate family breadth"
        elif best_ic >= 0.05 and int(row["effective_signal_breadth"]) >= 3:
            category = "NEEDS_OOS_FALSIFICATION"
            rationale = "diagnostic IC only; requires separate pre-registered paper-only OOS falsification"
        elif best_ic >= 0.03:
            category = "WATCH_DIAGNOSTIC_ONLY"
            rationale = "weak diagnostic signal family only; no edge claim"
        else:
            category = "KILL_SIGNAL"
            rationale = "fails null-robustness as a diagnostic family"
        rows.append(
            {
                "signal_family": row["signal_family"],
                "shortlist_category": category,
                "family_level_best_raw_ic": row["family_level_best_raw_ic"],
                "effective_signal_breadth": row["effective_signal_breadth"],
                "duplicate_ratio": row["duplicate_ratio"],
                "rationale": rationale,
                "is_strategy": False,
                "is_readiness": False,
                "claim_boundary": SIGNAL_DAG_CLAIM_BOUNDARY,
            }
        )
    for forbidden in FORBIDDEN_OUTPUT_LABELS:
        if any(item["shortlist_category"] == forbidden for item in rows):
            raise ValueError(f"forbidden shortlist output emitted: {forbidden}")
    return rows


def _dag_failed_gate_rows(
    *,
    diagnostics: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
    multiple_testing: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    leakage_fail = [row for row in diagnostics if row["leakage_check_status"] != "passed_shifted_by_decision_lag"]
    if leakage_fail:
        rows.append({"gate": "leakage_check_status", "status": "failed", "reason": f"{len(leakage_fail)} DAG signals failed leakage checks"})
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    if duplicate_count:
        rows.append({"gate": "duplicate_correlation_control", "status": "watch", "reason": f"{duplicate_count} redundant DAG signal variants marked"})
    if any(bool(row.get("cannot_claim_edge_due_to_multiple_testing")) for row in multiple_testing):
        rows.append({"gate": "multiple_testing_edge_claim", "status": "unclaimed", "reason": "combinatorial DAG scan cannot claim edge without separate OOS protocol"})
    if all(float(row["family_level_best_raw_ic"] or 0.0) < 0.03 for row in family_summary):
        rows.append({"gate": "null_robustness", "status": "unclaimed", "reason": "no family has even diagnostic IC above watch threshold"})
    rows.append({"gate": "readiness_boundary", "status": "passed", "reason": "readiness_baseline_verdict remains NOT_READY"})
    rows.append({"gate": "tsmom_state_unchanged", "status": "passed", "reason": "TSMOM artifacts are not modified or restored"})
    return rows


def _family_ic_values(frame: pd.DataFrame) -> list[float]:
    values: list[float] = []
    for column in ("forward_return_ic_8h", "forward_return_ic_24h", "forward_return_ic_72h"):
        values.extend(float(item) for item in _series(pd.to_numeric(frame[column], errors="coerce")).dropna())
    return values


def _dag_cluster_representative(cluster: set[str], spec_by_id: dict[str, SignalDagSpec]) -> str:
    return sorted(cluster, key=lambda signal_id: (spec_by_id[signal_id].complexity_score, spec_by_id[signal_id].depth, signal_id))[0]


def _operator_families_used(specs: list[SignalDagSpec]) -> list[str]:
    families: set[str] = set()
    for spec in specs:
        for operator in spec.operator_sequence:
            families.add(_operator_node_type(operator))
    return sorted(families)
