# pyright: reportAttributeAccessIssue=false
from __future__ import annotations

import json
import math
import random
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterable, Literal, cast

import numpy as np
import pandas as pd

from parangsae.marketdata.binance_funding import FundingRateBundle
from parangsae.marketdata.binance_vision import MarketDataBundle
from parangsae.research.alpha_signal_factory import (
    DEFAULT_ALPHA_SIGNAL_DAG_V2_ARTIFACT,
    DEFAULT_ALPHA_SIGNAL_OOS_FDR_V3_ARTIFACT,
    DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT,
    DUPLICATE_CORRELATION_THRESHOLD,
    FORWARD_HORIZONS,
    READINESS_BASELINE_VERDICT,
)
from parangsae.research.loop_artifacts import prepare_loop_output_dir
from parangsae.research.null_baselines import (
    deterministic_random_seed,
    inverse_signal_position,
    lagged_signal_position,
)

MULTI_ASSET_ALPHA_SCHEMA_VERSION = 5
MULTI_ASSET_ALPHA_LOOP_SLUG = "multi-asset-alpha-generator-strict-prefilter-v5"
MULTI_ASSET_ALPHA_NAMESPACE = "multi_asset_alpha_generator_strict_prefilter_v5_only_not_readiness"
DEFAULT_MULTI_ASSET_ALPHA_V4_ARTIFACT = Path(
    "reports/loops/20260627-alpha-signal-strict-falsification-protocol-v4"
)
DEFAULT_MULTI_ASSET_GENERATION_SEED = 20260629
DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET = 5000
HARD_MULTI_ASSET_CANDIDATE_BUDGET = 20000
DEFAULT_MULTI_ASSET_MAX_DEPTH = 4
DEFAULT_MULTI_ASSET_DECISION_LAG_BARS = 1
DEFAULT_MIN_BARS_PER_SYMBOL = 96
DEFAULT_SYMBOL_UNIVERSE = (
    "BTCUSDT",
    "ETHUSDT",
    "SOLUSDT",
    "BNBUSDT",
    "XRPUSDT",
    "ADAUSDT",
    "DOGEUSDT",
    "AVAXUSDT",
    "LINKUSDT",
    "TONUSDT",
)
MULTI_ASSET_SIGNAL_FAMILIES = (
    "cross_sectional_momentum",
    "cross_sectional_reversal",
    "funding_dispersion",
    "lead_lag",
    "beta_residual",
    "volatility_dispersion",
    "liquidity_volume_shock",
    "correlation_breakdown",
    "carry_x_trend",
    "relative_value_interaction",
)
MULTI_ASSET_NULL_BASELINES_USED = (
    "zero_cash_null",
    "buy_and_hold_same_window_baseline",
    "deterministic_random_sign",
    "lagged_signal",
    "inverse_signal",
    "shuffled_timestamp_signal",
    "family_permuted_signal",
    "symbol_permuted_signal",
    "universe_median_signal",
    "frozen_tsmom_negative_control",
)
MULTI_ASSET_ALLOWED_SIGNAL_LABELS = (
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEEDS_STRICT_OOS_FALSIFICATION",
    "NEGATIVE_CONTROL_CANDIDATE",
)
MULTI_ASSET_ALLOWED_SUITE_DECISIONS = (
    "MULTI_ASSET_SIGNAL_FACTORY_INFRA_ONLY",
    "NO_SURVIVING_SIGNAL_FAMILY",
    "DUPLICATES_DOMINATE",
    "FAILS_NULL_ROBUSTNESS",
    "NEEDS_STRICT_OOS_FALSIFICATION",
    "WATCH_FAMILY_ONLY_PAPER_RESEARCH",
)
MULTI_ASSET_FORBIDDEN_OUTPUT_LABELS = {
    "READY",
    "TRADE",
    "LIVE_READY",
    "PROFITABLE",
    "DEPLOY",
    "CAPITAL_ALLOCATE",
    "EDGE_CONFIRMED",
}
MULTI_ASSET_MULTIPLE_TESTING_BOUNDARY = (
    "V5 generates a large public multi-asset signal hypothesis universe. Every metric is "
    "diagnostic and exposed to multiple-testing, data-snooping, duplicate-family, and "
    "survivorship risk. V5 may only create paper-only next falsification candidates; it "
    "cannot claim alpha, edge, profitability, readiness, execution readiness, or permission "
    "to trade."
)
MULTI_ASSET_LEAKAGE_BOUNDARY = (
    "V5 panel features use public OHLCV/funding values available at or before each timestamp. "
    "Cross-sectional ranks use only symbols with actual data at the same timestamp, rolling "
    "features are historical, and every signal is shifted by at least one decision lag before "
    "forward-return label comparison."
)
MULTI_ASSET_CLAIM_BOUNDARY = (
    "MULTI_ASSET_ALPHA_GENERATOR_STRICT_PREFILTER_V5 is paper-only multi-asset signal "
    "factory and strict diagnostic prefilter infrastructure. Generated signals are "
    "hypotheses, not edges, not a trading strategy, not portfolio construction, not signal "
    "weight optimization, not profitability evidence, not readiness or live readiness, and "
    "not permission for live/testnet/account/API-key/order/fill use."
)
MULTI_ASSET_V4_LINEAGE_BOUNDARY = (
    "V4 artifacts are lineage/audit inputs only. V5 records the source V4 path and queue "
    "emptiness but never seeds candidates from V4 survivors, V4 family queues, V3/V4 "
    "representatives, V4 horizons, V4 thresholds, or V4 evidence."
)
MULTI_ASSET_REQUIRED_FILES = (
    "multi_asset_alpha_factory.json",
    "multi_asset_panel_summary.csv",
    "signal_catalog.jsonl",
    "signal_diagnostics.csv",
    "signal_family_summary.csv",
    "signal_correlation_clusters.csv",
    "oos_prefilter_summary.csv",
    "null_baseline_comparison.csv",
    "multiple_testing_summary.csv",
    "shortlist.csv",
    "failed_gate_rollup.csv",
    "next_strict_falsification_queue.csv",
    "report.md",
    "commands.txt",
    "changed_files.txt",
)
MULTI_ASSET_KNOWN_FILES = set(MULTI_ASSET_REQUIRED_FILES)
MULTI_ASSET_NEXT_QUEUE_COLUMNS = (
    "signal_family",
    "queue_decision",
    "paper_only",
    "effective_signal_breadth",
    "corresponding_oos_metric",
    "reason",
    "claim_boundary",
)
V4_QUEUE_REQUIRED_COLUMNS = {"signal_family", "horizon_bars", "queue_decision"}
PanelIndex = pd.MultiIndex
SignalLabel = Literal[
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEEDS_STRICT_OOS_FALSIFICATION",
    "NEGATIVE_CONTROL_CANDIDATE",
]


@dataclass(frozen=True)
class MultiAssetSignalSpec:
    signal_id: str
    signal_family: str
    feature_name: str
    symbol_scope: tuple[str, ...]
    pair_scope: tuple[str, str] | None
    depth: int
    max_depth: int
    complexity_score: float
    lag_bars: int
    uses_funding: bool
    uses_future_data: bool
    generation_seed: int
    generation_rule_id: str
    claim_boundary: str = MULTI_ASSET_CLAIM_BOUNDARY

    def __post_init__(self) -> None:
        if self.lag_bars < 1:
            raise ValueError("MultiAssetSignalSpec.lag_bars must be at least 1")
        if self.max_depth < 1 or self.max_depth > DEFAULT_MULTI_ASSET_MAX_DEPTH:
            raise ValueError("MultiAssetSignalSpec.max_depth must be between 1 and 4")
        if self.depth < 1 or self.depth > self.max_depth:
            raise ValueError("MultiAssetSignalSpec.depth must be between 1 and max_depth")
        if self.uses_future_data:
            raise ValueError("MultiAssetSignalSpec may not use future data")
        if self.signal_family not in MULTI_ASSET_SIGNAL_FAMILIES:
            raise ValueError(f"unknown V5 signal family: {self.signal_family}")

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> MultiAssetSignalSpec:
        pair = payload.get("pair_scope")
        pair_scope = tuple(str(item) for item in pair) if pair else None
        if pair_scope is not None and len(pair_scope) != 2:
            raise ValueError("pair_scope must contain exactly two symbols")
        return cls(
            signal_id=str(payload["signal_id"]),
            signal_family=str(payload["signal_family"]),
            feature_name=str(payload["feature_name"]),
            symbol_scope=tuple(str(item) for item in payload["symbol_scope"]),
            pair_scope=cast(tuple[str, str] | None, pair_scope),
            depth=int(payload["depth"]),
            max_depth=int(payload["max_depth"]),
            complexity_score=float(payload["complexity_score"]),
            lag_bars=int(payload["lag_bars"]),
            uses_funding=bool(payload["uses_funding"]),
            uses_future_data=bool(payload["uses_future_data"]),
            generation_seed=int(payload["generation_seed"]),
            generation_rule_id=str(payload["generation_rule_id"]),
            claim_boundary=str(payload.get("claim_boundary", MULTI_ASSET_CLAIM_BOUNDARY)),
        )


@dataclass(frozen=True)
class MultiAssetAlphaPrefilterResult:
    payload: dict[str, Any]
    panel_summary: list[dict[str, Any]]
    catalog: list[dict[str, Any]]
    diagnostics: list[dict[str, Any]]
    family_summary: list[dict[str, Any]]
    clusters: list[dict[str, Any]]
    oos_prefilter_summary: list[dict[str, Any]]
    null_baseline_comparison: list[dict[str, Any]]
    multiple_testing_summary: list[dict[str, Any]]
    shortlist: list[dict[str, Any]]
    failed_gates: list[dict[str, Any]]
    next_strict_falsification_queue: list[dict[str, Any]]


def default_multi_asset_alpha_prefilter_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{MULTI_ASSET_ALPHA_LOOP_SLUG}"


def normalize_symbol_universe(symbols: Iterable[str]) -> tuple[str, ...]:
    seen: set[str] = set()
    normalized: list[str] = []
    for raw in symbols:
        symbol = str(raw).strip().upper()
        if not symbol or symbol in seen:
            continue
        seen.add(symbol)
        normalized.append(symbol)
    return tuple(normalized)


def parse_symbol_universe(raw: str | Iterable[str] | None) -> tuple[str, ...]:
    if raw is None:
        return DEFAULT_SYMBOL_UNIVERSE
    if isinstance(raw, str):
        return normalize_symbol_universe(raw.split(","))
    return normalize_symbol_universe(raw)


def build_multi_asset_panel(
    market_bundles: dict[str, MarketDataBundle],
    *,
    funding_bundles: dict[str, FundingRateBundle | None] | None = None,
    funding_status_by_symbol: dict[str, str] | None = None,
) -> tuple[pd.DataFrame, list[dict[str, Any]]]:
    funding_bundles = funding_bundles or {}
    funding_status_by_symbol = funding_status_by_symbol or {}
    frames: list[pd.DataFrame] = []
    summary: list[dict[str, Any]] = []
    for symbol in sorted(market_bundles):
        bundle = market_bundles[symbol]
        ohlcv = _normalize_ohlcv_for_panel(bundle.frame)
        funding_bundle = funding_bundles.get(symbol)
        funding = _align_public_funding_to_ohlcv(
            ohlcv.index,
            funding_bundle.frame if funding_bundle is not None else None,
        )
        frame = pd.DataFrame(
            {
                "symbol": symbol,
                "timestamp": ohlcv.index,
                "open": _series(ohlcv["Open"]),
                "high": _series(ohlcv["High"]),
                "low": _series(ohlcv["Low"]),
                "close": _series(ohlcv["Close"]),
                "volume": _series(ohlcv["Volume"]),
                "funding_rate": funding,
            },
            index=ohlcv.index,
        )
        frames.append(frame)
        funding_status = funding_status_by_symbol.get(symbol, "available" if funding_bundle is not None else "unavailable")
        summary.append(
            {
                "symbol": symbol,
                "row_count": int(len(frame)),
                "first_timestamp": _timestamp(cast(pd.Timestamp, frame.index[0])),
                "last_timestamp": _timestamp(cast(pd.Timestamp, frame.index[-1])),
                "closed_bar_only": bool(bundle.quality.get("closed_bar_only", True)),
                "ohlcv_normalized_sha256": bundle.normalized_sha256,
                "funding_status": funding_status,
                "funding_available": bool(funding_bundle is not None and funding.notna().any()),
                "funding_non_null_count": int(funding.notna().sum()),
                "source": "public_binance_vision_ohlcv_and_optional_public_funding",
            }
        )
    if not frames:
        empty = pd.DataFrame(
            columns=[
                "symbol",
                "timestamp",
                "open",
                "high",
                "low",
                "close",
                "volume",
                "return",
                "log_return",
                "realized_volatility",
                "rolling_range",
                "volume_zscore",
                "funding_rate",
                "funding_change",
                "funding_zscore",
                "funding_rank",
                "cross_sectional_return_rank",
                "cross_sectional_volume_rank",
                "cross_sectional_volatility_rank",
            ]
        )
        return empty.set_index(["timestamp", "symbol"], drop=False), summary
    panel = pd.concat(frames, axis=0).sort_values(["timestamp", "symbol"]).reset_index(drop=True)
    panel = _add_panel_features(panel)
    panel = _frame(panel.set_index(["timestamp", "symbol"], drop=False).sort_index())
    return panel, summary


def generate_multi_asset_signal_specs(
    symbols: Iterable[str],
    *,
    seed: int = DEFAULT_MULTI_ASSET_GENERATION_SEED,
    candidate_budget: int = DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET,
    max_depth: int = DEFAULT_MULTI_ASSET_MAX_DEPTH,
    include_funding: bool = True,
) -> list[MultiAssetSignalSpec]:
    if candidate_budget <= 0 or candidate_budget > HARD_MULTI_ASSET_CANDIDATE_BUDGET:
        raise ValueError("candidate_budget must be between 1 and 20000")
    if max_depth < 1 or max_depth > DEFAULT_MULTI_ASSET_MAX_DEPTH:
        raise ValueError("max_depth must be between 1 and 4")
    universe = tuple(sorted(normalize_symbol_universe(symbols)))
    specs: list[MultiAssetSignalSpec] = []

    def add(
        *,
        family: str,
        feature: str,
        symbol_scope: tuple[str, ...],
        pair_scope: tuple[str, str] | None = None,
        depth: int = 1,
        complexity: float = 1.0,
        uses_funding: bool = False,
    ) -> None:
        if len(specs) >= candidate_budget or depth > max_depth:
            return
        if uses_funding and not include_funding:
            return
        scope_part = "-".join(symbol_scope) if symbol_scope else "universe"
        pair_part = "" if pair_scope is None else f".pair_{pair_scope[0]}_{pair_scope[1]}"
        signal_id = f"v5.{family}.{feature}.{scope_part}{pair_part}.d{depth}"
        specs.append(
            MultiAssetSignalSpec(
                signal_id=signal_id,
                signal_family=family,
                feature_name=feature,
                symbol_scope=symbol_scope,
                pair_scope=pair_scope,
                depth=depth,
                max_depth=max_depth,
                complexity_score=float(complexity + 0.1 * depth + (0.2 if pair_scope is not None else 0.0)),
                lag_bars=DEFAULT_MULTI_ASSET_DECISION_LAG_BARS,
                uses_funding=uses_funding,
                uses_future_data=False,
                generation_seed=deterministic_random_seed(signal_id, seed_base=seed),
                generation_rule_id=f"{family}:{feature}",
            )
        )

    per_symbol_templates: dict[str, tuple[tuple[str, int, float, bool], ...]] = {
        "cross_sectional_momentum": (
            ("return_rank", 1, 1.0, False),
            ("relative_strength_24h", 2, 1.5, False),
            ("rolling_return_rank_24h", 2, 1.4, False),
            ("momentum_spread_universe_median_72h", 2, 1.8, False),
        ),
        "cross_sectional_reversal": (
            ("short_horizon_loser_winner_reversal", 1, 1.0, False),
            ("zscore_deviation_from_universe_median", 2, 1.5, False),
            ("residual_reversal_proxy", 3, 2.0, False),
        ),
        "funding_dispersion": (
            ("high_funding_vs_universe_median", 1, 1.0, True),
            ("low_funding_vs_universe_median", 1, 1.0, True),
            ("funding_rank", 1, 1.0, True),
            ("funding_zscore", 2, 1.3, True),
            ("funding_change_rank", 2, 1.4, True),
        ),
        "lead_lag": (
            ("btc_return_leads_alt", 2, 1.8, False),
            ("eth_return_leads_alt", 2, 1.8, False),
            ("market_leader_lagged_return_interaction", 3, 2.2, False),
            ("leader_volatility_shock_interaction", 3, 2.4, False),
        ),
        "beta_residual": (
            ("return_minus_btc_beta_proxy", 3, 2.1, False),
            ("return_minus_universe_basket", 2, 1.5, False),
            ("residual_zscore", 3, 2.0, False),
            ("residual_reversal", 3, 2.1, False),
        ),
        "volatility_dispersion": (
            ("high_volatility_rank", 1, 1.0, False),
            ("low_volatility_rank", 1, 1.0, False),
            ("volatility_change_rank", 2, 1.4, False),
            ("volatility_compression_expansion", 2, 1.6, False),
        ),
        "liquidity_volume_shock": (
            ("volume_shock_rank", 1, 1.0, False),
            ("volume_x_return_direction", 2, 1.4, False),
            ("relative_volume_divergence", 2, 1.5, False),
        ),
        "correlation_breakdown": (
            ("rolling_correlation_to_btc", 2, 1.5, False),
            ("rolling_correlation_change", 3, 1.8, False),
            ("decorrelation_shock", 3, 2.0, False),
            ("recorrelation_proxy", 3, 2.0, False),
        ),
        "carry_x_trend": (
            ("funding_carry_gated_by_trend", 3, 2.0, True),
            ("funding_carry_gated_by_low_volatility", 3, 2.0, True),
            ("funding_extreme_gated_by_cross_sectional_rank", 4, 2.5, True),
        ),
    }
    pair_templates = (
        ("return_spread", 1, 1.2, False),
        ("funding_spread", 1, 1.2, True),
        ("volatility_spread", 1, 1.2, False),
        ("volume_spread", 1, 1.2, False),
        ("pair_residual_proxy", 3, 2.2, False),
    )
    for family in MULTI_ASSET_SIGNAL_FAMILIES:
        if family == "relative_value_interaction":
            for left, right in _ordered_pairs(universe):
                for feature, depth, complexity, uses_funding in pair_templates:
                    add(
                        family=family,
                        feature=feature,
                        symbol_scope=(left, right),
                        pair_scope=(left, right),
                        depth=depth,
                        complexity=complexity,
                        uses_funding=uses_funding,
                    )
        else:
            templates = per_symbol_templates[family]
            for symbol in universe:
                for feature, depth, complexity, uses_funding in templates:
                    add(
                        family=family,
                        feature=feature,
                        symbol_scope=(symbol,),
                        depth=depth,
                        complexity=complexity,
                        uses_funding=uses_funding,
                    )
    return specs[:candidate_budget]


def evaluate_multi_asset_alpha_prefilter(
    *,
    market_bundles: dict[str, MarketDataBundle],
    funding_bundles: dict[str, FundingRateBundle | None] | None = None,
    requested_symbols: Iterable[str] = DEFAULT_SYMBOL_UNIVERSE,
    excluded_symbols: Iterable[dict[str, Any]] = (),
    funding_status_by_symbol: dict[str, str] | None = None,
    source_v4_artifact_path: str | Path = DEFAULT_MULTI_ASSET_ALPHA_V4_ARTIFACT,
    seed: int = DEFAULT_MULTI_ASSET_GENERATION_SEED,
    candidate_budget: int = DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET,
    max_depth: int = DEFAULT_MULTI_ASSET_MAX_DEPTH,
    min_bars_per_symbol: int = DEFAULT_MIN_BARS_PER_SYMBOL,
    horizons: tuple[int, ...] = FORWARD_HORIZONS,
    fold_count: int = 3,
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> MultiAssetAlphaPrefilterResult:
    requested = parse_symbol_universe(requested_symbols)
    used_symbols = tuple(symbol for symbol in requested if symbol in market_bundles)
    source_v4_path = Path(source_v4_artifact_path)
    v4_queue_rows, v4_queue_audit = _read_v4_next_protocol_queue(source_v4_path)
    v4_queue_empty = bool(v4_queue_audit["v4_queue_empty"])
    panel, panel_summary = build_multi_asset_panel(
        {symbol: market_bundles[symbol] for symbol in used_symbols},
        funding_bundles=funding_bundles,
        funding_status_by_symbol=funding_status_by_symbol,
    )
    include_funding = bool(_series(panel["funding_rate"]).notna().any()) if (not panel.empty and "funding_rate" in panel.columns) else False
    specs = generate_multi_asset_signal_specs(
        used_symbols,
        seed=seed,
        candidate_budget=candidate_budget,
        max_depth=max_depth,
        include_funding=include_funding,
    )
    raw_signals: dict[str, pd.Series] = {}
    decision_signals: dict[str, pd.Series] = {}
    label_returns = _forward_return_labels(panel, horizons=horizons)
    folds = build_multi_asset_oos_folds(panel, horizons=horizons, fold_count=fold_count)
    diagnostics: list[dict[str, Any]] = []
    for spec in specs:
        raw_signal = build_multi_asset_signal_series(spec, panel)
        decision_signal = lag_multi_asset_signal_for_labels(raw_signal, lag_bars=spec.lag_bars)
        raw_signals[spec.signal_id] = raw_signal
        decision_signals[spec.signal_id] = decision_signal
        diagnostics.append(
            _multi_asset_signal_diagnostics(
                spec,
                raw_signal=raw_signal,
                decision_signal=decision_signal,
                forward_returns=label_returns,
                folds=folds,
                funding_available=include_funding,
            )
        )
    clusters = duplicate_correlation_clusters_for_multi_asset(decision_signals, specs)
    cluster_by_signal = {row["signal_id"]: row for row in clusters}
    for row in diagnostics:
        cluster = cluster_by_signal.get(str(row["signal_id"]), {})
        row["duplicate_cluster_id"] = cluster.get("duplicate_cluster_id")
        row["max_abs_correlation_to_existing_signal"] = cluster.get("max_abs_correlation_to_existing_signal")
        row["effective_independent_signal"] = not bool(cluster.get("is_redundant_duplicate", False))
        if bool(cluster.get("is_redundant_duplicate", False)) and row["failure_reason"] in (None, ""):
            row["failure_reason"] = "duplicate_correlation_control"
    oos_rows = _oos_prefilter_rows(
        signals=decision_signals,
        specs=specs,
        forward_returns=label_returns,
        folds=folds,
        clusters=clusters,
    )
    null_rows = _multi_asset_null_baseline_rows(
        signals=decision_signals,
        specs=specs,
        forward_returns=label_returns,
        seed=seed,
    )
    family_summary = _multi_asset_family_summary_rows(
        diagnostics,
        oos_rows,
        null_rows,
        used_symbols=used_symbols,
        funding_available=include_funding,
    )
    multiple_testing = _multi_asset_multiple_testing_rows(
        diagnostics=diagnostics,
        family_summary=family_summary,
        clusters=clusters,
        null_rows=null_rows,
        generated_signal_count=len(specs),
    )
    shortlist = _multi_asset_shortlist_rows(family_summary)
    if len(used_symbols) < 2:
        shortlist = [
            {
                **row,
                "signal_label": "KILL_SIGNAL",
                "shortlist_category": "KILL_SIGNAL",
                "rationale": "multi-asset minimum symbol count failed; single-symbol diagnostics cannot enter V5 strict queue",
            }
            for row in shortlist
        ]
    failed_gates = _multi_asset_failed_gate_rows(
        diagnostics=diagnostics,
        family_summary=family_summary,
        clusters=clusters,
        excluded_symbols=list(excluded_symbols),
        v4_queue_empty=v4_queue_empty,
        v4_queue_status=str(v4_queue_audit["v4_queue_status"]),
        v4_queue_reason=str(v4_queue_audit["v4_queue_reason"]),
        used_symbols=used_symbols,
    )
    next_queue = _next_strict_falsification_queue(shortlist, family_summary)
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    killed_count = sum(1 for row in shortlist if row["signal_label"] == "KILL_SIGNAL")
    watch_count = sum(1 for row in shortlist if row["signal_label"] in {"WATCH_FAMILY_ONLY", "WATCH_DIAGNOSTIC_ONLY", "NEEDS_STRICT_OOS_FALSIFICATION"})
    effective_breadth = len({row["duplicate_cluster_id"] for row in clusters if not bool(row.get("is_redundant_duplicate"))})
    duplicate_ratio = _safe_div(duplicate_count, len(clusters))
    suite_decision = _suite_decision(shortlist, duplicate_ratio)
    if suite_decision not in MULTI_ASSET_ALLOWED_SUITE_DECISIONS:
        raise ValueError(f"forbidden V5 suite decision emitted: {suite_decision}")
    payload = {
        "schema_version": MULTI_ASSET_ALPHA_SCHEMA_VERSION,
        "loop_id": f"{datetime.now(UTC).strftime('%Y%m%d')}-{MULTI_ASSET_ALPHA_LOOP_SLUG}",
        "loop_namespace": MULTI_ASSET_ALPHA_NAMESPACE,
        "loop_type": "validation_infrastructure",
        "status": "IMPLEMENTED",
        "suite_decision": suite_decision,
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "source_v4_artifact_path": str(source_v4_path),
        "v4_queue_empty": v4_queue_empty,
        "v4_queue_status": v4_queue_audit["v4_queue_status"],
        "v4_queue_reason": v4_queue_audit["v4_queue_reason"],
        "v4_queue_path": v4_queue_audit["v4_queue_path"],
        "v4_queue_row_count": len(v4_queue_rows),
        "v4_lineage_evidence_available": v4_queue_audit["v4_queue_status"] in {"present_empty", "present_non_empty"},
        "v4_families_revived": False,
        "v4_candidates_reused": False,
        "v4_lineage_boundary": MULTI_ASSET_V4_LINEAGE_BOUNDARY,
        "symbol_universe_requested": list(requested),
        "symbol_universe_used": list(used_symbols),
        "excluded_symbols": list(excluded_symbols),
        "universe_snapshot": {
            "universe_source": "fixed_requested_public_symbol_list",
            "requested_symbols": list(requested),
            "used_symbols": list(used_symbols),
            "excluded_symbols": list(excluded_symbols),
            "min_bars_per_symbol": int(min_bars_per_symbol),
            "timestamp_inclusion_policy": (
                "At timestamp t, cross-sectional ranks and medians use only symbols with an actual OHLCV row at t; "
                "missing symbols are excluded from that timestamp and never forward-filled."
            ),
        },
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
        "families_evaluated": list(MULTI_ASSET_SIGNAL_FAMILIES),
        "families_generated": sorted({spec.signal_family for spec in specs}),
        "horizons_evaluated": list(horizons),
        "null_baselines_used": list(MULTI_ASSET_NULL_BASELINES_USED),
        "multiple_testing_boundary": MULTI_ASSET_MULTIPLE_TESTING_BOUNDARY,
        "cannot_claim_edge_due_to_multiple_testing": True,
        "leakage_boundary": MULTI_ASSET_LEAKAGE_BOUNDARY,
        "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
        "safety_gates": _safety_gates(),
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "holdout_retune_used": False,
        "strategy_optimization_performed": False,
        "live_or_testnet_path_created": False,
        "tsmom_state_changed": False,
        "candidate_generation": {
            "deterministic_only": True,
            "used_forward_labels": False,
            "performance_based_generation": False,
            "max_dag_depth": int(max_depth),
            "generated_fewer_than_budget_reason": (
                None
                if len(specs) >= candidate_budget
                else "finite fixed V5 family grammar and available universe generated fewer candidates than requested budget"
            ),
        },
        "selection_boundary": {
            "no_holdout_retune": True,
            "no_best_horizon_cherry_picking": True,
            "representative_selection_rule": "lowest_complexity_then_depth_then_lexicographic_signal_id",
            "shortlist_is_strategy": False,
            "shortlist_is_readiness": False,
            "weight_optimization_performed": False,
        },
        "multiple_testing_summary": {
            "total_generated_signal_count": len(specs),
            "total_evaluated_signal_count": len(diagnostics),
            "total_signal_tests": int(len(diagnostics) * len(horizons)),
            "duplicate_adjusted_test_count": int(effective_breadth * len(horizons)),
            "effective_signal_breadth": int(effective_breadth),
            "approximate_false_discovery_warning": "high",
            "cannot_claim_edge_due_to_multiple_testing": True,
        },
        "source_artifacts": {
            "v2": str(DEFAULT_ALPHA_SIGNAL_DAG_V2_ARTIFACT),
            "v3": str(DEFAULT_ALPHA_SIGNAL_OOS_FDR_V3_ARTIFACT),
            "v4": str(source_v4_path),
            "tsmom_negative_control": str(DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT),
        },
        "summary": {
            "allowed_signal_labels": list(MULTI_ASSET_ALLOWED_SIGNAL_LABELS),
            "allowed_suite_decisions": list(MULTI_ASSET_ALLOWED_SUITE_DECISIONS),
            "cannot_claim_edge_due_to_multiple_testing": True,
            "paper_only_next_queue": True,
        },
        "validation_commands": list(commands),
        "changed_files": list(changed_files),
        "evidence_artifacts": list(MULTI_ASSET_REQUIRED_FILES),
        "next_loop": "Use next_strict_falsification_queue.csv only as input to a separate future paper-only strict falsification protocol.",
    }
    return MultiAssetAlphaPrefilterResult(
        payload=payload,
        panel_summary=panel_summary,
        catalog=[spec.to_dict() for spec in specs],
        diagnostics=diagnostics,
        family_summary=family_summary,
        clusters=clusters,
        oos_prefilter_summary=oos_rows,
        null_baseline_comparison=null_rows,
        multiple_testing_summary=multiple_testing,
        shortlist=shortlist,
        failed_gates=failed_gates,
        next_strict_falsification_queue=next_queue,
    )


def build_multi_asset_signal_series(spec: MultiAssetSignalSpec, panel: pd.DataFrame) -> pd.Series:
    if spec.uses_future_data:
        raise ValueError(f"V5 signal {spec.signal_id} uses future data")
    if panel.empty:
        return pd.Series(dtype=float)
    if spec.pair_scope is not None:
        return _pair_signal_series(spec, panel)
    if not spec.symbol_scope:
        return pd.Series(dtype=float)
    symbol = spec.symbol_scope[0]
    if symbol not in set(_series(panel["symbol"]).astype(str)):
        return pd.Series(dtype=float)
    frame = _symbol_panel(panel, symbol)
    feature = spec.feature_name
    mapping: dict[str, pd.Series] = {
        "return_rank": _series(frame["cross_sectional_return_rank"]),
        "relative_strength_24h": _series(frame["rolling_return_24h"] - frame["universe_rolling_return_24h_median"]),
        "rolling_return_rank_24h": _series(frame["cross_sectional_rolling_return_24h_rank"]),
        "momentum_spread_universe_median_72h": _series(frame["rolling_return_72h"] - frame["universe_rolling_return_72h_median"]),
        "short_horizon_loser_winner_reversal": -_series(frame["cross_sectional_rolling_return_8h_rank"]),
        "zscore_deviation_from_universe_median": -_safe_ratio(
            _series(frame["return"] - frame["universe_return_median"]),
            _series(frame["universe_return_std"]),
        ),
        "residual_reversal_proxy": -_series(frame["btc_residual_zscore"]),
        "high_funding_vs_universe_median": _series(frame["funding_rate"] - frame["universe_funding_median"]),
        "low_funding_vs_universe_median": -_series(frame["funding_rate"] - frame["universe_funding_median"]),
        "funding_rank": _series(frame["funding_rank"]),
        "funding_zscore": _series(frame["funding_zscore"]),
        "funding_change_rank": _series(frame["funding_change_rank"]),
        "btc_return_leads_alt": _series(frame["btc_return_lag1"]),
        "eth_return_leads_alt": _series(frame["eth_return_lag1"]),
        "market_leader_lagged_return_interaction": _series(frame["market_leader_return_lag1"] * frame["cross_sectional_return_rank"]),
        "leader_volatility_shock_interaction": _series(frame["market_leader_volatility_shock"] * frame["cross_sectional_volatility_rank"]),
        "return_minus_btc_beta_proxy": _series(frame["btc_beta_residual"]),
        "return_minus_universe_basket": _series(frame["return"] - frame["universe_return_mean"]),
        "residual_zscore": _series(frame["btc_residual_zscore"]),
        "residual_reversal": -_series(frame["btc_residual_zscore"]),
        "high_volatility_rank": _series(frame["cross_sectional_volatility_rank"]),
        "low_volatility_rank": -_series(frame["cross_sectional_volatility_rank"]),
        "volatility_change_rank": _series(frame["volatility_change_rank"]),
        "volatility_compression_expansion": _series(frame["volatility_zscore"] - frame["rolling_range_zscore"]),
        "volume_shock_rank": _series(frame["cross_sectional_volume_rank"]),
        "volume_x_return_direction": _series(frame["volume_zscore"] * frame["return"].map(_safe_sign)),
        "relative_volume_divergence": _series(frame["volume_zscore"] - frame["universe_volume_zscore_median"]),
        "rolling_correlation_to_btc": _series(frame["corr_to_btc"]),
        "rolling_correlation_change": _series(frame["corr_to_btc_change"]),
        "decorrelation_shock": -_series(frame["corr_to_btc_zscore"]),
        "recorrelation_proxy": _series(frame["corr_to_btc_change"]),
        "funding_carry_gated_by_trend": _series(-frame["funding_rate"] * (frame["rolling_return_24h"].abs() > frame["universe_rolling_return_24h_abs_median"]).astype(float)),
        "funding_carry_gated_by_low_volatility": _series(-frame["funding_rate"] * (frame["realized_volatility"] <= frame["universe_volatility_median"]).astype(float)),
        "funding_extreme_gated_by_cross_sectional_rank": _series(frame["funding_zscore"] * frame["cross_sectional_return_rank"]),
    }
    if feature not in mapping:
        raise ValueError(f"unsupported V5 feature: {feature}")
    values = _series(mapping[feature]).replace([math.inf, -math.inf], math.nan)
    values.index = _multi_index_for_symbol(frame, symbol)
    return _series(pd.to_numeric(values, errors="coerce"))


def lag_multi_asset_signal_for_labels(signal: pd.Series, *, lag_bars: int = DEFAULT_MULTI_ASSET_DECISION_LAG_BARS) -> pd.Series:
    if lag_bars < 1:
        raise ValueError("lag_bars must be at least 1 to prevent same-bar label leakage")
    if isinstance(signal.index, pd.MultiIndex) and "symbol" in signal.index.names:
        return _series(signal.groupby(level="symbol", group_keys=False).shift(lag_bars))
    return _series(signal.shift(lag_bars))


def duplicate_correlation_clusters_for_multi_asset(
    signals: dict[str, pd.Series],
    specs: list[MultiAssetSignalSpec],
    *,
    threshold: float = DUPLICATE_CORRELATION_THRESHOLD,
) -> list[dict[str, Any]]:
    signal_ids = list(signals)
    if not signal_ids:
        return []
    spec_by_id = {spec.signal_id: spec for spec in specs}
    common_index = _union_signal_index(signals.values())
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
    clusters = _clusters_from_corr_matrix(corr_matrix, signal_ids, threshold)
    max_corr = {
        signal_id: float(corr_matrix[idx].max()) if bool(valid[idx]) else 0.0
        for idx, signal_id in enumerate(signal_ids)
    }
    rows: list[dict[str, Any]] = []
    cluster_id_by_signal: dict[str, str] = {}
    representative_by_cluster: dict[str, str] = {}
    for idx, cluster in enumerate(clusters, start=1):
        cluster_id = f"MC{idx:04d}"
        representative = _multi_asset_cluster_representative(cluster, spec_by_id)
        representative_by_cluster[cluster_id] = representative
        for signal_id in cluster:
            cluster_id_by_signal[signal_id] = cluster_id
    for signal_id in signal_ids:
        cluster_id = cluster_id_by_signal[signal_id]
        members = sorted(item for item, cid in cluster_id_by_signal.items() if cid == cluster_id)
        spec = spec_by_id[signal_id]
        rows.append(
            {
                "signal_id": signal_id,
                "signal_family": spec.signal_family,
                "symbol_scope": ";".join(spec.symbol_scope),
                "pair_scope": "" if spec.pair_scope is None else ";".join(spec.pair_scope),
                "duplicate_cluster_id": cluster_id,
                "cluster_size": len(members),
                "cluster_members": ";".join(members),
                "representative_signal_id": representative_by_cluster[cluster_id],
                "representative_rule": "lowest_complexity_then_depth_then_lexicographic_signal_id",
                "is_redundant_duplicate": signal_id != representative_by_cluster[cluster_id],
                "max_abs_correlation_to_existing_signal": max_corr[signal_id],
                "duplicate_threshold": threshold,
                "complexity_score": spec.complexity_score,
                "depth": spec.depth,
                "correlation_engine": "vectorized_matrix_correlation_no_pairwise_pandas_corr_loop",
                "constant_or_all_nan_signal": not bool(valid[signal_ids.index(signal_id)]),
            }
        )
    return rows


def build_multi_asset_oos_folds(
    panel: pd.DataFrame,
    *,
    horizons: tuple[int, ...] = FORWARD_HORIZONS,
    fold_count: int = 3,
) -> list[dict[str, Any]]:
    if panel.empty:
        return []
    timestamps = pd.DatetimeIndex(sorted(pd.unique(_series(panel["timestamp"]))))
    if fold_count <= 0:
        raise ValueError("fold_count must be positive")
    max_horizon = max(int(item) for item in horizons)
    purge_bars = max_horizon
    embargo_bars = max_horizon
    segment = max(1, len(timestamps) // (fold_count + 3))
    rows: list[dict[str, Any]] = []
    for fold_idx in range(fold_count):
        train_start = 0
        train_end = segment * (fold_idx + 1)
        validation_start = train_end + purge_bars + embargo_bars
        validation_end = validation_start + segment
        test_start = validation_end + purge_bars + embargo_bars
        test_end = min(test_start + segment, len(timestamps) - max_horizon)
        if train_end <= train_start or validation_end <= validation_start or test_end <= test_start:
            continue
        rows.append(
            {
                "fold_id": f"fold_{fold_idx + 1:02d}",
                "train_start_pos": train_start,
                "train_end_pos": train_end,
                "validation_start_pos": validation_start,
                "validation_end_pos": validation_end,
                "test_start_pos": test_start,
                "test_end_pos": test_end,
                "train_start_timestamp": _timestamp(cast(pd.Timestamp, timestamps[train_start])),
                "train_end_timestamp": _timestamp(cast(pd.Timestamp, timestamps[train_end - 1])),
                "validation_start_timestamp": _timestamp(cast(pd.Timestamp, timestamps[validation_start])),
                "validation_end_timestamp": _timestamp(cast(pd.Timestamp, timestamps[validation_end - 1])),
                "test_start_timestamp": _timestamp(cast(pd.Timestamp, timestamps[test_start])),
                "test_end_timestamp": _timestamp(cast(pd.Timestamp, timestamps[test_end - 1])),
                "purge_bars": purge_bars,
                "embargo_bars": embargo_bars,
                "max_horizon_bars": max_horizon,
                "label_overlap_allowed": False,
                "holdout_retune_used": False,
                "no_best_horizon_cherry_picking": True,
            }
        )
    return rows


def prepare_multi_asset_alpha_prefilter_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="multi-asset-alpha-generator-strict-prefilter-run",
        known_files=MULTI_ASSET_KNOWN_FILES,
        overwrite=overwrite,
    )


def write_multi_asset_alpha_prefilter_artifacts(
    output_dir: str | Path,
    result: MultiAssetAlphaPrefilterResult,
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_multi_asset_alpha_prefilter_output_dir(output_dir, overwrite=overwrite)
    payload = dict(result.payload)
    if out.name.endswith(MULTI_ASSET_ALPHA_LOOP_SLUG):
        payload["loop_id"] = out.name
    _write_json(out / "multi_asset_alpha_factory.json", payload)
    pd.DataFrame(result.panel_summary).to_csv(out / "multi_asset_panel_summary.csv", index=False)
    _atomic_write_text(
        out / "signal_catalog.jsonl",
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in result.catalog),
    )
    pd.DataFrame(result.diagnostics).to_csv(out / "signal_diagnostics.csv", index=False)
    pd.DataFrame(result.family_summary).to_csv(out / "signal_family_summary.csv", index=False)
    pd.DataFrame(result.clusters).to_csv(out / "signal_correlation_clusters.csv", index=False)
    pd.DataFrame(result.oos_prefilter_summary).to_csv(out / "oos_prefilter_summary.csv", index=False)
    pd.DataFrame(result.null_baseline_comparison).to_csv(out / "null_baseline_comparison.csv", index=False)
    pd.DataFrame(result.multiple_testing_summary).to_csv(out / "multiple_testing_summary.csv", index=False)
    pd.DataFrame(result.shortlist).to_csv(out / "shortlist.csv", index=False)
    pd.DataFrame(result.failed_gates).to_csv(out / "failed_gate_rollup.csv", index=False)
    _next_queue_frame(result.next_strict_falsification_queue).to_csv(out / "next_strict_falsification_queue.csv", index=False)
    _atomic_write_text(out / "report.md", render_multi_asset_alpha_prefilter_report(payload, result))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    return out


def _next_queue_frame(rows: list[dict[str, Any]]) -> pd.DataFrame:
    frame = pd.DataFrame(rows)
    if frame.empty:
        return pd.DataFrame(columns=list(MULTI_ASSET_NEXT_QUEUE_COLUMNS))
    for column in MULTI_ASSET_NEXT_QUEUE_COLUMNS:
        if column not in frame.columns:
            frame[column] = None
    return frame.loc[:, list(MULTI_ASSET_NEXT_QUEUE_COLUMNS)]


def render_multi_asset_alpha_prefilter_report(
    payload: dict[str, Any],
    result: MultiAssetAlphaPrefilterResult,
) -> str:
    failed = result.failed_gates[:30]
    shortlist = [row for row in result.shortlist if row["signal_label"] in {"WATCH_FAMILY_ONLY", "WATCH_DIAGNOSTIC_ONLY", "NEEDS_STRICT_OOS_FALSIFICATION"}]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Purpose",
        "Generate a public multi-asset cross-sectional / relative-value / lead-lag / funding-dispersion signal universe and immediately apply a strict paper-only diagnostic prefilter.",
        "",
        "## Claim boundary",
        f"- suite_decision: {payload['suite_decision']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- claim_boundary: {payload['claim_boundary']}",
        f"- multiple_testing_boundary: {payload['multiple_testing_boundary']}",
        f"- v4_lineage_boundary: {payload['v4_lineage_boundary']}",
        "- Generated signals are hypotheses, not edges, strategies, profitability evidence, readiness, or trade permission.",
        "",
        "## Safety boundary",
        f"- no_live_orders_submitted: {payload['no_live_orders_submitted']}",
        f"- no_testnet_orders_submitted: {payload['no_testnet_orders_submitted']}",
        f"- no_account_data_used: {payload['no_account_data_used']}",
        f"- no_api_key_used: {payload['no_api_key_used']}",
        f"- no_private_order_or_fill_data_used: {payload['no_private_order_or_fill_data_used']}",
        f"- holdout_retune_used: {payload['holdout_retune_used']}",
        f"- strategy_optimization_performed: {payload['strategy_optimization_performed']}",
        f"- live_or_testnet_path_created: {payload['live_or_testnet_path_created']}",
        f"- tsmom_state_changed: {payload['tsmom_state_changed']}",
        "",
        "## V4 state",
        f"- source_v4_artifact_path: {payload['source_v4_artifact_path']}",
        f"- v4_queue_empty: {payload['v4_queue_empty']}",
        f"- v4_families_revived: {payload['v4_families_revived']}",
        f"- v4_candidates_reused: {payload['v4_candidates_reused']}",
        "",
        "## Universe",
        f"- requested: {', '.join(payload['symbol_universe_requested']) or 'none'}",
        f"- used: {', '.join(payload['symbol_universe_used']) or 'none'}",
        f"- excluded_count: {len(payload['excluded_symbols'])}",
        "",
        "## Generated signals",
        f"- generation_seed: {payload['generation_seed']}",
        f"- candidate_budget: {payload['candidate_budget']}",
        f"- max_depth: {payload['max_depth']}",
        f"- generated_signal_count: {payload['generated_signal_count']}",
        f"- evaluated_signal_count: {payload['evaluated_signal_count']}",
        f"- families_evaluated: {', '.join(payload['families_evaluated'])}",
        f"- horizons_evaluated: {', '.join(str(item) for item in payload['horizons_evaluated'])}",
        "",
        "## Duplicate and multiple-testing controls",
        f"- duplicate_signal_count: {payload['duplicate_signal_count']}",
        f"- duplicate_ratio: {payload['duplicate_ratio']}",
        f"- effective_signal_breadth: {payload['effective_signal_breadth']}",
        f"- cannot_claim_edge_due_to_multiple_testing: {payload['multiple_testing_summary']['cannot_claim_edge_due_to_multiple_testing']}",
        "",
        "## Shortlisted families",
    ]
    if shortlist:
        for row in shortlist[:15]:
            lines.append(f"- {row['signal_family']}: {row['signal_label']} ({row['rationale']})")
    else:
        lines.append("- none")
    lines.extend(["", "## Failed gates / warnings"])
    if failed:
        for row in failed:
            lines.append(f"- {row['gate']}: {row['status']} ({row['reason']})")
    else:
        lines.append("- none")
    lines.extend(
        [
            "",
            "## What cannot be claimed",
            "- alpha or edge confirmed",
            "- profitability",
            "- live readiness or execution readiness",
            "- permission to trade, allocate capital, or use live/testnet/account data",
            "- restoration of V4-falsified single-symbol candidate families",
            "",
            "## Next loop",
            payload["next_loop"],
            "",
        ]
    )
    return "\n".join(lines)


def _add_panel_features(panel: pd.DataFrame) -> pd.DataFrame:
    data = panel.copy()
    group = data.groupby("symbol", sort=False, group_keys=False)
    data["return"] = group["close"].pct_change()
    data["log_return"] = group["close"].transform(lambda s: _safe_log(_series(s).astype(float) / _series(s).astype(float).shift(1)))
    data["realized_volatility"] = group["return"].transform(lambda s: _series(s).rolling(24, min_periods=8).std())
    data["range_ratio"] = _safe_ratio(_series(data["high"] - data["low"]), _series(data["close"]))
    data["rolling_range"] = data.groupby("symbol", sort=False)["range_ratio"].transform(
        lambda s: _series(s).rolling(24, min_periods=8).mean()
    )
    data["volume_zscore"] = group["volume"].transform(lambda s: rolling_zscore(_series(s).astype(float), window=24, min_periods=8))
    data["funding_change"] = group["funding_rate"].diff()
    data["funding_zscore"] = group["funding_rate"].transform(lambda s: rolling_zscore(_series(s).astype(float), window=24, min_periods=6))
    for window in (8, 24, 72):
        data[f"rolling_return_{window}h"] = group["close"].transform(lambda s, w=window: _series(s).astype(float) / _series(s).astype(float).shift(w) - 1.0)
    data["volatility_change"] = group["realized_volatility"].diff()
    data["volatility_zscore"] = group["realized_volatility"].transform(lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12))
    data["rolling_range_zscore"] = group["rolling_range"].transform(lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12))
    for source, target in (
        ("return", "cross_sectional_return_rank"),
        ("volume_zscore", "cross_sectional_volume_rank"),
        ("realized_volatility", "cross_sectional_volatility_rank"),
        ("funding_rate", "funding_rank"),
        ("funding_change", "funding_change_rank"),
        ("rolling_return_8h", "cross_sectional_rolling_return_8h_rank"),
        ("rolling_return_24h", "cross_sectional_rolling_return_24h_rank"),
        ("rolling_return_72h", "cross_sectional_rolling_return_72h_rank"),
        ("volatility_change", "volatility_change_rank"),
    ):
        data[target] = _cross_sectional_rank(data, source)
    data["universe_return_median"] = data.groupby("timestamp")["return"].transform("median")
    data["universe_return_mean"] = data.groupby("timestamp")["return"].transform("mean")
    data["universe_return_std"] = data.groupby("timestamp")["return"].transform("std")
    data["universe_volume_zscore_median"] = data.groupby("timestamp")["volume_zscore"].transform("median")
    data["universe_volatility_median"] = data.groupby("timestamp")["realized_volatility"].transform("median")
    data["universe_funding_median"] = data.groupby("timestamp")["funding_rate"].transform("median")
    for window in (24, 72):
        data[f"universe_rolling_return_{window}h_median"] = data.groupby("timestamp")[f"rolling_return_{window}h"].transform("median")
    data["universe_rolling_return_24h_abs_median"] = data.groupby("timestamp")["rolling_return_24h"].transform(lambda s: _series(s).abs().median())
    data = _add_leader_and_residual_features(data)
    return _frame(data)


def _add_leader_and_residual_features(data: pd.DataFrame) -> pd.DataFrame:
    out = data.copy()
    for leader in ("BTCUSDT", "ETHUSDT"):
        leader_frame = out[out["symbol"] == leader].set_index("timestamp")
        ret = _series(leader_frame["return"]) if not leader_frame.empty else pd.Series(dtype=float)
        vol = _series(leader_frame["realized_volatility"]) if not leader_frame.empty else pd.Series(dtype=float)
        out[f"{leader[:3].lower()}_return_lag1"] = _series(out["timestamp"]).map(_series(ret.shift(1)))
        out[f"{leader[:3].lower()}_volatility_lag1"] = _series(out["timestamp"]).map(_series(vol.shift(1)))
    btc_ret_by_ts = _series(out[out["symbol"] == "BTCUSDT"].set_index("timestamp")["return"]) if _series(out["symbol"] == "BTCUSDT").any() else pd.Series(dtype=float)
    btc_vol_by_ts = _series(out[out["symbol"] == "BTCUSDT"].set_index("timestamp")["realized_volatility"]) if _series(out["symbol"] == "BTCUSDT").any() else pd.Series(dtype=float)
    eth_ret_by_ts = _series(out[out["symbol"] == "ETHUSDT"].set_index("timestamp")["return"]) if _series(out["symbol"] == "ETHUSDT").any() else pd.Series(dtype=float)
    out["btc_return_lag1"] = _series(out["timestamp"]).map(_series(btc_ret_by_ts.shift(1)))
    out["eth_return_lag1"] = _series(out["timestamp"]).map(_series(eth_ret_by_ts.shift(1)))
    out["market_leader_return_lag1"] = _series(out["btc_return_lag1"]).where(_series(out["btc_return_lag1"]).notna(), _series(out["eth_return_lag1"]))
    out["market_leader_volatility_shock"] = _series(out["timestamp"]).map(_series(rolling_zscore(btc_vol_by_ts, window=72, min_periods=12).shift(1)))
    residuals: list[pd.Series] = []
    betas: list[pd.Series] = []
    corrs: list[pd.Series] = []
    for symbol, group in out.groupby("symbol", sort=False):
        gf = _frame(group.set_index("timestamp").sort_index())
        returns = _series(gf["return"])
        btc = _series(btc_ret_by_ts.reindex(gf.index))
        covariance = _series(returns.rolling(72, min_periods=24).cov(btc))
        variance = _series(btc.rolling(72, min_periods=24).var())
        beta = _safe_ratio(covariance, variance)
        residual = returns - beta * btc
        corr = _series(returns.rolling(72, min_periods=24).corr(btc))
        idx = pd.MultiIndex.from_arrays([gf.index, [symbol] * len(gf)], names=["timestamp", "symbol"])
        residuals.append(pd.Series(residual.to_numpy(dtype=float), index=idx))
        betas.append(pd.Series(beta.to_numpy(dtype=float), index=idx))
        corrs.append(pd.Series(corr.to_numpy(dtype=float), index=idx))
    multi_index = pd.MultiIndex.from_frame(_frame(out.loc[:, ["timestamp", "symbol"]]))
    residual_series = pd.concat(residuals).reindex(multi_index) if residuals else pd.Series(dtype=float)
    beta_series = pd.concat(betas).reindex(multi_index) if betas else pd.Series(dtype=float)
    corr_series = pd.concat(corrs).reindex(multi_index) if corrs else pd.Series(dtype=float)
    out["btc_beta_proxy"] = beta_series.to_numpy(dtype=float) if len(beta_series) else math.nan
    out["btc_beta_residual"] = residual_series.to_numpy(dtype=float) if len(residual_series) else math.nan
    out["btc_residual_zscore"] = (
        out.groupby("symbol")["btc_beta_residual"].transform(lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12))
    )
    out["corr_to_btc"] = corr_series.to_numpy(dtype=float) if len(corr_series) else math.nan
    out["corr_to_btc_change"] = out.groupby("symbol")["corr_to_btc"].diff()
    out["corr_to_btc_zscore"] = out.groupby("symbol")["corr_to_btc"].transform(lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12))
    return _frame(out)


def _pair_signal_series(spec: MultiAssetSignalSpec, panel: pd.DataFrame) -> pd.Series:
    if spec.pair_scope is None:
        return pd.Series(dtype=float)
    left, right = spec.pair_scope
    left_frame = _symbol_panel(panel, left)
    right_frame = _symbol_panel(panel, right)
    if left_frame.empty or right_frame.empty:
        return pd.Series(dtype=float)
    joined = left_frame.join(
        right_frame,
        how="inner",
        lsuffix="_left",
        rsuffix="_right",
    )
    feature = spec.feature_name
    if feature == "return_spread":
        values = _series(joined["rolling_return_24h_left"] - joined["rolling_return_24h_right"])
    elif feature == "funding_spread":
        values = _series(joined["funding_rate_left"] - joined["funding_rate_right"])
    elif feature == "volatility_spread":
        values = _series(joined["realized_volatility_left"] - joined["realized_volatility_right"])
    elif feature == "volume_spread":
        values = _series(joined["volume_zscore_left"] - joined["volume_zscore_right"])
    elif feature == "pair_residual_proxy":
        values = rolling_zscore(_series(joined["return_left"] - joined["return_right"]), window=72, min_periods=12)
    else:
        raise ValueError(f"unsupported V5 pair feature: {feature}")
    values.index = _multi_index_for_symbol(joined, left)
    return _series(pd.to_numeric(values.replace([math.inf, -math.inf], math.nan), errors="coerce"))


def _multi_asset_signal_diagnostics(
    spec: MultiAssetSignalSpec,
    *,
    raw_signal: pd.Series,
    decision_signal: pd.Series,
    forward_returns: dict[int, pd.Series],
    folds: list[dict[str, Any]],
    funding_available: bool,
) -> dict[str, Any]:
    signal = _coerce_float_series(decision_signal)
    non_null = signal.dropna()
    row: dict[str, Any] = {
        "signal_id": spec.signal_id,
        "signal_family": spec.signal_family,
        "symbol_scope": ";".join(spec.symbol_scope),
        "pair_scope": "" if spec.pair_scope is None else ";".join(spec.pair_scope),
        "depth": spec.depth,
        "complexity_score": spec.complexity_score,
        "non_null_observation_count": int(len(non_null)),
        "coverage_ratio": _safe_div(len(non_null), len(signal)),
        "missing_ratio": 1.0 - _safe_div(len(non_null), len(signal)),
        "mean_signal": _finite_or_none(non_null.mean()) if not non_null.empty else None,
        "std_signal": _finite_or_none(non_null.std()) if len(non_null) > 1 else None,
        "signal_autocorrelation": _autocorr_or_none(non_null),
        "turnover_proxy": _finite_or_none(signal.groupby(level="symbol").diff().abs().mean()) if isinstance(signal.index, pd.MultiIndex) else _finite_or_none(signal.diff().abs().mean()),
        "duplicate_cluster_id": None,
        "max_abs_correlation_to_existing_signal": None,
        "effective_independent_signal": True,
        "chronological_oos_status": "evaluated" if folds else "insufficient_oos_folds",
        "null_comparison_status": "evaluated_by_family_rows",
        "leakage_check_status": "passed_shifted_by_decision_lag",
        "failure_reason": None,
        "uses_funding": spec.uses_funding,
        "funding_availability_status": "available" if (not spec.uses_funding or funding_available) else "funding_unavailable_for_required_feature",
        "lag_bars": spec.lag_bars,
        "uses_future_data": spec.uses_future_data,
        "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
    }
    for horizon, labels in forward_returns.items():
        ic = _series_corr(signal, labels)
        rank_ic = _series_corr(signal.rank(), labels.rank())
        hit_rate = _sign_hit_rate(signal, labels)
        row[f"forward_return_ic_{horizon}h"] = ic
        row[f"rank_ic_{horizon}h"] = rank_ic
        row[f"sign_hit_rate_{horizon}h"] = hit_rate
    if _decision_signal_lag_mismatch(raw_signal, decision_signal, lag_bars=spec.lag_bars):
        row["leakage_check_status"] = "failed_decision_lag_mismatch"
        row["failure_reason"] = "decision_signal_not_shifted_by_expected_lag"
    if spec.uses_funding and not funding_available:
        row["failure_reason"] = "funding_unavailable_for_required_feature"
    if len(non_null) < 3 and row["failure_reason"] is None:
        row["failure_reason"] = "insufficient_non_null_observations"
    return row


def _decision_signal_lag_mismatch(
    raw_signal: pd.Series,
    decision_signal: pd.Series,
    *,
    lag_bars: int,
) -> bool:
    expected = lag_multi_asset_signal_for_labels(raw_signal, lag_bars=lag_bars)
    expected, observed = expected.align(decision_signal, join="outer")
    expected_valid = _series(expected).notna()
    observed_valid = _series(observed).notna()
    if bool((expected_valid != observed_valid).any()):
        return True
    comparable = expected_valid & observed_valid
    if not bool(comparable.any()):
        return False
    expected_values = _series(expected.loc[comparable]).to_numpy(dtype=float)
    observed_values = _series(observed.loc[comparable]).to_numpy(dtype=float)
    return not bool(np.allclose(expected_values, observed_values, rtol=0.0, atol=1e-12))


def _oos_prefilter_rows(
    *,
    signals: dict[str, pd.Series],
    specs: list[MultiAssetSignalSpec],
    forward_returns: dict[int, pd.Series],
    folds: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    if not folds:
        return [
            {
                "signal_family": family,
                "horizon_hours": horizon,
                "fold_id": "none",
                "chronological_oos_status": "insufficient_oos_folds",
                "purge_bars": max(FORWARD_HORIZONS),
                "embargo_bars": max(FORWARD_HORIZONS),
                "holdout_retune_used": False,
                "no_best_horizon_cherry_picking": True,
                "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
            }
            for family in MULTI_ASSET_SIGNAL_FAMILIES
            for horizon in forward_returns
        ]
    representative_ids = {row["representative_signal_id"] for row in clusters if not bool(row.get("is_redundant_duplicate"))}
    rows: list[dict[str, Any]] = []
    for family in sorted({spec.signal_family for spec in specs}):
        family_specs = [spec for spec in specs if spec.signal_family == family and spec.signal_id in representative_ids]
        if not family_specs:
            family_specs = [spec for spec in specs if spec.signal_family == family]
        family_signal = _mean_signal([signals[spec.signal_id] for spec in family_specs])
        for horizon, labels in forward_returns.items():
            for fold in folds:
                train_ic = _segment_ic(family_signal, labels, fold, "train")
                validation_ic = _segment_ic(family_signal, labels, fold, "validation")
                test_ic = _segment_ic(family_signal, labels, fold, "test")
                rows.append(
                    {
                        "signal_family": family,
                        "horizon_hours": horizon,
                        "fold_id": fold["fold_id"],
                        "train_ic": train_ic,
                        "validation_ic": validation_ic,
                        "test_ic": test_ic,
                        "chronological_oos_status": "evaluated" if test_ic is not None else "insufficient_test_observations",
                        "purge_bars": fold["purge_bars"],
                        "embargo_bars": fold["embargo_bars"],
                        "label_overlap_allowed": False,
                        "holdout_retune_used": False,
                        "no_best_horizon_cherry_picking": True,
                        "representative_selection_rule": "lowest_complexity_then_depth_then_lexicographic_signal_id",
                        "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
                    }
                )
    return rows


def _multi_asset_null_baseline_rows(
    *,
    signals: dict[str, pd.Series],
    specs: list[MultiAssetSignalSpec],
    forward_returns: dict[int, pd.Series],
    seed: int,
) -> list[dict[str, Any]]:
    family_signals = {
        family: _mean_signal([signals[spec.signal_id] for spec in specs if spec.signal_family == family])
        for family in sorted({spec.signal_family for spec in specs})
    }
    rows: list[dict[str, Any]] = []
    families = sorted(family_signals)
    for family, family_signal in family_signals.items():
        for horizon, labels in forward_returns.items():
            signal_ic = _series_corr(family_signal, labels)
            family_index = families.index(family)
            family_permuted = family_signals[families[(family_index + 1) % len(families)]] if len(families) > 1 else None
            random_signal, random_seed = deterministic_random_sign_signal(family_signal, f"v5-{family}-{horizon}", seed_base=seed)
            shuffled, shuffled_seed = shuffled_timestamp_signal(family_signal, f"v5-{family}-{horizon}", seed_base=seed)
            baselines: list[tuple[str, pd.Series | None, str, int | None, bool]] = [
                ("zero_cash_null", pd.Series(0.0, index=family_signal.index, dtype=float), "available", None, False),
                ("buy_and_hold_same_window_baseline", None, "unavailable_constant_position_no_signal_ic", None, False),
                ("deterministic_random_sign", random_signal, "available", random_seed, False),
                ("lagged_signal", lagged_signal_position(family_signal, lag_bars=max(2, horizon // 8)), "available", None, False),
                ("inverse_signal", inverse_signal_position(family_signal), "available", None, False),
                ("shuffled_timestamp_signal", shuffled, "available", shuffled_seed, False),
                ("family_permuted_signal", family_permuted, "available" if family_permuted is not None else "unavailable_single_family", None, False),
                ("symbol_permuted_signal", symbol_permuted_signal(family_signal, f"v5-{family}-{horizon}", seed_base=seed), "available", deterministic_random_seed(f"symbol-permute-v5-{family}-{horizon}", seed_base=seed), False),
                ("universe_median_signal", _universe_median_signal_like(family_signal), "available", None, False),
                (
                    "frozen_tsmom_negative_control",
                    None,
                    "available_summary_only" if DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT.exists() else "unavailable_missing_artifact",
                    None,
                    False,
                ),
            ]
            for baseline_name, baseline_signal, status, deterministic_seed, uses_current_bar in baselines:
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
                        "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
                    }
                )
    return rows


def deterministic_random_sign_signal(
    signal: pd.Series,
    evidence_key: str,
    *,
    seed_base: int = DEFAULT_MULTI_ASSET_GENERATION_SEED,
) -> tuple[pd.Series, int]:
    seed = deterministic_random_seed(evidence_key, seed_base=seed_base)
    rng = random.Random(seed)
    base = _coerce_float_series(signal).abs()
    signs = pd.Series([1.0 if rng.random() >= 0.5 else -1.0 for _ in range(len(base))], index=base.index, dtype=float)
    return base * signs, seed


def shuffled_timestamp_signal(
    signal: pd.Series,
    evidence_key: str,
    *,
    seed_base: int = DEFAULT_MULTI_ASSET_GENERATION_SEED,
) -> tuple[pd.Series, int]:
    seed = deterministic_random_seed(f"shuffle-{evidence_key}", seed_base=seed_base)
    rng = random.Random(seed)
    base = _coerce_float_series(signal)
    if isinstance(base.index, pd.MultiIndex) and "timestamp" in base.index.names:
        frame = base.rename("signal").reset_index()
        values = list(_series(frame["signal"]).fillna(0.0))
        rng.shuffle(values)
        frame["signal"] = values
        index_cols = [str(name) for name in base.index.names]
        return _series(frame.set_index(index_cols)["signal"]).reindex(base.index), seed
    values = list(base.fillna(0.0))
    rng.shuffle(values)
    return pd.Series(values, index=base.index, dtype=float), seed


def symbol_permuted_signal(
    signal: pd.Series,
    evidence_key: str,
    *,
    seed_base: int = DEFAULT_MULTI_ASSET_GENERATION_SEED,
) -> pd.Series:
    _ = deterministic_random_seed(f"symbol-permute-{evidence_key}", seed_base=seed_base)
    data = _coerce_float_series(signal)
    if not isinstance(data.index, pd.MultiIndex) or "symbol" not in data.index.names:
        return _series(data.shift(1))
    frame = data.rename("signal").reset_index()
    out_parts: list[pd.DataFrame] = []
    for _, group in frame.groupby("timestamp", sort=True):
        gf = _frame(group.sort_values("symbol").copy())
        if len(gf) <= 1:
            gf["signal"] = pd.NA
        else:
            gf["signal"] = list(gf["signal"].shift(1).fillna(gf["signal"].iloc[-1]))
        out_parts.append(gf)
    if not out_parts:
        return pd.Series(dtype=float)
    out = pd.concat(out_parts, axis=0)
    index_cols = [str(name) for name in data.index.names]
    return _series(pd.to_numeric(out.set_index(index_cols)["signal"], errors="coerce")).reindex(data.index)


def _multi_asset_family_summary_rows(
    diagnostics: list[dict[str, Any]],
    oos_rows: list[dict[str, Any]],
    null_rows: list[dict[str, Any]],
    *,
    used_symbols: tuple[str, ...],
    funding_available: bool,
) -> list[dict[str, Any]]:
    if not diagnostics:
        return [
            _empty_family_summary_row(
                family,
                unavailable_reason=_family_unavailable_reason(
                    family,
                    used_symbols=used_symbols,
                    funding_available=funding_available,
                ),
            )
            for family in MULTI_ASSET_SIGNAL_FAMILIES
        ]
    frame = pd.DataFrame(diagnostics)
    oos = pd.DataFrame(oos_rows)
    nulls = pd.DataFrame(null_rows)
    rows: list[dict[str, Any]] = []
    for family, group in frame.groupby("signal_family", sort=True):
        gf = _frame(group)
        duplicate_count = int((gf["effective_independent_signal"] == False).sum())  # noqa: E712
        oos_group = oos[oos["signal_family"] == family] if not oos.empty and "signal_family" in oos else pd.DataFrame()
        null_group = nulls[nulls["signal_family"] == family] if not nulls.empty and "signal_family" in nulls else pd.DataFrame()
        oos_metric = _finite_or_none(pd.to_numeric(oos_group.get("test_ic", pd.Series(dtype=float)), errors="coerce").median()) if not oos_group.empty else None
        null_beats = pd.to_numeric(null_group.get("signal_beats_baseline", pd.Series(dtype=object)), errors="coerce") if not null_group.empty else pd.Series(dtype=float)
        null_rate = _finite_or_none(null_beats.mean()) if not null_beats.empty else None
        raw_values = [
            abs(float(value))
            for column in gf.columns
            if str(column).startswith("forward_return_ic_")
            for value in pd.to_numeric(gf[column], errors="coerce").dropna().tolist()
        ]
        rows.append(
            {
                "signal_family": str(family),
                "signal_count": int(len(gf)),
                "effective_signal_breadth": int((gf["effective_independent_signal"] == True).sum()),  # noqa: E712
                "duplicate_signal_count": duplicate_count,
                "family_duplicate_ratio": _safe_div(duplicate_count, len(gf)),
                "symbol_concentration_warning": _symbol_concentration_warning(gf),
                "max_abs_ic_8h": _max_abs_column(gf, "forward_return_ic_8h"),
                "max_abs_ic_24h": _max_abs_column(gf, "forward_return_ic_24h"),
                "max_abs_ic_72h": _max_abs_column(gf, "forward_return_ic_72h"),
                "family_level_best_raw_metric": max(raw_values) if raw_values else None,
                "family_level_median_metric": _finite_or_none(pd.Series(raw_values).median()) if raw_values else None,
                "corresponding_oos_metric": oos_metric,
                "null_robustness_rate": null_rate,
                "null_robustness_status": "evaluated" if null_rate is not None else "insufficient_null_rows",
                "chronological_oos_status": "evaluated" if oos_metric is not None else "insufficient_oos_observations",
                "leakage_fail_count": int((_series(gf["leakage_check_status"]) != "passed_shifted_by_decision_lag").sum()),
                "unavailable_reason": None,
                "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
            }
        )
    present = {str(row["signal_family"]) for row in rows}
    for family in MULTI_ASSET_SIGNAL_FAMILIES:
        if family not in present:
            rows.append(
                _empty_family_summary_row(
                    family,
                    unavailable_reason=_family_unavailable_reason(
                        family,
                        used_symbols=used_symbols,
                        funding_available=funding_available,
                    ),
                )
            )
    rows.sort(key=lambda row: str(row["signal_family"]))
    return rows


def _empty_family_summary_row(family: str, *, unavailable_reason: str) -> dict[str, Any]:
    return {
        "signal_family": family,
        "signal_count": 0,
        "effective_signal_breadth": 0,
        "duplicate_signal_count": 0,
        "family_duplicate_ratio": 0.0,
        "symbol_concentration_warning": None,
        "max_abs_ic_8h": None,
        "max_abs_ic_24h": None,
        "max_abs_ic_72h": None,
        "family_level_best_raw_metric": None,
        "family_level_median_metric": None,
        "corresponding_oos_metric": None,
        "null_robustness_rate": None,
        "null_robustness_status": "not_evaluated",
        "chronological_oos_status": "not_evaluated",
        "leakage_fail_count": 0,
        "unavailable_reason": unavailable_reason,
        "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
    }


def _family_unavailable_reason(
    family: str,
    *,
    used_symbols: tuple[str, ...],
    funding_available: bool,
) -> str:
    if family == "relative_value_interaction" and len(used_symbols) < 2:
        return "requires_at_least_two_symbols"
    if family in {"funding_dispersion", "carry_x_trend"} and not funding_available:
        return "requires_public_funding_panel"
    return "no_signals_generated_for_available_universe"


def _multi_asset_multiple_testing_rows(
    *,
    diagnostics: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
    null_rows: list[dict[str, Any]],
    generated_signal_count: int,
) -> list[dict[str, Any]]:
    effective_breadth = len({row["duplicate_cluster_id"] for row in clusters if not bool(row.get("is_redundant_duplicate"))})
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    rows: list[dict[str, Any]] = [
        {
            "scope": "global",
            "total_generated_signal_count": int(generated_signal_count),
            "total_evaluated_signal_count": int(len(diagnostics)),
            "total_signal_tests": int(len(diagnostics) * len(FORWARD_HORIZONS)),
            "duplicate_adjusted_test_count": int(effective_breadth * len(FORWARD_HORIZONS)),
            "effective_signal_breadth": int(effective_breadth),
            "duplicate_signal_count": int(duplicate_count),
            "duplicate_ratio": _safe_div(duplicate_count, len(clusters)),
            "family_level_best_raw_metric": _max_family_metric(family_summary, "family_level_best_raw_metric"),
            "family_level_median_metric": _median_family_metric(family_summary, "family_level_median_metric"),
            "corresponding_oos_metric": _median_family_metric(family_summary, "corresponding_oos_metric"),
            "null_baseline_row_count": int(len(null_rows)),
            "approximate_false_discovery_warning": "high_many_hypotheses_diagnostic_only",
            "multiple_testing_boundary": MULTI_ASSET_MULTIPLE_TESTING_BOUNDARY,
            "cannot_claim_edge_due_to_multiple_testing": True,
        }
    ]
    for row in family_summary:
        rows.append(
            {
                "scope": str(row["signal_family"]),
                "total_generated_signal_count": int(generated_signal_count),
                "total_evaluated_signal_count": int(row["signal_count"]),
                "total_signal_tests": int(row["signal_count"]) * len(FORWARD_HORIZONS),
                "duplicate_adjusted_test_count": int(row["effective_signal_breadth"]) * len(FORWARD_HORIZONS),
                "effective_signal_breadth": int(row["effective_signal_breadth"]),
                "duplicate_signal_count": int(row["duplicate_signal_count"]),
                "duplicate_ratio": row["family_duplicate_ratio"],
                "family_level_best_raw_metric": row["family_level_best_raw_metric"],
                "family_level_median_metric": row["family_level_median_metric"],
                "corresponding_oos_metric": row["corresponding_oos_metric"],
                "null_baseline_row_count": int(len([item for item in null_rows if item["signal_family"] == row["signal_family"]])),
                "approximate_false_discovery_warning": "high_family_level_diagnostic_only",
                "multiple_testing_boundary": MULTI_ASSET_MULTIPLE_TESTING_BOUNDARY,
                "cannot_claim_edge_due_to_multiple_testing": True,
            }
        )
    return rows


def _multi_asset_shortlist_rows(family_summary: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for row in family_summary:
        if int(row["signal_count"]) == 0:
            label: SignalLabel = "KILL_SIGNAL"
            rationale = "no evaluable signals for family"
        elif int(row["leakage_fail_count"]) > 0:
            label = "KILL_SIGNAL"
            rationale = "leakage check failed"
        elif float(row["family_duplicate_ratio"]) >= 0.95:
            label = "DUPLICATE_SIGNAL"
            rationale = "duplicates dominate family breadth"
        elif row["corresponding_oos_metric"] is not None and abs(float(row["corresponding_oos_metric"])) >= 0.02:
            label = "NEEDS_STRICT_OOS_FALSIFICATION"
            rationale = "diagnostic OOS metric warrants paper-only stricter falsification"
        elif row["family_level_best_raw_metric"] is not None and abs(float(row["family_level_best_raw_metric"])) >= 0.03:
            label = "WATCH_DIAGNOSTIC_ONLY"
            rationale = "raw diagnostic metric requires future OOS falsification before any claim"
        else:
            label = "KILL_SIGNAL"
            rationale = "weak diagnostic evidence under strict prefilter"
        if label not in MULTI_ASSET_ALLOWED_SIGNAL_LABELS:
            raise ValueError(f"forbidden V5 signal label emitted: {label}")
        rows.append(
            {
                "signal_family": row["signal_family"],
                "signal_label": label,
                "shortlist_category": label,
                "effective_signal_breadth": row["effective_signal_breadth"],
                "family_level_best_raw_metric": row["family_level_best_raw_metric"],
                "corresponding_oos_metric": row["corresponding_oos_metric"],
                "rationale": rationale,
                "paper_only": True,
                "is_strategy": False,
                "is_readiness": False,
                "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
            }
        )
    for forbidden in MULTI_ASSET_FORBIDDEN_OUTPUT_LABELS:
        if any(item["signal_label"] == forbidden for item in rows):
            raise ValueError(f"forbidden V5 output label emitted: {forbidden}")
    return rows


def _multi_asset_failed_gate_rows(
    *,
    diagnostics: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
    excluded_symbols: list[dict[str, Any]],
    v4_queue_empty: bool,
    v4_queue_status: str,
    v4_queue_reason: str,
    used_symbols: tuple[str, ...],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if len(used_symbols) < 2:
        rows.append({"gate": "multi_asset_minimum_symbol_count", "status": "failed", "reason": f"{len(used_symbols)} OHLCV-usable symbols; cross-sectional evidence is limited"})
    if excluded_symbols:
        rows.append({"gate": "symbol_availability", "status": "watch", "reason": f"{len(excluded_symbols)} requested symbols excluded for OHLCV/schema/min-bars reasons"})
    leakage_fail = [row for row in diagnostics if row["leakage_check_status"] != "passed_shifted_by_decision_lag"]
    if leakage_fail:
        rows.append({"gate": "leakage_check_status", "status": "failed", "reason": f"{len(leakage_fail)} V5 signals failed leakage checks"})
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    if duplicate_count:
        rows.append({"gate": "duplicate_correlation_control", "status": "watch", "reason": f"{duplicate_count} redundant V5 signal variants marked"})
    if v4_queue_status in {"missing", "malformed"}:
        rows.append({"gate": "v4_queue_audit_only", "status": "failed", "reason": v4_queue_reason})
    elif not v4_queue_empty:
        rows.append({"gate": "v4_queue_audit_only", "status": "watch", "reason": "source V4 queue was non-empty but was not used to seed V5 candidates"})
    else:
        rows.append({"gate": "v4_queue_audit_only", "status": "passed", "reason": v4_queue_reason})
    no_oos = all(row.get("corresponding_oos_metric") is None for row in family_summary)
    if no_oos:
        rows.append({"gate": "chronological_oos_prefilter", "status": "watch", "reason": "insufficient OOS folds or test observations for family metrics"})
    rows.append({"gate": "v4_state_preserved", "status": "passed", "reason": "V4 source artifact is audit-only and not mutated"})
    rows.append({"gate": "readiness_boundary", "status": "passed", "reason": "readiness_baseline_verdict remains NOT_READY"})
    rows.append({"gate": "tsmom_state_unchanged", "status": "passed", "reason": "TSMOM artifacts are not modified or restored"})
    rows.append({"gate": "multiple_testing_boundary", "status": "unclaimed", "reason": "cannot_claim_edge_due_to_multiple_testing is true"})
    return rows


def _next_strict_falsification_queue(
    shortlist: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    summary_by_family = {row["signal_family"]: row for row in family_summary}
    rows: list[dict[str, Any]] = []
    for row in shortlist:
        if row["signal_label"] != "NEEDS_STRICT_OOS_FALSIFICATION":
            continue
        family = str(row["signal_family"])
        summary = summary_by_family[family]
        rows.append(
            {
                "signal_family": family,
                "queue_decision": "NEEDS_STRICT_OOS_FALSIFICATION",
                "paper_only": True,
                "effective_signal_breadth": summary["effective_signal_breadth"],
                "corresponding_oos_metric": summary["corresponding_oos_metric"],
                "reason": "strict prefilter diagnostic only; future loop must pre-register before any OOS claim",
                "claim_boundary": MULTI_ASSET_CLAIM_BOUNDARY,
            }
        )
    return rows


def _suite_decision(shortlist: list[dict[str, Any]], duplicate_ratio: float) -> str:
    if duplicate_ratio >= 0.95 and shortlist:
        return "DUPLICATES_DOMINATE"
    labels = {str(row["signal_label"]) for row in shortlist}
    if "NEEDS_STRICT_OOS_FALSIFICATION" in labels:
        return "NEEDS_STRICT_OOS_FALSIFICATION"
    if "WATCH_FAMILY_ONLY" in labels or "WATCH_DIAGNOSTIC_ONLY" in labels:
        return "WATCH_FAMILY_ONLY_PAPER_RESEARCH"
    if not shortlist:
        return "MULTI_ASSET_SIGNAL_FACTORY_INFRA_ONLY"
    return "NO_SURVIVING_SIGNAL_FAMILY"


def _read_v4_next_protocol_queue(source_v4_artifact_path: Path) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    path = source_v4_artifact_path / "next_protocol_queue.csv"
    if not path.exists():
        return [], _v4_queue_audit(
            path,
            status="missing",
            row_count=0,
            reason="next_protocol_queue.csv missing from source V4 artifact path",
        )
    if not path.read_text().strip():
        return [], _v4_queue_audit(path, status="present_empty", row_count=0, reason="queue file present but blank")
    try:
        frame = pd.read_csv(path)
    except pd.errors.EmptyDataError:
        return [], _v4_queue_audit(path, status="malformed", row_count=0, reason="next_protocol_queue.csv could not be parsed")
    if not V4_QUEUE_REQUIRED_COLUMNS.issubset(set(frame.columns)):
        return [], _v4_queue_audit(
            path,
            status="malformed",
            row_count=0,
            reason=f"next_protocol_queue.csv missing required columns: {sorted(V4_QUEUE_REQUIRED_COLUMNS - set(frame.columns))}",
        )
    rows = [dict(row) for row in frame.to_dict(orient="records")]
    status = "present_empty" if len(rows) == 0 else "present_non_empty"
    reason = "queue file present with no rows" if len(rows) == 0 else "queue file present with rows"
    return rows, _v4_queue_audit(path, status=status, row_count=len(rows), reason=reason)


def _v4_queue_audit(path: Path, *, status: str, row_count: int, reason: str) -> dict[str, Any]:
    return {
        "v4_queue_path": str(path),
        "v4_queue_status": status,
        "v4_queue_empty": status == "present_empty",
        "v4_queue_row_count": int(row_count),
        "v4_queue_reason": reason,
    }


def _forward_return_labels(panel: pd.DataFrame, *, horizons: Iterable[int]) -> dict[int, pd.Series]:
    if panel.empty:
        return {int(horizon): pd.Series(dtype=float) for horizon in horizons}
    labels: dict[int, pd.Series] = {}
    for horizon in horizons:
        values = panel.groupby(level="symbol", group_keys=False)["close"].shift(-int(horizon)) / panel["close"] - 1.0
        values.index = cast(PanelIndex, panel.index)
        labels[int(horizon)] = _series(values)
    return labels


def _segment_ic(signal: pd.Series, labels: pd.Series, fold: dict[str, Any], segment: str) -> float | None:
    if not isinstance(signal.index, pd.MultiIndex) or "timestamp" not in signal.index.names:
        return None
    timestamps = pd.DatetimeIndex(sorted(pd.unique(signal.index.get_level_values("timestamp"))))
    start = int(fold[f"{segment}_start_pos"])
    end = int(fold[f"{segment}_end_pos"])
    if start >= len(timestamps) or end > len(timestamps):
        return None
    selected = set(timestamps[start:end])
    mask = signal.index.get_level_values("timestamp").isin(selected)
    return _series_corr(_series(signal[mask]), _series(labels.reindex(signal.index)[mask]))


def _normalize_ohlcv_for_panel(frame: pd.DataFrame) -> pd.DataFrame:
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
    data = _frame(data.sort_index().loc[:, ["Open", "High", "Low", "Close", "Volume"]].astype(float))
    if data.index.has_duplicates:
        raise ValueError("market frame duplicate timestamps are not allowed")
    return data


def _align_public_funding_to_ohlcv(index: pd.Index, funding_frame: pd.DataFrame | None) -> pd.Series:
    if funding_frame is None or funding_frame.empty:
        return pd.Series(math.nan, index=index, dtype=float)
    funding = funding_frame.copy()
    if not isinstance(funding.index, pd.DatetimeIndex):
        raise ValueError("funding frame index must be a DatetimeIndex")
    if funding.index.tz is None:
        funding.index = funding.index.tz_localize("UTC")
    else:
        funding.index = funding.index.tz_convert("UTC")
    rates = _series(pd.to_numeric(funding["funding_rate"], errors="coerce")).sort_index()
    aligned = rates.reindex(pd.DatetimeIndex(index), method="ffill")
    return _series(aligned.shift(1))


def _symbol_panel(panel: pd.DataFrame, symbol: str) -> pd.DataFrame:
    if panel.empty:
        return pd.DataFrame()
    data = _frame(panel[panel["symbol"] == symbol].copy())
    if data.empty:
        return data
    return _frame(data.set_index("timestamp", drop=False).sort_index())


def _multi_index_for_symbol(frame: pd.DataFrame, symbol: str) -> pd.MultiIndex:
    timestamps = pd.DatetimeIndex(frame.index if isinstance(frame.index, pd.DatetimeIndex) else frame["timestamp"])
    return pd.MultiIndex.from_arrays([timestamps, [symbol] * len(frame)], names=["timestamp", "symbol"])


def _cross_sectional_rank(data: pd.DataFrame, column: str) -> pd.Series:
    ranks = data.groupby("timestamp")[column].rank(method="average", pct=True)
    return _series(ranks * 2.0 - 1.0)


def rolling_zscore(series: pd.Series, *, window: int, min_periods: int | None = None) -> pd.Series:
    if window <= 1:
        raise ValueError("rolling zscore requires window > 1")
    data = _series(series).astype(float)
    periods = window if min_periods is None else min_periods
    mean = _series(data.rolling(window, min_periods=periods).mean())
    std = _series(data.rolling(window, min_periods=periods).std())
    return _safe_ratio(_series(data - mean), std)


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


def _safe_ratio(numerator: pd.Series, denominator: pd.Series) -> pd.Series:
    num = _coerce_float_series(numerator)
    den = _coerce_float_series(denominator).replace(0.0, math.nan)
    return _series(num / den).replace([math.inf, -math.inf], math.nan)


def _series_corr(left: pd.Series | None, right: pd.Series | None) -> float | None:
    if left is None or right is None:
        return None
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
    return _finite_or_none(float(np.dot(lhs_centered, rhs_centered) / denominator))


def _sign_hit_rate(signal: pd.Series, labels: pd.Series) -> float | None:
    data = pd.concat([signal.rename("signal"), labels.rename("label")], axis=1).dropna()
    if data.empty:
        return None
    signed = _series(data["signal"]).map(_safe_sign) * _series(data["label"]).map(_safe_sign)
    return _finite_or_none((signed > 0).mean())


def _autocorr_or_none(series: pd.Series) -> float | None:
    data = _series(series).dropna()
    if len(data) < 3 or _finite_float(data.std()) == 0.0:
        return None
    return _finite_or_none(data.autocorr(lag=1))


def _mean_signal(signals: list[pd.Series]) -> pd.Series:
    if not signals:
        return pd.Series(dtype=float)
    common_index = _union_signal_index(signals)
    frame = pd.DataFrame({idx: _coerce_float_series(signal).reindex(common_index) for idx, signal in enumerate(signals)})
    return _series(frame.mean(axis=1, skipna=True))


def _union_signal_index(signals: Iterable[pd.Series]) -> pd.Index:
    result: pd.Index | None = None
    for signal in signals:
        result = signal.index if result is None else result.union(signal.index)
    return pd.Index([]) if result is None else result.sort_values()


def _clusters_from_corr_matrix(corr_matrix: np.ndarray, signal_ids: list[str], threshold: float) -> list[set[str]]:
    parent = list(range(len(signal_ids)))

    def find(item: int) -> int:
        while parent[item] != item:
            parent[item] = parent[parent[item]]
            item = parent[item]
        return item

    def union(left: int, right: int) -> None:
        left_root = find(left)
        right_root = find(right)
        if left_root != right_root:
            parent[right_root] = left_root

    matches = np.argwhere(np.triu(corr_matrix >= threshold, k=1))
    for left, right in matches:
        union(int(left), int(right))
    grouped: dict[int, set[str]] = {}
    for idx, signal_id in enumerate(signal_ids):
        grouped.setdefault(find(idx), set()).add(signal_id)
    return [grouped[key] for key in sorted(grouped)]


def _multi_asset_cluster_representative(
    cluster: set[str],
    spec_by_id: dict[str, MultiAssetSignalSpec],
) -> str:
    return min(
        cluster,
        key=lambda signal_id: (
            spec_by_id[signal_id].complexity_score,
            spec_by_id[signal_id].depth,
            signal_id,
        ),
    )


def _ordered_pairs(symbols: tuple[str, ...]) -> list[tuple[str, str]]:
    pairs: list[tuple[str, str]] = []
    for idx, left in enumerate(symbols):
        for right in symbols[idx + 1 :]:
            pairs.append((left, right))
    return pairs


def _universe_median_signal_like(signal: pd.Series) -> pd.Series:
    data = _coerce_float_series(signal)
    if not isinstance(data.index, pd.MultiIndex) or "timestamp" not in data.index.names:
        return pd.Series(0.0, index=data.index, dtype=float)
    frame = data.rename("signal").reset_index()
    frame["median"] = frame.groupby("timestamp")["signal"].transform("median")
    index_cols = [str(name) for name in data.index.names]
    return _series(frame.set_index(index_cols)["median"]).reindex(data.index)


def _symbol_concentration_warning(frame: pd.DataFrame) -> str:
    if frame.empty or "symbol_scope" not in frame:
        return "unknown"
    counts: dict[str, int] = {}
    for raw in frame["symbol_scope"]:
        for symbol in str(raw).split(";"):
            if symbol:
                counts[symbol] = counts.get(symbol, 0) + 1
    if not counts:
        return "unknown"
    top = max(counts.values())
    return "high" if top / max(1, sum(counts.values())) > 0.5 else "normal"


def _max_abs_column(frame: pd.DataFrame, column: str) -> float | None:
    if column not in frame.columns:
        return None
    values = pd.to_numeric(frame[column], errors="coerce").dropna().abs()
    if values.empty:
        return None
    return _finite_or_none(values.max())


def _max_family_metric(rows: list[dict[str, Any]], column: str) -> float | None:
    values = [abs(float(row[column])) for row in rows if row.get(column) is not None and math.isfinite(float(row[column]))]
    return max(values) if values else None


def _median_family_metric(rows: list[dict[str, Any]], column: str) -> float | None:
    values = [float(row[column]) for row in rows if row.get(column) is not None and math.isfinite(float(row[column]))]
    return _finite_or_none(pd.Series(values).median()) if values else None


def _beats_baseline(signal_ic: float | None, baseline_ic: float | None) -> bool | None:
    if signal_ic is None or baseline_ic is None:
        return None
    return abs(signal_ic) > abs(baseline_ic)


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


def _safety_gates() -> dict[str, Any]:
    return {
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
    }


def _series(value: Any) -> pd.Series:
    return cast(pd.Series, value)


def _frame(value: Any) -> pd.DataFrame:
    return cast(pd.DataFrame, value)


def _coerce_float_series(value: pd.Series) -> pd.Series:
    return _series(pd.to_numeric(value, errors="coerce")).astype(float)


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    _atomic_write_text(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def _atomic_write_text(path: Path, payload: str) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(payload)
    tmp.replace(path)
