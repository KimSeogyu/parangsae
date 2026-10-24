//! Walk-forward over the sweep-input families.
//!
//! Each config is one trial. Train and test are both logged, and `config_id`
//! is the hash of the config on the family's outer window. Fold and segment
//! are fields on `RunStarted`. The out directory receives one OOS file per
//! family and `fold_daily_returns.csv`: one row per config, fold, and day for
//! each train (`IS_fold_k`) and test (`OOS_fold_k`) segment. The CSV is written
//! only after every trial has finished. Any aborted trial returns before that
//! write, so the file is not created. There is no full-window per-config file.
//!
//! S01, S02, S03, and S05 use the 30-day warmup schedule. S06 uses 365 days.
//! Selection and the headline row are 5 bps fee and 2 bps slippage, with
//! funding settled by the engine. The fee grid replays the selected test path
//! at 2/4/5/6 bps and at 5 bps fee with 5 bps slippage. Replays are not trials.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use parangsae_core::{Interval, UtcMicros, SCALE};

use crate::date::UtcDate;
use parangsae_sim::Cash;
use serde::Serialize;

use crate::batch::{canonical_config_id, SweepPins, TrialSpec, HOLDOUT_START_US};
use crate::engine::RunConfig;
use crate::metrics::total_return;
use crate::sweep::{load_configs, prepare_pinned_tape, ConfigRow};
use crate::walkforward::{
    btcusdt_schedule, run_walk_forward, selection_costs, FamilyGrid, FamilyOutput, GridPoint,
    SegmentRun, WalkContext, WalkForward, WindowAccounting, H1_WARMUP_DAYS, H3_WARMUP_DAYS,
    H4_WARMUP_DAYS, H5_WARMUP_DAYS, SESSION_WARMUP_DAYS, WARMUP_DAYS,
};
use crate::WindowConfig;
use parangsae_strategies::{truncation_probe, EARLIEST_US};

/// Parquet root used when `--data-root` is omitted.
pub const DEFAULT_DATA_ROOT: &str = "/workspace/data/parangsae";

/// What `wf_batch` was asked to run.
pub struct WfBatchRequest {
    pub inputs: PathBuf,
    pub data_root: PathBuf,
    pub out_dir: PathBuf,
    pub ledger: PathBuf,
    /// Ledger attempt. `trial_id` is `sha256` of `{attempt, config_id, pinned_root}`.
    pub attempt: u64,
}

/// Load the sweep file, check every `config_id`, and build one grid per family.
pub fn prepare_families(
    inputs: &Path,
    attempt: u64,
) -> Result<Vec<FamilyGrid<WindowConfig>>, String> {
    let loaded = load_configs(inputs)?;
    if loaded.slippage_bps != selection_costs().slippage.0 {
        return Err(format!(
            "slippage_bps {} in {} is not {}",
            loaded.slippage_bps,
            inputs.display(),
            selection_costs().slippage.0
        ));
    }
    if loaded.configs.is_empty() {
        return Err("sweep inputs have no configs".to_owned());
    }
    let cash = loaded.configs[0].initial_cash;
    if loaded.configs.iter().any(|row| row.initial_cash != cash) {
        return Err("configs do not share one initial cash".to_owned());
    }
    let mut order = Vec::new();
    let mut groups: BTreeMap<String, Vec<&ConfigRow>> = BTreeMap::new();
    for row in &loaded.configs {
        if !groups.contains_key(&row.family) {
            order.push(row.family.clone());
        }
        groups.entry(row.family.clone()).or_default().push(row);
    }
    let mut families = Vec::with_capacity(order.len());
    for name in order {
        let mut rows = groups.remove(&name).unwrap_or_default();
        rows.sort_by_key(|row| row.grid_index);
        let mut seen = BTreeSet::new();
        for row in &rows {
            if !seen.insert(row.grid_index) {
                return Err(format!("{name} repeats grid index {}", row.grid_index));
            }
        }
        let days = warmup_days(&name)?;
        let folds = btcusdt_schedule(days).map_err(|err| err.to_string())?;
        let start = folds[0].train_warmup.start;
        let end = folds
            .last()
            .ok_or_else(|| format!("{name} has no folds"))?
            .test
            .end;
        let slug = rows
            .first()
            .map(|row| row.slug.clone())
            .filter(|slug| !slug.is_empty())
            .unwrap_or_else(|| name.clone());
        let prereg_sha256 = rows
            .first()
            .map(|row| row.prereg_sha256.clone())
            .unwrap_or_default();
        let mut points = Vec::with_capacity(rows.len());
        for row in rows {
            if row.start_us != start.micros() || row.end_us != end.micros() {
                return Err(format!(
                    "{} {} window {}..{} is not the fold span {}..{}",
                    row.family,
                    row.grid_index,
                    row.start_us,
                    row.end_us,
                    start.micros(),
                    end.micros()
                ));
            }
            let id = window_config_id(
                &row.family,
                &row.params,
                row.initial_cash,
                start,
                end,
                attempt,
            )?;
            if id != row.config_id {
                return Err(format!(
                    "{} config_id {} is not the fold-window hash {id}",
                    row.family, row.config_id
                ));
            }
            points.push(GridPoint {
                config: WindowConfig {
                    family: row.family.clone(),
                    params: row.params.clone(),
                    initial_cash: row.initial_cash,
                },
                params: row.params.clone(),
                attempt,
            });
        }
        families.push(FamilyGrid {
            family: name,
            slug,
            prereg_sha256,
            folds,
            points,
        });
    }
    Ok(families)
}

/// Run every family on `request.data_root` and write OOS files under `out_dir`.
///
/// Both the output directory and ledger must be absent, including empty paths.
/// After config and pin validation, the directory is created atomically and
/// the ledger is opened with `create_new` before tape decoding or any report
/// or trial write. Claimed paths are kept on every later error.
///
/// Supported diagnostic runs use the checked launcher and require
/// `pinned_root_expected` in the inputs. Source and binary identity are checked
/// by Run Control outside this Rust API.
///
/// `fold_daily_returns.csv` is written only when every trial finishes. An
/// aborted trial, including a gate refusal or an engine error, returns here
/// before that write, so the file is absent.
pub fn run_wf_batch(request: &WfBatchRequest) -> Result<(), String> {
    let families = prepare_families(&request.inputs, request.attempt)?;
    let pins = SweepPins::load(&request.inputs).map_err(|err| err.to_string())?;
    let start_us = families[0].folds[0].train_warmup.start.micros();
    let end_us = families[0].folds.last().expect("folds").test.end.micros();
    let include_premium = families.iter().any(|family| family.family == "H5");
    let tape_read =
        prepare_pinned_tape(&request.data_root, &pins, start_us, end_us, include_premium)?;
    claim_run_paths(request)?;
    let tape = tape_read.load()?;
    write_run_log(&request.out_dir, &families, &tape)?;
    let cash = families[0].points[0].config.initial_cash;
    let fold = crate::sweep::FoldRunner::new(&tape);
    let ctx = WalkContext {
        pins: &pins,
        trials_path: &request.ledger,
        initial_cash: Cash(cash),
        holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
    };
    let mut result = run_walk_forward(
        &families,
        &ctx,
        &selection_costs(),
        |config, warmup, start, end, costs| fold.run_scored(config, warmup, start, end, costs),
    )
    .map_err(|err| err.to_string())?;
    for (family, output) in families.iter().zip(&mut result.families) {
        if family.family == "H2" {
            output.h2_member_sharpes = h2_member_diagnostics(&family.folds, cash, |config, f| {
                fold.run_scored(
                    config,
                    f.test_warmup.start.micros(),
                    f.test.start.micros(),
                    f.test.end.micros(),
                    &selection_costs(),
                )
            })?;
        }
    }
    write_oos(
        &request.out_dir,
        &result,
        &warmup_by_family(&families)?,
        cash,
    )?;
    write_fold_daily_returns(&request.out_dir, &result)?;
    crate::table::write_family_tables(
        &request.out_dir,
        &families,
        &result,
        &pins,
        &request.ledger,
    )?;
    Ok(())
}

fn claim_run_paths(request: &WfBatchRequest) -> Result<(), String> {
    if let Some(parent) = request.out_dir.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create {}: {err}", parent.display()))?;
        }
    }
    fs::create_dir(&request.out_dir)
        .map_err(|err| format!("claim fresh output {}: {err}", request.out_dir.display()))?;
    if let Some(parent) = request.ledger.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create {}: {err}", parent.display()))?;
        }
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request.ledger)
        .map_err(|err| format!("claim fresh ledger {}: {err}", request.ledger.display()))?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/common/run_admission.rs"]
mod admission_contract;

/// Fixed member diagnostics use the blend's folds and headline costs. They do
/// not call the trial ledger and cannot enter selection.
fn h2_member_diagnostics(
    folds: &[crate::Fold],
    cash: i64,
    mut run: impl FnMut(&WindowConfig, &crate::Fold) -> Result<crate::WindowResult, String>,
) -> Result<Vec<Option<i64>>, String> {
    let mut sharpes = Vec::with_capacity(12);
    for params in parangsae_strategies::h2_member_params() {
        let config = WindowConfig {
            family: "S01".into(),
            params,
            initial_cash: cash,
        };
        let mut daily = Vec::new();
        for fold in folds {
            daily.extend(run(&config, fold)?.daily_returns);
        }
        sharpes.push(
            crate::metrics::sharpe_of_return_ticks(&daily)
                .map_err(|e| e.to_string())?
                .1
                .map(|v| v.ticks()),
        );
    }
    Ok(sharpes)
}

fn warmup_by_family(
    families: &[FamilyGrid<WindowConfig>],
) -> Result<BTreeMap<String, i64>, String> {
    let mut out = BTreeMap::new();
    for family in families {
        out.insert(family.family.clone(), warmup_days(&family.family)?);
    }
    Ok(out)
}

fn warmup_days(family: &str) -> Result<i64, String> {
    match family {
        "S01" | "S02" | "S03" | "S05" | "H2" => Ok(WARMUP_DAYS),
        "S06" => Ok(SESSION_WARMUP_DAYS),
        "H1" => Ok(H1_WARMUP_DAYS),
        "H3" => Ok(H3_WARMUP_DAYS),
        "H4" => Ok(H4_WARMUP_DAYS),
        "H5" => Ok(H5_WARMUP_DAYS),
        other => Err(format!("no walk-forward schedule for {other}")),
    }
}

fn write_run_log(
    dir: &Path,
    families: &[FamilyGrid<WindowConfig>],
    tape: &crate::Tape,
) -> Result<(), String> {
    let (min_us, max_us) = tape_bounds(tape);
    let holdout_ok = min_us >= EARLIEST_US && max_us < HOLDOUT_START_US;
    let mut lines = vec![
        "holdout_guard partition_refusal=dates before 2020-12-18 and at or after 2025-10-01 are refused before open".to_owned(),
        format!(
            "holdout_guard max_key_us={max_us} min_key_us={min_us} holdout_start_us={HOLDOUT_START_US} sample_start_us={EARLIEST_US} passed={holdout_ok}"
        ),
    ];
    let mut lookahead_ok = true;
    for family in families {
        for (index, point) in family.points.iter().enumerate() {
            let probe = truncation_probe(&family.family, &point.params);
            let passed = probe.is_ok();
            if !passed {
                lookahead_ok = false;
            }
            lines.push(format!(
                "lookahead family={} grid_index={index} passed={passed} evidence=active_synthetic_prefix detail={:?}",
                family.family,
                probe.err()
            ));
        }
    }
    let path = dir.join("run_log.txt");
    fs::write(&path, lines.join("\n") + "\n")
        .map_err(|err| format!("write {}: {err}", path.display()))?;
    if !holdout_ok {
        return Err(format!("holdout guard failed: min {min_us} max {max_us}"));
    }
    if !lookahead_ok {
        return Err("lookahead probe failed".to_owned());
    }
    Ok(())
}

fn tape_bounds(tape: &crate::Tape) -> (i64, i64) {
    let mut min_us = i64::MAX;
    let mut max_us = i64::MIN;
    let mut touch = |ts: i64| {
        min_us = min_us.min(ts);
        max_us = max_us.max(ts);
    };
    for bar in tape.bars.iter().chain(tape.marks.iter()) {
        touch(bar.open_time.micros());
    }
    for stamp in &tape.funding {
        touch(stamp.funding_time.micros());
    }
    for row in &tape.premium {
        touch(row.open_time.micros());
    }
    if min_us == i64::MAX {
        (0, HOLDOUT_START_US)
    } else {
        (min_us, max_us)
    }
}

fn window_config_id(
    family: &str,
    params: &BTreeMap<String, i64>,
    cash: i64,
    start: UtcMicros,
    end: UtcMicros,
    attempt: u64,
) -> Result<String, String> {
    let spec = TrialSpec {
        run_id: String::new(),
        config_id: String::new(),
        family: family.to_owned(),
        prereg_sha256: String::new(),
        params: params.clone(),
        attempt,
        ts: start,
        run: RunConfig {
            initial_cash: Cash(cash),
            warmup_start: start,
            start,
            end,
            holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
            forced_flat: Vec::new(),
            flatten_before: None,
            mark_gaps: Vec::new(),
            interval: Interval::ONE_HOUR,
            lot_ticks: 100_000,
        },
    };
    Ok(canonical_config_id(&spec))
}

/// Write one `{family}.json` of stitched OOS results. Train rows are not written.
pub fn write_oos(
    dir: &Path,
    result: &WalkForward,
    warmup_days: &BTreeMap<String, i64>,
    initial_cash: i64,
) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
    for family in &result.families {
        let days = warmup_days
            .get(&family.family)
            .copied()
            .ok_or_else(|| format!("missing warmup for {}", family.family))?;
        let file = family_file(family, days, initial_cash)?;
        let h4 = if family.family == "H4" {
            Some(
                serde_json::to_string_pretty(&h4_file(family, initial_cash)?)
                    .map_err(|err| err.to_string())?,
            )
        } else {
            None
        };
        let path = dir.join(format!("{}.json", family.family));
        let text = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        fs::write(&path, format!("{text}\n"))
            .map_err(|err| format!("write {}: {err}", path.display()))?;
        if let Some(text) = h4 {
            let path = dir.join("H4-diagnostics.json");
            fs::write(&path, format!("{text}\n"))
                .map_err(|err| format!("write {}: {err}", path.display()))?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct H4WindowFile<'a> {
    run_id: &'a str,
    trial_id: &'a str,
    config_id: &'a str,
    grid_index: usize,
    fold: u32,
    segment: &'static str,
    fee_bps: u32,
    slippage_bps: u32,
    selected_test: bool,
    diagnostics: &'a crate::h4_report::H4WindowDiagnostics,
}

#[derive(Serialize)]
struct H4ScenarioFile {
    fee_bps: u32,
    slippage_bps: u32,
    selected: crate::h4_report::H4SelectedDiagnostics,
}

#[derive(Serialize)]
struct H4File<'a> {
    schema: &'static str,
    family: &'static str,
    claim: &'static str,
    windows: Vec<H4WindowFile<'a>>,
    selected_cost_scenarios: Vec<H4ScenarioFile>,
}

fn h4_window<'a>(
    row: &'a SegmentRun,
    fold: u32,
    segment: &'static str,
    fee_bps: u32,
    slippage_bps: u32,
    selected_test: bool,
) -> Result<H4WindowFile<'a>, String> {
    let diagnostics = row
        .h4
        .as_ref()
        .ok_or_else(|| format!("{} missing H4 diagnostics", row.run_id))?;
    if diagnostics.fee_bps != fee_bps
        || diagnostics.slippage_bps != slippage_bps
        || diagnostics.cash.net_ticks != i128::from(row.net_result_ticks)
    {
        return Err(format!("{} H4 diagnostics mismatch", row.run_id));
    }
    Ok(H4WindowFile {
        run_id: &row.run_id,
        trial_id: &row.trial_id,
        config_id: &row.config_id,
        grid_index: row.grid_index,
        fold,
        segment,
        fee_bps,
        slippage_bps,
        selected_test,
        diagnostics,
    })
}

fn h4_file(family: &FamilyOutput, initial: i64) -> Result<H4File<'_>, String> {
    use crate::h4_report::{aggregate_selected, SelectedFold};
    use crate::walkforward::Segment;
    let mut windows = Vec::new();
    for fold in &family.folds {
        for (segment, rows) in [("train", &fold.train), ("test", &fold.test)] {
            for row in rows {
                windows.push(h4_window(
                    row,
                    fold.fold_index,
                    segment,
                    5,
                    2,
                    segment == "test"
                        && !fold.selected_flat
                        && row.grid_index == fold.selected_index,
                )?);
            }
        }
        for replay in &fold.cost_replays {
            let segment = match replay.segment {
                Segment::Train => "train",
                Segment::Test => "test",
            };
            windows.push(h4_window(
                &replay.run,
                fold.fold_index,
                segment,
                replay.fee_bps,
                replay.slippage_bps,
                segment == "test"
                    && !fold.selected_flat
                    && replay.run.grid_index == fold.selected_index,
            )?);
        }
    }
    let expected = std::collections::BTreeSet::from([(2, 2), (4, 2), (5, 2), (6, 2), (5, 5)]);
    let actual: std::collections::BTreeSet<_> = family
        .fee_replays
        .iter()
        .map(|r| (r.fee_bps, r.slippage_bps))
        .collect();
    if actual != expected || family.fee_replays.len() != expected.len() {
        return Err("H4 diagnostic cost scenarios mismatch".to_owned());
    }
    let mut scenarios = Vec::new();
    for scenario in &family.fee_replays {
        let mut selected = Vec::new();
        let mut returns = Vec::new();
        for fold in &family.folds {
            let days = fold
                .test
                .first()
                .ok_or("H4 fold has no test row")?
                .daily_returns
                .len();
            if fold.selected_flat {
                returns.extend(std::iter::repeat_n(0, days));
                selected.push(SelectedFold {
                    fold: fold.fold_index,
                    selected_config_id: &fold.selected_config_id,
                    window: None,
                    daily_returns: None,
                    days,
                });
                continue;
            }
            let row = if (scenario.fee_bps, scenario.slippage_bps) == (5, 2) {
                fold.test
                    .get(fold.selected_index)
                    .ok_or("H4 selected index missing")?
            } else {
                let mut rows = fold.cost_replays.iter().filter(|r| {
                    r.segment == Segment::Test
                        && r.fee_bps == scenario.fee_bps
                        && r.slippage_bps == scenario.slippage_bps
                        && r.run.grid_index == fold.selected_index
                });
                let row = &rows.next().ok_or("H4 selected cost replay missing")?.run;
                if rows.next().is_some() {
                    return Err("H4 selected cost replay duplicated".to_owned());
                }
                row
            };
            if row.config_id != fold.selected_config_id || row.daily_returns.len() != days {
                return Err("H4 selection identity/calendar mismatch".to_owned());
            }
            returns.extend_from_slice(&row.daily_returns);
            selected.push(SelectedFold {
                fold: fold.fold_index,
                selected_config_id: &row.config_id,
                window: Some(row.h4.as_ref().ok_or("H4 selected diagnostics missing")?),
                daily_returns: Some(&row.daily_returns),
                days,
            });
        }
        if returns != scenario.daily_returns {
            return Err("H4 selected scenario return path mismatch".to_owned());
        }
        let out = aggregate_selected(Cash(initial), &selected).map_err(|err| err.to_string())?;
        if out.cash.net_ticks != i128::from(scenario.net_result_ticks)
            || out.cash.fees_ticks != i128::from(scenario.accounting.fees_paid_ticks)
            || out.cash.funding_ticks != i128::from(scenario.accounting.funding_net_ticks)
            || out.cash.slippage_ticks != i128::from(scenario.accounting.slippage_cost_ticks)
            || out
                .crowded_long_fades
                .closed_round_trips
                .checked_add(out.crowded_short_fades.closed_round_trips)
                != Some(scenario.round_trips)
        {
            return Err("H4 selected scenario accounting mismatch".to_owned());
        }
        scenarios.push(H4ScenarioFile {
            fee_bps: scenario.fee_bps,
            slippage_bps: scenario.slippage_bps,
            selected: out,
        });
    }
    Ok(H4File {
        schema: "parangsae-h4-diagnostics-v1",
        family: "H4",
        claim:
            "Report-only frozen falsifiers; no episode independence or strategy acceptance implied",
        windows,
        selected_cost_scenarios: scenarios,
    })
}

const FOLD_DAY_US: i64 = 86_400_000_000;

/// `fold_daily_returns.csv` under `dir`.
///
/// One row per config, fold, and UTC day, for every train and test segment in
/// [`FoldOutcome`]. Fee-grid replays are not rows. Downstream CSCV PBO,
/// `sr_var_cross_trial`, skew, and kurtosis read this file. Warmup days are
/// not rows: `daily_returns` starts at the scored open.
///
/// Callers write this only after every trial has finished. Any aborted trial
/// means this file is not written.
///
/// Columns, in order:
///
/// `family,config_id,trial_id,grid_index,fold,window,day,daily_return_ticks`
///
/// * `window` is `IS_fold_k` for train and `OOS_fold_k` for test. `k` is the
///   1-based fold index.
/// * `day` is the UTC date of that return, `YYYY-MM-DD`. Row 0 is the scored
///   start. Later rows step one UTC day.
/// * `daily_return_ticks` is `trunc((next - prev) * 1e8 / prev)`.
///
/// Rows are sorted by family, grid index, fold, train then test, then day.
/// The file ends with a newline. Fields contain no commas.
pub fn write_fold_daily_returns(dir: &Path, result: &WalkForward) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
    let mut rows = Vec::new();
    for family in &result.families {
        for fold in &family.folds {
            for run in &fold.train {
                push_fold_rows(&mut rows, &family.family, fold.fold_index, true, run)?;
            }
            for run in &fold.test {
                push_fold_rows(&mut rows, &family.family, fold.fold_index, false, run)?;
            }
        }
    }
    rows.sort_by(|left, right| {
        (
            &left.family,
            left.grid_index,
            left.fold,
            !left.train,
            &left.day,
        )
            .cmp(&(
                &right.family,
                right.grid_index,
                right.fold,
                !right.train,
                &right.day,
            ))
    });
    let mut text =
        String::from("family,config_id,trial_id,grid_index,fold,window,day,daily_return_ticks\n");
    for row in &rows {
        let window = if row.train {
            format!("IS_fold_{}", row.fold)
        } else {
            format!("OOS_fold_{}", row.fold)
        };
        for field in [
            row.family.as_str(),
            row.config_id.as_str(),
            row.trial_id.as_str(),
            window.as_str(),
            row.day.as_str(),
        ] {
            if field.contains([',', '\n', '\r']) {
                return Err(format!("fold return field {field} contains a comma"));
            }
        }
        text.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            row.family,
            row.config_id,
            row.trial_id,
            row.grid_index,
            row.fold,
            window,
            row.day,
            row.ret
        ));
    }
    let path = dir.join("fold_daily_returns.csv");
    fs::write(&path, text).map_err(|err| format!("write {}: {err}", path.display()))?;
    Ok(())
}

struct FoldRow {
    family: String,
    config_id: String,
    trial_id: String,
    grid_index: usize,
    fold: u32,
    train: bool,
    day: String,
    ret: i64,
}

fn push_fold_rows(
    rows: &mut Vec<FoldRow>,
    family: &str,
    fold: u32,
    train: bool,
    run: &SegmentRun,
) -> Result<(), String> {
    if run.start_us % FOLD_DAY_US != 0 {
        return Err(format!(
            "{} start {} is not a UTC midnight",
            run.run_id, run.start_us
        ));
    }
    for (index, ret) in run.daily_returns.iter().enumerate() {
        let offset = i64::try_from(index)
            .map_err(|_| "day index".to_owned())?
            .checked_mul(FOLD_DAY_US)
            .ok_or_else(|| format!("{} day offset overflowed", run.run_id))?;
        let ts = run
            .start_us
            .checked_add(offset)
            .ok_or_else(|| format!("{} day overflowed", run.run_id))?;
        rows.push(FoldRow {
            family: family.to_owned(),
            config_id: run.config_id.clone(),
            trial_id: run.trial_id.clone(),
            grid_index: run.grid_index,
            fold,
            train,
            day: UtcDate::from_micros(UtcMicros::from_micros(ts)).to_string(),
            ret: *ret,
        });
    }
    Ok(())
}

fn family_file(
    family: &FamilyOutput,
    warmup_days: i64,
    initial: i64,
) -> Result<FamilyFile, String> {
    let headline = family
        .fee_replays
        .iter()
        .find(|row| row.fee_bps == 5 && row.slippage_bps == 2)
        .ok_or_else(|| format!("{} has no 5 bps OOS replay", family.family))?;
    if headline.daily_returns != family.stitched_oos_daily_returns {
        return Err(format!(
            "{} headline daily returns are not the stitched OOS series",
            family.family
        ));
    }
    let mut replays = Vec::with_capacity(family.fee_replays.len());
    for row in &family.fee_replays {
        let metrics = oos_metrics(
            initial,
            &row.daily_returns,
            row.sharpe_annual_ticks,
            row.turnover_ticks,
        )?;
        let costs = cost_file(&row.accounting, row.round_trips)?;
        replays.push(ReplayFile {
            fee_bps: row.fee_bps,
            slippage_bps: row.slippage_bps,
            total_return_ticks: metrics.total_return_ticks,
            sharpe_annual_ticks: metrics.sharpe_annual_ticks,
            max_drawdown_ticks: row.max_dd_ticks,
            turnover_ticks: metrics.turnover_ticks,
            net_result_ticks: row.net_result_ticks,
            round_trips: row.round_trips,
            daily_returns: row.daily_returns.clone(),
            fees_paid_ticks: costs.fees_paid_ticks,
            funding_net_ticks: costs.funding_net_ticks,
            exposure_ticks: costs.exposure_ticks,
            avg_hold_us: costs.avg_hold_us,
            n_refused_fills: costs.n_refused_fills,
            n_dropped_entries: costs.n_dropped_entries,
        });
    }
    let metrics = oos_metrics(
        initial,
        &headline.daily_returns,
        headline.sharpe_annual_ticks,
        headline.turnover_ticks,
    )?;
    let costs = cost_file(&headline.accounting, headline.round_trips)?;
    let mut folds = Vec::with_capacity(family.folds.len());
    for fold in &family.folds {
        let is_sharpe = if fold.selected_flat {
            None
        } else {
            fold.train
                .get(fold.selected_index)
                .and_then(|row| row.sharpe_annual_ticks)
        };
        folds.push(FoldFile {
            fold: fold.fold_index,
            selected_config_id: fold.selected_config_id.clone(),
            is_sharpe_annual_ticks: is_sharpe,
            train_sharpe_annual_ticks: fold
                .train
                .iter()
                .map(|row| row.sharpe_annual_ticks)
                .collect(),
        });
    }
    let stay_flat_shadow = family.stay_flat_shadow.as_ref().map(|shadow| ShadowFile {
        daily_returns: shadow.daily_returns.clone(),
        fold_flat: shadow.fold_flat.clone(),
    });
    Ok(FamilyFile {
        h2_member_sharpes: family.h2_member_sharpes.clone(),
        family: family.family.clone(),
        n_trials: family.n_trials,
        warmup_days,
        total_return_ticks: metrics.total_return_ticks,
        sharpe_annual_ticks: metrics.sharpe_annual_ticks,
        max_drawdown_ticks: headline.max_dd_ticks,
        turnover_ticks: metrics.turnover_ticks,
        round_trips: headline.round_trips,
        daily_returns: headline.daily_returns.clone(),
        fees_paid_ticks: costs.fees_paid_ticks,
        funding_net_ticks: costs.funding_net_ticks,
        exposure_ticks: costs.exposure_ticks,
        avg_hold_us: costs.avg_hold_us,
        n_refused_fills: costs.n_refused_fills,
        n_dropped_entries: costs.n_dropped_entries,
        folds,
        stay_flat_shadow,
        fee_replays: replays,
    })
}

struct CostFile {
    fees_paid_ticks: i64,
    funding_net_ticks: i64,
    exposure_ticks: i64,
    avg_hold_us: i64,
    n_refused_fills: u64,
    n_dropped_entries: u64,
}

fn cost_file(accounting: &WindowAccounting, round_trips: u64) -> Result<CostFile, String> {
    Ok(CostFile {
        fees_paid_ticks: accounting.fees_paid_ticks,
        funding_net_ticks: accounting.funding_net_ticks,
        exposure_ticks: ratio_ticks(accounting.position_us, accounting.window_us)?,
        avg_hold_us: avg_hold_us(accounting.hold_us_sum, round_trips)?,
        n_refused_fills: accounting.n_refused_fills,
        n_dropped_entries: accounting.n_dropped_entries,
    })
}

fn ratio_ticks(numerator: i64, denominator: i64) -> Result<i64, String> {
    if denominator <= 0 {
        return Ok(0);
    }
    let ticks = i128::from(numerator)
        .checked_mul(SCALE)
        .ok_or("exposure overflow")?
        / i128::from(denominator);
    i64::try_from(ticks).map_err(|_| "exposure overflow".to_owned())
}

fn avg_hold_us(hold_us_sum: i64, round_trips: u64) -> Result<i64, String> {
    if round_trips == 0 {
        return Ok(0);
    }
    let trips = i64::try_from(round_trips).map_err(|_| "avg hold overflow".to_owned())?;
    if trips == 0 {
        return Ok(0);
    }
    Ok(hold_us_sum / trips)
}

struct OosMetrics {
    total_return_ticks: i64,
    sharpe_annual_ticks: Option<i64>,
    turnover_ticks: i64,
}

fn oos_metrics(
    initial: i64,
    returns: &[i64],
    sharpe_annual_ticks: Option<i64>,
    turnover_ticks: i64,
) -> Result<OosMetrics, String> {
    let path = compound(initial, returns)?;
    let end = path.last().copied().ok_or("oos path is empty")?;
    let total = total_return(Cash(initial), Cash(end)).map_err(|err| err.to_string())?;
    Ok(OosMetrics {
        total_return_ticks: total.ticks(),
        sharpe_annual_ticks,

        turnover_ticks,
    })
}

fn compound(initial: i64, returns: &[i64]) -> Result<Vec<i64>, String> {
    let mut level = initial;
    let mut path = Vec::with_capacity(returns.len().saturating_add(1));
    path.push(level);
    for ret in returns {
        let delta = i128::from(level)
            .checked_mul(i128::from(*ret))
            .ok_or("oos equity overflow")?
            / SCALE;
        level = i64::try_from(
            i128::from(level)
                .checked_add(delta)
                .ok_or("oos equity overflow")?,
        )
        .map_err(|_| "oos equity overflow".to_owned())?;
        path.push(level);
    }
    Ok(path)
}

#[derive(Serialize)]
struct FamilyFile {
    family: String,
    n_trials: u64,
    warmup_days: i64,
    total_return_ticks: i64,
    sharpe_annual_ticks: Option<i64>,
    max_drawdown_ticks: i64,
    turnover_ticks: i64,
    round_trips: u64,
    daily_returns: Vec<i64>,
    fees_paid_ticks: i64,
    funding_net_ticks: i64,
    exposure_ticks: i64,
    avg_hold_us: i64,
    n_refused_fills: u64,
    n_dropped_entries: u64,
    folds: Vec<FoldFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stay_flat_shadow: Option<ShadowFile>,
    fee_replays: Vec<ReplayFile>,
    h2_member_sharpes: Vec<Option<i64>>,
}

#[derive(Serialize)]
struct FoldFile {
    fold: u32,
    selected_config_id: String,
    is_sharpe_annual_ticks: Option<i64>,
    train_sharpe_annual_ticks: Vec<Option<i64>>,
}

#[derive(Serialize)]
struct ShadowFile {
    daily_returns: Vec<i64>,
    fold_flat: Vec<bool>,
}

#[derive(Serialize)]
struct ReplayFile {
    fee_bps: u32,
    slippage_bps: u32,
    total_return_ticks: i64,
    sharpe_annual_ticks: Option<i64>,
    max_drawdown_ticks: i64,
    turnover_ticks: i64,
    net_result_ticks: i64,
    round_trips: u64,
    daily_returns: Vec<i64>,
    fees_paid_ticks: i64,
    funding_net_ticks: i64,
    exposure_ticks: i64,
    avg_hold_us: i64,
    n_refused_fills: u64,
    n_dropped_entries: u64,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{prepare_families, write_fold_daily_returns, write_oos};
    use crate::walkforward::{
        FamilyOutput, FeeReplay, FoldOutcome, SegmentRun, WalkForward, SESSION_WARMUP_DAYS,
        WARMUP_DAYS,
    };

    #[test]
    fn sweep_inputs_are_forty_three_configs_and_s06_uses_the_year_schedule() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../research/sweep-inputs-v2.json");
        let families = prepare_families(&path, 1).unwrap();
        assert!(families
            .iter()
            .all(|family| family.points.iter().all(|point| point.attempt == 1)));
        let counts: BTreeMap<_, _> = families
            .iter()
            .map(|family| (family.family.as_str(), family.points.len()))
            .collect();
        assert_eq!(
            counts,
            BTreeMap::from([("S01", 12), ("S02", 9), ("S03", 8), ("S05", 8), ("S06", 6),])
        );
        assert_eq!(counts.values().sum::<usize>(), 43);
        for family in &families {
            let days = if family.family == "S06" {
                SESSION_WARMUP_DAYS
            } else {
                WARMUP_DAYS
            };
            assert!(
                family.folds[0].train_warmup.start.micros() < family.folds[0].train.start.micros()
            );
            let expected = if days == SESSION_WARMUP_DAYS { 11 } else { 15 };
            assert_eq!(family.folds.len(), expected, "{}", family.family);
        }
    }

    #[test]
    fn oos_files_carry_the_stitched_series_and_the_fee_grid() {
        let initial = 1_000 * 100_000_000;
        let returns = vec![10_000_000i64];
        let replay = |fee, slip| FeeReplay {
            fee_bps: fee,
            slippage_bps: slip,
            daily_returns: returns.clone(),
            equity_ticks: vec![initial, initial + initial / 10],
            net_result_ticks: initial / 10,
            round_trips: 3,
            sharpe_annual_ticks: None,
            turnover_ticks: 25_000_000,
            max_dd_ticks: 0,
            accounting: crate::walkforward::WindowAccounting::ZERO,
        };
        let family = FamilyOutput {
            h2_member_sharpes: Vec::new(),
            family: "S01".to_owned(),
            folds: vec![FoldOutcome {
                cost_replays: Vec::new(),
                fold_index: 1,
                train: Vec::new(),
                test: vec![segment()],
                selected_index: 0,
                selected_config_id: "abc".to_owned(),
                selected_flat: false,
            }],
            stitched_oos_daily_returns: returns.clone(),
            stitched_oos_equity: vec![initial, initial + initial / 10],
            oos_round_trips: 3,
            n_trials: 12,
            stay_flat_shadow: None,
            fee_replays: vec![
                replay(2, 2),
                replay(4, 2),
                replay(5, 2),
                replay(6, 2),
                replay(5, 5),
            ],
        };
        let result = WalkForward {
            families: vec![family],
            n_trials: 12,
        };
        let dir = tempfile::tempdir().unwrap();
        let mut warmup = BTreeMap::new();
        warmup.insert("S01".to_owned(), WARMUP_DAYS);
        write_oos(dir.path(), &result, &warmup, initial).unwrap();
        let text = std::fs::read_to_string(dir.path().join("S01.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["n_trials"], 12);
        assert_eq!(value["warmup_days"], WARMUP_DAYS);
        assert_eq!(value["daily_returns"], serde_json::json!([10_000_000]));
        assert_eq!(value["total_return_ticks"], 10_000_000);
        assert_eq!(value["max_drawdown_ticks"], 0);
        assert_eq!(value["turnover_ticks"], 25_000_000);
        assert!(value["sharpe_annual_ticks"].is_null());
        assert!(value.get("train").is_none());
        let rows = value["fee_replays"].as_array().unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows.iter()
                .map(|row| {
                    (
                        row["fee_bps"].as_u64().unwrap(),
                        row["slippage_bps"].as_u64().unwrap(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![(2, 2), (4, 2), (5, 2), (6, 2), (5, 5)]
        );
        assert_eq!(rows[2]["daily_returns"], value["daily_returns"]);
        assert_eq!(value["fees_paid_ticks"], 0);
        assert_eq!(value["funding_net_ticks"], 0);
        assert_eq!(value["exposure_ticks"], 0);
        assert_eq!(value["avg_hold_us"], 0);
        assert_eq!(value["n_refused_fills"], 0);
        assert_eq!(value["n_dropped_entries"], 0);
        assert_eq!(value["folds"][0]["selected_config_id"], "abc");
        assert!(value["folds"][0]["is_sharpe_annual_ticks"].is_null());
        assert!(value.get("stay_flat_shadow").is_none());
        assert_eq!(rows[2]["fees_paid_ticks"], 0);
    }

    #[test]
    fn fold_daily_returns_are_one_row_per_config_fold_and_day() {
        let initial = 1_000 * 100_000_000;
        let train_start = crate::UtcDate::parse("2021-01-17")
            .unwrap()
            .start_us()
            .micros();
        let test_start = crate::UtcDate::parse("2022-01-19")
            .unwrap()
            .start_us()
            .micros();
        let mut train = segment();
        train.start_us = train_start;
        train.daily_returns = vec![1, 2];
        train.grid_index = 1;
        train.config_id = "cfg-b".to_owned();
        train.trial_id = "trial-b".to_owned();
        let mut earlier = segment();
        earlier.start_us = train_start;
        earlier.daily_returns = vec![7];
        earlier.grid_index = 0;
        earlier.config_id = "cfg-a".to_owned();
        earlier.trial_id = "trial-a".to_owned();
        let mut test = segment();
        test.start_us = test_start;
        test.daily_returns = vec![3];
        test.grid_index = 1;
        test.config_id = "cfg-b".to_owned();
        test.trial_id = "trial-b".to_owned();
        let family = FamilyOutput {
            h2_member_sharpes: Vec::new(),
            family: "S01".to_owned(),
            folds: vec![FoldOutcome {
                cost_replays: Vec::new(),
                fold_index: 1,
                train: vec![train, earlier],
                test: vec![test],
                selected_index: 1,
                selected_config_id: "cfg-b".to_owned(),
                selected_flat: false,
            }],
            stitched_oos_daily_returns: vec![3],
            stitched_oos_equity: vec![initial, initial],
            oos_round_trips: 0,
            n_trials: 2,
            stay_flat_shadow: None,
            fee_replays: Vec::new(),
        };
        let dir = tempfile::tempdir().unwrap();
        write_fold_daily_returns(
            dir.path(),
            &WalkForward {
                families: vec![family],
                n_trials: 2,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(dir.path().join("fold_daily_returns.csv")).unwrap();
        assert_eq!(
            text,
            "\
family,config_id,trial_id,grid_index,fold,window,day,daily_return_ticks
S01,cfg-a,trial-a,0,1,IS_fold_1,2021-01-17,7
S01,cfg-b,trial-b,1,1,IS_fold_1,2021-01-17,1
S01,cfg-b,trial-b,1,1,IS_fold_1,2021-01-18,2
S01,cfg-b,trial-b,1,1,OOS_fold_1,2022-01-19,3
"
        );
    }

    fn segment() -> SegmentRun {
        SegmentRun {
            grid_index: 0,
            run_id: "S01-f01-test-c0".to_owned(),
            trial_id: "t".to_owned(),
            config_id: "abc".to_owned(),
            start_us: 1_610_841_600_000_000,
            daily_returns: vec![10_000_000],
            equity_ticks: vec![1, 2],
            net_result_ticks: 1,
            round_trips: 3,
            sharpe_annual_ticks: None,
            fill_notional_ticks: 0,
            max_dd_ticks: 0,
            accounting: crate::walkforward::WindowAccounting::ZERO,
            h4: None,
        }
    }
    #[test]
    fn qr1_all_twelve_members_are_replayed_on_identical_test_folds() {
        let folds = crate::btcusdt_schedule(crate::WARMUP_DAYS).unwrap();
        let mut calls = 0;
        let mut members = std::collections::BTreeSet::new();
        let values = super::h2_member_diagnostics(&folds, 1_000_000_000, |config, fold| {
            calls += 1;
            assert_eq!(config.family, "S01");
            assert_eq!(config.params["deadband"], 1);
            members.insert((
                config.params["interval"],
                config.params["n"],
                config.params["c"],
            ));
            assert!(fold.test.start >= fold.train.end);
            Ok(crate::WindowResult {
                daily_returns: vec![1_000_000, -500_000],
                equity_ticks: vec![1_000_000_000, 1_010_000_000, 1_004_950_000],
                net_result_ticks: 4_950_000,
                round_trips: 1,
                fill_notional_ticks: 0,
                max_dd_ticks: 0,
                accounting: crate::WindowAccounting::ZERO,
                h4: None,
            })
        })
        .unwrap();
        assert_eq!(calls, 12 * folds.len());
        assert_eq!(members.len(), 12);
        assert_eq!(values.len(), 12);
        assert!(values.iter().all(Option::is_some));
    }
}
