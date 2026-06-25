from __future__ import annotations

import json
import math
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
) -> BacktestResult:
    data = _validate_ohlcv_frame(frame)
    data_index = _datetime_index(data)
    decisions = build_tsmom_decision_frame(data, settings=settings, funding_frame=funding_frame)
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
