//! H4 funding-crowding fade.
//!
//! `F24` is the sum of the last `sum_settlements` rates. A settlement is
//! long-crowded when that sum is positive and strictly above the `(1 - q)`
//! type-7 quantile of F24 over `[ft - 365d, ft)`, and short-crowded on the
//! mirror. The fade waits `confirm_h` and trades only when the H1 close
//! moved against the crowd. An open position skips the event.

use std::collections::{BTreeMap, VecDeque};

use parangsae_core::{BarInterval, DecisionTime, Observation, ResampledBar, UtcMicros};
use parangsae_signals::Target;

use crate::aux::FundingPrint;
use crate::error::StrategyError;
use crate::quantile::type7;
use crate::series::{
    decision_micros, emitted, log_ratio, price_f64, signed_unit, ticks_to_unit, Cursor,
    WARMUP_366D_US,
};
use crate::{
    ActualHold, DecisionBook, ExecutionDirective, ExecutionPhase, H4Event, H4Observation,
    H4ObservationKind, H4Opportunity, PositionSide, StrategyOutput, H4_OBSERVATIONS_PER_EVENT,
};

const DAY_US: i64 = 86_400_000_000;
const HOUR_US: i64 = 3_600_000_000;
const WINDOW_DAYS: i64 = 365;
const WINDOW_COUNT: usize = 1_095;
const SLOT_US: i64 = 8 * HOUR_US;

pub struct FundingFade {
    q: f64,
    sum_n: usize,
    confirm_us: i64,
    cursor: Cursor,
    rates: VecDeque<(i64, f64, i64)>,
    f24: VecDeque<F24>,
    closes: VecDeque<(i64, f64)>,
    hold: ActualHold,
    last_tail: Option<H4Event>,
    gap_boundary: bool,
    pending_tails: VecDeque<(H4Event, i64)>,
    observations: VecDeque<H4Observation>,
    observation_cap: usize,
}

#[derive(Clone, Copy)]
struct F24 {
    time: i64,
    value: f64,
    available_us: i64,
    tail: Option<H4Event>,
    tail_available_us: i64,
}

impl FundingFade {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let q_ticks = *params.get("q").ok_or(StrategyError::Param("q"))?;
        let q = ticks_to_unit(q_ticks)?;
        if !(0.0..1.0).contains(&q) {
            return Err(StrategyError::Param("q"));
        }
        let sum_n = positive(
            *params
                .get("sum_settlements")
                .ok_or(StrategyError::Param("sum_settlements"))?,
            "sum_settlements",
        )?;
        let confirm_h = *params
            .get("confirm_h")
            .ok_or(StrategyError::Param("confirm_h"))?;
        if confirm_h <= 0 {
            return Err(StrategyError::Param("confirm_h"));
        }
        let confirm_us = confirm_h
            .checked_mul(HOUR_US)
            .ok_or(StrategyError::Overflow)?;
        let hold_h = *params.get("hold_h").ok_or(StrategyError::Param("hold_h"))?;
        let close_cap = usize::try_from(confirm_h)
            .map_err(|_| StrategyError::Param("confirm_h"))?
            .saturating_add(2);
        let seed_cap = WINDOW_COUNT
            .checked_add(sum_n)
            .and_then(|cap| cap.checked_add(close_cap / 8))
            .and_then(|cap| cap.checked_add(4))
            .ok_or(StrategyError::Overflow)?;
        Ok(Self {
            q,
            sum_n,
            confirm_us,
            cursor: Cursor::new(BarInterval::H1),
            rates: VecDeque::new(),
            f24: VecDeque::new(),
            closes: VecDeque::with_capacity(close_cap),
            hold: ActualHold::from_hours(hold_h)?,
            last_tail: None,
            gap_boundary: true,
            pending_tails: VecDeque::new(),
            observations: VecDeque::new(),
            observation_cap: seed_cap
                .checked_mul(H4_OBSERVATIONS_PER_EVENT)
                .ok_or(StrategyError::Overflow)?,
        })
    }

    pub fn ingest_funding(&mut self, rows: &[FundingPrint]) -> Result<(), StrategyError> {
        let mut ordered = rows.to_vec();
        ordered.sort_by_key(|row| row.funding_time_us);
        for row in ordered {
            if self
                .rates
                .back()
                .is_some_and(|(time, _, _)| *time >= row.funding_time_us)
            {
                continue;
            }
            if self
                .rates
                .back()
                .is_some_and(|(time, _, _)| time.checked_add(SLOT_US) != Some(row.funding_time_us))
            {
                self.last_tail = None;
                self.gap_boundary = true;
            }
            let rate = signed_unit(row.rate_ticks)? / 100_000_000.0;
            let available_us = row.available_us.max(
                row.funding_time_us
                    .checked_add(60_000_000)
                    .ok_or(StrategyError::Overflow)?,
            );
            self.rates
                .push_back((row.funding_time_us, rate, available_us));
            if self.rates.len() >= self.sum_n {
                let mut sum = 0.0;
                let mut available_us = i64::MIN;
                let mut expected = row.funding_time_us;
                let mut complete = true;
                for (time, value, available) in self.rates.iter().rev().take(self.sum_n) {
                    complete &= *time == expected && *time % SLOT_US == 0;
                    expected = expected
                        .checked_sub(SLOT_US)
                        .ok_or(StrategyError::Overflow)?;
                    sum += *value;
                    available_us = available_us.max(*available);
                }
                if sum.is_finite() && complete {
                    let (tail, tail_available_us) =
                        self.classify_tail(row.funding_time_us, sum, available_us)?;
                    if let Some(event) = tail {
                        if self.pending_tails.len()
                            >= self.observation_cap / H4_OBSERVATIONS_PER_EVENT
                        {
                            return Err(StrategyError::H4ObservationCapacity);
                        }
                        self.pending_tails.push_back((event, tail_available_us));
                    }
                    self.f24.push_back(F24 {
                        time: row.funding_time_us,
                        value: sum,
                        available_us,
                        tail,
                        tail_available_us,
                    });
                } else {
                    self.last_tail = None;
                    self.gap_boundary = true;
                }
            }
            let keep = self.sum_n.saturating_add(WINDOW_COUNT).saturating_add(4);
            while self.f24.len() > keep {
                self.f24.pop_front();
            }
            while self.rates.len() > keep {
                self.rates.pop_front();
            }
        }
        Ok(())
    }

    pub fn observe(&mut self, row: &Observation<ResampledBar>) -> Result<(), StrategyError> {
        let _ready = self.evaluate(row)?;
        Ok(())
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        book: Option<DecisionBook>,
    ) -> Result<Option<StrategyOutput>, StrategyError> {
        let book = book.ok_or(StrategyError::MissingExecutionContext)?;
        if matches!(book.execution, ExecutionPhase::Adjusting { .. }) {
            return Err(StrategyError::AdjustingExecutionPhase);
        }
        let (ready, opportunity) = self.evaluate(row)?;
        if ready.decision_us < WARMUP_366D_US {
            return Ok(None);
        }
        if let Some(opportunity) = opportunity {
            if !matches!(book.execution, ExecutionPhase::Flat) {
                self.record(
                    opportunity.event,
                    ready.decision_us,
                    H4ObservationKind::OccupiedSkip,
                )?;
            }
        }
        let (target, directive) = match book.execution {
            ExecutionPhase::Flat => match opportunity {
                Some(event) => (
                    event.side().target()?,
                    ExecutionDirective::Enter {
                        event: event.event,
                        hold: self.hold,
                    },
                ),
                None => (Target::FLAT, ExecutionDirective::Keep),
            },
            ExecutionPhase::EntryPending { side } => (side.target()?, ExecutionDirective::Keep),
            ExecutionPhase::Open { side, entered_at } => {
                if ready.decision_us >= self.hold.exit_decision(entered_at)?.micros().micros() {
                    (Target::FLAT, ExecutionDirective::SetTarget)
                } else {
                    (side.target()?, ExecutionDirective::Keep)
                }
            }
            ExecutionPhase::ExitPending { .. } => (Target::FLAT, ExecutionDirective::Keep),
            ExecutionPhase::Adjusting { .. } => return Err(StrategyError::AdjustingExecutionPhase),
        };
        Ok(Some(StrategyOutput {
            signal: emitted(&ready.bar, ready.decision_us, target),
            directive,
        }))
    }

    pub fn opportunity(
        &mut self,
        row: &Observation<ResampledBar>,
    ) -> Result<Option<H4Opportunity>, StrategyError> {
        self.evaluate(row).map(|(_, opportunity)| opportunity)
    }

    pub fn take_observations(&mut self) -> Vec<H4Observation> {
        self.observations.drain(..).collect()
    }

    pub fn observation_capacity(&self) -> usize {
        self.observation_cap
    }

    fn evaluate(
        &mut self,
        row: &Observation<ResampledBar>,
    ) -> Result<(Ready, Option<H4Opportunity>), StrategyError> {
        let ready = self.features(row)?;
        let count = self.pending_tails.len();
        for _ in 0..count {
            if let Some((event, available)) = self.pending_tails.pop_front() {
                if available <= ready.decision_us {
                    self.record(event, ready.decision_us, H4ObservationKind::Tail)?;
                    let due = event
                        .settlement
                        .micros()
                        .checked_add(self.confirm_us)
                        .and_then(|time| time.checked_add(1_000))
                        .ok_or(StrategyError::Overflow)?;
                    if ready.decision_us > due {
                        self.record(
                            event,
                            ready.decision_us,
                            H4ObservationKind::ConfirmationUnavailable,
                        )?;
                    }
                } else {
                    self.pending_tails.push_back((event, available));
                }
            }
        }
        let opportunity = self.event(&ready)?;
        Ok((ready, opportunity))
    }

    fn event(&mut self, ready: &Ready) -> Result<Option<H4Opportunity>, StrategyError> {
        if ready.decision_us < WARMUP_366D_US || self.rates.len() < self.sum_n {
            return Ok(None);
        }
        let event_time = ready
            .decision_us
            .checked_sub(self.confirm_us)
            .and_then(|time| time.checked_sub(1_000))
            .ok_or(StrategyError::Overflow)?;
        let Some(point) = self
            .f24
            .iter()
            .rev()
            .find(|point| point.time == event_time)
            .copied()
        else {
            return Ok(None);
        };
        let Some(event) = point.tail else {
            return Ok(None);
        };
        if point.tail_available_us > ready.decision_us {
            return Ok(None);
        }
        let past_time = event_time
            .checked_add(1_000)
            .ok_or(StrategyError::Overflow)?;
        let past = self.close_at(past_time);
        if past.is_none() {
            self.record(
                event,
                ready.decision_us,
                H4ObservationKind::ConfirmationUnavailable,
            )?;
            return Ok(None);
        }
        let rc = log_ratio(ready.close, past.ok_or(StrategyError::Price)?)?;
        let confirmed = match event.crowd_side {
            PositionSide::Long => rc < 0.0,
            PositionSide::Short => rc > 0.0,
        };
        self.record(
            event,
            ready.decision_us,
            H4ObservationKind::Confirmation { confirmed },
        )?;
        if !confirmed {
            return Ok(None);
        }
        Ok(Some(H4Opportunity {
            event,
            decision_ts: DecisionTime::from_micros(UtcMicros::from_micros(ready.decision_us)),
            bar_close_time_us: ready.bar.close_time(),
        }))
    }

    fn features(&mut self, row: &Observation<ResampledBar>) -> Result<Ready, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view)
            .checked_add(1_000)
            .ok_or(StrategyError::Overflow)?;
        let bar = *view.value();
        let close = price_f64(bar.close())?;
        self.closes.push_back((decision_us, close));
        let cap = usize::try_from(self.confirm_us / HOUR_US)
            .map_err(|_| StrategyError::Overflow)?
            .saturating_add(2);
        while self.closes.len() > cap {
            self.closes.pop_front();
        }
        Ok(Ready {
            bar,
            decision_us,
            close,
        })
    }

    fn classify_tail(
        &mut self,
        ft: i64,
        f24: f64,
        available_us: i64,
    ) -> Result<(Option<H4Event>, i64), StrategyError> {
        let Some(window_lo) = ft.checked_sub(WINDOW_DAYS * DAY_US) else {
            return Ok((None, available_us));
        };
        let mut sample = Vec::new();
        let mut tail_available_us = available_us;
        for point in &self.f24 {
            if point.time >= window_lo && point.time < ft {
                sample.push(point.value);
                tail_available_us = tail_available_us.max(point.available_us);
            }
        }
        if sample.len() != WINDOW_COUNT {
            self.last_tail = None;
            self.gap_boundary = true;
            return Ok((None, tail_available_us));
        }
        let q_lo = type7(&sample, self.q)?;
        let q_hi = type7(&sample, 1.0 - self.q)?;
        let long_crowded = f24 > 0.0 && f24 > q_hi;
        let short_crowded = f24 < 0.0 && f24 < q_lo;
        let crowd_side = if long_crowded {
            PositionSide::Long
        } else if short_crowded {
            PositionSide::Short
        } else {
            self.last_tail = None;
            self.gap_boundary = false;
            return Ok((None, tail_available_us));
        };
        let confirmation_due = ft
            .checked_add(self.confirm_us)
            .and_then(|time| time.checked_add(1_000))
            .ok_or(StrategyError::Overflow)?;
        if tail_available_us > confirmation_due {
            // Preserve the later-discovered tail, without treating an unavailable
            // confirmation event as an observed bridge between episodes.
            self.last_tail = None;
            self.gap_boundary = true;
            return Ok((
                Some(H4Event {
                    settlement: UtcMicros::from_micros(ft),
                    episode: UtcMicros::from_micros(ft),
                    crowd_side,
                    gap_boundary: true,
                }),
                tail_available_us,
            ));
        }
        let (episode, gap_boundary) = if let Some(previous) = self.last_tail.filter(|previous| {
            previous.crowd_side == crowd_side
                && previous.settlement.micros().checked_add(SLOT_US) == Some(ft)
        }) {
            (previous.episode, previous.gap_boundary)
        } else {
            (UtcMicros::from_micros(ft), self.gap_boundary)
        };
        let event = H4Event {
            settlement: UtcMicros::from_micros(ft),
            episode,
            crowd_side,
            gap_boundary,
        };
        self.last_tail = Some(event);
        self.gap_boundary = false;
        Ok((Some(event), tail_available_us))
    }

    fn record(
        &mut self,
        event: H4Event,
        decision_us: i64,
        kind: H4ObservationKind,
    ) -> Result<(), StrategyError> {
        if self.observations.len() >= self.observation_cap {
            return Err(StrategyError::H4ObservationCapacity);
        }
        self.observations.push_back(H4Observation {
            event,
            observed_at: DecisionTime::from_micros(UtcMicros::from_micros(decision_us)),
            kind,
        });
        Ok(())
    }

    fn close_at(&self, decision_us: i64) -> Option<f64> {
        self.closes
            .iter()
            .rev()
            .find(|(time, _)| *time == decision_us)
            .map(|(_, value)| *value)
    }
}

struct Ready {
    bar: ResampledBar,
    decision_us: i64,
    close: f64,
}

fn positive(value: i64, name: &'static str) -> Result<usize, StrategyError> {
    if value <= 0 {
        return Err(StrategyError::Param(name));
    }
    usize::try_from(value).map_err(|_| StrategyError::Param(name))
}
