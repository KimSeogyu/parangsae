from __future__ import annotations

from parangsae.core.contracts import ProductFamily, SignalFrame, utc_now_iso


def trend_risk_signals(prices: dict[str, list[float]], family: ProductFamily) -> list[SignalFrame]:
    signals: list[SignalFrame] = []
    now = utc_now_iso()
    for symbol, series in sorted(prices.items()):
        if len(series) < 2:
            continue
        score = (series[-1] - series[0]) / series[0]
        signals.append(
            SignalFrame(
                strategy="trend-risk",
                product_family=family,
                symbol=symbol,
                timestamp=now,
                score=score,
                reason="fixture_trend_return",
            )
        )
    return signals


def crypto_residual_ridge_placeholder(prices: dict[str, list[float]]) -> list[SignalFrame]:
    # Milestone-1 compatible placeholder preserving the strategy seam. It consumes the same
    # SignalFrame contract and remains suitable for fixture/paper tests until full migration.
    return trend_risk_signals(prices, ProductFamily.CRYPTO_PERP)
