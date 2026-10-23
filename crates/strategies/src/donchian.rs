//! S02 `donchian-breakout`. Channel excludes the decision bar. Opposite breaks flip.

use std::collections::{BTreeMap, VecDeque};

use parangsae_core::{BarInterval, Observation, ResampledBar};

use crate::error::StrategyError;
use crate::series::{decision_micros, emitted, side_target, Cursor, Hold, WARMUP_30D_US};

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

    /// Channel only. The hold is not stepped.
    pub fn observe(&mut self, row: &Observation<ResampledBar>) -> Result<(), StrategyError> {
        let _ready = self.ingest(row)?;
        Ok(())
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        _book: Option<crate::DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let ready = self.ingest(row)?;
        if !ready.channel_ready || ready.decision_us < WARMUP_30D_US {
            return Ok(None);
        }
        let side = self.hold.step(ready.decision_us, ready.signal_side)?;
        Ok(Some(emitted(
            &ready.bar,
            ready.decision_us,
            side_target(side)?,
        )))
    }

    fn ingest(&mut self, row: &Observation<ResampledBar>) -> Result<ChannelReady, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view);
        let bar = *view.value();
        let channel_ready = self.prev.len() >= self.n;
        let signal_side = channel_side(&self.prev, self.n, bar.close().ticks());
        self.prev.push_back((bar.high().ticks(), bar.low().ticks()));
        if self.prev.len() > self.n {
            self.prev.pop_front();
        }
        Ok(ChannelReady {
            bar,
            decision_us,
            channel_ready,
            signal_side,
        })
    }

    pub(crate) fn dropped_entries(&self) -> u64 {
        self.hold.dropped()
    }
}

struct ChannelReady {
    bar: ResampledBar,
    decision_us: i64,
    channel_ready: bool,
    signal_side: i8,
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
