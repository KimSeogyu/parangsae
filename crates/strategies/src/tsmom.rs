//! S01 `tsmom-slow` and the H1 daily cadence. The scaled return is `signals::Tsmom`.

use std::collections::BTreeMap;

use libm::sqrt;
use parangsae_core::{BarInterval, DecisionTime, Observation, ResampledBar, UtcMicros};
use parangsae_signals::{drive, Decision, Target, Tsmom};

use crate::error::StrategyError;
use crate::series::{
    decision_micros, emitted, ticks_to_unit, utc_hour, Cursor, DecisionBook, Ring, WARMUP_30D_US,
    WARMUP_60D_US,
};

const DEFAULT_DEADBAND_TICKS: i64 = 25_000_000;

pub struct TsmomSlow {
    leg: TsmomLeg,
    deadband: i64,
    decision_hour: Option<i64>,
    warmup_us: i64,
    held: Target,
    accept: u64,
    live: bool,
}

impl TsmomSlow {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        Self::build(params, false)
    }

    pub(crate) fn h1(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        Self::build(params, true)
    }

    fn build(params: &BTreeMap<String, i64>, hourly: bool) -> Result<Self, StrategyError> {
        let minutes = *params
            .get("interval")
            .ok_or(StrategyError::Param("interval"))?;
        let interval = match minutes {
            60 => BarInterval::H1,
            240 => BarInterval::H4,
            _ => return Err(StrategyError::Param("interval")),
        };
        let n_raw = *params.get("n").ok_or(StrategyError::Param("n"))?;
        let n = u16::try_from(n_raw).map_err(|_| StrategyError::Param("n"))?;
        if !n_supported(n) {
            return Err(StrategyError::Param("n"));
        }
        let c = *params.get("c").ok_or(StrategyError::Param("c"))?;
        let deadband = match params.get("deadband").copied() {
            Some(ticks) if ticks > 0 => ticks,
            Some(_) => return Err(StrategyError::Param("deadband")),
            None if hourly => return Err(StrategyError::Param("deadband")),
            None => DEFAULT_DEADBAND_TICKS,
        };
        let decision_hour = match params.get("decision_hour_utc").copied() {
            Some(hour) if (0..24).contains(&hour) => Some(hour),
            Some(_) => return Err(StrategyError::Param("decision_hour_utc")),
            None if hourly => return Err(StrategyError::Param("decision_hour_utc")),
            None => None,
        };
        Ok(Self {
            leg: TsmomLeg::new(interval, n, c)?,
            deadband,
            decision_hour,
            warmup_us: if hourly { WARMUP_60D_US } else { WARMUP_30D_US },
            held: Target::FLAT,
            accept: 0,
            live: false,
        })
    }

    pub fn interval(&self) -> BarInterval {
        self.leg.interval
    }

    /// Ring only. `held` and `accept` stay put, so a warmup target cannot reopen.
    pub fn observe(&mut self, row: &Observation<ResampledBar>) -> Result<(), StrategyError> {
        let _ready = self.leg.push_row(row)?;
        Ok(())
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        book: Option<DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let Some(ready) = self.leg.push_row(row)? else {
            return Ok(None);
        };
        if ready.decision_us < self.warmup_us {
            return Ok(None);
        }
        if !self.on_hour(ready.decision_us)? {
            if !self.live {
                return Ok(None);
            }
            return Ok(Some(self.emit(&ready)));
        }
        let current = match book {
            Some(book) => signed_weight(book)?,
            None => self.held.ticks(),
        };
        if passes_deadband(current, ready.target.ticks(), self.deadband) {
            self.held = ready.target;
            self.accept = self.accept.checked_add(1).ok_or(StrategyError::Overflow)?;
        }
        self.live = true;
        Ok(Some(self.emit(&ready)))
    }

    fn on_hour(&self, decision_us: i64) -> Result<bool, StrategyError> {
        match self.decision_hour {
            None => Ok(true),
            Some(hour) => Ok(utc_hour(decision_us)? == hour),
        }
    }

    fn emit(&self, ready: &LegOut) -> parangsae_signals::SignalOutput {
        let mut output = emitted(&ready.bar, ready.decision_us, self.held);
        output.accept = self.accept;
        output
    }
}

pub(crate) struct TsmomLeg {
    interval: BarInterval,
    n: u16,
    r_scale: f64,
    cursor: Cursor,
    ring: Ring,
    last: Target,
    ready: bool,
}

impl TsmomLeg {
    pub(crate) fn new(interval: BarInterval, n: u16, c_ticks: i64) -> Result<Self, StrategyError> {
        if !n_supported(n) {
            return Err(StrategyError::Param("n"));
        }
        let r_scale = r_scale(interval, n, c_ticks)?;
        Ok(Self {
            interval,
            n,
            r_scale,
            cursor: Cursor::new(interval),
            ring: Ring::new(usize::from(n) + 1),
            last: Target::FLAT,
            ready: false,
        })
    }

    pub(crate) fn ready(&self) -> bool {
        self.ready
    }

    pub(crate) fn last(&self) -> Target {
        self.last
    }

    pub(crate) fn push_row(
        &mut self,
        row: &Observation<ResampledBar>,
    ) -> Result<Option<LegOut>, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view);
        let bar = *view.value();
        self.ring.push(row.clone());
        let rows = self.ring.to_vec();
        let decision = DecisionTime::from_micros(UtcMicros::from_micros(decision_us));
        let raw = drive_n(self.n, self.interval, self.r_scale, &rows, decision)?;
        if let Some(raw) = raw {
            let target = match raw.decision {
                Decision::Target(target) => target,
                Decision::Flat(_) => return Err(StrategyError::Param("tsmom")),
            };
            self.last = target;
            self.ready = true;
            return Ok(Some(LegOut {
                bar,
                decision_us,
                target,
            }));
        }
        Ok(None)
    }
}

pub(crate) struct LegOut {
    pub bar: ResampledBar,
    pub decision_us: i64,
    pub target: Target,
}

fn signed_weight(book: DecisionBook) -> Result<i64, StrategyError> {
    if book.equity_ticks <= 0 || book.mark_ticks <= 0 {
        return Err(StrategyError::Price);
    }
    let weight = i128::from(book.position_lots)
        .checked_mul(i128::from(book.mark_ticks))
        .ok_or(StrategyError::Overflow)?
        / i128::from(book.equity_ticks);
    i64::try_from(weight).map_err(|_| StrategyError::Overflow)
}

fn passes_deadband(held: i64, next: i64, deadband: i64) -> bool {
    let diff = i128::from(next) - i128::from(held);
    diff.abs() >= i128::from(deadband)
}

fn r_scale(interval: BarInterval, n: u16, c_ticks: i64) -> Result<f64, StrategyError> {
    let c = ticks_to_unit(c_ticks)?;
    if !c.is_finite() || c <= 0.0 {
        return Err(StrategyError::Param("c"));
    }
    let (r0, n0) = match interval {
        BarInterval::H1 => (0.05, 24.0),
        BarInterval::H4 => (0.10, 42.0),
    };
    let scale = c * r0 * sqrt(f64::from(n) / n0);
    if scale.is_finite() && scale > 0.0 {
        Ok(scale)
    } else {
        Err(StrategyError::Param("c"))
    }
}

fn n_supported(n: u16) -> bool {
    matches!(n, 24 | 42 | 72 | 90 | 168 | 180 | 360)
}

fn drive_n(
    n: u16,
    interval: BarInterval,
    r_scale: f64,
    rows: &[Observation<ResampledBar>],
    decision: DecisionTime,
) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
    match n {
        24 => drive(&Tsmom::<24>::new(interval, r_scale)?, rows, decision).map_err(Into::into),
        42 => drive(&Tsmom::<42>::new(interval, r_scale)?, rows, decision).map_err(Into::into),
        72 => drive(&Tsmom::<72>::new(interval, r_scale)?, rows, decision).map_err(Into::into),
        90 => drive(&Tsmom::<90>::new(interval, r_scale)?, rows, decision).map_err(Into::into),
        168 => drive(&Tsmom::<168>::new(interval, r_scale)?, rows, decision).map_err(Into::into),
        180 => drive(&Tsmom::<180>::new(interval, r_scale)?, rows, decision).map_err(Into::into),
        360 => drive(&Tsmom::<360>::new(interval, r_scale)?, rows, decision).map_err(Into::into),
        _ => Err(StrategyError::Param("n")),
    }
}
