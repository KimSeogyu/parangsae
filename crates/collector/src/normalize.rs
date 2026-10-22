//! Declared clock for one source.
//!
//! Conversion goes through [`parangsae_core::require_declared_unit`]. A width
//! that does not match `expected` is an error. The unit is chosen from the
//! source and the archive date, never from the digits in the file.
//!
//! USD-M klines, including mark, index, and premium, are milliseconds
//! (still true on the 2026-10-20 archive). Spot klines are milliseconds before
//! 2025-01-01 and microseconds from that date.

use parangsae_core::{require_declared_unit, TimeUnit};

use crate::civil::CivilDay;

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

/// Spot klines before 2025-01-01. Declared, not inferred.
pub const SPOT_KLINE_MILLIS_CLOCK: SourceClock = SourceClock::new(TimeUnit::Millis);

/// Funding `calc_time` on the Vision monthly archive.
pub const FUNDING_CLOCK: SourceClock = SourceClock::new(TimeUnit::Millis);

/// First archive date whose spot klines are microseconds.
pub fn spot_micros_from() -> CivilDay {
    CivilDay::new(2025, 1, 1).expect("2025-01-01 is a real date")
}

pub fn spot_kline_clock(archive_day: CivilDay) -> SourceClock {
    if archive_day >= spot_micros_from() {
        SPOT_KLINE_CLOCK
    } else {
        SPOT_KLINE_MILLIS_CLOCK
    }
}
