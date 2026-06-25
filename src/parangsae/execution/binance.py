from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol

from parangsae.core.contracts import OrderIntent, OrderType, ProductFamily, ProtectiveOrderIntent


class HttpClient(Protocol):
    def request(self, method: str, path: str, payload: dict) -> dict: ...


@dataclass
class RecordingHttpClient:
    calls: list[tuple[str, str, dict]]

    def request(self, method: str, path: str, payload: dict) -> dict:
        self.calls.append((method, path, payload))
        return {"ok": True, "path": path}


def spot_order_payload(order: OrderIntent) -> dict:
    payload = {
        "symbol": order.symbol,
        "side": order.side.value,
        "type": order.order_type.value,
        "quantity": str(order.quantity),
        "newClientOrderId": order.client_order_id,
    }
    if order.price is not None:
        payload["price"] = str(order.price)
        payload["timeInForce"] = "GTC"
    return payload


def futures_order_payload(order: OrderIntent) -> dict:
    payload = {
        "symbol": order.symbol,
        "side": order.side.value,
        "type": order.order_type.value,
        "quantity": str(order.quantity),
        "newClientOrderId": order.client_order_id,
    }
    if order.price is not None:
        payload["price"] = str(order.price)
        payload["timeInForce"] = "GTC"
    if order.reduce_only:
        payload["reduceOnly"] = "true"
    return payload


def futures_algo_payload(order: OrderIntent, protective: ProtectiveOrderIntent) -> dict:
    if protective.kind == "trailing_stop":
        order_type = OrderType.TRAILING_STOP_MARKET.value
        payload = {
            "algoType": "CONDITIONAL",
            "symbol": order.symbol,
            "side": "SELL" if order.side.value == "BUY" else "BUY",
            "type": order_type,
            "quantity": str(order.quantity * protective.close_fraction),
            "callbackRate": str(protective.callback_rate_pct or 1.0),
            "clientAlgoId": f"{order.client_order_id}-trail",
        }
        return payload
    order_type = OrderType.STOP_MARKET.value if protective.kind == "stop_loss" else OrderType.TAKE_PROFIT_MARKET.value
    return {
        "algoType": "CONDITIONAL",
        "symbol": order.symbol,
        "side": "SELL" if order.side.value == "BUY" else "BUY",
        "type": order_type,
        "quantity": str(order.quantity * protective.close_fraction),
        "triggerPrice": str(protective.trigger_price),
        "clientAlgoId": f"{order.client_order_id}-{protective.kind}",
    }


class BinanceSpotBroker:
    def __init__(self, client: HttpClient) -> None:
        self.client = client

    def validate_dry_run(self, order: OrderIntent) -> dict:
        if order.product_family is not ProductFamily.SPOT_CRYPTO:
            raise ValueError("Spot broker only accepts spot_crypto")
        return self.client.request("POST", "/api/v3/order/test", spot_order_payload(order))


class BinanceFuturesBroker:
    def __init__(self, client: HttpClient) -> None:
        self.client = client

    def validate_dry_run(self, order: OrderIntent) -> dict:
        if order.product_family not in {ProductFamily.CRYPTO_PERP, ProductFamily.TRADFI_PERP}:
            raise ValueError("Futures broker only accepts futures product families")
        return self.client.request("POST", "/fapi/v1/order/test", futures_order_payload(order))

    def validate_protective_dry_run(self, order: OrderIntent, protective: ProtectiveOrderIntent) -> dict:
        return self.client.request("POST", "/fapi/v1/algoOrder", futures_algo_payload(order, protective))
