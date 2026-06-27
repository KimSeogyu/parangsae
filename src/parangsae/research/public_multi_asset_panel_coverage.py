# pyright: reportAttributeAccessIssue=false
from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Callable, Iterable, cast

import pandas as pd
import requests

from parangsae.marketdata.binance_funding import (
    BINANCE_USDM_FUNDING_RATE_DOCS,
    BINANCE_USDM_FUNDING_RATE_URL,
    BinanceFundingError,
    FundingDataValidationError,
    FundingRateBundle,
    fetch_funding_rate_history,
    funding_cache_path,
)
from parangsae.marketdata.binance_vision import (
    BINANCE_VISION_BASE,
    CANONICAL_INTERVAL,
    BinanceVisionError,
    DataValidationError,
    MarketDataBundle,
    canonical_cache_path,
    load_market_data_bundle,
    normalize_months,
    validate_interval,
    validate_symbol,
)
from parangsae.research.loop_artifacts import prepare_loop_output_dir

PUBLIC_MULTI_ASSET_PANEL_COVERAGE_SCHEMA_VERSION = 6
PUBLIC_MULTI_ASSET_PANEL_COVERAGE_LOOP_SLUG = "public-multi-asset-data-panel-coverage-v6"
PUBLIC_MULTI_ASSET_PANEL_COVERAGE_NAMESPACE = (
    "public_multi_asset_data_panel_coverage_v6_only_not_alpha"
)
DEFAULT_PUBLIC_MULTI_ASSET_PANEL_V5_ARTIFACT = Path(
    "reports/loops/20260627-multi-asset-alpha-generator-strict-prefilter-v5"
)
DEFAULT_PUBLIC_MULTI_ASSET_PANEL_MINIMUM_REQUIRED_SYMBOLS = 3
PUBLIC_MULTI_ASSET_PANEL_DEFAULT_UNIVERSE = (
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
PUBLIC_MULTI_ASSET_PANEL_ALLOWED_SUITE_DECISIONS = (
    "DATA_COVERAGE_INFRA_ONLY",
    "PANEL_READY_FOR_V5_RERUN",
    "PANEL_NOT_READY_MISSING_PUBLIC_DATA",
    "PANEL_NOT_READY_INSUFFICIENT_SYMBOLS",
    "PANEL_NOT_READY_INSUFFICIENT_HISTORY",
)
PUBLIC_MULTI_ASSET_PANEL_FORBIDDEN_OUTPUT_LABELS = {
    "READY",
    "TRADE",
    "LIVE_READY",
    "PROFITABLE",
    "DEPLOY",
    "CAPITAL_ALLOCATE",
    "EDGE_CONFIRMED",
}
PUBLIC_MULTI_ASSET_PANEL_CLAIM_BOUNDARY = (
    "PUBLIC_MULTI_ASSET_DATA_PANEL_COVERAGE_V6 is public-data coverage and provenance "
    "infrastructure only. It audits OHLCV/funding availability for a future paper-only V5 "
    "rerun. It is not alpha generation, not a trading strategy, not portfolio construction, "
    "not signal weight optimization, not profitability evidence, not readiness evidence, "
    "and not permission for live/testnet/account/API-key/order/fill use."
)
PUBLIC_MULTI_ASSET_PANEL_REQUIRED_FILES = (
    "public_multi_asset_panel_coverage.json",
    "symbol_coverage.csv",
    "missing_data_plan.csv",
    "panel_readiness.csv",
    "provenance_manifest.csv",
    "report.md",
    "commands.txt",
    "changed_files.txt",
)
PUBLIC_MULTI_ASSET_PANEL_KNOWN_FILES = set(PUBLIC_MULTI_ASSET_PANEL_REQUIRED_FILES)

SYMBOL_COVERAGE_COLUMNS = (
    "symbol",
    "requested",
    "included_in_panel",
    "ohlcv_available",
    "funding_available",
    "excluded_stage",
    "excluded_reason",
    "v5_excluded_stage",
    "v5_excluded_reason",
    "first_ohlcv_timestamp",
    "last_ohlcv_timestamp",
    "bar_count",
    "expected_bar_count",
    "coverage_ratio",
    "missing_ohlcv_months",
    "ohlcv_cache_files_found",
    "ohlcv_cache_files_requested",
    "ohlcv_normalized_sha256",
    "funding_status",
    "first_funding_timestamp",
    "last_funding_timestamp",
    "funding_observation_count",
    "expected_funding_observation_count",
    "funding_coverage_ratio",
    "funding_normalized_sha256",
    "data_source",
    "funding_source",
    "cache_mode",
)
MISSING_DATA_PLAN_COLUMNS = (
    "symbol",
    "data_type",
    "missing_item",
    "reason",
    "remediation",
    "public_only",
    "no_private_data_required",
    "cache_mode",
    "v5_excluded_reason",
)
PANEL_READINESS_COLUMNS = (
    "readiness_gate",
    "status",
    "observed",
    "threshold",
    "reason",
)
PROVENANCE_MANIFEST_COLUMNS = (
    "symbol",
    "data_type",
    "source",
    "source_url",
    "docs_url",
    "interval",
    "months",
    "cache_path",
    "sha256",
    "normalized_sha256",
    "from_cache",
    "http_status",
    "rows",
    "first_timestamp",
    "last_timestamp",
)

MarketLoader = Callable[..., MarketDataBundle]
FundingLoader = Callable[..., FundingRateBundle]


@dataclass(frozen=True)
class PublicMultiAssetPanelCoverageResult:
    payload: dict[str, Any]
    symbol_coverage: list[dict[str, Any]]
    missing_data_plan: list[dict[str, Any]]
    panel_readiness: list[dict[str, Any]]
    provenance_manifest: list[dict[str, Any]]


def default_public_multi_asset_panel_coverage_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{PUBLIC_MULTI_ASSET_PANEL_COVERAGE_LOOP_SLUG}"


def normalize_public_symbol_universe(symbols: Iterable[str]) -> tuple[str, ...]:
    seen: set[str] = set()
    normalized: list[str] = []
    for raw in symbols:
        symbol = validate_symbol(str(raw))
        if symbol in seen:
            continue
        seen.add(symbol)
        normalized.append(symbol)
    if not normalized:
        raise ValueError("at least one requested symbol is required")
    return tuple(normalized)


def parse_public_symbol_universe(raw: str | Iterable[str] | None) -> tuple[str, ...]:
    if raw is None:
        return PUBLIC_MULTI_ASSET_PANEL_DEFAULT_UNIVERSE
    if isinstance(raw, str):
        return normalize_public_symbol_universe(item for item in raw.split(",") if item.strip())
    return normalize_public_symbol_universe(raw)


def evaluate_public_multi_asset_panel_coverage(
    *,
    requested_symbols: str | Iterable[str] | None = None,
    interval: str = CANONICAL_INTERVAL,
    months: Iterable[str],
    cache_dir: str | Path = "data/binance-vision",
    funding_source: str = "public-binance",
    funding_cache_dir: str | Path = "data/binance-funding",
    cache_mode: str = "cache-only",
    minimum_required_symbols: int = DEFAULT_PUBLIC_MULTI_ASSET_PANEL_MINIMUM_REQUIRED_SYMBOLS,
    source_v5_artifact_path: str | Path = DEFAULT_PUBLIC_MULTI_ASSET_PANEL_V5_ARTIFACT,
    market_loader: MarketLoader = load_market_data_bundle,
    funding_loader: FundingLoader = fetch_funding_rate_history,
    commands: Iterable[str] | None = None,
    changed_files: Iterable[str] | None = None,
) -> PublicMultiAssetPanelCoverageResult:
    symbols = parse_public_symbol_universe(requested_symbols)
    interval = validate_interval(interval)
    months = normalize_months(months)
    if cache_mode not in {"cache-only", "download-missing"}:
        raise ValueError("cache_mode must be cache-only or download-missing")
    if funding_source not in {"public-binance", "none"}:
        raise ValueError("funding_source must be public-binance or none")
    if minimum_required_symbols <= 0:
        raise ValueError("minimum_required_symbols must be positive")

    expected_bar_count = _expected_bar_count(months, interval)
    v5_state = _read_v5_state(Path(source_v5_artifact_path))
    v5_excluded_by_symbol = {
        str(row.get("symbol")): row for row in cast(list[dict[str, Any]], v5_state["excluded_symbols"])
    }
    market_bundles: dict[str, MarketDataBundle] = {}
    funding_bundles: dict[str, FundingRateBundle] = {}
    symbol_coverage: list[dict[str, Any]] = []
    missing_data_plan: list[dict[str, Any]] = []
    provenance_manifest: list[dict[str, Any]] = []
    excluded_symbols: list[dict[str, Any]] = []
    downloaded_public_file_count = 0
    cached_public_file_count = 0

    for symbol in symbols:
        v5_exclusion = v5_excluded_by_symbol.get(symbol, {})
        missing_months = _missing_ohlcv_months(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=cache_dir,
        )
        row = _base_symbol_row(
            symbol=symbol,
            expected_bar_count=expected_bar_count,
            requested_months=months,
            missing_months=missing_months,
            cache_mode=cache_mode,
            funding_source=funding_source,
            v5_exclusion=v5_exclusion,
        )
        if cache_mode == "cache-only" and missing_months:
            reason = f"missing cached OHLCV month(s): {','.join(missing_months[:6])}"
            _exclude_symbol(
                row,
                excluded_symbols,
                stage="ohlcv_cache_availability",
                reason=reason,
                v5_exclusion=v5_exclusion,
            )
            missing_data_plan.append(
                _missing_plan_row(
                    symbol=symbol,
                    data_type="ohlcv",
                    missing_item=",".join(missing_months),
                    reason=reason,
                    remediation="download or provide public Binance Vision monthly kline cache files only",
                    cache_mode=cache_mode,
                    v5_exclusion=v5_exclusion,
                )
            )
            symbol_coverage.append(row)
            continue

        try:
            market_bundle = market_loader(
                symbol=symbol,
                interval=interval,
                months=months,
                cache_dir=cache_dir,
            )
        except (BinanceVisionError, DataValidationError, requests.RequestException, ValueError) as exc:
            reason = f"{type(exc).__name__}: {exc}"
            _exclude_symbol(
                row,
                excluded_symbols,
                stage="ohlcv_load_or_validation",
                reason=reason,
                v5_exclusion=v5_exclusion,
            )
            missing_data_plan.append(
                _missing_plan_row(
                    symbol=symbol,
                    data_type="ohlcv",
                    missing_item="load_or_validation_failed",
                    reason=reason,
                    remediation="repair public OHLCV cache provenance or retry public Binance Vision download",
                    cache_mode=cache_mode,
                    v5_exclusion=v5_exclusion,
                )
            )
            symbol_coverage.append(row)
            continue

        market_bundles[symbol] = market_bundle
        downloaded_public_file_count += sum(1 for record in market_bundle.downloads if not record.from_cache)
        cached_public_file_count += sum(1 for record in market_bundle.downloads if record.from_cache)
        _record_ohlcv_success(
            row,
            market_bundle,
            expected_bar_count=expected_bar_count,
            requested_months=months,
            cache_dir=cache_dir,
        )
        provenance_manifest.extend(_ohlcv_provenance_rows(market_bundle, months=months))

        if funding_source == "none":
            row["funding_status"] = "disabled"
            symbol_coverage.append(row)
            continue

        start_ts = _datetime_index_endpoint(market_bundle.frame.index, 0)
        end_ts = _datetime_index_endpoint(market_bundle.frame.index, -1)
        expected_funding_count = _expected_funding_observation_count(start_ts, end_ts)
        row["expected_funding_observation_count"] = expected_funding_count
        if cache_mode == "cache-only" and not _funding_cache_available(
            symbol=symbol,
            start_time=start_ts,
            end_time=end_ts,
            cache_dir=funding_cache_dir,
        ):
            row["funding_status"] = "unavailable_missing_cache"
            missing_data_plan.append(
                _missing_plan_row(
                    symbol=symbol,
                    data_type="funding",
                    missing_item="exact public funding cache for OHLCV timestamp span",
                    reason="missing cached public Binance funding history",
                    remediation="download public Binance USD-M funding history cache for this exact timestamp span",
                    cache_mode=cache_mode,
                    v5_exclusion=v5_exclusion,
                )
            )
            symbol_coverage.append(row)
            continue

        try:
            funding_bundle = funding_loader(
                symbol=symbol,
                start_time=start_ts,
                end_time=end_ts,
                cache_dir=funding_cache_dir,
            )
        except (BinanceFundingError, FundingDataValidationError, requests.RequestException, ValueError) as exc:
            row["funding_status"] = f"invalid_unavailable:{type(exc).__name__}"
            missing_data_plan.append(
                _missing_plan_row(
                    symbol=symbol,
                    data_type="funding",
                    missing_item="load_or_validation_failed",
                    reason=f"{type(exc).__name__}: {exc}",
                    remediation="repair public funding cache provenance or retry public Binance funding download",
                    cache_mode=cache_mode,
                    v5_exclusion=v5_exclusion,
                )
            )
            symbol_coverage.append(row)
            continue

        funding_bundles[symbol] = funding_bundle
        downloaded_public_file_count += sum(1 for record in funding_bundle.downloads if not record.from_cache)
        cached_public_file_count += sum(1 for record in funding_bundle.downloads if record.from_cache)
        _record_funding_success(row, funding_bundle, expected_funding_count=expected_funding_count)
        provenance_manifest.extend(_funding_provenance_rows(funding_bundle, interval=interval, months=months))
        symbol_coverage.append(row)

    usable_symbols = [symbol for symbol in symbols if symbol in market_bundles]
    common_index = _common_panel_index([market_bundles[symbol].frame for symbol in usable_symbols])
    common_start = _timestamp(cast(pd.Timestamp, common_index[0])) if len(common_index) else None
    common_end = _timestamp(cast(pd.Timestamp, common_index[-1])) if len(common_index) else None
    common_bar_count = int(len(common_index))
    minimum_common_bar_count = expected_bar_count
    minimum_symbols_passed = len(usable_symbols) >= minimum_required_symbols
    common_history_passed = common_bar_count >= minimum_common_bar_count and common_bar_count > 0
    v5_artifact_present = v5_state["status"] == "present"
    v5_exclusions_preserved = _v5_exclusions_preserved(symbol_coverage, v5_state)
    panel_ready_for_v5_rerun = bool(minimum_symbols_passed and common_history_passed and v5_artifact_present)
    suite_decision = _suite_decision(
        panel_ready_for_v5_rerun=panel_ready_for_v5_rerun,
        usable_symbol_count=len(usable_symbols),
        minimum_required_symbols=minimum_required_symbols,
        common_bar_count=common_bar_count,
        minimum_common_bar_count=minimum_common_bar_count,
        missing_data_plan=missing_data_plan,
    )
    panel_readiness = _panel_readiness_rows(
        minimum_symbols_passed=minimum_symbols_passed,
        usable_symbol_count=len(usable_symbols),
        minimum_required_symbols=minimum_required_symbols,
        common_history_passed=common_history_passed,
        common_bar_count=common_bar_count,
        minimum_common_bar_count=minimum_common_bar_count,
        panel_ready_for_v5_rerun=panel_ready_for_v5_rerun,
        v5_state=v5_state,
        v5_exclusions_preserved=v5_exclusions_preserved,
    )
    coverage_ratio_by_symbol = {
        row["symbol"]: row["coverage_ratio"] for row in symbol_coverage
    }
    funding_coverage_ratio_by_symbol = {
        row["symbol"]: row["funding_coverage_ratio"] for row in symbol_coverage
    }
    payload = {
        "schema_version": PUBLIC_MULTI_ASSET_PANEL_COVERAGE_SCHEMA_VERSION,
        "loop_id": f"{datetime.now(UTC).strftime('%Y%m%d')}-{PUBLIC_MULTI_ASSET_PANEL_COVERAGE_LOOP_SLUG}",
        "loop_namespace": PUBLIC_MULTI_ASSET_PANEL_COVERAGE_NAMESPACE,
        "suite_decision": suite_decision,
        "readiness_baseline_verdict": "NOT_READY",
        "source_v5_artifact_path": str(source_v5_artifact_path),
        "v5_state": v5_state["status"],
        "v5_symbol_universe_used": v5_state["symbol_universe_used"],
        "v5_excluded_symbols": v5_state["excluded_symbols"],
        "v5_excluded_symbols_preserved": v5_exclusions_preserved,
        "requested_symbol_universe": list(symbols),
        "usable_symbol_universe": usable_symbols,
        "excluded_symbols": excluded_symbols,
        "requested_symbol_count": len(symbols),
        "usable_symbol_count": len(usable_symbols),
        "panel_ready_for_v5_rerun": panel_ready_for_v5_rerun,
        "common_start": common_start,
        "common_end": common_end,
        "common_bar_count": common_bar_count,
        "minimum_required_symbols": minimum_required_symbols,
        "minimum_common_bar_count": minimum_common_bar_count,
        "requested_months": list(months),
        "interval": interval,
        "cache_mode": cache_mode,
        "funding_source": funding_source,
        "coverage_ratio_by_symbol": coverage_ratio_by_symbol,
        "funding_coverage_ratio_by_symbol": funding_coverage_ratio_by_symbol,
        "data_sources_used": _data_sources_used(funding_source=funding_source),
        "downloaded_public_file_count": downloaded_public_file_count,
        "cached_public_file_count": cached_public_file_count,
        "missing_data_plan_count": len(missing_data_plan),
        "private_data_used": False,
        "account_data_used": False,
        "api_key_used": False,
        "no_live_orders_submitted": True,
        "no_testnet_orders_submitted": True,
        "no_account_data_used": True,
        "no_api_key_used": True,
        "no_private_order_or_fill_data_used": True,
        "holdout_retune_used": False,
        "strategy_optimization_performed": False,
        "live_or_testnet_path_created": False,
        "tsmom_state_changed": False,
        "raw_public_data_committed": False,
        "claim_boundary": PUBLIC_MULTI_ASSET_PANEL_CLAIM_BOUNDARY,
        "safety_gates": {
            "no_live_orders_submitted": True,
            "no_testnet_orders_submitted": True,
            "no_account_data_used": True,
            "no_api_key_used": True,
            "no_private_order_or_fill_data_used": True,
            "holdout_retune_used": False,
            "strategy_optimization_performed": False,
            "live_or_testnet_path_created": False,
            "tsmom_state_changed": False,
            "readiness_baseline_verdict": "NOT_READY",
        },
        "validation_commands": list(commands or []),
        "changed_files": list(changed_files or []),
    }
    _assert_public_panel_payload_safe(payload)
    return PublicMultiAssetPanelCoverageResult(
        payload=payload,
        symbol_coverage=symbol_coverage,
        missing_data_plan=missing_data_plan,
        panel_readiness=panel_readiness,
        provenance_manifest=provenance_manifest,
    )


def prepare_public_multi_asset_panel_coverage_output_dir(
    output_dir: str | Path,
    *,
    overwrite: bool = False,
) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="public-multi-asset-panel-coverage-run",
        known_files=PUBLIC_MULTI_ASSET_PANEL_KNOWN_FILES,
        overwrite=overwrite,
    )


def write_public_multi_asset_panel_coverage_artifacts(
    output_dir: str | Path,
    result: PublicMultiAssetPanelCoverageResult,
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_public_multi_asset_panel_coverage_output_dir(output_dir, overwrite=overwrite)
    payload = dict(result.payload)
    if out.name.endswith(PUBLIC_MULTI_ASSET_PANEL_COVERAGE_LOOP_SLUG):
        payload["loop_id"] = out.name
    _assert_public_panel_payload_safe(payload)
    _write_json(out / "public_multi_asset_panel_coverage.json", payload)
    _csv_frame(result.symbol_coverage, SYMBOL_COVERAGE_COLUMNS).to_csv(out / "symbol_coverage.csv", index=False)
    _csv_frame(result.missing_data_plan, MISSING_DATA_PLAN_COLUMNS).to_csv(out / "missing_data_plan.csv", index=False)
    _csv_frame(result.panel_readiness, PANEL_READINESS_COLUMNS).to_csv(out / "panel_readiness.csv", index=False)
    _csv_frame(result.provenance_manifest, PROVENANCE_MANIFEST_COLUMNS).to_csv(
        out / "provenance_manifest.csv",
        index=False,
    )
    _atomic_write_text(out / "report.md", render_public_multi_asset_panel_coverage_report(payload, result))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    return out


def render_public_multi_asset_panel_coverage_report(
    payload: dict[str, Any],
    result: PublicMultiAssetPanelCoverageResult,
) -> str:
    excluded = result.symbol_coverage
    missing = result.missing_data_plan[:20]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Purpose",
        "Audit public multi-asset OHLCV/funding coverage and provenance for a future paper-only V5 rerun.",
        "",
        "## Suite decision",
        f"- suite_decision: {payload['suite_decision']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- panel_ready_for_v5_rerun: {payload['panel_ready_for_v5_rerun']}",
        "",
        "## V5 state",
        f"- source_v5_artifact_path: {payload['source_v5_artifact_path']}",
        f"- v5_state: {payload['v5_state']}",
        f"- v5_excluded_symbols_preserved: {payload['v5_excluded_symbols_preserved']}",
        "",
        "## Symbol universe",
        f"- requested_symbol_count: {payload['requested_symbol_count']}",
        f"- usable_symbol_count: {payload['usable_symbol_count']}",
        f"- usable_symbol_universe: {', '.join(payload['usable_symbol_universe']) or 'none'}",
        f"- common_start: {payload['common_start']}",
        f"- common_end: {payload['common_end']}",
        f"- common_bar_count: {payload['common_bar_count']}",
        "",
        "## Coverage",
    ]
    for row in excluded:
        lines.append(
            "- "
            f"{row['symbol']}: included={row['included_in_panel']} "
            f"ohlcv={row['ohlcv_available']} funding={row['funding_available']} "
            f"reason={row['excluded_reason'] or row['funding_status']}"
        )
    lines.extend(["", "## Missing data plan"])
    if not missing:
        lines.append("- no missing public data rows recorded")
    else:
        for row in missing:
            lines.append(f"- {row['symbol']} {row['data_type']}: {row['reason']}")
    lines.extend(
        [
            "",
            "## Safety",
            "- no live orders submitted: true",
            "- no testnet orders submitted: true",
            "- no account/API-key/private order/fill data used: true",
            "- TSMOM state changed: false",
            "- holdout retune used: false",
            "- strategy optimization performed: false",
            "",
            "## Claim boundary",
            payload["claim_boundary"],
            "",
        ]
    )
    return "\n".join(lines)


def _base_symbol_row(
    *,
    symbol: str,
    expected_bar_count: int,
    requested_months: tuple[str, ...],
    missing_months: list[str],
    cache_mode: str,
    funding_source: str,
    v5_exclusion: dict[str, Any],
) -> dict[str, Any]:
    return {
        "symbol": symbol,
        "requested": True,
        "included_in_panel": False,
        "ohlcv_available": False,
        "funding_available": False,
        "excluded_stage": "",
        "excluded_reason": "",
        "v5_excluded_stage": str(v5_exclusion.get("stage", "")),
        "v5_excluded_reason": str(v5_exclusion.get("reason", "")),
        "first_ohlcv_timestamp": "",
        "last_ohlcv_timestamp": "",
        "bar_count": 0,
        "expected_bar_count": expected_bar_count,
        "coverage_ratio": 0.0,
        "missing_ohlcv_months": ",".join(missing_months),
        "ohlcv_cache_files_found": len(requested_months) - len(missing_months),
        "ohlcv_cache_files_requested": len(requested_months),
        "ohlcv_normalized_sha256": "",
        "funding_status": "not_attempted",
        "first_funding_timestamp": "",
        "last_funding_timestamp": "",
        "funding_observation_count": 0,
        "expected_funding_observation_count": 0,
        "funding_coverage_ratio": 0.0,
        "funding_normalized_sha256": "",
        "data_source": "public_binance_vision_um_monthly_klines",
        "funding_source": funding_source,
        "cache_mode": cache_mode,
    }


def _exclude_symbol(
    row: dict[str, Any],
    excluded_symbols: list[dict[str, Any]],
    *,
    stage: str,
    reason: str,
    v5_exclusion: dict[str, Any],
) -> None:
    row["excluded_stage"] = stage
    row["excluded_reason"] = reason
    excluded_symbols.append(
        {
            "symbol": row["symbol"],
            "stage": stage,
            "reason": reason,
            "v5_excluded_stage": str(v5_exclusion.get("stage", "")),
            "v5_excluded_reason": str(v5_exclusion.get("reason", "")),
        }
    )


def _record_ohlcv_success(
    row: dict[str, Any],
    bundle: MarketDataBundle,
    *,
    expected_bar_count: int,
    requested_months: tuple[str, ...],
    cache_dir: str | Path,
) -> None:
    frame = bundle.frame
    row["included_in_panel"] = True
    row["ohlcv_available"] = True
    row["excluded_stage"] = ""
    row["excluded_reason"] = ""
    row["first_ohlcv_timestamp"] = _timestamp(_datetime_index_endpoint(frame.index, 0))
    row["last_ohlcv_timestamp"] = _timestamp(_datetime_index_endpoint(frame.index, -1))
    row["bar_count"] = int(len(frame))
    row["coverage_ratio"] = _ratio(len(frame), expected_bar_count)
    missing_months = _missing_ohlcv_months(
        symbol=bundle.symbol,
        interval=bundle.interval,
        months=requested_months,
        cache_dir=cache_dir,
    )
    row["missing_ohlcv_months"] = ",".join(missing_months)
    row["ohlcv_cache_files_found"] = len(requested_months) - len(missing_months)
    row["ohlcv_cache_files_requested"] = len(requested_months)
    row["ohlcv_normalized_sha256"] = bundle.normalized_sha256


def _record_funding_success(
    row: dict[str, Any],
    bundle: FundingRateBundle,
    *,
    expected_funding_count: int,
) -> None:
    frame = bundle.frame
    row["funding_available"] = True
    row["funding_status"] = "available"
    row["first_funding_timestamp"] = _timestamp(_datetime_index_endpoint(frame.index, 0))
    row["last_funding_timestamp"] = _timestamp(_datetime_index_endpoint(frame.index, -1))
    row["funding_observation_count"] = int(len(frame))
    row["expected_funding_observation_count"] = expected_funding_count
    row["funding_coverage_ratio"] = _ratio(len(frame), expected_funding_count)
    row["funding_normalized_sha256"] = bundle.normalized_sha256


def _missing_plan_row(
    *,
    symbol: str,
    data_type: str,
    missing_item: str,
    reason: str,
    remediation: str,
    cache_mode: str,
    v5_exclusion: dict[str, Any],
) -> dict[str, Any]:
    return {
        "symbol": symbol,
        "data_type": data_type,
        "missing_item": missing_item,
        "reason": reason,
        "remediation": remediation,
        "public_only": True,
        "no_private_data_required": True,
        "cache_mode": cache_mode,
        "v5_excluded_reason": str(v5_exclusion.get("reason", "")),
    }


def _missing_ohlcv_months(
    *,
    symbol: str,
    interval: str,
    months: tuple[str, ...],
    cache_dir: str | Path,
) -> list[str]:
    return [
        month
        for month in months
        if not canonical_cache_path(Path(cache_dir), symbol, interval, month).exists()
    ]


def _funding_cache_available(
    *,
    symbol: str,
    start_time: pd.Timestamp,
    end_time: pd.Timestamp,
    cache_dir: str | Path,
) -> bool:
    start_ms = int(start_time.timestamp() * 1000)
    end_ms = int(end_time.timestamp() * 1000)
    return funding_cache_path(cache_dir, symbol, start_ms, end_ms).exists()


def _expected_bar_count(months: tuple[str, ...], interval: str) -> int:
    interval = validate_interval(interval)
    if interval != "1h":
        raise ValueError("V6 panel coverage currently supports 1h public monthly klines")
    total = 0
    for month in months:
        total += int(pd.Period(month, freq="M").days_in_month) * 24
    return total


def _expected_funding_observation_count(start_time: pd.Timestamp, end_time: pd.Timestamp) -> int:
    if end_time < start_time:
        return 0
    span_hours = int((end_time.timestamp() - start_time.timestamp()) // 3600)
    return int(span_hours // 8) + 1


def _common_panel_index(frames: list[pd.DataFrame]) -> pd.DatetimeIndex:
    if not frames:
        return pd.DatetimeIndex([], tz="UTC")
    common = pd.DatetimeIndex(frames[0].index)
    for frame in frames[1:]:
        common = common.intersection(pd.DatetimeIndex(frame.index))
    return pd.DatetimeIndex(common.sort_values())


def _read_v5_state(path: Path) -> dict[str, Any]:
    artifact = path / "multi_asset_alpha_factory.json" if path.is_dir() else path
    if not artifact.exists():
        return {
            "status": "missing",
            "path": str(path),
            "symbol_universe_used": [],
            "excluded_symbols": [],
        }
    try:
        payload = json.loads(artifact.read_text())
    except json.JSONDecodeError:
        return {
            "status": "invalid_json",
            "path": str(path),
            "symbol_universe_used": [],
            "excluded_symbols": [],
        }
    return {
        "status": "present",
        "path": str(path),
        "suite_decision": payload.get("suite_decision"),
        "symbol_universe_used": list(payload.get("symbol_universe_used", [])),
        "excluded_symbols": list(payload.get("excluded_symbols", [])),
        "readiness_baseline_verdict": payload.get("readiness_baseline_verdict"),
    }


def _v5_exclusions_preserved(symbol_coverage: list[dict[str, Any]], v5_state: dict[str, Any]) -> bool:
    v5_excluded = {
        str(row.get("symbol")): str(row.get("reason", ""))
        for row in cast(list[dict[str, Any]], v5_state.get("excluded_symbols", []))
    }
    if not v5_excluded:
        return True
    coverage_by_symbol = {str(row["symbol"]): row for row in symbol_coverage}
    return all(
        coverage_by_symbol[symbol]["v5_excluded_reason"] == reason
        for symbol, reason in v5_excluded.items()
        if symbol in coverage_by_symbol
    )


def _suite_decision(
    *,
    panel_ready_for_v5_rerun: bool,
    usable_symbol_count: int,
    minimum_required_symbols: int,
    common_bar_count: int,
    minimum_common_bar_count: int,
    missing_data_plan: list[dict[str, Any]],
) -> str:
    if panel_ready_for_v5_rerun:
        return "PANEL_READY_FOR_V5_RERUN"
    if usable_symbol_count < minimum_required_symbols:
        return "PANEL_NOT_READY_INSUFFICIENT_SYMBOLS"
    if common_bar_count < minimum_common_bar_count:
        return "PANEL_NOT_READY_INSUFFICIENT_HISTORY"
    if missing_data_plan:
        return "PANEL_NOT_READY_MISSING_PUBLIC_DATA"
    return "DATA_COVERAGE_INFRA_ONLY"


def _panel_readiness_rows(
    *,
    minimum_symbols_passed: bool,
    usable_symbol_count: int,
    minimum_required_symbols: int,
    common_history_passed: bool,
    common_bar_count: int,
    minimum_common_bar_count: int,
    panel_ready_for_v5_rerun: bool,
    v5_state: dict[str, Any],
    v5_exclusions_preserved: bool,
) -> list[dict[str, Any]]:
    return [
        {
            "readiness_gate": "minimum_required_symbols",
            "status": "passed" if minimum_symbols_passed else "failed",
            "observed": usable_symbol_count,
            "threshold": minimum_required_symbols,
            "reason": "at least three OHLCV-usable public symbols are required",
        },
        {
            "readiness_gate": "common_history",
            "status": "passed" if common_history_passed else "failed",
            "observed": common_bar_count,
            "threshold": minimum_common_bar_count,
            "reason": "usable symbols must share the requested chronological public OHLCV span",
        },
        {
            "readiness_gate": "v5_artifact_present",
            "status": "passed" if v5_state["status"] == "present" else "failed",
            "observed": v5_state["status"],
            "threshold": "present",
            "reason": "V6 uses the V5 artifact only as immutable lineage for excluded symbol reasons",
        },
        {
            "readiness_gate": "v5_excluded_symbols_preserved",
            "status": "passed" if v5_exclusions_preserved else "failed",
            "observed": v5_exclusions_preserved,
            "threshold": True,
            "reason": "V6 records V5 excluded symbols and reasons without mutating V5 artifacts",
        },
        {
            "readiness_gate": "panel_ready_for_v5_rerun",
            "status": "passed" if panel_ready_for_v5_rerun else "failed",
            "observed": panel_ready_for_v5_rerun,
            "threshold": True,
            "reason": "V6 only enables a separate future paper-only V5 rerun when data coverage passes",
        },
    ]


def _ohlcv_provenance_rows(bundle: MarketDataBundle, *, months: tuple[str, ...]) -> list[dict[str, Any]]:
    rows = []
    first = _timestamp(_datetime_index_endpoint(bundle.frame.index, 0))
    last = _timestamp(_datetime_index_endpoint(bundle.frame.index, -1))
    for record in bundle.downloads:
        rows.append(
            {
                "symbol": bundle.symbol,
                "data_type": "ohlcv",
                "source": "public_binance_vision_um_monthly_klines",
                "source_url": record.url,
                "docs_url": BINANCE_VISION_BASE,
                "interval": bundle.interval,
                "months": ",".join(months),
                "cache_path": record.cache_path,
                "sha256": record.sha256,
                "normalized_sha256": bundle.normalized_sha256,
                "from_cache": record.from_cache,
                "http_status": record.http_status,
                "rows": int(len(bundle.frame)),
                "first_timestamp": first,
                "last_timestamp": last,
            }
        )
    return rows


def _funding_provenance_rows(
    bundle: FundingRateBundle,
    *,
    interval: str,
    months: tuple[str, ...],
) -> list[dict[str, Any]]:
    rows = []
    first = _timestamp(_datetime_index_endpoint(bundle.frame.index, 0))
    last = _timestamp(_datetime_index_endpoint(bundle.frame.index, -1))
    for record in bundle.downloads:
        rows.append(
            {
                "symbol": bundle.symbol,
                "data_type": "funding",
                "source": "public_binance_usdm_funding_history",
                "source_url": BINANCE_USDM_FUNDING_RATE_URL,
                "docs_url": BINANCE_USDM_FUNDING_RATE_DOCS,
                "interval": interval,
                "months": ",".join(months),
                "cache_path": record.cache_path,
                "sha256": record.sha256,
                "normalized_sha256": bundle.normalized_sha256,
                "from_cache": record.from_cache,
                "http_status": record.http_status,
                "rows": int(len(bundle.frame)),
                "first_timestamp": first,
                "last_timestamp": last,
            }
        )
    return rows


def _data_sources_used(*, funding_source: str) -> list[str]:
    sources = ["public_binance_vision_um_monthly_klines"]
    if funding_source == "public-binance":
        sources.append("public_binance_usdm_funding_history")
    return sources


def _csv_frame(rows: list[dict[str, Any]], columns: tuple[str, ...]) -> pd.DataFrame:
    frame = pd.DataFrame(rows)
    if frame.empty:
        return pd.DataFrame(columns=list(columns))
    for column in columns:
        if column not in frame.columns:
            frame[column] = ""
    return frame.loc[:, list(columns)]


def _assert_public_panel_payload_safe(payload: dict[str, Any]) -> None:
    if payload.get("suite_decision") not in PUBLIC_MULTI_ASSET_PANEL_ALLOWED_SUITE_DECISIONS:
        raise ValueError(f"invalid V6 suite_decision: {payload.get('suite_decision')}")
    if payload.get("suite_decision") in PUBLIC_MULTI_ASSET_PANEL_FORBIDDEN_OUTPUT_LABELS:
        raise ValueError("V6 emitted a forbidden suite_decision")
    if payload.get("readiness_baseline_verdict") != "NOT_READY":
        raise ValueError("V6 readiness_baseline_verdict must remain NOT_READY")
    safety = cast(dict[str, Any], payload.get("safety_gates", {}))
    required_true = (
        "no_live_orders_submitted",
        "no_testnet_orders_submitted",
        "no_account_data_used",
        "no_api_key_used",
        "no_private_order_or_fill_data_used",
    )
    for key in required_true:
        if safety.get(key) is not True or payload.get(key) is not True:
            raise ValueError(f"V6 safety gate must be true: {key}")
    required_false = (
        "holdout_retune_used",
        "strategy_optimization_performed",
        "live_or_testnet_path_created",
        "tsmom_state_changed",
    )
    for key in required_false:
        if safety.get(key) is not False or payload.get(key) is not False:
            raise ValueError(f"V6 safety gate must be false: {key}")


def _datetime_index_endpoint(index: pd.Index, position: int) -> pd.Timestamp:
    dt_index = pd.DatetimeIndex(index)
    return cast(pd.Timestamp, dt_index[position])


def _timestamp(value: pd.Timestamp) -> str:
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return ts.isoformat().replace("+00:00", "Z")


def _ratio(numerator: int | float, denominator: int | float) -> float:
    denominator_float = float(denominator)
    if denominator_float <= 0.0:
        return 0.0
    return float(numerator) / denominator_float


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    _atomic_write_text(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def _atomic_write_text(path: Path, payload: str) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(payload)
    tmp.replace(path)
