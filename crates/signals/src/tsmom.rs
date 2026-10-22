//! Time-series momentum. `r / r_scale`, clamped through [`crate::Target`].

use libm::log;
use parangsae_core::{BarInterval, ResampledBar};
use std::num::NonZeroUsize;

use crate::bars::prepare;
use crate::output::{ParamError, SignalError, SignalOutput};
use crate::target::Target;
use crate::window::{nonzero, CausalSignal, CausalWindow};

/// `ln(C_t / C_{t-n}) / r_scale` on one interval.
///
/// `N` is the lookback in bars. The window cap is `N + 1`.
#[derive(Clone, Debug)]
pub struct Tsmom<const N: u16> {
    interval: BarInterval,
    r_scale: f64,
}

impl<const N: u16> Tsmom<N> {
    pub fn new(interval: BarInterval, r_scale: f64) -> Result<Self, ParamError> {
        if N < 1 {
            return Err(ParamError::Lookback);
        }
        if !scale_ok(r_scale) {
            return Err(ParamError::Scale);
        }
        Ok(Self { interval, r_scale })
    }

    pub const fn n(&self) -> u16 {
        N
    }

    pub const fn interval(&self) -> BarInterval {
        self.interval
    }

    pub const fn r_scale(&self) -> f64 {
        self.r_scale
    }
}

impl<const N: u16> CausalSignal for Tsmom<N> {
    const ID: &'static str = "tsmom";
    const WINDOW_CAP: NonZeroUsize = nonzero((N as usize) + 1);
    type Row = ResampledBar;

    fn evaluate(
        &self,
        window: &CausalWindow<'_, Self::Row>,
    ) -> Result<Option<SignalOutput>, SignalError> {
        let need = usize::from(N) + 1;
        let Some(prepared) = prepare(window, self.interval, need)? else {
            return Ok(None);
        };
        let oldest = prepared.closes[0];
        let newest = prepared.closes[prepared.closes.len() - 1];
        let momentum = log(newest / oldest);
        if !momentum.is_finite() {
            return Err(SignalError::InvalidFeature);
        }
        let scaled = momentum / self.r_scale;
        if !scaled.is_finite() {
            return Err(SignalError::InvalidFeature);
        }
        let target = Target::from_f64(scaled).map_err(SignalError::Target)?;
        Ok(Some(SignalOutput::computed(
            prepared.timing.decision,
            prepared.timing.close_time,
            prepared.timing.interval,
            target,
        )))
    }
}

pub(crate) fn scale_ok(scale: f64) -> bool {
    scale.is_finite() && scale > 0.0
}
