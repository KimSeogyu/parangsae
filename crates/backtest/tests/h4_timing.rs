use std::collections::BTreeMap;
use std::path::PathBuf;

use cucumber::{then, when, World as _};
use parangsae_backtest::{
    run_targets, target_qty, Bar, BookState, FundingObs, FundingStamp, Report, RunConfig,
    SignalOutput, StrategyOutput, Tape, Target, TargetSignal, UtcDate,
};
use parangsae_core::{
    AvailableTs, Causal, EventTs, Interval, Observation, Price, Qty, Ratio, ReceivedTs,
    ResampledBar, Side, Timestamps, UtcMicros,
};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};

const MINUTE: i64 = 60_000_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const SCALE: i64 = 100_000_000;
const HOLD: i64 = 72 * HOUR;

fn time(value: i64) -> UtcMicros {
    UtcMicros::from_micros(value)
}

fn day(value: &str) -> i64 {
    UtcDate::parse(value).unwrap().start_us().micros()
}

fn price(value: i64) -> Price {
    Price::from_ticks(value * SCALE).unwrap()
}

fn bar(open: i64, value: i64) -> Bar {
    Bar {
        open_time: time(open),
        close_time: time(open + MINUTE - 1_000),
        available: time(open + MINUTE),
        open: price(value),
        high: price(value),
        low: price(value),
        close: price(value),
        volume: Qty::from_ticks(SCALE).unwrap(),
        count: 1,
    }
}

fn config(bars: &[Bar]) -> RunConfig {
    RunConfig {
        initial_cash: Cash(10_000 * SCALE),
        warmup_start: bars[0].open_time,
        start: bars[0].open_time,
        end: time(bars.last().unwrap().open_time.micros() + MINUTE),
        holdout_start: time(day("2025-10-01")),
        forced_flat: Vec::new(),
        flatten_before: None,
        mark_gaps: Vec::new(),
        interval: Interval::ONE_HOUR,
        lot_ticks: 100_000,
    }
}

fn no_costs() -> Costs {
    Costs {
        taker_fee: FeeBps(0),
        slippage: SlippageBps(0),
    }
}

fn output(close: UtcMicros, decision: UtcMicros, target: i64) -> SignalOutput {
    SignalOutput {
        bar_close_time_us: close,
        decision_ts: parangsae_core::DecisionTime::from_micros(decision),
        interval: parangsae_core::BarInterval::H1,
        decision: parangsae_backtest::Decision::Target(Target::from_ticks(target).unwrap()),
        accept: 0,
    }
}

#[derive(Debug)]
struct Capture {
    decision: i64,
    book: BookState,
    requested_lots: i64,
    visible_slots: Vec<i64>,
}

struct CashProbe {
    slot: i64,
    previously_held: bool,
    capture: Option<Capture>,
}

impl TargetSignal for CashProbe {
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
        let decision = last.close_time().micros() + 2_000;
        let target = if decision < self.slot - HOUR {
            i64::from(self.previously_held) * SCALE
        } else if decision < self.slot {
            0
        } else {
            SCALE
        };
        if decision == self.slot + 1_000 {
            self.capture = Some(Capture {
                decision,
                book,
                requested_lots: target_qty(SCALE, book.equity_ticks(), book.mark_ticks(), 100_000)
                    .unwrap(),
                visible_slots: funding
                    .value()
                    .iter()
                    .map(|row| row.funding_time.micros())
                    .collect(),
            });
        }
        Some(StrategyOutput::target(output(
            last.close_time(),
            time(decision),
            target,
        )))
    }
}

#[derive(Debug)]
struct CashCase {
    capture: Capture,
    report: Report,
    slot: i64,
    publication: i64,
}

fn cash_case(held: bool, input: &str, altered: bool) -> CashCase {
    let slot = day("2024-06-02");
    let start = slot - 3 * HOUR;
    let mut bars: Vec<_> = (0..5 * 60)
        .map(|index| bar(start + index * MINUTE, 100))
        .collect();
    if held {
        for row in &mut bars {
            if row.open_time.micros() >= slot - HOUR + MINUTE && row.open_time.micros() < slot {
                row.count = 0;
                row.volume = Qty::from_ticks(0).unwrap();
            }
        }
    }
    let mut marks = bars.clone();
    if input == "mark" && altered {
        let mark = marks
            .iter_mut()
            .find(|row| row.open_time.micros() == slot)
            .unwrap();
        mark.open = price(200);
        mark.high = price(200);
    }
    let rate = if input == "rate" && !altered {
        0
    } else {
        1_000_000
    };
    let funding = vec![FundingStamp {
        funding_time: time(slot),
        available: time(slot + MINUTE),
        rate: Ratio::from_ticks(rate),
        interval_hours: 8,
    }];
    let cfg = config(&bars);
    let tape = Tape::from_parts(bars, marks, funding);
    let mut signal = CashProbe {
        slot,
        previously_held: held,
        capture: None,
    };
    let report = run_targets(&tape, &mut signal, &no_costs(), &cfg).unwrap();
    CashCase {
        capture: signal.capture.unwrap(),
        report,
        slot,
        publication: slot + MINUTE,
    }
}

// Calls the real public H4 strategy, matching the production FamilySignal's
// causal row/funding adaptation. It changes no production logic or hold state.
struct ActualH4 {
    strategy: parangsae_strategies::Strategy,
    funding_through: i64,
    alter_kept_diagnostics: bool,
    keep_overrides: usize,
}

impl ActualH4 {
    fn new() -> Self {
        let params = BTreeMap::from([
            ("q".to_owned(), 10_000_000),
            ("sum_settlements".to_owned(), 3),
            ("confirm_h".to_owned(), 24),
            ("hold_h".to_owned(), 72),
        ]);
        Self {
            strategy: parangsae_strategies::Strategy::from_params("H4", &params).unwrap(),
            funding_through: i64::MIN,
            alter_kept_diagnostics: false,
            keep_overrides: 0,
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
                row.funding_time.micros() > self.funding_through
                    && row.available.micros() <= available
            })
            .map(|row| parangsae_strategies::FundingPrint {
                funding_time_us: row.funding_time.micros(),
                available_us: row.available.micros(),
                rate_ticks: row.rate.ticks(),
            })
            .collect();
        if let Some(last) = prints.last() {
            self.funding_through = last.funding_time_us;
        }
        self.strategy.ingest_funding(&prints).unwrap();
        let obs = Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(last.close_time()),
                available_ts: AvailableTs::from_micros(time(available)),
                received_ts: ReceivedTs::from_micros(time(available)),
            },
            last,
        );
        let mut output = self
            .strategy
            .push(
                &obs,
                Some(parangsae_strategies::DecisionBook {
                    equity_ticks: book.equity_ticks(),
                    position_lots: book.position_lots(),
                    mark_ticks: book.mark_ticks(),
                    execution: book.execution(),
                }),
            )
            .unwrap();
        if self.alter_kept_diagnostics
            && matches!(
                book.execution(),
                parangsae_backtest::ExecutionPhase::EntryPending { .. }
            )
        {
            let output = output.as_mut().unwrap();
            assert_eq!(
                output.directive,
                parangsae_backtest::ExecutionDirective::Keep
            );
            output.signal.decision =
                parangsae_backtest::Decision::Target(Target::from_ticks(SCALE).unwrap());
            output.signal.accept = 73;
            self.keep_overrides += 1;
        }
        output
    }
}

#[derive(Debug)]
struct HoldCase {
    report: Report,
    entry_decision: i64,
    dropped: u64,
    end: i64,
    fillers: i64,
    keep_overrides: usize,
}

fn hold_case(fillers: i64, boundary: &str) -> HoldCase {
    hold_case_with_diagnostic(fillers, boundary, false)
}

fn hold_case_with_diagnostic(fillers: i64, boundary: &str, alter_kept: bool) -> HoldCase {
    let ft = if boundary == "development" {
        day("2025-09-27")
    } else {
        day("2024-06-01")
    };
    let entry_decision = ft + DAY + 1_000;
    let start = ft - HOUR;
    let hours = if boundary == "development" {
        27
    } else if boundary == "fold" {
        28
    } else {
        100
    };
    let end = start + hours * HOUR;
    let mut bars: Vec<_> = (0..hours * 60)
        .map(|index| {
            let open = start + index * MINUTE;
            bar(open, if open >= ft + 23 * HOUR { 90 } else { 100 })
        })
        .collect();
    // D is 1ms after the hour open: that minute cannot fill. These additional
    // count0 minutes defer the first eligible open rather than the signal.
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
        funding.push(FundingStamp {
            funding_time: time(slot),
            available: time(slot + MINUTE),
            rate: Ratio::from_ticks(if slot == ft { 100_000_000 } else { 0 }),
            interval_hours: 8,
        });
        slot += 8 * HOUR;
    }
    let mut cfg = config(&bars);
    if boundary == "fold" {
        cfg.flatten_before = Some(time(end));
    }
    let tape = Tape::from_parts(bars.clone(), bars, funding);
    let mut signal = ActualH4::new();
    signal.alter_kept_diagnostics = alter_kept;
    let report = run_targets(&tape, &mut signal, &no_costs(), &cfg).unwrap();
    let dropped = report
        .h4
        .as_ref()
        .unwrap()
        .entry_drops
        .iter()
        .filter(|drop| drop.reason == parangsae_backtest::H4EntryDropReason::DevelopmentBoundary)
        .count() as u64;
    HoldCase {
        report,
        entry_decision,
        dropped,
        end,
        fillers,
        keep_overrides: signal.keep_overrides,
    }
}

#[derive(Debug, Default, cucumber::World)]
struct TimingWorld {
    cash: Option<(CashCase, CashCase, bool)>,
    hold: Option<HoldCase>,
    sizing: Option<SizingCase>,
}

#[when(
    regex = r"^the (held|flat) funding-slot account is replayed with two unpublished (rate|mark) values$"
)]
fn run_cash(world: &mut TimingWorld, position: String, input: String) {
    let held = position == "held";
    world.cash = Some((
        cash_case(held, &input, false),
        cash_case(held, &input, true),
        held,
    ));
}

#[then("the construction has the intended pre-fill position and pending-close history")]
fn cash_construction(world: &mut TimingWorld) {
    let (left, right, held) = world.cash.as_ref().unwrap();
    for case in [left, right] {
        assert_eq!(case.capture.decision, case.slot + 1_000);
        assert!(case.capture.decision < case.publication);
        assert_eq!(case.capture.book.position_lots(), 0);
        assert!(!case.capture.visible_slots.contains(&case.slot));
        let reentry = case
            .report
            .fills
            .iter()
            .find(|fill| fill.ts.micros() >= case.publication)
            .unwrap();
        assert_eq!(reentry.side, Side::Buy);
        assert_eq!(reentry.qty.ticks(), case.capture.requested_lots);
        let event = case
            .report
            .funding
            .iter()
            .find(|row| row.ts.micros() == case.slot)
            .unwrap();
        if *held {
            assert_eq!(event.position.0, 100 * SCALE);
            assert_eq!(case.report.n_refused_fills, 1);
            let close = case
                .report
                .fills
                .iter()
                .find(|fill| fill.ts.micros() == case.slot)
                .unwrap();
            assert_eq!(close.side, Side::Sell);
            assert_eq!(close.qty.ticks(), 100 * SCALE);
        } else {
            assert_eq!(event.position.0, 0);
            assert_eq!(event.amount.0, 0);
            assert_eq!(case.report.n_refused_fills, 0);
        }
    }
    let prior_fills = |case: &CashCase| {
        case.report
            .fills
            .iter()
            .filter(|fill| fill.ts.micros() < case.publication)
            .copied()
            .collect::<Vec<_>>()
    };
    assert_eq!(prior_fills(left), prior_fills(right));
}

#[then("the decision book and requested entry lots before publication are identical")]
fn cash_invariant(world: &mut TimingWorld) {
    let (left, right, _) = world.cash.as_ref().unwrap();
    assert_eq!(
        left.capture.book, right.capture.book,
        "unpublished mutation reached decision book: left={:?}, right={:?}, publication={}",
        left.capture, right.capture, left.publication
    );
    assert_eq!(left.capture.requested_lots, right.capture.requested_lots);
}

#[when(regex = r"^actual H4 waits through (\d+) filler minutes before entry$")]
fn run_hold(world: &mut TimingWorld, fillers: i64) {
    world.hold = Some(hold_case(fillers, "normal"));
}

#[then("a real H4 short entry and one time exit are observed")]
fn hold_construction(world: &mut TimingWorld) {
    let case = world.hold.as_ref().unwrap();
    assert_eq!(case.report.fills.len(), 2);
    assert_eq!(case.report.fills[0].side, Side::Sell);
    assert_eq!(
        case.report.fills[0].ts.micros(),
        case.entry_decision - 1_000 + (case.fillers + 1) * MINUTE
    );
    assert_eq!(case.report.fills[1].side, Side::Buy);
    assert_eq!(case.report.n_forced_exits, 0);
    assert_eq!(case.report.n_refused_fills, u64::from(case.fillers > 0));
}

#[then("the first exit decision is the first hourly decision at or after actual entry plus hold")]
fn hold_anchor(world: &mut TimingWorld) {
    let case = world.hold.as_ref().unwrap();
    let fill = case.report.fills[0].ts.micros();
    let actual = case
        .report
        .signal_outputs
        .iter()
        .find(|row| row.decision_ts.micros().micros() > fill && row.decision.target_ticks() == 0)
        .unwrap()
        .decision_ts
        .micros()
        .micros();
    let deadline = fill + HOLD;
    let expected = ((deadline - 1_000 + HOUR - 1) / HOUR) * HOUR + 1_000;
    assert_eq!(actual, expected,
        "actual H4 exit decision is early: entry_decision={}, actual_entry={}, hold_deadline={}, actual_exit_decision={}, expected_exit_decision={}, fill_duration={}",
        case.entry_decision, fill, deadline, actual, expected,
        case.report.fills[1].ts.micros() - fill);
}

#[when("a real H4 entry waits past another decision whose Keep diagnostic disagrees")]
fn run_keep(world: &mut TimingWorld) {
    world.hold = Some(hold_case_with_diagnostic(61, "normal", true));
}

#[then("the original short order keeps its sizing origin and one refusal until its actual fill")]
fn keep_order(world: &mut TimingWorld) {
    hold_construction(world);
    hold_anchor(world);
    let case = world.hold.as_ref().unwrap();
    assert!(
        case.keep_overrides > 0,
        "the pending decision must be exercised"
    );
    assert_eq!(case.report.n_rebalances, 2);
    let raw = case.report.h4.as_ref().unwrap();
    assert_eq!(raw.trades.len(), 1);
    assert_eq!(raw.trades[0].event.settlement.micros(), day("2024-06-01"));
    assert_eq!(
        raw.trades[0].event.crowd_side,
        parangsae_backtest::PositionSide::Long
    );
    assert_eq!(raw.trades[0].fills, case.report.fills);
    assert!(raw.entry_drops.is_empty());
    assert!(raw
        .observations
        .iter()
        .any(|item| item.kind == parangsae_strategies::H4ObservationKind::OccupiedSkip));
}

#[when("actual H4 is flattened by a short scored fold")]
fn run_fold(world: &mut TimingWorld) {
    world.hold = Some(hold_case(1, "fold"));
}

#[then("the fold close is forced before the hold deadline and no order crosses the fold")]
fn fold_control(world: &mut TimingWorld) {
    let case = world.hold.as_ref().unwrap();
    assert_eq!(case.report.fills.len(), 2);
    assert_eq!(case.report.n_forced_exits, 1);
    assert!(case.report.fills[1].ts.micros() < case.report.fills[0].ts.micros() + HOLD);
    assert!(case
        .report
        .fills
        .iter()
        .all(|fill| fill.ts.micros() < case.end));
}

#[when("actual H4 offers an entry whose hold passes the development end")]
fn run_end(world: &mut TimingWorld) {
    world.hold = Some(hold_case(0, "development"));
}

#[then("the entry is refused and its dropped-entry count is visible")]
fn end_control(world: &mut TimingWorld) {
    let case = world.hold.as_ref().unwrap();
    assert!(case.report.fills.is_empty());
    assert_eq!(case.dropped, 1);
    assert!(case.entry_decision + HOLD > day("2025-10-01") - MINUTE);
}

struct EntryProbe {
    origin: i64,
    sent: bool,
    phases: Vec<parangsae_backtest::ExecutionPhase>,
}

impl TargetSignal for EntryProbe {
    fn decision_delay_us(&self) -> i64 {
        1_000
    }
    fn h4_observation_capacity(&self) -> usize {
        3
    }
    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        _: Causal<'_, &[FundingObs]>,
        book: BookState,
    ) -> Option<StrategyOutput> {
        self.phases.push(book.execution());
        let row = bars.value().last().unwrap();
        let mut envelope = StrategyOutput::target(output(
            row.close_time(),
            time(row.close_time().micros() + 2_000),
            SCALE,
        ));
        envelope.directive = if self.sent {
            parangsae_backtest::ExecutionDirective::Keep
        } else {
            self.sent = true;
            parangsae_backtest::ExecutionDirective::Enter {
                event: parangsae_backtest::H4Event {
                    settlement: time(self.origin),
                    episode: time(self.origin),
                    crowd_side: parangsae_backtest::PositionSide::Short,
                    gap_boundary: false,
                },
                hold: parangsae_backtest::ActualHold::from_hours(72).unwrap(),
            }
        };
        Some(envelope)
    }
}

#[derive(Debug)]
struct SizingCase {
    report: Report,
    phases: Vec<parangsae_backtest::ExecutionPhase>,
    cheaper: bool,
    fillers: i64,
}

#[when(
    regex = r"^a provisional zero-lot long reaches a (cheaper|unchanged) open after (\d+) filler minutes$"
)]
fn sizing_case(world: &mut TimingWorld, price_change: String, fillers: i64) {
    let start = day("2024-06-01");
    let cheaper = price_change == "cheaper";
    let bars: Vec<_> = (0..4 * 60)
        .map(|index| {
            let open = start + index * MINUTE;
            let mut row = bar(
                open,
                if cheaper && open > start + HOUR {
                    50
                } else {
                    200
                },
            );
            if open >= start + HOUR + MINUTE && open < start + HOUR + (fillers + 1) * MINUTE {
                row.count = 0;
            }
            row
        })
        .collect();
    let mut cfg = config(&bars);
    cfg.initial_cash = Cash(100 * SCALE);
    cfg.lot_ticks = SCALE;
    cfg.flatten_before = Some(cfg.end);
    let funding = vec![FundingStamp {
        funding_time: time(start),
        available: time(start + MINUTE),
        rate: Ratio::from_ticks(0),
        interval_hours: 8,
    }];
    let tape = Tape::from_parts(bars.clone(), bars, funding);
    let mut signal = EntryProbe {
        origin: start,
        sent: false,
        phases: Vec::new(),
    };
    let report = run_targets(
        &tape,
        &mut signal,
        &Costs {
            taker_fee: FeeBps(5),
            slippage: SlippageBps(2),
        },
        &cfg,
    )
    .unwrap();
    world.sizing = Some(SizingCase {
        report,
        phases: signal.phases,
        cheaper,
        fillers,
    });
}

#[then("only actual-fill sizing can drop or arm the entry and pending feedback stays Long")]
fn sizing_disposition(world: &mut TimingWorld) {
    let case = world.sizing.as_ref().unwrap();
    let trace = case.report.h4.as_ref().unwrap();
    if case.fillers >= 60 {
        let pending: Vec<_> = case
            .phases
            .iter()
            .filter(|phase| {
                matches!(
                    phase,
                    parangsae_backtest::ExecutionPhase::EntryPending { .. }
                )
            })
            .collect();
        assert!(!pending.is_empty());
        assert!(pending.iter().all(|phase| **phase
            == parangsae_backtest::ExecutionPhase::EntryPending {
                side: parangsae_backtest::PositionSide::Long
            }));
        assert_eq!(case.report.n_refused_fills, 1);
    }
    if case.cheaper {
        assert_eq!(case.report.fills.len(), 2);
        assert_eq!(case.report.fills[0].side, Side::Buy);
        assert_eq!(case.report.fills[0].qty.ticks(), SCALE);
        assert!(
            trace.entry_drops.is_empty(),
            "a filled event must not also be dropped: {:?}",
            trace.entry_drops
        );
        assert_eq!(trace.trades.len(), 1);
        assert!(case.phases.iter().any(|phase| matches!(
            phase,
            parangsae_backtest::ExecutionPhase::Open {
                side: parangsae_backtest::PositionSide::Long,
                ..
            }
        )));
    } else {
        assert!(case.report.fills.is_empty());
        assert_eq!(case.report.metrics.fees_paid.0, 0);
        assert_eq!(trace.entry_drops.len(), 1);
        assert_eq!(
            trace.entry_drops[0].reason,
            parangsae_backtest::H4EntryDropReason::ZeroLots
        );
        assert!(trace.trades.is_empty());
        assert!(!case.phases.iter().any(|phase| matches!(
            phase,
            parangsae_backtest::ExecutionPhase::Open { .. }
                | parangsae_backtest::ExecutionPhase::ExitPending { .. }
        )));
    }
}

#[tokio::main]
async fn main() {
    TimingWorld::cucumber()
        .run_and_exit(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/h4_timing.feature"))
        .await;
}
