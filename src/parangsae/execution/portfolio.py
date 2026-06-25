from __future__ import annotations

from parangsae.core.contracts import (
    OrderIntent,
    OrderSide,
    OrderType,
    ProductFamily,
    ProtectiveOrderIntent,
    SignalFrame,
    TargetPortfolio,
    Venue,
    utc_now_iso,
)


def target_from_signals(signals: list[SignalFrame]) -> TargetPortfolio:
    positive = [s for s in signals if s.score > 0]
    if not positive:
        family = signals[0].product_family if signals else ProductFamily.SPOT_CRYPTO
        return TargetPortfolio(family, {}, utc_now_iso())
    weight = min(1.0 / len(positive), 0.05)
    return TargetPortfolio(positive[0].product_family, {s.symbol: weight for s in positive}, utc_now_iso())


def intents_from_target(target: TargetPortfolio) -> list[OrderIntent]:
    intents: list[OrderIntent] = []
    venue = Venue.BINANCE_SPOT if target.product_family is ProductFamily.SPOT_CRYPTO else Venue.BINANCE_USDS_FUTURES
    for idx, (symbol, weight) in enumerate(sorted(target.symbol_weights.items())):
        intents.append(
            OrderIntent(
                client_order_id=f"pq-{target.product_family.value}-{idx}",
                product_family=target.product_family,
                venue=venue,
                symbol=symbol,
                side=OrderSide.BUY,
                order_type=OrderType.LIMIT,
                quantity=1.0,
                price=None,
                leverage=1.0,
                notional_pct=weight,
                protective_orders=(ProtectiveOrderIntent(kind="stop_loss", trigger_price=0.95),),
            )
        )
    return intents
