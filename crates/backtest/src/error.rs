//! Failures a caller can see. A gap, a missing mark, and a missing funding stamp
//! abort the run instead of being filled forward.

use std::fmt;

use parangsae_core::UtcMicros;
use parangsae_sim::SimError;

pub use crate::tape::TapeError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    Gap {
        expected: UtcMicros,
        got: UtcMicros,
    },
    MissingMark(UtcMicros),
    MissingFunding(UtcMicros),
    MissingSignal(UtcMicros),
    HoldoutBar(UtcMicros),
    BeforeSample(UtcMicros),
    /// `warmup_start` is after the scored start.
    Warmup(UtcMicros),
    MoneyCapacity {
        at: UtcMicros,
    },
    MoneyClock {
        at: UtcMicros,
    },
    EconomicArithmetic {
        at: UtcMicros,
    },
    UnadmittedMoneyControl {
        at: UtcMicros,
    },
    UnavailableControlMark {
        at: UtcMicros,
    },
    UnfilledLiquidation {
        trigger: UtcMicros,
    },
    UnfilledBoundaryClose {
        boundary: UtcMicros,
    },
    InvalidExecution,
    TraceCapacity {
        at: UtcMicros,
    },
    Unsorted,
    Sim(SimError),
    Tape(TapeError),
    Resample(parangsae_core::ResampleError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gap { expected, got } => {
                write!(f, "open_time grid gap: expected {expected}, got {got}")
            }
            Self::MissingMark(ts) => write!(f, "missing mark bar at {ts}"),
            Self::MissingFunding(ts) => write!(f, "missing funding stamp at {ts}"),
            Self::MissingSignal(ts) => write!(f, "missing signal at {ts}"),
            Self::HoldoutBar(ts) => write!(f, "bar {ts} is in the holdout"),
            Self::BeforeSample(ts) => write!(f, "bar {ts} is before the in-sample start"),
            Self::Warmup(ts) => write!(f, "warmup {ts} starts after the scored window"),
            Self::MoneyCapacity { at } => write!(f, "money visibility capacity exceeded at {at}"),
            Self::MoneyClock { at } => write!(f, "money visibility clock is invalid at {at}"),
            Self::EconomicArithmetic { at } => {
                write!(f, "economic accounting cannot be represented at {at}")
            }
            Self::UnadmittedMoneyControl { at } => {
                write!(f, "unavailable money dependencies change control at {at}")
            }
            Self::UnavailableControlMark { at } => {
                write!(f, "no published original control mark at {at}")
            }
            Self::UnfilledLiquidation { trigger } => write!(
                f,
                "liquidation triggered at {trigger} cannot fill before the window ends"
            ),
            Self::UnfilledBoundaryClose { boundary } => write!(
                f,
                "calendar close at {boundary} cannot fill before the window ends"
            ),
            Self::InvalidExecution => write!(f, "execution phase contradicts the actual position"),
            Self::TraceCapacity { at } => write!(f, "H4 provenance capacity exceeded at {at}"),
            Self::Unsorted => write!(f, "tape timestamps are not strictly increasing"),
            Self::Sim(err) => write!(f, "{err}"),
            Self::Tape(err) => write!(f, "{err}"),
            Self::Resample(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sim(err) => Some(err),
            Self::Tape(err) => Some(err),
            Self::Resample(err) => Some(err),
            Self::Gap { .. }
            | Self::MissingMark(_)
            | Self::MissingFunding(_)
            | Self::MissingSignal(_)
            | Self::HoldoutBar(_)
            | Self::BeforeSample(_)
            | Self::Warmup(_)
            | Self::MoneyCapacity { .. }
            | Self::MoneyClock { .. }
            | Self::EconomicArithmetic { .. }
            | Self::UnadmittedMoneyControl { .. }
            | Self::UnavailableControlMark { .. }
            | Self::UnfilledLiquidation { .. }
            | Self::UnfilledBoundaryClose { .. }
            | Self::InvalidExecution
            | Self::TraceCapacity { .. }
            | Self::Unsorted => None,
        }
    }
}
