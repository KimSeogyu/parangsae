use std::collections::BTreeSet;

use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Qty, ReceivedTs, Timestamps, UtcMicros,
};
use parangsae_execution::carry::*;
use parangsae_execution::{
    ApprovalTiming, ExecutionConfig, MaxAttempts, Mono, NotFoundAuthority, PositiveMicros,
};
use parangsae_sim::carry::{Allocation, CarryAccountState, FeeCharge};
use parangsae_sim::Cash;
use parangsae_strategies::c1::{
    classify_c1_key, evaluate_c1_funding, C1FeatureError, C1FundingEvaluation, C1FundingFeature,
    C1History, C1KeyClass, C1MaxHold, C1RateProjection, C1SettledRate,
};
use sgd_owned_c1opportunitycontrol_50dd0338899d::{C1ControlMachine, Event, FiniteValue};

use crate::c1_source::{
    C1Instrument, C1Marks, C1PairRefs, C1SourceView, ExecutionState, SourceError, SourceHealth,
};
use crate::{daily_return_tick, UtcDate};

use super::report::{stage, ActorRecord, DailyCapital, ExitReason, PairRecord, ReplayResult};
use super::sizing::{
    forecast_above_cost, quote_entry, EntryPrices, EntryQuote, EntrySizing, PriceBand, Scenario,
    SizingError,
};
use super::venue::{consider, Candidate, OrderTerms, RawExecution, VenueError};

#[cfg(test)]
#[path = "actor_cases.rs"]
mod tests;

const MS: i64 = 1_000;
const MINUTE: i64 = 60_000_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const SLOT: i64 = 8 * HOUR;
const LEGGING: i64 = 600 * 1_000_000;
const C0: i64 = 100_000_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReplayError {
    Clock,
    Capacity,
    Arithmetic,
    Control(&'static str),
    Feature(C1FeatureError),
    Source { at: i64, error: SourceError },
    Native { at: i64, error: CarryError },
    Sizing(SizingError),
    Venue(VenueError),
    UnknownAtDeadline { at: i64, stage: &'static str },
    UnclosedBoundary { at: i64 },
    UnknownDailyLiability { at: i64 },
}

impl From<SizingError> for ReplayError {
    fn from(value: SizingError) -> Self {
        Self::Sizing(value)
    }
}
impl From<VenueError> for ReplayError {
    fn from(value: VenueError) -> Self {
        Self::Venue(value)
    }
}
impl From<C1FeatureError> for ReplayError {
    fn from(value: C1FeatureError) -> Self {
        Self::Feature(value)
    }
}

#[derive(Debug)]
pub(super) struct ReplayFailure {
    pub error: ReplayError,
    pub stop_error: Option<ReplayError>,
    pub prefix: ReplayResult,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PolicyWindow {
    pub start: i64,
    pub end: i64,
    pub hold: C1MaxHold,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug)]
enum NativeKeyCommit {
    Keep,
    Advance(C1SettledRate),
}

struct PreparedC1Event {
    event: Event,
    key: NativeKeyCommit,
}

struct PreparedEntry {
    at: i64,
    feature: C1FundingFeature,
    projection: C1RateProjection,
    quote: EntryQuote,
    refs: C1PairRefs,
    account: CarryAccountState,
    supported: bool,
    opening_deadline: i64,
}

#[derive(Clone, Copy, Debug)]
struct PendingVenue {
    terms: OrderTerms,
    next_open: i64,
}

/// One account and one generated control owner for the entire replay.
pub(super) struct C1Replay<'a> {
    source: &'a C1SourceView,
    engine: Option<CarryEngine>,
    observed: Option<CarrySnapshot>,
    control: C1ControlMachine,
    committed: Option<C1SettledRate>,
    entry_epoch_start: i64,
    scenario: Scenario,
    policies: Vec<PolicyWindow>,
    policy: usize,
    pending: Option<PendingVenue>,
    follow_up: Option<i64>,
    active_pair: Option<usize>,
    pairs: Vec<PairRecord>,
    records: Vec<ActorRecord>,
    daily: Vec<DailyCapital>,
    events: BTreeSet<i64>,
    start: i64,
    end: i64,
    record_cap: usize,
    pair_cap: usize,
    risk_ticks: u64,
    previous_equity: i64,
    last_at: i64,
}

impl<'a> C1Replay<'a> {
    pub(super) fn new(
        source: &'a C1SourceView,
        scenario: Scenario,
        policies: Vec<PolicyWindow>,
    ) -> Result<Self, ReplayError> {
        let first = *policies.first().ok_or(ReplayError::Clock)?;
        let last = *policies.last().ok_or(ReplayError::Clock)?;
        if first.start.rem_euclid(DAY) != 0
            || last.end.rem_euclid(DAY) != 0
            || policies.iter().any(|p| {
                p.start >= p.end || p.start.rem_euclid(DAY) != 0 || p.end.rem_euclid(DAY) != 0
            })
            || policies.windows(2).any(|w| w[0].end != w[1].start)
        {
            return Err(ReplayError::Clock);
        }
        let days =
            usize::try_from((last.end - first.start) / DAY).map_err(|_| ReplayError::Arithmetic)?;
        let pair_cap = days.checked_mul(3).ok_or(ReplayError::Arithmetic)?;
        let record_cap = pair_cap
            .checked_mul(96)
            .and_then(|n| n.checked_add(64))
            .ok_or(ReplayError::Arithmetic)?;
        Ok(Self {
            source,
            engine: None,
            observed: None,
            control: C1ControlMachine::new(),
            committed: None,
            entry_epoch_start: first.start,
            scenario,
            policies,
            policy: 0,
            pending: None,
            follow_up: None,
            active_pair: None,
            pairs: Vec::new(),
            records: Vec::new(),
            daily: Vec::new(),
            events: BTreeSet::new(),
            start: first.start,
            end: last.end,
            record_cap,
            pair_cap,
            risk_ticks: 0,
            previous_equity: C0,
            last_at: first.start,
        })
    }

    pub(super) fn run(mut self) -> Result<ReplayResult, ReplayFailure> {
        let outcome = self.drive();
        let stop_error = if outcome.is_err() && !self.mode("Stopped") {
            self.control_at(self.last_at, Event::Stop, NativeKeyCommit::Keep)
                .err()
        } else {
            None
        };
        let prefix = ReplayResult {
            records: self.records,
            pairs: self.pairs,
            daily: self.daily,
            snapshot: self.observed,
            risk_ticks: self.risk_ticks,
            final_equity: self.previous_equity,
            control: self.control.read(),
        };
        match outcome {
            Ok(()) => Ok(prefix),
            Err(error) => Err(ReplayFailure {
                error,
                stop_error,
                prefix,
            }),
        }
    }

    fn drive(&mut self) -> Result<(), ReplayError> {
        self.schedule(self.start)?;
        self.schedule(add(self.start, MS)?)?;
        self.control_at(self.start, Event::RequestEpoch, NativeKeyCommit::Keep)?;
        self.control_at(self.start, Event::NewEpoch, NativeKeyCommit::Keep)?;
        while let Some(at) = self.events.pop_first() {
            if at > add(self.end, MS)? {
                break;
            }
            self.last_at = at;
            if at.rem_euclid(MINUTE) == 0 && at < self.end {
                self.schedule(add(at, MINUTE)?)?;
                self.schedule(add(add(at, MINUTE)?, MS)?)?;
            }
            self.check_deadline(at)?;
            self.deliver_due(at)?;
            self.settle_due(at)?;
            self.synchronize(at)?;
            // Value the previous calendar day before any slot-at-end boundary.
            if at - MS > self.start && (at - MS).rem_euclid(DAY) == 0 {
                self.record_day(at)?;
            }
            if at >= self.end {
                if !self.boundary_ready(self.end) {
                    return Err(ReplayError::UnclosedBoundary { at: self.end });
                }
                continue;
            }
            self.activate_policy(at)?;
            if at.rem_euclid(SLOT) == 0 {
                self.capture_boundary(at)?;
            }
            if self.follow_up == Some(at) {
                self.follow_up = None;
                self.progress_pair(at)?;
            }
            let terminal = self.policies[self.policy].end - LEGGING;
            if at == terminal {
                self.begin_exit(at, ExitReason::Terminal)?;
            }
            if at.rem_euclid(MINUTE) == MS {
                self.risk_tick(at)?;
            }
            if at.rem_euclid(HOUR) == MS {
                self.hour(at)?;
            }
            self.check_deadline(at)?;
        }
        if !self.boundary_ready(self.end)
            || self.daily.len()
                != usize::try_from((self.end - self.start) / DAY)
                    .map_err(|_| ReplayError::Arithmetic)?
        {
            return Err(ReplayError::UnclosedBoundary { at: self.end });
        }
        Ok(())
    }

    fn schedule(&mut self, at: i64) -> Result<(), ReplayError> {
        if self.events.len() >= 32 && !self.events.contains(&at) {
            return Err(ReplayError::Capacity);
        }
        self.events.insert(at);
        Ok(())
    }

    fn source<T>(&self, at: i64, value: Result<T, SourceError>) -> Result<T, ReplayError> {
        value.map_err(|error| ReplayError::Source { at, error })
    }

    fn record(&mut self, row: ActorRecord) -> Result<(), ReplayError> {
        if self.records.len() >= self.record_cap {
            return Err(ReplayError::Capacity);
        }
        self.records.push(row);
        Ok(())
    }

    fn control_at(
        &mut self,
        at: i64,
        event: Event,
        key: NativeKeyCommit,
    ) -> Result<(), ReplayError> {
        let prepared = PreparedC1Event { event, key };
        if event == Event::NewEpoch
            && (!self.flat_ready() || self.observed.as_ref().is_some_and(|state| state.halted))
        {
            return Err(ReplayError::Control(
                "epoch requires healthy settled engine",
            ));
        }
        if self.records.len() >= self.record_cap {
            return Err(ReplayError::Capacity);
        }
        let receipt = self
            .control
            .send(prepared.event)
            .map_err(|_| ReplayError::Control("disabled generated transition"))?;
        if let NativeKeyCommit::Advance(value) = prepared.key {
            self.committed = Some(value);
        }
        if prepared.event == Event::NewEpoch {
            self.entry_epoch_start = at;
        }
        self.records.push(ActorRecord::Control {
            at,
            event: prepared.event.model_name(),
            before_node: receipt.before.node_id,
            after_node: receipt.after.node_id,
            revision: receipt.after.revision,
            committed_key: self.committed.map(|rate| rate.key().micros()),
            entry_epoch_start: self.entry_epoch_start,
        });
        Ok(())
    }

    fn mode(&self, expected: &str) -> bool {
        matches!(self.control.read().values.get("mode"), Some(FiniteValue::String(mode)) if mode == expected)
    }

    fn flag(&self, key: &str) -> bool {
        matches!(
            self.control.read().values.get(key),
            Some(FiniteValue::Boolean(true))
        )
    }

    fn flat_ready(&self) -> bool {
        self.pending.is_none()
            && self.observed.as_ref().is_none_or(|snap| {
                snap.account.spot_btc.ticks() == 0
                    && snap.account.perp_short.ticks() == 0
                    && snap.orders.iter().all(|o| o.complete_cut)
                    && snap.funding.iter().all(|f| f.receipt.is_some())
                    && matches!(snap.phase, CarryPhase::Flat | CarryPhase::Closed)
                    && snap.unsupported.is_none()
            })
    }

    fn boundary_ready(&self, boundary: i64) -> bool {
        self.flat_ready()
            && self
                .pairs
                .last()
                .is_none_or(|pair| pair.closed_observed_at.is_some_and(|at| at < boundary))
    }

    fn ensure_engine(&mut self, at: i64) -> Result<(), ReplayError> {
        if self.engine.is_some() {
            return Ok(());
        }
        let marks = self.source(at, self.source.marks_at(decision(at)))?;
        let session = session(marks, self.scenario, self.start, at, self.pair_cap)?;
        let engine =
            CarryEngine::new(session).map_err(|error| ReplayError::Native { at, error })?;
        self.observed = Some(engine.snapshot());
        self.engine = Some(engine);
        Ok(())
    }

    fn command(&mut self, at: i64, action: CarryAction) -> Result<CarryOutcome, ReplayError> {
        self.ensure_engine(at)?;
        let marks = self.source(at, self.source.marks_at(decision(at)))?;
        let now = u64::try_from(at.checked_sub(self.start).ok_or(ReplayError::Arithmetic)?)
            .map_err(|_| ReplayError::Clock)?;
        let engine = self
            .engine
            .as_mut()
            .ok_or(ReplayError::Control("missing engine"))?;
        let outcome = engine.step(&CarryCommand {
            now: Mono::from_micros(now),
            decision: decision(at),
            spot: marks.spot.observation,
            perp: marks.perp_risk.observation,
            action,
        });
        self.observed = Some(engine.snapshot());
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                self.control_at(at, Event::Stop, NativeKeyCommit::Keep)?;
                Err(ReplayError::Native { at, error })
            }
        }
    }

    fn snapshot(&self) -> Result<&CarrySnapshot, ReplayError> {
        self.observed
            .as_ref()
            .ok_or(ReplayError::Control("missing observed engine state"))
    }

    fn synchronize(&mut self, at: i64) -> Result<(), ReplayError> {
        let Some(snap) = &self.observed else {
            return Ok(());
        };
        if let Some(index) = self.active_pair {
            if self.pairs[index].observed_paired_at.is_none() && snap.phase == CarryPhase::Hedged {
                let observed = snap
                    .hedged_at
                    .ok_or(ReplayError::Control("missing matched completion"))?
                    .micros()
                    .micros();
                let physical = snap
                    .fills
                    .iter()
                    .filter(|fill| {
                        fill.source.value().order.pair.to_string() == self.pairs[index].pair
                    })
                    .filter(|fill| {
                        matches!(
                            fill.source.value().order.stage,
                            CarryStage::SpotEntry | CarryStage::PerpEntry
                        )
                    })
                    .map(|fill| fill.source.timestamps().event_ts.micros().micros())
                    .max()
                    .ok_or(ReplayError::Control("missing paired fills"))?;
                let raw = add(
                    observed,
                    self.policies[self.policy]
                        .hold
                        .hours()
                        .checked_mul(HOUR)
                        .ok_or(ReplayError::Arithmetic)?,
                )?;
                let first = add(raw.div_euclid(HOUR) * HOUR, MS)?;
                let max_exit = if first >= raw {
                    first
                } else {
                    add(first, HOUR)?
                };
                self.pairs[index].observed_paired_at = Some(observed);
                self.pairs[index].physical_paired_at = Some(physical);
                self.pairs[index].max_exit_decision = Some(max_exit);
            }
        }
        let unsupported = snap.unsupported.is_some();
        let halted = snap.halted;
        let loss = snap.loss_latched;
        let margin = snap.margin_latched;
        let unpaid = snap.funding.iter().any(|f| f.receipt.is_none());
        let account = snap.account.clone();
        if unsupported {
            self.control_at(at, Event::Stop, NativeKeyCommit::Keep)?;
            return Err(ReplayError::Native {
                at,
                error: CarryError::Unresolved,
            });
        }
        if halted && !self.mode("Stopped") {
            self.control_at(at, Event::Stop, NativeKeyCommit::Keep)?;
            self.record(ActorRecord::RiskStop { at, loss, margin })?;
        }
        if unpaid && !self.flag("fundingPending") {
            self.control_at(at, Event::RecordPendingFunding, NativeKeyCommit::Keep)?;
        } else if !unpaid && self.flag("fundingPending") {
            self.control_at(at, Event::RecordFundingSettled, NativeKeyCommit::Keep)?;
        }
        if self.flat_ready()
            && (self.mode("Busy") || (self.mode("Stopped") && !self.flag("settledIdle")))
        {
            self.control_at(at, Event::AllSettledIdle, NativeKeyCommit::Keep)?;
            if let Some(index) = self.active_pair.take() {
                self.pairs[index].closed_observed_at = Some(at);
                self.record(ActorRecord::PairClosed {
                    at,
                    pair: self.pairs[index].pair.clone(),
                    account: (&account).into(),
                })?;
            }
        }
        if self.flat_ready() && self.flag("epochPending") && !self.mode("Stopped") {
            self.control_at(at, Event::NewEpoch, NativeKeyCommit::Keep)?;
        }
        Ok(())
    }

    fn activate_policy(&mut self, at: i64) -> Result<(), ReplayError> {
        if self.policy + 1 < self.policies.len() && at == self.policies[self.policy + 1].start {
            if !self.boundary_ready(at) {
                return Err(ReplayError::UnclosedBoundary { at });
            }
            self.policy += 1;
            if !self.mode("Stopped") {
                self.control_at(at, Event::RequestEpoch, NativeKeyCommit::Keep)?;
                self.control_at(at, Event::NewEpoch, NativeKeyCommit::Keep)?;
            }
        }
        Ok(())
    }

    fn capture_boundary(&mut self, at: i64) -> Result<(), ReplayError> {
        if self.flat_ready() && (self.mode("Idle") || self.mode("Disabled") || self.mode("Stopped"))
        {
            return self.record(ActorRecord::ZeroExposureBoundary { at });
        }
        let key = u64::try_from(at / SLOT).map_err(|_| ReplayError::Arithmetic)?;
        match self.command(
            at,
            CarryAction::FundingBoundary {
                key,
                effective: UtcMicros::from_micros(at),
            },
        )? {
            CarryOutcome::BoundaryCaptured { short, .. } => {
                self.record(ActorRecord::FundingBoundary {
                    at,
                    key,
                    captured_short: short.ticks(),
                })?;
                self.schedule(add(at, MINUTE)?)?;
                self.synchronize(at)
            }
            _ => Err(ReplayError::Control("unexpected funding boundary outcome")),
        }
    }

    fn settle_due(&mut self, at: i64) -> Result<(), ReplayError> {
        let Some(snap) = &self.observed else {
            return Ok(());
        };
        let due: Vec<_> = snap
            .funding
            .iter()
            .filter(|f| f.receipt.is_none() && f.effective.micros() + MINUTE <= at)
            .cloned()
            .collect();
        for funding in due {
            let evidence =
                self.source(at, self.source.funding_at(funding.effective, decision(at)))?;
            if evidence.modeled_delivery.micros() != at {
                return Err(ReplayError::Source {
                    at,
                    error: SourceError::Invariant("funding delivery clock"),
                });
            }
            let rate = evidence.rate.value().rate();
            let mark = *evidence.mark_open_proxy.observation.value();
            let available = evidence.rate.timestamps().available_ts.micros().max(
                evidence
                    .mark_open_proxy
                    .observation
                    .timestamps()
                    .available_ts
                    .micros(),
            );
            let receipt = Observation::new(
                Timestamps {
                    event_ts: EventTs::from_micros(funding.effective),
                    available_ts: AvailableTs::from_micros(available),
                    received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(at)),
                },
                CarryFundingReceipt {
                    key: funding.key,
                    effective: funding.effective,
                    rate,
                    charge_mark: mark,
                },
            );
            let result = self.command(at, CarryAction::FundingReceipt(receipt))?;
            let CarryOutcome::FundingApplied { amount, .. } = result else {
                return Err(ReplayError::Control("unexpected funding receipt outcome"));
            };
            let account = self.snapshot()?.account.clone();
            self.record(ActorRecord::FundingReceipt {
                at,
                key: funding.key,
                effective: funding.effective.micros(),
                rate: rate.ticks(),
                charge_mark_proxy: mark.ticks(),
                cashflow: amount.0,
                rate_source: (&evidence.rate_source).into(),
                mark_source: (&evidence.mark_open_proxy.source).into(),
                account: (&account).into(),
            })?;
            if self
                .active_pair
                .is_some_and(|index| self.pairs[index].exit_decision.is_some())
            {
                let next = add(at, MS)?;
                self.follow_up = Some(next);
                self.schedule(next)?;
            }
        }
        Ok(())
    }

    fn risk_tick(&mut self, at: i64) -> Result<(), ReplayError> {
        if self.flat_ready() {
            return Ok(());
        }
        self.command(at, CarryAction::Observe)?;
        self.risk_ticks = self
            .risk_ticks
            .checked_add(1)
            .ok_or(ReplayError::Arithmetic)?;
        self.synchronize(at)?;
        if self.mode("Stopped") {
            self.begin_exit(at, ExitReason::Risk)?;
        }
        Ok(())
    }

    fn hour(&mut self, at: i64) -> Result<(), ReplayError> {
        if self.mode("Stopped") && self.flat_ready() {
            return Ok(());
        }
        let frame = self.source(at, self.source.rate_frame_at(decision(at)))?;
        let Some(expected) = frame.expected_latest else {
            return self.record(ActorRecord::Evaluation {
                at,
                key: None,
                disposition: "before_first_funding",
            });
        };
        let evaluated = evaluate_c1_funding(decision(at), expected, &frame.views())?;
        if let Some(index) = self.active_pair {
            if self.pairs[index].exit_decision.is_none() {
                let max_due = self.pairs[index]
                    .max_exit_decision
                    .is_some_and(|max| at >= max);
                let nonpositive = matches!(evaluated, C1FundingEvaluation::Latest { rate, .. } if rate.rate().ticks() <= 0);
                if max_due || nonpositive {
                    self.begin_exit(
                        at,
                        if max_due {
                            ExitReason::MaxHold
                        } else {
                            ExitReason::NonpositiveFunding
                        },
                    )?;
                }
            }
        }
        if self.mode("Stopped") {
            return Ok(());
        }
        let C1FundingEvaluation::Latest { rate, history } = evaluated else {
            if self.flat_ready()
                && frame
                    .expected_latest
                    .is_some_and(|key| self.committed.is_none_or(|old| key > old.key()))
            {
                self.control_at(at, Event::FreshIdle, NativeKeyCommit::Keep)?;
            }
            return self.record(ActorRecord::Evaluation {
                at,
                key: frame.expected_latest.map(UtcMicros::micros),
                disposition: "unavailable_latest",
            });
        };
        let class = classify_c1_key(&rate, self.committed.as_ref())?;
        if class == C1KeyClass::Fresh && rate.key().micros() < self.entry_epoch_start {
            return self.record(ActorRecord::Evaluation {
                at,
                key: Some(rate.key().micros()),
                disposition: "pre_epoch_key",
            });
        }
        if class != C1KeyClass::Fresh {
            if matches!(self.control.read().values.get("evaluation"), Some(FiniteValue::String(value)) if value == "Consumed")
            {
                self.control_at(at, Event::Replay, NativeKeyCommit::Keep)?;
            }
            return Ok(());
        }
        if !self.flat_ready() {
            self.control_at(at, Event::FreshOccupied, NativeKeyCommit::Advance(rate))?;
            return Ok(());
        }
        if at >= self.policies[self.policy].end - LEGGING || !self.policies[self.policy].enabled {
            return Ok(());
        }
        let C1History::Complete(feature) = history else {
            self.control_at(at, Event::FreshIdle, NativeKeyCommit::Keep)?;
            return self.record(ActorRecord::Evaluation {
                at,
                key: Some(rate.key().micros()),
                disposition: "incomplete_history",
            });
        };
        let open = UtcMicros::from_micros(at.div_euclid(HOUR) * HOUR - MINUTE);
        let refs = match self.source.pair_refs_at(open, decision(at)) {
            Ok(refs) => refs,
            Err(SourceError::Unavailable { health, .. }) => {
                self.idle_unavailable(at, rate.key().micros(), health)?;
                return Ok(());
            }
            Err(error) => return Err(ReplayError::Source { at, error }),
        };
        let marks = match self.source.marks_at(decision(at)) {
            Ok(marks) => marks,
            Err(SourceError::Unavailable { health, .. }) => {
                self.idle_unavailable(at, rate.key().micros(), health)?;
                return Ok(());
            }
            Err(error) => return Err(ReplayError::Source { at, error }),
        };
        if self.mode("Disabled") {
            self.control_at(at, Event::EnableIdle, NativeKeyCommit::Keep)?;
        }
        self.ensure_engine(at)?;
        let account = self.snapshot()?.account.clone();
        let quote = quote_entry(
            &account,
            EntryPrices {
                spot_reference: *refs.spot.observation.value(),
                perp_reference: *refs.um_trade.observation.value(),
                spot_risk: *marks.spot.observation.value(),
                perp_risk: *marks.perp_risk.observation.value(),
            },
            self.scenario,
        )?;
        let known_false = rate.rate().ticks() <= 0
            || refs.um_trade.observation.value() < refs.spot.observation.value();
        let projection = feature.project(self.policies[self.policy].hold)?;
        let (event, quote) = match quote {
            EntrySizing::Refused(_) => (Event::EvaluateNoSize, None),
            EntrySizing::Sized(_) if known_false => (Event::EvaluateFalse, None),
            EntrySizing::Sized(quote) => {
                if forecast_above_cost(
                    quote.net_btc,
                    *refs.um_trade.observation.value(),
                    projection.numerator(),
                    projection.denominator(),
                    quote.headline_c4,
                )? {
                    (Event::EvaluateEligible, Some(quote))
                } else {
                    (Event::EvaluateFalse, None)
                }
            }
        };
        let prepared = if let Some(quote) = quote {
            if self.pairs.len() >= self.pair_cap {
                return Err(ReplayError::Capacity);
            }
            let hold = self.policies[self.policy]
                .hold
                .hours()
                .checked_mul(HOUR)
                .ok_or(ReplayError::Arithmetic)?;
            let latest_complete = add(add(add(at, LEGGING)?, hold)?, HOUR + LEGGING)?;
            Some(PreparedEntry {
                at,
                feature,
                projection,
                quote,
                refs,
                account,
                supported: latest_complete < self.end - LEGGING,
                opening_deadline: add(at, LEGGING)?,
            })
        } else {
            None
        };
        self.control_at(at, Event::FreshIdle, NativeKeyCommit::Keep)?;
        self.control_at(at, event, NativeKeyCommit::Advance(rate))?;
        if self.mode("Reserved") {
            self.enter(prepared.ok_or(ReplayError::Control("reserved without prepared entry"))?)?;
        }
        Ok(())
    }

    fn idle_unavailable(
        &mut self,
        at: i64,
        key: i64,
        health: SourceHealth,
    ) -> Result<(), ReplayError> {
        let disposition = match health {
            SourceHealth::Gap | SourceHealth::Quarantined => {
                self.disable_idle(at)?;
                "source_discontinuity"
            }
            SourceHealth::Pending
            | SourceHealth::ZeroCount
            | SourceHealth::TooOld
            | SourceHealth::NonPositive => {
                self.control_at(at, Event::FreshIdle, NativeKeyCommit::Keep)?;
                "source_unavailable"
            }
        };
        self.record(ActorRecord::Evaluation {
            at,
            key: Some(key),
            disposition,
        })
    }

    fn disable_idle(&mut self, at: i64) -> Result<(), ReplayError> {
        if !self.mode("Disabled") {
            self.control_at(at, Event::RequestEpoch, NativeKeyCommit::Keep)?;
            self.control_at(at, Event::NewEpoch, NativeKeyCommit::Keep)?;
            self.control_at(at, Event::DisableIdle, NativeKeyCommit::Keep)?;
        }
        Ok(())
    }

    fn enter(&mut self, prepared: PreparedEntry) -> Result<(), ReplayError> {
        let PreparedEntry {
            at,
            feature,
            projection,
            quote,
            refs,
            account,
            supported,
            opening_deadline,
        } = prepared;
        let rate = feature.latest();
        if !supported {
            self.control_at(at, Event::IntentRefused, NativeKeyCommit::Keep)?;
            return self.record(ActorRecord::Evaluation {
                at,
                key: Some(rate.key().micros()),
                disposition: "development_support_refused",
            });
        }
        let pair = u128::try_from(self.pairs.len() + 1).map_err(|_| ReplayError::Arithmetic)?;
        let outcome = self.command(
            at,
            CarryAction::Plan(CarryPlan {
                pair,
                gross_spot: quote.gross_spot,
                net_btc: quote.net_btc,
                max_spot_price: quote.spot_band.upper,
                max_perp_price: quote.perp_band.upper,
            }),
        )?;
        let CarryOutcome::Planned(id) = outcome else {
            return Err(ReplayError::Control("entry did not plan"));
        };
        let index = self.pairs.len();
        self.pairs.push(PairRecord {
            pair: pair.to_string(),
            origin_key: rate.key().micros(),
            opened_decision: at,
            entry_epoch_start: self.entry_epoch_start,
            opening_deadline,
            physical_paired_at: None,
            observed_paired_at: None,
            max_exit_decision: None,
            exit_reason: None,
            exit_decision: None,
            closing_deadline: None,
            closed_observed_at: None,
            gross_spot: quote.gross_spot.ticks(),
            net_btc: quote.net_btc.ticks(),
            forecast_numerator: projection.numerator().to_string(),
            forecast_denominator: projection.denominator().to_string(),
            headline_c4: quote.headline_c4.0,
            sum9_ticks: feature.sum9_ticks().to_string(),
            latest_rate_ticks: rate.rate().ticks(),
            decision_spot_reference: refs.spot.observation.value().ticks(),
            decision_perp_reference: refs.um_trade.observation.value().ticks(),
            spot_reference_source: (&refs.spot.source).into(),
            perp_reference_source: (&refs.um_trade.source).into(),
            proposal_account: (&account).into(),
        });
        self.active_pair = Some(index);
        self.dispatch(at, id, quote.spot_band, &refs.spot.source)?;
        self.control_at(at, Event::IntentAccepted, NativeKeyCommit::Keep)
    }

    fn dispatch(
        &mut self,
        at: i64,
        id: CarryOrderId,
        band: PriceBand,
        reference: &crate::c1_source::SourceRef,
    ) -> Result<(), ReplayError> {
        if self.pending.is_some() {
            return Err(ReplayError::Control("multiple venue orders"));
        }
        self.command(at, CarryAction::Dispatch(id))?;
        let snap = self.snapshot()?.clone();
        let order = snap
            .orders
            .iter()
            .find(|order| order.id == id)
            .ok_or(ReplayError::Control("missing planned order"))?;
        let index = self
            .active_pair
            .ok_or(ReplayError::Control("missing pair"))?;
        let deadline = self.pairs[index]
            .closing_deadline
            .unwrap_or(self.pairs[index].opening_deadline);
        let fees = self.scenario.fees();
        let slippage = match id.stage {
            CarryStage::SpotEntry | CarryStage::SpotExit => fees.spot_slippage,
            CarryStage::PerpEntry | CarryStage::PerpExit => fees.perp_slippage,
        };
        self.pending = Some(PendingVenue {
            terms: OrderTerms {
                id,
                qty: order.qty,
                native_upper: order.max_price,
                fresh_band: band,
                dispatched: decision(at),
                deadline: decision(deadline),
                slippage,
            },
            next_open: add(at.div_euclid(MINUTE) * MINUTE, MINUTE)?,
        });
        self.schedule(deadline)?;
        self.schedule(add(deadline, MS)?)?;
        self.record(ActorRecord::Order {
            at,
            pair: id.pair.to_string(),
            stage: stage(id.stage),
            qty: order.qty.ticks(),
            lower: band.lower.ticks(),
            upper: band.upper.ticks(),
            native_upper: order.max_price.ticks(),
            deadline,
            reference: reference.into(),
        })
    }

    fn deliver_due(&mut self, at: i64) -> Result<(), ReplayError> {
        let Some(mut pending) = self.pending else {
            return Ok(());
        };
        let leg = match pending.terms.id.stage {
            CarryStage::SpotEntry | CarryStage::SpotExit => C1Instrument::Spot,
            CarryStage::PerpEntry | CarryStage::PerpExit => C1Instrument::Perp,
        };
        loop {
            let source = self.source(
                at,
                self.source.execution_at(
                    leg,
                    UtcMicros::from_micros(pending.next_open),
                    decision(at),
                ),
            )?;
            let (raw, provenance) = match source {
                ExecutionState::Pending { until } => {
                    self.schedule(until.micros())?;
                    return Ok(());
                }
                ExecutionState::Gap { .. } | ExecutionState::Quarantined { .. } => {
                    return Err(ReplayError::UnknownAtDeadline {
                        at,
                        stage: stage(pending.terms.id.stage),
                    })
                }
                ExecutionState::ZeroCount { .. } => {
                    pending.next_open = add(pending.next_open, MINUTE)?;
                    self.pending = Some(pending);
                    continue;
                }
                ExecutionState::Valid {
                    reference_open,
                    count,
                    timestamps,
                    source,
                } => (
                    RawExecution {
                        open: UtcMicros::from_micros(pending.next_open),
                        reference_open,
                        count,
                        source_available: timestamps.available_ts.micros(),
                    },
                    source,
                ),
            };
            match consider(pending.terms, raw, decision(at))? {
                Candidate::Deadline => {
                    return Err(ReplayError::UnknownAtDeadline {
                        at,
                        stage: stage(pending.terms.id.stage),
                    })
                }
                Candidate::Wait(_) => {
                    pending.next_open = add(pending.next_open, MINUTE)?;
                    self.pending = Some(pending);
                }
                Candidate::Filled(fill) => {
                    let outcome = self.command(at, CarryAction::Deliver(fill.report()))?;
                    let CarryOutcome::FillApplied(recorded) = outcome else {
                        return Err(ReplayError::Control("modeled fill not applied"));
                    };
                    if recorded.price != fill.expected_executed {
                        return Err(ReplayError::Control("execution applied slippage twice"));
                    }
                    self.command(at, CarryAction::Reconcile(fill.filled_cut()))?;
                    self.pending = None;
                    let (asset, fee) = match recorded.fee {
                        FeeCharge::Usdt(value) => ("USDT", value.0),
                        FeeCharge::Btc(value) => ("BTC", value.ticks()),
                    };
                    let snap = self.snapshot()?.clone();
                    self.record(ActorRecord::Fill {
                        at,
                        physical: raw.open.micros(),
                        pair: pending.terms.id.pair.to_string(),
                        stage: stage(pending.terms.id.stage),
                        qty: pending.terms.qty.ticks(),
                        raw_reference: raw.reference_open.ticks(),
                        executed: recorded.price.ticks(),
                        fee_asset: asset,
                        fee_ticks: fee,
                        source: (&provenance).into(),
                        account: (&snap.account).into(),
                    })?;
                    let next = add(at, MS)?;
                    self.follow_up = Some(next);
                    self.schedule(next)?;
                    return Ok(());
                }
            }
        }
    }

    fn progress_pair(&mut self, at: i64) -> Result<(), ReplayError> {
        let Some(index) = self.active_pair else {
            return Ok(());
        };
        if self.pending.is_some() {
            return Ok(());
        }
        let snap = self.snapshot()?.clone();
        if self.pairs[index].exit_decision.is_some()
            && !matches!(
                snap.phase,
                CarryPhase::ClosingPerp
                    | CarryPhase::ClosingSpot
                    | CarryPhase::Closed
                    | CarryPhase::Flat
            )
        {
            return self.start_exit(at);
        }
        match snap.phase {
            CarryPhase::OpeningSpot | CarryPhase::OpeningHedge => {
                if self.mode("Stopped") {
                    return self.begin_exit(at, ExitReason::Risk);
                }
                let reference = self.source(
                    at,
                    self.source
                        .decision_ref_at(C1Instrument::Perp, decision(at)),
                )?;
                let band = PriceBand::at(*reference.observation.value())?;
                let plan = snap
                    .pair
                    .as_ref()
                    .ok_or(ReplayError::Control("missing plan"))?;
                let original = plan.max_perp_price;
                let marks = self.source(at, self.source.marks_at(decision(at)))?;
                if band.lower > original
                    || *marks.perp_risk.observation.value() > original
                    || *marks.spot.observation.value() > plan.max_spot_price
                {
                    return self.begin_exit(at, ExitReason::HedgeEnvelopeUnwind);
                }
                let result = self.command(
                    at,
                    CarryAction::PlanHedge {
                        pair: snap
                            .pair
                            .as_ref()
                            .ok_or(ReplayError::Control("missing plan"))?
                            .pair,
                    },
                )?;
                let CarryOutcome::Planned(id) = result else {
                    return Err(ReplayError::Control("hedge did not plan"));
                };
                self.dispatch(at, id, band, &reference.source)?;
            }
            CarryPhase::Hedged => {}
            CarryPhase::ClosingPerp | CarryPhase::ClosingSpot => {
                if snap.account.perp_short.ticks() == 0 && snap.account.spot_btc.ticks() > 0 {
                    let reference = self.source(
                        at,
                        self.source
                            .decision_ref_at(C1Instrument::Spot, decision(at)),
                    )?;
                    let band = PriceBand::at(*reference.observation.value())?;
                    let pair = snap
                        .pair
                        .as_ref()
                        .ok_or(ReplayError::Control("missing plan"))?
                        .pair;
                    let result = self.command(
                        at,
                        CarryAction::PlanSpotExit {
                            pair,
                            max_spot_price: band.upper,
                        },
                    )?;
                    let CarryOutcome::Planned(id) = result else {
                        return Err(ReplayError::Control("spot close did not plan"));
                    };
                    self.dispatch(at, id, band, &reference.source)?;
                }
            }
            CarryPhase::Flat | CarryPhase::Closed => {
                self.synchronize(at)?;
            }
            CarryPhase::Unresolved => {
                return Err(ReplayError::Native {
                    at,
                    error: CarryError::Unresolved,
                })
            }
        }
        Ok(())
    }

    fn begin_exit(&mut self, at: i64, reason: ExitReason) -> Result<(), ReplayError> {
        let Some(index) = self.active_pair else {
            return Ok(());
        };
        if self.pairs[index].exit_decision.is_none() {
            let deadline = if self.pairs[index].observed_paired_at.is_some() {
                add(at, LEGGING)?
            } else {
                self.pairs[index].opening_deadline
            };
            self.pairs[index].exit_decision = Some(at);
            self.pairs[index].exit_reason = Some(reason);
            self.pairs[index].closing_deadline = Some(deadline);
            self.schedule(deadline)?;
            self.schedule(add(deadline, MS)?)?;
        }
        self.start_exit(at)
    }

    fn start_exit(&mut self, at: i64) -> Result<(), ReplayError> {
        if self.pending.is_some() {
            return Ok(());
        }
        let snap = self.snapshot()?.clone();
        if snap.funding.iter().any(|funding| funding.receipt.is_none()) {
            return Ok(());
        }
        if matches!(
            snap.phase,
            CarryPhase::ClosingPerp
                | CarryPhase::ClosingSpot
                | CarryPhase::Closed
                | CarryPhase::Flat
        ) {
            return Ok(());
        }
        let reference = self.source(
            at,
            self.source
                .decision_ref_at(C1Instrument::Perp, decision(at)),
        )?;
        let band = PriceBand::at(*reference.observation.value())?;
        let pair = snap.pair.ok_or(ReplayError::Control("missing plan"))?.pair;
        let outcome = self.command(
            at,
            CarryAction::BeginExit {
                pair,
                max_perp_price: band.upper,
            },
        )?;
        match outcome {
            CarryOutcome::Planned(id) => self.dispatch(at, id, band, &reference.source)?,
            CarryOutcome::Waiting => {
                let next = add(at, MS)?;
                self.follow_up = Some(next);
                self.schedule(next)?;
            }
            _ => return Err(ReplayError::Control("close did not plan")),
        }
        Ok(())
    }

    fn check_deadline(&self, at: i64) -> Result<(), ReplayError> {
        if let Some(index) = self.active_pair {
            let pair = &self.pairs[index];
            let deadline = pair.closing_deadline.unwrap_or(pair.opening_deadline);
            if at > deadline
                && (pair.exit_decision.is_some() || pair.observed_paired_at.is_none())
                && !self.flat_ready()
            {
                return Err(ReplayError::UnknownAtDeadline {
                    at,
                    stage: "unfinished_pair",
                });
            }
        }
        Ok(())
    }

    fn record_day(&mut self, at: i64) -> Result<(), ReplayError> {
        let end = at - MS;
        let account = match &self.observed {
            Some(snap) => snap.account.clone(),
            None => initial_account()?,
        };
        let pending = self.observed.as_ref().map_or(0, |snapshot| {
            snapshot
                .funding
                .iter()
                .filter(|f| f.effective.micros() < end && f.receipt.is_none())
                .count()
        });
        if pending != 0 {
            return Err(ReplayError::UnknownDailyLiability { at });
        }
        let cash_only = account.spot_btc.ticks() == 0 && account.perp_short.ticks() == 0;
        let equity = if cash_only {
            account
                .spot_quote
                .0
                .checked_add(account.perp_wallet.0)
                .and_then(|v| v.checked_add(account.idle.0))
                .ok_or(ReplayError::Arithmetic)?
        } else {
            let day = UtcDate::from_micros(UtcMicros::from_micros(end - DAY));
            let marks = self.source(at, self.source.daily_marks_at(day, decision(at)))?;
            account
                .equity(
                    *marks.spot.observation.value(),
                    *marks.perp_risk.observation.value(),
                )
                .map_err(|_| ReplayError::Arithmetic)?
                .0
        };
        if equity <= 0 || self.previous_equity <= 0 {
            return Err(ReplayError::Arithmetic);
        }
        let returns =
            daily_return_tick(self.previous_equity, equity).map_err(|_| ReplayError::Arithmetic)?;
        self.daily.push(DailyCapital {
            day_start: end - DAY,
            economic_end_exclusive: end,
            known_at: at,
            equity,
            return_ticks: returns,
            cash_only,
            pending_funding: pending,
            account: (&account).into(),
        });
        self.previous_equity = equity;
        Ok(())
    }
}

fn add(at: i64, delta: i64) -> Result<i64, ReplayError> {
    at.checked_add(delta).ok_or(ReplayError::Arithmetic)
}

fn decision(at: i64) -> DecisionTime {
    DecisionTime::from_micros(UtcMicros::from_micros(at))
}

fn initial_account() -> Result<CarryAccountState, ReplayError> {
    let zero = Qty::from_ticks(0).map_err(|_| ReplayError::Arithmetic)?;
    Ok(CarryAccountState {
        spot_quote: Cash(47_500_000_000_000),
        spot_btc: zero,
        perp_wallet: Cash(47_500_000_000_000),
        perp_short: zero,
        idle: Cash(5_000_000_000_000),
        spot_quote_fees: Cash(0),
        spot_btc_fees: zero,
        perp_fees: Cash(0),
        funding_net: Cash(0),
        realized_perp: Cash(0),
        perp_entry_cost_scaled: 0,
        realized_perp_scaled: 0,
    })
}

fn session(
    marks: C1Marks,
    scenario: Scenario,
    start: i64,
    at: i64,
    pair_cap: usize,
) -> Result<CarrySession, ReplayError> {
    let qty = |ticks| Qty::from_ticks(ticks).map_err(|_| ReplayError::Arithmetic);
    let age = PositiveMicros::new(120_000_000).map_err(|_| ReplayError::Arithmetic)?;
    let timing = ApprovalTiming::new(
        age,
        PositiveMicros::new(1_000).map_err(|_| ReplayError::Arithmetic)?,
    )
    .map_err(|_| ReplayError::Arithmetic)?;
    let execution = ExecutionConfig::new(
        timing,
        NotFoundAuthority::new(5_000, 1_000).map_err(|_| ReplayError::Arithmetic)?,
        MaxAttempts::new(1).map_err(|_| ReplayError::Arithmetic)?,
        age,
    )
    .map_err(|_| ReplayError::Arithmetic)?;
    let capacity = pair_cap.checked_mul(4).ok_or(ReplayError::Arithmetic)?;
    Ok(CarrySession {
        allocation: Allocation {
            total: Cash(C0),
            spot_quote: Cash(47_500_000_000_000),
            perp_collateral: Cash(47_500_000_000_000),
            idle: Cash(5_000_000_000_000),
        },
        fees: scenario.fees(),
        spot_grid: CarryGrid {
            lot: qty(1_000)?,
            min_notional: Cash(10_000_000_000),
            provenance: RuleProvenance::AssumedResearch,
        },
        perp_grid: CarryGrid {
            lot: qty(100_000)?,
            min_notional: Cash(10_000_000_000),
            provenance: RuleProvenance::AssumedResearch,
        },
        limits: CarryLimits {
            max_net_btc: qty(9_990_000_000)?,
            max_unhedged_notional: Cash(50_000_000_000_000),
            max_session_loss: Cash(5_000_000_000_000),
            max_mark_age_us: 120_000_000,
            max_legging_us: 600_000_000,
            max_orders: capacity,
            max_receipts: capacity,
            max_funding_events: pair_cap,
            max_batch_entries: 4,
        },
        margin: AssumedMarginPolicy {
            initial_bps: 10_000,
            maintenance_bps: 100,
            spot_close_buffer: Cash(2_500_000_000_000),
            perp_close_buffer: Cash(500_000_000_000),
            perp_funding_buffer: Cash(2_000_000_000_000),
        },
        execution,
        initial_spot: marks.spot.observation,
        initial_perp: marks.perp_risk.observation,
        initial_decision: decision(at),
        initial_now: Mono::from_micros(u64::try_from(at - start).map_err(|_| ReplayError::Clock)?),
    })
}
