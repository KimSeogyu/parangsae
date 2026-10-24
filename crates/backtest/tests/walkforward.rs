//! Walk-forward selection on a synthetic tape, plus the ledger and holdout gate.
//!
//! The tape is six UTC days from 2021-03-01. Warmup is flat, the two train days
//! rise, the two purge days crash, and the test day keeps falling. The long
//! config is the known train winner on annual Sharpe. A window that includes
//! the purge crash would pick the short config instead.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use parangsae_backtest::{
    btcusdt_schedule, canonical_config_id, check_window_result, daily_return_tick, fold_schedule,
    forced_flat_at, run, run_targets, run_walk_forward, selection_costs, sharpe_ticks, trial_id,
    window_result, BarView, Decision, EngineError, FamilyGrid, Fold, GridPoint, RunConfig,
    ScheduleSpec, SignalOutput, Strategy, StrategyOutput, SweepPins, Target, TargetSignal,
    TrialSpec, UtcDate, UtcRange, WalkContext, WalkError, WalkForward, WindowResult, FEE_GRID_BPS,
    HOLDOUT_START_US, STRESS_SLIPPAGE_BPS,
};
use parangsae_core::{Causal, DecisionTime, Interval, Qty, Side, UtcMicros};
use parangsae_sim::{Cash, Costs, FeeBps, MarketOrder, SlippageBps};
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

fn scaled_path(initial: i64, template: &[i64]) -> Vec<i64> {
    let base = template[0];
    template
        .iter()
        .map(|value| initial / base * value)
        .collect()
}

fn from_levels(levels: Vec<i64>, round_trips: u64) -> WindowResult {
    let daily_returns = levels
        .windows(2)
        .map(|pair| daily_return_tick(pair[0], pair[1]).unwrap())
        .collect();
    let net_result_ticks = levels.last().copied().unwrap() - levels[0];
    WindowResult {
        daily_returns,
        equity_ticks: levels,
        net_result_ticks,
        round_trips,
        fill_notional_ticks: 0,
        max_dd_ticks: 0,
        accounting: parangsae_backtest::WindowAccounting::ZERO,
        h4: None,
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
        fill_notional_ticks: 0,
        max_dd_ticks: 0,
        accounting: parangsae_backtest::WindowAccounting::ZERO,
        h4: None,
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
    let identity = UtcRange {
        start: folds[0].train_warmup.start,
        end: folds[0].test.end,
    };
    let family = FamilyGrid {
        family: "tie".to_owned(),
        slug: String::new(),
        prereg_sha256: String::new(),
        folds,
        points: vec![point(1, 3), point(2, 4)],
    };
    let output = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |config, _warmup, start, end, _| {
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
    assert_eq!(fold.train[0].config_id, fold.test[0].config_id);
    assert_ne!(fold.train[0].config_id, fold.train[1].config_id);
    assert_eq!(output.n_trials, 2);
    assert_eq!(output.families[0].n_trials, 2);
    assert_eq!(output.families[0].fee_replays.len(), 5);
    assert_eq!(
        output.families[0].fee_replays[2].daily_returns,
        output.families[0].stitched_oos_daily_returns
    );
    assert_eq!(output.families[0].oos_round_trips, 1);
    assert_eq!(
        output.families[0].stitched_oos_daily_returns,
        fold.test[0].daily_returns
    );

    let mut spec = TrialSpec {
        run_id: fold.train[0].run_id.clone(),
        config_id: String::new(),
        family: "tie".to_owned(),
        prereg_sha256: String::new(),
        params: point(1, 3).params,
        attempt: 3,
        ts: identity.start,
        run: RunConfig {
            initial_cash: initial,
            warmup_start: identity.start,
            start: identity.start,
            end: identity.end,
            holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
            forced_flat: Vec::new(),
            flatten_before: None,
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
    let started: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|row: &Value| row["kind"] == "RunStarted")
        .collect();
    assert_eq!(started.len(), 4);
    assert!(started.iter().all(|row| row["fold"] == 1));
    assert!(started.iter().any(|row| row["segment"] == "train"));
    assert!(started.iter().any(|row| row["segment"] == "test"));
    let ids: BTreeSet<_> = started
        .iter()
        .map(|row| row["config_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
}

#[test]
fn a_later_config_wins_on_train_sharpe_and_oos_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(1_000);
    let wild = scaled_path(initial.0, &[1_000, 8_000, 3_000]);
    let mild = scaled_path(initial.0, &[1_000, 1_050, 1_100]);
    let wild_sharpe = sharpe_ticks(&wild).unwrap().1.unwrap().ticks();
    let mild_sharpe = sharpe_ticks(&mild).unwrap().1.unwrap().ticks();
    assert!(mild_sharpe > wild_sharpe);
    assert!(wild.last().unwrap() - wild[0] > mild.last().unwrap() - mild[0]);
    let folds = fold_schedule(&ScheduleSpec {
        sample_start: date("2021-01-01"),
        holdout_start: date("2021-07-03"),
        warmup_days: 0,
        train_days: 2,
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
        slug: String::new(),
        prereg_sha256: String::new(),
        folds,
        points: vec![point(1, 0), point(2, 0)],
    };
    let output = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |config, _warmup, start, end, _| {
            let days = usize::try_from((end - start) / DAY_US).unwrap();
            let is_train = trains.contains(&(start, end));
            if is_train {
                assert_eq!(days, 2);
                let levels = if *config == 1 {
                    wild.clone()
                } else {
                    mild.clone()
                };
                return Ok::<WindowResult, String>(from_levels(levels, 0));
            }
            Ok::<WindowResult, String>(shaped(
                initial.0,
                days,
                *config * 1_000,
                u64::try_from(*config).unwrap(),
            ))
        },
    )
    .unwrap();
    let family = &output.families[0];
    assert!(family.folds[0].train[0].net_result_ticks > family.folds[0].train[1].net_result_ticks);
    assert!(
        family.folds[0].train[1].sharpe_annual_ticks > family.folds[0].train[0].sharpe_annual_ticks
    );
    assert_eq!(family.folds[0].selected_index, 1);
    assert_eq!(family.folds[1].selected_index, 1);
    assert_eq!(
        family.fee_replays[2].daily_returns,
        family.stitched_oos_daily_returns
    );
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
fn an_equal_train_sharpe_keeps_the_earlier_config() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(1_000);
    let small = scaled_path(initial.0, &[1_000, 1_000, 1_100]);
    let big = scaled_path(initial.0, &[1_000, 1_000, 2_000]);
    assert_eq!(
        sharpe_ticks(&small).unwrap().1.unwrap().ticks(),
        sharpe_ticks(&big).unwrap().1.unwrap().ticks()
    );
    assert!(big.last().unwrap() - big[0] > small.last().unwrap() - small[0]);
    let folds = fold_schedule(&ScheduleSpec {
        sample_start: date("2021-01-01"),
        holdout_start: date("2021-01-06"),
        warmup_days: 0,
        train_days: 2,
        purge_days: 1,
        test_days: 2,
        step_days: 2,
        remainder_own_fold_days: 45,
    })
    .unwrap();
    assert_eq!(folds.len(), 1);
    let train_end = folds[0].train.end.micros();
    let family = FamilyGrid {
        family: "tie-sharpe".to_owned(),
        slug: String::new(),
        prereg_sha256: String::new(),
        folds,
        points: vec![point(1, 0), point(2, 0)],
    };
    let output = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |config, _warmup, start, end, _| {
            let days = usize::try_from((end - start) / DAY_US).unwrap();
            if end == train_end {
                let levels = if *config == 1 {
                    small.clone()
                } else {
                    big.clone()
                };
                return Ok::<WindowResult, String>(from_levels(levels, 0));
            }
            Ok::<WindowResult, String>(shaped(initial.0, days, 1, 1))
        },
    )
    .unwrap();
    let fold = &output.families[0].folds[0];
    assert!(fold.train[1].net_result_ticks > fold.train[0].net_result_ticks);
    assert_eq!(
        fold.train[0].sharpe_annual_ticks,
        fold.train[1].sharpe_annual_ticks
    );
    assert_eq!(fold.selected_index, 0);
    assert_eq!(
        output.families[0].fee_replays[2].sharpe_annual_ticks,
        fold.test[0].sharpe_annual_ticks
    );
    assert_eq!(
        output.families[0]
            .fee_replays
            .iter()
            .map(|row| (row.fee_bps, row.slippage_bps))
            .collect::<Vec<_>>(),
        vec![
            (FEE_GRID_BPS[0], 2),
            (FEE_GRID_BPS[1], 2),
            (FEE_GRID_BPS[2], 2),
            (FEE_GRID_BPS[3], 2),
            (5, STRESS_SLIPPAGE_BPS),
        ]
    );
}

#[test]
fn s01_logs_twelve_config_trials_not_one_per_fold() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(1_000);
    let folds = btcusdt_schedule(30).unwrap();
    assert_eq!(folds.len(), 15);
    let bounds = UtcRange {
        start: folds[0].train_warmup.start,
        end: folds.last().unwrap().test.end,
    };
    let points: Vec<_> = (0..12).map(|side| point(side, 0)).collect();
    let family = FamilyGrid {
        family: "S01".to_owned(),
        slug: String::new(),
        prereg_sha256: String::new(),
        folds,
        points,
    };
    let output = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |_config, _warmup, start, end, _| {
            let days = usize::try_from((end - start) / DAY_US).unwrap();
            Ok::<WindowResult, String>(shaped(initial.0, days, 1, 0))
        },
    )
    .unwrap();
    assert_eq!(output.n_trials, 12);
    assert_eq!(output.families[0].n_trials, 12);
    let runs = output.families[0]
        .folds
        .iter()
        .map(|fold| fold.train.len() + fold.test.len())
        .sum::<usize>();
    assert_eq!(runs, 12 * 15 * 2);
    let config_id = output.families[0].folds[0].train[0].config_id.clone();
    assert!(output.families[0].folds.iter().all(|fold| {
        fold.train[0].config_id == config_id
            && fold.test[0].config_id == config_id
            && fold.train[0].trial_id == fold.test[0].trial_id
            && fold.train[0].run_id != fold.test[0].run_id
    }));
    let mut spec = TrialSpec {
        run_id: String::new(),
        config_id: String::new(),
        family: "S01".to_owned(),
        prereg_sha256: String::new(),
        params: point(0, 0).params,
        attempt: 0,
        ts: bounds.start,
        run: RunConfig {
            initial_cash: initial,
            warmup_start: bounds.start,
            start: bounds.start,
            end: bounds.end,
            holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
            forced_flat: Vec::new(),
            flatten_before: None,
            mark_gaps: Vec::new(),
            interval: Interval::ONE_HOUR,
            lot_ticks: 100_000,
        },
    };
    spec.config_id = canonical_config_id(&spec);
    assert_eq!(config_id, spec.config_id);
    let text = log_text(dir.path());
    assert_eq!(text.matches("RunStarted").count(), 360);
    assert_eq!(text.matches("RunFinished").count(), 360);
    let started: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|row| row["kind"] == "RunStarted")
        .collect();
    let ids: BTreeSet<_> = started
        .iter()
        .map(|row| row["config_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 12);
    let fold_ids: BTreeSet<_> = started
        .iter()
        .map(|row| row["fold"].as_u64().unwrap())
        .collect();
    assert_eq!(fold_ids.len(), 15);
    let segments: BTreeSet<_> = started
        .iter()
        .map(|row| row["segment"].as_str().unwrap())
        .collect();
    assert_eq!(segments, BTreeSet::from(["train", "test"]));
}

#[test]
fn selection_refuses_a_fee_other_than_five_bps_and_writes_no_trial() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let folds = btcusdt_schedule(30).unwrap();
    let family = FamilyGrid {
        family: "S01".to_owned(),
        slug: String::new(),
        prereg_sha256: String::new(),
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
        |_, _, _, _, _| {
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
        slug: String::new(),
        prereg_sha256: String::new(),
        folds: vec![fold],
        points: vec![point(1, 1)],
    };
    let mut called = false;
    let err = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, usd(1_000)),
        &selection_costs(),
        |_, _, _, _, _| {
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
    assert_eq!(rows[0]["fold"], 1);
    assert_eq!(rows[0]["segment"], "train");
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
        2 => (130, 160),
        3 => (160, 80),
        4 => (80, 40),
        5 => (40, 30),
        _ => (30, 30),
    };
    open + (close - open) * into / 1_439
}

fn toy_tape() -> parangsae_backtest::Tape {
    let start = date("2021-03-01").start_us().micros();
    let minutes = 6 * 1_440;
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
        holdout_start: date("2021-03-07"),
        warmup_days: 1,
        train_days: 2,
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
        _book: parangsae_backtest::BookState,
    ) -> Option<StrategyOutput> {
        let hour = view.value().last()?;
        let decision_ts = hour.close_time().checked_add_micros(1_000)?;
        Some(StrategyOutput::target(SignalOutput {
            decision_ts: DecisionTime::from_micros(decision_ts),
            bar_close_time_us: hour.close_time(),
            interval: hour.interval(),
            decision: Decision::Target(self.target),
            accept: 0,
        }))
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
        slug: String::new(),
        prereg_sha256: String::new(),
        folds,
        points: vec![point(-1, 1), point(1, 1)],
    };
    let mut seen = Vec::new();
    let output = run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |side, warmup, start, end, costs| {
            assert!(warmup < start);
            let mut config = cfg(&tape.bars, 10_000);
            config.warmup_start = UtcMicros::from_micros(warmup);
            config.start = UtcMicros::from_micros(start);
            config.end = UtcMicros::from_micros(end);
            config.initial_cash = initial;
            config.forced_flat = vec![forced_flat_at(config.end).unwrap()];
            config.flatten_before = Some(config.end);
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
    for grid_index in 0..2 {
        for segment in [
            parangsae_backtest::Segment::Train,
            parangsae_backtest::Segment::Test,
        ] {
            let mut scenarios = vec![(5, 2)];
            scenarios.extend(
                scored
                    .cost_replays
                    .iter()
                    .filter(|r| r.run.grid_index == grid_index && r.segment == segment)
                    .map(|r| (r.fee_bps, r.slippage_bps)),
            );
            scenarios.sort();
            assert_eq!(scenarios, vec![(2, 2), (4, 2), (5, 2), (5, 5), (6, 2)]);
        }
    }
    assert!(scored.train[1].net_result_ticks > scored.train[0].net_result_ticks);
    assert!(scored.train[1].net_result_ticks > 0);
    assert!(scored.train[0].net_result_ticks < 0);
    assert!(scored.train[1].sharpe_annual_ticks > scored.train[0].sharpe_annual_ticks);
    assert_eq!(scored.train[1].config_id, scored.test[1].config_id);
    assert_eq!(output.n_trials, 2);
    assert_eq!(scored.selected_index, 1);
    let replays = &output.families[0].fee_replays;
    assert_eq!(replays.len(), 5);
    assert_eq!(
        replays[2].daily_returns,
        output.families[0].stitched_oos_daily_returns
    );
    assert!(replays[4].net_result_ticks < replays[2].net_result_ticks);
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
    let full = 6 * DAY_US;
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
    assert_eq!(train_calls.len(), 2 * 5);
    assert!(train_calls
        .iter()
        .all(|call| call.start == fold.train.start.micros()));
    assert!(train_calls.iter().all(|call| call.end <= train_end));
    let tape = toy_tape();
    let initial = usd(10_000);
    let purge_close = tape
        .bars
        .iter()
        .rfind(|bar| bar.open_time.micros() >= train_end && bar.open_time.micros() < test_start)
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
    config.warmup_start = config.start;
    config.end = UtcMicros::from_micros(end);
    config.initial_cash = initial;
    config.forced_flat = vec![forced_flat_at(config.end).unwrap()];
    config.flatten_before = Some(config.end);
    let fraction = if side > 0 { 1.0 } else { -1.0 };
    let mut signal = Bias {
        target: Target::from_f64(fraction).unwrap(),
    };
    let report = run_targets(tape, &mut signal, &selection_costs(), &config).unwrap();
    report.equity_end().unwrap().0 - initial.0
}

#[test]
fn warmup_changes_the_first_decision_and_is_not_scored() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(10_000);
    let start_us = date("2021-03-01").start_us().micros();
    let minutes = 5 * 1_440;
    let bars: Vec<_> = (0..minutes)
        .map(|minute| {
            let day = minute / 1_440;
            let price = px(if day == 0 { 500 } else { 100 });
            bar_at(start_us + minute * MINUTE_US, price, price)
        })
        .collect();
    let last_open = bars.last().unwrap().open_time.micros();
    let mut funding = Vec::new();
    let mut ts = start_us;
    let step = 8 * 3_600_000_000;
    while ts <= last_open {
        funding.push(stamp(ts, 100_000));
        ts += step;
    }
    let tape = tape_marked(bars.clone(), bars, funding);
    let folds = fold_schedule(&ScheduleSpec {
        sample_start: date("2021-03-01"),
        holdout_start: date("2021-03-06"),
        warmup_days: 1,
        train_days: 1,
        purge_days: 2,
        test_days: 1,
        step_days: 1,
        remainder_own_fold_days: 45,
    })
    .unwrap();
    let train_start = folds[0].train.start.micros();
    let train_warmup = folds[0].train_warmup.start.micros();
    assert!(train_warmup < train_start);
    let mut warmed = None;
    let family = FamilyGrid {
        family: "warm".to_owned(),
        slug: String::new(),
        prereg_sha256: String::new(),
        folds,
        points: vec![point(1, 0)],
    };
    run_walk_forward(
        &[family],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |_, warmup, start, end, costs| {
            assert!(warmup < start);
            let mut config = cfg(&tape.bars, 10_000);
            config.warmup_start = UtcMicros::from_micros(warmup);
            config.start = UtcMicros::from_micros(start);
            config.end = UtcMicros::from_micros(end);
            config.initial_cash = initial;
            config.forced_flat = vec![forced_flat_at(config.end).unwrap()];
            config.flatten_before = Some(config.end);
            let mut signal = Spike;
            let report = run_targets(&tape, &mut signal, costs, &config)?;
            if start == train_start && costs.taker_fee.0 == 5 && costs.slippage.0 == 2 {
                assert_eq!(warmup, train_warmup);
                let first = report.signal_outputs.first().expect("decision").decision;
                warmed = Some(first.target_ticks());
                assert!(report
                    .signal_outputs
                    .iter()
                    .all(|output| output.bar_close_time_us.micros() >= start));
                assert!(report.fills.iter().all(|fill| fill.ts.micros() >= start));
                assert!(report
                    .equity_curve
                    .iter()
                    .all(|(ts, _)| ts.micros() >= start));
                assert!(report
                    .funding
                    .iter()
                    .all(|event| event.ts.micros() >= start));
                assert!(report
                    .daily_equity
                    .iter()
                    .all(|(day, _)| day.start_us().micros() >= start));
                let first_fill = report
                    .fills
                    .iter()
                    .map(|fill| fill.ts.micros())
                    .min()
                    .expect("fill");
                assert!(report
                    .equity_curve
                    .iter()
                    .filter(|(ts, _)| ts.micros() < first_fill)
                    .all(|(_, cash)| cash.0 == initial.0));
            }
            let result = window_result(initial, &report).expect("daily equity");
            Ok::<WindowResult, EngineError>(result)
        },
    )
    .unwrap();
    let warmed_ticks = warmed.expect("train decision");
    assert!(warmed_ticks > 0);

    let mut cold = cfg(&tape.bars, 10_000);
    cold.warmup_start = UtcMicros::from_micros(train_start);
    cold.start = cold.warmup_start;
    cold.end = UtcMicros::from_micros(train_start + DAY_US);
    cold.initial_cash = initial;
    cold.forced_flat = vec![forced_flat_at(cold.end).unwrap()];
    cold.flatten_before = Some(cold.end);
    let cold_report = run_targets(&tape, &mut Spike, &selection_costs(), &cold).unwrap();
    let cold_ticks = cold_report
        .signal_outputs
        .first()
        .expect("cold decision")
        .decision
        .target_ticks();
    assert!(cold_ticks < 0);
    assert_ne!(warmed_ticks, cold_ticks);
}

#[test]
fn a_mid_history_window_requires_its_fixed_scheduled_terminal() {
    let start_us = date("2022-06-14").start_us().micros();
    let minutes = 2 * 1_440;
    let bars: Vec<_> = (0..minutes)
        .map(|minute| {
            let day = minute / 1_440;
            let price = px(if day == 0 { 500 } else { 100 });
            let mut bar = bar_at(start_us + minute * MINUTE_US, price, price);
            if minute + 1 == minutes {
                bar.count = 0;
            }
            bar
        })
        .collect();
    let last_open = bars.last().unwrap().open_time.micros();
    assert_eq!(
        last_open,
        date("2022-06-15").start_us().micros() + DAY_US - MINUTE_US
    );
    assert_eq!(bars[bars.len() - 1].count, 0);
    assert_eq!(bars[bars.len() - 2].count, 1);
    let mut funding = Vec::new();
    let mut ts = start_us;
    let step = 8 * 3_600_000_000;
    while ts <= last_open {
        funding.push(stamp(ts, 100_000));
        ts += step;
    }
    let tape = tape_marked(bars.clone(), bars, funding);
    let scored = date("2022-06-15").start_us();
    let end = date("2022-06-16").start_us();
    let flat_open = end.micros() - MINUTE_US;
    let initial = usd(10_000);
    let mut config = cfg(&tape.bars, 10_000);
    config.warmup_start = UtcMicros::from_micros(start_us);
    config.start = scored;
    config.end = end;
    config.initial_cash = initial;
    config.forced_flat.clear();
    config.flatten_before = Some(end);

    let error = run_targets(&tape, &mut Spike, &selection_costs(), &config).unwrap_err();
    assert!(
        matches!(error, EngineError::UnfilledBoundaryClose { boundary }
        if boundary.micros() == flat_open)
    );

    // The same scheduled key, made tradable, is the active positive control.
    // No earlier open is selected from the future count pattern.
    let mut tradable = tape.clone();
    tradable.bars.last_mut().unwrap().count = 1;
    let report = run_targets(&tradable, &mut Spike, &selection_costs(), &config).unwrap();
    let first = report.signal_outputs.first().expect("decision").decision;
    assert!(first.target_ticks() > 0);
    assert!(report
        .signal_outputs
        .iter()
        .all(|output| output.bar_close_time_us >= scored));
    assert!(report.fills.iter().all(|fill| fill.ts >= scored));
    assert!(report
        .equity_curve
        .iter()
        .all(|(ts, _)| ts.micros() >= scored.micros()));
    let exit = report.fills.last().expect("flatten fill");
    assert_eq!(exit.ts.micros(), flat_open);
    assert_eq!(exit.side, Side::Sell);
    assert_eq!(
        report
            .fills
            .iter()
            .filter(|fill| fill.ts.micros() == last_open)
            .count(),
        1
    );
    assert!(report.trades.iter().all(|trade| trade.close_ts.is_some()));
    assert!(!report.trades.is_empty());

    let mut held = config.clone();
    held.flatten_before = None;
    let held = run_targets(&tape, &mut Spike, &selection_costs(), &held).unwrap();
    assert!(held.fills.iter().any(|fill| fill.side == Side::Buy));
    assert!(held
        .fills
        .iter()
        .all(|fill| fill.ts.micros() != last_open - MINUTE_US && fill.ts.micros() != last_open));
}

#[test]
fn warmup_on_run_changes_the_first_order_and_is_not_scored() {
    let start_us = date("2022-06-14").start_us().micros();
    let minutes = 2 * 1_440;
    let bars: Vec<_> = (0..minutes)
        .map(|minute| {
            let day = minute / 1_440;
            let price = px(if day == 0 { 500 } else { 100 });
            bar_at(start_us + minute * MINUTE_US, price, price)
        })
        .collect();
    let last_open = bars.last().unwrap().open_time.micros();
    let mut funding = Vec::new();
    let mut ts = start_us;
    let step = 8 * 3_600_000_000;
    while ts <= last_open {
        funding.push(stamp(ts, 100_000));
        ts += step;
    }
    let tape = tape_marked(bars.clone(), bars, funding);
    let scored = date("2022-06-15").start_us();
    let end = date("2022-06-16").start_us();
    let mut warm = cfg(&tape.bars, 10_000);
    warm.warmup_start = UtcMicros::from_micros(start_us);
    warm.start = scored;
    warm.end = end;
    warm.initial_cash = usd(10_000);
    let mut cold = warm.clone();
    cold.warmup_start = scored;

    let mut warm_signal = WarmOrder::default();
    let warm_report = run(&tape, &mut warm_signal, &selection_costs(), &warm).unwrap();
    let mut cold_signal = WarmOrder::default();
    let cold_report = run(&tape, &mut cold_signal, &selection_costs(), &cold).unwrap();

    let warm_fill = warm_report.fills.first().expect("warm fill");
    let cold_fill = cold_report.fills.first().expect("cold fill");
    assert_eq!(warm_fill.side, Side::Buy);
    assert_eq!(cold_fill.side, Side::Sell);
    assert!(warm_fill.ts >= scored);
    assert!(cold_fill.ts >= scored);
    assert!(warm_report
        .equity_curve
        .iter()
        .all(|(ts, _)| ts.micros() >= scored.micros()));
    assert!(cold_report
        .equity_curve
        .iter()
        .all(|(ts, _)| ts.micros() >= scored.micros()));
    let first_fill = warm_fill.ts.micros();
    assert!(warm_report
        .equity_curve
        .iter()
        .filter(|(ts, _)| ts.micros() < first_fill)
        .all(|(_, cash)| cash.0 == usd(10_000).0));
}

#[derive(Default)]
struct WarmOrder {
    placed: bool,
}

impl Strategy for WarmOrder {
    fn on_bar(&mut self, view: BarView<'_>) -> Option<MarketOrder> {
        if self.placed {
            return None;
        }
        self.placed = true;
        let hot = view.iter().any(|bar| bar.close.ticks() >= px(500).ticks());
        let side = if hot { Side::Buy } else { Side::Sell };
        Some(MarketOrder {
            side,
            qty: Qty::from_ticks(100_000_000).unwrap(),
        })
    }
}

struct Spike;

impl TargetSignal for Spike {
    fn on_bar(
        &mut self,
        view: Causal<'_, &[parangsae_core::ResampledBar]>,
        _funding: Causal<'_, &[parangsae_backtest::FundingObs]>,
        _book: parangsae_backtest::BookState,
    ) -> Option<StrategyOutput> {
        let hour = view.value().last()?;
        let decision_ts = hour.close_time().checked_add_micros(1_000)?;
        let hot = view
            .value()
            .iter()
            .any(|bar| bar.close().ticks() >= px(500).ticks());
        let target = Target::from_f64(if hot { 1.0 } else { -1.0 }).unwrap();
        Some(StrategyOutput::target(SignalOutput {
            decision_ts: DecisionTime::from_micros(decision_ts),
            bar_close_time_us: hour.close_time(),
            interval: hour.interval(),
            decision: Decision::Target(target),
            accept: 0,
        }))
    }
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

fn one_fold(family: &str) -> (FamilyGrid<i64>, i64) {
    let folds = fold_schedule(&ScheduleSpec {
        sample_start: date("2021-01-01"),
        holdout_start: date("2021-01-06"),
        warmup_days: 0,
        train_days: 2,
        purge_days: 1,
        test_days: 2,
        step_days: 2,
        remainder_own_fold_days: 45,
    })
    .unwrap();
    let train_end = folds[0].train.end.micros();
    (
        FamilyGrid {
            family: family.to_owned(),
            slug: String::new(),
            prereg_sha256: String::new(),
            folds,
            points: vec![point(1, 1), point(2, 1)],
        },
        train_end,
    )
}

fn run_family_choice(
    family: &str,
    train_extra: i64,
    test_extra: i64,
) -> parangsae_backtest::FamilyOutput {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(1_000);
    let (grid, train_end) = one_fold(family);
    run_walk_forward(
        &[grid],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |config, _warmup, _start, end, _| {
            let extra = if end == train_end {
                train_extra * *config
            } else {
                test_extra
            };
            Ok::<_, String>(shaped(initial.0, 2, extra, 1))
        },
    )
    .unwrap()
    .families
    .into_iter()
    .next()
    .unwrap()
}

#[test]
fn stay_flat_is_off_for_s01_and_on_when_every_h1_train_sharpe_is_non_positive() {
    let kept = run_family_choice("S01", -1_000, 5_000);
    assert!(!kept.folds[0].selected_flat);
    assert_eq!(kept.folds[0].selected_index, 0);
    assert!(kept.stitched_oos_daily_returns.iter().any(|ret| *ret != 0));

    let flat = run_family_choice("H1", -1_000, 5_000);
    assert!(flat.folds[0].selected_flat);
    assert_eq!(flat.folds[0].selected_config_id, "FLAT");
    assert!(flat.stitched_oos_daily_returns.iter().all(|ret| *ret == 0));
    assert_eq!(flat.oos_round_trips, 0);
    assert_eq!(flat.stitched_oos_daily_returns.len(), 2);

    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(1_000);
    let (grid, train_end) = one_fold("H1");
    let picked = run_walk_forward(
        &[grid],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |config, _warmup, _start, end, _| {
            let extra = if end == train_end {
                if *config == 1 {
                    -5_000_000_000
                } else {
                    5_000_000_000
                }
            } else {
                5_000
            };
            Ok::<_, String>(shaped(initial.0, 2, extra, 1))
        },
    )
    .unwrap();
    let picked = &picked.families[0];
    assert!(!picked.folds[0].selected_flat);
    assert_eq!(picked.folds[0].selected_index, 1);
    assert!(picked.oos_round_trips > 0);
}

#[test]
fn h2_reports_a_stay_flat_shadow_and_still_trades() {
    let dir = tempfile::tempdir().unwrap();
    let pins = pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let initial = usd(1_000);
    let (mut grid, train_end) = one_fold("H2");
    grid.points.truncate(1);
    let output = run_walk_forward(
        &[grid],
        &ctx(&trials, &pins, initial),
        &selection_costs(),
        |_config, _warmup, _start, end, _| {
            let extra = if end == train_end { -1_000 } else { 5_000 };
            Ok::<_, String>(shaped(initial.0, 2, extra, 1))
        },
    )
    .unwrap();
    let family = &output.families[0];
    assert!(!family.folds[0].selected_flat);
    assert!(family
        .stitched_oos_daily_returns
        .iter()
        .any(|ret| *ret != 0));
    let shadow = family.stay_flat_shadow.as_ref().expect("H2 shadow");
    assert_eq!(shadow.fold_flat, vec![true]);
    assert_eq!(shadow.daily_returns, vec![0, 0]);
}
