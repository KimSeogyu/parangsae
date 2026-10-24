use std::path::PathBuf;

use cucumber::{then, when, World as _};
use parangsae_backtest::{
    run_targets, Bar, BookState, EngineError, ForcedExitReason, FundingObs, FundingStamp,
    MarkAbsence, Report, RunConfig, SignalOutput, Tape, Target, TargetSignal, UtcDate, UtcRange,
};
use parangsae_core::{
    BarInterval, Causal, DecisionTime, Interval, Price, Qty, Ratio, ResampledBar, Side, UtcMicros,
};
use parangsae_sim::{Cash, Costs, FeeBps, Fill, SlippageBps};
use parangsae_strategies::StrategyOutput;

const MINUTE: i64 = 60_000_000;
const HOUR: i64 = 60 * MINUTE;
const SCALE: i64 = 100_000_000;

fn ts(value: i64) -> UtcMicros {
    UtcMicros::from_micros(value)
}
fn date(value: &str) -> i64 {
    UtcDate::parse(value).unwrap().start_us().micros()
}
fn price(value: i64) -> Price {
    Price::from_ticks(value * SCALE).unwrap()
}

struct FixedTarget(i64);
impl TargetSignal for FixedTarget {
    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        _: Causal<'_, &[FundingObs]>,
        _: BookState,
    ) -> Option<StrategyOutput> {
        let last = bars.value().last().unwrap();
        Some(StrategyOutput::target(SignalOutput {
            decision_ts: DecisionTime::from_micros(
                last.close_time().checked_add_micros(1_000).unwrap(),
            ),
            bar_close_time_us: last.close_time(),
            interval: BarInterval::H1,
            decision: parangsae_backtest::Decision::Target(Target::from_ticks(self.0).unwrap()),
            accept: 0,
        }))
    }
}

#[derive(Debug)]
struct Case {
    boundary: i64,
    result: Result<Report, EngineError>,
}

fn run_case(scope: &str, zero_tail: i64, shock: bool, unknown: bool) -> Case {
    let published = scope.starts_with("published ");
    let scope = scope.strip_prefix("published ").unwrap_or(scope);
    let pending_entry = scope.starts_with("pending ");
    let scope = scope.strip_prefix("pending ").unwrap_or(scope);
    let (start, end) = match scope {
        "fold" => (
            date("2024-06-02") + 12 * HOUR,
            date("2024-06-02") + 14 * HOUR,
        ),
        "development" => (date("2025-09-30") + 22 * HOUR, date("2025-10-01")),
        "funding" => (
            date("2024-06-02") + 14 * HOUR,
            date("2024-06-02") + 17 * HOUR,
        ),
        _ => unreachable!(),
    };
    let boundary = if scope == "funding" {
        date("2024-06-02") + 16 * HOUR
    } else {
        end - MINUTE
    };
    let mut bars: Vec<_> = (0..(end - start) / MINUTE)
        .map(|i| {
            let open = start + i * MINUTE;
            Bar {
                open_time: ts(open),
                close_time: ts(open + MINUTE - 1_000),
                available: ts(open + MINUTE),
                open: price(100),
                high: price(100),
                low: price(100),
                close: price(100),
                volume: Qty::from_ticks(SCALE).unwrap(),
                count: 1,
            }
        })
        .collect();
    let mut marks = bars.clone();
    if shock {
        let original = marks
            .iter_mut()
            .find(|row| row.open_time.micros() == boundary - MINUTE)
            .unwrap();
        original.open = price(300);
        original.high = price(300);
        original.low = price(300);
        original.close = price(300);
        // The original mark is published exactly at the fixed Terminal open.
        assert_eq!(original.available, ts(boundary));
        let venue = bars.last_mut().unwrap();
        venue.open = price(300);
        venue.high = price(300);
        venue.low = price(300);
        venue.close = price(300);
    }
    // Isolate the oracle: no timestamp, price, mark, volume or prior count changes.
    if pending_entry {
        for row in &mut bars {
            if row.open_time.micros() >= start + HOUR && row.open_time.micros() < boundary {
                row.count = 0;
            }
        }
    }
    for row in bars
        .iter_mut()
        .rev()
        .take(usize::try_from(zero_tail).unwrap())
    {
        row.count = 0;
    }
    let mut config = RunConfig {
        initial_cash: Cash(10_000 * SCALE),
        warmup_start: ts(start),
        start: ts(start),
        end: ts(end),
        holdout_start: ts(date("2025-10-01")),
        forced_flat: if scope == "funding" {
            vec![UtcRange {
                start: ts(boundary),
                end: ts(end),
            }]
        } else {
            Vec::new()
        },
        flatten_before: if scope == "development" {
            None
        } else {
            Some(ts(end))
        },
        mark_gaps: Vec::new(),
        interval: Interval::ONE_HOUR,
        lot_ticks: 100_000,
    };
    let funding = if unknown || published {
        vec![FundingStamp {
            funding_time: ts(boundary),
            available: ts(if published {
                boundary
            } else {
                boundary + MINUTE
            }),
            rate: Ratio::from_ticks(2 * SCALE),
            interval_hours: 8,
        }]
    } else {
        Vec::new()
    };
    if published {
        // A registered synthetic carry makes the funding's original mark fully
        // published at T. The native T-row's full minute is still unpublished.
        marks.retain(|row| row.open_time.micros() != boundary);
        config.mark_gaps.push(MarkAbsence {
            first_open: ts(boundary),
            last_open: ts(boundary),
            known_source_absence: true,
        });
    }
    let tape = Tape::from_parts(bars, marks, funding);
    let mut target = FixedTarget(if shock { -SCALE } else { SCALE });
    let result = run_targets(
        &tape,
        &mut target,
        &Costs {
            taker_fee: FeeBps(5),
            slippage: SlippageBps(2),
        },
        &config,
    );
    Case { boundary, result }
}

#[derive(Debug, Default, cucumber::World)]
struct BoundaryWorld {
    pairs: Vec<(i64, Case, Case)>,
    case: Option<Case>,
}

#[when(
    regex = r"^each one-to-five-minute terminal count tail is changed for the (fold|development) boundary$"
)]
fn run_tails(world: &mut BoundaryWorld, scope: String) {
    world.pairs = (1..=5)
        .map(|tail| {
            (
                tail,
                run_case(&scope, 0, false, false),
                run_case(&scope, tail, false, false),
            )
        })
        .collect();
}
#[then("every accepted fill prefix before the first changed count is unchanged")]
fn causal_prefix(world: &mut BoundaryWorld) {
    // Exhaust every member of this finite property domain; no random sampling.
    for (tail, base, changed) in &world.pairs {
        let baseline = base.result.as_ref().expect("valid active base");
        assert_eq!(baseline.fills.len(), 2);
        assert_eq!(baseline.n_forced_exits, 1);
        let first_changed = base.boundary - (tail - 1) * MINUTE;
        let prefix = |report: &Report| -> Vec<Fill> {
            report
                .fills
                .iter()
                .filter(|fill| fill.ts.micros() < first_changed)
                .copied()
                .collect()
        };
        if let Ok(report) = &changed.result {
            assert_eq!(prefix(report),prefix(baseline),"future count tail={tail} chose an earlier fill; first changed={first_changed}, fixed boundary={}, changed fills={:?}",base.boundary,report.fills);
        }
        assert!(
            matches!(&changed.result, Err(EngineError::UnfilledBoundaryClose { boundary }) if *boundary == ts(base.boundary)),
            "every held unfillable scheduled boundary must have its typed whole-run refusal: {:?}",
            changed.result
        );
    }
}

#[when(regex = r"^the fixed last scheduled (fold|development) open has count zero$")]
fn run_zero(world: &mut BoundaryWorld, scope: String) {
    world.case = Some(run_case(&scope, 1, false, false));
}
#[then("no accepted report substitutes an earlier terminal close")]
fn refused(world: &mut BoundaryWorld) {
    let case = world.case.as_ref().unwrap();
    assert!(
        matches!(&case.result, Err(EngineError::UnfilledBoundaryClose { boundary }) if *boundary == ts(case.boundary)),
        "unfilled scheduled boundary must have its exact typed failure: {:?}",
        case.result
    );
}

#[when(regex = r"^the fixed last scheduled (fold|development) open is tradable$")]
fn run_positive(world: &mut BoundaryWorld, scope: String) {
    world.case = Some(run_case(&scope, 0, false, false));
}
#[then("the actual terminal close occurs exactly at that scheduled open once")]
fn positive(world: &mut BoundaryWorld) {
    let case = world.case.as_ref().unwrap();
    let report = case.result.as_ref().unwrap();
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[0].side, Side::Buy);
    assert_eq!(report.fills[1].side, Side::Sell);
    assert_eq!(report.fills[1].ts, ts(case.boundary));
    assert_eq!(report.n_forced_exits, 1);
    assert_eq!(report.forced_exits.len(), 1);
    let event = &report.forced_exits[0];
    assert_eq!(event.trigger, ts(case.boundary));
    assert_eq!(event.filled_at, ts(case.boundary));
    assert!(matches!(&event.reason, ForcedExitReason::Terminal));
    assert!(report.fills.iter().all(|fill| fill.fee.0 > 0));
    assert!(report.slippage_cost.0 > 0);
}

#[when(
    regex = r"^a pending flat entry reaches the (fold|development) Terminal with final count (0|1)$"
)]
fn run_pending(world: &mut BoundaryWorld, scope: String, count: i64) {
    world.case = Some(run_case(
        &format!("pending {scope}"),
        1 - count,
        false,
        false,
    ));
}

#[then("the entry is cancelled without a fill or fee and retains one refusal")]
fn pending_cancel(world: &mut BoundaryWorld) {
    let case = world.case.as_ref().unwrap();
    let report = case.result.as_ref().expect("flat terminal needs no close");
    assert!(
        report.fills.is_empty(),
        "pending risk filled at Terminal: {:?}",
        report.fills
    );
    assert_eq!(report.metrics.fees_paid.0, 0);
    assert_eq!(report.metrics.funding_net.0, 0);
    assert_eq!(report.n_forced_exits, 0);
    assert!(report.forced_exits.is_empty());
    assert_eq!(
        report.n_refused_fills, 1,
        "intent must not use a future count scan before queueing"
    );
}

#[when("the admitted mark shock co-triggers Ruin at a tradable Terminal open")]
fn run_co_trigger(world: &mut BoundaryWorld) {
    world.case = Some(run_case("fold", 0, true, false));
}
#[then("Terminal closes once at that open without a backdated forced fill")]
fn co_trigger(world: &mut BoundaryWorld) {
    let case = world.case.as_ref().unwrap();
    let report = case.result.as_ref().unwrap();
    assert_eq!(report.fills.len(), 2);
    assert_eq!(report.fills[0].side, Side::Sell);
    assert_eq!(report.fills[1].side, Side::Buy);
    assert_eq!(
        report.fills[1].ts,
        ts(case.boundary),
        "new Ruin cannot backdate the predeclared Terminal fill"
    );
    assert_eq!(report.n_forced_exits, 1);
    assert_eq!(report.forced_exits.len(), 1);
    let event = &report.forced_exits[0];
    assert_eq!(event.trigger, ts(case.boundary));
    assert_eq!(event.filled_at, ts(case.boundary));
    assert!(matches!(&event.reason, ForcedExitReason::Terminal));
    assert!(report.fills.iter().all(|fill| fill.fee.0 > 0));
}

#[when("unknown funding makes economic and admitted solvency disagree at Terminal")]
fn run_unknown(world: &mut BoundaryWorld) {
    world.case = Some(run_case("funding", 0, false, true));
}
#[then("the whole trial refuses before accepting a terminal action")]
fn unknown(world: &mut BoundaryWorld) {
    let case = world.case.as_ref().unwrap();
    assert!(
        !matches!(
            &case.result,
            Err(EngineError::Resample(_) | EngineError::Gap { .. })
        ),
        "invalid fixture cannot stand in for economic admission: {:?}",
        case.result
    );
    assert!(
        matches!(&case.result, Err(EngineError::UnadmittedMoneyControl { at }) if *at == ts(case.boundary)),
        "unknown control must refuse before Terminal with exact use clock: {:?}",
        case.result
    );
}

#[when("a zero-delay prevenue ruin precedes a hidden receipt at the same Terminal open")]
fn same_boundary_receipt(world: &mut BoundaryWorld) {
    world.case = Some(run_case("funding", 0, true, true));
}

#[when("both postbooking predicates recover from a generic due-open published receipt")]
fn published_same_boundary_receipt(world: &mut BoundaryWorld) {
    // Generic native domain only. Canonical H4 still requires slot+60s.
    world.case = Some(run_case("published funding", 0, true, false));
}

#[then("the negative prevenue callback stays blocked without a new final Ruin latch")]
fn published_recovery(world: &mut BoundaryWorld) {
    let case = world.case.as_ref().unwrap();
    let report = case
        .result
        .as_ref()
        .expect("both admitted postbooking predicates are positive");
    assert_eq!(report.ruined, None);
    assert!(report.funding[0].amount.0 > 0);
    assert!(report.equity_end().unwrap().0 > 10_000 * SCALE);
    assert!(report
        .signal_outputs
        .iter()
        .all(|row| row.decision_ts.micros() != ts(case.boundary)));
    assert_eq!(report.forced_exits.len(), 1);
    assert_eq!(report.forced_exits[0].reason, ForcedExitReason::Terminal);
    assert_eq!(report.forced_exits[0].filled_at, ts(case.boundary));
}

#[tokio::main]
async fn main() {
    BoundaryWorld::cucumber()
        .run_and_exit(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/h4_terminal_boundary.feature"),
        )
        .await;
}
