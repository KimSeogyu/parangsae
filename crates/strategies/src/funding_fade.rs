//! H4 funding-crowding fade.
//!
//! `F24` is the sum of the last `sum_settlements` rates. A settlement is
//! long-crowded when that sum is positive and strictly above the `(1 - q)`
//! type-7 quantile of F24 over `[ft - 365d, ft)`, and short-crowded on the
//! mirror. The fade waits `confirm_h` and trades only when the H1 close
//! moved against the crowd. An open position skips the event.

use std::collections::{BTreeMap, VecDeque};

use parangsae_core::{BarInterval, Observation, ResampledBar};

use crate::aux::FundingPrint;
use crate::error::StrategyError;
use crate::quantile::type7;
use crate::series::{
    decision_micros, emitted, log_ratio, price_f64, side_target, signed_unit, ticks_to_unit,
    Cursor, Hold, WARMUP_366D_US,
};

const DAY_US: i64 = 86_400_000_000;
const HOUR_US: i64 = 3_600_000_000;
const WINDOW_DAYS: i64 = 365;
const WINDOW_COUNT: usize = 1_095;

pub struct FundingFade {
    q: f64,
    sum_n: usize,
    confirm_us: i64,
    cursor: Cursor,
    rates: VecDeque<(i64, f64)>,
    f24: VecDeque<(i64, f64)>,
    closes: VecDeque<(i64, f64)>,
    hold: Hold,
}

impl FundingFade {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let q_ticks = *params.get("q").ok_or(StrategyError::Param("q"))?;
        let q = ticks_to_unit(q_ticks)?;
        if !(0.0..1.0).contains(&q) {
            return Err(StrategyError::Param("q"));
        }
        let sum_n = positive(
            *params
                .get("sum_settlements")
                .ok_or(StrategyError::Param("sum_settlements"))?,
            "sum_settlements",
        )?;
        let confirm_h = *params
            .get("confirm_h")
            .ok_or(StrategyError::Param("confirm_h"))?;
        if confirm_h <= 0 {
            return Err(StrategyError::Param("confirm_h"));
        }
        let confirm_us = confirm_h
            .checked_mul(HOUR_US)
            .ok_or(StrategyError::Overflow)?;
        let hold_h = *params.get("hold_h").ok_or(StrategyError::Param("hold_h"))?;
        let close_cap = usize::try_from(confirm_h)
            .map_err(|_| StrategyError::Param("confirm_h"))?
            .saturating_add(2);
        Ok(Self {
            q,
            sum_n,
            confirm_us,
            cursor: Cursor::new(BarInterval::H1),
            rates: VecDeque::new(),
            f24: VecDeque::new(),
            closes: VecDeque::with_capacity(close_cap),
            hold: Hold::new(hold_h, false)?,
        })
    }

    pub fn ingest_funding(&mut self, rows: &[FundingPrint]) -> Result<(), StrategyError> {
        let mut ordered = rows.to_vec();
        ordered.sort_by_key(|row| row.funding_time_us);
        for row in ordered {
            if self
                .rates
                .back()
                .is_some_and(|(time, _)| *time >= row.funding_time_us)
            {
                continue;
            }
            let rate = signed_unit(row.rate_ticks)? / 100_000_000.0;
            self.rates.push_back((row.funding_time_us, rate));
            if self.rates.len() >= self.sum_n {
                let mut sum = 0.0;
                for (_, value) in self.rates.iter().rev().take(self.sum_n) {
                    sum += *value;
                }
                if sum.is_finite() {
                    self.f24.push_back((row.funding_time_us, sum));
                }
            }
            let keep = self.sum_n.saturating_add(WINDOW_COUNT).saturating_add(4);
            while self.f24.len() > keep {
                self.f24.pop_front();
            }
            while self.rates.len() > keep {
                self.rates.pop_front();
            }
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
        if ready.decision_us < WARMUP_366D_US || self.rates.len() < self.sum_n {
            return Ok(None);
        }
        let occupied = self.hold.side() != 0;
        let signal = if occupied {
            0
        } else {
            self.event_side(ready.decision_us, ready.close)?
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
        let close = price_f64(bar.close())?;
        self.closes.push_back((decision_us, close));
        let cap = usize::try_from(self.confirm_us / HOUR_US)
            .map_err(|_| StrategyError::Overflow)?
            .saturating_add(2);
        while self.closes.len() > cap {
            self.closes.pop_front();
        }
        Ok(Ready {
            bar,
            decision_us,
            close,
        })
    }

    fn event_side(&self, decision_us: i64, close: f64) -> Result<i8, StrategyError> {
        let Some(ft) = decision_us.checked_sub(self.confirm_us) else {
            return Ok(0);
        };
        let Some(f24) = self.f24_at(ft) else {
            return Ok(0);
        };
        let Some(window_lo) = ft.checked_sub(WINDOW_DAYS * DAY_US) else {
            return Ok(0);
        };
        let mut sample = Vec::new();
        for (time, value) in &self.f24 {
            if *time >= window_lo && *time < ft {
                sample.push(*value);
            }
        }
        if sample.len() != WINDOW_COUNT {
            return Ok(0);
        }
        let q_lo = type7(&sample, self.q)?;
        let q_hi = type7(&sample, 1.0 - self.q)?;
        let long_crowded = f24 > 0.0 && f24 > q_hi;
        let short_crowded = f24 < 0.0 && f24 < q_lo;
        if !long_crowded && !short_crowded {
            return Ok(0);
        }
        let Some(past) = self.close_at(ft) else {
            return Ok(0);
        };
        let rc = log_ratio(close, past)?;
        if long_crowded && rc < 0.0 {
            Ok(-1)
        } else if short_crowded && rc > 0.0 {
            Ok(1)
        } else {
            Ok(0)
        }
    }

    fn f24_at(&self, ft: i64) -> Option<f64> {
        self.f24
            .iter()
            .rev()
            .find(|(time, _)| *time == ft)
            .map(|(_, value)| *value)
    }

    fn close_at(&self, decision_us: i64) -> Option<f64> {
        self.closes
            .iter()
            .rev()
            .find(|(time, _)| *time == decision_us)
            .map(|(_, value)| *value)
    }
}

struct Ready {
    bar: ResampledBar,
    decision_us: i64,
    close: f64,
}

fn positive(value: i64, name: &'static str) -> Result<usize, StrategyError> {
    if value <= 0 {
        return Err(StrategyError::Param(name));
    }
    usize::try_from(value).map_err(|_| StrategyError::Param(name))
}
