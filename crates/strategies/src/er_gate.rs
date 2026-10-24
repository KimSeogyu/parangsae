//! H3 Donchian breakout with an efficiency-ratio quantile gate.
//!
//! The channel is S02's: the previous `n` highs and lows, excluding bar t.
//! `ER_t` uses `er_l` closes. The gate reads the previous 8,760 ER values and
//! stays shut until 8,000 of them are defined.

use std::collections::{BTreeMap, VecDeque};

use libm::fabs;
use parangsae_core::{BarInterval, Observation, ResampledBar};

use crate::donchian::{channel_side, positive_usize};
use crate::error::StrategyError;
use crate::quantile::type7;
use crate::series::{
    decision_micros, emitted, price_f64, side_target, ticks_to_unit, Cursor, Hold, WARMUP_373D_US,
};

const ER_WINDOW: usize = 8_760;
const MIN_DEFINED: usize = 8_000;

pub struct ErGate {
    n: usize,
    er_l: usize,
    er_q: f64,
    cursor: Cursor,
    prev: VecDeque<(i64, i64)>,
    closes: VecDeque<f64>,
    ers: VecDeque<Option<f64>>,
    hold: Hold,
}

impl ErGate {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let n = positive_usize(*params.get("n").ok_or(StrategyError::Param("n"))?, "n")?;
        let er_l = positive_usize(
            *params.get("er_l").ok_or(StrategyError::Param("er_l"))?,
            "er_l",
        )?;
        let er_q_ticks = *params.get("er_q").ok_or(StrategyError::Param("er_q"))?;
        let er_q = ticks_to_unit(er_q_ticks)?;
        if !(0.0..1.0).contains(&er_q) {
            return Err(StrategyError::Param("er_q"));
        }
        let hold_h = *params.get("hold_h").ok_or(StrategyError::Param("hold_h"))?;
        Ok(Self {
            n,
            er_l,
            er_q,
            cursor: Cursor::new(BarInterval::H1),
            prev: VecDeque::new(),
            closes: VecDeque::new(),
            ers: VecDeque::new(),
            hold: Hold::new(hold_h, true)?,
        })
    }

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
        if !ready.channel_ready || ready.decision_us < WARMUP_373D_US {
            return Ok(None);
        }
        let signal = if ready.gate_open { ready.break_side } else { 0 };
        let side = self.hold.step(ready.decision_us, signal)?;
        Ok(Some(emitted(
            &ready.bar,
            ready.decision_us,
            side_target(side)?,
        )))
    }

    pub(crate) fn dropped_entries(&self) -> u64 {
        self.hold.dropped()
    }

    fn ingest(&mut self, row: &Observation<ResampledBar>) -> Result<Ready, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view);
        let bar = *view.value();
        let channel_ready = self.prev.len() >= self.n;
        let break_side = channel_side(&self.prev, self.n, bar.close().ticks());
        let close = price_f64(bar.close())?;
        let er = efficiency(&self.closes, close, self.er_l);
        let gate_open = self.gate_open(er)?;
        self.prev.push_back((bar.high().ticks(), bar.low().ticks()));
        if self.prev.len() > self.n {
            self.prev.pop_front();
        }
        self.closes.push_back(close);
        if self.closes.len() > self.er_l.saturating_add(1) {
            self.closes.pop_front();
        }
        self.ers.push_back(er);
        while self.ers.len() > ER_WINDOW {
            self.ers.pop_front();
        }
        Ok(Ready {
            bar,
            decision_us,
            channel_ready,
            break_side,
            gate_open,
        })
    }

    fn gate_open(&self, er: Option<f64>) -> Result<bool, StrategyError> {
        let Some(er) = er else {
            return Ok(false);
        };
        let defined: Vec<f64> = self.ers.iter().copied().flatten().collect();
        if defined.len() < MIN_DEFINED {
            return Ok(false);
        }
        let threshold = type7(&defined, self.er_q)?;
        Ok(er >= threshold)
    }
}

struct Ready {
    bar: ResampledBar,
    decision_us: i64,
    channel_ready: bool,
    break_side: i8,
    gate_open: bool,
}

fn efficiency(prev: &VecDeque<f64>, close: f64, er_l: usize) -> Option<f64> {
    if prev.len() < er_l {
        return None;
    }
    let start = prev.len() - er_l;
    let oldest = prev[start];
    let mut path = fabs(close - prev[prev.len() - 1]);
    for index in (start + 1)..prev.len() {
        path += fabs(prev[index] - prev[index - 1]);
    }
    if path <= 0.0 {
        return None;
    }
    let net = fabs(close - oldest);
    let ratio = net / path;
    if ratio.is_finite() {
        Some(ratio)
    } else {
        None
    }
}
