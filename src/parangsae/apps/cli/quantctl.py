from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import cast

import pandas as pd
import requests

from parangsae.core.capabilities import fixture_capability
from parangsae.core.config import load_product_policies, load_risk_config, load_strategy_policies
from parangsae.core.contracts import ProductFamily
from parangsae.core.events import EventLog, read_events, replay_positions
from parangsae.core.risk import LiveTradingGuard, RuntimeLiveContext
from parangsae.execution.binance import BinanceFuturesBroker, BinanceSpotBroker, RecordingHttpClient
from parangsae.execution.paper import run_paper_loop
from parangsae.marketdata.binance_vision import (
    CANONICAL_INTERVAL,
    CANONICAL_MONTHS,
    CANONICAL_SYMBOL,
    BinanceVisionError,
    DataValidationError,
    MarketDataBundle,
    canonical_cache_path,
    download_monthly_klines,
    load_market_data_bundle,
    normalize_months,
    validate_interval,
    validate_symbol,
)
from parangsae.marketdata.binance_funding import (
    BinanceFundingError,
    FundingDataValidationError,
    FundingRateBundle,
    funding_cache_path,
    fetch_funding_rate_history,
    funding_metadata_for_report,
)
from parangsae.marketdata.binance_live import (
    BinanceLiveKlineError,
    LiveKlineValidationError,
    fetch_recent_klines,
)
from parangsae.reporting.real_run_report import (
    build_forward_paper_manifest,
    build_manifest,
    prepare_output_dir,
    write_forward_paper_success_report,
    write_failure_report,
    write_readiness_success_report,
    write_risk_success_report,
    write_success_report,
)
from parangsae.research.backtest_engine import (
    MeaningGateError,
    assert_meaningful_run,
    broker_settings_dict,
    run_risk_candidate_search,
    run_strategy_backtest,
    split_chronological,
    strategy_settings_dict,
)
from parangsae.research.alpha_signal_factory import (
    DEFAULT_ALPHA_SIGNAL_DAG_V2_ARTIFACT,
    DEFAULT_ALPHA_SIGNAL_OOS_FDR_V3_ARTIFACT,
    DEFAULT_MAX_DEPTH,
    DEFAULT_SIGNAL_DAG_CANDIDATE_BUDGET,
    DEFAULT_SIGNAL_DAG_GENERATION_SEED,
    DEFAULT_SIGNAL_DAG_MAX_DEPTH,
    DEFAULT_SIGNAL_GENERATION_SEED,
    default_alpha_signal_oos_fdr_validation_output_dir,
    default_alpha_signal_strict_falsification_output_dir,
    evaluate_alpha_signal_dag_expansion,
    evaluate_alpha_signal_factory,
    evaluate_alpha_signal_oos_fdr_validation,
    evaluate_alpha_signal_strict_falsification,
    default_alpha_signal_dag_expansion_output_dir,
    default_alpha_signal_factory_output_dir,
    write_alpha_signal_dag_expansion_artifacts,
    write_alpha_signal_factory_artifacts,
    write_alpha_signal_oos_fdr_validation_artifacts,
    write_alpha_signal_strict_falsification_artifacts,
)
from parangsae.research.data_depth import (
    build_data_depth_loop_payload,
    default_data_depth_months,
    default_data_depth_output_dir,
    write_data_depth_loop_artifacts,
)
from parangsae.research.forward_paper import ForwardPaperConfig, run_forward_paper
from parangsae.research.multi_asset_alpha_factory import (
    DEFAULT_MIN_BARS_PER_SYMBOL,
    DEFAULT_MULTI_ASSET_ALPHA_V4_ARTIFACT,
    DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET,
    DEFAULT_MULTI_ASSET_GENERATION_SEED,
    DEFAULT_MULTI_ASSET_MAX_DEPTH,
    DEFAULT_PANEL_READY_MULTI_ASSET_ALPHA_V7_ARTIFACT,
    DEFAULT_SYMBOL_UNIVERSE,
    default_panel_ready_multi_asset_alpha_prefilter_output_dir,
    default_multi_asset_alpha_prefilter_output_dir,
    evaluate_multi_asset_alpha_prefilter,
    evaluate_panel_ready_multi_asset_alpha_prefilter_rerun,
    parse_symbol_universe,
    read_panel_ready_multi_asset_v7_source_state,
    write_panel_ready_multi_asset_alpha_prefilter_artifacts,
    write_multi_asset_alpha_prefilter_artifacts,
)
from parangsae.research.public_multi_asset_panel_coverage import (
    DEFAULT_PUBLIC_MULTI_ASSET_DATA_BACKFILL_V6_ARTIFACT,
    DEFAULT_PUBLIC_MULTI_ASSET_PANEL_MINIMUM_REQUIRED_SYMBOLS,
    DEFAULT_PUBLIC_MULTI_ASSET_PANEL_V5_ARTIFACT,
    PUBLIC_MULTI_ASSET_PANEL_DEFAULT_UNIVERSE,
    default_public_multi_asset_data_backfill_output_dir,
    default_public_multi_asset_panel_coverage_output_dir,
    evaluate_public_multi_asset_data_backfill,
    evaluate_public_multi_asset_panel_coverage,
    parse_public_symbol_universe,
    write_public_multi_asset_data_backfill_artifacts,
    write_public_multi_asset_panel_coverage_artifacts,
)
from parangsae.research.null_baselines import (
    DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT,
    build_research_null_baseline_harness_payload,
    default_research_null_baseline_harness_output_dir,
    write_research_null_baseline_harness_artifacts,
)
from parangsae.research.regime import build_regime_report
from parangsae.research.readiness import build_readiness_report
from parangsae.research.signals import build_signal_report
from parangsae.research.tsmom import (
    DEFAULT_TSMOM_DIAGNOSTIC_ARTIFACT,
    DEFAULT_TSMOM_FAMILY_DECISION_ARTIFACT,
    DEFAULT_TSMOM_GROSS_EDGE_ARTIFACT,
    DEFAULT_TSMOM_RISK_ARTIFACT,
    DEFAULT_TSMOM_SOURCE_LOOP_ARTIFACT,
    build_regime_only_tsmom_falsification_payload,
    build_tsmom_candidate_family_decision_payload,
    build_tsmom_diagnostic_payload,
    build_tsmom_gross_edge_loop_payload,
    build_tsmom_loop_payload,
    build_tsmom_risk_loop_payload,
    default_regime_only_tsmom_falsification_output_dir,
    default_tsmom_diagnostic_output_dir,
    default_tsmom_family_decision_output_dir,
    default_tsmom_gross_edge_output_dir,
    default_tsmom_output_dir,
    default_tsmom_risk_output_dir,
    write_regime_only_tsmom_falsification_artifacts,
    write_tsmom_candidate_family_decision_artifacts,
    write_tsmom_diagnostic_artifacts,
    write_tsmom_gross_edge_loop_artifacts,
    write_tsmom_loop_artifacts,
    write_tsmom_risk_loop_artifacts,
)
from parangsae.execution.portfolio import intents_from_target, target_from_signals
from parangsae.research.fixtures import FIXTURE_PRICES
from parangsae.research.strategies import crypto_residual_ridge_placeholder, trend_risk_signals

def _find_repo_root(start: Path) -> Path:
    for candidate in (start, *start.parents):
        if (candidate / "pyproject.toml").exists() and (candidate / "configs").is_dir():
            return candidate
    raise RuntimeError(f"could not locate repository root from {start}")


ROOT = _find_repo_root(Path(__file__).resolve())
RISK_CONFIG = ROOT / "configs" / "risk.balanced.yaml"
PRODUCTS_CONFIG = ROOT / "configs" / "products.yaml"
STRATEGIES_CONFIG = ROOT / "configs" / "strategies.yaml"


def _signals(strategy: str, family: ProductFamily):
    policies = load_strategy_policies(STRATEGIES_CONFIG)
    if strategy not in policies:
        raise ValueError(f"unknown strategy: {strategy}")
    if family not in policies[strategy].enabled_for:
        raise ValueError(f"strategy {strategy} is not enabled for {family.value}")
    if strategy == "crypto-residual-ridge":
        return crypto_residual_ridge_placeholder(FIXTURE_PRICES)
    return trend_risk_signals(FIXTURE_PRICES, family)


def cmd_backtest(args: argparse.Namespace) -> int:
    family = ProductFamily(args.product_family)
    signals = _signals(args.strategy, family)
    target = target_from_signals(signals)
    first = {s: p[0] for s, p in FIXTURE_PRICES.items()}
    last = {s: p[-1] for s, p in FIXTURE_PRICES.items()}
    pnl = sum((last[s] - first[s]) * w for s, w in target.symbol_weights.items())
    report = {
        "strategy": args.strategy,
        "product_family": family.value,
        "target": target.symbol_weights,
        "fixture_pnl_units": round(pnl, 6),
        "max_drawdown_note": "fixture-only; not a profit guarantee",
        "no_profit_guarantee": True,
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


def cmd_paper_run(args: argparse.Namespace) -> int:
    risk = load_risk_config(RISK_CONFIG)
    log = EventLog(args.event_log)
    signals = _signals("trend-risk", ProductFamily.SPOT_CRYPTO)
    result = run_paper_loop(signals[: args.ticks], risk, log)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


def cmd_replay(args: argparse.Namespace) -> int:
    events = read_events(args.event_log)
    print(json.dumps({"positions": replay_positions(events), "events": len(events)}, indent=2, sort_keys=True))
    return 0


def cmd_live_dry_run(args: argparse.Namespace) -> int:
    risk = load_risk_config(RISK_CONFIG)
    family = ProductFamily(args.product_family)
    strategy = "crypto-residual-ridge" if family is ProductFamily.CRYPTO_PERP else "trend-risk"
    signals = _signals(strategy, family)
    target = target_from_signals(signals)
    orders = intents_from_target(target)
    guard = LiveTradingGuard(risk, load_product_policies(PRODUCTS_CONFIG))
    calls: list[tuple[str, str, dict]] = []
    spot = BinanceSpotBroker(RecordingHttpClient(calls))
    futures = BinanceFuturesBroker(RecordingHttpClient(calls))
    results = []
    for order in orders[:1]:
        decision = guard.evaluate(
            order,
            fixture_capability(order.symbol, order.product_family),
            RuntimeLiveContext(mode="dry_run", live_enabled=False),
        )
        if family is ProductFamily.SPOT_CRYPTO:
            spot.validate_dry_run(order)
        else:
            futures.validate_dry_run(order)
            for protective in order.protective_orders:
                futures.validate_protective_dry_run(order, protective)
        results.append({"symbol": order.symbol, "risk": decision.reason})
    print(json.dumps({"results": results, "http_calls": calls}, indent=2, sort_keys=True))
    return 0



def _parse_months(raw: str) -> tuple[str, ...]:
    return normalize_months(raw.split(","))



def _validate_canonical_real_run(symbol: str, interval: str, months: tuple[str, ...]) -> None:
    if symbol != CANONICAL_SYMBOL or interval != CANONICAL_INTERVAL or months != CANONICAL_MONTHS:
        raise ValueError(
            "real-run is locked to canonical BTCUSDT 1h 2024-01..2024-06; "
            "create a separate research command for alternate symbols/windows"
        )


def _failure_payload(report_dir: Path | None, failed_gate: str, reason: str) -> str:
    payload = {"status": "failed", "failed_gate": failed_gate, "reason": reason}
    if report_dir is not None:
        payload["report_dir"] = str(report_dir)
    return json.dumps(payload, indent=2, sort_keys=True)


def _datetime_index_endpoint(index: object, position: int) -> pd.Timestamp:
    datetime_index = pd.DatetimeIndex(index)
    return cast(pd.Timestamp, datetime_index[position])


def _default_data_depth_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl data-depth-run",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--output-dir {args.output_dir or default_data_depth_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_tsmom_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl tsmom-run",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--output-dir {args.output_dir or default_tsmom_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_tsmom_diagnostic_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl tsmom-diagnostic-run",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--output-dir {args.output_dir or default_tsmom_diagnostic_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_tsmom_risk_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl tsmom-risk-run",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--output-dir {args.output_dir or default_tsmom_risk_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_tsmom_gross_edge_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl tsmom-gross-edge-run",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--output-dir {args.output_dir or default_tsmom_gross_edge_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_tsmom_family_decision_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl tsmom-family-decision-run",
        f"--source-loop-artifact {args.source_loop_artifact}",
        f"--diagnostic-artifact {args.diagnostic_artifact}",
        f"--risk-artifact {args.risk_artifact}",
        f"--gross-edge-artifact {args.gross_edge_artifact}",
        f"--output-dir {args.output_dir or default_tsmom_family_decision_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_regime_only_tsmom_falsification_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl tsmom-regime-only-falsification-run",
        f"--source-loop-artifact {args.source_loop_artifact}",
        f"--diagnostic-artifact {args.diagnostic_artifact}",
        f"--risk-artifact {args.risk_artifact}",
        f"--gross-edge-artifact {args.gross_edge_artifact}",
        f"--family-decision-artifact {args.family_decision_artifact}",
        f"--output-dir {args.output_dir or default_regime_only_tsmom_falsification_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_research_null_baseline_harness_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl research-null-baseline-harness-run",
        f"--source-loop-artifact {args.source_loop_artifact}",
        f"--diagnostic-artifact {args.diagnostic_artifact}",
        f"--risk-artifact {args.risk_artifact}",
        f"--gross-edge-artifact {args.gross_edge_artifact}",
        f"--family-decision-artifact {args.family_decision_artifact}",
        f"--regime-only-artifact {args.regime_only_artifact}",
        f"--output-dir {args.output_dir or default_research_null_baseline_harness_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_alpha_signal_factory_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl alpha-signal-factory-composition-run",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--seed {args.seed}",
        f"--max-depth {args.max_depth}",
        f"--output-dir {args.output_dir or default_alpha_signal_factory_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_alpha_signal_dag_expansion_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl alpha-signal-dag-expansion-run",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--seed {args.seed}",
        f"--max-depth {args.max_depth}",
        f"--candidate-budget {args.candidate_budget}",
        f"--output-dir {args.output_dir or default_alpha_signal_dag_expansion_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_alpha_signal_oos_fdr_validation_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl alpha-signal-oos-fdr-validation-run",
        f"--source-v2-artifact {args.source_v2_artifact}",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--output-dir {args.output_dir or default_alpha_signal_oos_fdr_validation_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_alpha_signal_strict_falsification_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl alpha-signal-strict-falsification-run",
        f"--source-v3-artifact {args.source_v3_artifact}",
        f"--symbol {args.symbol}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--output-dir {args.output_dir or default_alpha_signal_strict_falsification_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_multi_asset_alpha_prefilter_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl multi-asset-alpha-generator-strict-prefilter-run",
        f"--source-v4-artifact {args.source_v4_artifact}",
        f"--symbols {args.symbols}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--cache-mode {args.cache_mode}",
        f"--seed {args.seed}",
        f"--candidate-budget {args.candidate_budget}",
        f"--max-depth {args.max_depth}",
        f"--min-bars-per-symbol {args.min_bars_per_symbol}",
        f"--output-dir {args.output_dir or default_multi_asset_alpha_prefilter_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_panel_ready_multi_asset_alpha_prefilter_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl panel-ready-multi-asset-alpha-prefilter-rerun",
        f"--source-v7-artifact {args.source_v7_artifact}",
        f"--source-v4-artifact {args.source_v4_artifact}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--seed {args.seed}",
        f"--candidate-budget {args.candidate_budget}",
        f"--max-depth {args.max_depth}",
        f"--min-bars-per-symbol {args.min_bars_per_symbol}",
        f"--output-dir {args.output_dir or default_panel_ready_multi_asset_alpha_prefilter_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.interval is not None:
        command.append(f"--interval {args.interval}")
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_public_multi_asset_panel_coverage_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl public-multi-asset-panel-coverage-run",
        f"--source-v5-artifact {args.source_v5_artifact}",
        f"--symbols {args.symbols}",
        f"--interval {args.interval}",
        f"--months {args.months}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--cache-mode {args.cache_mode}",
        f"--minimum-required-symbols {args.minimum_required_symbols}",
        f"--output-dir {args.output_dir or default_public_multi_asset_panel_coverage_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _default_public_multi_asset_data_backfill_command(args: argparse.Namespace) -> str:
    command = [
        "uv run quantctl public-multi-asset-data-backfill-run",
        f"--source-v6-artifact {args.source_v6_artifact}",
        f"--cache-dir {args.cache_dir}",
        f"--funding-source {args.funding_source}",
        f"--funding-cache-dir {args.funding_cache_dir}",
        f"--minimum-required-symbols {args.minimum_required_symbols}",
        f"--output-dir {args.output_dir or default_public_multi_asset_data_backfill_output_dir(root=Path(args.reports_root) / 'loops')}",
    ]
    if args.symbols is not None:
        command.append(f"--symbols {args.symbols}")
    if args.interval is not None:
        command.append(f"--interval {args.interval}")
    if args.start_month is not None:
        command.append(f"--start-month {args.start_month}")
    if args.end_month is not None:
        command.append(f"--end-month {args.end_month}")
    if args.plan_only:
        command.append("--plan-only")
    if args.force:
        command.append("--force")
    if args.overwrite:
        command.append("--overwrite")
    return " ".join(command)


def _cached_ohlcv_available(
    *,
    symbol: str,
    interval: str,
    months: tuple[str, ...],
    cache_dir: str | Path,
) -> tuple[bool, str | None]:
    missing = [
        month
        for month in months
        if not canonical_cache_path(Path(cache_dir), symbol, interval, month).exists()
    ]
    if missing:
        return False, f"missing cached OHLCV month(s): {','.join(missing[:6])}"
    return True, None


def _cached_funding_available(
    *,
    symbol: str,
    start_time: pd.Timestamp,
    end_time: pd.Timestamp,
    cache_dir: str | Path,
) -> bool:
    start_ms = int(start_time.timestamp() * 1000)
    end_ms = int(end_time.timestamp() * 1000)
    return funding_cache_path(cache_dir, symbol, start_ms, end_ms).exists()


def _load_multi_asset_public_bundles(
    args: argparse.Namespace,
    *,
    symbols: tuple[str, ...],
    interval: str,
    months: tuple[str, ...],
) -> tuple[dict[str, MarketDataBundle], dict[str, FundingRateBundle | None], list[dict[str, object]], dict[str, str]]:
    market_bundles: dict[str, MarketDataBundle] = {}
    funding_bundles: dict[str, FundingRateBundle | None] = {}
    excluded: list[dict[str, object]] = []
    funding_status: dict[str, str] = {}
    for symbol in symbols:
        try:
            if args.cache_mode == "cache-only":
                available, reason = _cached_ohlcv_available(
                    symbol=symbol,
                    interval=interval,
                    months=months,
                    cache_dir=args.cache_dir,
                )
                if not available:
                    excluded.append({"symbol": symbol, "stage": "ohlcv_cache_availability", "reason": reason})
                    continue
            bundle = load_market_data_bundle(
                symbol=symbol,
                interval=interval,
                months=months,
                cache_dir=args.cache_dir,
            )
            if len(bundle.frame) < int(args.min_bars_per_symbol):
                excluded.append(
                    {
                        "symbol": symbol,
                        "stage": "ohlcv_min_bars",
                        "reason": f"{len(bundle.frame)} bars below min_bars_per_symbol={args.min_bars_per_symbol}",
                    }
                )
                continue
            market_bundles[symbol] = bundle
            if args.funding_source == "none":
                funding_bundles[symbol] = None
                funding_status[symbol] = "disabled"
                continue
            start_ts = _datetime_index_endpoint(bundle.frame.index, 0)
            end_ts = _datetime_index_endpoint(bundle.frame.index, -1)
            if args.cache_mode == "cache-only" and not _cached_funding_available(
                symbol=symbol,
                start_time=start_ts,
                end_time=end_ts,
                cache_dir=args.funding_cache_dir,
            ):
                funding_bundles[symbol] = None
                funding_status[symbol] = "unavailable_missing_cache"
                continue
            try:
                funding_bundle = fetch_funding_rate_history(
                    symbol=symbol,
                    start_time=start_ts,
                    end_time=end_ts,
                    cache_dir=args.funding_cache_dir,
                )
                funding_bundles[symbol] = funding_bundle
                funding_status[symbol] = "available"
            except (BinanceFundingError, FundingDataValidationError, requests.RequestException, ValueError) as exc:
                funding_bundles[symbol] = None
                funding_status[symbol] = f"invalid_unavailable:{type(exc).__name__}"
        except (BinanceVisionError, DataValidationError, requests.RequestException, ValueError) as exc:
            excluded.append({"symbol": symbol, "stage": "ohlcv_load_or_validation", "reason": str(exc)})
    return market_bundles, funding_bundles, excluded, funding_status


def cmd_data_depth_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = Path(args.output_dir) if args.output_dir else default_data_depth_output_dir(
        root=Path(args.reports_root) / "loops"
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        funding_metadata = {
            "source": "none",
            "rows": 0,
            "limitation": (
                "funding disabled by explicit operator flag; data-depth evidence records "
                "OHLCV provenance only"
            ),
        }
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
            funding_metadata = funding_metadata_for_report(funding_bundle)
        commands = args.command or [_default_data_depth_command(args)]
        changed_files = args.changed_file or []
        payload = build_data_depth_loop_payload(
            bundle=bundle,
            funding_bundle=funding_bundle,
            funding_metadata=funding_metadata,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_data_depth_loop_artifacts(
            output_dir,
            payload,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "data-depth-run",
                    "output_dir": str(written),
                    "data_depth_loop_verdict": payload["data_depth_loop_verdict"],
                    "data_depth_loop_verdict_namespace": payload["data_depth_loop_verdict_namespace"],
                    "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
                    "data_only_research_grade_depth_passed": payload["validation_depth"][
                        "data_only_research_grade_depth_passed"
                    ],
                    "failed_gates": payload["failed_gates"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "data-depth-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_tsmom_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = Path(args.output_dir) if args.output_dir else default_tsmom_output_dir(
        root=Path(args.reports_root) / "loops"
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        funding_metadata = {
            "source": "none",
            "rows": 0,
            "limitation": "funding disabled by explicit operator flag; funding-aware gate is unclaimed",
        }
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
            funding_metadata = funding_metadata_for_report(funding_bundle)
        commands = args.command or [_default_tsmom_command(args)]
        changed_files = args.changed_file or []
        payload = build_tsmom_loop_payload(
            bundle=bundle,
            funding_bundle=funding_bundle,
            funding_metadata=funding_metadata,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_tsmom_loop_artifacts(output_dir, payload, overwrite=args.overwrite)
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "tsmom-run",
                    "output_dir": str(written),
                    "tsmom_loop_verdict": payload["tsmom_loop_verdict"],
                    "tsmom_loop_verdict_namespace": payload["tsmom_loop_verdict_namespace"],
                    "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
                    "failed_gates": payload["failed_gates"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "tsmom-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_tsmom_diagnostic_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = Path(args.output_dir) if args.output_dir else default_tsmom_diagnostic_output_dir(
        root=Path(args.reports_root) / "loops"
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        funding_metadata = {
            "source": "none",
            "rows": 0,
            "limitation": "funding disabled by explicit operator flag; funding-aware diagnostic remains public-data-only",
        }
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
            funding_metadata = funding_metadata_for_report(funding_bundle)
        commands = args.command or [_default_tsmom_diagnostic_command(args)]
        changed_files = args.changed_file or []
        payload = build_tsmom_diagnostic_payload(
            bundle=bundle,
            funding_bundle=funding_bundle,
            funding_metadata=funding_metadata,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_tsmom_diagnostic_artifacts(output_dir, payload, overwrite=args.overwrite)
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "tsmom-diagnostic-run",
                    "output_dir": str(written),
                    "tsmom_diagnostic_loop_verdict": payload["tsmom_diagnostic_loop_verdict"],
                    "tsmom_diagnostic_loop_verdict_namespace": payload[
                        "tsmom_diagnostic_loop_verdict_namespace"
                    ],
                    "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
                    "analyzed_failed_gates": payload["analyzed_failed_gates"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "tsmom-diagnostic-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_tsmom_risk_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = Path(args.output_dir) if args.output_dir else default_tsmom_risk_output_dir(
        root=Path(args.reports_root) / "loops"
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        funding_metadata = {
            "source": "none",
            "rows": 0,
            "limitation": "funding disabled by explicit operator flag; funding-aware risk gates remain unclaimed",
        }
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
            funding_metadata = funding_metadata_for_report(funding_bundle)
        commands = args.command or [_default_tsmom_risk_command(args)]
        changed_files = args.changed_file or []
        payload = build_tsmom_risk_loop_payload(
            bundle=bundle,
            funding_bundle=funding_bundle,
            funding_metadata=funding_metadata,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_tsmom_risk_loop_artifacts(output_dir, payload, overwrite=args.overwrite)
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "tsmom-risk-run",
                    "output_dir": str(written),
                    "tsmom_risk_loop_verdict": payload["tsmom_risk_loop_verdict"],
                    "tsmom_risk_loop_verdict_namespace": payload["tsmom_risk_loop_verdict_namespace"],
                    "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
                    "source_tsmom_loop_verdict": payload["source_tsmom_loop_verdict"],
                    "failed_risk_gates": payload["failed_risk_gates"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "tsmom-risk-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_tsmom_gross_edge_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = Path(args.output_dir) if args.output_dir else default_tsmom_gross_edge_output_dir(
        root=Path(args.reports_root) / "loops"
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        funding_metadata = {
            "source": "none",
            "rows": 0,
            "limitation": "funding disabled by explicit operator flag; funding-aware gross-edge diagnostics remain public-data-only",
        }
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
            funding_metadata = funding_metadata_for_report(funding_bundle)
        commands = args.command or [_default_tsmom_gross_edge_command(args)]
        changed_files = args.changed_file or []
        payload = build_tsmom_gross_edge_loop_payload(
            bundle=bundle,
            funding_bundle=funding_bundle,
            funding_metadata=funding_metadata,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_tsmom_gross_edge_loop_artifacts(output_dir, payload, overwrite=args.overwrite)
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "tsmom-gross-edge-run",
                    "output_dir": str(written),
                    "tsmom_gross_edge_loop_verdict": payload["tsmom_gross_edge_loop_verdict"],
                    "tsmom_gross_edge_loop_verdict_namespace": payload[
                        "tsmom_gross_edge_loop_verdict_namespace"
                    ],
                    "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
                    "source_tsmom_loop_verdict": payload["source_tsmom_loop_verdict"],
                    "failed_gross_edge_gates": payload["failed_gross_edge_gates"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "tsmom-gross-edge-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_tsmom_family_decision_run(args: argparse.Namespace) -> int:
    output_dir = Path(args.output_dir) if args.output_dir else default_tsmom_family_decision_output_dir(
        root=Path(args.reports_root) / "loops"
    )
    try:
        commands = args.command or [_default_tsmom_family_decision_command(args)]
        changed_files = args.changed_file or []
        payload = build_tsmom_candidate_family_decision_payload(
            source_loop_artifact=args.source_loop_artifact,
            diagnostic_artifact=args.diagnostic_artifact,
            risk_artifact=args.risk_artifact,
            gross_edge_artifact=args.gross_edge_artifact,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_tsmom_candidate_family_decision_artifacts(
            output_dir,
            payload,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "tsmom-family-decision-run",
                    "output_dir": str(written),
                    "tsmom_candidate_family_decision": payload["tsmom_candidate_family_decision"],
                    "tsmom_candidate_family_decision_namespace": payload[
                        "tsmom_candidate_family_decision_namespace"
                    ],
                    "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
                    "source_tsmom_loop_verdict": payload["source_tsmom_loop_verdict"],
                    "tsmom_risk_loop_verdict": payload["tsmom_risk_loop_verdict"],
                    "tsmom_gross_edge_loop_verdict": payload["tsmom_gross_edge_loop_verdict"],
                    "candidate_queue_status": payload["candidate_queue_status"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (FileNotFoundError, ValueError, FileExistsError) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "tsmom-family-decision-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_tsmom_regime_only_falsification_run(args: argparse.Namespace) -> int:
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_regime_only_tsmom_falsification_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        commands = args.command or [_default_regime_only_tsmom_falsification_command(args)]
        changed_files = args.changed_file or []
        payload = build_regime_only_tsmom_falsification_payload(
            source_loop_artifact=args.source_loop_artifact,
            diagnostic_artifact=args.diagnostic_artifact,
            risk_artifact=args.risk_artifact,
            gross_edge_artifact=args.gross_edge_artifact,
            family_decision_artifact=args.family_decision_artifact,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_regime_only_tsmom_falsification_artifacts(
            output_dir,
            payload,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "tsmom-regime-only-falsification-run",
                    "output_dir": str(written),
                    "regime_only_tsmom_falsification_decision": payload[
                        "regime_only_tsmom_falsification_decision"
                    ],
                    "regime_only_tsmom_falsification_namespace": payload[
                        "regime_only_tsmom_falsification_namespace"
                    ],
                    "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
                    "source_candidate_family_decision": payload["source_candidate_family_decision"],
                    "candidate_queue_status": payload["candidate_queue_status"],
                    "failed_regime_only_gates": payload["failed_regime_only_gates"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (FileNotFoundError, ValueError, FileExistsError) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "tsmom-regime-only-falsification-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_research_null_baseline_harness_run(args: argparse.Namespace) -> int:
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_research_null_baseline_harness_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        commands = args.command or [_default_research_null_baseline_harness_command(args)]
        changed_files = args.changed_file or []
        payload = build_research_null_baseline_harness_payload(
            source_loop_artifact=args.source_loop_artifact,
            diagnostic_artifact=args.diagnostic_artifact,
            risk_artifact=args.risk_artifact,
            gross_edge_artifact=args.gross_edge_artifact,
            family_decision_artifact=args.family_decision_artifact,
            regime_only_artifact=args.regime_only_artifact,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_research_null_baseline_harness_artifacts(
            output_dir,
            payload,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "research-null-baseline-harness-run",
                    "output_dir": str(written),
                    "research_null_baseline_harness_decision": payload[
                        "research_null_baseline_harness_decision"
                    ],
                    "research_null_baseline_harness_namespace": payload[
                        "research_null_baseline_harness_namespace"
                    ],
                    "readiness_baseline_verdict": payload["readiness_baseline_verdict"],
                    "comparison_row_count": payload["summary"]["comparison_row_count"],
                    "tsmom_state_changed": payload["summary"]["tsmom_state_changed"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (FileNotFoundError, ValueError, FileExistsError) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "research-null-baseline-harness-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_alpha_signal_factory_composition_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_alpha_signal_factory_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        market_bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(market_bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(market_bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
        commands = args.command or [_default_alpha_signal_factory_command(args)]
        changed_files = args.changed_file or []
        result = evaluate_alpha_signal_factory(
            market_bundle=market_bundle,
            funding_bundle=funding_bundle,
            seed=args.seed,
            max_depth=args.max_depth,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_alpha_signal_factory_artifacts(
            output_dir,
            result,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "alpha-signal-factory-composition-run",
                    "output_dir": str(written),
                    "suite_decision": result.payload["suite_decision"],
                    "readiness_baseline_verdict": result.payload["readiness_baseline_verdict"],
                    "generated_signal_count": result.payload["generated_signal_count"],
                    "evaluated_signal_count": result.payload["evaluated_signal_count"],
                    "effective_signal_breadth": result.payload["effective_signal_breadth"],
                    "duplicate_signal_count": result.payload["duplicate_signal_count"],
                    "watch_family_count": result.payload["watch_family_count"],
                    "tsmom_state_changed": result.payload["tsmom_state_changed"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": result.payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "alpha-signal-factory-composition-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_alpha_signal_dag_expansion_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_alpha_signal_dag_expansion_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        market_bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(market_bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(market_bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
        commands = args.command or [_default_alpha_signal_dag_expansion_command(args)]
        changed_files = args.changed_file or []
        result = evaluate_alpha_signal_dag_expansion(
            market_bundle=market_bundle,
            funding_bundle=funding_bundle,
            seed=args.seed,
            max_depth=args.max_depth,
            candidate_budget=args.candidate_budget,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_alpha_signal_dag_expansion_artifacts(
            output_dir,
            result,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "alpha-signal-dag-expansion-run",
                    "output_dir": str(written),
                    "suite_decision": result.payload["suite_decision"],
                    "readiness_baseline_verdict": result.payload["readiness_baseline_verdict"],
                    "generated_signal_count": result.payload["generated_signal_count"],
                    "evaluated_signal_count": result.payload["evaluated_signal_count"],
                    "effective_signal_breadth": result.payload["effective_signal_breadth"],
                    "duplicate_signal_count": result.payload["duplicate_signal_count"],
                    "duplicate_ratio": result.payload["duplicate_ratio"],
                    "watch_family_count": result.payload["watch_family_count"],
                    "tsmom_state_changed": result.payload["tsmom_state_changed"],
                    "holdout_retune_used": result.payload["holdout_retune_used"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": result.payload["claim_boundary"],
                    "multiple_testing_boundary": result.payload["multiple_testing_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "alpha-signal-dag-expansion-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_alpha_signal_oos_fdr_validation_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_alpha_signal_oos_fdr_validation_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        market_bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(market_bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(market_bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
        commands = args.command or [_default_alpha_signal_oos_fdr_validation_command(args)]
        changed_files = args.changed_file or []
        result = evaluate_alpha_signal_oos_fdr_validation(
            source_v2_artifact_path=args.source_v2_artifact,
            market_bundle=market_bundle,
            funding_bundle=funding_bundle,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_alpha_signal_oos_fdr_validation_artifacts(
            output_dir,
            result,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "alpha-signal-oos-fdr-validation-run",
                    "output_dir": str(written),
                    "suite_decision": result.payload["suite_decision"],
                    "readiness_baseline_verdict": result.payload["readiness_baseline_verdict"],
                    "source_v2_artifact_path": result.payload["source_v2_artifact_path"],
                    "frozen_candidate_count": result.payload["frozen_candidate_count"],
                    "evaluated_signal_count": result.payload["evaluated_signal_count"],
                    "chronological_fold_count": result.payload["chronological_fold_count"],
                    "effective_signal_breadth": result.payload["effective_signal_breadth"],
                    "duplicate_adjusted_test_count": result.payload["duplicate_adjusted_test_count"],
                    "families_watch_count": result.payload["families_watch_count"],
                    "families_killed_count": result.payload["families_killed_count"],
                    "tsmom_state_changed": result.payload["tsmom_state_changed"],
                    "holdout_retune_used": result.payload["holdout_retune_used"],
                    "strategy_optimization_performed": result.payload["strategy_optimization_performed"],
                    "live_or_testnet_path_created": result.payload["live_or_testnet_path_created"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": result.payload["claim_boundary"],
                    "multiple_testing_boundary": result.payload["multiple_testing_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
        FileNotFoundError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "alpha-signal-oos-fdr-validation-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "source_v2_artifact_path": str(args.source_v2_artifact),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_alpha_signal_strict_falsification_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_alpha_signal_strict_falsification_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        market_bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(market_bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(market_bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
        commands = args.command or [_default_alpha_signal_strict_falsification_command(args)]
        changed_files = args.changed_file or []
        result = evaluate_alpha_signal_strict_falsification(
            source_v3_artifact_path=args.source_v3_artifact,
            market_bundle=market_bundle,
            funding_bundle=funding_bundle,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_alpha_signal_strict_falsification_artifacts(
            output_dir,
            result,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "alpha-signal-strict-falsification-run",
                    "output_dir": str(written),
                    "suite_decision": result.payload["suite_decision"],
                    "readiness_baseline_verdict": result.payload["readiness_baseline_verdict"],
                    "source_v3_artifact_path": result.payload["source_v3_artifact_path"],
                    "preregistered_hypothesis_count": result.payload["preregistered_hypothesis_count"],
                    "evaluated_family_horizon_count": result.payload["evaluated_family_horizon_count"],
                    "frozen_candidate_count": result.payload["frozen_candidate_count"],
                    "evaluated_signal_count": result.payload["evaluated_signal_count"],
                    "strict_fold_count": result.payload["strict_fold_count"],
                    "purge_bars": result.payload["purge_bars"],
                    "embargo_bars": result.payload["embargo_bars"],
                    "bootstrap_resamples": result.payload["bootstrap_resamples"],
                    "survived_strict_falsification_count": result.payload["survived_strict_falsification_count"],
                    "falsified_count": result.payload["falsified_count"],
                    "tsmom_state_changed": result.payload["tsmom_state_changed"],
                    "holdout_retune_used": result.payload["holdout_retune_used"],
                    "strategy_optimization_performed": result.payload["strategy_optimization_performed"],
                    "live_or_testnet_path_created": result.payload["live_or_testnet_path_created"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": result.payload["claim_boundary"],
                    "multiple_testing_boundary": result.payload["multiple_testing_boundary"],
                    "preregistration_boundary": result.payload["preregistration_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
        FileNotFoundError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "alpha-signal-strict-falsification-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "source_v3_artifact_path": str(args.source_v3_artifact),
                    "symbol": symbol,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_multi_asset_alpha_generator_strict_prefilter_run(args: argparse.Namespace) -> int:
    symbols = parse_symbol_universe(args.symbols)
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_multi_asset_alpha_prefilter_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        symbols = tuple(validate_symbol(symbol) for symbol in symbols)
        market_bundles, funding_bundles, excluded, funding_status = _load_multi_asset_public_bundles(
            args,
            symbols=symbols,
            interval=interval,
            months=months,
        )
        commands = args.command or [_default_multi_asset_alpha_prefilter_command(args)]
        changed_files = args.changed_file or []
        result = evaluate_multi_asset_alpha_prefilter(
            market_bundles=market_bundles,
            funding_bundles=funding_bundles,
            requested_symbols=symbols,
            excluded_symbols=excluded,
            funding_status_by_symbol=funding_status,
            source_v4_artifact_path=args.source_v4_artifact,
            seed=args.seed,
            candidate_budget=args.candidate_budget,
            max_depth=args.max_depth,
            min_bars_per_symbol=args.min_bars_per_symbol,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_multi_asset_alpha_prefilter_artifacts(
            output_dir,
            result,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "multi-asset-alpha-generator-strict-prefilter-run",
                    "output_dir": str(written),
                    "suite_decision": result.payload["suite_decision"],
                    "readiness_baseline_verdict": result.payload["readiness_baseline_verdict"],
                    "source_v4_artifact_path": result.payload["source_v4_artifact_path"],
                    "v4_queue_empty": result.payload["v4_queue_empty"],
                    "symbol_universe_requested": result.payload["symbol_universe_requested"],
                    "symbol_universe_used": result.payload["symbol_universe_used"],
                    "excluded_symbols": result.payload["excluded_symbols"],
                    "generated_signal_count": result.payload["generated_signal_count"],
                    "evaluated_signal_count": result.payload["evaluated_signal_count"],
                    "effective_signal_breadth": result.payload["effective_signal_breadth"],
                    "duplicate_ratio": result.payload["duplicate_ratio"],
                    "watch_family_count": result.payload["watch_family_count"],
                    "tsmom_state_changed": result.payload["tsmom_state_changed"],
                    "holdout_retune_used": result.payload["holdout_retune_used"],
                    "strategy_optimization_performed": result.payload["strategy_optimization_performed"],
                    "live_or_testnet_path_created": result.payload["live_or_testnet_path_created"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": result.payload["claim_boundary"],
                    "multiple_testing_boundary": result.payload["multiple_testing_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        DataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
        FileNotFoundError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "multi-asset-alpha-generator-strict-prefilter-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "source_v4_artifact_path": str(args.source_v4_artifact),
                    "symbols": symbols,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_panel_ready_multi_asset_alpha_prefilter_rerun(args: argparse.Namespace) -> int:
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    symbols: tuple[str, ...] = ()
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_panel_ready_multi_asset_alpha_prefilter_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        v7_state = read_panel_ready_multi_asset_v7_source_state(args.source_v7_artifact)
        source_interval = validate_interval(str(v7_state["interval"]))
        interval = validate_interval(args.interval or source_interval)
        if interval != source_interval:
            raise ValueError("V8 interval override must match the V7-ready panel interval")
        months = tuple(str(month) for month in v7_state["months"])
        symbols = tuple(str(symbol) for symbol in v7_state["usable_symbols"])
        loader_args = argparse.Namespace(
            cache_dir=args.cache_dir,
            funding_source=args.funding_source,
            funding_cache_dir=args.funding_cache_dir,
            cache_mode="cache-only",
            min_bars_per_symbol=args.min_bars_per_symbol,
        )
        market_bundles, funding_bundles, excluded, funding_status = _load_multi_asset_public_bundles(
            loader_args,
            symbols=symbols,
            interval=interval,
            months=months,
        )
        missing_ready_symbols = [symbol for symbol in symbols if symbol not in market_bundles]
        if missing_ready_symbols:
            raise ValueError(
                "V8 cache-only rerun could not load all V7 usable symbols; "
                f"refusing download/backfill: {missing_ready_symbols}; loader_exclusions={excluded}"
            )
        commands = args.command or [_default_panel_ready_multi_asset_alpha_prefilter_command(args)]
        changed_files = args.changed_file or []
        result = evaluate_panel_ready_multi_asset_alpha_prefilter_rerun(
            source_v7_artifact_path=args.source_v7_artifact,
            market_bundles=market_bundles,
            funding_bundles=funding_bundles,
            funding_status_by_symbol=funding_status,
            source_v4_artifact_path=args.source_v4_artifact,
            seed=args.seed,
            candidate_budget=args.candidate_budget,
            max_depth=args.max_depth,
            min_bars_per_symbol=args.min_bars_per_symbol,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_panel_ready_multi_asset_alpha_prefilter_artifacts(
            output_dir,
            result,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "panel-ready-multi-asset-alpha-prefilter-rerun",
                    "output_dir": str(written),
                    "suite_decision": result.payload["suite_decision"],
                    "readiness_baseline_verdict": result.payload["readiness_baseline_verdict"],
                    "source_v7_artifact_path": result.payload["source_v7_artifact_path"],
                    "source_v7_panel_ready": result.payload["source_v7_panel_ready"],
                    "symbol_universe_requested": result.payload["symbol_universe_requested"],
                    "symbol_universe_used": result.payload["symbol_universe_used"],
                    "excluded_symbols": result.payload["excluded_symbols"],
                    "common_start": result.payload["common_start"],
                    "common_end": result.payload["common_end"],
                    "common_bar_count": result.payload["common_bar_count"],
                    "generated_signal_count": result.payload["generated_signal_count"],
                    "evaluated_signal_count": result.payload["evaluated_signal_count"],
                    "effective_signal_breadth": result.payload["effective_signal_breadth"],
                    "duplicate_ratio": result.payload["duplicate_ratio"],
                    "watch_family_count": result.payload["watch_family_count"],
                    "data_download_performed": result.payload["data_download_performed"],
                    "backfill_performed": result.payload["backfill_performed"],
                    "tsmom_state_changed": result.payload["tsmom_state_changed"],
                    "holdout_retune_used": result.payload["holdout_retune_used"],
                    "strategy_optimization_performed": result.payload["strategy_optimization_performed"],
                    "live_or_testnet_path_created": result.payload["live_or_testnet_path_created"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": result.payload["claim_boundary"],
                    "multiple_testing_boundary": result.payload["multiple_testing_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        DataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
        FileNotFoundError,
    ) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "panel-ready-multi-asset-alpha-prefilter-rerun",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "source_v7_artifact_path": str(args.source_v7_artifact),
                    "source_v4_artifact_path": str(args.source_v4_artifact),
                    "symbols": symbols,
                    "interval": interval,
                    "months": months,
                    "data_download_performed": False,
                    "backfill_performed": False,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                    "private_data_used": False,
                    "account_data_used": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_public_multi_asset_panel_coverage_run(args: argparse.Namespace) -> int:
    symbols = PUBLIC_MULTI_ASSET_PANEL_DEFAULT_UNIVERSE
    interval = CANONICAL_INTERVAL
    months = default_data_depth_months()
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_public_multi_asset_panel_coverage_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        symbols = parse_public_symbol_universe(args.symbols)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        commands = args.command or [_default_public_multi_asset_panel_coverage_command(args)]
        changed_files = args.changed_file or []
        result = evaluate_public_multi_asset_panel_coverage(
            requested_symbols=symbols,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
            funding_source=args.funding_source,
            funding_cache_dir=args.funding_cache_dir,
            cache_mode=args.cache_mode,
            minimum_required_symbols=args.minimum_required_symbols,
            source_v5_artifact_path=args.source_v5_artifact,
            market_loader=load_market_data_bundle,
            funding_loader=fetch_funding_rate_history,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_public_multi_asset_panel_coverage_artifacts(
            output_dir,
            result,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "public-multi-asset-panel-coverage-run",
                    "output_dir": str(written),
                    "suite_decision": result.payload["suite_decision"],
                    "readiness_baseline_verdict": result.payload["readiness_baseline_verdict"],
                    "source_v5_artifact_path": result.payload["source_v5_artifact_path"],
                    "requested_symbol_universe": result.payload["requested_symbol_universe"],
                    "usable_symbol_universe": result.payload["usable_symbol_universe"],
                    "excluded_symbols": result.payload["excluded_symbols"],
                    "panel_ready_for_v5_rerun": result.payload["panel_ready_for_v5_rerun"],
                    "common_start": result.payload["common_start"],
                    "common_end": result.payload["common_end"],
                    "common_bar_count": result.payload["common_bar_count"],
                    "tsmom_state_changed": result.payload["tsmom_state_changed"],
                    "holdout_retune_used": result.payload["holdout_retune_used"],
                    "strategy_optimization_performed": result.payload["strategy_optimization_performed"],
                    "live_or_testnet_path_created": result.payload["live_or_testnet_path_created"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": result.payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (ValueError, FileExistsError, FileNotFoundError, requests.RequestException) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "public-multi-asset-panel-coverage-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "source_v5_artifact_path": str(args.source_v5_artifact),
                    "symbols": symbols,
                    "interval": interval,
                    "months": months,
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                    "private_data_used": False,
                    "account_data_used": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def cmd_public_multi_asset_data_backfill_run(args: argparse.Namespace) -> int:
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else default_public_multi_asset_data_backfill_output_dir(root=Path(args.reports_root) / "loops")
    )
    try:
        commands = args.command or [_default_public_multi_asset_data_backfill_command(args)]
        changed_files = args.changed_file or []
        result = evaluate_public_multi_asset_data_backfill(
            source_v6_artifact_path=args.source_v6_artifact,
            requested_symbols=args.symbols,
            interval=args.interval,
            start_month=args.start_month,
            end_month=args.end_month,
            cache_dir=args.cache_dir,
            funding_source=args.funding_source,
            funding_cache_dir=args.funding_cache_dir,
            minimum_required_symbols=args.minimum_required_symbols,
            plan_only=args.plan_only,
            force=args.force,
            ohlcv_downloader=download_monthly_klines,
            market_loader=load_market_data_bundle,
            funding_loader=fetch_funding_rate_history,
            commands=commands,
            changed_files=changed_files,
        )
        written = write_public_multi_asset_data_backfill_artifacts(
            output_dir,
            result,
            overwrite=args.overwrite,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "public-multi-asset-data-backfill-run",
                    "output_dir": str(written),
                    "suite_decision": result.payload["suite_decision"],
                    "readiness_baseline_verdict": result.payload["readiness_baseline_verdict"],
                    "source_v6_artifact_path": result.payload["source_v6_artifact_path"],
                    "planned_download_count": result.payload["planned_download_count"],
                    "skipped_existing_count": result.payload["skipped_existing_count"],
                    "successful_download_count": result.payload["successful_download_count"],
                    "failed_download_count": result.payload["failed_download_count"],
                    "usable_symbol_universe_before": result.payload["usable_symbol_universe_before"],
                    "usable_symbol_universe_after": result.payload["usable_symbol_universe_after"],
                    "panel_ready_for_v5_rerun": result.payload["panel_ready_for_v5_rerun"],
                    "raw_data_files_committed": result.payload["raw_data_files_committed"],
                    "tsmom_state_changed": result.payload["tsmom_state_changed"],
                    "holdout_retune_used": result.payload["holdout_retune_used"],
                    "strategy_optimization_performed": result.payload["strategy_optimization_performed"],
                    "live_or_testnet_path_created": result.payload["live_or_testnet_path_created"],
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "claim_boundary": result.payload["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (ValueError, FileExistsError, FileNotFoundError, requests.RequestException) as exc:
        print(
            json.dumps(
                {
                    "status": "failed",
                    "run_type": "public-multi-asset-data-backfill-run",
                    "failed_gate": type(exc).__name__,
                    "reason": str(exc),
                    "output_dir": str(output_dir),
                    "source_v6_artifact_path": str(args.source_v6_artifact),
                    "live_orders_submitted": False,
                    "testnet_orders_submitted": False,
                    "api_keys_required_for_research": False,
                    "private_data_used": False,
                    "account_data_used": False,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


def _two_way_regime_segments(frame: pd.DataFrame, split_index: int) -> pd.Series:
    segments = pd.Series(["backtest"] * len(frame), index=frame.index, dtype="object")
    segments.iloc[split_index:] = "holdout_paper"
    return segments


def _risk_regime_frame_and_segments(risk_result) -> tuple[pd.DataFrame, pd.Series]:
    frames = [
        risk_result.split.train,
        risk_result.split.validation,
        risk_result.split.holdout,
    ]
    labels = ["train", "validation", "holdout_paper"]
    combined = pd.concat(frames)
    segments = pd.Series(
        [label for frame, label in zip(frames, labels, strict=True) for _ in range(len(frame))],
        index=combined.index,
        dtype="object",
    )
    return combined, segments


def cmd_real_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = CANONICAL_MONTHS
    bundle = None
    split_index = None
    output_dir: Path | None = None
    try:
        output_dir = prepare_output_dir(
            reports_root=args.reports_root,
            output_dir=args.output_dir,
            overwrite=args.overwrite,
        )
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        _validate_canonical_real_run(symbol, interval, months)
        bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        train, holdout, split_index = split_chronological(bundle.frame, train_fraction=0.70)
        backtest = run_strategy_backtest(train, label="backtest")
        holdout_result = run_strategy_backtest(holdout, label="holdout_paper")
        meaning = assert_meaningful_run(
            total_bars=len(bundle.frame),
            train_bars=len(train),
            holdout_bars=len(holdout),
            backtest_closed_trades=backtest.metrics["closed_trades"],
            holdout_closed_trades=holdout_result.metrics["closed_trades"],
        )
        manifest = build_manifest(
            bundle=bundle,
            symbol=symbol,
            interval=interval,
            months=months,
            split_index=split_index,
            project_root=ROOT,
        )
        segments = _two_way_regime_segments(bundle.frame, split_index)
        regime_report = build_regime_report(bundle.frame, segments=segments)
        regime_labels = cast(pd.Series, regime_report.rows["regime_label"])
        signal_report = build_signal_report(
            bundle.frame,
            segments=segments,
            regimes=regime_labels,
            asset=symbol,
            timeframe=interval,
        )
        write_success_report(
            output_dir=output_dir,
            manifest=manifest,
            meaning=meaning,
            backtest=backtest,
            holdout=holdout_result,
            regime_report=regime_report,
            signal_report=signal_report,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "report_dir": str(output_dir),
                    "strategy": strategy_settings_dict(),
                    "broker": broker_settings_dict(),
                    "meaning_gate": meaning,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except MeaningGateError as exc:
        if output_dir is None:
            output_dir = prepare_output_dir(reports_root=args.reports_root)
        manifest = build_manifest(
            bundle=bundle,
            symbol=symbol,
            interval=interval,
            months=months,
            split_index=split_index,
            project_root=ROOT,
        )
        write_failure_report(
            output_dir=output_dir,
            manifest=manifest,
            failed_gate=exc.failed_gate,
            reason=exc.reason,
            observed=exc.observed,
        )
        print(_failure_payload(output_dir, exc.failed_gate, exc.reason))
        return 2
    except (BinanceVisionError, DataValidationError, requests.RequestException, ValueError, FileExistsError) as exc:
        if output_dir is None and not isinstance(exc, FileExistsError):
            output_dir = prepare_output_dir(reports_root=args.reports_root)
        if output_dir is None:
            print(_failure_payload(None, "output_dir", str(exc)))
            return 2
        manifest = build_manifest(
            bundle=bundle,
            symbol=symbol,
            interval=interval,
            months=months,
            split_index=split_index,
            project_root=ROOT,
        )
        write_failure_report(
            output_dir=output_dir,
            manifest=manifest,
            failed_gate=type(exc).__name__,
            reason=str(exc),
            observed={"symbol": symbol, "interval": interval, "months": months},
        )
        print(_failure_payload(output_dir, type(exc).__name__, str(exc)))
        return 2


def cmd_risk_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = CANONICAL_MONTHS
    bundle = None
    output_dir: Path | None = None
    product_family = ProductFamily.SPOT_CRYPTO
    try:
        output_dir = prepare_output_dir(
            reports_root=args.reports_root,
            output_dir=args.output_dir,
            overwrite=args.overwrite,
            run_prefix="risk-run",
        )
        product_family = ProductFamily(args.product_family)
        product_policy = load_product_policies(PRODUCTS_CONFIG)[product_family]
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        _validate_canonical_real_run(symbol, interval, months)
        bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        risk_result = run_risk_candidate_search(
            bundle.frame,
            product_family=product_family,
            short_allowed=product_policy.short_allowed,
        )
        regime_frame, regime_segments = _risk_regime_frame_and_segments(risk_result)
        regime_report = build_regime_report(regime_frame, segments=regime_segments)
        regime_labels = cast(pd.Series, regime_report.rows["regime_label"])
        signal_report = build_signal_report(
            regime_frame,
            segments=regime_segments,
            regimes=regime_labels,
            asset=symbol,
            timeframe=interval,
        )
        manifest = build_manifest(
            bundle=bundle,
            symbol=symbol,
            interval=interval,
            months=months,
            split_index=risk_result.split.metadata["train_end_index"],
            project_root=ROOT,
        )
        metrics = write_risk_success_report(
            output_dir=output_dir,
            manifest=manifest,
            risk_result=risk_result,
            regime_report=regime_report,
            signal_report=signal_report,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "report_dir": str(output_dir),
                    "product_family": product_family.value,
                    "selection_objective": metrics["selection_objective"],
                    "candidate_count": metrics["candidate_count"],
                    "selected_candidate_id": metrics["selected_candidate_id"],
                    "selected_settings": metrics["selected_settings"],
                    "holdout_used_for_selection": False,
                    "claim_boundary": metrics["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except MeaningGateError as exc:
        if output_dir is None:
            output_dir = prepare_output_dir(reports_root=args.reports_root, run_prefix="risk-run")
        manifest = build_manifest(
            bundle=bundle,
            symbol=symbol,
            interval=interval,
            months=months,
            split_index=None,
            project_root=ROOT,
        )
        write_failure_report(
            output_dir=output_dir,
            manifest=manifest,
            failed_gate=exc.failed_gate,
            reason=exc.reason,
            observed=exc.observed,
            run_type="risk-run",
        )
        print(_failure_payload(output_dir, exc.failed_gate, exc.reason))
        return 2
    except (BinanceVisionError, DataValidationError, requests.RequestException, ValueError, FileExistsError) as exc:
        if output_dir is None and not isinstance(exc, FileExistsError):
            output_dir = prepare_output_dir(reports_root=args.reports_root, run_prefix="risk-run")
        if output_dir is None:
            print(_failure_payload(None, "output_dir", str(exc)))
            return 2
        manifest = build_manifest(
            bundle=bundle,
            symbol=symbol,
            interval=interval,
            months=months,
            split_index=None,
            project_root=ROOT,
        )
        write_failure_report(
            output_dir=output_dir,
            manifest=manifest,
            failed_gate=type(exc).__name__,
            reason=str(exc),
            observed={"symbol": symbol, "interval": interval, "months": months, "product_family": product_family.value},
            run_type="risk-run",
        )
        print(_failure_payload(output_dir, type(exc).__name__, str(exc)))
        return 2


def cmd_readiness_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    months = CANONICAL_MONTHS
    bundle = None
    output_dir: Path | None = None
    product_family = ProductFamily.SPOT_CRYPTO
    try:
        output_dir = prepare_output_dir(
            reports_root=args.reports_root,
            output_dir=args.output_dir,
            overwrite=args.overwrite,
            run_prefix="readiness-run",
        )
        product_family = ProductFamily(args.product_family)
        product_policy = load_product_policies(PRODUCTS_CONFIG)[product_family]
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        months = _parse_months(args.months)
        _validate_canonical_real_run(symbol, interval, months)
        bundle = load_market_data_bundle(
            symbol=symbol,
            interval=interval,
            months=months,
            cache_dir=args.cache_dir,
        )
        funding_bundle = None
        funding_metadata = None
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
            funding_metadata = funding_metadata_for_report(funding_bundle)
        readiness_report = build_readiness_report(
            bundle.frame,
            product_family=product_family,
            short_allowed=product_policy.short_allowed,
            funding_frame=funding_bundle.frame if funding_bundle is not None else None,
            funding_metadata=funding_metadata,
        )
        manifest = build_manifest(
            bundle=bundle,
            symbol=symbol,
            interval=interval,
            months=months,
            split_index=None,
            project_root=ROOT,
        )
        manifest["product_family"] = product_family.value
        metrics = write_readiness_success_report(
            output_dir=output_dir,
            manifest=manifest,
            readiness_report=readiness_report,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "report_dir": str(output_dir),
                    "run_type": "readiness-run",
                    "readiness_verdict": metrics["verdict"],
                    "failed_gates": metrics["failed_gates"],
                    "window_count": metrics["window_count"],
                    "live_orders_submitted": False,
                    "claim_boundary": metrics["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceVisionError,
        BinanceFundingError,
        DataValidationError,
        FundingDataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        if output_dir is None and not isinstance(exc, FileExistsError):
            output_dir = prepare_output_dir(reports_root=args.reports_root, run_prefix="readiness-run")
        if output_dir is None:
            print(_failure_payload(None, "output_dir", str(exc)))
            return 2
        manifest = build_manifest(
            bundle=bundle,
            symbol=symbol,
            interval=interval,
            months=months,
            split_index=None,
            project_root=ROOT,
        )
        manifest["product_family"] = product_family.value
        write_failure_report(
            output_dir=output_dir,
            manifest=manifest,
            failed_gate=type(exc).__name__,
            reason=str(exc),
            observed={"symbol": symbol, "interval": interval, "months": months, "product_family": product_family.value},
            run_type="readiness-run",
        )
        print(_failure_payload(output_dir, type(exc).__name__, str(exc)))
        return 2


def cmd_forward_paper_run(args: argparse.Namespace) -> int:
    symbol = CANONICAL_SYMBOL
    interval = CANONICAL_INTERVAL
    kline_bundle = None
    funding_metadata = None
    output_dir: Path | None = None
    try:
        output_dir = prepare_output_dir(
            reports_root=args.reports_root,
            output_dir=args.output_dir,
            overwrite=args.overwrite,
            run_prefix="forward-paper-run",
        )
        symbol = validate_symbol(args.symbol)
        interval = validate_interval(args.interval)
        kline_bundle = fetch_recent_klines(
            symbol=symbol,
            interval=interval,
            limit=args.lookback_bars,
            include_open_bar=args.include_open_bar,
        )
        funding_bundle = None
        if args.funding_source == "public-binance":
            funding_bundle = fetch_funding_rate_history(
                symbol=symbol,
                start_time=_datetime_index_endpoint(kline_bundle.frame.index, 0),
                end_time=_datetime_index_endpoint(kline_bundle.frame.index, -1),
                cache_dir=args.funding_cache_dir,
            )
            funding_metadata = funding_metadata_for_report(funding_bundle)
        else:
            funding_metadata = {
                "source": "none",
                "rows": 0,
                "limitation": "funding disabled by explicit operator flag; reported paper costs exclude funding cashflows",
            }
        config = ForwardPaperConfig(
            initial_cash=args.initial_cash,
            fee_rate=args.fee_rate,
            slippage_bps=args.slippage_bps,
            entry_threshold=args.entry_threshold,
            exit_threshold=args.exit_threshold,
            max_exposure=args.max_exposure,
            funding_source=args.funding_source,
        )
        result = run_forward_paper(
            kline_bundle.frame,
            config=config,
            funding_frame=funding_bundle.frame if funding_bundle is not None else None,
            symbol=symbol,
            interval=interval,
        )
        manifest = build_forward_paper_manifest(
            kline_bundle=kline_bundle,
            funding_metadata=funding_metadata,
            symbol=symbol,
            interval=interval,
            project_root=ROOT,
        )
        metrics = write_forward_paper_success_report(
            output_dir=output_dir,
            manifest=manifest,
            result=result,
        )
        print(
            json.dumps(
                {
                    "status": "success",
                    "run_type": "forward-paper-run",
                    "report_dir": str(output_dir),
                    "symbol": symbol,
                    "interval": interval,
                    "final_equity": metrics["final_equity"],
                    "return_pct": metrics["return_pct"],
                    "max_drawdown_pct": metrics["max_drawdown_pct"],
                    "live_orders_submitted": False,
                    "claim_boundary": metrics["claim_boundary"],
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (
        BinanceLiveKlineError,
        LiveKlineValidationError,
        BinanceFundingError,
        FundingDataValidationError,
        DataValidationError,
        requests.RequestException,
        ValueError,
        FileExistsError,
    ) as exc:
        if output_dir is None and not isinstance(exc, FileExistsError):
            output_dir = prepare_output_dir(reports_root=args.reports_root, run_prefix="forward-paper-run")
        if output_dir is None:
            print(_failure_payload(None, "output_dir", str(exc)))
            return 2
        manifest = {
            "run_type": "forward-paper-run",
            "symbol": symbol,
            "interval": interval,
            "live_orders_submitted": False,
            "api_keys_required": False,
        }
        if kline_bundle is not None:
            manifest = build_forward_paper_manifest(
                kline_bundle=kline_bundle,
                funding_metadata=funding_metadata,
                symbol=symbol,
                interval=interval,
                project_root=ROOT,
            )
        write_failure_report(
            output_dir=output_dir,
            manifest=manifest,
            failed_gate=type(exc).__name__,
            reason=str(exc),
            observed={"symbol": symbol, "interval": interval, "lookback_bars": args.lookback_bars},
            run_type="forward-paper-run",
        )
        print(_failure_payload(output_dir, type(exc).__name__, str(exc)))
        return 2


def cmd_kill_switch(args: argparse.Namespace) -> int:
    path = (ROOT / ".runtime" / "KILL_SWITCH").resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    if args.action == "enable":
        path.write_text("enabled\n")
    else:
        path.unlink(missing_ok=True)
    print(json.dumps({"kill_switch": args.action, "path": str(path)}))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="quantctl")
    sub = parser.add_subparsers(dest="cmd", required=True)

    backtest = sub.add_parser("backtest")
    backtest.add_argument("--fixture", action="store_true")
    backtest.add_argument("--strategy", default="trend-risk")
    backtest.add_argument("--product-family", default="spot_crypto")
    backtest.set_defaults(func=cmd_backtest)

    paper = sub.add_parser("paper-run")
    paper.add_argument("--fixture", action="store_true")
    paper.add_argument("--ticks", type=int, default=10)
    paper.add_argument("--event-log", default="logs/paper-events.jsonl")
    paper.set_defaults(func=cmd_paper_run)

    replay = sub.add_parser("replay")
    replay.add_argument("--event-log", default="logs/paper-events.jsonl")
    replay.set_defaults(func=cmd_replay)

    dry = sub.add_parser("live-dry-run")
    dry.add_argument("--fixture", action="store_true")
    dry.add_argument("--product-family", default="spot_crypto")
    dry.set_defaults(func=cmd_live_dry_run)


    data_depth = sub.add_parser("data-depth-run")
    data_depth.add_argument("--symbol", default=CANONICAL_SYMBOL)
    data_depth.add_argument("--interval", default=CANONICAL_INTERVAL)
    data_depth.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; default is 2024-01..2025-12 for data-depth evidence",
    )
    data_depth.add_argument("--cache-dir", default="data/binance-vision")
    data_depth.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    data_depth.add_argument("--funding-cache-dir", default="data/binance-funding")
    data_depth.add_argument("--reports-root", default="reports")
    data_depth.add_argument("--output-dir")
    data_depth.add_argument("--overwrite", action="store_true")
    data_depth.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    data_depth.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    data_depth.set_defaults(func=cmd_data_depth_run)

    tsmom = sub.add_parser("tsmom-run")
    tsmom.add_argument("--symbol", default=CANONICAL_SYMBOL)
    tsmom.add_argument("--interval", default=CANONICAL_INTERVAL)
    tsmom.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; default is 2024-01..2025-12 for TSMOM validation",
    )
    tsmom.add_argument("--cache-dir", default="data/binance-vision")
    tsmom.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    tsmom.add_argument("--funding-cache-dir", default="data/binance-funding")
    tsmom.add_argument("--reports-root", default="reports")
    tsmom.add_argument("--output-dir")
    tsmom.add_argument("--overwrite", action="store_true")
    tsmom.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    tsmom.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    tsmom.set_defaults(func=cmd_tsmom_run)

    tsmom_diag = sub.add_parser("tsmom-diagnostic-run")
    tsmom_diag.add_argument("--symbol", default=CANONICAL_SYMBOL)
    tsmom_diag.add_argument("--interval", default=CANONICAL_INTERVAL)
    tsmom_diag.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; default is 2024-01..2025-12 for TSMOM diagnostics",
    )
    tsmom_diag.add_argument("--cache-dir", default="data/binance-vision")
    tsmom_diag.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    tsmom_diag.add_argument("--funding-cache-dir", default="data/binance-funding")
    tsmom_diag.add_argument("--reports-root", default="reports")
    tsmom_diag.add_argument("--output-dir")
    tsmom_diag.add_argument("--overwrite", action="store_true")
    tsmom_diag.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    tsmom_diag.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    tsmom_diag.set_defaults(func=cmd_tsmom_diagnostic_run)

    tsmom_risk = sub.add_parser("tsmom-risk-run")
    tsmom_risk.add_argument("--symbol", default=CANONICAL_SYMBOL)
    tsmom_risk.add_argument("--interval", default=CANONICAL_INTERVAL)
    tsmom_risk.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; default is 2024-01..2025-12 for TSMOM risk/cost research",
    )
    tsmom_risk.add_argument("--cache-dir", default="data/binance-vision")
    tsmom_risk.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    tsmom_risk.add_argument("--funding-cache-dir", default="data/binance-funding")
    tsmom_risk.add_argument("--reports-root", default="reports")
    tsmom_risk.add_argument("--output-dir")
    tsmom_risk.add_argument("--overwrite", action="store_true")
    tsmom_risk.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    tsmom_risk.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    tsmom_risk.set_defaults(func=cmd_tsmom_risk_run)

    tsmom_gross = sub.add_parser("tsmom-gross-edge-run")
    tsmom_gross.add_argument("--symbol", default=CANONICAL_SYMBOL)
    tsmom_gross.add_argument("--interval", default=CANONICAL_INTERVAL)
    tsmom_gross.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; default is 2024-01..2025-12 for TSMOM gross-edge diagnostics",
    )
    tsmom_gross.add_argument("--cache-dir", default="data/binance-vision")
    tsmom_gross.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    tsmom_gross.add_argument("--funding-cache-dir", default="data/binance-funding")
    tsmom_gross.add_argument("--reports-root", default="reports")
    tsmom_gross.add_argument("--output-dir")
    tsmom_gross.add_argument("--overwrite", action="store_true")
    tsmom_gross.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    tsmom_gross.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    tsmom_gross.set_defaults(func=cmd_tsmom_gross_edge_run)

    tsmom_family = sub.add_parser("tsmom-family-decision-run")
    tsmom_family.add_argument("--reports-root", default="reports")
    tsmom_family.add_argument("--output-dir")
    tsmom_family.add_argument("--overwrite", action="store_true")
    tsmom_family.add_argument(
        "--source-loop-artifact",
        default=str(DEFAULT_TSMOM_SOURCE_LOOP_ARTIFACT),
        help="durable TSMOM source loop artifact path",
    )
    tsmom_family.add_argument(
        "--diagnostic-artifact",
        default=str(DEFAULT_TSMOM_DIAGNOSTIC_ARTIFACT),
        help="durable TSMOM stress/switch/funding diagnostic artifact path",
    )
    tsmom_family.add_argument(
        "--risk-artifact",
        default=str(DEFAULT_TSMOM_RISK_ARTIFACT),
        help="durable TSMOM switch-stability/funding-throttle risk artifact path",
    )
    tsmom_family.add_argument(
        "--gross-edge-artifact",
        default=str(DEFAULT_TSMOM_GROSS_EDGE_ARTIFACT),
        help="durable TSMOM gross-edge/regime/null robustness artifact path",
    )
    tsmom_family.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    tsmom_family.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    tsmom_family.set_defaults(func=cmd_tsmom_family_decision_run)

    regime_only = sub.add_parser("tsmom-regime-only-falsification-run")
    regime_only.add_argument("--reports-root", default="reports")
    regime_only.add_argument("--output-dir")
    regime_only.add_argument("--overwrite", action="store_true")
    regime_only.add_argument(
        "--source-loop-artifact",
        default=str(DEFAULT_TSMOM_SOURCE_LOOP_ARTIFACT),
        help="durable TSMOM source loop artifact path",
    )
    regime_only.add_argument(
        "--diagnostic-artifact",
        default=str(DEFAULT_TSMOM_DIAGNOSTIC_ARTIFACT),
        help="durable TSMOM stress/switch/funding diagnostic artifact path",
    )
    regime_only.add_argument(
        "--risk-artifact",
        default=str(DEFAULT_TSMOM_RISK_ARTIFACT),
        help="durable TSMOM switch-stability/funding-throttle risk artifact path",
    )
    regime_only.add_argument(
        "--gross-edge-artifact",
        default=str(DEFAULT_TSMOM_GROSS_EDGE_ARTIFACT),
        help="durable TSMOM gross-edge/regime/null robustness artifact path",
    )
    regime_only.add_argument(
        "--family-decision-artifact",
        default=str(DEFAULT_TSMOM_FAMILY_DECISION_ARTIFACT),
        help="durable TSMOM candidate-family decision artifact path",
    )
    regime_only.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    regime_only.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    regime_only.set_defaults(func=cmd_tsmom_regime_only_falsification_run)

    null_harness = sub.add_parser("research-null-baseline-harness-run")
    null_harness.add_argument("--reports-root", default="reports")
    null_harness.add_argument("--output-dir")
    null_harness.add_argument("--overwrite", action="store_true")
    null_harness.add_argument(
        "--source-loop-artifact",
        default=str(DEFAULT_TSMOM_SOURCE_LOOP_ARTIFACT),
        help="durable TSMOM source loop artifact path",
    )
    null_harness.add_argument(
        "--diagnostic-artifact",
        default=str(DEFAULT_TSMOM_DIAGNOSTIC_ARTIFACT),
        help="durable TSMOM stress/switch/funding diagnostic artifact path",
    )
    null_harness.add_argument(
        "--risk-artifact",
        default=str(DEFAULT_TSMOM_RISK_ARTIFACT),
        help="durable TSMOM switch-stability/funding-throttle risk artifact path",
    )
    null_harness.add_argument(
        "--gross-edge-artifact",
        default=str(DEFAULT_TSMOM_GROSS_EDGE_ARTIFACT),
        help="durable TSMOM gross-edge/null robustness artifact path",
    )
    null_harness.add_argument(
        "--family-decision-artifact",
        default=str(DEFAULT_TSMOM_FAMILY_DECISION_ARTIFACT),
        help="durable TSMOM candidate-family decision artifact path",
    )
    null_harness.add_argument(
        "--regime-only-artifact",
        default=str(DEFAULT_TSMOM_REGIME_ONLY_ARTIFACT),
        help="durable regime-only TSMOM falsification artifact path",
    )
    null_harness.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    null_harness.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    null_harness.set_defaults(func=cmd_research_null_baseline_harness_run)

    alpha_factory = sub.add_parser("alpha-signal-factory-composition-run")
    alpha_factory.add_argument("--symbol", default=CANONICAL_SYMBOL)
    alpha_factory.add_argument("--interval", default=CANONICAL_INTERVAL)
    alpha_factory.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; default is 2024-01..2025-12 for alpha signal diagnostics",
    )
    alpha_factory.add_argument("--cache-dir", default="data/binance-vision")
    alpha_factory.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    alpha_factory.add_argument("--funding-cache-dir", default="data/binance-funding")
    alpha_factory.add_argument("--seed", type=int, default=DEFAULT_SIGNAL_GENERATION_SEED)
    alpha_factory.add_argument("--max-depth", type=int, default=DEFAULT_MAX_DEPTH)
    alpha_factory.add_argument("--reports-root", default="reports")
    alpha_factory.add_argument("--output-dir")
    alpha_factory.add_argument("--overwrite", action="store_true")
    alpha_factory.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    alpha_factory.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    alpha_factory.set_defaults(func=cmd_alpha_signal_factory_composition_run)

    alpha_dag = sub.add_parser("alpha-signal-dag-expansion-run")
    alpha_dag.add_argument("--symbol", default=CANONICAL_SYMBOL)
    alpha_dag.add_argument("--interval", default=CANONICAL_INTERVAL)
    alpha_dag.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; default is 2024-01..2025-12 for alpha DAG diagnostics",
    )
    alpha_dag.add_argument("--cache-dir", default="data/binance-vision")
    alpha_dag.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    alpha_dag.add_argument("--funding-cache-dir", default="data/binance-funding")
    alpha_dag.add_argument("--seed", type=int, default=DEFAULT_SIGNAL_DAG_GENERATION_SEED)
    alpha_dag.add_argument("--max-depth", type=int, default=DEFAULT_SIGNAL_DAG_MAX_DEPTH)
    alpha_dag.add_argument("--candidate-budget", type=int, default=DEFAULT_SIGNAL_DAG_CANDIDATE_BUDGET)
    alpha_dag.add_argument("--reports-root", default="reports")
    alpha_dag.add_argument("--output-dir")
    alpha_dag.add_argument("--overwrite", action="store_true")
    alpha_dag.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    alpha_dag.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    alpha_dag.set_defaults(func=cmd_alpha_signal_dag_expansion_run)

    alpha_oos = sub.add_parser("alpha-signal-oos-fdr-validation-run")
    alpha_oos.add_argument(
        "--source-v2-artifact",
        default=str(DEFAULT_ALPHA_SIGNAL_DAG_V2_ARTIFACT),
        help="frozen V2 alpha Signal DAG artifact directory to validate without regeneration",
    )
    alpha_oos.add_argument("--symbol", default=CANONICAL_SYMBOL)
    alpha_oos.add_argument("--interval", default=CANONICAL_INTERVAL)
    alpha_oos.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; V3 validates frozen V2 candidates on chronological OOS folds",
    )
    alpha_oos.add_argument("--cache-dir", default="data/binance-vision")
    alpha_oos.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    alpha_oos.add_argument("--funding-cache-dir", default="data/binance-funding")
    alpha_oos.add_argument("--reports-root", default="reports")
    alpha_oos.add_argument("--output-dir")
    alpha_oos.add_argument("--overwrite", action="store_true")
    alpha_oos.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    alpha_oos.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    alpha_oos.set_defaults(func=cmd_alpha_signal_oos_fdr_validation_run)

    alpha_strict = sub.add_parser("alpha-signal-strict-falsification-run")
    alpha_strict.add_argument(
        "--source-v3-artifact",
        default=str(DEFAULT_ALPHA_SIGNAL_OOS_FDR_V3_ARTIFACT),
        help="frozen V3 OOS/FDR artifact directory whose next falsification queue is pre-registered and stress-tested without regeneration",
    )
    alpha_strict.add_argument("--symbol", default=CANONICAL_SYMBOL)
    alpha_strict.add_argument("--interval", default=CANONICAL_INTERVAL)
    alpha_strict.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months; V4 stress-tests frozen V3 queued hypotheses on stricter chronological OOS folds",
    )
    alpha_strict.add_argument("--cache-dir", default="data/binance-vision")
    alpha_strict.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    alpha_strict.add_argument("--funding-cache-dir", default="data/binance-funding")
    alpha_strict.add_argument("--reports-root", default="reports")
    alpha_strict.add_argument("--output-dir")
    alpha_strict.add_argument("--overwrite", action="store_true")
    alpha_strict.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    alpha_strict.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    alpha_strict.set_defaults(func=cmd_alpha_signal_strict_falsification_run)

    multi_asset_alpha = sub.add_parser("multi-asset-alpha-generator-strict-prefilter-run")
    multi_asset_alpha.add_argument(
        "--source-v4-artifact",
        default=str(DEFAULT_MULTI_ASSET_ALPHA_V4_ARTIFACT),
        help="audit-only V4 strict falsification artifact path; V5 records queue emptiness but never seeds candidates from it",
    )
    multi_asset_alpha.add_argument(
        "--symbols",
        default=",".join(DEFAULT_SYMBOL_UNIVERSE),
        help="comma-separated requested public symbol universe; normalized and recorded before exclusions",
    )
    multi_asset_alpha.add_argument("--interval", default=CANONICAL_INTERVAL)
    multi_asset_alpha.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months for V5 multi-asset panel diagnostics",
    )
    multi_asset_alpha.add_argument("--cache-dir", default="data/binance-vision")
    multi_asset_alpha.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    multi_asset_alpha.add_argument("--funding-cache-dir", default="data/binance-funding")
    multi_asset_alpha.add_argument(
        "--cache-mode",
        default="cache-only",
        choices=["cache-only", "download-missing"],
        help="cache-only records unavailable symbols instead of downloading missing public files; download-missing delegates to existing public loaders",
    )
    multi_asset_alpha.add_argument("--seed", type=int, default=DEFAULT_MULTI_ASSET_GENERATION_SEED)
    multi_asset_alpha.add_argument("--candidate-budget", type=int, default=DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET)
    multi_asset_alpha.add_argument("--max-depth", type=int, default=DEFAULT_MULTI_ASSET_MAX_DEPTH)
    multi_asset_alpha.add_argument("--min-bars-per-symbol", type=int, default=DEFAULT_MIN_BARS_PER_SYMBOL)
    multi_asset_alpha.add_argument("--reports-root", default="reports")
    multi_asset_alpha.add_argument("--output-dir")
    multi_asset_alpha.add_argument("--overwrite", action="store_true")
    multi_asset_alpha.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    multi_asset_alpha.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    multi_asset_alpha.set_defaults(func=cmd_multi_asset_alpha_generator_strict_prefilter_run)

    panel_ready_alpha = sub.add_parser("panel-ready-multi-asset-alpha-prefilter-rerun")
    panel_ready_alpha.add_argument(
        "--source-v7-artifact",
        default=str(DEFAULT_PANEL_READY_MULTI_ASSET_ALPHA_V7_ARTIFACT),
        help="V7 public data backfill artifact proving panel_ready_for_v5_rerun=true before the paper-only V5 rerun",
    )
    panel_ready_alpha.add_argument(
        "--source-v4-artifact",
        default=str(DEFAULT_MULTI_ASSET_ALPHA_V4_ARTIFACT),
        help="audit-only V4 strict falsification artifact path; V8 keeps V5 lineage semantics and does not revive V4 candidates",
    )
    panel_ready_alpha.add_argument(
        "--interval",
        default=None,
        help="optional interval assertion; omitted means use the V7-ready panel interval and any override must match it",
    )
    panel_ready_alpha.add_argument("--cache-dir", default="data/binance-vision")
    panel_ready_alpha.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    panel_ready_alpha.add_argument("--funding-cache-dir", default="data/binance-funding")
    panel_ready_alpha.add_argument("--seed", type=int, default=DEFAULT_MULTI_ASSET_GENERATION_SEED)
    panel_ready_alpha.add_argument("--candidate-budget", type=int, default=DEFAULT_MULTI_ASSET_CANDIDATE_BUDGET)
    panel_ready_alpha.add_argument("--max-depth", type=int, default=DEFAULT_MULTI_ASSET_MAX_DEPTH)
    panel_ready_alpha.add_argument("--min-bars-per-symbol", type=int, default=DEFAULT_MIN_BARS_PER_SYMBOL)
    panel_ready_alpha.add_argument("--reports-root", default="reports")
    panel_ready_alpha.add_argument("--output-dir")
    panel_ready_alpha.add_argument("--overwrite", action="store_true")
    panel_ready_alpha.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    panel_ready_alpha.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    panel_ready_alpha.set_defaults(func=cmd_panel_ready_multi_asset_alpha_prefilter_rerun)

    public_panel = sub.add_parser("public-multi-asset-panel-coverage-run")
    public_panel.add_argument(
        "--source-v5-artifact",
        default=str(DEFAULT_PUBLIC_MULTI_ASSET_PANEL_V5_ARTIFACT),
        help="V5 multi-asset alpha prefilter artifact path used only to preserve excluded symbol reasons",
    )
    public_panel.add_argument(
        "--symbols",
        default=",".join(PUBLIC_MULTI_ASSET_PANEL_DEFAULT_UNIVERSE),
        help="comma-separated requested public symbol universe; normalized and recorded before exclusions",
    )
    public_panel.add_argument("--interval", default=CANONICAL_INTERVAL)
    public_panel.add_argument(
        "--months",
        default=",".join(default_data_depth_months()),
        help="public Binance Vision months for V6 data coverage/provenance audit",
    )
    public_panel.add_argument("--cache-dir", default="data/binance-vision")
    public_panel.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    public_panel.add_argument("--funding-cache-dir", default="data/binance-funding")
    public_panel.add_argument(
        "--cache-mode",
        default="cache-only",
        choices=["cache-only", "download-missing"],
        help="cache-only records missing public files without downloading; download-missing delegates to existing public loaders",
    )
    public_panel.add_argument(
        "--minimum-required-symbols",
        type=int,
        default=DEFAULT_PUBLIC_MULTI_ASSET_PANEL_MINIMUM_REQUIRED_SYMBOLS,
    )
    public_panel.add_argument("--reports-root", default="reports")
    public_panel.add_argument("--output-dir")
    public_panel.add_argument("--overwrite", action="store_true")
    public_panel.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    public_panel.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    public_panel.set_defaults(func=cmd_public_multi_asset_panel_coverage_run)

    public_backfill = sub.add_parser("public-multi-asset-data-backfill-run")
    public_backfill.add_argument(
        "--source-v6-artifact",
        default=str(DEFAULT_PUBLIC_MULTI_ASSET_DATA_BACKFILL_V6_ARTIFACT),
        help="V6 public multi-asset panel coverage artifact used as immutable backfill input",
    )
    public_backfill.add_argument(
        "--symbols",
        default=None,
        help="optional comma-separated symbol universe override; omitted means derive from source V6 artifact",
    )
    public_backfill.add_argument(
        "--interval",
        default=None,
        help="optional interval override; omitted means derive from source V6 artifact",
    )
    public_backfill.add_argument(
        "--start-month",
        default=None,
        help="optional YYYY-MM target start override; omitted means derive from source V6 artifact",
    )
    public_backfill.add_argument(
        "--end-month",
        default=None,
        help="optional YYYY-MM target end override; omitted means derive from source V6 artifact",
    )
    public_backfill.add_argument("--cache-dir", default="data/binance-vision")
    public_backfill.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    public_backfill.add_argument("--funding-cache-dir", default="data/binance-funding")
    public_backfill.add_argument(
        "--minimum-required-symbols",
        type=int,
        default=DEFAULT_PUBLIC_MULTI_ASSET_PANEL_MINIMUM_REQUIRED_SYMBOLS,
    )
    public_backfill.add_argument(
        "--plan-only",
        action="store_true",
        help="write the deterministic public backfill plan without attempting downloads",
    )
    public_backfill.add_argument(
        "--force",
        action="store_true",
        help="mark existing cache files as download targets; existing loader resume semantics still preserve valid files",
    )
    public_backfill.add_argument("--reports-root", default="reports")
    public_backfill.add_argument("--output-dir")
    public_backfill.add_argument("--overwrite", action="store_true")
    public_backfill.add_argument(
        "--command",
        action="append",
        help="validation command to record in commands.txt; may be repeated",
    )
    public_backfill.add_argument(
        "--changed-file",
        action="append",
        help="changed file to record in changed_files.txt; may be repeated",
    )
    public_backfill.set_defaults(func=cmd_public_multi_asset_data_backfill_run)

    real = sub.add_parser("real-run")
    real.add_argument("--symbol", default=CANONICAL_SYMBOL, help="canonical validation only; non-BTCUSDT is rejected")
    real.add_argument("--interval", default=CANONICAL_INTERVAL, help="canonical validation only; non-1h is rejected")
    real.add_argument("--months", default=",".join(CANONICAL_MONTHS), help="canonical validation only; must be 2024-01..2024-06")
    real.add_argument("--cache-dir", default="data/binance-vision")
    real.add_argument("--reports-root", default="reports")
    real.add_argument("--output-dir")
    real.add_argument("--overwrite", action="store_true")
    real.set_defaults(func=cmd_real_run)

    risk = sub.add_parser("risk-run")
    risk.add_argument("--symbol", default=CANONICAL_SYMBOL, help="canonical validation only; non-BTCUSDT is rejected")
    risk.add_argument("--interval", default=CANONICAL_INTERVAL, help="canonical validation only; non-1h is rejected")
    risk.add_argument("--months", default=",".join(CANONICAL_MONTHS), help="canonical validation only; must be 2024-01..2024-06")
    risk.add_argument("--product-family", default=ProductFamily.SPOT_CRYPTO.value, choices=[item.value for item in ProductFamily])
    risk.add_argument("--cache-dir", default="data/binance-vision")
    risk.add_argument("--reports-root", default="reports")
    risk.add_argument("--output-dir")
    risk.add_argument("--overwrite", action="store_true")
    risk.set_defaults(func=cmd_risk_run)

    readiness = sub.add_parser("readiness-run")
    readiness.add_argument("--symbol", default=CANONICAL_SYMBOL, help="canonical validation only; non-BTCUSDT is rejected")
    readiness.add_argument("--interval", default=CANONICAL_INTERVAL, help="canonical validation only; non-1h is rejected")
    readiness.add_argument("--months", default=",".join(CANONICAL_MONTHS), help="canonical validation only; must be 2024-01..2024-06")
    readiness.add_argument("--product-family", default=ProductFamily.SPOT_CRYPTO.value, choices=[item.value for item in ProductFamily])
    readiness.add_argument("--cache-dir", default="data/binance-vision")
    readiness.add_argument("--funding-source", default="public-binance", choices=["public-binance", "sensitivity"])
    readiness.add_argument("--funding-cache-dir", default="data/binance-funding")
    readiness.add_argument("--reports-root", default="reports")
    readiness.add_argument("--output-dir")
    readiness.add_argument("--overwrite", action="store_true")
    readiness.set_defaults(func=cmd_readiness_run)

    forward = sub.add_parser("forward-paper-run")
    forward.add_argument("--symbol", default=CANONICAL_SYMBOL)
    forward.add_argument("--interval", default=CANONICAL_INTERVAL)
    forward.add_argument("--lookback-bars", type=int, default=720, help="saved output bars; default is 720 closed 1h bars")
    forward.add_argument("--initial-cash", type=float, default=10000.0)
    forward.add_argument("--fee-rate", type=float, default=0.0004)
    forward.add_argument("--slippage-bps", type=float, default=5.0)
    forward.add_argument("--entry-threshold", type=float, default=0.20)
    forward.add_argument("--exit-threshold", type=float, default=0.05)
    forward.add_argument("--max-exposure", type=float, default=1.0)
    forward.add_argument("--funding-source", default="public-binance", choices=["public-binance", "none"])
    forward.add_argument("--funding-cache-dir", default="data/binance-funding")
    forward.add_argument("--include-open-bar", action="store_true")
    forward.add_argument("--reports-root", default="reports")
    forward.add_argument("--output-dir")
    forward.add_argument("--overwrite", action="store_true")
    forward.set_defaults(func=cmd_forward_paper_run)

    kill = sub.add_parser("kill-switch")
    kill.add_argument("action", choices=["enable", "disable"])
    kill.set_defaults(func=cmd_kill_switch)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
