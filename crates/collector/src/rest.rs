//! Public market REST for gaps, the current funding month, and snapshots.
//!
//! No API key and no signed endpoint. Demo-trading hosts are not a market-data source.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicRestDataset {
    Klines,
    FundingRate,
    FundingInfo,
    PremiumIndex,
    OpenInterest,
    ExchangeInfo,
}
