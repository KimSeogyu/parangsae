//! Prefix replay matches the full replay. A later bar does not change earlier outputs.

mod support;

use std::collections::BTreeMap;

use parangsae_core::BarInterval;
use parangsae_strategies::Strategy;
use proptest::prelude::*;
use support::{assert_prefix, params, walk, SCALE};

fn check(
    family: &str,
    spec: &BTreeMap<String, i64>,
    interval: BarInterval,
    count: usize,
    seed: u64,
) {
    let bars = walk(interval, count, seed);
    assert_prefix(family, spec, &bars);
    let _ = Strategy::from_params(family, spec).expect("params");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn s01(seed in 1u64..50_000) {
        let spec = params(&[("c", SCALE), ("interval", 60), ("n", 24)]);
        check("S01", &spec, BarInterval::H1, 40, seed);
    }

    #[test]
    fn s02(seed in 1u64..50_000) {
        let spec = params(&[("n", 4), ("hold_h", 3)]);
        check("S02", &spec, BarInterval::H1, 48, seed);
    }

    #[test]
    fn s06(seed in 1u64..50_000) {
        let spec = params(&[("w_days", 2), ("k", SCALE)]);
        check("S06", &spec, BarInterval::H4, 36, seed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    #[test]
    fn s03(seed in 1u64..20_000) {
        let spec = params(&[("theta", 20_000_000), ("n", 24), ("hold_h", 4)]);
        check("S03", &spec, BarInterval::H1, 210, seed);
    }

    #[test]
    fn s05(seed in 1u64..20_000) {
        let spec = params(&[("l", 4), ("k", 200_000_000), ("hold_h", 4)]);
        check("S05", &spec, BarInterval::H1, 730, seed);
    }
}
