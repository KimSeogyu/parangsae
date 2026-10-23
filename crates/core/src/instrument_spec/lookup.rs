//! Point lookup. A bracket picks a row only through [`BracketPolicy`].

use super::clock::UtcDateTime;
use super::decimal::Decimal;
use super::table::BTCUSDT_RULES;
use super::types::{Boundary, BracketPolicy, Field, InstrumentRule, RuleValue};

#[cfg(any(test, kani))]
use super::clock::UtcDate;
#[cfg(any(test, kani))]
use super::types::Confidence;
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

/// Byte equality. A 7-byte name (every symbol in this table is `BTCUSDT`) is
/// compared without `memcmp`. The two-row Kani harness would otherwise unwind
/// every loop, including `pick_side` and `fold_conservative`, to one iteration
/// per symbol byte, and that formula exhausts the solver.
fn symbol_eq(left: &str, right: &str) -> bool {
    match (left.as_bytes(), right.as_bytes()) {
        ([a, b, c, d, e, f, g], [h, i, j, k, l, m, n]) => {
            a == h && b == i && c == j && d == k && e == l && f == m && g == n
        }
        (left, right) => left == right,
    }
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
        if !symbol_eq(rule.symbol, symbol) {
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

#[cfg(any(test, kani))]
fn bracket_row(
    field: Field,
    ticks: i64,
    valid_from: Boundary,
    valid_to: Option<Boundary>,
) -> InstrumentRule {
    InstrumentRule {
        symbol: "BTCUSDT",
        venue: "binance-usdm",
        field,
        value: RuleValue::decimal(ticks),
        valid_from,
        valid_to,
        source: "https://web.archive.org/web/kani",
        source_date: UtcDate::known(2024, 2, 20),
        retrieved: UtcDate::known(2026, 10, 22),
        confidence: Confidence::VerifiedArchive,
        revision: 1,
    }
}

/// The two taker rows that share [`super::table::TAKER_FEE_CHANGE`].
#[cfg(any(test, kani))]
fn taker_pair() -> [InstrumentRule; 2] {
    use super::table::{FEE_TAKER_4, FEE_TAKER_5, TAKER_FEE_CHANGE, VIP0_FEE_START};
    [
        bracket_row(
            Field::FeeTakerVip0,
            FEE_TAKER_4,
            VIP0_FEE_START,
            Some(TAKER_FEE_CHANGE),
        ),
        bracket_row(Field::FeeTakerVip0, FEE_TAKER_5, TAKER_FEE_CHANGE, None),
    ]
}

/// The two market-max rows that share [`super::table::MARKET_MAX_CHANGE`].
#[cfg(any(test, kani))]
fn market_pair() -> [InstrumentRule; 2] {
    use super::table::{MARKET_MAX_CHANGE, QTY_1000, QTY_120, WB2021};
    [
        bracket_row(
            Field::MarketMaxQty,
            QTY_1000,
            Boundary::Exact(WB2021),
            Some(MARKET_MAX_CHANGE),
        ),
        bracket_row(Field::MarketMaxQty, QTY_120, MARKET_MAX_CHANGE, None),
    ]
}

#[cfg(kani)]
mod proofs {
    use super::{
        lookup, market_pair, region, taker_pair, Boundary, BracketPolicy, Field, LookupError,
        Region, RuleValue, UtcDateTime,
    };
    use crate::instrument_spec::table::{
        FEE_FAQ_2023_US, FEE_FAQ_2024_US, FEE_TAKER_4, FEE_TAKER_5, QTY_1000, QTY_120, WB2021_US,
        WB2022_US,
    };
    use crate::UtcMicros;

    fn instant(micros: i64) -> UtcDateTime {
        UtcDateTime::from_micros(UtcMicros::from_micros(micros))
    }

    /// A bound classifies every instant. An inverted bracket is [`LookupError::TableFault`].
    #[kani::proof]
    fn boundary_region_does_not_panic() {
        let early: i64 = kani::any();
        let late: i64 = kani::any();
        let at_us: i64 = kani::any();
        let exact: bool = kani::any();
        let at = instant(at_us);
        if exact {
            let found = region(Boundary::Exact(instant(early)), at);
            match found {
                Ok(Region::Before) => assert!(at_us < early),
                Ok(Region::AfterOrOn) => assert!(at_us >= early),
                Ok(Region::Inside) => {
                    let inside_exact = false;
                    assert!(inside_exact);
                }
                Err(_) => {
                    let exact_fault = false;
                    assert!(exact_fault);
                }
            }
        } else {
            let found = region(
                Boundary::Bracketed {
                    earliest: instant(early),
                    latest: instant(late),
                },
                at,
            );
            match found {
                Ok(Region::Before) => assert!(early < late && at_us <= early),
                Ok(Region::Inside) => assert!(early < late && at_us > early && at_us < late),
                Ok(Region::AfterOrOn) => assert!(early < late && at_us >= late),
                Err(LookupError::TableFault) => assert!(early >= late),
                Err(_) => {
                    let other_fault = false;
                    assert!(other_fault);
                }
            }
        }
    }

    /// Two-row brackets only. A scan of the full static table is not a bounded
    /// harness: the string compare plus every row blows the solver budget.
    /// Seven-byte names go through `symbol_eq`, so this bound only has to cover
    /// the two rule rows (the exit check, plus slack for `continue`). One
    /// harness that called all three lookups, unwound to 9, exhausted CBMC
    /// on the self-hosted runner.
    fn two_row_at() -> (BracketPolicy, UtcDateTime) {
        let micros: i64 = kani::any();
        let policy: BracketPolicy = kani::any();
        (policy, instant(micros))
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn lookup_taker_btc_on_two_rows_does_not_panic() {
        let (policy, at) = two_row_at();
        let _ = lookup(&taker_pair(), "BTCUSDT", Field::FeeTakerVip0, at, policy);
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn lookup_unknown_symbol_on_two_rows_does_not_panic() {
        let (policy, at) = two_row_at();
        let _ = lookup(&taker_pair(), "ETHUSDT", Field::FeeTakerVip0, at, policy);
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn lookup_market_btc_on_two_rows_does_not_panic() {
        let (policy, at) = two_row_at();
        let _ = lookup(&market_pair(), "BTCUSDT", Field::MarketMaxQty, at, policy);
    }

    #[kani::proof]
    #[kani::unwind(9)]
    fn conservative_market_max_inside_bracket_is_the_smaller_cap() {
        let micros: i64 = kani::any();
        kani::assume(micros > WB2021_US);
        kani::assume(micros < WB2022_US);
        let at = instant(micros);
        let rules = market_pair();
        let conservative = lookup(
            &rules,
            "BTCUSDT",
            Field::MarketMaxQty,
            at,
            BracketPolicy::Conservative,
        );
        let latest = lookup(
            &rules,
            "BTCUSDT",
            Field::MarketMaxQty,
            at,
            BracketPolicy::Latest,
        );
        let rejected = lookup(
            &rules,
            "BTCUSDT",
            Field::MarketMaxQty,
            at,
            BracketPolicy::Reject,
        );
        assert!(matches!(
            conservative,
            Ok(rule) if matches!(rule.value, RuleValue::Value(decimal) if decimal.ticks() == QTY_120)
        ));
        assert!(matches!(
            latest,
            Ok(rule) if matches!(rule.value, RuleValue::Value(decimal) if decimal.ticks() == QTY_1000)
        ));
        assert!(matches!(
            rejected,
            Err(LookupError::Unresolved {
                field: Field::MarketMaxQty
            })
        ));
    }

    #[kani::proof]
    #[kani::unwind(9)]
    fn conservative_taker_fee_inside_bracket_is_the_higher_fee() {
        let micros: i64 = kani::any();
        kani::assume(micros > FEE_FAQ_2023_US);
        kani::assume(micros < FEE_FAQ_2024_US);
        let at = instant(micros);
        let rules = taker_pair();
        let conservative = lookup(
            &rules,
            "BTCUSDT",
            Field::FeeTakerVip0,
            at,
            BracketPolicy::Conservative,
        );
        let latest = lookup(
            &rules,
            "BTCUSDT",
            Field::FeeTakerVip0,
            at,
            BracketPolicy::Latest,
        );
        assert!(matches!(
            conservative,
            Ok(rule) if matches!(rule.value, RuleValue::Value(decimal) if decimal.ticks() == FEE_TAKER_5)
        ));
        assert!(matches!(
            latest,
            Ok(rule) if matches!(rule.value, RuleValue::Value(decimal) if decimal.ticks() == FEE_TAKER_4)
        ));
    }
}

#[cfg(test)]
mod proof_fixtures {
    use super::{market_pair, taker_pair, BracketPolicy, Field, UtcDateTime};
    use crate::instrument_spec::rule_at;
    use crate::instrument_spec::table::{FEE_FAQ_2023_US, FEE_FAQ_2024_US, WB2021_US, WB2022_US};
    use crate::UtcMicros;

    #[test]
    fn two_row_brackets_match_the_table() -> Result<(), String> {
        let taker_at = FEE_FAQ_2023_US
            .checked_add(
                FEE_FAQ_2024_US
                    .checked_sub(FEE_FAQ_2023_US)
                    .ok_or("taker span")?
                    / 2,
            )
            .ok_or("taker mid")?;
        let market_at = WB2021_US
            .checked_add(WB2022_US.checked_sub(WB2021_US).ok_or("market span")? / 2)
            .ok_or("market mid")?;
        let taker_at = UtcDateTime::from_micros(UtcMicros::from_micros(taker_at));
        let market_at = UtcDateTime::from_micros(UtcMicros::from_micros(market_at));
        let taker = taker_pair();
        let from_pair = super::lookup(
            &taker,
            "BTCUSDT",
            Field::FeeTakerVip0,
            taker_at,
            BracketPolicy::Conservative,
        )
        .map_err(|err| err.to_string())?;
        let from_table = rule_at(
            "BTCUSDT",
            Field::FeeTakerVip0,
            taker_at,
            BracketPolicy::Conservative,
        )
        .map_err(|err| err.to_string())?;
        if from_pair.value != from_table.value {
            return Err("taker pair diverges from the table".to_owned());
        }
        let market = market_pair();
        let from_pair = super::lookup(
            &market,
            "BTCUSDT",
            Field::MarketMaxQty,
            market_at,
            BracketPolicy::Conservative,
        )
        .map_err(|err| err.to_string())?;
        let from_table = rule_at(
            "BTCUSDT",
            Field::MarketMaxQty,
            market_at,
            BracketPolicy::Conservative,
        )
        .map_err(|err| err.to_string())?;
        if from_pair.value != from_table.value {
            return Err("market pair diverges from the table".to_owned());
        }
        Ok(())
    }
}
