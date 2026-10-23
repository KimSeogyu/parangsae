//! Shared v1a tapes. The cucumber harness and the unit tests each call a subset.

#![allow(dead_code)]

use crate::support::{at, bar_at, cfg, costs, px, series, stamp, tape_marked, tape_of};
use parangsae_backtest::fixture::AlwaysFlat;
use parangsae_backtest::{
    funding_visible, run, run_targets, Decision, EngineError, FlatReason, FundingObs, FundingStamp,
    MarkAbsence, Report, SignalOutput, TargetSignal, UtcDate, UtcRange,
};
use parangsae_core::{Causal, DecisionTime, Qty, Ratio, ResampledBar, UtcMicros};
use parangsae_signals::Target;

const MINUTE_US: i64 = 60_000_000;

#[derive(Debug)]
pub struct Script {
    steps: Vec<Option<Decision>>,
    cursor: usize,
    pub bars_seen: Vec<Vec<i64>>,
    pub funding_seen: Vec<Vec<(i64, i64)>>,
}

impl Script {
    pub fn new(steps: Vec<Option<Decision>>) -> Self {
        Self {
            steps,
            cursor: 0,
            bars_seen: Vec::new(),
            funding_seen: Vec::new(),
        }
    }
}

impl TargetSignal for Script {
    fn on_bar(
        &mut self,
        view: Causal<'_, &[ResampledBar]>,
        funding: Causal<'_, &[FundingObs]>,
    ) -> Option<SignalOutput> {
        let bars = *view.value();
        let funding_rows = *funding.value();
        self.bars_seen
            .push(bars.iter().map(|bar| bar.close().ticks()).collect());
        self.funding_seen.push(
            funding_rows
                .iter()
                .map(|row| (row.funding_time.micros(), row.rate.ticks()))
                .collect(),
        );
        let step = self.steps.get(self.cursor).copied().flatten();
        self.cursor += 1;
        let hour = bars.last()?;
        let decision_ts = hour.close_time().checked_add_micros(1_000)?;
        step.map(|decision| SignalOutput {
            decision_ts: DecisionTime::from_micros(decision_ts),
            bar_close_time_us: hour.close_time(),
            interval: hour.interval(),
            decision,
        })
    }
}

pub fn on(date: &str, hour: i64, minute: i64) -> i64 {
    UtcDate::parse(date).expect("date").start_us().micros() + (hour * 60 + minute) * MINUTE_US
}

pub fn tgt(fraction: f64) -> Decision {
    Decision::Target(Target::from_f64(fraction).expect("finite target"))
}

pub fn stale() -> Decision {
    Decision::Flat(FlatReason::FundingStale)
}

pub fn slot_missing() -> Decision {
    Decision::Flat(FlatReason::FundingSlotMissing)
}

fn hours(start: i64, count: usize, usdt: i64) -> Vec<parangsae_core::Bar> {
    series(start, &vec![(usdt, usdt); count * 60])
}

fn stamp_at(funding_us: i64, available_us: i64, rate: i64) -> FundingStamp {
    FundingStamp {
        funding_time: UtcMicros::from_micros(funding_us),
        available: UtcMicros::from_micros(available_us),
        rate: Ratio::from_ticks(rate),
        interval_hours: 8,
    }
}

pub fn execute(
    bars: Vec<parangsae_core::Bar>,
    marks: Vec<parangsae_core::Bar>,
    funding: Vec<FundingStamp>,
    steps: Vec<Option<Decision>>,
    cash_usdt: i64,
    mark_gaps: Vec<MarkAbsence>,
    forced_flat: Vec<UtcRange>,
) -> Result<(Report, Script), EngineError> {
    let mut config = cfg(&bars, cash_usdt);
    config.mark_gaps = mark_gaps;
    config.forced_flat = forced_flat;
    let tape = tape_marked(bars, marks, funding);
    let mut signal = Script::new(steps);
    let report = run_targets(&tape, &mut signal, &costs(0, 0), &config)?;
    Ok((report, signal))
}

pub fn t1_repeat() -> Report {
    let start = at(12, 0);
    let bars = hours(start, 2, 50_000);
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![Some(tgt(0.5)), Some(tgt(0.5))],
        10_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("repeated target")
    .0
}

pub fn t1_flip() -> Report {
    let start = at(12, 0);
    let bars = hours(start, 3, 50_000);
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![Some(tgt(0.5)), Some(tgt(-0.5)), Some(tgt(-0.5))],
        10_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("flip")
    .0
}

pub fn t2_abort() -> EngineError {
    let start = at(12, 0);
    let bars = hours(start, 4, 100);
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![None, None, Some(tgt(0.0)), None],
        1_000,
        Vec::new(),
        Vec::new(),
    )
    .expect_err("missing signal")
}

pub fn t2_warmup() -> Report {
    let start = at(12, 0);
    let bars = hours(start, 3, 100);
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![None, None, None],
        1_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("warmup")
    .0
}

pub fn t3() -> Report {
    let start = at(0, 0);
    let bars: Vec<_> = (0..120)
        .map(|index| {
            let open_usdt = match index {
                60 => 100,
                61 => 101,
                _ => 90,
            };
            bar_at(start + index * MINUTE_US, px(open_usdt), px(open_usdt))
        })
        .collect();
    let marks = bars
        .iter()
        .map(|bar| bar_at(bar.open_time.micros(), px(50_000), px(50_000)))
        .collect();
    execute(
        bars,
        marks,
        vec![stamp(start, 0)],
        vec![Some(tgt(1.0)), Some(stale())],
        10_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("t3")
    .0
}

pub fn t5() -> (Report, Script) {
    let start = at(6, 0);
    let bars = hours(start, 3, 100);
    let slot = at(8, 0);
    execute(
        bars.clone(),
        bars,
        vec![stamp_at(slot, slot + MINUTE_US, 10_000)],
        vec![Some(tgt(1.0)), Some(stale()), Some(stale())],
        100,
        Vec::new(),
        Vec::new(),
    )
    .expect("t5")
}

/// `(hidden at 08:00:30, visible at 08:01:00)` for the 08:00 funding row.
pub fn t5_flags() -> (bool, bool) {
    let slot = at(8, 0);
    let rows = [FundingObs {
        funding_time: UtcMicros::from_micros(slot),
        available: UtcMicros::from_micros(slot + MINUTE_US),
        rate: Ratio::from_ticks(10_000),
    }];
    let hidden = funding_visible(&rows, UtcMicros::from_micros(slot + 30_000_000)).is_empty();
    let visible = funding_visible(&rows, UtcMicros::from_micros(slot + MINUTE_US)).len() == 1;
    (hidden, visible)
}

pub fn t7(
    drop_from: usize,
    drop_to: usize,
    listed: bool,
    known: bool,
) -> Result<Report, EngineError> {
    let start = on("2024-08-12", 10, 0);
    let bars = hours(start, 1, 100);
    let first_open = bars[drop_from].open_time;
    let last_open = bars[drop_to].open_time;
    let marks = bars
        .iter()
        .filter(|bar| bar.open_time < first_open || bar.open_time > last_open)
        .copied()
        .collect();
    let gaps = if listed {
        vec![MarkAbsence {
            first_open,
            last_open,
            known_source_absence: known,
        }]
    } else {
        Vec::new()
    };
    execute(bars, marks, Vec::new(), vec![None], 1_000, gaps, Vec::new()).map(|(report, _)| report)
}

pub fn f1() -> Report {
    let start = at(12, 0);
    let bars = hours(start, 5, 100);
    execute(
        bars.clone(),
        bars,
        vec![stamp(at(16, 0), 0)],
        vec![
            Some(stale()),
            Some(stale()),
            Some(stale()),
            Some(slot_missing()),
            Some(slot_missing()),
        ],
        1_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("flat reasons")
    .0
}

pub fn f2(second: Decision) -> Report {
    let start = at(12, 0);
    let bars = hours(start, 3, 50_000);
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![Some(tgt(1.0)), Some(second), Some(tgt(0.0))],
        10_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("flat versus zero")
    .0
}

fn filler(bar: &mut parangsae_core::Bar) {
    bar.volume = Qty::from_ticks(0).expect("zero volume");
    bar.count = 0;
    bar.high = bar.open;
    bar.low = bar.open;
    bar.close = bar.open;
}

pub fn z1() -> Report {
    let start = at(12, 0);
    let mut bars = hours(start, 2, 100);
    filler(&mut bars[60]);
    bars[61] = bar_at(bars[61].open_time.micros(), px(111), px(111));
    let marks = hours(start, 2, 50_000);
    execute(
        bars,
        marks,
        Vec::new(),
        vec![Some(tgt(1.0)), Some(stale())],
        10_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("z1")
    .0
}

pub fn z2() -> Report {
    let start = at(12, 0);
    let mut bars = hours(start, 2, 100);
    filler(&mut bars[60]);
    filler(&mut bars[61]);
    bars[62] = bar_at(bars[62].open_time.micros(), px(111), px(111));
    let marks = hours(start, 2, 50_000);
    execute(
        bars,
        marks,
        Vec::new(),
        vec![Some(tgt(1.0)), Some(stale())],
        10_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("z2")
    .0
}

pub fn z3_bars(mask: &[bool]) -> (Report, Vec<parangsae_core::Bar>) {
    let start = at(12, 0);
    let mut bars = hours(start, 2, 50_000);
    let marks = bars.clone();
    for (index, is_filler) in mask.iter().enumerate() {
        if *is_filler && index + 1 != bars.len() {
            filler(&mut bars[index]);
        }
    }
    let report = execute(
        bars.clone(),
        marks,
        Vec::new(),
        vec![Some(tgt(1.0)), Some(stale())],
        10_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("z3")
    .0;
    (report, bars)
}

pub fn ff1() -> Report {
    let start = at(12, 0);
    let bars = hours(start, 3, 50_000);
    let window = vec![UtcRange {
        start: UtcMicros::from_micros(at(14, 0)),
        end: UtcMicros::from_micros(at(16, 0)),
    }];
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![Some(tgt(1.0)); 3],
        10_000,
        Vec::new(),
        window,
    )
    .expect("ff1")
    .0
}

pub fn ff2() -> Report {
    let start = at(12, 0);
    let bars = hours(start, 4, 50_000);
    let window = vec![UtcRange {
        start: UtcMicros::from_micros(start),
        end: UtcMicros::from_micros(at(15, 0)),
    }];
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![Some(tgt(1.0)); 4],
        10_000,
        Vec::new(),
        window,
    )
    .expect("ff2")
    .0
}

pub fn e1() -> Report {
    let start = on("2025-09-30", 22, 0);
    let bars = hours(start, 2, 100);
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![Some(tgt(1.0)), Some(stale())],
        100,
        Vec::new(),
        Vec::new(),
    )
    .expect("e1")
    .0
}

pub fn h2() -> EngineError {
    let open = on("2025-10-01", 0, 0);
    let bars = vec![bar_at(open, px(100), px(100))];
    let tape = tape_of(bars.clone());
    let mut strategy = AlwaysFlat;
    run(&tape, &mut strategy, &costs(0, 0), &cfg(&bars, 1_000)).expect_err("holdout bar")
}

pub fn before_sample() -> EngineError {
    let open = on("2020-12-17", 0, 0);
    let bars = vec![bar_at(open, px(100), px(100))];
    execute(
        bars.clone(),
        bars,
        Vec::new(),
        vec![None],
        1_000,
        Vec::new(),
        Vec::new(),
    )
    .expect_err("before sample")
}

pub fn sample_open() -> Report {
    let start = on("2020-12-18", 0, 0);
    let bars = hours(start, 1, 100);
    execute(
        bars.clone(),
        bars,
        vec![stamp(start, 0)],
        vec![None],
        1_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("sample open")
    .0
}

pub fn p7_run(bars: Vec<parangsae_core::Bar>, funding: Vec<FundingStamp>) -> Script {
    execute(
        bars.clone(),
        bars,
        funding,
        vec![None, None],
        1_000,
        Vec::new(),
        Vec::new(),
    )
    .expect("p7")
    .1
}

pub fn p7_base() -> (Vec<parangsae_core::Bar>, Vec<FundingStamp>) {
    let start = at(12, 0);
    let bars = hours(start, 2, 100);
    let funding = vec![stamp_at(at(8, 0), at(13, 30), 1_000)];
    (bars, funding)
}
