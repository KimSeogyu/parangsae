//! Failures a caller can distinguish. Integrity failures name the check.

use std::fmt;

use parangsae_core::{FixedError, SymbolError, TimeError, UnitError};

#[derive(Debug)]
pub enum Error {
    Scale(ScaleError),
    Unit(UnitError),
    Time(TimeError),
    Fixed(FixedError),
    Symbol(SymbolError),
    Check {
        name: &'static str,
        detail: String,
    },
    Io {
        path: String,
        source: std::io::Error,
    },
    Http {
        url: String,
        detail: String,
    },
    Xml(String),
    Zip(String),
    Csv {
        line: usize,
        detail: String,
    },
    Usage(String),
}

impl Error {
    pub fn check(name: &'static str, detail: impl Into<String>) -> Self {
        Self::Check {
            name,
            detail: detail.into(),
        }
    }

    pub fn check_name(&self) -> Option<&'static str> {
        match self {
            Self::Check { name, .. } => Some(*name),
            Self::Time(TimeError::CloseTime { .. }) => Some("close_time"),
            Self::Time(TimeError::GridGap { .. }) => Some("grid"),
            Self::Unit(_) => Some("timestamp_unit"),
            Self::Fixed(FixedError::Negative) => Some("negative"),
            Self::Fixed(FixedError::Overflow) => Some("overflow"),
            Self::Scale(ScaleError::Scale) => Some("scale"),
            Self::Scale(_) => Some("scale"),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scale(err) => write!(f, "{err}"),
            Self::Unit(err) => write!(f, "{err}"),
            Self::Time(err) => write!(f, "{err}"),
            Self::Fixed(err) => write!(f, "{err}"),
            Self::Symbol(err) => write!(f, "{err}"),
            Self::Check { name, detail } => write!(f, "check {name} failed: {detail}"),
            Self::Io { path, source } => write!(f, "io {path}: {source}"),
            Self::Http { url, detail } => write!(f, "http {url}: {detail}"),
            Self::Xml(detail) => write!(f, "listing xml: {detail}"),
            Self::Zip(detail) => write!(f, "zip: {detail}"),
            Self::Csv { line, detail } => write!(f, "csv line {line}: {detail}"),
            Self::Usage(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleError {
    Empty,
    Charset,
    Scale,
    Overflow,
}

impl fmt::Display for ScaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty decimal"),
            Self::Charset => write!(f, "decimal is not a plain base-10 literal"),
            Self::Scale => write!(f, "more than 8 significant fractional digits"),
            Self::Overflow => write!(f, "decimal does not fit in i64 ticks"),
        }
    }
}

impl From<ScaleError> for Error {
    fn from(value: ScaleError) -> Self {
        Self::Scale(value)
    }
}

impl From<UnitError> for Error {
    fn from(value: UnitError) -> Self {
        Self::Unit(value)
    }
}

impl From<TimeError> for Error {
    fn from(value: TimeError) -> Self {
        Self::Time(value)
    }
}

impl From<FixedError> for Error {
    fn from(value: FixedError) -> Self {
        Self::Fixed(value)
    }
}

impl From<SymbolError> for Error {
    fn from(value: SymbolError) -> Self {
        Self::Symbol(value)
    }
}
