//! Cucumber runner for the twelve engine goldens.

use std::ops::Sub;

#[path = "../../core/tests/support/feature_floors.rs"]
mod feature_floors;
#[path = "../../core/tests/support/feature_run.rs"]
mod feature_run;
mod support;

use cucumber::{then, when};
use parangsae_backtest::{max_drawdown, sharpe_f64, EngineError, Report, FEE_GRID_BPS};
use parangsae_core::UtcMicros;
use parangsae_sim::{Cash, Side};
use support::{
    at, g1, g10_gap, g10_missing_funding, g10_missing_mark, g11, g12, g2, g3, g4_flat,
    g4_short_pays, g4_short_receives, g5, g6, g7, g8, px, usd, TICK,
};

#[derive(Debug, Default, cucumber::World)]
struct EngineWorld {
    report: Option<Report>,
    error: Option<EngineError>,
    sharpe: Option<f64>,
    drawdown: Option<i64>,
    grid: Vec<i64>,
}

impl EngineWorld {
    fn report(&self) -> Result<&Report, String> {
        self.report
            .as_ref()
            .ok_or_else(|| "a report was produced".to_owned())
    }
}

fn keep(world: &mut EngineWorld, report: Result<Report, String>) -> Result<(), String> {
    world.error = None;
    world.report = Some(report?);
    Ok(())
}

fn must_err(result: Result<Report, EngineError>) -> Result<EngineError, String> {
    match result {
        Err(err) => Ok(err),
        Ok(_) => Err("the engine accepted a tape that should abort".to_owned()),
    }
}

#[when("the G1 round trip is run")]
fn g1_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g1())
}

#[when("the G2 next-open buy is run")]
fn g2_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g2())
}

#[when("the G3 funding hold is run")]
fn g3_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g3())
}

#[when("the G4 short pays case is run")]
fn g4_pays_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g4_short_pays())
}

#[when("the G4 short receives case is run")]
fn g4_receives_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g4_short_receives())
}

#[when("the G4 flat case is run")]
fn g4_flat_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g4_flat())
}

#[when("the G5 funding and flatten is run")]
fn g5_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g5())
}

#[when("the G6 one-lot buy is run")]
fn g6_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g6())
}

#[when("the G7 flip is run")]
fn g7_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g7())
}

#[when("the G8 fee grid is run")]
fn g8_step(world: &mut EngineWorld) -> Result<(), String> {
    let grid = g8()?;
    assert_eq!(
        grid.iter().map(|(bps, _)| bps.0).collect::<Vec<_>>(),
        FEE_GRID_BPS
    );
    let mut equities = Vec::with_capacity(grid.len());
    for (_, report) in &grid {
        let end = report.equity_end().ok_or("fee-grid report has no equity")?;
        equities.push(end.0);
    }
    world.grid = equities;
    Ok(())
}

#[when("the G9 levels are scored")]
fn g9_step(world: &mut EngineWorld) -> Result<(), String> {
    let levels = [100 * TICK, 110 * TICK, 121 * TICK, 121 * TICK];
    let (daily, _) = sharpe_f64(&levels).map_err(|err| err.to_string())?;
    world.sharpe = daily;
    let curve = [usd(100)?, usd(110)?, usd(99)?, usd(121)?];
    world.drawdown = Some(max_drawdown(&curve).map_err(|err| err.to_string())?.ticks());
    Ok(())
}

#[when("the G10 gapped tape is run")]
fn g10_gap_step(world: &mut EngineWorld) -> Result<(), String> {
    let start = at(12, 0)?;
    world.report = None;
    let err = must_err(g10_gap())?;
    assert_eq!(
        err,
        EngineError::Gap {
            expected: UtcMicros::from_micros(start + 120_000_000),
            got: UtcMicros::from_micros(start + 180_000_000),
        }
    );
    world.error = Some(err);
    Ok(())
}

#[when("the G10 missing mark tape is run")]
fn g10_mark_step(world: &mut EngineWorld) -> Result<(), String> {
    world.report = None;
    let err = must_err(g10_missing_mark())?;
    assert_eq!(
        err,
        EngineError::MissingMark(UtcMicros::from_micros(at(16, 0)?))
    );
    world.error = Some(err);
    Ok(())
}

#[when("the G10 missing funding tape is run")]
fn g10_funding_step(world: &mut EngineWorld) -> Result<(), String> {
    world.report = None;
    let err = must_err(g10_missing_funding())?;
    assert_eq!(
        err,
        EngineError::MissingFunding(UtcMicros::from_micros(at(16, 0)?))
    );
    world.error = Some(err);
    Ok(())
}

#[when("the G11 leveraged long is run")]
fn g11_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g11())
}

#[when("the G12 three buys are run")]
fn g12_step(world: &mut EngineWorld) -> Result<(), String> {
    keep(world, g12())
}

#[then(regex = r"^there are (\d+) fills$")]
fn fill_count(world: &mut EngineWorld, n: usize) -> Result<(), String> {
    assert_eq!(world.report()?.fills.len(), n);
    Ok(())
}

#[then(regex = r"^fill (\d+) is (buy|sell) at (\d+) USDT with fee (-?\d+)$")]
fn fill_is(
    world: &mut EngineWorld,
    index: usize,
    side: String,
    usdt: i64,
    fee: i64,
) -> Result<(), String> {
    let fill = &world.report()?.fills[index - 1];
    let want = if side == "buy" { Side::Buy } else { Side::Sell };
    assert_eq!(fill.side, want);
    assert_eq!(fill.price, px(usdt)?);
    assert_eq!(fill.fee, Cash(fee));
    Ok(())
}

#[then(regex = r"^the closed trade pnl is (-?\d+)$")]
fn closed_pnl(world: &mut EngineWorld, pnl: i64) -> Result<(), String> {
    let closed = world
        .report()?
        .trades
        .iter()
        .find(|trade| trade.close_ts.is_some())
        .ok_or("a closed trade")?;
    assert_eq!(closed.pnl, Cash(pnl));
    Ok(())
}

#[then(regex = r"^ending equity is (-?\d+)$")]
fn ending_equity(world: &mut EngineWorld, ticks: i64) -> Result<(), String> {
    let end = world.report()?.equity_end().ok_or("ending equity")?;
    assert_eq!(end, Cash(ticks));
    Ok(())
}

#[then(regex = r"^funding amount is (-?\d+) on position (-?\d+) at mark (\d+)$")]
fn funding_is(
    world: &mut EngineWorld,
    amount: i64,
    position: i64,
    mark: i64,
) -> Result<(), String> {
    let event = world.report()?.funding.first().ok_or("a funding event")?;
    assert_eq!(event.amount, Cash(amount));
    assert_eq!(event.position.0, position);
    assert_eq!(event.mark, px(mark)?);
    Ok(())
}

#[then(regex = r"^the closed trade count is (\d+)$")]
fn closed_count(world: &mut EngineWorld, n: usize) -> Result<(), String> {
    let count = world
        .report()?
        .trades
        .iter()
        .filter(|trade| trade.close_ts.is_some())
        .count();
    assert_eq!(count, n);
    Ok(())
}

#[then(regex = r"^the open trade count is (\d+)$")]
fn open_count(world: &mut EngineWorld, n: usize) -> Result<(), String> {
    let count = world
        .report()?
        .trades
        .iter()
        .filter(|trade| trade.close_ts.is_none())
        .count();
    assert_eq!(count, n);
    Ok(())
}

#[then(regex = r"^fee-grid ending equities are (\d+),(\d+),(\d+),(\d+)$")]
fn grid_equities(world: &mut EngineWorld, a: i64, b: i64, c: i64, d: i64) {
    assert_eq!(world.grid, vec![a, b, c, d]);
}

#[then(regex = r"^the daily sharpe is ([0-9.]+)$")]
fn daily_sharpe(world: &mut EngineWorld, text: String) -> Result<(), String> {
    let expected: f64 = text
        .parse()
        .map_err(|err: std::num::ParseFloatError| err.to_string())?;
    let got = world.sharpe.ok_or("sharpe")?;
    let delta = <f64 as Sub>::sub(got, expected).abs();
    assert!(delta < 1e-9, "{got} vs {expected}");
    Ok(())
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
fn ruined(world: &mut EngineWorld) -> Result<(), String> {
    assert!(world.report()?.ruined.is_some());
    Ok(())
}

#[then("the run is not ruined")]
fn not_ruined(world: &mut EngineWorld) -> Result<(), String> {
    assert!(world.report()?.ruined.is_none());
    Ok(())
}

#[then(regex = r"^equity at bar (\d+) is (-?\d+)$")]
fn equity_at(world: &mut EngineWorld, bar: usize, ticks: i64) -> Result<(), String> {
    assert_eq!(world.report()?.equity_curve[bar - 1].1, Cash(ticks));
    Ok(())
}

#[then("equity at bar 4 equals equity at bar 3")]
fn equity_frozen(world: &mut EngineWorld) -> Result<(), String> {
    let curve = &world.report()?.equity_curve;
    assert_eq!(curve[2].1, curve[3].1);
    Ok(())
}

#[then(regex = r"^dropped orders are (\d+)$")]
fn dropped(world: &mut EngineWorld, n: u64) -> Result<(), String> {
    assert_eq!(world.report()?.dropped_orders, n);
    Ok(())
}

const FEATURE_GLOB: &str = "specs/features/backtest_engine.feature";

#[tokio::main]
async fn main() -> Result<(), String> {
    let files = feature_floors::expand_feature_glob(FEATURE_GLOB)?;
    let mut passed_steps = 0usize;
    for path in &files {
        let counts = feature_run::run_feature::<EngineWorld>(path).await?;
        passed_steps += counts.passed_steps;
    }
    feature_floors::enforce_floors("backtest", &files, passed_steps)
}
