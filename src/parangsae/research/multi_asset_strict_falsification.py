# pyright: reportAttributeAccessIssue=false, reportArgumentType=false, reportCallIssue=false
from __future__ import annotations

import json
import math
import random
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterable, cast

import numpy as np
import pandas as pd

from parangsae.marketdata.binance_funding import FundingRateBundle
from parangsae.marketdata.binance_vision import MarketDataBundle
from parangsae.research.alpha_signal_factory import FORWARD_HORIZONS, READINESS_BASELINE_VERDICT
from parangsae.research.loop_artifacts import prepare_loop_output_dir
from parangsae.research.multi_asset_alpha_factory import (
    MULTI_ASSET_FORBIDDEN_OUTPUT_LABELS,
    MultiAssetSignalSpec,
    build_multi_asset_panel,
    build_multi_asset_signal_series,
    deterministic_random_sign_signal,
    lag_multi_asset_signal_for_labels,
    shuffled_timestamp_signal,
    symbol_permuted_signal,
)
from parangsae.research.null_baselines import (
    deterministic_random_seed,
    inverse_signal_position,
    lagged_signal_position,
)

MULTI_ASSET_STRICT_FALSIFICATION_SCHEMA_VERSION = 9
MULTI_ASSET_STRICT_FALSIFICATION_LOOP_SLUG = "multi-asset-strict-oos-fdr-falsification-v9"
MULTI_ASSET_STRICT_FALSIFICATION_NAMESPACE = (
    "multi_asset_strict_oos_fdr_falsification_v9_only_not_readiness"
)
DEFAULT_MULTI_ASSET_STRICT_FALSIFICATION_V8_ARTIFACT = Path(
    "reports/loops/20260628-panel-ready-multi-asset-alpha-prefilter-rerun-v8"
)
DEFAULT_MULTI_ASSET_STRICT_FALSIFICATION_SEED = 20260630
DEFAULT_MULTI_ASSET_STRICT_PERMUTATION_COUNT = 1000
DEFAULT_MULTI_ASSET_STRICT_FOLD_COUNT = 3
STRICT_HORIZON_AGGREGATION_RULE = "all_fixed_horizons_must_clear"
STRICT_PERMUTATION_BLOCK_SIZE_RULE = "block_size_equals_horizon_bars"
MULTI_ASSET_STRICT_FALSIFICATION_FAMILY_DECISIONS = (
    "KILL_FAMILY",
    "FREEZE_AS_NEGATIVE_CONTROL",
    "DUPLICATE_DOMINATED",
    "FALSIFIED_SIGN_FLIP",
    "FALSIFIED_NULL_FRAGILE",
    "FALSIFIED_OOS_INCONSISTENT",
    "FALSIFIED_REGIME_FRAGILE",
    "FALSIFIED_SYMBOL_CONCENTRATED",
    "NEEDS_MECHANISM_RESEARCH",
    "WATCH_FAMILY_ONLY",
)
MULTI_ASSET_STRICT_FALSIFICATION_SUITE_DECISIONS = (
    "MULTI_ASSET_STRICT_FALSIFICATION_INFRA_ONLY",
    "NO_V8_QUEUE_TO_FALSIFY",
    "NO_FAMILY_SURVIVES_STRICT_FALSIFICATION",
    "FAILS_FDR_BOUNDARY",
    "FAILS_OOS_ROBUSTNESS",
    "NEEDS_MECHANISM_RESEARCH",
    "WATCH_FAMILY_ONLY_PAPER_RESEARCH",
)
MULTI_ASSET_STRICT_FALSIFICATION_FORBIDDEN_OUTPUTS = set(MULTI_ASSET_FORBIDDEN_OUTPUT_LABELS)
MULTI_ASSET_STRICT_MULTIPLE_TESTING_BOUNDARY = (
    "V9 evaluates a frozen V8 queue through strict chronological OOS, deterministic block "
    "permutation nulls, and BH-FDR. Every result remains a paper-only falsification "
    "diagnostic exposed to multiple-testing and selection risk; V9 cannot claim alpha, "
    "edge, profitability, readiness, execution readiness, or permission to trade."
)
MULTI_ASSET_STRICT_FDR_BOUNDARY = (
    "BH-FDR q-values are duplicate-adjusted diagnostics over the frozen queue. Even a "
    "small q-value cannot confirm edge; family survival requires all fixed horizons to "
    "clear without best-horizon selection."
)
MULTI_ASSET_STRICT_LEAKAGE_BOUNDARY = (
    "V9 uses only public cached panel data and V8 catalog signals. Every signal is shifted "
    "by at least one decision lag before forward labels; chronological folds purge "
    "overlapping labels and embargo at least the max horizon."
)
MULTI_ASSET_STRICT_CLAIM_BOUNDARY = (
    "MULTI_ASSET_STRICT_OOS_FDR_FALSIFICATION_V9 freezes the V8 paper-only queue and "
    "performs stricter falsification. It is not alpha generation, not a trading strategy, "
    "not portfolio construction, not signal weight optimization, not profitability "
    "evidence, not readiness or live readiness, and not permission for live/testnet/"
    "account/API-key/order/fill use."
)
V8_REQUIRED_INPUT_FILES = (
    "panel_ready_multi_asset_alpha_prefilter.json",
    "signal_catalog.jsonl",
    "signal_diagnostics.csv",
    "signal_family_summary.csv",
    "signal_correlation_clusters.csv",
    "oos_prefilter_summary.csv",
    "null_baseline_comparison.csv",
    "multiple_testing_summary.csv",
    "shortlist.csv",
    "next_strict_falsification_queue.csv",
)
MULTI_ASSET_STRICT_FALSIFICATION_REQUIRED_FILES = (
    "loop.json",
    "multi_asset_strict_oos_fdr_falsification.json",
    "frozen_v8_queue.csv",
    "chronological_folds.csv",
    "signal_strict_oos_diagnostics.csv",
    "family_strict_summary.csv",
    "permutation_null_results.csv",
    "bh_fdr_summary.csv",
    "regime_robustness.csv",
    "symbol_robustness.csv",
    "null_baseline_stress.csv",
    "failed_gate_rollup.csv",
    "next_mechanism_research_queue.csv",
    "report.md",
    "commands.txt",
    "changed_files.txt",
)
MULTI_ASSET_STRICT_FALSIFICATION_KNOWN_FILES = set(MULTI_ASSET_STRICT_FALSIFICATION_REQUIRED_FILES)
FROZEN_QUEUE_COLUMNS = (
    "source_v8_signal_family",
    "signal_family",
    "signal_id",
    "horizon_bars",
    "queue_decision",
    "paper_only",
    "source_v8_effective_signal_breadth",
    "source_v8_corresponding_oos_metric",
    "representative_selection_rule",
    "representative_selection_source",
    "horizon_aggregation_rule",
    "horizon_required_for_family_survival",
    "permutation_block_size_rule",
    "block_size",
    "complexity_score",
    "depth",
    "lag_bars",
    "frozen_before_evaluation",
    "no_best_horizon_selection",
    "claim_boundary",
)
CHRONOLOGICAL_FOLD_COLUMNS = (
    "fold_id",
    "train_start",
    "train_end",
    "validation_start",
    "validation_end",
    "test_start",
    "test_end",
    "purge_bars",
    "embargo_bars",
    "horizon_bars",
    "fold_sample_count",
    "purged_observation_count",
    "embargoed_observation_count",
    "train_start_pos",
    "train_end_pos",
    "validation_start_pos",
    "validation_end_pos",
    "test_start_pos",
    "test_end_pos",
    "label_overlap_allowed",
    "holdout_retune_used",
    "no_best_horizon_cherry_picking",
)
SIGNAL_STRICT_OOS_DIAGNOSTIC_COLUMNS = (
    "signal_id",
    "signal_family",
    "symbol_scope",
    "pair_scope",
    "horizon_bars",
    "depth",
    "complexity_score",
    "lag_bars",
    "train_ic",
    "validation_ic",
    "test_ic",
    "train_rank_ic",
    "validation_rank_ic",
    "test_rank_ic",
    "validation_hit_rate",
    "test_hit_rate",
    "validation_coverage",
    "test_coverage",
    "validation_turnover_proxy",
    "test_turnover_proxy",
    "validation_missing_ratio",
    "test_missing_ratio",
    "validation_to_test_degradation",
    "sign_consistency_train_to_test",
    "fold_consistency_rate",
    "worst_fold_test_ic",
    "median_fold_test_ic",
    "chronological_oos_status",
    "leakage_check_status",
    "failure_reason",
    "horizon_aggregation_rule",
    "no_best_horizon_selection",
    "claim_boundary",
)
FAMILY_STRICT_SUMMARY_COLUMNS = (
    "signal_family",
    "horizon_bars",
    "queued_signal_count",
    "evaluated_signal_count",
    "duplicate_adjusted_signal_count",
    "effective_signal_breadth",
    "median_test_ic",
    "mean_test_ic",
    "worst_fold_test_ic",
    "worst_symbol_test_ic",
    "sign_consistency_rate",
    "fold_consistency_rate",
    "symbol_consistency_rate",
    "regime_survival_rate",
    "null_robustness_rate",
    "bh_min_q_value",
    "bh_family_all_horizon_q_value",
    "bh_significant_count",
    "validation_to_test_degradation",
    "turnover_proxy_median",
    "high_turnover_warning",
    "horizon_survived",
    "horizon_decision",
    "horizon_aggregation_rule",
    "family_decision",
    "claim_boundary",
)
PERMUTATION_NULL_COLUMNS = (
    "signal_family",
    "signal_id",
    "horizon_bars",
    "observed_test_metric",
    "permutation_null_median",
    "permutation_null_p05",
    "permutation_null_p95",
    "empirical_p_value",
    "null_fragility_status",
    "permutation_count",
    "block_size",
    "permutation_block_size_rule",
    "p_value_approximation_boundary",
    "claim_boundary",
)
BH_FDR_COLUMNS = (
    "scope",
    "signal_family",
    "signal_id",
    "horizon_bars",
    "total_signal_tests",
    "total_family_tests",
    "total_horizon_tests",
    "duplicate_adjusted_test_count",
    "effective_signal_breadth",
    "raw_p_value",
    "bh_q_value",
    "bh_significant",
    "selection_deflated_alpha",
    "horizon_aggregation_rule",
    "fdr_boundary",
    "cannot_claim_edge_due_to_multiple_testing",
)
REGIME_ROBUSTNESS_COLUMNS = (
    "signal_family",
    "signal_id",
    "horizon_bars",
    "regime",
    "regime_sample_count",
    "median_test_ic",
    "sign_consistency",
    "hit_rate",
    "null_status",
    "failure_reason",
    "regime_uses_forward_labels",
    "claim_boundary",
)
SYMBOL_ROBUSTNESS_COLUMNS = (
    "signal_family",
    "signal_id",
    "horizon_bars",
    "symbol",
    "per_symbol_test_ic",
    "per_symbol_hit_rate",
    "symbol_concentration",
    "worst_symbol_metric",
    "median_symbol_metric",
    "symbol_consistency_rate",
    "symbol_drop_one_robustness",
    "claim_boundary",
)
NULL_BASELINE_STRESS_COLUMNS = (
    "signal_family",
    "signal_id",
    "horizon_bars",
    "baseline_name",
    "observed_test_metric",
    "baseline_test_metric",
    "signal_beats_baseline",
    "baseline_status",
    "deterministic_seed",
    "lag_bars",
    "baseline_uses_current_bar_label",
    "baseline_uses_current_bar_signal",
    "claim_boundary",
)
FAILED_GATE_COLUMNS = ("gate", "status", "reason")
NEXT_MECHANISM_RESEARCH_QUEUE_COLUMNS = (
    "signal_family",
    "queue_decision",
    "paper_only",
    "reason",
    "claim_boundary",
)


@dataclass(frozen=True)
class MultiAssetStrictFalsificationResult:
    payload: dict[str, Any]
    frozen_queue: list[dict[str, Any]]
    chronological_folds: list[dict[str, Any]]
    signal_strict_oos_diagnostics: list[dict[str, Any]]
    family_strict_summary: list[dict[str, Any]]
    permutation_null_results: list[dict[str, Any]]
    bh_fdr_summary: list[dict[str, Any]]
    regime_robustness: list[dict[str, Any]]
    symbol_robustness: list[dict[str, Any]]
    null_baseline_stress: list[dict[str, Any]]
    failed_gates: list[dict[str, Any]]
    next_mechanism_research_queue: list[dict[str, Any]]


def default_multi_asset_strict_oos_fdr_falsification_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{MULTI_ASSET_STRICT_FALSIFICATION_LOOP_SLUG}"


def read_v8_strict_falsification_source_state(path: str | Path) -> dict[str, Any]:
    source_path = Path(path)
    artifact_dir = source_path if source_path.is_dir() else source_path.parent
    payload_path = (
        artifact_dir / "panel_ready_multi_asset_alpha_prefilter.json"
        if source_path.is_dir()
        else source_path
    )
    if not payload_path.exists():
        raise FileNotFoundError(f"missing V8 panel_ready_multi_asset_alpha_prefilter.json: {payload_path}")
    missing = [filename for filename in V8_REQUIRED_INPUT_FILES if not (artifact_dir / filename).exists()]
    if missing:
        raise FileNotFoundError(f"V8 artifact missing required input file(s): {missing}")
    payload = _read_json(payload_path)
    if "panel_ready_multi_asset_alpha_prefilter_rerun_v8" not in str(payload.get("loop_namespace", "")):
        raise ValueError("source_v8_artifact_path must point to a V8 panel-ready alpha prefilter artifact")
    if payload.get("readiness_baseline_verdict") != READINESS_BASELINE_VERDICT:
        raise ValueError("source_v8_artifact_path must preserve readiness_baseline_verdict=NOT_READY")
    if payload.get("source_v7_panel_ready") is not True:
        raise ValueError("source_v8_artifact_path requires source_v7_panel_ready=true")
    for field in (
        "no_live_orders_submitted",
        "no_testnet_orders_submitted",
        "no_account_data_used",
        "no_api_key_used",
        "no_private_order_or_fill_data_used",
    ):
        if payload.get(field) is not True and dict(payload.get("safety_gates", {})).get(field) is not True:
            raise ValueError(f"source_v8_artifact_path violates safety gate: {field}")
    for field in (
        "holdout_retune_used",
        "strategy_optimization_performed",
        "live_or_testnet_path_created",
        "tsmom_state_changed",
    ):
        if bool(payload.get(field)):
            raise ValueError(f"source_v8_artifact_path violates paper-only boundary: {field}=true")
    if bool(payload.get("data_download_performed")):
        raise ValueError("source_v8_artifact_path reports data_download_performed=true")
    queue_rows = _read_csv_records(artifact_dir / "next_strict_falsification_queue.csv")
    catalog_rows = _read_jsonl_records(artifact_dir / "signal_catalog.jsonl")
    clusters = _read_csv_records(artifact_dir / "signal_correlation_clusters.csv")
    for row in catalog_rows:
        MultiAssetSignalSpec.from_dict(row)
    return {
        "artifact_dir": artifact_dir,
        "payload_path": payload_path,
        "payload": payload,
        "queue_rows": queue_rows,
        "catalog_rows": catalog_rows,
        "cluster_rows": clusters,
    }


def materialize_frozen_v8_queue(
    v8_state: dict[str, Any],
    *,
    horizons: Iterable[int] = FORWARD_HORIZONS,
) -> list[dict[str, Any]]:
    queue_rows = list(cast(list[dict[str, Any]], v8_state.get("queue_rows", [])))
    catalog = [MultiAssetSignalSpec.from_dict(row) for row in cast(list[dict[str, Any]], v8_state["catalog_rows"])]
    cluster_rows = cast(list[dict[str, Any]], v8_state.get("cluster_rows", []))
    required_horizons = tuple(int(horizon) for horizon in horizons)
    rows: list[dict[str, Any]] = []
    for queue_row in queue_rows:
        family = str(queue_row["signal_family"])
        candidates = [spec for spec in catalog if spec.signal_family == family]
        if not candidates:
            raise ValueError(f"V8 frozen queue family has no matching signal_catalog entries: {family}")
        representative_id, representative_source = _select_frozen_representative(
            family=family,
            candidates=candidates,
            cluster_rows=cluster_rows,
        )
        representative = next(spec for spec in candidates if spec.signal_id == representative_id)
        for horizon in required_horizons:
            rows.append(
                {
                    "source_v8_signal_family": family,
                    "signal_family": family,
                    "signal_id": representative.signal_id,
                    "horizon_bars": int(horizon),
                    "queue_decision": str(queue_row.get("queue_decision", "NEEDS_STRICT_OOS_FALSIFICATION")),
                    "paper_only": _truthy(queue_row.get("paper_only", True)),
                    "source_v8_effective_signal_breadth": _finite_or_none(
                        queue_row.get("effective_signal_breadth")
                    ),
                    "source_v8_corresponding_oos_metric": _finite_or_none(
                        queue_row.get("corresponding_oos_metric")
                    ),
                    "representative_selection_rule": (
                        "v8_cluster_representative_then_lowest_complexity_then_depth_then_lexicographic_signal_id"
                    ),
                    "representative_selection_source": representative_source,
                    "horizon_aggregation_rule": STRICT_HORIZON_AGGREGATION_RULE,
                    "horizon_required_for_family_survival": ";".join(f"{item}h" for item in required_horizons),
                    "permutation_block_size_rule": STRICT_PERMUTATION_BLOCK_SIZE_RULE,
                    "block_size": int(horizon),
                    "complexity_score": representative.complexity_score,
                    "depth": representative.depth,
                    "lag_bars": representative.lag_bars,
                    "frozen_before_evaluation": True,
                    "no_best_horizon_selection": True,
                    "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
                }
            )
    return rows


def build_strict_chronological_folds(
    timestamps: Iterable[pd.Timestamp],
    *,
    horizons: Iterable[int] = FORWARD_HORIZONS,
    fold_count: int = DEFAULT_MULTI_ASSET_STRICT_FOLD_COUNT,
) -> list[dict[str, Any]]:
    index = pd.DatetimeIndex(sorted(pd.Timestamp(item) for item in timestamps))
    if index.empty:
        return []
    if fold_count <= 0:
        raise ValueError("fold_count must be positive")
    horizon_values = tuple(int(horizon) for horizon in horizons)
    max_horizon = max(horizon_values)
    purge_bars = max_horizon
    embargo_bars = max_horizon
    reserved_gap = 2 * (purge_bars + embargo_bars)
    segment = max(1, (len(index) - reserved_gap) // (fold_count + 3))
    rows: list[dict[str, Any]] = []
    for fold_idx in range(fold_count):
        train_start = 0
        train_end = segment * (fold_idx + 1)
        validation_start = train_end + purge_bars + embargo_bars
        validation_end = validation_start + segment
        test_start = validation_end + purge_bars + embargo_bars
        test_end = min(test_start + segment, len(index) - max_horizon)
        if train_end <= train_start or validation_end <= validation_start or test_end <= test_start:
            continue
        for horizon in horizon_values:
            rows.append(
                {
                    "fold_id": f"fold_{fold_idx + 1:02d}_h{horizon:03d}",
                    "train_start": _timestamp(index[train_start]),
                    "train_end": _timestamp(index[train_end - 1]),
                    "validation_start": _timestamp(index[validation_start]),
                    "validation_end": _timestamp(index[validation_end - 1]),
                    "test_start": _timestamp(index[test_start]),
                    "test_end": _timestamp(index[test_end - 1]),
                    "purge_bars": int(purge_bars),
                    "embargo_bars": int(embargo_bars),
                    "horizon_bars": int(horizon),
                    "fold_sample_count": int(
                        (train_end - train_start)
                        + (validation_end - validation_start)
                        + (test_end - test_start)
                    ),
                    "purged_observation_count": int(purge_bars),
                    "embargoed_observation_count": int(embargo_bars),
                    "train_start_pos": int(train_start),
                    "train_end_pos": int(train_end),
                    "validation_start_pos": int(validation_start),
                    "validation_end_pos": int(validation_end),
                    "test_start_pos": int(test_start),
                    "test_end_pos": int(test_end),
                    "label_overlap_allowed": False,
                    "holdout_retune_used": False,
                    "no_best_horizon_cherry_picking": True,
                }
            )
    return rows


def block_permutation_indices(*, length: int, block_size: int, seed: int) -> list[int]:
    if length <= 0:
        return []
    if block_size <= 0:
        raise ValueError("block_size must be positive")
    full_end = length - (length % block_size)
    full_blocks = [list(range(start, start + block_size)) for start in range(0, full_end, block_size)]
    final_block = list(range(full_end, length))
    rng = random.Random(seed)
    rng.shuffle(full_blocks)
    flattened = [item for block in full_blocks for item in block]
    if final_block:
        flattened.extend(final_block)
    return flattened


def moving_block_permutation_null(
    signal: pd.Series,
    labels: pd.Series,
    *,
    horizon_bars: int,
    permutation_count: int = DEFAULT_MULTI_ASSET_STRICT_PERMUTATION_COUNT,
    seed: int = DEFAULT_MULTI_ASSET_STRICT_FALSIFICATION_SEED,
) -> dict[str, Any]:
    if permutation_count <= 0:
        raise ValueError("permutation_count must be positive")
    data = _aligned_clean_frame(signal, labels)
    observed = _series_corr(_series(data["signal"]), _series(data["label"])) if len(data) >= 3 else None
    if observed is None:
        null_metrics: list[float] = []
        empirical_p = 1.0
    else:
        signal_values = _series(data["signal"]).reset_index(drop=True)
        label_values = _series(data["label"]).reset_index(drop=True)
        null_metrics = []
        for offset in range(permutation_count):
            permuted_index = block_permutation_indices(
                length=len(data),
                block_size=int(horizon_bars),
                seed=seed + offset,
            )
            permuted_labels = pd.Series(label_values.to_numpy(dtype=float)[permuted_index])
            metric = _series_corr(signal_values, permuted_labels)
            if metric is not None:
                null_metrics.append(float(metric))
        extreme_count = sum(1 for metric in null_metrics if abs(metric) >= abs(float(observed)))
        empirical_p = (extreme_count + 1.0) / (len(null_metrics) + 1.0) if null_metrics else 1.0
    values = pd.Series(null_metrics, dtype=float)
    return {
        "observed_test_metric": observed,
        "permutation_null_median": _finite_or_none(values.median()) if not values.empty else None,
        "permutation_null_p05": _finite_or_none(values.quantile(0.05)) if not values.empty else None,
        "permutation_null_p95": _finite_or_none(values.quantile(0.95)) if not values.empty else None,
        "empirical_p_value": _finite_or_none(empirical_p),
        "null_fragility_status": "null_robust" if empirical_p <= 0.05 else "null_fragile",
        "permutation_count": int(permutation_count),
        "block_size": int(horizon_bars),
        "permutation_block_size_rule": STRICT_PERMUTATION_BLOCK_SIZE_RULE,
        "p_value_approximation_boundary": f"add_one_empirical_resolution_1_over_{permutation_count + 1}",
    }


def benjamini_hochberg_q_values(
    p_values: Iterable[float | None],
    *,
    alpha: float = 0.05,
) -> tuple[list[float | None], list[bool]]:
    values = list(p_values)
    indexed = [
        (idx, float(value))
        for idx, value in enumerate(values)
        if value is not None and math.isfinite(float(value))
    ]
    q_values: list[float | None] = [None for _ in values]
    if not indexed:
        return q_values, [False for _ in q_values]
    total = len(indexed)
    ordered = sorted(indexed, key=lambda item: item[1])
    running = 1.0
    for rank_from_end, (idx, p_value) in enumerate(reversed(ordered), start=1):
        rank = total - rank_from_end + 1
        running = min(running, p_value * total / rank)
        q_values[idx] = round(min(running, 1.0), 12)
    return q_values, [bool(value is not None and value <= alpha) for value in q_values]


def family_passes_all_horizons(rows: Iterable[dict[str, Any]], *, horizons: Iterable[int]) -> bool:
    by_horizon = {int(row["horizon_bars"]): bool(row.get("horizon_survived")) for row in rows}
    return all(by_horizon.get(int(horizon), False) for horizon in horizons)


def strict_symbol_concentration_status(rows: Iterable[dict[str, Any]]) -> dict[str, Any]:
    values = [
        abs(float(row["test_ic"]))
        for row in rows
        if row.get("test_ic") is not None and math.isfinite(float(row["test_ic"]))
    ]
    signed = [
        float(row["test_ic"])
        for row in rows
        if row.get("test_ic") is not None and math.isfinite(float(row["test_ic"]))
    ]
    if not values:
        return {"symbol_concentration": "unknown", "symbol_consistency_rate": 0.0}
    top_share = max(values) / max(sum(values), 1e-12)
    positive = sum(1 for value in signed if value > 1e-12)
    negative = sum(1 for value in signed if value < -1e-12)
    consistency = max(positive, negative) / len(signed) if signed else 0.0
    concentration = "high" if top_share >= 0.8 or len(values) < 3 else "normal"
    return {"symbol_concentration": concentration, "symbol_consistency_rate": _finite_or_none(consistency)}


def evaluate_multi_asset_strict_oos_fdr_falsification(
    *,
    market_bundles: dict[str, MarketDataBundle],
    funding_bundles: dict[str, FundingRateBundle | None] | None = None,
    source_v8_artifact_path: str | Path = DEFAULT_MULTI_ASSET_STRICT_FALSIFICATION_V8_ARTIFACT,
    funding_status_by_symbol: dict[str, str] | None = None,
    horizons: tuple[int, ...] = FORWARD_HORIZONS,
    fold_count: int = DEFAULT_MULTI_ASSET_STRICT_FOLD_COUNT,
    permutation_count: int = DEFAULT_MULTI_ASSET_STRICT_PERMUTATION_COUNT,
    seed: int = DEFAULT_MULTI_ASSET_STRICT_FALSIFICATION_SEED,
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> MultiAssetStrictFalsificationResult:
    v8_state = read_v8_strict_falsification_source_state(source_v8_artifact_path)
    frozen_queue = materialize_frozen_v8_queue(v8_state, horizons=horizons)
    if not frozen_queue:
        return _empty_v9_result(
            v8_state=v8_state,
            suite_decision="NO_V8_QUEUE_TO_FALSIFY",
            permutation_count=permutation_count,
            commands=commands,
            changed_files=changed_files,
        )
    used_symbols = tuple(str(symbol) for symbol in v8_state["payload"].get("symbol_universe_used", []))
    missing_symbols = [symbol for symbol in used_symbols if symbol not in market_bundles]
    if missing_symbols:
        raise ValueError(
            "V9 strict falsification requires the complete frozen V8 symbol universe; "
            f"missing public OHLCV bundle(s): {missing_symbols}"
        )
    selected_market_bundles = {
        symbol: market_bundles[symbol]
        for symbol in used_symbols
    }
    panel, panel_summary = build_multi_asset_panel(
        selected_market_bundles,
        funding_bundles=funding_bundles or {},
        funding_status_by_symbol=funding_status_by_symbol or {},
    )
    labels_by_horizon = _forward_return_labels(panel, horizons=horizons)
    timestamps = pd.DatetimeIndex(sorted(pd.unique(panel["timestamp"]))) if not panel.empty else pd.DatetimeIndex([])
    folds = build_strict_chronological_folds(timestamps, horizons=horizons, fold_count=fold_count)
    spec_by_id = {
        str(row["signal_id"]): MultiAssetSignalSpec.from_dict(row)
        for row in cast(list[dict[str, Any]], v8_state["catalog_rows"])
    }
    raw_signals: dict[str, pd.Series] = {}
    decision_signals: dict[str, pd.Series] = {}
    for signal_id in sorted({str(row["signal_id"]) for row in frozen_queue}):
        spec = spec_by_id[signal_id]
        raw_signal = build_multi_asset_signal_series(spec, panel)
        decision_signal = lag_multi_asset_signal_for_labels(raw_signal, lag_bars=spec.lag_bars)
        raw_signals[signal_id] = raw_signal
        decision_signals[signal_id] = decision_signal
    diagnostics: list[dict[str, Any]] = []
    permutation_rows: list[dict[str, Any]] = []
    null_rows: list[dict[str, Any]] = []
    regime_rows: list[dict[str, Any]] = []
    symbol_rows: list[dict[str, Any]] = []
    signal_by_family = {
        str(row["signal_family"]): decision_signals[str(row["signal_id"])]
        for row in frozen_queue
    }
    for frozen in frozen_queue:
        signal_id = str(frozen["signal_id"])
        family = str(frozen["signal_family"])
        horizon = int(frozen["horizon_bars"])
        spec = spec_by_id[signal_id]
        signal = decision_signals[signal_id]
        labels = labels_by_horizon[horizon]
        horizon_folds = [fold for fold in folds if int(fold["horizon_bars"]) == horizon]
        row = _strict_signal_diagnostic_row(
            frozen,
            spec=spec,
            raw_signal=raw_signals[signal_id],
            decision_signal=signal,
            labels=labels,
            folds=horizon_folds,
        )
        diagnostics.append(row)
        test_signal, test_labels = _combined_segment(signal, labels, horizon_folds, "test")
        permutation_rows.append(
            {
                "signal_family": family,
                "signal_id": signal_id,
                "horizon_bars": horizon,
                **moving_block_permutation_null(
                    test_signal,
                    test_labels,
                    horizon_bars=horizon,
                    permutation_count=permutation_count,
                    seed=deterministic_random_seed(f"v9-permutation-{signal_id}-{horizon}", seed_base=seed),
                ),
                "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
            }
        )
        null_rows.extend(
            _null_baseline_stress_rows(
                family=family,
                signal_id=signal_id,
                horizon=horizon,
                signal=signal,
                labels=labels,
                folds=horizon_folds,
                signal_by_family=signal_by_family,
                seed=seed,
            )
        )
        regime_rows.extend(
            _regime_robustness_rows(
                family=family,
                signal_id=signal_id,
                horizon=horizon,
                signal=signal,
                labels=labels,
                panel=panel,
                folds=horizon_folds,
            )
        )
        symbol_rows.extend(
            _symbol_robustness_rows(
                family=family,
                signal_id=signal_id,
                horizon=horizon,
                signal=signal,
                labels=labels,
                folds=horizon_folds,
            )
        )
    p_values = [row.get("empirical_p_value") for row in permutation_rows]
    q_values, significant = benjamini_hochberg_q_values(p_values)
    bh_rows: list[dict[str, Any]] = []
    duplicate_adjusted_test_count = len(frozen_queue)
    selection_deflated_alpha = _safe_div(0.05, max(duplicate_adjusted_test_count, 1))
    for row, q_value, is_significant in zip(permutation_rows, q_values, significant, strict=True):
        bh_rows.append(
            {
                "scope": "signal_horizon",
                "signal_family": row["signal_family"],
                "signal_id": row["signal_id"],
                "horizon_bars": row["horizon_bars"],
                "total_signal_tests": len(frozen_queue),
                "total_family_tests": len({item["signal_family"] for item in frozen_queue}),
                "total_horizon_tests": len(set(horizons)),
                "duplicate_adjusted_test_count": duplicate_adjusted_test_count,
                "effective_signal_breadth": len({item["signal_id"] for item in frozen_queue}),
                "raw_p_value": row["empirical_p_value"],
                "bh_q_value": q_value,
                "bh_significant": is_significant,
                "selection_deflated_alpha": selection_deflated_alpha,
                "fdr_boundary": MULTI_ASSET_STRICT_FDR_BOUNDARY,
                "cannot_claim_edge_due_to_multiple_testing": True,
            }
        )
    family_summary = _family_summary_rows(
        diagnostics=diagnostics,
        permutation_rows=permutation_rows,
        bh_rows=bh_rows,
        regime_rows=regime_rows,
        symbol_rows=symbol_rows,
        null_rows=null_rows,
        horizons=horizons,
    )
    bh_rows.extend(_family_bh_rows(family_summary, horizons=horizons))
    failed_gates = _failed_gate_rows(
        family_summary=family_summary,
        diagnostics=diagnostics,
        permutation_rows=permutation_rows,
        panel_empty=panel.empty,
        panel_summary=panel_summary,
    )
    next_queue = _next_mechanism_research_queue(family_summary)
    family_decisions = {
        str(row["signal_family"]): str(row["family_decision"])
        for row in family_summary
    }
    unique_family_decisions = set(family_decisions.values())
    suite_decision = _strict_suite_decision(unique_family_decisions, bool(next_queue))
    _assert_allowed_decisions(suite_decision, family_summary)
    payload = _payload(
        v8_state=v8_state,
        suite_decision=suite_decision,
        frozen_queue=frozen_queue,
        folds=folds,
        diagnostics=diagnostics,
        family_summary=family_summary,
        permutation_count=permutation_count,
        duplicate_adjusted_test_count=duplicate_adjusted_test_count,
        commands=commands,
        changed_files=changed_files,
    )
    return MultiAssetStrictFalsificationResult(
        payload=payload,
        frozen_queue=frozen_queue,
        chronological_folds=folds,
        signal_strict_oos_diagnostics=diagnostics,
        family_strict_summary=family_summary,
        permutation_null_results=permutation_rows,
        bh_fdr_summary=bh_rows,
        regime_robustness=regime_rows,
        symbol_robustness=symbol_rows,
        null_baseline_stress=null_rows,
        failed_gates=failed_gates,
        next_mechanism_research_queue=next_queue,
    )


def prepare_multi_asset_strict_oos_fdr_falsification_output_dir(
    output_dir: str | Path,
    *,
    overwrite: bool = False,
) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="multi-asset-strict-oos-fdr-falsification-run",
        known_files=MULTI_ASSET_STRICT_FALSIFICATION_KNOWN_FILES,
        overwrite=overwrite,
    )


def write_multi_asset_strict_oos_fdr_falsification_artifacts(
    output_dir: str | Path,
    result: MultiAssetStrictFalsificationResult,
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_multi_asset_strict_oos_fdr_falsification_output_dir(output_dir, overwrite=overwrite)
    payload = dict(result.payload)
    if out.name.endswith(MULTI_ASSET_STRICT_FALSIFICATION_LOOP_SLUG):
        payload["loop_id"] = out.name
    _assert_payload_safe(payload)
    _write_json(out / "loop.json", _generic_loop_payload(payload))
    _write_json(out / "multi_asset_strict_oos_fdr_falsification.json", payload)
    _frame_with_columns(result.frozen_queue, FROZEN_QUEUE_COLUMNS).to_csv(out / "frozen_v8_queue.csv", index=False)
    _frame_with_columns(result.chronological_folds, CHRONOLOGICAL_FOLD_COLUMNS).to_csv(
        out / "chronological_folds.csv", index=False
    )
    _frame_with_columns(result.signal_strict_oos_diagnostics, SIGNAL_STRICT_OOS_DIAGNOSTIC_COLUMNS).to_csv(
        out / "signal_strict_oos_diagnostics.csv", index=False
    )
    _frame_with_columns(result.family_strict_summary, FAMILY_STRICT_SUMMARY_COLUMNS).to_csv(
        out / "family_strict_summary.csv", index=False
    )
    _frame_with_columns(result.permutation_null_results, PERMUTATION_NULL_COLUMNS).to_csv(
        out / "permutation_null_results.csv", index=False
    )
    _frame_with_columns(result.bh_fdr_summary, BH_FDR_COLUMNS).to_csv(out / "bh_fdr_summary.csv", index=False)
    _frame_with_columns(result.regime_robustness, REGIME_ROBUSTNESS_COLUMNS).to_csv(
        out / "regime_robustness.csv", index=False
    )
    _frame_with_columns(result.symbol_robustness, SYMBOL_ROBUSTNESS_COLUMNS).to_csv(
        out / "symbol_robustness.csv", index=False
    )
    _frame_with_columns(result.null_baseline_stress, NULL_BASELINE_STRESS_COLUMNS).to_csv(
        out / "null_baseline_stress.csv", index=False
    )
    _frame_with_columns(result.failed_gates, FAILED_GATE_COLUMNS).to_csv(out / "failed_gate_rollup.csv", index=False)
    _frame_with_columns(result.next_mechanism_research_queue, NEXT_MECHANISM_RESEARCH_QUEUE_COLUMNS).to_csv(
        out / "next_mechanism_research_queue.csv", index=False
    )
    _atomic_write_text(out / "report.md", render_multi_asset_strict_oos_fdr_falsification_report(payload, result))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    return out


def render_multi_asset_strict_oos_fdr_falsification_report(
    payload: dict[str, Any],
    result: MultiAssetStrictFalsificationResult,
) -> str:
    family_lines = [
        f"- {row['signal_family']} h{row['horizon_bars']}: {row['family_decision']} "
        f"(test_ic={row.get('median_test_ic')}, q={row.get('bh_family_all_horizon_q_value')})"
        for row in result.family_strict_summary
    ]
    return "\n".join(
        [
            "# MULTI_ASSET_STRICT_OOS_FDR_FALSIFICATION_V9",
            "",
            "## Verdict",
            f"- suite_decision: {payload['suite_decision']}",
            f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
            f"- claim_boundary: {payload['claim_boundary']}",
            "",
            "## Frozen V8 Queue",
            f"- source_v8_artifact_path: {payload['source_v8_artifact_path']}",
            f"- source_v8_queue_count: {payload['source_v8_queue_count']}",
            f"- frozen_queue_count: {payload['frozen_queue_count']}",
            f"- evaluated_signal_count: {payload['evaluated_signal_count']}",
            "",
            "## Strict OOS / FDR",
            f"- chronological_fold_count: {payload['chronological_fold_count']}",
            f"- purge_bars: {payload['purge_bars']}",
            f"- embargo_bars: {payload['embargo_bars']}",
            f"- permutation_count: {payload['permutation_count']}",
            f"- block_size: {payload['block_size']}",
            f"- duplicate_adjusted_test_count: {payload['duplicate_adjusted_test_count']}",
            "",
            "## Family decisions",
            *(family_lines or ["- none"]),
            "",
            "## Safety",
            f"- no_live_orders_submitted: {payload['no_live_orders_submitted']}",
            f"- no_testnet_orders_submitted: {payload['no_testnet_orders_submitted']}",
            f"- no_account_data_used: {payload['no_account_data_used']}",
            f"- no_api_key_used: {payload['no_api_key_used']}",
            f"- no_private_order_or_fill_data_used: {payload['no_private_order_or_fill_data_used']}",
            f"- data_download_performed: {payload['data_download_performed']}",
            f"- tsmom_state_changed: {payload['tsmom_state_changed']}",
            "",
            "## What cannot be claimed",
            "- alpha or edge confirmed",
            "- profitability",
            "- live readiness or execution readiness",
            "- permission to trade, allocate capital, or use live/testnet/account data",
            "- strategy, portfolio, signal-weight, threshold, or horizon optimization",
            "",
        ]
    )


def _select_frozen_representative(
    *,
    family: str,
    candidates: list[MultiAssetSignalSpec],
    cluster_rows: list[dict[str, Any]],
) -> tuple[str, str]:
    representative_ids = {
        str(row.get("representative_signal_id"))
        for row in cluster_rows
        if str(row.get("signal_family")) == family
        and str(row.get("representative_signal_id", ""))
        and not _truthy(row.get("is_redundant_duplicate", False))
    }
    selected = min(
        candidates,
        key=lambda spec: (
            spec.signal_id not in representative_ids,
            spec.complexity_score,
            spec.depth,
            spec.signal_id,
        ),
    )
    source = "v8_cluster_representative" if selected.signal_id in representative_ids else "catalog_nonperformance_sort"
    return selected.signal_id, source


def _strict_signal_diagnostic_row(
    frozen: dict[str, Any],
    *,
    spec: MultiAssetSignalSpec,
    raw_signal: pd.Series,
    decision_signal: pd.Series,
    labels: pd.Series,
    folds: list[dict[str, Any]],
) -> dict[str, Any]:
    train_metrics = [_segment_metrics(decision_signal, labels, fold, "train") for fold in folds]
    validation_metrics = [_segment_metrics(decision_signal, labels, fold, "validation") for fold in folds]
    test_metrics = [_segment_metrics(decision_signal, labels, fold, "test") for fold in folds]
    test_ics = [metric["ic"] for metric in test_metrics if metric["ic"] is not None]
    validation_ics = [metric["ic"] for metric in validation_metrics if metric["ic"] is not None]
    fold_consistency = _sign_consistency_rate(test_ics)
    signal = _coerce_float_series(decision_signal)
    non_null = signal.dropna()
    leakage_status = (
        "failed_decision_lag_mismatch"
        if _decision_signal_lag_mismatch(raw_signal, decision_signal, lag_bars=spec.lag_bars)
        else "passed_shifted_by_decision_lag"
    )
    return {
        "signal_id": spec.signal_id,
        "signal_family": spec.signal_family,
        "symbol_scope": ";".join(spec.symbol_scope),
        "pair_scope": "" if spec.pair_scope is None else ";".join(spec.pair_scope),
        "horizon_bars": int(frozen["horizon_bars"]),
        "depth": spec.depth,
        "complexity_score": spec.complexity_score,
        "lag_bars": spec.lag_bars,
        "non_null_observation_count": int(len(non_null)),
        "coverage_ratio": _safe_div(len(non_null), len(signal)),
        "missing_ratio": 1.0 - _safe_div(len(non_null), len(signal)),
        "train_ic": _median([metric["ic"] for metric in train_metrics]),
        "validation_ic": _median(validation_ics),
        "test_ic": _median(test_ics),
        "train_rank_ic": _median([metric["rank_ic"] for metric in train_metrics]),
        "validation_rank_ic": _median([metric["rank_ic"] for metric in validation_metrics]),
        "test_rank_ic": _median([metric["rank_ic"] for metric in test_metrics]),
        "validation_hit_rate": _median([metric["hit_rate"] for metric in validation_metrics]),
        "test_hit_rate": _median([metric["hit_rate"] for metric in test_metrics]),
        "validation_coverage": _median([metric["coverage"] for metric in validation_metrics]),
        "test_coverage": _median([metric["coverage"] for metric in test_metrics]),
        "validation_turnover_proxy": _median([metric["turnover_proxy"] for metric in validation_metrics]),
        "test_turnover_proxy": _median([metric["turnover_proxy"] for metric in test_metrics]),
        "validation_missing_ratio": _median([metric["missing_ratio"] for metric in validation_metrics]),
        "test_missing_ratio": _median([metric["missing_ratio"] for metric in test_metrics]),
        "validation_to_test_degradation": _degradation(_median(validation_ics), _median(test_ics)),
        "sign_consistency_train_to_test": _same_sign(
            _median([metric["ic"] for metric in train_metrics]),
            _median(test_ics),
        ),
        "fold_consistency_rate": fold_consistency,
        "worst_fold_test_ic": min(test_ics) if test_ics else None,
        "median_fold_test_ic": _median(test_ics),
        "chronological_oos_status": "evaluated" if test_ics else "insufficient_test_observations",
        "leakage_check_status": leakage_status,
        "failure_reason": None if test_ics and leakage_status.startswith("passed") else "insufficient_or_leaky_signal",
        "horizon_aggregation_rule": STRICT_HORIZON_AGGREGATION_RULE,
        "no_best_horizon_selection": True,
        "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
    }


def _segment_metrics(signal: pd.Series, labels: pd.Series, fold: dict[str, Any], segment: str) -> dict[str, Any]:
    segment_signal, segment_labels = _segment_series(signal, labels, fold, segment)
    clean = _aligned_clean_frame(segment_signal, segment_labels)
    ic = _series_corr(_series(clean["signal"]), _series(clean["label"])) if len(clean) >= 3 else None
    rank_ic = (
        _series_corr(_series(clean["signal"]).rank(), _series(clean["label"]).rank())
        if len(clean) >= 3
        else None
    )
    hit_rate = _sign_hit_rate(_series(clean["signal"]), _series(clean["label"])) if len(clean) else None
    coverage = _safe_div(len(clean), len(segment_signal))
    return {
        "ic": ic,
        "rank_ic": rank_ic,
        "hit_rate": hit_rate,
        "coverage": coverage,
        "missing_ratio": 1.0 - coverage,
        "turnover_proxy": _turnover_proxy(segment_signal),
    }


def _segment_series(
    signal: pd.Series,
    labels: pd.Series,
    fold: dict[str, Any],
    segment: str,
) -> tuple[pd.Series, pd.Series]:
    if not isinstance(signal.index, pd.MultiIndex) or "timestamp" not in signal.index.names:
        return pd.Series(dtype=float), pd.Series(dtype=float)
    timestamps = pd.DatetimeIndex(sorted(pd.unique(signal.index.get_level_values("timestamp"))))
    start = int(fold[f"{segment}_start_pos"])
    end = int(fold[f"{segment}_end_pos"])
    if start >= len(timestamps) or end > len(timestamps):
        return pd.Series(dtype=float), pd.Series(dtype=float)
    selected = set(timestamps[start:end])
    mask = signal.index.get_level_values("timestamp").isin(selected)
    return _coerce_float_series(signal[mask]), _coerce_float_series(labels.reindex(signal.index)[mask])


def _combined_segment(
    signal: pd.Series,
    labels: pd.Series,
    folds: list[dict[str, Any]],
    segment: str,
) -> tuple[pd.Series, pd.Series]:
    signal_parts: list[pd.Series] = []
    label_parts: list[pd.Series] = []
    for fold in folds:
        segment_signal, segment_labels = _segment_series(signal, labels, fold, segment)
        signal_parts.append(segment_signal)
        label_parts.append(segment_labels)
    if not signal_parts:
        return pd.Series(dtype=float), pd.Series(dtype=float)
    return pd.concat(signal_parts).sort_index(), pd.concat(label_parts).sort_index()


def _null_baseline_stress_rows(
    *,
    family: str,
    signal_id: str,
    horizon: int,
    signal: pd.Series,
    labels: pd.Series,
    folds: list[dict[str, Any]],
    signal_by_family: dict[str, pd.Series],
    seed: int,
) -> list[dict[str, Any]]:
    test_signal, test_labels = _combined_segment(signal, labels, folds, "test")
    observed = _series_corr(test_signal, test_labels)
    families = sorted(signal_by_family)
    family_index = families.index(family)
    family_permuted = signal_by_family[families[(family_index + 1) % len(families)]] if len(families) > 1 else None
    random_signal, random_seed = deterministic_random_sign_signal(
        signal,
        f"v9-{signal_id}-{horizon}",
        seed_base=seed,
    )
    shuffled, shuffled_seed = shuffled_timestamp_signal(signal, f"v9-{signal_id}-{horizon}", seed_base=seed)
    baselines: list[tuple[str, pd.Series | None, str, int | None, int | None]] = [
        ("zero_cash_null", pd.Series(0.0, index=signal.index, dtype=float), "available", None, None),
        ("buy_and_hold_same_window_baseline", None, "unavailable_constant_position_no_signal_ic", None, None),
        ("deterministic_random_sign", random_signal, "available", random_seed, None),
        ("lagged_signal", lagged_signal_position(signal, lag_bars=max(2, horizon // 8 + 1)), "available", None, max(2, horizon // 8 + 1)),
        ("inverse_signal", inverse_signal_position(signal), "available", None, None),
        ("shuffled_timestamp_signal", shuffled, "available", shuffled_seed, None),
        ("family_permuted_signal", family_permuted, "available" if family_permuted is not None else "unavailable_single_family", None, None),
        (
            "symbol_permuted_signal",
            symbol_permuted_signal(signal, f"v9-{signal_id}-{horizon}", seed_base=seed),
            "available",
            deterministic_random_seed(f"symbol-permute-v9-{signal_id}-{horizon}", seed_base=seed),
            None,
        ),
        ("universe_median_signal", _universe_median_signal_like(signal), "available", None, None),
        ("frozen_tsmom_negative_control", None, "unavailable_no_v9_tsmom_input", None, None),
    ]
    rows: list[dict[str, Any]] = []
    for baseline_name, baseline_signal, status, deterministic_seed, lag_bars in baselines:
        baseline_test_signal, baseline_test_labels = (
            _combined_segment(baseline_signal, labels, folds, "test")
            if baseline_signal is not None
            else (pd.Series(dtype=float), pd.Series(dtype=float))
        )
        baseline_metric = (
            _series_corr(baseline_test_signal, baseline_test_labels)
            if baseline_signal is not None
            else None
        )
        rows.append(
            {
                "signal_family": family,
                "signal_id": signal_id,
                "horizon_bars": horizon,
                "baseline_name": baseline_name,
                "observed_test_metric": observed,
                "baseline_test_metric": baseline_metric,
                "signal_beats_baseline": _beats_baseline(observed, baseline_metric),
                "baseline_status": status,
                "deterministic_seed": deterministic_seed,
                "lag_bars": lag_bars,
                "baseline_uses_current_bar_label": False,
                "baseline_uses_current_bar_signal": False,
                "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
            }
        )
    return rows


def _regime_robustness_rows(
    *,
    family: str,
    signal_id: str,
    horizon: int,
    signal: pd.Series,
    labels: pd.Series,
    panel: pd.DataFrame,
    folds: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    if panel.empty:
        return []
    test_signal, test_labels = _combined_segment(signal, labels, folds, "test")
    base = pd.concat([test_signal.rename("signal"), test_labels.rename("label")], axis=1)
    panel_features = _frame(panel.reindex(base.index))
    regimes = _regime_masks(panel_features)
    rows: list[dict[str, Any]] = []
    overall_ic = _series_corr(test_signal, test_labels)
    for regime_name, mask in regimes.items():
        regime_mask = _series(mask.reindex(base.index)).fillna(False).astype(bool)
        regime_signal = _coerce_float_series(base.loc[regime_mask, "signal"])
        regime_labels = _coerce_float_series(base.loc[regime_mask, "label"])
        sample_count = int(pd.concat([regime_signal, regime_labels], axis=1).dropna().shape[0])
        metric = _series_corr(regime_signal, regime_labels) if sample_count >= 3 else None
        rows.append(
            {
                "signal_family": family,
                "signal_id": signal_id,
                "horizon_bars": horizon,
                "regime": regime_name,
                "regime_sample_count": sample_count,
                "median_test_ic": metric,
                "sign_consistency": _same_sign(overall_ic, metric),
                "hit_rate": _sign_hit_rate(regime_signal, regime_labels) if sample_count else None,
                "null_status": "evaluated" if sample_count >= 3 else "insufficient_regime_sample",
                "failure_reason": None if sample_count >= 3 else "insufficient_regime_sample",
                "regime_uses_forward_labels": False,
                "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
            }
        )
    return rows


def _symbol_robustness_rows(
    *,
    family: str,
    signal_id: str,
    horizon: int,
    signal: pd.Series,
    labels: pd.Series,
    folds: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    test_signal, test_labels = _combined_segment(signal, labels, folds, "test")
    if not isinstance(test_signal.index, pd.MultiIndex) or "symbol" not in test_signal.index.names:
        return []
    rows: list[dict[str, Any]] = []
    symbol_metrics: list[dict[str, Any]] = []
    for symbol in sorted(set(str(item) for item in test_signal.index.get_level_values("symbol"))):
        symbol_signal = _coerce_float_series(test_signal.xs(symbol, level="symbol", drop_level=False))
        symbol_labels = _coerce_float_series(test_labels.reindex(symbol_signal.index))
        clean = _aligned_clean_frame(symbol_signal, symbol_labels)
        metric = _series_corr(_series(clean["signal"]), _series(clean["label"])) if len(clean) >= 3 else None
        symbol_metrics.append({"symbol": symbol, "test_ic": metric})
    concentration = strict_symbol_concentration_status(symbol_metrics)
    median_metric = _median([row.get("test_ic") for row in symbol_metrics])
    worst_metric = _min_or_none([row.get("test_ic") for row in symbol_metrics])
    pooled_signal, pooled_labels = test_signal, test_labels
    drop_one: list[float] = []
    for item in symbol_metrics:
        symbol = str(item["symbol"])
        mask = pooled_signal.index.get_level_values("symbol") != symbol
        dropped_metric = _series_corr(_coerce_float_series(pooled_signal[mask]), _coerce_float_series(pooled_labels[mask]))
        if dropped_metric is not None:
            drop_one.append(float(dropped_metric))
    for item in symbol_metrics:
        rows.append(
            {
                "signal_family": family,
                "signal_id": signal_id,
                "horizon_bars": horizon,
                "symbol": item["symbol"],
                "per_symbol_test_ic": item["test_ic"],
                "per_symbol_hit_rate": _symbol_hit_rate(test_signal, test_labels, str(item["symbol"])),
                "symbol_concentration": concentration["symbol_concentration"],
                "worst_symbol_metric": worst_metric,
                "median_symbol_metric": median_metric,
                "symbol_consistency_rate": concentration["symbol_consistency_rate"],
                "symbol_drop_one_robustness": _median(drop_one),
                "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
            }
        )
    return rows


def _family_summary_rows(
    *,
    diagnostics: list[dict[str, Any]],
    permutation_rows: list[dict[str, Any]],
    bh_rows: list[dict[str, Any]],
    regime_rows: list[dict[str, Any]],
    symbol_rows: list[dict[str, Any]],
    null_rows: list[dict[str, Any]],
    horizons: tuple[int, ...],
) -> list[dict[str, Any]]:
    diagnostic_frame = pd.DataFrame(diagnostics)
    bh_frame = pd.DataFrame(bh_rows)
    regime_frame = pd.DataFrame(regime_rows)
    symbol_frame = pd.DataFrame(symbol_rows)
    null_frame = pd.DataFrame(null_rows)
    rows: list[dict[str, Any]] = []
    for family in sorted(set(str(row["signal_family"]) for row in diagnostics)):
        family_horizon_rows: list[dict[str, Any]] = []
        q_values = pd.to_numeric(
            bh_frame.loc[bh_frame["signal_family"] == family, "bh_q_value"],
            errors="coerce",
        ).dropna()
        family_all_q = _finite_or_none(q_values.max()) if not q_values.empty else None
        for horizon in horizons:
            gf = diagnostic_frame[
                (diagnostic_frame["signal_family"] == family)
                & (diagnostic_frame["horizon_bars"] == int(horizon))
            ]
            bf = bh_frame[(bh_frame["signal_family"] == family) & (bh_frame["horizon_bars"] == int(horizon))]
            nf = null_frame[(null_frame["signal_family"] == family) & (null_frame["horizon_bars"] == int(horizon))]
            rf = regime_frame[(regime_frame["signal_family"] == family) & (regime_frame["horizon_bars"] == int(horizon))]
            sf = symbol_frame[(symbol_frame["signal_family"] == family) & (symbol_frame["horizon_bars"] == int(horizon))]
            null_rate = _boolean_rate(nf.get("signal_beats_baseline", pd.Series(dtype=object)))
            regime_rate = _boolean_rate(rf.get("sign_consistency", pd.Series(dtype=object)))
            symbol_rate = _finite_or_none(pd.to_numeric(sf.get("symbol_consistency_rate", pd.Series(dtype=float)), errors="coerce").median())
            bh_significant = bool(bf["bh_significant"].all()) if not bf.empty else False
            horizon_decision = _horizon_decision(
                diagnostic_rows=gf.to_dict(orient="records"),
                null_rate=null_rate,
                regime_rate=regime_rate,
                symbol_rate=symbol_rate,
                symbol_frame=sf,
                bh_significant=bh_significant,
            )
            family_horizon_rows.append(
                {
                    "signal_family": family,
                    "horizon_bars": int(horizon),
                    "horizon_survived": horizon_decision in {"NEEDS_MECHANISM_RESEARCH", "WATCH_FAMILY_ONLY"},
                    "horizon_decision": horizon_decision,
                }
            )
        final_decision = _family_decision_from_horizons(family_horizon_rows)
        for horizon_row in family_horizon_rows:
            horizon = int(horizon_row["horizon_bars"])
            gf = diagnostic_frame[
                (diagnostic_frame["signal_family"] == family)
                & (diagnostic_frame["horizon_bars"] == horizon)
            ]
            rf = regime_frame[(regime_frame["signal_family"] == family) & (regime_frame["horizon_bars"] == horizon)]
            sf = symbol_frame[(symbol_frame["signal_family"] == family) & (symbol_frame["horizon_bars"] == horizon)]
            nf = null_frame[(null_frame["signal_family"] == family) & (null_frame["horizon_bars"] == horizon)]
            bf = bh_frame[(bh_frame["signal_family"] == family) & (bh_frame["horizon_bars"] == horizon)]
            rows.append(
                {
                    "signal_family": family,
                    "horizon_bars": horizon,
                    "queued_signal_count": int(len(gf)),
                    "evaluated_signal_count": int((gf["chronological_oos_status"] == "evaluated").sum()) if not gf.empty else 0,
                    "duplicate_adjusted_signal_count": int(len(gf)),
                    "effective_signal_breadth": int(len(set(gf["signal_id"]))) if not gf.empty else 0,
                    "median_test_ic": _finite_or_none(pd.to_numeric(gf.get("test_ic", pd.Series(dtype=float)), errors="coerce").median()) if not gf.empty else None,
                    "mean_test_ic": _finite_or_none(pd.to_numeric(gf.get("test_ic", pd.Series(dtype=float)), errors="coerce").mean()) if not gf.empty else None,
                    "worst_fold_test_ic": _finite_or_none(pd.to_numeric(gf.get("worst_fold_test_ic", pd.Series(dtype=float)), errors="coerce").min()) if not gf.empty else None,
                    "worst_symbol_test_ic": _finite_or_none(pd.to_numeric(sf.get("worst_symbol_metric", pd.Series(dtype=float)), errors="coerce").min()) if not sf.empty else None,
                    "sign_consistency_rate": _sign_consistency_rate(pd.to_numeric(gf.get("test_ic", pd.Series(dtype=float)), errors="coerce").dropna().tolist()),
                    "fold_consistency_rate": _finite_or_none(pd.to_numeric(gf.get("fold_consistency_rate", pd.Series(dtype=float)), errors="coerce").median()) if not gf.empty else None,
                    "symbol_consistency_rate": _finite_or_none(pd.to_numeric(sf.get("symbol_consistency_rate", pd.Series(dtype=float)), errors="coerce").median()) if not sf.empty else None,
                    "regime_survival_rate": _boolean_rate(rf.get("sign_consistency", pd.Series(dtype=object))),
                    "null_robustness_rate": _boolean_rate(nf.get("signal_beats_baseline", pd.Series(dtype=object))),
                    "bh_min_q_value": _finite_or_none(pd.to_numeric(bf.get("bh_q_value", pd.Series(dtype=float)), errors="coerce").min()) if not bf.empty else None,
                    "bh_family_all_horizon_q_value": family_all_q,
                    "bh_significant_count": int((bf.get("bh_significant", pd.Series(dtype=bool)) == True).sum()) if not bf.empty else 0,  # noqa: E712
                    "validation_to_test_degradation": _finite_or_none(pd.to_numeric(gf.get("validation_to_test_degradation", pd.Series(dtype=float)), errors="coerce").median()) if not gf.empty else None,
                    "turnover_proxy_median": _finite_or_none(pd.to_numeric(gf.get("test_turnover_proxy", pd.Series(dtype=float)), errors="coerce").median()) if not gf.empty else None,
                    "high_turnover_warning": bool(pd.to_numeric(gf.get("test_turnover_proxy", pd.Series(dtype=float)), errors="coerce").median() > 1.0) if not gf.empty else False,
                    "horizon_survived": horizon_row["horizon_survived"],
                    "horizon_decision": horizon_row["horizon_decision"],
                    "horizon_aggregation_rule": STRICT_HORIZON_AGGREGATION_RULE,
                    "family_decision": final_decision,
                    "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
                }
            )
    return rows


def _horizon_decision(
    *,
    diagnostic_rows: list[dict[str, Any]],
    null_rate: float | None,
    regime_rate: float | None,
    symbol_rate: float | None,
    symbol_frame: pd.DataFrame,
    bh_significant: bool,
) -> str:
    if not diagnostic_rows or any(row.get("chronological_oos_status") != "evaluated" for row in diagnostic_rows):
        return "KILL_FAMILY"
    for row in diagnostic_rows:
        if row.get("sign_consistency_train_to_test") is False:
            return "FALSIFIED_SIGN_FLIP"
    if null_rate is None or null_rate < 0.5:
        return "FALSIFIED_NULL_FRAGILE"
    fold_rate = _finite_or_none(pd.Series([row.get("fold_consistency_rate") for row in diagnostic_rows], dtype=float).median())
    if fold_rate is None or fold_rate < 0.5:
        return "FALSIFIED_OOS_INCONSISTENT"
    if regime_rate is None or regime_rate < 0.5:
        return "FALSIFIED_REGIME_FRAGILE"
    concentration_series = (
        symbol_frame["symbol_concentration"]
        if "symbol_concentration" in symbol_frame
        else pd.Series(dtype=object)
    )
    concentration_values = set(str(value) for value in concentration_series)
    if "high" in concentration_values or symbol_rate is None or symbol_rate < 0.5:
        return "FALSIFIED_SYMBOL_CONCENTRATED"
    if not bh_significant:
        return "FALSIFIED_NULL_FRAGILE"
    return "NEEDS_MECHANISM_RESEARCH"


def _family_decision_from_horizons(rows: list[dict[str, Any]]) -> str:
    if family_passes_all_horizons(rows, horizons=FORWARD_HORIZONS):
        return "NEEDS_MECHANISM_RESEARCH"
    precedence = [
        "KILL_FAMILY",
        "DUPLICATE_DOMINATED",
        "FALSIFIED_SIGN_FLIP",
        "FALSIFIED_NULL_FRAGILE",
        "FALSIFIED_OOS_INCONSISTENT",
        "FALSIFIED_REGIME_FRAGILE",
        "FALSIFIED_SYMBOL_CONCENTRATED",
    ]
    decisions = {str(row["horizon_decision"]) for row in rows}
    for decision in precedence:
        if decision in decisions:
            return decision
    return "KILL_FAMILY"


def _family_bh_rows(family_summary: list[dict[str, Any]], *, horizons: Iterable[int]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for family in sorted(set(str(row["signal_family"]) for row in family_summary)):
        group = [row for row in family_summary if str(row["signal_family"]) == family]
        q = _finite_or_none(max([float(row["bh_family_all_horizon_q_value"]) for row in group if row.get("bh_family_all_horizon_q_value") is not None], default=math.nan))
        rows.append(
            {
                "scope": "family_all_horizon",
                "signal_family": family,
                "signal_id": "",
                "horizon_bars": ";".join(str(item) for item in horizons),
                "total_signal_tests": len(family_summary),
                "total_family_tests": len({row["signal_family"] for row in family_summary}),
                "total_horizon_tests": len(set(horizons)),
                "duplicate_adjusted_test_count": len(family_summary),
                "effective_signal_breadth": len({row["signal_family"] for row in family_summary}),
                "raw_p_value": None,
                "bh_q_value": q,
                "bh_significant": bool(q is not None and q <= 0.05),
                "selection_deflated_alpha": _safe_div(0.05, max(len(family_summary), 1)),
                "horizon_aggregation_rule": STRICT_HORIZON_AGGREGATION_RULE,
                "fdr_boundary": MULTI_ASSET_STRICT_FDR_BOUNDARY,
                "cannot_claim_edge_due_to_multiple_testing": True,
            }
        )
    return rows


def _next_mechanism_research_queue(family_summary: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    seen: set[str] = set()
    for row in family_summary:
        decision = str(row["family_decision"])
        family = str(row["signal_family"])
        if family in seen or decision not in {"NEEDS_MECHANISM_RESEARCH", "WATCH_FAMILY_ONLY"}:
            continue
        seen.add(family)
        rows.append(
            {
                "signal_family": family,
                "queue_decision": decision,
                "paper_only": True,
                "reason": "paper-only mechanism research only; no edge/readiness claim",
                "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
            }
        )
    return rows


def _payload(
    *,
    v8_state: dict[str, Any],
    suite_decision: str,
    frozen_queue: list[dict[str, Any]],
    folds: list[dict[str, Any]],
    diagnostics: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
    permutation_count: int,
    duplicate_adjusted_test_count: int,
    commands: Iterable[str],
    changed_files: Iterable[str],
) -> dict[str, Any]:
    source_queue_count = len(cast(list[dict[str, Any]], v8_state.get("queue_rows", [])))
    families = sorted({str(row["signal_family"]) for row in frozen_queue})
    family_decisions = {str(row["signal_family"]): str(row["family_decision"]) for row in family_summary}
    killed = sum(
        1
        for decision in family_decisions.values()
        if decision.startswith("FALSIFIED") or decision == "KILL_FAMILY"
    )
    watch = sum(
        1
        for decision in family_decisions.values()
        if decision in {"NEEDS_MECHANISM_RESEARCH", "WATCH_FAMILY_ONLY"}
    )
    negative = sum(1 for decision in family_decisions.values() if decision == "FREEZE_AS_NEGATIVE_CONTROL")
    return {
        "schema_version": MULTI_ASSET_STRICT_FALSIFICATION_SCHEMA_VERSION,
        "loop_id": f"{datetime.now(UTC).strftime('%Y%m%d')}-{MULTI_ASSET_STRICT_FALSIFICATION_LOOP_SLUG}",
        "loop_namespace": MULTI_ASSET_STRICT_FALSIFICATION_NAMESPACE,
        "suite_decision": suite_decision,
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "source_v8_artifact_path": str(v8_state["artifact_dir"]),
        "source_v8_queue_count": int(source_queue_count),
        "frozen_queue_count": int(len(frozen_queue)),
        "evaluated_signal_count": int(len(diagnostics)),
        "evaluated_family_count": int(len(families)),
        "family_count": int(len(families)),
        "horizon_count": int(len({int(row["horizon_bars"]) for row in frozen_queue})),
        "chronological_fold_count": int(len(folds)),
        "purge_bars": int(max([int(row["purge_bars"]) for row in folds], default=max(FORWARD_HORIZONS))),
        "embargo_bars": int(max([int(row["embargo_bars"]) for row in folds], default=max(FORWARD_HORIZONS))),
        "permutation_count": int(permutation_count),
        "block_size": "equals_horizon_bars",
        "block_size_values": sorted({int(row["horizon_bars"]) for row in frozen_queue}),
        "total_signal_tests": int(len(frozen_queue)),
        "duplicate_adjusted_test_count": int(duplicate_adjusted_test_count),
        "effective_signal_breadth": int(len({row["signal_id"] for row in frozen_queue})),
        "families_killed_count": int(killed),
        "families_watch_count": int(watch),
        "families_negative_control_count": int(negative),
        "multiple_testing_boundary": MULTI_ASSET_STRICT_MULTIPLE_TESTING_BOUNDARY,
        "cannot_claim_edge_due_to_multiple_testing": True,
        "fdr_boundary": MULTI_ASSET_STRICT_FDR_BOUNDARY,
        "leakage_boundary": MULTI_ASSET_STRICT_LEAKAGE_BOUNDARY,
        "claim_boundary": MULTI_ASSET_STRICT_CLAIM_BOUNDARY,
        "safety_gates": _safety_gates(),
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "tsmom_state_changed": False,
        "holdout_retune_used": False,
        "strategy_optimization_performed": False,
        "live_or_testnet_path_created": False,
        "data_download_performed": False,
        "validation_commands": list(commands),
        "changed_files": list(changed_files),
    }


def _empty_v9_result(
    *,
    v8_state: dict[str, Any],
    suite_decision: str,
    permutation_count: int,
    commands: Iterable[str],
    changed_files: Iterable[str],
) -> MultiAssetStrictFalsificationResult:
    payload = _payload(
        v8_state=v8_state,
        suite_decision=suite_decision,
        frozen_queue=[],
        folds=[],
        diagnostics=[],
        family_summary=[],
        permutation_count=permutation_count,
        duplicate_adjusted_test_count=0,
        commands=commands,
        changed_files=changed_files,
    )
    payload["source_v8_queue_count"] = len(cast(list[dict[str, Any]], v8_state.get("queue_rows", [])))
    return MultiAssetStrictFalsificationResult(
        payload=payload,
        frozen_queue=[],
        chronological_folds=[],
        signal_strict_oos_diagnostics=[],
        family_strict_summary=[],
        permutation_null_results=[],
        bh_fdr_summary=[],
        regime_robustness=[],
        symbol_robustness=[],
        null_baseline_stress=[],
        failed_gates=[
            {
                "gate": "v8_frozen_queue",
                "status": "failed_safe",
                "reason": "V8 next_strict_falsification_queue.csv has no rows; V9 generated no signals and did not rerun V8",
            }
        ],
        next_mechanism_research_queue=[],
    )


def _failed_gate_rows(
    *,
    family_summary: list[dict[str, Any]],
    diagnostics: list[dict[str, Any]],
    permutation_rows: list[dict[str, Any]],
    panel_empty: bool,
    panel_summary: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = [
        {
            "gate": "v8_queue_frozen",
            "status": "passed",
            "reason": "V9 evaluated only representatives derived from V8 next_strict_falsification_queue.csv and V8 catalog",
        },
        {
            "gate": "data_download",
            "status": "passed",
            "reason": "V9 does not download or backfill data",
        },
        {
            "gate": "readiness_boundary",
            "status": "passed",
            "reason": "readiness_baseline_verdict remains NOT_READY",
        },
        {
            "gate": "tsmom_state_unchanged",
            "status": "passed",
            "reason": "TSMOM code/artifacts are not changed by V9",
        },
        {
            "gate": "multiple_testing_boundary",
            "status": "unclaimed",
            "reason": "cannot_claim_edge_due_to_multiple_testing remains true",
        },
    ]
    if panel_empty:
        rows.append({"gate": "public_panel", "status": "failed", "reason": "no public OHLCV panel rows available"})
    if any(row.get("leakage_check_status") != "passed_shifted_by_decision_lag" for row in diagnostics):
        rows.append({"gate": "leakage_check", "status": "failed", "reason": "one or more signals failed lag check"})
    if any(row.get("empirical_p_value") is None for row in permutation_rows):
        rows.append({"gate": "permutation_null", "status": "watch", "reason": "one or more permutation null rows lacked enough observations"})
    killed = [row for row in family_summary if str(row.get("family_decision", "")).startswith("FALSIFIED") or row.get("family_decision") == "KILL_FAMILY"]
    if killed:
        rows.append({"gate": "family_survival", "status": "failed", "reason": f"{len(killed)} family x horizon rows failed strict falsification"})
    if panel_summary:
        rows.append({"gate": "public_panel_symbols", "status": "passed", "reason": f"{len(panel_summary)} cached public symbols loaded"})
    return rows


def _strict_suite_decision(decisions: set[str], has_next_queue: bool) -> str:
    if has_next_queue and "NEEDS_MECHANISM_RESEARCH" in decisions:
        return "NEEDS_MECHANISM_RESEARCH"
    if has_next_queue and "WATCH_FAMILY_ONLY" in decisions:
        return "WATCH_FAMILY_ONLY_PAPER_RESEARCH"
    if "FALSIFIED_OOS_INCONSISTENT" in decisions:
        return "FAILS_OOS_ROBUSTNESS"
    if "FALSIFIED_NULL_FRAGILE" in decisions:
        return "FAILS_FDR_BOUNDARY"
    if decisions:
        return "NO_FAMILY_SURVIVES_STRICT_FALSIFICATION"
    return "MULTI_ASSET_STRICT_FALSIFICATION_INFRA_ONLY"


def _assert_allowed_decisions(suite_decision: str, family_summary: list[dict[str, Any]]) -> None:
    if suite_decision not in MULTI_ASSET_STRICT_FALSIFICATION_SUITE_DECISIONS:
        raise ValueError(f"forbidden V9 suite decision emitted: {suite_decision}")
    if suite_decision in MULTI_ASSET_STRICT_FALSIFICATION_FORBIDDEN_OUTPUTS:
        raise ValueError(f"forbidden V9 output emitted: {suite_decision}")
    for row in family_summary:
        decision = str(row["family_decision"])
        if decision not in MULTI_ASSET_STRICT_FALSIFICATION_FAMILY_DECISIONS:
            raise ValueError(f"forbidden V9 family decision emitted: {decision}")
        if decision in MULTI_ASSET_STRICT_FALSIFICATION_FORBIDDEN_OUTPUTS:
            raise ValueError(f"forbidden V9 output emitted as family decision: {decision}")


def _assert_payload_safe(payload: dict[str, Any]) -> None:
    required = {
        "loop_id",
        "loop_namespace",
        "suite_decision",
        "readiness_baseline_verdict",
        "source_v8_artifact_path",
        "source_v8_queue_count",
        "frozen_queue_count",
        "evaluated_signal_count",
        "evaluated_family_count",
        "family_count",
        "horizon_count",
        "chronological_fold_count",
        "purge_bars",
        "embargo_bars",
        "permutation_count",
        "block_size",
        "total_signal_tests",
        "duplicate_adjusted_test_count",
        "effective_signal_breadth",
        "families_killed_count",
        "families_watch_count",
        "families_negative_control_count",
        "multiple_testing_boundary",
        "cannot_claim_edge_due_to_multiple_testing",
        "fdr_boundary",
        "leakage_boundary",
        "claim_boundary",
        "safety_gates",
        "tsmom_state_changed",
        "holdout_retune_used",
        "strategy_optimization_performed",
        "live_or_testnet_path_created",
        "data_download_performed",
        "validation_commands",
        "changed_files",
    }
    missing = sorted(required - set(payload))
    if missing:
        raise ValueError(f"V9 payload missing required field(s): {missing}")
    if payload["loop_namespace"] != MULTI_ASSET_STRICT_FALSIFICATION_NAMESPACE:
        raise ValueError("V9 payload has wrong loop_namespace")
    if payload["readiness_baseline_verdict"] != READINESS_BASELINE_VERDICT:
        raise ValueError("V9 payload must preserve readiness_baseline_verdict=NOT_READY")
    if payload["cannot_claim_edge_due_to_multiple_testing"] is not True:
        raise ValueError("V9 payload must preserve cannot_claim_edge_due_to_multiple_testing=true")
    for field, expected in (
        ("no_live_orders_submitted", True),
        ("no_testnet_orders_submitted", True),
        ("no_account_data_used", True),
        ("no_api_key_used", True),
        ("no_private_order_or_fill_data_used", True),
        ("holdout_retune_used", False),
        ("strategy_optimization_performed", False),
        ("live_or_testnet_path_created", False),
        ("data_download_performed", False),
        ("tsmom_state_changed", False),
    ):
        if payload.get(field) is not expected:
            raise ValueError(f"V9 payload safety field {field} must be {expected}")
    _assert_allowed_decisions(str(payload["suite_decision"]), [])


def _generic_loop_payload(payload: dict[str, Any]) -> dict[str, Any]:
    suite_decision = str(payload["suite_decision"])
    if suite_decision == "NO_FAMILY_SURVIVES_STRICT_FALSIFICATION":
        status = "KILLED"
    elif suite_decision in {"NO_V8_QUEUE_TO_FALSIFY", "NEEDS_MECHANISM_RESEARCH", "WATCH_FAMILY_ONLY_PAPER_RESEARCH"}:
        status = "PARKED"
    else:
        status = "IMPLEMENTED"
    return {
        "loop_id": payload["loop_id"],
        "status": status,
        "loop_type": "validation",
        "hypothesis": (
            "Frozen V8 multi-asset paper-only signal families may fail stricter chronological "
            "OOS, FDR, null, regime, and symbol robustness checks."
        ),
        "edge_source": "information_processing",
        "suite_decision": suite_decision,
        "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
        "source_v8_artifact_path": payload["source_v8_artifact_path"],
        "cannot_claim_edge_due_to_multiple_testing": True,
        "claim_boundary": payload["claim_boundary"],
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "data_download_performed": False,
        "tsmom_state_changed": False,
    }


def _regime_masks(panel: pd.DataFrame) -> dict[str, pd.Series]:
    frame = panel.copy()
    btc = _frame(frame[frame["symbol"] == "BTCUSDT"].set_index("timestamp"))
    btc_trend = _series(btc.get("rolling_return_24h", pd.Series(dtype=float)))
    high_vol_cut = _finite_or_none(pd.to_numeric(frame.get("realized_volatility"), errors="coerce").median())
    volume_cut = _finite_or_none(pd.to_numeric(frame.get("volume_zscore"), errors="coerce").median())
    funding_abs_cut = _finite_or_none(pd.to_numeric(frame.get("funding_rate"), errors="coerce").abs().quantile(0.9))
    range_cut = _finite_or_none(pd.to_numeric(frame.get("rolling_range"), errors="coerce").median())
    corr_change_cut = _finite_or_none(pd.to_numeric(frame.get("corr_to_btc_change"), errors="coerce").abs().quantile(0.9))
    index = frame.index
    btc_trend_by_index = _series(frame["timestamp"]).map(btc_trend)
    return {
        "high_volatility": _series(frame["realized_volatility"] >= (high_vol_cut or math.inf)).set_axis(index),
        "low_volatility": _series(frame["realized_volatility"] < (high_vol_cut or -math.inf)).set_axis(index),
        "positive_funding": _series(frame["funding_rate"] > 0.0).set_axis(index),
        "negative_funding": _series(frame["funding_rate"] < 0.0).set_axis(index),
        "extreme_funding": _series(frame["funding_rate"].abs() >= (funding_abs_cut or math.inf)).set_axis(index),
        "high_volume": _series(frame["volume_zscore"] >= (volume_cut or math.inf)).set_axis(index),
        "low_volume": _series(frame["volume_zscore"] < (volume_cut or -math.inf)).set_axis(index),
        "btc_uptrend_proxy": _series(btc_trend_by_index > 0.0).set_axis(index),
        "btc_downtrend_proxy": _series(btc_trend_by_index < 0.0).set_axis(index),
        "range_proxy": _series(frame["rolling_range"] <= (range_cut or -math.inf)).set_axis(index),
        "correlation_breakdown_proxy": _series(frame["corr_to_btc_change"].abs() >= (corr_change_cut or math.inf)).set_axis(index),
    }


def _forward_return_labels(panel: pd.DataFrame, *, horizons: Iterable[int]) -> dict[int, pd.Series]:
    if panel.empty:
        return {int(horizon): pd.Series(dtype=float) for horizon in horizons}
    labels: dict[int, pd.Series] = {}
    for horizon in horizons:
        values = panel.groupby(level="symbol", group_keys=False)["close"].shift(-int(horizon)) / panel["close"] - 1.0
        values.index = panel.index
        labels[int(horizon)] = _coerce_float_series(values)
    return labels


def _decision_signal_lag_mismatch(raw_signal: pd.Series, decision_signal: pd.Series, *, lag_bars: int) -> bool:
    expected = lag_multi_asset_signal_for_labels(raw_signal, lag_bars=lag_bars)
    expected, observed = expected.align(decision_signal, join="outer")
    expected_valid = _series(expected).notna()
    observed_valid = _series(observed).notna()
    if bool((expected_valid != observed_valid).any()):
        return True
    comparable = expected_valid & observed_valid
    if not bool(comparable.any()):
        return False
    return not bool(
        np.allclose(
            _series(expected.loc[comparable]).to_numpy(dtype=float),
            _series(observed.loc[comparable]).to_numpy(dtype=float),
            rtol=0.0,
            atol=1e-12,
        )
    )


def _aligned_clean_frame(signal: pd.Series, labels: pd.Series) -> pd.DataFrame:
    frame = pd.concat([_coerce_float_series(signal).rename("signal"), _coerce_float_series(labels).rename("label")], axis=1)
    frame = _frame(frame.replace([math.inf, -math.inf], math.nan).dropna())
    return frame


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
    data = _aligned_clean_frame(signal, labels)
    if data.empty:
        return None
    signed = _series(data["signal"]).map(_safe_sign) * _series(data["label"]).map(_safe_sign)
    return _finite_or_none((signed > 0).mean())


def _symbol_hit_rate(signal: pd.Series, labels: pd.Series, symbol: str) -> float | None:
    if not isinstance(signal.index, pd.MultiIndex):
        return None
    try:
        symbol_signal = _coerce_float_series(signal.xs(symbol, level="symbol", drop_level=False))
    except KeyError:
        return None
    return _sign_hit_rate(symbol_signal, _coerce_float_series(labels.reindex(symbol_signal.index)))


def _turnover_proxy(signal: pd.Series) -> float | None:
    data = _coerce_float_series(signal)
    if data.empty:
        return None
    if isinstance(data.index, pd.MultiIndex) and "symbol" in data.index.names:
        return _finite_or_none(data.groupby(level="symbol", group_keys=False).diff().abs().mean())
    return _finite_or_none(data.diff().abs().mean())


def _universe_median_signal_like(signal: pd.Series) -> pd.Series:
    data = _coerce_float_series(signal)
    if not isinstance(data.index, pd.MultiIndex) or "timestamp" not in data.index.names:
        return pd.Series(0.0, index=data.index, dtype=float)
    frame = data.rename("signal").reset_index()
    frame["median"] = frame.groupby("timestamp")["signal"].transform("median")
    index_cols = [str(name) for name in data.index.names]
    return _coerce_float_series(frame.set_index(index_cols)["median"]).reindex(data.index)


def _safe_sign(value: Any) -> float:
    if pd.isna(value):
        return 0.0
    number = float(value)
    if number > 0:
        return 1.0
    if number < 0:
        return -1.0
    return 0.0


def _beats_baseline(signal_metric: float | None, baseline_metric: float | None) -> bool | None:
    if signal_metric is None or baseline_metric is None:
        return None
    return abs(signal_metric) > abs(baseline_metric)


def _same_sign(left: float | None, right: float | None) -> bool | None:
    if left is None or right is None:
        return None
    if left == 0.0 or right == 0.0:
        return False
    return (left > 0.0 and right > 0.0) or (left < 0.0 and right < 0.0)


def _sign_consistency_rate(values: Iterable[float | None]) -> float | None:
    finite = [float(value) for value in values if value is not None and math.isfinite(float(value)) and abs(float(value)) > 1e-12]
    if not finite:
        return None
    positive = sum(1 for value in finite if value > 0.0)
    negative = sum(1 for value in finite if value < 0.0)
    return _finite_or_none(max(positive, negative) / len(finite))


def _boolean_rate(values: Iterable[Any]) -> float | None:
    collected = [value for value in values if value is not None and not pd.isna(value)]
    if not collected:
        return None
    return _finite_or_none(sum(1 for value in collected if bool(value)) / len(collected))


def _degradation(validation: float | None, test: float | None) -> float | None:
    if validation is None or test is None:
        return None
    return _finite_or_none(float(test) - float(validation))


def _median(values: Iterable[Any]) -> float | None:
    finite = [float(value) for value in values if value is not None and math.isfinite(float(value))]
    return _finite_or_none(pd.Series(finite, dtype=float).median()) if finite else None


def _min_or_none(values: Iterable[Any]) -> float | None:
    finite = [float(value) for value in values if value is not None and math.isfinite(float(value))]
    return min(finite) if finite else None


def _finite_or_none(value: Any) -> float | None:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(number):
        return None
    return number


def _safe_div(numerator: int | float, denominator: int | float) -> float:
    return 0.0 if float(denominator) == 0.0 else float(numerator) / float(denominator)


def _truthy(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    return str(value).strip().lower() in {"1", "true", "yes", "y"}


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


def _read_json(path: Path) -> dict[str, Any]:
    try:
        return cast(dict[str, Any], json.loads(path.read_text()))
    except json.JSONDecodeError as exc:
        raise ValueError(f"invalid JSON file {path}: {exc}") from exc


def _read_jsonl_records(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for line in path.read_text().splitlines():
        if line.strip():
            rows.append(cast(dict[str, Any], json.loads(line)))
    return rows


def _read_csv_records(path: Path) -> list[dict[str, Any]]:
    try:
        frame = pd.read_csv(path)
    except pd.errors.EmptyDataError:
        return []
    return [dict(row) for row in frame.to_dict(orient="records")]


def _frame_with_columns(rows: list[dict[str, Any]], columns: Iterable[str]) -> pd.DataFrame:
    ordered_columns = list(columns)
    if not rows:
        return pd.DataFrame(columns=ordered_columns)
    frame = pd.DataFrame(rows)
    for column in ordered_columns:
        if column not in frame.columns:
            frame[column] = pd.NA
    extra_columns = [column for column in frame.columns if column not in ordered_columns]
    return _frame(frame.loc[:, ordered_columns + extra_columns])


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    _atomic_write_text(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def _atomic_write_text(path: Path, payload: str) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(payload)
    tmp.replace(path)


def _series(value: Any) -> pd.Series:
    return cast(pd.Series, value)


def _frame(value: Any) -> pd.DataFrame:
    return cast(pd.DataFrame, value)


def _coerce_float_series(value: pd.Series) -> pd.Series:
    return _series(pd.to_numeric(value, errors="coerce")).astype(float)
