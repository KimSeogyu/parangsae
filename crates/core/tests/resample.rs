//! R1 and R2. Resample goldens and prefix independence.

use parangsae_core::{
    resample_series, Bar, Interval, Price, Qty, ResampleError, ResampledBar, UtcMicros,
};
use proptest::prelude::*;

const MINUTE: i64 = 60_000_000;

fn px(ticks: i64) -> Price {
    Price::from_ticks(ticks).unwrap()
}

fn qty(ticks: i64) -> Qty {
    Qty::from_ticks(ticks).unwrap()
}

fn minute(open: i64, open_px: i64, high: i64, low: i64, close: i64, volume: i64) -> Bar {
    Bar {
        open_time: UtcMicros::from_micros(open),
        close_time: UtcMicros::from_micros(open + MINUTE - 1_000),
        available: UtcMicros::from_micros(open + MINUTE),
        open: px(open_px),
        high: px(high),
        low: px(low),
        close: px(close),
        volume: qty(volume),
        count: 1,
    }
}

fn hour_of(start: i64) -> Vec<Bar> {
    (0..60)
        .map(|index| {
            let open = start + index * MINUTE;
            minute(
                open,
                100 + index,
                200 + index,
                50 + index,
                150 + index,
                10 + index,
            )
        })
        .collect()
}

#[test]
fn r1_one_hour_is_the_hand_fold_and_a_gap_refuses() {
    let start = 1_704_067_200_000_000; // 2024-01-01 00:00 UTC
    let minutes = hour_of(start);
    let bar = ResampledBar::from_minutes(&minutes, Interval::ONE_HOUR).unwrap();
    assert_eq!(bar.open_time().micros(), start);
    assert_eq!(bar.close_time().micros(), start + 3_600_000_000 - 1_000);
    assert_eq!(
        bar.close_time().micros() + 1_000,
        minutes[59].available.micros()
    );
    assert_eq!(bar.open(), minutes[0].open);
    assert_eq!(bar.close(), minutes[59].close);
    assert_eq!(bar.high(), minutes[59].high);
    assert_eq!(bar.low(), minutes[0].low);
    let volume: i64 = minutes.iter().map(|row| row.volume.ticks()).sum();
    assert!(volume > 0);

    let at_four = 4 * 3_600_000_000;
    let four = (0..240)
        .map(|index| minute(start + at_four + index * MINUTE, 1, 3, 1, 2, 1))
        .collect::<Vec<_>>();
    assert!(ResampledBar::from_minutes(&four, Interval::FOUR_HOURS).is_ok());

    let at_one = 3_600_000_000;
    let misaligned = (0..240)
        .map(|index| minute(start + at_one + index * MINUTE, 1, 3, 1, 2, 1))
        .collect::<Vec<_>>();
    assert_eq!(
        ResampledBar::from_minutes(&misaligned, Interval::FOUR_HOURS).unwrap_err(),
        ResampleError::Misaligned {
            open_time: UtcMicros::from_micros(start + at_one),
        }
    );

    let mut gapped = hour_of(start);
    gapped.remove(30);
    assert!(matches!(
        ResampledBar::from_minutes(&gapped, Interval::ONE_HOUR),
        Err(ResampleError::Gap { .. })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn r2_fold_matches_and_outside_minutes_do_not_leak(
        seed in 1i64..500,
        outside in 1i64..500,
    ) {
        let start = 1_704_067_200_000_000i64;
        let mut minutes = Vec::new();
        for index in 0..120 {
            let open = start + index * MINUTE;
            let wobble = seed + index;
            minutes.push(minute(open, wobble, wobble + 5, wobble, wobble + 1, 1 + (index % 7)));
        }
        let first = ResampledBar::from_minutes(&minutes[..60], Interval::ONE_HOUR).unwrap();
        let folded_open = minutes[0].open;
        let folded_close = minutes[59].close;
        let folded_high = minutes[..60].iter().map(|bar| bar.high.ticks()).max().unwrap();
        let folded_low = minutes[..60].iter().map(|bar| bar.low.ticks()).min().unwrap();
        let folded_volume: i64 = minutes[..60].iter().map(|bar| bar.volume.ticks()).sum();
        prop_assert!(folded_volume > 0);
        prop_assert_eq!(first.open(), folded_open);
        prop_assert_eq!(first.close(), folded_close);
        prop_assert_eq!(first.high().ticks(), folded_high);
        prop_assert_eq!(first.low().ticks(), folded_low);

        let series = resample_series(&minutes, Interval::ONE_HOUR).unwrap();
        prop_assert_eq!(series[0], first);

        minutes[90].close = px(outside);
        minutes[90].high = px(outside + 1);
        let again = ResampledBar::from_minutes(&minutes[..60], Interval::ONE_HOUR).unwrap();
        prop_assert_eq!(again, first);
    }
}
