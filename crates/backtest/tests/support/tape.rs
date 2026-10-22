use parangsae_backtest::{Bar, RunConfig, Tape, UtcDate};
use parangsae_core::{Price, Qty, UtcMicros};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};

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

pub fn usd(whole: i64) -> Result<Cash, String> {
    Ok(Cash(whole.checked_mul(TICK).ok_or("cash overflows")?))
}

pub fn costs(fee: u32, slip: u32) -> Costs {
    Costs {
        taker_fee: FeeBps(fee),
        slippage: SlippageBps(slip),
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
