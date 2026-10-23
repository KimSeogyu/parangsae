//! S01 `tsmom-slow`. The scaled return is `signals::Tsmom`. The 0.25 band is local state.

use std::collections::BTreeMap;

use libm::sqrt;
use parangsae_core::{BarInterval, DecisionTime, Observation, ResampledBar, UtcMicros};
use parangsae_signals::{drive, Decision, Target, Tsmom};

use crate::error::StrategyError;
use crate::series::{
    decision_micros, emitted, ticks_to_unit, Cursor, DecisionBook, Ring, WARMUP_30D_US,
};

const DEADBAND_TICKS: i64 = 25_000_000;

pub struct TsmomSlow {
    interval: BarInterval,
    n: u16,
    r_scale: f64,
    cursor: Cursor,
    ring: Ring,
    held: Target,
}

impl TsmomSlow {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
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
        let r_scale = r_scale(interval, n, c)?;
        let cap = usize::from(n) + 1;
        Ok(Self {
            interval,
            n,
            r_scale,
            cursor: Cursor::new(interval),
            ring: Ring::new(cap),
            held: Target::FLAT,
        })
    }

    pub fn interval(&self) -> BarInterval {
        self.interval
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        book: Option<DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view);
        self.ring.push(row.clone());
        let rows = self.ring.to_vec();
        let decision = DecisionTime::from_micros(UtcMicros::from_micros(decision_us));
        let raw = drive_n(self.n, self.interval, self.r_scale, &rows, decision)?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        if decision_us < WARMUP_30D_US {
            return Ok(None);
        }
        let next = match raw.decision {
            Decision::Target(target) => target,
            Decision::Flat(_) => return Err(StrategyError::Param("tsmom")),
        };
        let current = match book {
            Some(book) => signed_weight(book)?,
            None => self.held.ticks(),
        };
        if passes_deadband(current, next.ticks()) {
            self.held = next;
        }
        Ok(Some(emitted(view.value(), decision_us, self.held)))
    }
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

fn passes_deadband(held: i64, next: i64) -> bool {
    let diff = i128::from(next) - i128::from(held);
    diff.abs() >= i128::from(DEADBAND_TICKS)
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
    matches!(n, 24 | 42 | 72 | 90 | 168 | 180)
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
        _ => Err(StrategyError::Param("n")),
    }
}
