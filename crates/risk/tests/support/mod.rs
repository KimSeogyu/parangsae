use parangsae_core::{
    Amount, AvailableTs, DecisionTime, EventTs, Observation, Price, Qty, ReceivedTs, Side, Symbol,
    Timestamps, UtcMicros,
};
use parangsae_risk::{
    InitialPortfolio, Limits, Mono, OrderId, OrderRequest, RiskContext, RiskGate,
};

pub const UNIT: i64 = 100_000_000;
pub const AT: i64 = 1_704_067_200_000_000;
pub const EQUITY: i64 = 100_000 * UNIT;
pub const MARK: i64 = 10_000 * UNIT;

pub fn qty(ticks: i64) -> Qty {
    Qty::from_ticks(ticks).unwrap()
}
pub fn amount(ticks: i64) -> Amount {
    Amount::from_ticks(ticks).unwrap()
}
pub fn symbol() -> Symbol {
    Symbol::new("BTCUSDT").unwrap()
}
pub fn limits() -> Limits {
    Limits::new(
        qty(UNIT),
        qty(UNIT),
        amount(20_000 * UNIT),
        amount(1_000 * UNIT),
        10,
        1_000_000,
        128,
    )
    .unwrap()
}
pub fn gate(position: i64) -> RiskGate {
    RiskGate::new(
        limits(),
        InitialPortfolio {
            symbol: symbol(),
            position_ticks: position,
            equity_ticks: EQUITY,
            valuation: Price::from_ticks(MARK).unwrap(),
        },
        Mono::from_micros(0),
    )
    .unwrap()
}
pub fn mark(price: i64) -> Observation<Price> {
    let at = UtcMicros::from_micros(AT);
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(at),
            available_ts: AvailableTs::from_micros(at),
            received_ts: ReceivedTs::from_micros(at),
        },
        Price::from_ticks(price).unwrap(),
    )
}
pub fn context(mark: &Observation<Price>, position: i64, now: u64) -> RiskContext<'_> {
    let decision = DecisionTime::from_micros(UtcMicros::from_micros(AT));
    RiskContext {
        now: Mono::from_micros(now),
        decision,
        mark: mark.causal_at(decision).unwrap(),
        position_ticks: position,
        equity_ticks: EQUITY,
    }
}
pub fn request(id: u128, side: Side, ticks: i64, reduce_only: bool) -> OrderRequest {
    OrderRequest::new(
        OrderId::new(id, 0).unwrap(),
        symbol(),
        side,
        qty(ticks),
        reduce_only,
    )
    .unwrap()
}
