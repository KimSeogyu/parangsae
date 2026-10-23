#![allow(dead_code)]

use std::collections::BTreeMap;

use parangsae_core::{
    AvailableTs, BarInterval, EventTs, Observation, Price, ReceivedTs, ResampledBar, Timestamps,
    UtcMicros,
};
use parangsae_signals::SignalOutput;
use parangsae_strategies::replay;

pub const T0: i64 = 1_704_067_200_000_000;
pub const SCALE: i64 = 100_000_000;

pub fn params(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), *value))
        .collect()
}

pub fn usdt(whole: i64) -> i64 {
    whole * SCALE
}

pub fn flat(index: i64, interval: BarInterval, px: i64) -> Observation<ResampledBar> {
    ohlc(index, interval, px, px, px, px)
}

pub fn ohlc(
    index: i64,
    interval: BarInterval,
    open: i64,
    high: i64,
    low: i64,
    close: i64,
) -> Observation<ResampledBar> {
    let open_us = T0 + index * interval.micros();
    let bar = ResampledBar::new(
        UtcMicros::from_micros(open_us),
        interval,
        Price::from_ticks(open).expect("open"),
        Price::from_ticks(high).expect("high"),
        Price::from_ticks(low).expect("low"),
        Price::from_ticks(close).expect("close"),
    )
    .expect("bar");
    let available = open_us + interval.micros();
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(open_us)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(available)),
        },
        bar,
    )
}

pub fn ticks(out: &SignalOutput) -> i64 {
    out.decision.target_ticks()
}

pub fn assert_clock(out: &SignalOutput) {
    assert_eq!(
        out.decision_ts.micros().micros(),
        out.bar_close_time_us.micros() + 1_000
    );
}

pub fn assert_prefix(
    family: &str,
    params: &BTreeMap<String, i64>,
    bars: &[Observation<ResampledBar>],
) {
    let full = replay(family, params, bars).expect("full");
    for cut in cut_points(bars.len()) {
        let partial = replay(family, params, &bars[..=cut]).expect("prefix");
        let cut_us = bars[cut].timestamps().available_ts.micros().micros();
        let expected: Vec<_> = full
            .iter()
            .filter(|signal| signal.decision_ts.micros().micros() <= cut_us)
            .cloned()
            .collect();
        assert_eq!(partial, expected, "{family} prefix {cut}");
    }
    if bars.len() < 2 {
        return;
    }
    let prior = bars[bars.len() - 2]
        .timestamps()
        .available_ts
        .micros()
        .micros();
    let before: Vec<_> = full
        .iter()
        .filter(|signal| signal.decision_ts.micros().micros() <= prior)
        .cloned()
        .collect();
    for kind in [Mutate::High, Mutate::Low, Mutate::Close] {
        let mutated = mutate_last(bars, kind);
        let changed = replay(family, params, &mutated).expect("mutated");
        let after: Vec<_> = changed
            .iter()
            .filter(|signal| signal.decision_ts.micros().micros() <= prior)
            .cloned()
            .collect();
        assert_eq!(before, after, "{family} future {kind:?}");
    }
}

fn cut_points(len: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let mut points = vec![
        0,
        len / 4,
        len / 3,
        len / 2,
        (3 * len) / 4,
        len.saturating_sub(2),
        len - 1,
    ];
    points.sort_unstable();
    points.dedup();
    points.retain(|cut| *cut < len);
    points
}

#[derive(Clone, Copy, Debug)]
enum Mutate {
    High,
    Low,
    Close,
}

fn mutate_last(bars: &[Observation<ResampledBar>], kind: Mutate) -> Vec<Observation<ResampledBar>> {
    let mut mutated = bars.to_vec();
    let index = bars.len() - 1;
    let current = *mutated[index].value();
    let open = current.open().ticks();
    let (high, low, close) = match kind {
        Mutate::High => (
            current.high().ticks().checked_add(1).expect("high fits"),
            current.low().ticks(),
            current.close().ticks(),
        ),
        Mutate::Low => (
            current.high().ticks(),
            current.low().ticks().checked_sub(1).expect("low fits"),
            current.close().ticks(),
        ),
        Mutate::Close => {
            let close = current.close().ticks().checked_sub(1).expect("close fits");
            (
                current.high().ticks().max(open).max(close),
                current.low().ticks().min(open).min(close),
                close,
            )
        }
    };
    let price = |ticks: i64| Price::from_ticks(ticks).expect("price");
    let rebuilt = ResampledBar::new(
        current.open_time(),
        current.interval(),
        price(open),
        price(high),
        price(low),
        price(close),
    )
    .expect("rebuilt");
    assert_ne!(rebuilt, current, "mutation {kind:?} did not change the bar");
    mutated[index] = mutated[index].with_value(rebuilt);
    mutated
}

pub fn walk(interval: BarInterval, count: usize, seed: u64) -> Vec<Observation<ResampledBar>> {
    let mut state = if seed == 0 { 0x9E37_79B9 } else { seed };
    let mut price = 16_000_000_i64;
    let mut bars = Vec::with_capacity(count);
    for index in 0..count {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        if state % 2 == 0 {
            price = (price * 2).min(1 << 52);
        } else {
            price = (price / 2).max(16_000_000);
        }
        let index = i64::try_from(index).expect("index");
        bars.push(flat(index, interval, price));
    }
    bars
}
