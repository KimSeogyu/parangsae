//! Instrument-rule types. One row is one sourced value over one validity span.

use super::clock::{UtcDate, UtcDateTime};
use super::decimal::Decimal;
use std::fmt;

/// Exchange constant looked up by `(symbol, field, at)`.
///
/// `MinPrice`, `MaxPrice`, and the market lot step are not variants: the seed
/// CSV does not carry them. Price bounds are not invented from other fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Field {
    ContractType,
    OnboardDateMs,
    TickSize,
    StepSize,
    MinQty,
    LotMaxQty,
    MarketMaxQty,
    MinNotional,
    PricePrecision,
    QuantityPrecision,
    FundingIntervalHours,
    FeeMakerVip0,
    FeeTakerVip0,
    FeeBnbDiscount,
}

impl Field {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContractType => "contract_type",
            Self::OnboardDateMs => "onboard_date_ms",
            Self::TickSize => "tick_size",
            Self::StepSize => "step_size",
            Self::MinQty => "min_qty",
            Self::LotMaxQty => "lot_max_qty",
            Self::MarketMaxQty => "market_max_qty",
            Self::MinNotional => "min_notional",
            Self::PricePrecision => "price_precision",
            Self::QuantityPrecision => "quantity_precision",
            Self::FundingIntervalHours => "funding_interval_hours",
            Self::FeeMakerVip0 => "fee_maker_vip0",
            Self::FeeTakerVip0 => "fee_taker_vip0",
            Self::FeeBnbDiscount => "fee_bnb_discount",
        }
    }

    /// Fields the fill model consumes. An unverified value blocks paper use
    /// unless it is on the paper-block list.
    pub const fn is_fill(self) -> bool {
        matches!(
            self,
            Self::TickSize
                | Self::StepSize
                | Self::MinQty
                | Self::MinNotional
                | Self::FeeMakerVip0
                | Self::FeeTakerVip0
        )
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Confidence {
    VerifiedArchive,
    VerifiedDoc,
    CorroboratedFromData,
    Unverified,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VerifiedArchive => write!(f, "verified-archive"),
            Self::VerifiedDoc => write!(f, "verified-doc"),
            Self::CorroboratedFromData => write!(f, "corroborated-from-data"),
            Self::Unverified => write!(f, "unverified"),
        }
    }
}

/// When a bound happened.
///
/// `Bracketed` means the bound instant lies in `(earliest, latest]`.
/// A lookup strictly between the endpoints is unresolved until a policy
/// picks a row. The endpoints themselves are observations and are certain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Boundary {
    Exact(UtcDateTime),
    Bracketed {
        earliest: UtcDateTime,
        latest: UtcDateTime,
    },
}

impl Boundary {
    pub const fn earliest(self) -> UtcDateTime {
        match self {
            Self::Exact(at) => at,
            Self::Bracketed { earliest, .. } => earliest,
        }
    }

    pub const fn latest(self) -> UtcDateTime {
        match self {
            Self::Exact(at) => at,
            Self::Bracketed { latest, .. } => latest,
        }
    }

    pub const fn is_well_ordered(self) -> bool {
        match self {
            Self::Exact(_) => true,
            Self::Bracketed { earliest, latest } => {
                earliest.micros().micros() < latest.micros().micros()
            }
        }
    }
}

/// Stored constant. `Absent` is a filter that did not exist (min notional
/// before 2021-01-25). Non-decimal seed rows keep their own variants so a
/// contract type is not pretend-decimal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleValue {
    Value(Decimal),
    Absent,
    Token(&'static str),
    EpochMillis(i64),
}

impl RuleValue {
    pub const fn decimal(ticks: i64) -> Self {
        Self::Value(Decimal::from_ticks(ticks))
    }
}

/// One append-only row. A correction is a new `revision`, not an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InstrumentRule {
    pub symbol: &'static str,
    pub venue: &'static str,
    pub field: Field,
    pub value: RuleValue,
    pub valid_from: Boundary,
    pub valid_to: Option<Boundary>,
    pub source: &'static str,
    pub source_date: UtcDate,
    pub retrieved: UtcDate,
    pub confidence: Confidence,
    pub revision: u16,
}

/// How a lookup resolves an instant inside `(earliest, latest)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum BracketPolicy {
    /// Higher fee, smaller max qty, larger min notional, coarser tick/step.
    /// A single candidate is returned: there is no second value to raise.
    Conservative,
    /// The bound is the earliest possible instant, so the later row applies.
    Earliest,
    /// The bound is the latest possible instant, so the earlier row applies.
    /// A leading bracket with only one row has not started yet.
    Latest,
    /// Error. Tests use this to list what is still unpinned.
    Reject,
}

/// A fill field whose confidence is not good enough for paper.
/// The reason is part of the allow-list: an empty reason does not block, and
/// an unlisted unverified fill row fails provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaperBlock {
    pub symbol: &'static str,
    pub field: Field,
    pub reason: &'static str,
}

/// First certain instant of a field is after the backtest start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CoverageGap {
    pub field: Field,
    pub known_from: UtcDateTime,
    pub reason: &'static str,
}

/// Section (d) item that this tree cannot close.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnresolvedItem {
    pub id: &'static str,
    pub summary: &'static str,
}
