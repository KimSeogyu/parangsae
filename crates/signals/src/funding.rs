//! Funding rows. Visibility is applied here and nowhere else.

use parangsae_core::{AvailableTs, EventTs, Observation, Ratio, ReceivedTs, Timestamps, UtcMicros};

use crate::output::SignalError;

/// The only +60s visibility offset in this crate.
const VISIBILITY_LAG_US: i64 = 60_000_000;

/// One funding settlement. `interval_hours` is checked at evaluation, not here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundingRow {
    funding_time: UtcMicros,
    rate: Ratio,
    interval_hours: u8,
}

impl FundingRow {
    pub const fn new(funding_time: UtcMicros, rate: Ratio, interval_hours: u8) -> Self {
        Self {
            funding_time,
            rate,
            interval_hours,
        }
    }

    pub const fn funding_time(self) -> UtcMicros {
        self.funding_time
    }

    pub const fn rate(self) -> Ratio {
        self.rate
    }

    pub const fn interval_hours(self) -> u8 {
        self.interval_hours
    }

    pub const fn with_rate(self, rate: Ratio) -> Self {
        Self { rate, ..self }
    }
}

/// Stamp `event_ts = funding_time` and `available_ts = funding_time + 60s`.
///
/// Grid alignment and `interval_hours` are not validated. Those errors are
/// causal: they fire when the row becomes visible.
pub fn funding_observation(
    funding_time: UtcMicros,
    rate: Ratio,
    interval_hours: u8,
    received: ReceivedTs,
) -> Result<Observation<FundingRow>, SignalError> {
    let available = funding_time
        .checked_add_micros(VISIBILITY_LAG_US)
        .ok_or(SignalError::Overflow)?;
    Ok(Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(funding_time),
            available_ts: AvailableTs::from_micros(available),
            received_ts: received,
        },
        FundingRow {
            funding_time,
            rate,
            interval_hours,
        },
    ))
}
