//! Walk-forward over the sweep-input families.
//!
//! Each config is one trial. Train and test are both logged, and `config_id`
//! is the hash of the config on the family's outer window. Fold and segment
//! are fields on `RunStarted`. The out directory receives one OOS file per
//! family: stitched test results only. There is no full-window per-config file.
//!
//! S01, S02, S03, and S05 use the 30-day warmup schedule. S06 uses 365 days.
//! Selection and the headline row are 5 bps fee and 2 bps slippage, with
//! funding settled by the engine. The fee grid replays the selected test path
//! at 2/4/5/6 bps and at 5 bps fee with 5 bps slippage. Replays are not trials.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use parangsae_core::{Interval, UtcMicros, SCALE};
use parangsae_sim::Cash;
use serde::Serialize;

use crate::batch::{canonical_config_id, SweepPins, TrialSpec, HOLDOUT_START_US};
use crate::engine::RunConfig;
use crate::metrics::{max_drawdown, total_return};
use crate::sweep::{load_configs, load_pinned_tape, ConfigRow};
use crate::walkforward::{
    btcusdt_schedule, run_walk_forward, selection_costs, FamilyGrid, FamilyOutput, GridPoint,
    WalkContext, WalkForward, SESSION_WARMUP_DAYS, WARMUP_DAYS,
};
use crate::WindowConfig;

/// Parquet root used when `--data-root` is omitted.
pub const DEFAULT_DATA_ROOT: &str = "/workspace/data/parangsae";

/// What `wf_batch` was asked to run.
pub struct WfBatchRequest {
    pub inputs: PathBuf,
    pub data_root: PathBuf,
    pub out_dir: PathBuf,
    pub ledger: PathBuf,
}

/// Load the sweep file, check every `config_id`, and build one grid per family.
pub fn prepare_families(inputs: &Path) -> Result<Vec<FamilyGrid<WindowConfig>>, String> {
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
            let id = window_config_id(&row.family, &row.params, row.initial_cash, start, end)?;
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
                attempt: 0,
            });
        }
        families.push(FamilyGrid {
            family: name,
            folds,
            points,
        });
    }
    Ok(families)
}

/// Run every family on `request.data_root` and write OOS files under `out_dir`.
///
/// The ledger must be missing or empty. A second batch needs a new ledger,
/// because run ids are the family, fold, segment, and grid index.
pub fn run_wf_batch(request: &WfBatchRequest) -> Result<(), String> {
    let families = prepare_families(&request.inputs)?;
    if request.ledger.exists() {
        let text = fs::read_to_string(&request.ledger)
            .map_err(|err| format!("read {}: {err}", request.ledger.display()))?;
        if text.lines().any(|line| !line.is_empty()) {
            return Err(format!(
                "{} already has trials; choose a new ledger",
                request.ledger.display()
            ));
        }
    }
    let pins = SweepPins::load(&request.inputs).map_err(|err| err.to_string())?;
    let start_us = families[0].folds[0].train_warmup.start.micros();
    let end_us = families[0].folds.last().expect("folds").test.end.micros();
    let tape = load_pinned_tape(&request.data_root, &pins, start_us, end_us)?;
    let cash = families[0].points[0].config.initial_cash;
    fs::create_dir_all(&request.out_dir)
        .map_err(|err| format!("create {}: {err}", request.out_dir.display()))?;
    if let Some(parent) = request.ledger.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create {}: {err}", parent.display()))?;
        }
    }
    let fold = crate::sweep::FoldRunner::new(&tape);
    let ctx = WalkContext {
        pins: &pins,
        trials_path: &request.ledger,
        initial_cash: Cash(cash),
        holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
    };
    let result = run_walk_forward(
        &families,
        &ctx,
        &selection_costs(),
        |config, warmup, start, end, costs| fold.run_scored(config, warmup, start, end, costs),
    )
    .map_err(|err| err.to_string())?;
    write_oos(
        &request.out_dir,
        &result,
        &warmup_by_family(&families),
        cash,
    )?;
    Ok(())
}

fn warmup_by_family(families: &[FamilyGrid<WindowConfig>]) -> BTreeMap<String, i64> {
    families
        .iter()
        .map(|family| {
            let days = if family.family == "S06" {
                SESSION_WARMUP_DAYS
            } else {
                WARMUP_DAYS
            };
            (family.family.clone(), days)
        })
        .collect()
}

fn warmup_days(family: &str) -> Result<i64, String> {
    match family {
        "S01" | "S02" | "S03" | "S05" => Ok(WARMUP_DAYS),
        "S06" => Ok(SESSION_WARMUP_DAYS),
        other => Err(format!("no walk-forward schedule for {other}")),
    }
}

fn window_config_id(
    family: &str,
    params: &BTreeMap<String, i64>,
    cash: i64,
    start: UtcMicros,
    end: UtcMicros,
) -> Result<String, String> {
    let spec = TrialSpec {
        run_id: String::new(),
        config_id: String::new(),
        family: family.to_owned(),
        params: params.clone(),
        attempt: 0,
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
        let path = dir.join(format!("{}.json", family.family));
        let text = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        fs::write(&path, format!("{text}\n"))
            .map_err(|err| format!("write {}: {err}", path.display()))?;
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
        replays.push(ReplayFile {
            fee_bps: row.fee_bps,
            slippage_bps: row.slippage_bps,
            total_return_ticks: metrics.total_return_ticks,
            sharpe_annual_ticks: metrics.sharpe_annual_ticks,
            max_drawdown_ticks: metrics.max_drawdown_ticks,
            turnover_ticks: metrics.turnover_ticks,
            net_result_ticks: row.net_result_ticks,
            round_trips: row.round_trips,
            daily_returns: row.daily_returns.clone(),
        });
    }
    let metrics = oos_metrics(
        initial,
        &headline.daily_returns,
        headline.sharpe_annual_ticks,
        headline.turnover_ticks,
    )?;
    Ok(FamilyFile {
        family: family.family.clone(),
        n_trials: family.n_trials,
        warmup_days,
        total_return_ticks: metrics.total_return_ticks,
        sharpe_annual_ticks: metrics.sharpe_annual_ticks,
        max_drawdown_ticks: metrics.max_drawdown_ticks,
        turnover_ticks: metrics.turnover_ticks,
        round_trips: headline.round_trips,
        daily_returns: headline.daily_returns.clone(),
        fee_replays: replays,
    })
}

struct OosMetrics {
    total_return_ticks: i64,
    sharpe_annual_ticks: Option<i64>,
    max_drawdown_ticks: i64,
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
    let drawdown = max_drawdown(&path.iter().copied().map(Cash).collect::<Vec<_>>())
        .map_err(|err| err.to_string())?;
    Ok(OosMetrics {
        total_return_ticks: total.ticks(),
        sharpe_annual_ticks,
        max_drawdown_ticks: drawdown.ticks(),
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
    fee_replays: Vec<ReplayFile>,
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
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{prepare_families, write_oos};
    use crate::walkforward::{
        FamilyOutput, FeeReplay, FoldOutcome, SegmentRun, WalkForward, SESSION_WARMUP_DAYS,
        WARMUP_DAYS,
    };

    #[test]
    fn sweep_inputs_are_forty_three_configs_and_s06_uses_the_year_schedule() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../research/sweep-inputs-v2.json");
        let families = prepare_families(&path).unwrap();
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
        };
        let family = FamilyOutput {
            family: "S01".to_owned(),
            folds: vec![FoldOutcome {
                fold_index: 1,
                train: Vec::new(),
                test: vec![segment()],
                selected_index: 0,
                selected_config_id: "abc".to_owned(),
            }],
            stitched_oos_daily_returns: returns.clone(),
            stitched_oos_equity: vec![initial, initial + initial / 10],
            oos_round_trips: 3,
            n_trials: 12,
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
    }

    fn segment() -> SegmentRun {
        SegmentRun {
            grid_index: 0,
            run_id: "S01-f01-test-c0".to_owned(),
            trial_id: "t".to_owned(),
            config_id: "abc".to_owned(),
            daily_returns: vec![10_000_000],
            equity_ticks: vec![1, 2],
            net_result_ticks: 1,
            round_trips: 3,
            sharpe_annual_ticks: None,
            fill_notional_ticks: 0,
        }
    }
}
