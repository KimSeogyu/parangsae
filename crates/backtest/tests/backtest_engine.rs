//! Cucumber runner for the twelve engine goldens.

#[path = "common/cases.rs"]
mod cases;
#[path = "support.rs"]
mod support;

use std::path::PathBuf;

use cases::{t3, t5, t5_flags, t7};
use cucumber::{then, when, World as _};
use parangsae_backtest::fixture::BuyAndHold;
use parangsae_backtest::{run, EngineError, Report, FEE_GRID_BPS};
use parangsae_core::{Side, UtcMicros};
use parangsae_sim::Cash;
use support::{
    at, btc, costs, g1, g10_gap, g10_missing_funding, g10_missing_mark, g11, g12, g2, g3, g4_flat,
    g4_short_pays, g4_short_receives, g5, g6, g7, g8, g9_book, px,
};

#[derive(Debug, Default, cucumber::World)]
struct EngineWorld {
    report: Option<Report>,
    error: Option<EngineError>,
    sharpe_daily: Option<i64>,
    sharpe_annual: Option<i64>,
    drawdown: Option<i64>,
    grid: Vec<i64>,
    funding_hidden: bool,
    funding_visible: bool,
    funding_scripts: Option<(cases::Script, cases::Script)>,
    probe_result: Option<Result<(), String>>,
}

impl EngineWorld {
    fn report(&self) -> &Report {
        self.report.as_ref().expect("a report was produced")
    }
}

fn keep(world: &mut EngineWorld, report: Report) {
    world.error = None;
    world.report = Some(report);
}

#[when("the G1 round trip is run")]
fn g1_step(world: &mut EngineWorld) {
    keep(world, g1());
}

#[when("the G2 next-open buy is run")]
fn g2_step(world: &mut EngineWorld) {
    keep(world, g2());
}

#[when("the G3 funding hold is run")]
fn g3_step(world: &mut EngineWorld) {
    keep(world, g3());
}

#[when("the G4 short pays case is run")]
fn g4_pays_step(world: &mut EngineWorld) {
    keep(world, g4_short_pays());
}

#[when("the G4 short receives case is run")]
fn g4_receives_step(world: &mut EngineWorld) {
    keep(world, g4_short_receives());
}

#[when("the G4 flat case is run")]
fn g4_flat_step(world: &mut EngineWorld) {
    keep(world, g4_flat());
}

#[when("the G5 funding and flatten is run")]
fn g5_step(world: &mut EngineWorld) {
    keep(world, g5());
}

#[when("the G6 one-lot buy is run")]
fn g6_step(world: &mut EngineWorld) {
    keep(world, g6());
}

#[when("the G7 flip is run")]
fn g7_step(world: &mut EngineWorld) {
    keep(world, g7());
}

#[when("the G8 fee grid is run")]
fn g8_step(world: &mut EngineWorld) {
    let grid = g8();
    assert_eq!(
        grid.iter().map(|(bps, _)| bps.0).collect::<Vec<_>>(),
        FEE_GRID_BPS
    );
    world.grid = grid
        .iter()
        .map(|(_, report)| report.equity_end().unwrap().0)
        .collect();
}

#[when("the G9 buy-and-hold is run")]
fn g9_step(world: &mut EngineWorld) {
    let (tape, config) = g9_book();
    let mut strategy = BuyAndHold::new(btc(1));
    let report = run(&tape, &mut strategy, &costs(0, 0), &config).unwrap();
    world.sharpe_daily = report.metrics.sharpe_daily.map(|ratio| ratio.ticks());
    world.sharpe_annual = report.metrics.sharpe_annual.map(|ratio| ratio.ticks());
    world.drawdown = Some(report.metrics.max_drawdown.ticks());
    keep(world, report);
}

#[when("the G10 gapped tape is run")]
fn g10_gap_step(world: &mut EngineWorld) {
    let start = at(12, 0);
    world.report = None;
    world.error = Some(g10_gap().unwrap_err());
    assert_eq!(
        world.error.clone().unwrap(),
        EngineError::Gap {
            expected: UtcMicros::from_micros(start + 120_000_000),
            got: UtcMicros::from_micros(start + 180_000_000),
        }
    );
}

#[when("the G10 missing mark tape is run")]
fn g10_mark_step(world: &mut EngineWorld) {
    world.report = None;
    world.error = Some(g10_missing_mark().unwrap_err());
    assert_eq!(
        world.error.clone().unwrap(),
        EngineError::MissingMark(UtcMicros::from_micros(at(16, 0)))
    );
}

#[when("the G10 missing funding tape is run")]
fn g10_funding_step(world: &mut EngineWorld) {
    world.report = None;
    world.error = Some(g10_missing_funding().unwrap_err());
    assert_eq!(
        world.error.clone().unwrap(),
        EngineError::MissingFunding(UtcMicros::from_micros(at(16, 0)))
    );
}

#[when("the G11 leveraged long is run")]
fn g11_step(world: &mut EngineWorld) {
    keep(world, g11());
}

#[when("the G12 three buys are run")]
fn g12_step(world: &mut EngineWorld) {
    keep(world, g12());
}

#[when("the T3 hour target is run")]
fn t3_step(world: &mut EngineWorld) {
    keep(world, t3());
}

#[when("the T5 funding visibility case is run")]
fn t5_step(world: &mut EngineWorld) {
    let (hidden, visible) = t5_flags();
    world.funding_hidden = hidden;
    world.funding_visible = visible;
    keep(world, t5().0);
}

#[when("the T7 two-minute listed gap is run")]
fn t7_two_step(world: &mut EngineWorld) {
    match t7(2, 3, true, true) {
        Ok(report) => keep(world, report),
        Err(err) => {
            world.report = None;
            world.error = Some(err);
        }
    }
}

#[when("the T7 six-minute listed gap is run")]
fn t7_six_step(world: &mut EngineWorld) {
    world.report = None;
    world.error = Some(t7(2, 7, true, true).expect_err("six minute gap"));
}

#[when("the T7 unlisted gap is run")]
fn t7_unlisted_step(world: &mut EngineWorld) {
    world.report = None;
    world.error = Some(t7(2, 3, false, true).expect_err("unlisted gap"));
}

#[when("an interior minute is unavailable until the next hour")]
fn delayed_minute_step(world: &mut EngineWorld) {
    world.error = Some(cases::delayed_interior_minute());
}

#[when("an earlier funding slot is delayed beyond a later available slot")]
fn delayed_funding_step(world: &mut EngineWorld) {
    world.funding_scripts = Some((
        cases::delayed_funding_prefix(1_000),
        cases::delayed_funding_prefix(9_999),
    ));
}

#[then("the delayed funding mutation leaves the signal's funding prefix empty")]
fn delayed_funding_hidden(world: &mut EngineWorld) {
    let (first, mutated) = world
        .funding_scripts
        .as_ref()
        .expect("funding views recorded");
    assert_eq!(first.funding_seen, mutated.funding_seen);
    assert!(first.funding_seen.iter().all(Vec::is_empty));
}

#[then("the engine error is resample availability")]
fn err_available(world: &mut EngineWorld) {
    assert!(matches!(
        world.error,
        Some(EngineError::Resample(
            parangsae_core::ResampleError::Available { .. }
        ))
    ));
}

#[when(regex = r"^a (stale-flat|zero) target follows an untraded hour$")]
fn superseded_step(world: &mut EngineWorld, kind: String) {
    let flat = if kind == "stale-flat" {
        cases::stale()
    } else {
        cases::tgt(0.0)
    };
    keep(
        world,
        cases::filler_hour_targets(vec![Some(cases::tgt(1.0)), Some(flat), Some(flat)], 1),
    );
}

#[when("an unchanged target waits through two untraded hours")]
fn unchanged_step(world: &mut EngineWorld) {
    keep(
        world,
        cases::filler_hour_targets(vec![Some(cases::tgt(1.0)); 4], 2),
    );
}

#[then(regex = r"^refused orders are (\d+)$")]
fn refused(world: &mut EngineWorld, n: u64) {
    assert_eq!(world.report().n_refused_fills, n);
}

#[then(regex = r"^there are (\d+) fills$")]
fn fill_count(world: &mut EngineWorld, n: usize) {
    assert_eq!(world.report().fills.len(), n);
}

#[then(regex = r"^fill (\d+) is (buy|sell) at (\d+) USDT with fee (-?\d+)$")]
fn fill_is(world: &mut EngineWorld, index: usize, side: String, usdt: i64, fee: i64) {
    let fill = &world.report().fills[index - 1];
    let want = if side == "buy" { Side::Buy } else { Side::Sell };
    assert_eq!(fill.side, want);
    assert_eq!(fill.price, px(usdt));
    assert_eq!(fill.fee, Cash(fee));
}

#[then(regex = r"^the closed trade pnl is (-?\d+)$")]
fn closed_pnl(world: &mut EngineWorld, pnl: i64) {
    let closed = world
        .report()
        .trades
        .iter()
        .find(|trade| trade.close_ts.is_some())
        .expect("a closed trade");
    assert_eq!(closed.pnl, Cash(pnl));
}

#[then(regex = r"^ending equity is (-?\d+)$")]
fn ending_equity(world: &mut EngineWorld, ticks: i64) {
    assert_eq!(world.report().equity_end().unwrap(), Cash(ticks));
}

#[then(regex = r"^funding amount is (-?\d+) on position (-?\d+) at mark (\d+)$")]
fn funding_is(world: &mut EngineWorld, amount: i64, position: i64, mark: i64) {
    let event = world.report().funding.first().expect("a funding event");
    assert_eq!(event.amount, Cash(amount));
    assert_eq!(event.position.0, position);
    assert_eq!(event.mark, px(mark));
}

#[then(regex = r"^the closed trade count is (\d+)$")]
fn closed_count(world: &mut EngineWorld, n: usize) {
    let count = world
        .report()
        .trades
        .iter()
        .filter(|trade| trade.close_ts.is_some())
        .count();
    assert_eq!(count, n);
}

#[then(regex = r"^the open trade count is (\d+)$")]
fn open_count(world: &mut EngineWorld, n: usize) {
    let count = world
        .report()
        .trades
        .iter()
        .filter(|trade| trade.close_ts.is_none())
        .count();
    assert_eq!(count, n);
}

#[then(regex = r"^fee-grid ending equities are (\d+),(\d+),(\d+),(\d+)$")]
fn grid_equities(world: &mut EngineWorld, a: i64, b: i64, c: i64, d: i64) {
    assert_eq!(world.grid, vec![a, b, c, d]);
}

#[then(regex = r"^the daily sharpe ticks are (\d+)$")]
fn daily_sharpe(world: &mut EngineWorld, ticks: i64) {
    assert_eq!(world.sharpe_daily, Some(ticks));
}

#[then(regex = r"^the annual sharpe ticks are (\d+)$")]
fn annual_sharpe(world: &mut EngineWorld, ticks: i64) {
    assert_eq!(world.sharpe_annual, Some(ticks));
}

#[then(regex = r"^the max drawdown ticks are (\d+)$")]
fn max_dd(world: &mut EngineWorld, ticks: i64) {
    assert_eq!(world.drawdown, Some(ticks));
}

#[then("the engine error is a gap")]
fn err_gap(world: &mut EngineWorld) {
    assert!(matches!(world.error, Some(EngineError::Gap { .. })));
}

#[then("the engine error is a missing mark")]
fn err_mark(world: &mut EngineWorld) {
    assert!(matches!(world.error, Some(EngineError::MissingMark(_))));
}

#[then("the engine error is missing funding")]
fn err_funding(world: &mut EngineWorld) {
    assert!(matches!(world.error, Some(EngineError::MissingFunding(_))));
}

#[then("the run is ruined")]
fn ruined(world: &mut EngineWorld) {
    assert!(world.report().ruined.is_some());
}

#[then("the run is not ruined")]
fn not_ruined(world: &mut EngineWorld) {
    assert!(world.report().ruined.is_none());
}

#[then(regex = r"^equity at bar (\d+) is (-?\d+)$")]
fn equity_at(world: &mut EngineWorld, bar: usize, ticks: i64) {
    assert_eq!(world.report().equity_curve[bar - 1].1, Cash(ticks));
}

#[then("equity at bar 4 equals equity at bar 3")]
fn equity_frozen(world: &mut EngineWorld) {
    let curve = &world.report().equity_curve;
    assert_eq!(curve[2].1, curve[3].1);
}

#[then(regex = r"^dropped orders are (\d+)$")]
fn dropped(world: &mut EngineWorld, n: u64) {
    assert_eq!(world.report().dropped_orders, n);
}

#[then("the 08:00 funding is hidden at 08:00:30")]
fn funding_hidden_step(world: &mut EngineWorld) {
    assert!(world.funding_hidden);
}

#[then("the 08:00 funding is visible at 08:01:00")]
fn funding_visible_step(world: &mut EngineWorld) {
    assert!(world.funding_visible);
}

#[tokio::main]
async fn main() {
    let features = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/features/backtest_engine.feature");
    EngineWorld::cucumber().run_and_exit(features).await;
}

#[when("a target waits for a decision one millisecond after the boundary")]
fn qr_delayed_step(world: &mut EngineWorld) {
    keep(world, cases::delayed_target_report());
}
#[then("the first fill is the 13:01 open")]
fn qr_later_open(world: &mut EngineWorld) {
    assert_eq!(world.report().fills[0].ts.micros(), at(13, 1));
}
#[when("the target is half equity and the fill price doubles")]
fn qr_sizing_step(world: &mut EngineWorld) {
    keep(world, cases::moving_price());
}
#[then("the filled quantity is 5000000 ticks")]
fn qr_fill_qty(world: &mut EngineWorld) {
    assert_eq!(world.report().fills[0].qty.ticks(), 5_000_000);
}

#[when("the fractional-tick drawdown cash curve is measured")]
fn fractional_drawdown_step(world: &mut EngineWorld) {
    let curve = [
        Cash(100_000_000_000_000),
        Cash(90_000_000_000_000),
        Cash(71_999_999_999_999),
        Cash(90_000_000_000_000),
    ];
    world.drawdown = Some(parangsae_backtest::max_drawdown(&curve).unwrap().ticks());
}

#[when("the H2 lookahead probe runs with an unreachable deadband")]
fn inactive_probe(world: &mut EngineWorld) {
    let params = std::collections::BTreeMap::from([
        ("deadband".to_owned(), 200_000_001),
        ("n_members".to_owned(), 12),
    ]);
    world.probe_result = Some(parangsae_strategies::truncation_probe("H2", &params));
}

#[then("the probe refuses to attest causality without an active decision")]
fn inactive_probe_refused(world: &mut EngineWorld) {
    let error = world.probe_result.as_ref().unwrap().as_ref().unwrap_err();
    assert!(error.contains("no active comparison decision"), "{error}");
}

#[when("a pending position flip is canceled before a tradable minute")]
fn canceled_flip_step(world: &mut EngineWorld) {
    keep(world, cases::canceled_pending_flip());
}

#[then("only the initial realized rebalance counts toward the activity gate")]
fn realized_activity_only(world: &mut EngineWorld) {
    assert_eq!(world.report().n_rebalances, 1);
    assert_eq!(world.report().n_sign_changes, 0);
}
