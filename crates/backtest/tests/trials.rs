//! Trial log L1–L3 and the multi-config batch.
//!
//! L1 injects a panic where the first bar would be processed. The started line
//! is already on disk and no finished line is written.
//! L2 refuses a second `RunStarted` with the same `run_id`.
//! L3 locks `trial_id` to sha256 of the JCS object `{attempt, config_id, pinned_root}`
//! for the frozen ledger preimage (64 `a`s, 64 `b`s, attempt 0).

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parangsae_backtest::fixture::Scripted;
use parangsae_backtest::{
    canonical_config_id, finish_trial, run_batch, run_fee_grid, trial_id, turnover_ratio,
    BatchError, BookState, Decision, FeeGridRunner, FlatBars, FlatReason, FundingObs, SignalOutput,
    SweepPins, Target, TargetEngine, TargetRunner, TargetSignal, TrialError, TrialMetrics,
    TrialSpec, UtcRange, HOLDOUT_START_US,
};
use parangsae_core::{Causal, DecisionTime, Interval, Ratio, ResampledBar, UtcMicros};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};
use serde_json::Value;
use support::{at, btc, buy, cfg, costs, sell, series, tape_of};

const L3_GOLDEN: &str = "f1d676a241133f91c32cfbd9f578c43d475a103194a281c63cee13d3d254b1db";
const TRIAL_EXTRA: &str = "348a2ff92849201353936a41b07acb0a187315168a133dc2d8ba4053522143ba";
const PIN_GOLDEN: &str = "680da465962c596b6d728e73cbc93bd156e2afbd7c3fe1e2ffa6bf03bb9fbdc5";
const ALPHA_SHA: &str = "8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8";

fn spec(run_id: &str, start: i64, end: i64) -> TrialSpec {
    seal(TrialSpec {
        run_id: run_id.to_owned(),
        config_id: String::new(),
        family: "S01".to_owned(),
        prereg_sha256: String::new(),
        params: BTreeMap::new(),
        attempt: 1,
        ts: UtcMicros::from_micros(start),
        run: parangsae_backtest::RunConfig {
            initial_cash: Cash(1_000 * 100_000_000),
            warmup_start: UtcMicros::from_micros(start),
            start: UtcMicros::from_micros(start),
            end: UtcMicros::from_micros(end),
            holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
            forced_flat: Vec::new(),
            flatten_before: None,
            mark_gaps: Vec::new(),
            interval: Interval::ONE_HOUR,
            lot_ticks: 100_000,
        },
    })
}

fn seal(mut spec: TrialSpec) -> TrialSpec {
    spec.config_id = canonical_config_id(&spec);
    spec
}

fn in_sample(run_id: &str) -> TrialSpec {
    let start = at(12, 0);
    spec(run_id, start, start + 3 * 60_000_000)
}

fn write_pins(dir: &Path) -> SweepPins {
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

fn batch_costs(slippage: u32) -> Costs {
    Costs {
        taker_fee: FeeBps(5),
        slippage: SlippageBps(slippage),
    }
}

fn zero_metrics() -> TrialMetrics {
    let zero = Ratio::from_ticks(0);
    TrialMetrics {
        total_return: zero,
        sharpe_annual_ticks: None,
        max_drawdown: zero,
        turnover: zero,
        flat_bars: FlatBars {
            funding_stale: 0,
            funding_slot_missing: 0,
        },
        net_return: [zero, zero, zero, zero],
        n_refused_fills: 0,
        n_dropped_entries: 0,
    }
}

struct PanicOnFirstBar;

impl parangsae_backtest::TargetRunner for PanicOnFirstBar {
    type Error = String;

    fn run_targets(
        &mut self,
        _costs: &Costs,
        _cfg: &parangsae_backtest::RunConfig,
    ) -> Result<TrialMetrics, String> {
        panic!("first bar");
    }
}

struct FlagRunner {
    called: Arc<AtomicBool>,
    metrics: TrialMetrics,
}

impl parangsae_backtest::TargetRunner for FlagRunner {
    type Error = String;

    fn run_targets(
        &mut self,
        _costs: &Costs,
        _cfg: &parangsae_backtest::RunConfig,
    ) -> Result<TrialMetrics, String> {
        self.called.store(true, Ordering::SeqCst);
        Ok(self.metrics)
    }
}

fn flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
fn l1_run_started_is_on_disk_before_compute_and_panic_writes_no_finish() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let results = dir.path().join("results.jsonl");
    let caught = catch_unwind(AssertUnwindSafe(|| {
        run_batch(
            &[in_sample("run-l1")],
            &pins,
            &trials,
            &results,
            &batch_costs(0),
            &mut PanicOnFirstBar,
        )
    }));
    assert!(caught.is_err());
    let text = fs::read_to_string(&trials).unwrap();
    assert!(text.contains("\"kind\":\"RunStarted\""));
    assert!(text.contains("\"prereg_sha256\":\"\""));
    assert!(text.contains("\"run_id\":\"run-l1\""));
    let expected = trial_id(&in_sample("run-l1").config_id, PIN_GOLDEN, 1);
    assert!(text.contains(&format!("\"trial_id\":\"{expected}\"")));
    assert!(!text.contains("RunFinished"));
    assert!(text.contains("RunAborted"));
    assert!(text.contains("panic"));
}

#[test]
fn prereg_sha256_is_on_the_ledger_before_compute() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let mut spec = in_sample("run-prereg");
    let sha = "ab".repeat(32);
    spec.prereg_sha256 = sha.clone();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        run_batch(
            &[spec],
            &pins,
            &trials,
            &dir.path().join("results.jsonl"),
            &batch_costs(0),
            &mut PanicOnFirstBar,
        )
    }));
    assert!(caught.is_err());
    let text = fs::read_to_string(&trials).unwrap();
    assert!(text.contains(&format!("\"prereg_sha256\":\"{sha}\"")));
    assert!(text.contains("\"kind\":\"RunStarted\""));
    assert!(!text.contains("RunFinished"));
}

#[test]
fn l2_duplicate_run_id_is_refused_and_the_log_is_not_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let results = dir.path().join("results.jsonl");
    let mut runner = FlagRunner {
        called: flag(),
        metrics: zero_metrics(),
    };
    run_batch(
        &[in_sample("run-l2")],
        &pins,
        &trials,
        &results,
        &batch_costs(0),
        &mut runner,
    )
    .unwrap();
    let before = fs::read(&trials).unwrap();
    runner.called.store(false, Ordering::SeqCst);
    let err = run_batch(
        &[in_sample("run-l2")],
        &pins,
        &trials,
        &results,
        &batch_costs(0),
        &mut runner,
    )
    .unwrap_err();
    assert!(matches!(err, BatchError::DuplicateRunId(_)));
    assert!(!runner.called.load(Ordering::SeqCst));
    let after = fs::read(&trials).unwrap();
    assert_eq!(&after[..before.len()], &before[..]);
    assert_eq!(after, before);
}

#[test]
fn l3_trial_id_golden() {
    let config_id = "a".repeat(64);
    let pinned_root = "b".repeat(64);
    assert_eq!(trial_id(&config_id, &pinned_root, 0), L3_GOLDEN);
    assert!(L3_GOLDEN.starts_with("f1d676a2"));
    assert!(L3_GOLDEN.ends_with("b1db"));
    assert_eq!(trial_id("S01", "pin-v2", 1), TRIAL_EXTRA);
}

#[test]
fn s01_style_config_id_is_refused_before_compute() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let called = flag();
    let mut runner = FlagRunner {
        called: Arc::clone(&called),
        metrics: zero_metrics(),
    };
    let mut sample = in_sample("run-s01");
    sample.config_id = "S01".to_owned();
    let err = run_batch(
        &[sample],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut runner,
    )
    .unwrap_err();
    assert!(matches!(err, BatchError::ConfigId { .. }));
    let mut impostor = in_sample("run-hex");
    impostor.config_id = "a".repeat(64);
    let err = run_batch(
        &[impostor],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut runner,
    )
    .unwrap_err();
    assert!(matches!(err, BatchError::ConfigId { .. }));
    assert!(!called.load(Ordering::SeqCst));
    assert!(!dir.path().join("trials.jsonl").exists());
}

#[test]
fn configs_differing_by_one_parameter_get_different_trial_ids() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let mut slow = in_sample("run-24");
    slow.params.insert("n".to_owned(), 24);
    let slow = seal(slow);
    let mut fast = in_sample("run-72");
    fast.params.insert("n".to_owned(), 72);
    let fast = seal(fast);
    assert_eq!(slow.family, fast.family);
    assert_eq!(slow.attempt, fast.attempt);
    assert_eq!(slow.run, fast.run);
    let rows = run_batch(
        &[slow, fast],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut FlagRunner {
            called: flag(),
            metrics: zero_metrics(),
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].config_id, rows[1].config_id);
    assert_ne!(rows[0].trial_id, rows[1].trial_id);
    let text = fs::read_to_string(dir.path().join("trials.jsonl")).unwrap();
    assert!(text.contains(&rows[0].trial_id));
    assert!(text.contains(&rows[1].trial_id));
}

#[test]
fn append_failure_refuses_the_run_before_compute() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let called = flag();
    let mut runner = FlagRunner {
        called: Arc::clone(&called),
        metrics: zero_metrics(),
    };
    let err = run_batch(
        &[in_sample("run-io")],
        &pins,
        dir.path(),
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut runner,
    )
    .unwrap_err();
    assert!(matches!(err, BatchError::Append(_)));
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn holdout_window_is_refused_counts_as_aborted_and_does_not_compute() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    let results = dir.path().join("results.jsonl");
    let called = flag();
    let mut runner = FlagRunner {
        called: Arc::clone(&called),
        metrics: zero_metrics(),
    };
    let start = HOLDOUT_START_US - 60_000_000;
    let spec = spec("run-holdout", start, HOLDOUT_START_US + 60_000_000);
    let config_id = spec.config_id.clone();
    let err = run_batch(
        &[spec],
        &pins,
        &trials,
        &results,
        &batch_costs(0),
        &mut runner,
    )
    .unwrap_err();
    assert!(matches!(err, BatchError::Holdout { .. }));
    assert!(!called.load(Ordering::SeqCst));
    let text = fs::read_to_string(&trials).unwrap();
    assert!(text.contains("RunStarted"));
    assert!(text.contains("RunAborted"));
    assert!(text.contains("holdout"));
    assert!(!text.contains("RunFinished"));
    let row: Value = serde_json::from_str(
        fs::read_to_string(&results)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(row["status"], "aborted");
    assert_eq!(row["config_id"], config_id);
}

#[test]
fn window_ending_at_holdout_is_in_sample() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let called = flag();
    let mut runner = FlagRunner {
        called: Arc::clone(&called),
        metrics: zero_metrics(),
    };
    let spec = spec("run-edge", HOLDOUT_START_US - 60_000_000, HOLDOUT_START_US);
    run_batch(
        &[spec],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut runner,
    )
    .unwrap();
    assert!(called.load(Ordering::SeqCst));
}

#[test]
fn start_equal_holdout_with_end_at_or_before_holdout_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    for (name, end) in [
        ("run-eq", HOLDOUT_START_US),
        ("run-before", HOLDOUT_START_US - 1),
    ] {
        let called = flag();
        let mut runner = FlagRunner {
            called: Arc::clone(&called),
            metrics: zero_metrics(),
        };
        let err = run_batch(
            &[spec(name, HOLDOUT_START_US, end)],
            &pins,
            &dir.path().join(format!("{name}-trials.jsonl")),
            &dir.path().join(format!("{name}-results.jsonl")),
            &batch_costs(0),
            &mut runner,
        )
        .unwrap_err();
        assert!(
            matches!(err, BatchError::Holdout { .. }),
            "{name} must be a holdout refusal, got {err}"
        );
        assert!(!called.load(Ordering::SeqCst), "{name} computed");
    }
}

#[test]
fn start_on_holdout_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let called = flag();
    let mut runner = FlagRunner {
        called: Arc::clone(&called),
        metrics: zero_metrics(),
    };
    let spec = spec("run-start", HOLDOUT_START_US, HOLDOUT_START_US + 60_000_000);
    let err = run_batch(
        &[spec],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut runner,
    )
    .unwrap_err();
    assert!(matches!(err, BatchError::Holdout { .. }));
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn batch_writes_one_row_per_config_with_fee_grid_and_flat_bars() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    assert_eq!(pins.pinned_root(), PIN_GOLDEN);
    let bars = series(at(12, 0), &[(100, 100), (100, 100), (110, 110)]);
    let tape = tape_of(bars.clone());
    let base = cfg(&bars, 1000);
    let direct = run_fee_grid(
        &tape,
        || Scripted::new(vec![Some(buy(btc(1))), Some(sell(btc(1)))]),
        &costs(5, 2),
        &base,
    )
    .unwrap();
    let mut engine = FeeGridRunner::new(&tape, || {
        Scripted::new(vec![Some(buy(btc(1))), Some(sell(btc(1)))])
    })
    .with_flat_bars(FlatBars {
        funding_stale: 3,
        funding_slot_missing: 2,
    });
    let mut second = in_sample("run-b");
    second.family = "S02".to_owned();
    second.attempt = 1;
    second.run = base.clone();
    let second = seal(second);
    let mut first = in_sample("run-a");
    first.run = base;
    let first = seal(first);
    let rows = run_batch(
        &[first, second],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(2),
        &mut engine,
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    let lines: Vec<_> = fs::read_to_string(dir.path().join("trials.jsonl"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].contains("RunStarted"));
    assert!(lines[1].contains("RunFinished"));
    assert!(lines[2].contains("RunStarted"));
    assert!(lines[3].contains("RunFinished"));
    let result_lines: Vec<_> = fs::read_to_string(dir.path().join("results.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect();
    assert_eq!(result_lines.len(), 2);
    assert_eq!(result_lines[0]["status"], "finished");
    assert_eq!(
        result_lines[0]["flat_bars"]["FundingStale"].as_u64(),
        Some(3)
    );
    assert_eq!(
        result_lines[0]["flat_bars"]["FundingSlotMissing"].as_u64(),
        Some(2)
    );
    assert_eq!(
        result_lines[0]["total_return_ticks"].as_i64(),
        Some(direct[2].1.metrics.total_return.ticks())
    );
    assert_eq!(
        result_lines[0]["max_drawdown_ticks"].as_i64(),
        Some(direct[2].1.metrics.max_drawdown.ticks())
    );
    for (index, bps) in ["2", "4", "5", "6"].iter().enumerate() {
        assert_eq!(
            result_lines[0]["net_return_ticks"][bps].as_i64(),
            Some(direct[index].1.metrics.total_return.ticks())
        );
    }
    assert_eq!(result_lines[0]["pinned_root"], PIN_GOLDEN);
    assert!(result_lines[0]["trial_id"].as_str().unwrap().len() == 64);
    assert_ne!(
        result_lines[0]["trial_id"], result_lines[1]["trial_id"],
        "config_id is inside the trial id"
    );
    let turnover = result_lines[0]["turnover_ticks"].as_i64().unwrap();
    assert!(turnover > 0);
}

#[test]
fn second_terminal_line_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let trials = dir.path().join("trials.jsonl");
    run_batch(
        &[in_sample("run-term")],
        &pins,
        &trials,
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut FlagRunner {
            called: flag(),
            metrics: zero_metrics(),
        },
    )
    .unwrap();
    let before = fs::read(&trials).unwrap();
    let err = finish_trial(&trials, "run-term", "digest").unwrap_err();
    assert!(matches!(err, TrialError::AlreadyTerminal(_)));
    assert_eq!(fs::read(&trials).unwrap(), before);
}

#[test]
fn slippage_and_fee_grid_are_recorded_on_each_results_row() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let bars = series(at(12, 0), &[(100, 100), (100, 100), (110, 110)]);
    let tape = tape_of(bars.clone());
    let base = cfg(&bars, 1000);
    let mut engine = FeeGridRunner::new(&tape, || {
        Scripted::new(vec![Some(buy(btc(1))), Some(sell(btc(1)))])
    });
    let mut first = in_sample("run-slip-a");
    first.run = base.clone();
    let first = seal(first);
    let mut second = in_sample("run-slip-b");
    second.run = base;
    second.params.insert("n".to_owned(), 72);
    let second = seal(second);
    run_batch(
        &[first, second],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(2),
        &mut engine,
    )
    .unwrap();
    let rows: Vec<Value> = fs::read_to_string(dir.path().join("results.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row["slippage_bps"].as_u64(), Some(2));
        assert_eq!(row["fee_grid_bps"], serde_json::json!([2, 4, 5, 6]));
    }
}

#[test]
fn passed_costs_change_the_trial_return() {
    let bars = series(at(12, 0), &[(100, 100), (100, 100), (110, 110)]);
    let tape = tape_of(bars.clone());
    let cfg = cfg(&bars, 1000);
    let make = || Scripted::new(vec![Some(buy(btc(1))), Some(sell(btc(1)))]);
    let mut engine = FeeGridRunner::new(&tape, make);
    let plain = Costs {
        taker_fee: FeeBps(0),
        slippage: SlippageBps(0),
    };
    let fee = Costs {
        taker_fee: FeeBps(50),
        slippage: SlippageBps(0),
    };
    let slip = Costs {
        taker_fee: FeeBps(0),
        slippage: SlippageBps(100),
    };
    let plain_return = engine
        .run_targets(&plain, &cfg)
        .unwrap()
        .total_return
        .ticks();
    let fee_return = engine.run_targets(&fee, &cfg).unwrap().total_return.ticks();
    let slip_return = engine
        .run_targets(&slip, &cfg)
        .unwrap()
        .total_return
        .ticks();
    assert!(fee_return < plain_return);
    assert!(slip_return < plain_return);
}

#[test]
fn engine_error_aborts_the_trial_and_the_next_config_still_runs() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let mut which = 0u32;
    let rows = run_batch(
        &[in_sample("run-bad"), in_sample("run-ok")],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut ClosureRunner {
            run: &mut |cfg: &parangsae_backtest::RunConfig| {
                which += 1;
                if cfg.start == in_sample("run-bad").run.start && which == 1 {
                    Err("missing mark".to_owned())
                } else {
                    Ok(zero_metrics())
                }
            },
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].status, "aborted");
    assert_eq!(rows[0].reason.as_deref(), Some("missing mark"));
    assert_eq!(rows[1].status, "finished");
    let text = fs::read_to_string(dir.path().join("trials.jsonl")).unwrap();
    assert_eq!(text.matches("RunStarted").count(), 2);
    assert_eq!(text.matches("RunAborted").count(), 1);
    assert_eq!(text.matches("RunFinished").count(), 1);
}

#[test]
fn pin_mismatch_refuses_before_any_trial() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("obj.txt"), b"beta").unwrap();
    let pin_path = dir.path().join("sweep-inputs.json");
    fs::write(
        &pin_path,
        format!(
            "{{\"schema\":\"sweep-runner-inputs-v2\",\"objects\":[{{\"path\":\"obj.txt\",\"sha256\":\"{ALPHA_SHA}\"}}]}}\n"
        ),
    )
    .unwrap();
    let err = SweepPins::load(&pin_path).unwrap_err();
    assert!(matches!(err, BatchError::PinMismatch { .. }));
}

#[test]
fn turnover_one_round_trip_over_a_year_is_point_two() {
    let notional = 100 * 100_000_000_i128;
    let equity = 1000 * 100_000_000_i64;
    let daily = vec![equity; 365];
    let ratio = turnover_ratio(notional * 2, &daily).unwrap();
    assert_eq!(ratio.ticks(), 20_000_000);
}

#[test]
fn config_holdout_start_refuses_a_window_before_the_default_instant() {
    let dir = tempfile::tempdir().unwrap();
    let pins = write_pins(dir.path());
    let start = at(12, 0);
    let mut spec = spec("run-early-holdout", start, start + 3 * 60_000_000);
    spec.run.holdout_start = UtcMicros::from_micros(start);
    let called = flag();
    let mut runner = FlagRunner {
        called: Arc::clone(&called),
        metrics: zero_metrics(),
    };
    let err = run_batch(
        &[spec],
        &pins,
        &dir.path().join("trials.jsonl"),
        &dir.path().join("results.jsonl"),
        &batch_costs(0),
        &mut runner,
    )
    .unwrap_err();
    assert!(matches!(err, BatchError::Holdout { .. }));
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn target_engine_reports_flat_reasons_and_obeys_forced_flat() {
    let start = at(12, 0);
    let bars = series(start, &vec![(50_000, 50_000); 180]);
    let tape = tape_of(bars);
    let open = cfg(&tape.bars, 10_000);
    let mut forced = open.clone();
    forced.forced_flat = vec![UtcRange {
        start: UtcMicros::from_micros(start),
        end: UtcMicros::from_micros(start + 3 * 3_600_000_000),
    }];
    let steps = vec![
        Decision::Flat(FlatReason::FundingStale),
        Decision::Flat(FlatReason::FundingSlotMissing),
        Decision::Target(Target::from_f64(1.0).unwrap()),
    ];
    let mut flats = TargetEngine::new(&tape, {
        let steps = steps.clone();
        move || Repeat::new(steps.clone())
    });
    let counted = flats.run_targets(&batch_costs(0), &open).unwrap();
    assert_eq!(counted.flat_bars.funding_stale, 1);
    assert_eq!(counted.flat_bars.funding_slot_missing, 1);

    let mut engine = TargetEngine::new(&tape, || {
        Repeat::new(vec![Decision::Target(Target::from_f64(1.0).unwrap()); 3])
    });
    let held = engine.run_targets(&batch_costs(0), &open).unwrap();
    let flattened = engine.run_targets(&batch_costs(0), &forced).unwrap();
    assert!(held.turnover.ticks() > 0);
    assert_eq!(flattened.turnover.ticks(), 0);
}

struct Repeat {
    steps: Vec<Decision>,
    cursor: usize,
}

impl Repeat {
    fn new(steps: Vec<Decision>) -> Self {
        Self { steps, cursor: 0 }
    }
}

impl TargetSignal for Repeat {
    fn on_bar(
        &mut self,
        view: Causal<'_, &[ResampledBar]>,
        _funding: Causal<'_, &[FundingObs]>,
        _book: BookState,
    ) -> Option<SignalOutput> {
        let bars = *view.value();
        let hour = bars.last()?;
        let decision = self.steps.get(self.cursor).copied()?;
        self.cursor += 1;
        let decision_ts = hour.close_time().checked_add_micros(1_000)?;
        Some(SignalOutput {
            decision_ts: DecisionTime::from_micros(decision_ts),
            bar_close_time_us: hour.close_time(),
            interval: hour.interval(),
            decision,
            accept: 0,
        })
    }
}

struct ClosureRunner<F> {
    run: F,
}

impl<F> parangsae_backtest::TargetRunner for ClosureRunner<F>
where
    F: FnMut(&parangsae_backtest::RunConfig) -> Result<TrialMetrics, String>,
{
    type Error = String;

    fn run_targets(
        &mut self,
        _costs: &Costs,
        cfg: &parangsae_backtest::RunConfig,
    ) -> Result<TrialMetrics, String> {
        (self.run)(cfg)
    }
}
