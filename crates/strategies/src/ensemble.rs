//! H2 equal-weight blend of the twelve S01 members. Member deadbands are not applied.

use std::collections::BTreeMap;

use parangsae_core::{BarInterval, Observation, Price, ResampledBar};
use parangsae_signals::Target;

use crate::error::StrategyError;
use crate::series::{decision_micros, emitted, utc_hour, Cursor, DecisionBook, WARMUP_30D_US};
use crate::tsmom::TsmomLeg;

/// `(interval minutes, n bars, c ticks)` of S01 grid points 0..11.
const MEMBERS: [(i64, u16, i64); 12] = [
    (60, 24, 50_000_000),
    (60, 24, 100_000_000),
    (60, 72, 50_000_000),
    (60, 72, 100_000_000),
    (60, 168, 50_000_000),
    (60, 168, 100_000_000),
    (240, 42, 50_000_000),
    (240, 42, 100_000_000),
    (240, 90, 50_000_000),
    (240, 90, 100_000_000),
    (240, 180, 50_000_000),
    (240, 180, 100_000_000),
];

pub struct Ensemble {
    cursor: Cursor,
    legs: Vec<Member>,
    h1_chunk: Vec<Observation<ResampledBar>>,
    deadband: i64,
    held: Target,
    accept: u64,
    live: bool,
}

struct Member {
    h4: bool,
    leg: TsmomLeg,
}

impl Ensemble {
    pub fn from_params(params: &BTreeMap<String, i64>) -> Result<Self, StrategyError> {
        let n_members = *params
            .get("n_members")
            .ok_or(StrategyError::Param("n_members"))?;
        if n_members != i64::try_from(MEMBERS.len()).map_err(|_| StrategyError::Overflow)? {
            return Err(StrategyError::Param("n_members"));
        }
        let deadband = *params
            .get("deadband")
            .ok_or(StrategyError::Param("deadband"))?;
        if deadband <= 0 {
            return Err(StrategyError::Param("deadband"));
        }
        let mut legs = Vec::with_capacity(MEMBERS.len());
        for (minutes, n, c) in MEMBERS {
            let interval = match minutes {
                60 => BarInterval::H1,
                240 => BarInterval::H4,
                _ => return Err(StrategyError::Param("interval")),
            };
            legs.push(Member {
                h4: minutes == 240,
                leg: TsmomLeg::new(interval, n, c)?,
            });
        }
        Ok(Self {
            cursor: Cursor::new(BarInterval::H1),
            legs,
            h1_chunk: Vec::new(),
            deadband,
            held: Target::FLAT,
            accept: 0,
            live: false,
        })
    }

    pub fn observe(&mut self, row: &Observation<ResampledBar>) -> Result<(), StrategyError> {
        let _ready = self.ingest(row)?;
        Ok(())
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        book: Option<DecisionBook>,
    ) -> Result<Option<parangsae_signals::SignalOutput>, StrategyError> {
        let Some(ready) = self.ingest(row)? else {
            return Ok(None);
        };
        if ready.decision_us < WARMUP_30D_US {
            return Ok(None);
        }
        let blended = mean_target(&self.legs)?;
        let current = match book {
            Some(book) => signed_weight(book)?,
            None => self.held.ticks(),
        };
        if (i128::from(blended.ticks()) - i128::from(current)).abs() >= i128::from(self.deadband) {
            self.held = blended;
            self.accept = self.accept.checked_add(1).ok_or(StrategyError::Overflow)?;
        }
        self.live = true;
        let mut output = emitted(&ready.bar, ready.decision_us, self.held);
        output.accept = self.accept;
        Ok(Some(output))
    }

    fn ingest(&mut self, row: &Observation<ResampledBar>) -> Result<Option<Ready>, StrategyError> {
        let view = self.cursor.admit(row)?;
        let decision_us = decision_micros(&view);
        let bar = *view.value();
        for member in &mut self.legs {
            if !member.h4 {
                let _ = member.leg.push_row(row)?;
            }
        }
        self.feed_h4(row)?;
        if self.legs.iter().all(|member| member.leg.ready()) {
            Ok(Some(Ready { bar, decision_us }))
        } else {
            Ok(None)
        }
    }

    fn feed_h4(&mut self, row: &Observation<ResampledBar>) -> Result<(), StrategyError> {
        let open_hour = utc_hour(row.value().open_time().micros())?;
        if self.h1_chunk.is_empty() && open_hour % 4 != 0 {
            return Ok(());
        }
        self.h1_chunk.push(row.clone());
        if self.h1_chunk.len() < 4 {
            return Ok(());
        }
        let chunk = std::mem::take(&mut self.h1_chunk);
        let decision_hour = utc_hour(decision_micros_of(&chunk[3]))?;
        if decision_hour % 4 != 0 {
            return Err(StrategyError::Clock);
        }
        let synthetic = h4_bar(&chunk)?;
        for member in &mut self.legs {
            if member.h4 {
                let _ = member.leg.push_row(&synthetic)?;
            }
        }
        Ok(())
    }
}

struct Ready {
    bar: ResampledBar,
    decision_us: i64,
}

fn decision_micros_of(row: &Observation<ResampledBar>) -> i64 {
    row.timestamps().available_ts.micros().micros()
}

fn h4_bar(hours: &[Observation<ResampledBar>]) -> Result<Observation<ResampledBar>, StrategyError> {
    let open = hours[0].value().open();
    let close = hours[3].value().close();
    let mut high = hours[0].value().high().ticks();
    let mut low = hours[0].value().low().ticks();
    for row in hours {
        high = high.max(row.value().high().ticks());
        low = low.min(row.value().low().ticks());
    }
    let bar = ResampledBar::new(
        hours[0].value().open_time(),
        BarInterval::H4,
        open,
        Price::from_ticks(high).map_err(|_| StrategyError::Price)?,
        Price::from_ticks(low).map_err(|_| StrategyError::Price)?,
        close,
    )
    .map_err(|_| StrategyError::Price)?;
    Ok(Observation::new(hours[3].timestamps(), bar))
}

fn mean_target(legs: &[Member]) -> Result<Target, StrategyError> {
    let mut sum = 0i128;
    for member in legs {
        sum = sum
            .checked_add(i128::from(member.leg.last().ticks()))
            .ok_or(StrategyError::Overflow)?;
    }
    let width = i128::try_from(legs.len()).map_err(|_| StrategyError::Overflow)?;
    let mean = sum / width;
    let ticks = i64::try_from(mean).map_err(|_| StrategyError::Overflow)?;
    Target::from_ticks(ticks).map_err(|_| StrategyError::Price)
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
