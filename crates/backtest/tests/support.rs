#![allow(
    dead_code,
    reason = "scenario helpers are called by the other backtest test binaries that include this file"
)]

use parangsae_backtest::fixture::Scripted;
use parangsae_backtest::{
    run, run_fee_grid, Bar, EngineError, FundingStamp, Report, RunConfig, Tape, UtcDate,
};
use parangsae_core::{Price, Qty, Ratio, UtcMicros};
use parangsae_sim::{Cash, Costs, FeeBps, MarketOrder, Side, SlippageBps};

pub const TICK: i64 = 100_000_000;

fn show<T, E: std::fmt::Display>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|err| err.to_string())
}

pub fn at(hour: i64, minute: i64) -> Result<i64, String> {
    let day = UtcDate::parse("2024-01-01").ok_or("2024-01-01")?;
    let start = show(day.start_us())?.micros();
    let offset = (hour * 60 + minute)
        .checked_mul(60)
        .and_then(|seconds| seconds.checked_mul(1_000_000))
        .ok_or("clock offset overflows")?;
    start
        .checked_add(offset)
        .ok_or("clock overflows".to_owned())
}

pub fn px(usdt: i64) -> Result<Price, String> {
    show(Price::from_ticks(
        usdt.checked_mul(TICK).ok_or("price overflows")?,
    ))
}

pub fn btc(whole: i64) -> Result<Qty, String> {
    show(Qty::from_ticks(
        whole.checked_mul(TICK).ok_or("qty overflows")?,
    ))
}

pub fn lots(n: i64) -> Result<Qty, String> {
    show(Qty::from_ticks(n))
}

pub fn usd(whole: i64) -> Result<Cash, String> {
    Ok(Cash(whole.checked_mul(TICK).ok_or("cash overflows")?))
}

pub fn costs(fee: u32, slip: u32) -> Costs {
    Costs {
        taker_fee: FeeBps(fee),
        slippage: SlippageBps(slip),
    }
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

pub fn bar_at(open_us: i64, open_px: Price, close_px: Price) -> Result<Bar, String> {
    let open_time = UtcMicros::from_micros(open_us);
    let close_time = UtcMicros::from_micros(
        open_us
            .checked_add(60_000_000)
            .and_then(|us| us.checked_sub(1_000))
            .ok_or("close time overflows")?,
    );
    let available = UtcMicros::from_micros(
        close_time
            .micros()
            .checked_add(1_000)
            .ok_or("available time overflows")?,
    );
    let (high, low) = if open_px.ticks() >= close_px.ticks() {
        (open_px, close_px)
    } else {
        (close_px, open_px)
    };
    Ok(Bar {
        open_time,
        close_time,
        available,
        open: open_px,
        high,
        low,
        close: close_px,
        volume: btc(1)?,
    })
}

pub fn series(start_us: i64, rows: &[(i64, i64)]) -> Result<Vec<Bar>, String> {
    let mut bars = Vec::with_capacity(rows.len());
    for (index, (open_px, close_px)) in rows.iter().enumerate() {
        let offset = i64::try_from(index)
            .map_err(|_| "bar index does not fit")?
            .checked_mul(60_000_000)
            .ok_or("bar offset overflows")?;
        bars.push(bar_at(
            start_us.checked_add(offset).ok_or("bar time overflows")?,
            px(*open_px)?,
            px(*close_px)?,
        )?);
    }
    Ok(bars)
}

pub fn tape_of(bars: Vec<Bar>) -> Tape {
    let marks = bars.clone();
    Tape::from_parts(bars, marks, Vec::new())
}

pub fn tape_marked(bars: Vec<Bar>, marks: Vec<Bar>, funding: Vec<FundingStamp>) -> Tape {
    Tape::from_parts(bars, marks, funding)
}

pub fn cfg(bars: &[Bar], initial_usdt: i64) -> Result<RunConfig, String> {
    let first = bars.first().ok_or("window is empty")?;
    let last = bars.last().ok_or("window is empty")?;
    Ok(RunConfig {
        initial_cash: usd(initial_usdt)?,
        start: first.open_time,
        end: UtcMicros::from_micros(
            last.open_time
                .micros()
                .checked_add(60_000_000)
                .ok_or("window end overflows")?,
        ),
    })
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
