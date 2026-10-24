//! Engine v1a goldens. Values a caller can read: fills, errors, counts, ratios.

#[path = "common/cases.rs"]
mod cases;
#[path = "support.rs"]
mod support;

use parangsae_backtest::{abs_notional_ticks, sharpe_ticks, target_qty, EngineError};
use parangsae_core::{Side, UtcMicros};
use parangsae_signals::TARGET_SCALE;
use parangsae_sim::Cash;
use proptest::prelude::*;
use support::{at, px, TICK};

use crate::cases::{
    before_sample, count_zero_print, e1, f1, f2, ff1, ff2, h2, moving_price, on, p7_base, p7_run,
    sample_open, stale, t1_flip, t1_repeat, t2_abort, t2_warmup, t3, t5, t5_flags, t7, z1, z2,
    z3_bars,
};

#[test]
fn moving_price_keeps_the_sized_quantity() {
    let report = moving_price();
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[0].qty.ticks(), 5_000_000);
    assert_eq!(report.fills[0].price, px(100_000));
    assert_eq!(report.fills[0].ts.micros(), at(13, 0));
    assert_eq!(report.n_refused_fills, 0);
    assert_eq!(report.signal_outputs.len(), 2);
}

#[test]
fn count_zero_defers_when_the_minute_is_not_flat() {
    let report = count_zero_print();
    assert_eq!(report.n_refused_fills, 1);
    assert_eq!(report.fills[0].ts.micros(), at(13, 1));
    assert_eq!(report.fills[0].price, px(111));
    assert_ne!(report.fills[0].ts.micros(), at(13, 0));
}

#[test]
fn t1_repeated_target_is_one_fill() {
    let report = t1_repeat();
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[0].qty.ticks(), 10_000_000);
    assert_eq!(report.fills[0].ts.micros(), at(13, 0));
    assert_eq!(report.fills[0].fee, Cash(0));
}

#[test]
fn t1_sign_flip_is_one_order() {
    let report = t1_flip();
    let mut times: Vec<i64> = report.fills.iter().map(|fill| fill.ts.micros()).collect();
    times.dedup();
    assert_eq!(times, vec![at(13, 0), at(14, 0)]);
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[0].qty.ticks(), 10_000_000);
    let sold: i64 = report
        .fills
        .iter()
        .skip(1)
        .map(|fill| fill.qty.ticks())
        .sum();
    assert_eq!(sold, 20_000_000);
    assert!(report
        .fills
        .iter()
        .skip(1)
        .all(|fill| fill.side == Side::Sell));
    assert!(report
        .fills
        .iter()
        .skip(1)
        .all(|fill| fill.ts.micros() == at(14, 0)));
}

#[test]
fn t2_none_after_warmup_is_missing_signal() {
    assert_eq!(
        t2_abort(),
        EngineError::MissingSignal(UtcMicros::from_micros(at(16, 0)))
    );
}

#[test]
fn t2_warmup_nones_are_counted() {
    let report = t2_warmup();
    assert_eq!(report.warmup_bars, 3);
    assert!(report.fills.is_empty());
    assert!(report.signal_outputs.is_empty());
    assert!(report.ruined.is_none());
}

#[test]
fn t3_fill_is_the_0100_open() {
    let report = t3();
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[0].price, px(100));
    assert_ne!(report.fills[0].price, px(101));
    assert_eq!(report.fills[0].ts.micros(), at(1, 0));
    assert_eq!(report.fills[0].fee, Cash(0));
    assert_eq!(report.dropped_orders, 1);
}

#[test]
fn t4_sizing_truncates_toward_zero() {
    let equity = 10_000 * TICK;
    let mark = 50_000 * TICK;
    assert_eq!(
        target_qty(TARGET_SCALE, equity, mark, 100_000).unwrap(),
        20_000_000
    );
    assert_eq!(target_qty(33_333_333, equity, mark, 1).unwrap(), 6_666_666);
    assert_eq!(
        target_qty(33_333_333, equity, mark, 100_000).unwrap(),
        6_600_000
    );
    assert_eq!(target_qty(0, equity, mark, 1).unwrap(), 0);
    assert_eq!(target_qty(TARGET_SCALE, 0, mark, 1).unwrap(), 0);
    assert_eq!(
        target_qty(-TARGET_SCALE, equity, mark, 100_000).unwrap(),
        -20_000_000
    );
}

#[test]
fn t5_funding_hidden_until_slot_plus_60s() {
    let (hidden, visible) = t5_flags();
    assert!(hidden);
    assert!(visible);
    let (report, script) = t5();
    assert!(script.funding_seen[0].is_empty());
    assert!(script.funding_seen[1].is_empty());
    assert_eq!(script.funding_seen[2].len(), 1);
    assert_eq!(script.funding_seen[2][0].0, at(8, 0));
    assert_eq!(report.funding.len(), 1);
    assert_eq!(report.funding[0].ts.micros(), at(8, 0));
    assert_eq!(report.funding[0].position.0, TICK);
    assert_eq!(report.funding[0].amount, Cash(-1_000_000));
    assert_eq!(report.funding[0].mark, px(100));
    assert_eq!(report.fills[0].ts.micros(), at(7, 0));
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[0].qty.ticks(), TICK);
    assert!(report
        .fills
        .iter()
        .any(|fill| fill.side == Side::Sell && fill.ts.micros() == at(8, 0)));
}

#[test]
fn t7_listed_two_minute_gap_passes() {
    let report = t7(2, 3, true, true).expect("two minute gap");
    assert!(report.ruined.is_none());
    assert_eq!(report.warmup_bars, 1);
}

#[test]
fn t7_listed_six_minute_gap_aborts() {
    let err = t7(2, 7, true, true).expect_err("six minute gap");
    assert!(matches!(err, EngineError::MissingMark(_)));
}

#[test]
fn t7_unlisted_gap_aborts() {
    let err = t7(2, 3, false, true).expect_err("unlisted gap");
    assert!(matches!(err, EngineError::MissingMark(_)));
}

#[test]
fn t8_report_turnover_matches_the_spec_formula() {
    let report = t1_repeat();
    let mut notional = 0i128;
    for fill in &report.fills {
        notional += (i128::from(fill.price.ticks()) * i128::from(fill.qty.ticks())).abs()
            / i128::from(TICK);
    }
    let equity: i128 = report
        .daily_equity
        .iter()
        .map(|(_, cash)| i128::from(cash.0))
        .sum();
    let expected = i64::try_from(notional * 365 * i128::from(TICK) / equity).unwrap();
    assert_eq!(report.metrics.turnover.ticks(), expected);
    assert!(expected > 0);
}

#[test]
fn f1_flat_reasons_are_counted() {
    let report = f1();
    assert_eq!(report.flat_funding_stale, 3);
    assert_eq!(report.flat_funding_slot_missing, 2);
    assert!(report.fills.is_empty());
}

#[test]
fn f2_flat_matches_target_zero() {
    let flat = f2(stale());
    let zero = f2(cases::tgt(0.0));
    let signature = |report: &parangsae_backtest::Report| {
        report
            .fills
            .iter()
            .map(|fill| (fill.ts, fill.side, fill.qty, fill.price))
            .collect::<Vec<_>>()
    };
    assert_eq!(signature(&flat), signature(&zero));
    assert_eq!(flat.fills.len(), 2);
    assert_eq!(flat.fills[0].side, Side::Buy);
    assert_eq!(flat.fills[1].side, Side::Sell);
}

#[test]
fn z1_filler_fill_moves_to_the_next_open() {
    let report = z1();
    assert_eq!(report.n_refused_fills, 1);
    assert_eq!(report.fills[0].ts.micros(), at(13, 1));
    assert_eq!(report.fills[0].price, px(111));
    assert_ne!(report.fills[0].ts.micros(), at(13, 0));
}

#[test]
fn z2_two_fillers_fill_on_the_third() {
    let report = z2();
    assert_eq!(report.n_refused_fills, 1);
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].ts.micros(), at(13, 2));
    assert_eq!(report.fills[0].price, px(111));
}

#[test]
fn ff1_window_flattens_an_open_position() {
    let report = ff1();
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[0].ts.micros(), at(13, 0));
    assert_eq!(report.fills[1].side, Side::Sell);
    assert_eq!(report.fills[1].ts.micros(), at(14, 0));
    assert_eq!(report.fills[0].qty, report.fills[1].qty);
    assert!(report.trades.iter().all(|trade| trade.close_ts.is_some()));
    assert_eq!(report.forced_flat.len(), 1);
}

#[test]
fn ff2_targets_inside_the_window_do_not_fill() {
    let report = ff2();
    assert!(report
        .fills
        .iter()
        .all(|fill| fill.ts.micros() >= at(15, 0)));
    assert_eq!(report.fills[0].ts.micros(), at(15, 0));
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills.len(), 1);
}

#[test]
fn e1_sample_end_flattens_at_2359() {
    let report = e1();
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[0].ts.micros(), on("2025-09-30", 23, 0));
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[0].qty.ticks(), TICK);
    assert_eq!(report.fills[1].ts.micros(), on("2025-09-30", 23, 59));
    assert_eq!(report.fills[1].side, Side::Sell);
    assert_eq!(report.fills[1].qty.ticks(), TICK);
    assert!(report.trades.iter().all(|trade| trade.close_ts.is_some()));
}

#[test]
fn e1_zero_count_at_fixed_2359_refuses_the_whole_run() {
    let start = on("2025-09-30", 22, 0);
    let mut bars = cases::hours(start, 2, 100);
    let last = bars.len() - 1;
    assert_eq!(bars[last].open_time.micros(), on("2025-09-30", 23, 59));
    bars[last].count = 0;
    let error = cases::execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![Some(cases::tgt(1.0)), Some(cases::stale())],
        100,
        Vec::new(),
        Vec::new(),
    )
    .expect_err("R3 cannot substitute an earlier open for the fixed terminal");
    assert!(
        matches!(error, EngineError::UnfilledBoundaryClose { boundary }
        if boundary.micros() == on("2025-09-30", 23, 59))
    );
}

#[test]
fn h2_holdout_bar_is_refused() {
    assert!(matches!(h2(), EngineError::HoldoutBar(_)));
}

#[test]
fn before_sample_bar_is_refused() {
    assert!(matches!(before_sample(), EngineError::BeforeSample(_)));
}

#[test]
fn in_sample_open_is_tradable() {
    let report = sample_open();
    assert_eq!(report.warmup_bars, 1);
    assert!(report.fills.is_empty());
    assert!(report.ruined.is_none());
}

#[test]
fn p7_later_hour_does_not_change_the_first_view() {
    let (bars, funding) = p7_base();
    let first = p7_run(bars.clone(), funding.clone());
    let mut mutated_bars = bars;
    let mut mutated_funding = funding;
    mutated_bars[119].high = px(123);
    mutated_bars[119].close = px(123);
    mutated_funding[0].rate = parangsae_core::Ratio::from_ticks(9_999);
    let second = p7_run(mutated_bars, mutated_funding);
    assert_eq!(first.bars_seen[0], second.bars_seen[0]);
    assert_eq!(first.funding_seen[0], second.funding_seen[0]);
    assert!(first.funding_seen[0].is_empty());
    assert_ne!(first.funding_seen[1], second.funding_seen[1]);
    assert_eq!(second.funding_seen[1].len(), 1);
    assert_ne!(first.bars_seen[1], second.bars_seen[1]);
}

#[test]
fn g9_levels_match_the_integer_golden() {
    let tick = 100_000_000i64;
    let (daily, annual) = sharpe_ticks(&[100 * tick, 110 * tick, 121 * tick, 121 * tick]).unwrap();
    assert_eq!(daily.unwrap().ticks(), 115_470_053);
    assert_eq!(annual.unwrap().ticks(), 2_206_052_281);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn z3_no_fill_uses_a_filler_open(mask in prop::collection::vec(any::<bool>(), 120)) {
        let (report, bars) = z3_bars(&mask);
        let fillers: Vec<UtcMicros> = bars
            .iter()
            .filter(|bar| bar.is_filler())
            .map(|bar| bar.open_time)
            .collect();
        for fill in &report.fills {
            prop_assert!(!fillers.contains(&fill.ts));
        }
    }

    #[test]
    fn p9_sized_notional_does_not_exceed_equity(
        target in -100_000_000i64..=100_000_000,
        equity in 1i64..=10_000_000_000_000i64,
        mark in 1i64..=10_000_000_000_000i64,
        lot in 1i64..=10_000_000i64,
    ) {
        if let Ok(qty) = target_qty(target, equity, mark, lot) {
            let notional = abs_notional_ticks(qty, mark).unwrap();
            prop_assert!(notional <= i128::from(equity));
        }
    }
}

#[test]
fn flat_target_cancels_an_unfilled_order_after_a_filler_hour() {
    for flat in [cases::stale(), cases::tgt(0.0)] {
        let report =
            cases::filler_hour_targets(vec![Some(cases::tgt(1.0)), Some(flat), Some(flat)], 1);
        assert!(report.fills.is_empty(), "{:?}", report.fills);
        assert_eq!(report.n_refused_fills, 1);
        assert_eq!(report.dropped_orders, 0);
    }
}

#[test]
fn unchanged_target_keeps_one_refusal_across_filler_hours() {
    let report = cases::filler_hour_targets(vec![Some(cases::tgt(1.0)); 4], 2);
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].ts.micros(), at(15, 0));
    assert_eq!(report.n_refused_fills, 1);
}

#[test]
fn changed_target_replaces_the_order_after_a_filler_hour() {
    let report = cases::filler_hour_targets(
        vec![
            Some(cases::tgt(1.0)),
            Some(cases::tgt(-1.0)),
            Some(cases::tgt(-1.0)),
        ],
        1,
    );
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].side, Side::Sell);
    assert_eq!(report.fills[0].ts.micros(), at(14, 0));
    assert_eq!(report.n_refused_fills, 1);
}

#[test]
fn delayed_interior_minute_never_reaches_the_signal() {
    assert!(matches!(
        cases::delayed_interior_minute(),
        EngineError::Resample(parangsae_core::ResampleError::Available { .. })
    ));
}

#[test]
fn delayed_funding_does_not_leak_when_a_later_slot_is_available() {
    let first = cases::delayed_funding_prefix(1_000);
    let mutated = cases::delayed_funding_prefix(9_999);
    assert_eq!(first.funding_seen, mutated.funding_seen);
    assert!(first.funding_seen.iter().all(Vec::is_empty));
}

#[test]
fn qr4_quantity_uses_executed_fill_price() {
    let report = cases::moving_price();
    assert_eq!(report.fills[0].qty.ticks(), 5_000_000);
}

#[test]
fn qr3_delayed_decision_cannot_fill_the_boundary_open() {
    let report = cases::delayed_target_report();
    assert_eq!(report.fills[0].ts.micros(), at(13, 1));
    assert_eq!(
        report.signal_outputs[0].decision_ts.micros().micros(),
        at(13, 0) + 1_000
    );
}

#[test]
fn qr4_frozen_decision_equity_is_divided_by_the_slipped_fill_quote() {
    let bars = cases::hours(at(12, 0), 2, 100);
    let tape = support::tape_of(bars.clone());
    let mut signal = cases::Script::new(vec![Some(cases::tgt(1.0)); 2]);
    let report = parangsae_backtest::run_targets(
        &tape,
        &mut signal,
        &support::costs(0, 2500),
        &support::cfg(&bars, 10_000),
    )
    .unwrap();
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].price, px(125));
    assert_eq!(report.fills[0].qty.ticks(), 8_000_000_000);
}

#[test]
fn gate4_repeated_pending_decisions_count_one_realized_rebalance() {
    let report = cases::filler_hour_targets(vec![Some(cases::tgt(0.5)); 4], 2);
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].ts.micros(), at(15, 0));
    assert_eq!(report.n_rebalances, 1);
    assert_eq!(report.n_sign_changes, 0);
}

#[test]
fn gate4_a_canceled_pending_flip_is_not_a_held_position_sign_change() {
    let report = cases::canceled_pending_flip();
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.n_sign_changes, 0);
    assert_eq!(report.n_rebalances, 1);
}

#[test]
fn gate4_same_lot_reacceptance_does_not_count_without_a_fill() {
    let report = cases::same_lot_reaccept();
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.n_rebalances, 1);
    assert_eq!(report.n_sign_changes, 0);
}

#[test]
fn gate4_a_realized_flip_counts_once_across_its_two_fill_legs() {
    let report = cases::t1_flip();
    assert_eq!(report.fills.len(), 3);
    assert_eq!(report.n_rebalances, 2);
    assert_eq!(report.n_sign_changes, 1);
    assert_eq!(report.n_forced_exits, 0);
}

#[test]
fn gate4_a_realized_forced_close_does_not_count_as_a_strategy_rebalance() {
    let bars = cases::hours(at(12, 0), 3, 100);
    let mut cfg = support::cfg(&bars, 10_000);
    cfg.flatten_before = Some(UtcMicros::from_micros(at(14, 0)));
    let mut signal = cases::Script::new(vec![Some(cases::tgt(0.5)); 3]);
    let report = parangsae_backtest::run_targets(
        &support::tape_of(bars),
        &mut signal,
        &support::costs(0, 0),
        &cfg,
    )
    .unwrap();
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.n_rebalances, 1);
    assert_eq!(report.n_sign_changes, 0);
    assert_eq!(report.n_forced_exits, 1);
}
