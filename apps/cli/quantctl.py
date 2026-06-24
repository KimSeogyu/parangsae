from __future__ import annotations

import argparse
import json
from pathlib import Path

import requests

from packages.core.capabilities import fixture_capability
from packages.core.config import load_product_policies, load_risk_config, load_strategy_policies
from packages.core.contracts import ProductFamily
from packages.core.events import EventLog, read_events, replay_positions
from packages.core.risk import LiveTradingGuard, RuntimeLiveContext
from packages.execution.binance import BinanceFuturesBroker, BinanceSpotBroker, RecordingHttpClient
from packages.execution.paper import run_paper_loop
from packages.marketdata.binance_vision import (
    CANONICAL_INTERVAL,
    CANONICAL_MONTHS,
    CANONICAL_SYMBOL,
    BinanceVisionError,
    DataValidationError,
    load_market_data_bundle,
    normalize_months,
    validate_interval,
    validate_symbol,
)
from packages.reporting.real_run_report import (
    build_manifest,
    prepare_output_dir,
    write_failure_report,
    write_risk_success_report,
    write_success_report,
)
from packages.research.backtest_engine import (
    MeaningGateError,
    assert_meaningful_run,
    broker_settings_dict,
    run_risk_candidate_search,
    run_strategy_backtest,
    split_chronological,
    strategy_settings_dict,
)
from packages.execution.portfolio import intents_from_target, target_from_signals
from packages.research.fixtures import FIXTURE_PRICES
from packages.research.strategies import crypto_residual_ridge_placeholder, trend_risk_signals

ROOT = Path(__file__).resolve().parents[2]
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
        write_success_report(
            output_dir=output_dir,
            manifest=manifest,
            meaning=meaning,
            backtest=backtest,
            holdout=holdout_result,
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
