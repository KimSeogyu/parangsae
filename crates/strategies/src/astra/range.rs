//! B3-03 range excursion and return, evaluated without position state.
//!
//! The candidate is excluded from both the prior channel and ATR. The extra
//! oldest bar supplies the first prior true range's previous close. Arming,
//! fills and the actual six-hour hold belong to the common execution adapter.

use parangsae_core::{DecisionTime, Price, ResampledBar, UtcMicros};
use parangsae_signals::CausalWindow;

use crate::{PositionSide, StrategyError};

pub const RANGE_WINDOW_CAP: usize = 26;
const PRIOR_BARS: usize = 24;
const QUARTER_ATR_DENOMINATOR: i128 = 4 * PRIOR_BARS as i128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeUnavailable {
    WindowIncomplete,
    ZeroAtr,
    DegenerateRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeAssessment {
    Unavailable(RangeUnavailable),
    KnownFalse(RangeFacts),
    Ready {
        side: PositionSide,
        facts: RangeFacts,
    },
}

/// Exact feature facts, not order prices, quantities or restart authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeFacts {
    decision: DecisionTime,
    candidate_open: UtcMicros,
    bar_close_time: UtcMicros,
    upper: Price,
    lower: Price,
    true_range_sum_ticks: i128,
    upper_excursion: bool,
    lower_excursion: bool,
    close_inside: bool,
}

impl RangeFacts {
    pub const fn decision(self) -> DecisionTime {
        self.decision
    }

    pub const fn candidate_open(self) -> UtcMicros {
        self.candidate_open
    }

    pub const fn bar_close_time(self) -> UtcMicros {
        self.bar_close_time
    }

    pub const fn upper(self) -> Price {
        self.upper
    }

    pub const fn lower(self) -> Price {
        self.lower
    }

    /// ATR24 is this exact sum divided by24; no rounded threshold is stored.
    pub const fn true_range_sum_ticks(self) -> i128 {
        self.true_range_sum_ticks
    }

    pub const fn upper_excursion(self) -> bool {
        self.upper_excursion
    }

    pub const fn lower_excursion(self) -> bool {
        self.lower_excursion
    }

    pub const fn close_inside(self) -> bool {
        self.close_inside
    }
}

/// Pure assessment of the common adapter's bounded causal H1 window.
pub fn evaluate(window: &CausalWindow<'_, ResampledBar>) -> Result<RangeAssessment, StrategyError> {
    let Some(bars) = super::prepare_h1(window, RANGE_WINDOW_CAP)? else {
        return Ok(RangeAssessment::Unavailable(
            RangeUnavailable::WindowIncomplete,
        ));
    };
    Ok(assess(&bars, window.decision()))
}

fn assess(bars: &[&ResampledBar], decision: DecisionTime) -> RangeAssessment {
    let prior = &bars[1..=PRIOR_BARS];
    let candidate = bars[PRIOR_BARS + 1];
    let mut upper = prior[0].high();
    let mut lower = prior[0].low();
    let mut sum = 0_i128;
    for pair in bars[..=PRIOR_BARS].windows(2) {
        let previous_close = i128::from(pair[0].close().ticks());
        let bar = pair[1];
        let high = i128::from(bar.high().ticks());
        let low = i128::from(bar.low().ticks());
        sum += (high - low)
            .max((high - previous_close).abs())
            .max((low - previous_close).abs());
        upper = upper.max(bar.high());
        lower = lower.min(bar.low());
    }
    if sum == 0 {
        return RangeAssessment::Unavailable(RangeUnavailable::ZeroAtr);
    }
    if upper == lower {
        return RangeAssessment::Unavailable(RangeUnavailable::DegenerateRange);
    }
    // Prices are nonnegative i64 ticks. All24 terms and these products fit i128.
    let upper_excess = i128::from(candidate.high().ticks()) - i128::from(upper.ticks());
    let lower_excess = i128::from(lower.ticks()) - i128::from(candidate.low().ticks());
    let upper_excursion = upper_excess * QUARTER_ATR_DENOMINATOR > sum;
    let lower_excursion = lower_excess * QUARTER_ATR_DENOMINATOR > sum;
    let close_inside = lower < candidate.close() && candidate.close() < upper;
    let facts = RangeFacts {
        decision,
        candidate_open: candidate.open_time(),
        bar_close_time: candidate.close_time(),
        upper,
        lower,
        true_range_sum_ticks: sum,
        upper_excursion,
        lower_excursion,
        close_inside,
    };
    match (close_inside, upper_excursion, lower_excursion) {
        (true, true, false) => RangeAssessment::Ready {
            side: PositionSide::Short,
            facts,
        },
        (true, false, true) => RangeAssessment::Ready {
            side: PositionSide::Long,
            facts,
        },
        _ => RangeAssessment::KnownFalse(facts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{AvailableTs, BarInterval, EventTs, Observation, ReceivedTs, Timestamps};
    use proptest::prelude::*;
    use std::num::NonZeroUsize;

    const HOUR: i64 = 3_600_000_000;
    const T0: i64 = 1_704_067_200_000_000;

    fn bar(index: i64, high: i64, low: i64, close: i64) -> ResampledBar {
        ResampledBar::new(
            UtcMicros::from_micros(T0 + index * HOUR),
            BarInterval::H1,
            Price::from_ticks(close).expect("synthetic open"),
            Price::from_ticks(high).expect("synthetic high"),
            Price::from_ticks(low).expect("synthetic low"),
            Price::from_ticks(close).expect("synthetic close"),
        )
        .expect("synthetic closed bar")
    }

    fn history() -> Vec<ResampledBar> {
        (0..26).map(|index| bar(index, 1_024, 976, 1_000)).collect()
    }

    fn direct(rows: &[ResampledBar]) -> RangeAssessment {
        let refs: Vec<_> = rows.iter().collect();
        assess(
            &refs,
            DecisionTime::from_micros(UtcMicros::from_micros(T0 + 26 * HOUR + 1_000)),
        )
    }

    fn observed(bar: ResampledBar, available: i64) -> Observation<ResampledBar> {
        Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(bar.open_time()),
                available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
                // Synthetic receipt only; this fixture makes no historical receipt claim.
                received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(available)),
            },
            bar,
        )
    }

    fn causal_rows(rows: &[ResampledBar]) -> Vec<Observation<ResampledBar>> {
        rows.iter()
            .map(|bar| observed(*bar, bar.close_time().micros() + 1_000))
            .collect()
    }

    fn at_decision(
        rows: &[Observation<ResampledBar>],
        decision_us: i64,
    ) -> Result<RangeAssessment, StrategyError> {
        let decision = DecisionTime::from_micros(UtcMicros::from_micros(decision_us));
        let window = CausalWindow::from_views(
            decision,
            NonZeroUsize::new(RANGE_WINDOW_CAP).expect("fixed nonzero range cap"),
            rows.iter().filter_map(|row| row.causal_at(decision)),
        )
        .map_err(parangsae_signals::SignalError::from)?;
        evaluate(&window)
    }

    fn facts(value: RangeAssessment) -> RangeFacts {
        match value {
            RangeAssessment::KnownFalse(facts) | RangeAssessment::Ready { facts, .. } => facts,
            other => panic!("expected evaluated synthetic facts, got {other:?}"),
        }
    }

    #[test]
    fn quarter_atr_equality_is_false_and_one_tick_beyond_selects_the_opposite_side() {
        for (high, low, expected) in [
            (1_036, 980, None),
            (1_037, 980, Some(PositionSide::Short)),
            (1_020, 964, None),
            (1_020, 963, Some(PositionSide::Long)),
        ] {
            let mut rows = history();
            rows[25] = bar(25, high, low, 1_000);
            let value = direct(&rows);
            assert_eq!(facts(value).true_range_sum_ticks(), 24 * 48);
            match expected {
                Some(side) => assert!(
                    matches!(value, RangeAssessment::Ready { side: got, .. } if got == side)
                ),
                None => assert!(matches!(value, RangeAssessment::KnownFalse(_))),
            }
        }
    }

    #[test]
    fn both_excursions_and_neither_excursion_are_known_false() {
        for (high, low) in [(1_037, 963), (1_024, 976)] {
            let mut rows = history();
            rows[25] = bar(25, high, low, 1_000);
            assert!(matches!(direct(&rows), RangeAssessment::KnownFalse(_)));
        }
    }

    #[test]
    fn a_small_opposite_raw_excursion_does_not_make_both_qualified_predicates_true() {
        let mut rows = history();
        rows[25] = bar(25, 1_037, 975, 1_000);
        let value = direct(&rows);
        assert!(matches!(
            value,
            RangeAssessment::Ready {
                side: PositionSide::Short,
                ..
            }
        ));
        assert!(facts(value).upper_excursion());
        assert!(!facts(value).lower_excursion());
    }

    #[test]
    fn close_must_be_strictly_inside_both_prior_boundaries() {
        for close in [976, 1_024, 1_025] {
            let mut rows = history();
            rows[25] = bar(25, 1_037, 976, close);
            let value = direct(&rows);
            assert!(matches!(value, RangeAssessment::KnownFalse(_)));
            assert!(facts(value).upper_excursion());
            assert!(!facts(value).lower_excursion());
            assert!(!facts(value).close_inside());
        }
    }

    #[test]
    fn candidate_extremes_do_not_widen_the_channel_or_enter_atr() {
        let mut rows = history();
        rows[25] = bar(25, 1_037, 980, 1_000);
        let value = direct(&rows);
        assert!(matches!(
            value,
            RangeAssessment::Ready {
                side: PositionSide::Short,
                ..
            }
        ));
        let f = facts(value);
        assert_eq!((f.lower().ticks(), f.upper().ticks()), (976, 1_024));
        assert_eq!(f.true_range_sum_ticks(), 1_152);
        assert_eq!(f.candidate_open().micros(), T0 + 25 * HOUR);
        assert_eq!(f.bar_close_time().micros(), T0 + 26 * HOUR - 1_000);
        assert_eq!(f.decision().micros().micros(), T0 + 26 * HOUR + 1_000);
    }

    #[test]
    fn seed_close_is_an_active_dependency_only_for_the_first_prior_true_range() {
        let mut rows = history();
        rows[25] = bar(25, 1_037, 980, 1_000);
        assert!(matches!(direct(&rows), RangeAssessment::Ready { .. }));
        rows[0] = bar(0, 5_000, 5_000, 5_000);
        let changed = direct(&rows);
        assert!(matches!(changed, RangeAssessment::KnownFalse(_)));
        assert_eq!(facts(changed).true_range_sum_ticks(), 5_128);
        assert_eq!(facts(changed).upper().ticks(), 1_024);
    }

    #[test]
    fn zero_atr_and_degenerate_range_are_unavailable_instead_of_rearming_false() {
        let mut rows: Vec<_> = (0..26).map(|i| bar(i, 1_000, 1_000, 1_000)).collect();
        assert_eq!(
            direct(&rows),
            RangeAssessment::Unavailable(RangeUnavailable::ZeroAtr)
        );
        rows[0] = bar(0, 1_500, 1_500, 1_500);
        assert_eq!(
            direct(&rows),
            RangeAssessment::Unavailable(RangeUnavailable::DegenerateRange)
        );
    }

    #[test]
    fn incomplete_or_unavailable_candidate_is_not_an_observed_false() {
        let mut bars = history();
        bars[25] = bar(25, 1_037, 980, 1_000);
        let mut rows = causal_rows(&bars);
        let decision = T0 + 26 * HOUR + 1_000;
        assert!(matches!(
            at_decision(&rows, decision).expect("complete causal window"),
            RangeAssessment::Ready { .. }
        ));
        rows[25] = observed(bars[25], decision + 1);
        assert!(matches!(
            at_decision(&rows, decision).expect("candidate is not available"),
            RangeAssessment::Unavailable(_)
        ));
        let rows = causal_rows(&bars[1..]);
        assert!(matches!(
            at_decision(&rows, decision).expect("insufficient history"),
            RangeAssessment::Unavailable(_)
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn strict_threshold_is_exact_at_any_integer_price_scale(
            base in 100_000_i64..1_000_000_000,
            width in 1_i64..10_000,
        ) {
            let rows: Vec<_> = (0..26)
                .map(|i| bar(i, base + 2 * width, base - 2 * width, base))
                .collect();
            let mut equal = rows.clone();
            equal[25] = bar(25, base + 3 * width, base - width, base);
            prop_assert!(matches!(direct(&equal), RangeAssessment::KnownFalse(_)), "quarter-ATR equality is false");
            equal[25] = bar(25, base + 3 * width + 1, base - width, base);
            prop_assert!(matches!(direct(&equal), RangeAssessment::Ready { side: PositionSide::Short, .. }), "one tick above the exact threshold is short");
        }

        #[test]
        fn later_ohlc_mutation_cannot_change_an_active_causal_result(
            future_high in 1_100_i64..1_000_000,
            future_low in 1_i64..900,
        ) {
            let mut bars = history();
            bars[25] = bar(25, 1_037, 980, 1_000);
            let mut rows = causal_rows(&bars);
            let decision = T0 + 26 * HOUR + 1_000;
            let before = at_decision(&rows, decision).expect("baseline causal evaluation");
            prop_assert!(matches!(before, RangeAssessment::Ready { side: PositionSide::Short, .. }), "the immutable baseline is active");
            let future = bar(26, future_high, future_low, 1_000);
            rows.push(observed(future, T0 + 27 * HOUR));
            prop_assert_eq!(at_decision(&rows, decision).expect("future row is gated"), before);
            rows[26] = observed(bar(26, future_high + 1, future_low, 900), T0 + 27 * HOUR);
            prop_assert_eq!(at_decision(&rows, decision).expect("future mutation remains gated"), before);
            // Active past control: the admitted seed close raises the first true range.
            rows[0] = observed(bar(0, 5_000, 5_000, 5_000), T0 + HOUR);
            prop_assert!(matches!(at_decision(&rows, decision).expect("admitted past mutation"), RangeAssessment::KnownFalse(_)), "admitted past remains an active dependency");
        }
    }
}
