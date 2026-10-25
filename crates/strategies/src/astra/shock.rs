//! Confirmed shock reversal over a capped causal H1 window.

use std::num::NonZeroUsize;

use libm::{fabs, sqrt};
use parangsae_core::{DecisionTime, Price, ResampledBar, UtcMicros};
use parangsae_signals::{CausalWindow, SignalError};

use crate::series::price_f64;
use crate::{PositionSide, StrategyError};

use super::prepare_h1;

const BASELINE_RETURNS: usize = 672;
const SHOCK: usize = 674;
const LAST: usize = 676;

pub const SHOCK_BAR_CAP: NonZeroUsize = match NonZeroUsize::new(677) {
    Some(value) => value,
    None => unreachable!(),
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShockUnavailable {
    WindowIncomplete,
    ZeroScale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShockRejection {
    BelowShockThreshold,
    OutsideRetracement,
    NewExtreme,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShockFacts {
    pub shock_end: UtcMicros,
    pub confirmation_end: UtcMicros,
    pub decision_ts: DecisionTime,
    pub bar_close_time_us: UtcMicros,
    pub shock_return: f64,
    pub baseline_sd: f64,
    pub confirmation_return: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShockOpportunity {
    pub side: PositionSide,
    pub facts: ShockFacts,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShockAssessment {
    Unavailable(ShockUnavailable),
    KnownFalse {
        reason: ShockRejection,
        facts: ShockFacts,
    },
    Ready(ShockOpportunity),
}

pub fn assess_shock(
    window: &CausalWindow<'_, ResampledBar>,
) -> Result<ShockAssessment, StrategyError> {
    let Some(bars) = prepare_h1(window, SHOCK_BAR_CAP.get())? else {
        return Ok(ShockAssessment::Unavailable(
            ShockUnavailable::WindowIncomplete,
        ));
    };
    for bar in &bars {
        price_f64(bar.close())?;
    }
    let mut returns = [0.0; BASELINE_RETURNS];
    // Closes0..672 give return endpoints T-675h..T-4h. Close673 is
    // the deliberate gap, used only as the shock's preceding close.
    for (index, value) in returns.iter_mut().enumerate() {
        *value = simple_return(bars[index + 1].close(), bars[index].close())?;
    }
    let sd = sample_scale(&returns)?;
    if sd <= 0.0 {
        return Ok(ShockAssessment::Unavailable(ShockUnavailable::ZeroScale));
    }
    let shock = bars[SHOCK];
    let previous = bars[SHOCK - 1];
    let last = bars[LAST];
    let shock_return = simple_return(shock.close(), previous.close())?;
    let facts = ShockFacts {
        shock_end: end(shock)?,
        confirmation_end: end(last)?,
        decision_ts: window.decision(),
        bar_close_time_us: last.close_time(),
        shock_return,
        baseline_sd: sd,
        confirmation_return: simple_return(last.close(), shock.close())?,
    };
    if fabs(shock_return) <= 3.0 * sd {
        return Ok(ShockAssessment::KnownFalse {
            reason: ShockRejection::BelowShockThreshold,
            facts,
        });
    }
    let positive = shock.close() > previous.close();
    if !inside_retracement(previous.close(), shock.close(), last.close(), positive)? {
        return Ok(ShockAssessment::KnownFalse {
            reason: ShockRejection::OutsideRetracement,
            facts,
        });
    }
    let no_new_extreme = bars[SHOCK + 1..=LAST].iter().all(|bar| {
        if positive {
            bar.high() <= shock.high()
        } else {
            bar.low() >= shock.low()
        }
    });
    if !no_new_extreme {
        return Ok(ShockAssessment::KnownFalse {
            reason: ShockRejection::NewExtreme,
            facts,
        });
    }
    Ok(ShockAssessment::Ready(ShockOpportunity {
        side: if positive {
            PositionSide::Short
        } else {
            PositionSide::Long
        },
        facts,
    }))
}

fn end(bar: &ResampledBar) -> Result<UtcMicros, StrategyError> {
    bar.close_time()
        .checked_add_micros(1_000)
        .ok_or(StrategyError::Overflow)
}

fn simple_return(current: Price, previous: Price) -> Result<f64, StrategyError> {
    price_f64(current)?;
    let denominator = price_f64(previous)?;
    let difference = current
        .ticks()
        .checked_sub(previous.ticks())
        .ok_or(StrategyError::Overflow)?;
    let value = crate::series::signed_unit(difference)? / denominator;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(SignalError::InvalidFeature.into())
    }
}

fn sample_scale(values: &[f64; BASELINE_RETURNS]) -> Result<f64, StrategyError> {
    let count = u32::try_from(values.len()).map_err(|_| StrategyError::Overflow)?;
    let mean = values.iter().sum::<f64>() / f64::from(count);
    let sum = values
        .iter()
        .map(|value| {
            let difference = value - mean;
            difference * difference
        })
        .sum::<f64>();
    let variance = sum / f64::from(count - 1);
    if !mean.is_finite() || !variance.is_finite() || variance < 0.0 {
        return Err(SignalError::InvalidFeature.into());
    }
    let sd = sqrt(variance);
    if !sd.is_finite() || sd < 0.0 {
        Err(SignalError::InvalidFeature.into())
    } else {
        Ok(sd)
    }
}

fn inside_retracement(
    previous: Price,
    shock: Price,
    confirmed: Price,
    positive: bool,
) -> Result<bool, StrategyError> {
    let previous = i128::from(previous.ticks());
    let shock = i128::from(shock.ticks());
    let confirmed = i128::from(confirmed.ticks());
    let magnitude = (shock - previous).abs();
    let retraced = if positive {
        shock - confirmed
    } else {
        confirmed - shock
    };
    let observed = 4_i128
        .checked_mul(previous)
        .and_then(|value| value.checked_mul(retraced))
        .ok_or(StrategyError::Overflow)?;
    let lower = magnitude
        .checked_mul(shock)
        .ok_or(StrategyError::Overflow)?;
    let upper = lower.checked_mul(3).ok_or(StrategyError::Overflow)?;
    Ok(lower <= observed && observed <= upper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{AvailableTs, BarInterval, EventTs, Observation, ReceivedTs, Timestamps};
    use proptest::prelude::*;

    const SCALE: i64 = 100_000_000;
    const HOUR: i64 = 3_600_000_000;
    const ORIGIN: i64 = 1_704_067_200_000_000;

    fn price(ticks: i64) -> Price {
        Price::from_ticks(ticks).expect("positive synthetic price")
    }

    fn rows(positive: bool, flat_scale: bool, count: usize) -> Vec<Observation<ResampledBar>> {
        let mut out = Vec::with_capacity(count);
        let mut previous = 100 * SCALE;
        for index in 0..count {
            let close = match index {
                0..=672 if !flat_scale && index % 2 == 1 => 100 * SCALE + SCALE / 5,
                674 => {
                    if positive {
                        110 * SCALE
                    } else {
                        90 * SCALE
                    }
                }
                675 => {
                    if positive {
                        108 * SCALE
                    } else {
                        92 * SCALE
                    }
                }
                676 => {
                    if positive {
                        104 * SCALE + SCALE / 2
                    } else {
                        94 * SCALE + SCALE / 2
                    }
                }
                _ => 100 * SCALE,
            };
            let open = UtcMicros::from_micros(
                ORIGIN + i64::try_from(index).expect("bounded fixture") * HOUR,
            );
            let bar = ResampledBar::new(
                open,
                BarInterval::H1,
                price(previous),
                price(previous.max(close) + SCALE),
                price(previous.min(close) - SCALE),
                price(close),
            )
            .expect("consistent synthetic OHLC");
            let available = end(&bar).expect("fixture end");
            out.push(Observation::new(
                Timestamps {
                    event_ts: EventTs::from_micros(bar.close_time()),
                    available_ts: AvailableTs::from_micros(available),
                    received_ts: ReceivedTs::from_micros(available),
                },
                bar,
            ));
            previous = close;
        }
        out
    }

    fn decision(index: usize) -> DecisionTime {
        DecisionTime::from_micros(UtcMicros::from_micros(
            ORIGIN + (i64::try_from(index).expect("bounded fixture") + 1) * HOUR + 1_000,
        ))
    }

    fn assess(rows: &[Observation<ResampledBar>], at: DecisionTime) -> ShockAssessment {
        let views = rows.iter().filter_map(|row| row.causal_at(at));
        let window =
            CausalWindow::from_views(at, SHOCK_BAR_CAP, views).expect("causal ordered fixture");
        assess_shock(&window).expect("valid synthetic feature")
    }

    fn ready(value: ShockAssessment) -> ShockOpportunity {
        match value {
            ShockAssessment::Ready(event) => event,
            other => panic!("fixture must reach an active decision: {other:?}"),
        }
    }

    fn replace_close(row: &Observation<ResampledBar>, close: i64) -> Observation<ResampledBar> {
        let old = *row.value();
        row.with_value(
            ResampledBar::new(
                old.open_time(),
                old.interval(),
                old.open(),
                price(old.high().ticks().max(close)),
                price(old.low().ticks().min(close)),
                price(close),
            )
            .expect("consistent perturbed OHLC"),
        )
    }

    #[test]
    fn confirmed_positive_and_negative_shocks_reach_opposite_active_sides() {
        for (positive, side) in [(true, PositionSide::Short), (false, PositionSide::Long)] {
            let event = ready(assess(&rows(positive, false, 677), decision(LAST)));
            assert_eq!(event.side, side);
            assert_eq!(event.facts.shock_end.micros(), ORIGIN + 675 * HOUR);
            assert_eq!(event.facts.confirmation_end.micros(), ORIGIN + 677 * HOUR);
            assert_eq!(event.facts.decision_ts, decision(LAST));
            assert!(event.facts.baseline_sd > 0.0);
        }
    }

    #[test]
    fn incomplete_and_zero_scale_windows_are_unavailable_not_known_false() {
        assert_eq!(
            assess(&rows(true, false, 676), decision(675)),
            ShockAssessment::Unavailable(ShockUnavailable::WindowIncomplete)
        );
        assert_eq!(
            assess(&rows(true, true, 677), decision(LAST)),
            ShockAssessment::Unavailable(ShockUnavailable::ZeroScale)
        );
    }

    #[test]
    fn past_baseline_perturbation_changes_an_actual_ready_decision() {
        let original = rows(true, false, 677);
        ready(assess(&original, decision(LAST)));
        let mut changed = original;
        changed[300] = replace_close(&changed[300], 200 * SCALE);
        assert!(matches!(
            assess(&changed, decision(LAST)),
            ShockAssessment::KnownFalse {
                reason: ShockRejection::BelowShockThreshold,
                ..
            }
        ));
    }

    #[test]
    fn exactly_three_sigma_is_false_and_one_price_tick_more_reaches_ready() {
        let mut input = rows(true, false, 677);
        let mut close = 1_i64 << 48;
        input[0] = replace_close(&input[0], close);
        // Paired coefficients have sum0 and squared sum2684=671*4.
        // Thus672 ddof1 returns produce sd=2/64 exactly in binary.
        let coefficients = [36, -36, 6, -6, 3, -3, 1, -1];
        for (index, row) in input.iter_mut().enumerate().take(673).skip(1) {
            if let Some(coefficient) = coefficients.get(index - 1) {
                assert_eq!(close % 64, 0);
                close = close / 64 * (64 + coefficient);
            }
            *row = replace_close(row, close);
        }
        input[673] = replace_close(&input[673], close);
        assert_eq!(close % 32, 0);
        let shock = close / 32 * 35;
        input[674] = replace_close(&input[674], shock);
        input[675] = replace_close(&input[675], shock);
        input[676] = replace_close(&input[676], shock - shock * 3 / 64);
        let ShockAssessment::KnownFalse { reason, facts } = assess(&input, decision(LAST)) else {
            panic!("equality must fail the strict3sd condition");
        };
        assert_eq!(reason, ShockRejection::BelowShockThreshold);
        assert_eq!(facts.baseline_sd.to_bits(), (1.0_f64 / 32.0).to_bits());
        assert_eq!(facts.shock_return.to_bits(), (3.0_f64 / 32.0).to_bits());
        input[674] = replace_close(&input[674], shock + 1);
        assert_eq!(
            ready(assess(&input, decision(LAST))).side,
            PositionSide::Short
        );
    }

    #[test]
    fn the_deliberate_gap_hour_does_not_enter_the_baseline_scale() {
        let original = rows(true, false, 677);
        let before = ready(assess(&original, decision(LAST)));
        let mut changed = original;
        changed[673] = replace_close(&changed[673], 200 * SCALE);
        let after = match assess(&changed, decision(LAST)) {
            ShockAssessment::KnownFalse { facts, .. } => facts,
            ShockAssessment::Ready(event) => event.facts,
            other => panic!("scale is still available: {other:?}"),
        };
        assert_eq!(
            before.facts.baseline_sd.to_bits(),
            after.baseline_sd.to_bits()
        );
    }

    #[test]
    fn retracement_quarters_are_inclusive_and_one_tick_outside_is_rejected() {
        for (positive, boundary) in [
            (true, 107 * SCALE + SCALE / 4),
            (true, 101 * SCALE + 3 * SCALE / 4),
            (false, 92 * SCALE + SCALE / 4),
            (false, 96 * SCALE + 3 * SCALE / 4),
        ] {
            let mut input = rows(positive, false, 677);
            input[LAST] = replace_close(&input[LAST], boundary);
            ready(assess(&input, decision(LAST)));
        }
        let mut input = rows(true, false, 677);
        input[LAST] = replace_close(&input[LAST], 107 * SCALE + SCALE / 4 + 1);
        assert!(matches!(
            assess(&input, decision(LAST)),
            ShockAssessment::KnownFalse {
                reason: ShockRejection::OutsideRetracement,
                ..
            }
        ));
    }

    #[test]
    fn a_new_confirmation_extreme_is_a_valid_known_false() {
        let mut input = rows(true, false, 677);
        let old = *input[675].value();
        input[675] = input[675].with_value(
            ResampledBar::new(
                old.open_time(),
                old.interval(),
                old.open(),
                price(111 * SCALE + 1),
                old.low(),
                old.close(),
            )
            .expect("consistent higher confirmation high"),
        );
        assert!(matches!(
            assess(&input, decision(LAST)),
            ShockAssessment::KnownFalse {
                reason: ShockRejection::NewExtreme,
                ..
            }
        ));
    }

    #[test]
    fn nonfinite_scale_is_a_typed_error_instead_of_unavailable() {
        let mut values = [0.0; BASELINE_RETURNS];
        values[0] = f64::NAN;
        assert!(matches!(
            sample_scale(&values),
            Err(StrategyError::Signal(SignalError::InvalidFeature))
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]
        #[test]
        fn future_suffix_mutation_and_truncation_preserve_an_active_prefix(
            positive in any::<bool>(), future in 10_000_000_000i64..10_000_000_000_000i64,
        ) {
            let full = rows(positive, false, 690);
            let expected = assess(&full[..677], decision(LAST));
            ready(expected);
            prop_assert_eq!(assess(&full, decision(LAST)), expected);
            let mut changed = full;
            for row in &mut changed[677..] { *row = replace_close(row, future); }
            prop_assert_eq!(assess(&changed, decision(LAST)), expected);
        }
    }
}
