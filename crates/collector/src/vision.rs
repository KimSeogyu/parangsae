//! Tier-1 Vision bulk datasets. No downloader in this scaffold.
//!
//! Filename whitelist, checksum, and header detection belong here when a
//! client exists. `part-00000` style names are not a member of this enum.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisionDataset {
    UmKline,
    MarkPriceKline,
    IndexPriceKline,
    PremiumIndexKline,
    FundingRate,
    Metrics,
    BookDepth,
    SpotKline,
}
