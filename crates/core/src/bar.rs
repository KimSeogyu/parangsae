//! Resampled UTC bars. The tape reader is the only producer.
//!
//! `available_ts` is not a field. It lives on the enclosing observation and is
//! the last 1-minute bar's availability, which equals `close_time + 1 ms`.

use crate::fixed::Price;
use crate::time::{UtcMicros, MICROS_PER_MILLIS};
use std::fmt;

/// Width of one resampled bar. Opens are aligned to the UTC epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarInterval {
    H1,
    H4,
}

impl BarInterval {
    pub const fn micros(self) -> i64 {
        match self {
            Self::H1 => 3_600_000_000,
            Self::H4 => 14_400_000_000,
        }
    }
}

impl fmt::Display for BarInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::H1 => write!(f, "1h"),
            Self::H4 => write!(f, "4h"),
        }
    }
}

/// One closed OHLC bar. Fields stay private so a value only exists when the
/// grid and the price order have been checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResampledBar {
    open_time: UtcMicros,
    interval: BarInterval,
    open: Price,
    high: Price,
    low: Price,
    close: Price,
}

impl ResampledBar {
    pub fn new(
        open_time: UtcMicros,
        interval: BarInterval,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
    ) -> Result<Self, BarError> {
        if open_time.micros() % interval.micros() != 0 {
            return Err(BarError::Misaligned);
        }
        if low.ticks() > open.ticks().min(close.ticks())
            || high.ticks() < open.ticks().max(close.ticks())
        {
            return Err(BarError::OhlcOrder);
        }
        if close_of(open_time, interval).is_none() {
            return Err(BarError::Overflow);
        }
        Ok(Self {
            open_time,
            interval,
            open,
            high,
            low,
            close,
        })
    }

    pub const fn open_time(self) -> UtcMicros {
        self.open_time
    }

    pub const fn interval(self) -> BarInterval {
        self.interval
    }

    /// `open + interval − 1 ms`. The constructor rejects a sum that does not fit.
    pub fn close_time(self) -> UtcMicros {
        match close_of(self.open_time, self.interval) {
            Some(close) => close,
            None => unreachable!("close_time checked in ResampledBar::new"),
        }
    }

    pub const fn open(self) -> Price {
        self.open
    }

    pub const fn high(self) -> Price {
        self.high
    }

    pub const fn low(self) -> Price {
        self.low
    }

    pub const fn close(self) -> Price {
        self.close
    }
}

fn close_of(open_time: UtcMicros, interval: BarInterval) -> Option<UtcMicros> {
    open_time
        .checked_add_micros(interval.micros())?
        .checked_add_micros(-MICROS_PER_MILLIS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarError {
    Misaligned,
    OhlcOrder,
    Overflow,
}

impl fmt::Display for BarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Misaligned => write!(f, "bar open is not aligned to the interval"),
            Self::OhlcOrder => write!(f, "bar OHLC order is invalid"),
            Self::Overflow => write!(f, "bar close time overflowed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::Price;
    use proptest::prelude::*;

    fn px(ticks: i64) -> Price {
        Price::from_ticks(ticks).unwrap()
    }

    fn flat(open: i64, interval: BarInterval, ticks: i64) -> Result<ResampledBar, BarError> {
        let price = px(ticks);
        ResampledBar::new(
            UtcMicros::from_micros(open),
            interval,
            price,
            price,
            price,
            price,
        )
    }

    #[test]
    fn one_hour_close_is_one_millisecond_before_the_next_open() {
        let bar = flat(0, BarInterval::H1, 1).unwrap();
        assert_eq!(bar.close_time().micros(), 3_600_000_000 - 1_000);
        assert_eq!(bar.interval(), BarInterval::H1);
    }

    #[test]
    fn four_hour_opens_fall_on_the_utc_epoch_grid() {
        for hour in [0_i64, 4, 8, 12, 16, 20] {
            let open = hour * 3_600_000_000;
            assert!(flat(open, BarInterval::H4, 10).is_ok(), "hour {hour}");
        }
        for hour in [1_i64, 2, 3, 5, 6, 7, 21, 23] {
            let open = hour * 3_600_000_000;
            assert_eq!(
                flat(open, BarInterval::H4, 10),
                Err(BarError::Misaligned),
                "hour {hour}"
            );
        }
    }

    #[test]
    fn misaligned_open_is_rejected_before_ohlc() {
        let err = ResampledBar::new(
            UtcMicros::from_micros(1),
            BarInterval::H1,
            px(5),
            px(1),
            px(9),
            px(5),
        );
        assert_eq!(err, Err(BarError::Misaligned));
    }

    #[test]
    fn ohlc_must_bracket_open_and_close() {
        let open = UtcMicros::from_micros(0);
        assert_eq!(
            ResampledBar::new(open, BarInterval::H1, px(10), px(10), px(11), px(10)),
            Err(BarError::OhlcOrder)
        );
        assert_eq!(
            ResampledBar::new(open, BarInterval::H1, px(10), px(12), px(10), px(13)),
            Err(BarError::OhlcOrder)
        );
        let bar = ResampledBar::new(open, BarInterval::H1, px(10), px(14), px(8), px(12)).unwrap();
        assert_eq!(bar.low().ticks(), 8);
        assert_eq!(bar.high().ticks(), 14);
    }

    #[test]
    fn close_time_overflow_is_rejected() {
        let width = BarInterval::H1.micros();
        let remainder = i64::MAX % width;
        let open = i64::MAX - remainder;
        assert_eq!(open % width, 0);
        assert_eq!(flat(open, BarInterval::H1, 1), Err(BarError::Overflow));
    }

    // These generators sample the constructor. They are not a proof.
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn aligned_bars_roundtrip_ohlc_and_close(
            steps in 0i64..20_000,
            open_px in 0i64..1_000_000_000,
            close_px in 0i64..1_000_000_000,
            high_extra in 0i64..10_000,
            low_cut in 0i64..10_000,
            h4 in any::<bool>(),
        ) {
            let interval = if h4 { BarInterval::H4 } else { BarInterval::H1 };
            let open_us = steps * interval.micros();
            let low_ticks = open_px.min(close_px).saturating_sub(low_cut);
            let high_ticks = open_px.max(close_px).saturating_add(high_extra);
            let bar = ResampledBar::new(
                UtcMicros::from_micros(open_us),
                interval,
                px(open_px),
                px(high_ticks),
                px(low_ticks),
                px(close_px),
            )
            .unwrap();
            prop_assert_eq!(bar.open_time().micros(), open_us);
            prop_assert_eq!(bar.close_time().micros(), open_us + interval.micros() - 1_000);
            prop_assert!(bar.low().ticks() <= bar.open().ticks().min(bar.close().ticks()));
            prop_assert!(bar.high().ticks() >= bar.open().ticks().max(bar.close().ticks()));
        }

        #[test]
        fn unaligned_opens_are_misaligned(open in 1i64..5_000_000_000_000i64, h4 in any::<bool>()) {
            let interval = if h4 { BarInterval::H4 } else { BarInterval::H1 };
            prop_assume!(open % interval.micros() != 0);
            prop_assert_eq!(flat(open, interval, 1), Err(BarError::Misaligned));
        }
    }
}
