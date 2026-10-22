//! Shared bar checks for target and feature kernels.
//!
//! Closes are converted to f64 only inside the call. Nothing here stores them.

use std::collections::BTreeSet;

use parangsae_core::{BarInterval, DecisionTime, Price, ResampledBar, UtcMicros};

use crate::output::SignalError;
use crate::window::CausalWindow;

/// Largest price tick that converts to f64 without rounding.
pub(crate) const MAX_EXACT_PRICE_TICKS: i64 = 1 << 53;

pub(crate) struct BarTiming {
    pub decision: DecisionTime,
    pub close_time: UtcMicros,
    pub interval: BarInterval,
}

pub(crate) struct PreparedBars {
    pub timing: BarTiming,
    pub closes: Vec<f64>,
}

/// Structural checks, then the close series. `Ok(None)` is warmup: fewer than
/// `count` bars after the grid checks have passed.
pub(crate) fn prepare(
    window: &CausalWindow<'_, ResampledBar>,
    interval: BarInterval,
    count: usize,
) -> Result<Option<PreparedBars>, SignalError> {
    let timing = trigger(window, interval)?;
    let bars: Vec<&ResampledBar> = window.iter().map(|view| view.value()).collect();
    for bar in &bars {
        if bar.interval() != interval {
            return Err(SignalError::IntervalMismatch);
        }
    }
    let mut seen = BTreeSet::new();
    for bar in &bars {
        if !seen.insert(bar.open_time()) {
            return Err(SignalError::DuplicateBar);
        }
    }
    let step = interval.micros();
    for pair in bars.windows(2) {
        let expected = pair[0]
            .open_time()
            .checked_add_micros(step)
            .ok_or(SignalError::Overflow)?;
        if pair[1].open_time() != expected {
            return Err(SignalError::Gap {
                expected_open: expected,
            });
        }
    }
    let mut closes = Vec::with_capacity(bars.len());
    for bar in &bars {
        closes.push(price_to_f64(bar.close())?);
    }
    if closes.len() < count {
        return Ok(None);
    }
    Ok(Some(PreparedBars { timing, closes }))
}

pub(crate) fn trigger(
    window: &CausalWindow<'_, ResampledBar>,
    interval: BarInterval,
) -> Result<BarTiming, SignalError> {
    let last = window.last().ok_or(SignalError::NoTriggeringBar)?;
    if last.available_ts().micros() != window.decision().micros() {
        return Err(SignalError::NoTriggeringBar);
    }
    let bar = last.value();
    if bar.interval() != interval {
        return Err(SignalError::IntervalMismatch);
    }
    let close_time = bar.close_time();
    let expected = close_time
        .checked_add_micros(1_000)
        .ok_or(SignalError::Overflow)?;
    if expected != window.decision().micros() {
        return Err(SignalError::ClockMismatch);
    }
    Ok(BarTiming {
        decision: window.decision(),
        close_time,
        interval,
    })
}

fn price_to_f64(price: Price) -> Result<f64, SignalError> {
    let ticks = price.ticks();
    if ticks <= 0 || ticks > MAX_EXACT_PRICE_TICKS {
        return Err(SignalError::InvalidFeature);
    }
    Ok(ticks as f64)
}
