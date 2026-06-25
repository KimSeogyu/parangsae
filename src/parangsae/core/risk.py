from __future__ import annotations

import os
from dataclasses import dataclass

from parangsae.core.config import LIVE_ACK_ENV, LIVE_ACK_VALUE, RiskConfig, kill_switch_active
from parangsae.core.contracts import OrderIntent, ProductCapability, RiskDecision, RiskDecisionStatus


@dataclass(frozen=True)
class RuntimeLiveContext:
    mode: str = "paper"
    live_enabled: bool = False
    live_allowed_product_families: tuple[str, ...] = ()
    open_orders_total: int = 0
    open_orders_for_symbol: int = 0
    daily_loss_pct: float = 0.0
    rolling_7d_drawdown_pct: float = 0.0
    portfolio_drawdown_pct: float = 0.0


class LiveTradingGuard:
    def __init__(self, risk_config: RiskConfig, product_policies: dict | None = None) -> None:
        self.risk_config = risk_config
        self.product_policies: dict = product_policies or {}

    def evaluate(
        self,
        order: OrderIntent,
        capability: ProductCapability,
        ctx: RuntimeLiveContext,
    ) -> RiskDecision:
        eligible, reason = capability.live_eligible()
        if not eligible:
            return RiskDecision(RiskDecisionStatus.REJECTED, reason)
        policy = self.product_policies.get(order.product_family)
        live_intent = ctx.mode == "live" or ctx.live_enabled or bool(ctx.live_allowed_product_families)
        if live_intent and policy is None:
            return RiskDecision(RiskDecisionStatus.REJECTED, "product_policy_missing")
        if policy is not None and not policy.live_allowed:
            return RiskDecision(RiskDecisionStatus.REJECTED, "product_family_live_disabled_by_config")
        product_risk = self.risk_config.product_families[order.product_family]
        portfolio = self.risk_config.portfolio
        if order.leverage > product_risk.max_leverage:
            return RiskDecision(RiskDecisionStatus.REJECTED, "leverage_above_balanced_default")
        if order.notional_pct > product_risk.max_order_notional_pct:
            return RiskDecision(RiskDecisionStatus.REJECTED, "order_notional_above_cap")
        if ctx.open_orders_total >= portfolio.max_open_orders_total:
            return RiskDecision(RiskDecisionStatus.REJECTED, "too_many_open_orders_total")
        if ctx.open_orders_for_symbol >= portfolio.max_open_orders_per_symbol:
            return RiskDecision(RiskDecisionStatus.REJECTED, "too_many_open_orders_for_symbol")
        if ctx.daily_loss_pct <= -portfolio.daily_loss_stop_pct:
            return RiskDecision(RiskDecisionStatus.REJECTED, "daily_loss_stop")
        if ctx.portfolio_drawdown_pct <= -portfolio.hard_drawdown_kill_switch_pct:
            return RiskDecision(RiskDecisionStatus.REJECTED, "hard_drawdown_kill_switch")
        if ctx.rolling_7d_drawdown_pct <= -portfolio.rolling_7d_drawdown_reduce_only_pct:
            return RiskDecision(RiskDecisionStatus.REDUCE_ONLY, "rolling_drawdown_reduce_only")
        if ctx.mode != "live":
            return RiskDecision(RiskDecisionStatus.REJECTED, "not_live_mode")
        if not ctx.live_enabled:
            return RiskDecision(RiskDecisionStatus.REJECTED, "live_config_disabled")
        if os.getenv(LIVE_ACK_ENV) != LIVE_ACK_VALUE:
            return RiskDecision(RiskDecisionStatus.REJECTED, "live_ack_missing_or_invalid")
        if not os.getenv("BINANCE_API_KEY") or not os.getenv("BINANCE_API_SECRET"):
            return RiskDecision(RiskDecisionStatus.REJECTED, "credentials_missing")
        if kill_switch_active(self.risk_config.kill_switch_path):
            return RiskDecision(RiskDecisionStatus.REJECTED, "kill_switch_active")
        if order.product_family.value not in ctx.live_allowed_product_families:
            return RiskDecision(RiskDecisionStatus.REJECTED, "product_family_not_live_allowed")
        return RiskDecision(RiskDecisionStatus.APPROVED, "approved")
