//! One parsed Vision row. Payload equality ignores `receipt`.
//!
//! `zero_volume_flat` is descriptive: OHLC is flat and volume is 0. It is not a
//! halt flag and not a maintenance flag.

use parangsae_core::{Amount, AvailableTs, EventTs, Qty, Ratio, Receipt, Symbol, UtcMicros};

/// `volume == 0`, `count == 0`, and open, high, low, and close are equal.
///
/// Descriptive only. Mark, index, and premium rows do not use this; their
/// volume columns are always 0.
pub fn zero_volume_flat(
    volume_ticks: i64,
    count: i64,
    open_ticks: i64,
    high_ticks: i64,
    low_ticks: i64,
    close_ticks: i64,
) -> bool {
    volume_ticks == 0
        && count == 0
        && open_ticks == high_ticks
        && high_ticks == low_ticks
        && low_ticks == close_ticks
}

/// OHLC domain. Premium bars are ratios. Trade, mark, index, and spot bars are prices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickDomain {
    Price,
    Ratio,
}

impl TickDomain {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Price => "price",
            Self::Ratio => "ratio",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "price" => Some(Self::Price),
            "ratio" => Some(Self::Ratio),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct KlineFact {
    pub symbol: Symbol,
    pub interval_us: i64,
    pub open_time: UtcMicros,
    pub close_time: UtcMicros,
    pub open_ticks: i64,
    pub high_ticks: i64,
    pub low_ticks: i64,
    pub close_ticks: i64,
    pub volume: Qty,
    pub quote_volume: Amount,
    pub count: i64,
    pub taker_buy_volume: Qty,
    pub taker_buy_quote_volume: Amount,
    pub event_ts: EventTs,
    pub available_ts: AvailableTs,
    pub receipt: Receipt,
    pub schema_version: u32,
    pub tick_domain: TickDomain,
    /// Set from [`zero_volume_flat`] for um and spot klines. Always `false` for
    /// mark, index, and premium, and not part of their content hash.
    pub zero_volume_flat: bool,
}

impl PartialEq for KlineFact {
    fn eq(&self, other: &Self) -> bool {
        self.symbol == other.symbol
            && self.interval_us == other.interval_us
            && self.open_time == other.open_time
            && self.close_time == other.close_time
            && self.open_ticks == other.open_ticks
            && self.high_ticks == other.high_ticks
            && self.low_ticks == other.low_ticks
            && self.close_ticks == other.close_ticks
            && self.volume == other.volume
            && self.quote_volume == other.quote_volume
            && self.count == other.count
            && self.taker_buy_volume == other.taker_buy_volume
            && self.taker_buy_quote_volume == other.taker_buy_quote_volume
            && self.event_ts == other.event_ts
            && self.available_ts == other.available_ts
            && self.schema_version == other.schema_version
            && self.tick_domain == other.tick_domain
            && self.zero_volume_flat == other.zero_volume_flat
    }
}

impl Eq for KlineFact {}

#[derive(Clone, Debug)]
pub struct FundingFact {
    pub symbol: Symbol,
    pub raw_calc_time: UtcMicros,
    pub funding_time: UtcMicros,
    pub interval_hours: i64,
    pub rate: Ratio,
    pub event_ts: EventTs,
    pub available_ts: AvailableTs,
    pub receipt: Receipt,
    pub schema_version: u32,
}

impl PartialEq for FundingFact {
    fn eq(&self, other: &Self) -> bool {
        self.symbol == other.symbol
            && self.raw_calc_time == other.raw_calc_time
            && self.funding_time == other.funding_time
            && self.interval_hours == other.interval_hours
            && self.rate == other.rate
            && self.event_ts == other.event_ts
            && self.available_ts == other.available_ts
            && self.schema_version == other.schema_version
    }
}

impl Eq for FundingFact {}

#[derive(Clone, Debug)]
pub struct MetricsFact {
    pub symbol: Symbol,
    pub create_time: UtcMicros,
    pub sum_open_interest: Qty,
    pub sum_open_interest_value: Amount,
    pub count_toptrader_long_short_ratio: Ratio,
    pub sum_toptrader_long_short_ratio: Ratio,
    pub count_long_short_ratio: Ratio,
    pub sum_taker_long_short_vol_ratio: Ratio,
    pub event_ts: EventTs,
    pub available_ts: AvailableTs,
    pub receipt: Receipt,
    pub available_lag_verified: bool,
    pub schema_version: u32,
}

impl PartialEq for MetricsFact {
    fn eq(&self, other: &Self) -> bool {
        self.symbol == other.symbol
            && self.create_time == other.create_time
            && self.sum_open_interest == other.sum_open_interest
            && self.sum_open_interest_value == other.sum_open_interest_value
            && self.count_toptrader_long_short_ratio == other.count_toptrader_long_short_ratio
            && self.sum_toptrader_long_short_ratio == other.sum_toptrader_long_short_ratio
            && self.count_long_short_ratio == other.count_long_short_ratio
            && self.sum_taker_long_short_vol_ratio == other.sum_taker_long_short_vol_ratio
            && self.event_ts == other.event_ts
            && self.available_ts == other.available_ts
            && self.available_lag_verified == other.available_lag_verified
            && self.schema_version == other.schema_version
    }
}

impl Eq for MetricsFact {}

#[derive(Clone, Debug)]
pub struct BookFact {
    pub symbol: Symbol,
    pub timestamp: UtcMicros,
    pub percentage: Ratio,
    pub depth: Qty,
    pub notional: Amount,
    pub level_schema: u32,
    pub event_ts: EventTs,
    pub available_ts: AvailableTs,
    pub receipt: Receipt,
    pub schema_version: u32,
}

impl PartialEq for BookFact {
    fn eq(&self, other: &Self) -> bool {
        self.symbol == other.symbol
            && self.timestamp == other.timestamp
            && self.percentage == other.percentage
            && self.depth == other.depth
            && self.notional == other.notional
            && self.level_schema == other.level_schema
            && self.event_ts == other.event_ts
            && self.available_ts == other.available_ts
            && self.schema_version == other.schema_version
    }
}

impl Eq for BookFact {}
