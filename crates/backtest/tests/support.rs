#![allow(dead_code)]

use parangsae_backtest::fixture::Scripted;
use parangsae_backtest::{
    run, run_fee_grid, Bar, EngineError, FundingStamp, Report, RunConfig, Tape, UtcDate,
};
use parangsae_core::{Interval, Price, Qty, Ratio, Side, UtcMicros};
use parangsae_sim::{Cash, Costs, FeeBps, MarketOrder, SlippageBps};

pub const TICK: i64 = 100_000_000;

pub fn at(hour: i64, minute: i64) -> i64 {
    UtcDate::parse("2024-01-01").unwrap().start_us().micros()
        + (hour * 60 + minute) * 60 * 1_000_000
}

pub fn px(usdt: i64) -> Price {
    Price::from_ticks(usdt * TICK).unwrap()
}

pub fn btc(whole: i64) -> Qty {
    Qty::from_ticks(whole * TICK).unwrap()
}

pub fn lots(n: i64) -> Qty {
    Qty::from_ticks(n).unwrap()
}

pub fn usd(whole: i64) -> Cash {
    Cash(whole * TICK)
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

pub fn bar_at(open_us: i64, open_px: Price, close_px: Price) -> Bar {
    let open_time = UtcMicros::from_micros(open_us);
    let close_time = UtcMicros::from_micros(open_us + 60_000_000 - 1_000);
    let available = UtcMicros::from_micros(close_time.micros() + 1_000);
    let (high, low) = if open_px.ticks() >= close_px.ticks() {
        (open_px, close_px)
    } else {
        (close_px, open_px)
    };
    Bar {
        open_time,
        close_time,
        available,
        open: open_px,
        high,
        low,
        close: close_px,
        volume: btc(1),
        count: 1,
    }
}

pub fn series(start_us: i64, rows: &[(i64, i64)]) -> Vec<Bar> {
    rows.iter()
        .enumerate()
        .map(|(index, (open_px, close_px))| {
            bar_at(
                start_us + index as i64 * 60_000_000,
                px(*open_px),
                px(*close_px),
            )
        })
        .collect()
}

pub fn tape_of(bars: Vec<Bar>) -> Tape {
    let marks = bars.clone();
    Tape::from_parts(bars, marks, Vec::new())
}

pub fn tape_marked(bars: Vec<Bar>, marks: Vec<Bar>, funding: Vec<FundingStamp>) -> Tape {
    Tape::from_parts(bars, marks, funding)
}

pub fn cfg(bars: &[Bar], initial_usdt: i64) -> RunConfig {
    RunConfig {
        initial_cash: usd(initial_usdt),
        start: bars[0].open_time,
        end: UtcMicros::from_micros(bars.last().unwrap().open_time.micros() + 60_000_000),
        holdout_start: UtcDate::parse("2025-10-01").unwrap().start_us(),
        forced_flat: Vec::new(),
        mark_gaps: Vec::new(),
        interval: Interval::ONE_HOUR,
        lot_ticks: 100_000,
    }
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
) -> Report {
    let cfg = cfg(&bars, initial);
    let tape = tape_of(bars);
    let mut strategy = Scripted::new(orders);
    run(&tape, &mut strategy, &costs(fee, 0), &cfg).unwrap()
}

pub fn g1() -> Report {
    let bars = series(at(12, 0), &[(100, 100), (100, 100), (110, 110)]);
    run_script(bars, vec![Some(buy(btc(1))), Some(sell(btc(1)))], 1000, 5)
}

fn funding_hold(rate: i64, side: Option<Side>) -> Report {
    let start = at(15, 58);
    let bars = series(start, &[(100, 100), (100, 100), (100, 100)]);
    let marks: Vec<Bar> = bars
        .iter()
        .map(|bar| bar_at(bar.open_time.micros(), px(100), px(110)))
        .collect();
    let tape = tape_marked(bars.clone(), marks, vec![stamp(at(16, 0), rate)]);
    let orders = match side {
        Some(Side::Buy) => vec![Some(buy(btc(1)))],
        Some(Side::Sell) => vec![Some(sell(btc(1)))],
        None => Vec::new(),
    };
    let mut strategy = Scripted::new(orders);
    run(&tape, &mut strategy, &costs(5, 0), &cfg(&bars, 1000)).unwrap()
}

pub fn g3() -> Report {
    funding_hold(10_000, Some(Side::Buy))
}

pub fn g4_short_pays() -> Report {
    funding_hold(-10_000, Some(Side::Sell))
}

pub fn g4_short_receives() -> Report {
    funding_hold(10_000, Some(Side::Sell))
}

pub fn g4_flat() -> Report {
    funding_hold(10_000, None)
}

pub fn g5() -> Report {
    let start = at(15, 58);
    let bars = series(start, &[(100, 100), (100, 100), (100, 100)]);
    let marks: Vec<Bar> = bars
        .iter()
        .map(|bar| bar_at(bar.open_time.micros(), px(100), px(110)))
        .collect();
    let tape = tape_marked(bars.clone(), marks, vec![stamp(at(16, 0), 10_000)]);
    let mut strategy = Scripted::new(vec![Some(buy(btc(1))), Some(sell(btc(1)))]);
    run(&tape, &mut strategy, &costs(5, 0), &cfg(&bars, 1000)).unwrap()
}

pub fn g6() -> Report {
    let bars = series(at(12, 0), &[(100, 100), (100, 100)]);
    run_script(bars, vec![Some(buy(lots(1)))], 1000, 5)
}

pub fn g7() -> Report {
    let bars = series(at(12, 0), &[(100, 100), (100, 100), (105, 105)]);
    run_script(bars, vec![Some(buy(btc(1))), Some(sell(btc(2)))], 1000, 5)
}

pub fn g2() -> Report {
    let bars = series(at(12, 0), &[(100, 105), (100, 100)]);
    run_script(bars, vec![Some(buy(btc(1)))], 1000, 5)
}

pub fn g8() -> Vec<(FeeBps, Report)> {
    let bars = series(at(12, 0), &[(100, 100), (100, 100), (110, 110)]);
    let tape = tape_of(bars.clone());
    run_fee_grid(
        &tape,
        || Scripted::new(vec![Some(buy(btc(1))), Some(sell(btc(1)))]),
        &costs(5, 0),
        &cfg(&bars, 1000),
    )
    .unwrap()
}

pub fn g10_gap() -> Result<Report, EngineError> {
    let start = at(12, 0);
    let gapped = vec![
        bar_at(start, px(100), px(100)),
        bar_at(start + 60_000_000, px(100), px(100)),
        bar_at(start + 180_000_000, px(100), px(100)),
    ];
    let tape = tape_of(gapped.clone());
    let mut flat = Scripted::new(Vec::new());
    run(&tape, &mut flat, &costs(5, 0), &cfg(&gapped, 1000))
}

pub fn g10_missing_mark() -> Result<Report, EngineError> {
    let funding_open = at(16, 0);
    let bars = series(funding_open, &[(100, 100), (100, 100)]);
    let marks = vec![bar_at(funding_open + 60_000_000, px(100), px(100))];
    let tape = tape_marked(bars.clone(), marks, vec![stamp(funding_open, 10_000)]);
    let mut flat = Scripted::new(Vec::new());
    run(&tape, &mut flat, &costs(5, 0), &cfg(&bars, 1000))
}

pub fn g10_missing_funding() -> Result<Report, EngineError> {
    let start = at(15, 59);
    let bars = series(start, &[(100, 100), (100, 100)]);
    let tape = tape_of(bars.clone());
    let mut flat = Scripted::new(Vec::new());
    run(&tape, &mut flat, &costs(5, 0), &cfg(&bars, 1000))
}

pub fn g11() -> Report {
    let bars = series(at(12, 0), &[(100, 100), (100, 100), (50, 50), (40, 40)]);
    let tape = tape_of(bars.clone());
    let mut strategy = Scripted::new(vec![
        Some(buy(btc(1))),
        None,
        Some(buy(btc(1))),
        Some(buy(btc(1))),
    ]);
    run(&tape, &mut strategy, &costs(5, 0), &cfg(&bars, 50)).unwrap()
}

pub fn g9_book() -> (Tape, RunConfig) {
    let start = UtcDate::parse("2024-01-01").unwrap().start_us().micros();
    let minutes = 3 * 24 * 60;
    let mut bars = Vec::with_capacity(minutes);
    let mut marks = Vec::with_capacity(minutes);
    for index in 0..minutes {
        let open = start + index as i64 * 60_000_000;
        let day = index / (24 * 60);
        let mark_px = if day == 0 { 110 } else { 121 };
        bars.push(bar_at(open, px(100), px(100)));
        marks.push(bar_at(open, px(mark_px), px(mark_px)));
    }
    let mut funding = Vec::new();
    for day in 0..3 {
        for hour in [0i64, 8, 16] {
            let ts = start + (day * 24 + hour) * 3_600_000_000;
            funding.push(stamp(ts, 0));
        }
    }
    let config = cfg(&bars, 100);
    (tape_marked(bars, marks, funding), config)
}

pub fn g12() -> Report {
    let bars = series(at(12, 0), &[(100, 100), (100, 100), (100, 100)]);
    run_script(
        bars,
        vec![Some(buy(btc(1))), Some(buy(btc(1))), Some(buy(btc(1)))],
        1000,
        5,
    )
}
