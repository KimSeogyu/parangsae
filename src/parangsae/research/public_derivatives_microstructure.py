from __future__ import annotations

import csv
import io
import json
import zipfile
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Callable, Iterable, cast

import pandas as pd
import requests

from parangsae.marketdata.binance_funding import BINANCE_USDM_FUNDING_RATE_DOCS, BINANCE_USDM_FUNDING_RATE_URL
from parangsae.marketdata.binance_vision import (
    BINANCE_VISION_BASE,
    canonical_cache_path,
    sha256_file,
    validate_interval,
    validate_symbol,
)
from parangsae.research.loop_artifacts import prepare_loop_output_dir

PUBLIC_DERIVATIVES_MICROSTRUCTURE_SCHEMA_VERSION = 11
PUBLIC_DERIVATIVES_MICROSTRUCTURE_LOOP_SLUG = "public-derivatives-microstructure-data-axis-v11"
PUBLIC_DERIVATIVES_MICROSTRUCTURE_NAMESPACE = (
    "public_derivatives_microstructure_data_axis_v11_only_not_alpha"
)
DEFAULT_PUBLIC_DERIVATIVES_MICROSTRUCTURE_V10_ARTIFACT = Path(
    "reports/loops/20260628-v9-falsification-lineage-negative-control-governance-v10"
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_UNIVERSE = (
    "BTCUSDT",
    "ETHUSDT",
    "SOLUSDT",
    "BNBUSDT",
    "XRPUSDT",
    "ADAUSDT",
    "DOGEUSDT",
    "AVAXUSDT",
    "LINKUSDT",
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_EXCLUDED_SYMBOLS = (
    {
        "symbol": "TONUSDT",
        "reason": "excluded by default because previous coverage left TONUSDT missing in V7/V8 public panel artifacts",
    },
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_TARGET_FAMILIES = (
    "open_interest",
    "long_short_ratio",
    "taker_buy_sell_volume_ratio",
    "liquidation",
    "mark_price_index_basis",
    "funding_rate_extensions",
    "order_book_depth_proxy",
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_NON_FUNDING_FAMILIES = tuple(
    family
    for family in PUBLIC_DERIVATIVES_MICROSTRUCTURE_TARGET_FAMILIES
    if family != "funding_rate_extensions"
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_ALLOWED_SUITE_DECISIONS = (
    "MICROSTRUCTURE_DATA_AXIS_INFRA_ONLY",
    "MICROSTRUCTURE_AXIS_READY_FOR_V12",
    "MICROSTRUCTURE_AXIS_NOT_READY_MISSING_PUBLIC_DATA",
    "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS",
    "MICROSTRUCTURE_AXIS_NOT_READY_SOURCE_UNAVAILABLE",
    "MICROSTRUCTURE_AXIS_PLAN_ONLY",
    "MICROSTRUCTURE_AXIS_FAILED_SAFE",
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_FORBIDDEN_OUTPUTS = {
    "READY",
    "TRADE",
    "LIVE_READY",
    "PROFITABLE",
    "DEPLOY",
    "CAPITAL_ALLOCATE",
    "EDGE_CONFIRMED",
}
PUBLIC_DERIVATIVES_MICROSTRUCTURE_CLAIM_BOUNDARY = (
    "PUBLIC_DERIVATIVES_MICROSTRUCTURE_DATA_AXIS_V11 is public derivatives "
    "microstructure data coverage and provenance infrastructure only. It preserves "
    "the V10 kill-state governance evidence and prepares a future V12 data-axis "
    "input gate. It is not alpha generation, not a trading strategy, not portfolio "
    "construction, not signal weight optimization, not profitability or edge "
    "evidence, not readiness or live readiness, and not permission for live/testnet/"
    "account/API-key/order/fill use."
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_REQUIRED_FILES = (
    "public_derivatives_microstructure_axis.json",
    "data_family_coverage.csv",
    "symbol_data_coverage.csv",
    "missing_data_plan.csv",
    "download_manifest.csv",
    "provenance_manifest.csv",
    "readiness_by_family.csv",
    "v10_lineage_lock_reference.csv",
    "report.md",
    "commands.txt",
    "changed_files.txt",
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_KNOWN_FILES = set(PUBLIC_DERIVATIVES_MICROSTRUCTURE_REQUIRED_FILES) | {
    "loop.json"
}

PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_SCHEMA_VERSION = 12
PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_LOOP_SLUG = (
    "public-derivatives-microstructure-recent-window-v12"
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_NAMESPACE = (
    "public_derivatives_microstructure_recent_window_v12_only_not_alpha"
)
DEFAULT_PUBLIC_DERIVATIVES_MICROSTRUCTURE_V11_ARTIFACT = Path(
    "reports/loops/20260628-public-derivatives-microstructure-data-axis-v11"
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_TARGET_FAMILIES = (
    "kline_derived_microstructure_proxy",
    "open_interest_statistics_recent",
    "global_long_short_ratio_recent",
    "top_trader_long_short_position_ratio_recent",
    "top_trader_long_short_account_ratio_recent",
    "taker_buy_sell_volume_recent",
    "basis_recent",
    "mark_price_premium_index_recent",
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_ENDPOINT_FAMILIES = tuple(
    family
    for family in PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_TARGET_FAMILIES
    if family != "kline_derived_microstructure_proxy"
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_ALLOWED_SUITE_DECISIONS = (
    "MICROSTRUCTURE_RECENT_CONNECTOR_INFRA_ONLY",
    "MICROSTRUCTURE_PROXY_READY_FOR_V13",
    "MICROSTRUCTURE_RECENT_WINDOW_READY_FOR_V13",
    "MICROSTRUCTURE_AXIS_NOT_READY_MISSING_PUBLIC_DATA",
    "MICROSTRUCTURE_AXIS_NOT_READY_RETENTION_LIMITED",
    "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS",
    "MICROSTRUCTURE_AXIS_FAILED_SAFE",
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_CLAIM_BOUNDARY = (
    "PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_CONNECTOR_V12 is public derivatives "
    "microstructure connector and provenance infrastructure only. It preserves the V11/V10 "
    "state, separates long-span kline-derived public proxy coverage from recent-window REST "
    "endpoint coverage, and can only permit a future V13 paper research input gate. It is "
    "not alpha generation, not signal generation, not a trading strategy, not portfolio "
    "construction, not signal weight optimization, not profitability or edge evidence, "
    "not readiness or live readiness, and not permission for live/testnet/account/API-key/"
    "order/fill use."
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_REQUIRED_FILES = (
    "public_derivatives_microstructure_recent_window.json",
    "endpoint_feasibility.csv",
    "kline_microstructure_proxy_coverage.csv",
    "recent_window_coverage.csv",
    "symbol_data_coverage.csv",
    "download_manifest.csv",
    "failed_downloads.csv",
    "provenance_manifest.csv",
    "readiness_by_family.csv",
    "v11_reference.csv",
    "report.md",
    "commands.txt",
    "changed_files.txt",
)
PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_KNOWN_FILES = set(
    PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_REQUIRED_FILES
) | {"loop.json"}

DATA_FAMILY_COVERAGE_COLUMNS = (
    "symbol",
    "data_family",
    "available",
    "source_type",
    "source_location_or_cache_path",
    "first_timestamp",
    "last_timestamp",
    "observation_count",
    "expected_observation_count_if_known",
    "coverage_ratio_if_known",
    "missing_ranges",
    "unavailable_reason",
    "schema_version_or_detected_columns",
    "provenance_hash_if_available",
    "included_for_future_v12",
    "exclusion_reason",
)
SYMBOL_DATA_COVERAGE_COLUMNS = (
    "symbol",
    "available_family_count",
    "available_non_funding_family_count",
    "included_for_future_v12",
    "first_timestamp",
    "last_timestamp",
    "exclusion_reason",
)
MISSING_DATA_PLAN_COLUMNS = (
    "symbol",
    "data_family",
    "missing_item",
    "reason",
    "remediation",
    "public_only",
    "no_private_data_required",
    "source_type",
    "source_location_or_cache_path",
)
DOWNLOAD_MANIFEST_COLUMNS = (
    "symbol",
    "data_family",
    "status",
    "cache_path",
    "source_url",
    "skipped_existing",
    "successful_download",
    "failed_download",
    "reason",
)
PROVENANCE_MANIFEST_COLUMNS = (
    "symbol",
    "data_family",
    "source_type",
    "source_location_or_cache_path",
    "provenance_hash",
    "bytes",
    "first_timestamp",
    "last_timestamp",
    "observation_count",
    "detected_columns",
)
READINESS_BY_FAMILY_COLUMNS = (
    "data_family",
    "coverage_gate",
    "coverage_status",
    "observed_cache_files",
    "missing_items",
    "available_symbol_count",
    "included_symbol_count_for_future_v12",
    "common_start_by_family",
    "common_end_by_family",
    "coverage_sufficient_for_future_v12",
    "not_strategy_readiness",
    "claim_boundary",
)
V10_LINEAGE_LOCK_REFERENCE_COLUMNS = (
    "reference_type",
    "source_v10_artifact_path",
    "source_file",
    "source_hash",
    "source_v9_suite_decision",
    "source_v9_next_queue_empty",
    "signal_family",
    "source_v9_family_decision",
    "lineage_digest",
    "preserved",
)

RECENT_WINDOW_DATA_COVERAGE_COLUMNS = (
    "symbol",
    "data_family",
    "source_endpoint_or_cache",
    "public_source_only",
    "requires_api_key",
    "available",
    "retention_boundary",
    "historical_backfill_supported",
    "recent_window_supported",
    "first_timestamp",
    "last_timestamp",
    "observation_count",
    "expected_observation_count_if_known",
    "coverage_ratio_if_known",
    "missing_ranges",
    "unavailable_reason",
    "schema_columns",
    "provenance_hash_if_available",
    "raw_cache_files_created",
    "raw_cache_files_committed",
    "included_for_future_v13",
    "exclusion_reason",
)
RECENT_WINDOW_ENDPOINT_FEASIBILITY_COLUMNS = (
    "data_family",
    "source_endpoint_or_cache",
    "docs_url",
    "public_source_only",
    "requires_api_key",
    "retention_boundary",
    "historical_backfill_supported",
    "recent_window_supported",
    "default_period",
    "included_for_future_v13",
    "exclusion_reason",
)
RECENT_WINDOW_KLINE_PROXY_COVERAGE_COLUMNS = RECENT_WINDOW_DATA_COVERAGE_COLUMNS
RECENT_WINDOW_DOWNLOAD_MANIFEST_COLUMNS = (
    "symbol",
    "data_family",
    "status",
    "cache_path",
    "source_endpoint_or_cache",
    "skipped_existing",
    "successful_download",
    "failed_download",
    "force",
    "reason",
)
RECENT_WINDOW_FAILED_DOWNLOADS_COLUMNS = (
    "symbol",
    "data_family",
    "cache_path",
    "source_endpoint_or_cache",
    "error_type",
    "reason",
    "public_only",
    "fabricated_data",
)
RECENT_WINDOW_PROVENANCE_MANIFEST_COLUMNS = (
    "symbol",
    "data_family",
    "source_endpoint_or_cache",
    "provenance_hash",
    "bytes",
    "first_timestamp",
    "last_timestamp",
    "observation_count",
    "schema_columns",
    "retention_boundary",
)
RECENT_WINDOW_READINESS_BY_FAMILY_COLUMNS = (
    "data_family",
    "available_symbol_count",
    "included_symbol_count_for_future_v13",
    "common_start_by_family",
    "common_end_by_family",
    "historical_microstructure_proxy_readiness",
    "recent_window_microstructure_readiness",
    "coverage_sufficient_for_future_v13",
    "recent_window_only",
    "claim_boundary",
)
V11_REFERENCE_COLUMNS = (
    "reference_type",
    "source_v11_artifact_path",
    "source_file",
    "source_hash",
    "source_v11_suite_decision",
    "source_v11_coverage_sufficient_for_future_v12",
    "v10_kill_state_preserved",
    "v9_next_queue_empty",
    "preserved",
)

FAMILY_SOURCE_METADATA: dict[str, dict[str, str]] = {
    "open_interest": {
        "source_type": "public_binance_usdm_open_interest_cache_or_plan",
        "source_url": "https://fapi.binance.com/fapi/v1/openInterest",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Open-Interest",
    },
    "long_short_ratio": {
        "source_type": "public_binance_usdm_long_short_ratio_cache_or_plan",
        "source_url": "https://fapi.binance.com/futures/data/globalLongShortAccountRatio",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Long-Short-Ratio",
    },
    "taker_buy_sell_volume_ratio": {
        "source_type": "public_binance_usdm_taker_buy_sell_volume_cache_or_plan",
        "source_url": "https://fapi.binance.com/futures/data/takerlongshortRatio",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Taker-BuySell-Volume",
    },
    "liquidation": {
        "source_type": "public_binance_usdm_force_order_cache_or_plan",
        "source_url": "https://fapi.binance.com/fapi/v1/allForceOrders",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/All-Force-Orders",
    },
    "mark_price_index_basis": {
        "source_type": "public_binance_usdm_mark_index_price_cache_or_plan",
        "source_url": "https://fapi.binance.com/fapi/v1/premiumIndex",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Mark-Price",
    },
    "funding_rate_extensions": {
        "source_type": "existing_public_binance_usdm_funding_cache_or_plan",
        "source_url": BINANCE_USDM_FUNDING_RATE_URL,
        "docs_url": BINANCE_USDM_FUNDING_RATE_DOCS,
    },
    "order_book_depth_proxy": {
        "source_type": "public_order_book_depth_cache_or_plan_only",
        "source_url": "https://fapi.binance.com/fapi/v1/depth",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Order-Book",
    },
}

RECENT_ENDPOINT_METADATA: dict[str, dict[str, str | bool]] = {
    "kline_derived_microstructure_proxy": {
        "source_endpoint_or_cache": BINANCE_VISION_BASE,
        "docs_url": "https://data.binance.vision",
        "retention_boundary": "public_binance_vision_monthly_archive_cache_only; historical span only when raw zip cache exists",
        "historical_backfill_supported": True,
        "recent_window_supported": False,
        "default_period": "1h",
    },
    "open_interest_statistics_recent": {
        "source_endpoint_or_cache": "https://fapi.binance.com/futures/data/openInterestHist",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Open-Interest-Statistics",
        "retention_boundary": "latest_one_month_official_endpoint_retention",
        "historical_backfill_supported": False,
        "recent_window_supported": True,
        "default_period": "1h",
    },
    "global_long_short_ratio_recent": {
        "source_endpoint_or_cache": "https://fapi.binance.com/futures/data/globalLongShortAccountRatio",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Long-Short-Ratio",
        "retention_boundary": "latest_30_days_official_endpoint_retention",
        "historical_backfill_supported": False,
        "recent_window_supported": True,
        "default_period": "1h",
    },
    "top_trader_long_short_position_ratio_recent": {
        "source_endpoint_or_cache": "https://fapi.binance.com/futures/data/topLongShortPositionRatio",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Top-Trader-Long-Short-Ratio",
        "retention_boundary": "latest_30_days_official_endpoint_retention",
        "historical_backfill_supported": False,
        "recent_window_supported": True,
        "default_period": "1h",
    },
    "top_trader_long_short_account_ratio_recent": {
        "source_endpoint_or_cache": "https://fapi.binance.com/futures/data/topLongShortAccountRatio",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Top-Trader-Long-Short-Ratio",
        "retention_boundary": "latest_30_days_official_endpoint_retention",
        "historical_backfill_supported": False,
        "recent_window_supported": True,
        "default_period": "1h",
    },
    "taker_buy_sell_volume_recent": {
        "source_endpoint_or_cache": "https://fapi.binance.com/futures/data/takerlongshortRatio",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Taker-BuySell-Volume",
        "retention_boundary": "latest_30_days_official_endpoint_retention",
        "historical_backfill_supported": False,
        "recent_window_supported": True,
        "default_period": "1h",
    },
    "basis_recent": {
        "source_endpoint_or_cache": "https://fapi.binance.com/futures/data/basis",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Basis",
        "retention_boundary": "latest_30_days_official_endpoint_retention",
        "historical_backfill_supported": False,
        "recent_window_supported": True,
        "default_period": "1h",
    },
    "mark_price_premium_index_recent": {
        "source_endpoint_or_cache": "https://fapi.binance.com/fapi/v1/premiumIndex",
        "docs_url": "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Mark-Price",
        "retention_boundary": "current_snapshot_public_endpoint; no 2024_2025 historical REST backfill assumed",
        "historical_backfill_supported": False,
        "recent_window_supported": True,
        "default_period": "snapshot",
    },
}

KLINE_PROXY_REQUIRED_ALIASES: dict[str, tuple[str, ...]] = {
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
    ),
}
KLINE_PROXY_DERIVED_COLUMNS = (
    "taker_buy_volume_ratio",
    "taker_sell_volume_proxy",
    "taker_imbalance_proxy",
    "trade_count_zscore",
    "volume_imbalance_zscore",
)


@dataclass(frozen=True)
class PublicDerivativesMicrostructureResult:
    payload: dict[str, Any]
    data_family_coverage: list[dict[str, Any]]
    symbol_data_coverage: list[dict[str, Any]]
    missing_data_plan: list[dict[str, Any]]
    download_manifest: list[dict[str, Any]]
    provenance_manifest: list[dict[str, Any]]
    readiness_by_family: list[dict[str, Any]]
    v10_lineage_lock_reference: list[dict[str, Any]]


@dataclass(frozen=True)
class PublicDerivativesMicrostructureRecentWindowResult:
    payload: dict[str, Any]
    endpoint_feasibility: list[dict[str, Any]]
    kline_microstructure_proxy_coverage: list[dict[str, Any]]
    recent_window_coverage: list[dict[str, Any]]
    symbol_data_coverage: list[dict[str, Any]]
    download_manifest: list[dict[str, Any]]
    failed_downloads: list[dict[str, Any]]
    provenance_manifest: list[dict[str, Any]]
    readiness_by_family: list[dict[str, Any]]
    v11_reference: list[dict[str, Any]]


def default_public_derivatives_microstructure_axis_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{PUBLIC_DERIVATIVES_MICROSTRUCTURE_LOOP_SLUG}"


def default_public_derivatives_microstructure_recent_window_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_LOOP_SLUG}"


def parse_public_derivatives_symbol_universe(raw: str | Iterable[str] | None) -> tuple[str, ...]:
    if raw is None:
        return PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_UNIVERSE
    items = raw.split(",") if isinstance(raw, str) else raw
    seen: set[str] = set()
    symbols: list[str] = []
    for item in items:
        value = str(item).strip()
        if not value:
            continue
        symbol = validate_symbol(value)
        if symbol == "TONUSDT":
            continue
        if symbol in seen:
            continue
        seen.add(symbol)
        symbols.append(symbol)
    return tuple(symbols)


def evaluate_public_derivatives_microstructure_axis(
    *,
    source_v10_artifact_path: str | Path = DEFAULT_PUBLIC_DERIVATIVES_MICROSTRUCTURE_V10_ARTIFACT,
    requested_symbols: str | Iterable[str] | None = None,
    data_families: Iterable[str] | None = None,
    cache_root: str | Path = "data/binance-derivatives-microstructure",
    funding_cache_dir: str | Path = "data/binance-funding",
    start: str = "2024-01-01",
    end: str = "2025-12-31",
    minimum_required_symbols: int = 3,
    commands: Iterable[str] | None = None,
    changed_files: Iterable[str] | None = None,
) -> PublicDerivativesMicrostructureResult:
    if minimum_required_symbols <= 0:
        raise ValueError("minimum_required_symbols must be positive")
    symbols = parse_public_derivatives_symbol_universe(requested_symbols)
    families = _normalize_data_families(data_families)
    start_ts = _utc_timestamp(start)
    end_ts = _utc_timestamp(end)
    if end_ts < start_ts:
        raise ValueError("end must be greater than or equal to start")

    v10_path = Path(source_v10_artifact_path)
    v10_state = _read_v10_lineage_lock_state(v10_path)
    data_family_coverage: list[dict[str, Any]] = []
    missing_data_plan: list[dict[str, Any]] = []
    download_manifest: list[dict[str, Any]] = []
    provenance_manifest: list[dict[str, Any]] = []
    skipped_existing_count = 0

    for symbol in symbols:
        for family in families:
            row = _coverage_row_for_family(
                symbol=symbol,
                data_family=family,
                cache_root=Path(cache_root),
                funding_cache_dir=Path(funding_cache_dir),
                start_ts=start_ts,
                end_ts=end_ts,
            )
            data_family_coverage.append(row)
            if row["available"]:
                skipped_existing_count += 1
                download_manifest.append(
                    _download_manifest_row(
                        symbol=symbol,
                        data_family=family,
                        status="skipped_existing",
                        cache_path=str(row["source_location_or_cache_path"]),
                        source_url=FAMILY_SOURCE_METADATA[family]["source_url"],
                        reason="existing public cache file detected; V11 did not redownload",
                    )
                )
                provenance_manifest.append(_provenance_row(row))
            else:
                missing_data_plan.append(
                    _missing_data_plan_row(
                        symbol=symbol,
                        data_family=family,
                        source_location_or_cache_path=str(row["source_location_or_cache_path"]),
                        reason=str(row["unavailable_reason"]),
                    )
                )
                download_manifest.append(
                    _download_manifest_row(
                        symbol=symbol,
                        data_family=family,
                        status="not_attempted_missing_public_cache",
                        cache_path=str(row["source_location_or_cache_path"]),
                        source_url=FAMILY_SOURCE_METADATA[family]["source_url"],
                        reason=str(row["unavailable_reason"]),
                    )
                )

    symbol_data_coverage = _symbol_data_coverage_rows(symbols, data_family_coverage)
    readiness_by_family = _readiness_by_family_rows(families, data_family_coverage)
    usable_symbol_universe = [
        row["symbol"]
        for row in symbol_data_coverage
        if int(row["available_non_funding_family_count"]) > 0
    ]
    data_families_available = sorted(
        {
            str(row["data_family"])
            for row in data_family_coverage
            if bool(row["available"])
        }
    )
    non_funding_symbols = {
        str(row["symbol"])
        for row in data_family_coverage
        if bool(row["available"]) and row["data_family"] in PUBLIC_DERIVATIVES_MICROSTRUCTURE_NON_FUNDING_FAMILIES
    }
    provenance_recorded = all(str(row["provenance_hash"]) for row in provenance_manifest) if provenance_manifest else False
    coverage_sufficient_for_future_v12 = bool(
        len(non_funding_symbols) >= minimum_required_symbols and provenance_recorded and v10_state["v10_kill_state_preserved"]
    )
    excluded_symbols = _excluded_symbol_rows(
        symbols=symbols,
        symbol_data_coverage=symbol_data_coverage,
    )
    missing_data_plan_required = bool(missing_data_plan)
    suite_decision = _suite_decision(
        coverage_sufficient_for_future_v12=coverage_sufficient_for_future_v12,
        non_funding_symbol_count=len(non_funding_symbols),
        minimum_required_symbols=minimum_required_symbols,
        missing_data_plan_required=missing_data_plan_required,
    )
    raw_data_files_created = _raw_data_paths(changed_files or [])
    filtered_changed_files = _filter_report_changed_files(changed_files or [])
    payload = {
        "schema_version": PUBLIC_DERIVATIVES_MICROSTRUCTURE_SCHEMA_VERSION,
        "loop_id": f"{datetime.now(UTC).strftime('%Y%m%d')}-{PUBLIC_DERIVATIVES_MICROSTRUCTURE_LOOP_SLUG}",
        "loop_namespace": PUBLIC_DERIVATIVES_MICROSTRUCTURE_NAMESPACE,
        "suite_decision": suite_decision,
        "readiness_baseline_verdict": "NOT_READY",
        "source_v10_artifact_path": str(v10_path),
        "source_v10_payload_hash": v10_state["source_v10_payload_hash"],
        "source_v9_suite_decision": v10_state["source_v9_suite_decision"],
        "source_v9_next_queue_empty": bool(v10_state["v9_next_queue_empty"]),
        "v10_kill_state_preserved": bool(v10_state["v10_kill_state_preserved"]),
        "v9_next_queue_empty": bool(v10_state["v9_next_queue_empty"]),
        "negative_control_families": list(v10_state["negative_control_families"]),
        "disallowed_revival_entries": list(v10_state["disallowed_revival_entries"]),
        "source_v9_hash_manifest_digest": v10_state["source_v9_hash_manifest_digest"],
        "source_v8_hash_manifest_digest": v10_state["source_v8_hash_manifest_digest"],
        "requested_symbol_universe": list(symbols),
        "usable_symbol_universe": usable_symbol_universe,
        "excluded_symbols": excluded_symbols,
        "requested_symbol_count": len(symbols),
        "usable_symbol_count": len(usable_symbol_universe),
        "data_families_requested": list(families),
        "data_families_available": data_families_available,
        "target_start": _timestamp(start_ts),
        "target_end": _timestamp(end_ts),
        "target_granularity": "source_stable_public_granularity_or_existing_cache_timestamp",
        "coverage_sufficient_for_future_v12": coverage_sufficient_for_future_v12,
        "minimum_required_symbols": minimum_required_symbols,
        "microstructure_symbol_count": len(non_funding_symbols),
        "missing_data_plan_required": missing_data_plan_required,
        "successful_download_count": 0,
        "failed_download_count": 0,
        "skipped_existing_count": skipped_existing_count,
        "raw_data_files_created": raw_data_files_created,
        "raw_data_files_committed": False,
        "raw_public_data_committed": False,
        "private_data_used": False,
        "account_data_used": False,
        "api_key_used": False,
        "live_or_testnet_path_created": False,
        "alpha_generation_performed": False,
        "strategy_optimization_performed": False,
        "holdout_retune_used": False,
        "tsmom_state_changed": False,
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "claim_boundary": PUBLIC_DERIVATIVES_MICROSTRUCTURE_CLAIM_BOUNDARY,
        "safety_gates": {
            "no_live_orders_submitted": True,
            "no_testnet_orders_submitted": True,
            "no_account_data_used": True,
            "no_api_key_used": True,
            "no_private_order_or_fill_data_used": True,
            "holdout_retune_used": False,
            "alpha_generation_performed": False,
            "strategy_optimization_performed": False,
            "live_or_testnet_path_created": False,
            "tsmom_state_changed": False,
            "readiness_baseline_verdict": "NOT_READY",
        },
        "validation_commands": list(commands or []),
        "changed_files": filtered_changed_files,
    }
    _assert_payload_safe(payload)
    return PublicDerivativesMicrostructureResult(
        payload=payload,
        data_family_coverage=data_family_coverage,
        symbol_data_coverage=symbol_data_coverage,
        missing_data_plan=missing_data_plan,
        download_manifest=download_manifest,
        provenance_manifest=provenance_manifest,
        readiness_by_family=readiness_by_family,
        v10_lineage_lock_reference=_v10_lineage_reference_rows(v10_path, v10_state),
    )


def write_public_derivatives_microstructure_axis_artifacts(
    output_dir: str | Path,
    result: PublicDerivativesMicrostructureResult,
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_public_derivatives_microstructure_axis_output_dir(output_dir, overwrite=overwrite)
    payload = dict(result.payload)
    if out.name.endswith(PUBLIC_DERIVATIVES_MICROSTRUCTURE_LOOP_SLUG):
        payload["loop_id"] = out.name
    _assert_payload_safe(payload)
    _write_json(out / "public_derivatives_microstructure_axis.json", payload)
    _write_json(out / "loop.json", payload)
    _csv_frame(result.data_family_coverage, DATA_FAMILY_COVERAGE_COLUMNS).to_csv(
        out / "data_family_coverage.csv",
        index=False,
    )
    _csv_frame(result.symbol_data_coverage, SYMBOL_DATA_COVERAGE_COLUMNS).to_csv(
        out / "symbol_data_coverage.csv",
        index=False,
    )
    _csv_frame(result.missing_data_plan, MISSING_DATA_PLAN_COLUMNS).to_csv(
        out / "missing_data_plan.csv",
        index=False,
    )
    _csv_frame(result.download_manifest, DOWNLOAD_MANIFEST_COLUMNS).to_csv(
        out / "download_manifest.csv",
        index=False,
    )
    _csv_frame(result.provenance_manifest, PROVENANCE_MANIFEST_COLUMNS).to_csv(
        out / "provenance_manifest.csv",
        index=False,
    )
    _csv_frame(result.readiness_by_family, READINESS_BY_FAMILY_COLUMNS).to_csv(
        out / "readiness_by_family.csv",
        index=False,
    )
    _csv_frame(result.v10_lineage_lock_reference, V10_LINEAGE_LOCK_REFERENCE_COLUMNS).to_csv(
        out / "v10_lineage_lock_reference.csv",
        index=False,
    )
    _atomic_write_text(out / "report.md", render_public_derivatives_microstructure_report(payload, result))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    return out


def prepare_public_derivatives_microstructure_axis_output_dir(
    output_dir: str | Path,
    *,
    overwrite: bool = False,
) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="public-derivatives-microstructure-data-axis-run",
        known_files=PUBLIC_DERIVATIVES_MICROSTRUCTURE_KNOWN_FILES,
        overwrite=overwrite,
    )


def render_public_derivatives_microstructure_report(
    payload: dict[str, Any],
    result: PublicDerivativesMicrostructureResult,
) -> str:
    available = ", ".join(payload["data_families_available"]) or "none"
    usable = ", ".join(payload["usable_symbol_universe"]) or "none"
    negative_controls = ", ".join(payload["negative_control_families"]) or "none"
    missing_count = len(result.missing_data_plan)
    return "\n".join(
        [
            "# Public Derivatives Microstructure Data Axis V11",
            "",
            f"- Suite decision: `{payload['suite_decision']}`",
            f"- Readiness baseline verdict: `{payload['readiness_baseline_verdict']}`",
            f"- Source V10 artifact: `{payload['source_v10_artifact_path']}`",
            f"- V10 kill-state preserved: `{payload['v10_kill_state_preserved']}`",
            f"- V9 next queue empty: `{payload['v9_next_queue_empty']}`",
            f"- Negative-control families: {negative_controls}",
            f"- Requested symbols: {', '.join(payload['requested_symbol_universe'])}",
            f"- Usable future-V12 microstructure symbols: {usable}",
            f"- Data families with existing public cache: {available}",
            f"- Missing data plan rows: `{missing_count}`",
            f"- Coverage sufficient for future V12: `{payload['coverage_sufficient_for_future_v12']}`",
            f"- Raw data files committed: `{payload['raw_data_files_committed']}`",
            f"- Alpha generation performed: `{payload['alpha_generation_performed']}`",
            "",
            "## Claim Boundary",
            "",
            payload["claim_boundary"],
            "",
            "## Safety",
            "",
            "- No live orders submitted.",
            "- No testnet orders submitted.",
            "- No account, API-key, private order, or fill data used.",
            "- Existing V8/V9/V10 artifacts were read only and not rerun.",
            "",
        ]
    )


def evaluate_public_derivatives_microstructure_recent_window_connector(
    *,
    source_v11_artifact_path: str | Path = DEFAULT_PUBLIC_DERIVATIVES_MICROSTRUCTURE_V11_ARTIFACT,
    requested_symbols: str | Iterable[str] | None = None,
    data_families: Iterable[str] | None = None,
    binance_vision_cache_dir: str | Path = "data/binance-vision",
    recent_cache_root: str | Path = "data/binance-derivatives-microstructure-recent-window",
    interval: str = "1h",
    target_start: str = "2024-01-01",
    target_end: str = "2025-12-31",
    recent_start: str | None = None,
    recent_end: str | None = None,
    minimum_required_symbols: int = 3,
    download_recent: bool = False,
    force: bool = False,
    endpoint_fetcher: Callable[[dict[str, Any]], list[dict[str, Any]]] | None = None,
    commands: Iterable[str] | None = None,
    changed_files: Iterable[str] | None = None,
) -> PublicDerivativesMicrostructureRecentWindowResult:
    if minimum_required_symbols <= 0:
        raise ValueError("minimum_required_symbols must be positive")
    symbols = parse_public_derivatives_symbol_universe(requested_symbols)
    families = _normalize_recent_window_data_families(data_families)
    interval = validate_interval(interval)
    target_start_ts = _utc_timestamp(target_start)
    target_end_ts = _utc_timestamp(target_end)
    if target_end_ts < target_start_ts:
        raise ValueError("target_end must be greater than or equal to target_start")
    recent_end_ts = cast(pd.Timestamp, _utc_timestamp(recent_end) if recent_end else _recent_window_default_end())
    recent_start_ts = cast(
        pd.Timestamp,
        _utc_timestamp(recent_start) if recent_start else recent_end_ts - pd.Timedelta(days=30),
    )
    if recent_end_ts < recent_start_ts:
        raise ValueError("recent_end must be greater than or equal to recent_start")

    source_v11_path = Path(source_v11_artifact_path)
    v11_state = _read_v11_microstructure_state(source_v11_path)
    endpoint_feasibility = _recent_endpoint_feasibility_rows(families)
    kline_rows: list[dict[str, Any]] = []
    recent_rows: list[dict[str, Any]] = []
    download_manifest: list[dict[str, Any]] = []
    failed_downloads: list[dict[str, Any]] = []
    provenance_manifest: list[dict[str, Any]] = []
    raw_files_created_by_download: list[str] = []
    skipped_existing_count = 0
    successful_download_count = 0
    failed_download_count = 0

    if "kline_derived_microstructure_proxy" in families:
        months = _month_strings_between(target_start_ts, target_end_ts)
        for symbol in symbols:
            row = _kline_microstructure_proxy_coverage_row(
                symbol=symbol,
                interval=interval,
                months=months,
                cache_dir=Path(binance_vision_cache_dir),
                target_start_ts=target_start_ts,
                target_end_ts=target_end_ts,
            )
            kline_rows.append(row)
            if row["available"]:
                provenance_manifest.append(_recent_window_provenance_row(row))

    for family in families:
        if family == "kline_derived_microstructure_proxy":
            continue
        for symbol in symbols:
            cache_path = _recent_endpoint_cache_path(
                cache_root=Path(recent_cache_root),
                data_family=family,
                symbol=symbol,
                period=str(RECENT_ENDPOINT_METADATA[family]["default_period"]),
                recent_start_ts=recent_start_ts,
                recent_end_ts=recent_end_ts,
            )
            cache_path = _resolve_existing_recent_endpoint_cache_path(
                exact_path=cache_path,
                cache_root=Path(recent_cache_root),
                data_family=family,
                symbol=symbol,
                period=str(RECENT_ENDPOINT_METADATA[family]["default_period"]),
                force=force,
            )
            metadata = RECENT_ENDPOINT_METADATA[family]
            if cache_path.exists() and not force:
                skipped_existing_count += 1
                download_manifest.append(
                    _recent_download_manifest_row(
                        symbol=symbol,
                        data_family=family,
                        status="skipped_existing",
                        cache_path=str(cache_path),
                        source_endpoint_or_cache=str(metadata["source_endpoint_or_cache"]),
                        skipped_existing=True,
                        reason="existing valid public recent-window cache file detected",
                    )
                )
            elif download_recent:
                request = _recent_endpoint_request(
                    symbol=symbol,
                    data_family=family,
                    recent_start_ts=recent_start_ts,
                    recent_end_ts=recent_end_ts,
                )
                try:
                    records = (endpoint_fetcher or _fetch_public_recent_endpoint)(request)
                    _write_recent_endpoint_cache(cache_path, records)
                    raw_files_created_by_download.append(str(cache_path))
                    successful_download_count += 1
                    download_manifest.append(
                        _recent_download_manifest_row(
                            symbol=symbol,
                            data_family=family,
                            status="successful_download",
                            cache_path=str(cache_path),
                            source_endpoint_or_cache=str(metadata["source_endpoint_or_cache"]),
                            successful_download=True,
                            force=force,
                            reason="public no-key recent-window endpoint payload cached",
                        )
                    )
                except (requests.RequestException, OSError, ValueError, TypeError, RuntimeError) as exc:
                    failed_download_count += 1
                    failed_downloads.append(
                        _recent_failed_download_row(
                            symbol=symbol,
                            data_family=family,
                            cache_path=str(cache_path),
                            source_endpoint_or_cache=str(metadata["source_endpoint_or_cache"]),
                            exc=exc,
                        )
                    )
                    download_manifest.append(
                        _recent_download_manifest_row(
                            symbol=symbol,
                            data_family=family,
                            status="failed_download",
                            cache_path=str(cache_path),
                            source_endpoint_or_cache=str(metadata["source_endpoint_or_cache"]),
                            failed_download=True,
                            force=force,
                            reason=f"{type(exc).__name__}: {exc}",
                        )
                    )
            else:
                download_manifest.append(
                    _recent_download_manifest_row(
                        symbol=symbol,
                        data_family=family,
                        status="not_attempted_missing_public_cache",
                        cache_path=str(cache_path),
                        source_endpoint_or_cache=str(metadata["source_endpoint_or_cache"]),
                        reason="recent-window endpoint fetch disabled; V12 does not fabricate missing data",
                    )
                )

            row = _recent_endpoint_coverage_row(
                symbol=symbol,
                data_family=family,
                cache_path=cache_path,
                recent_start_ts=recent_start_ts,
                recent_end_ts=recent_end_ts,
                raw_cache_files_created=str(cache_path) if str(cache_path) in raw_files_created_by_download else "",
            )
            recent_rows.append(row)
            if row["available"]:
                provenance_manifest.append(_recent_window_provenance_row(row))

    all_coverage = kline_rows + recent_rows
    historical_symbols = {
        str(row["symbol"])
        for row in kline_rows
        if row["data_family"] == "kline_derived_microstructure_proxy" and bool(row["available"])
    }
    recent_symbols = {
        str(row["symbol"])
        for row in recent_rows
        if row["data_family"] in PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_ENDPOINT_FAMILIES
        and bool(row["available"])
    }
    historical_microstructure_proxy_readiness = bool(
        len(historical_symbols) >= minimum_required_symbols
        and v11_state["v11_state_preserved"]
        and _has_provenance_for_symbols(provenance_manifest, historical_symbols, "kline_derived_microstructure_proxy")
    )
    recent_window_microstructure_readiness = bool(
        len(recent_symbols) >= minimum_required_symbols
        and v11_state["v11_state_preserved"]
        and _has_any_recent_provenance_for_symbols(provenance_manifest, recent_symbols)
    )
    recent_window_only = bool(recent_window_microstructure_readiness and not historical_microstructure_proxy_readiness)
    temporal_mismatch_warning = (
        "recent-window REST endpoint data is retention-limited and must not be mixed with "
        "2024-2025 labels without a V13 recent-window-only protocol"
        if recent_window_microstructure_readiness
        else ""
    )
    coverage_sufficient_for_future_v13 = bool(
        historical_microstructure_proxy_readiness
        or (recent_window_microstructure_readiness and recent_window_only and temporal_mismatch_warning)
    )
    data_families_available = sorted(
        {
            str(row["data_family"])
            for row in all_coverage
            if bool(row["available"])
        }
    )
    usable_symbol_universe = sorted(historical_symbols | recent_symbols)
    excluded_symbols = _recent_window_excluded_symbol_rows(
        symbols=symbols,
        usable_symbols=set(usable_symbol_universe),
    )
    endpoint_retention_boundaries = {
        family: str(RECENT_ENDPOINT_METADATA[family]["retention_boundary"])
        for family in families
    }
    raw_data_files_created = sorted(set(raw_files_created_by_download + _raw_data_paths(changed_files or [])))
    payload = {
        "schema_version": PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_SCHEMA_VERSION,
        "loop_id": f"{datetime.now(UTC).strftime('%Y%m%d')}-{PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_LOOP_SLUG}",
        "loop_namespace": PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_NAMESPACE,
        "suite_decision": _recent_window_suite_decision(
            historical_microstructure_proxy_readiness=historical_microstructure_proxy_readiness,
            recent_window_microstructure_readiness=recent_window_microstructure_readiness,
            coverage_sufficient_for_future_v13=coverage_sufficient_for_future_v13,
            historical_symbol_count=len(historical_symbols),
            recent_symbol_count=len(recent_symbols),
            minimum_required_symbols=minimum_required_symbols,
            failed_download_count=failed_download_count,
        ),
        "readiness_baseline_verdict": "NOT_READY",
        "source_v11_artifact_path": str(source_v11_path),
        "source_v11_payload_hash": v11_state["source_v11_payload_hash"],
        "source_v11_suite_decision": v11_state["source_v11_suite_decision"],
        "source_v11_coverage_sufficient_for_future_v12": v11_state["source_v11_coverage_sufficient_for_future_v12"],
        "v11_state_preserved": bool(v11_state["v11_state_preserved"]),
        "v10_kill_state_preserved": bool(v11_state["v10_kill_state_preserved"]),
        "v9_next_queue_empty": bool(v11_state["v9_next_queue_empty"]),
        "requested_symbol_universe": list(symbols),
        "usable_symbol_universe": usable_symbol_universe,
        "excluded_symbols": excluded_symbols,
        "data_families_requested": list(families),
        "data_families_available": data_families_available,
        "historical_microstructure_proxy_readiness": historical_microstructure_proxy_readiness,
        "recent_window_microstructure_readiness": recent_window_microstructure_readiness,
        "coverage_sufficient_for_future_v13": coverage_sufficient_for_future_v13,
        "recent_window_only": recent_window_only,
        "temporal_mismatch_warning": temporal_mismatch_warning,
        "endpoint_retention_boundaries": endpoint_retention_boundaries,
        "target_start": _timestamp(target_start_ts),
        "target_end": _timestamp(target_end_ts),
        "recent_start": _timestamp(recent_start_ts),
        "recent_end": _timestamp(recent_end_ts),
        "minimum_required_symbols": minimum_required_symbols,
        "successful_download_count": successful_download_count,
        "failed_download_count": failed_download_count,
        "skipped_existing_count": skipped_existing_count,
        "raw_data_files_created": raw_data_files_created,
        "raw_data_files_committed": False,
        "private_data_used": False,
        "account_data_used": False,
        "api_key_used": False,
        "live_or_testnet_path_created": False,
        "alpha_generation_performed": False,
        "strategy_optimization_performed": False,
        "holdout_retune_used": False,
        "tsmom_state_changed": False,
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "claim_boundary": PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_CLAIM_BOUNDARY,
        "safety_gates": {
            "no_live_orders_submitted": True,
            "no_testnet_orders_submitted": True,
            "no_account_data_used": True,
            "no_api_key_used": True,
            "no_private_order_or_fill_data_used": True,
            "holdout_retune_used": False,
            "alpha_generation_performed": False,
            "strategy_optimization_performed": False,
            "live_or_testnet_path_created": False,
            "tsmom_state_changed": False,
            "readiness_baseline_verdict": "NOT_READY",
        },
        "validation_commands": list(commands or []),
        "changed_files": _filter_report_changed_files(changed_files or []),
    }
    _assert_recent_window_payload_safe(payload)
    readiness_by_family = _recent_window_readiness_by_family_rows(
        families=families,
        coverage_rows=all_coverage,
        historical_microstructure_proxy_readiness=historical_microstructure_proxy_readiness,
        recent_window_microstructure_readiness=recent_window_microstructure_readiness,
        coverage_sufficient_for_future_v13=coverage_sufficient_for_future_v13,
        recent_window_only=recent_window_only,
    )
    return PublicDerivativesMicrostructureRecentWindowResult(
        payload=payload,
        endpoint_feasibility=endpoint_feasibility,
        kline_microstructure_proxy_coverage=kline_rows,
        recent_window_coverage=recent_rows,
        symbol_data_coverage=all_coverage,
        download_manifest=download_manifest,
        failed_downloads=failed_downloads,
        provenance_manifest=provenance_manifest,
        readiness_by_family=readiness_by_family,
        v11_reference=_v11_reference_rows(source_v11_path, v11_state),
    )


def write_public_derivatives_microstructure_recent_window_artifacts(
    output_dir: str | Path,
    result: PublicDerivativesMicrostructureRecentWindowResult,
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_public_derivatives_microstructure_recent_window_output_dir(output_dir, overwrite=overwrite)
    payload = dict(result.payload)
    if out.name.endswith(PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_LOOP_SLUG):
        payload["loop_id"] = out.name
    _assert_recent_window_payload_safe(payload)
    _write_json(out / "public_derivatives_microstructure_recent_window.json", payload)
    _write_json(out / "loop.json", payload)
    _csv_frame(result.endpoint_feasibility, RECENT_WINDOW_ENDPOINT_FEASIBILITY_COLUMNS).to_csv(
        out / "endpoint_feasibility.csv",
        index=False,
    )
    _csv_frame(result.kline_microstructure_proxy_coverage, RECENT_WINDOW_KLINE_PROXY_COVERAGE_COLUMNS).to_csv(
        out / "kline_microstructure_proxy_coverage.csv",
        index=False,
    )
    _csv_frame(result.recent_window_coverage, RECENT_WINDOW_DATA_COVERAGE_COLUMNS).to_csv(
        out / "recent_window_coverage.csv",
        index=False,
    )
    _csv_frame(result.symbol_data_coverage, RECENT_WINDOW_DATA_COVERAGE_COLUMNS).to_csv(
        out / "symbol_data_coverage.csv",
        index=False,
    )
    _csv_frame(result.download_manifest, RECENT_WINDOW_DOWNLOAD_MANIFEST_COLUMNS).to_csv(
        out / "download_manifest.csv",
        index=False,
    )
    _csv_frame(result.failed_downloads, RECENT_WINDOW_FAILED_DOWNLOADS_COLUMNS).to_csv(
        out / "failed_downloads.csv",
        index=False,
    )
    _csv_frame(result.provenance_manifest, RECENT_WINDOW_PROVENANCE_MANIFEST_COLUMNS).to_csv(
        out / "provenance_manifest.csv",
        index=False,
    )
    _csv_frame(result.readiness_by_family, RECENT_WINDOW_READINESS_BY_FAMILY_COLUMNS).to_csv(
        out / "readiness_by_family.csv",
        index=False,
    )
    _csv_frame(result.v11_reference, V11_REFERENCE_COLUMNS).to_csv(out / "v11_reference.csv", index=False)
    _atomic_write_text(out / "report.md", render_public_derivatives_microstructure_recent_window_report(payload, result))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    return out


def prepare_public_derivatives_microstructure_recent_window_output_dir(
    output_dir: str | Path,
    *,
    overwrite: bool = False,
) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="public-derivatives-microstructure-recent-window-run",
        known_files=PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_KNOWN_FILES,
        overwrite=overwrite,
    )


def render_public_derivatives_microstructure_recent_window_report(
    payload: dict[str, Any],
    result: PublicDerivativesMicrostructureRecentWindowResult,
) -> str:
    available = ", ".join(payload["data_families_available"]) or "none"
    usable = ", ".join(payload["usable_symbol_universe"]) or "none"
    return "\n".join(
        [
            "# Public Derivatives Microstructure Recent Window Connector V12",
            "",
            f"- Suite decision: `{payload['suite_decision']}`",
            f"- Readiness baseline verdict: `{payload['readiness_baseline_verdict']}`",
            f"- Source V11 artifact: `{payload['source_v11_artifact_path']}`",
            f"- V11 state preserved: `{payload['v11_state_preserved']}`",
            f"- Requested symbols: {', '.join(payload['requested_symbol_universe'])}",
            f"- Usable future-V13 symbols: {usable}",
            f"- Data families with public coverage: {available}",
            f"- Historical kline proxy gate: `{payload['historical_microstructure_proxy_readiness']}`",
            f"- Recent-window endpoint gate: `{payload['recent_window_microstructure_readiness']}`",
            f"- Coverage sufficient for future V13: `{payload['coverage_sufficient_for_future_v13']}`",
            f"- Recent-window only: `{payload['recent_window_only']}`",
            f"- Temporal mismatch warning: {payload['temporal_mismatch_warning'] or 'none'}",
            f"- Endpoint failed downloads: `{payload['failed_download_count']}`",
            f"- Raw data files committed: `{payload['raw_data_files_committed']}`",
            f"- Alpha generation performed: `{payload['alpha_generation_performed']}`",
            "",
            "## Endpoint Retention Boundaries",
            "",
            *[
                f"- `{family}`: {boundary}"
                for family, boundary in payload["endpoint_retention_boundaries"].items()
            ],
            "",
            "## Claim Boundary",
            "",
            payload["claim_boundary"],
            "",
            "## Safety",
            "",
            "- No live orders submitted.",
            "- No testnet orders submitted.",
            "- No account, API-key, private order, or fill data used.",
            "- Existing V8/V9/V10/V11 artifacts were read only and not rerun.",
            f"- Failed download rows: `{len(result.failed_downloads)}`.",
            "",
        ]
    )


def _normalize_data_families(data_families: Iterable[str] | None) -> tuple[str, ...]:
    if data_families is None:
        return PUBLIC_DERIVATIVES_MICROSTRUCTURE_TARGET_FAMILIES
    seen: set[str] = set()
    normalized: list[str] = []
    for item in data_families:
        family = str(item).strip()
        if not family:
            continue
        if family not in PUBLIC_DERIVATIVES_MICROSTRUCTURE_TARGET_FAMILIES:
            raise ValueError(f"unsupported public derivatives data family: {family}")
        if family in seen:
            continue
        seen.add(family)
        normalized.append(family)
    if not normalized:
        raise ValueError("at least one public derivatives data family is required")
    return tuple(normalized)


def _normalize_recent_window_data_families(data_families: Iterable[str] | None) -> tuple[str, ...]:
    if data_families is None:
        return PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_TARGET_FAMILIES
    seen: set[str] = set()
    normalized: list[str] = []
    for item in data_families:
        family = str(item).strip()
        if not family:
            continue
        if family not in PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_TARGET_FAMILIES:
            raise ValueError(f"unsupported V12 public derivatives data family: {family}")
        if family in seen:
            continue
        seen.add(family)
        normalized.append(family)
    if not normalized:
        raise ValueError("at least one V12 public derivatives data family is required")
    return tuple(normalized)


def _read_v11_microstructure_state(path: Path) -> dict[str, Any]:
    payload_path = path / "public_derivatives_microstructure_axis.json"
    if not payload_path.exists():
        raise FileNotFoundError(f"missing V11 microstructure payload: {payload_path}")
    payload = json.loads(payload_path.read_text())
    if "public_derivatives_microstructure_data_axis_v11" not in str(payload.get("loop_namespace", "")):
        raise ValueError("source V11 artifact does not look like V11 public derivatives microstructure axis")
    source_v11_suite_decision = str(payload.get("suite_decision", ""))
    source_v11_coverage = bool(payload.get("coverage_sufficient_for_future_v12", True))
    v11_state_preserved = bool(
        source_v11_suite_decision == "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS"
        and source_v11_coverage is False
        and payload.get("readiness_baseline_verdict") == "NOT_READY"
        and payload.get("v10_kill_state_preserved") is True
        and payload.get("v9_next_queue_empty") is True
        and payload.get("alpha_generation_performed") is False
        and payload.get("private_data_used") is False
        and payload.get("api_key_used") is False
        and payload.get("live_or_testnet_path_created") is False
    )
    return {
        "payload": payload,
        "source_v11_payload_hash": sha256_file(payload_path),
        "source_v11_suite_decision": source_v11_suite_decision,
        "source_v11_coverage_sufficient_for_future_v12": source_v11_coverage,
        "v11_state_preserved": v11_state_preserved,
        "v10_kill_state_preserved": bool(payload.get("v10_kill_state_preserved", False)),
        "v9_next_queue_empty": bool(payload.get("v9_next_queue_empty", False)),
    }


def _recent_endpoint_feasibility_rows(families: tuple[str, ...]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for family in families:
        metadata = RECENT_ENDPOINT_METADATA[family]
        included = bool(metadata["historical_backfill_supported"]) or bool(metadata["recent_window_supported"])
        rows.append(
            {
                "data_family": family,
                "source_endpoint_or_cache": str(metadata["source_endpoint_or_cache"]),
                "docs_url": str(metadata["docs_url"]),
                "public_source_only": True,
                "requires_api_key": False,
                "retention_boundary": str(metadata["retention_boundary"]),
                "historical_backfill_supported": bool(metadata["historical_backfill_supported"]),
                "recent_window_supported": bool(metadata["recent_window_supported"]),
                "default_period": str(metadata["default_period"]),
                "included_for_future_v13": included,
                "exclusion_reason": "" if included else "not included for V13",
            }
        )
    return rows


def _month_strings_between(start_ts: pd.Timestamp, end_ts: pd.Timestamp) -> tuple[str, ...]:
    start_month = start_ts.tz_convert("UTC").tz_localize(None) if start_ts.tzinfo else start_ts
    end_month = end_ts.tz_convert("UTC").tz_localize(None) if end_ts.tzinfo else end_ts
    periods = pd.period_range(start=start_month.to_period("M"), end=end_month.to_period("M"), freq="M")
    return tuple(str(period) for period in periods)


def _kline_microstructure_proxy_coverage_row(
    *,
    symbol: str,
    interval: str,
    months: tuple[str, ...],
    cache_dir: Path,
    target_start_ts: pd.Timestamp,
    target_end_ts: pd.Timestamp,
) -> dict[str, Any]:
    paths = [canonical_cache_path(cache_dir, symbol, interval, month) for month in months]
    missing_paths = [path for path in paths if not path.exists()]
    metadata = RECENT_ENDPOINT_METADATA["kline_derived_microstructure_proxy"]
    if missing_paths:
        reason = (
            "unavailable_with_reason: missing existing public Binance Vision raw kline cache files; "
            f"missing_count={len(missing_paths)}"
        )
        return _recent_window_coverage_row(
            symbol=symbol,
            data_family="kline_derived_microstructure_proxy",
            source_endpoint_or_cache=str(paths[0] if paths else cache_dir),
            available=False,
            retention_boundary=str(metadata["retention_boundary"]),
            historical_backfill_supported=True,
            recent_window_supported=False,
            missing_ranges=";".join(path.name for path in missing_paths[:5]),
            unavailable_reason=reason,
            exclusion_reason=reason,
        )

    frames: list[pd.DataFrame] = []
    for path in paths:
        parsed = _read_raw_kline_zip_frame(path)
        if not parsed["parse_ok"]:
            reason = f"unavailable_with_reason: raw kline cache parse failed: {parsed['error']}"
            return _recent_window_coverage_row(
                symbol=symbol,
                data_family="kline_derived_microstructure_proxy",
                source_endpoint_or_cache=str(path),
                available=False,
                retention_boundary=str(metadata["retention_boundary"]),
                historical_backfill_supported=True,
                recent_window_supported=False,
                schema_columns=str(parsed["schema_columns"]),
                unavailable_reason=reason,
                exclusion_reason=reason,
            )
        frame = cast(pd.DataFrame, parsed["frame"])
        resolved = _resolve_kline_proxy_columns(frame)
        if resolved["missing"]:
            reason = (
                "unavailable_with_reason: missing required raw kline microstructure columns: "
                + ",".join(cast(list[str], resolved["missing"]))
            )
            return _recent_window_coverage_row(
                symbol=symbol,
                data_family="kline_derived_microstructure_proxy",
                source_endpoint_or_cache=str(path),
                available=False,
                retention_boundary=str(metadata["retention_boundary"]),
                historical_backfill_supported=True,
                recent_window_supported=False,
                schema_columns=str(parsed["schema_columns"]),
                unavailable_reason=reason,
                exclusion_reason=reason,
            )
        frames.append(frame)

    combined = pd.concat(frames, ignore_index=True)
    timestamps = _timestamp_series_recent(combined, preferred=("open_time",))
    first_ts = None
    last_ts = None
    if timestamps is not None and not timestamps.dropna().empty:
        ordered = cast(pd.Series, timestamps.dropna()).sort_values()
        first_ts = cast(pd.Timestamp, ordered.iloc[0])
        last_ts = cast(pd.Timestamp, ordered.iloc[-1])
    if first_ts is None or last_ts is None:
        reason = "unavailable_with_reason: raw kline cache lacks parseable open_time timestamp"
        return _recent_window_coverage_row(
            symbol=symbol,
            data_family="kline_derived_microstructure_proxy",
            source_endpoint_or_cache=str(paths[0]),
            available=False,
            retention_boundary=str(metadata["retention_boundary"]),
            historical_backfill_supported=True,
            recent_window_supported=False,
            schema_columns=",".join(str(column) for column in combined.columns),
            unavailable_reason=reason,
            exclusion_reason=reason,
        )
    missing_ranges = _missing_range_note(first_ts, last_ts, target_start_ts, target_end_ts)
    if missing_ranges:
        reason = f"unavailable_with_reason: existing raw kline proxy cache does not cover target range; {missing_ranges}"
        return _recent_window_coverage_row(
            symbol=symbol,
            data_family="kline_derived_microstructure_proxy",
            source_endpoint_or_cache=str(paths[0]),
            available=False,
            retention_boundary=str(metadata["retention_boundary"]),
            historical_backfill_supported=True,
            recent_window_supported=False,
            first_timestamp=_timestamp(first_ts),
            last_timestamp=_timestamp(last_ts),
            observation_count=int(len(combined)),
            missing_ranges=missing_ranges,
            schema_columns=",".join(str(column) for column in combined.columns),
            provenance_hash_if_available=_combined_file_hash(paths),
            unavailable_reason=reason,
            exclusion_reason=reason,
        )
    schema_columns = ",".join(
        [str(column) for column in combined.columns] + list(KLINE_PROXY_DERIVED_COLUMNS)
    )
    return _recent_window_coverage_row(
        symbol=symbol,
        data_family="kline_derived_microstructure_proxy",
        source_endpoint_or_cache=str(paths[0]),
        available=True,
        retention_boundary=str(metadata["retention_boundary"]),
        historical_backfill_supported=True,
        recent_window_supported=False,
        first_timestamp=_timestamp(first_ts),
        last_timestamp=_timestamp(last_ts),
        observation_count=int(len(combined)),
        expected_observation_count_if_known=str(len(combined)),
        coverage_ratio_if_known="1.0",
        schema_columns=schema_columns,
        provenance_hash_if_available=_combined_file_hash(paths),
        included_for_future_v13=True,
    )


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


def _resolve_kline_proxy_columns(frame: pd.DataFrame) -> dict[str, Any]:
    resolved: dict[str, str] = {}
    missing: list[str] = []
    for canonical, aliases in KLINE_PROXY_REQUIRED_ALIASES.items():
        match = next((alias for alias in aliases if alias in frame.columns), None)
        if match is None:
            missing.append(canonical)
        else:
            resolved[canonical] = match
    return {"resolved": resolved, "missing": missing}


def _recent_endpoint_cache_path(
    *,
    cache_root: Path,
    data_family: str,
    symbol: str,
    period: str,
    recent_start_ts: pd.Timestamp,
    recent_end_ts: pd.Timestamp,
) -> Path:
    start_ms = _timestamp_ms(recent_start_ts)
    end_ms = _timestamp_ms(recent_end_ts)
    safe_period = period.replace("/", "_")
    return cache_root / data_family / symbol / safe_period / f"{symbol}-{data_family}-{safe_period}-{start_ms}-{end_ms}.json"


def _resolve_existing_recent_endpoint_cache_path(
    *,
    exact_path: Path,
    cache_root: Path,
    data_family: str,
    symbol: str,
    period: str,
    force: bool,
) -> Path:
    if force or exact_path.exists():
        return exact_path
    safe_period = period.replace("/", "_")
    root = cache_root / data_family / symbol / safe_period
    candidates = sorted(root.glob(f"{symbol}-{data_family}-{safe_period}-*.json"))
    return candidates[-1] if candidates else exact_path


def _recent_endpoint_request(
    *,
    symbol: str,
    data_family: str,
    recent_start_ts: pd.Timestamp,
    recent_end_ts: pd.Timestamp,
) -> dict[str, Any]:
    metadata = RECENT_ENDPOINT_METADATA[data_family]
    endpoint = str(metadata["source_endpoint_or_cache"])
    period = str(metadata["default_period"])
    start_ms = _timestamp_ms(recent_start_ts)
    end_ms = _timestamp_ms(recent_end_ts)
    params: dict[str, Any]
    if data_family == "basis_recent":
        params = {
            "pair": symbol,
            "contractType": "PERPETUAL",
            "period": period,
            "startTime": start_ms,
            "endTime": end_ms,
            "limit": 500,
        }
    elif data_family == "mark_price_premium_index_recent":
        params = {"symbol": symbol}
    else:
        params = {
            "symbol": symbol,
            "period": period,
            "startTime": start_ms,
            "endTime": end_ms,
            "limit": 500,
        }
    return {
        "symbol": symbol,
        "data_family": data_family,
        "endpoint": endpoint,
        "params": params,
        "start_time_ms": start_ms,
        "end_time_ms": end_ms,
        "public_source_only": True,
        "requires_api_key": False,
    }


def _fetch_public_recent_endpoint(request: dict[str, Any]) -> list[dict[str, Any]]:
    response = requests.get(
        str(request["endpoint"]),
        params=cast(dict[str, Any], request["params"]),
        timeout=30,
    )
    response.raise_for_status()
    payload = response.json()
    if isinstance(payload, list):
        records = payload
    elif isinstance(payload, dict):
        records = [payload]
    else:
        raise ValueError("public endpoint returned non-tabular JSON payload")
    normalized = [record for record in records if isinstance(record, dict)]
    if not normalized:
        raise ValueError("public endpoint returned zero object records")
    return cast(list[dict[str, Any]], normalized)


def _write_recent_endpoint_cache(path: Path, records: list[dict[str, Any]]) -> None:
    if not records:
        raise ValueError("refusing to cache empty recent-window endpoint payload")
    frame = pd.DataFrame(records)
    timestamps = _timestamp_series_recent(frame)
    if timestamps is None or timestamps.dropna().empty:
        raise ValueError("refusing to cache recent-window endpoint payload without parseable timestamp")
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(records, sort_keys=True) + "\n")
    tmp.replace(path)


def _recent_endpoint_coverage_row(
    *,
    symbol: str,
    data_family: str,
    cache_path: Path,
    recent_start_ts: pd.Timestamp,
    recent_end_ts: pd.Timestamp,
    raw_cache_files_created: str = "",
) -> dict[str, Any]:
    metadata = RECENT_ENDPOINT_METADATA[data_family]
    if not cache_path.exists():
        reason = "unavailable_with_reason: missing public recent-window cache file; V12 did not fabricate data"
        return _recent_window_coverage_row(
            symbol=symbol,
            data_family=data_family,
            source_endpoint_or_cache=str(metadata["source_endpoint_or_cache"]),
            available=False,
            retention_boundary=str(metadata["retention_boundary"]),
            historical_backfill_supported=False,
            recent_window_supported=True,
            unavailable_reason=reason,
            exclusion_reason=reason,
        )
    parsed = _inspect_recent_endpoint_cache_file(cache_path)
    if not parsed["parse_ok"]:
        reason = f"unavailable_with_reason: recent-window cache parse failed: {parsed['error']}"
        return _recent_window_coverage_row(
            symbol=symbol,
            data_family=data_family,
            source_endpoint_or_cache=str(cache_path),
            available=False,
            retention_boundary=str(metadata["retention_boundary"]),
            historical_backfill_supported=False,
            recent_window_supported=True,
            schema_columns=str(parsed["schema_columns"]),
            raw_cache_files_created=raw_cache_files_created,
            unavailable_reason=reason,
            exclusion_reason=reason,
        )
    first_ts = cast(pd.Timestamp | None, parsed["first_timestamp"])
    last_ts = cast(pd.Timestamp | None, parsed["last_timestamp"])
    observation_count = int(parsed["observation_count"])
    if observation_count <= 0:
        reason = "unavailable_with_reason: recent-window cache has zero observations"
        return _recent_window_coverage_row(
            symbol=symbol,
            data_family=data_family,
            source_endpoint_or_cache=str(cache_path),
            available=False,
            retention_boundary=str(metadata["retention_boundary"]),
            historical_backfill_supported=False,
            recent_window_supported=True,
            schema_columns=str(parsed["schema_columns"]),
            raw_cache_files_created=raw_cache_files_created,
            unavailable_reason=reason,
            exclusion_reason=reason,
        )
    if first_ts is None or last_ts is None:
        reason = "unavailable_with_reason: recent-window cache lacks parseable timestamp"
        return _recent_window_coverage_row(
            symbol=symbol,
            data_family=data_family,
            source_endpoint_or_cache=str(cache_path),
            available=False,
            retention_boundary=str(metadata["retention_boundary"]),
            historical_backfill_supported=False,
            recent_window_supported=True,
            observation_count=observation_count,
            schema_columns=str(parsed["schema_columns"]),
            raw_cache_files_created=raw_cache_files_created,
            unavailable_reason=reason,
            exclusion_reason=reason,
        )
    missing_ranges = _missing_range_note(first_ts, last_ts, recent_start_ts, recent_end_ts)
    if missing_ranges:
        reason = (
            "unavailable_with_reason: existing public recent-window cache does not cover requested recent window; "
            f"{missing_ranges}"
        )
        return _recent_window_coverage_row(
            symbol=symbol,
            data_family=data_family,
            source_endpoint_or_cache=str(cache_path),
            available=False,
            retention_boundary=str(metadata["retention_boundary"]),
            historical_backfill_supported=False,
            recent_window_supported=True,
            first_timestamp=_timestamp(first_ts),
            last_timestamp=_timestamp(last_ts),
            observation_count=observation_count,
            missing_ranges=missing_ranges,
            schema_columns=str(parsed["schema_columns"]),
            provenance_hash_if_available=sha256_file(cache_path),
            raw_cache_files_created=raw_cache_files_created,
            unavailable_reason=reason,
            exclusion_reason=reason,
        )
    return _recent_window_coverage_row(
        symbol=symbol,
        data_family=data_family,
        source_endpoint_or_cache=str(cache_path),
        available=True,
        retention_boundary=str(metadata["retention_boundary"]),
        historical_backfill_supported=False,
        recent_window_supported=True,
        first_timestamp=_timestamp(first_ts),
        last_timestamp=_timestamp(last_ts),
        observation_count=observation_count,
        expected_observation_count_if_known="",
        coverage_ratio_if_known="",
        missing_ranges=missing_ranges,
        schema_columns=str(parsed["schema_columns"]),
        provenance_hash_if_available=sha256_file(cache_path),
        raw_cache_files_created=raw_cache_files_created,
        included_for_future_v13=True,
        exclusion_reason=(
            "retention-limited recent-window data; only usable under a future V13 recent-window-only protocol"
        ),
    )


def _inspect_recent_endpoint_cache_file(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text())
        records = payload if isinstance(payload, list) else [payload]
        frame = pd.DataFrame(records)
    except (OSError, json.JSONDecodeError, ValueError) as exc:
        return {
            "parse_ok": False,
            "error": f"{type(exc).__name__}: {exc}",
            "schema_columns": "",
            "observation_count": 0,
            "first_timestamp": None,
            "last_timestamp": None,
        }
    timestamps = _timestamp_series_recent(frame)
    first_ts: pd.Timestamp | None = None
    last_ts: pd.Timestamp | None = None
    if timestamps is not None and not timestamps.dropna().empty:
        ordered = cast(pd.Series, timestamps.dropna()).sort_values()
        first_ts = cast(pd.Timestamp, ordered.iloc[0])
        last_ts = cast(pd.Timestamp, ordered.iloc[-1])
    return {
        "parse_ok": True,
        "error": "",
        "schema_columns": ",".join(str(column) for column in frame.columns),
        "observation_count": int(len(frame)),
        "first_timestamp": first_ts,
        "last_timestamp": last_ts,
    }


def _timestamp_series_recent(
    frame: pd.DataFrame,
    *,
    preferred: tuple[str, ...] = (),
) -> pd.Series | None:
    for column in preferred + (
        "timestamp",
        "time",
        "open_time",
        "close_time",
        "fundingTime",
        "nextFundingTime",
        "event_time",
    ):
        if column not in frame.columns:
            continue
        series = frame[column]
        numeric = cast(pd.Series, pd.to_numeric(series, errors="coerce"))
        non_null = cast(pd.Series, numeric.dropna())
        if not non_null.empty:
            unit = "ms" if float(non_null.abs().max()) > 10_000_000_000 else "s"
            return cast(pd.Series, pd.Series(pd.to_datetime(cast(Any, numeric), unit=unit, utc=True)))
        return cast(pd.Series, pd.Series(pd.to_datetime(cast(Any, series), utc=True, errors="coerce")))
    return None


def _recent_window_coverage_row(
    *,
    symbol: str,
    data_family: str,
    source_endpoint_or_cache: str,
    available: bool,
    retention_boundary: str,
    historical_backfill_supported: bool,
    recent_window_supported: bool,
    first_timestamp: str = "",
    last_timestamp: str = "",
    observation_count: int = 0,
    expected_observation_count_if_known: str = "",
    coverage_ratio_if_known: str = "",
    missing_ranges: str = "",
    unavailable_reason: str = "",
    schema_columns: str = "",
    provenance_hash_if_available: str = "",
    raw_cache_files_created: str = "",
    included_for_future_v13: bool = False,
    exclusion_reason: str = "",
) -> dict[str, Any]:
    return {
        "symbol": symbol,
        "data_family": data_family,
        "source_endpoint_or_cache": source_endpoint_or_cache,
        "public_source_only": True,
        "requires_api_key": False,
        "available": available,
        "retention_boundary": retention_boundary,
        "historical_backfill_supported": historical_backfill_supported,
        "recent_window_supported": recent_window_supported,
        "first_timestamp": first_timestamp,
        "last_timestamp": last_timestamp,
        "observation_count": observation_count,
        "expected_observation_count_if_known": expected_observation_count_if_known,
        "coverage_ratio_if_known": coverage_ratio_if_known,
        "missing_ranges": missing_ranges,
        "unavailable_reason": unavailable_reason,
        "schema_columns": schema_columns,
        "provenance_hash_if_available": provenance_hash_if_available,
        "raw_cache_files_created": raw_cache_files_created,
        "raw_cache_files_committed": False,
        "included_for_future_v13": included_for_future_v13,
        "exclusion_reason": exclusion_reason,
    }


def _recent_download_manifest_row(
    *,
    symbol: str,
    data_family: str,
    status: str,
    cache_path: str,
    source_endpoint_or_cache: str,
    skipped_existing: bool = False,
    successful_download: bool = False,
    failed_download: bool = False,
    force: bool = False,
    reason: str = "",
) -> dict[str, Any]:
    return {
        "symbol": symbol,
        "data_family": data_family,
        "status": status,
        "cache_path": cache_path,
        "source_endpoint_or_cache": source_endpoint_or_cache,
        "skipped_existing": skipped_existing,
        "successful_download": successful_download,
        "failed_download": failed_download,
        "force": force,
        "reason": reason,
    }


def _recent_failed_download_row(
    *,
    symbol: str,
    data_family: str,
    cache_path: str,
    source_endpoint_or_cache: str,
    exc: BaseException,
) -> dict[str, Any]:
    return {
        "symbol": symbol,
        "data_family": data_family,
        "cache_path": cache_path,
        "source_endpoint_or_cache": source_endpoint_or_cache,
        "error_type": type(exc).__name__,
        "reason": str(exc),
        "public_only": True,
        "fabricated_data": False,
    }


def _recent_window_provenance_row(row: dict[str, Any]) -> dict[str, Any]:
    path = Path(str(row["source_endpoint_or_cache"]))
    return {
        "symbol": row["symbol"],
        "data_family": row["data_family"],
        "source_endpoint_or_cache": row["source_endpoint_or_cache"],
        "provenance_hash": row["provenance_hash_if_available"],
        "bytes": path.stat().st_size if path.exists() else 0,
        "first_timestamp": row["first_timestamp"],
        "last_timestamp": row["last_timestamp"],
        "observation_count": row["observation_count"],
        "schema_columns": row["schema_columns"],
        "retention_boundary": row["retention_boundary"],
    }


def _recent_window_excluded_symbol_rows(
    *,
    symbols: tuple[str, ...],
    usable_symbols: set[str],
) -> list[dict[str, str]]:
    excluded = [
        {
            "symbol": symbol,
            "reason": "no V12 public microstructure proxy or recent-window endpoint coverage available",
        }
        for symbol in symbols
        if symbol not in usable_symbols
    ]
    present = {row["symbol"] for row in excluded}
    for row in PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_EXCLUDED_SYMBOLS:
        if row["symbol"] not in symbols and row["symbol"] not in present:
            excluded.append(dict(row))
    return excluded


def _recent_window_readiness_by_family_rows(
    *,
    families: tuple[str, ...],
    coverage_rows: list[dict[str, Any]],
    historical_microstructure_proxy_readiness: bool,
    recent_window_microstructure_readiness: bool,
    coverage_sufficient_for_future_v13: bool,
    recent_window_only: bool,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for family in families:
        family_rows = [row for row in coverage_rows if row["data_family"] == family]
        available_rows = [row for row in family_rows if bool(row["available"])]
        included = [row for row in available_rows if bool(row["included_for_future_v13"])]
        first_values = [str(row["first_timestamp"]) for row in available_rows if row["first_timestamp"]]
        last_values = [str(row["last_timestamp"]) for row in available_rows if row["last_timestamp"]]
        included_symbol_count = len({row["symbol"] for row in included})
        family_historical_gate = bool(
            family == "kline_derived_microstructure_proxy"
            and historical_microstructure_proxy_readiness
            and included_symbol_count >= 3
        )
        family_recent_gate = bool(
            family in PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_ENDPOINT_FAMILIES
            and recent_window_microstructure_readiness
            and included_symbol_count >= 3
        )
        rows.append(
            {
                "data_family": family,
                "available_symbol_count": len({row["symbol"] for row in available_rows}),
                "included_symbol_count_for_future_v13": included_symbol_count,
                "common_start_by_family": max(first_values) if first_values else "",
                "common_end_by_family": min(last_values) if last_values else "",
                "historical_microstructure_proxy_readiness": family_historical_gate,
                "recent_window_microstructure_readiness": family_recent_gate,
                "coverage_sufficient_for_future_v13": bool(
                    coverage_sufficient_for_future_v13 and (family_historical_gate or family_recent_gate)
                ),
                "recent_window_only": bool(recent_window_only and family_recent_gate),
                "claim_boundary": PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_CLAIM_BOUNDARY,
            }
        )
    return rows


def _v11_reference_rows(path: Path, v11_state: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        {
            "reference_type": "v11_payload",
            "source_v11_artifact_path": str(path),
            "source_file": str(path / "public_derivatives_microstructure_axis.json"),
            "source_hash": v11_state["source_v11_payload_hash"],
            "source_v11_suite_decision": v11_state["source_v11_suite_decision"],
            "source_v11_coverage_sufficient_for_future_v12": v11_state[
                "source_v11_coverage_sufficient_for_future_v12"
            ],
            "v10_kill_state_preserved": v11_state["v10_kill_state_preserved"],
            "v9_next_queue_empty": v11_state["v9_next_queue_empty"],
            "preserved": v11_state["v11_state_preserved"],
        }
    ]


def _recent_window_suite_decision(
    *,
    historical_microstructure_proxy_readiness: bool,
    recent_window_microstructure_readiness: bool,
    coverage_sufficient_for_future_v13: bool,
    historical_symbol_count: int,
    recent_symbol_count: int,
    minimum_required_symbols: int,
    failed_download_count: int,
) -> str:
    if coverage_sufficient_for_future_v13 and historical_microstructure_proxy_readiness:
        return "MICROSTRUCTURE_PROXY_READY_FOR_V13"
    if coverage_sufficient_for_future_v13 and recent_window_microstructure_readiness:
        return "MICROSTRUCTURE_RECENT_WINDOW_READY_FOR_V13"
    if failed_download_count > 0:
        return "MICROSTRUCTURE_AXIS_FAILED_SAFE"
    if max(historical_symbol_count, recent_symbol_count) < minimum_required_symbols:
        return "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS"
    if recent_symbol_count >= minimum_required_symbols and not historical_microstructure_proxy_readiness:
        return "MICROSTRUCTURE_AXIS_NOT_READY_RETENTION_LIMITED"
    return "MICROSTRUCTURE_AXIS_NOT_READY_MISSING_PUBLIC_DATA"


def _has_provenance_for_symbols(
    provenance_rows: list[dict[str, Any]],
    symbols: set[str],
    data_family: str,
) -> bool:
    if not symbols:
        return False
    proven = {
        str(row["symbol"])
        for row in provenance_rows
        if row["data_family"] == data_family and str(row.get("provenance_hash", ""))
    }
    return symbols.issubset(proven)


def _has_any_recent_provenance_for_symbols(
    provenance_rows: list[dict[str, Any]],
    symbols: set[str],
) -> bool:
    if not symbols:
        return False
    proven = {
        str(row["symbol"])
        for row in provenance_rows
        if row["data_family"] in PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_ENDPOINT_FAMILIES
        and str(row.get("provenance_hash", ""))
    }
    return symbols.issubset(proven)


def _combined_file_hash(paths: list[Path]) -> str:
    import hashlib

    h = hashlib.sha256()
    for path in paths:
        h.update(path.name.encode("utf-8"))
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                h.update(chunk)
    return h.hexdigest()


def _timestamp_ms(value: pd.Timestamp) -> int:
    timestamp = value.tz_convert("UTC") if value.tzinfo is not None else value.tz_localize("UTC")
    return int(timestamp.timestamp() * 1000)


def _recent_window_default_end() -> pd.Timestamp:
    now = pd.Timestamp(datetime.now(UTC))
    return cast(pd.Timestamp, now.floor("h"))


def _read_v10_lineage_lock_state(path: Path) -> dict[str, Any]:
    payload_path = path / "v9_falsification_lineage_governance.json"
    negative_path = path / "negative_control_registry.csv"
    disallowed_path = path / "disallowed_revival_registry.csv"
    if not payload_path.exists():
        raise FileNotFoundError(f"missing V10 governance payload: {payload_path}")
    payload = json.loads(payload_path.read_text())
    if "v9_falsification_lineage_negative_control_governance_v10" not in str(payload.get("loop_namespace", "")):
        raise ValueError("source V10 artifact does not look like V10 lineage governance")
    negative = _read_csv_records(negative_path)
    disallowed = _read_csv_records(disallowed_path)
    negative_families = sorted(
        str(row.get("signal_family", ""))
        for row in negative
        if str(row.get("signal_family", "")).strip()
    )
    disallowed_families = sorted(
        str(row.get("signal_family", ""))
        for row in disallowed
        if str(row.get("signal_family", "")).strip()
    )
    source_v9_suite_decision = str(payload.get("source_v9_suite_decision", ""))
    v9_next_queue_empty = bool(payload.get("source_v9_next_queue_empty", False))
    v10_kill_state_preserved = bool(
        payload.get("suite_decision") == "V9_KILL_STATE_GOVERNANCE_LOCKED"
        and source_v9_suite_decision == "NO_FAMILY_SURVIVES_STRICT_FALSIFICATION"
        and v9_next_queue_empty
        and negative_families
        and disallowed_families
        and payload.get("readiness_baseline_verdict") == "NOT_READY"
        and payload.get("alpha_generation_performed") is False
        and payload.get("tsmom_state_changed") is False
    )
    return {
        "payload": payload,
        "source_v10_payload_hash": sha256_file(payload_path),
        "source_v9_suite_decision": source_v9_suite_decision,
        "v9_next_queue_empty": v9_next_queue_empty,
        "negative_control_families": negative_families,
        "disallowed_revival_entries": disallowed_families,
        "source_v9_hash_manifest_digest": str(payload.get("source_v9_hash_manifest_digest", "")),
        "source_v8_hash_manifest_digest": str(payload.get("source_v8_hash_manifest_digest", "")),
        "negative_control_registry_hash": sha256_file(negative_path) if negative_path.exists() else "",
        "disallowed_revival_registry_hash": sha256_file(disallowed_path) if disallowed_path.exists() else "",
        "negative_control_rows": negative,
        "disallowed_revival_rows": disallowed,
        "v10_kill_state_preserved": v10_kill_state_preserved,
    }


def _coverage_row_for_family(
    *,
    symbol: str,
    data_family: str,
    cache_root: Path,
    funding_cache_dir: Path,
    start_ts: pd.Timestamp,
    end_ts: pd.Timestamp,
) -> dict[str, Any]:
    cache_path = _resolve_cache_path(
        symbol=symbol,
        data_family=data_family,
        cache_root=cache_root,
        funding_cache_dir=funding_cache_dir,
    )
    metadata = FAMILY_SOURCE_METADATA[data_family]
    if cache_path is None:
        planned = _planned_cache_path(symbol=symbol, data_family=data_family, cache_root=cache_root)
        reason = "unavailable_with_reason: missing public cache file; V11 does not fabricate or download data by default"
        return _coverage_row(
            symbol=symbol,
            data_family=data_family,
            available=False,
            source_type=metadata["source_type"],
            source_location_or_cache_path=str(planned),
            unavailable_reason=reason,
            inclusion_reason=reason,
        )

    parsed = _inspect_cache_file(cache_path)
    if not parsed["parse_ok"]:
        reason = f"unavailable_with_reason: existing public cache file could not be parsed: {parsed['error']}"
        return _coverage_row(
            symbol=symbol,
            data_family=data_family,
            available=False,
            source_type=metadata["source_type"],
            source_location_or_cache_path=str(cache_path),
            unavailable_reason=reason,
            schema_version_or_detected_columns=parsed["detected_columns"],
            inclusion_reason=reason,
        )

    first_ts = cast(pd.Timestamp | None, parsed["first_timestamp"])
    last_ts = cast(pd.Timestamp | None, parsed["last_timestamp"])
    observation_count = int(parsed["observation_count"])
    provenance_hash = sha256_file(cache_path)
    if observation_count <= 0:
        reason = "unavailable_with_reason: existing public cache file has zero observations"
        return _coverage_row(
            symbol=symbol,
            data_family=data_family,
            available=False,
            source_type=metadata["source_type"],
            source_location_or_cache_path=str(cache_path),
            observation_count=observation_count,
            unavailable_reason=reason,
            schema_version_or_detected_columns=parsed["detected_columns"],
            provenance_hash_if_available=provenance_hash,
            inclusion_reason=reason,
        )
    if first_ts is None or last_ts is None:
        reason = "unavailable_with_reason: existing public cache file lacks a recognized causal timestamp column"
        return _coverage_row(
            symbol=symbol,
            data_family=data_family,
            available=False,
            source_type=metadata["source_type"],
            source_location_or_cache_path=str(cache_path),
            observation_count=observation_count,
            unavailable_reason=reason,
            schema_version_or_detected_columns=parsed["detected_columns"],
            provenance_hash_if_available=provenance_hash,
            inclusion_reason=reason,
        )
    missing_ranges = _missing_range_note(first_ts, last_ts, start_ts, end_ts)
    if missing_ranges:
        reason = f"unavailable_with_reason: existing public cache file does not cover target range; {missing_ranges}"
        return _coverage_row(
            symbol=symbol,
            data_family=data_family,
            available=False,
            source_type=metadata["source_type"],
            source_location_or_cache_path=str(cache_path),
            first_timestamp=_timestamp(first_ts),
            last_timestamp=_timestamp(last_ts),
            observation_count=observation_count,
            missing_ranges=missing_ranges,
            unavailable_reason=reason,
            schema_version_or_detected_columns=parsed["detected_columns"],
            provenance_hash_if_available=provenance_hash,
            inclusion_reason=reason,
        )
    included = data_family != "funding_rate_extensions"
    exclusion_reason = "" if included else "funding extension is supplemental and does not satisfy non-funding V12 gate alone"
    return _coverage_row(
        symbol=symbol,
        data_family=data_family,
        available=True,
        source_type=metadata["source_type"],
        source_location_or_cache_path=str(cache_path),
        first_timestamp=_timestamp(first_ts),
        last_timestamp=_timestamp(last_ts),
        observation_count=observation_count,
        missing_ranges=missing_ranges,
        schema_version_or_detected_columns=parsed["detected_columns"],
        provenance_hash_if_available=provenance_hash,
        included_for_future_v12=included,
        exclusion_reason=exclusion_reason,
    )


def _coverage_row(
    *,
    symbol: str,
    data_family: str,
    available: bool,
    source_type: str,
    source_location_or_cache_path: str,
    first_timestamp: str = "",
    last_timestamp: str = "",
    observation_count: int = 0,
    expected_observation_count_if_known: str = "",
    coverage_ratio_if_known: str = "",
    missing_ranges: str = "",
    unavailable_reason: str = "",
    schema_version_or_detected_columns: str = "",
    provenance_hash_if_available: str = "",
    included_for_future_v12: bool = False,
    exclusion_reason: str = "",
    inclusion_reason: str = "",
) -> dict[str, Any]:
    return {
        "symbol": symbol,
        "data_family": data_family,
        "available": available,
        "source_type": source_type,
        "source_location_or_cache_path": source_location_or_cache_path,
        "first_timestamp": first_timestamp,
        "last_timestamp": last_timestamp,
        "observation_count": observation_count,
        "expected_observation_count_if_known": expected_observation_count_if_known,
        "coverage_ratio_if_known": coverage_ratio_if_known,
        "missing_ranges": missing_ranges,
        "unavailable_reason": unavailable_reason,
        "schema_version_or_detected_columns": schema_version_or_detected_columns,
        "provenance_hash_if_available": provenance_hash_if_available,
        "included_for_future_v12": included_for_future_v12,
        "exclusion_reason": exclusion_reason or inclusion_reason,
    }


def _resolve_cache_path(
    *,
    symbol: str,
    data_family: str,
    cache_root: Path,
    funding_cache_dir: Path,
) -> Path | None:
    candidates = [
        cache_root / data_family / f"{symbol}.csv",
        cache_root / data_family / f"{symbol}.json",
        cache_root / data_family / f"{symbol}.parquet",
    ]
    if data_family == "funding_rate_extensions":
        funding_candidates = [
            candidate
            for candidate in (funding_cache_dir / "futures" / "um" / "fundingRate" / symbol).glob(f"{symbol}-*.json")
            if not candidate.name.endswith(".metadata.json")
        ]
        candidates.extend(sorted(funding_candidates, key=_funding_cache_span_sort_key, reverse=True))
    for candidate in candidates:
        if candidate.exists() and candidate.is_file():
            return candidate
    return None


def _planned_cache_path(*, symbol: str, data_family: str, cache_root: Path) -> Path:
    return cache_root / data_family / f"{symbol}.csv"


def _funding_cache_span_sort_key(path: Path) -> tuple[int, int]:
    stem = path.stem
    parts = stem.split("-")
    if len(parts) < 3:
        return (0, 0)
    try:
        start_ms = int(parts[-2])
        end_ms = int(parts[-1])
    except ValueError:
        return (0, 0)
    return (end_ms - start_ms, end_ms)


def _inspect_cache_file(path: Path) -> dict[str, Any]:
    try:
        if path.suffix == ".csv":
            frame = pd.read_csv(path)
        elif path.suffix == ".json":
            payload = json.loads(path.read_text())
            frame = pd.DataFrame(payload if isinstance(payload, list) else [payload])
        elif path.suffix == ".parquet":
            frame = pd.read_parquet(path)
        else:
            return {
                "parse_ok": False,
                "error": f"unsupported cache file suffix: {path.suffix}",
                "detected_columns": "",
                "observation_count": 0,
                "first_timestamp": None,
                "last_timestamp": None,
            }
    except (ValueError, OSError, json.JSONDecodeError) as exc:
        return {
            "parse_ok": False,
            "error": f"{type(exc).__name__}: {exc}",
            "detected_columns": "",
            "observation_count": 0,
            "first_timestamp": None,
            "last_timestamp": None,
        }
    detected_columns = ",".join(str(column) for column in frame.columns)
    timestamp_series = _timestamp_series(frame)
    first_timestamp: pd.Timestamp | None = None
    last_timestamp: pd.Timestamp | None = None
    if timestamp_series is not None and not timestamp_series.dropna().empty:
        normalized = cast(pd.Series, timestamp_series.dropna()).sort_values()
        first_timestamp = cast(pd.Timestamp, normalized.iloc[0])
        last_timestamp = cast(pd.Timestamp, normalized.iloc[-1])
    return {
        "parse_ok": True,
        "error": "",
        "detected_columns": detected_columns,
        "observation_count": int(len(frame)),
        "first_timestamp": first_timestamp,
        "last_timestamp": last_timestamp,
    }


def _timestamp_series(frame: pd.DataFrame) -> pd.Series | None:
    for column in (
        "timestamp",
        "time",
        "date",
        "open_time",
        "close_time",
        "funding_time",
        "fundingTime",
        "funding_time_ms",
        "event_time",
    ):
        if column not in frame.columns:
            continue
        series = frame[column]
        if column in {"fundingTime", "funding_time_ms"}:
            numeric = pd.to_numeric(series, errors="coerce")
            return cast(pd.Series, pd.Series(pd.to_datetime(cast(Any, numeric), unit="ms", utc=True)))
        return cast(pd.Series, pd.Series(pd.to_datetime(cast(Any, series), utc=True, errors="coerce")))
    return None


def _symbol_data_coverage_rows(
    symbols: tuple[str, ...],
    data_family_coverage: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for symbol in symbols:
        symbol_rows = [row for row in data_family_coverage if row["symbol"] == symbol]
        available_rows = [row for row in symbol_rows if bool(row["available"])]
        non_funding_rows = [
            row
            for row in available_rows
            if row["data_family"] in PUBLIC_DERIVATIVES_MICROSTRUCTURE_NON_FUNDING_FAMILIES
        ]
        first_values = [str(row["first_timestamp"]) for row in available_rows if row["first_timestamp"]]
        last_values = [str(row["last_timestamp"]) for row in available_rows if row["last_timestamp"]]
        included = bool(non_funding_rows)
        rows.append(
            {
                "symbol": symbol,
                "available_family_count": len(available_rows),
                "available_non_funding_family_count": len(non_funding_rows),
                "included_for_future_v12": included,
                "first_timestamp": min(first_values) if first_values else "",
                "last_timestamp": max(last_values) if last_values else "",
                "exclusion_reason": "" if included else "no non-funding public microstructure cache available",
            }
        )
    return rows


def _readiness_by_family_rows(
    families: tuple[str, ...],
    data_family_coverage: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for family in families:
        family_rows = [row for row in data_family_coverage if row["data_family"] == family]
        available = [row for row in family_rows if bool(row["available"])]
        included = [row for row in available if bool(row["included_for_future_v12"])]
        first_values = [str(row["first_timestamp"]) for row in available if row["first_timestamp"]]
        last_values = [str(row["last_timestamp"]) for row in available if row["last_timestamp"]]
        rows.append(
            {
                "data_family": family,
                "coverage_gate": "future_v12_input_coverage_only",
                "coverage_status": "available_cache_detected" if available else "missing_or_unavailable",
                "observed_cache_files": len(available),
                "missing_items": len(family_rows) - len(available),
                "available_symbol_count": len({row["symbol"] for row in available}),
                "included_symbol_count_for_future_v12": len({row["symbol"] for row in included}),
                "common_start_by_family": max(first_values) if first_values else "",
                "common_end_by_family": min(last_values) if last_values else "",
                "coverage_sufficient_for_future_v12": len({row["symbol"] for row in included}) >= 3,
                "not_strategy_readiness": True,
                "claim_boundary": PUBLIC_DERIVATIVES_MICROSTRUCTURE_CLAIM_BOUNDARY,
            }
        )
    return rows


def _excluded_symbol_rows(
    *,
    symbols: tuple[str, ...],
    symbol_data_coverage: list[dict[str, Any]],
) -> list[dict[str, str]]:
    excluded = [
        {
            "symbol": row["symbol"],
            "reason": row["exclusion_reason"],
        }
        for row in symbol_data_coverage
        if row["exclusion_reason"]
    ]
    present = {row["symbol"] for row in excluded}
    for row in PUBLIC_DERIVATIVES_MICROSTRUCTURE_DEFAULT_EXCLUDED_SYMBOLS:
        if row["symbol"] not in symbols and row["symbol"] not in present:
            excluded.append(dict(row))
    return excluded


def _missing_data_plan_row(
    *,
    symbol: str,
    data_family: str,
    source_location_or_cache_path: str,
    reason: str,
) -> dict[str, Any]:
    return {
        "symbol": symbol,
        "data_family": data_family,
        "missing_item": "public_cache_or_supported_public_source",
        "reason": reason,
        "remediation": "add an idempotent public no-key cache/backfill path before any V12 signal factory use",
        "public_only": True,
        "no_private_data_required": True,
        "source_type": FAMILY_SOURCE_METADATA[data_family]["source_type"],
        "source_location_or_cache_path": source_location_or_cache_path,
    }


def _download_manifest_row(
    *,
    symbol: str,
    data_family: str,
    status: str,
    cache_path: str,
    source_url: str,
    reason: str,
) -> dict[str, Any]:
    return {
        "symbol": symbol,
        "data_family": data_family,
        "status": status,
        "cache_path": cache_path,
        "source_url": source_url,
        "skipped_existing": status == "skipped_existing",
        "successful_download": False,
        "failed_download": False,
        "reason": reason,
    }


def _provenance_row(coverage_row: dict[str, Any]) -> dict[str, Any]:
    path = Path(str(coverage_row["source_location_or_cache_path"]))
    return {
        "symbol": coverage_row["symbol"],
        "data_family": coverage_row["data_family"],
        "source_type": coverage_row["source_type"],
        "source_location_or_cache_path": str(path),
        "provenance_hash": coverage_row["provenance_hash_if_available"],
        "bytes": path.stat().st_size if path.exists() else 0,
        "first_timestamp": coverage_row["first_timestamp"],
        "last_timestamp": coverage_row["last_timestamp"],
        "observation_count": coverage_row["observation_count"],
        "detected_columns": coverage_row["schema_version_or_detected_columns"],
    }


def _v10_lineage_reference_rows(path: Path, v10_state: dict[str, Any]) -> list[dict[str, Any]]:
    payload = cast(dict[str, Any], v10_state["payload"])
    rows: list[dict[str, Any]] = [
        {
            "reference_type": "v10_payload",
            "source_v10_artifact_path": str(path),
            "source_file": str(path / "v9_falsification_lineage_governance.json"),
            "source_hash": v10_state["source_v10_payload_hash"],
            "source_v9_suite_decision": v10_state["source_v9_suite_decision"],
            "source_v9_next_queue_empty": v10_state["v9_next_queue_empty"],
            "signal_family": "",
            "source_v9_family_decision": "",
            "lineage_digest": "",
            "preserved": v10_state["v10_kill_state_preserved"],
        },
        {
            "reference_type": "v9_lineage_digest",
            "source_v10_artifact_path": str(path),
            "source_file": "v9_lineage_manifest.csv",
            "source_hash": "",
            "source_v9_suite_decision": v10_state["source_v9_suite_decision"],
            "source_v9_next_queue_empty": v10_state["v9_next_queue_empty"],
            "signal_family": "",
            "source_v9_family_decision": "",
            "lineage_digest": payload.get("source_v9_hash_manifest_digest", ""),
            "preserved": True,
        },
        {
            "reference_type": "v8_lineage_digest",
            "source_v10_artifact_path": str(path),
            "source_file": "v8_lineage_manifest.csv",
            "source_hash": "",
            "source_v9_suite_decision": v10_state["source_v9_suite_decision"],
            "source_v9_next_queue_empty": v10_state["v9_next_queue_empty"],
            "signal_family": "",
            "source_v9_family_decision": "",
            "lineage_digest": payload.get("source_v8_hash_manifest_digest", ""),
            "preserved": True,
        },
    ]
    for item in cast(list[dict[str, Any]], v10_state["negative_control_rows"]):
        rows.append(
            {
                "reference_type": "negative_control_family",
                "source_v10_artifact_path": str(path),
                "source_file": str(path / "negative_control_registry.csv"),
                "source_hash": v10_state["negative_control_registry_hash"],
                "source_v9_suite_decision": v10_state["source_v9_suite_decision"],
                "source_v9_next_queue_empty": v10_state["v9_next_queue_empty"],
                "signal_family": item.get("signal_family", ""),
                "source_v9_family_decision": item.get("source_v9_family_decision", ""),
                "lineage_digest": "",
                "preserved": True,
            }
        )
    for item in cast(list[dict[str, Any]], v10_state["disallowed_revival_rows"]):
        rows.append(
            {
                "reference_type": "disallowed_revival_entry",
                "source_v10_artifact_path": str(path),
                "source_file": str(path / "disallowed_revival_registry.csv"),
                "source_hash": v10_state["disallowed_revival_registry_hash"],
                "source_v9_suite_decision": v10_state["source_v9_suite_decision"],
                "source_v9_next_queue_empty": v10_state["v9_next_queue_empty"],
                "signal_family": item.get("signal_family", ""),
                "source_v9_family_decision": item.get("source_v9_family_decision", ""),
                "lineage_digest": "",
                "preserved": True,
            }
        )
    return rows


def _suite_decision(
    *,
    coverage_sufficient_for_future_v12: bool,
    non_funding_symbol_count: int,
    minimum_required_symbols: int,
    missing_data_plan_required: bool,
) -> str:
    if coverage_sufficient_for_future_v12:
        return "MICROSTRUCTURE_AXIS_READY_FOR_V12"
    if non_funding_symbol_count < minimum_required_symbols:
        return "MICROSTRUCTURE_AXIS_NOT_READY_INSUFFICIENT_SYMBOLS"
    if missing_data_plan_required:
        return "MICROSTRUCTURE_AXIS_NOT_READY_MISSING_PUBLIC_DATA"
    return "MICROSTRUCTURE_DATA_AXIS_INFRA_ONLY"


def _missing_range_note(
    first_timestamp: pd.Timestamp | None,
    last_timestamp: pd.Timestamp | None,
    start_ts: pd.Timestamp,
    end_ts: pd.Timestamp,
) -> str:
    if first_timestamp is None or last_timestamp is None:
        return "timestamp column unavailable; exact missing ranges unknown"
    notes: list[str] = []
    if first_timestamp > start_ts:
        notes.append(f"before {first_timestamp.isoformat().replace('+00:00', 'Z')}")
    if last_timestamp < end_ts:
        notes.append(f"after {last_timestamp.isoformat().replace('+00:00', 'Z')}")
    return "; ".join(notes)


def _filter_report_changed_files(changed_files: Iterable[str]) -> list[str]:
    filtered: list[str] = []
    for raw in changed_files:
        path = str(raw)
        normalized = path.replace("\\", "/")
        if normalized.startswith("data/") or "/data/" in normalized:
            continue
        filtered.append(path)
    return filtered


def _raw_data_paths(changed_files: Iterable[str]) -> list[str]:
    raw_paths: list[str] = []
    for raw in changed_files:
        path = str(raw)
        normalized = path.replace("\\", "/")
        if normalized.startswith("data/") or "/data/" in normalized:
            raw_paths.append(path)
    return sorted(set(raw_paths))


def _assert_payload_safe(payload: dict[str, Any]) -> None:
    if payload.get("suite_decision") not in PUBLIC_DERIVATIVES_MICROSTRUCTURE_ALLOWED_SUITE_DECISIONS:
        raise ValueError(f"invalid V11 suite_decision: {payload.get('suite_decision')}")
    if payload.get("readiness_baseline_verdict") != "NOT_READY":
        raise ValueError("V11 readiness_baseline_verdict must remain NOT_READY")
    forbidden_values = {
        str(value)
        for value in payload.values()
        if isinstance(value, str)
    } & PUBLIC_DERIVATIVES_MICROSTRUCTURE_FORBIDDEN_OUTPUTS
    if forbidden_values:
        raise ValueError(f"forbidden V11 output label(s): {sorted(forbidden_values)}")
    required_false_fields = (
        "private_data_used",
        "account_data_used",
        "api_key_used",
        "live_or_testnet_path_created",
        "alpha_generation_performed",
        "strategy_optimization_performed",
        "holdout_retune_used",
        "tsmom_state_changed",
        "raw_data_files_committed",
    )
    for field in required_false_fields:
        if payload.get(field) is not False:
            raise ValueError(f"V11 safety field must be false: {field}")
    required_true_fields = (
        "no_live_orders_submitted",
        "no_testnet_orders_submitted",
        "no_account_data_used",
        "no_api_key_used",
        "no_private_order_or_fill_data_used",
    )
    for field in required_true_fields:
        if payload.get(field) is not True:
            raise ValueError(f"V11 safety field must be true: {field}")
    if payload.get("raw_data_files_committed") is not False:
        raise ValueError("V11 must not mark raw data files as committed")
    if "not alpha generation" not in str(payload.get("claim_boundary", "")):
        raise ValueError("V11 claim_boundary must explicitly forbid alpha generation")
    if not payload.get("v10_kill_state_preserved"):
        raise ValueError("V11 must preserve V10 kill-state before writing artifacts")
    if payload.get("v9_next_queue_empty") is not True:
        raise ValueError("V11 must preserve V9 empty queue state")


def _assert_recent_window_payload_safe(payload: dict[str, Any]) -> None:
    if payload.get("suite_decision") not in PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_ALLOWED_SUITE_DECISIONS:
        raise ValueError(f"invalid V12 suite_decision: {payload.get('suite_decision')}")
    if payload.get("readiness_baseline_verdict") != "NOT_READY":
        raise ValueError("V12 readiness_baseline_verdict must remain NOT_READY")
    forbidden_values = {
        str(value)
        for value in payload.values()
        if isinstance(value, str)
    } & PUBLIC_DERIVATIVES_MICROSTRUCTURE_FORBIDDEN_OUTPUTS
    if forbidden_values:
        raise ValueError(f"forbidden V12 output label(s): {sorted(forbidden_values)}")
    required_false_fields = (
        "private_data_used",
        "account_data_used",
        "api_key_used",
        "live_or_testnet_path_created",
        "alpha_generation_performed",
        "strategy_optimization_performed",
        "holdout_retune_used",
        "tsmom_state_changed",
        "raw_data_files_committed",
    )
    for field in required_false_fields:
        if payload.get(field) is not False:
            raise ValueError(f"V12 safety field must be false: {field}")
    required_true_fields = (
        "no_live_orders_submitted",
        "no_testnet_orders_submitted",
        "no_account_data_used",
        "no_api_key_used",
        "no_private_order_or_fill_data_used",
    )
    for field in required_true_fields:
        if payload.get(field) is not True:
            raise ValueError(f"V12 safety field must be true: {field}")
    if "not alpha generation" not in str(payload.get("claim_boundary", "")):
        raise ValueError("V12 claim_boundary must explicitly forbid alpha generation")
    if payload.get("v11_state_preserved") is not True:
        raise ValueError("V12 must preserve V11 state before writing artifacts")
    if payload.get("coverage_sufficient_for_future_v13") and payload.get("recent_window_microstructure_readiness"):
        if not payload.get("historical_microstructure_proxy_readiness") and payload.get("recent_window_only") is not True:
            raise ValueError("V12 recent-window-only gate must be explicit when historical proxy gate is false")


def _read_csv_records(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        raise FileNotFoundError(f"missing required V10 CSV artifact: {path}")
    return cast(list[dict[str, Any]], pd.read_csv(path).fillna("").to_dict("records"))


def _csv_frame(rows: list[dict[str, Any]], columns: tuple[str, ...]) -> pd.DataFrame:
    return pd.DataFrame(rows, columns=list(columns))


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    _atomic_write_text(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def _atomic_write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(text)
    tmp.replace(path)


def _timestamp(value: pd.Timestamp) -> str:
    timestamp = value.tz_convert("UTC") if value.tzinfo is not None else value.tz_localize("UTC")
    return timestamp.isoformat().replace("+00:00", "Z")


def _utc_timestamp(value: str) -> pd.Timestamp:
    timestamp = cast(pd.Timestamp, pd.Timestamp(value))
    if timestamp.tzinfo is None:
        return cast(pd.Timestamp, timestamp.tz_localize("UTC"))
    return cast(pd.Timestamp, timestamp.tz_convert("UTC"))
