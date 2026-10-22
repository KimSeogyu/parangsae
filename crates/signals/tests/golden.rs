//! Exact ticks from the v0.1 fixtures. Flat is only stale funding or a missing slot.

#[path = "support/mod.rs"]
mod support;

use parangsae_core::{BarInterval, Ratio, ReceivedTs};
use parangsae_signals::{
    drive, drive_feature, drive_funding_z, funding_observation, Decision, FlatReason, FundingZ,
    Rank, SignalError, Target, Tsmom, VolRegime, PROD_MAX_AGE_US, RANK_SCALE,
};
use support::{
    at, bar_ending_at, expect_feature, expect_flat, expect_none, expect_target, fund_custom,
    fund_obs, gap_funding, hour, ramp_bars, toy_funding, toy_tsmom_bars, toy_vol_bars, us,
    usdt_ticks,
};

mod golden {
    use super::*;

    #[test]
    fn g_tsmom() {
        let signal = Tsmom::<3>::new(BarInterval::H1, 0.05).unwrap();
        let bars = toy_tsmom_bars();
        expect_none(drive(&signal, &bars, at(hour(3))));
        expect_target(drive(&signal, &bars, at(hour(4))), 49_385_225);
        expect_target(drive(&signal, &bars, at(hour(5))), -70_536_277);
        expect_target(drive(&signal, &bars, at(hour(6))), 100_000_000);
    }

    #[test]
    fn g_tsmom_prod_shape() {
        let h1 = Tsmom::<24>::new(BarInterval::H1, 0.05).unwrap();
        let (bars, decision) =
            ramp_bars(BarInterval::H1, 24, usdt_ticks(40_000), usdt_ticks(40_800));
        expect_target(drive(&h1, &bars, decision), 39_605_254);

        let h4 = Tsmom::<42>::new(BarInterval::H4, 0.10).unwrap();
        let (bars, decision) =
            ramp_bars(BarInterval::H4, 42, usdt_ticks(40_000), usdt_ticks(50_000));
        expect_target(drive(&h4, &bars, decision), 100_000_000);
    }

    #[test]
    fn g_volregime() {
        let feature = VolRegime::<2, 3>::new(BarInterval::H1).unwrap();
        let closes = [
            10_000_000_000,
            11_000_000_000,
            9_900_000_000,
            9_900_000_000,
            10_890_000_000,
            9_801_000_000,
        ];
        let bars = toy_vol_bars(&closes);
        assert_eq!(drive_feature(&feature, &bars, at(hour(5))), Ok(None));
        expect_feature(drive_feature(&feature, &bars, at(hour(6))), 83_333_333);
    }

    #[test]
    fn g_volregime_flat_closes() {
        let feature = VolRegime::<2, 3>::new(BarInterval::H1).unwrap();
        let bars = toy_vol_bars(&[10_000_000_000; 6]);
        expect_feature(drive_feature(&feature, &bars, at(hour(6))), 50_000_000);
    }

    #[test]
    fn g_volregime_extremes_and_prod_integers() {
        assert_eq!(Rank::of(1.0, &[0.0, 0.0, 0.0]).unwrap().ticks(), RANK_SCALE);
        assert_eq!(Rank::of(0.0, &[1.0, 2.0, 3.0]).unwrap().ticks(), 0);

        let mut refs = vec![0.0; 100];
        refs.extend(std::iter::repeat_n(2.0, 67));
        refs.push(1.0);
        assert_eq!(refs.len(), 168);
        assert_eq!(Rank::of(1.0, &refs).unwrap().ticks(), 59_821_428);

        let mut wide = vec![0.0; 179];
        wide.push(1.0);
        assert_eq!(wide.len(), 180);
        assert_eq!(Rank::of(1.0, &wide).unwrap().ticks(), 99_722_222);
    }

    #[test]
    fn g_funding_z() {
        let signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let funding = toy_funding();
        let at_bar = |h| {
            let bars = vec![bar_ending_at(hour(h), BarInterval::H1, usdt_ticks(40_000))];
            drive_funding_z(&signal, &bars, &funding, at(hour(h)))
        };
        expect_none(at_bar(24));
        expect_target(at_bar(25), -81_649_658);
        expect_target(at_bar(33), 69_337_524);
        expect_flat(at_bar(41), FlatReason::FundingStale);
    }

    #[test]
    fn g_funding_constant_is_target_zero() {
        let signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let funding: Vec<_> = [0, 8, 16, 24]
            .into_iter()
            .map(|h| fund_obs(hour(h), 10_000, 8))
            .collect();
        let bars = vec![bar_ending_at(hour(25), BarInterval::H1, usdt_ticks(40_000))];
        let got = drive_funding_z(&signal, &bars, &funding, at(hour(25))).unwrap();
        let out = got.expect("constant window emits");
        assert_eq!(out.decision, Decision::Target(Target::FLAT));
        assert_eq!(out.decision.target_ticks(), 0);
    }

    #[test]
    fn g_funding_sigma_zero_is_target_zero() {
        let signal = FundingZ::<2>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let huge = 1_i64 << 53;
        let funding = vec![fund_obs(hour(0), huge, 8), fund_obs(hour(8), huge + 1, 8)];
        let bars = vec![bar_ending_at(hour(9), BarInterval::H1, usdt_ticks(40_000))];
        let got = drive_funding_z(&signal, &bars, &funding, at(hour(9))).unwrap();
        let out = got.expect("degenerate sigma emits");
        assert_eq!(out.decision, Decision::Target(Target::FLAT));
    }

    #[test]
    fn g_funding_gap() {
        let signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let funding = gap_funding();
        let at_bar = |h| {
            let bars = vec![bar_ending_at(hour(h), BarInterval::H1, usdt_ticks(40_000))];
            drive_funding_z(&signal, &bars, &funding, at(hour(h)))
        };
        expect_none(at_bar(25));
        expect_flat(at_bar(33), FlatReason::FundingSlotMissing);
        expect_flat(at_bar(41), FlatReason::FundingSlotMissing);
        expect_target(at_bar(49), -26_726_124);
        let bridged = at_bar(33).unwrap().expect("emitted");
        assert_ne!(bridged.decision.target_ticks(), 63_012_603);
    }

    #[test]
    fn g_funding_interval() {
        let signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let mut funding = toy_funding();
        let bad = funding.pop().unwrap();
        funding.push(fund_obs(bad.value().funding_time().micros(), -10_000, 4));
        let at_bar = |h| {
            let bars = vec![bar_ending_at(hour(h), BarInterval::H1, usdt_ticks(40_000))];
            drive_funding_z(&signal, &bars, &funding, at(hour(h)))
        };
        expect_target(at_bar(25), -81_649_658);
        assert_eq!(
            at_bar(33),
            Err(SignalError::FundingIntervalChange {
                funding_time: us(hour(32)),
                interval_hours: 4,
            })
        );
    }

    #[test]
    fn g_funding_edge() {
        let row = funding_observation(
            us(hour(24)),
            Ratio::from_ticks(10_000),
            8,
            ReceivedTs::from_micros(us(hour(24))),
        )
        .unwrap();
        assert!(row.causal_at(at(hour(24) + 60_000_000)).is_some());
        assert!(row.causal_at(at(hour(24) + 59_000_000)).is_none());
        assert!(row.causal_at(at(hour(24) + 60_000_000 - 1)).is_none());

        let signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let funding = toy_funding();
        let bars = vec![bar_ending_at(hour(24), BarInterval::H1, usdt_ticks(40_000))];
        expect_none(drive_funding_z(&signal, &bars, &funding, at(hour(24))));

        let mut off_grid = toy_funding();
        off_grid.push(fund_custom(hour(32) + 5_000, 1_000, 8, 60_000_000));
        let bars = vec![bar_ending_at(hour(33), BarInterval::H1, usdt_ticks(40_000))];
        assert_eq!(
            drive_funding_z(&signal, &bars, &off_grid, at(hour(33))),
            Err(SignalError::FundingOffGrid {
                funding_time: us(hour(32) + 5_000),
            })
        );

        let d = hour(33);
        let mut hidden = toy_funding();
        let newest = hidden.pop().unwrap();
        hidden.push(support::restamp(
            &newest,
            newest.value().funding_time().micros(),
            d + 1,
        ));
        let bars = vec![bar_ending_at(d, BarInterval::H1, usdt_ticks(40_000))];
        expect_flat(
            drive_funding_z(&signal, &bars, &hidden, at(d)),
            FlatReason::FundingStale,
        );

        let mut included = toy_funding();
        let newest = included.pop().unwrap();
        included.push(support::restamp(
            &newest,
            newest.value().funding_time().micros(),
            d,
        ));
        expect_target(
            drive_funding_z(&signal, &bars, &included, at(d)),
            69_337_524,
        );
    }
}
