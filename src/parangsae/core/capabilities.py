from __future__ import annotations

from parangsae.core.contracts import ProductCapability, ProductFamily, Venue


def fixture_capability(symbol: str, family: ProductFamily) -> ProductCapability:
    if family is ProductFamily.SPOT_CRYPTO:
        return ProductCapability(family, symbol, Venue.BINANCE_SPOT, True, False)
    if family is ProductFamily.CRYPTO_PERP:
        return ProductCapability(family, symbol, Venue.BINANCE_USDS_FUTURES, True, True)
    return ProductCapability(
        family,
        symbol,
        Venue.BINANCE_USDS_FUTURES,
        True,
        True,
        schedule_known=False,
        agreement_known=False,
        agreement_signed=False,
        constituent_prices_hidden=True,
    )
