//! Failures a caller can see. A gap, a missing mark, and a missing funding stamp
//! abort the run instead of being filled forward.

use std::fmt;

use parangsae_core::UtcMicros;
use parangsae_sim::SimError;

pub use crate::tape::TapeError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    Gap { expected: UtcMicros, got: UtcMicros },
    MissingMark(UtcMicros),
    MissingFunding(UtcMicros),
    Unsorted,
    Date,
    Broken(&'static str),
    Sim(SimError),
    Tape(TapeError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gap { expected, got } => {
                write!(f, "open_time grid gap: expected {expected}, got {got}")
            }
            Self::MissingMark(ts) => write!(f, "missing mark bar at {ts}"),
            Self::MissingFunding(ts) => write!(f, "missing funding stamp at {ts}"),
            Self::Unsorted => write!(f, "tape timestamps are not strictly increasing"),
            Self::Date => write!(f, "UTC date is outside the microsecond range"),
            Self::Broken(what) => write!(f, "broken engine invariant: {what}"),
            Self::Sim(err) => write!(f, "{err}"),
            Self::Tape(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sim(err) => Some(err),
            Self::Tape(err) => Some(err),
            Self::Gap { .. }
            | Self::MissingMark(_)
            | Self::MissingFunding(_)
            | Self::Unsorted
            | Self::Date
            | Self::Broken(_) => None,
        }
    }
}
