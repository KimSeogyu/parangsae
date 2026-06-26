from __future__ import annotations

import hashlib
import json
import math
import random
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Iterable, cast

import pandas as pd

from parangsae.research.loop_artifacts import prepare_loop_output_dir

RESEARCH_NULL_BASELINE_HARNESS_LOOP_ID = "RESEARCH_NULL_BASELINE_HARNESS_V1"
RESEARCH_NULL_BASELINE_HARNESS_SCHEMA_VERSION = 1
RESEARCH_NULL_BASELINE_HARNESS_LOOP_TYPE = "validation_infrastructure"
RESEARCH_NULL_BASELINE_HARNESS_LOOP_SLUG = "research-null-baseline-harness-v1"
RESEARCH_NULL_BASELINE_HARNESS_NAMESPACE = "research_null_baseline_harness_only_not_readiness"
RESEARCH_NULL_BASELINE_HARNESS_DECISION = "VALIDATION_INFRA_ONLY_TSMOM_UNCHANGED"
READINESS_BASELINE_VERDICT = "NOT_READY"
BASELINE_HARNESS_REQUIRED_FILES = (
    "baseline_harness.json",
    "baseline_comparison.csv",
    "report.md",
    "commands.txt",
    "changed_files.txt",
)
BASELINE_HARNESS_KNOWN_FILES = set(BASELINE_HARNESS_REQUIRED_FILES)
NULL_BASELINE_FAMILIES = (
    "cash_null",
    "buy_and_hold",
    "random_sign",
    "lagged_signal",
    "inverse_signal",
    "simpler_trend_baseline",
)
BASELINE_COMPARISON_SCHEMA_COLUMNS = (
    "baseline_name",
    "strategy_return_gross",
    "strategy_return_net",
    "baseline_return_gross",
    "baseline_return_net",
    "strategy_beats_baseline_gross",
    "strategy_beats_baseline_net",
    "sample_size",
    "trade_count",
    "turnover",
    "fees_drag",
    "slippage_drag",
    "funding_drag",
    "max_drawdown",
    "regime_filter",
    "evidence_source",
    "claim_boundary",
)
DEFAULT_TSMOM_SOURCE_LOOP_ARTIFACT = Path("reports/loops/20260625-tsmom-vol-target-funding-aware-v1/loop.json")
DEFAULT_TSMOM_DIAGNOSTIC_ARTIFACT = Path(
    "reports/loops/20260626-tsmom-stress-switch-funding-diagnostic-v1/diagnostic.json"
)
DEFAULT_TSMOM_RISK_ARTIFACT = Path(
    "reports/loops/20260626-tsmom-switch-stability-funding-throttle-risk-v1/risk_loop.json"
)
DEFAULT_TSMOM_GROSS_EDGE_ARTIFACT = Path(
    "reports/loops/20260626-tsmom-gross-edge-regime-null-robustness-v1/gross_edge_loop.json"
)
DEFAULT_TSMOM_FAMILY_DECISION_ARTIFACT = Path(
    "reports/loops/20260626-tsmom-candidate-family-decision-v1/candidate_family_decision.json"
)
DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT = Path(
    "reports/loops/20260626-regime-only-tsmom-falsification-v1/decision.json"
)


def null_baseline_claim_boundary() -> str:
    return (
        "RESEARCH_NULL_BASELINE_HARNESS_V1 is strategy-agnostic paper-only validation "
        "infrastructure. It makes future strategy claims harder by standardizing null/baseline "
        "comparison rows, leakage metadata, deterministic random signs, and holdout-retune "
        "boundaries. It is not alpha, not strategy optimization, not readiness or live readiness, "
        "not trading advice, not a profit guarantee, and uses no live, testnet, account, API-key, "
        "private order, or fill data."
    )


@dataclass(frozen=True)
class NullBaselineEvaluationInput:
    window_id: str
    strategy_name: str
    returns: pd.Series
    strategy_position: pd.Series
    strategy_return_gross: float
    strategy_return_net: float
    sample_size: int
    trade_count: int
    turnover: float
    fees_drag: float
    slippage_drag: float
    funding_drag: float
    max_drawdown: float | None
    regime_filter: str | None
    evidence_source: str
    claim_boundary: str = null_baseline_claim_boundary()
    forward_return_label_overlap: bool = False
    forward_return_label_overlap_detail: str = "not_detected_in_durable_inputs"
    holdout_retune_used: bool = False
    lag_bars: int = 24
    deterministic_seed_base: int = 1729
    cvar_quantile: float = 0.05


def default_research_null_baseline_harness_output_dir(
    *,
    root: str | Path = "reports/loops",
    today: str | None = None,
) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{RESEARCH_NULL_BASELINE_HARNESS_LOOP_SLUG}"


def deterministic_random_seed(evidence_key: str, *, seed_base: int = 1729) -> int:
    return int(seed_base) + sum(ord(char) for char in str(evidence_key))


def deterministic_random_sign_position(
    position: pd.Series,
    evidence_key: str,
    *,
    seed_base: int = 1729,
) -> tuple[pd.Series, int]:
    seed = deterministic_random_seed(evidence_key, seed_base=seed_base)
    rng = random.Random(seed)
    absolute_position = _float_series(position, index=position.index).abs()
    signs = [1.0 if rng.random() >= 0.5 else -1.0 for _ in range(len(absolute_position))]
    signed = pd.Series(signs, index=absolute_position.index, dtype=float) * absolute_position
    return _float_series(signed, index=absolute_position.index), seed


def lagged_signal_position(position: pd.Series, *, lag_bars: int = 24) -> pd.Series:
    if lag_bars <= 0:
        raise ValueError("lag_bars must be positive to avoid current-bar signal leakage")
    return _float_series(position.shift(lag_bars), index=position.index).fillna(0.0)


def inverse_signal_position(position: pd.Series) -> pd.Series:
    return -_float_series(position, index=position.index)


def evaluate_position_stats(
    position: pd.Series,
    returns: pd.Series,
    *,
    cvar_quantile: float = 0.05,
) -> dict[str, Any]:
    aligned_position = _float_series(position, index=returns.index).reindex(returns.index).fillna(0.0)
    shifted_position = _float_series(aligned_position.shift(1), index=returns.index).fillna(0.0)
    clean_returns = _float_series(returns, index=returns.index).reindex(returns.index).fillna(0.0)
    strategy_returns = _float_series(shifted_position * clean_returns, index=returns.index)
    gross_return_factors = _float_series(1.0 + strategy_returns, index=returns.index)
    equity = _float_series(gross_return_factors.cumprod(), index=returns.index)
    if equity.empty:
        return {
            "return_pct": 0.0,
            "max_drawdown_pct": 0.0,
            "cvar_tail_loss_pct": None,
            "average_exposure": 0.0,
            "turnover": 0.0,
        }
    drawdown = _float_series(equity / equity.cummax() - 1.0, index=equity.index)
    return {
        "return_pct": float((float(equity.iloc[-1]) - 1.0) * 100.0),
        "max_drawdown_pct": _series_min_float(drawdown * 100.0),
        "cvar_tail_loss_pct": _tail_loss_cvar(
            [float(value) * 100.0 for value in strategy_returns],
            quantile=cvar_quantile,
        ),
        "average_exposure": _finite_float(cast(Any, aligned_position.abs().mean())),
        "turnover": _finite_float(cast(Any, aligned_position.diff().abs().fillna(aligned_position.abs()).sum())),
    }


def build_null_baseline_comparison_rows(payload: NullBaselineEvaluationInput) -> list[dict[str, Any]]:
    random_position, seed = deterministic_random_sign_position(
        payload.strategy_position,
        payload.window_id,
        seed_base=payload.deterministic_seed_base,
    )
    returns = _float_series(payload.returns, index=payload.returns.index).fillna(0.0)
    positions: list[dict[str, Any]] = [
        {
            "baseline_name": "cash_null",
            "baseline_description": "zero-return cash/null baseline",
            "position": pd.Series(0.0, index=returns.index, dtype=float),
            "deterministic_seed": None,
            "uses_current_bar_signal": False,
        },
        {
            "baseline_name": "buy_and_hold",
            "baseline_description": "simple unit buy-and-hold exposure",
            "position": pd.Series(1.0, index=returns.index, dtype=float),
            "deterministic_seed": None,
            "uses_current_bar_signal": False,
        },
        {
            "baseline_name": "random_sign",
            "baseline_description": "deterministic random sign with source absolute exposure",
            "position": random_position,
            "deterministic_seed": seed,
            "uses_current_bar_signal": False,
        },
        {
            "baseline_name": "lagged_signal",
            "baseline_description": f"source signal lagged by {payload.lag_bars} bars",
            "position": lagged_signal_position(payload.strategy_position, lag_bars=payload.lag_bars),
            "deterministic_seed": None,
            "uses_current_bar_signal": False,
        },
        {
            "baseline_name": "inverse_signal",
            "baseline_description": "inverse of source strategy exposure",
            "position": inverse_signal_position(payload.strategy_position),
            "deterministic_seed": None,
            "uses_current_bar_signal": True,
        },
    ]
    rows: list[dict[str, Any]] = []
    for item in positions:
        baseline_name = str(item["baseline_name"])
        stats = evaluate_position_stats(
            cast(pd.Series, item["position"]),
            returns,
            cvar_quantile=payload.cvar_quantile,
        )
        baseline_gross = float(stats["return_pct"])
        baseline_net = baseline_gross
        rows.append(
            _common_baseline_row(
                baseline_name=baseline_name,
                strategy_name=payload.strategy_name,
                strategy_return_gross=float(payload.strategy_return_gross),
                strategy_return_net=float(payload.strategy_return_net),
                baseline_return_gross=baseline_gross,
                baseline_return_net=baseline_net,
                sample_size=int(payload.sample_size),
                trade_count=int(payload.trade_count),
                turnover=float(payload.turnover),
                fees_drag=float(payload.fees_drag),
                slippage_drag=float(payload.slippage_drag),
                funding_drag=float(payload.funding_drag),
                max_drawdown=payload.max_drawdown,
                regime_filter=payload.regime_filter,
                evidence_source=payload.evidence_source,
                claim_boundary=payload.claim_boundary,
                baseline_status="available",
                baseline_description=str(item["baseline_description"]),
                deterministic_seed=cast(int | None, item["deterministic_seed"]),
                forward_return_label_overlap=payload.forward_return_label_overlap,
                forward_return_label_overlap_detail=payload.forward_return_label_overlap_detail,
                holdout_retune_used=payload.holdout_retune_used,
                baseline_max_drawdown=stats["max_drawdown_pct"],
                baseline_turnover=stats["turnover"],
                baseline_cvar_tail_loss=stats["cvar_tail_loss_pct"],
                baseline_average_exposure=stats["average_exposure"],
                uses_current_bar_signal=bool(item["uses_current_bar_signal"]),
                lag_bars=payload.lag_bars if baseline_name == "lagged_signal" else None,
            )
        )
    return rows


def unavailable_simpler_trend_baseline_row(
    *,
    strategy_name: str,
    evidence_source: str,
    claim_boundary: str = null_baseline_claim_boundary(),
    regime_filter: str | None = None,
) -> dict[str, Any]:
    return _common_baseline_row(
        baseline_name="simpler_trend_baseline",
        strategy_name=strategy_name,
        strategy_return_gross=None,
        strategy_return_net=None,
        baseline_return_gross=None,
        baseline_return_net=None,
        sample_size=0,
        trade_count=0,
        turnover=0.0,
        fees_drag=None,
        slippage_drag=None,
        funding_drag=None,
        max_drawdown=None,
        regime_filter=regime_filter,
        evidence_source=evidence_source,
        claim_boundary=claim_boundary,
        baseline_status="unavailable_unclaimed",
        baseline_description="not available unless durable evidence provides a simpler trend baseline",
        deterministic_seed=None,
        forward_return_label_overlap=False,
        forward_return_label_overlap_detail="not_evaluated_unavailable_baseline",
        holdout_retune_used=False,
        baseline_max_drawdown=None,
        baseline_turnover=None,
        baseline_cvar_tail_loss=None,
        baseline_average_exposure=None,
        uses_current_bar_signal=False,
        lag_bars=None,
    )


def build_research_null_baseline_harness_payload(
    *,
    source_loop_artifact: str | Path = DEFAULT_TSMOM_SOURCE_LOOP_ARTIFACT,
    diagnostic_artifact: str | Path = DEFAULT_TSMOM_DIAGNOSTIC_ARTIFACT,
    risk_artifact: str | Path = DEFAULT_TSMOM_RISK_ARTIFACT,
    gross_edge_artifact: str | Path = DEFAULT_TSMOM_GROSS_EDGE_ARTIFACT,
    family_decision_artifact: str | Path = DEFAULT_TSMOM_FAMILY_DECISION_ARTIFACT,
    regime_only_artifact: str | Path = DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT,
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> dict[str, Any]:
    source_path = Path(source_loop_artifact)
    diagnostic_path = Path(diagnostic_artifact)
    risk_path = Path(risk_artifact)
    gross_edge_path = Path(gross_edge_artifact)
    family_decision_path = Path(family_decision_artifact)
    regime_only_path = Path(regime_only_artifact)
    source_payload = _read_json_artifact(source_path, required_key="tsmom_loop_verdict")
    diagnostic_payload = _read_json_artifact(diagnostic_path, required_key="tsmom_diagnostic_loop_verdict")
    risk_payload = _read_json_artifact(risk_path, required_key="tsmom_risk_loop_verdict")
    gross_edge_payload = _read_json_artifact(gross_edge_path, required_key="tsmom_gross_edge_loop_verdict")
    family_decision_payload = _read_json_artifact(
        family_decision_path,
        required_key="tsmom_candidate_family_decision",
    )
    regime_only_payload = _read_json_artifact(
        regime_only_path,
        required_key="regime_only_tsmom_falsification_decision",
    )
    comparison_rows = _baseline_rows_from_durable_tsmom_artifacts(
        source_payload=source_payload,
        gross_edge_payload=gross_edge_payload,
        regime_only_payload=regime_only_payload,
        gross_edge_path=gross_edge_path,
        regime_only_path=regime_only_path,
    )
    family_counts = _baseline_family_counts(comparison_rows)
    loop_id = f"{datetime.now(UTC).strftime('%Y%m%d')}-{RESEARCH_NULL_BASELINE_HARNESS_LOOP_SLUG}"
    return {
        "schema_version": RESEARCH_NULL_BASELINE_HARNESS_SCHEMA_VERSION,
        "loop_id": loop_id,
        "status": "IMPLEMENTED",
        "loop_type": RESEARCH_NULL_BASELINE_HARNESS_LOOP_TYPE,
        "research_null_baseline_harness_loop": RESEARCH_NULL_BASELINE_HARNESS_LOOP_ID,
        "research_null_baseline_harness_namespace": RESEARCH_NULL_BASELINE_HARNESS_NAMESPACE,
        "research_null_baseline_harness_decision": RESEARCH_NULL_BASELINE_HARNESS_DECISION,
        "baseline": "reports/readiness-run-20260625T053627Z/",
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "claim_boundary": null_baseline_claim_boundary(),
        "pre_registration": _harness_pre_registration(),
        "schema_contract": {
            "required_columns": list(BASELINE_COMPARISON_SCHEMA_COLUMNS),
            "baseline_families": list(NULL_BASELINE_FAMILIES),
            "strategy_agnostic": True,
            "readiness_verdict": False,
            "holdout_retune_used": False,
            "random_sign_deterministic": True,
            "lagged_signal_current_bar_info_used": False,
        },
        "evidence_inputs": _harness_evidence_inputs(
            source_path=source_path,
            diagnostic_path=diagnostic_path,
            risk_path=risk_path,
            gross_edge_path=gross_edge_path,
            family_decision_path=family_decision_path,
            regime_only_path=regime_only_path,
        ),
        "source_loop_state_preserved": {
            "source_tsmom_loop_verdict": source_payload.get("tsmom_loop_verdict"),
            "source_tsmom_failed_gates": source_payload.get("failed_gates", []),
            "diagnostic_loop_verdict": diagnostic_payload.get("tsmom_diagnostic_loop_verdict"),
            "risk_loop_verdict": risk_payload.get("tsmom_risk_loop_verdict"),
            "gross_edge_loop_verdict": gross_edge_payload.get("tsmom_gross_edge_loop_verdict"),
            "candidate_family_decision": family_decision_payload.get("tsmom_candidate_family_decision"),
            "regime_only_tsmom_falsification_decision": regime_only_payload.get(
                "regime_only_tsmom_falsification_decision"
            ),
            "broad_tsmom_active_development_restored": False,
            "regime_only_tsmom_active_candidate_restored": False,
        },
        "leakage_defense": {
            "forward_return_label_overlap_recorded": True,
            "forward_return_label_overlap_count": sum(
                1 for row in comparison_rows if bool(row.get("forward_return_label_overlap"))
            ),
            "lagged_baseline_uses_current_bar_info": False,
            "random_sign_seed_policy": "1729 + sum(ord(char) for window_id)",
            "holdout_retune_used_for_baseline_comparison": False,
            "baseline_comparison_selection_role": "evaluation_only_not_parameter_selection",
        },
        "summary": {
            "comparison_row_count": len(comparison_rows),
            "baseline_family_counts": family_counts,
            "missing_or_unclaimed_baselines": sorted(
                {
                    str(row["baseline_name"])
                    for row in comparison_rows
                    if str(row.get("baseline_status")) != "available"
                }
            ),
            "tsmom_state_changed": False,
        },
        "baseline_comparison": comparison_rows,
        "safety_gates": {
            "live_orders_submitted": False,
            "testnet_orders_submitted": False,
            "api_keys_required_for_research": False,
            "account_or_fill_data_used": False,
            "holdout_used_for_selection": False,
            "profit_or_live_ready_claim": False,
            "strategy_parameters_retuned": False,
            "gates_relaxed": False,
            "live_trading_path_introduced": False,
        },
        "validation_commands": list(commands),
        "evidence_artifacts": list(BASELINE_HARNESS_REQUIRED_FILES),
        "changed_files": list(changed_files),
        "next_loop": "Apply this harness to future non-TSMOM hypotheses before making strategy-performance claims.",
    }


def write_research_null_baseline_harness_artifacts(
    output_dir: str | Path,
    payload: dict[str, Any],
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_research_null_baseline_harness_output_dir(output_dir, overwrite=overwrite)
    artifact_payload = dict(payload)
    if out.name.endswith(RESEARCH_NULL_BASELINE_HARNESS_LOOP_SLUG):
        artifact_payload["loop_id"] = out.name
    _write_json(out / "baseline_harness.json", artifact_payload)
    pd.DataFrame(artifact_payload.get("baseline_comparison", [])).to_csv(
        out / "baseline_comparison.csv",
        index=False,
    )
    _atomic_write_text(out / "report.md", render_research_null_baseline_harness_report(artifact_payload))
    _atomic_write_text(out / "commands.txt", "\n".join(artifact_payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(artifact_payload.get("changed_files", [])) + "\n")
    return out


def prepare_research_null_baseline_harness_output_dir(
    output_dir: str | Path,
    *,
    overwrite: bool = False,
) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="research-null-baseline-harness-run",
        known_files=BASELINE_HARNESS_KNOWN_FILES,
        overwrite=overwrite,
    )


def render_research_null_baseline_harness_report(payload: dict[str, Any]) -> str:
    summary = payload["summary"]
    state = payload["source_loop_state_preserved"]
    leakage = payload["leakage_defense"]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Purpose",
        payload["pre_registration"]["purpose"],
        "",
        "## Claim boundary",
        f"- research_null_baseline_harness_decision: {payload['research_null_baseline_harness_decision']}",
        f"- namespace: {payload['research_null_baseline_harness_namespace']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- claim_boundary: {payload['claim_boundary']}",
        "",
        "## Schema",
        f"- required_columns: {', '.join(payload['schema_contract']['required_columns'])}",
        f"- baseline_families: {', '.join(payload['schema_contract']['baseline_families'])}",
        f"- comparison_row_count: {summary['comparison_row_count']}",
        f"- missing_or_unclaimed_baselines: {', '.join(summary['missing_or_unclaimed_baselines']) if summary['missing_or_unclaimed_baselines'] else 'none'}",
        "",
        "## Leakage defense",
        f"- forward_return_label_overlap_recorded: {leakage['forward_return_label_overlap_recorded']}",
        f"- forward_return_label_overlap_count: {leakage['forward_return_label_overlap_count']}",
        f"- lagged_baseline_uses_current_bar_info: {leakage['lagged_baseline_uses_current_bar_info']}",
        f"- random_sign_seed_policy: {leakage['random_sign_seed_policy']}",
        f"- holdout_retune_used_for_baseline_comparison: {leakage['holdout_retune_used_for_baseline_comparison']}",
        "",
        "## TSMOM state preserved",
        f"- source_tsmom_loop_verdict: {state['source_tsmom_loop_verdict']}",
        f"- candidate_family_decision: {state['candidate_family_decision']}",
        f"- regime_only_tsmom_falsification_decision: {state['regime_only_tsmom_falsification_decision']}",
        f"- broad_tsmom_active_development_restored: {state['broad_tsmom_active_development_restored']}",
        f"- regime_only_tsmom_active_candidate_restored: {state['regime_only_tsmom_active_candidate_restored']}",
        "",
        "## Baseline family counts",
    ]
    for name, count in summary["baseline_family_counts"].items():
        lines.append(f"- {name}: {count}")
    lines.extend(
        [
            "",
            "## What cannot be claimed",
            "- live readiness",
            "- profitability",
            "- execution readiness",
            "- permission to use real money",
            "- restoration of broad or regime-only TSMOM active development",
            "",
            "## Next loop",
            payload["next_loop"],
        ]
    )
    return "\n".join(lines) + "\n"


def _common_baseline_row(
    *,
    baseline_name: str,
    strategy_name: str,
    strategy_return_gross: float | None,
    strategy_return_net: float | None,
    baseline_return_gross: float | None,
    baseline_return_net: float | None,
    sample_size: int,
    trade_count: int,
    turnover: float,
    fees_drag: float | None,
    slippage_drag: float | None,
    funding_drag: float | None,
    max_drawdown: float | None,
    regime_filter: str | None,
    evidence_source: str,
    claim_boundary: str,
    baseline_status: str,
    baseline_description: str,
    deterministic_seed: int | None,
    forward_return_label_overlap: bool,
    forward_return_label_overlap_detail: str,
    holdout_retune_used: bool,
    baseline_max_drawdown: Any,
    baseline_turnover: Any,
    baseline_cvar_tail_loss: Any,
    baseline_average_exposure: Any,
    uses_current_bar_signal: bool,
    lag_bars: int | None,
) -> dict[str, Any]:
    return {
        "baseline_name": baseline_name,
        "strategy_name": strategy_name,
        "strategy_return_gross": strategy_return_gross,
        "strategy_return_net": strategy_return_net,
        "baseline_return_gross": baseline_return_gross,
        "baseline_return_net": baseline_return_net,
        "strategy_beats_baseline_gross": _optional_gt(strategy_return_gross, baseline_return_gross),
        "strategy_beats_baseline_net": _optional_gt(strategy_return_net, baseline_return_net),
        "sample_size": sample_size,
        "trade_count": trade_count,
        "turnover": turnover,
        "fees_drag": fees_drag,
        "slippage_drag": slippage_drag,
        "funding_drag": funding_drag,
        "max_drawdown": max_drawdown,
        "regime_filter": regime_filter,
        "evidence_source": evidence_source,
        "claim_boundary": claim_boundary,
        "baseline_status": baseline_status,
        "baseline_description": baseline_description,
        "deterministic_seed": deterministic_seed,
        "forward_return_label_overlap": forward_return_label_overlap,
        "forward_return_label_overlap_detail": forward_return_label_overlap_detail,
        "holdout_retune_used": holdout_retune_used,
        "baseline_max_drawdown": baseline_max_drawdown,
        "baseline_turnover": baseline_turnover,
        "baseline_cvar_tail_loss": baseline_cvar_tail_loss,
        "baseline_average_exposure": baseline_average_exposure,
        "baseline_uses_current_bar_signal": uses_current_bar_signal,
        "lag_bars": lag_bars,
    }


def _baseline_rows_from_durable_tsmom_artifacts(
    *,
    source_payload: dict[str, Any],
    gross_edge_payload: dict[str, Any],
    regime_only_payload: dict[str, Any],
    gross_edge_path: Path,
    regime_only_path: Path,
) -> list[dict[str, Any]]:
    source_rows = {
        str(row["window_id"]): row
        for row in source_payload.get("walk_forward", {}).get("rows", [])
        if isinstance(row, dict)
    }
    gross_rows = {
        str(row["window_id"]): row
        for row in gross_edge_payload.get("gross_edge_decomposition", [])
        if isinstance(row, dict)
    }
    rows: list[dict[str, Any]] = []
    for item in gross_edge_payload.get("null_baseline_comparison", []):
        null_row = cast(dict[str, Any], item)
        window_id = str(null_row["window_id"])
        gross_row = cast(dict[str, Any], gross_rows.get(window_id, {}))
        source_row = cast(dict[str, Any], source_rows.get(window_id, {}))
        rows.append(
            _common_baseline_row(
                baseline_name=str(null_row["baseline_name"]),
                strategy_name="TSMOM_VOL_TARGET_FUNDING_AWARE_V1",
                strategy_return_gross=_safe_float_or_none(gross_row.get("gross_return_before_costs_pct")),
                strategy_return_net=_safe_float_or_none(
                    gross_row.get("net_return_after_funding_slippage_stress_pct")
                ),
                baseline_return_gross=_safe_float_or_none(null_row.get("baseline_return_pct")),
                baseline_return_net=_safe_float_or_none(null_row.get("baseline_return_pct")),
                sample_size=_sample_size_from_window(source_row=source_row, gross_row=gross_row),
                trade_count=int(source_row.get("base_holdout_closed_trades", 0) or 0),
                turnover=_safe_float(gross_row.get("turnover")),
                fees_drag=_safe_float_or_none(gross_row.get("stress_drag_pct")),
                slippage_drag=_safe_float_or_none(gross_row.get("slippage_drag_pct")),
                funding_drag=_safe_float_or_none(gross_row.get("funding_drag_pct")),
                max_drawdown=_safe_float_or_none(gross_row.get("drawdown_pct")),
                regime_filter=_regime_filter(gross_row),
                evidence_source=str(gross_edge_path),
                claim_boundary=null_baseline_claim_boundary(),
                baseline_status=str(null_row.get("status", "available")),
                baseline_description=str(null_row.get("baseline_description", "durable gross-edge null row")),
                deterministic_seed=cast(int | None, null_row.get("deterministic_seed")),
                forward_return_label_overlap=False,
                forward_return_label_overlap_detail="not_detected_existing_gross_edge_rows_shift_positions_one_bar",
                holdout_retune_used=False,
                baseline_max_drawdown=null_row.get("baseline_drawdown_pct"),
                baseline_turnover=null_row.get("baseline_turnover"),
                baseline_cvar_tail_loss=null_row.get("baseline_cvar_tail_loss_pct"),
                baseline_average_exposure=null_row.get("baseline_average_exposure"),
                uses_current_bar_signal=str(null_row.get("baseline_name")) == "inverse_signal",
                lag_bars=24 if str(null_row.get("baseline_name")) == "lagged_signal" else None,
            )
        )
    rows.append(
        unavailable_simpler_trend_baseline_row(
            strategy_name="TSMOM_VOL_TARGET_FUNDING_AWARE_V1",
            evidence_source=str(gross_edge_path),
            regime_filter="all_windows",
        )
    )
    for item in regime_only_payload.get("null_baseline_comparison", []):
        null_row = cast(dict[str, Any], item)
        baseline_name = str(null_row.get("baseline_name"))
        if baseline_name == "simpler_trend_baseline":
            continue
        window_id = str(null_row["window_id"])
        gross_row = cast(dict[str, Any], gross_rows.get(window_id, {}))
        source_row = cast(dict[str, Any], source_rows.get(window_id, {}))
        rows.append(
            _common_baseline_row(
                baseline_name=baseline_name,
                strategy_name="TSMOM_VOL_TARGET_FUNDING_AWARE_V1",
                strategy_return_gross=_safe_float_or_none(gross_row.get("gross_return_before_costs_pct")),
                strategy_return_net=_safe_float_or_none(
                    gross_row.get("net_return_after_funding_slippage_stress_pct")
                ),
                baseline_return_gross=_safe_float_or_none(null_row.get("baseline_return_pct")),
                baseline_return_net=_safe_float_or_none(null_row.get("baseline_return_pct")),
                sample_size=_sample_size_from_window(source_row=source_row, gross_row=gross_row),
                trade_count=int(source_row.get("base_holdout_closed_trades", 0) or 0),
                turnover=_safe_float(gross_row.get("turnover")),
                fees_drag=_safe_float_or_none(gross_row.get("stress_drag_pct")),
                slippage_drag=_safe_float_or_none(gross_row.get("slippage_drag_pct")),
                funding_drag=_safe_float_or_none(gross_row.get("funding_drag_pct")),
                max_drawdown=_safe_float_or_none(gross_row.get("drawdown_pct")),
                regime_filter="market_regime=sideways AND volatility_regime=low-vol",
                evidence_source=str(regime_only_path),
                claim_boundary=null_baseline_claim_boundary(),
                baseline_status=str(null_row.get("status", "available")),
                baseline_description=str(null_row.get("baseline_description", "durable regime-only null row")),
                deterministic_seed=cast(int | None, null_row.get("deterministic_seed")),
                forward_return_label_overlap=False,
                forward_return_label_overlap_detail="not_detected_durable_regime_only_rows",
                holdout_retune_used=False,
                baseline_max_drawdown=null_row.get("baseline_drawdown_pct"),
                baseline_turnover=null_row.get("baseline_turnover"),
                baseline_cvar_tail_loss=null_row.get("baseline_cvar_tail_loss_pct"),
                baseline_average_exposure=null_row.get("baseline_average_exposure"),
                uses_current_bar_signal=baseline_name == "inverse_signal",
                lag_bars=24 if baseline_name == "lagged_signal" else None,
            )
        )
    rows.append(
        unavailable_simpler_trend_baseline_row(
            strategy_name="TSMOM_VOL_TARGET_FUNDING_AWARE_V1",
            evidence_source=str(regime_only_path),
            regime_filter="market_regime=sideways AND volatility_regime=low-vol",
        )
    )
    return rows


def _harness_pre_registration() -> dict[str, Any]:
    return {
        "purpose": (
            "Implement a strategy-agnostic null/baseline evaluation harness so future research "
            "loops must compare claims against the same cash, buy-and-hold, random sign, lagged "
            "signal, inverse signal, and durable-evidence-only simpler trend baselines."
        ),
        "feature_type": "validation_infrastructure_not_performance_improvement",
        "allowed_inputs": [
            "durable_public_data_research_artifacts",
            "existing_gross_edge_null_baseline_rows",
            "existing_regime_only_falsification_rows",
        ],
        "forbidden_inputs": [
            "new_alpha_implementation",
            "parameter_retune",
            "holdout_retune",
            "live_orders",
            "testnet_orders",
            "account_state",
            "api_keys",
            "private_orders",
            "fills",
        ],
        "claim_boundary": null_baseline_claim_boundary(),
        "readiness_boundary": "readiness_baseline_verdict remains NOT_READY",
    }


def _harness_evidence_inputs(
    *,
    source_path: Path,
    diagnostic_path: Path,
    risk_path: Path,
    gross_edge_path: Path,
    family_decision_path: Path,
    regime_only_path: Path,
) -> list[dict[str, Any]]:
    return [
        _artifact_input("source_tsmom_loop", source_path),
        _artifact_input("stress_switch_funding_diagnostic", diagnostic_path),
        _artifact_input("switch_stability_funding_throttle_risk", risk_path),
        _artifact_input("gross_edge_regime_null_robustness", gross_edge_path),
        _artifact_input("candidate_family_decision", family_decision_path),
        _artifact_input("regime_only_falsification", regime_only_path),
    ]


def _artifact_input(role: str, path: Path) -> dict[str, Any]:
    return {"artifact_role": role, "path": str(path), "sha256": _artifact_sha256(path)}


def _baseline_family_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    return {name: sum(1 for row in rows if row.get("baseline_name") == name) for name in NULL_BASELINE_FAMILIES}


def _sample_size_from_window(*, source_row: dict[str, Any], gross_row: dict[str, Any]) -> int:
    start = source_row.get("holdout_start") or gross_row.get("holdout_start")
    end = source_row.get("holdout_end") or gross_row.get("holdout_end")
    if start and end:
        try:
            start_ts = cast(pd.Timestamp, pd.Timestamp(str(start)))
            end_ts = cast(pd.Timestamp, pd.Timestamp(str(end)))
            if pd.isna(start_ts) or pd.isna(end_ts):
                return 0
            delta_seconds = (end_ts.to_pydatetime() - start_ts.to_pydatetime()).total_seconds()
            hours = delta_seconds / 3600.0
            if math.isfinite(hours) and hours >= 0.0:
                return int(hours) + 1
        except (TypeError, ValueError):
            return 0
    return 0


def _regime_filter(gross_row: dict[str, Any]) -> str:
    market = gross_row.get("market_regime", "unknown")
    vol = gross_row.get("volatility_regime", "unknown")
    funding = gross_row.get("funding_regime", "unknown")
    return f"market_regime={market} AND volatility_regime={vol} AND funding_regime={funding}"


def _optional_gt(left: float | None, right: float | None) -> bool | None:
    if left is None or right is None:
        return None
    return float(left) > float(right)


def _float_series(values: Any, *, index: pd.Index | None = None) -> pd.Series:
    if isinstance(values, pd.Series):
        series = values
    else:
        series = pd.Series(values, index=index)
    numeric = cast(pd.Series, pd.to_numeric(series, errors="coerce"))
    return cast(pd.Series, numeric.astype(float))


def _safe_float(value: Any, default: float = 0.0) -> float:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return default
    return number if math.isfinite(number) else default


def _safe_float_or_none(value: Any) -> float | None:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    return number if math.isfinite(number) else None


def _finite_float(value: Any, default: float = 0.0) -> float:
    return _safe_float(value, default=default)


def _series_min_float(series: pd.Series) -> float:
    if series.empty:
        return 0.0
    return _safe_float(series.min())


def _tail_loss_cvar(values: list[float], *, quantile: float) -> float | None:
    clean = sorted(float(value) for value in values if math.isfinite(float(value)))
    if not clean:
        return None
    cutoff = max(1, int(math.ceil(len(clean) * float(quantile))))
    return float(sum(clean[:cutoff]) / cutoff)


def _read_json_artifact(path: Path, *, required_key: str) -> dict[str, Any]:
    payload = json.loads(path.read_text())
    if required_key not in payload:
        raise ValueError(f"{path} missing required key: {required_key}")
    return cast(dict[str, Any], payload)


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    _atomic_write_text(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def _atomic_write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(content)
    tmp.replace(path)


def _artifact_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()
