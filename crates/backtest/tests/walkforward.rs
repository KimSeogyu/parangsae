//! Walk-forward selection on a synthetic tape, plus the ledger and holdout gate.
//!
//! The tape is five UTC days from 2021-03-01. Warmup is flat, the train day
//! rises, the two purge days crash, and the test day keeps falling. The long
//! config is the known train winner. A window that includes the purge crash
//! would pick the short config instead.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use parangsae_backtest::{
    btcusdt_schedule, canonical_config_id, check_window_result, daily_return_tick, fold_schedule,
    forced_flat_at, run_targets, run_walk_forward, selection_costs, trial_id, window_result,
    Decision, EngineError, FamilyGrid, Fold, GridPoint, RunConfig, ScheduleSpec, SignalOutput,
    SweepPins, Target, TargetSignal, TrialSpec, UtcDate, UtcRange, WalkContext, WalkError,
    WalkForward, WindowResult, HOLDOUT_START_US,
};
use parangsae_core::{Causal, DecisionTime, Interval, Side, UtcMicros};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};
use serde_json::Value;
use support::{bar_at, cfg, px, stamp, tape_marked, usd};

const DAY_US: i64 = 86_400_000_000;
const MINUTE_US: i64 = 60_000_000;
const ALPHA_SHA: &str = "8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8";

fn date(text: &str) -> UtcDate {
    UtcDate::parse(text).expect(text)
}

fn pins(dir: &Path) -> SweepPins {
    fs::write(dir.join("obj.txt"), b"alpha").unwrap();
    let pin_path = dir.join("sweep-inputs.json");
    fs::write(
        &pin_path,
        format!(
            "{{\"schema\":\"sweep-runner-inputs-v2\",\"objects\":[{{\"path\":\"obj.txt\",\"sha256\":\"{ALPHA_SHA}\"}}]}}\n"
        ),
    )
    .unwrap();
    SweepPins::load(&pin_path).unwrap()
}

fn point(config: i64, attempt: u64) -> GridPoint<i64> {
    let mut params = BTreeMap::new();
    params.insert("side".to_owned(), config);
    GridPoint {
        config,
        params,
        attempt,
    }
}

fn ctx<'a>(trials_path: &'a Path, pins: &'a SweepPins, initial: Cash) -> WalkContext<'a> {
    WalkContext {
        pins,
        trials_path,
        initial_cash: initial,
        holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
    }
}

fn shaped(initial: i64, days: usize, extra: i64, round_trips: u64) -> WindowResult {
    let mut equity_ticks = vec![initial; days + 1];
    let last = equity_ticks.last_mut().expect("opening equity");
    *last = initial + extra;
    let daily_returns = equity_ticks
        .windows(2)
        .map(|pair| daily_return_tick(pair[0], pair[1]).unwrap())
        .collect();
    WindowResult {
        daily_returns,
        equity_ticks,
        net_result_ticks: extra,
        round_trips,
    }
}

fn log_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("trials.jsonl")).unwrap()
}

#[test]
fn tie_keeps_grid_order_and_ignores_the_test_window() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(1_000);
    let folds = fold_schedule(&ScheduleSpec {
        sample_start: date("2021-01-01"),
        holdout_start: date("2021-01-04"),
        warmup_days: 0,
        train_days: 1,
        purge_days: 1,
        test_days: 1,
        step_days: 1,
        remainder_own_fold_days: 45,
    })
    .unwrap();
    let train_end = folds[0].train.end.micros();
    let train = folds[0].train;
    let family = FamilyGrid {
        family: "tie".to_owned(),
        folds,
        points: vec![point(1, 3), point(2, 4)],
    };
    let output = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |config, start, end, _| {
            let days = usize::try_from((end - start) / DAY_US).unwrap();
            let is_train = end == train_end;
            let (extra, trips) = if is_train {
                (5_000, 0)
            } else {
                (*config * 1_000, u64::try_from(*config).unwrap())
            };
            Ok::<WindowResult, String>(shaped(initial.0, days, extra, trips))
        },
    )
    .unwrap();
    let fold = &output.families[0].folds[0];
    assert_eq!(
        fold.train[0].net_result_ticks,
        fold.train[1].net_result_ticks
    );
    assert!(fold.test[1].net_result_ticks > fold.test[0].net_result_ticks);
    assert_eq!(fold.selected_index, 0);
    assert_eq!(output.families[0].oos_round_trips, 1);
    assert_eq!(
        output.families[0].stitched_oos_daily_returns,
        fold.test[0].daily_returns
    );

    let mut spec = TrialSpec {
        run_id: fold.train[0].run_id.clone(),
        config_id: String::new(),
        family: "tie".to_owned(),
        params: point(1, 3).params,
        attempt: 3,
        ts: train.start,
        run: RunConfig {
            initial_cash: initial,
            start: train.start,
            end: train.end,
            holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
            forced_flat: vec![forced_flat_at(train.end).unwrap()],
            mark_gaps: Vec::new(),
            interval: Interval::ONE_HOUR,
            lot_ticks: 100_000,
        },
    };
    spec.config_id = canonical_config_id(&spec);
    assert_eq!(fold.train[0].config_id, spec.config_id);
    assert_eq!(
        fold.train[0].trial_id,
        trial_id(&spec.config_id, pins.pinned_root(), 3)
    );
    let text = log_text(dir.path());
    assert_eq!(
        text.lines()
            .filter(|line| line.contains("RunStarted"))
            .count(),
        4
    );
    assert_eq!(
        text.lines()
            .filter(|line| line.contains("RunFinished"))
            .count(),
        4
    );
    assert!(text.contains(&fold.train[0].trial_id));
}

#[test]
fn a_later_config_wins_when_its_train_net_is_higher_and_oos_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(1_000);
    let folds = fold_schedule(&ScheduleSpec {
        sample_start: date("2021-01-01"),
        holdout_start: date("2021-05-18"),
        warmup_days: 0,
        train_days: 1,
        purge_days: 1,
        test_days: 90,
        step_days: 90,
        remainder_own_fold_days: 45,
    })
    .unwrap();
    assert_eq!(folds.len(), 2);
    let trains: Vec<(i64, i64)> = folds
        .iter()
        .map(|fold| (fold.train.start.micros(), fold.train.end.micros()))
        .collect();
    let family = FamilyGrid {
        family: "rank".to_owned(),
        folds,
        points: vec![point(1, 0), point(2, 0)],
    };
    let output = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |config, start, end, _| {
            let days = usize::try_from((end - start) / DAY_US).unwrap();
            let is_train = trains.contains(&(start, end));
            let extra = if is_train {
                *config * 10_000
            } else {
                *config * 1_000
            };
            let trips = if is_train {
                0
            } else {
                u64::try_from(*config).unwrap()
            };
            Ok::<WindowResult, String>(shaped(initial.0, days, extra, trips))
        },
    )
    .unwrap();
    let family = &output.families[0];
    assert_eq!(family.folds[0].selected_index, 1);
    assert_eq!(family.folds[1].selected_index, 1);
    assert_eq!(family.oos_round_trips, 4);
    let equity = &family.stitched_oos_equity;
    let first_days = family.folds[0].test[1].equity_ticks.len();
    assert_eq!(equity[..first_days], family.folds[0].test[1].equity_ticks);
    assert_eq!(equity[first_days], initial.0);
    assert_ne!(equity[first_days - 1], equity[first_days]);
    assert_eq!(
        family.stitched_oos_daily_returns.len(),
        family.folds[0].test[1].daily_returns.len() + family.folds[1].test[1].daily_returns.len()
    );
    let calls = family
        .folds
        .iter()
        .map(|fold| fold.train.len() + fold.test.len())
        .sum::<usize>();
    assert_eq!(calls, 8);
    let text = log_text(dir.path());
    assert_eq!(text.lines().count(), 16);
    let row: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(row["kind"], "RunStarted");
    assert_eq!(
        row["trial_id"],
        trial_id(
            row["config_id"].as_str().unwrap(),
            pins.pinned_root(),
            row["attempt"].as_u64().unwrap()
        )
        .as_str()
    );
}

#[test]
fn selection_refuses_a_fee_other_than_five_bps_and_writes_no_trial() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let folds = btcusdt_schedule(30).unwrap();
    let family = FamilyGrid {
        family: "S01".to_owned(),
        folds,
        points: vec![point(1, 0)],
    };
    let costs = Costs {
        taker_fee: FeeBps(0),
        slippage: SlippageBps(2),
    };
    let mut called = false;
    let err = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, usd(1)),
        &costs,
        |_, _, _, _| {
            called = true;
            Err::<WindowResult, String>("ran".to_owned())
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        WalkError::SelectionCosts {
            fee_bps: 0,
            slippage_bps: 2
        }
    ));
    assert!(!called);
    assert!(!dir.path().join("trials.jsonl").exists());
}

#[test]
fn holdout_window_is_refused_and_the_closure_is_not_called() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let day = DAY_US;
    let train = UtcRange {
        start: UtcMicros::from_micros(HOLDOUT_START_US - day),
        end: UtcMicros::from_micros(HOLDOUT_START_US + day),
    };
    let test = UtcRange {
        start: UtcMicros::from_micros(HOLDOUT_START_US + day),
        end: UtcMicros::from_micros(HOLDOUT_START_US + 2 * day),
    };
    let fold = Fold {
        index: 1,
        train_warmup: train,
        train,
        purge: UtcRange {
            start: train.end,
            end: test.start,
        },
        test_warmup: test,
        test,
        train_flat: forced_flat_at(train.end).unwrap(),
        test_flat: forced_flat_at(test.end).unwrap(),
    };
    let family = FamilyGrid {
        family: "hold".to_owned(),
        folds: vec![fold],
        points: vec![point(1, 1)],
    };
    let mut called = false;
    let err = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, usd(1_000)),
        &selection_costs(),
        |_, _, _, _| {
            called = true;
            Err::<WindowResult, String>("ran".to_owned())
        },
    )
    .unwrap_err();
    assert!(matches!(err, WalkError::Holdout { .. }));
    assert!(!called);
    let text = log_text(dir.path());
    assert!(text.contains("\"kind\":\"RunStarted\""));
    assert!(text.contains("\"kind\":\"RunAborted\""));
    assert!(text.contains("holdout"));
    assert!(!text.contains("RunFinished"));
    let rows: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["run_id"], rows[1]["run_id"]);
}

struct Seen {
    side: i64,
    start: i64,
    end: i64,
    fees: i64,
    funding: i64,
    closed: bool,
    fills: Vec<(i64, Side, i64, i64, i64)>,
}

fn price_at(minute: i64) -> i64 {
    let day = minute / 1_440;
    let into = minute % 1_440;
    let (open, close) = match day {
        0 => (100, 100),
        1 => (100, 130),
        2 => (130, 80),
        3 => (80, 40),
        4 => (40, 30),
        _ => (30, 30),
    };
    open + (close - open) * into / 1_439
}

fn toy_tape() -> parangsae_backtest::Tape {
    let start = date("2021-03-01").start_us().micros();
    let minutes = 5 * 1_440;
    let bars: Vec<_> = (0..minutes)
        .map(|minute| {
            let price = px(price_at(minute));
            bar_at(start + minute * MINUTE_US, price, price)
        })
        .collect();
    let last_open = bars.last().unwrap().open_time.micros();
    let mut funding = Vec::new();
    let mut ts = start;
    let step = 8 * 3_600_000_000;
    while ts <= last_open {
        funding.push(stamp(ts, 100_000));
        ts += step;
    }
    tape_marked(bars.clone(), bars, funding)
}

fn toy_schedule() -> Vec<Fold> {
    fold_schedule(&ScheduleSpec {
        sample_start: date("2021-03-01"),
        holdout_start: date("2021-03-06"),
        warmup_days: 1,
        train_days: 1,
        purge_days: 2,
        test_days: 1,
        step_days: 1,
        remainder_own_fold_days: 45,
    })
    .unwrap()
}

struct Bias {
    target: Target,
}

impl TargetSignal for Bias {
    fn on_bar(
        &mut self,
        view: Causal<'_, &[parangsae_core::ResampledBar]>,
        _funding: Causal<'_, &[parangsae_backtest::FundingObs]>,
    ) -> Option<SignalOutput> {
        let hour = view.value().last()?;
        let decision_ts = hour.close_time().checked_add_micros(1_000)?;
        Some(SignalOutput {
            decision_ts: DecisionTime::from_micros(decision_ts),
            bar_close_time_us: hour.close_time(),
            interval: hour.interval(),
            decision: Decision::Target(self.target),
        })
    }
}

fn run_toy(dir: &Path) -> (WalkForward, Vec<Seen>, String) {
    let pins = pins(dir);
    let trials = dir.join("trials.jsonl");
    let initial = usd(10_000);
    let tape = toy_tape();
    let folds = toy_schedule();
    let family = FamilyGrid {
        family: "toy".to_owned(),
        folds,
        points: vec![point(-1, 1), point(1, 1)],
    };
    let mut seen = Vec::new();
    let output = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |side, start, end, costs| {
            assert_eq!(costs.taker_fee, FeeBps(5));
            assert_eq!(costs.slippage, SlippageBps(2));
            let mut config = cfg(&tape.bars, 10_000);
            config.start = UtcMicros::from_micros(start);
            config.end = UtcMicros::from_micros(end);
            config.initial_cash = initial;
            config.forced_flat = vec![forced_flat_at(config.end).unwrap()];
            let fraction = if *side > 0 { 1.0 } else { -1.0 };
            let mut signal = Bias {
                target: Target::from_f64(fraction).unwrap(),
            };
            let report = run_targets(&tape, &mut signal, costs, &config)?;
            let result = window_result(initial, &report).expect("daily equity");
            check_window_result(initial.0, result.daily_returns.len(), &result)
                .expect("daily returns");
            seen.push(play_from(&tape, *side, start, end, &report));
            Ok::<WindowResult, EngineError>(result)
        },
    )
    .unwrap();
    (output, seen, log_text(dir))
}

fn play_from(
    tape: &parangsae_backtest::Tape,
    side: i64,
    start: i64,
    end: i64,
    report: &parangsae_backtest::Report,
) -> Seen {
    let fills = report
        .fills
        .iter()
        .map(|fill| {
            let open = tape
                .bars
                .iter()
                .find(|bar| bar.open_time == fill.ts)
                .unwrap()
                .open
                .ticks();
            (
                fill.ts.micros(),
                fill.side,
                fill.price.ticks(),
                fill.fee.0,
                open,
            )
        })
        .collect();
    Seen {
        side,
        start,
        end,
        fees: report.metrics.fees_paid.0,
        funding: report.metrics.funding_net.0,
        closed: report.trades.iter().all(|trade| trade.close_ts.is_some())
            && !report.trades.is_empty(),
        fills,
    }
}

#[test]
fn synthetic_tape_selects_the_long_train_and_flattens_with_costs() {
    let dir = tempfile::tempdir().unwrap();
    let (output, seen, log) = run_toy(dir.path());
    let fold = toy_schedule();
    let scored = &output.families[0].folds[0];
    assert_eq!(scored.train.len(), 2);
    assert!(scored.train[1].net_result_ticks > scored.train[0].net_result_ticks);
    assert!(scored.train[1].net_result_ticks > 0);
    assert!(scored.train[0].net_result_ticks < 0);
    assert_eq!(scored.selected_index, 1);
    assert_eq!(
        output.families[0].oos_round_trips,
        scored.test[1].round_trips
    );
    assert!(output.families[0].oos_round_trips >= 1);
    assert_eq!(
        output.families[0].stitched_oos_daily_returns,
        scored.test[1].daily_returns
    );
    assert_eq!(output.families[0].stitched_oos_equity[0], usd(10_000).0);
    let full = 5 * DAY_US;
    assert!(seen.iter().all(|call| call.end - call.start != full));
    assert!(seen
        .iter()
        .any(|call| call.end == fold[0].train.end.micros()));
    for call in &seen {
        assert!(call.closed);
        assert!(call.fees > 0);
        assert_ne!(call.funding, 0);
        let exit = call.fills.last().unwrap();
        assert!(exit.3 > 0);
        if call.side > 0 {
            assert_eq!(exit.1, Side::Sell);
            assert!(exit.2 < exit.4);
        } else {
            assert_eq!(exit.1, Side::Buy);
            assert!(exit.2 > exit.4);
        }
        let flat = if call.end == fold[0].train.end.micros() {
            fold[0].train_flat.start.micros()
        } else {
            fold[0].test_flat.start.micros()
        };
        assert_eq!(exit.0, flat);
    }
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("RunFinished"))
            .count(),
        4
    );
}

#[test]
fn purge_bars_are_not_used_for_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (output, seen, _) = run_toy(dir.path());
    let fold = &toy_schedule()[0];
    let train_end = fold.train.end.micros();
    let test_start = fold.test.start.micros();
    assert_eq!(test_start - train_end, 2 * DAY_US);
    let train_calls: Vec<_> = seen.iter().filter(|call| call.end == train_end).collect();
    assert_eq!(train_calls.len(), 2);
    assert!(train_calls
        .iter()
        .all(|call| call.start == fold.train.start.micros()));
    assert!(train_calls.iter().all(|call| call.end <= train_end));
    let tape = toy_tape();
    let initial = usd(10_000);
    let purge_close = tape
        .bars
        .iter()
        .filter(|bar| bar.open_time.micros() >= train_end && bar.open_time.micros() < test_start)
        .next_back()
        .expect("purge bar");
    assert!(purge_close.close.ticks() < px(50).ticks());
    assert!(train_calls
        .iter()
        .all(|call| call.end <= purge_close.open_time.micros()));
    let short_included = window_net(&tape, -1, fold.train.start.micros(), test_start, initial);
    let long_included = window_net(&tape, 1, fold.train.start.micros(), test_start, initial);
    assert!(short_included > long_included);
    assert_eq!(output.families[0].folds[0].selected_index, 1);
}

fn window_net(
    tape: &parangsae_backtest::Tape,
    side: i64,
    start: i64,
    end: i64,
    initial: Cash,
) -> i64 {
    let mut config = cfg(tape.bars.as_slice(), 10_000);
    config.start = UtcMicros::from_micros(start);
    config.end = UtcMicros::from_micros(end);
    config.initial_cash = initial;
    config.forced_flat = vec![forced_flat_at(config.end).unwrap()];
    let fraction = if side > 0 { 1.0 } else { -1.0 };
    let mut signal = Bias {
        target: Target::from_f64(fraction).unwrap(),
    };
    let report = run_targets(tape, &mut signal, &selection_costs(), &config).unwrap();
    report.equity_end().unwrap().0 - initial.0
}

#[test]
fn the_same_tape_run_is_deterministic() {
    let left_dir = tempfile::tempdir().unwrap();
    let right_dir = tempfile::tempdir().unwrap();
    let (left, left_seen, left_log) = run_toy(left_dir.path());
    let (right, right_seen, right_log) = run_toy(right_dir.path());
    assert_eq!(left, right);
    assert_eq!(left_log, right_log);
    assert_eq!(left_seen.len(), right_seen.len());
    for (a, b) in left_seen.iter().zip(&right_seen) {
        assert_eq!(a.side, b.side);
        assert_eq!(a.start, b.start);
        assert_eq!(a.end, b.end);
        assert_eq!(a.fees, b.fees);
        assert_eq!(a.funding, b.funding);
        assert_eq!(a.fills, b.fills);
    }
}
