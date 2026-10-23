//! Failures a caller can see. A bad param is refused before any bar is read.

use std::fmt;

use parangsae_signals::{ParamError, SignalError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StrategyError {
    Param(&'static str),
    Signal(SignalError),
    Clock,
    Gap { expected_open: i64 },
    Interval,
    BeforeEarliest,
    Overflow,
    Price,
}

impl fmt::Display for StrategyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Param(name) => write!(f, "strategy param {name} is not usable"),
            Self::Signal(err) => write!(f, "{err}"),
            Self::Clock => write!(f, "bar availability is not close time plus 1 ms"),
            Self::Gap { expected_open } => {
                write!(f, "bar gap, expected open {expected_open}")
            }
            Self::Interval => write!(f, "bar interval does not match the strategy"),
            Self::BeforeEarliest => write!(f, "bar opens before 2020-12-18 00:00 UTC"),
            Self::Overflow => write!(f, "timestamp arithmetic overflowed"),
            Self::Price => write!(f, "price is not a positive exact f64"),
        }
    }
}

impl std::error::Error for StrategyError {}

impl From<SignalError> for StrategyError {
    fn from(err: SignalError) -> Self {
        Self::Signal(err)
    }
}

impl From<ParamError> for StrategyError {
    fn from(err: ParamError) -> Self {
        match err {
            ParamError::Lookback => Self::Param("lookback"),
            ParamError::Scale => Self::Param("scale"),
            ParamError::MaxAge => Self::Param("max_age"),
        }
    }
}
