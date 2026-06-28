# pyright: reportPrivateUsage=false, reportAttributeAccessIssue=false, reportArgumentType=false, reportCallIssue=false, reportOptionalOperand=false, reportReturnType=false
from __future__ import annotations

import csv
import io
import json
import math
import zipfile
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterable, Literal, cast

import numpy as np
import pandas as pd

from parangsae.marketdata.binance_vision import canonical_cache_path, sha256_file, validate_interval
from parangsae.research.alpha_signal_factory import (
    DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT,
    DUPLICATE_CORRELATION_THRESHOLD,
    FORWARD_HORIZONS,
    READINESS_BASELINE_VERDICT,
)
from parangsae.research.loop_artifacts import prepare_loop_output_dir
from parangsae.research.multi_asset_alpha_factory import (
    DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET,
    DEFAULT_MULTI_ASSET_DECISION_LAG_BARS,
    DEFAULT_MULTI_ASSET_GENERATION_SEED,
    DEFAULT_MULTI_ASSET_MAX_DEPTH,
    HARD_MULTI_ASSET_CANDIDATE_BUDGET,
    build_multi_asset_oos_folds,
    deterministic_random_sign_signal,
    lag_multi_asset_signal_for_labels,
    rolling_zscore,
    shuffled_timestamp_signal,
    symbol_permuted_signal,
    _beats_baseline,
    _clusters_from_corr_matrix,
    _coerce_float_series,
    _cross_sectional_rank,
    _finite_or_none,
    _frame,
    _mean_signal,
    _safe_div,
    _segment_ic,
    _series,
    _series_corr,
    _timestamp,
)
from parangsae.research.null_baselines import deterministic_random_seed, inverse_signal_position, lagged_signal_position
from parangsae.research.public_derivatives_microstructure import (
    PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_EXCLUDED_SYMBOLS,
)

HISTORICAL_KLINE_MICROSTRUCTURE_SCHEMA_VERSION = 13
HISTORICAL_KLINE_MICROSTRUCTURE_LOOP_SLUG = "historical-kline-microstructure-proxy-signal-factory-v13"
HISTORICAL_KLINE_MICROSTRUCTURE_NAMESPACE = (
    "historical_kline_microstructure_proxy_signal_factory_v13_only_not_readiness"
)
DEFAULT_HISTORICAL_KLINE_MICROSTRUCTURE_V12_ARTIFACT = Path(
    "reports/loops/20260628-public-derivatives-microstructure-recent-window-v12"
)
HISTORICAL_KLINE_MICROSTRUCTURE_DEFAULT_UNIVERSE = (
    "ADAUSDT",
    "AVAXUSDT",
    "BNBUSDT",
    "BTCUSDT",
    "DOGEUSDT",
    "ETHUSDT",
    "LINKUSDT",
    "SOLUSDT",
    "XRPUSDT",
)
HISTORICAL_KLINE_MICROSTRUCTURE_SIGNAL_FAMILIES = (
    "taker_imbalance_momentum",
    "taker_imbalance_reversal",
    "trade_count_pressure",
    "quote_volume_pressure",
    "avg_trade_size_proxy",
    "microstructure_divergence",
    "cross_sectional_microstructure_rank",
    "lead_lag_microstructure",
    "microstructure_x_funding",
    "microstructure_x_volatility_regime",
)
HISTORICAL_KLINE_MICROSTRUCTURE_ALLOWED_SUITE_DECISIONS = (
    "HISTORICAL_KLINE_MICROSTRUCTURE_FACTORY_INFRA_ONLY",
    "NO_SURVIVING_SIGNAL_FAMILY",
    "DUPLICATES_DOMINATE",
    "FAILS_NULL_ROBUSTNESS",
    "NEEDS_STRICT_OOS_FALSIFICATION",
    "WATCH_FAMILY_ONLY_PAPER_RESEARCH",
)
HISTORICAL_KLINE_MICROSTRUCTURE_ALLOWED_SIGNAL_LABELS = (
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEEDS_STRICT_OOS_FALSIFICATION",
    "NEGATIVE_CONTROL_CANDIDATE",
)
HISTORICAL_KLINE_MICROSTRUCTURE_FORBIDDEN_OUTPUT_LABELS = {
    "READY",
    "TRADE",
    "LIVE_READY",
    "PROFITABLE",
    "DEPLOY",
    "CAPITAL_ALLOCATE",
    "EDGE_CONFIRMED",
}
HISTORICAL_KLINE_MICROSTRUCTURE_REQUIRED_RAW_COLUMNS = (
    "open",
    "high",
    "low",
    "close",
    "volume",
    "quote_asset_volume",
    "number_of_trades",
    "taker_buy_base_volume",
    "taker_buy_quote_volume",
)
HISTORICAL_KLINE_MICROSTRUCTURE_DERIVED_COLUMNS = (
    "trade_count",
    "quote_volume",
    "taker_buy_base_volume",
    "taker_buy_quote_volume",
    "taker_buy_volume_ratio",
    "taker_sell_base_volume_proxy",
    "taker_sell_quote_volume_proxy",
    "taker_imbalance_proxy",
    "taker_imbalance_zscore",
    "trade_count_zscore",
    "quote_volume_zscore",
    "volume_imbalance_zscore",
    "avg_trade_size_proxy",
    "taker_buy_avg_trade_size_proxy",
    "taker_sell_avg_trade_size_proxy",
    "trade_intensity_change",
    "taker_imbalance_change",
    "microstructure_volatility_interaction",
)
HISTORICAL_KLINE_MICROSTRUCTURE_PANEL_COLUMNS = (
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
    *HISTORICAL_KLINE_MICROSTRUCTURE_DERIVED_COLUMNS,
    "cross_sectional_taker_imbalance_rank",
    "cross_sectional_trade_count_rank",
    "cross_sectional_quote_volume_rank",
    "cross_sectional_avg_trade_size_rank",
    "cross_sectional_microstructure_volatility_rank",
)
HISTORICAL_KLINE_MICROSTRUCTURE_NULL_BASELINES_USED = (
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
    "v10_negative_control_families",
)
HISTORICAL_KLINE_MICROSTRUCTURE_MULTIPLE_TESTING_BOUNDARY = (
    "V13 generates a large historical kline-derived microstructure proxy signal hypothesis "
    "universe from existing public Binance Vision raw kline cache. Every metric is diagnostic "
    "and exposed to multiple-testing, data-snooping, duplicate-family, and survivorship risk. "
    "V13 may only create a paper-only strict falsification queue; it cannot claim alpha, edge, "
    "profitability, readiness, execution readiness, or permission to trade."
)
HISTORICAL_KLINE_MICROSTRUCTURE_LEAKAGE_BOUNDARY = (
    "V13 uses only historical public kline fields available in each closed bar. Cross-sectional "
    "ranks use only symbols with actual data at the same timestamp, rolling features are "
    "historical, and every signal is shifted by at least one decision lag before forward-return "
    "label comparison. Recent-window endpoint data is not used."
)
HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY = (
    "HISTORICAL_KLINE_MICROSTRUCTURE_PROXY_SIGNAL_FACTORY_V13 is a paper-only signal factory "
    "over historical kline-derived public microstructure proxies. Generated signals are "
    "hypotheses, not edges, not a trading strategy, not portfolio construction, not signal "
    "weight optimization, not profitability evidence, not readiness or live readiness, and "
    "not permission for live/testnet/account/API-key/order/fill use."
)
HISTORICAL_KLINE_MICROSTRUCTURE_REQUIRED_FILES = (
    "historical_kline_microstructure_signal_factory.json",
    "microstructure_panel_summary.csv",
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
HISTORICAL_KLINE_MICROSTRUCTURE_KNOWN_FILES = set(HISTORICAL_KLINE_MICROSTRUCTURE_REQUIRED_FILES) | {
    "loop.json"
}
HISTORICAL_KLINE_MICROSTRUCTURE_NEXT_QUEUE_COLUMNS = (
    "signal_family",
    "queue_decision",
    "paper_only",
    "effective_signal_breadth",
    "corresponding_oos_metric",
    "reason",
    "claim_boundary",
)
MICROSTRUCTURE_RAW_COLUMN_ALIASES: dict[str, tuple[str, ...]] = {
    "quote_asset_volume": ("quote_asset_volume", "quote_volume"),
    "number_of_trades": ("number_of_trades", "count"),
    "taker_buy_base_volume": (
        "taker_buy_base_volume",
        "taker_buy_base_asset_volume",
        "taker_buy_volume",
    ),
    "taker_buy_quote_volume": (
        "taker_buy_quote_volume",
        "taker_buy_quote_asset_volume",
        "taker_buy_quote_volume",
    ),
}
SignalLabel = Literal[
    "KILL_SIGNAL",
    "DUPLICATE_SIGNAL",
    "WATCH_FAMILY_ONLY",
    "WATCH_DIAGNOSTIC_ONLY",
    "NEEDS_STRICT_OOS_FALSIFICATION",
    "NEGATIVE_CONTROL_CANDIDATE",
]


@dataclass(frozen=True)
class KlineMicrostructureSignalSpec:
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
    claim_boundary: str = HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY

    def __post_init__(self) -> None:
        if self.lag_bars < 1:
            raise ValueError("KlineMicrostructureSignalSpec.lag_bars must be at least 1")
        if self.max_depth < 1 or self.max_depth > DEFAULT_MULTI_ASSET_MAX_DEPTH:
            raise ValueError("KlineMicrostructureSignalSpec.max_depth must be between 1 and 4")
        if self.depth < 1 or self.depth > self.max_depth:
            raise ValueError("KlineMicrostructureSignalSpec.depth must be between 1 and max_depth")
        if self.uses_future_data:
            raise ValueError("KlineMicrostructureSignalSpec may not use future data")
        if self.signal_family not in HISTORICAL_KLINE_MICROSTRUCTURE_SIGNAL_FAMILIES:
            raise ValueError(f"unknown V13 signal family: {self.signal_family}")

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> KlineMicrostructureSignalSpec:
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
            claim_boundary=str(payload.get("claim_boundary", HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY)),
        )


@dataclass(frozen=True)
class HistoricalKlineMicrostructureSignalFactoryResult:
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


def default_historical_kline_microstructure_signal_factory_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{HISTORICAL_KLINE_MICROSTRUCTURE_LOOP_SLUG}"


def read_v12_historical_kline_microstructure_source_state(path: str | Path) -> dict[str, Any]:
    source_path = Path(path)
    artifact_dir = source_path if source_path.is_dir() else source_path.parent
    payload_path = artifact_dir / "public_derivatives_microstructure_recent_window.json" if source_path.is_dir() else source_path
    if not payload_path.exists():
        raise FileNotFoundError(f"missing V12 public_derivatives_microstructure_recent_window.json: {payload_path}")
    payload = json.loads(payload_path.read_text())
    if "public_derivatives_microstructure_recent_window_v12" not in str(payload.get("loop_namespace", "")):
        raise ValueError("source_v12_artifact_path must point to a V12 microstructure recent-window artifact")
    if payload.get("readiness_baseline_verdict") != READINESS_BASELINE_VERDICT:
        raise ValueError("source V12 artifact must preserve readiness_baseline_verdict=NOT_READY")
    if payload.get("coverage_sufficient_for_future_v13") is not True:
        raise ValueError("PRECONDITION_FAILED_SOURCE_V12_NOT_READY: coverage_sufficient_for_future_v13=false")
    if payload.get("historical_microstructure_proxy_readiness") is not True:
        raise ValueError("PRECONDITION_FAILED_SOURCE_V12_NOT_READY: historical_microstructure_proxy_readiness=false")
    for field in ("private_data_used", "account_data_used", "api_key_used", "live_or_testnet_path_created"):
        if bool(payload.get(field)):
            raise ValueError(f"source V12 artifact violates public/no-live boundary: {field}=true")
    if bool(payload.get("raw_data_files_committed")):
        raise ValueError("source V12 artifact reports raw_data_files_committed=true")
    coverage_path = artifact_dir / "kline_microstructure_proxy_coverage.csv"
    readiness_path = artifact_dir / "readiness_by_family.csv"
    provenance_path = artifact_dir / "provenance_manifest.csv"
    for required in (coverage_path, readiness_path, provenance_path, artifact_dir / "v11_reference.csv"):
        if not required.exists():
            raise FileNotFoundError(f"missing required V12 artifact file: {required}")
    coverage_rows = _read_csv_records(coverage_path)
    available_rows = [
        row
        for row in coverage_rows
        if row.get("data_family") == "kline_derived_microstructure_proxy"
        and _truthy(row.get("available"))
        and _truthy(row.get("included_for_future_v13"))
        and _truthy(row.get("public_source_only"))
        and not _truthy(row.get("requires_api_key"))
    ]
    symbols = tuple(sorted(str(row["symbol"]) for row in available_rows))
    if len(symbols) < 3:
        raise ValueError("PRECONDITION_FAILED_SOURCE_V12_NOT_READY: fewer than 3 historical proxy symbols")
    return {
        "artifact_dir": artifact_dir,
        "payload_path": payload_path,
        "payload_hash": sha256_file(payload_path),
        "coverage_path": coverage_path,
        "coverage_hash": sha256_file(coverage_path),
        "readiness_path": readiness_path,
        "readiness_hash": sha256_file(readiness_path),
        "provenance_path": provenance_path,
        "provenance_hash": sha256_file(provenance_path),
        "payload": payload,
        "coverage_rows": coverage_rows,
        "usable_symbols": symbols,
        "excluded_symbols": list(payload.get("excluded_symbols", PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_EXCLUDED_SYMBOLS)),
        "common_start": _min_timestamp(row.get("first_timestamp") for row in available_rows),
        "common_end": _max_timestamp(row.get("last_timestamp") for row in available_rows),
    }


def build_historical_kline_microstructure_panel(
    *,
    source_v12_state: dict[str, Any],
    binance_vision_cache_dir: str | Path = "data/binance-vision",
    interval: str = "1h",
    target_start: str = "2024-01-01",
    target_end: str = "2025-12-31",
    requested_symbols: Iterable[str] | None = None,
) -> tuple[pd.DataFrame, list[dict[str, Any]], list[dict[str, Any]]]:
    interval = validate_interval(interval)
    symbols = tuple(sorted(_normalize_symbols(requested_symbols or source_v12_state["usable_symbols"])))
    approved_symbols = set(_normalize_symbols(source_v12_state["usable_symbols"]))
    unapproved_symbols = sorted(set(symbols) - approved_symbols)
    if unapproved_symbols:
        raise ValueError(
            "V13 requested symbols must be approved by V12 historical kline proxy readiness: "
            + ",".join(unapproved_symbols)
        )
    target_start_ts = _utc_timestamp(target_start)
    target_end_ts = _utc_timestamp(target_end)
    months = _month_strings_between(target_start_ts, target_end_ts)
    frames: list[pd.DataFrame] = []
    summary: list[dict[str, Any]] = []
    excluded: list[dict[str, Any]] = []
    for symbol in symbols:
        paths = [canonical_cache_path(Path(binance_vision_cache_dir), symbol, interval, month) for month in months]
        missing_paths = [path for path in paths if not path.exists()]
        if missing_paths:
            reason = f"missing existing public Binance Vision raw kline cache files; missing_count={len(missing_paths)}"
            excluded.append({"symbol": symbol, "reason": reason})
            continue
        parsed_frames: list[pd.DataFrame] = []
        symbol_hashes: list[str] = []
        unavailable_reason = ""
        for path in paths:
            parsed = _read_raw_kline_zip_frame(path)
            if not parsed["parse_ok"]:
                unavailable_reason = f"raw kline cache parse failed: {parsed['error']}"
                break
            frame = cast(pd.DataFrame, parsed["frame"])
            resolved = _resolve_required_raw_columns(frame)
            if resolved["missing"]:
                unavailable_reason = "missing required raw kline microstructure columns: " + ",".join(
                    cast(list[str], resolved["missing"])
                )
                break
            parsed_frames.append(_normalize_raw_kline_frame(frame, cast(dict[str, str], resolved["resolved"])))
            symbol_hashes.append(sha256_file(path))
        if unavailable_reason:
            excluded.append({"symbol": symbol, "reason": unavailable_reason})
            continue
        if not parsed_frames:
            excluded.append({"symbol": symbol, "reason": "no parseable raw kline frames"})
            continue
        raw = pd.concat(parsed_frames, ignore_index=True).drop_duplicates(subset=["timestamp"]).sort_values("timestamp")
        raw = raw[(raw["timestamp"] >= target_start_ts) & (raw["timestamp"] <= target_end_ts)]
        if raw.empty:
            excluded.append({"symbol": symbol, "reason": "existing raw kline proxy cache has no rows in target range"})
            continue
        raw["symbol"] = symbol
        frames.append(raw)
        summary.append(
            {
                "symbol": symbol,
                "row_count": int(len(raw)),
                "first_timestamp": _timestamp(cast(pd.Timestamp, raw["timestamp"].iloc[0])),
                "last_timestamp": _timestamp(cast(pd.Timestamp, raw["timestamp"].iloc[-1])),
                "source": "existing_public_binance_vision_raw_kline_cache",
                "source_cache_first_path": str(paths[0]),
                "source_cache_file_count": len(paths),
                "raw_kline_microstructure_columns_available": True,
                "required_raw_columns": ",".join(HISTORICAL_KLINE_MICROSTRUCTURE_REQUIRED_RAW_COLUMNS),
                "derived_proxy_columns": ",".join(HISTORICAL_KLINE_MICROSTRUCTURE_DERIVED_COLUMNS),
                "recent_window_data_used": False,
                "data_download_performed": False,
                "normalized_sha256": _combined_hash(symbol_hashes),
            }
        )
    if not frames:
        empty = pd.DataFrame(columns=HISTORICAL_KLINE_MICROSTRUCTURE_PANEL_COLUMNS)
        return empty.set_index(["timestamp", "symbol"], drop=False), summary, excluded
    panel = pd.concat(frames, ignore_index=True).sort_values(["timestamp", "symbol"]).reset_index(drop=True)
    panel = _add_historical_kline_microstructure_features(panel)
    panel = _frame(panel.set_index(["timestamp", "symbol"], drop=False).sort_index())
    return panel, summary, excluded


def generate_historical_kline_microstructure_signal_specs(
    symbols: Iterable[str],
    *,
    seed: int = DEFAULT_MULTI_ASSET_GENERATION_SEED,
    candidate_budget: int = DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET,
    max_depth: int = DEFAULT_MULTI_ASSET_MAX_DEPTH,
) -> list[KlineMicrostructureSignalSpec]:
    if candidate_budget <= 0 or candidate_budget > HARD_MULTI_ASSET_CANDIDATE_BUDGET:
        raise ValueError("candidate_budget must be between 1 and 20000")
    if max_depth < 1 or max_depth > DEFAULT_MULTI_ASSET_MAX_DEPTH:
        raise ValueError("max_depth must be between 1 and 4")
    universe = tuple(sorted(_normalize_symbols(symbols)))
    specs: list[KlineMicrostructureSignalSpec] = []

    def add(
        *,
        family: str,
        feature: str,
        symbol_scope: tuple[str, ...],
        pair_scope: tuple[str, str] | None = None,
        depth: int,
        complexity: float,
        uses_funding: bool = False,
    ) -> None:
        if len(specs) >= candidate_budget or depth > max_depth:
            return
        scope_part = "-".join(symbol_scope) if symbol_scope else "universe"
        pair_part = "" if pair_scope is None else f".pair_{pair_scope[0]}_{pair_scope[1]}"
        signal_id = f"v13.{family}.{feature}.{scope_part}{pair_part}.d{depth}"
        specs.append(
            KlineMicrostructureSignalSpec(
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

    windows = (3, 6, 12, 24, 72, 168)
    family_bases: dict[str, tuple[str, ...]] = {
        "taker_imbalance_momentum": (
            "taker_imbalance_proxy",
            "taker_imbalance_zscore",
            "taker_imbalance_change",
            "taker_buy_volume_ratio",
        ),
        "taker_imbalance_reversal": (
            "taker_imbalance_zscore",
            "taker_imbalance_change",
            "volume_imbalance_zscore",
        ),
        "trade_count_pressure": (
            "trade_count_zscore",
            "trade_intensity_change",
            "trade_count",
        ),
        "quote_volume_pressure": (
            "quote_volume_zscore",
            "volume_imbalance_zscore",
            "quote_volume",
        ),
        "avg_trade_size_proxy": (
            "avg_trade_size_proxy",
            "taker_buy_avg_trade_size_proxy",
            "taker_sell_avg_trade_size_proxy",
        ),
        "microstructure_divergence": (
            "taker_imbalance_zscore",
            "trade_count_zscore",
            "quote_volume_zscore",
            "microstructure_volatility_interaction",
        ),
        "cross_sectional_microstructure_rank": (
            "taker_imbalance_zscore",
            "trade_count_zscore",
            "quote_volume_zscore",
            "avg_trade_size_proxy",
        ),
        "lead_lag_microstructure": (
            "taker_imbalance_zscore",
            "trade_count_zscore",
            "quote_volume_zscore",
        ),
        "microstructure_x_funding": (
            "taker_imbalance_zscore",
            "trade_count_zscore",
            "quote_volume_zscore",
        ),
        "microstructure_x_volatility_regime": (
            "taker_imbalance_zscore",
            "trade_count_zscore",
            "quote_volume_zscore",
            "microstructure_volatility_interaction",
        ),
    }
    ops_by_family: dict[str, tuple[tuple[str, int, float], ...]] = {
        "taker_imbalance_momentum": (
            ("raw", 1, 1.0),
            ("rolling_mean", 2, 1.3),
            ("rolling_diff", 2, 1.4),
            ("cs_rank", 2, 1.2),
        ),
        "taker_imbalance_reversal": (
            ("neg", 1, 1.0),
            ("neg_rolling_mean", 2, 1.3),
            ("median_spread_neg", 2, 1.5),
            ("rolling_zscore_neg", 3, 1.8),
        ),
        "trade_count_pressure": (
            ("raw", 1, 1.0),
            ("rolling_mean", 2, 1.3),
            ("rolling_diff", 2, 1.4),
            ("return_direction_interaction", 3, 1.8),
        ),
        "quote_volume_pressure": (
            ("raw", 1, 1.0),
            ("rolling_mean", 2, 1.3),
            ("rolling_diff", 2, 1.4),
            ("return_direction_interaction", 3, 1.8),
        ),
        "avg_trade_size_proxy": (
            ("rolling_zscore", 2, 1.4),
            ("rolling_diff", 2, 1.4),
            ("cs_rank", 2, 1.2),
            ("median_spread", 2, 1.5),
        ),
        "microstructure_divergence": (
            ("return_divergence", 3, 2.0),
            ("volatility_divergence", 3, 2.1),
            ("volume_divergence", 3, 2.0),
            ("median_spread", 2, 1.5),
        ),
        "cross_sectional_microstructure_rank": (
            ("cs_rank", 1, 1.0),
            ("cs_rank_neg", 1, 1.0),
            ("median_spread", 2, 1.4),
            ("median_spread_neg", 2, 1.4),
        ),
        "lead_lag_microstructure": (
            ("btc_lag_interaction", 3, 2.2),
            ("eth_lag_interaction", 3, 2.2),
            ("leader_lag_spread", 3, 2.3),
            ("leader_lag_rank_interaction", 4, 2.6),
        ),
        "microstructure_x_funding": (
            ("funding_unavailable_boundary", 3, 2.0),
            ("funding_rank_unavailable_boundary", 3, 2.1),
        ),
        "microstructure_x_volatility_regime": (
            ("high_volatility_gate", 3, 2.0),
            ("low_volatility_gate", 3, 2.0),
            ("volatility_rank_interaction", 3, 2.1),
            ("volatility_change_interaction", 4, 2.5),
        ),
    }
    for family in HISTORICAL_KLINE_MICROSTRUCTURE_SIGNAL_FAMILIES:
        for symbol in universe:
            for base in family_bases[family]:
                for window in windows:
                    for op, depth, complexity in ops_by_family[family]:
                        add(
                            family=family,
                            feature=f"{op}:{base}:w{window}",
                            symbol_scope=(symbol,),
                            depth=depth,
                            complexity=complexity,
                            uses_funding=family == "microstructure_x_funding",
                        )
    pair_families = {
        "microstructure_divergence": ("pair_spread", "pair_zscore_spread"),
        "lead_lag_microstructure": ("pair_lead_lag_spread", "pair_leader_interaction"),
    }
    pair_bases = ("taker_imbalance_zscore", "trade_count_zscore", "quote_volume_zscore")
    for family, ops in pair_families.items():
        for left in universe:
            for right in universe:
                if left == right:
                    continue
                for base in pair_bases:
                    for window in windows:
                        for op in ops:
                            depth = 3 if op != "pair_leader_interaction" else 4
                            add(
                                family=family,
                                feature=f"{op}:{base}:w{window}",
                                symbol_scope=(left, right),
                                pair_scope=(left, right),
                                depth=depth,
                                complexity=2.2,
                            )
    return specs[:candidate_budget]


def evaluate_historical_kline_microstructure_signal_factory(
    *,
    source_v12_artifact_path: str | Path = DEFAULT_HISTORICAL_KLINE_MICROSTRUCTURE_V12_ARTIFACT,
    binance_vision_cache_dir: str | Path = "data/binance-vision",
    requested_symbols: Iterable[str] | None = HISTORICAL_KLINE_MICROSTRUCTURE_DEFAULT_UNIVERSE,
    interval: str = "1h",
    target_start: str = "2024-01-01",
    target_end: str = "2025-12-31",
    seed: int = DEFAULT_MULTI_ASSET_GENERATION_SEED,
    candidate_budget: int = DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET,
    max_depth: int = DEFAULT_MULTI_ASSET_MAX_DEPTH,
    horizons: tuple[int, ...] = FORWARD_HORIZONS,
    fold_count: int = 3,
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> HistoricalKlineMicrostructureSignalFactoryResult:
    source_v12_state = read_v12_historical_kline_microstructure_source_state(source_v12_artifact_path)
    v12_payload = cast(dict[str, Any], source_v12_state["payload"])
    requested = tuple(_normalize_symbols(requested_symbols or source_v12_state["usable_symbols"]))
    panel, panel_summary, panel_excluded = build_historical_kline_microstructure_panel(
        source_v12_state=source_v12_state,
        binance_vision_cache_dir=binance_vision_cache_dir,
        interval=interval,
        target_start=target_start,
        target_end=target_end,
        requested_symbols=requested,
    )
    used_symbols = tuple(sorted(str(symbol) for symbol in pd.unique(panel["symbol"]))) if not panel.empty else ()
    specs = generate_historical_kline_microstructure_signal_specs(
        used_symbols,
        seed=seed,
        candidate_budget=candidate_budget,
        max_depth=max_depth,
    )
    label_returns = _forward_return_labels(panel, horizons=horizons)
    label_ranks = {horizon: labels.rank() for horizon, labels in label_returns.items()}
    folds = build_multi_asset_oos_folds(panel, horizons=horizons, fold_count=fold_count)
    raw_signals: dict[str, pd.Series] = {}
    decision_signals: dict[str, pd.Series] = {}
    diagnostics: list[dict[str, Any]] = []
    for spec in specs:
        raw_signal = build_historical_kline_microstructure_signal_series(spec, panel)
        decision_signal = lag_multi_asset_signal_for_labels(raw_signal, lag_bars=spec.lag_bars)
        raw_signals[spec.signal_id] = raw_signal
        decision_signals[spec.signal_id] = decision_signal
        diagnostics.append(
            _historical_kline_microstructure_signal_diagnostics(
                spec,
                raw_signal=raw_signal,
                decision_signal=decision_signal,
                forward_returns=label_returns,
                forward_return_ranks=label_ranks,
                folds=folds,
            )
        )
    clusters = duplicate_correlation_clusters_for_historical_kline_microstructure(decision_signals, specs)
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
    null_rows = _historical_kline_microstructure_null_baseline_rows(
        signals=decision_signals,
        specs=specs,
        clusters=clusters,
        forward_returns=label_returns,
        seed=seed,
    )
    family_summary = _family_summary_rows(
        diagnostics=diagnostics,
        oos_rows=oos_rows,
        null_rows=null_rows,
        clusters=clusters,
        used_symbols=used_symbols,
    )
    multiple_testing = _multiple_testing_rows(
        diagnostics=diagnostics,
        family_summary=family_summary,
        clusters=clusters,
        generated_signal_count=len(specs),
        horizons=horizons,
    )
    shortlist = _shortlist_rows(family_summary)
    failed_gates = _failed_gate_rows(
        diagnostics=diagnostics,
        family_summary=family_summary,
        clusters=clusters,
        excluded_symbols=_excluded_symbols(v12_payload, panel_excluded),
        used_symbols=used_symbols,
    )
    next_queue = _next_strict_falsification_queue(shortlist, family_summary)
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    killed_count = sum(1 for row in shortlist if row["signal_label"] == "KILL_SIGNAL")
    watch_count = sum(
        1
        for row in shortlist
        if row["signal_label"] in {"WATCH_FAMILY_ONLY", "WATCH_DIAGNOSTIC_ONLY", "NEEDS_STRICT_OOS_FALSIFICATION"}
    )
    effective_breadth = len({row["duplicate_cluster_id"] for row in clusters if not bool(row.get("is_redundant_duplicate"))})
    duplicate_ratio = _safe_div(duplicate_count, len(clusters))
    suite_decision = _suite_decision(shortlist, duplicate_ratio)
    if suite_decision not in HISTORICAL_KLINE_MICROSTRUCTURE_ALLOWED_SUITE_DECISIONS:
        raise ValueError(f"forbidden V13 suite decision emitted: {suite_decision}")
    common_start = _timestamp(cast(pd.Timestamp, pd.to_datetime(panel["timestamp"]).min())) if not panel.empty else None
    common_end = _timestamp(cast(pd.Timestamp, pd.to_datetime(panel["timestamp"]).max())) if not panel.empty else None
    common_bar_count = int(len(pd.unique(panel["timestamp"]))) if not panel.empty else 0
    payload = {
        "schema_version": HISTORICAL_KLINE_MICROSTRUCTURE_SCHEMA_VERSION,
        "loop_id": f"{datetime.now(UTC).strftime('%Y%m%d')}-{HISTORICAL_KLINE_MICROSTRUCTURE_LOOP_SLUG}",
        "loop_namespace": HISTORICAL_KLINE_MICROSTRUCTURE_NAMESPACE,
        "loop_type": "validation_infrastructure",
        "status": "IMPLEMENTED",
        "suite_decision": suite_decision,
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "source_v12_artifact_path": str(source_v12_state["artifact_dir"]),
        "source_v12_payload_hash": source_v12_state["payload_hash"],
        "source_v12_coverage_sufficient_for_future_v13": bool(v12_payload.get("coverage_sufficient_for_future_v13")),
        "source_v12_historical_microstructure_proxy_readiness": bool(
            v12_payload.get("historical_microstructure_proxy_readiness")
        ),
        "source_v12_recent_window_microstructure_readiness": bool(
            v12_payload.get("recent_window_microstructure_readiness")
        ),
        "recent_window_data_used": False,
        "temporal_mismatch_warning": (
            "V13 uses only long-span historical kline-derived proxy data. Recent-window endpoint data was not used "
            "and must not be mixed with 2024-2025 labels without a separate recent-window-only protocol."
        ),
        "symbol_universe_requested": list(requested),
        "symbol_universe_used": list(used_symbols),
        "excluded_symbols": _excluded_symbols(v12_payload, panel_excluded),
        "common_start": common_start,
        "common_end": common_end,
        "common_bar_count": common_bar_count,
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
        "families_evaluated": list(HISTORICAL_KLINE_MICROSTRUCTURE_SIGNAL_FAMILIES),
        "families_generated": sorted({spec.signal_family for spec in specs}),
        "horizons_evaluated": list(horizons),
        "null_baselines_used": list(HISTORICAL_KLINE_MICROSTRUCTURE_NULL_BASELINES_USED),
        "multiple_testing_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_MULTIPLE_TESTING_BOUNDARY,
        "cannot_claim_edge_due_to_multiple_testing": True,
        "leakage_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_LEAKAGE_BOUNDARY,
        "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
        "safety_gates": _safety_gates(),
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "holdout_retune_used": False,
        "strategy_optimization_performed": False,
        "live_or_testnet_path_created": False,
        "data_download_performed": False,
        "tsmom_state_changed": False,
        "candidate_generation": {
            "deterministic_only": True,
            "used_forward_labels": False,
            "performance_based_generation": False,
            "max_dag_depth": int(max_depth),
            "generated_fewer_than_budget_reason": (
                None
                if len(specs) >= candidate_budget
                else "finite fixed V13 historical kline microstructure grammar generated fewer candidates than requested budget"
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
            "v10_negative_control": "reports/loops/20260628-v9-falsification-lineage-negative-control-governance-v10",
            "v12": str(source_v12_state["artifact_dir"]),
            "tsmom_negative_control": str(DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT),
        },
        "summary": {
            "allowed_signal_labels": list(HISTORICAL_KLINE_MICROSTRUCTURE_ALLOWED_SIGNAL_LABELS),
            "allowed_suite_decisions": list(HISTORICAL_KLINE_MICROSTRUCTURE_ALLOWED_SUITE_DECISIONS),
            "cannot_claim_edge_due_to_multiple_testing": True,
            "paper_only_next_queue": True,
        },
        "validation_commands": list(commands),
        "changed_files": _non_raw_changed_files(changed_files),
        "evidence_artifacts": list(HISTORICAL_KLINE_MICROSTRUCTURE_REQUIRED_FILES),
        "next_loop": (
            "If next_strict_falsification_queue.csv is non-empty, use it only as a frozen input to V14 strict "
            "OOS/FDR falsification. If empty, stop this data axis without retuning."
        ),
    }
    _assert_payload_safe(payload)
    return HistoricalKlineMicrostructureSignalFactoryResult(
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


def build_historical_kline_microstructure_signal_series(
    spec: KlineMicrostructureSignalSpec,
    panel: pd.DataFrame,
) -> pd.Series:
    if spec.uses_future_data:
        raise ValueError(f"V13 signal {spec.signal_id} uses future data")
    if panel.empty:
        return pd.Series(dtype=float)
    if spec.uses_funding:
        return _empty_signal_for_scope(spec, panel)
    if spec.pair_scope is not None:
        return _pair_signal_series(spec, panel)
    if not spec.symbol_scope:
        return pd.Series(dtype=float)
    symbol = spec.symbol_scope[0]
    frame = _symbol_panel(panel, symbol)
    if frame.empty:
        return pd.Series(dtype=float)
    op, base, window = _parse_feature(spec.feature_name)
    values = _apply_unary_operation(frame, panel, symbol=symbol, op=op, base=base, window=window)
    values.index = _multi_index_for_symbol(frame, symbol)
    return _series(pd.to_numeric(values.replace([math.inf, -math.inf], math.nan), errors="coerce"))


def duplicate_correlation_clusters_for_historical_kline_microstructure(
    signals: dict[str, pd.Series],
    specs: list[KlineMicrostructureSignalSpec],
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
        cluster_id = f"KC{idx:04d}"
        representative = _cluster_representative(cluster, spec_by_id)
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


def prepare_historical_kline_microstructure_signal_factory_output_dir(
    output_dir: str | Path,
    *,
    overwrite: bool = False,
) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="historical-kline-microstructure-proxy-signal-factory-run",
        known_files=HISTORICAL_KLINE_MICROSTRUCTURE_KNOWN_FILES,
        overwrite=overwrite,
    )


def write_historical_kline_microstructure_signal_factory_artifacts(
    output_dir: str | Path,
    result: HistoricalKlineMicrostructureSignalFactoryResult,
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_historical_kline_microstructure_signal_factory_output_dir(output_dir, overwrite=overwrite)
    payload = dict(result.payload)
    if out.name.endswith(HISTORICAL_KLINE_MICROSTRUCTURE_LOOP_SLUG):
        payload["loop_id"] = out.name
    payload["failed_gates"] = _compact_failed_gate_rows(result.failed_gates)
    _assert_payload_safe(payload)
    _write_json(out / "loop.json", _generic_loop_payload(payload))
    _write_json(out / "historical_kline_microstructure_signal_factory.json", payload)
    pd.DataFrame(result.panel_summary).to_csv(out / "microstructure_panel_summary.csv", index=False)
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
    _atomic_write_text(out / "report.md", render_historical_kline_microstructure_signal_factory_report(payload, result))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    return out


def render_historical_kline_microstructure_signal_factory_report(
    payload: dict[str, Any],
    result: HistoricalKlineMicrostructureSignalFactoryResult,
) -> str:
    family_lines = [
        f"- {row['signal_family']}: {row['family_prefilter_status']} "
        f"(median_oos={row.get('corresponding_oos_metric')}, null={row.get('null_comparison_status')})"
        for row in result.family_summary
    ]
    shortlist_lines = [
        f"- {row['signal_family']}: {row['signal_label']} ({row['rationale']})"
        for row in result.shortlist
    ]
    return "\n".join(
        [
            "# HISTORICAL_KLINE_MICROSTRUCTURE_PROXY_SIGNAL_FACTORY_V13",
            "",
            "## Verdict",
            f"- suite_decision: {payload['suite_decision']}",
            f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
            f"- claim_boundary: {payload['claim_boundary']}",
            "",
            "## Source V12",
            f"- source_v12_artifact_path: {payload['source_v12_artifact_path']}",
            f"- source_v12_coverage_sufficient_for_future_v13: {payload['source_v12_coverage_sufficient_for_future_v13']}",
            f"- source_v12_historical_microstructure_proxy_readiness: {payload['source_v12_historical_microstructure_proxy_readiness']}",
            f"- source_v12_recent_window_microstructure_readiness: {payload['source_v12_recent_window_microstructure_readiness']}",
            f"- recent_window_data_used: {payload['recent_window_data_used']}",
            "",
            "## Universe",
            f"- symbol_universe_used: {', '.join(payload['symbol_universe_used'])}",
            f"- common_start: {payload['common_start']}",
            f"- common_end: {payload['common_end']}",
            f"- common_bar_count: {payload['common_bar_count']}",
            "",
            "## Signal Factory",
            f"- generated_signal_count: {payload['generated_signal_count']}",
            f"- evaluated_signal_count: {payload['evaluated_signal_count']}",
            f"- effective_signal_breadth: {payload['effective_signal_breadth']}",
            f"- duplicate_ratio: {payload['duplicate_ratio']}",
            f"- candidate_budget: {payload['candidate_budget']}",
            f"- max_depth: {payload['max_depth']}",
            "",
            "## Family Summary",
            *family_lines,
            "",
            "## Shortlist",
            *(shortlist_lines or ["- none"]),
            "",
            "## Multiple Testing",
            "",
            payload["multiple_testing_boundary"],
            "",
            "## Safety",
            "",
            "- No live orders submitted.",
            "- No testnet orders submitted.",
            "- No account, API-key, private order, or fill data used.",
            "- No data download or backfill performed.",
            "- TSMOM state was not changed.",
            "",
            "## What Cannot Be Claimed",
            "",
            "- No alpha, edge, profitability, readiness, execution readiness, live/testnet permission, strategy, portfolio, or signal weight optimization claim.",
        ]
    )


def _add_historical_kline_microstructure_features(panel: pd.DataFrame) -> pd.DataFrame:
    data = panel.copy()
    data["return"] = data.groupby("symbol")["close"].pct_change()
    data["log_return"] = np.log(data["close"]).groupby(data["symbol"]).diff()
    data["realized_volatility"] = data.groupby("symbol")["return"].transform(
        lambda s: _series(s).rolling(24, min_periods=6).std()
    )
    data["rolling_range"] = (data["high"] - data["low"]) / data["close"].replace(0, math.nan)
    data["volume_zscore"] = data.groupby("symbol")["volume"].transform(
        lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12)
    )
    data["trade_count"] = data["number_of_trades"].astype(float)
    data["quote_volume"] = data["quote_asset_volume"].astype(float)
    data["taker_buy_base_volume"] = data["taker_buy_base_volume"].astype(float)
    data["taker_buy_quote_volume"] = data["taker_buy_quote_volume"].astype(float)
    data["taker_buy_volume_ratio"] = _safe_ratio(data["taker_buy_base_volume"], data["volume"])
    data["taker_sell_base_volume_proxy"] = (data["volume"] - data["taker_buy_base_volume"]).clip(lower=0.0)
    data["taker_sell_quote_volume_proxy"] = (data["quote_volume"] - data["taker_buy_quote_volume"]).clip(lower=0.0)
    data["taker_imbalance_proxy"] = _safe_ratio(
        data["taker_buy_base_volume"] - data["taker_sell_base_volume_proxy"],
        data["volume"],
    )
    data["taker_imbalance_zscore"] = data.groupby("symbol")["taker_imbalance_proxy"].transform(
        lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12)
    )
    data["trade_count_zscore"] = data.groupby("symbol")["trade_count"].transform(
        lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12)
    )
    data["quote_volume_zscore"] = data.groupby("symbol")["quote_volume"].transform(
        lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12)
    )
    data["volume_imbalance_zscore"] = data.groupby("symbol")["taker_buy_volume_ratio"].transform(
        lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12)
    )
    data["avg_trade_size_proxy"] = _safe_ratio(data["volume"], data["trade_count"])
    data["taker_buy_avg_trade_size_proxy"] = _safe_ratio(data["taker_buy_base_volume"], data["trade_count"])
    data["taker_sell_avg_trade_size_proxy"] = _safe_ratio(data["taker_sell_base_volume_proxy"], data["trade_count"])
    data["trade_intensity_change"] = data.groupby("symbol")["trade_count_zscore"].diff()
    data["taker_imbalance_change"] = data.groupby("symbol")["taker_imbalance_proxy"].diff()
    vol_z = data.groupby("symbol")["realized_volatility"].transform(
        lambda s: rolling_zscore(_series(s).astype(float), window=72, min_periods=12)
    )
    data["microstructure_volatility_interaction"] = data["taker_imbalance_zscore"] * vol_z
    for source, target in (
        ("taker_imbalance_zscore", "cross_sectional_taker_imbalance_rank"),
        ("trade_count_zscore", "cross_sectional_trade_count_rank"),
        ("quote_volume_zscore", "cross_sectional_quote_volume_rank"),
        ("avg_trade_size_proxy", "cross_sectional_avg_trade_size_rank"),
        ("microstructure_volatility_interaction", "cross_sectional_microstructure_volatility_rank"),
    ):
        data[target] = _cross_sectional_rank(data, source)
        data[f"universe_{source}_median"] = data.groupby("timestamp")[source].transform("median")
    data["universe_return_median"] = data.groupby("timestamp")["return"].transform("median")
    data["universe_volume_zscore_median"] = data.groupby("timestamp")["volume_zscore"].transform("median")
    data["universe_realized_volatility_median"] = data.groupby("timestamp")["realized_volatility"].transform("median")
    data["realized_volatility_change"] = data.groupby("symbol")["realized_volatility"].diff()
    data = _add_microstructure_leader_features(data)
    return _frame(data)


def _add_microstructure_leader_features(data: pd.DataFrame) -> pd.DataFrame:
    out = data.copy()
    for leader in ("BTCUSDT", "ETHUSDT"):
        leader_frame = out[out["symbol"] == leader].set_index("timestamp")
        for column in ("taker_imbalance_zscore", "trade_count_zscore", "quote_volume_zscore"):
            series = _series(leader_frame[column]) if not leader_frame.empty else pd.Series(dtype=float)
            out[f"{leader[:3].lower()}_{column}_lag1"] = _series(out["timestamp"]).map(_series(series.shift(1)))
    return _frame(out)


def _historical_kline_microstructure_signal_diagnostics(
    spec: KlineMicrostructureSignalSpec,
    *,
    raw_signal: pd.Series,
    decision_signal: pd.Series,
    forward_returns: dict[int, pd.Series],
    forward_return_ranks: dict[int, pd.Series],
    folds: list[dict[str, Any]],
) -> dict[str, Any]:
    signal = _coerce_float_series(decision_signal)
    non_null = signal.dropna()
    signal_rank = signal.rank()
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
        "turnover_proxy": _turnover_proxy(signal),
        "duplicate_cluster_id": None,
        "max_abs_correlation_to_existing_signal": None,
        "effective_independent_signal": True,
        "chronological_oos_status": "evaluated" if folds else "insufficient_oos_folds",
        "null_comparison_status": "evaluated_by_family_rows",
        "leakage_check_status": "passed_shifted_by_decision_lag",
        "failure_reason": None,
        "uses_funding": spec.uses_funding,
        "funding_availability_status": "unavailable_by_v13_raw_kline_only_boundary" if spec.uses_funding else "not_required",
        "lag_bars": spec.lag_bars,
        "uses_future_data": spec.uses_future_data,
        "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
    }
    for horizon, labels in forward_returns.items():
        ic = _series_corr(signal, labels)
        rank_ic = _series_corr(signal_rank, forward_return_ranks[horizon])
        hit_rate = _fast_sign_hit_rate(signal, labels)
        row[f"forward_return_ic_{horizon}h"] = ic
        row[f"rank_ic_{horizon}h"] = rank_ic
        row[f"sign_hit_rate_{horizon}h"] = hit_rate
    if _decision_signal_lag_mismatch(raw_signal, decision_signal, lag_bars=spec.lag_bars):
        row["leakage_check_status"] = "failed_decision_lag_mismatch"
        row["failure_reason"] = "decision_signal_not_shifted_by_expected_lag"
    if spec.uses_funding:
        row["failure_reason"] = "funding_unavailable_by_raw_kline_only_v13_boundary"
    if len(non_null) < 3 and row["failure_reason"] is None:
        row["failure_reason"] = "insufficient_non_null_observations"
    return row


def _oos_prefilter_rows(
    *,
    signals: dict[str, pd.Series],
    specs: list[KlineMicrostructureSignalSpec],
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
                "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
            }
            for family in HISTORICAL_KLINE_MICROSTRUCTURE_SIGNAL_FAMILIES
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
                train_ic = _fast_segment_ic(family_signal, labels, fold, "train")
                validation_ic = _fast_segment_ic(family_signal, labels, fold, "validation")
                test_ic = _fast_segment_ic(family_signal, labels, fold, "test")
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
                        "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
                    }
                )
    return rows


def _historical_kline_microstructure_null_baseline_rows(
    *,
    signals: dict[str, pd.Series],
    specs: list[KlineMicrostructureSignalSpec],
    clusters: list[dict[str, Any]],
    forward_returns: dict[int, pd.Series],
    seed: int,
) -> list[dict[str, Any]]:
    family_representatives = _family_representative_signal_ids(specs, clusters)
    family_signals = {
        family: signals[signal_id]
        for family, signal_id in family_representatives.items()
    }
    rows: list[dict[str, Any]] = []
    families = sorted(family_signals)
    v10_negative_controls = _v10_negative_control_families()
    for family, family_signal in family_signals.items():
        for horizon, labels in forward_returns.items():
            signal_ic = _series_corr(family_signal, labels)
            family_index = families.index(family)
            family_permuted = family_signals[families[(family_index + 1) % len(families)]] if len(families) > 1 else None
            random_signal, random_seed = deterministic_random_sign_signal(family_signal, f"v13-{family}-{horizon}", seed_base=seed)
            shuffled, shuffled_seed = shuffled_timestamp_signal(family_signal, f"v13-{family}-{horizon}", seed_base=seed)
            baselines: list[tuple[str, pd.Series | None, str, int | None, bool]] = [
                ("zero_cash_null", pd.Series(0.0, index=family_signal.index, dtype=float), "available", None, False),
                ("buy_and_hold_same_window_baseline", None, "unavailable_constant_position_no_signal_ic", None, False),
                ("deterministic_random_sign", random_signal, "available", random_seed, False),
                ("lagged_signal", lagged_signal_position(family_signal, lag_bars=max(2, horizon // 8)), "available", None, False),
                ("inverse_signal", inverse_signal_position(family_signal), "available", None, False),
                ("shuffled_timestamp_signal", shuffled, "available", shuffled_seed, False),
                ("family_permuted_signal", family_permuted, "available" if family_permuted is not None else "unavailable_single_family", None, False),
                (
                    "symbol_permuted_signal",
                    _fast_symbol_permuted_signal(family_signal, f"v13-{family}-{horizon}", seed_base=seed),
                    "available",
                    deterministic_random_seed(f"symbol-permute-v13-{family}-{horizon}", seed_base=seed),
                    False,
                ),
                ("universe_median_signal", _universe_median_signal_like(family_signal), "available", None, False),
                (
                    "frozen_tsmom_negative_control",
                    None,
                    "available_summary_only" if DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT.exists() else "unavailable_missing_artifact",
                    None,
                    False,
                ),
                (
                    "v10_negative_control_families",
                    None,
                    "available_summary_only:" + ";".join(v10_negative_controls) if v10_negative_controls else "unavailable_missing_v10_negative_controls",
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
                        "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
                    }
                )
    return rows


def _family_summary_rows(
    *,
    diagnostics: list[dict[str, Any]],
    oos_rows: list[dict[str, Any]],
    null_rows: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
    used_symbols: tuple[str, ...],
) -> list[dict[str, Any]]:
    diagnostic_frame = pd.DataFrame(diagnostics)
    oos_frame = pd.DataFrame(oos_rows)
    null_frame = pd.DataFrame(null_rows)
    cluster_frame = pd.DataFrame(clusters)
    rows: list[dict[str, Any]] = []
    for family in HISTORICAL_KLINE_MICROSTRUCTURE_SIGNAL_FAMILIES:
        gf = diagnostic_frame[diagnostic_frame["signal_family"] == family] if not diagnostic_frame.empty else pd.DataFrame()
        of = oos_frame[oos_frame["signal_family"] == family] if not oos_frame.empty else pd.DataFrame()
        nf = null_frame[null_frame["signal_family"] == family] if not null_frame.empty else pd.DataFrame()
        cf = cluster_frame[cluster_frame["signal_family"] == family] if not cluster_frame.empty else pd.DataFrame()
        effective = int((gf["effective_independent_signal"] == True).sum()) if not gf.empty else 0  # noqa: E712
        duplicate_count = int((gf["effective_independent_signal"] == False).sum()) if not gf.empty else 0  # noqa: E712
        oos_values = pd.to_numeric(of.get("test_ic", pd.Series(dtype=float)), errors="coerce").dropna()
        best_raw_metric = _finite_or_none(pd.to_numeric(gf.get("forward_return_ic_24h", pd.Series(dtype=float)), errors="coerce").abs().max()) if not gf.empty else None
        median_metric = _finite_or_none(pd.to_numeric(gf.get("forward_return_ic_24h", pd.Series(dtype=float)), errors="coerce").median()) if not gf.empty else None
        corresponding_oos = _finite_or_none(oos_values.median()) if not oos_values.empty else None
        null_status = _family_null_status(nf)
        oos_status = _family_oos_status(of)
        prefilter_status = _family_prefilter_status(
            family=family,
            diagnostics=gf,
            oos_status=oos_status,
            null_status=null_status,
            duplicate_ratio=_safe_div(duplicate_count, len(gf)),
        )
        rows.append(
            {
                "signal_family": family,
                "generated_signal_count": int(len(gf)),
                "evaluated_signal_count": int((gf["non_null_observation_count"] > 0).sum()) if not gf.empty else 0,
                "duplicate_signal_count": duplicate_count,
                "family_duplicate_ratio": _safe_div(duplicate_count, len(gf)),
                "effective_signal_breadth": effective,
                "symbol_count": len(used_symbols),
                "symbol_concentration_warning": bool(len(used_symbols) < 3),
                "family_level_best_raw_metric": best_raw_metric,
                "family_level_median_metric": median_metric,
                "corresponding_oos_metric": corresponding_oos,
                "chronological_oos_status": oos_status,
                "null_comparison_status": null_status,
                "leakage_check_status": "passed" if bool((gf.get("leakage_check_status", pd.Series(dtype=str)) == "passed_shifted_by_decision_lag").all()) else "failed",
                "representative_signal_id": _family_representative_id(cf),
                "representative_selection_rule": "lowest_complexity_then_depth_then_lexicographic_signal_id",
                "family_prefilter_status": prefilter_status,
                "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
            }
        )
    return rows


def _multiple_testing_rows(
    *,
    diagnostics: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
    generated_signal_count: int,
    horizons: tuple[int, ...],
) -> list[dict[str, Any]]:
    effective_breadth = len({row["duplicate_cluster_id"] for row in clusters if not bool(row.get("is_redundant_duplicate"))})
    duplicate_count = sum(1 for row in clusters if bool(row.get("is_redundant_duplicate")))
    rows = [
        {
            "scope": "suite",
            "signal_family": "ALL",
            "total_generated_signal_count": int(generated_signal_count),
            "total_evaluated_signal_count": len(diagnostics),
            "total_signal_tests": int(len(diagnostics) * len(horizons)),
            "duplicate_adjusted_test_count": int(effective_breadth * len(horizons)),
            "effective_signal_breadth": int(effective_breadth),
            "duplicate_signal_count": int(duplicate_count),
            "duplicate_ratio": _safe_div(duplicate_count, len(clusters)),
            "family_level_best_raw_metric": _finite_or_none(
                pd.to_numeric(pd.DataFrame(family_summary).get("family_level_best_raw_metric", pd.Series(dtype=float)), errors="coerce").max()
            ),
            "family_level_median_metric": _finite_or_none(
                pd.to_numeric(pd.DataFrame(family_summary).get("family_level_median_metric", pd.Series(dtype=float)), errors="coerce").median()
            ),
            "corresponding_oos_metric": _finite_or_none(
                pd.to_numeric(pd.DataFrame(family_summary).get("corresponding_oos_metric", pd.Series(dtype=float)), errors="coerce").median()
            ),
            "approximate_false_discovery_warning": "high",
            "multiple_testing_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_MULTIPLE_TESTING_BOUNDARY,
            "cannot_claim_edge_due_to_multiple_testing": True,
        }
    ]
    for row in family_summary:
        rows.append(
            {
                "scope": "family",
                "signal_family": row["signal_family"],
                "total_generated_signal_count": int(row["generated_signal_count"]),
                "total_evaluated_signal_count": int(row["evaluated_signal_count"]),
                "total_signal_tests": int(row["evaluated_signal_count"]) * len(horizons),
                "duplicate_adjusted_test_count": int(row["effective_signal_breadth"]) * len(horizons),
                "effective_signal_breadth": int(row["effective_signal_breadth"]),
                "duplicate_signal_count": int(row["duplicate_signal_count"]),
                "duplicate_ratio": row["family_duplicate_ratio"],
                "family_level_best_raw_metric": row["family_level_best_raw_metric"],
                "family_level_median_metric": row["family_level_median_metric"],
                "corresponding_oos_metric": row["corresponding_oos_metric"],
                "approximate_false_discovery_warning": "high",
                "multiple_testing_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_MULTIPLE_TESTING_BOUNDARY,
                "cannot_claim_edge_due_to_multiple_testing": True,
            }
        )
    return rows


def _shortlist_rows(family_summary: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for row in family_summary:
        status = str(row["family_prefilter_status"])
        if status == "needs_strict_oos_falsification":
            label: SignalLabel = "NEEDS_STRICT_OOS_FALSIFICATION"
            rationale = "family passed pre-registered diagnostic gates and must be frozen before stricter falsification"
        elif status == "watch_family_only":
            label = "WATCH_FAMILY_ONLY"
            rationale = "family has diagnostic activity but insufficient strict null/OOS robustness for a V14 queue"
        elif status == "duplicate_dominated":
            label = "DUPLICATE_SIGNAL"
            rationale = "family breadth dominated by duplicate/correlated signals"
        else:
            label = "KILL_SIGNAL"
            rationale = "family failed strict prefilter gates"
        rows.append(
            {
                "signal_family": row["signal_family"],
                "signal_label": label,
                "shortlist_category": label,
                "effective_signal_breadth": row["effective_signal_breadth"],
                "corresponding_oos_metric": row["corresponding_oos_metric"],
                "rationale": rationale,
                "paper_only": True,
                "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
            }
        )
    forbidden = {str(row["signal_label"]) for row in rows} & HISTORICAL_KLINE_MICROSTRUCTURE_FORBIDDEN_OUTPUT_LABELS
    if forbidden:
        raise ValueError(f"forbidden V13 signal label emitted: {sorted(forbidden)}")
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
        summary = summary_by_family[str(row["signal_family"])]
        rows.append(
            {
                "signal_family": row["signal_family"],
                "queue_decision": "NEEDS_STRICT_OOS_FALSIFICATION",
                "paper_only": True,
                "effective_signal_breadth": summary["effective_signal_breadth"],
                "corresponding_oos_metric": summary["corresponding_oos_metric"],
                "reason": row["rationale"],
                "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
            }
        )
    return rows


def _failed_gate_rows(
    *,
    diagnostics: list[dict[str, Any]],
    family_summary: list[dict[str, Any]],
    clusters: list[dict[str, Any]],
    excluded_symbols: list[dict[str, Any]],
    used_symbols: tuple[str, ...],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if len(used_symbols) < 3:
        rows.append(_failed_gate("minimum_multi_asset_symbol_count", "failed", f"used_symbol_count={len(used_symbols)}"))
    for item in excluded_symbols:
        rows.append(_failed_gate("symbol_excluded_with_reason", "diagnostic", f"{item.get('symbol')}: {item.get('reason')}"))
    duplicate_ratio = _safe_div(sum(1 for row in clusters if bool(row.get("is_redundant_duplicate"))), len(clusters))
    if duplicate_ratio >= 0.95:
        rows.append(_failed_gate("duplicate_ratio", "failed", f"duplicate_ratio={duplicate_ratio}"))
    for row in family_summary:
        status = str(row["family_prefilter_status"])
        if status != "needs_strict_oos_falsification":
            rows.append(_failed_gate(f"family_{row['signal_family']}", "failed", status))
    if any(row.get("leakage_check_status") != "passed_shifted_by_decision_lag" for row in diagnostics):
        rows.append(_failed_gate("leakage_check_status", "failed", "at least one signal failed decision-lag leakage check"))
    return rows


def _suite_decision(shortlist: list[dict[str, Any]], duplicate_ratio: float) -> str:
    if not shortlist:
        return "HISTORICAL_KLINE_MICROSTRUCTURE_FACTORY_INFRA_ONLY"
    labels = {str(row["signal_label"]) for row in shortlist}
    if "NEEDS_STRICT_OOS_FALSIFICATION" in labels:
        return "NEEDS_STRICT_OOS_FALSIFICATION"
    if duplicate_ratio >= 0.95:
        return "DUPLICATES_DOMINATE"
    if all(label == "KILL_SIGNAL" for label in labels):
        return "NO_SURVIVING_SIGNAL_FAMILY"
    if "WATCH_FAMILY_ONLY" in labels or "WATCH_DIAGNOSTIC_ONLY" in labels:
        return "WATCH_FAMILY_ONLY_PAPER_RESEARCH"
    return "FAILS_NULL_ROBUSTNESS"


def _family_prefilter_status(
    *,
    family: str,
    diagnostics: pd.DataFrame,
    oos_status: str,
    null_status: str,
    duplicate_ratio: float,
) -> str:
    if diagnostics.empty:
        return "killed_no_signals"
    if family == "microstructure_x_funding":
        return "killed_funding_unavailable_by_raw_kline_only_boundary"
    if bool((diagnostics.get("leakage_check_status", pd.Series(dtype=str)) != "passed_shifted_by_decision_lag").any()):
        return "killed_leakage_check_failed"
    if duplicate_ratio >= 0.95:
        return "duplicate_dominated"
    if oos_status != "oos_consistent":
        return "killed_oos_inconsistent"
    if null_status != "null_robust":
        return "killed_null_fragile"
    median_abs_ic = abs(float(pd.to_numeric(diagnostics.get("forward_return_ic_24h", pd.Series(dtype=float)), errors="coerce").median()))
    if not math.isfinite(median_abs_ic) or median_abs_ic < 0.01:
        return "watch_family_only"
    return "needs_strict_oos_falsification"


def _family_oos_status(oos_frame: pd.DataFrame) -> str:
    if oos_frame.empty:
        return "insufficient_oos_folds"
    test_values = pd.to_numeric(oos_frame.get("test_ic", pd.Series(dtype=float)), errors="coerce").dropna()
    validation_values = pd.to_numeric(oos_frame.get("validation_ic", pd.Series(dtype=float)), errors="coerce").dropna()
    if len(test_values) < 3 or len(validation_values) < 3:
        return "insufficient_oos_observations"
    consistency = _sign_consistency_rate(test_values.tolist())
    validation_test_same_sign = _same_sign_rate(validation_values.tolist(), test_values.tolist())
    if consistency >= 0.67 and validation_test_same_sign >= 0.67:
        return "oos_consistent"
    return "oos_inconsistent"


def _family_null_status(null_frame: pd.DataFrame) -> str:
    if null_frame.empty:
        return "missing_null_comparison"
    comparable = null_frame[null_frame["baseline_ic"].notna()] if "baseline_ic" in null_frame.columns else pd.DataFrame()
    if comparable.empty:
        return "null_summary_only"
    beat_rate = _boolean_rate(comparable.get("signal_beats_baseline", pd.Series(dtype=object)))
    return "null_robust" if beat_rate is not None and beat_rate >= 0.8 else "null_fragile"


def _read_raw_kline_zip_frame(path: Path) -> dict[str, Any]:
    try:
        with zipfile.ZipFile(path) as zf:
            csv_names = [name for name in zf.namelist() if name.lower().endswith(".csv")]
            if len(csv_names) != 1:
                raise ValueError(f"expected exactly one CSV in {path}, found {csv_names}")
            payload = zf.read(csv_names[0])
        sample = payload[:256].decode("utf-8", errors="replace")
        first_cell = next(csv.reader(io.StringIO(sample)))[0].strip() if sample.strip() else ""
        has_header = first_cell == "open_time"
        frame = pd.read_csv(io.BytesIO(payload), header=0 if has_header else None)
        if not has_header:
            frame.columns = [
                "open_time",
                "open",
                "high",
                "low",
                "close",
                "volume",
                "close_time",
                "quote_volume",
                "count",
                "taker_buy_volume",
                "taker_buy_quote_volume",
                "ignore",
            ][: len(frame.columns)]
        frame.columns = [str(column).strip() for column in frame.columns]
        return {
            "parse_ok": True,
            "frame": frame,
            "schema_columns": ",".join(str(column) for column in frame.columns),
            "error": "",
        }
    except (OSError, ValueError, zipfile.BadZipFile, pd.errors.ParserError) as exc:
        return {
            "parse_ok": False,
            "frame": pd.DataFrame(),
            "schema_columns": "",
            "error": f"{type(exc).__name__}: {exc}",
        }


def _resolve_required_raw_columns(frame: pd.DataFrame) -> dict[str, Any]:
    resolved: dict[str, str] = {
        "open": "open",
        "high": "high",
        "low": "low",
        "close": "close",
        "volume": "volume",
    }
    missing = [column for column in ("open", "high", "low", "close", "volume") if column not in frame.columns]
    for canonical, aliases in MICROSTRUCTURE_RAW_COLUMN_ALIASES.items():
        match = next((alias for alias in aliases if alias in frame.columns), None)
        if match is None:
            missing.append(canonical)
        else:
            resolved[canonical] = match
    return {"resolved": resolved, "missing": missing}


def _normalize_raw_kline_frame(frame: pd.DataFrame, resolved: dict[str, str]) -> pd.DataFrame:
    out = pd.DataFrame(
        {
            "timestamp": _timestamp_series(frame["open_time"]),
            "open": pd.to_numeric(frame[resolved["open"]], errors="coerce"),
            "high": pd.to_numeric(frame[resolved["high"]], errors="coerce"),
            "low": pd.to_numeric(frame[resolved["low"]], errors="coerce"),
            "close": pd.to_numeric(frame[resolved["close"]], errors="coerce"),
            "volume": pd.to_numeric(frame[resolved["volume"]], errors="coerce"),
            "quote_asset_volume": pd.to_numeric(frame[resolved["quote_asset_volume"]], errors="coerce"),
            "number_of_trades": pd.to_numeric(frame[resolved["number_of_trades"]], errors="coerce"),
            "taker_buy_base_volume": pd.to_numeric(frame[resolved["taker_buy_base_volume"]], errors="coerce"),
            "taker_buy_quote_volume": pd.to_numeric(frame[resolved["taker_buy_quote_volume"]], errors="coerce"),
        }
    )
    return _frame(out.dropna(subset=["timestamp", "open", "high", "low", "close"]))


def _parse_feature(feature: str) -> tuple[str, str, int]:
    parts = feature.split(":")
    if len(parts) != 3 or not parts[2].startswith("w"):
        raise ValueError(f"invalid V13 feature spec: {feature}")
    return parts[0], parts[1], int(parts[2][1:])


def _apply_unary_operation(
    frame: pd.DataFrame,
    panel: pd.DataFrame,
    *,
    symbol: str,
    op: str,
    base: str,
    window: int,
) -> pd.Series:
    source = _base_series(frame, base)
    if op == "raw":
        return source
    if op == "neg":
        return -source
    if op == "rolling_mean":
        return _series(source.rolling(window, min_periods=max(2, min(window, 12))).mean())
    if op == "neg_rolling_mean":
        return -_series(source.rolling(window, min_periods=max(2, min(window, 12))).mean())
    if op == "rolling_diff":
        return _series(source - source.shift(window))
    if op == "rolling_zscore":
        return rolling_zscore(source, window=window, min_periods=max(2, min(window, 12)))
    if op == "rolling_zscore_neg":
        return -rolling_zscore(source, window=window, min_periods=max(2, min(window, 12)))
    if op == "cs_rank":
        return _cs_rank_series(frame, base)
    if op == "cs_rank_neg":
        return -_cs_rank_series(frame, base)
    if op == "median_spread":
        return _series(source - frame.get(f"universe_{base}_median", pd.Series(index=frame.index, dtype=float)))
    if op == "median_spread_neg":
        return -_series(source - frame.get(f"universe_{base}_median", pd.Series(index=frame.index, dtype=float)))
    if op == "return_direction_interaction":
        return _series(source * frame["return"].map(_safe_sign))
    if op == "return_divergence":
        return _series(source - rolling_zscore(_series(frame["return"]).fillna(0.0), window=window, min_periods=max(2, min(window, 12))))
    if op == "volatility_divergence":
        return _series(source - rolling_zscore(_series(frame["realized_volatility"]), window=window, min_periods=max(2, min(window, 12))))
    if op == "volume_divergence":
        return _series(source - _series(frame["volume_zscore"]))
    if op == "btc_lag_interaction":
        return _series(source * frame.get(f"btc_{base}_lag1", pd.Series(index=frame.index, dtype=float)))
    if op == "eth_lag_interaction":
        return _series(source * frame.get(f"eth_{base}_lag1", pd.Series(index=frame.index, dtype=float)))
    if op == "leader_lag_spread":
        btc = frame.get(f"btc_{base}_lag1", pd.Series(index=frame.index, dtype=float))
        eth = frame.get(f"eth_{base}_lag1", pd.Series(index=frame.index, dtype=float))
        leader = _series(btc).where(_series(btc).notna(), _series(eth))
        return _series(source - leader)
    if op == "leader_lag_rank_interaction":
        rank = _cs_rank_series(frame, base)
        btc = frame.get(f"btc_{base}_lag1", pd.Series(index=frame.index, dtype=float))
        eth = frame.get(f"eth_{base}_lag1", pd.Series(index=frame.index, dtype=float))
        leader = _series(btc).where(_series(btc).notna(), _series(eth))
        return _series(leader * rank)
    if op == "funding_unavailable_boundary" or op == "funding_rank_unavailable_boundary":
        return pd.Series(math.nan, index=frame.index, dtype=float)
    if op == "high_volatility_gate":
        return _series(source * (_series(frame["realized_volatility"]) > _series(frame["universe_realized_volatility_median"])).astype(float))
    if op == "low_volatility_gate":
        return _series(source * (_series(frame["realized_volatility"]) <= _series(frame["universe_realized_volatility_median"])).astype(float))
    if op == "volatility_rank_interaction":
        return _series(source * _series(frame["cross_sectional_microstructure_volatility_rank"]))
    if op == "volatility_change_interaction":
        return _series(source * rolling_zscore(_series(frame["realized_volatility_change"]), window=window, min_periods=max(2, min(window, 12))))
    raise ValueError(f"unsupported V13 unary operation: {op}")


def _pair_signal_series(spec: KlineMicrostructureSignalSpec, panel: pd.DataFrame) -> pd.Series:
    if spec.pair_scope is None:
        return pd.Series(dtype=float)
    left, right = spec.pair_scope
    left_frame = _symbol_panel(panel, left)
    right_frame = _symbol_panel(panel, right)
    if left_frame.empty or right_frame.empty:
        return pd.Series(dtype=float)
    joined = left_frame.join(right_frame, how="inner", lsuffix="_left", rsuffix="_right")
    op, base, window = _parse_feature(spec.feature_name)
    left_base = _series(joined[f"{base}_left"])
    right_base = _series(joined[f"{base}_right"])
    if op == "pair_spread":
        values = left_base - right_base
    elif op == "pair_zscore_spread":
        values = rolling_zscore(left_base - right_base, window=window, min_periods=max(2, min(window, 12)))
    elif op == "pair_lead_lag_spread":
        values = left_base - _series(right_base.shift(1))
    elif op == "pair_leader_interaction":
        values = left_base * _series(right_base.shift(1))
    else:
        raise ValueError(f"unsupported V13 pair operation: {op}")
    values.index = _multi_index_for_symbol(joined, left)
    return _series(pd.to_numeric(values.replace([math.inf, -math.inf], math.nan), errors="coerce"))


def _base_series(frame: pd.DataFrame, base: str) -> pd.Series:
    if base not in frame.columns:
        raise ValueError(f"missing V13 panel feature: {base}")
    return _coerce_float_series(_series(frame[base]))


def _cs_rank_series(frame: pd.DataFrame, base: str) -> pd.Series:
    mapping = {
        "taker_imbalance_zscore": "cross_sectional_taker_imbalance_rank",
        "trade_count_zscore": "cross_sectional_trade_count_rank",
        "quote_volume_zscore": "cross_sectional_quote_volume_rank",
        "avg_trade_size_proxy": "cross_sectional_avg_trade_size_rank",
        "microstructure_volatility_interaction": "cross_sectional_microstructure_volatility_rank",
    }
    return _coerce_float_series(_series(frame[mapping.get(base, base)]))


def _empty_signal_for_scope(spec: KlineMicrostructureSignalSpec, panel: pd.DataFrame) -> pd.Series:
    if spec.symbol_scope:
        frame = _symbol_panel(panel, spec.symbol_scope[0])
        empty = pd.Series(math.nan, index=_multi_index_for_symbol(frame, spec.symbol_scope[0]), dtype=float)
        return empty
    return pd.Series(dtype=float)


def _forward_return_labels(panel: pd.DataFrame, *, horizons: Iterable[int]) -> dict[int, pd.Series]:
    labels: dict[int, pd.Series] = {}
    if panel.empty:
        return {int(horizon): pd.Series(dtype=float) for horizon in horizons}
    close = _coerce_float_series(_series(panel["close"]))
    close.index = panel.index
    for horizon in horizons:
        h = int(horizon)
        forward = close.groupby(level="symbol", group_keys=False).shift(-h)
        labels[h] = _series(forward / close - 1.0)
    return labels


def _fast_segment_ic(signal: pd.Series, labels: pd.Series, fold: dict[str, Any], segment: str) -> float | None:
    if not isinstance(signal.index, pd.MultiIndex) or "timestamp" not in signal.index.names:
        return _segment_ic(signal, labels, fold, segment)
    start_key = f"{segment}_start_timestamp"
    end_key = f"{segment}_end_timestamp"
    if start_key not in fold or end_key not in fold:
        return None
    timestamps = pd.DatetimeIndex(signal.index.get_level_values("timestamp"))
    start = pd.Timestamp(fold[start_key])
    end = pd.Timestamp(fold[end_key])
    mask = (timestamps >= start) & (timestamps <= end)
    if not bool(mask.any()):
        return None
    return _series_corr(_coerce_float_series(signal[mask]), _coerce_float_series(labels.reindex(signal.index)[mask]))


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
    expected_values = _series(expected.loc[comparable]).to_numpy(dtype=float)
    observed_values = _series(observed.loc[comparable]).to_numpy(dtype=float)
    return not bool(np.allclose(expected_values, observed_values, rtol=0.0, atol=1e-12))


def _symbol_panel(panel: pd.DataFrame, symbol: str) -> pd.DataFrame:
    if panel.empty:
        return pd.DataFrame()
    if isinstance(panel.index, pd.MultiIndex):
        try:
            return _frame(panel.xs(symbol, level="symbol", drop_level=False).copy())
        except KeyError:
            return pd.DataFrame()
    return _frame(panel[panel["symbol"] == symbol].copy())


def _multi_index_for_symbol(frame: pd.DataFrame, symbol: str) -> pd.MultiIndex:
    timestamps = frame.index.get_level_values("timestamp") if isinstance(frame.index, pd.MultiIndex) else pd.DatetimeIndex(frame["timestamp"])
    return pd.MultiIndex.from_arrays([timestamps, [symbol] * len(frame)], names=["timestamp", "symbol"])


def _union_signal_index(signals: Iterable[pd.Series]) -> pd.Index:
    index: pd.Index | None = None
    for signal in signals:
        index = signal.index if index is None else index.union(signal.index)
    return index if index is not None else pd.Index([])


def _cluster_representative(
    cluster: set[str],
    spec_by_id: dict[str, KlineMicrostructureSignalSpec],
) -> str:
    return sorted(
        cluster,
        key=lambda signal_id: (
            spec_by_id[signal_id].complexity_score,
            spec_by_id[signal_id].depth,
            signal_id,
        ),
    )[0]


def _family_representative_id(cluster_frame: pd.DataFrame) -> str | None:
    if cluster_frame.empty:
        return None
    representative_rows = cluster_frame[cluster_frame["is_redundant_duplicate"] == False]  # noqa: E712
    if representative_rows.empty:
        representative_rows = cluster_frame
    ranked = representative_rows.sort_values(["complexity_score", "depth", "signal_id"])
    return str(ranked.iloc[0]["signal_id"]) if not ranked.empty else None


def _family_representative_signal_ids(
    specs: list[KlineMicrostructureSignalSpec],
    clusters: list[dict[str, Any]],
) -> dict[str, str]:
    cluster_frame = pd.DataFrame(clusters)
    rows: dict[str, str] = {}
    for family in sorted({spec.signal_family for spec in specs}):
        cf = cluster_frame[cluster_frame["signal_family"] == family] if not cluster_frame.empty else pd.DataFrame()
        representative = _family_representative_id(cf)
        if representative is None:
            candidates = [spec for spec in specs if spec.signal_family == family]
            representative = sorted(candidates, key=lambda spec: (spec.complexity_score, spec.depth, spec.signal_id))[0].signal_id
        rows[family] = representative
    return rows


def _universe_median_signal_like(signal: pd.Series) -> pd.Series:
    if not isinstance(signal.index, pd.MultiIndex) or "timestamp" not in signal.index.names:
        return pd.Series(0.0, index=signal.index, dtype=float)
    median = signal.groupby(level="timestamp").transform("median")
    return _series(median)


def _sign_consistency_rate(values: list[float]) -> float | None:
    finite = [value for value in values if math.isfinite(value) and value != 0.0]
    if not finite:
        return None
    positive = sum(1 for value in finite if value > 0.0)
    negative = sum(1 for value in finite if value < 0.0)
    return max(positive, negative) / len(finite)


def _same_sign_rate(left: list[float], right: list[float]) -> float | None:
    pairs = [
        (a, b)
        for a, b in zip(left, right, strict=False)
        if math.isfinite(a) and math.isfinite(b) and a != 0.0 and b != 0.0
    ]
    if not pairs:
        return None
    return sum(1 for a, b in pairs if (a > 0.0) == (b > 0.0)) / len(pairs)


def _boolean_rate(values: pd.Series) -> float | None:
    if values.empty:
        return None
    clean = [bool(item) for item in values.dropna().tolist()]
    return _safe_div(sum(1 for item in clean if item), len(clean)) if clean else None


def _turnover_proxy(signal: pd.Series) -> float | None:
    if signal.empty:
        return None
    if isinstance(signal.index, pd.MultiIndex) and "symbol" in signal.index.names:
        return _finite_or_none(signal.groupby(level="symbol").diff().abs().mean())
    return _finite_or_none(signal.diff().abs().mean())


def _autocorr_or_none(series: pd.Series) -> float | None:
    if len(series) < 3:
        return None
    value = series.autocorr(lag=1)
    return _finite_or_none(value)


def _fast_sign_hit_rate(signal: pd.Series, labels: pd.Series) -> float | None:
    data = pd.concat(
        [
            _coerce_float_series(signal).rename("signal"),
            _coerce_float_series(labels).rename("label"),
        ],
        axis=1,
    ).dropna()
    if data.empty:
        return None
    signal_sign = np.sign(data["signal"].to_numpy(dtype=float))
    label_sign = np.sign(data["label"].to_numpy(dtype=float))
    valid = (signal_sign != 0.0) & (label_sign != 0.0)
    if not bool(valid.any()):
        return None
    return float((signal_sign[valid] == label_sign[valid]).mean())


def _fast_symbol_permuted_signal(signal: pd.Series, label: str, *, seed_base: int) -> pd.Series:
    if not isinstance(signal.index, pd.MultiIndex) or "symbol" not in signal.index.names:
        return symbol_permuted_signal(signal, label, seed_base=seed_base)
    wide = signal.unstack("symbol")
    columns = list(wide.columns)
    if len(columns) <= 1:
        return signal.copy()
    rng = np.random.default_rng(deterministic_random_seed(f"symbol-permute-{label}", seed_base=seed_base))
    permuted = list(columns)
    rng.shuffle(permuted)
    if permuted == columns:
        permuted = permuted[1:] + permuted[:1]
    permuted_wide = wide.loc[:, permuted]
    permuted_wide.columns = columns
    stacked = permuted_wide.stack(dropna=False)
    stacked.index = stacked.index.set_names(signal.index.names)
    return _series(stacked.reindex(signal.index))


def _safe_ratio(numerator: pd.Series | np.ndarray | float, denominator: pd.Series | np.ndarray | float) -> pd.Series:
    num = pd.Series(numerator) if not isinstance(numerator, pd.Series) else numerator
    den = pd.Series(denominator) if not isinstance(denominator, pd.Series) else denominator
    return _series(num / den.replace(0, math.nan))


def _safe_sign(value: Any) -> float:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return 0.0
    if not math.isfinite(number) or number == 0.0:
        return 0.0
    return 1.0 if number > 0.0 else -1.0


def _timestamp_series(value: pd.Series) -> pd.Series:
    numeric = pd.to_numeric(value, errors="coerce")
    if numeric.dropna().empty:
        return pd.to_datetime(value, utc=True, errors="coerce")
    unit = "ms" if float(numeric.dropna().median()) > 10_000_000_000 else "s"
    return pd.to_datetime(numeric, unit=unit, utc=True, errors="coerce")


def _utc_timestamp(value: str) -> pd.Timestamp:
    ts = pd.Timestamp(value)
    return ts.tz_localize("UTC") if ts.tzinfo is None else ts.tz_convert("UTC")


def _month_strings_between(start_ts: pd.Timestamp, end_ts: pd.Timestamp) -> tuple[str, ...]:
    start_month = start_ts.tz_convert("UTC").tz_localize(None) if start_ts.tzinfo else start_ts
    end_month = end_ts.tz_convert("UTC").tz_localize(None) if end_ts.tzinfo else end_ts
    return tuple(str(period) for period in pd.period_range(start=start_month.to_period("M"), end=end_month.to_period("M"), freq="M"))


def _normalize_symbols(symbols: Iterable[str]) -> tuple[str, ...]:
    seen: set[str] = set()
    normalized: list[str] = []
    for raw in symbols:
        symbol = str(raw).strip().upper()
        if not symbol or symbol in seen:
            continue
        seen.add(symbol)
        normalized.append(symbol)
    return tuple(normalized)


def _combined_hash(hashes: Iterable[str]) -> str:
    return "|".join(sorted(str(item) for item in hashes))


def _read_csv_records(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    return pd.read_csv(path).fillna("").to_dict(orient="records")


def _truthy(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    return str(value).strip().lower() in {"1", "true", "yes", "y"}


def _min_timestamp(values: Iterable[Any]) -> str | None:
    timestamps = [pd.Timestamp(value) for value in values if value not in (None, "")]
    return _timestamp(min(timestamps)) if timestamps else None


def _max_timestamp(values: Iterable[Any]) -> str | None:
    timestamps = [pd.Timestamp(value) for value in values if value not in (None, "")]
    return _timestamp(max(timestamps)) if timestamps else None


def _excluded_symbols(v12_payload: dict[str, Any], panel_excluded: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows = [dict(row) for row in v12_payload.get("excluded_symbols", [])]
    rows.extend(panel_excluded)
    return rows


def _non_raw_changed_files(paths: Iterable[str]) -> list[str]:
    return [
        path
        for path in (str(item) for item in paths)
        if not (path == "data" or path.startswith("data/") or "/data/" in path)
    ]


def _v10_negative_control_families() -> list[str]:
    path = Path("reports/loops/20260628-v9-falsification-lineage-negative-control-governance-v10/negative_control_registry.csv")
    if not path.exists():
        return []
    frame = pd.read_csv(path)
    if "signal_family" not in frame.columns:
        return []
    return sorted(str(item) for item in frame["signal_family"].dropna().unique())


def _failed_gate(gate: str, status: str, reason: str) -> dict[str, Any]:
    return {
        "gate": gate,
        "status": status,
        "failure_reason": reason,
        "claim_boundary": HISTORICAL_KLINE_MICROSTRUCTURE_CLAIM_BOUNDARY,
    }


def _next_queue_frame(rows: list[dict[str, Any]]) -> pd.DataFrame:
    if rows:
        return pd.DataFrame(rows, columns=HISTORICAL_KLINE_MICROSTRUCTURE_NEXT_QUEUE_COLUMNS)
    return pd.DataFrame(columns=HISTORICAL_KLINE_MICROSTRUCTURE_NEXT_QUEUE_COLUMNS)


def _generic_loop_payload(payload: dict[str, Any]) -> dict[str, Any]:
    return {
        "loop_id": payload["loop_id"],
        "status": payload["status"],
        "loop_type": payload["loop_type"],
        "hypothesis": (
            "Historical public kline-derived microstructure proxy features may expose non-duplicative "
            "paper-only signal hypotheses; V13 only prefilters and may not claim edge."
        ),
        "edge_source": "structural_microstructure",
        "baseline": str(DEFAULT_HISTORICAL_KLINE_MICROSTRUCTURE_V12_ARTIFACT),
        "strategy_family": "HISTORICAL_KLINE_MICROSTRUCTURE_PROXY_SIGNAL_FACTORY_V13",
        "data_scope": "public Binance Vision raw USD-M futures 1h klines, 2024-01-01 through 2025-12-31",
        "selection_inputs": ["diagnostic_prefilter_only"],
        "selection_excludes": ["holdout", "live_orders", "api_keys", "account_state", "recent_window_endpoint_data"],
        "pre_registered_grid": {
            "candidate_budget": payload["candidate_budget"],
            "max_depth": payload["max_depth"],
            "families": payload["families_evaluated"],
            "horizons": payload["horizons_evaluated"],
        },
        "expected_failure_modes": [
            "duplicate_family_dominance",
            "null_fragility",
            "chronological_oos_inconsistency",
            "multiple_testing_boundary",
        ],
        "cost_model": {"fees": "not_strategy_pnl", "spread": "not_strategy_pnl", "slippage": "not_strategy_pnl"},
        "safety_gates": payload["safety_gates"],
        "validation_commands": payload["validation_commands"],
        "evidence_artifacts": payload["evidence_artifacts"],
        "failed_gates": payload.get("failed_gates", []),
        "decision": payload["suite_decision"],
        "next_loop": payload["next_loop"],
    }


def _safety_gates() -> dict[str, Any]:
    return {
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "holdout_retune_used": False,
        "strategy_optimization_performed": False,
        "live_or_testnet_path_created": False,
        "data_download_performed": False,
        "tsmom_state_changed": False,
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
    }


def _assert_payload_safe(payload: dict[str, Any]) -> None:
    forbidden = {str(payload.get("suite_decision", ""))} & HISTORICAL_KLINE_MICROSTRUCTURE_FORBIDDEN_OUTPUT_LABELS
    if forbidden:
        raise ValueError(f"forbidden V13 suite decision emitted: {sorted(forbidden)}")
    if payload.get("readiness_baseline_verdict") != READINESS_BASELINE_VERDICT:
        raise ValueError("V13 payload must keep readiness_baseline_verdict=NOT_READY")
    for field in (
        "recent_window_data_used",
        "no_live_orders_submitted",
        "no_testnet_orders_submitted",
        "no_account_data_used",
        "no_api_key_used",
        "no_private_order_or_fill_data_used",
    ):
        expected = False if field == "recent_window_data_used" else True
        if bool(payload.get(field)) is not expected:
            raise ValueError(f"V13 safety field violated: {field}")
    for field in (
        "holdout_retune_used",
        "strategy_optimization_performed",
        "live_or_testnet_path_created",
        "data_download_performed",
        "tsmom_state_changed",
    ):
        if bool(payload.get(field)):
            raise ValueError(f"V13 safety field must remain false: {field}")


def _compact_failed_gate_rows(rows: list[dict[str, Any]]) -> list[dict[str, str]]:
    compact: list[dict[str, str]] = []
    seen: set[tuple[str, str, str]] = set()
    for row in rows:
        item = (
            str(row.get("gate", "")),
            str(row.get("status", "")),
            str(row.get("failure_reason", "")),
        )
        if item in seen:
            continue
        seen.add(item)
        compact.append({"gate": item[0], "status": item[1], "failure_reason": item[2]})
    return compact


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    _atomic_write_text(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def _atomic_write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(text)
    tmp.replace(path)
