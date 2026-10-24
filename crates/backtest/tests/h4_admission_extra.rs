//! Independent synthetic observers for the EL-selected A/S1-S5 contract.
//! Uses the agreed new public API; not compiled until Builder publishes it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cucumber::{then, when, World as _};
use parangsae_backtest::{
    run_targets, target_qty, Bar, BookState, EngineError, ForcedExit, ForcedExitReason, FundingObs,
    FundingStamp, MarkAbsence, Report, RunConfig, SignalOutput, Tape, Target, TargetSignal,
    UtcDate,
};
use parangsae_core::{
    AvailableTs, Causal, DecisionTime, EventTs, Interval, Observation, Price, Qty, Ratio,
    ReceivedTs, ResampledBar, Side, Timestamps, UtcMicros,
};
use parangsae_sim::{Cash, Costs, FeeBps, Ledger, MarketOrder, SimError, SlippageBps};
use parangsae_strategies::StrategyOutput;

const SCALE: i64 = 100_000_000;
const MINUTE: i64 = 60_000_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const HOLD168: i64 = 168 * HOUR;

fn ts(value: i64) -> UtcMicros {
    UtcMicros::from_micros(value)
}
fn date(value: &str) -> i64 {
    UtcDate::parse(value).unwrap().start_us().micros()
}
fn price(value: i64) -> Price {
    Price::from_ticks(value * SCALE).unwrap()
}
fn costs(fee: u32, slip: u32) -> Costs {
    Costs {
        taker_fee: FeeBps(fee),
        slippage: SlippageBps(slip),
    }
}
fn bar(open: i64, value: i64) -> Bar {
    Bar {
        open_time: ts(open),
        close_time: ts(open + MINUTE - 1_000),
        available: ts(open + MINUTE),
        open: price(value),
        high: price(value),
        low: price(value),
        close: price(value),
        volume: Qty::from_ticks(SCALE).unwrap(),
        count: 1,
    }
}
fn cfg(bars: &[Bar]) -> RunConfig {
    RunConfig {
        initial_cash: Cash(10_000 * SCALE),
        warmup_start: bars[0].open_time,
        start: bars[0].open_time,
        end: ts(bars.last().unwrap().open_time.micros() + MINUTE),
        holdout_start: ts(date("2025-10-01")),
        forced_flat: Vec::new(),
        flatten_before: None,
        mark_gaps: Vec::new(),
        interval: Interval::ONE_HOUR,
        lot_ticks: 100_000,
    }
}
fn emit(last: &ResampledBar, decision: i64, target: i64) -> StrategyOutput {
    StrategyOutput::target(SignalOutput {
        bar_close_time_us: last.close_time(),
        decision_ts: DecisionTime::from_micros(ts(decision)),
        interval: last.interval(),
        decision: parangsae_backtest::Decision::Target(Target::from_ticks(target).unwrap()),
        accept: 0,
    })
}

#[derive(Clone, Debug)]
struct Probe {
    at: i64,
    equity: i64,
    position: i64,
    requested_lots: i64,
    visible_slots: Vec<i64>,
}
#[derive(Clone, Copy)]
enum Script {
    Money,
    Ruin,
}
struct Recorder {
    slot: i64,
    script: Script,
    probes: Vec<Probe>,
}
impl TargetSignal for Recorder {
    fn decision_delay_us(&self) -> i64 {
        1_000
    }
    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        funding: Causal<'_, &[FundingObs]>,
        book: BookState,
    ) -> Option<StrategyOutput> {
        let last = bars.value().last().unwrap();
        let at = last.close_time().micros() + 2_000;
        self.probes.push(Probe {
            at,
            equity: book.equity_ticks(),
            position: book.position_lots(),
            requested_lots: target_qty(SCALE, book.equity_ticks(), book.mark_ticks(), 100_000)
                .unwrap(),
            visible_slots: funding
                .value()
                .iter()
                .map(|row| row.funding_time.micros())
                .collect(),
        });
        let target = match self.script {
            Script::Money if at < self.slot - HOUR => SCALE,
            Script::Money if at < self.slot + HOUR => 0,
            Script::Money => SCALE,
            Script::Ruin if at < self.slot => -SCALE,
            Script::Ruin => 0,
        };
        Some(emit(last, at, target))
    }
}

#[derive(Debug)]
struct Run {
    result: Result<Report, EngineError>,
    probes: Vec<Probe>,
    slot: i64,
}
impl Run {
    fn at(&self, at: i64) -> &Probe {
        self.probes.iter().find(|row| row.at == at).unwrap()
    }
    fn report(&self) -> &Report {
        self.result
            .as_ref()
            .expect("the valid active fixture completes")
    }
}

fn stamp(slot: i64, available: i64, rate: i64) -> FundingStamp {
    FundingStamp {
        funding_time: ts(slot),
        available: ts(available),
        rate: Ratio::from_ticks(rate),
        interval_hours: 8,
    }
}

fn money_run(kind: &str, variant: bool) -> Run {
    let equality = kind.ends_with(" equality");
    let kind = kind.strip_suffix(" equality").unwrap_or(kind);
    let t = date("2024-06-02");
    let long_history = kind == "nonmonotone";
    let start = t - if long_history { 12 * HOUR } else { 3 * HOUR };
    let end = t + 4 * HOUR;
    let probe1 = t + HOUR + 1_000;
    let probe2 = t + 2 * HOUR + 1_000;
    let mut bars: Vec<_> = (0..(end - start) / MINUTE)
        .map(|i| bar(start + i * MINUTE, 100))
        .collect();
    for row in &mut bars {
        if row.open_time.micros() >= t - HOUR + MINUTE && row.open_time.micros() < t {
            row.count = 0;
            row.volume = Qty::from_ticks(0).unwrap();
        }
    }
    let mut marks = bars.clone();
    let mut config = cfg(&bars);
    let mut funding = Vec::new();
    if long_history {
        funding.push(stamp(
            t - 8 * HOUR,
            probe2,
            if variant { -1_000_000 } else { 1_000_000 },
        ));
    }
    let rate = match kind {
        "nonmonotone" | "delayed mark" | "delayed carried mark" | "published carried mark" => {
            500_000
        }
        "delayed rate" => {
            if variant {
                -500_000
            } else {
                500_000
            }
        }
        "overflow rate" => {
            if variant {
                i64::MAX
            } else {
                0
            }
        }
        "overflow mark" => SCALE,
        _ => unreachable!(),
    };
    let rate_available = match kind {
        "nonmonotone" => probe1,
        "delayed rate" => {
            if equality {
                probe1
            } else {
                probe2
            }
        }
        _ => t + MINUTE,
    };
    funding.push(stamp(t, rate_available, rate));
    let mark_key = if kind.ends_with("carried mark") {
        t - MINUTE
    } else {
        t
    };
    if matches!(
        kind,
        "delayed mark" | "delayed carried mark" | "published carried mark" | "overflow mark"
    ) {
        let mark = marks
            .iter_mut()
            .find(|row| row.open_time.micros() == mark_key)
            .unwrap();
        mark.available = ts(if kind == "overflow mark" {
            t + MINUTE
        } else if kind == "published carried mark" {
            t
        } else if equality {
            probe1
        } else {
            probe2
        });
        mark.open = if kind == "overflow mark" {
            if variant {
                Price::from_ticks(i64::MAX).unwrap()
            } else {
                price(1)
            }
        } else {
            price(if variant { 200 } else { 100 })
        };
        mark.high = mark.open.max(mark.close);
        mark.low = mark.open.min(mark.close);
    }
    if kind.ends_with("carried mark") {
        marks.retain(|row| row.open_time.micros() != t);
        config.mark_gaps.push(MarkAbsence {
            first_open: ts(t),
            last_open: ts(t),
            known_source_absence: true,
        });
    }
    let tape = Tape::from_parts(bars, marks, funding);
    let mut recorder = Recorder {
        slot: t,
        script: Script::Money,
        probes: Vec::new(),
    };
    let result = run_targets(&tape, &mut recorder, &costs(0, 0), &config);
    Run {
        result,
        probes: recorder.probes,
        slot: t,
    }
}

struct ActualH4 {
    strategy: parangsae_strategies::Strategy,
    through: i64,
}
impl ActualH4 {
    fn new() -> Self {
        let params = BTreeMap::from([
            ("q".into(), 10_000_000),
            ("sum_settlements".into(), 3),
            ("confirm_h".into(), 24),
            ("hold_h".into(), 168),
        ]);
        Self {
            strategy: parangsae_strategies::Strategy::from_params("H4", &params).unwrap(),
            through: i64::MIN,
        }
    }
}
impl TargetSignal for ActualH4 {
    fn take_h4_observations(&mut self) -> Vec<parangsae_strategies::H4Observation> {
        self.strategy.take_h4_observations()
    }
    fn h4_observation_capacity(&self) -> usize {
        self.strategy.h4_observation_capacity()
    }

    fn decision_delay_us(&self) -> i64 {
        1_000
    }
    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        funding: Causal<'_, &[FundingObs]>,
        book: BookState,
    ) -> Option<StrategyOutput> {
        let last = *bars.value().last().unwrap();
        let available = last.close_time().micros() + 1_000;
        let prints: Vec<_> = funding
            .value()
            .iter()
            .filter(|row| {
                row.funding_time.micros() > self.through && row.available.micros() <= available
            })
            .map(|row| parangsae_strategies::FundingPrint {
                funding_time_us: row.funding_time.micros(),
                available_us: row.available.micros(),
                rate_ticks: row.rate.ticks(),
            })
            .collect();
        if let Some(last) = prints.last() {
            self.through = last.funding_time_us;
        }
        self.strategy.ingest_funding(&prints).unwrap();
        self.strategy
            .push(
                &Observation::new(
                    Timestamps {
                        event_ts: EventTs::from_micros(last.close_time()),
                        available_ts: AvailableTs::from_micros(ts(available)),
                        received_ts: ReceivedTs::from_micros(ts(available)),
                    },
                    last,
                ),
                Some(parangsae_strategies::DecisionBook {
                    equity_ticks: book.equity_ticks(),
                    position_lots: book.position_lots(),
                    mark_ticks: book.mark_ticks(),
                    execution: book.execution(),
                }),
            )
            .unwrap()
    }
}

fn hold_run(fillers: i64) -> Report {
    let ft = date("2024-06-01");
    let start = ft - HOUR;
    let end = ft + 196 * HOUR;
    let mut bars: Vec<_> = (0..(end - start) / MINUTE)
        .map(|i| {
            let open = start + i * MINUTE;
            bar(open, if open >= ft + 23 * HOUR { 90 } else { 100 })
        })
        .collect();
    for row in &mut bars {
        let delta = row.open_time.micros() - (ft + DAY + MINUTE);
        if delta >= 0 && delta < fillers * MINUTE {
            row.count = 0;
            row.volume = Qty::from_ticks(0).unwrap();
        }
    }
    let mut funding = Vec::new();
    let mut slot = ft - 365 * DAY - 16 * HOUR;
    while slot < end {
        funding.push(stamp(
            slot,
            slot + MINUTE,
            if slot == ft { SCALE } else { 0 },
        ));
        slot += 8 * HOUR;
    }
    let config = cfg(&bars);
    let tape = Tape::from_parts(bars.clone(), bars, funding);
    run_targets(&tape, &mut ActualH4::new(), &costs(5, 2), &config).unwrap()
}

fn ruin_run(unfillable: bool) -> Run {
    let t = date("2024-06-02");
    let r = t + 6 * HOUR;
    let start = t + 4 * HOUR;
    let end = t + 10 * HOUR;
    let fill = t + 8 * HOUR + MINUTE;
    let mut bars: Vec<_> = (0..(end - start) / MINUTE)
        .map(|i| {
            let open = start + i * MINUTE;
            bar(open, if open >= r + MINUTE { 300 } else { 100 })
        })
        .collect();
    let mut marks = bars.clone();
    let shock = marks
        .iter_mut()
        .find(|row| row.open_time.micros() == r)
        .unwrap();
    shock.open = price(300);
    shock.high = price(300);
    shock.low = price(300);
    shock.close = price(300);
    for row in &mut bars {
        if row.open_time.micros() >= r + MINUTE && (unfillable || row.open_time.micros() < fill) {
            row.count = 0;
            row.volume = Qty::from_ticks(0).unwrap();
        }
    }
    let config = cfg(&bars);
    let funding = vec![stamp(t + 8 * HOUR, t + 8 * HOUR + MINUTE, 1_000_000)];
    let tape = Tape::from_parts(bars, marks, funding);
    let mut recorder = Recorder {
        slot: r,
        script: Script::Ruin,
        probes: Vec::new(),
    };
    let result = run_targets(&tape, &mut recorder, &costs(5, 2), &config);
    Run {
        result,
        probes: recorder.probes,
        slot: r,
    }
}

#[derive(Debug, Default, cucumber::World)]
struct ExtraWorld {
    pair: Option<(Run, Run, String)>,
    equality_pair: Option<(Run, Run)>,
    hold: Option<(Report, i64)>,
    ruin: Option<Run>,
    ledger: Option<(Ledger, Ledger, SimError)>,
}

#[when(regex = r"^actual H4 holds 168 hours after (\d+) extra filler minutes$")]
fn run_hold(world: &mut ExtraWorld, fillers: i64) {
    world.hold = Some((hold_run(fillers), fillers));
}
#[then("its first exit decision is the hourly ceiling of actual fill plus 168 hours")]
fn check_hold(world: &mut ExtraWorld) {
    let (report, fillers) = world.hold.as_ref().unwrap();
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[0].side, Side::Sell);
    assert_eq!(report.n_forced_exits, 0);
    let base = date("2024-06-02");
    let entry = report.fills[0].ts.micros();
    assert_eq!(entry, base + (fillers + 1) * MINUTE);
    let exit = report
        .signal_outputs
        .iter()
        .find(|row| row.decision_ts.micros().micros() > entry && row.decision.target_ticks() == 0)
        .unwrap()
        .decision_ts
        .micros()
        .micros();
    let deadline = entry + HOLD168;
    let expected = ((deadline - 1_000 + HOUR - 1) / HOUR) * HOUR + 1_000;
    assert_eq!(
        exit, expected,
        "actual entry={entry}, deadline={deadline}; first exit decision must use actual fill"
    );
    assert!(report.fills[1].ts.micros() > exit);
    assert!(report.fills.iter().all(|fill| fill.fee.0 > 0));
    assert!(report.slippage_cost.0 > 0);
}

#[when("two signed delayed earlier rates accompany a later rate available at the first probe")]
fn run_nonmonotone(world: &mut ExtraWorld) {
    world.pair = Some((
        money_run("nonmonotone", false),
        money_run("nonmonotone", true),
        "nonmonotone".into(),
    ));
}
#[then("the first probe admits only the later cash while the signal prefix remains blocked")]
fn check_nonmonotone(world: &mut ExtraWorld) {
    let (left, right, _) = world.pair.as_ref().unwrap();
    for run in [left, right] {
        let p = run.at(run.slot + HOUR + 1_000);
        assert!(p.visible_slots.is_empty());
        assert_eq!(p.position, 0);
        assert_eq!(p.equity, 995_000_000_000);
        assert_eq!(p.requested_lots, 9_950_000_000);
        assert_eq!(run.report().n_refused_fills, 1);
        let entry = run
            .report()
            .fills
            .iter()
            .find(|fill| fill.ts.micros() > p.at)
            .unwrap();
        assert_eq!(entry.qty.ticks(), p.requested_lots);
    }
}
#[then("exact availability at the second probe admits both signed earlier cash values")]
fn check_retirement(world: &mut ExtraWorld) {
    let (left, right, _) = world.pair.as_ref().unwrap();
    for (run, expected) in [(left, 985_000_000_000), (right, 1_005_000_000_000)] {
        let p = run.at(run.slot + 2 * HOUR + 1_000);
        assert_eq!(p.equity, expected);
        assert_eq!(p.visible_slots, vec![run.slot - 8 * HOUR, run.slot]);
    }
}

#[when(regex = r"^the funding money probe varies a (delayed mark|delayed rate) dependency$")]
fn run_split(world: &mut ExtraWorld, kind: String) {
    let equality = format!("{kind} equality");
    world.equality_pair = Some((money_run(&equality, false), money_run(&equality, true)));
    world.pair = Some((money_run(&kind, false), money_run(&kind, true), kind));
}
#[then("decision equity and active requested lots before full publication are equal")]
fn check_split(world: &mut ExtraWorld) {
    let (left, right, kind) = world.pair.as_ref().unwrap();
    for run in [left, right] {
        let p = run.at(run.slot + HOUR + 1_000);
        assert_eq!(p.position, 0);
        assert_eq!(p.equity, 1_000_000_000_000);
        assert_eq!(p.requested_lots, 10_000_000_000);
        assert_eq!(p.visible_slots.contains(&run.slot), kind != "delayed rate");
        let entry = run
            .report()
            .fills
            .iter()
            .find(|fill| fill.ts.micros() > p.at)
            .unwrap();
        assert_eq!(entry.qty.ticks(), p.requested_lots);
    }
}
#[then("exact publication controls reveal the original economic difference")]
fn check_split_equality(world: &mut ExtraWorld) {
    let (left, right, kind) = world.pair.as_ref().unwrap();
    let right_expected = if kind == "delayed rate" {
        1_005_000_000_000
    } else {
        990_000_000_000
    };
    for (run, expected) in [(left, 995_000_000_000), (right, right_expected)] {
        let equality = run.slot + 2 * HOUR + 1_000;
        assert_eq!(run.at(equality).equity, expected);
        run.report(); // Native evidence shows this strict case must complete.
    }
    // Change only the delayed input's availability to equal probe1. These
    // published controls must actually complete and execute the known sizing.
    let (known_left, known_right) = world.equality_pair.as_ref().unwrap();
    for (run, expected) in [(known_left, 995_000_000_000), (known_right, right_expected)] {
        let probe = run.at(run.slot + HOUR + 1_000);
        assert_eq!(probe.equity, expected);
        let fill = run
            .report()
            .fills
            .iter()
            .find(|fill| fill.ts.micros() > probe.at)
            .unwrap();
        assert_eq!(fill.qty.ticks(), probe.requested_lots);
    }
}

#[when("an occupied control has an unpublished original carried mark")]
fn run_carried_control(world: &mut ExtraWorld) {
    world.pair = Some((
        money_run("delayed carried mark", false),
        money_run("delayed carried mark", true),
        "delayed carried mark".into(),
    ));
}

#[then("both hidden mark values refuse at the control clock without an older mark fallback")]
fn check_carried_control(world: &mut ExtraWorld) {
    let (left, right, _) = world.pair.as_ref().unwrap();
    for run in [left, right] {
        assert!(
            matches!(&run.result, Err(EngineError::UnavailableControlMark { at }) if at.micros() == run.slot),
            "occupied control must require the ORIGINAL T-1m mark at T: {:?}",
            run.result
        );
        // A known T-2m row is present. Borrowing it instead of the required
        // carried T-1m source would wrongly continue to these callbacks.
        assert!(run.probes.iter().all(|probe| probe.at < run.slot));
    }
}

#[when("the same original carried mark is published at the occupied control")]
fn run_published_carry(world: &mut ExtraWorld) {
    world.pair = Some((
        money_run("published carried mark", false),
        money_run("published carried mark", true),
        "published carried mark".into(),
    ));
}
#[then("both published carried controls complete the active cash and actual fill observations")]
fn check_published_carry(world: &mut ExtraWorld) {
    let (left, right, _) = world.pair.as_ref().unwrap();
    for (run, expected) in [(left, 995_000_000_000), (right, 990_000_000_000)] {
        let probe = run.at(run.slot + HOUR + 1_000);
        assert_eq!(probe.position, 0);
        assert_eq!(probe.equity, expected);
        let fill = run
            .report()
            .fills
            .iter()
            .find(|fill| fill.ts.micros() > probe.at)
            .unwrap();
        assert_eq!(fill.qty.ticks(), probe.requested_lots);
    }
}

#[when(
    regex = r"^an unpublished (rate|mark) overflows economic funding with representable visible money$"
)]
fn run_overflow(world: &mut ExtraWorld, input: String) {
    let kind = format!("overflow {input}");
    world.pair = Some((money_run(&kind, false), money_run(&kind, true), kind));
}
#[then("checked technical failure returns no accepted report or later decision")]
fn check_overflow(world: &mut ExtraWorld) {
    let (_, bad, _) = world.pair.as_ref().unwrap();
    assert!(
        matches!(&bad.result,Err(EngineError::EconomicArithmetic{at}) if at.micros()==bad.slot),
        "expected typed economic arithmetic failure, got {:?}",
        bad.result
    );
    assert!(bad.probes.iter().all(|p| p.at < bad.slot));
}
#[then("the corresponding nonoverflow control completes an active reentry")]
fn check_nonoverflow(world: &mut ExtraWorld) {
    let (good, _, kind) = world.pair.as_ref().unwrap();
    let p = good.at(good.slot + HOUR + 1_000);
    assert_eq!(p.position, 0);
    assert_eq!(
        p.equity,
        if kind == "overflow rate" {
            1_000_000_000_000
        } else {
            990_000_000_000
        }
    );
    assert!(good
        .report()
        .fills
        .iter()
        .any(|fill| fill.ts.micros() > p.at && fill.qty.ticks() == p.requested_lots));
}

#[when("explicitly known extreme funding arithmetic is applied to a settled synthetic ledger")]
fn run_known_overflow(world: &mut ExtraWorld) {
    let mut ledger = Ledger::new(Cash(10_000 * SCALE));
    ledger
        .apply_fill(
            MarketOrder {
                side: Side::Buy,
                qty: Qty::from_ticks(100 * SCALE).unwrap(),
            },
            price(100),
            &costs(0, 0),
            ts(0),
        )
        .unwrap();
    let before = ledger.clone();
    let error = ledger
        .apply_funding(Ratio::from_ticks(i64::MAX), price(100), ts(MINUTE))
        .unwrap_err();
    world.ledger = Some((before, ledger, error));
}
#[then("the typed overflow preserves the whole ledger and ordinary signed funding still works")]
fn check_known_overflow(world: &mut ExtraWorld) {
    let (before, after, error) = world.ledger.as_ref().unwrap();
    assert_eq!(*error, SimError::Overflow);
    assert_eq!(before, after);
    let mut safe = before.clone();
    let flow = safe
        .apply_funding(Ratio::from_ticks(1_000_000), price(100), ts(MINUTE))
        .unwrap();
    assert_eq!(flow.amount.0, -100 * SCALE);
    assert_eq!(safe.equity(price(100)).unwrap(), Cash(9_900 * SCALE));
}

#[when("an admitted short mark shock waits through filler minutes and a funding settlement")]
fn run_ruin(world: &mut ExtraWorld) {
    world.ruin = Some(ruin_run(false));
}
#[then("its Ruin close is costed strictly after the original mark publication")]
fn check_ruin(world: &mut ExtraWorld) {
    let run = world.ruin.as_ref().unwrap();
    let report = run.report();
    let trigger = run.slot + MINUTE;
    let expected = date("2024-06-02") + 8 * HOUR + MINUTE;
    assert_eq!(report.ruined, Some(ts(trigger)));
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[0].side, Side::Sell);
    assert_eq!(report.fills[1].side, Side::Buy);
    assert_eq!(report.fills[1].ts, ts(expected));
    assert!(report.fills[1].ts.micros() > trigger);
    assert_eq!(report.fills[0].qty, report.fills[1].qty);
    assert_eq!(report.n_forced_exits, 1);
    assert_eq!(
        report.forced_exits,
        vec![ForcedExit {
            trigger: ts(trigger),
            filled_at: ts(expected),
            reason: ForcedExitReason::Ruin,
        }]
    );
    assert!(report.fills.iter().all(|fill| fill.fee.0 > 0));
    assert!(report.slippage_cost.0 > 0);
    assert!(report
        .signal_outputs
        .iter()
        .all(|row| row.decision_ts.micros().micros() < trigger));
}
#[then("funding continues on the held position until actual flat before freezing")]
fn check_liquidating_funding(world: &mut ExtraWorld) {
    let run = world.ruin.as_ref().unwrap();
    let report = run.report();
    let flow = report
        .funding
        .iter()
        .find(|row| row.ts.micros() == date("2024-06-02") + 8 * HOUR)
        .unwrap();
    assert!(flow.ts.micros() > run.slot + MINUTE);
    assert!(flow.ts < report.fills[1].ts);
    assert_eq!(flow.position.0, -report.fills[0].qty.ticks());
    assert!(flow.amount.0 > 0);
    assert_eq!(report.metrics.funding_net, flow.amount);
    let curve_at = |at| {
        report
            .equity_curve
            .iter()
            .find(|(label, _)| label.micros() == at)
            .unwrap()
            .1
             .0
    };
    // The earlier interval label must exclude the next boundary's settlement.
    assert_eq!(
        curve_at(flow.ts.micros() + MINUTE - 1_000) - curve_at(flow.ts.micros() - 1_000),
        flow.amount.0
    );
    let mut oracle = Ledger::new(Cash(10_000 * SCALE));
    let opening = report.fills[0];
    assert_eq!(
        oracle
            .apply_fill(
                MarketOrder {
                    side: opening.side,
                    qty: opening.qty
                },
                price(100),
                &costs(5, 2),
                opening.ts,
            )
            .unwrap(),
        opening
    );
    assert_eq!(
        oracle
            .apply_funding(Ratio::from_ticks(1_000_000), price(300), flow.ts)
            .unwrap()
            .amount,
        flow.amount
    );
    let closing = report.fills[1];
    assert_eq!(
        oracle
            .apply_fill(
                MarketOrder {
                    side: closing.side,
                    qty: closing.qty
                },
                price(300),
                &costs(5, 2),
                closing.ts,
            )
            .unwrap(),
        closing
    );
    assert_eq!(oracle.position().0, 0);
    let close = report.fills[1].ts;
    let frozen = report
        .equity_curve
        .iter()
        .filter(|(at, _)| *at >= close)
        .map(|(_, cash)| *cash)
        .collect::<Vec<_>>();
    assert!(!frozen.is_empty());
    assert!(frozen.iter().all(|cash| *cash == frozen[0]));
    assert_eq!(frozen[0], oracle.cash());
    assert!(frozen[0].0 < 0);
}
#[when("no tradable close remains after the admitted short mark shock")]
fn run_unfillable(world: &mut ExtraWorld) {
    world.ruin = Some(ruin_run(true));
}
#[then("unfilled liquidation fails with the actual trigger and no invented report")]
fn check_unfillable(world: &mut ExtraWorld) {
    let run = world.ruin.as_ref().unwrap();
    assert!(
        matches!(&run.result,Err(EngineError::UnfilledLiquidation{trigger})if trigger.micros()==run.slot+MINUTE),
        "expected typed unfilled-liquidation failure, got {:?}",
        run.result
    );
}

#[tokio::main]
async fn main() {
    ExtraWorld::cucumber()
        .run_and_exit(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/h4_admission_extra.feature"),
        )
        .await;
}
