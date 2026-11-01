mod support;
use parangsae_core::BarInterval;
use parangsae_strategies::Strategy;
use support::{ohlc as bar, params, usdt};
#[test]
fn rejected_zero_price_does_not_consume_the_correctable_bar() {
    for (family, p) in [
        (
            "S05",
            params(&[("l", 24), ("k", 100_000_000), ("hold_h", 4)]),
        ),
        (
            "S01",
            params(&[("interval", 60), ("n", 24), ("c", 100_000_000)]),
        ),
    ] {
        let mut repaired = Strategy::from_params(family, &p).unwrap();
        let mut fresh = Strategy::from_params(family, &p).unwrap();
        // TSMOM only computes a return once its window is full; poison that row.
        let prefix = if family == "S01" { 24 } else { 0 };
        for i in 0..prefix {
            let valid = bar(
                i,
                BarInterval::H1,
                usdt(100),
                usdt(100),
                usdt(100),
                usdt(100),
            );
            assert_eq!(repaired.push(&valid, None), fresh.push(&valid, None));
        }
        let invalid = bar(prefix, BarInterval::H1, 0, 0, 0, 0);
        assert!(
            repaired.push(&invalid, None).is_err(),
            "{family} must refuse zero"
        );
        for i in prefix..prefix + 50 {
            let valid = bar(
                i,
                BarInterval::H1,
                usdt(100 + i),
                usdt(100 + i),
                usdt(100 + i),
                usdt(100 + i),
            );
            assert_eq!(
                repaired.push(&valid, None),
                fresh.push(&valid, None),
                "{family} corrected row {i}"
            );
        }
    }
}

#[test]
fn invalid_book_does_not_consume_a_valid_signal_bar() {
    let p = params(&[("interval", 60), ("n", 24), ("c", 100_000_000)]);
    let mut repaired = Strategy::from_params("S01", &p).unwrap();
    let mut fresh = Strategy::from_params("S01", &p).unwrap();
    for i in 0..24 {
        let valid = bar(
            i,
            BarInterval::H1,
            usdt(100 + i),
            usdt(100 + i),
            usdt(100 + i),
            usdt(100 + i),
        );
        assert_eq!(repaired.push(&valid, None), fresh.push(&valid, None));
    }
    let valid = bar(
        24,
        BarInterval::H1,
        usdt(124),
        usdt(124),
        usdt(124),
        usdt(124),
    );
    let invalid = parangsae_strategies::DecisionBook {
        equity_ticks: 0,
        mark_ticks: usdt(124),
        position_lots: 0,
        execution: parangsae_strategies::ExecutionPhase::Flat,
    };
    assert!(repaired.push(&valid, Some(invalid)).is_err());
    for i in 24..30 {
        let valid = bar(
            i,
            BarInterval::H1,
            usdt(100 + i),
            usdt(100 + i),
            usdt(100 + i),
            usdt(100 + i),
        );
        assert_eq!(repaired.push(&valid, None), fresh.push(&valid, None));
    }
}

#[test]
fn h1_and_h2_refuse_bad_books_before_consuming_an_active_bar() {
    use parangsae_strategies::{DecisionBook, ExecutionPhase, StrategyError};
    for (family, parameters, prefix) in [
        (
            "H1",
            params(&[
                ("interval", 60),
                ("n", 24),
                ("c", 100_000_000),
                ("deadband", 1),
                ("decision_hour_utc", 0),
            ]),
            47,
        ),
        ("H2", params(&[("n_members", 12), ("deadband", 1)]), 727),
    ] {
        let mut repaired = Strategy::from_params(family, &parameters).expect("family");
        let mut fresh = Strategy::from_params(family, &parameters).expect("family");
        for i in 0..prefix {
            let p = usdt(100 + i);
            let valid = bar(i, BarInterval::H1, p, p, p, p);
            assert_eq!(repaired.push(&valid, None), fresh.push(&valid, None));
        }
        let p = usdt(100 + prefix);
        let candidate = bar(prefix, BarInterval::H1, p, p, p, p);
        for (equity_ticks, mark_ticks, position_lots, expected) in [
            (0, p, 0, StrategyError::Price),
            (usdt(1000), 0, 0, StrategyError::Price),
            (1, i64::MAX, i64::MAX, StrategyError::Overflow),
        ] {
            let bad = DecisionBook {
                equity_ticks,
                mark_ticks,
                position_lots,
                execution: ExecutionPhase::Flat,
            };
            assert_eq!(
                repaired.push(&candidate, Some(bad)),
                Err(expected),
                "{family} invalid book"
            );
        }
        let mut outputs = 0;
        for i in prefix..prefix + 30 {
            let p = usdt(100 + i);
            let valid = bar(i, BarInterval::H1, p, p, p, p);
            let book = DecisionBook {
                equity_ticks: usdt(1000),
                mark_ticks: p,
                position_lots: 0,
                execution: ExecutionPhase::Flat,
            };
            let actual = repaired
                .push(&valid, Some(book))
                .expect("corrected bar accepted");
            let control = fresh
                .push(&valid, Some(book))
                .expect("clean control accepted");
            assert_eq!(actual, control, "{family} corrected row {i}");
            outputs += usize::from(actual.is_some());
        }
        assert!(
            outputs > 0,
            "{family} comparison must observe an active output"
        );
    }
}

#[test]
fn h3_zero_price_rejection_preserves_corrected_channel_and_suffix() {
    use parangsae_strategies::StrategyError;
    let parameters = params(&[("n", 24), ("er_l", 24), ("er_q", 50_000_000), ("hold_h", 4)]);
    let mut repaired = Strategy::from_params("H3", &parameters).expect("H3");
    let mut fresh = Strategy::from_params("H3", &parameters).expect("H3");
    for i in 0..24 {
        let p = usdt(100 + i);
        let valid = bar(i, BarInterval::H1, p, p, p, p);
        assert_eq!(repaired.push(&valid, None), fresh.push(&valid, None));
    }
    assert_eq!(
        repaired.push(&bar(24, BarInterval::H1, 0, 0, 0, 0), None),
        Err(StrategyError::Price)
    );
    let mut outputs = 0;
    for i in 24..74 {
        let p = usdt(100 + i);
        let valid = bar(i, BarInterval::H1, p, p, p, p);
        let actual = repaired.push(&valid, None).expect("corrected bar accepted");
        let control = fresh.push(&valid, None).expect("clean control accepted");
        assert_eq!(actual, control, "H3 corrected row {i}");
        outputs += usize::from(actual.is_some());
    }
    assert!(
        outputs > 0,
        "comparison observes H3 outputs, not just missing warmup"
    );
}

#[test]
fn inexact_high_is_refused_before_all_families_consume_the_key() {
    use parangsae_strategies::StrategyError;
    for (family, parameters, prefix) in [
        (
            "S01",
            params(&[("interval", 60), ("n", 24), ("c", 100_000_000)]),
            24,
        ),
        (
            "S05",
            params(&[("l", 24), ("k", 100_000_000), ("hold_h", 4)]),
            720,
        ),
        (
            "H1",
            params(&[
                ("interval", 60),
                ("n", 24),
                ("c", 100_000_000),
                ("deadband", 1),
                ("decision_hour_utc", 0),
            ]),
            47,
        ),
        ("H2", params(&[("n_members", 12), ("deadband", 1)]), 727),
        (
            "H3",
            params(&[("n", 24), ("er_l", 24), ("er_q", 50_000_000), ("hold_h", 4)]),
            24,
        ),
    ] {
        let mut repaired = Strategy::from_params(family, &parameters).expect("family");
        let mut fresh = Strategy::from_params(family, &parameters).expect("family");
        for i in 0..prefix {
            let p = usdt(100 + i);
            let valid = bar(i, BarInterval::H1, p, p, p, p);
            assert_eq!(repaired.push(&valid, None), fresh.push(&valid, None));
        }
        let p = usdt(100 + prefix);
        // ResampledBar allows this coherent high, but the shared exact-float
        // price domain must refuse it without claiming the timestamp.
        let inexact = bar(prefix, BarInterval::H1, p, (1_i64 << 53) + 1, p, p);
        assert_eq!(
            repaired.push(&inexact, None),
            Err(StrategyError::Price),
            "{family} inexact high"
        );
        let mut outputs = 0;
        for i in prefix..prefix + 30 {
            let p = usdt(100 + i);
            let valid = bar(i, BarInterval::H1, p, p, p, p);
            let actual = repaired.push(&valid, None).expect("corrected bar accepted");
            let control = fresh.push(&valid, None).expect("clean control accepted");
            assert_eq!(actual, control, "{family} corrected row {i}");
            outputs += usize::from(actual.is_some());
        }
        assert!(outputs > 0, "{family} comparison must observe outputs");
    }
}

#[test]
fn refused_observe_does_not_poison_the_following_push() {
    let p = params(&[("l", 24), ("k", 100_000_000), ("hold_h", 4)]);
    let mut repaired = Strategy::from_params("S05", &p).unwrap();
    let mut fresh = Strategy::from_params("S05", &p).unwrap();
    let zero = bar(0, BarInterval::H1, 0, 0, 0, 0);
    assert!(repaired.observe(&zero).is_err());
    for i in 0..50 {
        let px = usdt(100 + i);
        let valid = bar(i, BarInterval::H1, px, px, px, px);
        assert_eq!(repaired.push(&valid, None), fresh.push(&valid, None));
    }
}
