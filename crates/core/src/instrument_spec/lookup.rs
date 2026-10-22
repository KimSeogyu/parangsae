//! Point lookup. A bracket picks a row only through [`BracketPolicy`].

use super::clock::UtcDateTime;
use super::decimal::Decimal;
use super::table::BTCUSDT_RULES;
use super::types::{Boundary, BracketPolicy, Field, InstrumentRule, RuleValue};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupError {
    UnknownSymbol,
    NoCoverage { field: Field },
    Unresolved { field: Field },
    Overlap { field: Field },
    NoConservativeOrder { field: Field },
    TableFault,
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSymbol => write!(f, "unknown symbol"),
            Self::NoCoverage { field } => write!(f, "no {field} rule covers the instant"),
            Self::Unresolved { field } => write!(f, "{field} is inside an unresolved bracket"),
            Self::Overlap { field } => write!(f, "{field} rules overlap"),
            Self::NoConservativeOrder { field } => {
                write!(f, "{field} has no conservative order")
            }
            Self::TableFault => write!(f, "instrument table is not well formed"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Before,
    Inside,
    AfterOrOn,
}

fn region(boundary: Boundary, at: UtcDateTime) -> Result<Region, LookupError> {
    match boundary {
        Boundary::Exact(instant) => {
            if at < instant {
                Ok(Region::Before)
            } else {
                Ok(Region::AfterOrOn)
            }
        }
        Boundary::Bracketed { earliest, latest } => {
            if earliest >= latest {
                return Err(LookupError::TableFault);
            }
            if at <= earliest {
                Ok(Region::Before)
            } else if at >= latest {
                Ok(Region::AfterOrOn)
            } else {
                Ok(Region::Inside)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cover {
    No,
    Yes,
    Maybe,
}

fn cover(rule: &InstrumentRule, at: UtcDateTime) -> Result<Cover, LookupError> {
    let started = match region(rule.valid_from, at)? {
        Region::Before => Cover::No,
        Region::AfterOrOn => Cover::Yes,
        Region::Inside => Cover::Maybe,
    };
    let ended = match rule.valid_to {
        None => Cover::No,
        Some(end) => match region(end, at)? {
            Region::Before => Cover::No,
            Region::AfterOrOn => Cover::Yes,
            Region::Inside => Cover::Maybe,
        },
    };
    Ok(match (started, ended) {
        (Cover::No, _) | (_, Cover::Yes) => Cover::No,
        (Cover::Yes, Cover::No) => Cover::Yes,
        (Cover::Yes, Cover::Maybe) | (Cover::Maybe, Cover::No) | (Cover::Maybe, Cover::Maybe) => {
            Cover::Maybe
        }
    })
}

fn starts_inside(rule: &InstrumentRule, at: UtcDateTime) -> Result<bool, LookupError> {
    Ok(region(rule.valid_from, at)? == Region::Inside)
}

fn ends_inside(rule: &InstrumentRule, at: UtcDateTime) -> Result<bool, LookupError> {
    match rule.valid_to {
        Some(end) => Ok(region(end, at)? == Region::Inside),
        None => Ok(false),
    }
}

fn push(slots: &mut [usize], len: &mut usize, index: usize) -> Result<(), LookupError> {
    let slot = slots.get_mut(*len).ok_or(LookupError::TableFault)?;
    *slot = index;
    *len = len.saturating_add(1);
    Ok(())
}

fn rule_at_index(rules: &[InstrumentRule], index: usize) -> Result<&InstrumentRule, LookupError> {
    rules.get(index).ok_or(LookupError::TableFault)
}

fn decimal_value(rule: &InstrumentRule) -> Result<Decimal, LookupError> {
    match rule.value {
        RuleValue::Value(decimal) => Ok(decimal),
        RuleValue::Absent | RuleValue::Token(_) | RuleValue::EpochMillis(_) => {
            Err(LookupError::NoConservativeOrder { field: rule.field })
        }
    }
}

fn notional_rank(rule: &InstrumentRule) -> Result<Decimal, LookupError> {
    match rule.value {
        RuleValue::Absent => Ok(Decimal::ZERO),
        RuleValue::Value(decimal) => Ok(decimal),
        RuleValue::Token(_) | RuleValue::EpochMillis(_) => {
            Err(LookupError::NoConservativeOrder { field: rule.field })
        }
    }
}

/// `true` keeps `left`. Equal values keep `left`.
fn keep_left(left: Decimal, right: Decimal, higher_wins: bool) -> bool {
    if left == right {
        true
    } else if higher_wins {
        left > right
    } else {
        left < right
    }
}

fn prefer<'a>(
    field: Field,
    left: &'a InstrumentRule,
    right: &'a InstrumentRule,
) -> Result<&'a InstrumentRule, LookupError> {
    let higher = match field {
        Field::FeeMakerVip0 | Field::FeeTakerVip0 => {
            keep_left(decimal_value(left)?, decimal_value(right)?, true)
        }
        // A smaller BNB discount leaves a higher fee.
        Field::FeeBnbDiscount => keep_left(decimal_value(left)?, decimal_value(right)?, false),
        Field::MinNotional => keep_left(notional_rank(left)?, notional_rank(right)?, true),
        Field::MarketMaxQty | Field::LotMaxQty => {
            keep_left(decimal_value(left)?, decimal_value(right)?, false)
        }
        Field::TickSize | Field::StepSize | Field::MinQty => {
            keep_left(decimal_value(left)?, decimal_value(right)?, true)
        }
        Field::ContractType
        | Field::OnboardDateMs
        | Field::PricePrecision
        | Field::QuantityPrecision
        | Field::FundingIntervalHours => {
            if left.value == right.value {
                true
            } else {
                return Err(LookupError::NoConservativeOrder { field });
            }
        }
    };
    Ok(if higher { left } else { right })
}

fn fold_conservative<'a>(
    rules: &'a [InstrumentRule],
    field: Field,
    indexes: &[usize],
) -> Result<&'a InstrumentRule, LookupError> {
    let mut best: Option<&InstrumentRule> = None;
    for index in indexes {
        let rule = rule_at_index(rules, *index)?;
        best = Some(match best {
            None => rule,
            Some(current) => prefer(field, current, rule)?,
        });
    }
    best.ok_or(LookupError::NoCoverage { field })
}

fn pick_side<'a>(
    rules: &'a [InstrumentRule],
    indexes: &[usize],
    at: UtcDateTime,
    want_start: bool,
) -> Result<Option<&'a InstrumentRule>, LookupError> {
    let mut best: Option<&InstrumentRule> = None;
    for index in indexes {
        let rule = rule_at_index(rules, *index)?;
        let matches = if want_start {
            starts_inside(rule, at)?
        } else {
            ends_inside(rule, at)?
        };
        if !matches {
            continue;
        }
        best = Some(match best {
            None => rule,
            Some(current) => {
                let candidate_key = rule.valid_from.earliest();
                let current_key = current.valid_from.earliest();
                if want_start {
                    if candidate_key >= current_key {
                        rule
                    } else {
                        current
                    }
                } else if candidate_key <= current_key {
                    rule
                } else {
                    current
                }
            }
        });
    }
    Ok(best)
}

fn resolve_maybe<'a>(
    rules: &'a [InstrumentRule],
    field: Field,
    at: UtcDateTime,
    policy: BracketPolicy,
    maybe: &[usize],
) -> Result<&'a InstrumentRule, LookupError> {
    match policy {
        BracketPolicy::Reject => Err(LookupError::Unresolved { field }),
        BracketPolicy::Conservative => fold_conservative(rules, field, maybe),
        BracketPolicy::Earliest => {
            pick_side(rules, maybe, at, true)?.ok_or(LookupError::NoCoverage { field })
        }
        BracketPolicy::Latest => {
            pick_side(rules, maybe, at, false)?.ok_or(LookupError::NoCoverage { field })
        }
    }
}

/// Lookup `field` for `symbol` at `at` in `rules`.
pub fn lookup<'a>(
    rules: &'a [InstrumentRule],
    symbol: &str,
    field: Field,
    at: UtcDateTime,
    policy: BracketPolicy,
) -> Result<&'a InstrumentRule, LookupError> {
    const MAX: usize = 8;
    let mut certain = [0usize; MAX];
    let mut certain_len = 0usize;
    let mut maybe = [0usize; MAX];
    let mut maybe_len = 0usize;
    let mut saw_symbol = false;
    for (index, rule) in rules.iter().enumerate() {
        if rule.symbol != symbol {
            continue;
        }
        saw_symbol = true;
        if rule.field != field {
            continue;
        }
        match cover(rule, at)? {
            Cover::No => {}
            Cover::Yes => push(&mut certain, &mut certain_len, index)?,
            Cover::Maybe => push(&mut maybe, &mut maybe_len, index)?,
        }
    }
    if !saw_symbol {
        return Err(LookupError::UnknownSymbol);
    }
    if certain_len > 1 || (certain_len == 1 && maybe_len > 0) {
        return Err(LookupError::Overlap { field });
    }
    if certain_len == 1 && maybe_len == 0 {
        let index = certain.first().copied().ok_or(LookupError::TableFault)?;
        return rule_at_index(rules, index);
    }
    if maybe_len == 0 {
        return Err(LookupError::NoCoverage { field });
    }
    let maybe = maybe.get(..maybe_len).ok_or(LookupError::TableFault)?;
    resolve_maybe(rules, field, at, policy, maybe)
}

pub fn rule_at(
    symbol: &str,
    field: Field,
    at: UtcDateTime,
    policy: BracketPolicy,
) -> Result<&'static InstrumentRule, LookupError> {
    lookup(BTCUSDT_RULES, symbol, field, at, policy)
}

#[cfg(kani)]
mod proofs {
    use super::*;
    use crate::instrument_spec::table::{FEE_TAKER_5, QTY_1000, QTY_120, WB2021_US, WB2022_US};

    /// Every instant, field, and policy returns `Ok` or a [`LookupError`].
    /// The table scan is bounded by the static row count.
    #[kani::proof]
    #[kani::unwind(24)]
    fn lookup_does_not_panic() {
        let micros: i64 = kani::any();
        let field: Field = kani::any();
        let policy: BracketPolicy = kani::any();
        let at = UtcDateTime::from_micros(crate::UtcMicros::from_micros(micros));
        let _ = rule_at("BTCUSDT", field, at, policy);
        let _ = rule_at("ETHUSDT", field, at, policy);
    }

    /// Inside the market-max bracket, conservative is the smaller cap and
    /// does not panic. Latest keeps the observed 1000.
    #[kani::proof]
    #[kani::unwind(24)]
    fn conservative_market_max_inside_bracket_is_the_smaller_cap() {
        let micros: i64 = kani::any();
        kani::assume(micros > WB2021_US);
        kani::assume(micros < WB2022_US);
        let at = UtcDateTime::from_micros(crate::UtcMicros::from_micros(micros));
        let conservative = rule_at(
            "BTCUSDT",
            Field::MarketMaxQty,
            at,
            BracketPolicy::Conservative,
        );
        let latest = rule_at("BTCUSDT", Field::MarketMaxQty, at, BracketPolicy::Latest);
        let rejected = rule_at("BTCUSDT", Field::MarketMaxQty, at, BracketPolicy::Reject);
        match conservative {
            Ok(rule) => match rule.value {
                RuleValue::Value(decimal) => assert_eq!(decimal.ticks(), QTY_120),
                RuleValue::Absent | RuleValue::Token(_) | RuleValue::EpochMillis(_) => {
                    assert!(false);
                }
            },
            Err(_) => assert!(false),
        }
        match latest {
            Ok(rule) => match rule.value {
                RuleValue::Value(decimal) => assert_eq!(decimal.ticks(), QTY_1000),
                RuleValue::Absent | RuleValue::Token(_) | RuleValue::EpochMillis(_) => {
                    assert!(false);
                }
            },
            Err(_) => assert!(false),
        }
        assert!(matches!(
            rejected,
            Err(LookupError::Unresolved {
                field: Field::MarketMaxQty
            })
        ));
    }

    /// The taker bracket resolves to the higher fee under the conservative policy.
    #[kani::proof]
    #[kani::unwind(24)]
    fn conservative_taker_fee_inside_bracket_is_the_higher_fee() {
        use crate::instrument_spec::table::{FEE_FAQ_2023_US, FEE_FAQ_2024_US};
        let micros: i64 = kani::any();
        kani::assume(micros > FEE_FAQ_2023_US);
        kani::assume(micros < FEE_FAQ_2024_US);
        let at = UtcDateTime::from_micros(crate::UtcMicros::from_micros(micros));
        let rule = rule_at(
            "BTCUSDT",
            Field::FeeTakerVip0,
            at,
            BracketPolicy::Conservative,
        );
        match rule {
            Ok(rule) => match rule.value {
                RuleValue::Value(decimal) => assert_eq!(decimal.ticks(), FEE_TAKER_5),
                RuleValue::Absent | RuleValue::Token(_) | RuleValue::EpochMillis(_) => {
                    assert!(false);
                }
            },
            Err(_) => assert!(false),
        }
    }
}
