from __future__ import annotations

from typing import Any, Callable, Protocol, cast

from parangsae.core.contracts import OrderIntent, ProductCapability, RiskDecisionStatus
from parangsae.core.risk import LiveTradingGuard, RuntimeLiveContext


class CcxtSubmissionError(PermissionError):
    """Raised when a credentialed ccxt submit path is not explicitly allowed."""


ExchangeFactory = Callable[[dict[str, Any]], Any]


class ExchangeLike(Protocol):
    def set_sandbox_mode(self, enabled: bool) -> None: ...

    def load_markets(self) -> Any: ...

    def create_order(
        self, symbol: str, type: str, side: str, amount: float, price: float | None = None, params: dict[str, Any] | None = None
    ) -> Any: ...


def _default_binance_factory(config: dict[str, Any]) -> Any:
    import ccxt  # imported only inside credentialed helper surface, never by real-run

    ccxt_module = cast(Any, ccxt)
    return ccxt_module.binance(config)


def build_binance_futures_sandbox_client(
    *,
    api_key: str,
    secret: str,
    exchange_factory: ExchangeFactory | None = None,
    load_markets: bool = True,
) -> ExchangeLike:
    if not api_key or not secret:
        raise CcxtSubmissionError("credentials_missing")
    factory = exchange_factory or _default_binance_factory
    exchange: ExchangeLike = factory(
        {
            "apiKey": api_key,
            "secret": secret,
            "options": {"defaultType": "future"},
        }
    )
    exchange.set_sandbox_mode(True)
    if load_markets:
        exchange.load_markets()
    return exchange


def _ccxt_symbol(symbol: str) -> str:
    if symbol.endswith("USDT") and "/" not in symbol:
        return f"{symbol[:-4]}/USDT"
    return symbol


def submit_order_sandbox(
    *,
    order: OrderIntent,
    capability: ProductCapability,
    guard: LiveTradingGuard,
    ctx: RuntimeLiveContext,
    api_key: str | None = None,
    secret: str | None = None,
    allow_submit: bool = False,
    exchange_factory: ExchangeFactory | None = None,
) -> Any:
    if not allow_submit:
        raise CcxtSubmissionError("allow_submit_required")
    if not api_key or not secret:
        raise CcxtSubmissionError("credentials_missing")
    if not isinstance(guard, LiveTradingGuard):
        raise CcxtSubmissionError("real_live_trading_guard_required")
    decision = guard.evaluate(order, capability, ctx)
    if decision.status is not RiskDecisionStatus.APPROVED:
        raise CcxtSubmissionError(f"risk_guard_rejected:{decision.reason}")
    exchange = build_binance_futures_sandbox_client(
        api_key=api_key,
        secret=secret,
        exchange_factory=exchange_factory,
        load_markets=True,
    )
    return exchange.create_order(
        _ccxt_symbol(order.symbol),
        order.order_type.value.lower(),
        order.side.value.lower(),
        order.quantity,
        order.price,
        {"newClientOrderId": order.client_order_id, "reduceOnly": order.reduce_only},
    )
