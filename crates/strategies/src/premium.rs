//! H5 premium reversal.
//!
//! `P_d` is the mean of 1m premium closes, in unit premium, over minutes with
//! `count > 0`, when that count is at least 1,380. A close of 0 is a premium of 0.
//! `z_d` uses the previous `lookback_d` calendar days' non-missing values and
//! needs at least 80 of them. A high z is short. Holds do not flip or extend.

use std::collections::{BTreeMap, VecDeque};

use parangsae_core::{BarInterval, Observation, ResampledBar};

use crate::aux::PremiumMinute;
use crate::error::StrategyError;
use crate::series::{
    decision_micros, emitted, sample_sd, side_target, signed_unit, ticks_to_unit, Cursor, Hold,
    WARMUP_91D_US,
};

const DAY_US: i64 = 86_400_000_000;
const MIN_VALID_MINUTES: u32 = 1_380;
const MIN_DAYS: usize = 80;

pub struct PremiumReversal {
    lookback: usize,
    k: f64,
    cursor: Cursor,
    days: VecDeque<(i64, Option<f64>)>,
    acc: Option<DayAcc>,
    last_open: Option<i64>,
    hold: Hold,
}

struct DayAcc {
    day_open: i64,
    sum: f64,
    valid: u32,
}

impl PremiumReversal {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let lookback_d = *params
            .get("lookback_d")
            .ok_or(StrategyError::Param("lookback_d"))?;
        if lookback_d <= 0 {
            return Err(StrategyError::Param("lookback_d"));
        }
        let lookback =
            usize::try_from(lookback_d).map_err(|_| StrategyError::Param("lookback_d"))?;
        let k_ticks = *params.get("k").ok_or(StrategyError::Param("k"))?;
        let k = ticks_to_unit(k_ticks)?;
        if k <= 0.0 {
            return Err(StrategyError::Param("k"));
        }
        let hold_h = *params.get("hold_h").ok_or(StrategyError::Param("hold_h"))?;
        Ok(Self {
            lookback,
            k,
            cursor: Cursor::new(BarInterval::H1),
            days: VecDeque::new(),
            acc: None,
            last_open: None,
            hold: Hold::new(hold_h, false)?,
        })
    }

    pub fn ingest_premium(&mut self, rows: &[PremiumMinute]) -> Result<(), StrategyError> {
        let mut ordered = rows.to_vec();
        ordered.sort_by_key(|row| (row.open_us, row.available_us));
        for row in ordered {
            if self.last_open.is_some_and(|open| open >= row.open_us) {
                continue;
            }
            self.push_minute(row)?;
            self.last_open = Some(row.open_us);
        }
        Ok(())
    }

    pub fn observe(&mut self, row: &Observation<ResampledBar>) -> Result<(), StrategyError> {
        let _ready = self.features(row)?;
        Ok(())
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        _book: Option<crate::DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let ready = self.features(row)?;
        if ready.decision_us < WARMUP_91D_US || self.days.len() < self.lookback {
            return Ok(None);
        }
        let midnight = ready.decision_us % DAY_US == 0;
        let occupied = self.hold.side() != 0;
        let signal = if midnight && !occupied {
            self.entry_side(ready.decision_us)?
        } else {
            0
        };
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

    fn features(&mut self, row: &Observation<ResampledBar>) -> Result<Ready, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view);
        let bar = *view.value();
        self.close_elapsed(decision_us)?;
        Ok(Ready { bar, decision_us })
    }

    fn push_minute(&mut self, row: PremiumMinute) -> Result<(), StrategyError> {
        let day_open = row.open_us - row.open_us.rem_euclid(DAY_US);
        if let Some(acc) = &self.acc {
            if acc.day_open != day_open {
                self.finish_acc()?;
            }
        }
        if self.acc.is_none() {
            self.acc = Some(DayAcc {
                day_open,
                sum: 0.0,
                valid: 0,
            });
        }
        let acc = self.acc.as_mut().ok_or(StrategyError::Overflow)?;
        if row.count > 0 {
            let unit = signed_unit(row.close_ticks)? / 100_000_000.0;
            acc.sum += unit;
            acc.valid = acc.valid.saturating_add(1);
        }
        Ok(())
    }

    fn close_elapsed(&mut self, decision_us: i64) -> Result<(), StrategyError> {
        let finished = self
            .acc
            .as_ref()
            .is_some_and(|acc| decision_us >= acc.day_open + DAY_US);
        if finished {
            self.finish_acc()?;
        }
        Ok(())
    }

    fn finish_acc(&mut self) -> Result<(), StrategyError> {
        let Some(acc) = self.acc.take() else {
            return Ok(());
        };
        let value = if acc.valid >= MIN_VALID_MINUTES {
            let width = f64::from(acc.valid);
            let mean = acc.sum / width;
            if mean.is_finite() {
                Some(mean)
            } else {
                None
            }
        } else {
            None
        };
        self.store_day(acc.day_open, value);
        Ok(())
    }

    fn store_day(&mut self, day_open: i64, value: Option<f64>) {
        if let Some((prev, _)) = self.days.back().copied() {
            let mut cursor = prev + DAY_US;
            while cursor < day_open {
                self.days.push_back((cursor, None));
                cursor += DAY_US;
            }
        }
        self.days.push_back((day_open, value));
        let cap = self.lookback.saturating_add(2);
        while self.days.len() > cap {
            self.days.pop_front();
        }
    }

    fn entry_side(&self, decision_us: i64) -> Result<i8, StrategyError> {
        let Some(day_open) = decision_us.checked_sub(DAY_US) else {
            return Ok(0);
        };
        let Some(premium) = self.day_value(day_open) else {
            return Ok(0);
        };
        let Some(z) = self.z_score(day_open, premium) else {
            return Ok(0);
        };
        if z >= self.k {
            Ok(-1)
        } else if z <= -self.k {
            Ok(1)
        } else {
            Ok(0)
        }
    }

    fn day_value(&self, day_open: i64) -> Option<f64> {
        self.days
            .iter()
            .rev()
            .find(|(open, _)| *open == day_open)
            .and_then(|(_, value)| *value)
    }

    fn z_score(&self, day_open: i64, premium: f64) -> Option<f64> {
        let mut values = VecDeque::new();
        for step in 1..=self.lookback {
            let open = day_open.checked_sub(DAY_US * i64::try_from(step).ok()?)?;
            if let Some(value) = self.day_value(open) {
                values.push_back(value);
            }
        }
        if values.len() < MIN_DAYS {
            return None;
        }
        let sd = sample_sd(&values)?;
        let mut sum = 0.0;
        for value in &values {
            sum += *value;
        }
        let width = f64::from(u32::try_from(values.len()).ok()?);
        let mean = sum / width;
        let z = (premium - mean) / sd;
        if z.is_finite() {
            Some(z)
        } else {
            None
        }
    }
}

struct Ready {
    bar: ResampledBar,
    decision_us: i64,
}
