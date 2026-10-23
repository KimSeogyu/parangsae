//! S05 `overextension-fade`. Fade `z` when the same-bar vol rank is at most one half.

use std::collections::{BTreeMap, VecDeque};

use libm::sqrt;
use parangsae_core::{BarInterval, Observation, ResampledBar};
use parangsae_signals::VolRegime;

use crate::donchian::positive_usize;
use crate::error::StrategyError;
use crate::series::{
    at_least, decision_micros, emitted, log_ratio, price_f64, sample_sd, side_target, sign_of,
    ticks_to_unit, Cursor, Hold, Ring, WARMUP_30D_US,
};
use crate::vol::rank_of;

const SIGMA_BARS: usize = 720;
const QUIET_TICKS: i64 = 50_000_000;
const VOL_CAP: usize = 24 + 168 + 1;

pub struct Fade {
    l: usize,
    k: f64,
    cursor: Cursor,
    ring: Ring,
    vol: VolRegime<24, 168>,
    returns: VecDeque<f64>,
    prev_close: Option<f64>,
    hold: Hold,
}

impl Fade {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let l = positive_usize(*params.get("l").ok_or(StrategyError::Param("l"))?, "l")?;
        if l > SIGMA_BARS {
            return Err(StrategyError::Param("l"));
        }
        let k_ticks = *params.get("k").ok_or(StrategyError::Param("k"))?;
        if k_ticks <= 0 {
            return Err(StrategyError::Param("k"));
        }
        let k = ticks_to_unit(k_ticks)?;
        let hold_h = *params.get("hold_h").ok_or(StrategyError::Param("hold_h"))?;
        Ok(Self {
            l,
            k,
            cursor: Cursor::new(BarInterval::H1),
            ring: Ring::new(VOL_CAP),
            vol: VolRegime::<24, 168>::new(BarInterval::H1)?,
            returns: VecDeque::with_capacity(SIGMA_BARS),
            prev_close: None,
            hold: Hold::new(hold_h, false)?,
        })
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        _book: Option<crate::DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view);
        let close = price_f64(view.value().close())?;
        if let Some(prev) = self.prev_close {
            self.returns.push_back(log_ratio(close, prev)?);
            if self.returns.len() > SIGMA_BARS {
                self.returns.pop_front();
            }
        }
        self.prev_close = Some(close);
        self.ring.push(row.clone());
        let rank = rank_of(&self.vol, &self.ring.to_vec())?;
        let Some(rank_ticks) = rank else {
            return Ok(None);
        };
        if self.returns.len() < SIGMA_BARS || decision_us < WARMUP_30D_US {
            return Ok(None);
        }
        let signal_side = self.event(rank_ticks);
        let side = self.hold.step(decision_us, signal_side)?;
        Ok(Some(emitted(view.value(), decision_us, side_target(side)?)))
    }

    pub(crate) fn dropped_entries(&self) -> u64 {
        self.hold.dropped()
    }

    fn event(&self, rank_ticks: i64) -> i8 {
        if rank_ticks > QUIET_TICKS {
            return 0;
        }
        let Some(sd) = sample_sd(&self.returns) else {
            return 0;
        };
        let mut move_l = 0.0;
        for value in self.returns.iter().rev().take(self.l) {
            move_l += *value;
        }
        let width = match u16::try_from(self.l) {
            Ok(width) => width,
            Err(_) => return 0,
        };
        let sigma = sqrt(f64::from(width)) * sd;
        if !sigma.is_finite() || sigma <= 0.0 {
            return 0;
        }
        let z = move_l / sigma;
        if !z.is_finite() || !at_least(z, self.k) {
            return 0;
        }
        -sign_of(z)
    }
}
