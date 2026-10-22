//! Declared clock for one source.
//!
//! Conversion goes through [`parangsae_core::require_declared_unit`]. A width
//! that does not match `expected` is an error. USD-M klines are milliseconds.
//! Spot klines are microseconds.

use parangsae_core::{require_declared_unit, TimeUnit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceClock {
    expected: TimeUnit,
}

impl SourceClock {
    pub const fn new(expected: TimeUnit) -> Self {
        Self { expected }
    }

    pub const fn expected(self) -> TimeUnit {
        self.expected
    }

    pub fn read(self, raw: i64) -> Result<parangsae_core::UtcMicros, parangsae_core::UnitError> {
        require_declared_unit(raw, self.expected)
    }
}

/// USD-M futures klines, as verified on the Vision archive: millisecond timestamps.
pub const UM_KLINE_CLOCK: SourceClock = SourceClock::new(TimeUnit::Millis);

/// Spot klines from 2025-01-01 onward: microsecond timestamps, no header.
pub const SPOT_KLINE_CLOCK: SourceClock = SourceClock::new(TimeUnit::Micros);
