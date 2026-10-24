//! Strategy policy consumes execution feedback; feature evaluation does not own fills.

use parangsae_core::{BarInterval, DecisionTime, UtcMicros};
use parangsae_signals::{Decision, SignalOutput, Target};

use crate::StrategyError;

const HOUR_US: i64 = 3_600_000_000;
const DECISION_OFFSET_US: i64 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionSide {
    Long,
    Short,
}

impl PositionSide {
    pub const fn opposite(self) -> Self {
        match self {
            Self::Long => Self::Short,
            Self::Short => Self::Long,
        }
    }

    pub const fn target_ticks(self) -> i64 {
        match self {
            Self::Long => 100_000_000,
            Self::Short => -100_000_000,
        }
    }

    pub(crate) fn target(self) -> Result<Target, StrategyError> {
        Target::from_ticks(self.target_ticks()).map_err(|_| StrategyError::Price)
    }
}

/// The runtime reports actual fills and pending requests at the decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPhase {
    Flat,
    EntryPending {
        side: PositionSide,
    },
    Open {
        side: PositionSide,
        entered_at: UtcMicros,
    },
    ExitPending {
        side: PositionSide,
        entered_at: UtcMicros,
    },
    Adjusting {
        side: PositionSide,
        entered_at: UtcMicros,
    },
}

/// A duration. Its exit clock is anchored only to an actual entry supplied by the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActualHold(i64);

impl ActualHold {
    pub fn from_hours(hours: i64) -> Result<Self, StrategyError> {
        if hours <= 0 {
            return Err(StrategyError::Param("hold_h"));
        }
        hours
            .checked_mul(HOUR_US)
            .map(Self)
            .ok_or(StrategyError::Overflow)
    }

    pub const fn micros(self) -> i64 {
        self.0
    }

    /// First UTC H1 decision (hour boundary + 1 ms) at or after entry + H.
    pub fn exit_decision(self, actual_entry: UtcMicros) -> Result<DecisionTime, StrategyError> {
        let deadline = actual_entry
            .micros()
            .checked_add(self.0)
            .ok_or(StrategyError::Overflow)?;
        let hour = deadline
            .div_euclid(HOUR_US)
            .checked_mul(HOUR_US)
            .ok_or(StrategyError::Overflow)?;
        let first = hour
            .checked_add(DECISION_OFFSET_US)
            .ok_or(StrategyError::Overflow)?;
        let exit = if first < deadline {
            first.checked_add(HOUR_US).ok_or(StrategyError::Overflow)?
        } else {
            first
        };
        Ok(DecisionTime::from_micros(UtcMicros::from_micros(exit)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionDirective {
    SetTarget,
    Enter {
        event: H4Event,
        hold: ActualHold,
    },
    /// Preserve the runtime's current order and quantity, regardless of diagnostic target.
    Keep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrategyOutput {
    pub signal: SignalOutput,
    pub directive: ExecutionDirective,
}

impl StrategyOutput {
    pub const fn target(signal: SignalOutput) -> Self {
        Self {
            signal,
            directive: ExecutionDirective::SetTarget,
        }
    }
}

/// A causal H4 feature result. It contains no assumed fill, position or hold timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct H4Opportunity {
    pub event: H4Event,
    pub decision_ts: DecisionTime,
    pub bar_close_time_us: UtcMicros,
}

impl H4Opportunity {
    pub const fn side(self) -> PositionSide {
        self.event.crowd_side.opposite()
    }

    pub fn signal(self) -> Result<SignalOutput, StrategyError> {
        Ok(SignalOutput {
            decision_ts: self.decision_ts,
            bar_close_time_us: self.bar_close_time_us,
            interval: BarInterval::H1,
            decision: Decision::Target(self.side().target()?),
            accept: 0,
        })
    }
}

/// Origin identity survives pending orders, fills and later accounting reduction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct H4Event {
    pub settlement: UtcMicros,
    pub episode: UtcMicros,
    pub crowd_side: PositionSide,
    pub gap_boundary: bool,
}

pub const H4_OBSERVATIONS_PER_EVENT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H4ObservationKind {
    Tail,
    Confirmation { confirmed: bool },
    ConfirmationUnavailable,
    OccupiedSkip,
}

/// `observed_at` is a feature evaluation time, not a claimed source publication time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct H4Observation {
    pub event: H4Event,
    pub observed_at: DecisionTime,
    pub kind: H4ObservationKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_hold_uses_the_first_decision_after_the_actual_entry_deadline() {
        let hold = ActualHold::from_hours(1).unwrap();
        for (entry, expected) in [
            (0, HOUR_US + 1_000),
            (1_000, HOUR_US + 1_000),
            (60_000_000, 2 * HOUR_US + 1_000),
        ] {
            assert_eq!(
                hold.exit_decision(UtcMicros::from_micros(entry))
                    .unwrap()
                    .micros()
                    .micros(),
                expected
            );
        }
        assert_eq!(
            ActualHold::from_hours(0),
            Err(StrategyError::Param("hold_h"))
        );
        assert_eq!(
            ActualHold::from_hours(i64::MAX),
            Err(StrategyError::Overflow)
        );
        assert_eq!(
            hold.exit_decision(UtcMicros::from_micros(i64::MAX)),
            Err(StrategyError::Overflow)
        );
    }
}
