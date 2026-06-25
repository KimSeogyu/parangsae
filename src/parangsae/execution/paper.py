from __future__ import annotations

from parangsae.core.capabilities import fixture_capability
from parangsae.core.config import RiskConfig
from parangsae.core.contracts import (
    ExecutionEvent,
    ExecutionEventType,
    OrderIntent,
    RiskDecisionStatus,
    SignalFrame,
)
from parangsae.core.events import EventLog
from parangsae.core.risk import LiveTradingGuard, RuntimeLiveContext
from parangsae.execution.portfolio import intents_from_target, target_from_signals


def run_paper_loop(signals: list[SignalFrame], risk: RiskConfig, event_log: EventLog) -> dict:
    for signal in signals:
        event_log.append(ExecutionEvent(ExecutionEventType.SIGNAL, signal.__dict__.copy()))
    target = target_from_signals(signals)
    event_log.append(ExecutionEvent(ExecutionEventType.TARGET, {"symbol_weights": target.symbol_weights}))
    guard = LiveTradingGuard(risk)
    filled = 0
    rejected = 0
    for order in intents_from_target(target):
        event_log.append(ExecutionEvent(ExecutionEventType.ORDER_INTENT, _order_payload(order)))
        decision = guard.evaluate(
            order,
            fixture_capability(order.symbol, order.product_family),
            RuntimeLiveContext(mode="paper"),
        )
        event_log.append(ExecutionEvent(ExecutionEventType.RISK_DECISION, decision.__dict__.copy()))
        if decision.status is RiskDecisionStatus.REJECTED and decision.reason == "not_live_mode":
            event_log.append(
                ExecutionEvent(
                    ExecutionEventType.PAPER_FILL,
                    {"symbol": order.symbol, "side": order.side.value, "quantity": order.quantity},
                )
            )
            filled += 1
        else:
            rejected += 1
    event_log.append(ExecutionEvent(ExecutionEventType.PNL, {"paper_fills": filled, "rejected": rejected}))
    return {"paper_fills": filled, "rejected": rejected, "target": target.symbol_weights}


def _order_payload(order: OrderIntent) -> dict:
    payload = order.__dict__.copy()
    payload["product_family"] = order.product_family.value
    payload["venue"] = order.venue.value
    payload["side"] = order.side.value
    payload["order_type"] = order.order_type.value
    payload["protective_orders"] = [p.__dict__ for p in order.protective_orders]
    return payload
