//! Warmup, emit-every-bar counts, and the structural rejections.

#[path = "support/mod.rs"]
mod support;

use std::cmp::Ordering;

use parangsae_core::{BarInterval, Ratio, ReceivedTs};
use parangsae_signals::{
    drive, drive_feature, drive_funding_z, funding_observation, CausalFeature, CausalSignal,
    Decision, FlatReason, FundingZ, ParamError, SignalError, SignalOutput, Tsmom, VolRegime,
    PROD_MAX_AGE_US,
};
use support::{
    at, bar_ending_at, bar_obs, contiguous_bars, fund_obs, hour, restamp, us, usdt_ticks, H4_US,
    SLOT_US, T0,
};

fn classify_funding(got: Result<Option<SignalOutput>, SignalError>) -> &'static str {
    match got {
        Ok(None) => "none",
        Ok(Some(out)) => match out.decision {
            Decision::Target(_) => "target",
            Decision::Flat(FlatReason::FundingStale) => "stale",
            Decision::Flat(FlatReason::FundingSlotMissing) => "missing",
        },
        Err(err) => panic!("emit-every-bar returned {err:?}"),
    }
}

mod warmup_emit {
    use super::*;

    #[test]
    fn prod_params_match_the_registered_grid() {
        let tsmom_h1 = Tsmom::<24>::new(BarInterval::H1, 0.05).unwrap();
        let tsmom_h4 = Tsmom::<42>::new(BarInterval::H4, 0.10).unwrap();
        let vol_h1 = VolRegime::<24, 168>::new(BarInterval::H1).unwrap();
        let vol_h4 = VolRegime::<42, 180>::new(BarInterval::H4).unwrap();
        let funding = FundingZ::<90>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        assert_eq!(tsmom_h1.n(), 24);
        assert_eq!(tsmom_h4.n(), 42);
        assert_eq!(tsmom_h1.r_scale().partial_cmp(&0.05), Some(Ordering::Equal));
        assert_eq!(tsmom_h4.r_scale().partial_cmp(&0.10), Some(Ordering::Equal));
        assert_eq!(vol_h1.w(), 24);
        assert_eq!(vol_h1.k(), 168);
        assert_eq!(vol_h4.w(), 42);
        assert_eq!(vol_h4.k(), 180);
        assert_eq!(funding.m(), 90);
        assert_eq!(funding.z_scale().partial_cmp(&2.0), Some(Ordering::Equal));
        assert_eq!(funding.max_age_us(), PROD_MAX_AGE_US);
        assert_eq!(Tsmom::<24>::WINDOW_CAP.get(), 25);
        assert_eq!(Tsmom::<42>::WINDOW_CAP.get(), 43);
        assert_eq!(VolRegime::<24, 168>::WINDOW_CAP.get(), 193);
        assert_eq!(VolRegime::<42, 180>::WINDOW_CAP.get(), 223);
        assert_eq!(FundingZ::<90>::FUNDING_CAP.get(), 90);
        assert_eq!(FundingZ::<90>::BAR_CAP.get(), 1);
    }

    #[test]
    fn h1_emit_counts_and_flat_reasons() {
        let bars = contiguous_bars(BarInterval::H1, 800, |index| {
            usdt_ticks(40_000) + index * 1_000_000
        });
        let funding: Vec<_> = (0..100)
            .filter(|slot| !matches!(slot, 92..=94))
            .map(|slot| fund_obs(T0 + slot * SLOT_US, slot * 1_000 - 5_000, 8))
            .collect();
        let tsmom = Tsmom::<24>::new(BarInterval::H1, 0.05).unwrap();
        let vol = VolRegime::<24, 168>::new(BarInterval::H1).unwrap();
        let funding_z = FundingZ::<90>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let mut tsmom_some = 0_i64;
        let mut vol_some = 0_i64;
        let mut funding_none = 0_i64;
        let mut funding_target = 0_i64;
        let mut funding_stale = 0_i64;
        let mut funding_missing = 0_i64;
        for bar in 1..=800 {
            let decision = at(hour(bar));
            match drive(&tsmom, &bars, decision).unwrap() {
                Some(out) => {
                    assert!(bar >= 25, "bar {bar}");
                    assert!(matches!(out.decision, Decision::Target(_)));
                    tsmom_some += 1;
                }
                None => assert!(bar < 25, "bar {bar}"),
            }
            match drive_feature(&vol, &bars, decision).unwrap() {
                Some(out) => {
                    assert!(bar >= 193, "bar {bar}");
                    assert!((0..=100_000_000).contains(&out.value.ticks()));
                    vol_some += 1;
                }
                None => assert!(bar < 193, "bar {bar}"),
            }
            let class = classify_funding(drive_funding_z(&funding_z, &bars, &funding, decision));
            match (bar, class) {
                (1..=712, "none") => funding_none += 1,
                (713..=736, "target") => funding_target += 1,
                (737..=760, "stale") => funding_stale += 1,
                (761..=800, "missing") => funding_missing += 1,
                _ => panic!("bar {bar} classified as {class}"),
            }
        }
        assert_eq!(tsmom_some, 776);
        assert_eq!(vol_some, 608);
        assert_eq!(
            (funding_none, funding_target, funding_stale, funding_missing),
            (712, 24, 24, 40)
        );
    }

    #[test]
    fn h4_emit_counts_and_flat_reasons() {
        let bars = contiguous_bars(BarInterval::H4, 228, |index| {
            usdt_ticks(40_000) + index * 1_000_000
        });
        let funding: Vec<_> = (0..114)
            .filter(|slot| !matches!(slot, 100..=102))
            .map(|slot| fund_obs(T0 + slot * SLOT_US, slot * 1_000 - 5_000, 8))
            .collect();
        let tsmom = Tsmom::<42>::new(BarInterval::H4, 0.10).unwrap();
        let vol = VolRegime::<42, 180>::new(BarInterval::H4).unwrap();
        let funding_z = FundingZ::<90>::new(BarInterval::H4, 2.0, PROD_MAX_AGE_US).unwrap();
        let mut tsmom_some = 0_i64;
        let mut vol_some = 0_i64;
        let mut funding_none = 0_i64;
        let mut funding_target = 0_i64;
        let mut funding_stale = 0_i64;
        let mut funding_missing = 0_i64;
        for bar in 1..=228 {
            let decision = at(T0 + bar * H4_US);
            match drive(&tsmom, &bars, decision).unwrap() {
                Some(_) => {
                    assert!(bar >= 43, "bar {bar}");
                    tsmom_some += 1;
                }
                None => assert!(bar < 43, "bar {bar}"),
            }
            match drive_feature(&vol, &bars, decision).unwrap() {
                Some(_) => {
                    assert!(bar >= 223, "bar {bar}");
                    vol_some += 1;
                }
                None => assert!(bar < 223, "bar {bar}"),
            }
            let class = classify_funding(drive_funding_z(&funding_z, &bars, &funding, decision));
            match (bar, class) {
                (1..=178, "none") => funding_none += 1,
                (179..=200, "target") => funding_target += 1,
                (201..=206, "stale") => funding_stale += 1,
                (207..=228, "missing") => funding_missing += 1,
                _ => panic!("bar {bar} classified as {class}"),
            }
        }
        assert_eq!(tsmom_some, 186);
        assert_eq!(vol_some, 6);
        assert_eq!(
            (funding_none, funding_target, funding_stale, funding_missing),
            (178, 22, 6, 22)
        );
    }

    #[test]
    fn a_gap_inside_the_lookback_is_an_error() {
        let signal = Tsmom::<3>::new(BarInterval::H1, 0.05).unwrap();
        let bars = vec![
            bar_obs(hour(0), BarInterval::H1, usdt_ticks(40_000)),
            bar_obs(hour(1), BarInterval::H1, usdt_ticks(40_100)),
            bar_obs(hour(2), BarInterval::H1, usdt_ticks(40_200)),
            bar_obs(hour(4), BarInterval::H1, usdt_ticks(40_400)),
        ];
        assert_eq!(
            drive(&signal, &bars, at(hour(5))),
            Err(SignalError::Gap {
                expected_open: us(hour(3)),
            })
        );
    }

    #[test]
    fn decision_off_the_bar_is_not_a_trigger() {
        let signal = Tsmom::<3>::new(BarInterval::H1, 0.05).unwrap();
        let bars = contiguous_bars(BarInterval::H1, 6, |_| usdt_ticks(40_000));
        assert_eq!(
            drive(&signal, &bars, at(hour(6) - 1)),
            Err(SignalError::NoTriggeringBar)
        );
    }

    #[test]
    fn clock_mismatch_is_rejected() {
        let signal = Tsmom::<1>::new(BarInterval::H1, 0.05).unwrap();
        let bar = bar_obs(hour(0), BarInterval::H1, usdt_ticks(40_000));
        let shifted = restamp(&bar, hour(0), hour(1) - 1);
        assert_eq!(
            drive(&signal, &[shifted], at(hour(1) - 1)),
            Err(SignalError::ClockMismatch)
        );
    }

    #[test]
    fn interval_mismatch_beats_warmup() {
        let signal = Tsmom::<24>::new(BarInterval::H1, 0.05).unwrap();
        let bar = bar_obs(T0, BarInterval::H4, usdt_ticks(40_000));
        assert_eq!(
            drive(&signal, &[bar], at(T0 + H4_US)),
            Err(SignalError::IntervalMismatch)
        );
    }

    #[test]
    fn duplicate_bar_is_rejected() {
        let signal = Tsmom::<1>::new(BarInterval::H1, 0.05).unwrap();
        let bar = bar_obs(hour(0), BarInterval::H1, usdt_ticks(40_000));
        assert_eq!(
            drive(&signal, &[bar.clone(), bar], at(hour(1))),
            Err(SignalError::DuplicateBar)
        );
    }

    #[test]
    fn duplicate_funding_time_is_rejected() {
        let signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let row = fund_obs(hour(0), 10_000, 8);
        let bars = vec![bar_ending_at(hour(1), BarInterval::H1, usdt_ticks(40_000))];
        assert_eq!(
            drive_funding_z(
                &signal,
                &bars,
                &[row, fund_obs(hour(0), 20_000, 8)],
                at(hour(1))
            ),
            Err(SignalError::DuplicateFunding)
        );
    }

    #[test]
    fn non_positive_close_is_invalid() {
        let signal = Tsmom::<3>::new(BarInterval::H1, 0.05).unwrap();
        let bar = bar_obs(hour(0), BarInterval::H1, 0);
        assert_eq!(
            drive(&signal, &[bar], at(hour(1))),
            Err(SignalError::InvalidFeature)
        );
    }

    #[test]
    fn price_above_2_pow_53_is_invalid_and_the_bound_is_allowed() {
        let signal = Tsmom::<1>::new(BarInterval::H1, 0.05).unwrap();
        let too_big = (1_i64 << 53) + 1;
        let bar = bar_obs(hour(0), BarInterval::H1, too_big);
        assert_eq!(
            drive(&signal, &[bar], at(hour(1))),
            Err(SignalError::InvalidFeature)
        );
        let exact = 1_i64 << 53;
        let bars = vec![
            bar_obs(hour(0), BarInterval::H1, exact),
            bar_obs(hour(1), BarInterval::H1, exact),
        ];
        let got = drive(&signal, &bars, at(hour(2))).unwrap();
        assert_eq!(
            got.expect("exact mantissa emits").decision.position_ticks(),
            0
        );
    }

    #[test]
    fn params_reject_bad_lookback_scale_and_age() {
        assert!(matches!(
            Tsmom::<0>::new(BarInterval::H1, 0.05),
            Err(ParamError::Lookback)
        ));
        assert!(matches!(
            VolRegime::<0, 1>::new(BarInterval::H1),
            Err(ParamError::Lookback)
        ));
        assert!(matches!(
            VolRegime::<1, 0>::new(BarInterval::H1),
            Err(ParamError::Lookback)
        ));
        assert!(matches!(
            FundingZ::<0>::new(BarInterval::H1, 2.0, 1),
            Err(ParamError::Lookback)
        ));
        assert!(matches!(
            FundingZ::<1>::new(BarInterval::H1, 2.0, 1),
            Err(ParamError::Lookback)
        ));
        for scale in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            assert!(matches!(
                Tsmom::<3>::new(BarInterval::H1, scale),
                Err(ParamError::Scale)
            ));
            assert!(matches!(
                FundingZ::<4>::new(BarInterval::H1, scale, 1),
                Err(ParamError::Scale)
            ));
        }
        assert!(matches!(
            FundingZ::<4>::new(BarInterval::H1, 2.0, 0),
            Err(ParamError::MaxAge)
        ));
        assert!(matches!(
            FundingZ::<4>::new(BarInterval::H1, 2.0, -5),
            Err(ParamError::MaxAge)
        ));
        let overflow = funding_observation(
            us(i64::MAX - 10),
            Ratio::from_ticks(1),
            8,
            ReceivedTs::from_micros(us(0)),
        );
        assert_eq!(overflow, Err(SignalError::Overflow));
    }

    #[test]
    fn volregime_depends_on_the_oldest_in_window_bar_only() {
        let feature = VolRegime::<2, 3>::new(BarInterval::H1).unwrap();
        // Index 0 is outside the cap. Indices 1..=6 repeat two returns so the
        // oldest in-window close is the only thing tying one reference RV.
        let base = vec![
            5_000_000_000,
            10_000_000_000,
            20_000_000_000,
            10_000_000_000,
            40_000_000_000,
            80_000_000_000,
            40_000_000_000,
        ];
        let bars = |closes: &[i64]| {
            closes
                .iter()
                .enumerate()
                .map(|(index, ticks)| {
                    bar_obs(hour(i64::try_from(index).unwrap()), BarInterval::H1, *ticks)
                })
                .collect::<Vec<_>>()
        };
        let decision = at(hour(7));
        let original = drive_feature(&feature, &bars(&base), decision)
            .unwrap()
            .expect("in window");
        let mut outside = base.clone();
        outside[0] += 5_000_000_000;
        let untouched = drive_feature(&feature, &bars(&outside), decision)
            .unwrap()
            .expect("outside the cap");
        assert_eq!(untouched, original);
        let mut inside = base.clone();
        inside[1] += 5_000_000_000;
        let changed = drive_feature(&feature, &bars(&inside), decision)
            .unwrap()
            .expect("oldest in window");
        assert_ne!(changed.value.ticks(), original.value.ticks());
    }
}
