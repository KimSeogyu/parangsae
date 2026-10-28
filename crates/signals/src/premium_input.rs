//! Signal-facing premium data. This index ratio is not a traded price or volume.

use std::fmt;

use parangsae_core::{Ratio, UtcMicros};

const MINUTE_US: i64 = 60_000_000;

/// One signed premium close at an exact UTC minute start.
///
/// Source schema, clocks and delivery remain the source adapter's responsibility.
/// A caller supplies an [`parangsae_core::Observation`] to obtain a causal view;
/// this value does not manufacture availability or reception timestamps.
///
/// Opaque source fields are deliberately absent from the signal interface.
///
/// ```compile_fail
/// use parangsae_core::{Ratio, UtcMicros};
/// use parangsae_signals::premium_input::PremiumMinuteInput;
/// let input = PremiumMinuteInput::try_new(UtcMicros::from_micros(0), Ratio::from_ticks(0)).expect("minute");
/// let _ = input.count();
/// ```
///
/// ```compile_fail
/// use parangsae_core::{Ratio, UtcMicros};
/// use parangsae_signals::premium_input::PremiumMinuteInput;
/// let input = PremiumMinuteInput::try_new(UtcMicros::from_micros(0), Ratio::from_ticks(0)).expect("minute");
/// let _ = input.samples();
/// ```
///
/// ```compile_fail
/// use parangsae_core::{Ratio, UtcMicros};
/// use parangsae_signals::premium_input::PremiumMinuteInput;
/// let input = PremiumMinuteInput::try_new(UtcMicros::from_micros(0), Ratio::from_ticks(0)).expect("minute");
/// let _ = input.volume();
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PremiumMinuteInput {
    minute_start: UtcMicros,
    close: Ratio,
}

/// Failure to construct a premium minute value. Study bounds belong to the reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PremiumInputError {
    OffMinuteGrid { minute_start: UtcMicros },
}

impl fmt::Display for PremiumInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OffMinuteGrid { minute_start } => {
                write!(
                    f,
                    "premium input time {minute_start} is not an exact UTC minute"
                )
            }
        }
    }
}

impl std::error::Error for PremiumInputError {}

impl PremiumMinuteInput {
    /// Preserve the signed ratio. Zero is a value, not an inferred missing row.
    pub fn try_new(minute_start: UtcMicros, close: Ratio) -> Result<Self, PremiumInputError> {
        if minute_start.micros() % MINUTE_US != 0 {
            return Err(PremiumInputError::OffMinuteGrid { minute_start });
        }
        Ok(Self {
            minute_start,
            close,
        })
    }

    pub const fn minute_start(self) -> UtcMicros {
        self.minute_start
    }

    pub const fn close(self) -> Ratio {
        self.close
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{AvailableTs, DecisionTime, EventTs, Observation, ReceivedTs, Timestamps};

    #[test]
    fn the_entire_signed_ratio_domain_is_preserved() {
        let minute = UtcMicros::from_micros(1_608_249_600_000_000);
        for ticks in [i64::MIN, -1, 0, 1, i64::MAX] {
            let input =
                PremiumMinuteInput::try_new(minute, Ratio::from_ticks(ticks)).expect("minute");
            assert_eq!(input.minute_start(), minute);
            assert_eq!(input.close().ticks(), ticks);
        }
    }

    #[test]
    fn misaligned_times_are_typed_errors_without_rounding() {
        for time in [-1, 1, MINUTE_US - 1, MINUTE_US + 1] {
            let minute_start = UtcMicros::from_micros(time);
            assert_eq!(
                PremiumMinuteInput::try_new(minute_start, Ratio::from_ticks(0)),
                Err(PremiumInputError::OffMinuteGrid { minute_start })
            );
        }
    }

    #[test]
    fn study_bounds_are_not_a_signal_value_constructor_policy() {
        for time in [-MINUTE_US, 0, MINUTE_US] {
            let minute = UtcMicros::from_micros(time);
            let input = PremiumMinuteInput::try_new(minute, Ratio::from_ticks(-1)).expect("minute");
            assert_eq!(input.minute_start(), minute);
        }
    }

    #[test]
    fn a_causal_view_keeps_the_source_observation_clocks_unchanged() {
        let minute = UtcMicros::from_micros(1_608_249_600_000_000);
        let end = minute.checked_add_micros(MINUTE_US).expect("end");
        let recorded = UtcMicros::from_micros(1_792_776_118_572_302);
        let timestamps = Timestamps {
            event_ts: EventTs::from_micros(minute),
            available_ts: AvailableTs::from_micros(end),
            received_ts: ReceivedTs::from_micros(recorded),
        };
        let input = PremiumMinuteInput::try_new(minute, Ratio::from_ticks(0)).expect("minute");
        let observation = Observation::new(timestamps, input);
        assert!(observation
            .causal_at(DecisionTime::from_micros(
                end.checked_add_micros(-1).expect("before")
            ))
            .is_none());
        let view = observation
            .causal_at(DecisionTime::from_micros(end))
            .expect("available");
        assert_eq!(view.value().close().ticks(), 0);
        assert_eq!(observation.timestamps(), timestamps);
    }
}
