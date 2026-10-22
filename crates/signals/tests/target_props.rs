//! `Target` and `Rank` totality. These generators sample the map. They are not a proof.

#[path = "support/mod.rs"]
mod support;

use std::cmp::Ordering;

use libm::trunc;

use parangsae_core::BarInterval;
use parangsae_signals::{
    drive, drive_feature, drive_funding_z, Decision, FlatReason, FundingZ, Rank, RankError,
    SignalError, SignalOutput, Target, TargetError, Tsmom, VolRegime, FEATURE_IDS, PROD_MAX_AGE_US,
    RANK_SCALE, TARGET_SCALE,
};
use proptest::prelude::*;
use support::{at, contiguous_bars, fund_obs, hour, usdt_ticks, SLOT_US, T0};

fn trunc_to_i64(value: f64) -> Result<i64, TargetError> {
    let bits = value.to_bits();
    let negative = (bits >> 63) == 1;
    let exponent = i32::try_from((bits >> 52) & 0x7ff).map_err(|_| TargetError::NonFinite)?;
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent == 0 {
        return Ok(0);
    }
    if exponent == 0x7ff {
        return Err(TargetError::NonFinite);
    }
    let unbiased = exponent - 1023;
    if unbiased < 0 {
        return Ok(0);
    }
    if unbiased > 62 {
        return Err(TargetError::NonFinite);
    }
    let shift = u32::try_from(unbiased).map_err(|_| TargetError::NonFinite)?;
    let mantissa = (1_u64 << 52) | fraction;
    let magnitude = if shift >= 52 {
        mantissa << (shift - 52)
    } else {
        mantissa >> (52 - shift)
    };
    let ticks = i64::try_from(magnitude).map_err(|_| TargetError::NonFinite)?;
    if negative {
        Ok(-ticks)
    } else {
        Ok(ticks)
    }
}

fn assert_emitted_target(out: &SignalOutput) {
    assert_eq!(
        out.decision_ts.micros().micros(),
        out.bar_close_time_us.micros() + 1_000
    );
    let ticks = out.decision.position_ticks();
    assert!((-TARGET_SCALE..=TARGET_SCALE).contains(&ticks));
    if let Decision::Flat(reason) = out.decision {
        assert_eq!(ticks, 0);
        assert!(matches!(
            reason,
            FlatReason::FundingStale | FlatReason::FundingSlotMissing
        ));
    }
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x1234_5678_9ABC_DEF0
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, span: u64) -> u64 {
        self.next_u64() % span
    }
}

mod target_props {
    use super::*;

    #[test]
    fn special_values_map_to_the_stated_ticks() {
        for bits in [
            f64::NAN.to_bits(),
            0x7ff8_0000_0000_0001,
            0x7ff0_0000_0000_0001,
            f64::INFINITY.to_bits(),
            f64::NEG_INFINITY.to_bits(),
        ] {
            assert_eq!(
                Target::from_f64(f64::from_bits(bits)),
                Err(TargetError::NonFinite)
            );
        }
        assert_eq!(Target::from_f64(0.0).unwrap().ticks(), 0);
        assert_eq!(Target::from_f64(-0.0).unwrap().ticks(), 0);
        assert_eq!(Target::from_f64(f64::MIN_POSITIVE).unwrap().ticks(), 0);
        assert_eq!(Target::from_f64(f64::from_bits(1)).unwrap().ticks(), 0);
        assert_eq!(Target::from_f64(1.0).unwrap().ticks(), TARGET_SCALE);
        assert_eq!(Target::from_f64(-1.0).unwrap().ticks(), -TARGET_SCALE);
        assert_eq!(Target::from_f64(0.999_999_999).unwrap().ticks(), 99_999_999);
        assert_eq!(Target::from_f64(-0.000_000_009).unwrap().ticks(), 0);
        assert_eq!(Target::from_f64(f64::MAX).unwrap().ticks(), TARGET_SCALE);
        assert_eq!(Target::from_f64(f64::MIN).unwrap().ticks(), -TARGET_SCALE);
        assert_eq!(Target::from_f64(2.0).unwrap().ticks(), TARGET_SCALE);
        assert_eq!(Target::from_f64(-2.0).unwrap().ticks(), -TARGET_SCALE);
    }

    #[test]
    fn rank_rejects_empty_and_non_finite() {
        assert_eq!(Rank::of(1.0, &[]), Err(RankError::EmptyReference));
        assert_eq!(Rank::of(f64::NAN, &[0.0]), Err(RankError::NonFinite));
        assert_eq!(Rank::of(0.0, &[f64::INFINITY]), Err(RankError::NonFinite));
        assert!(!FEATURE_IDS.contains(&"tsmom"));
    }

    // These generators sample the maps. They are not a proof.
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn any_bits_are_total(bits in any::<u64>()) {
            let value = f64::from_bits(bits);
            let got = Target::from_f64(value);
            if value.is_finite() {
                let ticks = got.unwrap().ticks();
                prop_assert!((-TARGET_SCALE..=TARGET_SCALE).contains(&ticks));
            } else {
                prop_assert_eq!(got, Err(TargetError::NonFinite));
            }
        }

        #[test]
        fn finite_unit_interval_matches_truncation(bits in any::<u64>()) {
            let value = f64::from_bits(bits);
            prop_assume!(value.is_finite());
            prop_assume!((-1.0..=1.0).contains(&value));
            let expected = trunc_to_i64(trunc(value * 100_000_000.0)).unwrap();
            prop_assert_eq!(Target::from_f64(value).unwrap().ticks(), expected);
        }

        #[test]
        fn outside_unit_interval_saturates(bits in any::<u64>()) {
            let value = f64::from_bits(bits);
            prop_assume!(value.is_finite());
            if value > 1.0 {
                prop_assert_eq!(Target::from_f64(value).unwrap().ticks(), TARGET_SCALE);
            } else if value < -1.0 {
                prop_assert_eq!(Target::from_f64(value).unwrap().ticks(), -TARGET_SCALE);
            }
        }

        #[test]
        fn rank_matches_the_integer_formula(seed in any::<u64>()) {
            let mut rng = Rng::new(seed);
            let len = usize::try_from(rng.below(400) + 1).unwrap();
            let cur_i = i16::try_from(rng.below(2_001)).unwrap() - 1_000;
            let cur = f64::from(cur_i);
            let mut refs = Vec::with_capacity(len);
            for index in 0..len {
                let sample = i16::try_from(rng.below(2_001)).unwrap() - 1_000;
                let value = if index == 0 { cur } else { f64::from(sample) };
                refs.push(value);
            }
            let mut less = 0_u64;
            let mut equal = 0_u64;
            for reference in &refs {
                match reference.partial_cmp(&cur) {
                    Some(Ordering::Less) => less += 1,
                    Some(Ordering::Equal) => equal += 1,
                    Some(Ordering::Greater) => {}
                    None => prop_assert!(false, "finite inputs"),
                }
            }
            let k = u64::try_from(refs.len()).unwrap();
            let expected = i64::try_from((less * 2 + equal) * 100_000_000 / (k * 2)).unwrap();
            let rank = Rank::of(cur, &refs).unwrap();
            prop_assert_eq!(rank.ticks(), expected);
            prop_assert!((0..=RANK_SCALE).contains(&rank.ticks()));

            let higher = f64::from(cur_i.saturating_add(1));
            let raised = Rank::of(higher, &refs).unwrap().ticks();
            prop_assert!(raised >= rank.ticks());
        }

        #[test]
        fn non_finite_rank_inputs_are_errors(seed in any::<u64>()) {
            let mut rng = Rng::new(seed);
            let refs = [0.0, 1.0, -1.0];
            let bad = f64::from_bits(0x7ff8_0000_0000_0001);
            let slot = usize::try_from(rng.below(4)).unwrap();
            if slot == 3 {
                prop_assert_eq!(Rank::of(bad, &refs), Err(RankError::NonFinite));
            } else {
                let mut values = refs;
                values[slot] = bad;
                prop_assert_eq!(Rank::of(0.0, &values), Err(RankError::NonFinite));
            }
        }

        #[test]
        fn emitted_decisions_stay_in_range(seed in any::<u64>()) {
            let mut rng = Rng::new(seed);
            let count = i64::try_from(rng.below(40) + 1).unwrap();
            let bars = contiguous_bars(BarInterval::H1, count, |index| {
                usdt_ticks(30_000) + index * 1_000_000
            });
            let tsmom = Tsmom::<3>::new(BarInterval::H1, 0.05).unwrap();
            let vol = VolRegime::<2, 3>::new(BarInterval::H1).unwrap();
            let funding_signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
            let funding: Vec<_> = (0..8)
                .map(|slot| {
                    let rate = slot * 1_000 - 500;
                    fund_obs(T0 + slot * SLOT_US, rate, 8)
                })
                .collect();
            for bar in 1..=count {
                let decision = at(hour(bar));
                match drive(&tsmom, &bars, decision) {
                    Ok(Some(out)) => {
                        assert_emitted_target(&out);
                        prop_assert!(matches!(out.decision, Decision::Target(_)));
                    }
                    Ok(None) => {}
                    Err(SignalError::NoTriggeringBar) => {}
                    Err(err) => prop_assert!(false, "{err:?}"),
                }
                match drive_feature(&vol, &bars, decision) {
                    Ok(Some(out)) => {
                        prop_assert_eq!(
                            out.decision_ts.micros().micros(),
                            out.bar_close_time_us.micros() + 1_000
                        );
                        prop_assert!((0..=RANK_SCALE).contains(&out.value.ticks()));
                    }
                    Ok(None) => {}
                    Err(SignalError::NoTriggeringBar) => {}
                    Err(err) => prop_assert!(false, "{err:?}"),
                }
                match drive_funding_z(&funding_signal, &bars, &funding, decision) {
                    Ok(Some(out)) => assert_emitted_target(&out),
                    Ok(None) => {}
                    Err(SignalError::NoTriggeringBar) => {}
                    Err(err) => prop_assert!(false, "{err:?}"),
                }
            }
        }
    }
}
