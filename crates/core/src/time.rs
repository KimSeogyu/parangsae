//! UTC timestamps in microseconds, declared source units, and kline time rules.
//!
//! A raw exchange timestamp is accepted only when its decimal width matches the
//! unit declared for that source. A 13-digit value is milliseconds. A 16-digit
//! value is microseconds. Any other width, or a width that disagrees with the
//! declaration, is an error. This module does not guess a conversion.

use std::fmt;

/// Microseconds since the Unix epoch, UTC. Negative values are before the epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcMicros(i64);

impl UtcMicros {
    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub const fn micros(self) -> i64 {
        self.0
    }

    pub fn checked_add_micros(self, delta: i64) -> Option<Self> {
        self.0.checked_add(delta).map(Self)
    }
}

impl fmt::Display for UtcMicros {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}us", self.0)
    }
}

/// Unit declared for one source and period. The caller names it. A timestamp's width is checked against that declaration and never chooses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeUnit {
    Millis,
    Micros,
}

impl fmt::Display for TimeUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Millis => write!(f, "milliseconds"),
            Self::Micros => write!(f, "microseconds"),
        }
    }
}

/// Failure to interpret a raw timestamp under a declared unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitError {
    Negative,
    UnexpectedWidth { digits: u32 },
    DeclaredMismatch { declared: TimeUnit, digits: u32 },
    Overflow,
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Negative => write!(f, "negative timestamp"),
            Self::UnexpectedWidth { digits } => {
                write!(
                    f,
                    "unit mismatch: {digits}-digit timestamp has no declared width"
                )
            }
            Self::DeclaredMismatch { declared, digits } => {
                write!(
                    f,
                    "unit mismatch: declared {declared} but width is {digits} digits"
                )
            }
            Self::Overflow => write!(f, "timestamp overflow"),
        }
    }
}

/// Read `raw` as `declared`. Width and declaration must agree.
pub fn require_declared_unit(raw: i64, declared: TimeUnit) -> Result<UtcMicros, UnitError> {
    let magnitude = u64::try_from(raw).map_err(|_| UnitError::Negative)?;
    let digits = decimal_digits(magnitude);
    let detected = match digits {
        13 => TimeUnit::Millis,
        16 => TimeUnit::Micros,
        _ => return Err(UnitError::UnexpectedWidth { digits }),
    };
    if detected != declared {
        return Err(UnitError::DeclaredMismatch { declared, digits });
    }
    match declared {
        TimeUnit::Millis => {
            let wide = magnitude.checked_mul(1_000).ok_or(UnitError::Overflow)?;
            let micros = i64::try_from(wide).map_err(|_| UnitError::Overflow)?;
            Ok(UtcMicros::from_micros(micros))
        }
        TimeUnit::Micros => {
            let micros = i64::try_from(magnitude).map_err(|_| UnitError::Overflow)?;
            Ok(UtcMicros::from_micros(micros))
        }
    }
}

fn decimal_digits(n: u64) -> u32 {
    if n == 0 {
        return 1;
    }
    let mut digits = 0u32;
    let mut rest = n;
    while rest > 0 {
        rest /= 10;
        digits += 1;
    }
    digits
}

/// One millisecond, in microseconds.
pub const MICROS_PER_MILLIS: i64 = 1_000;

/// Width of one declared source tick, in microseconds.
pub fn source_tick_us(unit: TimeUnit) -> i64 {
    match unit {
        TimeUnit::Millis => MICROS_PER_MILLIS,
        TimeUnit::Micros => 1,
    }
}

/// Positive bar width in microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interval {
    micros: i64,
}

impl Interval {
    pub const ONE_MINUTE: Self = Self { micros: 60_000_000 };

    pub const fn from_micros(micros: i64) -> Option<Self> {
        if micros > 0 {
            Some(Self { micros })
        } else {
            None
        }
    }

    pub const fn micros(self) -> i64 {
        self.micros
    }
}

/// Open, close, interval, and the declared source tick of one kline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KlineSpan {
    pub open_time: UtcMicros,
    pub close_time: UtcMicros,
    pub interval: Interval,
    pub unit: TimeUnit,
}

/// `close_time = open_time + interval - 1 source tick`.
///
/// The tick is `unit`: 1ms or 1µs. A close that uses the other tick fails.
pub fn expected_close(
    open: UtcMicros,
    interval: Interval,
    unit: TimeUnit,
) -> Result<UtcMicros, TimeError> {
    let end = open
        .checked_add_micros(interval.micros())
        .ok_or(TimeError::Overflow)?;
    end.checked_add_micros(-source_tick_us(unit))
        .ok_or(TimeError::Overflow)
}

pub fn validate_close_time(span: KlineSpan) -> Result<(), TimeError> {
    let expected = expected_close(span.open_time, span.interval, span.unit)?;
    if span.close_time != expected {
        Err(TimeError::CloseTime {
            expected,
            found: span.close_time,
        })
    } else {
        Ok(())
    }
}

/// Successive open times differ by exactly `interval`. An empty or singleton slice has no gap.
pub fn require_regular_grid(opens: &[UtcMicros], interval: Interval) -> Result<(), TimeError> {
    for window in opens.windows(2) {
        let expected = window[0]
            .checked_add_micros(interval.micros())
            .ok_or(TimeError::Overflow)?;
        if window[1] != expected {
            return Err(TimeError::GridGap {
                expected,
                found: window[1],
            });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeError {
    Overflow,
    CloseTime {
        expected: UtcMicros,
        found: UtcMicros,
    },
    GridGap {
        expected: UtcMicros,
        found: UtcMicros,
    },
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => write!(f, "kline timestamp overflow"),
            Self::CloseTime { expected, found } => {
                write!(f, "close_time {found} != {expected}")
            }
            Self::GridGap { expected, found } => {
                write!(f, "open_time grid gap: expected {expected}, found {found}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_not_a_millisecond_timestamp() -> Result<(), String> {
        let err = match require_declared_unit(0, TimeUnit::Millis) {
            Err(err) => err,
            Ok(_) => return Err("expected a unit error".to_string()),
        };
        assert!(matches!(err, UnitError::UnexpectedWidth { digits: 1 }));
        Ok(())
    }
}
