from __future__ import annotations

import hashlib
import json
import math
import random
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from itertools import product
from pathlib import Path
from typing import Any, Iterable, cast

import pandas as pd

from parangsae.marketdata.binance_funding import FundingRateBundle
from parangsae.marketdata.binance_vision import MarketDataBundle
from parangsae.research.backtest_engine import (
    BacktestResult,
    BrokerSettings,
    risk_adjusted_score_v1,
)
from parangsae.research.loop_artifacts import prepare_loop_output_dir
from parangsae.research.regime import build_regime_report

TSMOM_LOOP_ID = "TSMOM_VOL_TARGET_FUNDING_AWARE_V1"
TSMOM_SCHEMA_VERSION = 1
TSMOM_LOOP_TYPE = "strategy"
TSMOM_LOOP_SLUG = "tsmom-vol-target-funding-aware-v1"
TSMOM_LOOP_VERDICT_NAMESPACE = "tsmom_strategy_validation_only_not_live_readiness"
BASELINE_ARTIFACT = "reports/readiness-run-20260625T053627Z/"
READINESS_BASELINE_VERDICT = "NOT_READY"
TSMOM_WATCH_VERDICT = "WATCH"
TSMOM_BLOCKED_VERDICT = "BLOCKED"
TSMOM_PASS_PAPER_ONLY_VERDICT = "PASS_PAPER_ONLY"
TSMOM_ALLOWED_LOOP_VERDICTS = (
    TSMOM_BLOCKED_VERDICT,
    TSMOM_WATCH_VERDICT,
    TSMOM_PASS_PAPER_ONLY_VERDICT,
)
TSMOM_REQUIRED_FILES = (
    "loop.json",
    "report.md",
    "commands.txt",
    "changed_files.txt",
    "tsmom_windows.csv",
    "tsmom_candidates.csv",
)
TSMOM_KNOWN_FILES = set(TSMOM_REQUIRED_FILES)
TSMOM_DIAGNOSTIC_LOOP_ID = "TSMOM_STRESS_SWITCH_FUNDING_DIAGNOSTIC_V1"
TSMOM_DIAGNOSTIC_SCHEMA_VERSION = 1
TSMOM_DIAGNOSTIC_LOOP_TYPE = "diagnostic"
TSMOM_DIAGNOSTIC_LOOP_SLUG = "tsmom-stress-switch-funding-diagnostic-v1"
TSMOM_DIAGNOSTIC_VERDICT_NAMESPACE = "tsmom_stress_switch_funding_diagnostic_only_not_readiness"
TSMOM_DIAGNOSTIC_COMPLETE = "DIAGNOSTIC_COMPLETE"
TSMOM_DIAGNOSTIC_BLOCKED = "BLOCKED"
TSMOM_DIAGNOSTIC_ALLOWED_VERDICTS = (
    TSMOM_DIAGNOSTIC_BLOCKED,
    TSMOM_DIAGNOSTIC_COMPLETE,
)
TSMOM_DIAGNOSTIC_REQUIRED_FILES = (
    "diagnostic.json",
    "report.md",
    "failed_windows.csv",
    "candidate_switches.csv",
    "funding_decomposition.csv",
    "commands.txt",
    "changed_files.txt",
)
TSMOM_DIAGNOSTIC_KNOWN_FILES = set(TSMOM_DIAGNOSTIC_REQUIRED_FILES)
TSMOM_FAILED_GATE_DIAGNOSTIC_NAMES = (
    "median_stress_return_beats_cash_null",
    "selected_candidate_switch_rate",
    "funding_aware_net_return",
)
TSMOM_RISK_LOOP_ID = "TSMOM_SWITCH_STABILITY_FUNDING_THROTTLE_RISK_V1"
TSMOM_RISK_SCHEMA_VERSION = 1
TSMOM_RISK_LOOP_TYPE = "risk"
TSMOM_RISK_LOOP_SLUG = "tsmom-switch-stability-funding-throttle-risk-v1"
TSMOM_RISK_VERDICT_NAMESPACE = "tsmom_switch_stability_funding_throttle_risk_only_not_readiness"
TSMOM_RISK_BLOCKED = "BLOCKED"
TSMOM_RISK_WATCH = "WATCH"
TSMOM_RISK_PASS_PAPER_ONLY = "PASS_PAPER_ONLY"
TSMOM_RISK_ALLOWED_VERDICTS = (
    TSMOM_RISK_BLOCKED,
    TSMOM_RISK_WATCH,
    TSMOM_RISK_PASS_PAPER_ONLY,
)
TSMOM_RISK_REQUIRED_FILES = (
    "risk_loop.json",
    "report.md",
    "candidate_switch_stability.csv",
    "funding_throttle_decomposition.csv",
    "regime_breakdown.csv",
    "commands.txt",
    "changed_files.txt",
)
TSMOM_RISK_KNOWN_FILES = set(TSMOM_RISK_REQUIRED_FILES)
TSMOM_GROSS_EDGE_LOOP_ID = "TSMOM_GROSS_EDGE_REGIME_NULL_ROBUSTNESS_V1"
TSMOM_GROSS_EDGE_SCHEMA_VERSION = 1
TSMOM_GROSS_EDGE_LOOP_TYPE = "diagnostic_research"
TSMOM_GROSS_EDGE_LOOP_SLUG = "tsmom-gross-edge-regime-null-robustness-v1"
TSMOM_GROSS_EDGE_VERDICT_NAMESPACE = "tsmom_gross_edge_regime_null_robustness_only_not_readiness"
TSMOM_GROSS_EDGE_BLOCKED = "BLOCKED"
TSMOM_GROSS_EDGE_WATCH = "WATCH"
TSMOM_GROSS_EDGE_PASS_PAPER_ONLY = "PASS_PAPER_ONLY"
TSMOM_GROSS_EDGE_ALLOWED_VERDICTS = (
    TSMOM_GROSS_EDGE_BLOCKED,
    TSMOM_GROSS_EDGE_WATCH,
    TSMOM_GROSS_EDGE_PASS_PAPER_ONLY,
)
TSMOM_GROSS_EDGE_REQUIRED_FILES = (
    "gross_edge_loop.json",
    "report.md",
    "gross_edge_decomposition.csv",
    "regime_edge_breakdown.csv",
    "null_baseline_comparison.csv",
    "failed_edge_windows.csv",
    "commands.txt",
    "changed_files.txt",
)
TSMOM_GROSS_EDGE_KNOWN_FILES = set(TSMOM_GROSS_EDGE_REQUIRED_FILES)
TSMOM_FAMILY_DECISION_LOOP_ID = "TSMOM_CANDIDATE_FAMILY_DECISION_V1"
TSMOM_FAMILY_DECISION_SCHEMA_VERSION = 1
TSMOM_FAMILY_DECISION_LOOP_TYPE = "research_governance"
TSMOM_FAMILY_DECISION_LOOP_SLUG = "tsmom-candidate-family-decision-v1"
TSMOM_FAMILY_DECISION_NAMESPACE = "tsmom_candidate_family_decision_only_not_readiness"
TSMOM_FAMILY_DECISION_KEEP_WATCH = "KEEP_WATCH"
TSMOM_FAMILY_DECISION_REGIME_ONLY_WATCH = "REGIME_ONLY_WATCH"
TSMOM_FAMILY_DECISION_FREEZE_NEGATIVE_CONTROL = "FREEZE_AS_NEGATIVE_CONTROL"
TSMOM_FAMILY_DECISION_DEPRECATE = "DEPRECATE_CANDIDATE_FAMILY"
TSMOM_FAMILY_DECISION_ALLOWED_DECISIONS = (
    TSMOM_FAMILY_DECISION_KEEP_WATCH,
    TSMOM_FAMILY_DECISION_REGIME_ONLY_WATCH,
    TSMOM_FAMILY_DECISION_FREEZE_NEGATIVE_CONTROL,
    TSMOM_FAMILY_DECISION_DEPRECATE,
)
TSMOM_FAMILY_DECISION_REQUIRED_FILES = (
    "candidate_family_decision.json",
    "report.md",
    "evidence_rollup.csv",
    "failed_gate_rollup.csv",
    "regime_exception_review.csv",
    "decision_rationale.md",
    "commands.txt",
    "changed_files.txt",
)
TSMOM_FAMILY_DECISION_KNOWN_FILES = set(TSMOM_FAMILY_DECISION_REQUIRED_FILES)
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
HARD_GATE_NAMES = (
    "candidate_count",
    "candidate_window_evaluations",
    "bars_processed_estimate",
    "median_stress_return_beats_cash_null",
    "selected_candidate_switch_rate",
    "funding_aware_net_return",
    "max_drawdown_abs_pct",
    "tail_loss_cvar_pct",
)
CLAIM_BOUNDARY = (
    "TSMOM_VOL_TARGET_FUNDING_AWARE_V1 is a pre-registered paper-only strategy "
    "validation loop, not live readiness, not trading advice, not a profit guarantee, "
    "and not permission for live orders"
)
FUNDING_CAUSALITY_POLICY = (
    "Public funding events are aligned to OHLCV bars with ffill only after the event "
    "timestamp, then shifted one decision bar. Funding is never backfilled into earlier "
    "bars and never represents account-specific funding cashflows."
)


@dataclass(frozen=True)
class TsmomLoopConfig:
    train_bars: int = 24 * 45
    validation_bars: int = 24 * 21
    holdout_bars: int = 24 * 21
    step_bars: int = 24 * 21
    min_windows: int = 12
    min_research_grade_candidate_count: int = 25
    min_research_grade_candidate_window_evaluations: int = 300
    min_research_grade_bars_processed_estimate: int = 500_000
    max_selected_candidate_switch_rate: float = 0.75
    max_drawdown_abs_pct: float = 35.0
    min_tail_loss_cvar_pct: float = -15.0
    tail_loss_cvar_quantile: float = 0.05
    stress_commission: float = 0.001
    stress_spread: float = 0.001
    stress_slippage_proxy_bps: float = 5.0
    stress_funding_rate_per_8h: float = 0.0001
    funding_rate_quantile: float = 0.75
    stable_selection_score_margin: float = 0.05
    funding_drag_throttle_abs_rate: float = 0.0001
    funding_drag_throttle_multiplier: float = 0.50
    gross_edge_high_funding_drag_pct: float = 0.25
    gross_edge_min_hit_rate: float = 0.50
    asset: str = "BTCUSDT"
    timeframe: str = "1h"


@dataclass(frozen=True)
class TsmomCandidateSettings:
    lookback_bars: int
    vol_window_bars: int
    target_annual_volatility: float
    max_exposure: float
    funding_abs_rate_limit: float
    allow_short: bool = False


@dataclass(frozen=True)
class TsmomWindow:
    window_id: str
    start: int
    train: pd.DataFrame
    validation: pd.DataFrame
    holdout: pd.DataFrame


@dataclass(frozen=True)
class TsmomCandidateEvaluation:
    candidate_id: str
    settings: TsmomCandidateSettings
    train: BacktestResult
    validation: BacktestResult
    score: float
    eligible: bool
    rejection_reasons: tuple[str, ...]


def default_tsmom_output_dir(*, root: str | Path = "reports/loops", today: str | None = None) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{TSMOM_LOOP_SLUG}"


def default_tsmom_diagnostic_output_dir(*, root: str | Path = "reports/loops", today: str | None = None) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{TSMOM_DIAGNOSTIC_LOOP_SLUG}"


def default_tsmom_risk_output_dir(*, root: str | Path = "reports/loops", today: str | None = None) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{TSMOM_RISK_LOOP_SLUG}"


def default_tsmom_gross_edge_output_dir(*, root: str | Path = "reports/loops", today: str | None = None) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{TSMOM_GROSS_EDGE_LOOP_SLUG}"


def default_tsmom_family_decision_output_dir(*, root: str | Path = "reports/loops", today: str | None = None) -> Path:
    stamp = today or datetime.now(UTC).strftime("%Y%m%d")
    return Path(root) / f"{stamp}-{TSMOM_FAMILY_DECISION_LOOP_SLUG}"


def tsmom_candidate_grid() -> tuple[TsmomCandidateSettings, ...]:
    return tuple(
        TsmomCandidateSettings(
            lookback_bars=lookback_bars,
            vol_window_bars=vol_window_bars,
            target_annual_volatility=target_annual_volatility,
            max_exposure=max_exposure,
            funding_abs_rate_limit=funding_abs_rate_limit,
            allow_short=False,
        )
        for lookback_bars, vol_window_bars, target_annual_volatility, max_exposure, funding_abs_rate_limit in product(
            (168, 336, 672),
            (168, 336),
            (0.25, 0.40),
            (0.50, 0.80),
            (0.00025, 0.00050),
        )
    )


def align_public_funding_to_decision_bars(
    ohlcv_index: pd.DatetimeIndex,
    funding_frame: pd.DataFrame | None,
) -> pd.Series:
    index = _utc_index(ohlcv_index)
    if funding_frame is None or funding_frame.empty:
        return pd.Series(0.0, index=index, dtype=float)
    funding = _normalize_funding_frame(funding_frame)
    rates = _float_series(funding["funding_rate"], index=funding.index).dropna().sort_index()
    if rates.empty:
        return pd.Series(0.0, index=index, dtype=float)
    aligned = cast(pd.Series, rates.reindex(index, method="ffill")).shift(1)
    return _float_series(aligned, index=index).fillna(0.0)


def build_tsmom_decision_frame(
    frame: pd.DataFrame,
    *,
    settings: TsmomCandidateSettings,
    funding_frame: pd.DataFrame | None = None,
) -> pd.DataFrame:
    data = _validate_ohlcv_frame(frame)
    data_index = _datetime_index(data)
    close = _float_series(data["Close"], index=data_index)
    returns = _float_series(close.pct_change(), index=data_index).fillna(0.0)
    momentum = cast(pd.Series, (close / close.shift(settings.lookback_bars) - 1.0)).shift(1)
    realized_volatility = cast(
        pd.Series,
        returns.rolling(
            settings.vol_window_bars,
            min_periods=settings.vol_window_bars,
        ).std(),
    ).shift(1)
    annualized_volatility = cast(pd.Series, realized_volatility * math.sqrt(24 * 365))
    vol_target_raw = cast(
        pd.Series,
        cast(pd.Series, annualized_volatility.replace(0.0, pd.NA)).rdiv(
            settings.target_annual_volatility
        ),
    ).clip(lower=0.0, upper=settings.max_exposure)
    vol_target = _float_series(vol_target_raw, index=data_index).fillna(0.0)

    direction = pd.Series(0.0, index=data_index, dtype=float)
    direction = cast(pd.Series, direction.mask(momentum > 0, 1.0))
    if settings.allow_short:
        direction = cast(pd.Series, direction.mask(momentum < 0, -1.0))

    regime = build_regime_report(data).rows
    risk_multiplier = (
        _float_series(regime["risk_multiplier"], index=regime.index)
        .reindex(data_index)
        .fillna(0.0)
        .shift(1)
        .fillna(0.0)
        .clip(lower=0.0, upper=1.0)
    )
    risk_multiplier = _float_series(risk_multiplier, index=data_index)
    funding_rate = align_public_funding_to_decision_bars(data_index, funding_frame)
    funding_allowed = (funding_rate.abs() <= settings.funding_abs_rate_limit).astype(float)
    position = (direction * vol_target * risk_multiplier * funding_allowed).fillna(0.0)
    return pd.DataFrame(
        {
            "momentum": momentum.fillna(0.0),
            "realized_volatility": realized_volatility.fillna(0.0),
            "annualized_volatility": annualized_volatility.fillna(0.0),
            "risk_multiplier": risk_multiplier,
            "funding_rate": funding_rate,
            "funding_allowed": funding_allowed,
            "position": position.clip(lower=-settings.max_exposure, upper=settings.max_exposure),
        },
        index=data_index,
    )


def run_tsmom_backtest(
    frame: pd.DataFrame,
    *,
    label: str,
    settings: TsmomCandidateSettings,
    broker_settings: BrokerSettings = BrokerSettings(),
    funding_frame: pd.DataFrame | None = None,
    position_multiplier: pd.Series | None = None,
) -> BacktestResult:
    data = _validate_ohlcv_frame(frame)
    data_index = _datetime_index(data)
    decisions = build_tsmom_decision_frame(data, settings=settings, funding_frame=funding_frame)
    if position_multiplier is not None:
        multiplier = (
            _float_series(position_multiplier, index=position_multiplier.index)
            .reindex(data_index)
            .fillna(1.0)
            .clip(lower=0.0, upper=1.0)
        )
        decisions = decisions.copy()
        decisions["position"] = _float_series(decisions["position"], index=data_index) * multiplier
    close = _float_series(data["Close"], index=data_index)
    returns = _float_series(close.pct_change(), index=data_index).fillna(0.0)
    equity = float(broker_settings.cash)
    current_position = 0.0
    open_trade: dict[str, Any] | None = None
    trades: list[dict[str, Any]] = []
    equity_values: list[float] = []
    strategy_returns: list[float] = []

    for raw_timestamp, bar_return in returns.items():
        timestamp = cast(pd.Timestamp, pd.Timestamp(cast(Any, raw_timestamp)))
        desired_position = _dataframe_at_float(decisions, timestamp, "position")
        turnover = abs(desired_position - current_position)
        trade_cost = turnover * (float(broker_settings.commission) + float(broker_settings.spread))
        period_return = current_position * float(bar_return) - trade_cost
        equity *= 1.0 + period_return
        strategy_returns.append(period_return)
        equity_values.append(equity)

        direction_changed = (
            current_position != 0.0
            and (desired_position == 0.0 or math.copysign(1.0, desired_position) != math.copysign(1.0, current_position))
        )
        if direction_changed and open_trade is not None:
            trades.append(_close_trade(open_trade, timestamp, _series_at_float(close, timestamp), broker_settings))
            open_trade = None
        if open_trade is None and desired_position != 0.0:
            open_trade = {
                "entry_time": timestamp,
                "entry_price": _series_at_float(close, timestamp),
                "direction": 1.0 if desired_position > 0 else -1.0,
                "position": desired_position,
            }
        current_position = desired_position

    if open_trade is not None and broker_settings.finalize_trades:
        timestamp = _timestamp_at(data_index, -1)
        trades.append(_close_trade(open_trade, timestamp, _series_at_float(close, timestamp), broker_settings))

    returns_series = pd.Series(strategy_returns, index=data_index, dtype=float)
    equity_frame = pd.DataFrame({"Equity": equity_values}, index=data_index)
    equity_series = _float_series(equity_frame["Equity"], index=data_index)
    drawdown = cast(pd.Series, equity_series / equity_series.cummax() - 1.0)
    equity_frame["DrawdownPct"] = drawdown * 100.0
    drawdown_pct = _float_series(equity_frame["DrawdownPct"], index=data_index)
    trades_frame = pd.DataFrame.from_records(trades)
    position = _float_series(decisions["position"], index=data_index)
    metrics = {
        "label": label,
        "closed_trades": int(len(trades_frame)),
        "Return_pct": float((equity / float(broker_settings.cash) - 1.0) * 100.0),
        "Sharpe_Ratio": _annualized_sharpe(returns_series),
        "Max_Drawdown_pct": _series_min_float(drawdown_pct) if not equity_frame.empty else 0.0,
        "Buy_&_Hold_Return_pct": _close_to_close_return_pct(data),
        "Exposure_Time_pct": float((position.abs() > 0.0).mean() * 100.0),
        "Turnover_proxy": float(position.diff().abs().fillna(position.abs()).sum()),
    }
    return BacktestResult(label=label, metrics=metrics, trades=trades_frame, equity=equity_frame)


def build_tsmom_loop_payload(
    *,
    bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None = None,
    funding_metadata: dict[str, Any] | None = None,
    config: TsmomLoopConfig = TsmomLoopConfig(),
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> dict[str, Any]:
    data = _validate_ohlcv_frame(bundle.frame)
    funding_frame = funding_bundle.frame if funding_bundle is not None else None
    grid = tsmom_candidate_grid()
    windows = _generate_windows(data, config)
    candidates = _candidate_registry_rows(grid)
    window_records: list[dict[str, Any]] = []
    selected_ids: list[str] = []
    for window in windows:
        selected = _select_candidate(
            window.train,
            window.validation,
            grid,
            config=config,
            funding_frame=funding_frame,
        )
        selected_ids.append(selected.candidate_id)
        base_holdout = run_tsmom_backtest(
            window.holdout,
            label=f"{window.window_id}_holdout_base",
            settings=selected.settings,
            broker_settings=BrokerSettings(),
            funding_frame=funding_frame,
        )
        stress_holdout = run_tsmom_backtest(
            window.holdout,
            label=f"{window.window_id}_holdout_stress",
            settings=selected.settings,
            broker_settings=BrokerSettings(
                commission=config.stress_commission,
                spread=config.stress_spread,
            ),
            funding_frame=funding_frame,
        )
        stress_return = _metric_float(stress_holdout.metrics, "Return_pct")
        funding_rate = _window_funding_rate_per_8h(
            funding_frame,
            _timestamp_at(window.holdout.index, 0),
            _timestamp_at(window.holdout.index, -1),
            config=config,
        )
        funding_drag = _funding_drag_pct(
            bars=len(window.holdout),
            exposure_time_pct=_metric_float(stress_holdout.metrics, "Exposure_Time_pct"),
            funding_rate_per_8h=funding_rate,
        )
        slip_drag = _slippage_drag_pct(
            closed_trades=int(stress_holdout.metrics.get("closed_trades", 0) or 0),
            slippage_proxy_bps=config.stress_slippage_proxy_bps,
        )
        funding_aware_net_return = stress_return - funding_drag - slip_drag
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
                "selected_lookback_bars": selected.settings.lookback_bars,
                "selected_vol_window_bars": selected.settings.vol_window_bars,
                "selected_target_annual_volatility": selected.settings.target_annual_volatility,
                "selected_max_exposure": selected.settings.max_exposure,
                "selected_funding_abs_rate_limit": selected.settings.funding_abs_rate_limit,
                "train_score": _metric_float(selected.train.metrics, "Sharpe_Ratio"),
                "validation_score": selected.score,
                "base_holdout_return_pct": _metric_float(base_holdout.metrics, "Return_pct"),
                "base_holdout_max_drawdown_pct": _metric_float(
                    base_holdout.metrics,
                    "Max_Drawdown_pct",
                ),
                "base_holdout_closed_trades": int(base_holdout.metrics.get("closed_trades", 0) or 0),
                "base_buy_hold_return_pct": _close_to_close_return_pct(window.holdout),
                "cash_null_return_pct": 0.0,
                "stress_holdout_return_pct": funding_aware_net_return,
                "stress_holdout_raw_return_pct": stress_return,
                "stress_holdout_max_drawdown_pct": _metric_float(
                    stress_holdout.metrics,
                    "Max_Drawdown_pct",
                ),
                "stress_funding_rate_per_8h": funding_rate,
                "stress_funding_drag_pct": funding_drag,
                "stress_slippage_drag_pct": slip_drag,
                "stress_slippage_proxy_bps": config.stress_slippage_proxy_bps,
                "funding_aware_net_return_pct": funding_aware_net_return,
                "beats_cash_null": funding_aware_net_return > 0.0,
                "verdict": "PASS" if funding_aware_net_return > 0.0 else "FAIL",
            }
        )

    compute_profile = _compute_profile(data=data, windows=windows, candidate_count=len(grid))
    funding_validated = funding_bundle is not None and funding_frame is not None and not funding_frame.empty
    hard_gates = _hard_gates(
        window_records=window_records,
        selected_ids=selected_ids,
        compute_profile=compute_profile,
        funding_validated=funding_validated,
        config=config,
    )
    failed_gates = [name for name, gate in hard_gates.items() if not bool(gate["passed"])]
    loop_verdict = _loop_verdict(windows=windows, failed_gates=failed_gates)
    loop_id = f"{datetime.now(UTC).strftime('%Y%m%d')}-{TSMOM_LOOP_SLUG}"
    payload = {
        "schema_version": TSMOM_SCHEMA_VERSION,
        "loop_id": loop_id,
        "status": "IMPLEMENTED",
        "loop_type": TSMOM_LOOP_TYPE,
        "strategy_family": TSMOM_LOOP_ID,
        "hypothesis": (
            "A causal time-series momentum strategy with volatility-targeted sizing and public "
            "funding throttles can be falsified under research-grade walk-forward validation."
        ),
        "baseline": BASELINE_ARTIFACT,
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "tsmom_loop_verdict": loop_verdict,
        "tsmom_loop_verdict_namespace": TSMOM_LOOP_VERDICT_NAMESPACE,
        "loop_contract": {
            "allowed_loop_verdicts": list(TSMOM_ALLOWED_LOOP_VERDICTS),
            "loop_verdict_namespace": TSMOM_LOOP_VERDICT_NAMESPACE,
            "readiness_verdict": False,
            "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
            "claim_boundary": CLAIM_BOUNDARY,
        },
        "data_scope": _data_scope(bundle),
        "dataset_provenance": _dataset_provenance(bundle, funding_bundle),
        "funding_coverage": _funding_coverage(funding_bundle, funding_metadata),
        "funding_causality_policy": FUNDING_CAUSALITY_POLICY,
        "selection_inputs": ["train", "validation"],
        "selection_excludes": ["holdout", "live_orders", "testnet_orders", "api_keys", "account_state", "fills"],
        "expected_failure_modes": [
            "median stress return fails to beat cash/null after funding and slippage stress",
            "selected candidate switches too often across walk-forward windows",
            "public funding-aware net return is not positive",
            "drawdown exceeds the pre-registered absolute drawdown threshold",
            "tail-loss CVaR breaches the pre-registered stress threshold",
            "research-grade candidate/evaluation/bars-processed depth is insufficient",
        ],
        "selection_metric": {
            "name": "risk_adjusted_score_v1",
            "formula_source": "parangsae.research.backtest_engine.risk_adjusted_score_v1",
            "tie_breakers": [
                "higher_validation_return_pct",
                "smaller_abs_validation_max_drawdown_pct",
                "lower_validation_closed_trades_turnover_proxy",
                "lexicographically_smaller_candidate_id",
            ],
            "selection_uses_holdout": False,
        },
        "pre_registered_grid": {
            "candidate_count": len(grid),
            "lookback_bars": [168, 336, 672],
            "vol_window_bars": [168, 336],
            "target_annual_volatility": [0.25, 0.40],
            "max_exposure": [0.50, 0.80],
            "funding_abs_rate_limit": [0.00025, 0.00050],
            "allow_short": False,
            "rows": candidates,
        },
        "validation_depth": {
            "observed": {
                key: compute_profile[key]
                for key in (
                    "candidate_count",
                    "candidate_window_evaluations",
                    "bars_processed_estimate",
                    "window_count",
                    "total_input_bars",
                )
            },
            "thresholds": {
                "candidate_count": config.min_research_grade_candidate_count,
                "candidate_window_evaluations": config.min_research_grade_candidate_window_evaluations,
                "bars_processed_estimate": config.min_research_grade_bars_processed_estimate,
            },
        },
        "hard_gates": hard_gates,
        "failed_gates": failed_gates,
        "compute_profile": compute_profile,
        "walk_forward": {"window_count": len(window_records), "rows": window_records},
        "candidate_registry": {"version": 1, "frozen": True, "rows": candidates},
        "safety_gates": {
            "live_orders_submitted": False,
            "testnet_orders_submitted": False,
            "api_keys_required_for_research": False,
            "account_or_fill_data_used": False,
            "holdout_used_for_selection": False,
            "profit_or_live_ready_claim": False,
        },
        "validation_commands": list(commands),
        "evidence_artifacts": [
            "loop.json",
            "report.md",
            "commands.txt",
            "changed_files.txt",
            "tsmom_windows.csv",
            "tsmom_candidates.csv",
        ],
        "changed_files": list(changed_files),
        "claim_boundary": CLAIM_BOUNDARY,
        "tsmom_loop_decision": "ITERATE_ON_SAME_HYPOTHESIS" if failed_gates else "PROMOTE_TO_NEXT_LOOP",
        "next_loop": (
            "Fix failed hard gates without retuning holdout, or run stricter paper validation "
            "only if all gates pass."
        ),
    }
    return payload


def write_tsmom_loop_artifacts(
    output_dir: str | Path,
    payload: dict[str, Any],
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_tsmom_output_dir(output_dir, overwrite=overwrite)
    _write_json(out / "loop.json", payload)
    _atomic_write_text(out / "report.md", render_tsmom_report(payload))
    _atomic_write_text(out / "commands.txt", "\n".join(payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(payload.get("changed_files", [])) + "\n")
    pd.DataFrame(payload.get("walk_forward", {}).get("rows", [])).to_csv(
        out / "tsmom_windows.csv",
        index=False,
    )
    pd.DataFrame(payload.get("candidate_registry", {}).get("rows", [])).to_csv(
        out / "tsmom_candidates.csv",
        index=False,
    )
    return out


def prepare_tsmom_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="tsmom-run",
        known_files=TSMOM_KNOWN_FILES,
        overwrite=overwrite,
    )


def build_tsmom_diagnostic_payload(
    *,
    bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None = None,
    funding_metadata: dict[str, Any] | None = None,
    config: TsmomLoopConfig = TsmomLoopConfig(),
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> dict[str, Any]:
    source_payload = build_tsmom_loop_payload(
        bundle=bundle,
        funding_bundle=funding_bundle,
        funding_metadata=funding_metadata,
        config=config,
        commands=commands,
        changed_files=changed_files,
    )
    data = _validate_ohlcv_frame(bundle.frame)
    funding_frame = funding_bundle.frame if funding_bundle is not None else None
    grid = tsmom_candidate_grid()
    windows = _generate_windows(data, config)
    selected_by_window = {
        str(row["window_id"]): row
        for row in source_payload.get("walk_forward", {}).get("rows", [])
        if isinstance(row, dict)
    }
    funding_rows: list[dict[str, Any]] = []
    switch_rows: list[dict[str, Any]] = []
    previous_selected: dict[str, Any] | None = None
    previous_settings: TsmomCandidateSettings | None = None

    for window in windows:
        row = selected_by_window.get(window.window_id)
        if row is None:
            continue
        selected_settings = _settings_from_window_row(row)
        rankings = _rank_candidate_evaluations(
            window.train,
            window.validation,
            grid,
            funding_frame=funding_frame,
        )
        runner_up = rankings[1] if len(rankings) > 1 else None
        decisions = build_tsmom_decision_frame(
            window.holdout,
            settings=selected_settings,
            funding_frame=funding_frame,
        )
        regime = _window_regime_diagnostics(window.holdout, decisions, funding_frame, config=config)
        decomposition = {
            "window_id": window.window_id,
            "holdout_start": row["holdout_start"],
            "holdout_end": row["holdout_end"],
            "selected_candidate_id": row["selected_candidate_id"],
            "market_regime": regime["market_regime"],
            "volatility_regime": regime["volatility_regime"],
            "stress_window": False,
            "funding_regime": regime["funding_regime"],
            "gross_return_pct": row["base_holdout_return_pct"],
            "stress_raw_return_pct": row["stress_holdout_raw_return_pct"],
            "funding_cost_drag_pct": row["stress_funding_drag_pct"],
            "slippage_drag_pct": row["stress_slippage_drag_pct"],
            "funding_aware_net_return_pct": row["funding_aware_net_return_pct"],
            "cash_null_return_pct": row["cash_null_return_pct"],
            "cash_null_failure": bool(float(row["funding_aware_net_return_pct"]) <= 0.0),
            "max_drawdown_pct": row["stress_holdout_max_drawdown_pct"],
            "mean_funding_rate": regime["mean_funding_rate"],
            "mean_abs_funding_rate": regime["mean_abs_funding_rate"],
            "exposure_time_pct": regime["exposure_time_pct"],
            "diagnosed_primary_failure": _funding_failure_reason(row, regime),
        }
        funding_rows.append(decomposition)
        switched = (
            previous_selected is not None
            and previous_selected.get("selected_candidate_id") != row["selected_candidate_id"]
        )
        switch_causes = (
            _candidate_switch_causes(
                previous_settings,
                selected_settings,
                score_gap=_score_gap(rankings),
                funding_cost_drag_pct=float(row["stress_funding_drag_pct"]),
            )
            if switched
            else []
        )
        switch_rows.append(
            {
                "window_id": window.window_id,
                "holdout_start": row["holdout_start"],
                "holdout_end": row["holdout_end"],
                "previous_candidate_id": previous_selected.get("selected_candidate_id") if previous_selected else None,
                "selected_candidate_id": row["selected_candidate_id"],
                "switched": switched,
                "switch_causes": ";".join(switch_causes) if switch_causes else "no_switch",
                "selected_lookback_bars": selected_settings.lookback_bars,
                "selected_vol_window_bars": selected_settings.vol_window_bars,
                "selected_target_annual_volatility": selected_settings.target_annual_volatility,
                "selected_max_exposure": selected_settings.max_exposure,
                "selected_funding_abs_rate_limit": selected_settings.funding_abs_rate_limit,
                "selected_validation_score": row["validation_score"],
                "runner_up_candidate_id": runner_up.candidate_id if runner_up is not None else None,
                "runner_up_validation_score": runner_up.score if runner_up is not None else None,
                "score_gap_to_runner_up": _score_gap(rankings),
            }
        )
        previous_selected = row
        previous_settings = selected_settings

    funding_rows = _mark_stress_windows(funding_rows)
    failed_windows = [row for row in funding_rows if row["cash_null_failure"]]
    switch_summary = _candidate_switch_summary(switch_rows)
    stress_summary = _stress_return_distribution(funding_rows, config)
    funding_summary = _funding_decomposition_summary(funding_rows)
    regime_summary = _regime_decomposition_summary(funding_rows)
    verdict = TSMOM_DIAGNOSTIC_COMPLETE if funding_rows else TSMOM_DIAGNOSTIC_BLOCKED
    loop_id = f"{datetime.now(UTC).strftime('%Y%m%d')}-{TSMOM_DIAGNOSTIC_LOOP_SLUG}"
    return {
        "schema_version": TSMOM_DIAGNOSTIC_SCHEMA_VERSION,
        "loop_id": loop_id,
        "status": "IMPLEMENTED" if funding_rows else "BLOCKED",
        "loop_type": TSMOM_DIAGNOSTIC_LOOP_TYPE,
        "diagnostic_family": TSMOM_DIAGNOSTIC_LOOP_ID,
        "source_strategy_family": TSMOM_LOOP_ID,
        "baseline": BASELINE_ARTIFACT,
        "source_loop_artifact": "reports/loops/20260625-tsmom-vol-target-funding-aware-v1/",
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "tsmom_diagnostic_loop_verdict": verdict,
        "tsmom_diagnostic_loop_verdict_namespace": TSMOM_DIAGNOSTIC_VERDICT_NAMESPACE,
        "loop_contract": {
            "allowed_diagnostic_loop_verdicts": list(TSMOM_DIAGNOSTIC_ALLOWED_VERDICTS),
            "diagnostic_loop_verdict_namespace": TSMOM_DIAGNOSTIC_VERDICT_NAMESPACE,
            "readiness_verdict": False,
            "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
            "claim_boundary": _diagnostic_claim_boundary(),
        },
        "hypothesis": (
            "The failed TSMOM stress, switch-rate, and funding-aware net-return gates can be "
            "explained without retuning by decomposing selected walk-forward holdouts by regime, "
            "funding drag, and candidate-selection instability."
        ),
        "analyzed_failed_gates": list(TSMOM_FAILED_GATE_DIAGNOSTIC_NAMES),
        "source_tsmom_loop": {
            "tsmom_loop_verdict": source_payload["tsmom_loop_verdict"],
            "tsmom_loop_decision": source_payload["tsmom_loop_decision"],
            "readiness_baseline_verdict": source_payload["readiness_baseline_verdict"],
            "failed_gates": source_payload["failed_gates"],
            "hard_gates": {
                name: source_payload["hard_gates"][name]
                for name in TSMOM_FAILED_GATE_DIAGNOSTIC_NAMES
                if name in source_payload["hard_gates"]
            },
        },
        "data_scope": source_payload["data_scope"],
        "dataset_provenance": source_payload["dataset_provenance"],
        "funding_coverage": source_payload["funding_coverage"],
        "selection_inputs": source_payload["selection_inputs"],
        "selection_excludes": source_payload["selection_excludes"],
        "diagnostic_summary": {
            "stress_return_distribution": stress_summary,
            "candidate_switch_summary": switch_summary,
            "funding_decomposition_summary": funding_summary,
            "regime_decomposition_summary": regime_summary,
            "cash_null_failure_window_count": len(failed_windows),
            "window_count": len(funding_rows),
        },
        "failed_windows": failed_windows,
        "candidate_switches": switch_rows,
        "funding_decomposition": funding_rows,
        "safety_gates": {
            "live_orders_submitted": False,
            "testnet_orders_submitted": False,
            "api_keys_required_for_research": False,
            "account_or_fill_data_used": False,
            "holdout_used_for_selection": False,
            "profit_or_live_ready_claim": False,
            "strategy_parameters_retuned": False,
        },
        "validation_commands": list(commands),
        "evidence_artifacts": list(TSMOM_DIAGNOSTIC_REQUIRED_FILES),
        "changed_files": list(changed_files),
        "claim_boundary": _diagnostic_claim_boundary(),
        "tsmom_diagnostic_loop_decision": "DIAGNOSTIC_ONLY_NO_PROMOTION",
        "next_pre_registered_loop_recommendation": (
            "Pre-register a risk/cost loop that reduces candidate-switch instability and "
            "explicitly tests whether funding drag should throttle exposure before holdout, "
            "without relaxing gates or claiming readiness."
        ),
    }


def write_tsmom_diagnostic_artifacts(
    output_dir: str | Path,
    payload: dict[str, Any],
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_tsmom_diagnostic_output_dir(output_dir, overwrite=overwrite)
    artifact_payload = dict(payload)
    if out.name.endswith(TSMOM_DIAGNOSTIC_LOOP_SLUG):
        artifact_payload["loop_id"] = out.name
    _write_json(out / "diagnostic.json", artifact_payload)
    _atomic_write_text(out / "report.md", render_tsmom_diagnostic_report(artifact_payload))
    pd.DataFrame(artifact_payload.get("failed_windows", [])).to_csv(out / "failed_windows.csv", index=False)
    pd.DataFrame(artifact_payload.get("candidate_switches", [])).to_csv(out / "candidate_switches.csv", index=False)
    pd.DataFrame(artifact_payload.get("funding_decomposition", [])).to_csv(
        out / "funding_decomposition.csv",
        index=False,
    )
    _atomic_write_text(out / "commands.txt", "\n".join(artifact_payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(artifact_payload.get("changed_files", [])) + "\n")
    return out


def prepare_tsmom_diagnostic_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="tsmom-diagnostic-run",
        known_files=TSMOM_DIAGNOSTIC_KNOWN_FILES,
        overwrite=overwrite,
    )


def build_tsmom_risk_loop_payload(
    *,
    bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None = None,
    funding_metadata: dict[str, Any] | None = None,
    config: TsmomLoopConfig = TsmomLoopConfig(),
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> dict[str, Any]:
    source_payload = build_tsmom_loop_payload(
        bundle=bundle,
        funding_bundle=funding_bundle,
        funding_metadata=funding_metadata,
        config=config,
        commands=commands,
        changed_files=changed_files,
    )
    data = _validate_ohlcv_frame(bundle.frame)
    funding_frame = funding_bundle.frame if funding_bundle is not None else None
    funding_validated = funding_bundle is not None and funding_frame is not None and not funding_frame.empty
    grid = tsmom_candidate_grid()
    windows = _generate_windows(data, config)
    source_rows = {
        str(row["window_id"]): row
        for row in source_payload.get("walk_forward", {}).get("rows", [])
        if isinstance(row, dict)
    }
    switch_rows: list[dict[str, Any]] = []
    throttle_rows: list[dict[str, Any]] = []
    previous_baseline: dict[str, Any] | None = None
    previous_stable: TsmomCandidateEvaluation | None = None
    previous_stable_regime: str | None = None
    baseline_ids: list[str] = []
    stable_ids: list[str] = []

    for window in windows:
        source_row = source_rows.get(window.window_id)
        if source_row is None:
            continue
        rankings = _rank_candidate_evaluations(
            window.train,
            window.validation,
            grid,
            funding_frame=funding_frame,
        )
        top = rankings[0]
        runner_up = rankings[1] if len(rankings) > 1 else None
        previous_stable_ranked = (
            next((candidate for candidate in rankings if candidate.candidate_id == previous_stable.candidate_id), None)
            if previous_stable is not None
            else None
        )
        score_gap = _score_gap(rankings)
        stable_selected = _stable_selected_candidate(
            rankings,
            previous_stable=previous_stable,
            config=config,
        )
        top_vs_previous_stable_gap = (
            float(top.score - previous_stable_ranked.score) if previous_stable_ranked is not None else None
        )
        sticky_selection_triggered = bool(stable_selected.candidate_id != top.candidate_id)
        baseline_ids.append(str(source_row["selected_candidate_id"]))
        stable_ids.append(stable_selected.candidate_id)
        multiplier = _funding_drag_throttle_multiplier(_datetime_index(window.holdout), funding_frame, config=config)
        risk_base_holdout = run_tsmom_backtest(
            window.holdout,
            label=f"{window.window_id}_risk_variant_base",
            settings=stable_selected.settings,
            funding_frame=funding_frame,
            position_multiplier=multiplier,
        )
        risk_stress_holdout = run_tsmom_backtest(
            window.holdout,
            label=f"{window.window_id}_risk_variant_stress",
            settings=stable_selected.settings,
            broker_settings=BrokerSettings(
                commission=config.stress_commission,
                spread=config.stress_spread,
            ),
            funding_frame=funding_frame,
            position_multiplier=multiplier,
        )
        risk_stress_raw = _metric_float(risk_stress_holdout.metrics, "Return_pct")
        risk_funding_rate = _window_funding_rate_per_8h(
            funding_frame,
            _timestamp_at(window.holdout.index, 0),
            _timestamp_at(window.holdout.index, -1),
            config=config,
        )
        risk_funding_drag = _funding_drag_pct(
            bars=len(window.holdout),
            exposure_time_pct=_metric_float(risk_stress_holdout.metrics, "Exposure_Time_pct"),
            funding_rate_per_8h=risk_funding_rate,
        )
        risk_slippage_drag = _slippage_drag_pct(
            closed_trades=int(risk_stress_holdout.metrics.get("closed_trades", 0) or 0),
            slippage_proxy_bps=config.stress_slippage_proxy_bps,
        )
        risk_net = risk_stress_raw - risk_funding_drag - risk_slippage_drag
        decisions = build_tsmom_decision_frame(
            window.holdout,
            settings=stable_selected.settings,
            funding_frame=funding_frame,
        )
        decisions = decisions.copy()
        decisions["position"] = _float_series(decisions["position"], index=decisions.index) * multiplier
        regime = _window_regime_diagnostics(window.holdout, decisions, funding_frame, config=config)
        stable_switched = (
            previous_stable is not None and previous_stable.candidate_id != stable_selected.candidate_id
        )
        baseline_switched = (
            previous_baseline is not None
            and previous_baseline.get("selected_candidate_id") != source_row["selected_candidate_id"]
        )
        switch_causes = _risk_switch_causes(
            previous_stable.settings if previous_stable is not None else None,
            stable_selected.settings,
            score_gap=score_gap,
            previous_market_regime=previous_stable_regime,
            market_regime=str(regime["market_regime"]),
            switched=stable_switched,
        )
        switch_rows.append(
            {
                "window_id": window.window_id,
                "holdout_start": source_row["holdout_start"],
                "holdout_end": source_row["holdout_end"],
                "baseline_selected_candidate_id": source_row["selected_candidate_id"],
                "stable_selected_candidate_id": stable_selected.candidate_id,
                "baseline_switched": baseline_switched,
                "stable_switched": stable_switched,
                "sticky_selection_triggered": sticky_selection_triggered,
                "top_candidate_id": top.candidate_id,
                "previous_stable_candidate_id": previous_stable.candidate_id if previous_stable is not None else None,
                "runner_up_candidate_id": runner_up.candidate_id if runner_up is not None else None,
                "top_validation_score": top.score,
                "stable_validation_score": stable_selected.score,
                "top_vs_runner_up_score_gap": score_gap,
                "top_vs_previous_stable_score_gap": top_vs_previous_stable_gap,
                "score_margin_to_top": float(top.score - stable_selected.score),
                "runner_up_near_tie_candidate": bool(
                    score_gap is not None and abs(score_gap) <= config.stable_selection_score_margin
                ),
                "sticky_selection_eligible": bool(
                    top_vs_previous_stable_gap is not None
                    and top_vs_previous_stable_gap <= config.stable_selection_score_margin
                ),
                "switch_causes": ";".join(switch_causes) if switch_causes else "no_switch",
                "market_regime": regime["market_regime"],
                "volatility_regime": regime["volatility_regime"],
                "funding_regime": regime["funding_regime"],
            }
        )
        throttle_rows.append(
            {
                "window_id": window.window_id,
                "holdout_start": source_row["holdout_start"],
                "holdout_end": source_row["holdout_end"],
                "baseline_selected_candidate_id": source_row["selected_candidate_id"],
                "risk_selected_candidate_id": stable_selected.candidate_id,
                "baseline_gross_return_pct": source_row["base_holdout_return_pct"],
                "baseline_stress_raw_return_pct": source_row["stress_holdout_raw_return_pct"],
                "baseline_funding_drag_pct": source_row["stress_funding_drag_pct"],
                "baseline_slippage_drag_pct": source_row["stress_slippage_drag_pct"],
                "baseline_stress_net_return_pct": source_row["funding_aware_net_return_pct"],
                "risk_gross_return_pct": _metric_float(risk_base_holdout.metrics, "Return_pct"),
                "risk_stress_raw_return_pct": risk_stress_raw,
                "risk_funding_drag_pct": risk_funding_drag,
                "risk_slippage_drag_pct": risk_slippage_drag,
                "risk_stress_net_return_pct": risk_net,
                "risk_delta_net_return_pct": risk_net - float(source_row["funding_aware_net_return_pct"]),
                "risk_max_drawdown_pct": _metric_float(risk_stress_holdout.metrics, "Max_Drawdown_pct"),
                "risk_closed_trades": int(risk_stress_holdout.metrics.get("closed_trades", 0) or 0),
                "risk_exposure_time_pct": _metric_float(risk_stress_holdout.metrics, "Exposure_Time_pct"),
                "funding_throttle_bars": int((multiplier < 1.0).sum()),
                "funding_throttle_bar_pct": float((multiplier < 1.0).mean() * 100.0) if len(multiplier) else 0.0,
                "funding_throttle_abs_rate_threshold": config.funding_drag_throttle_abs_rate,
                "funding_throttle_multiplier": config.funding_drag_throttle_multiplier,
                "cash_null_return_pct": 0.0,
                "risk_beats_cash_null": risk_net > 0.0,
                "market_regime": regime["market_regime"],
                "volatility_regime": regime["volatility_regime"],
                "funding_regime": regime["funding_regime"],
            }
        )
        previous_baseline = source_row
        previous_stable = stable_selected
        previous_stable_regime = str(regime["market_regime"])

    compute_profile = dict(source_payload["compute_profile"])
    compute_profile["risk_variant_holdout_evaluations"] = len(throttle_rows) * 2
    compute_profile["engine"] = "vectorized_paper_only_tsmom_switch_stability_funding_throttle_risk_loop"
    risk_hard_gates = _risk_hard_gates(
        risk_rows=throttle_rows,
        stable_ids=stable_ids,
        compute_profile=compute_profile,
        funding_validated=funding_validated,
        config=config,
    )
    failed_risk_gates = [name for name, gate in risk_hard_gates.items() if not bool(gate["passed"])]
    verdict = _risk_loop_verdict(windows=windows, failed_gates=failed_risk_gates)
    loop_id = f"{datetime.now(UTC).strftime('%Y%m%d')}-{TSMOM_RISK_LOOP_SLUG}"
    return {
        "schema_version": TSMOM_RISK_SCHEMA_VERSION,
        "loop_id": loop_id,
        "status": "IMPLEMENTED" if throttle_rows else "BLOCKED",
        "loop_type": TSMOM_RISK_LOOP_TYPE,
        "risk_loop_family": TSMOM_RISK_LOOP_ID,
        "source_strategy_family": TSMOM_LOOP_ID,
        "baseline": BASELINE_ARTIFACT,
        "source_loop_artifact": "reports/loops/20260625-tsmom-vol-target-funding-aware-v1/",
        "source_diagnostic_artifact": "reports/loops/20260626-tsmom-stress-switch-funding-diagnostic-v1/",
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "source_tsmom_loop_verdict": source_payload["tsmom_loop_verdict"],
        "source_tsmom_failed_gates": source_payload["failed_gates"],
        "tsmom_risk_loop_verdict": verdict,
        "tsmom_risk_loop_verdict_namespace": TSMOM_RISK_VERDICT_NAMESPACE,
        "loop_contract": {
            "allowed_risk_loop_verdicts": list(TSMOM_RISK_ALLOWED_VERDICTS),
            "risk_loop_verdict_namespace": TSMOM_RISK_VERDICT_NAMESPACE,
            "readiness_verdict": False,
            "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
            "claim_boundary": _risk_claim_boundary(),
        },
        "pre_registration": _risk_pre_registration(config),
        "hypothesis": _risk_pre_registration(config)["hypothesis"],
        "data_scope": source_payload["data_scope"],
        "dataset_provenance": source_payload["dataset_provenance"],
        "funding_coverage": source_payload["funding_coverage"],
        "funding_causality_policy": source_payload["funding_causality_policy"],
        "selection_inputs": ["train", "validation"],
        "selection_excludes": source_payload["selection_excludes"],
        "source_hard_gates": source_payload["hard_gates"],
        "source_failed_hard_gates": {
            name: source_payload["hard_gates"][name]
            for name in source_payload["failed_gates"]
            if name in source_payload["hard_gates"]
        },
        "risk_evaluation_gates": risk_hard_gates,
        "failed_risk_gates": failed_risk_gates,
        "source_failed_gates_preserved": list(source_payload["failed_gates"]),
        "switch_stability_summary": {
            "baseline": _candidate_switch_summary(
                [
                    {
                        "switched": index > 0 and baseline_ids[index] != baseline_ids[index - 1],
                        "switch_causes": "baseline_source_tsmom",
                    }
                    for index in range(len(baseline_ids))
                ]
            ),
            "stable": _candidate_switch_summary(
                [
                    {
                        "switched": index > 0 and stable_ids[index] != stable_ids[index - 1],
                        "switch_causes": str(switch_rows[index]["switch_causes"]),
                    }
                    for index in range(len(stable_ids))
                ]
            ),
            "runner_up_near_tie_window_count": sum(1 for row in switch_rows if bool(row["runner_up_near_tie_candidate"])),
            "sticky_selection_eligible_count": sum(1 for row in switch_rows if bool(row["sticky_selection_eligible"])),
            "sticky_selection_triggered_count": sum(1 for row in switch_rows if bool(row["sticky_selection_triggered"])),
        },
        "funding_throttle_summary": _risk_funding_throttle_summary(throttle_rows, config),
        "regime_breakdown": _risk_regime_breakdown(throttle_rows),
        "compute_profile": compute_profile,
        "candidate_switch_stability": switch_rows,
        "funding_throttle_decomposition": throttle_rows,
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
        "evidence_artifacts": list(TSMOM_RISK_REQUIRED_FILES),
        "changed_files": list(changed_files),
        "claim_boundary": _risk_claim_boundary(),
        "tsmom_risk_loop_decision": "ITERATE_RISK_COST_RESEARCH" if failed_risk_gates else "PAPER_ONLY_RISK_LOOP_PASS",
        "next_loop": (
            "If risk gates remain failed, inspect gross edge and selection stability before any strategy retune; "
            "if all risk gates pass, run a separate readiness process without claiming live readiness."
        ),
    }


def write_tsmom_risk_loop_artifacts(
    output_dir: str | Path,
    payload: dict[str, Any],
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_tsmom_risk_output_dir(output_dir, overwrite=overwrite)
    artifact_payload = dict(payload)
    if out.name.endswith(TSMOM_RISK_LOOP_SLUG):
        artifact_payload["loop_id"] = out.name
    _write_json(out / "risk_loop.json", artifact_payload)
    _atomic_write_text(out / "report.md", render_tsmom_risk_report(artifact_payload))
    pd.DataFrame(artifact_payload.get("candidate_switch_stability", [])).to_csv(
        out / "candidate_switch_stability.csv",
        index=False,
    )
    pd.DataFrame(artifact_payload.get("funding_throttle_decomposition", [])).to_csv(
        out / "funding_throttle_decomposition.csv",
        index=False,
    )
    pd.DataFrame(artifact_payload.get("regime_breakdown", [])).to_csv(out / "regime_breakdown.csv", index=False)
    _atomic_write_text(out / "commands.txt", "\n".join(artifact_payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(artifact_payload.get("changed_files", [])) + "\n")
    return out


def prepare_tsmom_risk_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="tsmom-risk-run",
        known_files=TSMOM_RISK_KNOWN_FILES,
        overwrite=overwrite,
    )


def build_tsmom_gross_edge_loop_payload(
    *,
    bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None = None,
    funding_metadata: dict[str, Any] | None = None,
    config: TsmomLoopConfig = TsmomLoopConfig(),
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> dict[str, Any]:
    source_payload = build_tsmom_loop_payload(
        bundle=bundle,
        funding_bundle=funding_bundle,
        funding_metadata=funding_metadata,
        config=config,
        commands=commands,
        changed_files=changed_files,
    )
    data = _validate_ohlcv_frame(bundle.frame)
    funding_frame = funding_bundle.frame if funding_bundle is not None else None
    windows = _generate_windows(data, config)
    source_rows = {
        str(row["window_id"]): row
        for row in source_payload.get("walk_forward", {}).get("rows", [])
        if isinstance(row, dict)
    }
    gross_rows: list[dict[str, Any]] = []
    null_rows: list[dict[str, Any]] = []
    failed_edge_windows: list[dict[str, Any]] = []

    for window in windows:
        source_row = source_rows.get(window.window_id)
        if source_row is None:
            continue
        settings = _settings_from_window_row(source_row)
        decisions = build_tsmom_decision_frame(
            window.holdout,
            settings=settings,
            funding_frame=funding_frame,
        )
        position = _float_series(decisions["position"], index=decisions.index)
        returns = _gross_edge_bar_returns(window.holdout)
        gross_stats = _gross_edge_position_stats(position, returns, config=config)
        regimes = _gross_edge_regime_labels(
            window=window,
            decisions=decisions,
            source_row=source_row,
            funding_frame=funding_frame,
            config=config,
        )
        gross_return = float(gross_stats["return_pct"])
        net_return = float(source_row["funding_aware_net_return_pct"])
        stress_raw_return = float(source_row["stress_holdout_raw_return_pct"])
        funding_drag = float(source_row["stress_funding_drag_pct"])
        slippage_drag = float(source_row["stress_slippage_drag_pct"])
        buy_hold_return = float(source_row["base_buy_hold_return_pct"])
        gross_row = {
            "window_id": window.window_id,
            "holdout_start": source_row["holdout_start"],
            "holdout_end": source_row["holdout_end"],
            "selected_candidate_id": source_row["selected_candidate_id"],
            "selected_lookback_bars": source_row["selected_lookback_bars"],
            "selected_vol_window_bars": source_row["selected_vol_window_bars"],
            "selected_target_annual_volatility": source_row["selected_target_annual_volatility"],
            "selected_max_exposure": source_row["selected_max_exposure"],
            "selected_funding_abs_rate_limit": source_row["selected_funding_abs_rate_limit"],
            "gross_return_before_costs_pct": gross_return,
            "net_return_after_funding_slippage_stress_pct": net_return,
            "funding_drag_pct": funding_drag,
            "slippage_drag_pct": slippage_drag,
            "stress_drag_pct": gross_return - stress_raw_return,
            "stress_raw_return_pct": stress_raw_return,
            "cash_null_return_pct": 0.0,
            "difference_vs_cash_null_pct": gross_return,
            "buy_and_hold_return_pct": buy_hold_return,
            "difference_vs_buy_and_hold_pct": gross_return - buy_hold_return,
            "direction_correctness_rate": gross_stats["direction_correctness_rate"],
            "average_exposure": gross_stats["average_exposure"],
            "turnover": gross_stats["turnover"],
            "drawdown_pct": gross_stats["max_drawdown_pct"],
            "cvar_tail_loss_pct": gross_stats["cvar_tail_loss_pct"],
            "gross_edge_positive": gross_return > 0.0,
            "cash_null_failure": gross_return <= 0.0,
            **regimes,
        }
        gross_rows.append(gross_row)
        null_rows.extend(
            _gross_edge_null_baseline_rows(
                window=window,
                position=position,
                returns=returns,
                tsmom_gross_return_pct=gross_return,
                config=config,
            )
        )
        if gross_return <= 0.0:
            failed_edge_windows.append(
                {
                    "window_id": window.window_id,
                    "holdout_start": source_row["holdout_start"],
                    "holdout_end": source_row["holdout_end"],
                    "selected_candidate_id": source_row["selected_candidate_id"],
                    "gross_return_before_costs_pct": gross_return,
                    "net_return_after_funding_slippage_stress_pct": net_return,
                    "cash_null_return_pct": 0.0,
                    "difference_vs_cash_null_pct": gross_return,
                    "market_regime": regimes["market_regime"],
                    "volatility_regime": regimes["volatility_regime"],
                    "funding_regime": regimes["funding_regime"],
                    "funding_drag_regime": regimes["funding_drag_regime"],
                    "trend_regime": regimes["trend_regime"],
                    "failure_reason": _gross_edge_failure_reason(gross_row),
                }
            )

    gross_summary = _gross_edge_summary(gross_rows, config)
    null_summary = _gross_edge_null_summary(null_rows)
    regime_breakdown = _gross_edge_regime_breakdown(gross_rows, config=config)
    edge_gates = _gross_edge_evaluation_gates(
        gross_rows=gross_rows,
        null_summary=null_summary,
        source_payload=source_payload,
        config=config,
    )
    failed_edge_gates = [name for name, gate in edge_gates.items() if not bool(gate["passed"])]
    verdict = _gross_edge_loop_verdict(windows=windows, failed_gates=failed_edge_gates)
    loop_id = f"{datetime.now(UTC).strftime('%Y%m%d')}-{TSMOM_GROSS_EDGE_LOOP_SLUG}"
    return {
        "schema_version": TSMOM_GROSS_EDGE_SCHEMA_VERSION,
        "loop_id": loop_id,
        "status": "IMPLEMENTED" if gross_rows else "BLOCKED",
        "loop_type": TSMOM_GROSS_EDGE_LOOP_TYPE,
        "gross_edge_loop_family": TSMOM_GROSS_EDGE_LOOP_ID,
        "source_strategy_family": TSMOM_LOOP_ID,
        "baseline": BASELINE_ARTIFACT,
        "source_loop_artifact": "reports/loops/20260625-tsmom-vol-target-funding-aware-v1/",
        "source_risk_artifact": "reports/loops/20260626-tsmom-switch-stability-funding-throttle-risk-v1/",
        "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
        "source_tsmom_loop_verdict": source_payload["tsmom_loop_verdict"],
        "source_tsmom_failed_gates": source_payload["failed_gates"],
        "tsmom_gross_edge_loop_verdict": verdict,
        "tsmom_gross_edge_loop_verdict_namespace": TSMOM_GROSS_EDGE_VERDICT_NAMESPACE,
        "loop_contract": {
            "allowed_gross_edge_loop_verdicts": list(TSMOM_GROSS_EDGE_ALLOWED_VERDICTS),
            "gross_edge_loop_verdict_namespace": TSMOM_GROSS_EDGE_VERDICT_NAMESPACE,
            "readiness_verdict": False,
            "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
            "claim_boundary": _gross_edge_claim_boundary(),
        },
        "pre_registration": _gross_edge_pre_registration(config),
        "hypothesis": _gross_edge_pre_registration(config)["hypothesis"],
        "data_scope": source_payload["data_scope"],
        "dataset_provenance": source_payload["dataset_provenance"],
        "funding_coverage": source_payload["funding_coverage"],
        "funding_causality_policy": source_payload["funding_causality_policy"],
        "selection_inputs": source_payload["selection_inputs"],
        "selection_excludes": source_payload["selection_excludes"],
        "source_hard_gates": source_payload["hard_gates"],
        "source_failed_hard_gates": {
            name: source_payload["hard_gates"][name]
            for name in source_payload["failed_gates"]
            if name in source_payload["hard_gates"]
        },
        "source_failed_gates_preserved": list(source_payload["failed_gates"]),
        "gross_edge_evaluation_gates": edge_gates,
        "failed_gross_edge_gates": failed_edge_gates,
        "gross_edge_summary": gross_summary,
        "null_robustness_summary": null_summary,
        "regime_edge_summary": _gross_edge_regime_summary(regime_breakdown),
        "compute_profile": source_payload["compute_profile"],
        "gross_edge_decomposition": gross_rows,
        "regime_edge_breakdown": regime_breakdown,
        "null_baseline_comparison": null_rows,
        "failed_edge_windows": failed_edge_windows,
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
        "evidence_artifacts": list(TSMOM_GROSS_EDGE_REQUIRED_FILES),
        "changed_files": list(changed_files),
        "claim_boundary": _gross_edge_claim_boundary(),
        "tsmom_gross_edge_loop_decision": (
            "PAPER_ONLY_GROSS_EDGE_PASS" if not failed_edge_gates else "WATCH_OR_NARROW_OR_DEPRECATE_TSMOM"
        ),
        "next_loop": (
            "If gross edge remains weak versus null baselines, pre-register either a narrower regime-specific "
            "falsification loop or park/deprecate this TSMOM family; do not retune on holdout."
        ),
    }


def write_tsmom_gross_edge_loop_artifacts(
    output_dir: str | Path,
    payload: dict[str, Any],
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_tsmom_gross_edge_output_dir(output_dir, overwrite=overwrite)
    artifact_payload = dict(payload)
    if out.name.endswith(TSMOM_GROSS_EDGE_LOOP_SLUG):
        artifact_payload["loop_id"] = out.name
    _write_json(out / "gross_edge_loop.json", artifact_payload)
    _atomic_write_text(out / "report.md", render_tsmom_gross_edge_report(artifact_payload))
    pd.DataFrame(artifact_payload.get("gross_edge_decomposition", [])).to_csv(
        out / "gross_edge_decomposition.csv",
        index=False,
    )
    pd.DataFrame(artifact_payload.get("regime_edge_breakdown", [])).to_csv(
        out / "regime_edge_breakdown.csv",
        index=False,
    )
    pd.DataFrame(artifact_payload.get("null_baseline_comparison", [])).to_csv(
        out / "null_baseline_comparison.csv",
        index=False,
    )
    pd.DataFrame(artifact_payload.get("failed_edge_windows", [])).to_csv(
        out / "failed_edge_windows.csv",
        index=False,
    )
    _atomic_write_text(out / "commands.txt", "\n".join(artifact_payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(artifact_payload.get("changed_files", [])) + "\n")
    return out


def prepare_tsmom_gross_edge_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="tsmom-gross-edge-run",
        known_files=TSMOM_GROSS_EDGE_KNOWN_FILES,
        overwrite=overwrite,
    )


def render_tsmom_gross_edge_report(payload: dict[str, Any]) -> str:
    summary = payload["gross_edge_summary"]
    null_summary = payload["null_robustness_summary"]
    gates = payload["gross_edge_evaluation_gates"]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Hypothesis",
        payload["hypothesis"],
        "",
        "## Claim boundary",
        f"- tsmom_gross_edge_loop_verdict: {payload['tsmom_gross_edge_loop_verdict']}",
        f"- namespace: {payload['tsmom_gross_edge_loop_verdict_namespace']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- source_tsmom_loop_verdict: {payload['source_tsmom_loop_verdict']}",
        f"- claim_boundary: {payload['claim_boundary']}",
        "",
        "## Pre-registration",
        f"- allowed_inputs: {', '.join(payload['pre_registration']['allowed_inputs'])}",
        f"- forbidden_inputs: {', '.join(payload['pre_registration']['forbidden_inputs'])}",
        f"- holdout_boundary: {payload['pre_registration']['holdout_boundary']}",
        "",
        "## Source failed gates preserved",
        f"- {', '.join(payload['source_failed_gates_preserved']) if payload['source_failed_gates_preserved'] else 'none'}",
        "",
        "## Gross edge summary",
        f"- window_count: {summary['window_count']}",
        f"- median_gross_return_pct: {summary['median_gross_return_pct']}",
        f"- gross_edge_hit_rate: {summary['gross_edge_hit_rate']}",
        f"- median_net_return_pct: {summary['median_net_return_pct']}",
        f"- cash_null_failure_count: {summary['cash_null_failure_count']}",
        f"- median_difference_vs_buy_and_hold_pct: {summary['median_difference_vs_buy_and_hold_pct']}",
        f"- cvar_tail_gross_return_pct: {summary['cvar_tail_gross_return_pct']}",
        "",
        "## Gross edge gates",
    ]
    for name, gate in gates.items():
        lines.append(
            f"- {name}: {str(gate.get('status', 'failed')).upper()} "
            f"(observed={gate.get('observed')}, threshold={gate.get('threshold')})"
        )
    lines.extend(
        [
            "",
            "## Null robustness",
        ]
    )
    for baseline, values in null_summary.get("by_baseline", {}).items():
        lines.append(
            f"- {baseline}: median_baseline_return_pct={values['median_baseline_return_pct']}, "
            f"median_tsmom_minus_baseline_return_pct={values['median_tsmom_minus_baseline_return_pct']}, "
            f"baseline_beats_or_matches_tsmom_count={values['baseline_beats_or_matches_tsmom_count']}"
        )
    lines.extend(
        [
            "",
            "## Regime gross edge",
        ]
    )
    for regime in payload["regime_edge_breakdown"]:
        lines.append(
            f"- {regime['regime_axis']}={regime['regime_label']}: count={regime['window_count']}, "
            f"median_gross={regime['median_gross_return_pct']}, hit_rate={regime['gross_edge_hit_rate']}, "
            f"gate={regime['regime_gate_status']}"
        )
    lines.extend(
        [
            "",
            "## Safety",
            "- live_orders_submitted: false",
            "- testnet_orders_submitted: false",
            "- api_keys_required_for_research: false",
            "- account_or_fill_data_used: false",
            "- strategy_parameters_retuned: false",
            "- gates_relaxed: false",
            "- live_trading_path_introduced: false",
            "",
            "## Artifacts",
            "- gross_edge_loop.json",
            "- gross_edge_decomposition.csv",
            "- regime_edge_breakdown.csv",
            "- null_baseline_comparison.csv",
            "- failed_edge_windows.csv",
            "- commands.txt",
            "- changed_files.txt",
            "",
            "## Next loop",
            payload["next_loop"],
        ]
    )
    return "\n".join(lines) + "\n"


def build_tsmom_candidate_family_decision_payload(
    *,
    source_loop_artifact: str | Path = DEFAULT_TSMOM_SOURCE_LOOP_ARTIFACT,
    diagnostic_artifact: str | Path = DEFAULT_TSMOM_DIAGNOSTIC_ARTIFACT,
    risk_artifact: str | Path = DEFAULT_TSMOM_RISK_ARTIFACT,
    gross_edge_artifact: str | Path = DEFAULT_TSMOM_GROSS_EDGE_ARTIFACT,
    commands: Iterable[str] = (),
    changed_files: Iterable[str] = (),
) -> dict[str, Any]:
    source_path = Path(source_loop_artifact)
    diagnostic_path = Path(diagnostic_artifact)
    risk_path = Path(risk_artifact)
    gross_edge_path = Path(gross_edge_artifact)
    source_payload = _read_json_artifact(source_path, required_key="tsmom_loop_verdict")
    diagnostic_payload = _read_json_artifact(diagnostic_path, required_key="tsmom_diagnostic_loop_verdict")
    risk_payload = _read_json_artifact(risk_path, required_key="tsmom_risk_loop_verdict")
    gross_edge_payload = _read_json_artifact(gross_edge_path, required_key="tsmom_gross_edge_loop_verdict")

    evidence_inputs = _family_evidence_inputs(
        source_path=source_path,
        diagnostic_path=diagnostic_path,
        risk_path=risk_path,
        gross_edge_path=gross_edge_path,
    )
    failed_gate_rows = _family_failed_gate_rollup(
        source_payload=source_payload,
        diagnostic_payload=diagnostic_payload,
        risk_payload=risk_payload,
        gross_edge_payload=gross_edge_payload,
    )
    evidence_rows = _family_evidence_rollup(
        source_payload=source_payload,
        diagnostic_payload=diagnostic_payload,
        risk_payload=risk_payload,
        gross_edge_payload=gross_edge_payload,
    )
    regime_rows = _family_regime_exception_review(gross_edge_payload)
    decision_gates = _family_decision_gates(
        gross_edge_payload=gross_edge_payload,
        risk_payload=risk_payload,
        regime_rows=regime_rows,
    )
    family_decision = _family_decision_from_gates(decision_gates)
    loop_id = f"{datetime.now(UTC).strftime('%Y%m%d')}-{TSMOM_FAMILY_DECISION_LOOP_SLUG}"
    return {
        "schema_version": TSMOM_FAMILY_DECISION_SCHEMA_VERSION,
        "loop_id": loop_id,
        "status": "IMPLEMENTED",
        "loop_type": TSMOM_FAMILY_DECISION_LOOP_TYPE,
        "candidate_family_decision_loop": TSMOM_FAMILY_DECISION_LOOP_ID,
        "source_strategy_family": TSMOM_LOOP_ID,
        "baseline": BASELINE_ARTIFACT,
        "readiness_baseline_verdict": _family_readiness_baseline(
            source_payload,
            diagnostic_payload,
            risk_payload,
            gross_edge_payload,
        ),
        "source_tsmom_loop_verdict": source_payload["tsmom_loop_verdict"],
        "tsmom_risk_loop_verdict": risk_payload["tsmom_risk_loop_verdict"],
        "tsmom_gross_edge_loop_verdict": gross_edge_payload["tsmom_gross_edge_loop_verdict"],
        "tsmom_candidate_family_decision": family_decision,
        "tsmom_candidate_family_decision_namespace": TSMOM_FAMILY_DECISION_NAMESPACE,
        "candidate_queue_status": _family_candidate_queue_status(family_decision),
        "loop_contract": {
            "allowed_decisions": list(TSMOM_FAMILY_DECISION_ALLOWED_DECISIONS),
            "candidate_family_decision_namespace": TSMOM_FAMILY_DECISION_NAMESPACE,
            "readiness_verdict": False,
            "readiness_baseline_verdict": READINESS_BASELINE_VERDICT,
            "claim_boundary": _family_decision_claim_boundary(),
        },
        "pre_registration": _family_decision_pre_registration(),
        "hypothesis": _family_decision_pre_registration()["decision_purpose"],
        "evidence_inputs": evidence_inputs,
        "decision_gates": decision_gates,
        "failed_decision_gates": [name for name, gate in decision_gates.items() if not bool(gate["passed"])],
        "evidence_rollup": evidence_rows,
        "failed_gate_rollup": failed_gate_rows,
        "regime_exception_review": regime_rows,
        "evidence_summary": _family_evidence_summary(evidence_rows=evidence_rows, regime_rows=regime_rows),
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
        "evidence_artifacts": list(TSMOM_FAMILY_DECISION_REQUIRED_FILES),
        "changed_files": list(changed_files),
        "claim_boundary": _family_decision_claim_boundary(),
        "tsmom_candidate_family_decision_rationale": _family_decision_rationale_text(
            family_decision=family_decision,
            decision_gates=decision_gates,
            evidence_rows=evidence_rows,
            regime_rows=regime_rows,
        ),
        "next_loop": _family_next_loop(family_decision),
    }


def write_tsmom_candidate_family_decision_artifacts(
    output_dir: str | Path,
    payload: dict[str, Any],
    *,
    overwrite: bool = False,
) -> Path:
    out = prepare_tsmom_family_decision_output_dir(output_dir, overwrite=overwrite)
    artifact_payload = dict(payload)
    if out.name.endswith(TSMOM_FAMILY_DECISION_LOOP_SLUG):
        artifact_payload["loop_id"] = out.name
    _write_json(out / "candidate_family_decision.json", artifact_payload)
    _atomic_write_text(out / "report.md", render_tsmom_candidate_family_decision_report(artifact_payload))
    pd.DataFrame(artifact_payload.get("evidence_rollup", [])).to_csv(out / "evidence_rollup.csv", index=False)
    pd.DataFrame(artifact_payload.get("failed_gate_rollup", [])).to_csv(out / "failed_gate_rollup.csv", index=False)
    pd.DataFrame(artifact_payload.get("regime_exception_review", [])).to_csv(
        out / "regime_exception_review.csv",
        index=False,
    )
    _atomic_write_text(out / "decision_rationale.md", artifact_payload["tsmom_candidate_family_decision_rationale"])
    _atomic_write_text(out / "commands.txt", "\n".join(artifact_payload.get("validation_commands", [])) + "\n")
    _atomic_write_text(out / "changed_files.txt", "\n".join(artifact_payload.get("changed_files", [])) + "\n")
    return out


def prepare_tsmom_family_decision_output_dir(output_dir: str | Path, *, overwrite: bool = False) -> Path:
    return prepare_loop_output_dir(
        output_dir,
        run_type="tsmom-family-decision-run",
        known_files=TSMOM_FAMILY_DECISION_KNOWN_FILES,
        overwrite=overwrite,
    )


def render_tsmom_candidate_family_decision_report(payload: dict[str, Any]) -> str:
    summary = payload["evidence_summary"]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Decision purpose",
        payload["pre_registration"]["decision_purpose"],
        "",
        "## Claim and readiness boundary",
        f"- tsmom_candidate_family_decision: {payload['tsmom_candidate_family_decision']}",
        f"- namespace: {payload['tsmom_candidate_family_decision_namespace']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- source_tsmom_loop_verdict: {payload['source_tsmom_loop_verdict']}",
        f"- tsmom_risk_loop_verdict: {payload['tsmom_risk_loop_verdict']}",
        f"- tsmom_gross_edge_loop_verdict: {payload['tsmom_gross_edge_loop_verdict']}",
        f"- claim_boundary: {payload['claim_boundary']}",
        "",
        "## Evidence rollup",
        f"- median_gross_return_pct: {summary['median_gross_return_pct']}",
        f"- gross_edge_hit_rate: {summary['gross_edge_hit_rate']}",
        f"- median_net_stress_return_pct: {summary['median_net_stress_return_pct']}",
        f"- diagnostic_cash_null_failure_windows: {summary['diagnostic_cash_null_failure_windows']}",
        f"- gross_edge_cash_null_failure_windows: {summary['gross_edge_cash_null_failure_windows']}",
        f"- baseline_switch_rate: {summary['baseline_switch_rate']}",
        f"- stable_switch_rate: {summary['stable_switch_rate']}",
        f"- null_baselines_matching_or_beating_tsmom: {summary['null_baselines_matching_or_beating_tsmom']}",
        f"- regime_exception_candidates: {', '.join(summary['regime_exception_candidates']) if summary['regime_exception_candidates'] else 'none'}",
        "",
        "## Decision gates",
    ]
    for name, gate in payload["decision_gates"].items():
        lines.append(
            f"- {name}: {str(gate.get('status', 'failed')).upper()} "
            f"(observed={gate.get('observed')}, threshold={gate.get('threshold')})"
        )
    lines.extend(
        [
            "",
            "## Candidate queue status",
            f"- {payload['candidate_queue_status']}",
            "",
            "## Safety",
            "- live_orders_submitted: false",
            "- testnet_orders_submitted: false",
            "- api_keys_required_for_research: false",
            "- account_or_fill_data_used: false",
            "- holdout_used_for_selection: false",
            "- strategy_parameters_retuned: false",
            "- gates_relaxed: false",
            "- live_trading_path_introduced: false",
            "",
            "## Artifacts",
            "- candidate_family_decision.json",
            "- evidence_rollup.csv",
            "- failed_gate_rollup.csv",
            "- regime_exception_review.csv",
            "- decision_rationale.md",
            "- commands.txt",
            "- changed_files.txt",
            "",
            "## Next loop",
            payload["next_loop"],
        ]
    )
    return "\n".join(lines) + "\n"


def render_tsmom_risk_report(payload: dict[str, Any]) -> str:
    gates = payload["risk_evaluation_gates"]
    switch = payload["switch_stability_summary"]
    funding = payload["funding_throttle_summary"]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Hypothesis",
        payload["hypothesis"],
        "",
        "## Claim boundary",
        f"- tsmom_risk_loop_verdict: {payload['tsmom_risk_loop_verdict']}",
        f"- namespace: {payload['tsmom_risk_loop_verdict_namespace']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- source_tsmom_loop_verdict: {payload['source_tsmom_loop_verdict']}",
        f"- claim_boundary: {payload['claim_boundary']}",
        "",
        "## Pre-registration",
        f"- allowed_inputs: {', '.join(payload['pre_registration']['allowed_inputs'])}",
        f"- forbidden_inputs: {', '.join(payload['pre_registration']['forbidden_inputs'])}",
        f"- holdout_boundary: {payload['pre_registration']['holdout_boundary']}",
        "",
        "## Source failed gates preserved",
        f"- {', '.join(payload['source_failed_gates_preserved']) if payload['source_failed_gates_preserved'] else 'none'}",
        "",
        "## Risk evaluation gates",
    ]
    for name in HARD_GATE_NAMES:
        gate = gates[name]
        lines.append(
            f"- {name}: {str(gate.get('status', 'failed')).upper()} "
            f"(observed={gate.get('observed')}, threshold={gate.get('threshold')})"
        )
    lines.extend(
        [
            "",
            "## Switch stability",
            f"- baseline_switch_count: {switch['baseline']['switch_count']}",
            f"- baseline_switch_rate: {switch['baseline']['switch_rate']}",
            f"- stable_switch_count: {switch['stable']['switch_count']}",
            f"- stable_switch_rate: {switch['stable']['switch_rate']}",
            f"- runner_up_near_tie_window_count: {switch['runner_up_near_tie_window_count']}",
            f"- sticky_selection_eligible_count: {switch['sticky_selection_eligible_count']}",
            f"- sticky_selection_triggered_count: {switch['sticky_selection_triggered_count']}",
            "",
            "## Funding throttle comparison",
            f"- median_baseline_net_return_pct: {funding['median_baseline_net_return_pct']}",
            f"- median_risk_net_return_pct: {funding['median_risk_net_return_pct']}",
            f"- median_delta_net_return_pct: {funding['median_delta_net_return_pct']}",
            f"- median_risk_funding_drag_pct: {funding['median_risk_funding_drag_pct']}",
            f"- throttle_active_window_count: {funding['throttle_active_window_count']}",
            f"- max_throttle_bar_pct: {funding['max_throttle_bar_pct']}",
            f"- risk_tail_cvar_pct: {funding['risk_tail_cvar_pct']}",
            f"- risk_cash_null_failure_count: {funding['risk_cash_null_failure_count']}",
            "",
            "## Safety",
            "- live_orders_submitted: false",
            "- testnet_orders_submitted: false",
            "- api_keys_required_for_research: false",
            "- account_or_fill_data_used: false",
            "- strategy_parameters_retuned: false",
            "- gates_relaxed: false",
            "- live_trading_path_introduced: false",
            "",
            "## Artifacts",
            "- risk_loop.json",
            "- candidate_switch_stability.csv",
            "- funding_throttle_decomposition.csv",
            "- regime_breakdown.csv",
            "- commands.txt",
            "- changed_files.txt",
            "",
            "## Next loop",
            payload["next_loop"],
        ]
    )
    return "\n".join(lines) + "\n"


def render_tsmom_diagnostic_report(payload: dict[str, Any]) -> str:
    summary = payload["diagnostic_summary"]
    stress = summary["stress_return_distribution"]
    switch = summary["candidate_switch_summary"]
    funding = summary["funding_decomposition_summary"]
    source = payload["source_tsmom_loop"]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Hypothesis",
        payload["hypothesis"],
        "",
        "## Claim boundary",
        f"- tsmom_diagnostic_loop_verdict: {payload['tsmom_diagnostic_loop_verdict']}",
        f"- namespace: {payload['tsmom_diagnostic_loop_verdict_namespace']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        f"- This is diagnostic-only, not readiness; claim_boundary: {payload['claim_boundary']}",
        "",
        "## Source failed gates",
        f"- source_tsmom_loop_verdict: {source['tsmom_loop_verdict']}",
        f"- source_tsmom_loop_decision: {source['tsmom_loop_decision']}",
        f"- analyzed_failed_gates: {', '.join(payload['analyzed_failed_gates'])}",
        "",
        "## Median stress return vs cash/null",
        f"- count: {stress['count']}",
        f"- median_net_return_pct: {stress['median_net_return_pct']}",
        f"- mean_net_return_pct: {stress['mean_net_return_pct']}",
        f"- worst_net_return_pct: {stress['worst_net_return_pct']}",
        f"- cvar_tail_net_return_pct: {stress['cvar_tail_net_return_pct']}",
        f"- failed_window_count: {summary['cash_null_failure_window_count']}",
        "",
        "## Candidate switch diagnostic",
        f"- switch_count: {switch['switch_count']}",
        f"- switch_rate: {switch['switch_rate']}",
        f"- top_switch_causes: {', '.join(switch['top_switch_causes']) if switch['top_switch_causes'] else 'none'}",
        "",
        "## Funding-aware net return diagnostic",
        f"- median_gross_return_pct: {funding['median_gross_return_pct']}",
        f"- median_funding_cost_drag_pct: {funding['median_funding_cost_drag_pct']}",
        f"- median_slippage_drag_pct: {funding['median_slippage_drag_pct']}",
        f"- median_net_return_pct: {funding['median_net_return_pct']}",
        f"- primary_failure_modes: {', '.join(funding['primary_failure_modes']) if funding['primary_failure_modes'] else 'none'}",
        "",
        "## Regime decomposition",
    ]
    for regime, values in summary["regime_decomposition_summary"].items():
        lines.append(
            f"- {regime}: count={values['count']}, median_net={values['median_net_return_pct']}, "
            f"failure_rate={values['cash_null_failure_rate']}"
        )
    lines.extend(
        [
            "",
            "## Artifacts",
            "- diagnostic.json",
            "- failed_windows.csv",
            "- candidate_switches.csv",
            "- funding_decomposition.csv",
            "- commands.txt",
            "- changed_files.txt",
            "",
            "## Safety",
            "- live_orders_submitted: false",
            "- testnet_orders_submitted: false",
            "- api_keys_required_for_research: false",
            "- account_or_fill_data_used: false",
            "- strategy_parameters_retuned: false",
            "",
            "## Next pre-registered loop recommendation",
            payload["next_pre_registered_loop_recommendation"],
        ]
    )
    return "\n".join(lines) + "\n"


def render_tsmom_report(payload: dict[str, Any]) -> str:
    data_scope = payload["data_scope"]
    hard_gates = payload["hard_gates"]
    failed_gates = payload["failed_gates"]
    lines = [
        f"# Quant Loop Report: {payload['loop_id']}",
        "",
        "## Hypothesis",
        payload["hypothesis"],
        "",
        "## Baseline",
        f"- baseline: {payload['baseline']}",
        f"- readiness_baseline_verdict: {payload['readiness_baseline_verdict']}",
        "- baseline readiness remains NOT_READY unless a separate readiness report passes all gates.",
        "",
        "## Loop verdict boundary",
        f"- tsmom_loop_verdict: {payload['tsmom_loop_verdict']}",
        f"- tsmom_loop_verdict_namespace: {payload['tsmom_loop_verdict_namespace']}",
        f"- allowed loop verdicts: {', '.join(payload['loop_contract']['allowed_loop_verdicts'])}",
        "- This is a loop-only paper validation verdict, not a readiness verdict or live authorization.",
        "",
        "## Data and selection boundary",
        f"- source: {data_scope['source_name']}",
        f"- symbol/timeframe: {data_scope['symbol']} / {data_scope['timeframe']}",
        f"- UTC range: {data_scope['first_timestamp']} to {data_scope['last_timestamp']}",
        f"- row count: {data_scope['row_count']}",
        f"- months: {', '.join(data_scope['months'])}",
        f"- normalized OHLCV sha256: {data_scope['normalized_ohlcv_sha256']}",
        f"- funding coverage: {payload['funding_coverage']['status']}",
        f"- funding causality: {payload['funding_causality_policy']}",
        f"- selection inputs: {', '.join(payload['selection_inputs'])}",
        f"- selection excludes: {', '.join(payload['selection_excludes'])}",
        "",
        "## Pre-registered grid",
        f"- strategy family: {payload['strategy_family']}",
        f"- candidate count: {payload['pre_registered_grid']['candidate_count']}",
        f"- selection metric: {payload['selection_metric']['name']}",
        "- holdout used for selection: false",
        "",
        "## Hard gates",
    ]
    for name in HARD_GATE_NAMES:
        gate = hard_gates[name]
        gate_label = str(gate.get("status", "failed")).upper()
        lines.append(
            f"- {name}: {gate_label} "
            f"(observed={gate.get('observed')}, threshold={gate.get('threshold')}, "
            f"status={gate.get('status')})"
        )
    lines.extend(
        [
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
            f"- walk-forward windows: {payload['walk_forward']['window_count']}",
            f"- candidate-window evaluations: {payload['compute_profile']['candidate_window_evaluations']}",
            f"- bars processed estimate: {payload['compute_profile']['bars_processed_estimate']}",
            f"- selected candidate switch rate: {hard_gates['selected_candidate_switch_rate']['observed']}",
            f"- tail-loss CVaR pct: {hard_gates['tail_loss_cvar_pct']['observed']}",
            "",
            "## Result interpretation",
            f"- What passed: {', '.join(name for name, gate in hard_gates.items() if gate['passed']) or 'none'}",
            f"- What failed: {', '.join(failed_gates) if failed_gates else 'none'}",
            "- Evidence depth used: public closed-bar OHLCV and public funding metadata where available.",
            "- Cannot claim: live readiness, profit, trading advice, exchange matching, or live/testnet performance.",
            "",
            "## TSMOM loop decision",
            f"- tsmom_loop_decision: {payload['tsmom_loop_decision']}",
            "",
            "## Why",
            f"- claim boundary: {payload['claim_boundary']}",
            "- No live/testnet/account/API-key/order/fill data supports these performance claims.",
            "",
            "## Next loop",
            payload["next_loop"],
        ]
    )
    return "\n".join(lines) + "\n"


def _diagnostic_claim_boundary() -> str:
    return (
        "TSMOM_STRESS_SWITCH_FUNDING_DIAGNOSTIC_V1 is a paper-only diagnostic loop, "
        "not readiness or live readiness, not trading advice, not a profit guarantee, "
        "not strategy parameter optimization, not permission for live orders or testnet orders, "
        "and it uses no account, API-key, order, or fill data for performance claims"
    )


def _risk_claim_boundary() -> str:
    return (
        "TSMOM_SWITCH_STABILITY_FUNDING_THROTTLE_RISK_V1 is a pre-registered paper-only "
        "risk/cost research loop, not readiness or live readiness, not trading advice, "
        "not a profit guarantee, not execution readiness, not permission for live orders "
        "or testnet orders, and it uses no account, API-key, private order, or fill data "
        "for performance claims"
    )


def _risk_pre_registration(config: TsmomLoopConfig) -> dict[str, Any]:
    return {
        "strategy_family": TSMOM_RISK_LOOP_ID,
        "hypothesis": (
            "A train/validation-only near-tie sticky-selection rule plus a pre-registered "
            "public-funding-drag exposure throttle can reduce selected-candidate switch "
            "instability and improve funding-aware stress robustness without holdout tuning."
        ),
        "allowed_inputs": [
            "public_historical_ohlcv_closed_bars",
            "public_historical_binance_usdm_funding_rates",
            "train_split",
            "validation_split",
            "holdout_for_once_only_evaluation",
        ],
        "forbidden_inputs": [
            "live_orders",
            "testnet_orders",
            "account_state",
            "api_keys",
            "private_orders",
            "fills",
            "signed_endpoints",
            "holdout_for_parameter_selection",
        ],
        "candidate_risk_rules": {
            "near_tie_sticky_selection": {
                "score_margin": config.stable_selection_score_margin,
                "definition": (
                    "If the prior stable candidate is still eligible and the current top "
                    "validation score beats it by no more than this margin, keep the prior "
                    "candidate for stability. Uses train/validation scores only."
                ),
            },
            "funding_drag_exposure_throttle": {
                "abs_public_funding_rate_threshold": config.funding_drag_throttle_abs_rate,
                "exposure_multiplier_when_threshold_exceeded": config.funding_drag_throttle_multiplier,
                "definition": (
                    "On decision bars where causally aligned public funding abs(rate) exceeds "
                    "the frozen threshold, multiply selected exposure by the frozen multiplier."
                ),
            },
        },
        "evaluation_gates": list(HARD_GATE_NAMES),
        "holdout_boundary": "holdout is evaluated once after train/validation-only selection and is never used to tune rules",
        "claim_boundary": _risk_claim_boundary(),
        "expected_failure_modes": [
            "sticky selection reduces switches but does not improve net stress return",
            "funding throttle lowers exposure but gross edge remains weak",
            "cash/null baseline still beats median risk variant returns",
            "tail loss or drawdown worsens under reduced exposure timing",
            "public funding coverage is missing and funding-aware gates remain unclaimed",
        ],
    }


def _gross_edge_claim_boundary() -> str:
    return (
        "TSMOM_GROSS_EDGE_REGIME_NULL_ROBUSTNESS_V1 is a pre-registered paper-only "
        "diagnostic/research loop for gross-edge and null-baseline falsification, not "
        "readiness or live readiness, not trading advice, not a profit guarantee, not "
        "execution readiness, not permission for live orders or testnet orders, and it "
        "uses no live, testnet, account, API-key, private order, or fill data for "
        "performance claims"
    )


def _gross_edge_pre_registration(config: TsmomLoopConfig) -> dict[str, Any]:
    return {
        "strategy_family": TSMOM_GROSS_EDGE_LOOP_ID,
        "source_strategy_family": TSMOM_LOOP_ID,
        "hypothesis": (
            "Before any retuning, the current frozen TSMOM family should show positive gross "
            "return before funding, slippage, and stress costs, and should beat simple null "
            "baselines across public-data-derived regimes; otherwise the family remains WATCH "
            "or should be narrowed or deprecated."
        ),
        "allowed_inputs": [
            "public_historical_ohlcv_closed_bars",
            "public_historical_binance_usdm_funding_rates",
            "train_split",
            "validation_split",
            "holdout_for_once_only_evaluation",
            "source_tsmom_selected_candidates",
        ],
        "forbidden_inputs": [
            "live_orders",
            "testnet_orders",
            "account_state",
            "api_keys",
            "private_orders",
            "fills",
            "signed_endpoints",
            "holdout_for_parameter_selection",
            "live_or_testnet_performance",
        ],
        "regime_definitions": {
            "market_regime": "holdout close-to-close return: bull > 2%, bear < -2%, otherwise sideways",
            "volatility_regime": "annualized holdout realized volatility: high >= 80%, low <= 35%, otherwise normal",
            "funding_regime": "causally aligned public funding mean/abs rate from _window_regime_diagnostics",
            "funding_drag_regime": (
                f"high when pre-registered funding drag >= {config.gross_edge_high_funding_drag_pct}%"
            ),
            "trend_regime": (
                "trend_persistence if validation and holdout close-to-close returns share sign with "
                "meaningful magnitude, trend_reversal if signs oppose, otherwise trend_mixed"
            ),
        },
        "null_baselines": [
            "cash_null",
            "random_sign_deterministic_seed",
            "lagged_signal_24_bars",
            "inverse_signal",
            "buy_and_hold",
        ],
        "evaluation_gates": [
            "median_gross_return_beats_cash_null",
            "gross_edge_hit_rate",
            "median_gross_beats_random_sign_baseline",
            "median_gross_beats_lagged_signal_baseline",
            "median_gross_beats_inverse_signal_baseline",
            "median_gross_beats_buy_and_hold_baseline",
            "source_failed_gates_preserved",
        ],
        "holdout_boundary": (
            "holdout is evaluated once using the source TSMOM train/validation-selected candidate; "
            "null comparisons and regime labels are diagnostic only and never tune source parameters"
        ),
        "claim_boundary": _gross_edge_claim_boundary(),
        "expected_failure_modes": [
            "median gross return is already negative before funding, slippage, and stress costs",
            "gross edge appears only in a small regime subset and is not a broad strategy claim",
            "random, lagged, inverse, or buy-and-hold nulls match or beat the TSMOM gross return",
            "net failures are driven primarily by weak raw signal quality, not only costs",
            "source TSMOM failed gates remain failed or unclaimed and readiness remains NOT_READY",
        ],
    }


def _family_decision_claim_boundary() -> str:
    return (
        "TSMOM_CANDIDATE_FAMILY_DECISION_V1 is a paper-only research-governance loop "
        "that converts prior public-data paper research artifacts into a candidate-family "
        "queue decision. It is not strategy optimization, not readiness or live readiness, "
        "not trading advice, not a profit guarantee, not execution readiness, not permission "
        "for live orders or testnet orders, and it uses no live, testnet, account, API-key, "
        "private order, or fill data for performance claims."
    )


def _family_decision_pre_registration() -> dict[str, Any]:
    return {
        "decision_purpose": (
            "Decide whether the current TSMOM candidate family should remain broad WATCH, "
            "be narrowed to specific regimes, be frozen as a negative-control benchmark, "
            "or be deprecated from active candidate development based only on accumulated "
            "paper-only durable artifacts."
        ),
        "allowed_decisions": list(TSMOM_FAMILY_DECISION_ALLOWED_DECISIONS),
        "evidence_inputs": [
            "TSMOM_VOL_TARGET_FUNDING_AWARE_V1 durable source loop artifact",
            "TSMOM_STRESS_SWITCH_FUNDING_DIAGNOSTIC_V1 durable diagnostic artifact",
            "TSMOM_SWITCH_STABILITY_FUNDING_THROTTLE_RISK_V1 durable risk/cost artifact",
            "TSMOM_GROSS_EDGE_REGIME_NULL_ROBUSTNESS_V1 durable gross-edge artifact",
        ],
        "forbidden_inputs": [
            "live_orders",
            "testnet_orders",
            "account_state",
            "api_keys",
            "private_orders",
            "fills",
            "signed_endpoints",
            "holdout_for_parameter_selection",
            "new_parameter_optimization",
            "live_or_testnet_performance",
        ],
        "decision_gates": [
            "readiness_boundary_not_live_ready",
            "active_keep_watch_gross_edge",
            "active_keep_watch_null_robustness",
            "regime_only_watch_exception",
            "negative_control_utility",
            "deprecate_candidate_family",
            "failed_gates_preserved",
        ],
        "regime_exception_rules": {
            "minimum_windows": 5,
            "minimum_gross_edge_hit_rate": 0.60,
            "minimum_median_gross_return_pct": 0.0,
            "required_status": "regime_gate_status must be passed in the gross-edge artifact",
            "limitation": "regime exception is paper-only and does not prove net profitability or readiness",
        },
        "deprecation_rules": {
            "broad_watch_fails_when": (
                "median gross return is negative, gross edge hit rate is below 50%, or null baselines "
                "frequently match/beat TSMOM"
            ),
            "deprecate_when": (
                "broad WATCH fails, no regime exception passes, and the family is not useful as a "
                "negative-control benchmark"
            ),
        },
        "claim_boundary": _family_decision_claim_boundary(),
        "readiness_boundary": "readiness_baseline_verdict remains NOT_READY unless a separate readiness process proves otherwise",
    }


def _read_json_artifact(path: Path, *, required_key: str) -> dict[str, Any]:
    if not path.exists():
        raise FileNotFoundError(f"required TSMOM evidence artifact is missing: {path}")
    try:
        payload = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        raise ValueError(f"invalid JSON artifact: {path}") from exc
    if not isinstance(payload, dict):
        raise ValueError(f"artifact must be a JSON object: {path}")
    if required_key not in payload:
        raise ValueError(f"artifact {path} is missing required key {required_key!r}")
    return cast(dict[str, Any], payload)


def _artifact_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _family_evidence_inputs(
    *,
    source_path: Path,
    diagnostic_path: Path,
    risk_path: Path,
    gross_edge_path: Path,
) -> list[dict[str, Any]]:
    return [
        {
            "artifact_role": "source_tsmom_loop",
            "path": str(source_path),
            "sha256": _artifact_sha256(source_path),
        },
        {
            "artifact_role": "stress_switch_funding_diagnostic",
            "path": str(diagnostic_path),
            "sha256": _artifact_sha256(diagnostic_path),
        },
        {
            "artifact_role": "switch_stability_funding_throttle_risk",
            "path": str(risk_path),
            "sha256": _artifact_sha256(risk_path),
        },
        {
            "artifact_role": "gross_edge_regime_null_robustness",
            "path": str(gross_edge_path),
            "sha256": _artifact_sha256(gross_edge_path),
        },
    ]


def _family_readiness_baseline(*payloads: dict[str, Any]) -> str:
    observed = {str(payload.get("readiness_baseline_verdict")) for payload in payloads}
    if observed == {READINESS_BASELINE_VERDICT}:
        return READINESS_BASELINE_VERDICT
    return READINESS_BASELINE_VERDICT


def _family_failed_gate_rollup(
    *,
    source_payload: dict[str, Any],
    diagnostic_payload: dict[str, Any],
    risk_payload: dict[str, Any],
    gross_edge_payload: dict[str, Any],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    source_hard_gates = cast(dict[str, Any], source_payload.get("hard_gates", {}))
    for name in source_payload.get("failed_gates", []):
        gate = cast(dict[str, Any], source_hard_gates.get(str(name), {}))
        rows.append(_family_failed_gate_row("source_tsmom_loop", str(name), gate))
    diagnostic_hard_gates = cast(dict[str, Any], diagnostic_payload.get("source_tsmom_loop", {}).get("hard_gates", {}))
    for name in diagnostic_payload.get("analyzed_failed_gates", []):
        gate = cast(dict[str, Any], diagnostic_hard_gates.get(str(name), {}))
        rows.append(_family_failed_gate_row("stress_switch_funding_diagnostic", str(name), gate))
    risk_gates = cast(dict[str, Any], risk_payload.get("risk_evaluation_gates", {}))
    for name in risk_payload.get("failed_risk_gates", []):
        gate = cast(dict[str, Any], risk_gates.get(str(name), {}))
        rows.append(_family_failed_gate_row("switch_stability_funding_throttle_risk", str(name), gate))
    gross_gates = cast(dict[str, Any], gross_edge_payload.get("gross_edge_evaluation_gates", {}))
    for name in gross_edge_payload.get("failed_gross_edge_gates", []):
        gate = cast(dict[str, Any], gross_gates.get(str(name), {}))
        rows.append(_family_failed_gate_row("gross_edge_regime_null_robustness", str(name), gate))
    return rows


def _family_failed_gate_row(loop_name: str, gate_name: str, gate: dict[str, Any]) -> dict[str, Any]:
    return {
        "loop_name": loop_name,
        "failed_gate": gate_name,
        "status": gate.get("status", "failed"),
        "observed": gate.get("observed"),
        "threshold": gate.get("threshold"),
        "preserved_for_decision": True,
    }


def _family_evidence_rollup(
    *,
    source_payload: dict[str, Any],
    diagnostic_payload: dict[str, Any],
    risk_payload: dict[str, Any],
    gross_edge_payload: dict[str, Any],
) -> list[dict[str, Any]]:
    diagnostic_summary = cast(dict[str, Any], diagnostic_payload.get("diagnostic_summary", {}))
    diagnostic_stress = cast(dict[str, Any], diagnostic_summary.get("stress_return_distribution", {}))
    diagnostic_switch = cast(dict[str, Any], diagnostic_summary.get("candidate_switch_summary", {}))
    risk_switch = cast(dict[str, Any], risk_payload.get("switch_stability_summary", {}))
    risk_switch_baseline = cast(dict[str, Any], risk_switch.get("baseline", {}))
    risk_switch_stable = cast(dict[str, Any], risk_switch.get("stable", {}))
    risk_funding = cast(dict[str, Any], risk_payload.get("funding_throttle_summary", {}))
    gross_summary = cast(dict[str, Any], gross_edge_payload.get("gross_edge_summary", {}))
    null_summary = cast(dict[str, Any], gross_edge_payload.get("null_robustness_summary", {})).get("by_baseline", {})
    rows = [
        _family_metric_row("source_loop_verdict", source_payload.get("tsmom_loop_verdict"), "source_tsmom_loop"),
        _family_metric_row("risk_loop_verdict", risk_payload.get("tsmom_risk_loop_verdict"), "risk_loop"),
        _family_metric_row(
            "gross_edge_loop_verdict",
            gross_edge_payload.get("tsmom_gross_edge_loop_verdict"),
            "gross_edge_loop",
        ),
        _family_metric_row(
            "readiness_baseline_verdict",
            gross_edge_payload.get("readiness_baseline_verdict"),
            "gross_edge_loop",
        ),
        _family_metric_row("median_gross_return_pct", gross_summary.get("median_gross_return_pct"), "gross_edge_loop"),
        _family_metric_row("gross_edge_hit_rate", gross_summary.get("gross_edge_hit_rate"), "gross_edge_loop"),
        _family_metric_row(
            "median_net_stress_return_pct",
            risk_funding.get("median_risk_net_return_pct", diagnostic_stress.get("median_net_return_pct")),
            "risk_loop",
        ),
        _family_metric_row(
            "diagnostic_cash_null_failure_windows",
            diagnostic_summary.get("cash_null_failure_window_count"),
            "diagnostic_loop",
        ),
        _family_metric_row(
            "gross_edge_cash_null_failure_windows",
            gross_summary.get("cash_null_failure_count"),
            "gross_edge_loop",
        ),
        _family_metric_row("baseline_switch_rate", risk_switch_baseline.get("switch_rate"), "risk_loop"),
        _family_metric_row("stable_switch_rate", risk_switch_stable.get("switch_rate"), "risk_loop"),
        _family_metric_row("diagnostic_switch_rate", diagnostic_switch.get("switch_rate"), "diagnostic_loop"),
    ]
    if isinstance(null_summary, dict):
        for baseline_name, values in sorted(null_summary.items()):
            baseline_values = cast(dict[str, Any], values)
            rows.append(
                _family_metric_row(
                    f"null_{baseline_name}_beats_or_matches_tsmom_count",
                    baseline_values.get("baseline_beats_or_matches_tsmom_count"),
                    "gross_edge_loop",
                )
            )
            rows.append(
                _family_metric_row(
                    f"null_{baseline_name}_median_tsmom_minus_baseline_return_pct",
                    baseline_values.get("median_tsmom_minus_baseline_return_pct"),
                    "gross_edge_loop",
                )
            )
    return rows


def _family_metric_row(metric_name: str, value: Any, source_artifact: str) -> dict[str, Any]:
    return {
        "metric_name": metric_name,
        "value": value,
        "source_artifact": source_artifact,
        "interpretation": _family_metric_interpretation(metric_name, value),
    }


def _family_metric_interpretation(metric_name: str, value: Any) -> str:
    if metric_name == "median_gross_return_pct":
        return "negative_or_zero_raw_signal_quality" if _safe_float(value) <= 0.0 else "positive_gross_edge_observed"
    if metric_name == "gross_edge_hit_rate":
        return "below_even_hit_rate" if _safe_float(value) < 0.5 else "at_or_above_even_hit_rate"
    if metric_name.endswith("_beats_or_matches_tsmom_count"):
        return "null_baseline_frequently_matches_or_beats_tsmom" if _safe_float(value) >= 16.0 else "null_baseline_not_dominant"
    if metric_name.endswith("_verdict"):
        return "loop_boundary_status_not_readiness"
    return "decision_evidence"


def _family_regime_exception_review(gross_edge_payload: dict[str, Any]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for item in gross_edge_payload.get("regime_edge_breakdown", []):
        row = cast(dict[str, Any], item)
        window_count = int(row.get("window_count", 0) or 0)
        median_gross = _safe_float(row.get("median_gross_return_pct"))
        hit_rate = _safe_float(row.get("gross_edge_hit_rate"))
        qualifies = (
            str(row.get("regime_gate_status")) == "passed"
            and window_count >= 5
            and median_gross > 0.0
            and hit_rate >= 0.60
        )
        rows.append(
            {
                "regime_axis": row.get("regime_axis"),
                "regime_label": row.get("regime_label"),
                "window_count": window_count,
                "median_gross_return_pct": row.get("median_gross_return_pct"),
                "median_net_return_pct": row.get("median_net_return_pct"),
                "gross_edge_hit_rate": row.get("gross_edge_hit_rate"),
                "cash_null_failure_count": row.get("cash_null_failure_count"),
                "regime_gate_status": row.get("regime_gate_status"),
                "qualifies_for_regime_only_watch": qualifies,
                "materiality_note": (
                    "gross-edge exception only; net return/readiness remains unclaimed"
                    if qualifies
                    else "does not meet pre-registered regime exception rule"
                ),
            }
        )
    return rows


def _family_decision_gates(
    *,
    gross_edge_payload: dict[str, Any],
    risk_payload: dict[str, Any],
    regime_rows: list[dict[str, Any]],
) -> dict[str, dict[str, Any]]:
    gross_summary = cast(dict[str, Any], gross_edge_payload.get("gross_edge_summary", {}))
    median_gross = _safe_float(gross_summary.get("median_gross_return_pct"))
    hit_rate = _safe_float(gross_summary.get("gross_edge_hit_rate"))
    null_counts = _family_null_baseline_match_counts(gross_edge_payload)
    frequent_null_match = any(count >= 16 for count in null_counts.values())
    regime_candidates = [row for row in regime_rows if bool(row["qualifies_for_regime_only_watch"])]
    weak_aggregate = median_gross <= 0.0 or hit_rate < 0.5 or frequent_null_match
    negative_control_utility = (
        weak_aggregate
        and bool(gross_edge_payload.get("gross_edge_decomposition"))
        and bool(risk_payload.get("candidate_switch_stability"))
    )
    return {
        "readiness_boundary_not_live_ready": _gate(
            gross_edge_payload.get("readiness_baseline_verdict"),
            "must remain NOT_READY and not emit live readiness",
            gross_edge_payload.get("readiness_baseline_verdict") == READINESS_BASELINE_VERDICT,
        ),
        "active_keep_watch_gross_edge": _gate(
            {"median_gross_return_pct": median_gross, "gross_edge_hit_rate": hit_rate},
            "median gross > 0 and hit rate >= 0.50",
            median_gross > 0.0 and hit_rate >= 0.50,
        ),
        "active_keep_watch_null_robustness": _gate(
            null_counts,
            "no null baseline beats or matches TSMOM in >= 16 of 31 windows",
            not frequent_null_match,
        ),
        "regime_only_watch_exception": _gate(
            [f"{row['regime_axis']}={row['regime_label']}" for row in regime_candidates],
            ">=1 regime with >=5 windows, median gross > 0, hit rate >= 0.60",
            bool(regime_candidates),
        ),
        "negative_control_utility": _gate(
            negative_control_utility,
            "weak aggregate evidence plus durable artifacts for future pipeline validation",
            negative_control_utility,
        ),
        "deprecate_candidate_family": _gate(
            not regime_candidates and not negative_control_utility,
            "broad WATCH fails, no regime exception, and no benchmark utility",
            not regime_candidates and not negative_control_utility,
        ),
        "failed_gates_preserved": _gate(
            gross_edge_payload.get("source_failed_gates_preserved"),
            "source failed gates remain recorded and unrenamed",
            bool(gross_edge_payload.get("source_failed_gates_preserved")),
        ),
    }


def _family_null_baseline_match_counts(gross_edge_payload: dict[str, Any]) -> dict[str, int]:
    by_baseline = cast(dict[str, Any], gross_edge_payload.get("null_robustness_summary", {}).get("by_baseline", {}))
    return {
        str(name): int(cast(dict[str, Any], values).get("baseline_beats_or_matches_tsmom_count", 0) or 0)
        for name, values in by_baseline.items()
    }


def _family_decision_from_gates(decision_gates: dict[str, dict[str, Any]]) -> str:
    keep_gross = bool(decision_gates["active_keep_watch_gross_edge"]["passed"])
    keep_null = bool(decision_gates["active_keep_watch_null_robustness"]["passed"])
    if keep_gross and keep_null:
        return TSMOM_FAMILY_DECISION_KEEP_WATCH
    if bool(decision_gates["regime_only_watch_exception"]["passed"]):
        return TSMOM_FAMILY_DECISION_REGIME_ONLY_WATCH
    if bool(decision_gates["negative_control_utility"]["passed"]):
        return TSMOM_FAMILY_DECISION_FREEZE_NEGATIVE_CONTROL
    return TSMOM_FAMILY_DECISION_DEPRECATE


def _family_candidate_queue_status(family_decision: str) -> str:
    if family_decision == TSMOM_FAMILY_DECISION_KEEP_WATCH:
        return "TSMOM remains in the broad active research queue under WATCH only."
    if family_decision == TSMOM_FAMILY_DECISION_REGIME_ONLY_WATCH:
        return "TSMOM is removed from broad active development and may only continue in pre-registered regime-only WATCH tests."
    if family_decision == TSMOM_FAMILY_DECISION_FREEZE_NEGATIVE_CONTROL:
        return "TSMOM is frozen as a negative-control benchmark, not an active alpha candidate."
    return "TSMOM is deprecated from the active strategy-candidate queue until materially new evidence exists."


def _family_evidence_summary(
    *,
    evidence_rows: list[dict[str, Any]],
    regime_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    metrics = {str(row["metric_name"]): row["value"] for row in evidence_rows}
    null_match_names = [
        name.removeprefix("null_").removesuffix("_beats_or_matches_tsmom_count")
        for name, value in metrics.items()
        if name.startswith("null_") and name.endswith("_beats_or_matches_tsmom_count") and _safe_float(value) >= 16.0
    ]
    regime_candidates = [
        f"{row['regime_axis']}={row['regime_label']}"
        for row in regime_rows
        if bool(row["qualifies_for_regime_only_watch"])
    ]
    return {
        "median_gross_return_pct": metrics.get("median_gross_return_pct"),
        "gross_edge_hit_rate": metrics.get("gross_edge_hit_rate"),
        "median_net_stress_return_pct": metrics.get("median_net_stress_return_pct"),
        "diagnostic_cash_null_failure_windows": metrics.get("diagnostic_cash_null_failure_windows"),
        "gross_edge_cash_null_failure_windows": metrics.get("gross_edge_cash_null_failure_windows"),
        "baseline_switch_rate": metrics.get("baseline_switch_rate"),
        "stable_switch_rate": metrics.get("stable_switch_rate"),
        "null_baselines_matching_or_beating_tsmom": null_match_names,
        "regime_exception_candidates": regime_candidates,
    }


def _family_decision_rationale_text(
    *,
    family_decision: str,
    decision_gates: dict[str, dict[str, Any]],
    evidence_rows: list[dict[str, Any]],
    regime_rows: list[dict[str, Any]],
) -> str:
    summary = _family_evidence_summary(evidence_rows=evidence_rows, regime_rows=regime_rows)
    failed_gates = [name for name, gate in decision_gates.items() if not bool(gate["passed"])]
    passed_regimes = summary["regime_exception_candidates"]
    return "\n".join(
        [
            f"# TSMOM candidate family decision: {family_decision}",
            "",
            "## Why broad KEEP_WATCH did not pass",
            f"- median_gross_return_pct: {summary['median_gross_return_pct']}",
            f"- gross_edge_hit_rate: {summary['gross_edge_hit_rate']}",
            f"- null baselines matching/beating TSMOM: {', '.join(summary['null_baselines_matching_or_beating_tsmom'])}",
            "",
            "## Regime exception review",
            f"- qualifying regimes: {', '.join(passed_regimes) if passed_regimes else 'none'}",
            "- Any regime exception remains paper-only and does not prove positive net return, readiness, or permission to trade.",
            "",
            "## Failed decision gates",
            f"- {', '.join(failed_gates) if failed_gates else 'none'}",
            "",
            "## Claim boundary",
            _family_decision_claim_boundary(),
            "",
        ]
    )


def _family_next_loop(family_decision: str) -> str:
    if family_decision == TSMOM_FAMILY_DECISION_REGIME_ONLY_WATCH:
        return (
            "Only if desired, pre-register a narrow regime-only falsification loop for the qualifying "
            "sideways/low-vol style regimes; otherwise freeze TSMOM as a negative-control benchmark."
        )
    if family_decision == TSMOM_FAMILY_DECISION_FREEZE_NEGATIVE_CONTROL:
        return "Use frozen TSMOM as a negative-control benchmark for validating future research pipelines."
    if family_decision == TSMOM_FAMILY_DECISION_DEPRECATE:
        return "Remove TSMOM from active candidate development until a materially different signal definition is proposed."
    return "Continue WATCH only with a stricter pre-registered validation loop and no holdout retuning."


def _safe_float(value: Any, default: float = 0.0) -> float:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return default
    return number if math.isfinite(number) else default


def _stable_selected_candidate(
    rankings: list[TsmomCandidateEvaluation],
    *,
    previous_stable: TsmomCandidateEvaluation | None,
    config: TsmomLoopConfig,
) -> TsmomCandidateEvaluation:
    top = rankings[0]
    if previous_stable is None:
        return top
    previous = next(
        (candidate for candidate in rankings if candidate.candidate_id == previous_stable.candidate_id),
        None,
    )
    if previous is None:
        return top
    if float(top.score - previous.score) <= config.stable_selection_score_margin:
        return previous
    return top


def _funding_drag_throttle_multiplier(
    index: pd.DatetimeIndex,
    funding_frame: pd.DataFrame | None,
    *,
    config: TsmomLoopConfig,
) -> pd.Series:
    funding_rate = align_public_funding_to_decision_bars(index, funding_frame)
    multiplier = pd.Series(1.0, index=index, dtype=float)
    multiplier = cast(
        pd.Series,
        multiplier.mask(
            funding_rate.abs() > config.funding_drag_throttle_abs_rate,
            config.funding_drag_throttle_multiplier,
        ),
    )
    return _float_series(multiplier, index=index).clip(lower=0.0, upper=1.0)


def _risk_switch_causes(
    previous: TsmomCandidateSettings | None,
    selected: TsmomCandidateSettings,
    *,
    score_gap: float | None,
    previous_market_regime: str | None,
    market_regime: str,
    switched: bool,
) -> list[str]:
    if previous is None or not switched:
        return []
    causes: list[str] = []
    if score_gap is not None and abs(score_gap) < 0.05:
        causes.append("near_tie_instability")
    if previous.lookback_bars != selected.lookback_bars:
        causes.append("lookback_sensitivity")
    if previous.vol_window_bars != selected.vol_window_bars or previous.target_annual_volatility != selected.target_annual_volatility:
        causes.append("volatility_target_sensitivity")
    if previous.max_exposure != selected.max_exposure:
        causes.append("exposure_cap_sensitivity")
    if previous.funding_abs_rate_limit != selected.funding_abs_rate_limit:
        causes.append("funding_threshold_sensitivity")
    if previous_market_regime is not None and previous_market_regime != market_regime:
        causes.append("regime_change")
    return causes or ["unknown"]


def _risk_hard_gates(
    *,
    risk_rows: list[dict[str, Any]],
    stable_ids: list[str],
    compute_profile: dict[str, Any],
    funding_validated: bool,
    config: TsmomLoopConfig,
) -> dict[str, dict[str, Any]]:
    median_stress = _median_float([float(row["risk_stress_net_return_pct"]) for row in risk_rows])
    switch_rate = _selected_candidate_switch_rate(stable_ids)
    worst_drawdown_abs = _worst_abs_records(risk_rows, "risk_max_drawdown_pct")
    cvar = _tail_loss_cvar([float(row["risk_stress_net_return_pct"]) for row in risk_rows], config)
    return {
        "candidate_count": _gate(
            compute_profile["candidate_count"],
            config.min_research_grade_candidate_count,
            compute_profile["candidate_count"] >= config.min_research_grade_candidate_count,
        ),
        "candidate_window_evaluations": _gate(
            compute_profile["candidate_window_evaluations"],
            config.min_research_grade_candidate_window_evaluations,
            compute_profile["candidate_window_evaluations"]
            >= config.min_research_grade_candidate_window_evaluations,
        ),
        "bars_processed_estimate": _gate(
            compute_profile["bars_processed_estimate"],
            config.min_research_grade_bars_processed_estimate,
            compute_profile["bars_processed_estimate"] >= config.min_research_grade_bars_processed_estimate,
        ),
        "median_stress_return_beats_cash_null": _gate(median_stress, "> 0.0", _gt(median_stress, 0.0)),
        "selected_candidate_switch_rate": _gate(
            switch_rate,
            f"<= {config.max_selected_candidate_switch_rate}",
            switch_rate <= config.max_selected_candidate_switch_rate,
        ),
        "funding_aware_net_return": _gate(
            median_stress,
            "> 0.0 with validated public funding",
            funding_validated and _gt(median_stress, 0.0),
            status="unclaimed" if not funding_validated else None,
        ),
        "max_drawdown_abs_pct": _gate(
            worst_drawdown_abs,
            f"<= {config.max_drawdown_abs_pct}",
            worst_drawdown_abs is not None and worst_drawdown_abs <= config.max_drawdown_abs_pct,
        ),
        "tail_loss_cvar_pct": _gate(
            cvar,
            f">= {config.min_tail_loss_cvar_pct}",
            cvar is not None and cvar >= config.min_tail_loss_cvar_pct,
        ),
    }


def _risk_loop_verdict(*, windows: tuple[TsmomWindow, ...], failed_gates: list[str]) -> str:
    if not windows:
        return TSMOM_RISK_BLOCKED
    return TSMOM_RISK_WATCH if failed_gates else TSMOM_RISK_PASS_PAPER_ONLY


def _risk_funding_throttle_summary(rows: list[dict[str, Any]], config: TsmomLoopConfig) -> dict[str, Any]:
    return {
        "window_count": len(rows),
        "median_baseline_net_return_pct": _median_float(
            [float(row["baseline_stress_net_return_pct"]) for row in rows]
        ),
        "median_risk_net_return_pct": _median_float([float(row["risk_stress_net_return_pct"]) for row in rows]),
        "median_delta_net_return_pct": _median_float([float(row["risk_delta_net_return_pct"]) for row in rows]),
        "median_risk_funding_drag_pct": _median_float([float(row["risk_funding_drag_pct"]) for row in rows]),
        "median_risk_slippage_drag_pct": _median_float([float(row["risk_slippage_drag_pct"]) for row in rows]),
        "risk_tail_cvar_pct": _tail_loss_cvar([float(row["risk_stress_net_return_pct"]) for row in rows], config),
        "risk_cash_null_failure_count": sum(1 for row in rows if not bool(row["risk_beats_cash_null"])),
        "median_throttle_bar_pct": _median_float([float(row["funding_throttle_bar_pct"]) for row in rows]),
        "throttle_active_window_count": sum(1 for row in rows if int(row["funding_throttle_bars"]) > 0),
        "max_throttle_bar_pct": max([float(row["funding_throttle_bar_pct"]) for row in rows], default=0.0),
    }


def _risk_regime_breakdown(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for row in rows:
        for key in ("market_regime", "volatility_regime", "funding_regime"):
            grouped.setdefault((key, str(row[key])), []).append(row)
    return [
        {
            "regime_axis": axis,
            "regime_label": label,
            "window_count": len(items),
            "median_baseline_net_return_pct": _median_float(
                [float(item["baseline_stress_net_return_pct"]) for item in items]
            ),
            "median_risk_net_return_pct": _median_float([float(item["risk_stress_net_return_pct"]) for item in items]),
            "median_delta_net_return_pct": _median_float([float(item["risk_delta_net_return_pct"]) for item in items]),
            "risk_cash_null_failure_rate": float(
                sum(1 for item in items if not bool(item["risk_beats_cash_null"])) / max(1, len(items))
            ),
        }
        for (axis, label), items in sorted(grouped.items())
    ]


def _gross_edge_bar_returns(frame: pd.DataFrame) -> pd.Series:
    close = _float_series(frame["Close"], index=frame.index)
    return _float_series(close.pct_change(), index=frame.index).fillna(0.0)


def _gross_edge_position_stats(
    position: pd.Series,
    returns: pd.Series,
    *,
    config: TsmomLoopConfig,
) -> dict[str, Any]:
    aligned_position = _float_series(position, index=returns.index).reindex(returns.index).fillna(0.0)
    shifted_position = _float_series(aligned_position.shift(1), index=returns.index).fillna(0.0)
    clean_returns = _float_series(returns, index=returns.index).reindex(returns.index).fillna(0.0)
    strategy_returns = cast(pd.Series, shifted_position * clean_returns)
    gross_return_factors = cast(pd.Series, pd.Series(1.0, index=returns.index, dtype=float) + strategy_returns)
    equity = _float_series(gross_return_factors.cumprod(), index=returns.index)
    if equity.empty:
        return {
            "return_pct": 0.0,
            "max_drawdown_pct": 0.0,
            "cvar_tail_loss_pct": None,
            "direction_correctness_rate": None,
            "average_exposure": 0.0,
            "turnover": 0.0,
        }
    drawdown = _float_series(equity / equity.cummax() - 1.0, index=equity.index)
    exposed = shifted_position.abs() > 0.0
    nonzero_returns = clean_returns != 0.0
    correctness_mask = cast(pd.Series, exposed & nonzero_returns)
    direction_correctness_rate = None
    if bool(correctness_mask.any()):
        correct = cast(pd.Series, (shifted_position[correctness_mask] * clean_returns[correctness_mask]) > 0.0)
        direction_correctness_rate = _finite_float(cast(Any, correct.mean()))
    return {
        "return_pct": float((float(equity.iloc[-1]) - 1.0) * 100.0),
        "max_drawdown_pct": _series_min_float(drawdown * 100.0),
        "cvar_tail_loss_pct": _tail_loss_cvar([float(value) * 100.0 for value in strategy_returns], config),
        "direction_correctness_rate": direction_correctness_rate,
        "average_exposure": _finite_float(cast(Any, aligned_position.abs().mean())),
        "turnover": _finite_float(cast(Any, aligned_position.diff().abs().fillna(aligned_position.abs()).sum())),
    }


def _gross_edge_random_sign_position(position: pd.Series, window_id: str) -> tuple[pd.Series, int]:
    seed = 1729 + sum(ord(char) for char in window_id)
    rng = random.Random(seed)
    absolute_position = _float_series(position, index=position.index).abs()
    signs = [1.0 if rng.random() >= 0.5 else -1.0 for _ in range(len(absolute_position))]
    return pd.Series(signs, index=absolute_position.index, dtype=float) * absolute_position, seed


def _gross_edge_null_baseline_rows(
    *,
    window: TsmomWindow,
    position: pd.Series,
    returns: pd.Series,
    tsmom_gross_return_pct: float,
    config: TsmomLoopConfig,
) -> list[dict[str, Any]]:
    random_position, seed = _gross_edge_random_sign_position(position, window.window_id)
    baselines: list[tuple[str, pd.Series, str, int | None]] = [
        (
            "cash_null",
            pd.Series(0.0, index=returns.index, dtype=float),
            "zero-return cash/null baseline",
            None,
        ),
        (
            "random_sign",
            random_position,
            "deterministic random long/flat sign baseline with source absolute exposure",
            seed,
        ),
        (
            "lagged_signal",
            _float_series(position.shift(24), index=position.index).fillna(0.0),
            "source TSMOM signal lagged by 24 bars",
            None,
        ),
        (
            "inverse_signal",
            -_float_series(position, index=position.index),
            "inverse of source TSMOM exposure",
            None,
        ),
        (
            "buy_and_hold",
            pd.Series(1.0, index=returns.index, dtype=float),
            "simple unit buy-and-hold exposure",
            None,
        ),
    ]
    rows: list[dict[str, Any]] = []
    for name, baseline_position, description, deterministic_seed in baselines:
        stats = _gross_edge_position_stats(baseline_position, returns, config=config)
        baseline_return = float(stats["return_pct"])
        rows.append(
            {
                "window_id": window.window_id,
                "holdout_start": _timestamp(window.holdout.index[0]),
                "holdout_end": _timestamp(window.holdout.index[-1]),
                "baseline_name": name,
                "baseline_description": description,
                "deterministic_seed": deterministic_seed,
                "tsmom_gross_return_pct": tsmom_gross_return_pct,
                "baseline_return_pct": baseline_return,
                "tsmom_minus_baseline_return_pct": tsmom_gross_return_pct - baseline_return,
                "baseline_beats_or_matches_tsmom": baseline_return >= tsmom_gross_return_pct,
                "baseline_drawdown_pct": stats["max_drawdown_pct"],
                "baseline_cvar_tail_loss_pct": stats["cvar_tail_loss_pct"],
                "baseline_average_exposure": stats["average_exposure"],
                "baseline_turnover": stats["turnover"],
            }
        )
    return rows


def _gross_edge_regime_labels(
    *,
    window: TsmomWindow,
    decisions: pd.DataFrame,
    source_row: dict[str, Any],
    funding_frame: pd.DataFrame | None,
    config: TsmomLoopConfig,
) -> dict[str, Any]:
    regime = _window_regime_diagnostics(window.holdout, decisions, funding_frame, config=config)
    validation_return = _close_to_close_return_pct(window.validation)
    holdout_return = _close_to_close_return_pct(window.holdout)
    if abs(validation_return) < 1.0 or abs(holdout_return) < 1.0:
        trend_regime = "trend_mixed"
    elif validation_return * holdout_return > 0.0:
        trend_regime = "trend_persistence"
    else:
        trend_regime = "trend_reversal"
    funding_drag = float(source_row["stress_funding_drag_pct"])
    funding_drag_regime = (
        "high-funding-drag"
        if funding_drag >= config.gross_edge_high_funding_drag_pct
        else "low-funding-drag"
    )
    return {
        **regime,
        "funding_drag_regime": funding_drag_regime,
        "trend_regime": trend_regime,
        "validation_close_to_close_return_pct": validation_return,
        "holdout_close_to_close_return_pct": holdout_return,
    }


def _gross_edge_failure_reason(row: dict[str, Any]) -> str:
    gross = float(row["gross_return_before_costs_pct"])
    buy_hold_delta = float(row["difference_vs_buy_and_hold_pct"])
    correctness = row.get("direction_correctness_rate")
    if gross <= 0.0 and correctness is not None and float(correctness) < 0.5:
        return "wrong_direction_or_low_direction_correctness"
    if gross <= 0.0 and str(row["market_regime"]) == "bear":
        return "negative_gross_edge_in_bear_regime"
    if gross <= 0.0 and str(row["trend_regime"]) == "trend_reversal":
        return "negative_gross_edge_in_trend_reversal_window"
    if buy_hold_delta < 0.0:
        return "underperformed_simple_buy_and_hold_before_costs"
    return "non_positive_gross_edge_before_costs"


def _gross_edge_summary(rows: list[dict[str, Any]], config: TsmomLoopConfig) -> dict[str, Any]:
    gross_returns = [float(row["gross_return_before_costs_pct"]) for row in rows]
    net_returns = [float(row["net_return_after_funding_slippage_stress_pct"]) for row in rows]
    hit_count = sum(1 for row in rows if bool(row["gross_edge_positive"]))
    return {
        "window_count": len(rows),
        "median_gross_return_pct": _median_float(gross_returns),
        "mean_gross_return_pct": _mean_float(gross_returns),
        "worst_gross_return_pct": min(gross_returns) if gross_returns else None,
        "gross_edge_hit_rate": float(hit_count / max(1, len(rows))),
        "median_net_return_pct": _median_float(net_returns),
        "median_funding_drag_pct": _median_float([float(row["funding_drag_pct"]) for row in rows]),
        "median_slippage_drag_pct": _median_float([float(row["slippage_drag_pct"]) for row in rows]),
        "median_stress_drag_pct": _median_float([float(row["stress_drag_pct"]) for row in rows]),
        "cash_null_failure_count": sum(1 for row in rows if bool(row["cash_null_failure"])),
        "median_difference_vs_buy_and_hold_pct": _median_float(
            [float(row["difference_vs_buy_and_hold_pct"]) for row in rows]
        ),
        "cvar_tail_gross_return_pct": _tail_loss_cvar(gross_returns, config),
        "worst_drawdown_pct": min([float(row["drawdown_pct"]) for row in rows], default=None),
    }


def _gross_edge_null_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    grouped: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        grouped.setdefault(str(row["baseline_name"]), []).append(row)
    return {
        "by_baseline": {
            name: {
                "window_count": len(items),
                "median_baseline_return_pct": _median_float(
                    [float(item["baseline_return_pct"]) for item in items]
                ),
                "median_tsmom_gross_return_pct": _median_float(
                    [float(item["tsmom_gross_return_pct"]) for item in items]
                ),
                "median_tsmom_minus_baseline_return_pct": _median_float(
                    [float(item["tsmom_minus_baseline_return_pct"]) for item in items]
                ),
                "baseline_beats_or_matches_tsmom_count": sum(
                    1 for item in items if bool(item["baseline_beats_or_matches_tsmom"])
                ),
            }
            for name, items in sorted(grouped.items())
        }
    }


def _gross_edge_evaluation_gates(
    *,
    gross_rows: list[dict[str, Any]],
    null_summary: dict[str, Any],
    source_payload: dict[str, Any],
    config: TsmomLoopConfig,
) -> dict[str, dict[str, Any]]:
    summary = _gross_edge_summary(gross_rows, config)
    by_baseline = cast(dict[str, dict[str, Any]], null_summary.get("by_baseline", {}))

    def median_delta(baseline_name: str) -> float | None:
        values = by_baseline.get(baseline_name)
        if values is None:
            return None
        value = values.get("median_tsmom_minus_baseline_return_pct")
        return float(value) if value is not None else None

    source_failed_gates = list(source_payload.get("failed_gates", []))
    return {
        "median_gross_return_beats_cash_null": _gate(
            summary["median_gross_return_pct"],
            "> 0.0",
            _gt(cast(float | None, summary["median_gross_return_pct"]), 0.0),
        ),
        "gross_edge_hit_rate": _gate(
            summary["gross_edge_hit_rate"],
            f"> {config.gross_edge_min_hit_rate}",
            float(summary["gross_edge_hit_rate"]) > config.gross_edge_min_hit_rate,
        ),
        "median_gross_beats_random_sign_baseline": _gate(
            median_delta("random_sign"),
            "> 0.0",
            _gt(median_delta("random_sign"), 0.0),
        ),
        "median_gross_beats_lagged_signal_baseline": _gate(
            median_delta("lagged_signal"),
            "> 0.0",
            _gt(median_delta("lagged_signal"), 0.0),
        ),
        "median_gross_beats_inverse_signal_baseline": _gate(
            median_delta("inverse_signal"),
            "> 0.0",
            _gt(median_delta("inverse_signal"), 0.0),
        ),
        "median_gross_beats_buy_and_hold_baseline": _gate(
            median_delta("buy_and_hold"),
            "> 0.0",
            _gt(median_delta("buy_and_hold"), 0.0),
        ),
        "source_failed_gates_preserved": _gate(
            source_failed_gates,
            "source failed gates must remain present and unrenamed",
            bool(source_failed_gates),
        ),
    }


def _gross_edge_loop_verdict(*, windows: tuple[TsmomWindow, ...], failed_gates: list[str]) -> str:
    if not windows:
        return TSMOM_GROSS_EDGE_BLOCKED
    return TSMOM_GROSS_EDGE_WATCH if failed_gates else TSMOM_GROSS_EDGE_PASS_PAPER_ONLY


def _gross_edge_regime_breakdown(
    rows: list[dict[str, Any]],
    *,
    config: TsmomLoopConfig,
) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for row in rows:
        for key in (
            "market_regime",
            "volatility_regime",
            "funding_regime",
            "funding_drag_regime",
            "trend_regime",
        ):
            grouped.setdefault((key, str(row[key])), []).append(row)
    breakdown: list[dict[str, Any]] = []
    for (axis, label), items in sorted(grouped.items()):
        gross_returns = [float(item["gross_return_before_costs_pct"]) for item in items]
        net_returns = [float(item["net_return_after_funding_slippage_stress_pct"]) for item in items]
        hit_rate = sum(1 for item in items if bool(item["gross_edge_positive"])) / max(1, len(items))
        median_gross = _median_float(gross_returns)
        passes = median_gross is not None and median_gross > 0.0 and hit_rate > config.gross_edge_min_hit_rate
        breakdown.append(
            {
                "regime_axis": axis,
                "regime_label": label,
                "window_count": len(items),
                "median_gross_return_pct": median_gross,
                "median_net_return_pct": _median_float(net_returns),
                "cash_null_failure_count": sum(1 for item in items if bool(item["cash_null_failure"])),
                "gross_edge_hit_rate": float(hit_rate),
                "worst_drawdown_pct": min([float(item["drawdown_pct"]) for item in items], default=None),
                "cvar_tail_loss_pct": _tail_loss_cvar(gross_returns, config),
                "regime_gate_status": "passed" if passes else "failed_or_unclaimed",
            }
        )
    return breakdown


def _gross_edge_regime_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    passed = [row for row in rows if row["regime_gate_status"] == "passed"]
    return {
        "regime_count": len(rows),
        "passed_regime_count": len(passed),
        "passed_regimes": [f"{row['regime_axis']}={row['regime_label']}" for row in passed],
    }


def _settings_from_window_row(row: dict[str, Any]) -> TsmomCandidateSettings:
    return TsmomCandidateSettings(
        lookback_bars=int(row["selected_lookback_bars"]),
        vol_window_bars=int(row["selected_vol_window_bars"]),
        target_annual_volatility=float(row["selected_target_annual_volatility"]),
        max_exposure=float(row["selected_max_exposure"]),
        funding_abs_rate_limit=float(row["selected_funding_abs_rate_limit"]),
        allow_short=False,
    )


def _window_regime_diagnostics(
    holdout: pd.DataFrame,
    decisions: pd.DataFrame,
    funding_frame: pd.DataFrame | None,
    *,
    config: TsmomLoopConfig,
) -> dict[str, Any]:
    close_return = _close_to_close_return_pct(holdout)
    market_regime = "bull" if close_return > 2.0 else "bear" if close_return < -2.0 else "sideways"
    close = _float_series(holdout["Close"], index=holdout.index)
    returns = _float_series(close.pct_change(), index=holdout.index).fillna(0.0)
    realized_vol_pct = _finite_float(cast(Any, returns.std())) * math.sqrt(24 * 365) * 100.0
    volatility_regime = "high-vol" if realized_vol_pct >= 80.0 else "low-vol" if realized_vol_pct <= 35.0 else "normal-vol"
    funding_rate = align_public_funding_to_decision_bars(_datetime_index(holdout), funding_frame)
    mean_funding = _finite_float(cast(Any, funding_rate.mean()))
    mean_abs_funding = _finite_float(cast(Any, funding_rate.abs().mean()))
    high_cost_threshold = max(float(config.stress_funding_rate_per_8h), 0.00025)
    if mean_abs_funding >= high_cost_threshold:
        funding_regime = "high-funding-cost"
    elif mean_funding > 0.0:
        funding_regime = "positive-funding"
    elif mean_funding < 0.0:
        funding_regime = "negative-funding"
    else:
        funding_regime = "flat-or-missing-funding"
    position = _float_series(decisions["position"], index=decisions.index)
    exposure_time_pct = _finite_float(cast(Any, (position.abs() > 0.0).mean())) * 100.0
    return {
        "market_regime": market_regime,
        "volatility_regime": volatility_regime,
        "funding_regime": funding_regime,
        "mean_funding_rate": mean_funding,
        "mean_abs_funding_rate": mean_abs_funding,
        "realized_volatility_pct": realized_vol_pct,
        "exposure_time_pct": exposure_time_pct,
    }


def _funding_failure_reason(row: dict[str, Any], regime: dict[str, Any]) -> str:
    gross = float(row["base_holdout_return_pct"])
    raw = float(row["stress_holdout_raw_return_pct"])
    funding_drag = float(row["stress_funding_drag_pct"])
    net = float(row["funding_aware_net_return_pct"])
    if net > 0.0:
        return "passed_cash_null"
    if gross <= 0.0 and raw <= 0.0:
        return f"low_or_wrong_direction_gross_edge_{regime['market_regime']}"
    if funding_drag > abs(raw) * 0.5:
        return "funding_cost_drag_dominates_raw_edge"
    if float(regime["exposure_time_pct"]) > 60.0 and funding_drag > 0.0:
        return "excessive_holding_under_funding_drag"
    if raw <= 0.0:
        return "stress_costs_erased_edge"
    return "low_gross_edge_after_funding_and_slippage"


def _score_gap(rankings: list[TsmomCandidateEvaluation]) -> float | None:
    if len(rankings) < 2:
        return None
    return float(rankings[0].score - rankings[1].score)


def _candidate_switch_causes(
    previous: TsmomCandidateSettings | None,
    selected: TsmomCandidateSettings,
    *,
    score_gap: float | None,
    funding_cost_drag_pct: float,
) -> list[str]:
    if previous is None:
        return []
    causes: list[str] = []
    if previous.lookback_bars != selected.lookback_bars:
        causes.append("lookback_sensitivity")
    if previous.vol_window_bars != selected.vol_window_bars or previous.target_annual_volatility != selected.target_annual_volatility:
        causes.append("volatility_target_change")
    if previous.max_exposure != selected.max_exposure:
        causes.append("exposure_cap_change")
    if previous.funding_abs_rate_limit != selected.funding_abs_rate_limit:
        causes.append("funding_threshold_change")
    if score_gap is not None and abs(score_gap) < 0.05:
        causes.append("unstable_near_tie_candidates")
    if funding_cost_drag_pct > 0.25:
        causes.append("funding_drag_pressure")
    return causes or ["unclassified_parameter_switch"]


def _mark_stress_windows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    if not rows:
        return rows
    returns = sorted(float(row["funding_aware_net_return_pct"]) for row in rows)
    cutoff_index = max(0, math.ceil(len(returns) * 0.2) - 1)
    stress_cutoff = returns[cutoff_index]
    updated: list[dict[str, Any]] = []
    for row in rows:
        copy = dict(row)
        copy["stress_window"] = bool(float(copy["funding_aware_net_return_pct"]) <= stress_cutoff)
        updated.append(copy)
    return updated


def _candidate_switch_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    switched = [row for row in rows if bool(row.get("switched"))]
    cause_counts: dict[str, int] = {}
    for row in switched:
        for cause in str(row.get("switch_causes", "")).split(";"):
            if cause and cause != "no_switch":
                cause_counts[cause] = cause_counts.get(cause, 0) + 1
    return {
        "switch_count": len(switched),
        "switch_rate": float(len(switched) / max(1, len(rows) - 1)) if len(rows) > 1 else 0.0,
        "top_switch_causes": [
            cause
            for cause, _ in sorted(cause_counts.items(), key=lambda item: (-item[1], item[0]))[:5]
        ],
        "cause_counts": cause_counts,
    }


def _stress_return_distribution(rows: list[dict[str, Any]], config: TsmomLoopConfig) -> dict[str, Any]:
    values = [float(row["funding_aware_net_return_pct"]) for row in rows]
    return {
        "count": len(values),
        "median_net_return_pct": _median_float(values),
        "mean_net_return_pct": _mean_float(values),
        "worst_net_return_pct": min(values) if values else None,
        "cvar_tail_net_return_pct": _tail_loss_cvar(values, config),
        "cash_null_return_pct": 0.0,
    }


def _funding_decomposition_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    failure_modes = sorted({str(row["diagnosed_primary_failure"]) for row in rows if bool(row["cash_null_failure"])})
    return {
        "median_gross_return_pct": _median_float([float(row["gross_return_pct"]) for row in rows]),
        "median_stress_raw_return_pct": _median_float([float(row["stress_raw_return_pct"]) for row in rows]),
        "median_funding_cost_drag_pct": _median_float([float(row["funding_cost_drag_pct"]) for row in rows]),
        "median_slippage_drag_pct": _median_float([float(row["slippage_drag_pct"]) for row in rows]),
        "median_net_return_pct": _median_float([float(row["funding_aware_net_return_pct"]) for row in rows]),
        "primary_failure_modes": failure_modes,
    }


def _regime_decomposition_summary(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    grouped: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        for key in ("market_regime", "volatility_regime", "funding_regime"):
            label = f"{key}:{row[key]}"
            grouped.setdefault(label, []).append(row)
    return {
        label: {
            "count": len(items),
            "median_net_return_pct": _median_float([float(item["funding_aware_net_return_pct"]) for item in items]),
            "cash_null_failure_rate": float(
                sum(1 for item in items if bool(item["cash_null_failure"])) / max(1, len(items))
            ),
        }
        for label, items in sorted(grouped.items())
    }


def _median_float(values: list[float]) -> float | None:
    if not values:
        return None
    value = _float_series(values).median()
    return float(value) if not pd.isna(value) else None


def _mean_float(values: list[float]) -> float | None:
    if not values:
        return None
    value = _float_series(values).mean()
    return float(value) if not pd.isna(value) else None


def _generate_windows(frame: pd.DataFrame, config: TsmomLoopConfig) -> tuple[TsmomWindow, ...]:
    total = len(frame)
    span = config.train_bars + config.validation_bars + config.holdout_bars
    windows: list[TsmomWindow] = []
    start = 0
    index = 1
    while start + span <= total:
        train_end = start + config.train_bars
        validation_end = train_end + config.validation_bars
        holdout_end = validation_end + config.holdout_bars
        train = frame.iloc[start:train_end].copy()
        validation = frame.iloc[train_end:validation_end].copy()
        holdout = frame.iloc[validation_end:holdout_end].copy()
        windows.append(TsmomWindow(f"W{index:03d}", start, train, validation, holdout))
        start += config.step_bars
        index += 1
    return tuple(windows)


def _select_candidate(
    train: pd.DataFrame,
    validation: pd.DataFrame,
    grid: tuple[TsmomCandidateSettings, ...],
    *,
    config: TsmomLoopConfig,
    funding_frame: pd.DataFrame | None,
) -> TsmomCandidateEvaluation:
    del config
    evaluations = _rank_candidate_evaluations(
        train,
        validation,
        grid,
        funding_frame=funding_frame,
    )
    return evaluations[0]


def _rank_candidate_evaluations(
    train: pd.DataFrame,
    validation: pd.DataFrame,
    grid: tuple[TsmomCandidateSettings, ...],
    *,
    funding_frame: pd.DataFrame | None,
) -> list[TsmomCandidateEvaluation]:
    evaluations: list[TsmomCandidateEvaluation] = []
    for index, settings in enumerate(grid, start=1):
        candidate_id = _candidate_id(index, settings)
        train_result = run_tsmom_backtest(
            train,
            label=f"{candidate_id}_train",
            settings=settings,
            funding_frame=funding_frame,
        )
        validation_result = run_tsmom_backtest(
            validation,
            label=f"{candidate_id}_validation",
            settings=settings,
            funding_frame=funding_frame,
        )
        reasons = []
        if int(train_result.metrics.get("closed_trades", 0) or 0) < 1:
            reasons.append("train_closed_trades_below_minimum")
        if int(validation_result.metrics.get("closed_trades", 0) or 0) < 1:
            reasons.append("validation_closed_trades_below_minimum")
        evaluations.append(
            TsmomCandidateEvaluation(
                candidate_id=candidate_id,
                settings=settings,
                train=train_result,
                validation=validation_result,
                score=risk_adjusted_score_v1(train_result.metrics, validation_result.metrics),
                eligible=not reasons,
                rejection_reasons=tuple(reasons),
            )
        )
    candidates = [candidate for candidate in evaluations if candidate.eligible] or evaluations
    return sorted(
        candidates,
        key=lambda candidate: (
            -candidate.score,
            -_metric_float(candidate.validation.metrics, "Return_pct"),
            abs(_metric_float(candidate.validation.metrics, "Max_Drawdown_pct")),
            int(candidate.validation.metrics.get("closed_trades", 0) or 0),
            candidate.candidate_id,
        ),
    )


def _hard_gates(
    *,
    window_records: list[dict[str, Any]],
    selected_ids: list[str],
    compute_profile: dict[str, Any],
    funding_validated: bool,
    config: TsmomLoopConfig,
) -> dict[str, dict[str, Any]]:
    median_stress = _median_records(window_records, "stress_holdout_return_pct")
    switch_rate = _selected_candidate_switch_rate(selected_ids)
    worst_drawdown_abs = _worst_abs_records(window_records, "stress_holdout_max_drawdown_pct")
    cvar = _tail_loss_cvar([float(row["stress_holdout_return_pct"]) for row in window_records], config)
    return {
        "candidate_count": _gate(
            compute_profile["candidate_count"],
            config.min_research_grade_candidate_count,
            compute_profile["candidate_count"] >= config.min_research_grade_candidate_count,
        ),
        "candidate_window_evaluations": _gate(
            compute_profile["candidate_window_evaluations"],
            config.min_research_grade_candidate_window_evaluations,
            compute_profile["candidate_window_evaluations"]
            >= config.min_research_grade_candidate_window_evaluations,
        ),
        "bars_processed_estimate": _gate(
            compute_profile["bars_processed_estimate"],
            config.min_research_grade_bars_processed_estimate,
            compute_profile["bars_processed_estimate"] >= config.min_research_grade_bars_processed_estimate,
        ),
        "median_stress_return_beats_cash_null": _gate(median_stress, "> 0.0", _gt(median_stress, 0.0)),
        "selected_candidate_switch_rate": _gate(
            switch_rate,
            f"<= {config.max_selected_candidate_switch_rate}",
            switch_rate <= config.max_selected_candidate_switch_rate,
        ),
        "funding_aware_net_return": _gate(
            median_stress,
            "> 0.0 with validated public funding",
            funding_validated and _gt(median_stress, 0.0),
            status="unclaimed" if not funding_validated else None,
        ),
        "max_drawdown_abs_pct": _gate(
            worst_drawdown_abs,
            f"<= {config.max_drawdown_abs_pct}",
            worst_drawdown_abs is not None and worst_drawdown_abs <= config.max_drawdown_abs_pct,
        ),
        "tail_loss_cvar_pct": _gate(
            cvar,
            f">= {config.min_tail_loss_cvar_pct}",
            cvar is not None and cvar >= config.min_tail_loss_cvar_pct,
        ),
    }


def _gate(
    observed: Any,
    threshold: Any,
    passed: bool,
    *,
    status: str | None = None,
) -> dict[str, Any]:
    return {
        "observed": observed,
        "threshold": threshold,
        "passed": bool(passed),
        "status": status or ("passed" if passed else "failed"),
    }


def _compute_profile(
    *,
    data: pd.DataFrame,
    windows: tuple[TsmomWindow, ...],
    candidate_count: int,
) -> dict[str, Any]:
    bars_processed_estimate = int(
        sum(
            candidate_count * (len(window.train) + len(window.validation)) + 2 * len(window.holdout)
            for window in windows
        )
    )
    return {
        "profiled_at": _utc_now_iso(),
        "engine": "vectorized_paper_only_tsmom_loop",
        "total_input_bars": int(len(data)),
        "input_start": _timestamp(data.index[0]) if not data.empty else None,
        "input_end": _timestamp(data.index[-1]) if not data.empty else None,
        "window_count": int(len(windows)),
        "candidate_count": int(candidate_count),
        "candidate_window_evaluations": int(candidate_count * len(windows)),
        "candidate_selection_evaluations": int(candidate_count * 2 * len(windows)),
        "selected_holdout_evaluations": int(2 * len(windows)),
        "bars_processed_estimate": bars_processed_estimate,
        "bars_processed_formula": (
            "sum(candidate_count * (train_bars + validation_bars) + 2 * holdout_bars "
            "for each generated window)"
        ),
    }


def _candidate_registry_rows(grid: tuple[TsmomCandidateSettings, ...]) -> list[dict[str, Any]]:
    return [
        {
            "candidate_id": _candidate_id(index, settings),
            **asdict(settings),
        }
        for index, settings in enumerate(grid, start=1)
    ]


def _candidate_id(index: int, settings: TsmomCandidateSettings) -> str:
    direction = "short-ok" if settings.allow_short else "long-only"
    return (
        f"t{index:03d}-lb{settings.lookback_bars}-vol{settings.vol_window_bars}-"
        f"tv{settings.target_annual_volatility:g}-mx{settings.max_exposure:g}-"
        f"fr{settings.funding_abs_rate_limit:g}-{direction}"
    )


def _data_scope(bundle: MarketDataBundle) -> dict[str, Any]:
    return {
        "source_name": "Binance Vision public USDⓈ-M Futures monthly klines",
        "symbol": bundle.symbol,
        "product_family": "crypto_perp_public_market_data",
        "timeframe": bundle.interval,
        "timezone": "UTC",
        "months": list(bundle.months),
        "first_timestamp": bundle.quality.get("first_timestamp"),
        "last_timestamp": bundle.quality.get("last_timestamp"),
        "row_count": int(len(bundle.frame)),
        "closed_bar_only": bool(bundle.quality.get("closed_bar_only", False)),
        "open_bar_included": bool(bundle.quality.get("open_bar_included", True)),
        "normalized_ohlcv_sha256": bundle.normalized_sha256,
        "source_urls": [record.url for record in bundle.downloads],
        "downloads": [asdict(record) for record in bundle.downloads],
    }


def _dataset_provenance(
    bundle: MarketDataBundle,
    funding_bundle: FundingRateBundle | None,
) -> list[dict[str, Any]]:
    datasets = [
        {
            "dataset": "ohlcv",
            "source_name": "Binance Vision public USDⓈ-M Futures monthly klines",
            "symbol": bundle.symbol,
            "timeframe": bundle.interval,
            "row_count": int(len(bundle.frame)),
            "normalized_sha256": bundle.normalized_sha256,
            "no_key_public_data": True,
            "account_or_signed_data_used": False,
            "live_or_testnet_data_used": False,
        }
    ]
    if funding_bundle is not None:
        datasets.append(
            {
                "dataset": "funding",
                "source_name": "Binance public USDⓈ-M fundingRate history",
                "symbol": funding_bundle.symbol,
                "timeframe": "8h_funding_events",
                "row_count": int(len(funding_bundle.frame)),
                "normalized_sha256": funding_bundle.normalized_sha256,
                "no_key_public_data": True,
                "account_or_signed_data_used": False,
                "live_or_testnet_data_used": False,
            }
        )
    return datasets


def _funding_coverage(
    funding_bundle: FundingRateBundle | None,
    funding_metadata: dict[str, Any] | None,
) -> dict[str, Any]:
    if funding_bundle is None:
        return {
            "status": "missing_public_funding_unclaimed",
            "rows": 0,
            "source": "none",
            "account_cashflows_validated": False,
            "metadata": funding_metadata or {},
        }
    return {
        "status": "validated_public_data" if not funding_bundle.frame.empty else "empty_public_funding",
        "rows": int(len(funding_bundle.frame)),
        "source": "public_binance_usdm_funding_history",
        "first_timestamp": funding_bundle.quality.get("first_timestamp"),
        "last_timestamp": funding_bundle.quality.get("last_timestamp"),
        "normalized_funding_sha256": funding_bundle.normalized_sha256,
        "account_cashflows_validated": False,
        "metadata": funding_metadata or {},
    }


def _validate_ohlcv_frame(frame: pd.DataFrame) -> pd.DataFrame:
    required = ["Open", "High", "Low", "Close", "Volume"]
    missing = [column for column in required if column not in frame.columns]
    if missing:
        raise ValueError(f"missing OHLCV columns: {missing}")
    if not isinstance(frame.index, pd.DatetimeIndex):
        raise ValueError("OHLCV frame must be indexed by timestamp")
    data = frame.loc[:, required].copy()
    data.index = _utc_index(pd.DatetimeIndex(data.index))
    if not data.index.is_monotonic_increasing:
        raise ValueError("OHLCV timestamps must be monotonic increasing")
    if data.index.has_duplicates:
        raise ValueError("OHLCV timestamps must be unique")
    for column in required:
        data[column] = pd.to_numeric(data[column], errors="coerce")
    if data.isna().any().any():
        raise ValueError("OHLCV frame contains non-numeric or missing values")
    return data


def _datetime_index(frame: pd.DataFrame) -> pd.DatetimeIndex:
    if not isinstance(frame.index, pd.DatetimeIndex):
        raise ValueError("frame must be indexed by timestamp")
    return frame.index


def _timestamp_at(index: pd.Index, position: int) -> pd.Timestamp:
    return cast(pd.Timestamp, pd.Timestamp(cast(Any, index[position])))


def _float_series(values: Any, *, index: pd.Index | None = None) -> pd.Series:
    if isinstance(values, pd.Series):
        series = values
    else:
        series = pd.Series(values, index=index)
    numeric = cast(pd.Series, pd.to_numeric(series, errors="coerce"))
    return cast(pd.Series, numeric.astype(float))


def _series_at_float(series: pd.Series, key: Any, default: float = 0.0) -> float:
    try:
        value = float(series.at[key])
    except (KeyError, TypeError, ValueError):
        return default
    return value if math.isfinite(value) else default


def _dataframe_at_float(frame: pd.DataFrame, key: Any, column: str, default: float = 0.0) -> float:
    try:
        value = float(frame.at[key, column])
    except (KeyError, TypeError, ValueError):
        return default
    return value if math.isfinite(value) else default


def _series_min_float(series: pd.Series, default: float = 0.0) -> float:
    return _finite_float(cast(Any, series.min()), default=default)


def _finite_float(value: Any, default: float = 0.0) -> float:
    try:
        converted = float(value)
    except (TypeError, ValueError):
        return default
    return converted if math.isfinite(converted) else default


def _normalize_funding_frame(frame: pd.DataFrame) -> pd.DataFrame:
    if "funding_rate" not in frame.columns:
        raise ValueError("funding frame must include funding_rate")
    data = frame.copy()
    if not isinstance(data.index, pd.DatetimeIndex):
        raise ValueError("funding frame must be indexed by timestamp")
    data.index = _utc_index(pd.DatetimeIndex(data.index))
    return data.sort_index()


def _close_trade(
    open_trade: dict[str, Any],
    exit_time: pd.Timestamp,
    exit_price: float,
    broker_settings: BrokerSettings,
) -> dict[str, Any]:
    entry_price = float(open_trade["entry_price"])
    direction = float(open_trade["direction"])
    ret = direction * (exit_price / entry_price - 1.0)
    return {
        "EntryTime": open_trade["entry_time"],
        "ExitTime": exit_time,
        "PnL": float(ret * float(broker_settings.cash) * abs(float(open_trade["position"]))),
        "ReturnPct": float(ret * 100.0),
    }


def _window_funding_rate_per_8h(
    funding_frame: pd.DataFrame | None,
    start: pd.Timestamp,
    end: pd.Timestamp,
    *,
    config: TsmomLoopConfig,
) -> float:
    if funding_frame is None or funding_frame.empty:
        return float(config.stress_funding_rate_per_8h)
    funding = _normalize_funding_frame(funding_frame)
    window = funding.loc[pd.Timestamp(start) : pd.Timestamp(end)]
    if window.empty:
        return float(config.stress_funding_rate_per_8h)
    observed = _float_series(window["funding_rate"], index=window.index).dropna().abs()
    if observed.empty:
        return float(config.stress_funding_rate_per_8h)
    quantile = float(observed.quantile(config.funding_rate_quantile))
    return max(float(config.stress_funding_rate_per_8h), quantile) if math.isfinite(quantile) else float(
        config.stress_funding_rate_per_8h
    )


def _funding_drag_pct(*, bars: int, exposure_time_pct: float, funding_rate_per_8h: float) -> float:
    return float(exposure_time_pct) / 100.0 * int(bars) / 8.0 * float(funding_rate_per_8h) * 100.0


def _slippage_drag_pct(*, closed_trades: int, slippage_proxy_bps: float) -> float:
    return int(closed_trades) * 2.0 * float(slippage_proxy_bps) / 100.0


def _annualized_sharpe(returns: pd.Series) -> float:
    clean = _float_series(returns).dropna()
    if len(clean) < 3:
        return 0.0
    std = _finite_float(cast(Any, clean.std()))
    if std <= 0.0 or not math.isfinite(std):
        return 0.0
    mean = _finite_float(cast(Any, clean.mean()))
    value = mean / std * math.sqrt(24 * 365)
    return value if math.isfinite(value) else 0.0


def _close_to_close_return_pct(frame: pd.DataFrame) -> float:
    first = float(frame["Close"].iloc[0])
    last = float(frame["Close"].iloc[-1])
    return (last / first - 1.0) * 100.0


def _metric_float(metrics: dict[str, Any], key: str, default: float = 0.0) -> float:
    try:
        value = float(metrics.get(key, default))
    except (TypeError, ValueError):
        return default
    return value if math.isfinite(value) else default


def _median_records(rows: list[dict[str, Any]], key: str) -> float | None:
    if not rows:
        return None
    value = _float_series([row.get(key) for row in rows]).median()
    return float(value) if not pd.isna(value) else None


def _worst_abs_records(rows: list[dict[str, Any]], key: str) -> float | None:
    if not rows:
        return None
    values = _float_series([row.get(key) for row in rows]).dropna()
    if values.empty:
        return None
    return float(values.abs().max())


def _tail_loss_cvar(values: list[float], config: TsmomLoopConfig) -> float | None:
    clean = sorted(value for value in values if math.isfinite(value))
    if not clean:
        return None
    count = max(1, math.ceil(len(clean) * config.tail_loss_cvar_quantile))
    return float(sum(clean[:count]) / count)


def _selected_candidate_switch_rate(selected_ids: list[str]) -> float:
    if len(selected_ids) <= 1:
        return 0.0
    switches = sum(1 for left, right in zip(selected_ids, selected_ids[1:], strict=False) if left != right)
    return float(switches / (len(selected_ids) - 1))


def _loop_verdict(*, windows: tuple[TsmomWindow, ...], failed_gates: list[str]) -> str:
    if not windows:
        return TSMOM_BLOCKED_VERDICT
    if failed_gates:
        return TSMOM_WATCH_VERDICT
    return TSMOM_PASS_PAPER_ONLY_VERDICT


def _gt(value: float | None, threshold: float) -> bool:
    return value is not None and value > threshold


def _utc_index(index: pd.DatetimeIndex) -> pd.DatetimeIndex:
    if index.tz is None:
        return index.tz_localize("UTC")
    return index.tz_convert("UTC")


def _timestamp(value: Any) -> str:
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return ts.isoformat().replace("+00:00", "Z")


def _utc_now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def _json_safe(value: Any) -> Any:
    if isinstance(value, dict):
        return {str(key): _json_safe(item) for key, item in value.items()}
    if isinstance(value, list | tuple):
        return [_json_safe(item) for item in value]
    if isinstance(value, pd.Timestamp):
        return _timestamp(value)
    try:
        if pd.isna(value) and not isinstance(value, str | bytes):
            return None
    except (TypeError, ValueError):
        pass
    return value


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    path.write_text(json.dumps(_json_safe(payload), indent=2, sort_keys=True, ensure_ascii=False) + "\n")


def _atomic_write_text(path: Path, text: str) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(text)
    tmp.replace(path)
