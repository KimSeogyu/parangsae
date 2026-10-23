//! Hand-checked targets. The numbers below are the position ticks, not a fit.

mod support;

use parangsae_core::{
    AvailableTs, BarInterval, DecisionTime, EventTs, Observation, Price, ReceivedTs, ResampledBar,
    Timestamps, UtcMicros,
};
use parangsae_signals::{drive, drive_feature, Tsmom, VolRegime};
use parangsae_strategies::{replay, DecisionBook, Strategy, StrategyError};
use support::{assert_clock, assert_prefix, flat, ohlc, params, ticks, usdt, SCALE, T0};

#[test]
fn s01_deadband_holds_a_small_move_and_takes_a_clamp() {
    let spec = params(&[("c", SCALE), ("interval", 60), ("n", 24)]);
    let mut bars = Vec::new();
    for index in 0..24 {
        bars.push(flat(index, BarInterval::H1, usdt(40_000)));
    }
    bars.push(flat(24, BarInterval::H1, usdt(40_800)));
    bars.push(flat(25, BarInterval::H1, usdt(40_900)));
    bars.push(flat(26, BarInterval::H1, usdt(50_000)));
    let out = replay("S01", &spec, &bars).unwrap();
    assert_eq!(out.len(), 3);
    for signal in &out {
        assert_clock(signal);
    }
    // ln(40800/40000) / 0.05 truncates to the signals-v0 tsmom golden.
    assert_eq!(ticks(&out[0]), 39_605_254);
    let raw = drive(
        &Tsmom::<24>::new(BarInterval::H1, 0.05).unwrap(),
        &bars[..=25],
        at(25),
    )
    .unwrap()
    .unwrap();
    assert_ne!(raw.decision.target_ticks(), 39_605_254);
    assert_eq!(ticks(&out[1]), 39_605_254);
    // ln(50000/40000) / 0.05 > 1, so the target saturates at +1.
    assert_eq!(ticks(&out[2]), SCALE);
    assert_prefix("S01", &spec, &bars);

    let h4 = params(&[("c", SCALE), ("interval", 240), ("n", 42)]);
    let mut wide = Vec::new();
    for index in 0..42 {
        wide.push(flat(index, BarInterval::H4, usdt(40_000)));
    }
    wide.push(flat(42, BarInterval::H4, usdt(50_000)));
    let out = replay("S01", &h4, &wide).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(ticks(&out[0]), SCALE);
}

#[test]
fn s02_exits_without_extending_and_flips_on_the_opposite_break() {
    let spec = params(&[("n", 2), ("hold_h", 2)]);
    let px = |index: i64, usdt_px: i64| flat(index, BarInterval::H1, usdt(usdt_px));
    let hold = vec![px(0, 100), px(1, 100), px(2, 120), px(3, 130), px(4, 125)];
    let out = replay("S02", &spec, &hold).unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(
        out.iter().map(ticks).collect::<Vec<_>>(),
        vec![SCALE, SCALE, 0]
    );
    for signal in &out {
        assert_clock(signal);
    }
    assert_prefix("S02", &spec, &hold);

    let flip = vec![px(0, 100), px(1, 100), px(2, 120), px(3, 70)];
    let out = replay("S02", &spec, &flip).unwrap();
    assert_eq!(
        out.iter().map(ticks).collect::<Vec<_>>(),
        vec![SCALE, -SCALE]
    );
}

#[test]
fn s03_requires_the_prior_vol_rank_and_does_not_flip() {
    let spec = params(&[("theta", 20_000_000), ("n", 24), ("hold_h", 12)]);
    let mut quiet = Vec::new();
    for index in 0..168 {
        let px = if index % 2 == 0 { usdt(100) } else { usdt(200) };
        quiet.push(flat(index, BarInterval::H1, px));
    }
    for index in 168..193 {
        quiet.push(flat(index, BarInterval::H1, usdt(150)));
    }
    quiet.push(ohlc(
        193,
        BarInterval::H1,
        usdt(150),
        usdt(200),
        usdt(150),
        usdt(200),
    ));
    quiet.push(ohlc(
        194,
        BarInterval::H1,
        usdt(150),
        usdt(150),
        usdt(100),
        usdt(100),
    ));
    let feature = VolRegime::<24, 168>::new(BarInterval::H1).unwrap();
    let rank = drive_feature(&feature, &quiet[..=192], at(192))
        .unwrap()
        .unwrap();
    assert_eq!(rank.value.ticks(), 0);
    let out = replay("S03", &spec, &quiet).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(ticks(&out[0]), SCALE);
    assert_eq!(ticks(&out[1]), SCALE);
    assert_prefix("S03", &spec, &quiet);

    let mut loud = Vec::new();
    for index in 0..193 {
        let px = if index % 2 == 0 { usdt(100) } else { usdt(200) };
        loud.push(flat(index, BarInterval::H1, px));
    }
    loud.push(ohlc(
        193,
        BarInterval::H1,
        usdt(200),
        usdt(400),
        usdt(200),
        usdt(400),
    ));
    let prior = drive_feature(&feature, &loud[..=192], at(192))
        .unwrap()
        .unwrap();
    assert!(prior.value.ticks() > 20_000_000);
    let out = replay("S03", &spec, &loud).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(ticks(&out[0]), 0);
}

#[test]
fn s05_fades_a_quiet_positive_extension() {
    // 708 alternating ×4 / ×4 log-returns, then 12 ×3 steps.
    // Historical 24-bar RV is log(4). The last window mixes log(4) and log(3),
    // so its RV is lower and p_t = 0. The 12-bar move is about 2.7 long-window
    // sigma, above k = 2, and the sign is positive, so the fade is short.
    let spec = params(&[("l", 12), ("k", 200_000_000), ("hold_h", 4)]);
    let mut price = 16_000_000_i64;
    let mut bars = Vec::with_capacity(721);
    bars.push(flat(0, BarInterval::H1, price));
    for step in 0..720 {
        if step >= 708 {
            price *= 3;
        } else if step % 2 == 0 {
            price *= 4;
        } else {
            price /= 4;
        }
        assert!(price > 0 && price <= (1_i64 << 53));
        let index = i64::from(step + 1);
        bars.push(flat(index, BarInterval::H1, price));
    }
    assert_eq!(bars.len(), 721);
    let out = replay("S05", &spec, &bars).unwrap();
    assert_eq!(out.len(), 1);
    assert_clock(&out[0]);
    assert_eq!(ticks(&out[0]), -SCALE);
    assert_prefix("S05", &spec, &bars);
}

#[test]
fn s06_signs_the_next_block_from_earlier_days_only() {
    // Two 00:00 bars, ln(2) and ln(4). t = 3 exactly in real arithmetic, so
    // |t| >= 1 and the sign is positive. The two 04:00 bars are the negatives,
    // so the next decision is short. Flat blocks have sd 0 and would be 0.
    let spec = params(&[("w_days", 2), ("k", SCALE)]);
    let mut bars = Vec::new();
    for day in 0..2 {
        let base = day * 6;
        if day == 0 {
            bars.push(ohlc(
                base,
                BarInterval::H4,
                usdt(100),
                usdt(200),
                usdt(100),
                usdt(200),
            ));
            bars.push(ohlc(
                base + 1,
                BarInterval::H4,
                usdt(200),
                usdt(200),
                usdt(100),
                usdt(100),
            ));
        } else {
            bars.push(ohlc(
                base,
                BarInterval::H4,
                usdt(100),
                usdt(400),
                usdt(100),
                usdt(400),
            ));
            bars.push(ohlc(
                base + 1,
                BarInterval::H4,
                usdt(400),
                usdt(400),
                usdt(100),
                usdt(100),
            ));
        }
        for slot in 2..6 {
            bars.push(flat(base + slot, BarInterval::H4, usdt(100)));
        }
    }
    bars.push(flat(12, BarInterval::H4, usdt(100)));
    let out = replay("S06", &spec, &bars).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(ticks(&out[0]), SCALE);
    assert_eq!(ticks(&out[1]), -SCALE);
    for signal in &out {
        assert_clock(signal);
    }
    assert_prefix("S06", &spec, &bars);
}

#[test]
fn warmup_and_the_earliest_read_are_enforced() {
    let spec = params(&[("n", 2), ("hold_h", 2)]);
    let early = 1_608_249_600_000_000 - 86_400_000_000;
    let mut bars = Vec::new();
    for index in 0..3 {
        let open = early + index * BarInterval::H1.micros();
        let px = usdt(100 + index * 20);
        bars.push(shift(open, px).unwrap());
    }
    let err = replay("S02", &spec, &bars).unwrap_err();
    assert_eq!(err, StrategyError::BeforeEarliest);

    let start = 1_608_249_600_000_000 + 86_400_000_000;
    let mut december = Vec::new();
    for index in 0..3 {
        let open = start + index * BarInterval::H1.micros();
        december.push(shift(open, usdt(100 + index * 50)).unwrap());
    }
    let out = replay("S02", &spec, &december).unwrap();
    assert!(out.is_empty());
}

#[test]
fn s01_deadband_uses_notional_over_equity() {
    let spec = params(&[("c", SCALE), ("interval", 60), ("n", 24)]);
    let mut bars = Vec::new();
    for index in 0..24 {
        bars.push(flat(index, BarInterval::H1, usdt(40_000)));
    }
    bars.push(flat(24, BarInterval::H1, usdt(40_800)));
    bars.push(flat(25, BarInterval::H1, usdt(40_900)));
    bars.push(flat(26, BarInterval::H1, usdt(50_000)));
    let flat_book = DecisionBook {
        equity_ticks: 1_000_000 * SCALE,
        position_lots: 0,
        mark_ticks: usdt(40_000),
    };
    let mut flat_weight = Strategy::from_params("S01", &spec).unwrap();
    let mut drifted = Vec::new();
    for bar in &bars {
        let mut book = flat_book;
        book.mark_ticks = bar.value().close().ticks();
        if let Some(signal) = flat_weight.push(bar, Some(book)).unwrap() {
            drifted.push(signal);
        }
    }
    assert_eq!(ticks(&drifted[0]), 39_605_254);
    let raw = drive(
        &Tsmom::<24>::new(BarInterval::H1, 0.05).unwrap(),
        &bars[..=25],
        at(25),
    )
    .unwrap()
    .unwrap();
    assert_eq!(ticks(&drifted[1]), raw.decision.target_ticks());
    assert_ne!(ticks(&drifted[1]), 39_605_254);

    let mark = bars[25].value().close().ticks();
    let equity = 1_000_000 * SCALE;
    let position =
        i64::try_from(i128::from(39_605_254) * i128::from(equity) / i128::from(mark)).unwrap();
    let mut held = Strategy::from_params("S01", &spec).unwrap();
    let mut outputs = Vec::new();
    for (index, bar) in bars.iter().enumerate() {
        let book = if index == 25 {
            DecisionBook {
                equity_ticks: equity,
                position_lots: position,
                mark_ticks: mark,
            }
        } else {
            DecisionBook {
                equity_ticks: equity,
                position_lots: 0,
                mark_ticks: bar.value().close().ticks(),
            }
        };
        if let Some(signal) = held.push(bar, Some(book)).unwrap() {
            outputs.push(signal);
        }
    }
    assert_eq!(ticks(&outputs[1]), 39_605_254);
    assert_eq!(ticks(&outputs[2]), SCALE);
}

#[test]
fn s02_refused_flip_goes_flat() {
    let spec = params(&[("n", 2), ("hold_h", 2)]);
    let holdout = 1_759_276_800_000_000i64;
    let hour = BarInterval::H1.micros();
    let opens = [
        holdout - 6 * hour,
        holdout - 5 * hour,
        holdout - 4 * hour,
        holdout - 3 * hour,
    ];
    let prices = [usdt(100), usdt(100), usdt(120), usdt(70)];
    let mut bars = Vec::new();
    for (open, px) in opens.iter().zip(prices) {
        bars.push(shift(*open, px).unwrap());
    }
    let out = replay("S02", &spec, &bars).unwrap();
    assert_eq!(out.iter().map(ticks).collect::<Vec<_>>(), vec![SCALE, 0]);
}

fn at(index: i64) -> DecisionTime {
    DecisionTime::from_micros(UtcMicros::from_micros(
        T0 + (index + 1) * BarInterval::H1.micros(),
    ))
}

fn shift(open_us: i64, px: i64) -> Result<Observation<ResampledBar>, String> {
    let price = Price::from_ticks(px).map_err(|err| err.to_string())?;
    let bar = ResampledBar::new(
        UtcMicros::from_micros(open_us),
        BarInterval::H1,
        price,
        price,
        price,
        price,
    )
    .map_err(|err| err.to_string())?;
    let available = open_us + BarInterval::H1.micros();
    Ok(Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(open_us)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(available)),
        },
        bar,
    ))
}
