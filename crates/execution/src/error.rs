//! Configuration failures. Constructors return these instead of clamping.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    RecvWindowOutOfRange,
    MarginOutOfRange,
    BoundOverflow,
    MaxAttemptsOutOfRange,
    NonPositiveMicros,
    ApprovalTtlNotAboveDispatchLatency,
    ApprovalAgeMismatch,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecvWindowOutOfRange => write!(f, "recv window is outside 1..=60000 ms"),
            Self::MarginOutOfRange => write!(f, "not-found margin is below 1 ms"),
            Self::BoundOverflow => write!(f, "not-found authority bound overflows"),
            Self::MaxAttemptsOutOfRange => write!(f, "max attempts is outside 1..=16"),
            Self::NonPositiveMicros => write!(f, "duration is zero"),
            Self::ApprovalTtlNotAboveDispatchLatency => {
                write!(f, "approval ttl is not above dispatch latency")
            }
            Self::ApprovalAgeMismatch => {
                write!(f, "approval age does not match the risk limit")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
