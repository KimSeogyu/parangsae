use parangsae_backtest::fixture::Scripted;
use parangsae_backtest::{run, run_fee_grid, Bar, EngineError, FundingStamp, Report, Tape};
use parangsae_core::{Qty, Ratio, UtcMicros};
use parangsae_sim::{FeeBps, MarketOrder, Side};

use super::tape::{at, bar_at, btc, cfg, costs, px, series, tape_of};

fn show<T, E: std::fmt::Display>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|err| err.to_string())
}

pub fn lots(n: i64) -> Result<Qty, String> {
    show(Qty::from_ticks(n))
}

pub fn buy(qty: Qty) -> MarketOrder {
    MarketOrder {
        side: Side::Buy,
        qty,
    }
}

pub fn sell(qty: Qty) -> MarketOrder {
    MarketOrder {
        side: Side::Sell,
        qty,
    }
}

pub fn tape_marked(bars: Vec<Bar>, marks: Vec<Bar>, funding: Vec<FundingStamp>) -> Tape {
    Tape::from_parts(bars, marks, funding)
}

pub fn stamp(us: i64, rate: i64) -> FundingStamp {
    let ts = UtcMicros::from_micros(us);
    FundingStamp {
        funding_time: ts,
        available: ts,
        rate: Ratio::from_ticks(rate),
        interval_hours: 8,
    }
}

pub fn run_script(
    bars: Vec<Bar>,
    orders: Vec<Option<MarketOrder>>,
    initial: i64,
    fee: u32,
) -> Result<Report, String> {
    let cfg = cfg(&bars, initial)?;
    let tape = tape_of(bars);
    let mut strategy = Scripted::new(orders);
    show(run(&tape, &mut strategy, &costs(fee, 0), &cfg))
}

pub fn g1() -> Result<Report, String> {
    let bars = series(at(12, 0)?, &[(100, 100), (100, 100), (110, 110)])?;
    run_script(bars, vec![Some(buy(btc(1)?)), Some(sell(btc(1)?))], 1000, 5)
}

fn funding_hold(rate: i64, side: Option<Side>) -> Result<Report, String> {
    let start = at(15, 58)?;
    let bars = series(start, &[(100, 100), (100, 100), (100, 100)])?;
    let mut marks = Vec::with_capacity(bars.len());
    for bar in &bars {
        marks.push(bar_at(bar.open_time.micros(), px(100)?, px(110)?)?);
    }
    let tape = tape_marked(bars.clone(), marks, vec![stamp(at(16, 0)?, rate)]);
    let orders = match side {
        Some(Side::Buy) => vec![Some(buy(btc(1)?))],
        Some(Side::Sell) => vec![Some(sell(btc(1)?))],
        None => Vec::new(),
    };
    let mut strategy = Scripted::new(orders);
    show(run(&tape, &mut strategy, &costs(5, 0), &cfg(&bars, 1000)?))
}

pub fn g3() -> Result<Report, String> {
    funding_hold(10_000, Some(Side::Buy))
}

pub fn g4_short_pays() -> Result<Report, String> {
    funding_hold(-10_000, Some(Side::Sell))
}

pub fn g4_short_receives() -> Result<Report, String> {
    funding_hold(10_000, Some(Side::Sell))
}

pub fn g4_flat() -> Result<Report, String> {
    funding_hold(10_000, None)
}

pub fn g5() -> Result<Report, String> {
    let start = at(15, 58)?;
    let bars = series(start, &[(100, 100), (100, 100), (100, 100)])?;
    let mut marks = Vec::with_capacity(bars.len());
    for bar in &bars {
        marks.push(bar_at(bar.open_time.micros(), px(100)?, px(110)?)?);
    }
    let tape = tape_marked(bars.clone(), marks, vec![stamp(at(16, 0)?, 10_000)]);
    let mut strategy = Scripted::new(vec![Some(buy(btc(1)?)), Some(sell(btc(1)?))]);
    show(run(&tape, &mut strategy, &costs(5, 0), &cfg(&bars, 1000)?))
}

pub fn g6() -> Result<Report, String> {
    let bars = series(at(12, 0)?, &[(100, 100), (100, 100)])?;
    run_script(bars, vec![Some(buy(lots(1)?))], 1000, 5)
}

pub fn g7() -> Result<Report, String> {
    let bars = series(at(12, 0)?, &[(100, 100), (100, 100), (105, 105)])?;
    run_script(bars, vec![Some(buy(btc(1)?)), Some(sell(btc(2)?))], 1000, 5)
}

pub fn g2() -> Result<Report, String> {
    let bars = series(at(12, 0)?, &[(100, 105), (100, 100)])?;
    run_script(bars, vec![Some(buy(btc(1)?))], 1000, 5)
}

pub fn g8() -> Result<Vec<(FeeBps, Report)>, String> {
    let bars = series(at(12, 0)?, &[(100, 100), (100, 100), (110, 110)])?;
    let tape = tape_of(bars.clone());
    let opened = buy(btc(1)?);
    let closed = sell(btc(1)?);
    let window = cfg(&bars, 1000)?;
    show(run_fee_grid(
        &tape,
        || Scripted::new(vec![Some(opened), Some(closed)]),
        &costs(5, 0),
        &window,
    ))
}

pub fn g10_gap() -> Result<Report, EngineError> {
    let start = at(12, 0).map_err(|_| EngineError::Broken("fixture clock"))?;
    let gapped = vec![
        bar_at(
            start,
            px(100).map_err(|_| EngineError::Broken("fixture price"))?,
            px(100).map_err(|_| EngineError::Broken("fixture price"))?,
        )
        .map_err(|_| EngineError::Broken("fixture bar"))?,
        bar_at(
            start + 60_000_000,
            px(100).map_err(|_| EngineError::Broken("fixture price"))?,
            px(100).map_err(|_| EngineError::Broken("fixture price"))?,
        )
        .map_err(|_| EngineError::Broken("fixture bar"))?,
        bar_at(
            start + 180_000_000,
            px(100).map_err(|_| EngineError::Broken("fixture price"))?,
            px(100).map_err(|_| EngineError::Broken("fixture price"))?,
        )
        .map_err(|_| EngineError::Broken("fixture bar"))?,
    ];
    let tape = tape_of(gapped.clone());
    let mut flat = Scripted::new(Vec::new());
    run(
        &tape,
        &mut flat,
        &costs(5, 0),
        &cfg(&gapped, 1000).map_err(|_| EngineError::Broken("fixture window"))?,
    )
}

pub fn g10_missing_mark() -> Result<Report, EngineError> {
    let funding_open = at(16, 0).map_err(|_| EngineError::Broken("fixture clock"))?;
    let bars = series(funding_open, &[(100, 100), (100, 100)])
        .map_err(|_| EngineError::Broken("fixture bars"))?;
    let marks = vec![bar_at(
        funding_open + 60_000_000,
        px(100).map_err(|_| EngineError::Broken("fixture price"))?,
        px(100).map_err(|_| EngineError::Broken("fixture price"))?,
    )
    .map_err(|_| EngineError::Broken("fixture bar"))?];
    let tape = tape_marked(bars.clone(), marks, vec![stamp(funding_open, 10_000)]);
    let mut flat = Scripted::new(Vec::new());
    run(
        &tape,
        &mut flat,
        &costs(5, 0),
        &cfg(&bars, 1000).map_err(|_| EngineError::Broken("fixture window"))?,
    )
}

pub fn g10_missing_funding() -> Result<Report, EngineError> {
    let start = at(15, 59).map_err(|_| EngineError::Broken("fixture clock"))?;
    let bars = series(start, &[(100, 100), (100, 100)])
        .map_err(|_| EngineError::Broken("fixture bars"))?;
    let tape = tape_of(bars.clone());
    let mut flat = Scripted::new(Vec::new());
    run(
        &tape,
        &mut flat,
        &costs(5, 0),
        &cfg(&bars, 1000).map_err(|_| EngineError::Broken("fixture window"))?,
    )
}

pub fn g11() -> Result<Report, String> {
    let bars = series(at(12, 0)?, &[(100, 100), (100, 100), (50, 50), (40, 40)])?;
    let tape = tape_of(bars.clone());
    let mut strategy = Scripted::new(vec![
        Some(buy(btc(1)?)),
        None,
        Some(buy(btc(1)?)),
        Some(buy(btc(1)?)),
    ]);
    show(run(&tape, &mut strategy, &costs(5, 0), &cfg(&bars, 50)?))
}

pub fn g12() -> Result<Report, String> {
    let bars = series(at(12, 0)?, &[(100, 100), (100, 100), (100, 100)])?;
    run_script(
        bars,
        vec![Some(buy(btc(1)?)), Some(buy(btc(1)?)), Some(buy(btc(1)?))],
        1000,
        5,
    )
}
