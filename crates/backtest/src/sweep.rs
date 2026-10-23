//! Read `sweep-inputs-v2.json` and run one trial per config on the v1a tape.
//!
//! [`FoldRunner::run_config_window`] is the per-config entry a fold runner can
//! call with its own `(start_us, end_us)`. This driver calls it once per config
//! on the config's full window. It writes the ledger and the out directory.
//! It writes the trial ledger and `{out}/digest.jsonl`. It does not write
//! full-window performance numbers. Those come from a walk-forward run.
//!
//! Sizing uses equity marked at the decision close. That close stands in for
//! the next 1m open, which is the fill. A hold that `arm` refuses, because it
//! would end after 2025-09-30 23:59 UTC, increments `n_dropped_entries` on
//! the trial row. `Tape::load` reads the pinned klines,
//! `runs/funding-4db8a75`, and the `um-kline-2023-11-10` overlay. A path that
//! is not in the pin list is not opened.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use parangsae_core::{
    AvailableTs, BarInterval, Causal, EventTs, Interval, Observation, ReceivedTs, ResampledBar,
    Symbol, Timestamps, UtcMicros, SCALE,
};
use parangsae_signals::SignalOutput;
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};
use parangsae_strategies::EARLIEST_US;
use serde::Deserialize;

use crate::batch::{
    metrics_from_grid, run_batch_digest, with_headline, FlatBars, SweepPins, TargetRunner,
    TrialMetrics, TrialSpec, HOLDOUT_START_US,
};
use crate::engine::{run_targets, MarkAbsence, RunConfig, FEE_GRID_BPS};
use crate::signal::{BookState, FundingObs, TargetSignal};
use crate::tape::{planned_paths, Overlay, TapeConfig};
use crate::{Report, ResultRow, Tape, UtcDate};

const LOT_TICKS: i64 = 100_000;
const FUNDING_REL: &str = "runs/funding-4db8a75";
const OVERLAY_REL: &str = "runs/resolutions/um-kline-2023-11-10.parquet";

/// What the batch binary was asked to run.
pub struct SweepRequest {
    pub inputs: PathBuf,
    pub data_root: PathBuf,
    pub out_dir: PathBuf,
    pub ledger: PathBuf,
    pub slippage_bps: u32,
}

/// One grid point. `initial_cash` is cash ticks.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    pub family: String,
    pub params: BTreeMap<String, i64>,
    pub initial_cash: i64,
}

/// The trial metrics plus the daily return series for this window.
///
/// `daily_returns[i]` is `(equity_i - prev) * 1e8 / prev`, toward zero. `prev`
/// starts at `initial_cash`.
#[derive(Clone, Debug)]
pub struct WindowOutput {
    pub trial: TrialMetrics,
    pub daily_returns: Vec<i64>,
}

/// One loaded tape. A later fold runner calls [`Self::run_config_window`] per fold.
pub struct FoldRunner<'a> {
    tape: &'a Tape,
}

impl<'a> FoldRunner<'a> {
    pub fn new(tape: &'a Tape) -> Self {
        Self { tape }
    }

    /// Run `config` on `[start_us, end_us)` at `costs`.
    ///
    /// The 2/4/5/6 grid keeps `costs.slippage`. The returned trial is the
    /// headline at `costs`. The daily series is not printed.
    pub fn run_config_window(
        &self,
        config: &WindowConfig,
        start_us: i64,
        end_us: i64,
        costs: &Costs,
    ) -> Result<WindowOutput, String> {
        let mut grid = Vec::with_capacity(FEE_GRID_BPS.len());
        for bps in FEE_GRID_BPS {
            let scenario = Costs {
                taker_fee: FeeBps(bps),
                slippage: costs.slippage,
            };
            let report = self.run_once(config, start_us, end_us, &scenario)?;
            grid.push((FeeBps(bps), report));
        }
        let (headline, dropped) = self.run_counted(config, start_us, end_us, costs)?;
        let flat_bars = FlatBars {
            funding_stale: headline.flat_funding_stale,
            funding_slot_missing: headline.flat_funding_slot_missing,
        };
        let metrics = metrics_from_grid(Cash(config.initial_cash), &grid, flat_bars)
            .map_err(|err| err.to_string())?;
        let mut trial = with_headline(metrics, Cash(config.initial_cash), &headline)
            .map_err(|err| err.to_string())?;
        trial.n_dropped_entries = dropped;
        let daily: Vec<i64> = headline
            .daily_equity
            .iter()
            .map(|(_, cash)| cash.0)
            .collect();
        let daily_returns = daily_return_ticks(config.initial_cash, &daily)?;
        Ok(WindowOutput {
            trial,
            daily_returns,
        })
    }

    pub(crate) fn run_once(
        &self,
        config: &WindowConfig,
        start_us: i64,
        end_us: i64,
        costs: &Costs,
    ) -> Result<Report, String> {
        self.run_counted(config, start_us, end_us, costs)
            .map(|(report, _count)| report)
    }

    fn run_counted(
        &self,
        config: &WindowConfig,
        start_us: i64,
        end_us: i64,
        costs: &Costs,
    ) -> Result<(Report, u64), String> {
        let mut signal = FamilySignal::new(&config.family, &config.params)?;
        let cfg = research_config(
            config.initial_cash,
            start_us,
            start_us,
            end_us,
            signal.interval,
            false,
        )?;
        let tape = self.tape;
        let costs = *costs;
        let report = match panic::catch_unwind(AssertUnwindSafe(|| {
            run_targets(tape, &mut signal, &costs, &cfg)
        })) {
            Ok(result) => result.map_err(|err| err.to_string())?,
            Err(payload) => return Err(panic_message(&payload)),
        };
        Ok((report, signal.dropped_entries()))
    }

    /// One scored window. Bars from `warmup_us` reach the signal. Fills start at
    /// `start_us`. The position is flattened at the last traded minute before `end_us`.
    pub fn run_scored(
        &self,
        config: &WindowConfig,
        warmup_us: i64,
        start_us: i64,
        end_us: i64,
        costs: &Costs,
    ) -> Result<crate::walkforward::WindowResult, String> {
        let mut signal = FamilySignal::new(&config.family, &config.params)?;
        let cfg = research_config(
            config.initial_cash,
            warmup_us,
            start_us,
            end_us,
            signal.interval,
            true,
        )?;
        let tape = self.tape;
        let costs = *costs;
        let report = match panic::catch_unwind(AssertUnwindSafe(|| {
            run_targets(tape, &mut signal, &costs, &cfg)
        })) {
            Ok(result) => result.map_err(|err| err.to_string())?,
            Err(payload) => return Err(panic_message(&payload)),
        };
        crate::walkforward::window_result(Cash(config.initial_cash), &report)
            .map_err(|err| err.to_string())
    }
}

pub(crate) struct Loaded {
    pub(crate) slippage_bps: u32,
    pub(crate) configs: Vec<ConfigRow>,
}

#[derive(Deserialize)]
struct SweepFile {
    schema: String,
    slippage_bps: u32,
    configs: Vec<ConfigRow>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct ConfigRow {
    pub(crate) config_id: String,
    pub(crate) family: String,
    pub(crate) grid_index: u32,
    pub(crate) params: BTreeMap<String, i64>,
    pub(crate) start_us: i64,
    pub(crate) end_us: i64,
    pub(crate) initial_cash: i64,
}

struct ConfigRunner<'a> {
    fold: &'a FoldRunner<'a>,
    family: String,
    params: BTreeMap<String, i64>,
}

impl TargetRunner for ConfigRunner<'_> {
    type Error = String;

    fn run_targets(&mut self, costs: &Costs, cfg: &RunConfig) -> Result<TrialMetrics, String> {
        let config = WindowConfig {
            family: self.family.clone(),
            params: self.params.clone(),
            initial_cash: cfg.initial_cash.0,
        };
        self.fold
            .run_config_window(&config, cfg.start.micros(), cfg.end.micros(), costs)
            .map(|output| output.trial)
    }
}

/// Run every config. Writes the ledger and `{out}/digest.jsonl`.
///
/// The digest line is the trial identity and `metrics_digest`. It has no
/// return, Sharpe, or stress figures.
pub fn run_sweep(request: &SweepRequest) -> Result<Vec<ResultRow>, String> {
    let loaded = load_configs(&request.inputs)?;
    if loaded.slippage_bps != request.slippage_bps {
        return Err(format!(
            "slippage_bps {} in {} conflicts with --slippage-bps {}",
            loaded.slippage_bps,
            request.inputs.display(),
            request.slippage_bps
        ));
    }
    if loaded.configs.is_empty() {
        return Err("sweep inputs have no configs".to_owned());
    }
    let start_us = loaded.configs[0].start_us;
    let end_us = loaded.configs[0].end_us;
    if loaded
        .configs
        .iter()
        .any(|row| row.start_us != start_us || row.end_us != end_us)
    {
        return Err("configs do not share one window".to_owned());
    }
    let pins = SweepPins::load(&request.inputs).map_err(|err| err.to_string())?;
    let tape = load_pinned_tape(&request.data_root, &pins, start_us, end_us)?;
    let fold = FoldRunner::new(&tape);
    let ts = UtcMicros::from_micros(now_us()?);
    fs::create_dir_all(&request.out_dir)
        .map_err(|err| format!("create {}: {err}", request.out_dir.display()))?;
    if let Some(parent) = request.ledger.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create {}: {err}", parent.display()))?;
        }
    }
    let digest_path = request.out_dir.join("digest.jsonl");
    let costs = Costs {
        taker_fee: FeeBps(5),
        slippage: SlippageBps(request.slippage_bps),
    };
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(loaded.configs.len());
    for row in &loaded.configs {
        let attempt = next_attempt(&request.ledger, &row.config_id)?;
        let spec = trial_spec(row, ts, attempt)?;
        if !seen.insert(spec.run_id.clone()) {
            return Err(format!("duplicate run_id {}", spec.run_id));
        }
        let mut runner = ConfigRunner {
            fold: &fold,
            family: row.family.clone(),
            params: row.params.clone(),
        };
        let batch = run_batch_digest(
            std::slice::from_ref(&spec),
            &pins,
            &request.ledger,
            &digest_path,
            &costs,
            &mut runner,
        )
        .map_err(|err| err.to_string())?;
        rows.extend(batch);
    }
    Ok(rows)
}

pub(crate) fn load_configs(path: &Path) -> Result<Loaded, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let file: SweepFile =
        serde_json::from_str(&text).map_err(|err| format!("sweep inputs: {err}"))?;
    if file.schema != "sweep-runner-inputs-v2" {
        return Err(format!(
            "schema {} is not sweep-runner-inputs-v2",
            file.schema
        ));
    }
    Ok(Loaded {
        slippage_bps: file.slippage_bps,
        configs: file.configs,
    })
}

fn trial_spec(row: &ConfigRow, ts: UtcMicros, attempt: u64) -> Result<TrialSpec, String> {
    let strategy = parangsae_strategies::Strategy::from_params(&row.family, &row.params)
        .map_err(|err| format!("{}: {err}", row.family))?;
    let spec = TrialSpec {
        run_id: format!("{}-{:02}-a{attempt}", row.family, row.grid_index),
        config_id: row.config_id.clone(),
        family: row.family.clone(),
        params: row.params.clone(),
        attempt,
        ts,
        run: research_config(
            row.initial_cash,
            row.start_us,
            row.start_us,
            row.end_us,
            strategy.interval(),
            false,
        )?,
    };
    let canonical = crate::canonical_config_id(&spec);
    if spec.config_id != canonical {
        return Err(format!(
            "config_id {} is not {canonical} for {}",
            spec.config_id, spec.run_id
        ));
    }
    Ok(spec)
}

pub(crate) fn load_pinned_tape(
    root: &Path,
    pins: &SweepPins,
    start_us: i64,
    end_us: i64,
) -> Result<Tape, String> {
    let (start, end) = inclusive_days(start_us, end_us)?;
    let symbol = Symbol::new("BTCUSDT").map_err(|err| err.to_string())?;
    let cfg = tape_config(root, pins)?;
    let paths = planned_paths(root, &symbol, start, end, &cfg).map_err(|err| err.to_string())?;
    for path in &paths {
        if !pins.allows(path) {
            return Err(format!("{} is not in the pinned objects", path.display()));
        }
    }
    let tape = Tape::load(root, &symbol, start, end, &cfg).map_err(|err| err.to_string())?;
    assert_inside(&tape, start_us, end_us)?;
    Ok(tape)
}

fn tape_config(root: &Path, pins: &SweepPins) -> Result<TapeConfig, String> {
    let overlay_path = root.join(OVERLAY_REL);
    let sha_hex = pins
        .sha256(&overlay_path)
        .ok_or_else(|| format!("{} is not in the pinned objects", overlay_path.display()))?;
    Ok(TapeConfig {
        holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
        overlays: vec![Overlay {
            dataset: "um-kline".to_owned(),
            path: overlay_path,
            sha256: parse_sha(sha_hex)?,
        }],
        funding_root: root.join(FUNDING_REL),
    })
}

fn inclusive_days(start_us: i64, end_us: i64) -> Result<(UtcDate, UtcDate), String> {
    if start_us < EARLIEST_US {
        return Err(format!("start {start_us} is before 2020-12-18 00:00 UTC"));
    }
    if end_us <= start_us {
        return Err("window end is not after start".to_owned());
    }
    if start_us >= HOLDOUT_START_US || end_us > HOLDOUT_START_US {
        return Err(format!("window {start_us}..{end_us} touches the holdout"));
    }
    let start = UtcDate::from_micros(UtcMicros::from_micros(start_us));
    let end = UtcDate::from_micros(UtcMicros::from_micros(end_us - 1));
    let holdout = UtcDate::from_micros(UtcMicros::from_micros(HOLDOUT_START_US));
    if start >= holdout || end >= holdout {
        return Err("a date partition is on or after 2025-10-01".to_owned());
    }
    Ok((start, end))
}

fn assert_inside(tape: &Tape, start_us: i64, end_us: i64) -> Result<(), String> {
    for bar in tape.bars.iter().chain(tape.marks.iter()) {
        let open = bar.open_time.micros();
        let in_window = (start_us..end_us).contains(&open);
        let in_research = (EARLIEST_US..HOLDOUT_START_US).contains(&open);
        if !in_window || !in_research {
            return Err(format!("bar open {open} is outside the research window"));
        }
    }
    for stamp in &tape.funding {
        let ts = stamp.funding_time.micros();
        if !(EARLIEST_US..HOLDOUT_START_US).contains(&ts) {
            return Err(format!("funding {ts} is outside the research window"));
        }
    }
    Ok(())
}

/// Next attempt is one past the largest `RunStarted` attempt for `config_id`.
pub(crate) fn next_attempt(ledger: &Path, config_id: &str) -> Result<u64, String> {
    if !ledger.exists() {
        return Ok(0);
    }
    let text =
        fs::read_to_string(ledger).map_err(|err| format!("read {}: {err}", ledger.display()))?;
    let mut max_attempt: Option<u64> = None;
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|err| format!("ledger line {}: {err}", index + 1))?;
        if value.get("kind").and_then(|kind| kind.as_str()) != Some("RunStarted") {
            continue;
        }
        if value.get("config_id").and_then(|id| id.as_str()) != Some(config_id) {
            continue;
        }
        let attempt = value
            .get("attempt")
            .and_then(|attempt| attempt.as_u64())
            .ok_or_else(|| format!("ledger line {} has no attempt", index + 1))?;
        max_attempt = Some(max_attempt.map_or(attempt, |seen| seen.max(attempt)));
    }
    match max_attempt {
        None => Ok(0),
        Some(seen) => seen
            .checked_add(1)
            .ok_or_else(|| format!("attempt for {config_id} does not fit")),
    }
}

fn research_config(
    cash: i64,
    warmup_us: i64,
    start_us: i64,
    end_us: i64,
    interval: BarInterval,
    flatten: bool,
) -> Result<RunConfig, String> {
    if cash <= 0 {
        return Err("initial cash is not positive".to_owned());
    }
    if warmup_us > start_us || end_us < start_us {
        return Err("window end is before start".to_owned());
    }
    let end = UtcMicros::from_micros(end_us);
    Ok(RunConfig {
        initial_cash: Cash(cash),
        warmup_start: UtcMicros::from_micros(warmup_us),
        start: UtcMicros::from_micros(start_us),
        end,
        holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
        forced_flat: Vec::new(),
        flatten_before: flatten.then_some(end),
        mark_gaps: vec![mark_gap()?],
        interval: match interval {
            BarInterval::H1 => Interval::ONE_HOUR,
            BarInterval::H4 => Interval::FOUR_HOURS,
        },
        lot_ticks: LOT_TICKS,
    })
}

fn mark_gap() -> Result<MarkAbsence, String> {
    let gap_day = UtcDate::parse("2024-08-12")
        .ok_or("2024-08-12 is not a date")?
        .start_us();
    let minute = 60_000_000i64;
    let first = gap_day
        .checked_add_micros(10 * 3_600_000_000 + 2 * minute)
        .ok_or("mark gap start")?;
    let last = gap_day
        .checked_add_micros(10 * 3_600_000_000 + 3 * minute)
        .ok_or("mark gap end")?;
    Ok(MarkAbsence {
        first_open: first,
        last_open: last,
        known_source_absence: true,
    })
}

fn daily_return_ticks(initial: i64, daily: &[i64]) -> Result<Vec<i64>, String> {
    let mut out = Vec::with_capacity(daily.len());
    let mut prev = initial;
    for equity in daily {
        if prev <= 0 {
            return Err("daily return has no positive base".to_owned());
        }
        let scaled = (i128::from(*equity) - i128::from(prev))
            .checked_mul(SCALE)
            .ok_or("daily return overflow")?;
        let ret = scaled / i128::from(prev);
        out.push(i64::try_from(ret).map_err(|_| "daily return overflow".to_owned())?);
        prev = *equity;
    }
    Ok(out)
}

fn parse_sha(hex_text: &str) -> Result<[u8; 32], String> {
    if hex_text.len() != 64 {
        return Err(format!("sha length {}", hex_text.len()));
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        let digits = hex_text
            .get(index * 2..index * 2 + 2)
            .ok_or("sha is short")?;
        *byte = u8::from_str_radix(digits, 16).map_err(|_| format!("bad sha {hex_text}"))?;
    }
    Ok(out)
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    "strategy panic".to_owned()
}

fn now_us() -> Result<i64, String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?;
    i64::try_from(elapsed.as_micros()).map_err(|_| "clock does not fit in i64".to_owned())
}

struct FamilySignal {
    strategy: parangsae_strategies::Strategy,
    family: String,
    params: BTreeMap<String, i64>,
    interval: BarInterval,
    seen: Vec<Observation<ResampledBar>>,
}

impl FamilySignal {
    fn new(family: &str, params: &BTreeMap<String, i64>) -> Result<Self, String> {
        let strategy = parangsae_strategies::Strategy::from_params(family, params)
            .map_err(|err| err.to_string())?;
        let interval = strategy.interval();
        Ok(Self {
            strategy,
            family: family.to_owned(),
            params: params.clone(),
            interval,
            seen: Vec::new(),
        })
    }

    fn dropped_entries(&self) -> u64 {
        self.strategy.dropped_entries()
    }

    /// Bars already pushed stay put. A broken prefix rebuilds the strategy and
    /// replays from the first row. Warmup rows call `observe`. The last row of
    /// a scored bar is the only `push`.
    fn absorb(
        &mut self,
        rows: &[ResampledBar],
        decide_last: bool,
        book: Option<parangsae_strategies::DecisionBook>,
    ) -> Option<SignalOutput> {
        let fresh = self.fresh_rows(rows);
        let last = fresh.len().saturating_sub(1);
        let mut output = None;
        for (index, row) in fresh.into_iter().enumerate() {
            if decide_last && index == last {
                let Some(marked) = book else {
                    panic::panic_any("scored bar has no book");
                };
                output = match self.strategy.push(&row, Some(marked)) {
                    Ok(value) => value,
                    Err(err) => panic::panic_any(err.to_string()),
                };
            } else if let Err(err) = self.strategy.observe(&row) {
                panic::panic_any(err.to_string());
            }
            self.seen.push(row);
        }
        output
    }

    fn fresh_rows(&mut self, rows: &[ResampledBar]) -> Vec<Observation<ResampledBar>> {
        if !self.prefix_holds(rows) {
            match parangsae_strategies::Strategy::from_params(&self.family, &self.params) {
                Ok(strategy) => self.strategy = strategy,
                Err(err) => panic::panic_any(err.to_string()),
            }
            self.seen.clear();
        }
        rows[self.seen.len()..]
            .iter()
            .copied()
            .map(bar_observation)
            .collect()
    }

    fn prefix_holds(&self, rows: &[ResampledBar]) -> bool {
        self.seen.len() <= rows.len()
            && self
                .seen
                .iter()
                .zip(rows)
                .all(|(seen, row)| seen.value().open_time() == row.open_time())
    }
}

impl TargetSignal for FamilySignal {
    fn on_warmup_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        _funding: Causal<'_, &[FundingObs]>,
    ) {
        self.absorb(bars.value(), false, None);
    }

    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        _funding: Causal<'_, &[FundingObs]>,
        book: BookState,
    ) -> Option<SignalOutput> {
        let marked = parangsae_strategies::DecisionBook {
            equity_ticks: book.equity_ticks,
            position_lots: book.position_lots,
            mark_ticks: book.mark_ticks,
        };
        self.absorb(bars.value(), true, Some(marked))
    }
}

fn bar_observation(bar: ResampledBar) -> Observation<ResampledBar> {
    let close = bar.close_time();
    let available = close
        .checked_add_micros(1_000)
        .expect("bar availability fits");
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(close),
            available_ts: AvailableTs::from_micros(available),
            received_ts: ReceivedTs::from_micros(available),
        },
        bar,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::run_batch;
    use crate::report::Report;
    use crate::{canonical_config_id, run_targets, selection_costs, Bar, FundingStamp, RunConfig};
    use parangsae_core::{Price, Qty, Ratio, UtcMicros};
    use parangsae_sim::{Cash, FeeBps, SlippageBps};

    const ALPHA_SHA: &str = "8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8";

    #[test]
    fn the_published_inputs_have_43_canonical_configs() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../research/sweep-inputs-v2.json");
        let loaded = load_configs(&path).unwrap();
        assert_eq!(loaded.slippage_bps, 2);
        assert_eq!(loaded.configs.len(), 43);
        let mut families = BTreeMap::<String, usize>::new();
        for row in &loaded.configs {
            *families.entry(row.family.clone()).or_insert(0) += 1;
            let spec = trial_spec(row, UtcMicros::from_micros(1), 0).unwrap();
            assert_eq!(spec.config_id, row.config_id);
            assert_eq!(spec.run.end.micros(), HOLDOUT_START_US);
            assert_eq!(spec.run.start.micros(), EARLIEST_US);
            assert_eq!(spec.attempt, 0);
            assert!(spec.run_id.ends_with("-a0"));
        }
        assert_eq!(families.get("S01").copied(), Some(12));
        assert_eq!(families.get("S02").copied(), Some(9));
        assert_eq!(families.get("S03").copied(), Some(8));
        assert_eq!(families.get("S05").copied(), Some(8));
        assert_eq!(families.get("S06").copied(), Some(6));
    }

    #[test]
    fn a_holdout_window_is_refused_before_any_file_is_opened() {
        let err = inclusive_days(EARLIEST_US, HOLDOUT_START_US + 1).unwrap_err();
        assert!(err.contains("holdout"));
        let err = inclusive_days(EARLIEST_US - 1, HOLDOUT_START_US).unwrap_err();
        assert!(err.contains("2020-12-18"));
        let (start, end) = inclusive_days(EARLIEST_US, HOLDOUT_START_US).unwrap();
        assert_eq!(start.to_string(), "2020-12-18");
        assert_eq!(end.to_string(), "2025-09-30");
    }

    #[test]
    fn an_unpinned_day_is_refused_before_it_is_opened() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let notes = dir.path().join("notes.md");
        fs::write(&notes, b"alpha").unwrap();
        let pin_path = dir.path().join("sweep-inputs.json");
        let body = serde_json::json!({
            "schema": "sweep-runner-inputs-v2",
            "objects": [{"path": notes.to_string_lossy(), "sha256": ALPHA_SHA}],
        });
        fs::write(&pin_path, serde_json::to_string(&body).unwrap()).unwrap();
        let pins = SweepPins::load(&pin_path).unwrap();
        let start = UtcDate::parse("2024-01-01").unwrap().start_us().micros();
        let end = UtcDate::parse("2024-01-02").unwrap().start_us().micros();
        let err = load_pinned_tape(&root, &pins, start, end).unwrap_err();
        assert!(err.contains("not in the pinned"), "{err}");
        assert!(!err.contains("missing parquet"), "{err}");
        assert!(!err.contains("notes.md"), "{err}");
    }

    #[test]
    fn a_pinned_day_is_opened_and_an_extra_pin_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let start_day = UtcDate::parse("2024-01-01").unwrap();
        let start = start_day.start_us().micros();
        let end = UtcDate::parse("2024-01-02").unwrap().start_us().micros();
        let symbol = Symbol::new("BTCUSDT").unwrap();
        let notes = dir.path().join("notes.md");
        fs::write(&notes, b"alpha").unwrap();
        let overlay = root.join(OVERLAY_REL);
        let cfg = TapeConfig {
            holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
            overlays: vec![Overlay {
                dataset: "um-kline".to_owned(),
                path: overlay.clone(),
                sha256: parse_sha(ALPHA_SHA).unwrap(),
            }],
            funding_root: root.join(FUNDING_REL),
        };
        let planned = planned_paths(&root, &symbol, start_day, start_day, &cfg).unwrap();
        let mut objects = Vec::new();
        for path in &planned {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, b"alpha").unwrap();
            objects.push(serde_json::json!({
                "path": path.to_string_lossy(),
                "sha256": ALPHA_SHA,
            }));
        }
        objects.push(serde_json::json!({
            "path": notes.to_string_lossy(),
            "sha256": ALPHA_SHA,
        }));
        let pin_path = dir.path().join("sweep-inputs.json");
        fs::write(
            &pin_path,
            serde_json::to_string(&serde_json::json!({
                "schema": "sweep-runner-inputs-v2",
                "objects": objects,
            }))
            .unwrap(),
        )
        .unwrap();
        let pins = SweepPins::load(&pin_path).unwrap();
        let err = load_pinned_tape(&root, &pins, start, end).unwrap_err();
        assert!(!err.contains("not in the pinned"), "{err}");
        assert!(!err.contains("notes.md"), "{err}");
    }

    #[test]
    fn the_ledger_picks_the_next_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("trials.jsonl");
        assert_eq!(next_attempt(&ledger, "cfg").unwrap(), 0);
        fs::write(
            &ledger,
            "{\"kind\":\"RunStarted\",\"config_id\":\"cfg\",\"attempt\":0}\n\
             {\"kind\":\"RunFinished\",\"config_id\":\"cfg\",\"attempt\":0}\n\
             {\"kind\":\"RunStarted\",\"config_id\":\"other\",\"attempt\":4}\n\
             {\"kind\":\"RunStarted\",\"config_id\":\"cfg\",\"attempt\":2}\n",
        )
        .unwrap();
        assert_eq!(next_attempt(&ledger, "cfg").unwrap(), 3);
        assert_eq!(next_attempt(&ledger, "missing").unwrap(), 0);
    }

    #[test]
    fn a_window_over_the_2024_08_12_mark_gap_finishes() {
        let day = UtcDate::parse("2024-08-12").unwrap().start_us().micros();
        let minute = 60_000_000i64;
        let gap_1002 = day + 10 * 3_600_000_000 + 2 * minute;
        let gap_1003 = day + 10 * 3_600_000_000 + 3 * minute;
        assert_eq!(gap_1002, 1_723_456_920_000_000);
        assert_eq!(gap_1003, 1_723_456_980_000_000);
        let start = day + 9 * 3_600_000_000;
        let mut bars = Vec::new();
        for index in 0..180 {
            let open_us = start + index * minute;
            let price = Price::from_ticks(50 * 100_000_000).unwrap();
            bars.push(Bar {
                open_time: UtcMicros::from_micros(open_us),
                close_time: UtcMicros::from_micros(open_us + minute - 1_000),
                available: UtcMicros::from_micros(open_us + minute),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: Qty::from_ticks(100_000_000).unwrap(),
                count: 1,
            });
        }
        let marks: Vec<Bar> = bars
            .iter()
            .copied()
            .filter(|bar| {
                let open = bar.open_time.micros();
                open != gap_1002 && open != gap_1003
            })
            .collect();
        assert_eq!(marks.len(), bars.len() - 2);
        let tape = Tape::from_parts(bars, marks, Vec::new());
        let mut params = BTreeMap::new();
        params.insert("hold_h".to_owned(), 2);
        params.insert("n".to_owned(), 2);
        let row = ConfigRow {
            config_id: String::new(),
            family: "S02".to_owned(),
            grid_index: 0,
            params: params.clone(),
            start_us: start,
            end_us: start + 180 * minute,
            initial_cash: 100_000_000_000_000,
        };
        let mut spec = TrialSpec {
            run_id: "pending".to_owned(),
            config_id: String::new(),
            family: row.family.clone(),
            params: row.params.clone(),
            attempt: 0,
            ts: UtcMicros::from_micros(start),
            run: research_config(
                row.initial_cash,
                row.start_us,
                row.start_us,
                row.end_us,
                BarInterval::H1,
                false,
            )
            .unwrap(),
        };
        spec.config_id = canonical_config_id(&spec);
        spec.run_id = format!("S02-00-a{}", spec.attempt);
        let gap = &spec.run.mark_gaps;
        assert_eq!(gap.len(), 1);
        assert_eq!(gap[0].first_open.micros(), gap_1002);
        assert_eq!(gap[0].last_open.micros(), gap_1003);
        assert!(gap[0].known_source_absence);
        let fold = FoldRunner::new(&tape);
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("obj.txt"), b"alpha").unwrap();
        let pin_path = dir.path().join("sweep-inputs.json");
        fs::write(
            &pin_path,
            "{\"schema\":\"sweep-runner-inputs-v2\",\"objects\":[{\"path\":\"obj.txt\",\"sha256\":\"8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8\"}]}\n",
        )
        .unwrap();
        let pins = SweepPins::load(&pin_path).unwrap();
        let mut runner = ConfigRunner {
            fold: &fold,
            family: row.family,
            params,
        };
        let costs = Costs {
            taker_fee: FeeBps(5),
            slippage: SlippageBps(2),
        };
        let rows = run_batch(
            &[spec],
            &pins,
            &dir.path().join("trials.jsonl"),
            &dir.path().join("results.jsonl"),
            &costs,
            &mut runner,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "finished");
        assert_eq!(rows[0].reason, None);
        let trials = fs::read_to_string(dir.path().join("trials.jsonl")).unwrap();
        assert!(trials.contains("\"kind\":\"RunFinished\""));
        assert!(!trials.contains("RunAborted"));
    }

    #[test]
    fn one_config_writes_a_finished_trial_and_a_digest() {
        let start = 1_704_067_200_000_000 + 3_600_000_000;
        let mut bars = Vec::new();
        for index in 0..240 {
            let usdt = if index < 120 { 50 } else { 100 };
            let open_us = start + index * 60_000_000;
            let price = Price::from_ticks(usdt * 100_000_000).unwrap();
            bars.push(Bar {
                open_time: UtcMicros::from_micros(open_us),
                close_time: UtcMicros::from_micros(open_us + 60_000_000 - 1_000),
                available: UtcMicros::from_micros(open_us + 60_000_000),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: Qty::from_ticks(100_000_000).unwrap(),
                count: 1,
            });
        }
        let tape = Tape::from_parts(bars.clone(), bars.clone(), Vec::new());
        let mut params = BTreeMap::new();
        params.insert("hold_h".to_owned(), 2);
        params.insert("n".to_owned(), 2);
        let fold = FoldRunner::new(&tape);
        let config = WindowConfig {
            family: "S02".to_owned(),
            params: params.clone(),
            initial_cash: 100_000_000_000_000,
        };
        let costs = Costs {
            taker_fee: FeeBps(5),
            slippage: SlippageBps(2),
        };
        let output = fold
            .run_config_window(
                &config,
                bars[0].open_time.micros(),
                bars.last().unwrap().open_time.micros() + 60_000_000,
                &costs,
            )
            .unwrap();
        assert!(!output.daily_returns.is_empty());
        assert_eq!(output.trial.n_refused_fills, 0);

        let mut spec = TrialSpec {
            run_id: "S02-00-a0".to_owned(),
            config_id: String::new(),
            family: "S02".to_owned(),
            params: params.clone(),
            attempt: 0,
            ts: UtcMicros::from_micros(start),
            run: RunConfig {
                initial_cash: Cash(100_000_000_000_000),
                warmup_start: bars[0].open_time,
                start: bars[0].open_time,
                end: UtcMicros::from_micros(bars.last().unwrap().open_time.micros() + 60_000_000),
                holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
                forced_flat: Vec::new(),
                flatten_before: None,
                mark_gaps: Vec::new(),
                interval: Interval::ONE_HOUR,
                lot_ticks: LOT_TICKS,
            },
        };
        spec.config_id = canonical_config_id(&spec);
        let config_id = spec.config_id.clone();
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("obj.txt"), b"alpha").unwrap();
        let pin_path = dir.path().join("sweep-inputs.json");
        fs::write(
            &pin_path,
            "{\"schema\":\"sweep-runner-inputs-v2\",\"objects\":[{\"path\":\"obj.txt\",\"sha256\":\"8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8\"}]}\n",
        )
        .unwrap();
        let pins = SweepPins::load(&pin_path).unwrap();
        let mut runner = ConfigRunner {
            fold: &fold,
            family: "S02".to_owned(),
            params,
        };
        let digest_path = dir.path().join("digest.jsonl");
        let rows = run_batch_digest(
            &[spec],
            &pins,
            &dir.path().join("trials.jsonl"),
            &digest_path,
            &costs,
            &mut runner,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "finished");
        assert_eq!(rows[0].slippage_bps, 2);
        assert_eq!(rows[0].n_refused_fills, Some(0));
        assert_eq!(rows[0].n_dropped_entries, Some(0));
        assert_eq!(rows[0].fee_grid_bps, [2, 4, 5, 6]);
        let trials = fs::read_to_string(dir.path().join("trials.jsonl")).unwrap();
        assert!(trials.contains("\"kind\":\"RunStarted\""));
        assert!(trials.contains("\"kind\":\"RunFinished\""));
        assert!(trials.contains("\"metrics_digest\""));
        assert_eq!(trials.lines().count(), 2);
        let digest = fs::read_to_string(&digest_path).unwrap();
        assert!(digest.contains(&config_id), "{digest}");
        assert!(digest.contains("\"metrics_digest\""), "{digest}");
        assert!(!digest.contains("total_return"), "{digest}");
        assert!(!digest.contains("sharpe"), "{digest}");
        assert!(!dir.path().join("results.jsonl").exists());
        assert!(!dir.path().join("stress.jsonl").exists());
    }

    #[test]
    fn a_hold_that_crosses_the_holdout_is_counted_on_the_trial() {
        let holdout = HOLDOUT_START_US;
        let hour = 3_600_000_000i64;
        let minute = 60_000_000i64;
        let start = holdout - 6 * hour;
        let prices = [100i64, 100, 120, 70];
        let mut bars = Vec::new();
        for index in 0..240 {
            let hour_index = usize::try_from(index / 60).unwrap();
            let open_us = start + index * minute;
            let price = Price::from_ticks(prices[hour_index] * 100_000_000).unwrap();
            bars.push(Bar {
                open_time: UtcMicros::from_micros(open_us),
                close_time: UtcMicros::from_micros(open_us + minute - 1_000),
                available: UtcMicros::from_micros(open_us + minute),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: Qty::from_ticks(100_000_000).unwrap(),
                count: 1,
            });
        }
        let tape = Tape::from_parts(bars.clone(), bars, Vec::new());
        let mut params = BTreeMap::new();
        params.insert("hold_h".to_owned(), 2);
        params.insert("n".to_owned(), 2);
        let mut spec = TrialSpec {
            run_id: "pending".to_owned(),
            config_id: String::new(),
            family: "S02".to_owned(),
            params: params.clone(),
            attempt: 0,
            ts: UtcMicros::from_micros(start),
            run: research_config(
                100_000_000_000_000,
                start,
                start,
                start + 240 * minute,
                BarInterval::H1,
                false,
            )
            .unwrap(),
        };
        spec.config_id = canonical_config_id(&spec);
        spec.run_id = "S02-00-a0".to_owned();
        let fold = FoldRunner::new(&tape);
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("obj.txt"), b"alpha").unwrap();
        let pin_path = dir.path().join("sweep-inputs.json");
        fs::write(
            &pin_path,
            "{\"schema\":\"sweep-runner-inputs-v2\",\"objects\":[{\"path\":\"obj.txt\",\"sha256\":\"8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8\"}]}\n",
        )
        .unwrap();
        let pins = SweepPins::load(&pin_path).unwrap();
        let mut runner = ConfigRunner {
            fold: &fold,
            family: "S02".to_owned(),
            params,
        };
        let costs = Costs {
            taker_fee: FeeBps(5),
            slippage: SlippageBps(2),
        };
        let results_path = dir.path().join("results.jsonl");
        let rows = run_batch(
            &[spec],
            &pins,
            &dir.path().join("trials.jsonl"),
            &results_path,
            &costs,
            &mut runner,
        )
        .unwrap();
        assert_eq!(rows[0].status, "finished");
        assert_eq!(rows[0].n_dropped_entries, Some(1));
        let results = fs::read_to_string(&results_path).unwrap();
        assert!(results.contains("\"n_dropped_entries\":1"), "{results}");
    }

    #[test]
    fn a_clipped_short_rebalances_once_after_a_twelve_percent_rally() {
        let start = 1_704_067_200_000_000i64;
        let minute = 60_000_000i64;
        // Hours 0-23 are 100. The short is signaled on the first 80 close and
        // filled on the next 80 open, then marked through 90 (+12.5%).
        let hours = 28usize;
        let mut bars = Vec::with_capacity(hours * 60);
        for index in 0..hours * 60 {
            let hour = index / 60;
            let usdt = if hour < 24 {
                100
            } else if hour < 26 {
                80
            } else {
                90
            };
            let open_us = start + i64::try_from(index).unwrap() * minute;
            let price = Price::from_ticks(usdt * 100_000_000).unwrap();
            bars.push(Bar {
                open_time: UtcMicros::from_micros(open_us),
                close_time: UtcMicros::from_micros(open_us + minute - 1_000),
                available: UtcMicros::from_micros(open_us + minute),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: Qty::from_ticks(100_000_000).unwrap(),
                count: 1,
            });
        }
        let mut funding = Vec::new();
        let last_open = bars.last().unwrap().open_time.micros();
        let mut slot = start;
        while slot <= last_open {
            funding.push(FundingStamp {
                funding_time: UtcMicros::from_micros(slot),
                available: UtcMicros::from_micros(slot + 60_000_000),
                rate: parangsae_core::Ratio::from_ticks(0),
                interval_hours: 8,
            });
            slot += 8 * 3_600_000_000;
        }
        let tape = Tape::from_parts(bars.clone(), bars.clone(), funding);
        let mut params = BTreeMap::new();
        params.insert("c".to_owned(), 100_000_000);
        params.insert("interval".to_owned(), 60);
        params.insert("n".to_owned(), 24);
        let fold = FoldRunner::new(&tape);
        let config = WindowConfig {
            family: "S01".to_owned(),
            params,
            initial_cash: 100_000_000_000_000,
        };
        let report = fold
            .run_once(
                &config,
                start,
                start + i64::try_from(hours).unwrap() * 3_600_000_000,
                &Costs {
                    taker_fee: FeeBps(0),
                    slippage: SlippageBps(0),
                },
            )
            .unwrap();
        let sells = report
            .fills
            .iter()
            .filter(|fill| fill.side == parangsae_core::Side::Sell)
            .count();
        let buys = report
            .fills
            .iter()
            .filter(|fill| fill.side == parangsae_core::Side::Buy)
            .count();
        assert_eq!(sells, 1, "{:?}", report.fills);
        assert_eq!(buys, 1, "{:?}", report.fills);
        assert_eq!(report.fills.len(), 2, "{:?}", report.fills);
        let sell = report
            .fills
            .iter()
            .find(|fill| fill.side == parangsae_core::Side::Sell)
            .unwrap();
        let buy = report
            .fills
            .iter()
            .find(|fill| fill.side == parangsae_core::Side::Buy)
            .unwrap();
        assert_eq!(sell.price.ticks(), 80 * 100_000_000);
        assert_eq!(buy.price.ticks(), 90 * 100_000_000);
        assert!(buy.ts.micros() > sell.ts.micros());
    }

    #[test]
    fn a_scored_window_sets_warmup_and_flattens_at_its_own_end() {
        let flat = research_config(1_000, 10, 20, 30, BarInterval::H1, true).unwrap();
        assert_eq!(flat.warmup_start.micros(), 10);
        assert_eq!(flat.start.micros(), 20);
        assert_eq!(flat.end.micros(), 30);
        assert_eq!(flat.flatten_before, Some(flat.end));
        let held = research_config(1_000, 20, 20, 30, BarInterval::H1, false).unwrap();
        assert!(held.flatten_before.is_none());
        assert_eq!(held.warmup_start, held.start);
    }

    /// S01 n=24 through [`FamilySignal`] and [`run_targets`].
    ///
    /// Hour 24 is still warmup and would enter. The first scored hour is flat
    /// on a fresh decision. A cold scored window has no ring, so it emits nothing.
    #[test]
    fn s01_warmup_changes_the_first_scored_output_and_does_not_open() {
        let start = UtcDate::parse("2024-06-01").unwrap().start_us().micros();
        let hour = 3_600_000_000i64;
        let hours = 29i64;
        let scored = start + 25 * hour;
        let end = start + hours * hour;
        let bars = hour_bars(
            start,
            hours,
            |hour_index| {
                if hour_index == 24 {
                    40_800
                } else {
                    40_000
                }
            },
        );
        let tape = tape_with_funding(bars);
        let mut params = BTreeMap::new();
        params.insert("c".to_owned(), 100_000_000);
        params.insert("interval".to_owned(), 60);
        params.insert("n".to_owned(), 24);

        let full = run_family(&tape, "S01", &params, start, start, end);
        assert!(
            full.signal_outputs.iter().any(|output| {
                output.decision.target_ticks() != 0 && output.bar_close_time_us.micros() < scored
            }),
            "full window should enter before the scored start"
        );
        assert!(
            full.fills.iter().any(|fill| fill.ts.micros() == scored),
            "the warmup entry fills at the scored open when the whole window is scored"
        );

        let warm = run_family(&tape, "S01", &params, start, scored, end);
        let cold = run_family(&tape, "S01", &params, scored, scored, end);
        assert!(
            !warm.signal_outputs.is_empty(),
            "warmup should reach the tsmom ring"
        );
        assert!(
            cold.signal_outputs.is_empty() && cold.fills.is_empty(),
            "cold scored window is the same four hours with an empty ring"
        );
        assert_ne!(
            warm.signal_outputs.len(),
            cold.signal_outputs.len(),
            "warm and cold first scored outputs differ"
        );
        assert_eq!(
            warm.signal_outputs[0].decision.target_ticks(),
            0,
            "a warmup entry must not be the first scored target"
        );
        assert!(warm
            .signal_outputs
            .iter()
            .all(|output| output.bar_close_time_us.micros() >= scored));
        assert!(
            warm.fills.is_empty(),
            "nothing opens at the scored start from warmup state: {:?}",
            warm.fills
        );
        assert!(warm
            .equity_curve
            .iter()
            .all(|(ts, _)| ts.micros() >= scored));
        assert!(warm.funding.iter().all(|event| event.ts.micros() >= scored));
    }

    /// S06 W=2 through [`FamilySignal`] and [`run_targets`].
    ///
    /// The last warmup block has a non-zero season signal. The first scored
    /// block is flat. Cold has no history, so it emits nothing.
    #[test]
    fn s06_warmup_changes_the_first_scored_output_and_does_not_open() {
        let start = UtcDate::parse("2024-06-01").unwrap().start_us().micros();
        let day = 86_400_000_000i64;
        let scored = start + 2 * day;
        let end = start + 3 * day;
        let bars = minute_bars(start, 3 * 1_440, s06_usdt);
        let tape = tape_with_funding(bars);
        let mut params = BTreeMap::new();
        params.insert("k".to_owned(), 100_000_000);
        params.insert("w_days".to_owned(), 2);

        let full = run_family(&tape, "S06", &params, start, start, end);
        assert!(
            full.signal_outputs.iter().any(|output| {
                output.decision.target_ticks() != 0 && output.bar_close_time_us.micros() < scored
            }),
            "full window should signal during warmup"
        );
        assert!(
            full.fills.iter().any(|fill| fill.ts.micros() == scored),
            "that warmup signal fills at the scored open when the whole window is scored"
        );

        let warm = run_family(&tape, "S06", &params, start, scored, end);
        let cold = run_family(&tape, "S06", &params, scored, scored, end);
        assert!(
            !warm.signal_outputs.is_empty(),
            "warmup should fill the season histogram"
        );
        assert!(
            cold.signal_outputs.is_empty() && cold.fills.is_empty(),
            "cold scored day has no block history"
        );
        assert_ne!(warm.signal_outputs.len(), cold.signal_outputs.len());
        assert_eq!(warm.signal_outputs[0].decision.target_ticks(), 0);
        assert!(warm
            .signal_outputs
            .iter()
            .all(|output| output.bar_close_time_us.micros() >= scored));
        assert!(
            warm.fills.is_empty(),
            "nothing opens at the scored start from warmup state: {:?}",
            warm.fills
        );
        assert!(warm
            .equity_curve
            .iter()
            .all(|(ts, _)| ts.micros() >= scored));
        assert!(warm.funding.iter().all(|event| event.ts.micros() >= scored));
    }

    /// S02 n=2 through [`FamilySignal`] and [`run_targets`].
    ///
    /// Hour 2 breaks the channel and would enter. Hour 3 is inside it, so the
    /// fresh decision is flat. A leaked hold would still be long. Cold has no
    /// channel, so it emits nothing.
    #[test]
    fn s02_warmup_changes_the_first_scored_output_and_does_not_open() {
        let start = UtcDate::parse("2024-06-01").unwrap().start_us().micros();
        let hour = 3_600_000_000i64;
        let hours = 5i64;
        let scored = start + 3 * hour;
        let end = start + hours * hour;
        let bars = hour_bars(
            start,
            hours,
            |hour_index| {
                if hour_index < 2 {
                    40_000
                } else {
                    48_000
                }
            },
        );
        let tape = tape_with_funding(bars);
        let mut params = BTreeMap::new();
        params.insert("n".to_owned(), 2);
        params.insert("hold_h".to_owned(), 12);
        assert_warmup_differs_and_stays_flat(&tape, "S02", &params, start, scored, end);
    }

    /// S03 θ=0.20, n=24 through [`FamilySignal`] and [`run_targets`].
    ///
    /// Hour 193 breaks a quiet channel and would enter. Hour 194 is back inside
    /// it, so the fresh decision is flat. A leaked hold would still be long.
    /// Cold has no vol rank, so it emits nothing.
    #[test]
    fn s03_warmup_changes_the_first_scored_output_and_does_not_open() {
        let start = UtcDate::parse("2024-06-01").unwrap().start_us().micros();
        let hour = 3_600_000_000i64;
        let hours = 195i64;
        let scored = start + 194 * hour;
        let end = start + hours * hour;
        let bars = hour_bars(start, hours, |hour_index| {
            if hour_index < 168 {
                if hour_index % 2 == 0 {
                    100
                } else {
                    200
                }
            } else if hour_index == 193 {
                200
            } else {
                150
            }
        });
        let tape = tape_with_funding(bars);
        let mut params = BTreeMap::new();
        params.insert("theta".to_owned(), 20_000_000);
        params.insert("n".to_owned(), 24);
        params.insert("hold_h".to_owned(), 12);
        assert_warmup_differs_and_stays_flat(&tape, "S03", &params, start, scored, end);
    }

    /// S05 L=12, k=2.5 through [`FamilySignal`] and [`run_targets`].
    ///
    /// Hour 720 is the quiet extension and would fade. Hour 721 gives back one
    /// step, so the fresh decision is flat. A leaked hold would still be short.
    /// Cold has no return history, so it emits nothing.
    #[test]
    fn s05_warmup_changes_the_first_scored_output_and_does_not_open() {
        let start = UtcDate::parse("2024-06-01").unwrap().start_us().micros();
        let hour = 3_600_000_000i64;
        let hours = 722i64;
        let scored = start + 721 * hour;
        let end = start + hours * hour;
        let bars = hour_bars(start, hours, s05_usdt);
        let tape = tape_with_funding(bars);
        let mut params = BTreeMap::new();
        params.insert("l".to_owned(), 12);
        params.insert("k".to_owned(), 250_000_000);
        params.insert("hold_h".to_owned(), 4);
        assert_warmup_differs_and_stays_flat(&tape, "S05", &params, start, scored, end);
    }

    fn assert_warmup_differs_and_stays_flat(
        tape: &Tape,
        family: &str,
        params: &BTreeMap<String, i64>,
        start: i64,
        scored: i64,
        end: i64,
    ) {
        let full = run_family(tape, family, params, start, start, end);
        assert!(
            full.signal_outputs.iter().any(|output| {
                output.decision.target_ticks() != 0 && output.bar_close_time_us.micros() < scored
            }),
            "{family} full window should signal during warmup"
        );
        assert!(
            full.fills.iter().any(|fill| fill.ts.micros() == scored),
            "{family} warmup signal fills at the scored open when the whole window is scored: {:?}",
            full.fills
        );

        let warm = run_family(tape, family, params, start, scored, end);
        let cold = run_family(tape, family, params, scored, scored, end);
        assert!(
            !warm.signal_outputs.is_empty(),
            "{family} warmup should change the first scored output"
        );
        assert!(
            cold.signal_outputs.is_empty() && cold.fills.is_empty(),
            "{family} cold scored window has no feature state"
        );
        assert_ne!(warm.signal_outputs.len(), cold.signal_outputs.len());
        assert_eq!(
            warm.signal_outputs[0].decision.target_ticks(),
            0,
            "{family} warmup state must not be the first scored target"
        );
        assert!(warm
            .signal_outputs
            .iter()
            .all(|output| output.bar_close_time_us.micros() >= scored));
        assert!(
            warm.fills.is_empty(),
            "{family} nothing opens at the scored start from warmup state: {:?}",
            warm.fills
        );
        assert!(warm
            .equity_curve
            .iter()
            .all(|(ts, _)| ts.micros() >= scored));
        assert!(warm.funding.iter().all(|event| event.ts.micros() >= scored));
    }

    fn run_family(
        tape: &Tape,
        family: &str,
        params: &BTreeMap<String, i64>,
        warmup: i64,
        start: i64,
        end: i64,
    ) -> Report {
        let mut signal = FamilySignal::new(family, params).unwrap();
        let cfg = research_config(
            1_000_000 * 100_000_000,
            warmup,
            start,
            end,
            signal.interval,
            true,
        )
        .unwrap();
        run_targets(tape, &mut signal, &selection_costs(), &cfg).unwrap()
    }

    fn hour_bars(start_us: i64, hours: i64, close_usdt: impl Fn(i64) -> i64) -> Vec<Bar> {
        let mut bars = Vec::with_capacity(usize::try_from(hours * 60).unwrap());
        for hour in 0..hours {
            let px = close_usdt(hour);
            for minute in 0..60 {
                let index = hour * 60 + minute;
                bars.push(flat_minute(start_us + index * 60_000_000, px));
            }
        }
        bars
    }

    fn minute_bars(start_us: i64, minutes: i64, usdt_at: impl Fn(i64) -> i64) -> Vec<Bar> {
        (0..minutes)
            .map(|minute| flat_minute(start_us + minute * 60_000_000, usdt_at(minute)))
            .collect()
    }

    fn s05_usdt(hour: i64) -> i64 {
        let mut price = 16i64;
        let last = hour.min(720);
        for step in 0..last {
            if step >= 708 {
                price *= 3;
            } else if step % 2 == 0 {
                price *= 4;
            } else {
                price /= 4;
            }
        }
        if hour > 720 {
            price /= 3;
        }
        price
    }

    fn s06_usdt(minute: i64) -> i64 {
        let day = minute / 1_440;
        let within = minute % 1_440;
        match day {
            0 if within < 239 => 40_000,
            0 => 48_000,
            1 if within < 239 => 48_000,
            1 => 50_000,
            _ => 50_000,
        }
    }

    fn flat_minute(open_us: i64, usdt: i64) -> Bar {
        let price = Price::from_ticks(usdt * 100_000_000).unwrap();
        let open_time = UtcMicros::from_micros(open_us);
        let close_time = UtcMicros::from_micros(open_us + 60_000_000 - 1_000);
        Bar {
            open_time,
            close_time,
            available: UtcMicros::from_micros(close_time.micros() + 1_000),
            open: price,
            high: price,
            low: price,
            close: price,
            volume: Qty::from_ticks(100_000_000).unwrap(),
            count: 1,
        }
    }

    fn tape_with_funding(bars: Vec<Bar>) -> Tape {
        let first = bars[0].open_time.micros();
        let last = bars.last().unwrap().open_time.micros();
        let step = 8 * 3_600_000_000;
        let mut funding = Vec::new();
        let mut ts = first;
        while ts <= last {
            let stamp = UtcMicros::from_micros(ts);
            funding.push(FundingStamp {
                funding_time: stamp,
                available: stamp,
                rate: Ratio::from_ticks(0),
                interval_hours: 8,
            });
            ts += step;
        }
        let marks = bars.clone();
        Tape::from_parts(bars, marks, funding)
    }
}

#[cfg(test)]
mod causal_impact_diagnostic {
    use super::*;
    #[test]
    fn real_data_before_after() {
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../research/sweep-inputs-v2.json");
        let families = crate::wfbatch::prepare_families(&input, 1).unwrap();
        let pins = SweepPins::load(&input).unwrap();
        let trigger = 1730149200000000;
        let mut first = trigger;
        for f in &families { for fold in &f.folds {
            for (warmup, segment) in [(fold.train_warmup, fold.train), (fold.test_warmup, fold.test)] {
                if segment.start.micros() <= trigger && segment.end.micros() > trigger { first = first.min(warmup.start.micros()); }
            }
        }}
        let last = 1730246400000000; // 2024-10-30 00:00 UTC.
        let tape = load_pinned_tape(Path::new("/workspace/data/parangsae"), &pins, first, last).unwrap();
        for family in &families { for (fi, fold) in family.folds.iter().enumerate() {
            for (segment_name, warmup, segment) in [("train", fold.train_warmup, fold.train), ("test", fold.test_warmup, fold.test)] {
                if segment.start.micros() > trigger || segment.end.micros() <= trigger { continue; }
                let end = last.min(segment.end.micros());
                for (index, point) in family.points.iter().enumerate() {
                    for (fee, slip) in [(2,2), (4,2), (5,2), (6,2), (5,5)] {
                        if segment_name == "train" && (fee != 5 || slip != 2) { continue; }
                        let costs = Costs { taker_fee: FeeBps(fee), slippage: SlippageBps(slip) };
                        let run = |before| {
                            if before { std::env::set_var("CAUSAL_BEFORE", "1"); } else { std::env::remove_var("CAUSAL_BEFORE"); }
                            let mut signal = FamilySignal::new(&point.config.family, &point.config.params).unwrap();
                            let cfg = research_config(point.config.initial_cash, warmup.start.micros(), segment.start.micros(), end, signal.interval, false).unwrap();
                            run_targets(&tape, &mut signal, &costs, &cfg).unwrap()
                        };
                        eprintln!("CASE {} fold={} {} c{} fee={} slip={}", family.family, fi+1, segment_name, index, fee, slip);
                        let before = run(true); let after = run(false);
                        println!("IMPACT {} fold={} {} c{} fee={} slip={} fills_changed={} equity_changed={} refused_before={} refused_after={}",
                            family.family, fi+1, segment_name, index, fee, slip, before.fills != after.fills,
                            before.equity_curve != after.equity_curve, before.n_refused_fills, after.n_refused_fills);
                    }
                }
            }
        }}
    }
}
