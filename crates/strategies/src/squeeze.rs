//! S03 `squeeze-breakout`. Donchian as in S02, gated by `volregime` rank at t−1.

use std::collections::{BTreeMap, VecDeque};

use parangsae_core::{BarInterval, Observation, ResampledBar};
use parangsae_signals::VolRegime;

use crate::donchian::{channel_side, positive_usize};
use crate::error::StrategyError;
use crate::series::{emitted, side_target, Cursor, Hold, Ring, WARMUP_30D_US};
use crate::vol::rank_of;

const VOL_CAP: usize = 24 + 168 + 1;

pub struct Squeeze {
    n: usize,
    theta_ticks: i64,
    cursor: Cursor,
    prev: VecDeque<(i64, i64)>,
    ring: Ring,
    vol: VolRegime<24, 168>,
    prev_rank: Option<i64>,
    hold: Hold,
}

impl Squeeze {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let n = positive_usize(*params.get("n").ok_or(StrategyError::Param("n"))?, "n")?;
        let theta = *params.get("theta").ok_or(StrategyError::Param("theta"))?;
        if theta < 0 {
            return Err(StrategyError::Param("theta"));
        }
        let hold_h = *params.get("hold_h").ok_or(StrategyError::Param("hold_h"))?;
        Ok(Self {
            n,
            theta_ticks: theta,
            cursor: Cursor::new(BarInterval::H1),
            prev: VecDeque::new(),
            ring: Ring::new(VOL_CAP),
            vol: VolRegime::<24, 168>::new(BarInterval::H1)?,
            prev_rank: None,
            hold: Hold::new(hold_h, false)?,
        })
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        _book: Option<crate::DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let decision_us = self.cursor.admit(row)?;
        let bar = row.value();
        let channel_ready = self.prev.len() >= self.n;
        let breakout = channel_side(&self.prev, self.n, bar.close().ticks());
        self.prev.push_back((bar.high().ticks(), bar.low().ticks()));
        if self.prev.len() > self.n {
            self.prev.pop_front();
        }
        self.ring.push(row.clone());
        let rank = rank_of(&self.vol, &self.ring.to_vec())?;
        let gate = self.prev_rank;
        self.prev_rank = rank;
        let Some(p_prev) = gate else {
            return Ok(None);
        };
        if !channel_ready || decision_us < WARMUP_30D_US {
            return Ok(None);
        }
        let signal_side = if p_prev <= self.theta_ticks {
            breakout
        } else {
            0
        };
        let side = self.hold.step(decision_us, signal_side);
        Ok(Some(emitted(bar, decision_us, side_target(side)?)))
    }
}
