from __future__ import annotations

from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from enum import StrEnum
from typing import Any, Literal
import json

SCHEMA_VERSION = 1


class ProductFamily(StrEnum):
    SPOT_CRYPTO = "spot_crypto"
    CRYPTO_PERP = "crypto_perp"
    TRADFI_PERP = "tradfi_perp"


class Venue(StrEnum):
    BINANCE_SPOT = "BINANCE_SPOT"
    BINANCE_USDS_FUTURES = "BINANCE_USDS_FUTURES"


class OrderSide(StrEnum):
    BUY = "BUY"
    SELL = "SELL"


class OrderType(StrEnum):
    MARKET = "MARKET"
    LIMIT = "LIMIT"
    STOP_LOSS = "STOP_LOSS"
    STOP_LOSS_LIMIT = "STOP_LOSS_LIMIT"
    TAKE_PROFIT = "TAKE_PROFIT"
    TAKE_PROFIT_LIMIT = "TAKE_PROFIT_LIMIT"
    STOP_MARKET = "STOP_MARKET"
    TAKE_PROFIT_MARKET = "TAKE_PROFIT_MARKET"
    TRAILING_STOP_MARKET = "TRAILING_STOP_MARKET"


class RiskDecisionStatus(StrEnum):
    APPROVED = "APPROVED"
    REJECTED = "REJECTED"
    REDUCE_ONLY = "REDUCE_ONLY"


class ExecutionEventType(StrEnum):
    SIGNAL = "SignalGenerated"
    TARGET = "TargetPortfolioBuilt"
    ORDER_INTENT = "OrderIntentCreated"
    RISK_DECISION = "RiskDecisionRecorded"
    PAPER_FILL = "PaperFillRecorded"
    BROKER_DRY_RUN = "BrokerDryRunValidated"
    CAPABILITY_REJECTED = "CapabilityRejected"
    KILL_SWITCH = "KillSwitchStateChanged"
    PNL = "PnlSnapshot"


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def _normalize(value: Any) -> Any:
    if isinstance(value, StrEnum):
        return value.value
    if isinstance(value, list):
        return [_normalize(v) for v in value]
    if isinstance(value, dict):
        return {k: _normalize(v) for k, v in value.items()}
    return value


def canonical_json(data: Any) -> str:
    return json.dumps(_normalize(data), sort_keys=True, separators=(",", ":"), ensure_ascii=False)


@dataclass(frozen=True)
class ProductCapability:
    product_family: ProductFamily
    symbol: str
    venue: Venue
    tradable: bool
    short_allowed: bool
    schedule_known: bool = True
    agreement_known: bool = True
    agreement_signed: bool = False
    constituent_prices_hidden: bool = False
    snapshot_age_seconds: int = 0
    max_snapshot_age_seconds: int = 3600

    def is_fresh(self) -> bool:
        return self.snapshot_age_seconds <= self.max_snapshot_age_seconds

    def live_eligible(self) -> tuple[bool, str]:
        if not self.tradable:
            return False, "symbol_not_tradable"
        if not self.is_fresh():
            return False, "capability_snapshot_stale"
        if self.product_family is ProductFamily.TRADFI_PERP:
            if not self.schedule_known:
                return False, "tradfi_schedule_unknown"
            if not self.agreement_known or not self.agreement_signed:
                return False, "tradfi_agreement_not_confirmed"
            if self.constituent_prices_hidden:
                return False, "tradfi_constituent_prices_hidden"
        return True, "eligible"


SymbolCapability = ProductCapability


@dataclass(frozen=True)
class SignalFrame:
    strategy: str
    product_family: ProductFamily
    symbol: str
    timestamp: str
    score: float
    reason: str


@dataclass(frozen=True)
class TargetPortfolio:
    product_family: ProductFamily
    symbol_weights: dict[str, float]
    generated_at: str


@dataclass(frozen=True)
class ProtectiveOrderIntent:
    kind: Literal["stop_loss", "take_profit", "trailing_stop", "time_stop"]
    trigger_price: float | None = None
    limit_price: float | None = None
    trailing_delta_bips: int | None = None
    callback_rate_pct: float | None = None
    close_fraction: float = 1.0


@dataclass(frozen=True)
class OrderIntent:
    client_order_id: str
    product_family: ProductFamily
    venue: Venue
    symbol: str
    side: OrderSide
    order_type: OrderType
    quantity: float
    price: float | None = None
    leverage: float = 1.0
    notional_pct: float = 0.0
    reduce_only: bool = False
    protective_orders: tuple[ProtectiveOrderIntent, ...] = ()


@dataclass(frozen=True)
class RiskDecision:
    status: RiskDecisionStatus
    reason: str
    checked_at: str = field(default_factory=utc_now_iso)


RiskApproval = RiskDecision


@dataclass(frozen=True)
class ExecutionPlan:
    order: OrderIntent
    risk: RiskDecision
    mode: Literal["paper", "dry_run", "live"]


@dataclass(frozen=True)
class CapabilityRejected:
    symbol: str
    product_family: ProductFamily
    reason: str


@dataclass(frozen=True)
class ExecutionEvent:
    event_type: ExecutionEventType
    payload: dict[str, Any]


@dataclass(frozen=True)
class EventEnvelope:
    event_id: str
    sequence: int
    timestamp: str
    schema_version: int
    event: ExecutionEvent

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    def to_json(self) -> str:
        return canonical_json(self.to_dict())


def envelope(sequence: int, event: ExecutionEvent, *, event_id: str | None = None) -> EventEnvelope:
    ts = utc_now_iso()
    return EventEnvelope(
        event_id=event_id or f"evt-{sequence:08d}",
        sequence=sequence,
        timestamp=ts,
        schema_version=SCHEMA_VERSION,
        event=event,
    )
