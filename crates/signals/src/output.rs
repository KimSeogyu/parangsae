//! Engine-facing signal result.
//!
//! [`Decision::Flat`] is an emitted output with position 0. It is only used for
//! funding staleness and a missing funding slot. A constant funding window is
//! [`Decision::Target`] of [`crate::Target::FLAT`], not [`Decision::Flat`].

use parangsae_core::{BarInterval, DecisionTime, UtcMicros};
use std::fmt;

use crate::rank::RankError;
use crate::target::{Target, TargetError};

/// A view rejected while building a causal window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowError {
    /// `available_ts` is after the decision, including a row the cap would drop.
    AfterDecision,
    /// `available_ts` decreased. Equal timestamps are allowed.
    Unordered,
}

impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AfterDecision => write!(f, "causal view is after the decision"),
            Self::Unordered => write!(f, "causal views are not in availability order"),
        }
    }
}

/// Why a funding decision is flat. These are the only flat reasons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlatReason {
    FundingStale,
    FundingSlotMissing,
}

/// What the engine reads. `Flat` means position 0 and is not a missing signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Decision {
    Target(Target),
    Flat(FlatReason),
}

impl Decision {
    /// Target ticks. `Flat` is 0.
    pub const fn target_ticks(self) -> i64 {
        match self {
            Self::Target(target) => target.ticks(),
            Self::Flat(_) => 0,
        }
    }
}

/// One target decision for one closed bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalOutput {
    pub decision_ts: DecisionTime,
    pub bar_close_time_us: UtcMicros,
    pub interval: BarInterval,
    pub decision: Decision,
}

impl SignalOutput {
    pub(crate) fn computed(
        decision_ts: DecisionTime,
        bar_close_time_us: UtcMicros,
        interval: BarInterval,
        target: Target,
    ) -> Self {
        Self {
            decision_ts,
            bar_close_time_us,
            interval,
            decision: Decision::Target(target),
        }
    }

    pub(crate) fn flat(
        decision_ts: DecisionTime,
        bar_close_time_us: UtcMicros,
        interval: BarInterval,
        reason: FlatReason,
    ) -> Self {
        Self {
            decision_ts,
            bar_close_time_us,
            interval,
            decision: Decision::Flat(reason),
        }
    }
}

/// A rejected parameter. The signal is not constructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamError {
    Lookback,
    Scale,
    MaxAge,
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lookback => write!(f, "signal lookback is below the minimum"),
            Self::Scale => write!(f, "signal scale is not finite and positive"),
            Self::MaxAge => write!(f, "funding max age is not positive"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalError {
    Window(WindowError),
    Gap {
        expected_open: UtcMicros,
    },
    ClockMismatch,
    /// The bar window and the funding window were built at different decisions.
    DecisionMismatch,
    IntervalMismatch,
    NoTriggeringBar,
    InvalidFeature,
    Target(TargetError),
    Rank(RankError),
    DuplicateBar,
    DuplicateFunding,
    FundingOffGrid {
        funding_time: UtcMicros,
    },
    FundingIntervalChange {
        funding_time: UtcMicros,
        interval_hours: u8,
    },
    Overflow,
}

impl fmt::Display for SignalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Window(err) => write!(f, "{err}"),
            Self::Gap { expected_open } => write!(f, "bar gap, expected open {expected_open}"),
            Self::ClockMismatch => write!(f, "bar availability is not close time plus 1 ms"),
            Self::DecisionMismatch => {
                write!(f, "bar and funding windows do not share a decision time")
            }
            Self::IntervalMismatch => write!(f, "bar interval does not match the signal"),
            Self::NoTriggeringBar => write!(f, "decision is not the last bar's availability"),
            Self::InvalidFeature => write!(f, "feature input is not usable"),
            Self::Target(err) => write!(f, "{err}"),
            Self::Rank(err) => write!(f, "{err}"),
            Self::DuplicateBar => write!(f, "duplicate bar open time"),
            Self::DuplicateFunding => write!(f, "duplicate funding time"),
            Self::FundingOffGrid { funding_time } => {
                write!(f, "funding time {funding_time} is off the 8h grid")
            }
            Self::FundingIntervalChange {
                funding_time,
                interval_hours,
            } => write!(
                f,
                "funding interval at {funding_time} is {interval_hours}h, not 8h"
            ),
            Self::Overflow => write!(f, "timestamp arithmetic overflowed"),
        }
    }
}

impl From<WindowError> for SignalError {
    fn from(err: WindowError) -> Self {
        Self::Window(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{Decision, FlatReason};
    use crate::target::Target;

    #[test]
    fn decision_flat_maps_to_zero() {
        assert_eq!(Decision::Flat(FlatReason::FundingStale).target_ticks(), 0);
        assert_eq!(
            Decision::Flat(FlatReason::FundingSlotMissing).target_ticks(),
            0
        );
        assert_ne!(
            Decision::Target(Target::from_f64(1.0).expect("unit target")).target_ticks(),
            0
        );
    }
}
