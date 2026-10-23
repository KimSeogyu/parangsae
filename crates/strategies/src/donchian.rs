//! S02 `donchian-breakout`. Channel excludes the decision bar. Opposite breaks flip.

use std::collections::{BTreeMap, VecDeque};

use parangsae_core::{BarInterval, Observation, ResampledBar};

use crate::error::StrategyError;
use crate::series::{emitted, side_target, Cursor, Hold, WARMUP_30D_US};

pub struct Donchian {
    n: usize,
    cursor: Cursor,
    prev: VecDeque<(i64, i64)>,
    hold: Hold,
}

impl Donchian {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let n = positive_usize(*params.get("n").ok_or(StrategyError::Param("n"))?, "n")?;
        let hold_h = *params.get("hold_h").ok_or(StrategyError::Param("hold_h"))?;
        Ok(Self {
            n,
            cursor: Cursor::new(BarInterval::H1),
            prev: VecDeque::new(),
            hold: Hold::new(hold_h, true)?,
        })
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        _book: Option<crate::DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let decision_us = self.cursor.admit(row)?;
        let bar = row.value();
        let ready = self.prev.len() >= self.n;
        let signal_side = channel_side(&self.prev, self.n, bar.close().ticks());
        self.prev.push_back((bar.high().ticks(), bar.low().ticks()));
        if self.prev.len() > self.n {
            self.prev.pop_front();
        }
        if !ready || decision_us < WARMUP_30D_US {
            return Ok(None);
        }
        let side = self.hold.step(decision_us, signal_side);
        Ok(Some(emitted(bar, decision_us, side_target(side)?)))
    }
}

pub(crate) fn positive_usize(value: i64, name: &'static str) -> Result<usize, StrategyError> {
    if value <= 0 {
        return Err(StrategyError::Param(name));
    }
    usize::try_from(value).map_err(|_| StrategyError::Param(name))
}

pub(crate) fn channel_side(prev: &VecDeque<(i64, i64)>, n: usize, close: i64) -> i8 {
    if prev.len() < n {
        return 0;
    }
    let mut upper = i64::MIN;
    let mut lower = i64::MAX;
    for (high, low) in prev {
        upper = upper.max(*high);
        lower = lower.min(*low);
    }
    if close > upper {
        1
    } else if close < lower {
        -1
    } else {
        0
    }
}
