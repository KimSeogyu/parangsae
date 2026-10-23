//! Volatility mid-rank. This is a feature. It does not emit a target.

use libm::{log, sqrt};
use parangsae_core::{BarInterval, ResampledBar};
use std::num::NonZeroUsize;

use crate::bars::prepare;
use crate::feature::FeatureOutput;
use crate::output::{ParamError, SignalError};
use crate::rank::Rank;
use crate::window::{nonzero, CausalFeature, CausalWindow};

/// Realized-vol percentile over the last `W + K + 1` bars.
#[derive(Clone, Copy, Debug)]
pub struct VolRegime<const W: u16, const K: u16> {
    interval: BarInterval,
}

impl<const W: u16, const K: u16> VolRegime<W, K> {
    pub fn new(interval: BarInterval) -> Result<Self, ParamError> {
        if W < 1 || K < 1 {
            return Err(ParamError::Lookback);
        }
        Ok(Self { interval })
    }

    pub const fn w(&self) -> u16 {
        W
    }

    pub const fn k(&self) -> u16 {
        K
    }

    pub const fn interval(&self) -> BarInterval {
        self.interval
    }
}

impl<const W: u16, const K: u16> CausalFeature for VolRegime<W, K> {
    const ID: &'static str = "volregime";
    const WINDOW_CAP: NonZeroUsize = nonzero((W as usize) + (K as usize) + 1);
    type Row = ResampledBar;

    fn evaluate(
        &self,
        window: &CausalWindow<'_, Self::Row>,
    ) -> Result<Option<FeatureOutput>, SignalError> {
        let need = usize::from(W) + usize::from(K) + 1;
        let Some(prepared) = prepare(window, self.interval, need)? else {
            return Ok(None);
        };
        let returns = log_returns(&prepared.closes)?;
        let width = usize::from(W);
        let last = prepared.closes.len() - 1;
        let current = realized_vol(&returns, last, width)?;
        let mut refs = Vec::with_capacity(usize::from(K));
        for back in 1..=usize::from(K) {
            refs.push(realized_vol(&returns, last - back, width)?);
        }
        let rank = Rank::of(current, &refs).map_err(SignalError::Rank)?;
        Ok(Some(FeatureOutput {
            decision_ts: prepared.timing.decision,
            bar_close_time_us: prepared.timing.close_time,
            interval: prepared.timing.interval,
            value: rank,
        }))
    }
}

fn log_returns(closes: &[f64]) -> Result<Vec<f64>, SignalError> {
    let mut returns = Vec::with_capacity(closes.len().saturating_sub(1));
    for pair in closes.windows(2) {
        if pair[0] <= 0.0 || pair[1] <= 0.0 {
            return Err(SignalError::InvalidFeature);
        }
        let step = log(pair[1] / pair[0]);
        if !step.is_finite() {
            return Err(SignalError::InvalidFeature);
        }
        returns.push(step);
    }
    Ok(returns)
}

/// `RV` at bar index `end`, using the `width` returns that land on that bar.
fn realized_vol(returns: &[f64], end: usize, width: usize) -> Result<f64, SignalError> {
    if end < width {
        return Err(SignalError::InvalidFeature);
    }
    let start = end - width;
    let mut squares = 0.0;
    for value in &returns[start..end] {
        squares += value * value;
    }
    let mean = squares / f64::from(u16::try_from(width).map_err(|_| SignalError::InvalidFeature)?);
    let vol = sqrt(mean);
    if !vol.is_finite() {
        return Err(SignalError::InvalidFeature);
    }
    Ok(vol)
}
