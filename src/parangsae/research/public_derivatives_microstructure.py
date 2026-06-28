from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterable, cast

import pandas as pd

from parangsae.marketdata.binance_funding import BINANCE_USDM_FUNDING_RATE_DOCS, BINANCE_USDM_FUNDING_RATE_URL
from parangsae.marketdata.binance_vision import sha256_file, validate_symbol
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


def default_public_derivatives_microstructure_axis_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{PUBLIC_DERIVATIVES_MICROSTRUCTURE_LOOP_SLUG}"


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
