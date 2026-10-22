//! Binance Vision bulk archives.
//!
//! Public `data.binance.vision` objects only. No API key, no REST market
//! endpoint, and no order URL. Filename whitelist, checksum, header detection,
//! and the parsers live in the submodules.

mod check;
mod checksum;
mod listxml;
mod name;
mod parse;
mod rows;

use crate::civil::CivilDay;
use crate::normalize::{spot_kline_clock, SourceClock, FUNDING_CLOCK, UM_KLINE_CLOCK};

pub use check::{
    book_day_gaps, book_row_faults, collapse_by, funding_window, kline_day_gaps, kline_row_faults,
    metrics_day_gaps, metrics_row_faults, missing_coverage, FaultReport, Gap, GapKind,
    BOOK_GAP_LIMIT_US,
};
pub use checksum::{parse_checksum, require_sha256, sha256_bytes};
pub use listxml::{list_page, ListPage, ListedObject};
pub use name::{basename, classify, ArchiveName, Cadence, Classified};
pub use parse::{parse_book, parse_funding, parse_klines, parse_metrics, unzip_csv};
pub use rows::{BookFact, FundingFact, KlineFact, MetricsFact, TickDomain};

/// Column-schema versions recorded on the manifest. Book level sets are a
/// separate per-timestamp version in [`BookFact::level_schema`].
pub const SCHEMA_KLINE_HEADERLESS: u32 = 1;
pub const SCHEMA_KLINE_HEADER: u32 = 2;
pub const SCHEMA_FUNDING: u32 = 3;
pub const SCHEMA_METRICS: u32 = 4;
pub const SCHEMA_BOOK: u32 = 5;

pub const DOWNLOAD_BASE: &str = "https://data.binance.vision";
pub const LIST_ENDPOINT: &str = "https://s3-ap-northeast-1.amazonaws.com/data.binance.vision";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

impl VisionDataset {
    pub const ALL: [Self; 8] = [
        Self::UmKline,
        Self::MarkPriceKline,
        Self::IndexPriceKline,
        Self::PremiumIndexKline,
        Self::FundingRate,
        Self::Metrics,
        Self::BookDepth,
        Self::SpotKline,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UmKline => "um-kline",
            Self::MarkPriceKline => "mark-price-kline",
            Self::IndexPriceKline => "index-price-kline",
            Self::PremiumIndexKline => "premium-index-kline",
            Self::FundingRate => "funding-rate",
            Self::Metrics => "metrics",
            Self::BookDepth => "book-depth",
            Self::SpotKline => "spot-kline",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "um-kline" | "klines" => Some(Self::UmKline),
            "mark-price-kline" | "markPriceKlines" => Some(Self::MarkPriceKline),
            "index-price-kline" | "indexPriceKlines" => Some(Self::IndexPriceKline),
            "premium-index-kline" | "premiumIndexKlines" => Some(Self::PremiumIndexKline),
            "funding-rate" | "fundingRate" => Some(Self::FundingRate),
            "metrics" => Some(Self::Metrics),
            "book-depth" | "bookDepth" => Some(Self::BookDepth),
            "spot-kline" => Some(Self::SpotKline),
            "all" => None,
            _ => None,
        }
    }

    pub const fn file_token(self) -> &'static str {
        match self {
            Self::UmKline
            | Self::MarkPriceKline
            | Self::IndexPriceKline
            | Self::PremiumIndexKline
            | Self::SpotKline => "1m",
            Self::FundingRate => "fundingRate",
            Self::Metrics => "metrics",
            Self::BookDepth => "bookDepth",
        }
    }

    pub const fn kline_interval_label(self) -> Option<&'static str> {
        match self {
            Self::UmKline
            | Self::MarkPriceKline
            | Self::IndexPriceKline
            | Self::PremiumIndexKline
            | Self::SpotKline => Some("1m"),
            Self::FundingRate | Self::Metrics | Self::BookDepth => None,
        }
    }

    /// Clock declared for an archive whose calendar day is `archive_day`.
    /// Monthly archives use the first day of the month. Metrics and book depth
    /// carry civil datetime strings, so they have no epoch unit.
    pub fn clock(self, archive_day: CivilDay) -> Option<SourceClock> {
        match self {
            Self::UmKline
            | Self::MarkPriceKline
            | Self::IndexPriceKline
            | Self::PremiumIndexKline => Some(UM_KLINE_CLOCK),
            Self::FundingRate => Some(FUNDING_CLOCK),
            Self::SpotKline => Some(spot_kline_clock(archive_day)),
            Self::Metrics | Self::BookDepth => None,
        }
    }

    pub fn prefixes(self, symbol: &str) -> Vec<(Cadence, String)> {
        let sym = symbol;
        match self {
            Self::UmKline => kline_prefixes("futures/um", "klines", sym),
            Self::MarkPriceKline => kline_prefixes("futures/um", "markPriceKlines", sym),
            Self::IndexPriceKline => kline_prefixes("futures/um", "indexPriceKlines", sym),
            Self::PremiumIndexKline => kline_prefixes("futures/um", "premiumIndexKlines", sym),
            Self::SpotKline => kline_prefixes("spot", "klines", sym),
            Self::FundingRate => vec![(
                Cadence::Monthly,
                format!("data/futures/um/monthly/fundingRate/{sym}/"),
            )],
            Self::Metrics => vec![(
                Cadence::Daily,
                format!("data/futures/um/daily/metrics/{sym}/"),
            )],
            Self::BookDepth => vec![(
                Cadence::Daily,
                format!("data/futures/um/daily/bookDepth/{sym}/"),
            )],
        }
    }

    pub const fn tick_domain(self) -> Option<TickDomain> {
        match self {
            Self::PremiumIndexKline => Some(TickDomain::Ratio),
            Self::UmKline | Self::MarkPriceKline | Self::IndexPriceKline | Self::SpotKline => {
                Some(TickDomain::Price)
            }
            Self::FundingRate | Self::Metrics | Self::BookDepth => None,
        }
    }

    pub fn allows_monthly(self) -> bool {
        match self {
            Self::UmKline
            | Self::MarkPriceKline
            | Self::IndexPriceKline
            | Self::PremiumIndexKline
            | Self::SpotKline
            | Self::FundingRate => true,
            Self::Metrics | Self::BookDepth => false,
        }
    }
}

fn kline_prefixes(market: &str, dataset: &str, symbol: &str) -> Vec<(Cadence, String)> {
    vec![
        (
            Cadence::Daily,
            format!("data/{market}/daily/{dataset}/{symbol}/1m/"),
        ),
        (
            Cadence::Monthly,
            format!("data/{market}/monthly/{dataset}/{symbol}/1m/"),
        ),
    ]
}

pub fn download_url(key: &str) -> String {
    format!("{DOWNLOAD_BASE}/{key}")
}

pub fn list_url(prefix: &str, marker: &str) -> Result<String, crate::Error> {
    let mut url = url::Url::parse(&format!("{LIST_ENDPOINT}/"))
        .map_err(|err| crate::Error::check("listing", format!("list endpoint: {err}")))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("prefix", prefix);
        query.append_pair("max-keys", "1000");
        if !marker.is_empty() {
            query.append_pair("marker", marker);
        }
    }
    Ok(url.to_string())
}

pub fn archive_clock_day(name: &ArchiveName) -> CivilDay {
    match name.day {
        Some(day) => day,
        None => name.month.first(),
    }
}

impl std::fmt::Display for VisionDataset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
