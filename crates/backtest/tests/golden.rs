//! Hand-computed goldens G1–G12. Slippage is zero. Initial cash is 1_000 USDT
//! unless a case says otherwise. Prices below are whole USDT; cash is 1e8 ticks.

mod support;

use parangsae_backtest::fixture::{FlipEveryN, Scripted};
use parangsae_backtest::{max_drawdown, run, run_fee_grid, sharpe_f64, EngineError, FEE_GRID_BPS};
use parangsae_core::UtcMicros;
use parangsae_sim::{Cash, Side};
use support::{
    at, bar_at, btc, cfg, costs, g1, g10_gap, g10_missing_funding, g10_missing_mark, g11, g12, g2,
    g3, g4_flat, g4_short_pays, g4_short_receives, g5, g6, g7, g8, px, series, tape_of, usd, TICK,
};

fn show<T, E: std::fmt::Display>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|err| err.to_string())
}

fn must_err<T: std::fmt::Debug, E>(result: Result<T, E>) -> Result<E, String> {
    match result {
        Err(err) => Ok(err),
        Ok(value) => Err(format!("expected an error, got {value:?}")),
    }
}

#[test]
fn g1_round_trip_five_bps() -> Result<(), String> {
    let report = g1()?;
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[0].price, px(100)?);
    assert_eq!(report.fills[0].fee, Cash(5_000_000));
    assert_eq!(report.fills[1].side, Side::Sell);
    assert_eq!(report.fills[1].price, px(110)?);
    assert_eq!(report.fills[1].fee, Cash(5_500_000));
    assert_eq!(report.trades.len(), 1);
    assert!(report.trades[0].close_ts.is_some());
    assert_eq!(report.trades[0].pnl, Cash(989_500_000));
    let end = report.equity_end().ok_or("ending equity")?;
    assert_eq!(end, Cash(100_989_500_000));
    assert_eq!(report.dropped_orders, 0);

    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    report
        .write_dir(dir.path())
        .map_err(|err| err.to_string())?;
    let metrics =
        std::fs::read_to_string(dir.path().join("metrics.json")).map_err(|err| err.to_string())?;
    assert!(metrics.contains("\"fees_paid_ticks\": \"10500000\""));
    assert!(metrics.contains("\"taker_fee_bps\": \"5\""));
    assert!(metrics.contains("\"slippage_bps\": \"0\""));
    assert!(metrics.contains("\"bars\": \"3\""));
    assert!(metrics.contains("\"scale\": \"100000000\""));
    assert!(metrics.contains("\"dropped_orders\": \"0\""));
    let equity =
        std::fs::read_to_string(dir.path().join("equity.csv")).map_err(|err| err.to_string())?;
    assert!(equity.contains("100989500000"));
    assert!(!metrics.contains("0.05"));
    Ok(())
}

#[test]
fn g2_fill_uses_next_bar_open() -> Result<(), String> {
    let report = g2()?;
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].price, px(100)?);
    assert_ne!(report.fills[0].price, px(105)?);
    Ok(())
}

#[test]
fn g3_funding_long_pays_positive_rate() -> Result<(), String> {
    let report = g3()?;
    assert_eq!(report.funding.len(), 1);
    assert_eq!(report.funding[0].position.0, TICK);
    assert_eq!(report.funding[0].amount, Cash(-1_000_000));
    assert_eq!(report.funding[0].mark, px(100)?);
    Ok(())
}

#[test]
fn g4_funding_short_pays_receives_and_flat_is_zero() -> Result<(), String> {
    let pays = g4_short_pays()?;
    assert_eq!(pays.funding.len(), 1);
    assert_eq!(pays.funding[0].position.0, -TICK);
    assert_eq!(pays.funding[0].amount, Cash(-1_000_000));

    let receives = g4_short_receives()?;
    assert_eq!(receives.funding[0].position.0, -TICK);
    assert_eq!(receives.funding[0].amount, Cash(1_000_000));

    let flat = g4_flat()?;
    assert_eq!(flat.funding.len(), 1);
    assert_eq!(flat.funding[0].position.0, 0);
    assert_eq!(flat.funding[0].amount, Cash(0));
    Ok(())
}

#[test]
fn g5_funding_uses_pre_fill_position() -> Result<(), String> {
    let report = g5()?;
    assert_eq!(report.funding.len(), 1);
    assert_eq!(
        report.funding[0].position.0, TICK,
        "funding still sees the long"
    );
    assert_eq!(report.funding[0].amount, Cash(-1_000_000));
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[1].side, Side::Sell);
    assert!(report.trades.iter().any(|trade| trade.close_ts.is_some()));
    assert!(report.trades.iter().all(|trade| trade.close_ts.is_some()));
    Ok(())
}

#[test]
fn g6_fee_rounds_against_the_trader() -> Result<(), String> {
    let report = g6()?;
    assert_eq!(report.fills.len(), 1);
    assert_eq!(report.fills[0].qty.ticks(), 1);
    assert_eq!(report.fills[0].price, px(100)?);
    // Exact fee is 0.05 cash ticks. The trader pays 1 tick.
    assert_eq!(report.fills[0].fee, Cash(1));
    Ok(())
}

#[test]
fn g7_flip_splits_the_fill_into_two_legs() -> Result<(), String> {
    let report = g7()?;
    let closed: Vec<_> = report
        .trades
        .iter()
        .filter(|trade| trade.close_ts.is_some())
        .collect();
    let open: Vec<_> = report
        .trades
        .iter()
        .filter(|trade| trade.close_ts.is_none())
        .collect();
    assert_eq!(closed.len(), 1);
    assert_eq!(open.len(), 1);
    assert_eq!(closed[0].side, Side::Buy);
    assert_eq!(closed[0].qty, btc(1)?);
    // 5 USDT of price minus the open fee 0.05 and the close-leg fee 0.0525.
    assert_eq!(closed[0].pnl, Cash(489_750_000));
    assert_eq!(open[0].side, Side::Sell);
    assert_eq!(open[0].qty, btc(1)?);
    assert_eq!(report.fills.len(), 3);
    Ok(())
}

#[test]
fn g8_fee_grid_on_the_round_trip() -> Result<(), String> {
    let grid = g8()?;
    let mut ends = Vec::with_capacity(grid.len());
    for (_, report) in &grid {
        ends.push(report.equity_end().ok_or("fee-grid equity")?.0);
    }
    assert_eq!(
        ends,
        vec![
            100_995_800_000,
            100_991_600_000,
            100_989_500_000,
            100_987_400_000,
        ]
    );
    assert_eq!(
        grid.iter().map(|(bps, _)| bps.0).collect::<Vec<_>>(),
        FEE_GRID_BPS
    );
    for pair in ends.windows(2) {
        assert!(pair[0] >= pair[1]);
    }
    Ok(())
}

#[test]
#[allow(
    clippy::float_arithmetic,
    reason = "the golden compares the one documented Sharpe float to its printed digits"
)]
fn g9_sharpe_and_max_drawdown() -> Result<(), String> {
    let levels = [100 * TICK, 110 * TICK, 121 * TICK, 121 * TICK];
    let (daily, annual) = show(sharpe_f64(&levels))?;
    let daily = daily.ok_or("three returns have a sharpe")?;
    let annual = annual.ok_or("annualized sharpe")?;
    // mean/std of 0.1, 0.1, 0 with ddof 1.
    let expected_daily = 1.1547005383792515_f64;
    assert!((daily - expected_daily).abs() < 1e-9);
    assert!((annual - expected_daily * 365f64.sqrt()).abs() < 1e-6);

    let curve = [usd(100)?, usd(110)?, usd(99)?, usd(121)?];
    assert_eq!(show(max_drawdown(&curve))?.ticks(), 10_000_000);

    let (none_daily, none_annual) = show(sharpe_f64(&[100, 100, 100]))?;
    assert!(none_daily.is_none());
    assert!(none_annual.is_none());
    let (one_day, _) = show(sharpe_f64(&[100 * TICK, 110 * TICK]))?;
    assert!(one_day.is_none());
    Ok(())
}

#[test]
fn g10_gap_missing_mark_and_missing_funding() -> Result<(), String> {
    let start = at(12, 0)?;
    let err = must_err(g10_gap())?;
    assert_eq!(
        err,
        EngineError::Gap {
            expected: UtcMicros::from_micros(start + 120_000_000),
            got: UtcMicros::from_micros(start + 180_000_000),
        }
    );

    let funding_open = at(16, 0)?;
    let err = must_err(g10_missing_mark())?;
    assert_eq!(
        err,
        EngineError::MissingMark(UtcMicros::from_micros(funding_open))
    );

    let err = must_err(g10_missing_funding())?;
    assert_eq!(
        err,
        EngineError::MissingFunding(UtcMicros::from_micros(at(16, 0)?))
    );
    Ok(())
}

#[test]
fn g11_ruin_freezes_equity_and_stops_fills() -> Result<(), String> {
    let bars = series(at(12, 0)?, &[(100, 100), (100, 100), (50, 50), (40, 40)])?;
    let report = g11()?;
    let ruined = report.ruined.ok_or("ruin is reported")?;
    assert_eq!(ruined, bars[2].close_time);
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[1].side, Side::Sell);
    assert_eq!(report.fills[1].price, px(50)?);
    assert_eq!(report.fills[1].fee, Cash(2_500_000));
    assert!(report.fills.iter().all(|fill| fill.ts <= ruined));
    assert_eq!(report.equity_curve[2].1, Cash(-7_500_000));
    assert_eq!(report.equity_curve[2].1, report.equity_curve[3].1);
    assert_eq!(report.equity_curve.len(), 4);
    Ok(())
}

#[test]
fn g12_last_bar_order_is_dropped() -> Result<(), String> {
    let report = g12()?;
    assert_eq!(report.dropped_orders, 1);
    assert_eq!(report.fills.len(), 2);
    assert!(report.ruined.is_none());
    Ok(())
}

#[test]
fn fee_grid_equity_does_not_rise_with_bps() -> Result<(), String> {
    let bars = series(at(12, 0)?, &[(100, 100); 6])?;
    let tape = tape_of(bars.clone());
    let qty = btc(1)?;
    let window = cfg(&bars, 1_000_000)?;
    let grid = show(run_fee_grid(
        &tape,
        || FlipEveryN::new(1, qty),
        &costs(5, 0),
        &window,
    ))?;
    let mut ends = Vec::with_capacity(grid.len());
    for (_, report) in &grid {
        ends.push(report.equity_end().ok_or("fee-grid equity")?.0);
    }
    for pair in ends.windows(2) {
        assert!(pair[0] >= pair[1], "equity rose as the fee rose: {ends:?}");
    }
    Ok(())
}

#[test]
fn unsorted_bars_are_rejected() -> Result<(), String> {
    let start = at(12, 0)?;
    let price = px(100)?;
    let bars = vec![
        bar_at(start + 60_000_000, price, price)?,
        bar_at(start, price, price)?,
    ];
    let tape = tape_of(bars.clone());
    let mut strategy = Scripted::new(Vec::new());
    let err = must_err(run(&tape, &mut strategy, &costs(5, 0), &cfg(&bars, 1000)?))?;
    assert_eq!(err, EngineError::Unsorted);
    Ok(())
}
