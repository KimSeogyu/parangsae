//! The six table guards. Each one returns `Err` on a broken fixture.

use super::clock::UtcDateTime;
use super::decimal::Decimal;
use super::lookup::{lookup, LookupError};
use super::table::{BACKTEST_START, BTCUSDT_RULES, TICK_COARSEN_GRACE_US};
use super::types::{
    Boundary, BracketPolicy, Confidence, CoverageGap, Field, InstrumentRule, PaperBlock, RuleValue,
};
use crate::{notional, Notional};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContinuityError {
    Empty,
    BadBracket { field: Field },
    Inverted { field: Field },
    BadValue { field: Field },
    DuplicateIdentity { field: Field },
    BrokenLink { field: Field },
    OpenNotLast { field: Field },
    OpenCount { field: Field, open: u32 },
    DuplicateStart { field: Field },
    UnlistedGap { field: Field },
    StaleGap { field: Field },
    GapMismatch { field: Field },
    EmptyGapReason { field: Field },
}

impl fmt::Display for ContinuityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "no rules"),
            Self::BadBracket { field } => write!(f, "{field} bracket is not ordered"),
            Self::Inverted { field } => write!(f, "{field} exact span is empty or reversed"),
            Self::BadValue { field } => write!(f, "{field} value is empty or negative"),
            Self::DuplicateIdentity { field } => write!(f, "duplicate {field} revision"),
            Self::BrokenLink { field } => write!(f, "{field} has a gap or an overlap"),
            Self::OpenNotLast { field } => write!(f, "{field} open row is not last"),
            Self::OpenCount { field, open } => write!(f, "{field} has {open} open rows"),
            Self::DuplicateStart { field } => write!(f, "{field} has two rows with one start"),
            Self::UnlistedGap { field } => {
                write!(f, "{field} starts after the backtest and is not listed")
            }
            Self::StaleGap { field } => {
                write!(f, "{field} is listed but already covers the backtest")
            }
            Self::GapMismatch { field } => write!(f, "{field} coverage-gap instant does not match"),
            Self::EmptyGapReason { field } => write!(f, "{field} coverage gap has an empty reason"),
        }
    }
}

fn value_ok(rule: &InstrumentRule) -> bool {
    match rule.value {
        RuleValue::Value(decimal) => decimal.ticks() >= 0,
        RuleValue::Absent => true,
        RuleValue::Token(text) => !text.is_empty(),
        RuleValue::EpochMillis(millis) => millis > 0,
    }
}

fn well_formed(rules: &[InstrumentRule]) -> Result<(), ContinuityError> {
    for (index, rule) in rules.iter().enumerate() {
        if rule.symbol.is_empty() || rule.venue.is_empty() || rule.revision == 0 {
            return Err(ContinuityError::BadValue { field: rule.field });
        }
        if !rule.valid_from.is_well_ordered() {
            return Err(ContinuityError::BadBracket { field: rule.field });
        }
        if let Some(end) = rule.valid_to {
            if !end.is_well_ordered() {
                return Err(ContinuityError::BadBracket { field: rule.field });
            }
        }
        if let (Boundary::Exact(start), Some(Boundary::Exact(end))) =
            (rule.valid_from, rule.valid_to)
        {
            if start >= end {
                return Err(ContinuityError::Inverted { field: rule.field });
            }
        }
        if !value_ok(rule) {
            return Err(ContinuityError::BadValue { field: rule.field });
        }
        for other in rules.iter().take(index) {
            if other.symbol == rule.symbol
                && other.field == rule.field
                && other.revision == rule.revision
                && other.valid_from == rule.valid_from
            {
                return Err(ContinuityError::DuplicateIdentity { field: rule.field });
            }
        }
    }
    Ok(())
}

fn certain_start(rule: &InstrumentRule) -> UtcDateTime {
    match rule.valid_from {
        Boundary::Exact(at) => at,
        Boundary::Bracketed { latest, .. } => latest,
    }
}

fn listed(gaps: &[CoverageGap], field: Field) -> Option<&CoverageGap> {
    gaps.iter().find(|gap| gap.field == field)
}

fn check_group(group: &[&InstrumentRule], gaps: &[CoverageGap]) -> Result<(), ContinuityError> {
    let (first, _) = group.split_first().ok_or(ContinuityError::Empty)?;
    let mut open = 0u32;
    let mut previous: Option<&InstrumentRule> = None;
    for rule in group {
        if rule.valid_to.is_none() {
            open = open.saturating_add(1);
        }
        if let Some(prev) = previous {
            let end = prev
                .valid_to
                .ok_or(ContinuityError::OpenNotLast { field: rule.field })?;
            if end != rule.valid_from {
                return Err(ContinuityError::BrokenLink { field: rule.field });
            }
            if prev.valid_from == rule.valid_from {
                return Err(ContinuityError::DuplicateStart { field: rule.field });
            }
        }
        previous = Some(rule);
    }
    if open != 1 {
        return Err(ContinuityError::OpenCount {
            field: first.field,
            open,
        });
    }
    let last = group.last().ok_or(ContinuityError::Empty)?;
    if last.valid_to.is_some() {
        return Err(ContinuityError::OpenNotLast { field: last.field });
    }
    let known_from = certain_start(first);
    let listed = listed(gaps, first.field);
    if known_from > BACKTEST_START {
        match listed {
            Some(gap) if gap.reason.is_empty() => {
                Err(ContinuityError::EmptyGapReason { field: first.field })
            }
            Some(gap) if gap.known_from != known_from => {
                Err(ContinuityError::GapMismatch { field: first.field })
            }
            Some(_) => Ok(()),
            None => Err(ContinuityError::UnlistedGap { field: first.field }),
        }
    } else if listed.is_some() {
        Err(ContinuityError::StaleGap { field: first.field })
    } else {
        Ok(())
    }
}

/// No gaps and no overlaps per `(symbol, field)`. Exactly one open row.
/// A field whose first certain instant is after [`BACKTEST_START`] must be
/// in `gaps` with that instant and a non-empty reason.
pub fn audit_continuity(
    rules: &[InstrumentRule],
    gaps: &[CoverageGap],
) -> Result<(), ContinuityError> {
    if rules.is_empty() {
        return Err(ContinuityError::Empty);
    }
    well_formed(rules)?;
    for gap in gaps {
        if gap.reason.is_empty() {
            return Err(ContinuityError::EmptyGapReason { field: gap.field });
        }
    }
    let mut seen: Vec<(&str, Field)> = Vec::new();
    for rule in rules {
        if seen
            .iter()
            .any(|(symbol, field)| *symbol == rule.symbol && *field == rule.field)
        {
            continue;
        }
        seen.push((rule.symbol, rule.field));
        let mut group: Vec<&InstrumentRule> = rules
            .iter()
            .filter(|other| other.symbol == rule.symbol && other.field == rule.field)
            .collect();
        group.sort_by(|left, right| {
            left.valid_from
                .earliest()
                .cmp(&right.valid_from.earliest())
                .then_with(|| {
                    let rank = |bound: Boundary| match bound {
                        Boundary::Exact(_) => 0,
                        Boundary::Bracketed { .. } => 1,
                    };
                    rank(left.valid_from).cmp(&rank(right.valid_from))
                })
        });
        check_group(&group, gaps)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvenanceError {
    EmptySource { field: Field },
    SourceNotHttps { field: Field },
    RetrievedBeforeSource { field: Field },
    UnverifiedFill { field: Field },
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySource { field } => write!(f, "{field} has an empty source"),
            Self::SourceNotHttps { field } => write!(f, "{field} source is not https"),
            Self::RetrievedBeforeSource { field } => {
                write!(f, "{field} was retrieved before its source date")
            }
            Self::UnverifiedFill { field } => {
                write!(f, "{field} is unverified and not blocked for paper")
            }
        }
    }
}

pub fn is_paper_blocked(blocked: &[PaperBlock], rule: &InstrumentRule) -> bool {
    blocked.iter().any(|block| {
        block.symbol == rule.symbol && block.field == rule.field && !block.reason.is_empty()
    })
}

/// Every row has an https source retrieved on or after `source_date`.
/// An unverified fill field must be on `blocked`, which refuses paper use.
pub fn audit_provenance(
    rules: &[InstrumentRule],
    blocked: &[PaperBlock],
) -> Result<(), ProvenanceError> {
    for rule in rules {
        if rule.source.is_empty() {
            return Err(ProvenanceError::EmptySource { field: rule.field });
        }
        if !rule.source.starts_with("https://") {
            return Err(ProvenanceError::SourceNotHttps { field: rule.field });
        }
        if rule.retrieved < rule.source_date {
            return Err(ProvenanceError::RetrievedBeforeSource { field: rule.field });
        }
        if rule.field.is_fill()
            && rule.confidence == Confidence::Unverified
            && !is_paper_blocked(blocked, rule)
        {
            return Err(ProvenanceError::UnverifiedFill { field: rule.field });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintFit {
    Active,
    Grace { previous_tick: Decimal },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridError {
    Lookup(LookupError),
    OffGrid { price: Decimal, tick: Decimal },
    OffStep { qty: Decimal, step: Decimal },
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lookup(err) => write!(f, "{err}"),
            Self::OffGrid { price, tick } => {
                write!(f, "price {price} is off tick {tick}")
            }
            Self::OffStep { qty, step } => write!(f, "qty {qty} is off step {step}"),
        }
    }
}

fn value_decimal(rule: &InstrumentRule) -> Option<Decimal> {
    match rule.value {
        RuleValue::Value(decimal) => Some(decimal),
        RuleValue::Absent | RuleValue::Token(_) | RuleValue::EpochMillis(_) => None,
    }
}

fn in_grace(switch: UtcDateTime, at: UtcDateTime) -> bool {
    let Some(end) = switch.checked_add_micros(TICK_COARSEN_GRACE_US) else {
        return false;
    };
    at > switch && at < end
}

fn grace_tick(
    rules: &[InstrumentRule],
    symbol: &str,
    at: UtcDateTime,
    price: Decimal,
) -> Result<Option<Decimal>, GridError> {
    let mut ticks: Vec<&InstrumentRule> = rules
        .iter()
        .filter(|rule| rule.symbol == symbol && rule.field == Field::TickSize)
        .collect();
    ticks.sort_by_key(|rule| rule.valid_from.earliest());
    let mut previous: Option<&InstrumentRule> = None;
    for rule in &ticks {
        if let Some(prev) = previous {
            if let (Some(Boundary::Exact(switch)), Some(old), Some(new)) =
                (prev.valid_to, value_decimal(prev), value_decimal(rule))
            {
                if rule.valid_from == Boundary::Exact(switch)
                    && new > old
                    && in_grace(switch, at)
                    && price.is_multiple_of(old)
                {
                    return Ok(Some(old));
                }
            }
        }
        previous = Some(rule);
    }
    Ok(None)
}

/// Price on the active tick, qty on the active step.
/// After an exact tick coarsening, a print on the previous tick is
/// [`PrintFit::Grace`] for [`TICK_COARSEN_GRACE_US`]. New orders do not use it.
pub fn classify_print(
    rules: &[InstrumentRule],
    symbol: &str,
    at: UtcDateTime,
    price: Decimal,
    qty: Option<Decimal>,
) -> Result<PrintFit, GridError> {
    let tick_rule = lookup(rules, symbol, Field::TickSize, at, BracketPolicy::Reject)
        .map_err(GridError::Lookup)?;
    let tick = value_decimal(tick_rule).ok_or(GridError::Lookup(LookupError::TableFault))?;
    let fit = if price.is_multiple_of(tick) {
        PrintFit::Active
    } else if let Some(previous) = grace_tick(rules, symbol, at, price)? {
        PrintFit::Grace {
            previous_tick: previous,
        }
    } else {
        return Err(GridError::OffGrid { price, tick });
    };
    if let Some(qty) = qty {
        let step_rule = lookup(rules, symbol, Field::StepSize, at, BracketPolicy::Reject)
            .map_err(GridError::Lookup)?;
        let step = value_decimal(step_rule).ok_or(GridError::Lookup(LookupError::TableFault))?;
        if !qty.is_multiple_of(step) {
            return Err(GridError::OffStep { qty, step });
        }
    }
    Ok(fit)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridReport {
    pub rows: u64,
    pub active: u64,
    pub grace: u64,
    pub violations: u64,
}

impl GridReport {
    pub const fn is_clean(self) -> bool {
        self.violations == 0
    }
}

pub fn audit_grid(rows: &[GridSample<'_>]) -> Result<GridReport, GridReport> {
    let mut report = GridReport {
        rows: 0,
        active: 0,
        grace: 0,
        violations: 0,
    };
    for sample in rows {
        report.rows = report.rows.saturating_add(1);
        match classify_print(
            BTCUSDT_RULES,
            sample.symbol,
            sample.at,
            sample.price,
            sample.qty,
        ) {
            Ok(PrintFit::Active) => report.active = report.active.saturating_add(1),
            Ok(PrintFit::Grace { .. }) => report.grace = report.grace.saturating_add(1),
            Err(_) => report.violations = report.violations.saturating_add(1),
        }
    }
    if report.violations == 0 {
        Ok(report)
    } else {
        Err(report)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSample<'a> {
    pub symbol: &'a str,
    pub at: UtcDateTime,
    pub price: Decimal,
    pub qty: Option<Decimal>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaperOrder<'a> {
    pub symbol: &'a str,
    pub at: UtcDateTime,
    pub price: Decimal,
    pub qty: Decimal,
    pub market: bool,
    pub reduce_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderError {
    Lookup(LookupError),
    BelowMinQty { qty: Decimal, min: Decimal },
    AboveMaxQty { qty: Decimal, max: Decimal },
    BelowMinNotional { notional: Notional, min: Decimal },
    OffTick { price: Decimal, tick: Decimal },
    OffStep { qty: Decimal, step: Decimal },
    PaperBlocked { field: Field },
    BadNumber,
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lookup(err) => write!(f, "{err}"),
            Self::BelowMinQty { qty, min } => write!(f, "qty {qty} < min {min}"),
            Self::AboveMaxQty { qty, max } => write!(f, "qty {qty} > max {max}"),
            Self::BelowMinNotional { notional, min } => {
                write!(f, "notional {} < min {min}", notional.ticks())
            }
            Self::OffTick { price, tick } => write!(f, "price {price} off tick {tick}"),
            Self::OffStep { qty, step } => write!(f, "qty {qty} off step {step}"),
            Self::PaperBlocked { field } => write!(f, "{field} is blocked for paper"),
            Self::BadNumber => write!(f, "order number is not a price or qty"),
        }
    }
}

fn require_decimal(
    rules: &[InstrumentRule],
    blocked: &[PaperBlock],
    order: &PaperOrder<'_>,
    field: Field,
) -> Result<Decimal, OrderError> {
    let rule = lookup(
        rules,
        order.symbol,
        field,
        order.at,
        BracketPolicy::Conservative,
    )
    .map_err(OrderError::Lookup)?;
    if is_paper_blocked(blocked, rule) {
        return Err(OrderError::PaperBlocked { field });
    }
    value_decimal(rule).ok_or(OrderError::BadNumber)
}

/// Paper filter. No network and no submit.
///
/// Min and max price are not applied: the seed has no `minPrice` or `maxPrice`
/// row, and tick size is never derived from `pricePrecision`.
/// Quantity uses `StepSize` and `MinQty` for both limit and market orders;
/// the seed has no separate market step. The cap is `MarketMaxQty` or `LotMaxQty`.
/// Reduce-only skips min notional. `Absent` min notional is no filter.
pub fn check_paper_order(
    rules: &[InstrumentRule],
    blocked: &[PaperBlock],
    order: &PaperOrder<'_>,
) -> Result<(), OrderError> {
    let min_qty = require_decimal(rules, blocked, order, Field::MinQty)?;
    if order.qty < min_qty {
        return Err(OrderError::BelowMinQty {
            qty: order.qty,
            min: min_qty,
        });
    }
    let step = require_decimal(rules, blocked, order, Field::StepSize)?;
    if !order.qty.is_multiple_of(step) {
        return Err(OrderError::OffStep {
            qty: order.qty,
            step,
        });
    }
    let cap_field = if order.market {
        Field::MarketMaxQty
    } else {
        Field::LotMaxQty
    };
    let cap = require_decimal(rules, blocked, order, cap_field)?;
    if order.qty > cap {
        return Err(OrderError::AboveMaxQty {
            qty: order.qty,
            max: cap,
        });
    }
    let tick = require_decimal(rules, blocked, order, Field::TickSize)?;
    if !order.price.is_multiple_of(tick) {
        return Err(OrderError::OffTick {
            price: order.price,
            tick,
        });
    }
    let notional_rule = lookup(
        rules,
        order.symbol,
        Field::MinNotional,
        order.at,
        BracketPolicy::Conservative,
    )
    .map_err(OrderError::Lookup)?;
    if is_paper_blocked(blocked, notional_rule) {
        return Err(OrderError::PaperBlocked {
            field: Field::MinNotional,
        });
    }
    if !order.reduce_only {
        if let RuleValue::Value(min) = notional_rule.value {
            let price = order.price.as_price().map_err(|_| OrderError::BadNumber)?;
            let qty = order.qty.as_qty().map_err(|_| OrderError::BadNumber)?;
            let notion = notional(price, qty).map_err(|_| OrderError::BadNumber)?;
            if notion.ticks() < min.ticks() {
                return Err(OrderError::BelowMinNotional {
                    notional: notion,
                    min,
                });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotFields {
    pub symbol: &'static str,
    pub server_time: UtcDateTime,
    pub tick_size: Decimal,
    pub step_size: Decimal,
    pub min_qty: Decimal,
    pub lot_max_qty: Decimal,
    pub market_max_qty: Decimal,
    pub min_notional: Option<Decimal>,
    pub price_precision: Decimal,
    pub quantity_precision: Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    Symbol,
    Tick { found: Decimal, expected: Decimal },
    Step { found: Decimal, expected: Decimal },
    MinQty { found: Decimal, expected: Decimal },
    LotMax { found: Decimal, expected: Decimal },
    MarketMax { found: Decimal, expected: Decimal },
    MinNotional,
    PricePrecision { found: Decimal, expected: Decimal },
    QuantityPrecision { found: Decimal, expected: Decimal },
    Lookup(LookupError),
    InsideBracket { field: Field },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbol => write!(f, "snapshot symbol"),
            Self::Tick { found, expected } => write!(f, "tick {found} != {expected}"),
            Self::Step { found, expected } => write!(f, "step {found} != {expected}"),
            Self::MinQty { found, expected } => write!(f, "min qty {found} != {expected}"),
            Self::LotMax { found, expected } => write!(f, "lot max {found} != {expected}"),
            Self::MarketMax { found, expected } => {
                write!(f, "market max {found} != {expected}")
            }
            Self::MinNotional => write!(f, "min notional does not match the snapshot"),
            Self::PricePrecision { found, expected } => {
                write!(f, "price precision {found} != {expected}")
            }
            Self::QuantityPrecision { found, expected } => {
                write!(f, "quantity precision {found} != {expected}")
            }
            Self::Lookup(err) => write!(f, "{err}"),
            Self::InsideBracket { field } => {
                write!(f, "{field} is unresolved at the snapshot instant")
            }
        }
    }
}

fn certain<'a>(
    rules: &'a [InstrumentRule],
    symbol: &str,
    field: Field,
    at: UtcDateTime,
) -> Result<&'a InstrumentRule, SnapshotError> {
    match lookup(rules, symbol, field, at, BracketPolicy::Reject) {
        Ok(rule) => Ok(rule),
        Err(LookupError::Unresolved { field }) => Err(SnapshotError::InsideBracket { field }),
        Err(err) => Err(SnapshotError::Lookup(err)),
    }
}

fn expect_decimal(
    rule: &InstrumentRule,
    found: Decimal,
    mismatch: impl FnOnce(Decimal, Decimal) -> SnapshotError,
) -> Result<(), SnapshotError> {
    match rule.value {
        RuleValue::Value(expected) if expected == found => Ok(()),
        RuleValue::Value(expected) => Err(mismatch(found, expected)),
        RuleValue::Absent | RuleValue::Token(_) | RuleValue::EpochMillis(_) => {
            Err(SnapshotError::Lookup(LookupError::TableFault))
        }
    }
}

/// `server_time` is an observation, so `Reject` must succeed and the value
/// must equal the archived field. A bracket that covers the snapshot fails.
pub fn audit_snapshot(
    rules: &[InstrumentRule],
    snap: &SnapshotFields,
) -> Result<(), SnapshotError> {
    if snap.symbol != "BTCUSDT" {
        return Err(SnapshotError::Symbol);
    }
    let at = snap.server_time;
    expect_decimal(
        certain(rules, snap.symbol, Field::TickSize, at)?,
        snap.tick_size,
        |found, expected| SnapshotError::Tick { found, expected },
    )?;
    expect_decimal(
        certain(rules, snap.symbol, Field::StepSize, at)?,
        snap.step_size,
        |found, expected| SnapshotError::Step { found, expected },
    )?;
    expect_decimal(
        certain(rules, snap.symbol, Field::MinQty, at)?,
        snap.min_qty,
        |found, expected| SnapshotError::MinQty { found, expected },
    )?;
    expect_decimal(
        certain(rules, snap.symbol, Field::LotMaxQty, at)?,
        snap.lot_max_qty,
        |found, expected| SnapshotError::LotMax { found, expected },
    )?;
    expect_decimal(
        certain(rules, snap.symbol, Field::MarketMaxQty, at)?,
        snap.market_max_qty,
        |found, expected| SnapshotError::MarketMax { found, expected },
    )?;
    expect_decimal(
        certain(rules, snap.symbol, Field::PricePrecision, at)?,
        snap.price_precision,
        |found, expected| SnapshotError::PricePrecision { found, expected },
    )?;
    expect_decimal(
        certain(rules, snap.symbol, Field::QuantityPrecision, at)?,
        snap.quantity_precision,
        |found, expected| SnapshotError::QuantityPrecision { found, expected },
    )?;
    let notional_rule = certain(rules, snap.symbol, Field::MinNotional, at)?;
    match (snap.min_notional, notional_rule.value) {
        (None, RuleValue::Absent) => Ok(()),
        (Some(found), RuleValue::Value(expected)) if found == expected => Ok(()),
        (Some(_), RuleValue::Value(_))
        | (None, RuleValue::Value(_))
        | (Some(_), RuleValue::Absent)
        | (None, RuleValue::Token(_))
        | (Some(_), RuleValue::Token(_))
        | (None, RuleValue::EpochMillis(_))
        | (Some(_), RuleValue::EpochMillis(_)) => Err(SnapshotError::MinNotional),
    }
}
