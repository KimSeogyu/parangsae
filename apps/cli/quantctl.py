from __future__ import annotations

import argparse
import json
from pathlib import Path

from packages.core.capabilities import fixture_capability
from packages.core.config import load_product_policies, load_risk_config, load_strategy_policies
from packages.core.contracts import ProductFamily
from packages.core.events import EventLog, read_events, replay_positions
from packages.core.risk import LiveTradingGuard, RuntimeLiveContext
from packages.execution.binance import BinanceFuturesBroker, BinanceSpotBroker, RecordingHttpClient
from packages.execution.paper import run_paper_loop
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
