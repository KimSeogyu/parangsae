//! `table_<slug>.csv` and `cscv_is_<slug>.csv`.
//!
//! Columns are runner-inputs v2 §5.1 plus the per-fold fields named in the
//! second-batch preregs. Ratios are decimal strings of `10^8` ticks. The CSCV
//! file is one integer daily-return tick per config per in-sample day.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use parangsae_core::{Ratio, UtcMicros, SCALE};
use parangsae_sim::Cash;

use crate::batch::SweepPins;
use crate::canon::json_string;
use crate::date::UtcDate;
use crate::metrics::{sharpe_of_return_ticks, total_return, turnover_ticks};
use crate::stats::{
    breakeven_fee, cscv_gap_days, deflated_sharpe, dsr_hurdle_sr_ann, fee_slope_ticks, is_matrix,
    pbo_ticks, skew_kurt, sr_var_ann_ticks, Breakeven,
};
use crate::trial_log::distinct_trial_count;
use crate::walkforward::{
    FamilyGrid, FamilyOutput, FeeReplay, FoldOutcome, SegmentRun, WalkForward,
};

const DAY_US: i64 = 86_400_000_000;
const HOUR_US: i128 = 3_600_000_000;

pub(crate) const TABLE_HEADER: &str = "\
trial_id,config_id,slug,params,pinned_root,read_path_sha,window,start,end,\
fee_bps,slip_bps,funding_on,days_T,total_return,sharpe_annual,sharpe_denominator,\
sharpe_daily,skew,kurt,max_dd,turnover_ann,n_trades,avg_hold_h,exposure_pct,\
n_refused_fills,fees_paid,slippage_cost,funding_net,fee_sens,breakeven_fee_bps,\
n_global,sr_var_cross_trial,dsr_hurdle_sr_ann,dsr,pbo,gate_pass,selected_grid_index,\
selected_config_id,S_j,n_rebalances,n_sign_changes,n_forced_exits,h2_member_sharpes,h2_member_median,h2_gate2prime";

pub(crate) fn write_family_tables<C>(
    dir: &Path,
    families: &[FamilyGrid<C>],
    result: &WalkForward,
    pins: &SweepPins,
    ledger: &Path,
) -> Result<(), String> {
    let read_path_sha = pins
        .sha_by_suffix("research-read-path.sql")
        .unwrap_or("")
        .to_owned();
    write_tables(
        dir,
        families,
        result,
        pins.pinned_root(),
        &read_path_sha,
        ledger,
    )
}

pub(crate) fn write_tables<C>(
    dir: &Path,
    families: &[FamilyGrid<C>],
    result: &WalkForward,
    pinned_root: &str,
    read_path_sha: &str,
    ledger: &Path,
) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
    let n_global = n_global(ledger)?;
    for output in &result.families {
        let grid = families
            .iter()
            .find(|family| family.family == output.family)
            .ok_or_else(|| format!("{} has no grid", output.family))?;
        let slug = if grid.slug.is_empty() {
            grid.family.clone()
        } else {
            grid.slug.clone()
        };
        if slug.is_empty() || slug.contains(['/', '\\']) || slug.contains("..") {
            return Err(format!("slug {slug} is not a file name"));
        }
        let text = family_table(grid, output, pinned_root, read_path_sha, n_global)?;
        let table_path = dir.join(format!("table_{slug}.csv"));
        fs::write(&table_path, text)
            .map_err(|err| format!("write {}: {err}", table_path.display()))?;
        let cscv = cscv_csv(output)?;
        let cscv_path = dir.join(format!("cscv_is_{slug}.csv"));
        fs::write(&cscv_path, cscv)
            .map_err(|err| format!("write {}: {err}", cscv_path.display()))?;
    }
    Ok(())
}

fn n_global(ledger: &Path) -> Result<u64, String> {
    let counted = if ledger.exists() {
        distinct_trial_count(ledger).map_err(|err| err.to_string())?
    } else {
        0
    };
    Ok(counted.max(202))
}

fn family_table<C>(
    grid: &FamilyGrid<C>,
    output: &FamilyOutput,
    pinned_root: &str,
    read_path_sha: &str,
    n_global: u64,
) -> Result<String, String> {
    let (_dates, columns) = is_columns(output);
    let sr_var = match &columns {
        Some(columns) => sr_var_ann_ticks(columns).map_err(|_| "sr variance overflowed")?,
        None => 25_000_000,
    };
    let pbo = match &columns {
        Some(columns) => {
            pbo_ticks(columns, cscv_gap_days(&output.family)).map_err(|_| "pbo overflowed")?
        }
        None => None,
    };
    let (fee_sens, breakeven) = fee_cells(&output.fee_replays)?;
    let gate = gate_cell(output, n_global, sr_var, pbo)?;
    let mut lines = vec![TABLE_HEADER.to_owned()];
    for fold in &output.folds {
        let s_j = s_j_cell(fold);
        let (selected_index, selected_id) = selection_cells(fold);
        for (train, run, fee, slip) in fold
            .train
            .iter()
            .map(|run| (true, run, 5, 2))
            .chain(fold.test.iter().map(|run| (false, run, 5, 2)))
            .chain(fold.cost_replays.iter().map(|r| {
                (
                    matches!(r.segment, crate::walkforward::Segment::Train),
                    &r.run,
                    r.fee_bps,
                    r.slippage_bps,
                )
            }))
        {
            let window = if train {
                format!("IS_fold_{}", fold.fold_index)
            } else {
                format!("OOS_fold_{}", fold.fold_index)
            };
            let params = grid
                .points
                .get(run.grid_index)
                .map(|point| params_json(&point.params))
                .ok_or_else(|| {
                    format!("{} grid index {} is missing", run.run_id, run.grid_index)
                })?;
            lines.push(segment_row(
                run,
                &Printed {
                    slug: &grid_slug(grid),
                    pinned_root,
                    read_path_sha,
                },
                &params,
                &window,
                (fee, slip),
                &Selection {
                    s_j: &s_j,
                    selected_index: &selected_index,
                    selected_id: &selected_id,
                },
            )?);
        }
    }
    for replay in &output.fee_replays {
        let headline = replay.fee_bps == 5 && replay.slippage_bps == 2;
        lines.push(stitched_row(
            output,
            replay,
            &Printed {
                slug: &grid_slug(grid),
                pinned_root,
                read_path_sha,
            },
            &StitchedStats {
                n_global,
                sr_var,
                pbo,
                fee_sens: &fee_sens,
                breakeven: &breakeven,
                gate: if headline { &gate } else { "" },
            },
        )?);
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn grid_slug<C>(grid: &FamilyGrid<C>) -> String {
    if grid.slug.is_empty() {
        grid.family.clone()
    } else {
        grid.slug.clone()
    }
}

fn is_columns(output: &FamilyOutput) -> (Option<Vec<i64>>, Option<Vec<Vec<i64>>>) {
    let folds: Vec<Vec<(i64, Vec<i64>)>> = output
        .folds
        .iter()
        .map(|fold| {
            fold.train
                .iter()
                .map(|run| (run.start_us, run.daily_returns.clone()))
                .collect()
        })
        .collect();
    if folds.is_empty() || folds.iter().any(|fold| fold.is_empty()) {
        return (None, None);
    }
    match is_matrix(&folds) {
        Ok((dates, columns)) => (Some(dates), Some(columns)),
        Err(_) => (None, None),
    }
}

struct Printed<'a> {
    slug: &'a str,
    pinned_root: &'a str,
    read_path_sha: &'a str,
}

struct Selection<'a> {
    s_j: &'a str,
    selected_index: &'a str,
    selected_id: &'a str,
}

struct StitchedStats<'a> {
    n_global: u64,
    sr_var: i64,
    pbo: Option<i64>,
    fee_sens: &'a str,
    breakeven: &'a str,
    gate: &'a str,
}

fn segment_row(
    run: &SegmentRun,
    printed: &Printed<'_>,
    params: &str,
    window: &str,
    costs: (u32, u32),
    selection: &Selection<'_>,
) -> Result<String, String> {
    let days = run.daily_returns.len();
    let end = inclusive_end(run.start_us, days)?;
    let (daily, annual) = sharpes(&run.daily_returns)?;
    let (skew, kurt) = moment_cells(&run.daily_returns)?;
    let total = ratio_cell(total_of(
        run.equity_ticks.first().copied(),
        run.equity_ticks.last().copied(),
    )?);
    let turnover = turnover_ticks(run.fill_notional_ticks, &run.equity_ticks[1..])
        .map(|ratio| format_scale(i128::from(ratio.ticks())))
        .map_err(|err| err.to_string())?;
    let exposure = exposure_cell(run.accounting.position_us, run.accounting.window_us)?;
    Ok(join_row(&[
        &run.trial_id,
        &run.config_id,
        printed.slug,
        params,
        printed.pinned_root,
        printed.read_path_sha,
        window,
        &date_string(run.start_us),
        &date_string(end),
        &costs.0.to_string(),
        &costs.1.to_string(),
        "true",
        &days.to_string(),
        &total,
        &annual,
        &denominator(days),
        &daily,
        &skew,
        &kurt,
        &format_scale(i128::from(run.max_dd_ticks)),
        &turnover,
        &run.round_trips.to_string(),
        &hold_hours(run.accounting.hold_us_sum, run.round_trips)?,
        &exposure,
        &run.accounting.n_refused_fills.to_string(),
        &format_scale(i128::from(run.accounting.fees_paid_ticks)),
        &format_scale(i128::from(run.accounting.slippage_cost_ticks)),
        &format_scale(i128::from(run.accounting.funding_net_ticks)),
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        selection.selected_index,
        selection.selected_id,
        selection.s_j,
        &run.accounting.n_rebalances.to_string(),
        &run.accounting.n_sign_changes.to_string(),
        &run.accounting.n_forced_exits.to_string(),
        "",
        "",
        "",
    ]))
}

fn stitched_row(
    output: &FamilyOutput,
    replay: &FeeReplay,
    printed: &Printed<'_>,
    stats: &StitchedStats<'_>,
) -> Result<String, String> {
    let days = replay.daily_returns.len();
    let (start, end) = stitched_bounds(output)?;
    let (daily, annual) = sharpes(&replay.daily_returns)?;
    let moments = skew_kurt(&replay.daily_returns).map_err(|_| "moments overflowed")?;
    let (skew, kurt) = match moments {
        Some((skew, kurt)) => (
            format_scale(i128::from(skew)),
            format_scale(i128::from(kurt)),
        ),
        None => (String::new(), String::new()),
    };
    let initial = opening_cash(output)?;
    let total = compounded_return(initial, &replay.daily_returns)?;
    let max_dd = replay.max_dd_ticks;
    let headline = replay.fee_bps == 5 && replay.slippage_bps == 2;
    let (hurdle, dsr) = if let Some((skew_ticks, kurt_ticks)) = moments {
        let hurdle = dsr_hurdle_sr_ann(
            stats.n_global,
            stats.sr_var,
            u64::try_from(days).map_err(|_| "days")?,
            skew_ticks,
            kurt_ticks,
        )
        .map_err(|_| "dsr hurdle overflowed")?;
        let dsr = if headline {
            let sr = annual_ticks(&replay.daily_returns)?.unwrap_or(0);
            deflated_sharpe(
                sr,
                stats.n_global,
                stats.sr_var,
                u64::try_from(days).map_err(|_| "days")?,
                skew_ticks,
                kurt_ticks,
            )
            .map_err(|_| "dsr overflowed")?
        } else {
            None
        };
        (
            opt_scale(hurdle),
            if headline {
                opt_scale(dsr)
            } else {
                String::new()
            },
        )
    } else {
        (String::new(), String::new())
    };
    let pbo_cell = if headline {
        opt_scale(stats.pbo)
    } else {
        String::new()
    };
    Ok(join_row(&[
        "",
        "",
        printed.slug,
        "",
        printed.pinned_root,
        printed.read_path_sha,
        "OOS_stitched",
        &start,
        &end,
        &replay.fee_bps.to_string(),
        &replay.slippage_bps.to_string(),
        "true",
        &days.to_string(),
        &total,
        &annual,
        &denominator(days),
        &daily,
        &skew,
        &kurt,
        &format_scale(i128::from(max_dd)),
        &format_scale(i128::from(replay.turnover_ticks)),
        &replay.round_trips.to_string(),
        &hold_hours(replay.accounting.hold_us_sum, replay.round_trips)?,
        &exposure_cell(replay.accounting.position_us, replay.accounting.window_us)?,
        &replay.accounting.n_refused_fills.to_string(),
        &format_scale(i128::from(replay.accounting.fees_paid_ticks)),
        &format_scale(i128::from(replay.accounting.slippage_cost_ticks)),
        &format_scale(i128::from(replay.accounting.funding_net_ticks)),
        stats.fee_sens,
        stats.breakeven,
        &stats.n_global.to_string(),
        &format_scale(i128::from(stats.sr_var)),
        &hurdle,
        &dsr,
        &pbo_cell,
        stats.gate,
        "",
        "",
        "",
        &replay.accounting.n_rebalances.to_string(),
        &replay.accounting.n_sign_changes.to_string(),
        &replay.accounting.n_forced_exits.to_string(),
        &output
            .h2_member_sharpes
            .iter()
            .map(|v| opt_scale(*v))
            .collect::<Vec<_>>()
            .join(";"),
        &opt_scale(h2_member_median(output)),
        &if output.family == "H2" {
            h2_gate2prime(output)?.to_string()
        } else {
            String::new()
        },
    ]))
}

fn cscv_csv(output: &FamilyOutput) -> Result<String, String> {
    let (dates, columns) = is_columns(output);
    let ids: Vec<String> = output
        .folds
        .first()
        .map(|fold| fold.train.iter().map(|run| run.config_id.clone()).collect())
        .unwrap_or_default();
    let mut header = String::from("day");
    for id in &ids {
        header.push(',');
        header.push_str(&csv_field(id));
    }
    let mut lines = vec![header];
    if let (Some(dates), Some(columns)) = (dates, columns) {
        for (index, date) in dates.iter().enumerate() {
            let mut line = date_string(*date);
            for column in &columns {
                line.push(',');
                line.push_str(
                    &column
                        .get(index)
                        .copied()
                        .ok_or_else(|| format!("{} CSCV row {index} is short", output.family))?
                        .to_string(),
                );
            }
            lines.push(line);
        }
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn fee_cells(replays: &[FeeReplay]) -> Result<(String, String), String> {
    let mut sharpes = [None; 4];
    for replay in replays {
        if replay.slippage_bps != 2 {
            continue;
        }
        let slot = match replay.fee_bps {
            2 => 0,
            4 => 1,
            5 => 2,
            6 => 3,
            _ => continue,
        };
        sharpes[slot] = Some(annual_ticks(&replay.daily_returns)?.unwrap_or(0));
    }
    let Some(plain) = sharpes
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .and_then(|values| <[i64; 4]>::try_from(values).ok())
    else {
        return Ok((String::new(), String::new()));
    };
    let slope = fee_slope_ticks(plain).map_err(|_| "fee slope overflowed".to_owned())?;
    let breakeven = breakeven_cell(breakeven_fee(sharpes));
    Ok((format_scale(i128::from(slope)), breakeven))
}

fn gate_cell(
    output: &FamilyOutput,
    n_global: u64,
    sr_var: i64,
    pbo: Option<i64>,
) -> Result<String, String> {
    let headline = output
        .fee_replays
        .iter()
        .find(|row| row.fee_bps == 5 && row.slippage_bps == 2);
    let six = output
        .fee_replays
        .iter()
        .find(|row| row.fee_bps == 6 && row.slippage_bps == 2);
    let (Some(headline), Some(six)) = (headline, six) else {
        return Ok("false".to_owned());
    };
    let days = u64::try_from(headline.daily_returns.len()).map_err(|_| "days")?;
    let moments = skew_kurt(&headline.daily_returns).map_err(|_| "moments overflowed")?;
    let Some((skew, kurt)) = moments else {
        return Ok("false".to_owned());
    };
    let sharpe = annual_ticks(&headline.daily_returns)?.unwrap_or(0);
    let hurdle = dsr_hurdle_sr_ann(n_global, sr_var, days, skew, kurt)
        .map_err(|_| "dsr hurdle overflowed")?;
    let gate1 = hurdle.is_some_and(|level| sharpe >= level);
    let gate2 = if output.family == "H2" {
        h2_gate2prime(output)?
    } else {
        pbo.is_some_and(|value| value <= 20_000_000)
    };
    let initial = opening_cash(output)?;
    let total = compounded_return_ticks(initial, &headline.daily_returns)?;
    let sharpe6 = annual_ticks(&six.daily_returns)?.unwrap_or(0);
    let gate3 = total.is_some_and(|value| value > 0) && sharpe > 0 && sharpe6 > 0;
    let gate4 = if matches!(output.family.as_str(), "S01" | "H1" | "H2") {
        headline.accounting.n_rebalances >= 100 && headline.accounting.n_sign_changes >= 10
    } else {
        headline
            .round_trips
            .saturating_sub(headline.accounting.n_forced_exits)
            >= 100
    };
    let turnover_gate = output.family != "H1" || headline.turnover_ticks <= 4_000_000_000;
    Ok((gate1 && gate2 && gate3 && gate4 && turnover_gate).to_string())
}

fn h2_member_median(output: &FamilyOutput) -> Option<i64> {
    if output.h2_member_sharpes.len() != 12 {
        return None;
    }
    let mut values = output
        .h2_member_sharpes
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?;
    values.sort_unstable();
    i64::try_from((i128::from(values[5]) + i128::from(values[6])) / 2).ok()
}

fn h2_gate2prime(output: &FamilyOutput) -> Result<bool, String> {
    let Some(median) = h2_member_median(output) else {
        return Ok(false);
    };
    if output.folds.len() != 15 {
        return Ok(false);
    }
    let mut positive = 0;
    let mut total = 0i128;
    let mut largest = 0i128;
    for fold in &output.folds {
        let run = fold.test.first().ok_or("H2 fold has no blend")?;
        if annual_ticks(&run.daily_returns)?.is_some_and(|s| s > 0) {
            positive += 1;
        }
        let sum = run
            .daily_returns
            .iter()
            .map(|r| i128::from(*r))
            .sum::<i128>();
        total = total.checked_add(sum).ok_or("H2 fold sum overflow")?;
        largest = largest.max(sum);
    }
    let blend = annual_ticks(&output.stitched_oos_daily_returns)?;
    Ok(positive >= 9
        && total > 0
        && largest.checked_mul(2).ok_or("H2 concentration overflow")? <= total
        && blend.is_some_and(|s| s >= median))
}

fn selection_cells(fold: &FoldOutcome) -> (String, String) {
    if fold.selected_flat {
        (String::new(), "FLAT".to_owned())
    } else {
        (
            fold.selected_index.to_string(),
            fold.selected_config_id.clone(),
        )
    }
}

fn s_j_cell(fold: &FoldOutcome) -> String {
    fold.train
        .iter()
        .map(|run| {
            run.sharpe_annual_ticks
                .map(|ticks| format_scale(i128::from(ticks)))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn sharpes(returns: &[i64]) -> Result<(String, String), String> {
    let (daily, annual) = sharpe_of_return_ticks(returns).map_err(|err| err.to_string())?;
    Ok((
        opt_scale(daily.map(Ratio::ticks)),
        opt_scale(annual.map(Ratio::ticks)),
    ))
}

fn annual_ticks(returns: &[i64]) -> Result<Option<i64>, String> {
    sharpe_of_return_ticks(returns)
        .map(|(_, annual)| annual.map(Ratio::ticks))
        .map_err(|err| err.to_string())
}

fn moment_cells(returns: &[i64]) -> Result<(String, String), String> {
    match skew_kurt(returns).map_err(|_| "moments overflowed".to_owned())? {
        Some((skew, kurt)) => Ok((
            format_scale(i128::from(skew)),
            format_scale(i128::from(kurt)),
        )),
        None => Ok((String::new(), String::new())),
    }
}

fn total_of(first: Option<i64>, last: Option<i64>) -> Result<Option<i64>, String> {
    match (first, last) {
        (Some(first), Some(last)) if first > 0 => total_return(Cash(first), Cash(last))
            .map(|ratio| Some(ratio.ticks()))
            .map_err(|err| err.to_string()),
        _ => Ok(None),
    }
}

fn ratio_cell(ticks: Option<i64>) -> String {
    opt_scale(ticks)
}

fn compounded_return(initial: i64, returns: &[i64]) -> Result<String, String> {
    Ok(opt_scale(compounded_return_ticks(initial, returns)?))
}

fn compounded_return_ticks(initial: i64, returns: &[i64]) -> Result<Option<i64>, String> {
    let path = compound(initial, returns)?;
    let end = path.last().copied().ok_or("oos path is empty")?;
    total_return(Cash(initial), Cash(end))
        .map(|ratio| Some(ratio.ticks()))
        .map_err(|err| err.to_string())
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

fn opening_cash(output: &FamilyOutput) -> Result<i64, String> {
    output
        .folds
        .iter()
        .find_map(|fold| {
            fold.test
                .first()
                .and_then(|row| row.equity_ticks.first().copied())
                .or_else(|| {
                    fold.train
                        .first()
                        .and_then(|row| row.equity_ticks.first().copied())
                })
        })
        .filter(|cash| *cash > 0)
        .ok_or_else(|| format!("{} has no opening cash", output.family))
}

fn stitched_bounds(output: &FamilyOutput) -> Result<(String, String), String> {
    let first = output
        .folds
        .iter()
        .find_map(|fold| fold.test.first())
        .ok_or_else(|| format!("{} has no test window", output.family))?;
    let last = output
        .folds
        .iter()
        .rev()
        .find_map(|fold| fold.test.first())
        .ok_or_else(|| format!("{} has no test window", output.family))?;
    let end = inclusive_end(last.start_us, last.daily_returns.len())?;
    Ok((date_string(first.start_us), date_string(end)))
}

fn exposure_cell(position_us: i64, window_us: i64) -> Result<String, String> {
    if window_us <= 0 {
        return Ok("0".to_owned());
    }
    let ticks = i128::from(position_us)
        .checked_mul(SCALE)
        .ok_or("exposure overflow")?
        / i128::from(window_us);
    Ok(format_scale(ticks))
}

fn hold_hours(hold_us_sum: i64, round_trips: u64) -> Result<String, String> {
    if round_trips == 0 {
        return Ok("0".to_owned());
    }
    let den = i128::from(round_trips)
        .checked_mul(HOUR_US)
        .ok_or("avg hold overflow")?;
    Ok(format_div(i128::from(hold_us_sum), den))
}

fn inclusive_end(start_us: i64, days: usize) -> Result<i64, String> {
    if days == 0 {
        return Ok(start_us);
    }
    let span = i64::try_from(days - 1).map_err(|_| "days".to_owned())?;
    start_us
        .checked_add(span.checked_mul(DAY_US).ok_or("days")?)
        .ok_or_else(|| "days".to_owned())
}

fn denominator(days: usize) -> String {
    format!("daily 00:00Z net, ddof1, sqrt365, T={days}")
}

fn date_string(us: i64) -> String {
    UtcDate::from_micros(UtcMicros::from_micros(us)).to_string()
}

fn params_json(params: &BTreeMap<String, i64>) -> String {
    let mut body = String::from("{");
    for (index, (key, value)) in params.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&json_string(key));
        body.push(':');
        body.push_str(&value.to_string());
    }
    body.push('}');
    body
}

fn breakeven_cell(value: Option<Breakeven>) -> String {
    match value {
        Some(Breakeven::Fee(ticks)) => format_scale(ticks),
        Some(Breakeven::Above6) => ">6".to_owned(),
        Some(Breakeven::Below2) => "<2".to_owned(),
        None => String::new(),
    }
}

fn opt_scale(ticks: Option<i64>) -> String {
    ticks
        .map(|ticks| format_scale(i128::from(ticks)))
        .unwrap_or_default()
}

fn format_scale(ticks: i128) -> String {
    format_div(ticks, SCALE)
}

fn format_div(num: i128, den: i128) -> String {
    if den == 0 {
        return String::new();
    }
    let neg = (num < 0) ^ (den < 0);
    let num = num.abs();
    let den = den.abs();
    let whole = num / den;
    let mut frac = num % den;
    if frac == 0 {
        return if neg && whole != 0 {
            format!("-{whole}")
        } else {
            whole.to_string()
        };
    }
    let mut digits = String::new();
    for _ in 0..8 {
        frac *= 10;
        digits.push(char::from(b'0' + u8::try_from(frac / den).unwrap_or(0)));
        frac %= den;
        if frac == 0 {
            break;
        }
    }
    while digits.ends_with('0') {
        digits.pop();
    }
    if neg {
        format!("-{whole}.{digits}")
    } else {
        format!("{whole}.{digits}")
    }
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn join_row(cells: &[&str]) -> String {
    cells
        .iter()
        .map(|cell| csv_field(cell))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::walkforward::{FoldOutcome, GridPoint, WindowAccounting};

    fn swing() -> Vec<i64> {
        let mut out = Vec::new();
        for _ in 0..16 {
            out.extend_from_slice(&[200_000, -100_000, 180_000, -80_000]);
        }
        out
    }

    fn weak_swing() -> Vec<i64> {
        let mut out = Vec::new();
        for _ in 0..16 {
            out.extend_from_slice(&[120_000, -100_000, 100_000, -80_000]);
        }
        out
    }

    fn run(config_id: &str, grid: usize, returns: Vec<i64>, rebalances: u64) -> SegmentRun {
        let start = 1_609_459_200_000_000i64;
        SegmentRun {
            grid_index: grid,
            run_id: format!("{config_id}-{grid}"),
            trial_id: format!("trial-{config_id}"),
            config_id: config_id.to_owned(),
            start_us: start,
            daily_returns: returns,
            equity_ticks: vec![100_000_000_000_000, 100_000_000_000_000],
            net_result_ticks: 0,
            round_trips: 1,
            sharpe_annual_ticks: Some(50_000_000),
            fill_notional_ticks: 0,
            max_dd_ticks: 5_000_000,
            accounting: WindowAccounting {
                fees_paid_ticks: 100_000_000,
                slippage_cost_ticks: 2_000_000,
                n_rebalances: rebalances,
                n_sign_changes: 1,
                n_forced_exits: 2,
                ..WindowAccounting::ZERO
            },
        }
    }

    #[test]
    fn the_table_names_every_prereg_column_and_the_cscv_matrix() {
        let returns = swing();
        let weak = weak_swing();
        let train_a = run("cfg-strong", 0, returns.clone(), 4);
        let train_b = run("cfg-weak", 1, weak, 1);
        let test_a = run("cfg-strong", 0, returns.clone(), 3);
        let test_b = run("cfg-weak", 1, train_b.daily_returns.clone(), 1);
        let replay = |fee, slip| FeeReplay {
            fee_bps: fee,
            slippage_bps: slip,
            daily_returns: returns.clone(),
            equity_ticks: vec![100_000_000_000_000],
            net_result_ticks: 0,
            round_trips: 2,
            sharpe_annual_ticks: None,
            turnover_ticks: 0,
            max_dd_ticks: 5_000_000,
            accounting: WindowAccounting {
                n_rebalances: 7,
                n_sign_changes: 1,
                n_forced_exits: 4,
                slippage_cost_ticks: 2_000_000,
                ..WindowAccounting::ZERO
            },
        };
        let mut params = BTreeMap::new();
        params.insert("n".to_owned(), 24);
        let grid = FamilyGrid {
            family: "S01".to_owned(),
            slug: "tsmom-slow".to_owned(),
            prereg_sha256: "ab".repeat(32),
            folds: Vec::new(),
            points: vec![
                GridPoint {
                    config: (),
                    params: params.clone(),
                    attempt: 1,
                },
                GridPoint {
                    config: (),
                    params,
                    attempt: 1,
                },
            ],
        };
        let output = FamilyOutput {
            h2_member_sharpes: Vec::new(),
            family: "S01".to_owned(),
            folds: vec![FoldOutcome {
                cost_replays: Vec::new(),
                fold_index: 1,
                train: vec![train_a, train_b],
                test: vec![test_a, test_b],
                selected_index: 0,
                selected_config_id: "cfg-strong".to_owned(),
                selected_flat: false,
            }],
            stitched_oos_daily_returns: returns.clone(),
            stitched_oos_equity: Vec::new(),
            oos_round_trips: 2,
            n_trials: 2,
            fee_replays: vec![
                replay(2, 2),
                replay(4, 2),
                replay(5, 2),
                replay(6, 2),
                replay(5, 5),
            ],
            stay_flat_shadow: None,
        };
        let result = WalkForward {
            families: vec![output],
            n_trials: 2,
        };
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("missing.jsonl");
        write_tables(dir.path(), &[grid], &result, "pinned", "readsha", &ledger).unwrap();
        let table = fs::read_to_string(dir.path().join("table_tsmom-slow.csv")).unwrap();
        let mut lines = table.lines();
        assert_eq!(lines.next().unwrap(), TABLE_HEADER);
        let rows: Vec<Vec<String>> = lines
            .filter(|line| !line.is_empty())
            .map(parse_csv)
            .collect();
        assert_eq!(rows.len(), 9);
        let header: Vec<&str> = TABLE_HEADER.split(',').collect();
        let col = |name: &str| header.iter().position(|item| *item == name).unwrap();
        assert!(rows.iter().any(|row| row[col("breakeven_fee_bps")] == "<2"));
        assert!(rows
            .iter()
            .any(|row| !row[col("dsr_hurdle_sr_ann")].is_empty()));
        assert!(rows.iter().any(|row| !row[col("dsr")].is_empty()));
        assert!(rows.iter().any(|row| !row[col("pbo")].is_empty()));
        assert!(rows.iter().any(|row| row[col("slippage_cost")] == "0.02"));
        assert!(rows.iter().any(|row| row[col("fees_paid")] == "1"));
        assert!(rows.iter().any(|row| row[col("n_rebalances")] == "3"));
        assert!(rows.iter().any(|row| row[col("n_sign_changes")] == "1"));
        assert!(rows.iter().any(|row| row[col("n_forced_exits")] == "2"));
        assert!(rows
            .iter()
            .any(|row| row[col("selected_config_id")] == "cfg-strong"));
        assert!(rows.iter().any(|row| row[col("S_j")].contains(';')));
        let stitched = rows
            .iter()
            .find(|row| {
                row[col("window")] == "OOS_stitched"
                    && row[col("fee_bps")] == "5"
                    && row[col("slip_bps")] == "2"
            })
            .unwrap();
        assert_eq!(stitched[col("n_global")], "202");
        assert_eq!(stitched[col("funding_on")], "true");
        assert_eq!(
            stitched[col("sharpe_denominator")],
            "daily 00:00Z net, ddof1, sqrt365, T=64"
        );
        assert_eq!(stitched[col("gate_pass")], "false");
        assert!(
            stitched[col("fee_sens")].starts_with('0')
                || stitched[col("fee_sens")].starts_with('-')
        );
        let is_row = rows
            .iter()
            .find(|row| row[col("window")] == "IS_fold_1")
            .unwrap();
        assert_eq!(is_row[col("fee_bps")], "5");
        assert_eq!(is_row[col("slip_bps")], "2");
        assert!(is_row[col("params")].contains("\"n\":24"));
        let cscv = fs::read_to_string(dir.path().join("cscv_is_tsmom-slow.csv")).unwrap();
        let mut cscv_lines = cscv.lines();
        assert_eq!(cscv_lines.next().unwrap(), "day,cfg-strong,cfg-weak");
        assert_eq!(cscv_lines.filter(|line| !line.is_empty()).count(), 64);
    }

    fn parse_csv(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cell = String::new();
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if quoted {
                if ch == '"' && chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else if ch == '"' {
                    quoted = false;
                } else {
                    cell.push(ch);
                }
            } else if ch == '"' {
                quoted = true;
            } else if ch == ',' {
                out.push(std::mem::take(&mut cell));
            } else {
                cell.push(ch);
            }
        }
        out.push(cell);
        out
    }
    fn qr_fixture(family: &str) -> (FamilyGrid<()>, FamilyOutput) {
        let returns = vec![1_000_000, 2_000_000, 3_000_000, 4_000_000];
        let mut selected = run("config", 0, returns.clone(), 100);
        selected.accounting.n_sign_changes = 10;
        let mut folds = Vec::new();
        for fold_index in 1..=15 {
            let mut replay_rows = Vec::new();
            for segment in [
                crate::walkforward::Segment::Train,
                crate::walkforward::Segment::Test,
            ] {
                for (fee_bps, slippage_bps) in [(2, 2), (4, 2), (6, 2), (5, 5)] {
                    replay_rows.push(crate::walkforward::WindowReplay {
                        segment,
                        fee_bps,
                        slippage_bps,
                        run: selected.clone(),
                    });
                }
            }
            folds.push(FoldOutcome {
                cost_replays: replay_rows,
                fold_index,
                train: vec![selected.clone()],
                test: vec![selected.clone()],
                selected_index: 0,
                selected_config_id: "config".into(),
                selected_flat: false,
            });
        }
        let replay = |fee_bps, slippage_bps| FeeReplay {
            fee_bps,
            slippage_bps,
            daily_returns: returns.repeat(15),
            equity_ticks: vec![100_000_000_000_000],
            net_result_ticks: 0,
            round_trips: 200,
            sharpe_annual_ticks: None,
            turnover_ticks: 4_000_000_000,
            max_dd_ticks: 0,
            accounting: selected.accounting,
        };
        (
            FamilyGrid {
                family: family.into(),
                slug: family.into(),
                prereg_sha256: "a".repeat(64),
                folds: Vec::new(),
                points: vec![GridPoint {
                    config: (),
                    params: BTreeMap::new(),
                    attempt: 2,
                }],
            },
            FamilyOutput {
                h2_member_sharpes: vec![Some(100_000_000); 12],
                family: family.into(),
                folds,
                stitched_oos_daily_returns: returns.repeat(15),
                stitched_oos_equity: vec![100_000_000_000_000],
                oos_round_trips: 200,
                n_trials: 1,
                stay_flat_shadow: None,
                fee_replays: [(2, 2), (4, 2), (5, 2), (6, 2), (5, 5)]
                    .into_iter()
                    .map(|(f, s)| replay(f, s))
                    .collect(),
            },
        )
    }

    #[test]
    fn qr1_h2_uses_ratified_gate_with_all_twelve_member_sharpes() {
        let (_, mut output) = qr_fixture("H2");
        assert_eq!(gate_cell(&output, 202, 25_000_000, None).unwrap(), "true");
        let mut few_positive = output.clone();
        for fold in &mut few_positive.folds[..7] {
            fold.test[0].daily_returns = vec![-1_000_000, -2_000_000, -3_000_000, -4_000_000];
        }
        assert!(!h2_gate2prime(&few_positive).unwrap());
        let mut concentrated = output.clone();
        concentrated.folds[0].test[0].daily_returns = vec![100_000_000; 4];
        assert!(!h2_gate2prime(&concentrated).unwrap());
        let mut undefined = output.clone();
        undefined.h2_member_sharpes[0] = None;
        assert!(!h2_gate2prime(&undefined).unwrap());
        output.h2_member_sharpes = vec![Some(10_000_000_000); 12];
        assert_eq!(gate_cell(&output, 202, 25_000_000, None).unwrap(), "false");
        output.h2_member_sharpes.pop();
        assert_eq!(gate_cell(&output, 202, 25_000_000, None).unwrap(), "false");
    }

    #[test]
    fn qr5_every_config_window_has_all_five_cost_scenarios() {
        let (grid, output) = qr_fixture("H1");
        let text = family_table(&grid, &output, "pin", "sql", 202).unwrap();
        let header: Vec<_> = TABLE_HEADER.split(',').collect();
        let column = |key| header.iter().position(|v| *v == key).unwrap();
        let rows: Vec<_> = text
            .lines()
            .skip(1)
            .filter(|s| !s.is_empty())
            .map(parse_csv)
            .collect();
        for window in ["IS_fold_1", "OOS_fold_1"] {
            let mut costs: Vec<_> = rows
                .iter()
                .filter(|r| r[column("window")] == window)
                .map(|r| (r[column("fee_bps")].clone(), r[column("slip_bps")].clone()))
                .collect();
            costs.sort();
            assert_eq!(
                costs,
                vec![
                    ("2".into(), "2".into()),
                    ("4".into(), "2".into()),
                    ("5".into(), "2".into()),
                    ("5".into(), "5".into()),
                    ("6".into(), "2".into())
                ]
            );
        }
    }

    #[test]
    fn qr6_h1_turnover_limit_is_inclusive_and_mandatory() {
        let (_, mut output) = qr_fixture("H1");
        assert_eq!(
            gate_cell(&output, 202, 25_000_000, Some(0)).unwrap(),
            "true"
        );
        output
            .fee_replays
            .iter_mut()
            .find(|r| r.fee_bps == 5 && r.slippage_bps == 2)
            .unwrap()
            .turnover_ticks += 1;
        assert_eq!(
            gate_cell(&output, 202, 25_000_000, Some(0)).unwrap(),
            "false"
        );
    }

    #[test]
    fn qr7_stitched_drawdown_retains_a_prior_fold_peak() {
        let (_, mut output) = qr_fixture("H1");
        output.folds.truncate(2);
        let cash = 100_000_000_000_000i64;
        let a = &mut output.folds[0].test[0];
        a.equity_ticks = vec![cash, cash * 9 / 10];
        a.daily_returns = vec![-10_000_000];
        a.max_dd_ticks = 10_000_000;
        a.accounting.equity_range = Some((cash * 9 / 10, cash));
        let b = &mut output.folds[1].test[0];
        b.equity_ticks = vec![cash, cash];
        b.daily_returns = vec![0];
        b.max_dd_ticks = 20_000_000;
        b.accounting.equity_range = Some((cash * 8 / 10, cash));
        let replay =
            crate::walkforward::replay_from_selection::<String>(5, 2, &output.folds).unwrap();
        assert_eq!(replay.max_dd_ticks, 28_000_000);
    }
}
