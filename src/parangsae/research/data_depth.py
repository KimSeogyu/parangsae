from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterable, cast

import pandas as pd

from parangsae.marketdata.binance_funding import FundingRateBundle
from parangsae.marketdata.binance_vision import MarketDataBundle
from parangsae.research.loop_artifacts import prepare_loop_output_dir

DATA_DEPTH_LOOP_ID = "DATA_DEPTH_RESEARCH_GRADE_V1"
DATA_DEPTH_SCHEMA_VERSION = 2
DATA_DEPTH_LOOP_TYPE = "data"
DATA_DEPTH_STRATEGY_FAMILY = "TSMOM_VOL_TARGET_FUNDING_AWARE_V1"
DATA_DEPTH_LOOP_SLUG = "data-depth-research-grade"
DATA_DEPTH_REQUIRED_FILES = ("loop.json", "report.md", "commands.txt", "changed_files.txt")
DATA_DEPTH_KNOWN_FILES = set(DATA_DEPTH_REQUIRED_FILES)
BASELINE_ARTIFACT = "reports/readiness-run-20260625T053627Z/"
READINESS_BASELINE_VERDICT = "NOT_READY"
DATA_DEPTH_WATCH_VERDICT = "WATCH"
DATA_DEPTH_INSUFFICIENT_VERDICT = "INSUFFICIENT_DATA_DEPTH"
CLAIM_BOUNDARY = (
    "DATA_DEPTH_RESEARCH_GRADE_V1 is a public-data provenance/depth loop, not a strategy-performance "
    "validation, not live-fill proof, not trading advice, not a profit guarantee, and not permission for live orders"
)
FUNDING_ALIGNMENT_POLICY = (
    "Public funding events are aligned causally to the first OHLCV bar whose timestamp is greater than "
    "or equal to the funding timestamp; funding is never forward-filled into earlier decision bars. "
    "This validates public metadata alignment only, not account-specific funding cashflows."
)


@dataclass(frozen=True)
class DataDepthConfig:
    """Frozen data-depth contract for DATA_DEPTH_RESEARCH_GRADE_V1.

    These defaults intentionally snapshot the constitution's current
    research-grade depth contract rather than importing readiness-run defaults.
    Later strategy/readiness tuning must not silently redefine this loop's
    evidence meaning.
    """

    train_bars: int = 24 * 45
    validation_bars: int = 24 * 21
    holdout_bars: int = 24 * 21
    step_bars: int = 24 * 21
    min_research_grade_total_input_bars: int = 24 * 365 * 2
    min_research_grade_window_count: int = 12
    min_research_grade_candidate_count: int = 25
    min_research_grade_candidate_window_evaluations: int = 300
    min_research_grade_bars_processed_estimate: int = 500_000


def default_data_depth_months(start: str = "2024-01", end: str = "2025-12") -> tuple[str, ...]:
    periods = pd.period_range(start=start, end=end, freq="M")
    return tuple(str(period) for period in periods)


def default_data_depth_output_dir(*, root: str | Path = "reports/loops", today: str | None = None) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{DATA_DEPTH_LOOP_SLUG}"


def prepare_data_depth_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="data-depth-run",
        known_files=DATA_DEPTH_KNOWN_FILES,
        overwrite=overwrite,
    )


def build_data_depth_loop_payload(
    *,
    bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None = None,
    funding_metadata: dict[str, Any] | None = None,
    config: DataDepthConfig = DataDepthConfig(),
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> dict[str, Any]:
    window_count = _project_data_depth_window_count(len(bundle.frame), config)
    thresholds = {
        "total_input_bars": config.min_research_grade_total_input_bars,
        "window_count": config.min_research_grade_window_count,
        "candidate_count": config.min_research_grade_candidate_count,
        "candidate_window_evaluations": config.min_research_grade_candidate_window_evaluations,
        "bars_processed_estimate": config.min_research_grade_bars_processed_estimate,
    }
    observed = {
        "total_input_bars": int(len(bundle.frame)),
        "window_count": int(window_count),
        "candidate_count": None,
        "candidate_window_evaluations": None,
        "bars_processed_estimate": None,
    }
    total_input_bars_gate = {
        "observed": observed["total_input_bars"],
        "threshold": thresholds["total_input_bars"],
        "passed": observed["total_input_bars"] >= thresholds["total_input_bars"],
    }
    projected_window_count_gate = {
        "observed": observed["window_count"],
        "threshold": thresholds["window_count"],
        "passed": observed["window_count"] >= thresholds["window_count"],
    }
    sample_depth_passed = bool(total_input_bars_gate["passed"] and projected_window_count_gate["passed"])
    data_depth_gates = {
        "total_input_bars": total_input_bars_gate,
        "window_count": projected_window_count_gate,
        "minimum_sample_depth": {
            "total_input_bars": total_input_bars_gate,
            "projected_window_count": projected_window_count_gate,
            "passed": sample_depth_passed,
            "status": "passed" if sample_depth_passed else "insufficient_public_data_depth",
        },
        "walk_forward_coverage": {
            "observed": observed["window_count"],
            "threshold": thresholds["window_count"],
            "passed": bool(projected_window_count_gate["passed"]),
            "status": "data_only_projected_window_coverage",
            "candidate_or_performance_stability_claimed": False,
        },
        "candidate_count": {
            "observed": None,
            "threshold": thresholds["candidate_count"],
            "passed": False,
            "status": "out_of_scope_for_data_depth_loop",
        },
        "candidate_window_evaluations": {
            "observed": None,
            "threshold": thresholds["candidate_window_evaluations"],
            "passed": False,
            "status": "out_of_scope_for_data_depth_loop",
        },
        "bars_processed_estimate": {
            "observed": None,
            "threshold": thresholds["bars_processed_estimate"],
            "passed": False,
            "status": "out_of_scope_until_candidate_grid_loop",
        },
    }
    data_only_passed = bool(sample_depth_passed)
    ohlcv_index = pd.DatetimeIndex(bundle.frame.index)
    funding_payload = _funding_payload(
        funding_bundle=funding_bundle,
        funding_metadata=funding_metadata,
        ohlcv_index=ohlcv_index,
    )
    strategy_performance_gates_unclaimed = _strategy_performance_gates_unclaimed()
    dataset_provenance = _dataset_provenance_payload(
        bundle=bundle,
        funding_bundle=funding_bundle,
        funding_payload=funding_payload,
    )
    coverage_analysis = _coverage_analysis_payload(
        frame=bundle.frame,
        funding_bundle=funding_bundle,
        config=config,
    )
    loop_id = f"{datetime.now(UTC).strftime('%Y%m%d')}-{DATA_DEPTH_LOOP_SLUG}"
    data_depth_loop_verdict = DATA_DEPTH_WATCH_VERDICT if data_only_passed else DATA_DEPTH_INSUFFICIENT_VERDICT
    report_artifacts = _report_artifact_provenance()
    return {
        "schema_version": DATA_DEPTH_SCHEMA_VERSION,
        "data_depth_contract": asdict(config),
        "loop_id": loop_id,
        "data_depth_loop_verdict": data_depth_loop_verdict,
        "data_depth_loop_verdict_namespace": "data_depth_only_not_readiness",
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "status": "IMPLEMENTED",
        "loop_type": DATA_DEPTH_LOOP_TYPE,
        "strategy_family": DATA_DEPTH_STRATEGY_FAMILY,
        "hypothesis": (
            "Extending public closed-bar historical market data to research-grade bar/window depth improves "
            "validation authority before any TSMOM strategy selection."
        ),
        "baseline": BASELINE_ARTIFACT,
        "data_scope": {
            "source_name": "Binance Vision public USDⓈ-M Futures monthly klines",
            "symbol": bundle.symbol,
            "product_family": "crypto_perp_public_market_data",
            "timeframe": bundle.interval,
            "timezone": "UTC",
            "months": list(bundle.months),
            "first_timestamp": bundle.quality["first_timestamp"],
            "last_timestamp": bundle.quality["last_timestamp"],
            "row_count": int(len(bundle.frame)),
            "closed_bar_only": bool(bundle.quality.get("closed_bar_only", False)),
            "open_bar_included": bool(bundle.quality.get("open_bar_included", True)),
            "close_time_policy": bundle.quality.get("close_time_policy"),
            "volume_policy": bundle.quality.get("volume_policy"),
            "zero_volume_bars": int(bundle.quality.get("zero_volume_bars", 0)),
            "normalized_ohlcv_sha256": bundle.normalized_sha256,
            "gap_policy": {
                "expected_gap_seconds": bundle.quality.get("expected_gap_seconds"),
                "gap_tolerance_seconds": bundle.quality.get("gap_tolerance_seconds", 0),
                "missing_bars_allowed": False,
            },
            "duplicate_policy": {
                "duplicates": bundle.quality.get("duplicates", 0),
                "duplicates_allowed": False,
            },
            "source_urls": [record.url for record in bundle.downloads],
            "downloads": [asdict(record) for record in bundle.downloads],
        },
        "funding_coverage": funding_payload,
        "dataset_provenance": dataset_provenance,
        "coverage_analysis": coverage_analysis,
        "selection_inputs": ["train", "validation"],
        "selection_excludes": ["holdout", "live_orders", "api_keys", "account_state", "testnet_outcomes", "forward_paper_current_window"],
        "pre_registered_grid": {},
        "expected_failure_modes": [
            "Public archive gaps or duplicate timestamps block data-depth evidence.",
            "Data-only depth can pass while candidate-count, candidate-window, stress-return, or candidate-stability gates still fail.",
            "Public funding metadata does not validate account-specific funding cashflows.",
        ],
        "safety_gates": {
            "live_orders_submitted": False,
            "testnet_orders_submitted": False,
            "api_keys_required_for_research": False,
            "signed_or_account_endpoints_used": False,
            "holdout_used_for_selection": False,
            "profit_or_live_ready_claim": False,
        },
        "validation_depth": {
            "thresholds": thresholds,
            "observed": observed,
            "data_depth_gates": data_depth_gates,
            "strategy_performance_gates_unclaimed": strategy_performance_gates_unclaimed,
            "data_only_research_grade_depth_passed": data_only_passed,
            "strategy_grid_depth_unclaimed": True,
            "interpretation": (
                "This loop can satisfy public data bar/window depth only. Candidate-count, candidate-window, "
                "bars-processed, stress-return, and candidate-stability gates remain for later strategy/validation loops."
            ),
        },
        "validation_commands": list(commands),
        "evidence_artifacts": [
            "loop.json",
            "report.md",
            "commands.txt",
            "changed_files.txt",
        ],
        "report_artifact_provenance": report_artifacts,
        "failed_gates": _failed_data_depth_gates(data_depth_gates)
        + [key for key, gate in strategy_performance_gates_unclaimed.items() if not bool(gate.get("passed"))],
        "decision": "PROMOTE_TO_NEXT_LOOP" if data_only_passed else "ITERATE_ON_SAME_HYPOTHESIS",
        "next_loop": "Narrow paper-only TSMOM_VOL_TARGET_FUNDING_AWARE_V1 strategy loop after data-depth evidence is durable.",
        "changed_files": list(changed_files),
        "claim_boundary": CLAIM_BOUNDARY,
        "created_at": datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
    }


def write_data_depth_loop_artifacts(output_dir: str | Path, payload: dict[str, Any], *, overwrite: bool = False) -> Path:
    out = prepare_data_depth_output_dir(output_dir, overwrite=overwrite)
    _write_json(out / "loop.json", payload)
    _atomic_write_text(out / "report.md", render_data_depth_report(payload))
    _atomic_write_text(
        out / "commands.txt",
        "\n".join(payload.get("validation_commands", [])) + "\n",
    )
    _atomic_write_text(
        out / "changed_files.txt",
        "\n".join(payload.get("changed_files", [])) + "\n",
    )
    return out


def render_data_depth_report(payload: dict[str, Any]) -> str:
    scope = payload["data_scope"]
    depth = payload["validation_depth"]
    funding = payload["funding_coverage"]
    gates = depth["data_depth_gates"]
    coverage = payload.get("coverage_analysis", {})
    strategy_gates = depth.get("strategy_performance_gates_unclaimed", {})
    failed = payload.get("failed_gates", [])
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Hypothesis",
        payload["hypothesis"],
        "",
        "## Baseline",
        f"- baseline: {payload['baseline']}",
        f"- readiness baseline verdict: {payload.get('readiness_baseline_verdict', READINESS_BASELINE_VERDICT)}",
        "- baseline verdict remains NOT_READY; this loop does not replace it.",
        "",
        "## Loop verdict boundary",
        f"- data_depth_loop_verdict: {payload.get('data_depth_loop_verdict')}",
        "- WATCH is data-depth-only when present; it is not a readiness verdict, not a performance verdict, and not live authorization.",
        "- data-depth-run does not change readiness verdict semantics, strategy-performance claims, or any live/trading authorization surface.",
        "",
        "## Change",
        "- Added/ran a data-depth loop for public historical closed-bar OHLCV and optional public funding metadata.",
        "- No strategy parameters were optimized and no holdout/testnet/live/account outcomes were used for selection.",
        f"- schema version: {payload['schema_version']}",
        "- data-depth contract is frozen in `loop.json.data_depth_contract` and does not import readiness-run defaults.",
        "",
        "## Data and selection boundary",
        f"- source: {scope['source_name']}",
        f"- symbol/timeframe: {scope['symbol']} / {scope['timeframe']}",
        f"- UTC range: {scope['first_timestamp']} to {scope['last_timestamp']}",
        f"- row count: {scope['row_count']}",
        f"- months: {', '.join(scope['months'])}",
        f"- closed bar only: {scope['closed_bar_only']}",
        f"- open bar included: {scope['open_bar_included']}",
        f"- close-time policy: {scope.get('close_time_policy')}",
        f"- volume policy: {scope.get('volume_policy')}",
        f"- zero-volume bars: {scope.get('zero_volume_bars')}",
        f"- normalized OHLCV sha256: {scope['normalized_ohlcv_sha256']}",
        f"- gap policy: `{json.dumps(scope['gap_policy'], sort_keys=True)}`",
        f"- duplicate policy: `{json.dumps(scope['duplicate_policy'], sort_keys=True)}`",
        f"- funding source: {funding['source']}",
        f"- funding rows: {funding['rows']}",
        f"- funding alignment policy: {funding['alignment_policy']}",
        f"- selection excludes: {', '.join(payload['selection_excludes'])}",
        "",
        "## Dataset provenance",
    ]
    for dataset in payload.get("dataset_provenance", []):
        lines.append(
            f"- {dataset['dataset']}: source={dataset['source_name']}; rows={dataset['row_count']}; "
            f"hash={dataset.get('normalized_sha256')}; no-key-public={dataset['no_key_public_data']}; "
            f"account/signed={dataset['account_or_signed_data_used']}"
        )
    lines.extend(
        [
            "",
            "## Coverage analysis",
            f"- projected walk-forward windows: {len(coverage.get('walk_forward_windows', []))}",
            f"- volatility regimes: `{json.dumps(coverage.get('volatility_regimes', {}), sort_keys=True)}`",
            f"- stress periods: `{json.dumps(coverage.get('stress_periods', {}), sort_keys=True)}`",
            f"- funding regimes: `{json.dumps(coverage.get('funding_regimes', {}), sort_keys=True)}`",
            "",
            "## Gates",
            f"- minimum sample depth: {'PASS' if gates['minimum_sample_depth']['passed'] else 'FAIL'}",
            f"- walk-forward coverage: {'PASS' if gates['walk_forward_coverage']['passed'] else 'FAIL'} (data-only projected windows, not candidate stability)",
            f"- strategy_performance_gates_unclaimed: {', '.join(strategy_gates.keys()) if strategy_gates else 'none'}",
            "",
            "## Commands",
        ]
    )
    commands = payload.get("validation_commands", [])
    lines.extend(f"- `{command}`" for command in commands) if commands else lines.append("- not recorded")
    lines.extend(
        [
            "",
            "## Evidence",
            f"- total input bars: {depth['observed']['total_input_bars']} / threshold {depth['thresholds']['total_input_bars']} — {'PASS' if gates['total_input_bars']['passed'] else 'FAIL'}",
            f"- walk-forward windows projected with frozen data-depth contract: {depth['observed']['window_count']} / threshold {depth['thresholds']['window_count']} — {'PASS' if gates['window_count']['passed'] else 'FAIL'}",
            f"- data-depth loop verdict: {payload.get('data_depth_loop_verdict')} (data-depth-only)",
            f"- data-only research-grade depth passed: {depth['data_only_research_grade_depth_passed']}",
            f"- strategy-grid depth unclaimed: {depth['strategy_grid_depth_unclaimed']}",
            "",
            "## Result interpretation",
            f"- What passed: public closed-bar OHLCV metadata and data-only bar/window depth gates: {depth['data_only_research_grade_depth_passed']}.",
            f"- What failed/remains unclaimed: {', '.join(failed) if failed else 'none for data-only gates'}.",
            "- Evidence depth used: public historical OHLCV plus optional public funding metadata; no live/testnet/account data.",
            "- Cannot claim: profitability, stress-return pass, candidate stability, full research-grade strategy validation, execution mechanics, or live readiness.",
            "",
            "## Decision",
            f"- decision: {payload['decision']}",
            "",
            "## Why",
            f"- claim boundary: {payload['claim_boundary']}",
            "- The loop improves data authority before alpha evaluation while preserving the committed NOT_READY baseline.",
            "",
            "## Next loop",
            payload["next_loop"],
        ]
    )
    return "\n".join(lines) + "\n"


def _funding_payload(
    *,
    funding_bundle: FundingRateBundle | None,
    funding_metadata: dict[str, Any] | None,
    ohlcv_index: pd.DatetimeIndex,
) -> dict[str, Any]:
    base = {
        "source": "none",
        "rows": 0,
        "first_timestamp": None,
        "last_timestamp": None,
        "normalized_funding_sha256": None,
        "coverage_status": "not_requested",
        "alignment_policy": FUNDING_ALIGNMENT_POLICY,
        "causal_alignment_checked": False,
        "account_cashflows_validated": False,
        "metadata": funding_metadata or {},
    }
    if funding_bundle is None:
        return base
    frame = funding_bundle.frame
    if frame.empty:
        return {**base, "source": "public_binance_usdm_funding_history", "coverage_status": "empty"}
    funding_index = pd.DatetimeIndex(frame.index)
    first_timestamp = cast(pd.Timestamp, funding_index[0])
    last_timestamp = cast(pd.Timestamp, funding_index[-1])
    return {
        **base,
        "source": "public_binance_usdm_funding_history",
        "rows": int(len(frame)),
        "first_timestamp": first_timestamp.isoformat().replace("+00:00", "Z"),
        "last_timestamp": last_timestamp.isoformat().replace("+00:00", "Z"),
        "normalized_funding_sha256": funding_bundle.normalized_sha256,
        "coverage_status": "validated_public_data",
        "causal_alignment_checked": _funding_events_align_to_known_bars(funding_index, ohlcv_index),
        "metadata": funding_metadata or {},
    }


def _funding_events_align_to_known_bars(funding_index: pd.DatetimeIndex, ohlcv_index: pd.DatetimeIndex) -> bool:
    if funding_index.tz is None or ohlcv_index.tz is None:
        return False
    if funding_index.empty or ohlcv_index.empty:
        return False
    positions = ohlcv_index.searchsorted(funding_index, side="left")
    return bool((positions < len(ohlcv_index)).all())


def _failed_data_depth_gates(data_depth_gates: dict[str, dict[str, Any]]) -> list[str]:
    failures: list[str] = []
    for key, gate in data_depth_gates.items():
        if key in {"minimum_sample_depth", "walk_forward_coverage"}:
            if not bool(gate.get("passed")):
                failures.append(key)
            continue
        if not bool(gate.get("passed")):
            failures.append(key)
    return failures


def _strategy_performance_gates_unclaimed() -> dict[str, dict[str, Any]]:
    base = {
        "observed": None,
        "passed": False,
        "status": "blocked_until_strategy_validation",
        "claim_boundary": "DATA_DEPTH_RESEARCH_GRADE_V1 records the gate but cannot satisfy it without a pre-registered paper strategy validation.",
    }
    return {
        "median_stress_return_beats_cash_null": {
            **base,
            "threshold": "median stress-adjusted strategy return > 0.0 cash/null return",
        },
        "selected_candidate_switch_rate": {
            **base,
            "threshold": "readiness max selected-candidate switch rate <= 0.75",
        },
        "funding_aware_net_return": {
            **base,
            "threshold": "public-funding-adjusted net strategy return must beat declared baseline",
        },
        "max_drawdown_abs_pct": {
            **base,
            "threshold": "pre-registered strategy drawdown threshold required",
        },
        "tail_loss_cvar_pct": {
            **base,
            "threshold": "pre-registered strategy CVaR/tail-loss threshold required",
        },
    }


def _dataset_provenance_payload(
    *,
    bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None,
    funding_payload: dict[str, Any],
) -> list[dict[str, Any]]:
    ohlcv = {
        "dataset": "ohlcv",
        "source_name": "Binance Vision public USDⓈ-M Futures monthly klines",
        "symbol": bundle.symbol,
        "product_family": "crypto_perp_public_market_data",
        "timeframe": bundle.interval,
        "timezone": "UTC",
        "months": list(bundle.months),
        "first_timestamp": bundle.quality["first_timestamp"],
        "last_timestamp": bundle.quality["last_timestamp"],
        "row_count": int(len(bundle.frame)),
        "normalized_sha256": bundle.normalized_sha256,
        "source_urls": [record.url for record in bundle.downloads],
        "downloads": [asdict(record) for record in bundle.downloads],
        "closed_bar_only": bool(bundle.quality.get("closed_bar_only", False)),
        "open_bar_included": bool(bundle.quality.get("open_bar_included", True)),
        "gap_policy": {
            "expected_gap_seconds": bundle.quality.get("expected_gap_seconds"),
            "gap_tolerance_seconds": bundle.quality.get("gap_tolerance_seconds", 0),
            "missing_bars_allowed": False,
        },
        "duplicate_policy": {
            "duplicates": bundle.quality.get("duplicates", 0),
            "duplicates_allowed": False,
        },
        "no_key_public_data": True,
        "account_or_signed_data_used": False,
        "live_or_testnet_data_used": False,
    }
    funding = {
        "dataset": "funding",
        "source_name": funding_payload["source"],
        "symbol": bundle.symbol,
        "product_family": "crypto_perp_public_funding_data",
        "timeframe": "8h_funding_events",
        "timezone": "UTC",
        "months": list(bundle.months),
        "first_timestamp": funding_payload.get("first_timestamp"),
        "last_timestamp": funding_payload.get("last_timestamp"),
        "row_count": int(funding_payload.get("rows", 0)),
        "normalized_sha256": funding_payload.get("normalized_funding_sha256"),
        "source_urls": [record.url for record in funding_bundle.downloads] if funding_bundle is not None else [],
        "downloads": [asdict(record) for record in funding_bundle.downloads] if funding_bundle is not None else [],
        "coverage_status": funding_payload.get("coverage_status"),
        "causal_alignment_checked": bool(funding_payload.get("causal_alignment_checked", False)),
        "alignment_policy": funding_payload.get("alignment_policy"),
        "no_key_public_data": True,
        "account_or_signed_data_used": False,
        "live_or_testnet_data_used": False,
    }
    return [ohlcv, funding]


def _coverage_analysis_payload(
    *,
    frame: pd.DataFrame,
    funding_bundle: FundingRateBundle | None,
    config: DataDepthConfig,
) -> dict[str, Any]:
    volatility_payload, volatility_labels = _volatility_regime_payload(frame)
    return {
        "walk_forward_windows": _project_data_depth_windows(frame, config),
        "volatility_regimes": volatility_payload,
        "stress_periods": _stress_period_payload(frame, volatility_labels),
        "funding_regimes": _funding_regime_payload(funding_bundle),
    }


def _project_data_depth_windows(frame: pd.DataFrame, config: DataDepthConfig) -> list[dict[str, Any]]:
    total = len(frame)
    span = config.train_bars + config.validation_bars + config.holdout_bars
    if total < span:
        return []
    index = pd.DatetimeIndex(frame.index)
    windows: list[dict[str, Any]] = []
    start = 0
    number = 1
    while start + span <= total:
        train_start = start
        train_end = start + config.train_bars
        validation_start = train_end
        validation_end = validation_start + config.validation_bars
        holdout_start = validation_end
        holdout_end = holdout_start + config.holdout_bars
        windows.append(
            {
                "window_id": f"W{number:03d}",
                "train_start": _timestamp(index[train_start]),
                "train_end": _timestamp(index[train_end - 1]),
                "validation_start": _timestamp(index[validation_start]),
                "validation_end": _timestamp(index[validation_end - 1]),
                "holdout_start": _timestamp(index[holdout_start]),
                "holdout_end": _timestamp(index[holdout_end - 1]),
                "train_bars": int(config.train_bars),
                "validation_bars": int(config.validation_bars),
                "holdout_bars": int(config.holdout_bars),
                "complete": True,
            }
        )
        start += config.step_bars
        number += 1
    return windows


def _volatility_regime_payload(frame: pd.DataFrame) -> tuple[dict[str, Any], pd.Series]:
    close = _numeric_series(frame, "Close")
    returns = cast(pd.Series, close.pct_change())
    rolling_vol = cast(pd.Series, returns.rolling(window=24, min_periods=24).std())
    valid = cast(pd.Series, rolling_vol.dropna())
    labels = pd.Series("insufficient_history", index=frame.index, dtype="object")
    if valid.empty:
        return (
            {
                "method": "24_bar_rolling_std_of_hourly_close_returns",
                "low_threshold": None,
                "high_threshold": None,
                "low_count": 0,
                "normal_count": 0,
                "high_count": 0,
                "insufficient_history_count": int(len(frame)),
                "total_bars": int(len(frame)),
            },
            labels,
        )
    low_threshold = float(cast(float, valid.quantile(0.33)))
    high_threshold = float(cast(float, valid.quantile(0.67)))
    labels.loc[valid.index[valid <= low_threshold]] = "low"
    labels.loc[valid.index[(valid > low_threshold) & (valid < high_threshold)]] = "normal"
    labels.loc[valid.index[valid >= high_threshold]] = "high"
    return (
        {
            "method": "24_bar_rolling_std_of_hourly_close_returns",
            "low_threshold": low_threshold,
            "high_threshold": high_threshold,
            "low_count": int((labels == "low").sum()),
            "normal_count": int((labels == "normal").sum()),
            "high_count": int((labels == "high").sum()),
            "insufficient_history_count": int((labels == "insufficient_history").sum()),
            "total_bars": int(len(frame)),
        },
        labels,
    )


def _stress_period_payload(frame: pd.DataFrame, volatility_labels: pd.Series) -> dict[str, Any]:
    close = _numeric_series(frame, "Close")
    returns = cast(pd.Series, cast(pd.Series, close.pct_change()).dropna())
    if close.empty or returns.empty:
        return {
            "context_only_not_strategy_pnl": True,
            "max_underlying_drawdown_pct": None,
            "tail_loss_cvar_5_pct": None,
            "worst_24h_return_pct": None,
            "high_vol_bar_count": int((volatility_labels == "high").sum()),
            "stress_bar_count": int((volatility_labels == "high").sum()),
        }
    equity = close / close.iloc[0]
    drawdown = equity / equity.cummax() - 1.0
    tail_threshold = float(returns.quantile(0.05))
    tail = cast(pd.Series, returns[returns <= tail_threshold])
    cvar_5 = float(tail.mean() * 100) if not tail.empty else None
    rolling_24h = cast(pd.Series, cast(pd.Series, close.pct_change(24)).dropna())
    stress_returns = cast(pd.Series, returns[returns <= tail_threshold])
    high_vol_labels = cast(pd.Series, volatility_labels[volatility_labels == "high"])
    stress_return_index = set(stress_returns.index)
    high_vol_index = set(high_vol_labels.index)
    return {
        "context_only_not_strategy_pnl": True,
        "return_series": "hourly_close_pct_change",
        "stress_rule": "high_vol_regime_or_hourly_return_at_or_below_5th_percentile",
        "max_underlying_drawdown_pct": float(drawdown.min() * 100),
        "tail_loss_cvar_5_pct": cvar_5,
        "worst_24h_return_pct": float(rolling_24h.min() * 100) if not rolling_24h.empty else None,
        "high_vol_bar_count": int(len(high_vol_index)),
        "stress_bar_count": int(len(high_vol_index | stress_return_index)),
    }


def _funding_regime_payload(funding_bundle: FundingRateBundle | None) -> dict[str, Any]:
    if funding_bundle is None or funding_bundle.frame.empty:
        return {
            "coverage_status": "not_requested",
            "rows": 0,
            "positive_count": 0,
            "negative_count": 0,
            "zero_count": 0,
            "abs_rate_75pct_threshold": None,
            "high_abs_funding_count": 0,
            "first_timestamp": None,
            "last_timestamp": None,
            "account_cashflows_validated": False,
        }
    rates = _numeric_series(funding_bundle.frame, "funding_rate").dropna()
    threshold = float(rates.abs().quantile(0.75)) if not rates.empty else None
    return {
        "coverage_status": "validated_public_data",
        "rows": int(len(funding_bundle.frame)),
        "positive_count": int((rates > 0).sum()),
        "negative_count": int((rates < 0).sum()),
        "zero_count": int((rates == 0).sum()),
        "abs_rate_75pct_threshold": threshold,
        "high_abs_funding_count": int((rates.abs() >= threshold).sum()) if threshold is not None else 0,
        "first_timestamp": _timestamp(funding_bundle.frame.index[0]),
        "last_timestamp": _timestamp(funding_bundle.frame.index[-1]),
        "account_cashflows_validated": False,
    }


def _report_artifact_provenance() -> list[dict[str, Any]]:
    return [
        {
            "artifact": name,
            "path": name,
            "generated_by": "write_data_depth_loop_artifacts",
            "schema_version": DATA_DEPTH_SCHEMA_VERSION if name == "loop.json" else None,
            "source": "DATA_DEPTH_RESEARCH_GRADE_V1 public-data loop payload",
            "live_or_testnet_data_used": False,
            "account_or_signed_data_used": False,
        }
        for name in DATA_DEPTH_REQUIRED_FILES
    ]


def _timestamp(value: Any) -> str:
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return ts.isoformat().replace("+00:00", "Z")


def _numeric_series(frame: pd.DataFrame, column: str) -> pd.Series:
    return cast(pd.Series, pd.to_numeric(frame[column], errors="coerce")).astype(float)


def _project_data_depth_window_count(total_bars: int, config: DataDepthConfig) -> int:
    span = config.train_bars + config.validation_bars + config.holdout_bars
    if total_bars < span:
        return 0
    return int(((total_bars - span) // config.step_bars) + 1)


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    _atomic_write_text(
        path,
        json.dumps(_json_safe(payload), indent=2, sort_keys=True, ensure_ascii=False) + "\n",
    )


def _atomic_write_text(path: Path, payload: str) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(payload, encoding="utf-8")
    tmp.replace(path)


def _json_safe(value: Any) -> Any:
    if isinstance(value, dict):
        return {str(k): _json_safe(v) for k, v in value.items()}
    if isinstance(value, list | tuple):
        return [_json_safe(item) for item in value]
    if isinstance(value, pd.Timestamp):
        if value.tzinfo is None:
            value = value.tz_localize("UTC")
        else:
            value = value.tz_convert("UTC")
        return value.isoformat().replace("+00:00", "Z")
    if pd.isna(value) if not isinstance(value, (dict, list, tuple, str, bytes)) else False:
        return None
    return value
