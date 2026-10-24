//! Second-batch families. Names and keys are the frozen prereg's.

mod support;

use parangsae_core::BarInterval;
use parangsae_strategies::{replay, FundingPrint, PremiumMinute, Strategy, StrategyError};
use support::{assert_prefix, flat, params, ticks, usdt, SCALE, T0};

const HOUR_US: i64 = 3_600_000_000;
const DAY_US: i64 = 86_400_000_000;

#[test]
fn s01_omitted_cadence_matches_the_explicit_default_deadband() {
    let base = params(&[("c", SCALE), ("interval", 60), ("n", 24)]);
    let mut explicit = base.clone();
    explicit.insert("deadband".to_owned(), 25_000_000);
    let bars = s01_path();
    let plain = replay("S01", &base, &bars).unwrap();
    let named = replay("S01", &explicit, &bars).unwrap();
    assert_eq!(plain, named);
    assert_eq!(ticks(&plain[0]), 39_605_254);
    assert_eq!(ticks(&plain[1]), 39_605_254);

    let mut tight = base.clone();
    tight.insert("deadband".to_owned(), 1);
    let moved = replay("S01", &tight, &bars).unwrap();
    assert_ne!(ticks(&moved[1]), ticks(&plain[1]));
}

#[test]
fn h1_decides_only_at_the_named_hour_and_keeps_s01_off_that_path() {
    let mut h1 = params(&[
        ("c", SCALE),
        ("interval", 240),
        ("n", 42),
        ("deadband", 1),
        ("decision_hour_utc", 0),
    ]);
    let mut bars = Vec::new();
    for index in 0..42 {
        bars.push(flat(index, BarInterval::H4, usdt(40_000)));
    }
    bars.push(flat(42, BarInterval::H4, usdt(50_000)));
    for index in 43..48 {
        bars.push(flat(index, BarInterval::H4, usdt(40_000)));
    }
    let s01 = params(&[("c", SCALE), ("interval", 240), ("n", 42)]);
    let intraday = replay("S01", &s01, &bars).unwrap();
    assert_eq!(ticks(&intraday[0]), SCALE);
    assert_eq!(
        intraday[0].decision_ts.micros().micros(),
        bars[42].timestamps().available_ts.micros().micros()
    );

    let daily = replay("H1", &h1, &bars).unwrap();
    assert!(daily.iter().all(|signal| {
        let hour = signal.decision_ts.micros().micros() / HOUR_US % 24;
        hour == 0 && ticks(signal) == 0
    }));
    assert!(!daily.is_empty());
    h1.remove("decision_hour_utc");
    match Strategy::from_params("H1", &h1) {
        Err(StrategyError::Param("decision_hour_utc")) => {}
        Err(err) => panic!("expected decision_hour_utc, got {err}"),
        Ok(_) => panic!("H1 accepted a missing decision hour"),
    }
    let wide = params(&[
        ("c", SCALE),
        ("interval", 240),
        ("n", 360),
        ("deadband", 50_000_000),
        ("decision_hour_utc", 0),
    ]);
    assert_eq!(
        Strategy::from_params("H1", &wide).unwrap().interval(),
        BarInterval::H4
    );
    assert_prefix("H1", &wide_ready(), &h4_flat(8));
}

#[test]
fn h2_blends_twelve_members_and_ignores_a_later_bar() {
    let spec = params(&[("deadband", 25_000_000), ("n_members", 12)]);
    assert!(Strategy::from_params(
        "H2",
        &params(&[("deadband", 25_000_000), ("n_members", 11)])
    )
    .is_err());
    let flat_bars = h1_flat(730);
    let out = replay("H2", &spec, &flat_bars).unwrap();
    assert!(out.iter().all(|signal| ticks(signal) == 0));
    assert!(!out.is_empty());
    let mut rising = Vec::new();
    for index in 0..730 {
        rising.push(flat(index, BarInterval::H1, usdt(40_000 + index * 20)));
    }
    let sensitive = params(&[("deadband", 1), ("n_members", 12)]);
    let trend = replay("H2", &sensitive, &rising).unwrap();
    assert!(trend.iter().any(|signal| ticks(signal) > 0));
    assert_prefix("H2", &spec, &rising);
}

#[test]
fn h3_blocks_a_break_until_the_efficiency_gate_opens() {
    let s02 = params(&[("n", 2), ("hold_h", 48)]);
    let h3 = params(&[("n", 2), ("er_l", 2), ("er_q", 50_000_000), ("hold_h", 48)]);
    let px = |index: i64, usdt_px: i64| flat(index, BarInterval::H1, usdt(usdt_px));
    let early = vec![px(0, 100), px(1, 100), px(2, 120), px(3, 130)];
    let open = replay("S02", &s02, &early).unwrap();
    assert_eq!(ticks(&open[0]), SCALE);
    let shut = replay("H3", &h3, &early).unwrap();
    assert!(shut.iter().all(|signal| ticks(signal) == 0));

    let mut trend = Vec::new();
    for index in 0..8_010 {
        trend.push(flat(index, BarInterval::H1, usdt(100 + index)));
    }
    let gated = replay("H3", &h3, &trend).unwrap();
    assert!(
        gated.iter().any(|signal| ticks(signal) == SCALE),
        "a complete ER history must open the gate on a clean break"
    );
    assert_prefix("H3", &h3, &early);
}

#[test]
fn h4_fades_a_crowded_long_only_after_confirmation_and_observe_does_not_arm() {
    let spec = params(&[
        ("q", 10_000_000),
        ("sum_settlements", 3),
        ("confirm_h", 24),
        ("hold_h", 72),
    ]);
    let funding = crowding_rates();
    let bars = confirm_bars();
    let mut armed = Strategy::from_params("H4", &spec).unwrap();
    armed.ingest_funding(&funding).unwrap();
    let mut saw_short = false;
    for bar in &bars {
        if let Some(opportunity) = armed.h4_opportunity(bar).unwrap() {
            if ticks(&opportunity.signal().unwrap()) == -SCALE {
                saw_short = true;
            }
        }
    }
    assert!(
        saw_short,
        "long-crowded funding with a down confirm is short"
    );

    let mut watched = Strategy::from_params("H4", &spec).unwrap();
    watched.ingest_funding(&funding).unwrap();
    for bar in &bars {
        watched.observe(bar).unwrap();
    }
    let next = bar_at(T0 + 24 * HOUR_US, usdt(90));
    let followed = watched.h4_opportunity(&next).unwrap();
    assert_eq!(followed, None);
}

#[test]
fn h5_shorts_a_rich_premium_and_drops_a_day_under_1380_valid_minutes() {
    let spec = params(&[("k", 200_000_000), ("hold_h", 72), ("lookback_d", 90)]);
    let decision = midnight_bar(90);
    let mut rich = Strategy::from_params("H5", &spec).unwrap();
    rich.ingest_premium(&premium_history(90, 1_380, 1_380, 1_000, 100_000_000))
        .unwrap();
    let signal = rich.push(&decision, None).unwrap().expect("ready");
    assert_eq!(ticks(&signal.signal), -SCALE);

    let mut thin = Strategy::from_params("H5", &spec).unwrap();
    thin.ingest_premium(&premium_history(90, 1_380, 1_379, 1_000, 100_000_000))
        .unwrap();
    let skipped = thin.push(&decision, None).unwrap().expect("history exists");
    assert_eq!(ticks(&skipped.signal), 0);

    let mut zeros = Strategy::from_params("H5", &spec).unwrap();
    zeros.ingest_premium(&premium_with_a_zero_minute()).unwrap();
    let counted = zeros.push(&decision, None).unwrap().expect("zero counts");
    assert_eq!(ticks(&counted.signal), -SCALE);

    let mut watched = Strategy::from_params("H5", &spec).unwrap();
    watched
        .ingest_premium(&premium_history(90, 1_380, 1_380, 1_000, 100_000_000))
        .unwrap();
    watched.observe(&decision).unwrap();
    let after = bar_at(T0 + 90 * DAY_US, usdt(40_000));
    let held = watched.push(&after, None).unwrap().expect("after observe");
    assert_eq!(ticks(&held.signal), 0);
}

fn s01_path() -> Vec<parangsae_core::Observation<parangsae_core::ResampledBar>> {
    let mut bars = Vec::new();
    for index in 0..24 {
        bars.push(flat(index, BarInterval::H1, usdt(40_000)));
    }
    bars.push(flat(24, BarInterval::H1, usdt(40_800)));
    bars.push(flat(25, BarInterval::H1, usdt(40_900)));
    bars.push(flat(26, BarInterval::H1, usdt(50_000)));
    bars
}

fn h4_flat(count: i64) -> Vec<parangsae_core::Observation<parangsae_core::ResampledBar>> {
    (0..count)
        .map(|index| flat(index, BarInterval::H4, usdt(40_000)))
        .collect()
}

fn h1_flat(count: i64) -> Vec<parangsae_core::Observation<parangsae_core::ResampledBar>> {
    (0..count)
        .map(|index| flat(index, BarInterval::H1, usdt(40_000)))
        .collect()
}

fn wide_ready() -> std::collections::BTreeMap<String, i64> {
    params(&[
        ("c", SCALE),
        ("interval", 240),
        ("n", 42),
        ("deadband", 50_000_000),
        ("decision_hour_utc", 0),
    ])
}

fn crowding_rates() -> Vec<FundingPrint> {
    let event = T0;
    let start = event - 365 * DAY_US - 16 * HOUR_US;
    let mut rows = Vec::new();
    let mut time = start;
    while time < event {
        rows.push(FundingPrint {
            funding_time_us: time,
            available_us: time + 60_000_000,
            rate_ticks: 0,
        });
        time += 8 * HOUR_US;
    }
    rows.push(FundingPrint {
        funding_time_us: event,
        available_us: event + 60_000_000,
        rate_ticks: 100_000_000,
    });
    rows
}

fn confirm_bars() -> Vec<parangsae_core::Observation<parangsae_core::ResampledBar>> {
    let mut bars = Vec::new();
    for step in 0..25 {
        let open = T0 - HOUR_US + step * HOUR_US;
        let px = if step < 24 { usdt(100) } else { usdt(90) };
        bars.push(bar_at(open, px));
    }
    bars
}

fn bar_at(open_us: i64, px: i64) -> parangsae_core::Observation<parangsae_core::ResampledBar> {
    let index = (open_us - T0) / HOUR_US;
    flat(index, BarInterval::H1, px)
}

fn premium_history(
    days: i64,
    baseline_valid: i64,
    shock_valid: i64,
    base_ticks: i64,
    shock_ticks: i64,
) -> Vec<PremiumMinute> {
    let mut rows = Vec::new();
    for day in 0..days {
        let last = day + 1 == days;
        let close = if last { shock_ticks } else { base_ticks };
        let valid = if last { shock_valid } else { baseline_valid };
        let day_open = T0 + day * DAY_US;
        for minute in 0..valid {
            let open = day_open + minute * 60_000_000;
            rows.push(PremiumMinute {
                open_us: open,
                close_ticks: close,
                count: 1,
                available_us: open + 60_000_000,
            });
        }
    }
    rows
}

fn premium_with_a_zero_minute() -> Vec<PremiumMinute> {
    let mut rows = Vec::new();
    for day in 0..90 {
        let last = day + 1 == 90;
        let day_open = T0 + day * DAY_US;
        for minute in 0..1_380 {
            let close = if last {
                100_000_000
            } else if minute == 0 {
                0
            } else {
                1_000
            };
            let open = day_open + minute * 60_000_000;
            rows.push(PremiumMinute {
                open_us: open,
                close_ticks: close,
                count: 1,
                available_us: open + 60_000_000,
            });
        }
    }
    rows
}

fn midnight_bar(day_index: i64) -> parangsae_core::Observation<parangsae_core::ResampledBar> {
    let open = T0 + day_index * DAY_US - HOUR_US;
    bar_at(open, usdt(40_000))
}
