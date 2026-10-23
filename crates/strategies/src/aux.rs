//! Funding prints and premium minutes a family may read. The adapter
//! passes only rows whose `available_us` is at or before the decision.

/// One funding settlement. `rate_ticks` is the engine ratio, `10^8` scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundingPrint {
    pub funding_time_us: i64,
    pub available_us: i64,
    pub rate_ticks: i64,
}

/// One premium-index minute. `close_ticks` may be negative. `0` is a filler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PremiumMinute {
    pub open_us: i64,
    pub close_ticks: i64,
    pub count: i64,
    pub available_us: i64,
}
