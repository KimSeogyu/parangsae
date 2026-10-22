//! Funding z-score on expected 8-hour slots.
//!
//! The window is anchored at the newest visible row. A missing slot is flat
//! and is never filled from an older row. `interval_hours` other than 8 is an
//! error. The sign is contrarian and fixed: `clamp(-z / z_scale)`.

use libm::sqrt;
use parangsae_core::{BarInterval, DecisionTime, Observation, Ratio, ResampledBar, UtcMicros};
use std::cmp::Ordering;
use std::num::NonZeroUsize;

use crate::bars::{trigger, BarTiming};
use crate::funding::FundingRow;
use crate::output::{FlatReason, ParamError, SignalError, SignalOutput, WindowError};
use crate::target::Target;
use crate::tsmom::scale_ok;
use crate::window::{nonzero, CausalWindow};

/// 8h + 60s. Fixed. This is the staleness bound, not the visibility lag.
pub const PROD_MAX_AGE_US: i64 = 8 * 3_600_000_000 + 60 * 1_000_000;

const SLOT_US: i64 = 8 * 3_600_000_000;
const REQUIRED_INTERVAL_HOURS: u8 = 8;

/// Contrarian funding z-score. `M` is the number of expected 8h slots.
#[derive(Clone, Debug)]
pub struct FundingZ<const M: u16> {
    interval: BarInterval,
    z_scale: f64,
    max_age_us: i64,
}

impl<const M: u16> FundingZ<M> {
    pub const ID: &'static str = "funding_z";
    pub const BAR_CAP: NonZeroUsize = nonzero(1);
    pub const FUNDING_CAP: NonZeroUsize = nonzero(M as usize);

    pub fn new(interval: BarInterval, z_scale: f64, max_age_us: i64) -> Result<Self, ParamError> {
        if M < 2 {
            return Err(ParamError::Lookback);
        }
        if !scale_ok(z_scale) {
            return Err(ParamError::Scale);
        }
        if max_age_us <= 0 {
            return Err(ParamError::MaxAge);
        }
        Ok(Self {
            interval,
            z_scale,
            max_age_us,
        })
    }

    pub const fn m(&self) -> u16 {
        M
    }

    pub const fn interval(&self) -> BarInterval {
        self.interval
    }

    pub const fn z_scale(&self) -> f64 {
        self.z_scale
    }

    pub const fn max_age_us(&self) -> i64 {
        self.max_age_us
    }

    /// Both windows must share a decision time. A later funding window is
    /// [`SignalError::DecisionMismatch`] and is not read. [`drive_funding_z`]
    /// builds both windows at one decision.
    pub fn evaluate(
        &self,
        bars: &CausalWindow<'_, ResampledBar>,
        funding: &CausalWindow<'_, FundingRow>,
    ) -> Result<Option<SignalOutput>, SignalError> {
        if bars.decision() != funding.decision() {
            return Err(SignalError::DecisionMismatch);
        }
        let timing = trigger(bars, self.interval)?;
        self.decide(&timing, funding)
    }

    fn decide(
        &self,
        timing: &BarTiming,
        funding: &CausalWindow<'_, FundingRow>,
    ) -> Result<Option<SignalOutput>, SignalError> {
        // 1. Errors, in order: off-grid, not strictly increasing, interval != 8.
        for view in funding.iter() {
            let row = view.value();
            if row.funding_time().micros() % SLOT_US != 0 {
                return Err(SignalError::FundingOffGrid {
                    funding_time: row.funding_time(),
                });
            }
        }
        let mut previous: Option<UtcMicros> = None;
        for view in funding.iter() {
            let funding_time = view.value().funding_time();
            if let Some(prev) = previous {
                if funding_time == prev {
                    return Err(SignalError::DuplicateFunding);
                }
                if funding_time < prev {
                    return Err(SignalError::Window(WindowError::Unordered));
                }
            }
            previous = Some(funding_time);
        }
        for view in funding.iter() {
            let row = view.value();
            if row.interval_hours() != REQUIRED_INTERVAL_HOURS {
                return Err(SignalError::FundingIntervalChange {
                    funding_time: row.funding_time(),
                    interval_hours: row.interval_hours(),
                });
            }
        }

        // 2. Warmup.
        if funding.len() < usize::from(M) {
            return Ok(None);
        }

        let newest_view = funding.last().ok_or(SignalError::NoTriggeringBar)?;
        let newest = newest_view.value();
        // 3. Stale head. Checked before slot presence so a missing head is stale.
        let age = timing
            .decision
            .micros()
            .micros()
            .checked_sub(newest_view.available_ts().micros().micros())
            .ok_or(SignalError::Overflow)?;
        if age > self.max_age_us {
            return Ok(Some(SignalOutput::flat(
                timing.decision,
                timing.close_time,
                timing.interval,
                FlatReason::FundingStale,
            )));
        }

        // 4. Expected slots ending at the newest visible row. Never bridged.
        let origin = newest.funding_time();
        let mut rates = Vec::with_capacity(usize::from(M));
        for index in 0..M {
            let back_slots = i64::from(M - 1 - index);
            let back = back_slots
                .checked_mul(SLOT_US)
                .ok_or(SignalError::Overflow)?;
            let slot = origin
                .checked_add_micros(-back)
                .ok_or(SignalError::Overflow)?;
            match rate_at(funding, slot) {
                Some(rate) => rates.push(rate),
                None => {
                    return Ok(Some(SignalOutput::flat(
                        timing.decision,
                        timing.close_time,
                        timing.interval,
                        FlatReason::FundingSlotMissing,
                    )))
                }
            }
        }

        // 5. Constant windows stay a computed zero, before any f64 math.
        let first = rates[0].ticks();
        if rates.iter().all(|rate| rate.ticks() == first) {
            return Ok(Some(self.target(timing, Target::FLAT)));
        }
        let values: Vec<f64> = rates.iter().map(|rate| rate.ticks() as f64).collect();
        let width = f64::from(M);
        let mut sum = 0.0;
        for value in &values {
            sum += value;
        }
        let mean = sum / width;
        let mut squares = 0.0;
        for value in &values {
            let deviation = value - mean;
            squares += deviation * deviation;
        }
        let sigma = sqrt(squares / width);
        if !mean.is_finite() || !sigma.is_finite() {
            return Err(SignalError::InvalidFeature);
        }
        if sigma.partial_cmp(&0.0) == Some(Ordering::Equal) {
            return Ok(Some(self.target(timing, Target::FLAT)));
        }
        let last = values[values.len() - 1];
        let z = (last - mean) / sigma;
        let scaled = -z / self.z_scale;
        if !scaled.is_finite() {
            return Err(SignalError::InvalidFeature);
        }
        let target = Target::from_f64(scaled).map_err(SignalError::Target)?;
        Ok(Some(self.target(timing, target)))
    }

    fn target(&self, timing: &BarTiming, target: Target) -> SignalOutput {
        SignalOutput::computed(timing.decision, timing.close_time, timing.interval, target)
    }
}

fn rate_at(funding: &CausalWindow<'_, FundingRow>, slot: UtcMicros) -> Option<Ratio> {
    funding.iter().find_map(|view| {
        let row = view.value();
        if row.funding_time() == slot {
            Some(row.rate())
        } else {
            None
        }
    })
}

/// Bars and funding are both gated at `decision`.
pub fn drive_funding_z<const M: u16>(
    signal: &FundingZ<M>,
    bars: &[Observation<ResampledBar>],
    funding: &[Observation<FundingRow>],
    decision: DecisionTime,
) -> Result<Option<SignalOutput>, SignalError> {
    let bar_views = bars.iter().filter_map(|row| row.causal_at(decision));
    let fund_views = funding.iter().filter_map(|row| row.causal_at(decision));
    let bar_window = CausalWindow::from_views(decision, FundingZ::<M>::BAR_CAP, bar_views)?;
    let fund_window = CausalWindow::from_views(decision, FundingZ::<M>::FUNDING_CAP, fund_views)?;
    signal.evaluate(&bar_window, &fund_window)
}
