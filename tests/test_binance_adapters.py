from parangsae.core.contracts import OrderIntent, OrderSide, OrderType, ProductFamily, ProtectiveOrderIntent, Venue
from parangsae.execution.binance import BinanceFuturesBroker, BinanceSpotBroker, RecordingHttpClient


def test_spot_dry_run_uses_order_test_endpoint_only():
    calls = []
    broker = BinanceSpotBroker(RecordingHttpClient(calls))
    order = OrderIntent("spot-1", ProductFamily.SPOT_CRYPTO, Venue.BINANCE_SPOT, "BTCUSDT", OrderSide.BUY, OrderType.LIMIT, 1.0)
    broker.validate_dry_run(order)
    assert calls[0][1] == "/api/v3/order/test"
    assert "/api/v3/order" not in [path for _, path, _ in calls]


def test_futures_dry_run_uses_order_test_and_algo_for_protective():
    calls = []
    broker = BinanceFuturesBroker(RecordingHttpClient(calls))
    order = OrderIntent("fut-1", ProductFamily.CRYPTO_PERP, Venue.BINANCE_USDS_FUTURES, "BTCUSDT", OrderSide.BUY, OrderType.LIMIT, 1.0)
    broker.validate_dry_run(order)
    broker.validate_protective_dry_run(order, ProtectiveOrderIntent(kind="stop_loss", trigger_price=95.0))
    assert calls[0][1] == "/fapi/v1/order/test"
    assert calls[1][1] == "/fapi/v1/algoOrder"
