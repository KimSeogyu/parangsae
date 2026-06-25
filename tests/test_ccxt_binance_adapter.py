from __future__ import annotations

import pytest
from typing import Any, cast

from parangsae.core.config import PortfolioRisk, ProductPolicy, ProductRisk, RiskConfig
from parangsae.core.contracts import OrderIntent, OrderSide, OrderType, ProductCapability, ProductFamily, RiskDecision, RiskDecisionStatus, Venue
from parangsae.execution.ccxt_binance import CcxtSubmissionError, build_binance_futures_sandbox_client, submit_order_sandbox
from parangsae.core.risk import LiveTradingGuard, RuntimeLiveContext


class FakeExchange:
    def __init__(self, events):
        self.events = events

    def set_sandbox_mode(self, enabled: bool):
        self.events.append(("sandbox", enabled))

    def load_markets(self):
        self.events.append(("load_markets", None))

    def create_order(self, *args, **kwargs):
        self.events.append(("create_order", args, kwargs))
        return {"ok": True}


class FakeGuard:
    def __init__(self, decision):
        self.decision = decision

    def evaluate(self, order, capability, ctx):
        return self.decision


def order() -> OrderIntent:
    return OrderIntent("cid", ProductFamily.CRYPTO_PERP, Venue.BINANCE_USDS_FUTURES, "BTCUSDT", OrderSide.BUY, OrderType.MARKET, 0.01, leverage=1.0, notional_pct=0.01)


def capability() -> ProductCapability:
    return ProductCapability(ProductFamily.CRYPTO_PERP, "BTCUSDT", Venue.BINANCE_USDS_FUTURES, True, True)



def real_guard(tmp_path) -> LiveTradingGuard:
    risk = RiskConfig(
        profile="balanced",
        kill_switch_path=tmp_path / "KILL_SWITCH",
        portfolio=PortfolioRisk(
            daily_loss_stop_pct=0.02,
            rolling_7d_drawdown_reduce_only_pct=0.05,
            hard_drawdown_kill_switch_pct=0.10,
            max_open_orders_total=10,
            max_open_orders_per_symbol=3,
            max_hourly_turnover_pct=0.10,
            max_daily_turnover_pct=0.50,
        ),
        product_families={
            ProductFamily.SPOT_CRYPTO: ProductRisk(1.0, 1.0, 1.0, 0.10, {}),
            ProductFamily.CRYPTO_PERP: ProductRisk(2.0, 1.0, 1.0, 0.10, {}),
            ProductFamily.TRADFI_PERP: ProductRisk(1.0, 1.0, 1.0, 0.10, {}),
        },
    )
    policies = {
        ProductFamily.CRYPTO_PERP: ProductPolicy(ProductFamily.CRYPTO_PERP, "BINANCE_USDS_FUTURES", True, True, True)
    }
    return LiveTradingGuard(risk, policies)

def test_build_sandbox_client_sets_sandbox_before_load_markets():
    events = []
    def factory(config):
        events.append(("factory", config))
        return FakeExchange(events)
    build_binance_futures_sandbox_client(api_key="key", secret="secret", exchange_factory=factory)
    assert events[0][0] == "factory"
    assert events[1] == ("sandbox", True)
    assert events[2] == ("load_markets", None)
    assert events[0][1]["options"]["defaultType"] == "future"


def test_submit_refuses_before_any_exchange_construction_without_allow_or_credentials():
    def factory(config):
        raise AssertionError("exchange must not be constructed")
    with pytest.raises(CcxtSubmissionError, match="allow_submit_required"):
        submit_order_sandbox(order=order(), capability=capability(), guard=cast(Any, FakeGuard(RiskDecision(RiskDecisionStatus.APPROVED, "approved"))), ctx=RuntimeLiveContext(mode="live", live_enabled=True), exchange_factory=factory)
    with pytest.raises(CcxtSubmissionError, match="credentials_missing"):
        submit_order_sandbox(order=order(), capability=capability(), guard=cast(Any, FakeGuard(RiskDecision(RiskDecisionStatus.APPROVED, "approved"))), ctx=RuntimeLiveContext(mode="live", live_enabled=True), allow_submit=True, exchange_factory=factory)


def test_submit_rejects_fake_guard_even_if_it_returns_approved():
    events = []
    with pytest.raises(CcxtSubmissionError, match="real_live_trading_guard_required"):
        submit_order_sandbox(order=order(), capability=capability(), guard=cast(Any, FakeGuard(RiskDecision(RiskDecisionStatus.APPROVED, "approved"))), ctx=RuntimeLiveContext(mode="live", live_enabled=True), api_key="key", secret="secret", allow_submit=True, exchange_factory=lambda config: FakeExchange(events))
    assert events == []


def test_submit_requires_real_guard_approval_before_create_order(tmp_path, monkeypatch):
    events = []
    guard = real_guard(tmp_path)
    monkeypatch.delenv("PARANGSAE_LIVE_TRADING_ACK", raising=False)
    with pytest.raises(CcxtSubmissionError, match="risk_guard_rejected"):
        submit_order_sandbox(order=order(), capability=capability(), guard=guard, ctx=RuntimeLiveContext(mode="live", live_enabled=True, live_allowed_product_families=("crypto_perp",)), api_key="key", secret="secret", allow_submit=True, exchange_factory=lambda config: FakeExchange(events))
    assert events == []


def test_submit_can_call_create_order_only_when_all_real_guard_gates_pass(tmp_path, monkeypatch):
    events = []
    guard = real_guard(tmp_path)
    monkeypatch.setenv("PARANGSAE_LIVE_TRADING_ACK", "I_UNDERSTAND_THIS_CAN_LOSE_MONEY")
    monkeypatch.setenv("BINANCE_API_KEY", "key")
    monkeypatch.setenv("BINANCE_API_SECRET", "secret")
    response = submit_order_sandbox(order=order(), capability=capability(), guard=guard, ctx=RuntimeLiveContext(mode="live", live_enabled=True, live_allowed_product_families=("crypto_perp",)), api_key="key", secret="secret", allow_submit=True, exchange_factory=lambda config: FakeExchange(events))
    assert response == {"ok": True}
    assert events[-1][0] == "create_order"
