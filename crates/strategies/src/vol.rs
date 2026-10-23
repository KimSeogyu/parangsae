//! `volregime` on the capped closed-bar ring. The rank stays a feature.

use parangsae_core::{BarInterval, DecisionTime, Observation, ResampledBar, UtcMicros};
use parangsae_signals::{drive_feature, VolRegime};

use crate::error::StrategyError;

pub fn rank_of(
    vol: &VolRegime<24, 168>,
    rows: &[Observation<ResampledBar>],
) -> Result<Option<i64>, StrategyError> {
    let Some(last) = rows.last() else {
        return Ok(None);
    };
    let decision_us = last.timestamps().available_ts.micros().micros();
    let decision = DecisionTime::from_micros(UtcMicros::from_micros(decision_us));
    match drive_feature(vol, rows, decision)? {
        Some(out) => {
            if out.interval != BarInterval::H1 {
                return Err(StrategyError::Interval);
            }
            Ok(Some(out.value.ticks()))
        }
        None => Ok(None),
    }
}
