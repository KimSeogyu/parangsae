//! S06 `session-seasonality`. Block stats use strictly earlier days of that block.

use std::array;
use std::collections::{BTreeMap, VecDeque};

use parangsae_core::{BarInterval, Observation, ResampledBar};

use crate::donchian::positive_usize;
use crate::error::StrategyError;
use crate::series::{
    at_least, decision_micros, emitted, hold_fits, log_ratio, mean_t, price_f64, side_target,
    sign_of, ticks_to_unit, Cursor, WARMUP_365D_US,
};

const BLOCKS: usize = 6;
const H4_US: i64 = 14_400_000_000;

pub struct Season {
    w: usize,
    k: f64,
    cursor: Cursor,
    hist: [VecDeque<f64>; BLOCKS],
}

impl Season {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let w = positive_usize(
            *params.get("w_days").ok_or(StrategyError::Param("w_days"))?,
            "w_days",
        )?;
        if w < 2 {
            return Err(StrategyError::Param("w_days"));
        }
        let k_ticks = *params.get("k").ok_or(StrategyError::Param("k"))?;
        if k_ticks < 0 {
            return Err(StrategyError::Param("k"));
        }
        Ok(Self {
            w,
            k: ticks_to_unit(k_ticks)?,
            cursor: Cursor::new(BarInterval::H4),
            hist: array::from_fn(|_| VecDeque::new()),
        })
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        _book: Option<crate::DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view);
        let bar = view.value();
        let block = block_index(bar.open_time().micros())?;
        let next = (block + 1) % BLOCKS;
        let ready = self.hist[next].len() >= self.w;
        let mut side = 0i8;
        if ready {
            if let Some((mean, t)) = mean_t(&self.hist[next]) {
                if at_least(t, self.k) {
                    side = sign_of(mean);
                }
            }
        }
        let ret = log_ratio(price_f64(bar.close())?, price_f64(bar.open())?)?;
        self.hist[block].push_back(ret);
        if self.hist[block].len() > self.w {
            self.hist[block].pop_front();
        }
        if !ready || decision_us < WARMUP_365D_US {
            return Ok(None);
        }
        if side != 0 && !hold_fits(decision_us, H4_US) {
            side = 0;
        }
        Ok(Some(emitted(bar, decision_us, side_target(side)?)))
    }
}

fn block_index(open_us: i64) -> Result<usize, StrategyError> {
    let hour = open_us.div_euclid(3_600_000_000).rem_euclid(24);
    match hour {
        0 => Ok(0),
        4 => Ok(1),
        8 => Ok(2),
        12 => Ok(3),
        16 => Ok(4),
        20 => Ok(5),
        _ => Err(StrategyError::Clock),
    }
}
