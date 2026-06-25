from __future__ import annotations

import math
import time
from dataclasses import asdict, dataclass, replace
from datetime import UTC, datetime
from typing import Any

import pandas as pd

from parangsae.core.contracts import ProductFamily
from parangsae.research.backtest_engine import (
    BacktestResult,
    BrokerSettings,
    StrategySettings,
    broker_settings_dict,
    default_risk_candidate_grid,
    risk_adjusted_score_v1,
    run_strategy_backtest,
)
from parangsae.research.regime import build_regime_report
from parangsae.research.signals import build_signal_report

READINESS_SCHEMA_VERSION = 2
COMPUTE_PROFILE_SCHEMA_VERSION = 1
VALIDATION_DEPTH_SCHEMA_VERSION = 1
VALIDATION_DEPTH_GATE = "validation_depth_research_grade"
VALIDATION_DEPTH_GRADE_ORDER = ["smoke", "prototype", "research_grade"]
READINESS_JSON_TOP_LEVEL_KEYS = [
    "schema_version",
    "method",
    "config",
    "artifact_schemas",
    "compute_profile",
    "validation_depth",
    "cost_scenarios",
    "funding_scenarios",
    "summary",
    "walk_forward",
    "baselines",
    "ablation",
    "duplicate_risk",
    "regime_split",
    "stress",
    "lookahead_checks",
    "overfit_checks",
    "live_blockers",
    "claim_boundary",
]
READINESS_WINDOWS_COLUMNS = [
    "window_id",
    "train_start",
    "train_end",
    "validation_start",
    "validation_end",
    "holdout_start",
    "holdout_end",
    "selected_candidate_id",
    "selected_fast_sma",
    "selected_slow_sma",
    "selected_risk_fraction",
    "train_score",
    "validation_score",
    "base_holdout_return_pct",
    "base_holdout_sharpe",
    "base_holdout_max_drawdown_pct",
    "base_holdout_closed_trades",
    "base_buy_hold_return_pct",
    "cash_null_return_pct",
    "stress_holdout_return_pct",
    "stress_holdout_sharpe",
    "stress_holdout_max_drawdown_pct",
    "stress_funding_rate_per_8h",
    "stress_funding_drag_pct",
    "stress_slippage_proxy_bps",
    "beats_cash_null",
    "beats_buy_hold",
    "stress_survived",
    "verdict",
]
READINESS_ABLATION_COLUMNS = [
    "family",
    "ic_24h",
    "ic_24h_without_family",
    "ic_delta_abs",
    "rows",
    "status",
]
READINESS_REGIME_COLUMNS = [
    "window_id",
    "regime_label",
    "trade_count",
    "pnl_sum",
    "return_pct_proxy",
    "avg_trade_pct",
]
READINESS_SUCCESS_FILES = {
    "manifest.json",
    "readiness.json",
    "readiness_windows.csv",
    "readiness_ablation.csv",
    "readiness_regime.csv",
    "report.md",
}
READINESS_VERDICTS = {"NOT_READY", "CONDITIONALLY_READY_FOR_PAPER_ONLY", "BLOCKED"}
CLAIM_BOUNDARY = (
    "readiness-run is a destructive public-historical research validation package, not live-fill proof, "
    "not trading advice, not a profit guarantee, and not permission for live orders"
)
METHOD_NAME = "destructive_readiness_validation_v2"
READINESS_CANDIDATE_REGISTRY_VERSION = 1
POST_REPORT_CHANGE_POLICY = (
    "A strategy or protocol change made after a NOT_READY report does not close a failed gate "
    "unless it is evaluated on untouched future/OOS data or a separately predeclared validation suite."
)
READINESS_CANDIDATE_REGISTRY = {
    "version": READINESS_CANDIDATE_REGISTRY_VERSION,
    "frozen": True,
    "families": {
        "sma_atr_bracket": "default_risk_candidate_grid(product_family, short_allowed)",
        "alpha_signal_factory_regime": "predeclared SignalCandidateSettings grid from ReadinessConfig defaults",
    },
    "selection_inputs": ["train", "validation"],
    "selection_excludes": ["holdout", "live_orders", "api_keys", "account_state"],
    "post_report_change_policy": POST_REPORT_CHANGE_POLICY,
}


@dataclass(frozen=True)
class ReadinessConfig:
    train_bars: int = 24 * 45
    validation_bars: int = 24 * 21
    holdout_bars: int = 24 * 21
    step_bars: int = 24 * 21
    min_windows: int = 3
    min_holdout_trades_each: int = 1
    max_high_corr_family_pairs: int = 6
    max_selected_candidate_switch_rate: float = 0.75
    min_regime_labels: int = 2
    stress_return_floor_pct: float = -15.0
    stress_commission: float = 0.001
    stress_spread: float = 0.001
    stress_slippage_proxy_bps: float = 5.0
    stress_funding_rate_per_8h: float = 0.0001
    funding_rate_quantile: float = 0.75
    execution_range_slippage_fraction: float = 0.01
    execution_assumed_order_notional_usd: float = 2500.0
    execution_participation_impact_bps: float = 10.0
    execution_max_slippage_proxy_bps: float = 50.0
    signal_forward_bars: int = 24
    high_corr_threshold: float = 0.8
    include_signal_strategy_candidates: bool = True
    signal_entry_threshold: float = 0.25
    signal_exit_threshold: float = 0.05
    signal_risk_off_floor: float = -0.45
    asset: str = "BTCUSDT"
    timeframe: str = "1h"
    min_research_grade_total_input_bars: int = 24 * 365 * 2
    min_research_grade_window_count: int = 12
    min_research_grade_candidate_count: int = 25
    min_research_grade_candidate_window_evaluations: int = 300
    min_research_grade_bars_processed_estimate: int = 500_000


@dataclass(frozen=True)
class WalkForwardWindow:
    window_id: str
    start: int
    train: pd.DataFrame
    validation: pd.DataFrame
    holdout: pd.DataFrame


@dataclass(frozen=True)
class SignalCandidateSettings:
    entry_threshold: float
    exit_threshold: float
    risk_off_floor: float
    risk_fraction: float
    allow_short: bool


@dataclass(frozen=True)
class ReadinessCandidateEvaluation:
    candidate_id: str
    family: str
    settings: StrategySettings | SignalCandidateSettings
    train: BacktestResult
    validation: BacktestResult
    score: float
    eligible: bool
    rejection_reasons: tuple[str, ...]


@dataclass(frozen=True)
class ReadinessReport:
    windows: pd.DataFrame
    ablation: pd.DataFrame
    regime: pd.DataFrame
    payload: dict[str, Any]
    config: ReadinessConfig = ReadinessConfig()

    def to_json_payload(self) -> dict[str, Any]:
        return self.payload


def generate_walk_forward_windows(frame: pd.DataFrame, config: ReadinessConfig = ReadinessConfig()) -> tuple[WalkForwardWindow, ...]:
    _validate_ohlcv_frame(frame)
    total = len(frame)
    span = config.train_bars + config.validation_bars + config.holdout_bars
    windows: list[WalkForwardWindow] = []
    start = 0
    index = 1
    while start + span <= total:
        train_end = start + config.train_bars
        validation_end = train_end + config.validation_bars
        holdout_end = validation_end + config.holdout_bars
        train = frame.iloc[start:train_end].copy()
        validation = frame.iloc[train_end:validation_end].copy()
        holdout = frame.iloc[validation_end:holdout_end].copy()
        if not (train.index[-1] < validation.index[0] < validation.index[-1] < holdout.index[0]):
            raise ValueError("walk-forward windows must be chronological and non-overlapping")
        windows.append(WalkForwardWindow(f"W{index:03d}", start, train, validation, holdout))
        start += config.step_bars
        index += 1
    return tuple(windows)


def funding_drag_pct(*, bars: int, exposure_time_pct: float, funding_rate_per_8h: float) -> float:
    return float(exposure_time_pct) / 100 * int(bars) / 8 * float(funding_rate_per_8h) * 100


def slippage_drag_pct(*, closed_trades: int, slippage_proxy_bps: float) -> float:
    return int(closed_trades) * 2 * float(slippage_proxy_bps) / 100


def execution_slippage_proxy_bps(
    frame: pd.DataFrame,
    *,
    closed_trades: int,
    config: ReadinessConfig = ReadinessConfig(),
) -> float:
    """Deterministic OHLCV execution-cost proxy; not exchange matching."""

    if frame.empty:
        return float(config.stress_slippage_proxy_bps)
    data = _validate_ohlcv_frame(frame)
    range_bps = ((data["High"] - data["Low"]).abs() / data["Close"].replace(0, pd.NA)).dropna() * 10000
    median_range_bps = float(range_bps.median()) if not range_bps.empty else 0.0
    median_dollar_volume = float((data["Close"].astype(float) * data["Volume"].astype(float)).median())
    participation_rate = (
        float(config.execution_assumed_order_notional_usd) / median_dollar_volume
        if median_dollar_volume > 0 and math.isfinite(median_dollar_volume)
        else 1.0
    )
    volume_participation_bps = (
        min(1.0, max(0.0, participation_rate))
        * float(config.execution_participation_impact_bps)
        * math.sqrt(max(1, int(closed_trades)))
    )
    trade_density_penalty = max(0.0, int(closed_trades) - 1) * 0.25
    dynamic = (
        float(config.stress_slippage_proxy_bps)
        + max(0.0, median_range_bps) * float(config.execution_range_slippage_fraction)
        + volume_participation_bps
        + trade_density_penalty
    )
    return min(float(config.execution_max_slippage_proxy_bps), max(float(config.stress_slippage_proxy_bps), dynamic))


def build_readiness_report(
    frame: pd.DataFrame,
    *,
    product_family: ProductFamily = ProductFamily.SPOT_CRYPTO,
    short_allowed: bool = False,
    config: ReadinessConfig = ReadinessConfig(),
    candidate_grid: tuple[StrategySettings, ...] | None = None,
    base_broker: BrokerSettings = BrokerSettings(),
    funding_frame: pd.DataFrame | None = None,
    funding_metadata: dict[str, Any] | None = None,
) -> ReadinessReport:
    started_at = time.perf_counter()
    data = _validate_ohlcv_frame(frame)
    cfg = config
    funding_data = _normalize_funding_frame(funding_frame) if funding_frame is not None else None
    grid = candidate_grid or default_risk_candidate_grid(product_family, short_allowed=short_allowed)
    candidate_count, candidate_families = _candidate_context(cfg, grid, short_allowed)
    windows = generate_walk_forward_windows(data, cfg)
    if len(windows) < cfg.min_windows:
        empty_windows = pd.DataFrame(columns=READINESS_WINDOWS_COLUMNS)
        empty_ablation = pd.DataFrame(columns=READINESS_ABLATION_COLUMNS)
        empty_regime = pd.DataFrame(columns=READINESS_REGIME_COLUMNS)
        compute_profile = _compute_profile(
            data=data,
            windows=windows,
            candidate_count=candidate_count,
            candidate_families=candidate_families,
            elapsed_seconds=time.perf_counter() - started_at,
        )
        validation_depth = _validation_depth(compute_profile, cfg)
        failed_gates = ["min_windows"]
        if not validation_depth["research_grade"]:
            failed_gates.append(VALIDATION_DEPTH_GATE)
        payload = _payload(
            config=cfg,
            windows=empty_windows,
            ablation=empty_ablation,
            regime=empty_regime,
            duplicate_risk=_empty_duplicate_risk(cfg),
            lookahead_checks={"status": "blocked", "reason": "insufficient windows"},
            overfit_checks={"selected_candidate_switch_rate": None, "status": "blocked"},
            compute_profile=compute_profile,
            validation_depth=validation_depth,
            summary=_summary(
                verdict="BLOCKED",
                config=cfg,
                windows=empty_windows,
                duplicate_risk=_empty_duplicate_risk(cfg),
                regime_labels=0,
                selected_candidate_switch_rate=None,
                lookahead_passed=False,
                failed_gates=failed_gates,
            ),
        )
        return ReadinessReport(empty_windows, empty_ablation, empty_regime, payload, cfg)

    window_records: list[dict[str, Any]] = []
    regime_records: list[dict[str, Any]] = []
    selected_ids: list[str] = []
    for window in windows:
        selected = _select_candidate(window.train, window.validation, grid, base_broker, cfg, short_allowed)
        selected_ids.append(selected.candidate_id)
        stress_broker = replace(
            base_broker,
            commission=cfg.stress_commission,
            spread=cfg.stress_spread,
        )
        base_holdout = _run_candidate_backtest(
            window.holdout,
            label=f"{window.window_id}_holdout_base",
            candidate=selected,
            broker_settings=base_broker,
            config=cfg,
        )
        stress_holdout = _run_candidate_backtest(
            window.holdout,
            label=f"{window.window_id}_holdout_stress",
            candidate=selected,
            broker_settings=stress_broker,
            config=cfg,
        )
        stress_return = _metric_float(stress_holdout.metrics, "Return_pct")
        funding_rate_used = _window_funding_rate_per_8h(
            funding_data,
            window.holdout.index[0],
            window.holdout.index[-1],
            config=cfg,
        )
        funding_drag = funding_drag_pct(
            bars=len(window.holdout),
            exposure_time_pct=_metric_float(stress_holdout.metrics, "Exposure_Time_pct"),
            funding_rate_per_8h=funding_rate_used,
        )
        slippage_proxy_bps = execution_slippage_proxy_bps(
            window.holdout,
            closed_trades=int(stress_holdout.metrics.get("closed_trades", 0) or 0),
            config=cfg,
        )
        slip_drag = slippage_drag_pct(
            closed_trades=int(stress_holdout.metrics.get("closed_trades", 0) or 0),
            slippage_proxy_bps=slippage_proxy_bps,
        )
        adjusted_stress_return = stress_return - funding_drag - slip_drag
        buy_hold, buy_hold_source = _buy_hold_return_pct_and_source(window.holdout, base_holdout.metrics)
        beats_cash = adjusted_stress_return > 0.0
        beats_buy_hold = adjusted_stress_return > buy_hold
        stress_survived = adjusted_stress_return >= cfg.stress_return_floor_pct
        closed_trades = int(base_holdout.metrics.get("closed_trades", 0) or 0)
        window_records.append(
            {
                "window_id": window.window_id,
                "train_start": _timestamp(window.train.index[0]),
                "train_end": _timestamp(window.train.index[-1]),
                "validation_start": _timestamp(window.validation.index[0]),
                "validation_end": _timestamp(window.validation.index[-1]),
                "holdout_start": _timestamp(window.holdout.index[0]),
                "holdout_end": _timestamp(window.holdout.index[-1]),
                "selected_candidate_id": selected.candidate_id,
                "selected_fast_sma": selected.settings.fast_sma if isinstance(selected.settings, StrategySettings) else None,
                "selected_slow_sma": selected.settings.slow_sma if isinstance(selected.settings, StrategySettings) else None,
                "selected_risk_fraction": selected.settings.risk_fraction,
                "train_score": _metric_float(selected.train.metrics, "Sharpe_Ratio"),
                "validation_score": selected.score,
                "base_holdout_return_pct": _metric_float(base_holdout.metrics, "Return_pct"),
                "base_holdout_sharpe": _metric_float(base_holdout.metrics, "Sharpe_Ratio"),
                "base_holdout_max_drawdown_pct": _metric_float(base_holdout.metrics, "Max_Drawdown_pct"),
                "base_holdout_closed_trades": closed_trades,
                "base_buy_hold_return_pct": buy_hold,
                "_buy_hold_source": buy_hold_source,
                "cash_null_return_pct": 0.0,
                "stress_holdout_return_pct": adjusted_stress_return,
                "stress_holdout_sharpe": _metric_float(stress_holdout.metrics, "Sharpe_Ratio"),
                "stress_holdout_max_drawdown_pct": _metric_float(stress_holdout.metrics, "Max_Drawdown_pct"),
                "stress_funding_rate_per_8h": funding_rate_used,
                "stress_funding_drag_pct": funding_drag,
                "stress_slippage_proxy_bps": slippage_proxy_bps,
                "beats_cash_null": bool(beats_cash),
                "beats_buy_hold": bool(beats_buy_hold),
                "stress_survived": bool(stress_survived),
                "verdict": "PASS" if closed_trades >= cfg.min_holdout_trades_each and stress_survived else "FAIL",
            }
        )
        regime_records.extend(_regime_records_for_window(window, base_holdout))

    window_internal = pd.DataFrame.from_records(window_records)
    buy_hold_sources = tuple(str(value) for value in window_internal.get("_buy_hold_source", pd.Series(dtype=str)).unique())
    window_rows = window_internal.reindex(columns=READINESS_WINDOWS_COLUMNS)
    regime_rows = pd.DataFrame.from_records(regime_records, columns=READINESS_REGIME_COLUMNS)
    full_regime = build_regime_report(data)
    signal_report = build_signal_report(
        data,
        regimes=full_regime.rows["regime_label"],
        asset=cfg.asset,
        timeframe=cfg.timeframe,
    )
    ablation_rows, duplicate_risk = _ablation_and_duplicate_risk(data, signal_report.rows, cfg)
    lookahead_checks = _lookahead_checks(data, cfg)
    selected_candidate_switch_rate = _selected_candidate_switch_rate(selected_ids)
    regime_label_count = int(regime_rows["regime_label"].nunique()) if not regime_rows.empty else 0
    compute_profile = _compute_profile(
        data=data,
        windows=windows,
        candidate_count=candidate_count,
        candidate_families=candidate_families,
        elapsed_seconds=time.perf_counter() - started_at,
    )
    validation_depth = _validation_depth(compute_profile, cfg)
    failed_gates = _failed_gates(
        config=cfg,
        windows=window_rows,
        duplicate_risk=duplicate_risk,
        regime_label_count=regime_label_count,
        selected_candidate_switch_rate=selected_candidate_switch_rate,
        lookahead_checks=lookahead_checks,
        validation_depth=validation_depth,
    )
    verdict = "CONDITIONALLY_READY_FOR_PAPER_ONLY" if not failed_gates else "NOT_READY"
    summary = _summary(
        verdict=verdict,
        config=cfg,
        windows=window_rows,
        duplicate_risk=duplicate_risk,
        regime_labels=regime_label_count,
        selected_candidate_switch_rate=selected_candidate_switch_rate,
        lookahead_passed=bool(lookahead_checks.get("passed")),
        failed_gates=failed_gates,
    )
    payload = _payload(
        config=cfg,
        windows=window_rows,
        ablation=ablation_rows,
        regime=regime_rows,
        duplicate_risk=duplicate_risk,
        lookahead_checks=lookahead_checks,
        overfit_checks={
            "selected_candidate_switch_rate": selected_candidate_switch_rate,
            "max_selected_candidate_switch_rate": cfg.max_selected_candidate_switch_rate,
            "status": "passed" if selected_candidate_switch_rate <= cfg.max_selected_candidate_switch_rate else "failed",
            "candidate_registry_version": READINESS_CANDIDATE_REGISTRY_VERSION,
            "selection_uses_holdout": False,
            "selector_static_contract": "_select_candidate(train, validation, candidate_grid, broker_settings, config, short_allowed)",
            "post_report_change_policy": POST_REPORT_CHANGE_POLICY,
        },
        compute_profile=compute_profile,
        validation_depth=validation_depth,
        summary=summary,
        buy_hold_source=buy_hold_sources[0] if len(buy_hold_sources) == 1 else "close_to_close_fallback",
        funding_frame=funding_data,
        funding_metadata=funding_metadata,
    )
    return ReadinessReport(window_rows, ablation_rows, regime_rows, payload, cfg)


def _payload(
    *,
    config: ReadinessConfig,
    windows: pd.DataFrame,
    ablation: pd.DataFrame,
    regime: pd.DataFrame,
    duplicate_risk: dict[str, Any],
    lookahead_checks: dict[str, Any],
    overfit_checks: dict[str, Any],
    compute_profile: dict[str, Any],
    validation_depth: dict[str, Any],
    summary: dict[str, Any],
    buy_hold_source: str = "backtesting_metric",
    funding_frame: pd.DataFrame | None = None,
    funding_metadata: dict[str, Any] | None = None,
) -> dict[str, Any]:
    funding_scenario = _funding_scenario_payload(config, funding_frame, funding_metadata)
    execution_proxy = _execution_proxy_payload(config)
    data_coverage = _data_coverage_payload(funding_frame, funding_metadata)
    return {
        "schema_version": READINESS_SCHEMA_VERSION,
        "method": {
            "name": METHOD_NAME,
            "causal_no_lookahead": True,
            "inputs": ["Open", "High", "Low", "Close", "Volume"],
            "selection_uses_holdout": False,
            "live_orders_submitted": False,
            "api_keys_required": False,
            "schema_version_policy": (
                "schema_version 2 adds compute_profile and validation_depth; breaking key, "
                "column, formula, or verdict changes require another version bump and migration note"
            ),
            "validation_depth_policy": (
                "elapsed_seconds is diagnostic only; research-grade claims require workload "
                "thresholds for bars, windows, candidates, and candidate-window evaluations"
            ),
            "candidate_families": (
                ["sma_atr_bracket", "alpha_signal_factory_regime"]
                if config.include_signal_strategy_candidates
                else ["sma_atr_bracket"]
            ),
            "candidate_registry": READINESS_CANDIDATE_REGISTRY,
            "data_coverage": data_coverage,
        },
        "config": asdict(config),
        "artifact_schemas": {
            "readiness_windows.csv": READINESS_WINDOWS_COLUMNS,
            "readiness_ablation.csv": READINESS_ABLATION_COLUMNS,
            "readiness_regime.csv": READINESS_REGIME_COLUMNS,
            "readiness.json": READINESS_JSON_TOP_LEVEL_KEYS,
        },
        "compute_profile": compute_profile,
        "validation_depth": validation_depth,
        "cost_scenarios": {
            "base": broker_settings_dict(),
            "stress": {
                "commission": config.stress_commission,
                "spread": config.stress_spread,
                "slippage_proxy_bps": config.stress_slippage_proxy_bps,
                "dynamic_slippage_proxy": execution_proxy,
                "slippage_formula": "closed_trades * 2 * dynamic_slippage_proxy_bps / 100 percentage points",
            },
        },
        "funding_scenarios": funding_scenario,
        "summary": summary,
        "walk_forward": {
            "window_count": int(len(windows)),
            "rows": _records(windows),
        },
        "baselines": {
            "cash_null_return_pct": 0.0,
            "buy_hold_source": buy_hold_source,
            "beats_cash_null_formula": "stress_holdout_return_pct > 0.0",
            "beats_buy_hold_formula": "stress_holdout_return_pct > base_buy_hold_return_pct",
        },
        "ablation": {
            "forward_bars": config.signal_forward_bars,
            "method": "family-score Pearson IC against 24-bar forward returns, then drop-one-family recomputation",
            "rows": _records(ablation),
        },
        "duplicate_risk": duplicate_risk,
        "regime_split": {
            "regime_label_count": int(regime["regime_label"].nunique()) if not regime.empty else 0,
            "rows": _records(regime),
        },
        "stress": {
            "median_stress_holdout_return_pct": _median(windows, "stress_holdout_return_pct"),
            "stress_return_floor_pct": config.stress_return_floor_pct,
            "all_windows_survived": bool(windows["stress_survived"].all()) if not windows.empty else False,
            "execution_proxy": execution_proxy,
        },
        "lookahead_checks": lookahead_checks,
        "overfit_checks": overfit_checks,
        "live_blockers": [
            "No live/testnet order path was validated or authorized.",
            "No exchange matching or live-fill proof exists; OHLCV execution proxy is not fill proof.",
            _funding_blocker_text(funding_frame),
            "Catalog-only orderbook/on-chain/macro/sentiment signals remain inactive without configured data-source validation.",
            "Passing this report can at most justify further paper-only validation, never live trading.",
        ],
        "claim_boundary": CLAIM_BOUNDARY,
    }


def _funding_scenario_payload(
    config: ReadinessConfig,
    funding_frame: pd.DataFrame | None,
    funding_metadata: dict[str, Any] | None,
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "stress_funding_rate_per_8h": config.stress_funding_rate_per_8h,
        "funding_formula": "exposure_time_pct / 100 * bars / 8 * funding_rate_per_8h * 100",
        "rate_selection": (
            "per-window max(default stress rate, observed public funding "
            f"quantile {config.funding_rate_quantile:g}) when public funding is supplied"
        ),
    }
    if funding_frame is None or funding_frame.empty:
        payload["source"] = "sensitivity_assumption_not_exchange_download"
        return payload
    rates = pd.to_numeric(funding_frame["funding_rate"], errors="coerce").dropna()
    payload.update(
        {
            "source": "public_binance_usdm_funding_history",
            "rows": int(len(funding_frame)),
            "first_timestamp": _timestamp(funding_frame.index[0]),
            "last_timestamp": _timestamp(funding_frame.index[-1]),
            "mean_funding_rate": float(rates.mean()),
            "median_funding_rate": float(rates.median()),
            "max_funding_rate": float(rates.max()),
            "metadata": funding_metadata or {},
        }
    )
    return payload


def _execution_proxy_payload(config: ReadinessConfig) -> dict[str, Any]:
    return {
        "source": "ohlcv_range_trade_count_proxy",
        "base_slippage_proxy_bps": config.stress_slippage_proxy_bps,
        "range_slippage_fraction": config.execution_range_slippage_fraction,
        "assumed_order_notional_usd": config.execution_assumed_order_notional_usd,
        "participation_impact_bps": config.execution_participation_impact_bps,
        "max_slippage_proxy_bps": config.execution_max_slippage_proxy_bps,
        "not_exchange_matching": True,
        "not_live_fill_proof": True,
        "formula": (
            "min(max_bps, max(base_bps, base_bps + median((High-Low)/Close)*10000"
            "*range_fraction + min(1, assumed_order_notional_usd/median(Close*Volume))"
            "*participation_impact_bps*sqrt(max(1, closed_trades)) + max(0, closed_trades-1)*0.25))"
        ),
    }


def _data_coverage_payload(
    funding_frame: pd.DataFrame | None,
    funding_metadata: dict[str, Any] | None,
) -> dict[str, Any]:
    funding_status = "validated_public_data" if funding_frame is not None and not funding_frame.empty else "sensitivity_proxy_only"
    return {
        "ohlcv": {"status": "validated_public_data", "source": "Binance Vision monthly klines"},
        "derivatives_funding": {
            "status": funding_status,
            "source": (funding_metadata or {}).get("source_url") or "not_downloaded",
            "rows": int(len(funding_frame)) if funding_frame is not None else 0,
        },
        "derivatives_open_interest": {"status": "catalog_only_not_configured"},
        "orderbook_orderflow": {"status": "blocked_requires_provider"},
        "on_chain": {"status": "blocked_requires_provider"},
        "options": {"status": "blocked_requires_provider"},
        "macro_events": {"status": "blocked_requires_provider"},
        "sentiment_news": {"status": "blocked_requires_provider"},
        "execution": {"status": "proxy_only_no_exchange_matching"},
    }


def _funding_blocker_text(funding_frame: pd.DataFrame | None) -> str:
    if funding_frame is not None and not funding_frame.empty:
        return "Funding history is downloaded from public Binance USDⓈ-M data, but account-specific funding cashflows are still not validated."
    return "Funding is a sensitivity assumption, not downloaded account/exchange funding data."


def _candidate_context(
    config: ReadinessConfig,
    grid: tuple[StrategySettings, ...],
    short_allowed: bool,
) -> tuple[int, list[str]]:
    families = ["sma_atr_bracket"]
    candidate_count = len(grid)
    if config.include_signal_strategy_candidates:
        families.append("alpha_signal_factory_regime")
        candidate_count += len(_signal_candidate_grid(config, short_allowed))
    return int(candidate_count), families


def _compute_profile(
    *,
    data: pd.DataFrame,
    windows: tuple[WalkForwardWindow, ...],
    candidate_count: int,
    candidate_families: list[str],
    elapsed_seconds: float,
) -> dict[str, Any]:
    input_start = _timestamp(data.index[0]) if not data.empty else None
    input_end = _timestamp(data.index[-1]) if not data.empty else None
    span_days = None
    if len(data.index) >= 2:
        span_days = float((data.index[-1] - data.index[0]).total_seconds() / 86400)
    candidate_window_evaluations = int(candidate_count * len(windows))
    selection_evaluations = int(candidate_count * 2 * len(windows))
    selected_holdout_evaluations = int(2 * len(windows))
    bars_processed_estimate = int(
        sum(candidate_count * (len(window.train) + len(window.validation)) + 2 * len(window.holdout) for window in windows)
    )
    return {
        "schema_version": COMPUTE_PROFILE_SCHEMA_VERSION,
        "profiled_at": _utc_now_iso(),
        "elapsed_seconds": round(float(elapsed_seconds), 6),
        "elapsed_seconds_role": (
            "diagnostic only; a fast run can be valid for smoke checks but is not proof of research-grade validation"
        ),
        "engine": "vectorized_or_library_backtest_loop_with_predeclared_candidate_grid",
        "total_input_bars": int(len(data)),
        "input_start": input_start,
        "input_end": input_end,
        "input_span_days": span_days,
        "window_count": int(len(windows)),
        "candidate_count": int(candidate_count),
        "candidate_families": list(candidate_families),
        "candidate_window_evaluations": candidate_window_evaluations,
        "candidate_selection_evaluations": selection_evaluations,
        "selected_holdout_evaluations": selected_holdout_evaluations,
        "total_strategy_evaluations": int(selection_evaluations + selected_holdout_evaluations),
        "bars_processed_estimate": bars_processed_estimate,
        "bars_processed_formula": (
            "sum(candidate_count * (train_bars + validation_bars) + 2 * holdout_bars for each generated window)"
        ),
        "profile_notes": [
            "Selection evaluates each candidate on train and validation only; holdout is evaluated only for the selected candidate.",
            "Wall-clock speed depends on vectorization, machine speed, caching, and I/O; it must not be used as a pass/fail gate.",
            "Research-grade validation is gated by validation_depth thresholds, not by deliberately slowing computation.",
        ],
    }


def _validation_depth(profile: dict[str, Any], config: ReadinessConfig) -> dict[str, Any]:
    observed = {
        "total_input_bars": int(profile.get("total_input_bars") or 0),
        "window_count": int(profile.get("window_count") or 0),
        "candidate_count": int(profile.get("candidate_count") or 0),
        "candidate_window_evaluations": int(profile.get("candidate_window_evaluations") or 0),
        "bars_processed_estimate": int(profile.get("bars_processed_estimate") or 0),
    }
    thresholds = _validation_depth_thresholds(config)
    failed_checks = [key for key, threshold in thresholds.items() if observed.get(key, 0) < threshold]
    research_grade = not failed_checks
    if observed["window_count"] < config.min_windows or observed["candidate_count"] < 2:
        grade = "smoke"
    elif research_grade:
        grade = "research_grade"
    else:
        grade = "prototype"
    return {
        "schema_version": VALIDATION_DEPTH_SCHEMA_VERSION,
        "grade": grade,
        "grade_order": list(VALIDATION_DEPTH_GRADE_ORDER),
        "research_grade": research_grade,
        "gate": VALIDATION_DEPTH_GATE,
        "observed": observed,
        "thresholds": thresholds,
        "failed_depth_checks": failed_checks,
        "classification_reason": _validation_depth_reason(grade, failed_checks),
        "wall_clock_policy": "elapsed_seconds is diagnostic only and never upgraded to a readiness pass/fail criterion",
    }


def _validation_depth_reason(grade: str, failed_checks: list[str]) -> str:
    if grade == "research_grade":
        return "all configured validation-depth workload thresholds passed"
    if grade == "smoke":
        return "insufficient windows or candidate coverage for more than a fast smoke check"
    return f"prototype coverage only; failed depth checks: {', '.join(failed_checks)}"


def _validation_depth_thresholds(config: ReadinessConfig) -> dict[str, int]:
    return {
        "total_input_bars": config.min_research_grade_total_input_bars,
        "window_count": config.min_research_grade_window_count,
        "candidate_count": config.min_research_grade_candidate_count,
        "candidate_window_evaluations": config.min_research_grade_candidate_window_evaluations,
        "bars_processed_estimate": config.min_research_grade_bars_processed_estimate,
    }


def _utc_now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def _summary(
    *,
    verdict: str,
    config: ReadinessConfig,
    windows: pd.DataFrame,
    duplicate_risk: dict[str, Any],
    regime_labels: int,
    selected_candidate_switch_rate: float | None,
    lookahead_passed: bool,
    failed_gates: list[str],
) -> dict[str, Any]:
    if verdict not in READINESS_VERDICTS:
        raise ValueError(f"invalid readiness verdict: {verdict}")
    return {
        "verdict": verdict,
        "failed_gates": failed_gates,
        "gate_thresholds": _gate_thresholds(config),
        "window_count": int(len(windows)),
        "median_stress_holdout_return_pct": _median(windows, "stress_holdout_return_pct"),
        "median_base_holdout_return_pct": _median(windows, "base_holdout_return_pct"),
        "all_holdouts_have_trades": bool((windows["base_holdout_closed_trades"] >= config.min_holdout_trades_each).all()) if not windows.empty else False,
        "cash_null_return_pct": 0.0,
        "regime_label_count": regime_labels,
        "duplicate_risk": duplicate_risk,
        "selected_candidate_switch_rate": selected_candidate_switch_rate,
        "lookahead_passed": lookahead_passed,
        "claim_boundary": CLAIM_BOUNDARY,
    }


def _gate_thresholds(config: ReadinessConfig) -> dict[str, Any]:
    return {
        "min_windows": config.min_windows,
        "min_holdout_trades_each": config.min_holdout_trades_each,
        "max_high_corr_family_pairs": config.max_high_corr_family_pairs,
        "max_selected_candidate_switch_rate": config.max_selected_candidate_switch_rate,
        "min_regime_labels": config.min_regime_labels,
        "stress_return_floor_pct": config.stress_return_floor_pct,
        VALIDATION_DEPTH_GATE: _validation_depth_thresholds(config),
    }


def _failed_gates(
    *,
    config: ReadinessConfig,
    windows: pd.DataFrame,
    duplicate_risk: dict[str, Any],
    regime_label_count: int,
    selected_candidate_switch_rate: float,
    lookahead_checks: dict[str, Any],
    validation_depth: dict[str, Any],
) -> list[str]:
    failed: list[str] = []
    if len(windows) < config.min_windows:
        failed.append("min_windows")
    if windows.empty or not (windows["base_holdout_closed_trades"] >= config.min_holdout_trades_each).all():
        failed.append("min_holdout_trades_each")
    if _median(windows, "stress_holdout_return_pct") <= 0.0:
        failed.append("median_stress_return_beats_cash_null")
    if _median(windows, "stress_holdout_return_pct") < config.stress_return_floor_pct:
        failed.append("stress_return_floor_pct")
    if regime_label_count < config.min_regime_labels:
        failed.append("min_regime_labels")
    if int(duplicate_risk["high_corr_family_pair_count"]) > config.max_high_corr_family_pairs:
        failed.append("max_high_corr_family_pairs")
    if selected_candidate_switch_rate > config.max_selected_candidate_switch_rate:
        failed.append("max_selected_candidate_switch_rate")
    if not bool(lookahead_checks.get("passed")):
        failed.append("lookahead_checks")
    if not bool(validation_depth.get("research_grade")):
        failed.append(VALIDATION_DEPTH_GATE)
    return failed


def _select_candidate(
    train: pd.DataFrame,
    validation: pd.DataFrame,
    candidate_grid: tuple[StrategySettings, ...],
    broker_settings: BrokerSettings,
    config: ReadinessConfig,
    short_allowed: bool,
) -> ReadinessCandidateEvaluation:
    evaluations: list[ReadinessCandidateEvaluation] = []
    for index, settings in enumerate(candidate_grid, start=1):
        candidate_id = _candidate_id(index, settings)
        train_result = run_strategy_backtest(
            train,
            label=f"{candidate_id}_train",
            strategy_settings=settings,
            broker_settings=broker_settings,
        )
        validation_result = run_strategy_backtest(
            validation,
            label=f"{candidate_id}_validation",
            strategy_settings=settings,
            broker_settings=broker_settings,
        )
        reasons = []
        if int(train_result.metrics.get("closed_trades", 0) or 0) < 1:
            reasons.append("train_closed_trades_below_minimum")
        if int(validation_result.metrics.get("closed_trades", 0) or 0) < 1:
            reasons.append("validation_closed_trades_below_minimum")
        evaluations.append(
            ReadinessCandidateEvaluation(
                candidate_id=candidate_id,
                family="sma_atr_bracket",
                settings=settings,
                train=train_result,
                validation=validation_result,
                score=risk_adjusted_score_v1(train_result.metrics, validation_result.metrics),
                eligible=not reasons,
                rejection_reasons=tuple(reasons),
            )
        )
    if config.include_signal_strategy_candidates:
        for index, settings in enumerate(_signal_candidate_grid(config, short_allowed), start=1):
            candidate_id = _signal_candidate_id(index, settings)
            train_result = _run_signal_strategy_backtest(
                train,
                label=f"{candidate_id}_train",
                settings=settings,
                broker_settings=broker_settings,
                config=config,
            )
            validation_result = _run_signal_strategy_backtest(
                validation,
                label=f"{candidate_id}_validation",
                settings=settings,
                broker_settings=broker_settings,
                config=config,
            )
            reasons = []
            if int(train_result.metrics.get("closed_trades", 0) or 0) < 1:
                reasons.append("train_closed_trades_below_minimum")
            if int(validation_result.metrics.get("closed_trades", 0) or 0) < 1:
                reasons.append("validation_closed_trades_below_minimum")
            evaluations.append(
                ReadinessCandidateEvaluation(
                    candidate_id=candidate_id,
                    family="alpha_signal_factory_regime",
                    settings=settings,
                    train=train_result,
                    validation=validation_result,
                    score=risk_adjusted_score_v1(train_result.metrics, validation_result.metrics),
                    eligible=not reasons,
                    rejection_reasons=tuple(reasons),
                )
            )
    eligible = [candidate for candidate in evaluations if candidate.eligible]
    candidates = eligible or evaluations
    return sorted(
        candidates,
        key=lambda candidate: (
            candidate.score,
            -abs(_metric_float(candidate.validation.metrics, "Max_Drawdown_pct")),
            int(candidate.validation.metrics.get("closed_trades", 0) or 0),
        ),
        reverse=True,
    )[0]


def _run_candidate_backtest(
    frame: pd.DataFrame,
    *,
    label: str,
    candidate: ReadinessCandidateEvaluation,
    broker_settings: BrokerSettings,
    config: ReadinessConfig,
) -> BacktestResult:
    if isinstance(candidate.settings, StrategySettings):
        return run_strategy_backtest(
            frame,
            label=label,
            strategy_settings=candidate.settings,
            broker_settings=broker_settings,
        )
    return _run_signal_strategy_backtest(
        frame,
        label=label,
        settings=candidate.settings,
        broker_settings=broker_settings,
        config=config,
    )


def _candidate_id(index: int, settings: StrategySettings) -> str:
    direction = "short-ok" if settings.allow_short else "long-only"
    return f"c{index:03d}-f{settings.fast_sma}-s{settings.slow_sma}-{direction}"


def _signal_candidate_grid(config: ReadinessConfig, short_allowed: bool) -> tuple[SignalCandidateSettings, ...]:
    return (
        SignalCandidateSettings(
            entry_threshold=config.signal_entry_threshold,
            exit_threshold=config.signal_exit_threshold,
            risk_off_floor=config.signal_risk_off_floor,
            risk_fraction=0.25,
            allow_short=False,
        ),
        SignalCandidateSettings(
            entry_threshold=max(0.15, config.signal_entry_threshold - 0.05),
            exit_threshold=config.signal_exit_threshold,
            risk_off_floor=config.signal_risk_off_floor,
            risk_fraction=0.40,
            allow_short=short_allowed,
        ),
    )


def _signal_candidate_id(index: int, settings: SignalCandidateSettings) -> str:
    direction = "short-ok" if settings.allow_short else "long-only"
    return f"s{index:03d}-signal-th{settings.entry_threshold:g}-{direction}"


def _run_signal_strategy_backtest(
    frame: pd.DataFrame,
    *,
    label: str,
    settings: SignalCandidateSettings,
    broker_settings: BrokerSettings,
    config: ReadinessConfig,
) -> BacktestResult:
    data = _validate_ohlcv_frame(frame)
    regime = build_regime_report(data).rows
    signal_rows = build_signal_report(
        data,
        regimes=regime["regime_label"],
        asset=config.asset,
        timeframe=config.timeframe,
    ).rows
    score = _signal_score_series(signal_rows).reindex(data.index).fillna(0.0).shift(1).fillna(0.0)
    risk_multiplier = pd.to_numeric(regime["risk_multiplier"], errors="coerce").reindex(data.index).fillna(0.0).shift(1).fillna(0.0)
    close = data["Close"].astype(float)
    returns = close.pct_change().fillna(0.0)

    positions: list[float] = []
    current = 0.0
    open_trade: dict[str, Any] | None = None
    trades: list[dict[str, Any]] = []
    equity_values: list[float] = []
    strategy_returns: list[float] = []
    equity = float(broker_settings.cash)

    for timestamp, bar_return in returns.items():
        desired = current
        signal_value = float(score.loc[timestamp])
        risk_value = float(risk_multiplier.loc[timestamp])
        if risk_value <= 0 or signal_value <= settings.risk_off_floor:
            desired = 0.0
        elif current == 0.0:
            if signal_value >= settings.entry_threshold:
                desired = settings.risk_fraction * min(1.0, max(0.0, risk_value))
            elif settings.allow_short and signal_value <= -settings.entry_threshold:
                desired = -settings.risk_fraction * min(1.0, max(0.0, risk_value))
        elif current > 0 and signal_value < settings.exit_threshold:
            desired = 0.0
        elif current < 0 and signal_value > -settings.exit_threshold:
            desired = 0.0

        turnover = abs(desired - current)
        trade_cost = turnover * (float(broker_settings.commission) + float(broker_settings.spread))
        period_return = current * float(bar_return) - trade_cost
        equity *= 1 + period_return
        strategy_returns.append(period_return)
        equity_values.append(equity)

        if desired != current:
            if open_trade is not None:
                entry_price = float(open_trade["entry_price"])
                exit_price = float(close.loc[timestamp])
                direction = float(open_trade["direction"])
                ret = direction * (exit_price / entry_price - 1)
                trades.append(
                    {
                        "EntryTime": open_trade["entry_time"],
                        "ExitTime": timestamp,
                        "PnL": float(ret * broker_settings.cash * abs(open_trade["position"])),
                        "ReturnPct": float(ret * 100),
                    }
                )
                open_trade = None
            if desired != 0:
                open_trade = {
                    "entry_time": timestamp,
                    "entry_price": float(close.loc[timestamp]),
                    "direction": 1.0 if desired > 0 else -1.0,
                    "position": desired,
                }
        current = desired
        positions.append(current)

    if open_trade is not None and broker_settings.finalize_trades:
        timestamp = data.index[-1]
        entry_price = float(open_trade["entry_price"])
        exit_price = float(close.loc[timestamp])
        direction = float(open_trade["direction"])
        ret = direction * (exit_price / entry_price - 1)
        trades.append(
            {
                "EntryTime": open_trade["entry_time"],
                "ExitTime": timestamp,
                "PnL": float(ret * broker_settings.cash * abs(open_trade["position"])),
                "ReturnPct": float(ret * 100),
            }
        )

    return_pct = (equity / float(broker_settings.cash) - 1) * 100
    returns_series = pd.Series(strategy_returns, index=data.index, dtype=float)
    sharpe = _annualized_sharpe(returns_series)
    equity_frame = pd.DataFrame({"Equity": equity_values}, index=data.index)
    drawdown = equity_frame["Equity"] / equity_frame["Equity"].cummax() - 1
    equity_frame["DrawdownPct"] = drawdown * 100
    trades_frame = pd.DataFrame.from_records(trades)
    positions_series = pd.Series(positions, index=data.index, dtype=float)
    buy_hold = _close_to_close_return_pct(data)
    metrics = {
        "label": label,
        "closed_trades": int(len(trades_frame)),
        "Return_pct": float(return_pct),
        "Sharpe_Ratio": sharpe,
        "Max_Drawdown_pct": float(equity_frame["DrawdownPct"].min()) if not equity_frame.empty else 0.0,
        "Buy_&_Hold_Return_pct": buy_hold,
        "Exposure_Time_pct": float((positions_series.abs() > 0).mean() * 100),
    }
    return BacktestResult(label=label, metrics=metrics, trades=trades_frame, equity=equity_frame)


def _signal_score_series(signal_rows: pd.DataFrame) -> pd.Series:
    if signal_rows.empty:
        return pd.Series(dtype=float)
    family_role = (
        signal_rows.groupby([signal_rows.index, "role", "family"])["weighted_score"].mean().reset_index()
    )
    by_role = family_role.groupby(["Timestamp", "role"])["weighted_score"].mean().unstack(fill_value=0.0)
    for role in ("direction", "entry", "risk", "exit"):
        if role not in by_role:
            by_role[role] = 0.0
    score = (
        by_role["direction"] * 0.45
        + by_role["entry"] * 0.25
        + by_role["risk"] * 0.20
        + by_role["exit"] * 0.10
    )
    return score.clip(lower=-1.0, upper=1.0)


def _annualized_sharpe(returns: pd.Series) -> float:
    clean = pd.to_numeric(returns, errors="coerce").dropna()
    if len(clean) < 3:
        return 0.0
    std = float(clean.std())
    if std <= 0 or not math.isfinite(std):
        return 0.0
    value = float(clean.mean() / std * math.sqrt(24 * 365))
    return value if math.isfinite(value) else 0.0


def _regime_records_for_window(window: WalkForwardWindow, holdout: BacktestResult) -> list[dict[str, Any]]:
    if holdout.trades.empty:
        return []
    regime = build_regime_report(window.holdout).rows
    rows: list[dict[str, Any]] = []
    trades = holdout.trades.copy()
    for label, group in trades.groupby(_trade_regime_labels(trades, regime), sort=True):
        pnl_sum = float(pd.to_numeric(group.get("PnL", pd.Series(dtype=float)), errors="coerce").fillna(0).sum())
        trade_pct = pd.to_numeric(group.get("ReturnPct", group.get("Return [%]", pd.Series(dtype=float))), errors="coerce")
        rows.append(
            {
                "window_id": window.window_id,
                "regime_label": str(label),
                "trade_count": int(len(group)),
                "pnl_sum": pnl_sum,
                "return_pct_proxy": float(trade_pct.fillna(0).sum()) if not trade_pct.empty else 0.0,
                "avg_trade_pct": float(trade_pct.mean()) if not trade_pct.empty and not pd.isna(trade_pct.mean()) else 0.0,
            }
        )
    return rows


def _trade_regime_labels(trades: pd.DataFrame, regime: pd.DataFrame) -> pd.Series:
    labels: list[str] = []
    for _, trade in trades.iterrows():
        entry_time = trade.get("EntryTime")
        try:
            ts = pd.Timestamp(entry_time)
            if ts.tzinfo is None:
                ts = ts.tz_localize("UTC")
            else:
                ts = ts.tz_convert("UTC")
            prior = regime.loc[:ts]
            labels.append(str(prior["regime_label"].iloc[-1]) if not prior.empty else "unknown")
        except Exception:
            labels.append("unknown")
    return pd.Series(labels, index=trades.index, dtype="object")


def _ablation_and_duplicate_risk(
    frame: pd.DataFrame,
    signal_rows: pd.DataFrame,
    config: ReadinessConfig,
) -> tuple[pd.DataFrame, dict[str, Any]]:
    if signal_rows.empty:
        return pd.DataFrame(columns=READINESS_ABLATION_COLUMNS), _empty_duplicate_risk(config)
    family_scores = signal_rows.pivot_table(
        index=signal_rows.index,
        columns="family",
        values="weighted_score",
        aggfunc="mean",
    ).sort_index()
    forward_return = frame["Close"].shift(-config.signal_forward_bars) / frame["Close"] - 1
    aggregate = family_scores.mean(axis=1)
    base_ic = _corr(aggregate, forward_return)
    records: list[dict[str, Any]] = []
    for family in sorted(str(column) for column in family_scores.columns):
        without = family_scores.drop(columns=[family]).mean(axis=1) if len(family_scores.columns) > 1 else pd.Series(index=family_scores.index, dtype=float)
        without_ic = _corr(without, forward_return)
        records.append(
            {
                "family": family,
                "ic_24h": base_ic,
                "ic_24h_without_family": without_ic,
                "ic_delta_abs": abs(base_ic - without_ic) if _finite(base_ic) and _finite(without_ic) else None,
                "rows": int(family_scores[family].notna().sum()),
                "status": "ok" if _finite(base_ic) else "insufficient_variation",
            }
        )
    corr = family_scores.corr().fillna(0.0)
    pairs: list[dict[str, Any]] = []
    columns = [str(column) for column in corr.columns]
    for i, left in enumerate(columns):
        for right in columns[i + 1 :]:
            value = float(corr.loc[left, right])
            if abs(value) >= config.high_corr_threshold:
                pairs.append({"left": left, "right": right, "correlation": value})
    raw_counts = signal_rows.groupby([signal_rows.index, "family"]).size()
    duplicate_risk = {
        "max_raw_signals_per_family_per_timestamp": int(raw_counts.max()) if not raw_counts.empty else 0,
        "high_corr_family_pair_count": len(pairs),
        "high_corr_threshold": config.high_corr_threshold,
        "high_corr_pairs": pairs,
    }
    return pd.DataFrame.from_records(records, columns=READINESS_ABLATION_COLUMNS), duplicate_risk


def _lookahead_checks(frame: pd.DataFrame, config: ReadinessConfig) -> dict[str, Any]:
    if len(frame) < max(120, config.signal_forward_bars + 30):
        return {"passed": False, "status": "blocked", "reason": "insufficient rows for prefix probe"}
    prefix_len = max(120, len(frame) // 2)
    prefix = frame.iloc[:prefix_len].copy()
    future = frame.iloc[prefix_len:].copy()
    if future.empty:
        return {"passed": False, "status": "blocked", "reason": "no future suffix for shock probe"}
    shocked = future.copy()
    shocked["Close"] = shocked["Close"] * 5
    shocked["High"] = shocked[["High", "Close"]].max(axis=1) * 5
    shocked["Low"] = shocked[["Low", "Close"]].min(axis=1) * 5
    shocked["Volume"] = 1.0
    extended = pd.concat([prefix, shocked])
    try:
        prefix_regime = build_regime_report(prefix).rows
        extended_regime = build_regime_report(extended).rows.loc[prefix_regime.index]
        pd.testing.assert_frame_equal(prefix_regime, extended_regime, check_dtype=False, check_exact=False, rtol=1e-12, atol=1e-12)
        prefix_signal = build_signal_report(prefix, regimes=prefix_regime["regime_label"]).rows
        extended_full_regime = build_regime_report(extended).rows
        extended_signal = build_signal_report(extended, regimes=extended_full_regime["regime_label"]).rows.loc[prefix_signal.index.unique()]
        pd.testing.assert_frame_equal(prefix_signal, extended_signal, check_dtype=False, check_exact=False, rtol=1e-12, atol=1e-12)
        prefix_ablation, _ = _ablation_and_duplicate_risk(prefix, prefix_signal, config)
        extended_prefix_ablation, _ = _ablation_and_duplicate_risk(prefix, extended_signal, config)
        pd.testing.assert_frame_equal(
            prefix_ablation,
            extended_prefix_ablation,
            check_dtype=False,
            check_exact=False,
            rtol=1e-12,
            atol=1e-12,
        )
    except AssertionError as exc:
        return {"passed": False, "status": "failed", "reason": str(exc)[:500]}
    return {"passed": True, "status": "passed", "prefix_rows": int(len(prefix)), "ablation_prefix_invariant": True}


def _selected_candidate_switch_rate(selected_ids: list[str]) -> float:
    if len(selected_ids) <= 1:
        return 0.0
    switches = sum(1 for left, right in zip(selected_ids, selected_ids[1:]) if left != right)
    return switches / (len(selected_ids) - 1)


def _buy_hold_return_pct_and_source(holdout: pd.DataFrame, metrics: dict[str, Any]) -> tuple[float, str]:
    metric_value = _metric_float(metrics, "Buy_&_Hold_Return_pct", default=math.nan)
    if _finite(metric_value):
        return metric_value, "backtesting_metric"
    return _close_to_close_return_pct(holdout), "close_to_close_fallback"


def _close_to_close_return_pct(frame: pd.DataFrame) -> float:
    first = float(frame["Close"].iloc[0])
    last = float(frame["Close"].iloc[-1])
    return (last / first - 1) * 100


def _metric_float(metrics: dict[str, Any], key: str, default: float = 0.0) -> float:
    try:
        value = float(metrics.get(key, default))
    except (TypeError, ValueError):
        return default
    return value if math.isfinite(value) else default


def _normalize_funding_frame(frame: pd.DataFrame | None) -> pd.DataFrame | None:
    if frame is None:
        return None
    if frame.empty:
        return frame.copy()
    data = frame.copy()
    if not isinstance(data.index, pd.DatetimeIndex):
        if "funding_time" not in data:
            raise ValueError("funding_frame requires DatetimeIndex or funding_time column")
        data.index = pd.to_datetime(data["funding_time"], utc=True)
    elif data.index.tz is None:
        data.index = data.index.tz_localize("UTC")
    else:
        data.index = data.index.tz_convert("UTC")
    if "funding_rate" not in data:
        raise ValueError("funding_frame requires funding_rate column")
    data["funding_rate"] = pd.to_numeric(data["funding_rate"], errors="coerce")
    if data["funding_rate"].isna().any():
        raise ValueError("funding_frame funding_rate contains non-numeric values")
    return data.sort_index()


def _window_funding_rate_per_8h(
    funding_frame: pd.DataFrame | None,
    start: pd.Timestamp,
    end: pd.Timestamp,
    *,
    config: ReadinessConfig,
) -> float:
    if funding_frame is None or funding_frame.empty:
        return float(config.stress_funding_rate_per_8h)
    window = funding_frame.loc[pd.Timestamp(start) : pd.Timestamp(end)]
    if window.empty:
        return float(config.stress_funding_rate_per_8h)
    observed = pd.to_numeric(window["funding_rate"], errors="coerce").dropna()
    if observed.empty:
        return float(config.stress_funding_rate_per_8h)
    quantile = float(observed.quantile(config.funding_rate_quantile))
    if not math.isfinite(quantile):
        return float(config.stress_funding_rate_per_8h)
    return max(float(config.stress_funding_rate_per_8h), quantile)


def _corr(left: pd.Series, right: pd.Series) -> float:
    aligned = pd.concat([left, right], axis=1).dropna()
    if len(aligned) < 3:
        return math.nan
    if aligned.iloc[:, 0].nunique() < 2 or aligned.iloc[:, 1].nunique() < 2:
        return math.nan
    value = float(aligned.iloc[:, 0].corr(aligned.iloc[:, 1]))
    return value if math.isfinite(value) else math.nan


def _median(frame: pd.DataFrame, column: str) -> float | None:
    if frame.empty or column not in frame:
        return None
    value = pd.to_numeric(frame[column], errors="coerce").median()
    return float(value) if not pd.isna(value) else None


def _empty_duplicate_risk(config: ReadinessConfig) -> dict[str, Any]:
    return {
        "max_raw_signals_per_family_per_timestamp": 0,
        "high_corr_family_pair_count": 0,
        "high_corr_threshold": config.high_corr_threshold,
        "high_corr_pairs": [],
    }


def _records(frame: pd.DataFrame) -> list[dict[str, Any]]:
    return [{str(key): _plain_value(value) for key, value in row.items()} for row in frame.to_dict(orient="records")]


def _plain_value(value: Any) -> Any:
    if isinstance(value, pd.Timestamp):
        return _timestamp(value)
    if isinstance(value, bool | str | int):
        return value
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if pd.isna(value):
        return None
    if hasattr(value, "item"):
        try:
            return _plain_value(value.item())
        except (TypeError, ValueError):
            return str(value)
    return value


def _validate_ohlcv_frame(frame: pd.DataFrame) -> pd.DataFrame:
    required = ["Open", "High", "Low", "Close", "Volume"]
    missing = [column for column in required if column not in frame.columns]
    if missing:
        raise ValueError(f"missing OHLCV columns for readiness report: {missing}")
    data = frame.loc[:, required].copy()
    if not isinstance(data.index, pd.DatetimeIndex):
        raise ValueError("readiness report requires a DatetimeIndex")
    if data.index.tz is None:
        data.index = data.index.tz_localize("UTC")
    else:
        data.index = data.index.tz_convert("UTC")
    if not data.index.is_monotonic_increasing:
        raise ValueError("readiness report requires monotonic increasing timestamps")
    return data


def _timestamp(value: Any) -> str:
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return ts.isoformat().replace("+00:00", "Z")


def _finite(value: float | None) -> bool:
    return value is not None and math.isfinite(float(value))
