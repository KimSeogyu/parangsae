//! Resample a contiguous 1m grid into one 1h or 4h bar.
//!
//! A missing minute is [`ResampleError::Gap`]. The constructor never emits a
//! partial bar. 4h bars open at 00:00, 04:00, 08:00, 12:00, 16:00, or 20:00 UTC.

use std::fmt;

use crate::bar::{BarError, BarInterval, ResampledBar};
use crate::fixed::{Price, Qty};
use crate::time::{Interval, UtcMicros, MICROS_PER_MILLIS};

/// One 1m bar the resampler reads. `available` is the stored timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub open_time: UtcMicros,
    pub close_time: UtcMicros,
    pub available: UtcMicros,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Qty,
    pub count: i64,
}

impl Bar {
    /// Stored zero-volume filler: flat print, no trades. Not a fill bar.
    pub fn is_filler(self) -> bool {
        self.volume.ticks() == 0
            && self.count == 0
            && self.open == self.high
            && self.high == self.low
            && self.low == self.close
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResampleError {
    Gap {
        expected: UtcMicros,
        got: Option<UtcMicros>,
    },
    Misaligned {
        open_time: UtcMicros,
    },
    Interval,
    Overflow,
    Available {
        expected: UtcMicros,
        got: UtcMicros,
    },
}

impl fmt::Display for ResampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gap { expected, got } => match got {
                Some(got) => write!(f, "resample gap: expected {expected}, got {got}"),
                None => write!(f, "resample gap: expected {expected}, minute missing"),
            },
            Self::Misaligned { open_time } => {
                write!(f, "resample open {open_time} is not on the interval grid")
            }
            Self::Interval => write!(f, "resample interval is not 1h or 4h"),
            Self::Overflow => write!(f, "resample volume overflow"),
            Self::Available { expected, got } => {
                write!(f, "resample available {got} is not {expected}")
            }
        }
    }
}

impl std::error::Error for ResampleError {}

impl ResampledBar {
    /// Fold exactly one interval of consecutive 1m bars.
    ///
    /// OHLC is the first open, the max high, the min low, and the last close.
    /// Volume is a checked sum. `close_time` is `open + interval - 1ms`.
    /// Every minute must be available at its own open + 1m before folding.
    pub fn from_minutes(minutes: &[Bar], interval: Interval) -> Result<Self, ResampleError> {
        if interval != Interval::ONE_HOUR && interval != Interval::FOUR_HOURS {
            return Err(ResampleError::Interval);
        }
        let minute = Interval::ONE_MINUTE.micros();
        let width =
            usize::try_from(interval.micros() / minute).map_err(|_| ResampleError::Overflow)?;
        if minutes.len() != width {
            let expected = minutes
                .first()
                .map(|bar| bar.open_time)
                .unwrap_or(UtcMicros::from_micros(0));
            return Err(ResampleError::Gap {
                expected,
                got: None,
            });
        }
        let open_time = minutes[0].open_time;
        if open_time.micros().rem_euclid(interval.micros()) != 0 {
            return Err(ResampleError::Misaligned { open_time });
        }
        for (offset, bar) in minutes.iter().enumerate().skip(1) {
            let expected = open_time
                .micros()
                .checked_add(
                    minute
                        .checked_mul(i64::try_from(offset).map_err(|_| ResampleError::Overflow)?)
                        .ok_or(ResampleError::Overflow)?,
                )
                .ok_or(ResampleError::Overflow)?;
            if bar.open_time.micros() != expected {
                return Err(ResampleError::Gap {
                    expected: UtcMicros::from_micros(expected),
                    got: Some(bar.open_time),
                });
            }
        }
        let mut high = minutes[0].high.ticks();
        let mut low = minutes[0].low.ticks();
        let mut volume: i64 = 0;
        for bar in minutes {
            let expected = bar
                .open_time
                .checked_add_micros(minute)
                .ok_or(ResampleError::Overflow)?;
            if bar.available != expected {
                return Err(ResampleError::Available {
                    expected,
                    got: bar.available,
                });
            }
            high = high.max(bar.high.ticks());
            low = low.min(bar.low.ticks());
            volume = volume
                .checked_add(bar.volume.ticks())
                .ok_or(ResampleError::Overflow)?;
        }
        let _ = volume;
        let last = minutes[minutes.len() - 1];
        let bar_interval = match interval {
            Interval::ONE_HOUR => BarInterval::H1,
            Interval::FOUR_HOURS => BarInterval::H4,
            _ => return Err(ResampleError::Interval),
        };
        let high = Price::from_ticks(high).map_err(|_| ResampleError::Overflow)?;
        let low = Price::from_ticks(low).map_err(|_| ResampleError::Overflow)?;
        let built = ResampledBar::new(
            open_time,
            bar_interval,
            minutes[0].open,
            high,
            low,
            last.close,
        )
        .map_err(|err| match err {
            BarError::Misaligned => ResampleError::Misaligned { open_time },
            BarError::OhlcOrder | BarError::Overflow => ResampleError::Overflow,
        })?;
        let expected = built
            .close_time()
            .checked_add_micros(MICROS_PER_MILLIS)
            .ok_or(ResampleError::Overflow)?;
        if last.available != expected {
            return Err(ResampleError::Available {
                expected,
                got: last.available,
            });
        }
        Ok(built)
    }
}

/// Fold a whole 1m window. The length must be a multiple of the interval.
/// An empty window is an empty series. A trailing partial hour is a gap.
pub fn resample_series(
    minutes: &[Bar],
    interval: Interval,
) -> Result<Vec<ResampledBar>, ResampleError> {
    if interval != Interval::ONE_HOUR && interval != Interval::FOUR_HOURS {
        return Err(ResampleError::Interval);
    }
    if minutes.is_empty() {
        return Ok(Vec::new());
    }
    let width = usize::try_from(interval.micros() / Interval::ONE_MINUTE.micros())
        .map_err(|_| ResampleError::Overflow)?;
    if !minutes.len().is_multiple_of(width) {
        return Err(ResampleError::Gap {
            expected: minutes[0].open_time,
            got: None,
        });
    }
    let mut out = Vec::with_capacity(minutes.len() / width);
    for chunk in minutes.chunks(width) {
        out.push(ResampledBar::from_minutes(chunk, interval)?);
    }
    Ok(out)
}
