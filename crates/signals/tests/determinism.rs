//! Same inputs, fresh instances and a reused instance, agree on the tick.

#[path = "support/mod.rs"]
mod support;

pub use support::Must;

use parangsae_core::BarInterval;
use parangsae_signals::{
    drive, drive_feature, drive_funding_z, FundingZ, Tsmom, VolRegime, PROD_MAX_AGE_US,
};
use support::{at, bar_ending_at, hour, toy_funding, toy_tsmom_bars, toy_vol_bars, usdt_ticks};

mod determinism {
    use super::*;

    #[test]
    fn repeated_evaluation_matches_the_golden_ticks() {
        let bars = toy_tsmom_bars();
        let decision = at(hour(4));
        let other = at(hour(5));
        let signal = Tsmom::<3>::new(BarInterval::H1, 0.05).must();
        let first = drive(&signal, &bars, decision).must();
        let again = drive(&signal, &bars, decision).must();
        assert_eq!(first, again);
        let _interleaved = drive(&signal, &bars, other).must();
        assert_eq!(drive(&signal, &bars, decision).must(), first);
        let fresh = Tsmom::<3>::new(BarInterval::H1, 0.05).must();
        assert_eq!(drive(&fresh, &bars, decision).must(), first);
        match first {
            Some(out) => assert_eq!(out.decision.target_ticks(), 49_385_225),
            None => support::fail_test("04:00 emits"),
        }

        let closes = [
            10_000_000_000,
            11_000_000_000,
            9_900_000_000,
            9_900_000_000,
            10_890_000_000,
            9_801_000_000,
        ];
        let vol_bars = toy_vol_bars(&closes);
        let vol = VolRegime::<2, 3>::new(BarInterval::H1).must();
        let feature = drive_feature(&vol, &vol_bars, at(hour(6))).must();
        assert_eq!(drive_feature(&vol, &vol_bars, at(hour(6))).must(), feature);
        let _other = drive_feature(&vol, &vol_bars, at(hour(5))).must();
        assert_eq!(drive_feature(&vol, &vol_bars, at(hour(6))).must(), feature);
        let fresh_vol = VolRegime::<2, 3>::new(BarInterval::H1).must();
        assert_eq!(
            drive_feature(&fresh_vol, &vol_bars, at(hour(6))).must(),
            feature
        );
        assert_eq!(feature.must_because("06:00 rank").value.ticks(), 83_333_333);

        let funding = toy_funding();
        let signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).must();
        let bars = vec![
            bar_ending_at(hour(25), BarInterval::H1, usdt_ticks(40_000)),
            bar_ending_at(hour(33), BarInterval::H1, usdt_ticks(40_000)),
        ];
        let first = drive_funding_z(&signal, &bars, &funding, at(hour(25))).must();
        let _later = drive_funding_z(&signal, &bars, &funding, at(hour(33))).must();
        assert_eq!(
            drive_funding_z(&signal, &bars, &funding, at(hour(25))).must(),
            first
        );
        let fresh = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).must();
        assert_eq!(
            drive_funding_z(&fresh, &bars, &funding, at(hour(25))).must(),
            first
        );
        assert_eq!(
            first.must_because("01:00 funding").decision.target_ticks(),
            -81_649_658
        );
    }
}
