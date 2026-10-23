//! Closed-bar gate. A bar is usable only when `available_ts` equals `close + 1 ms`.

use std::collections::VecDeque;

use libm::{fabs, log, sqrt};
use parangsae_core::{
    BarInterval, Causal, DecisionTime, Observation, Price, ResampledBar, UtcMicros,
};
use parangsae_signals::{Decision, SignalOutput, Target};

use crate::error::StrategyError;

/// 2020-12-18 00:00:00 UTC. No bar before this open is read.
pub const EARLIEST_US: i64 = 1_608_249_600_000_000;

/// 2021-01-17 00:00:00 UTC. S01, S02, S03, and S05 emit nothing earlier.
pub const WARMUP_30D_US: i64 = EARLIEST_US + 30 * 86_400_000_000;

/// 2021-02-16 00:00:00 UTC. H1 emits nothing earlier.
pub const WARMUP_60D_US: i64 = EARLIEST_US + 60 * 86_400_000_000;

/// 2021-03-19 00:00:00 UTC. H5 emits nothing earlier.
pub const WARMUP_91D_US: i64 = EARLIEST_US + 91 * 86_400_000_000;

/// 2021-12-18 00:00:00 UTC. S06 emits nothing earlier.
pub const WARMUP_365D_US: i64 = EARLIEST_US + 365 * 86_400_000_000;

/// 2021-12-19 00:00:00 UTC. H4 emits nothing earlier.
pub const WARMUP_366D_US: i64 = EARLIEST_US + 366 * 86_400_000_000;

/// 2021-12-26 00:00:00 UTC. H3 emits nothing earlier.
pub const WARMUP_373D_US: i64 = EARLIEST_US + 373 * 86_400_000_000;

/// Open of 2025-09-30 23:59 UTC. A hold that ends after this is not opened.
pub const LAST_TRADABLE_OPEN_US: i64 = 1_759_276_800_000_000 - 60_000_000;

const MAX_EXACT_PRICE_TICKS: i64 = 1 << 53;
const HOUR_US: i64 = 3_600_000_000;

/// Equity and position at one decision. `mark_ticks` is the decision close.
///
/// Signed weight is `position_lots * mark_ticks / equity_ticks`, toward zero.
/// The close marks the decision book; order quantity uses the actual fill price.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionBook {
    pub equity_ticks: i64,
    pub position_lots: i64,
    pub mark_ticks: i64,
}

/// One position in `{-1, 0, +1}` with a fixed hold. Same-side signals do not extend it.
#[derive(Clone, Debug)]
pub struct Hold {
    side: i8,
    exit_us: i64,
    hold_us: i64,
    flip: bool,
    dropped: u64,
}

impl Hold {
    pub fn new(hold_hours: i64, flip: bool) -> Result<Self, StrategyError> {
        if hold_hours <= 0 {
            return Err(StrategyError::Param("hold_h"));
        }
        let hold_us = hold_hours
            .checked_mul(HOUR_US)
            .ok_or(StrategyError::Overflow)?;
        Ok(Self {
            side: 0,
            exit_us: 0,
            hold_us,
            flip,
            dropped: 0,
        })
    }

    /// Holds `arm` refused because they would end after 2025-09-30 23:59 UTC.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub(crate) fn side(&self) -> i8 {
        self.side
    }

    /// `signal_side` is the entry direction on this bar, or 0 when there is no event.
    pub fn step(&mut self, decision_us: i64, signal_side: i8) -> Result<i8, StrategyError> {
        if self.side != 0 && decision_us >= self.exit_us {
            self.side = 0;
        }
        let opening = self.side == 0 && signal_side != 0;
        let flipping = self.side != 0 && self.flip && signal_side == -self.side;
        if opening || flipping {
            let armed = self.arm(decision_us, signal_side)?;
            if flipping && !armed {
                self.side = 0;
            }
        }
        Ok(self.side)
    }

    fn arm(&mut self, decision_us: i64, side: i8) -> Result<bool, StrategyError> {
        match decision_us.checked_add(self.hold_us) {
            Some(end) if end <= LAST_TRADABLE_OPEN_US => {
                self.side = side;
                self.exit_us = end;
                Ok(true)
            }
            _ => {
                self.dropped = self.dropped.checked_add(1).ok_or(StrategyError::Overflow)?;
                Ok(false)
            }
        }
    }
}

pub fn hold_fits(decision_us: i64, hold_us: i64) -> bool {
    match decision_us.checked_add(hold_us) {
        Some(end) => end <= LAST_TRADABLE_OPEN_US,
        None => false,
    }
}

/// Gap and clock checks. Returns the causal view of the closed bar.
pub struct Cursor {
    interval: BarInterval,
    last_open: Option<i64>,
}

impl Cursor {
    pub fn new(interval: BarInterval) -> Self {
        Self {
            interval,
            last_open: None,
        }
    }

    pub fn admit<'a>(
        &mut self,
        row: &'a Observation<ResampledBar>,
    ) -> Result<Causal<'a, ResampledBar>, StrategyError> {
        let bar = row.value();
        if bar.interval() != self.interval {
            return Err(StrategyError::Interval);
        }
        let open = bar.open_time().micros();
        if open < EARLIEST_US {
            return Err(StrategyError::BeforeEarliest);
        }
        let close = bar.close_time().micros();
        let expected = close.checked_add(1_000).ok_or(StrategyError::Overflow)?;
        let available = row.timestamps().available_ts.micros().micros();
        if available != expected {
            return Err(StrategyError::Clock);
        }
        let decision = DecisionTime::from_micros(UtcMicros::from_micros(available));
        let view = row.causal_at(decision).ok_or(StrategyError::Clock)?;
        if let Some(prev) = self.last_open {
            let next = prev
                .checked_add(self.interval.micros())
                .ok_or(StrategyError::Overflow)?;
            if view.value().open_time().micros() != next {
                return Err(StrategyError::Gap {
                    expected_open: next,
                });
            }
        }
        self.last_open = Some(view.value().open_time().micros());
        Ok(view)
    }
}

pub(crate) fn decision_micros(view: &Causal<'_, ResampledBar>) -> i64 {
    view.available_ts().micros().micros()
}

#[derive(Clone, Debug)]
pub struct Ring {
    cap: usize,
    rows: VecDeque<Observation<ResampledBar>>,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            rows: VecDeque::new(),
        }
    }

    pub fn push(&mut self, row: Observation<ResampledBar>) {
        self.rows.push_back(row);
        while self.rows.len() > self.cap {
            self.rows.pop_front();
        }
    }

    pub fn to_vec(&self) -> Vec<Observation<ResampledBar>> {
        self.rows.iter().cloned().collect()
    }
}

pub fn price_f64(price: Price) -> Result<f64, StrategyError> {
    let ticks = price.ticks();
    if ticks <= 0 || ticks > MAX_EXACT_PRICE_TICKS {
        return Err(StrategyError::Price);
    }
    Ok(ticks as f64)
}

pub fn log_ratio(numer: f64, denom: f64) -> Result<f64, StrategyError> {
    if !numer.is_finite() || !denom.is_finite() || numer <= 0.0 || denom <= 0.0 {
        return Err(StrategyError::Price);
    }
    let value = log(numer / denom);
    if value.is_finite() {
        Ok(value)
    } else {
        Err(StrategyError::Price)
    }
}

pub fn side_target(side: i8) -> Result<Target, StrategyError> {
    let value = match side {
        0 => return Ok(Target::FLAT),
        1 => 1.0,
        -1 => -1.0,
        _ => return Err(StrategyError::Param("side")),
    };
    Target::from_f64(value).map_err(|_| StrategyError::Price)
}

pub fn emitted(bar: &ResampledBar, decision_us: i64, target: Target) -> SignalOutput {
    SignalOutput {
        decision_ts: DecisionTime::from_micros(UtcMicros::from_micros(decision_us)),
        bar_close_time_us: bar.close_time(),
        interval: bar.interval(),
        decision: Decision::Target(target),
        accept: 0,
    }
}

/// Sample standard deviation, ddof 1. `None` when it is not strictly positive.
pub fn sample_sd(values: &VecDeque<f64>) -> Option<f64> {
    let n = values.len();
    if n < 2 {
        return None;
    }
    let width = match u32::try_from(n) {
        Ok(width) => width,
        Err(_) => return None,
    };
    let nf = f64::from(width);
    let mut sum = 0.0;
    for value in values {
        sum += *value;
    }
    let mean = sum / nf;
    let mut squares = 0.0;
    for value in values {
        let delta = *value - mean;
        squares += delta * delta;
    }
    let denom = f64::from(width - 1);
    let sd = sqrt(squares / denom);
    if sd.is_finite() && sd > 0.0 {
        Some(sd)
    } else {
        None
    }
}

/// `(mean, t)` with `t = mean / (sd / sqrt(n))`. `None` when `sd` is not positive.
pub fn mean_t(values: &VecDeque<f64>) -> Option<(f64, f64)> {
    let n = values.len();
    if n < 2 {
        return None;
    }
    let width = match u32::try_from(n) {
        Ok(width) => width,
        Err(_) => return None,
    };
    let nf = f64::from(width);
    let mut sum = 0.0;
    for value in values {
        sum += *value;
    }
    let mean = sum / nf;
    let sd = sample_sd(values)?;
    let t = mean / (sd / sqrt(nf));
    if t.is_finite() {
        Some((mean, t))
    } else {
        None
    }
}

pub fn at_least(magnitude: f64, threshold: f64) -> bool {
    fabs(magnitude) >= threshold
}

pub fn sign_of(value: f64) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

pub fn ticks_to_unit(ticks: i64) -> Result<f64, StrategyError> {
    if !(0..=MAX_EXACT_PRICE_TICKS).contains(&ticks) {
        return Err(StrategyError::Param("scale"));
    }
    Ok((ticks as f64) / 100_000_000.0)
}

/// UTC hour of an epoch-aligned decision instant.
pub(crate) fn utc_hour(decision_us: i64) -> Result<i64, StrategyError> {
    if decision_us < 0 {
        return Err(StrategyError::Clock);
    }
    Ok((decision_us % (24 * HOUR_US)) / HOUR_US)
}

/// Signed ticks whose magnitude is an exact f64.
pub(crate) fn signed_unit(ticks: i64) -> Result<f64, StrategyError> {
    let negative = ticks < 0;
    let magnitude = ticks.unsigned_abs();
    if magnitude > (1_u64 << 53) {
        return Err(StrategyError::Price);
    }
    let unit = magnitude as f64;
    Ok(if negative { -unit } else { unit })
}
