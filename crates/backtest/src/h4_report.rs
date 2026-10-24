//! Checked H4 accounting and provenance. These reports do not add strategy gates.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use parangsae_core::Side;
use parangsae_sim::{Cash, Fill};
use parangsae_strategies::{H4Event, H4ObservationKind, PositionSide};
use serde::{Serialize, Serializer};

use crate::report::{ForcedExitReason, H4EntryDropReason, Report};

const SCALE: i128 = 100_000_000;
const CONFIRM_US: i64 = 86_400_000_000 + 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H4ReportError {
    Scope,
    Origin,
    Observation,
    OpenTrade,
    Trade,
    Cashflow,
    ForcedExit,
    Reconciliation,
    MissingWindow,
    Overflow,
}

impl fmt::Display for H4ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "H4 report {:?}", self)
    }
}

impl std::error::Error for H4ReportError {}

fn add(a: i128, b: i128) -> Result<i128, H4ReportError> {
    a.checked_add(b).ok_or(H4ReportError::Overflow)
}

fn increment(value: &mut u64) -> Result<(), H4ReportError> {
    *value = value.checked_add(1).ok_or(H4ReportError::Overflow)?;
    Ok(())
}

fn integer<S: Serializer>(value: &i128, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CashTotals {
    #[serde(serialize_with = "integer")]
    pub net_ticks: i128,
    #[serde(serialize_with = "integer")]
    pub fees_ticks: i128,
    #[serde(serialize_with = "integer")]
    pub slippage_ticks: i128,
    #[serde(serialize_with = "integer")]
    pub funding_ticks: i128,
    #[serde(serialize_with = "integer")]
    pub gross_price_ticks: i128,
}

impl CashTotals {
    fn new(net: i128, fees: i128, slip: i128, funding: i128) -> Result<Self, H4ReportError> {
        if fees < 0 || slip < 0 {
            return Err(H4ReportError::Cashflow);
        }
        let gross = add(add(net, fees)?, slip)?
            .checked_sub(funding)
            .ok_or(H4ReportError::Overflow)?;
        Ok(Self {
            net_ticks: net,
            fees_ticks: fees,
            slippage_ticks: slip,
            funding_ticks: funding,
            gross_price_ticks: gross,
        })
    }

    fn checked_add(self, other: Self) -> Result<Self, H4ReportError> {
        Self::new(
            add(self.net_ticks, other.net_ticks)?,
            add(self.fees_ticks, other.fees_ticks)?,
            add(self.slippage_ticks, other.slippage_ticks)?,
            add(self.funding_ticks, other.funding_ticks)?,
        )
    }

    fn funding_test(self) -> Result<FundingTest, H4ReportError> {
        let ten_f = self
            .funding_ticks
            .checked_mul(10)
            .ok_or(H4ReportError::Overflow)?;
        Ok(FundingTest {
            original_literal_flag: self.funding_ticks < 0 || ten_f < self.gross_price_ticks,
            funding_over_gross: Fraction::positive_denominator(
                self.funding_ticks,
                self.gross_price_ticks,
            ),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fraction {
    pub numerator: String,
    pub denominator: String,
}

impl Fraction {
    fn positive_denominator(numerator: i128, denominator: i128) -> Option<Self> {
        (denominator > 0).then(|| Self {
            numerator: numerator.to_string(),
            denominator: denominator.to_string(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FundingTest {
    pub original_literal_flag: bool,
    /// Undefined when G <= 0. F and G remain in CashTotals in every case.
    pub funding_over_gross: Option<Fraction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrowdSide {
    Long,
    Short,
}

impl CrowdSide {
    fn from_raw(side: PositionSide) -> Self {
        match side {
            PositionSide::Long => Self::Long,
            PositionSide::Short => Self::Short,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Long => 0,
            Self::Short => 1,
        }
    }

    fn expected_fill(self) -> Side {
        match self {
            Self::Long => Side::Sell,
            Self::Short => Side::Buy,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Origin {
    pub settlement_us: i64,
    pub episode_us: i64,
    pub crowded_side: CrowdSide,
    pub gap_boundary: bool,
}

impl From<H4Event> for Origin {
    fn from(event: H4Event) -> Self {
        Self {
            settlement_us: event.settlement.micros(),
            episode_us: event.episode.micros(),
            crowded_side: CrowdSide::from_raw(event.crowd_side),
            gap_boundary: event.gap_boundary,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EventRecord {
    pub origin: Origin,
    pub scored_tail: bool,
    pub scored_confirmation: Option<bool>,
    pub scored_confirmation_unavailable: bool,
    pub scored_occupied_skips: u64,
    pub scored_entries: u64,
    pub late_tail_observation: bool,
}

fn event_record(
    events: &mut BTreeMap<i64, EventRecord>,
    raw: H4Event,
) -> Result<&mut EventRecord, H4ReportError> {
    let origin = Origin::from(raw);
    if origin.episode_us > origin.settlement_us {
        return Err(H4ReportError::Origin);
    }
    let record = events.entry(origin.settlement_us).or_insert(EventRecord {
        origin,
        scored_tail: false,
        scored_confirmation: None,
        scored_confirmation_unavailable: false,
        scored_occupied_skips: 0,
        scored_entries: 0,
        late_tail_observation: false,
    });
    if record.origin != origin {
        return Err(H4ReportError::Origin);
    }
    Ok(record)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    Tail,
    ConfirmationTrue,
    ConfirmationFalse,
    ConfirmationUnavailable,
    OccupiedSkip,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservationRecord {
    pub origin: Origin,
    /// Actual feature evaluation time, not a source publication timestamp.
    pub observed_at_us: i64,
    pub scored: bool,
    pub kind: ObservationKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EpisodeRecord {
    pub episode_us: i64,
    pub crowded_side: CrowdSide,
    pub gap_boundary: bool,
    pub starts_before_scored_window: bool,
    pub scored_tail_settlements_us: BTreeSet<i64>,
    pub scored_confirmed_settlements_us: BTreeSet<i64>,
    pub entered_settlements_us: BTreeSet<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FillRecord {
    pub at_us: i64,
    pub side: String,
    pub qty_ticks: String,
    pub price_ticks: String,
    pub fee_ticks: String,
}

impl From<&Fill> for FillRecord {
    fn from(fill: &Fill) -> Self {
        Self {
            at_us: fill.ts.micros(),
            side: fill.side.to_string(),
            qty_ticks: fill.qty.ticks().to_string(),
            price_ticks: fill.price.ticks().to_string(),
            fee_ticks: fill.fee.0.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FundingRecord {
    pub at_us: i64,
    pub rate_ticks: String,
    pub mark_ticks: String,
    pub position_ticks: String,
    pub amount_ticks: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForcedReason {
    Terminal,
    Ruin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TradeRecord {
    pub origin: Origin,
    pub open_us: i64,
    pub close_us: i64,
    pub actual_side: String,
    pub qty_ticks: String,
    pub requested_hold_us: i64,
    pub scheduled_exit_decision_us: i64,
    pub forced_reason: Option<ForcedReason>,
    pub forced_trigger_us: Option<i64>,
    pub cash: CashTotals,
    pub fills: Vec<FillRecord>,
    pub funding: Vec<FundingRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    DevelopmentBoundary,
    ZeroLots,
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DropRecord {
    pub origin: Origin,
    pub at_us: i64,
    pub reason: DropReason,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SideTotals {
    pub entries: u64,
    pub closed_round_trips: u64,
    pub strategy_round_trips: u64,
    pub forced_round_trips: u64,
    pub cash: CashTotals,
}

impl SideTotals {
    fn checked_add(&mut self, other: &Self) -> Result<(), H4ReportError> {
        self.entries = self
            .entries
            .checked_add(other.entries)
            .ok_or(H4ReportError::Overflow)?;
        self.closed_round_trips = self
            .closed_round_trips
            .checked_add(other.closed_round_trips)
            .ok_or(H4ReportError::Overflow)?;
        self.strategy_round_trips = self
            .strategy_round_trips
            .checked_add(other.strategy_round_trips)
            .ok_or(H4ReportError::Overflow)?;
        self.forced_round_trips = self
            .forced_round_trips
            .checked_add(other.forced_round_trips)
            .ok_or(H4ReportError::Overflow)?;
        self.cash = self.cash.checked_add(other.cash)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct H4WindowDiagnostics {
    pub scored_start_us: i64,
    pub scored_end_us: i64,
    pub fee_bps: u32,
    pub slippage_bps: u32,
    pub cash: CashTotals,
    pub crowded_long_fades: SideTotals,
    pub crowded_short_fades: SideTotals,
    pub funding_test: FundingTest,
    pub events: Vec<EventRecord>,
    pub episodes: Vec<EpisodeRecord>,
    pub observations: Vec<ObservationRecord>,
    pub trades: Vec<TradeRecord>,
    pub entry_drops: Vec<DropRecord>,
}

fn fill_cash(fill: &Fill) -> Result<i128, H4ReportError> {
    let product = i128::from(fill.price.ticks())
        .checked_mul(i128::from(fill.qty.ticks()))
        .ok_or(H4ReportError::Overflow)?;
    let notional = match fill.side {
        Side::Buy => add(product, SCALE - 1)? / SCALE,
        Side::Sell => product / SCALE,
    };
    let signed = match fill.side {
        Side::Buy => notional.checked_neg().ok_or(H4ReportError::Overflow)?,
        Side::Sell => notional,
    };
    signed
        .checked_sub(i128::from(fill.fee.0))
        .ok_or(H4ReportError::Overflow)
}

/// Called while the complete native report still owns its raw streams.
pub fn reduce_window(
    initial: Cash,
    report: &Report,
) -> Result<Option<H4WindowDiagnostics>, H4ReportError> {
    let Some(trace) = report.h4.as_ref() else {
        return Ok(None);
    };
    let start = trace.scored.start.micros();
    let end = trace.scored.end.micros();
    if start >= end {
        return Err(H4ReportError::Scope);
    }
    let scored = |at: i64| start <= at && at < end;
    let mut events = BTreeMap::new();
    let mut observations = Vec::new();
    let mut seen = BTreeSet::new();
    for observation in &trace.observations {
        let at = observation.observed_at.micros().micros();
        let record = event_record(&mut events, observation.event)?;
        let in_window = scored(at);
        let (phase, kind) = match observation.kind {
            H4ObservationKind::Tail => {
                let cutoff = record
                    .origin
                    .settlement_us
                    .checked_add(CONFIRM_US)
                    .ok_or(H4ReportError::Overflow)?;
                record.late_tail_observation |= at > cutoff;
                record.scored_tail |= in_window && at <= cutoff;
                (0, ObservationKind::Tail)
            }
            H4ObservationKind::Confirmation { confirmed } => {
                if in_window {
                    record.scored_confirmation = Some(confirmed);
                }
                (
                    1,
                    if confirmed {
                        ObservationKind::ConfirmationTrue
                    } else {
                        ObservationKind::ConfirmationFalse
                    },
                )
            }
            H4ObservationKind::ConfirmationUnavailable => {
                record.scored_confirmation_unavailable |= in_window;
                (1, ObservationKind::ConfirmationUnavailable)
            }
            H4ObservationKind::OccupiedSkip => {
                if in_window {
                    increment(&mut record.scored_occupied_skips)?;
                }
                (2, ObservationKind::OccupiedSkip)
            }
        };
        if !seen.insert((record.origin.settlement_us, phase)) {
            return Err(H4ReportError::Observation);
        }
        observations.push(ObservationRecord {
            origin: record.origin,
            observed_at_us: at,
            scored: in_window,
            kind,
        });
    }
    let mut forced = BTreeMap::new();
    for exit in &report.forced_exits {
        let invalid_time = match exit.reason {
            ForcedExitReason::Terminal => {
                exit.filled_at.micros()
                    != end.checked_sub(60_000_000).ok_or(H4ReportError::Overflow)?
            }
            ForcedExitReason::Ruin => exit.trigger >= exit.filled_at,
        };
        if invalid_time
            || !scored(exit.filled_at.micros())
            || exit.trigger > exit.filled_at
            || forced.insert(exit.filled_at.micros(), *exit).is_some()
        {
            return Err(H4ReportError::ForcedExit);
        }
    }
    if u64::try_from(forced.len()).map_err(|_| H4ReportError::Overflow)? != report.n_forced_exits {
        return Err(H4ReportError::ForcedExit);
    }
    let mut sides = [SideTotals::default(), SideTotals::default()];
    let mut trades = Vec::new();
    let mut all_fills = Vec::new();
    let mut all_funding = Vec::new();
    let mut raw_trades = Vec::new();
    for raw in &trace.trades {
        let trip = raw.trade;
        let close = trip.close_ts.ok_or(H4ReportError::OpenTrade)?;
        let origin = Origin::from(raw.event);
        if trip.qty.ticks() <= 0
            || !scored(trip.open_ts.micros())
            || !scored(close.micros())
            || close < trip.open_ts
            || trip.side != origin.crowded_side.expected_fill()
        {
            return Err(H4ReportError::Trade);
        }
        let confirmation = origin
            .settlement_us
            .checked_add(CONFIRM_US)
            .ok_or(H4ReportError::Overflow)?;
        if trip.open_ts.micros() < confirmation {
            return Err(H4ReportError::Trade);
        }
        let scheduled = raw
            .hold
            .exit_decision(trip.open_ts)
            .map_err(|_| H4ReportError::Overflow)?
            .micros()
            .micros();
        let forced_exit = forced.remove(&close.micros());
        let reason = forced_exit.map(|exit| match exit.reason {
            ForcedExitReason::Terminal => ForcedReason::Terminal,
            ForcedExitReason::Ruin => ForcedReason::Ruin,
        });
        if reason.is_none() && close.micros() < scheduled {
            return Err(H4ReportError::Trade);
        }
        let first = raw.fills.first().ok_or(H4ReportError::Trade)?;
        let last = raw.fills.last().ok_or(H4ReportError::Trade)?;
        if first.ts != trip.open_ts
            || first.side != trip.side
            || first.qty != trip.qty
            || last.ts != close
        {
            return Err(H4ReportError::Trade);
        }
        let mut cash = 0i128;
        let mut fees = 0i128;
        let mut position = 0i128;
        for fill in &raw.fills {
            if fill.qty.ticks() <= 0 || fill.fee.0 < 0 || fill.ts < trip.open_ts || fill.ts > close
            {
                return Err(H4ReportError::Cashflow);
            }
            cash = add(cash, fill_cash(fill)?)?;
            fees = add(fees, i128::from(fill.fee.0))?;
            let qty = i128::from(fill.qty.ticks());
            position = add(
                position,
                match fill.side {
                    Side::Buy => qty,
                    Side::Sell => qty.checked_neg().ok_or(H4ReportError::Overflow)?,
                },
            )?;
            all_fills.push(*fill);
        }
        if position != 0 {
            return Err(H4ReportError::OpenTrade);
        }
        let mut funding = 0i128;
        for event in &raw.funding {
            if event.position.0 == 0
                || event.position.0.checked_abs() != Some(trip.qty.ticks())
                || event.ts <= trip.open_ts
                || event.ts > close
                || (event.position.0 > 0) != (trip.side == Side::Buy)
            {
                return Err(H4ReportError::Cashflow);
            }
            funding = add(funding, i128::from(event.amount.0))?;
            all_funding.push(*event);
        }
        if add(cash, funding)? != i128::from(trip.pnl.0) {
            return Err(H4ReportError::Cashflow);
        }
        let totals = CashTotals::new(
            i128::from(trip.pnl.0),
            fees,
            i128::from(raw.slippage_cost.0),
            funding,
        )?;
        let side: &mut SideTotals = &mut sides[origin.crowded_side.index()];
        increment(&mut side.entries)?;
        increment(&mut side.closed_round_trips)?;
        if reason.is_some() {
            increment(&mut side.forced_round_trips)?;
        } else {
            increment(&mut side.strategy_round_trips)?;
        }
        side.cash = side.cash.checked_add(totals)?;
        let event = event_record(&mut events, raw.event)?;
        increment(&mut event.scored_entries)?;
        if event.scored_entries > 1 || event.scored_confirmation != Some(true) {
            return Err(H4ReportError::Origin);
        }
        trades.push(TradeRecord {
            origin,
            open_us: trip.open_ts.micros(),
            close_us: close.micros(),
            actual_side: trip.side.to_string(),
            qty_ticks: trip.qty.ticks().to_string(),
            requested_hold_us: raw.hold.micros(),
            scheduled_exit_decision_us: scheduled,
            forced_reason: reason,
            forced_trigger_us: forced_exit.map(|exit| exit.trigger.micros()),
            cash: totals,
            fills: raw.fills.iter().map(FillRecord::from).collect(),
            funding: raw
                .funding
                .iter()
                .map(|event| FundingRecord {
                    at_us: event.ts.micros(),
                    rate_ticks: event.rate.ticks().to_string(),
                    mark_ticks: event.mark.ticks().to_string(),
                    position_ticks: event.position.0.to_string(),
                    amount_ticks: event.amount.0.to_string(),
                })
                .collect(),
        });
        raw_trades.push(trip);
    }
    if !forced.is_empty() || all_fills != report.fills || raw_trades != report.trades {
        return Err(H4ReportError::Reconciliation);
    }
    if report
        .funding
        .iter()
        .any(|event| event.position.0 == 0 && event.amount.0 != 0)
        || all_funding
            != report
                .funding
                .iter()
                .filter(|event| event.position.0 != 0)
                .copied()
                .collect::<Vec<_>>()
    {
        return Err(H4ReportError::Reconciliation);
    }
    let cash = sides[0].cash.checked_add(sides[1].cash)?;
    let net = i128::from(report.equity_end().ok_or(H4ReportError::Reconciliation)?.0)
        .checked_sub(i128::from(initial.0))
        .ok_or(H4ReportError::Overflow)?;
    if cash.net_ticks != net
        || cash.fees_ticks != i128::from(report.metrics.fees_paid.0)
        || cash.funding_ticks != i128::from(report.metrics.funding_net.0)
        || cash.slippage_ticks != i128::from(report.slippage_cost.0)
    {
        return Err(H4ReportError::Reconciliation);
    }
    let mut entry_drops = Vec::new();
    let mut dropped_events = BTreeSet::new();
    for drop in &trace.entry_drops {
        if !scored(drop.at.micros()) {
            return Err(H4ReportError::Scope);
        }
        let event = event_record(&mut events, drop.event)?;
        if event.scored_entries > 0 || !dropped_events.insert(event.origin.settlement_us) {
            return Err(H4ReportError::Origin);
        }
        entry_drops.push(DropRecord {
            origin: Origin::from(drop.event),
            at_us: drop.at.micros(),
            reason: match drop.reason {
                H4EntryDropReason::DevelopmentBoundary => DropReason::DevelopmentBoundary,
                H4EntryDropReason::ZeroLots => DropReason::ZeroLots,
                H4EntryDropReason::Terminal => DropReason::Terminal,
            },
        });
    }
    let mut episodes = BTreeMap::new();
    for event in events.values() {
        if !event.scored_tail
            && event.scored_confirmation.is_none()
            && !event.scored_confirmation_unavailable
            && event.scored_occupied_skips == 0
            && event.scored_entries == 0
        {
            continue;
        }
        let origin = event.origin;
        let episode = episodes
            .entry((origin.episode_us, origin.crowded_side))
            .or_insert_with(|| EpisodeRecord {
                episode_us: origin.episode_us,
                crowded_side: origin.crowded_side,
                gap_boundary: false,
                starts_before_scored_window: origin.episode_us < start,
                scored_tail_settlements_us: BTreeSet::new(),
                scored_confirmed_settlements_us: BTreeSet::new(),
                entered_settlements_us: BTreeSet::new(),
            });
        episode.gap_boundary |= origin.gap_boundary;
        if event.scored_tail {
            episode
                .scored_tail_settlements_us
                .insert(origin.settlement_us);
        }
        if event.scored_confirmation == Some(true) {
            episode
                .scored_confirmed_settlements_us
                .insert(origin.settlement_us);
        }
        if event.scored_entries > 0 {
            episode.entered_settlements_us.insert(origin.settlement_us);
        }
    }
    Ok(Some(H4WindowDiagnostics {
        scored_start_us: start,
        scored_end_us: end,
        fee_bps: report.costs.taker_fee.0,
        slippage_bps: report.costs.slippage.0,
        cash,
        crowded_long_fades: sides[0].clone(),
        crowded_short_fades: sides[1].clone(),
        funding_test: cash.funding_test()?,
        events: events.into_values().collect(),
        episodes: episodes.into_values().collect(),
        observations,
        trades,
        entry_drops,
    }))
}

/// A selected test fold. None/None means the declared FLAT selection.
pub struct SelectedFold<'a> {
    pub fold: u32,
    pub selected_config_id: &'a str,
    pub window: Option<&'a H4WindowDiagnostics>,
    pub daily_returns: Option<&'a [i64]>,
    pub days: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EpisodeKey {
    pub episode_us: i64,
    pub crowded_side: CrowdSide,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FoldContribution {
    pub fold: u32,
    pub selected_config_id: String,
    pub days: usize,
    #[serde(serialize_with = "integer")]
    pub opening_compounded_wealth_ticks: i128,
    #[serde(serialize_with = "integer")]
    pub closing_compounded_wealth_ticks: i128,
    #[serde(serialize_with = "integer")]
    pub compounded_increment_ticks: i128,
    #[serde(serialize_with = "integer")]
    pub reset_cash_net_ticks: i128,
    pub compounded_gain_share: Option<Fraction>,
    pub reset_cash_share_sensitivity: Option<Fraction>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct H4SelectedDiagnostics {
    pub money_scale: &'static str,
    pub accounting_basis: &'static str,
    pub episode_independence_claimed: bool,
    pub days: usize,
    pub cash: CashTotals,
    pub crowded_long_fades: SideTotals,
    pub crowded_short_fades: SideTotals,
    pub funding_test: FundingTest,
    pub crowded_long_reset_cash_share: Option<Fraction>,
    pub crowded_short_reset_cash_share: Option<Fraction>,
    pub original_side_falsifier: Option<bool>,
    pub original_fold_falsifier_compounded: Option<bool>,
    pub fold_falsifier_reset_cash_sensitivity: Option<bool>,
    pub tail_episode_keys: BTreeSet<EpisodeKey>,
    pub confirmed_episode_keys: BTreeSet<EpisodeKey>,
    pub entered_episode_keys: BTreeSet<EpisodeKey>,
    pub gap_boundary_episode_keys: BTreeSet<EpisodeKey>,
    pub folds: Vec<FoldContribution>,
}

fn exceeds_share(
    numerator: i128,
    denominator: i128,
    percent: i128,
) -> Result<Option<bool>, H4ReportError> {
    if denominator <= 0 {
        return Ok(None);
    }
    let left = numerator.checked_mul(100).ok_or(H4ReportError::Overflow)?;
    let right = denominator
        .checked_mul(percent)
        .ok_or(H4ReportError::Overflow)?;
    Ok(Some(left > right))
}

/// Compounds the same integer daily returns as the native family JSON writer.
/// Reset-window cash is never divided by compounded wealth.
pub fn aggregate_selected(
    initial: Cash,
    selected: &[SelectedFold<'_>],
) -> Result<H4SelectedDiagnostics, H4ReportError> {
    if initial.0 <= 0 || selected.is_empty() {
        return Err(H4ReportError::Scope);
    }
    let mut level = initial.0;
    let mut cash = CashTotals::default();
    let mut sides = [SideTotals::default(), SideTotals::default()];
    let mut folds = Vec::new();
    let mut days = 0usize;
    let mut tail_episode_keys = BTreeSet::new();
    let mut confirmed_episode_keys = BTreeSet::new();
    let mut entered_episode_keys = BTreeSet::new();
    let mut gap_boundary_episode_keys = BTreeSet::new();
    let mut seen_folds = BTreeSet::new();
    for fold in selected {
        if fold.days == 0 || !seen_folds.insert(fold.fold) {
            return Err(H4ReportError::Scope);
        }
        days = days.checked_add(fold.days).ok_or(H4ReportError::Overflow)?;
        let opening = i128::from(level);
        let reset_net = match (fold.window, fold.daily_returns) {
            (None, None) if fold.selected_config_id == "FLAT" => 0,
            (Some(window), Some(returns))
                if fold.selected_config_id != "FLAT" && returns.len() == fold.days =>
            {
                cash = cash.checked_add(window.cash)?;
                sides[0].checked_add(&window.crowded_long_fades)?;
                sides[1].checked_add(&window.crowded_short_fades)?;
                for ret in returns {
                    let delta = i128::from(level)
                        .checked_mul(i128::from(*ret))
                        .ok_or(H4ReportError::Overflow)?
                        / SCALE;
                    level = i64::try_from(add(i128::from(level), delta)?)
                        .map_err(|_| H4ReportError::Overflow)?;
                }
                for episode in &window.episodes {
                    let key = EpisodeKey {
                        episode_us: episode.episode_us,
                        crowded_side: episode.crowded_side,
                    };
                    if !episode.scored_tail_settlements_us.is_empty() {
                        tail_episode_keys.insert(key);
                    }
                    if !episode.scored_confirmed_settlements_us.is_empty() {
                        confirmed_episode_keys.insert(key);
                    }
                    if !episode.entered_settlements_us.is_empty() {
                        entered_episode_keys.insert(key);
                    }
                    if episode.gap_boundary {
                        gap_boundary_episode_keys.insert(key);
                    }
                }
                window.cash.net_ticks
            }
            _ => return Err(H4ReportError::MissingWindow),
        };
        let closing = i128::from(level);
        folds.push(FoldContribution {
            fold: fold.fold,
            selected_config_id: fold.selected_config_id.to_owned(),
            days: fold.days,
            opening_compounded_wealth_ticks: opening,
            closing_compounded_wealth_ticks: closing,
            compounded_increment_ticks: closing
                .checked_sub(opening)
                .ok_or(H4ReportError::Overflow)?,
            reset_cash_net_ticks: reset_net,
            compounded_gain_share: None,
            reset_cash_share_sensitivity: None,
        });
    }
    if sides[0].cash.checked_add(sides[1].cash)? != cash {
        return Err(H4ReportError::Reconciliation);
    }
    let gain = i128::from(level)
        .checked_sub(i128::from(initial.0))
        .ok_or(H4ReportError::Overflow)?;
    let mut compound_flag = (gain > 0).then_some(false);
    let mut reset_flag = (cash.net_ticks > 0).then_some(false);
    for fold in &mut folds {
        fold.compounded_gain_share =
            Fraction::positive_denominator(fold.compounded_increment_ticks, gain);
        fold.reset_cash_share_sensitivity =
            Fraction::positive_denominator(fold.reset_cash_net_ticks, cash.net_ticks);
        if exceeds_share(fold.compounded_increment_ticks, gain, 50)? == Some(true) {
            compound_flag = Some(true);
        }
        if exceeds_share(fold.reset_cash_net_ticks, cash.net_ticks, 50)? == Some(true) {
            reset_flag = Some(true);
        }
    }
    let long = sides[0].cash.net_ticks;
    let short = sides[1].cash.net_ticks;
    let side_flag = if cash.net_ticks > 0 {
        Some(
            (exceeds_share(long, cash.net_ticks, 80)? == Some(true) && short < 0)
                || (exceeds_share(short, cash.net_ticks, 80)? == Some(true) && long < 0),
        )
    } else {
        None
    };
    Ok(H4SelectedDiagnostics {
        money_scale: "100000000", accounting_basis: "actual-quantity reset-fold cash; fold concentration uses daily truncated compounded wealth separately",
        episode_independence_claimed: false, days, cash,
        crowded_long_fades: sides[0].clone(), crowded_short_fades: sides[1].clone(), funding_test: cash.funding_test()?,
        crowded_long_reset_cash_share: Fraction::positive_denominator(long, cash.net_ticks),
        crowded_short_reset_cash_share: Fraction::positive_denominator(short, cash.net_ticks),
        original_side_falsifier: side_flag, original_fold_falsifier_compounded: compound_flag,
        fold_falsifier_reset_cash_sensitivity: reset_flag,
        tail_episode_keys, confirmed_episode_keys, entered_episode_keys, gap_boundary_episode_keys, folds,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn raw_fixture(reason: Option<ForcedExitReason>) -> (Cash, Report) {
        use crate::engine::UtcRange;
        use crate::report::{ForcedExit, H4Trace, H4Trade, Metrics, RoundTrip};
        use parangsae_core::{Price, Qty, Ratio, UtcMicros};
        use parangsae_sim::{Costs, FeeBps, FundingEvent, PositionQty, SlippageBps};
        use parangsae_strategies::{ActualHold, H4Observation};
        let ts = UtcMicros::from_micros;
        let price = |ticks| Price::from_ticks(ticks).unwrap();
        let ratio = Ratio::from_ticks;
        let event = H4Event {
            settlement: ts(0),
            episode: ts(0),
            crowd_side: PositionSide::Long,
            gap_boundary: false,
        };
        let close = if reason == Some(ForcedExitReason::Terminal) {
            431_940_000_000
        } else {
            349_260_000_000
        };
        let qty = Qty::from_ticks(100_000_000).unwrap();
        let fills = vec![
            Fill {
                ts: ts(86_460_000_000),
                side: Side::Sell,
                qty,
                price: price(9_900_000_000),
                fee: Cash(99_000_000),
            },
            Fill {
                ts: ts(close),
                side: Side::Buy,
                qty,
                price: price(8_989_000_000),
                fee: Cash(89_890_000),
            },
        ];
        let funding = vec![FundingEvent {
            ts: ts(172_800_000_000),
            rate: ratio(2_000_000),
            mark: price(10_000_000_000),
            position: PositionQty(-100_000_000),
            amount: Cash(200_000_000),
        }];
        let trade = RoundTrip {
            open_ts: fills[0].ts,
            close_ts: Some(ts(close)),
            side: Side::Sell,
            qty,
            pnl: Cash(922_110_000),
        };
        let forced_exits = reason
            .map(|reason| ForcedExit {
                trigger: ts(if reason == ForcedExitReason::Terminal {
                    close
                } else {
                    close - 1_000
                }),
                filled_at: ts(close),
                reason,
            })
            .into_iter()
            .collect();
        let trace = H4Trace {
            scored: UtcRange {
                start: ts(86_400_000_000),
                end: ts(432_000_000_000),
            },
            observations: vec![
                H4Observation {
                    event,
                    observed_at: parangsae_core::DecisionTime::from_micros(ts(3_600_001_000)),
                    kind: H4ObservationKind::Tail,
                },
                H4Observation {
                    event,
                    observed_at: parangsae_core::DecisionTime::from_micros(ts(86_400_001_000)),
                    kind: H4ObservationKind::Confirmation { confirmed: true },
                },
            ],
            trades: vec![H4Trade {
                trade,
                event,
                hold: ActualHold::from_hours(72).unwrap(),
                funding: funding.clone(),
                fills: fills.clone(),
                slippage_cost: Cash(189_000_000),
            }],
            entry_drops: Vec::new(),
        };
        let initial = Cash(100_000_000_000);
        let equities = [
            initial.0 - 199_000_000,
            initial.0 + 1_000_000,
            initial.0 + 1_000_000,
            initial.0 + 922_110_000,
        ];
        let curve: Vec<_> = equities
            .into_iter()
            .enumerate()
            .map(|(i, value)| (ts((i as i64 + 2) * 86_400_000_000 - 1_000), Cash(value)))
            .collect();
        let report = Report {
            daily_equity: crate::report::daily_equity(&curve),
            equity_curve: curve,
            fills,
            funding,
            trades: vec![trade],
            metrics: Metrics {
                days: 4,
                total_return: ratio(922_110),
                max_drawdown: ratio(199_000),
                sharpe_daily: None,
                sharpe_annual: None,
                turnover: ratio(0),
                fees_paid: Cash(188_890_000),
                funding_net: Cash(200_000_000),
                fills: 2,
                trades: 1,
            },
            costs: Costs {
                taker_fee: FeeBps(100),
                slippage: SlippageBps(100),
            },
            bars: 5_760,
            ruined: None,
            dropped_orders: 0,
            warmup_bars: 0,
            flat_funding_stale: 0,
            flat_funding_slot_missing: 0,
            n_refused_fills: 0,
            slippage_cost: Cash(189_000_000),
            n_rebalances: 2,
            n_sign_changes: 0,
            n_forced_exits: u64::from(reason.is_some()),
            forced_exits,
            h4: Some(trace),
            signal_outputs: Vec::new(),
            overlays: Vec::new(),
            mark_gaps: Vec::new(),
            forced_flat: Vec::new(),
        };
        (initial, report)
    }

    #[test]
    fn raw_trades_reconcile_and_warmup_is_not_a_scored_tail() {
        let (initial, report) = raw_fixture(None);
        let out = reduce_window(initial, &report).unwrap().unwrap();
        assert_eq!(
            out.cash,
            CashTotals::new(922_110_000, 188_890_000, 189_000_000, 200_000_000).unwrap()
        );
        assert_eq!(out.cash.gross_price_ticks, 1_100_000_000);
        assert_eq!(out.crowded_long_fades.strategy_round_trips, 1);
        assert_eq!(out.crowded_short_fades.entries, 0);
        assert!(!out.observations[0].scored);
        assert!(out.episodes[0].scored_tail_settlements_us.is_empty());
        assert_eq!(out.episodes[0].scored_confirmed_settlements_us.len(), 1);
        assert_eq!(out.episodes[0].entered_settlements_us.len(), 1);
        for reason in [ForcedExitReason::Terminal, ForcedExitReason::Ruin] {
            let (initial, report) = raw_fixture(Some(reason));
            let forced = reduce_window(initial, &report).unwrap().unwrap();
            assert_eq!(forced.crowded_long_fades.strategy_round_trips, 0);
            assert_eq!(forced.crowded_long_fades.forced_round_trips, 1);
            assert_eq!(forced.cash, out.cash);
            assert_eq!(
                forced.trades[0].forced_trigger_us,
                Some(report.forced_exits[0].trigger.micros())
            );
        }
    }

    #[test]
    fn unexplained_cash_offset_or_raw_cost_loss_is_refused() {
        let (initial, mut report) = raw_fixture(None);
        report.equity_curve.last_mut().unwrap().1 .0 += 1;
        assert_eq!(
            reduce_window(initial, &report),
            Err(H4ReportError::Reconciliation)
        );
        let (initial, mut report) = raw_fixture(None);
        report.fills[0].fee.0 += 1;
        assert_eq!(
            reduce_window(initial, &report),
            Err(H4ReportError::Reconciliation)
        );
        let (initial, mut report) = raw_fixture(None);
        report.h4.as_mut().unwrap().trades[0].slippage_cost.0 += 1;
        assert_eq!(
            reduce_window(initial, &report),
            Err(H4ReportError::Reconciliation)
        );
        let (initial, mut report) = raw_fixture(None);
        report.h4.as_mut().unwrap().trades[0].trade.close_ts = None;
        assert_eq!(
            reduce_window(initial, &report),
            Err(H4ReportError::OpenTrade)
        );
        let (initial, mut report) = raw_fixture(None);
        let trace = report.h4.as_mut().unwrap();
        let drop = crate::report::H4EntryDrop {
            event: trace.trades[0].event,
            at: trace.trades[0].trade.open_ts,
            reason: H4EntryDropReason::ZeroLots,
        };
        trace.entry_drops.push(drop);
        assert_eq!(reduce_window(initial, &report), Err(H4ReportError::Origin));
        report.h4.as_mut().unwrap().trades.clear();
        report.trades.clear();
        report.fills.clear();
        report.funding.clear();
        let metrics: &mut crate::report::Metrics = &mut report.metrics;
        metrics.fees_paid = Cash(0);
        metrics.funding_net = Cash(0);
        report.slippage_cost = Cash(0);
        for (_, value) in &mut report.equity_curve {
            *value = initial;
        }
        report.h4.as_mut().unwrap().entry_drops.push(drop);
        assert_eq!(reduce_window(initial, &report), Err(H4ReportError::Origin));
    }

    #[test]
    fn funding_flag_preserves_literal_comparison_and_undefined_ratio() {
        let positive = CashTotals::new(100, 5, 2, 20).unwrap();
        assert_eq!(positive.gross_price_ticks, 87);
        assert_eq!(
            positive.funding_test().unwrap().funding_over_gross,
            Fraction::positive_denominator(20, 87)
        );
        assert!(!positive.funding_test().unwrap().original_literal_flag);
        let negative = CashTotals::new(-100, 5, 2, -5).unwrap();
        assert_eq!(negative.gross_price_ticks, -88);
        assert!(negative.funding_test().unwrap().original_literal_flag);
        assert_eq!(negative.funding_test().unwrap().funding_over_gross, None);
        let zero = CashTotals::default().funding_test().unwrap();
        assert!(!zero.original_literal_flag);
        assert_eq!(zero.funding_over_gross, None);
    }

    #[test]
    fn report_percentages_are_strict_and_overflow_is_an_error() {
        assert_eq!(exceeds_share(80, 100, 80), Ok(Some(false)));
        assert_eq!(exceeds_share(81, 100, 80), Ok(Some(true)));
        assert_eq!(exceeds_share(50, 100, 50), Ok(Some(false)));
        assert_eq!(exceeds_share(51, 100, 50), Ok(Some(true)));
        assert_eq!(exceeds_share(1, 0, 50), Ok(None));
        assert_eq!(
            exceeds_share(i128::MAX, 1, 50),
            Err(H4ReportError::Overflow)
        );
        assert_eq!(
            CashTotals::new(i128::MAX, 1, 0, 0),
            Err(H4ReportError::Overflow)
        );
        assert_eq!(CashTotals::new(0, -1, 0, 0), Err(H4ReportError::Cashflow));
    }

    fn window(net: i128) -> H4WindowDiagnostics {
        let cash = CashTotals::new(net, 0, 0, 0).unwrap();
        H4WindowDiagnostics {
            scored_start_us: 0,
            scored_end_us: 86_400_000_000,
            fee_bps: 5,
            slippage_bps: 2,
            cash,
            crowded_long_fades: SideTotals {
                cash,
                ..SideTotals::default()
            },
            crowded_short_fades: SideTotals::default(),
            funding_test: cash.funding_test().unwrap(),
            events: Vec::new(),
            episodes: Vec::new(),
            observations: Vec::new(),
            trades: Vec::new(),
            entry_drops: Vec::new(),
        }
    }

    #[test]
    fn compounded_concentration_and_flat_duration_are_separate_from_reset_cash() {
        let first = window(2_000);
        let second = window(-1_000);
        let out = aggregate_selected(
            Cash(10_000),
            &[
                SelectedFold {
                    fold: 1,
                    selected_config_id: "a",
                    window: Some(&first),
                    daily_returns: Some(&[20_000_000]),
                    days: 1,
                },
                SelectedFold {
                    fold: 2,
                    selected_config_id: "FLAT",
                    window: None,
                    daily_returns: None,
                    days: 4,
                },
                SelectedFold {
                    fold: 3,
                    selected_config_id: "b",
                    window: Some(&second),
                    daily_returns: Some(&[-10_000_000]),
                    days: 1,
                },
            ],
        )
        .unwrap();
        assert_eq!(out.days, 6);
        assert_eq!(out.cash.net_ticks, 1_000);
        assert_eq!(out.folds[0].compounded_increment_ticks, 2_000);
        assert_eq!(out.folds[1].compounded_increment_ticks, 0);
        assert_eq!(out.folds[2].compounded_increment_ticks, -1_200);
        assert_eq!(
            out.folds[0].compounded_gain_share,
            Fraction::positive_denominator(2_000, 800)
        );
        assert_eq!(
            out.folds[0].reset_cash_share_sensitivity,
            Fraction::positive_denominator(2_000, 1_000)
        );
        let json = serde_json::to_value(out).unwrap();
        assert_eq!(json["cash"]["net_ticks"], "1000");
        assert_eq!(json["episode_independence_claimed"], false);
    }

    #[test]
    fn fractional_loss_truncates_the_delta_before_adding_to_wealth() {
        let first = window(10);
        let second = window(-9);
        let out = aggregate_selected(
            Cash(101),
            &[
                SelectedFold {
                    fold: 1,
                    selected_config_id: "a",
                    window: Some(&first),
                    daily_returns: Some(&[10_000_000]),
                    days: 1,
                },
                SelectedFold {
                    fold: 2,
                    selected_config_id: "b",
                    window: Some(&second),
                    daily_returns: Some(&[-9_000_000]),
                    days: 1,
                },
            ],
        )
        .unwrap();
        assert_eq!(out.folds[0].closing_compounded_wealth_ticks, 111);
        assert_eq!(out.folds[1].closing_compounded_wealth_ticks, 102);
        assert_eq!(out.folds[1].compounded_increment_ticks, -9);
    }
}
