//! Shared fixtures and the truncation checker.
//!
//! The proptest below samples the causal gate. It is not a proof.

use std::cell::RefCell;
use std::fmt;

use parangsae_core::{
    AvailableTs, BarInterval, DecisionTime, EventTs, Observation, Price, Ratio, ReceivedTs,
    ResampledBar, Timestamps, UtcMicros,
};
use parangsae_signals::{
    drive_funding_z, funding_observation, CausalFeature, CausalSignal, CausalWindow, Decision,
    FeatureOutput, FlatReason, FundingRow, FundingZ, SignalError, SignalOutput, Target, Tsmom,
    VolRegime,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence, TestCaseError, TestError, TestRunner};

pub const T0: i64 = 1_704_067_200_000_000;
pub const H1_US: i64 = 3_600_000_000;
pub const H4_US: i64 = 14_400_000_000;
pub const SLOT_US: i64 = 8 * H1_US;
pub const VISIBILITY_LAG_US: i64 = 60_000_000;

pub const TSMOM_REGRESSION: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/proptest-regressions/truncation/tsmom.txt"
);
pub const VOLREGIME_REGRESSION: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/proptest-regressions/truncation/volregime.txt"
);
pub const FUNDING_REGRESSION: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/proptest-regressions/truncation/funding_z.txt"
);

pub fn us(micros: i64) -> UtcMicros {
    UtcMicros::from_micros(micros)
}

pub fn at(micros: i64) -> DecisionTime {
    DecisionTime::from_micros(us(micros))
}

pub fn hour(h: i64) -> i64 {
    T0 + h * H1_US
}

pub fn usdt_ticks(usdt: i64) -> i64 {
    usdt * 100_000_000
}

pub fn observation<T>(event: i64, available: i64, value: T) -> Observation<T> {
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(us(event)),
            available_ts: AvailableTs::from_micros(us(available)),
            received_ts: ReceivedTs::from_micros(us(available)),
        },
        value,
    )
}

pub fn avail_us<T>(row: &Observation<T>) -> i64 {
    row.timestamps().available_ts.micros().micros()
}

pub fn restamp<T: Copy>(row: &Observation<T>, event: i64, available: i64) -> Observation<T> {
    observation(event, available, *row.value())
}

pub fn bar_obs(open_us: i64, interval: BarInterval, close_ticks: i64) -> Observation<ResampledBar> {
    let price = Price::from_ticks(close_ticks).expect("price ticks");
    let bar =
        ResampledBar::new(us(open_us), interval, price, price, price, price).expect("aligned bar");
    observation(open_us, open_us + interval.micros(), bar)
}

pub fn bar_ending_at(
    available: i64,
    interval: BarInterval,
    close_ticks: i64,
) -> Observation<ResampledBar> {
    bar_obs(available - interval.micros(), interval, close_ticks)
}

pub fn fund_obs(funding_us: i64, rate_ticks: i64, interval_hours: u8) -> Observation<FundingRow> {
    funding_observation(
        us(funding_us),
        Ratio::from_ticks(rate_ticks),
        interval_hours,
        ReceivedTs::from_micros(us(funding_us)),
    )
    .expect("funding visibility fits")
}

pub fn fund_custom(
    funding_us: i64,
    rate_ticks: i64,
    interval_hours: u8,
    lag_us: i64,
) -> Observation<FundingRow> {
    let row = FundingRow::new(
        us(funding_us),
        Ratio::from_ticks(rate_ticks),
        interval_hours,
    );
    observation(funding_us, funding_us + lag_us, row)
}

pub fn toy_tsmom_bars() -> Vec<Observation<ResampledBar>> {
    let closes = [40_000, 40_400, 40_200, 41_000, 39_000, 45_000];
    closes
        .into_iter()
        .enumerate()
        .map(|(index, px)| {
            let open = hour(i64::try_from(index).expect("index"));
            bar_obs(open, BarInterval::H1, usdt_ticks(px))
        })
        .collect()
}

pub fn toy_vol_bars(close_ticks: &[i64]) -> Vec<Observation<ResampledBar>> {
    close_ticks
        .iter()
        .enumerate()
        .map(|(index, ticks)| {
            let open = hour(i64::try_from(index).expect("index"));
            bar_obs(open, BarInterval::H1, *ticks)
        })
        .collect()
}

pub fn toy_funding() -> Vec<Observation<FundingRow>> {
    let rows = [
        (0, 10_000),
        (8, 10_000),
        (16, 20_000),
        (24, 40_000),
        (32, -10_000),
    ];
    rows.into_iter()
        .map(|(h, rate)| fund_obs(hour(h), rate, 8))
        .collect()
}

pub fn gap_funding() -> Vec<Observation<FundingRow>> {
    let rows = [
        (0, 10_000),
        (8, 10_000),
        (24, 40_000),
        (32, -10_000),
        (40, 20_000),
        (48, 30_000),
    ];
    rows.into_iter()
        .map(|(h, rate)| fund_obs(hour(h), rate, 8))
        .collect()
}

pub fn ramp_bars(
    interval: BarInterval,
    lookback: i64,
    first: i64,
    last: i64,
) -> (Vec<Observation<ResampledBar>>, DecisionTime) {
    let mut bars = Vec::new();
    for index in 0..=lookback {
        let ticks = if index == lookback { last } else { first };
        let open = T0 + index * interval.micros();
        bars.push(bar_obs(open, interval, ticks));
    }
    let decision = at(T0 + (lookback + 1) * interval.micros());
    (bars, decision)
}

pub fn contiguous_bars(
    interval: BarInterval,
    count: i64,
    close_ticks: impl Fn(i64) -> i64,
) -> Vec<Observation<ResampledBar>> {
    (0..count)
        .map(|index| {
            let open = T0 + index * interval.micros();
            bar_obs(open, interval, close_ticks(index))
        })
        .collect()
}

/// PROD tsmom witness: 40 H1 bars, decision on bar 30, so the cap is full and
/// later bars sit in the future.
pub fn tsmom_prod_witness() -> (Vec<Observation<ResampledBar>>, DecisionTime) {
    let bars = contiguous_bars(BarInterval::H1, 40, |index| {
        4_000_000_000_000 + index * 1_000_000
    });
    (bars, at(hour(30)))
}

/// PROD volregime witness on H1. The random generator stops at 200 bars, below
/// the H4 cap of 223, so the H4 emit is pinned by the warmup test.
pub fn volregime_prod_witness() -> (Vec<Observation<ResampledBar>>, DecisionTime) {
    let bars = contiguous_bars(BarInterval::H1, 205, |index| {
        4_000_000_000_000 + index * 50_000_000
    });
    (bars, at(hour(198)))
}

/// PROD funding witness: 95 slots and an H1 decision on bar 713.
pub fn funding_prod_witness() -> (
    Vec<Observation<ResampledBar>>,
    Vec<Observation<FundingRow>>,
    DecisionTime,
) {
    let bars = contiguous_bars(BarInterval::H1, 730, |index| {
        4_000_000_000_000 + index * 1_000
    });
    let funding = (0..95)
        .map(|slot| fund_obs(T0 + slot * SLOT_US, slot * 1_000 - 5_000, 8))
        .collect();
    (bars, funding, at(hour(713)))
}

pub fn expect_target(got: Result<Option<SignalOutput>, SignalError>, ticks: i64) {
    match got {
        Ok(Some(out)) => {
            assert_eq!(
                out.decision_ts.micros().micros(),
                out.bar_close_time_us.micros() + 1_000,
                "{out:?}"
            );
            match out.decision {
                Decision::Target(target) => assert_eq!(target.ticks(), ticks, "{out:?}"),
                Decision::Flat(reason) => {
                    panic!("expected target {ticks}, got flat {reason:?}: {out:?}")
                }
            }
        }
        other => panic!("expected target {ticks}, got {other:?}"),
    }
}

pub fn expect_flat(got: Result<Option<SignalOutput>, SignalError>, reason: FlatReason) {
    match got {
        Ok(Some(out)) => {
            assert_eq!(out.decision, Decision::Flat(reason), "{out:?}");
            assert_eq!(out.decision.position_ticks(), 0);
        }
        other => panic!("expected flat {reason:?}, got {other:?}"),
    }
}

pub fn expect_none(got: Result<Option<SignalOutput>, SignalError>) {
    assert_eq!(got, Ok(None));
}

pub fn expect_feature(got: Result<Option<FeatureOutput>, SignalError>, ticks: i64) {
    match got {
        Ok(Some(out)) => {
            assert_eq!(
                out.decision_ts.micros().micros(),
                out.bar_close_time_us.micros() + 1_000
            );
            assert_eq!(out.value.ticks(), ticks, "{out:?}");
        }
        other => panic!("expected rank {ticks}, got {other:?}"),
    }
}

#[derive(Debug)]
pub struct Mismatch {
    pub family: &'static str,
    pub decision_us: i64,
    pub full: String,
    pub reference: String,
}

pub fn case_count() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(256)
        .max(256)
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, span: u64) -> u64 {
        self.next_u64() % span
    }
}

fn runner(path: &'static str) -> TestRunner {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).expect("regression directory");
    }
    TestRunner::new(Config {
        cases: case_count(),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(path))),
        ..Config::default()
    })
}

fn finish<T: PartialEq + fmt::Debug>(
    family: &'static str,
    decision: DecisionTime,
    full: Result<Option<T>, SignalError>,
    reference: Result<Option<T>, SignalError>,
) -> Result<(), Mismatch> {
    if full == reference {
        Ok(())
    } else {
        Err(Mismatch {
            family,
            decision_us: decision.micros().micros(),
            full: format!("{full:?}"),
            reference: format!("{reference:?}"),
        })
    }
}

fn skipped(
    family: &'static str,
    decision: DecisionTime,
    require_all: bool,
) -> Result<(), Mismatch> {
    if require_all {
        Err(Mismatch {
            family,
            decision_us: decision.micros().micros(),
            full: "skipped".to_string(),
            reference: "witness must exercise this family".to_string(),
        })
    } else {
        Ok(())
    }
}

fn truncate<T: Clone>(rows: &[Observation<T>], decision_us: i64) -> Vec<Observation<T>> {
    rows.iter()
        .filter(|row| avail_us(row) <= decision_us)
        .cloned()
        .collect()
}

fn sort_bars(bars: &mut [Observation<ResampledBar>]) {
    bars.sort_by(|left, right| {
        avail_us(left)
            .cmp(&avail_us(right))
            .then_with(|| left.value().open_time().cmp(&right.value().open_time()))
    });
}

fn sort_funding(rows: &mut [Observation<FundingRow>]) {
    rows.sort_by(|left, right| {
        avail_us(left).cmp(&avail_us(right)).then_with(|| {
            left.value()
                .funding_time()
                .cmp(&right.value().funding_time())
        })
    });
}

fn append_bar(
    bars: &[Observation<ResampledBar>],
    decision_us: i64,
    interval: BarInterval,
) -> Observation<ResampledBar> {
    let step = interval.micros();
    let mut open = bars
        .iter()
        .map(|row| row.value().open_time().micros())
        .max()
        .map_or(T0, |last| last + step);
    while open + step <= decision_us {
        open += step;
    }
    bar_obs(open, interval, 1_000_000_000)
}

fn append_funding(rows: &[Observation<FundingRow>], decision_us: i64) -> Observation<FundingRow> {
    let mut funding_time = rows
        .iter()
        .map(|row| row.value().funding_time().micros())
        .max()
        .map_or(T0, |last| last + SLOT_US);
    let remainder = funding_time % SLOT_US;
    if remainder != 0 {
        funding_time += SLOT_US - remainder;
    }
    loop {
        if funding_time + VISIBILITY_LAG_US > decision_us {
            return fund_custom(funding_time, 12_345, 8, VISIBILITY_LAG_US);
        }
        funding_time += SLOT_US;
    }
}

fn bar_families<T, Drive>(
    bars: &[Observation<ResampledBar>],
    decision: DecisionTime,
    interval: BarInterval,
    drive_at: &mut Drive,
    require_all: bool,
) -> Result<(), Mismatch>
where
    T: PartialEq + fmt::Debug,
    Drive: FnMut(&[Observation<ResampledBar>], DecisionTime) -> Result<Option<T>, SignalError>,
{
    let decision_us = decision.micros().micros();
    let future = bars.iter().position(|row| avail_us(row) > decision_us);

    if let Some(index) = future {
        let mut modified = bars.to_vec();
        let current = *modified[index].value();
        let ticks = current.close().ticks();
        let next = if ticks > 1 { ticks - 1 } else { ticks + 1 };
        let price = Price::from_ticks(next).expect("bumped price");
        let rebuilt = ResampledBar::new(
            current.open_time(),
            current.interval(),
            price,
            price,
            price,
            price,
        )
        .expect("rebuilt bar");
        modified[index] = modified[index].with_value(rebuilt);
        sort_bars(&mut modified);
        finish(
            "ModifyFuture",
            decision,
            drive_at(&modified, decision),
            drive_at(&truncate(&modified, decision_us), decision),
        )?;
    } else {
        skipped("ModifyFuture", decision, require_all)?;
    }

    if let Some(index) = future {
        let mut modified = bars.to_vec();
        modified.remove(index);
        finish(
            "DeleteFuture",
            decision,
            drive_at(&modified, decision),
            drive_at(&truncate(&modified, decision_us), decision),
        )?;
    } else {
        skipped("DeleteFuture", decision, require_all)?;
    }

    let mut appended = bars.to_vec();
    appended.push(append_bar(bars, decision_us, interval));
    sort_bars(&mut appended);
    finish(
        "AppendFuture",
        decision,
        drive_at(&appended, decision),
        drive_at(&truncate(&appended, decision_us), decision),
    )?;

    let mut late = bars.to_vec();
    let price = Price::from_ticks(1_000_000_000).expect("late price");
    let bar = ResampledBar::new(us(T0), interval, price, price, price, price).expect("late bar");
    late.push(observation(decision_us, decision_us + 1, bar));
    sort_bars(&mut late);
    finish(
        "LateArrival",
        decision,
        drive_at(&late, decision),
        drive_at(&truncate(&late, decision_us), decision),
    )?;

    if let Some(index) = future {
        let mut modified = bars.to_vec();
        let event = modified[index].timestamps().event_ts.micros().micros();
        let available = avail_us(&modified[index]) + interval.micros();
        modified[index] = restamp(&modified[index], event, available);
        sort_bars(&mut modified);
        finish(
            "DelayFuture",
            decision,
            drive_at(&modified, decision),
            drive_at(&truncate(&modified, decision_us), decision),
        )?;
    } else {
        skipped("DelayFuture", decision, require_all)?;
    }
    Ok(())
}

fn funding_families<Drive>(
    bars: &[Observation<ResampledBar>],
    funding: &[Observation<FundingRow>],
    decision: DecisionTime,
    drive_at: &mut Drive,
    require_all: bool,
) -> Result<(), Mismatch>
where
    Drive: FnMut(
        &[Observation<ResampledBar>],
        &[Observation<FundingRow>],
        DecisionTime,
    ) -> Result<Option<SignalOutput>, SignalError>,
{
    let decision_us = decision.micros().micros();
    let future = funding.iter().position(|row| avail_us(row) > decision_us);
    let bars_now = truncate(bars, decision_us);

    if let Some(index) = future {
        let mut modified = funding.to_vec();
        let row = *modified[index].value();
        let ticks = row.rate().ticks();
        let bumped = if ticks == i64::MAX {
            ticks - 1
        } else {
            ticks + 1
        };
        modified[index] = modified[index].with_value(row.with_rate(Ratio::from_ticks(bumped)));
        sort_funding(&mut modified);
        finish(
            "ModifyFuture",
            decision,
            drive_at(bars, &modified, decision),
            drive_at(&bars_now, &truncate(&modified, decision_us), decision),
        )?;
    } else {
        skipped("ModifyFuture", decision, require_all)?;
    }

    if let Some(index) = future {
        let mut modified = funding.to_vec();
        modified.remove(index);
        finish(
            "DeleteFuture",
            decision,
            drive_at(bars, &modified, decision),
            drive_at(&bars_now, &truncate(&modified, decision_us), decision),
        )?;
    } else {
        skipped("DeleteFuture", decision, require_all)?;
    }

    let mut appended = funding.to_vec();
    appended.push(append_funding(funding, decision_us));
    sort_funding(&mut appended);
    finish(
        "AppendFuture",
        decision,
        drive_at(bars, &appended, decision),
        drive_at(&bars_now, &truncate(&appended, decision_us), decision),
    )?;

    let mut late = funding.to_vec();
    let funding_time = if decision_us % SLOT_US == 0 {
        decision_us - 1
    } else {
        decision_us
    };
    let row = FundingRow::new(us(funding_time), Ratio::from_ticks(1), 8);
    late.push(observation(funding_time, decision_us + 1, row));
    sort_funding(&mut late);
    finish(
        "LateArrival",
        decision,
        drive_at(bars, &late, decision),
        drive_at(&bars_now, &truncate(&late, decision_us), decision),
    )?;

    if let Some(index) = future {
        let mut modified = funding.to_vec();
        let event = modified[index].timestamps().event_ts.micros().micros();
        let available = avail_us(&modified[index]) + SLOT_US;
        modified[index] = restamp(&modified[index], event, available);
        sort_funding(&mut modified);
        finish(
            "DelayFuture",
            decision,
            drive_at(bars, &modified, decision),
            drive_at(&bars_now, &truncate(&modified, decision_us), decision),
        )?;
    } else {
        skipped("DelayFuture", decision, require_all)?;
    }
    Ok(())
}

fn gen_bars(seed: u64, interval: BarInterval) -> (Vec<Observation<ResampledBar>>, DecisionTime) {
    let mut rng = Rng::new(seed);
    let count = rng.below(201);
    let mut close = rng.below((1 << 53) - 2) + 1;
    let mut bars = Vec::new();
    for index in 0..count {
        close = (close + rng.below(1_000_000) + 1).min((1 << 53) - 1);
        let open = T0 + i64::try_from(index).expect("index") * interval.micros();
        let ticks = i64::try_from(close).expect("close ticks");
        bars.push(bar_obs(open, interval, ticks));
    }
    let decision = pick_decision(&bars, &mut rng, interval);
    (bars, decision)
}

fn pick_decision(
    bars: &[Observation<ResampledBar>],
    rng: &mut Rng,
    interval: BarInterval,
) -> DecisionTime {
    if bars.is_empty() {
        return at(T0);
    }
    let kind = rng.below(10);
    let index = usize::try_from(rng.below(u64::try_from(bars.len()).expect("len"))).expect("index");
    let available = avail_us(&bars[index]);
    let choice = match kind {
        0..=5 => available,
        6 => available - 1,
        7 => available + 1,
        8 => avail_us(&bars[0]) - 1,
        _ => avail_us(bars.last().expect("last bar")) + interval.micros(),
    };
    at(choice)
}

fn gen_funding(
    seed: u64,
) -> (
    Vec<Observation<ResampledBar>>,
    Vec<Observation<FundingRow>>,
    DecisionTime,
) {
    let (bars, decision) = gen_bars(seed, BarInterval::H1);
    let mut rng = Rng::new(seed ^ 0xA5A5_A5A5_A5A5_A5A5);
    let slots = rng.below(121);
    let mut funding = Vec::new();
    for slot in 0..slots {
        if rng.below(20) == 0 {
            continue;
        }
        let lag = match rng.below(4) {
            0 => 0,
            1 => 1,
            2 => VISIBILITY_LAG_US,
            _ => SLOT_US,
        };
        let interval_hours: u8 = if rng.below(100) == 0 { 4 } else { 8 };
        let rate = i64::try_from(rng.below(2_000_001)).expect("rate") - 1_000_000;
        let funding_time = T0 + i64::try_from(slot).expect("slot") * SLOT_US;
        funding.push(fund_custom(funding_time, rate, interval_hours, lag));
    }
    sort_funding(&mut funding);
    (bars, funding, decision)
}

fn run_seeds(
    path: &'static str,
    check: impl Fn(u64) -> Result<(), Mismatch>,
) -> Result<(), Mismatch> {
    let caught = RefCell::new(None);
    let mut runner = runner(path);
    let strategy = any::<u64>();
    let outcome = runner.run(&strategy, |seed| match check(seed) {
        Ok(()) => Ok(()),
        Err(mismatch) => {
            *caught.borrow_mut() = Some(mismatch);
            Err(TestCaseError::fail("truncation mismatch"))
        }
    });
    match outcome {
        Ok(()) => Ok(()),
        Err(TestError::Fail(_, _)) => match caught.borrow_mut().take() {
            Some(mismatch) => Err(mismatch),
            None => panic!("proptest reported a failure without a mismatch"),
        },
        Err(other) => panic!("proptest aborted: {other:?}"),
    }
}

pub fn check_bar_truncation<T, Drive>(
    regression: &'static str,
    interval: BarInterval,
    witness: &[Observation<ResampledBar>],
    witness_d: DecisionTime,
    mut drive_at: Drive,
) -> Result<(), Mismatch>
where
    T: PartialEq + fmt::Debug,
    Drive: FnMut(&[Observation<ResampledBar>], DecisionTime) -> Result<Option<T>, SignalError>,
{
    bar_families(witness, witness_d, interval, &mut drive_at, true)?;
    let drive_at = RefCell::new(drive_at);
    run_seeds(regression, |seed| {
        let (bars, decision) = gen_bars(seed, interval);
        let mut drive_at = drive_at.borrow_mut();
        bar_families(&bars, decision, interval, &mut *drive_at, false)
    })
}

pub fn check_funding_truncation<Drive>(
    regression: &'static str,
    bars: &[Observation<ResampledBar>],
    funding: &[Observation<FundingRow>],
    witness_d: DecisionTime,
    mut drive_at: Drive,
) -> Result<(), Mismatch>
where
    Drive: FnMut(
        &[Observation<ResampledBar>],
        &[Observation<FundingRow>],
        DecisionTime,
    ) -> Result<Option<SignalOutput>, SignalError>,
{
    funding_families(bars, funding, witness_d, &mut drive_at, true)?;
    let drive_at = RefCell::new(drive_at);
    run_seeds(regression, |seed| {
        let (bars, funding, decision) = gen_funding(seed);
        let mut drive_at = drive_at.borrow_mut();
        funding_families(&bars, &funding, decision, &mut *drive_at, false)
    })
}

pub fn leaky_next_bar_signal<S>(
    signal: &S,
    rows: &[Observation<S::Row>],
    decision: DecisionTime,
    interval_us: i64,
) -> Result<Option<SignalOutput>, SignalError>
where
    S: CausalSignal,
{
    let leaked = at(decision.micros().micros() + interval_us);
    let views = rows.iter().filter_map(|row| row.causal_at(leaked));
    let window = CausalWindow::from_views(leaked, S::WINDOW_CAP, views)?;
    signal.evaluate(&window)
}

pub fn leaky_next_bar_feature<F>(
    feature: &F,
    rows: &[Observation<F::Row>],
    decision: DecisionTime,
    interval_us: i64,
) -> Result<Option<FeatureOutput>, SignalError>
where
    F: CausalFeature,
{
    let leaked = at(decision.micros().micros() + interval_us);
    let views = rows.iter().filter_map(|row| row.causal_at(leaked));
    let window = CausalWindow::from_views(leaked, F::WINDOW_CAP, views)?;
    feature.evaluate(&window)
}

pub fn leaky_funding_horizon<const M: u16>(
    signal: &FundingZ<M>,
    bars: &[Observation<ResampledBar>],
    funding: &[Observation<FundingRow>],
    decision: DecisionTime,
) -> Result<Option<SignalOutput>, SignalError> {
    let horizon = at(decision.micros().micros() + SLOT_US);
    let bar_views = bars.iter().filter_map(|row| row.causal_at(decision));
    let fund_views = funding.iter().filter_map(|row| row.causal_at(horizon));
    let bar_window = CausalWindow::from_views(decision, FundingZ::<M>::BAR_CAP, bar_views)?;
    let fund_window = CausalWindow::from_views(horizon, FundingZ::<M>::FUNDING_CAP, fund_views)?;
    signal.evaluate(&bar_window, &fund_window)
}

/// Restamp rows whose event is already known but whose availability is still
/// in the future, then drive at `decision`.
pub fn drive_event_gate<const M: u16>(
    signal: &FundingZ<M>,
    bars: &[Observation<ResampledBar>],
    funding: &[Observation<FundingRow>],
    decision: DecisionTime,
) -> Result<Option<SignalOutput>, SignalError> {
    let decision_us = decision.micros().micros();
    let mut restamped = Vec::with_capacity(funding.len());
    for row in funding {
        let event = row.timestamps().event_ts.micros().micros();
        let available = avail_us(row);
        if event <= decision_us && decision_us < available {
            restamped.push(restamp(row, event, event));
        } else {
            restamped.push(row.clone());
        }
    }
    sort_funding(&mut restamped);
    drive_funding_z(signal, bars, &restamped, decision)
}

/// Name every harness entry point so each test crate keeps this module live.
pub fn anchors() {
    std::hint::black_box(T0);
    std::hint::black_box(H1_US);
    std::hint::black_box(H4_US);
    std::hint::black_box(SLOT_US);
    std::hint::black_box(VISIBILITY_LAG_US);
    std::hint::black_box(TSMOM_REGRESSION);
    std::hint::black_box(VOLREGIME_REGRESSION);
    std::hint::black_box(FUNDING_REGRESSION);
    std::hint::black_box(us as fn(i64) -> UtcMicros);
    std::hint::black_box(at as fn(i64) -> DecisionTime);
    std::hint::black_box(hour as fn(i64) -> i64);
    std::hint::black_box(usdt_ticks as fn(i64) -> i64);
    std::hint::black_box(observation::<()> as fn(i64, i64, ()) -> Observation<()>);
    std::hint::black_box(avail_us::<()> as fn(&Observation<()>) -> i64);
    std::hint::black_box(restamp::<i64> as fn(&Observation<i64>, i64, i64) -> Observation<i64>);
    std::hint::black_box(bar_obs as fn(i64, BarInterval, i64) -> Observation<ResampledBar>);
    std::hint::black_box(bar_ending_at as fn(i64, BarInterval, i64) -> Observation<ResampledBar>);
    std::hint::black_box(fund_obs as fn(i64, i64, u8) -> Observation<FundingRow>);
    std::hint::black_box(fund_custom as fn(i64, i64, u8, i64) -> Observation<FundingRow>);
    std::hint::black_box(toy_tsmom_bars as fn() -> Vec<Observation<ResampledBar>>);
    std::hint::black_box(toy_vol_bars as fn(&[i64]) -> Vec<Observation<ResampledBar>>);
    std::hint::black_box(toy_funding as fn() -> Vec<Observation<FundingRow>>);
    std::hint::black_box(gap_funding as fn() -> Vec<Observation<FundingRow>>);
    std::hint::black_box(
        ramp_bars
            as fn(BarInterval, i64, i64, i64) -> (Vec<Observation<ResampledBar>>, DecisionTime),
    );
    std::hint::black_box(contiguous_bars as fn(BarInterval, i64, fn(i64) -> i64) -> _);
    std::hint::black_box(tsmom_prod_witness as fn() -> _);
    std::hint::black_box(volregime_prod_witness as fn() -> _);
    std::hint::black_box(funding_prod_witness as fn() -> _);
    std::hint::black_box(expect_target as fn(_, _) -> _);
    std::hint::black_box(expect_flat as fn(_, _) -> _);
    std::hint::black_box(expect_none as fn(_) -> _);
    std::hint::black_box(expect_feature as fn(_, _) -> _);
    std::hint::black_box(case_count as fn() -> u32);
    std::hint::black_box(
        check_bar_truncation::<
            (),
            fn(&[Observation<ResampledBar>], DecisionTime) -> Result<Option<()>, SignalError>,
        >,
    );
    std::hint::black_box(
        check_funding_truncation::<
            fn(
                &[Observation<ResampledBar>],
                &[Observation<FundingRow>],
                DecisionTime,
            ) -> Result<Option<SignalOutput>, SignalError>,
        >,
    );
    std::hint::black_box(leaky_next_bar_signal::<Tsmom<1>>);
    std::hint::black_box(leaky_next_bar_feature::<VolRegime<1, 1>>);
    std::hint::black_box(leaky_funding_horizon::<2>);
    std::hint::black_box(drive_event_gate::<2>);
    let mismatch = Mismatch {
        family: "anchor",
        decision_us: 0,
        full: String::new(),
        reference: String::new(),
    };
    std::hint::black_box(mismatch.family);
    std::hint::black_box(mismatch.decision_us);
    std::hint::black_box(mismatch.full);
    std::hint::black_box(mismatch.reference);
    std::hint::black_box(Decision::Target(Target::FLAT));
}

#[cfg(test)]
#[test]
fn harness_entry_points_are_linked() {
    anchors();
}
