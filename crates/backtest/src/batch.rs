//! Multi-config batch. One trial per config, then the 2/4/5/6 bps net returns.
//!
//! `FeeGridRunner::run_targets` is the v0 adapter. It calls `run_fee_grid`.
//! engine-v1a's `run_targets` replaces that body; the log and the row stay.
//! Fee scenarios are not trials. Flat-bar counts come from the runner. The v0
//! report has none, so the adapter records the counts it was given (zero
//! unless a test or v1a supplies them).
//!
//! A window touches the holdout when `start >= 2025-10-01T00:00:00Z` or
//! `end` is past that instant, including a start on the holdout whose end is
//! at or before that instant. `end` is exclusive, so an end equal to the
//! holdout instant with an earlier start is the in-sample boundary.
//!
//! `config_id` is the lowercase sha256 of the canonical config. Anything else
//! is refused before a trial line is written.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use parangsae_core::{notional, FixedError, Ratio, UtcMicros, SCALE};
use parangsae_sim::{Cash, Costs, FeeBps, SimError, SlippageBps};
use serde::Deserialize;

use crate::canon::{json_string, sha256_file, sha256_hex};
use crate::engine::{run_fee_grid, RunConfig, FEE_GRID_BPS};
use crate::error::EngineError;
use crate::metrics::{div_half_even, level_returns};
use crate::report::Report;
use crate::trial_log::{append_fsync, trial_id, StartRecord, TrialError, TrialLog};
use crate::{Strategy, Tape};

/// 2025-10-01T00:00:00Z. Rows at or after this instant are the holdout.
pub const HOLDOUT_START_US: i64 = 1_759_276_800_000_000;

/// Bars whose decision was `Flat`, per reason. Both keys are always present.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlatBars {
    pub funding_stale: u64,
    pub funding_slot_missing: u64,
}

/// What one config's compute step reports. `net_return[i]` follows [`FEE_GRID_BPS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrialMetrics {
    pub total_return: Ratio,
    pub sharpe_annual_ticks: Option<i64>,
    pub max_drawdown: Ratio,
    pub turnover: Ratio,
    pub flat_bars: FlatBars,
    pub net_return: [Ratio; 4],
}

/// One config in a batch, plus the trial identity written before compute.
///
/// `params` is the grid point. Keys are ASCII and ordered by byte. Values are integers.
/// Non-integer values such as `c = 0.5` are stored as integer ticks at `1e8` scale, and the interval is minutes.
/// `config_id` must be [`canonical_config_id`] of this spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrialSpec {
    pub run_id: String,
    pub config_id: String,
    pub family: String,
    pub params: BTreeMap<String, i64>,
    pub attempt: u64,
    pub ts: UtcMicros,
    pub run: RunConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultRow {
    pub run_id: String,
    pub trial_id: String,
    pub config_id: String,
    pub status: String,
    pub reason: Option<String>,
    pub total_return_ticks: Option<i64>,
    pub sharpe_annual_ticks: Option<i64>,
    pub max_drawdown_ticks: Option<i64>,
    pub turnover_ticks: Option<i64>,
    pub flat_bars: Option<FlatBars>,
    pub net_return_ticks: Option<[i64; 4]>,
    pub slippage_bps: u32,
    pub fee_grid_bps: [u32; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchError {
    Append(String),
    DuplicateRunId(String),
    Holdout {
        run_id: String,
        start_us: i64,
        end_us: i64,
    },
    BadWindow {
        run_id: String,
    },
    Pins(String),
    PinMismatch {
        path: String,
        expected: String,
        got: String,
    },
    ConfigId {
        run_id: String,
    },
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Append(msg) => write!(f, "{msg}"),
            Self::DuplicateRunId(id) => write!(f, "duplicate run_id {id}"),
            Self::Holdout {
                run_id,
                start_us,
                end_us,
            } => write!(f, "holdout window for {run_id}: {start_us}..{end_us}"),
            Self::BadWindow { run_id } => write!(f, "window end is before start for {run_id}"),
            Self::Pins(msg) => write!(f, "sweep inputs: {msg}"),
            Self::PinMismatch {
                path,
                expected,
                got,
            } => write!(f, "pin sha256 mismatch for {path}: {got} != {expected}"),
            Self::ConfigId { run_id } => {
                write!(
                    f,
                    "config_id is not sha256 of the canonical config for {run_id}"
                )
            }
        }
    }
}

impl std::error::Error for BatchError {}

/// Pinned sweep inputs. `pinned_root` is sha256 of the JCS list of `(path, sha256)` pairs, sorted by path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweepPins {
    pinned_root: String,
}

impl SweepPins {
    pub fn pinned_root(&self) -> &str {
        &self.pinned_root
    }

    pub fn load(path: &Path) -> Result<Self, BatchError> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| BatchError::Pins(format!("{}: {err}", path.display())))?;
        let file: PinFile = serde_json::from_str(&text)
            .map_err(|err| BatchError::Pins(format!("sweep inputs json: {err}")))?;
        if file.schema != "sweep-runner-inputs-v2" {
            return Err(BatchError::Pins(format!(
                "schema {} is not sweep-runner-inputs-v2",
                file.schema
            )));
        }
        let base = path.parent().unwrap_or(Path::new("."));
        let mut objects = Vec::with_capacity(file.objects.len());
        for object in file.objects {
            if object.path.is_empty() || !is_lower_hex64(&object.sha256) {
                return Err(BatchError::Pins(
                    "object path or sha256 is not usable".to_owned(),
                ));
            }
            let full = if Path::new(&object.path).is_absolute() {
                Path::new(&object.path).to_path_buf()
            } else {
                base.join(&object.path)
            };
            let got = sha256_file(&full)
                .map_err(|err| BatchError::Pins(format!("{}: {err}", full.display())))?;
            if got != object.sha256 {
                return Err(BatchError::PinMismatch {
                    path: object.path,
                    expected: object.sha256,
                    got,
                });
            }
            objects.push((object.path, object.sha256));
        }
        objects.sort_by(|left, right| left.0.cmp(&right.0));
        if objects.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(BatchError::Pins("duplicate pin path".to_owned()));
        }
        let pinned_root = sha256_hex(pin_preimage(&objects).as_bytes());
        Ok(Self { pinned_root })
    }
}

#[derive(Deserialize)]
struct PinFile {
    schema: String,
    objects: Vec<PinObject>,
}

#[derive(Deserialize)]
struct PinObject {
    path: String,
    sha256: String,
}

fn is_lower_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn pin_preimage(objects: &[(String, String)]) -> String {
    let mut body = String::from("[");
    for (index, (path, sha)) in objects.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push('[');
        body.push_str(&json_string(path));
        body.push(',');
        body.push_str(&json_string(sha));
        body.push(']');
    }
    body.push(']');
    body
}

/// The engine step the batch calls once per config.
///
/// The method name is the spec's `run_targets`. `FeeGridRunner` fills it
/// until engine-v1a provides the real one.
pub trait TargetRunner {
    type Error: std::fmt::Display;

    fn run_targets(&mut self, cfg: &RunConfig) -> Result<TrialMetrics, Self::Error>;

    /// Slippage recorded on every results row. Fee scenarios are not trials.
    fn slippage_bps(&self) -> u32 {
        0
    }
}

/// v0 adapter: four fee replays, one set of metrics, headline at 5 bps.
pub struct FeeGridRunner<'a, S, F> {
    tape: &'a Tape,
    slippage: SlippageBps,
    make: F,
    flat_bars: FlatBars,
    _strategy: PhantomData<fn() -> S>,
}

impl<'a, S, F> FeeGridRunner<'a, S, F>
where
    S: Strategy,
    F: FnMut() -> S,
{
    pub fn new(tape: &'a Tape, slippage: SlippageBps, make: F) -> Self {
        Self {
            tape,
            slippage,
            make,
            flat_bars: FlatBars::default(),
            _strategy: PhantomData,
        }
    }

    pub fn with_flat_bars(mut self, flat_bars: FlatBars) -> Self {
        self.flat_bars = flat_bars;
        self
    }
}

impl<'a, S, F> TargetRunner for FeeGridRunner<'a, S, F>
where
    S: Strategy,
    F: FnMut() -> S,
{
    type Error = EngineError;

    fn slippage_bps(&self) -> u32 {
        self.slippage.0
    }

    fn run_targets(&mut self, cfg: &RunConfig) -> Result<TrialMetrics, EngineError> {
        let costs = Costs {
            taker_fee: FeeBps(FEE_GRID_BPS[2]),
            slippage: self.slippage,
        };
        let grid = run_fee_grid(self.tape, &mut self.make, &costs, cfg)?;
        metrics_from_grid(cfg.initial_cash, &grid, self.flat_bars).map_err(EngineError::Sim)
    }
}

pub fn metrics_from_grid(
    initial: Cash,
    grid: &[(FeeBps, Report)],
    flat_bars: FlatBars,
) -> Result<TrialMetrics, SimError> {
    if grid.len() != FEE_GRID_BPS.len() {
        return Err(SimError::Overflow);
    }
    for (index, bps) in FEE_GRID_BPS.iter().enumerate() {
        if grid[index].0 .0 != *bps {
            return Err(SimError::Overflow);
        }
    }
    let headline = &grid[2].1;
    let mut net_return = [Ratio::from_ticks(0); 4];
    for (index, (_, report)) in grid.iter().enumerate() {
        net_return[index] = report.metrics.total_return;
    }
    let daily: Vec<i64> = headline
        .daily_equity
        .iter()
        .map(|(_, cash)| cash.0)
        .collect();
    let mut levels = Vec::with_capacity(daily.len() + 1);
    levels.push(initial.0);
    levels.extend_from_slice(&daily);
    Ok(TrialMetrics {
        total_return: headline.metrics.total_return,
        sharpe_annual_ticks: annual_sharpe_ticks(&levels)?,
        max_drawdown: headline.metrics.max_drawdown,
        turnover: turnover_ratio(sum_notional(headline)?, &daily)?,
        flat_bars,
        net_return,
    })
}

fn sum_notional(report: &Report) -> Result<i128, SimError> {
    let mut sum = 0i128;
    for fill in &report.fills {
        let notion = match notional(fill.price, fill.qty) {
            Ok(value) => value,
            Err(FixedError::Negative) => return Err(SimError::Negative),
            Err(FixedError::Overflow) => return Err(SimError::Overflow),
        };
        sum = sum
            .checked_add(i128::from(notion.ticks()))
            .ok_or(SimError::Overflow)?;
    }
    Ok(sum)
}

/// Annualised Sharpe in `1e8` ticks.
///
/// Daily returns are consecutive level changes at scale `1e18`, then rescaled
/// to `1e8`. The sample standard deviation uses `n - 1`. Ticks are
/// `sign(sum) * isqrt(floor(sum^2 * (n - 1) * 365 * 1e16 / sum_sq))`, toward
/// zero. The `1e16` factor is applied as two `1e8` steps. Fewer than two
/// returns, or a zero sum of squares, is `None`.
pub fn annual_sharpe_ticks(levels_ticks: &[i64]) -> Result<Option<i64>, SimError> {
    let returns = level_returns(levels_ticks)?;
    if returns.len() < 2 {
        return Ok(None);
    }
    let mut scaled = Vec::with_capacity(returns.len());
    for value in returns {
        scaled.push(div_half_even(value, 10_000_000_000)?);
    }
    let n = i128::try_from(scaled.len()).map_err(|_| SimError::Overflow)?;
    let mut sum = 0i128;
    for value in &scaled {
        sum = sum.checked_add(*value).ok_or(SimError::Overflow)?;
    }
    let mut squares = 0i128;
    for value in &scaled {
        let centered = n
            .checked_mul(*value)
            .ok_or(SimError::Overflow)?
            .checked_sub(sum)
            .ok_or(SimError::Overflow)?;
        squares = squares
            .checked_add(centered.checked_mul(centered).ok_or(SimError::Overflow)?)
            .ok_or(SimError::Overflow)?;
    }
    if squares == 0 {
        return Ok(None);
    }
    let periods = (n - 1).checked_mul(365).ok_or(SimError::Overflow)?;
    if periods <= 0 {
        return Err(SimError::Overflow);
    }
    let inside = sharpe_ratio_floor(sum, periods, squares)?;
    let root = isqrt(inside).ok_or(SimError::Overflow)?;
    let magnitude = i128::try_from(root).map_err(|_| SimError::Overflow)?;
    let ticks = if sum < 0 { -magnitude } else { magnitude };
    Ok(Some(i64::try_from(ticks).map_err(|_| SimError::Overflow)?))
}

/// `floor(sum^2 * periods * 1e16 / squares)` with the scale applied in two `1e8` steps.
fn sharpe_ratio_floor(sum: i128, periods: i128, squares: i128) -> Result<u128, SimError> {
    let scale = 100_000_000u128;
    let sum2 = sum
        .unsigned_abs()
        .checked_mul(sum.unsigned_abs())
        .ok_or(SimError::Overflow)?;
    let periods = u128::try_from(periods).map_err(|_| SimError::Overflow)?;
    let squares = u128::try_from(squares).map_err(|_| SimError::Overflow)?;
    let base = sum2.checked_mul(periods).ok_or(SimError::Overflow)?;
    let (quot, rem) = (base / squares, base % squares);
    let rem_scaled = rem.checked_mul(scale).ok_or(SimError::Overflow)?;
    let once = quot
        .checked_mul(scale)
        .ok_or(SimError::Overflow)?
        .checked_add(rem_scaled / squares)
        .ok_or(SimError::Overflow)?;
    let rem = rem_scaled % squares;
    let rem_scaled = rem.checked_mul(scale).ok_or(SimError::Overflow)?;
    once.checked_mul(scale)
        .ok_or(SimError::Overflow)?
        .checked_add(rem_scaled / squares)
        .ok_or(SimError::Overflow)
}

/// `sum |fill notional| * 365 / sum(daily equity)`, in `1e8` ticks.
///
/// That is `sum |notional| / mean daily equity / (days/365)`.
pub fn turnover_ratio(
    sum_abs_notional_ticks: i128,
    daily_equity_ticks: &[i64],
) -> Result<Ratio, SimError> {
    if sum_abs_notional_ticks < 0 {
        return Err(SimError::Negative);
    }
    if daily_equity_ticks.is_empty() {
        if sum_abs_notional_ticks == 0 {
            return Ok(Ratio::from_ticks(0));
        }
        return Err(SimError::Negative);
    }
    let mut sum_equity = 0i128;
    for value in daily_equity_ticks {
        sum_equity = sum_equity
            .checked_add(i128::from(*value))
            .ok_or(SimError::Overflow)?;
    }
    if sum_equity <= 0 {
        return Err(SimError::Negative);
    }
    let num = sum_abs_notional_ticks
        .checked_mul(365)
        .ok_or(SimError::Overflow)?
        .checked_mul(SCALE)
        .ok_or(SimError::Overflow)?;
    let ticks = div_half_even(num, sum_equity)?;
    Ok(Ratio::from_ticks(
        i64::try_from(ticks).map_err(|_| SimError::Overflow)?,
    ))
}

fn isqrt(n: u128) -> Option<u128> {
    if n == 0 {
        return Some(0);
    }
    let mut x0 = n;
    let mut x1 = n / 2 + n % 2;
    while x1 < x0 {
        x0 = x1;
        let sum = x0.checked_add(n / x0)?;
        x1 = sum / 2;
    }
    Some(x0)
}

/// Canonical metrics body hashed into `RunFinished.metrics_digest`.
pub fn metrics_preimage(metrics: &TrialMetrics) -> String {
    let sharpe = match metrics.sharpe_annual_ticks {
        Some(value) => value.to_string(),
        None => "null".to_owned(),
    };
    format!(
        "{{\"flat_funding_slot_missing\":{},\"flat_funding_stale\":{},\"max_drawdown_ticks\":{},\"net_2\":{},\"net_4\":{},\"net_5\":{},\"net_6\":{},\"sharpe_annual_ticks\":{sharpe},\"total_return_ticks\":{},\"turnover_ticks\":{}}}",
        metrics.flat_bars.funding_slot_missing,
        metrics.flat_bars.funding_stale,
        metrics.max_drawdown.ticks(),
        metrics.net_return[0].ticks(),
        metrics.net_return[1].ticks(),
        metrics.net_return[2].ticks(),
        metrics.net_return[3].ticks(),
        metrics.total_return.ticks(),
        metrics.turnover.ticks()
    )
}

pub fn metrics_digest(metrics: &TrialMetrics) -> String {
    sha256_hex(metrics_preimage(metrics).as_bytes())
}

/// sha256 of the JCS object `{end_us, family, initial_cash, params, start_us}`.
///
/// `attempt`, `run_id`, `ts`, and `pinned_root` are not part of the config.
/// `params` values are JSON integers. Keys are written in byte order.
pub fn canonical_config_id(spec: &TrialSpec) -> String {
    sha256_hex(config_preimage(spec).as_bytes())
}

fn config_preimage(spec: &TrialSpec) -> String {
    let mut params = String::from("{");
    for (index, (key, value)) in spec.params.iter().enumerate() {
        if index > 0 {
            params.push(',');
        }
        params.push_str(&json_string(key));
        params.push(':');
        params.push_str(&value.to_string());
    }
    params.push('}');
    format!(
        "{{\"end_us\":{},\"family\":{},\"initial_cash\":{},\"params\":{params},\"start_us\":{}}}",
        spec.run.end.micros(),
        json_string(&spec.family),
        spec.run.initial_cash.0,
        spec.run.start.micros(),
    )
}

/// Log every config, then compute the ones whose window stays before the holdout.
///
/// `config_id` must equal [`canonical_config_id`]. Any other value, including a
/// family label, is refused before the log is touched and before compute.
/// `RunStarted` is fsynced before `run_targets`. An engine error aborts that
/// trial and the batch continues. A holdout window is an aborted trial and
/// does not call `run_targets`. Later configs are still logged. The call then
/// returns [`BatchError::Holdout`]. A failed append returns an error and does
/// not compute that config.
pub fn run_batch<R: TargetRunner>(
    specs: &[TrialSpec],
    pins: &SweepPins,
    trials_path: &Path,
    results_path: &Path,
    runner: &mut R,
) -> Result<Vec<ResultRow>, BatchError> {
    let log = TrialLog::new(trials_path);
    let mut rows = Vec::with_capacity(specs.len());
    for spec in specs {
        rows.push(drive(spec, pins, &log, results_path, runner)?);
    }
    for spec in specs {
        if let Some(reason) = gate(spec) {
            return Err(gate_error(spec, reason));
        }
    }
    Ok(rows)
}

fn drive<R: TargetRunner>(
    spec: &TrialSpec,
    pins: &SweepPins,
    log: &TrialLog,
    results_path: &Path,
    runner: &mut R,
) -> Result<ResultRow, BatchError> {
    if spec.config_id != canonical_config_id(spec) {
        return Err(BatchError::ConfigId {
            run_id: spec.run_id.clone(),
        });
    }
    let slippage_bps = runner.slippage_bps();
    let id = trial_id(&spec.config_id, pins.pinned_root(), spec.attempt);
    log.begin(StartRecord {
        run_id: &spec.run_id,
        trial_id: &id,
        config_id: &spec.config_id,
        pinned_root: pins.pinned_root(),
        attempt: spec.attempt,
        family: &spec.family,
        ts: spec.ts.micros(),
    })
    .map_err(map_trial)?;
    if let Some(reason) = gate(spec) {
        log.abort(&spec.run_id, reason).map_err(map_trial)?;
        let row = aborted_row(spec, &id, reason, slippage_bps);
        write_row(results_path, spec, &row, pins.pinned_root(), None)?;
        return Ok(row);
    }
    let computed = panic::catch_unwind(AssertUnwindSafe(|| runner.run_targets(&spec.run)));
    match computed {
        Ok(Ok(metrics)) => {
            let digest = metrics_digest(&metrics);
            log.finish(&spec.run_id, &digest).map_err(map_trial)?;
            let row = ResultRow {
                run_id: spec.run_id.clone(),
                trial_id: id,
                config_id: spec.config_id.clone(),
                status: "finished".to_owned(),
                reason: None,
                total_return_ticks: Some(metrics.total_return.ticks()),
                sharpe_annual_ticks: metrics.sharpe_annual_ticks,
                max_drawdown_ticks: Some(metrics.max_drawdown.ticks()),
                turnover_ticks: Some(metrics.turnover.ticks()),
                flat_bars: Some(metrics.flat_bars),
                net_return_ticks: Some(metrics.net_return.map(Ratio::ticks)),
                slippage_bps,
                fee_grid_bps: FEE_GRID_BPS,
            };
            write_row(
                results_path,
                spec,
                &row,
                pins.pinned_root(),
                Some(digest.as_str()),
            )?;
            Ok(row)
        }
        Ok(Err(err)) => {
            let reason = err.to_string();
            log.abort(&spec.run_id, &reason).map_err(map_trial)?;
            let row = aborted_row(spec, &id, &reason, slippage_bps);
            write_row(results_path, spec, &row, pins.pinned_root(), None)?;
            Ok(row)
        }
        Err(payload) => {
            let _ = log.abort(&spec.run_id, "panic");
            let row = aborted_row(spec, &id, "panic", slippage_bps);
            let _ = write_row(results_path, spec, &row, pins.pinned_root(), None);
            panic::resume_unwind(payload);
        }
    }
}

fn gate(spec: &TrialSpec) -> Option<&'static str> {
    let start = spec.run.start.micros();
    let end = spec.run.end.micros();
    if start >= HOLDOUT_START_US || end > HOLDOUT_START_US {
        return Some("holdout");
    }
    if end < start {
        return Some("bad window");
    }
    None
}

fn gate_error(spec: &TrialSpec, reason: &str) -> BatchError {
    match reason {
        "holdout" => BatchError::Holdout {
            run_id: spec.run_id.clone(),
            start_us: spec.run.start.micros(),
            end_us: spec.run.end.micros(),
        },
        _ => BatchError::BadWindow {
            run_id: spec.run_id.clone(),
        },
    }
}

fn aborted_row(spec: &TrialSpec, trial_id: &str, reason: &str, slippage_bps: u32) -> ResultRow {
    ResultRow {
        run_id: spec.run_id.clone(),
        trial_id: trial_id.to_owned(),
        config_id: spec.config_id.clone(),
        status: "aborted".to_owned(),
        reason: Some(reason.to_owned()),
        total_return_ticks: None,
        sharpe_annual_ticks: None,
        max_drawdown_ticks: None,
        turnover_ticks: None,
        flat_bars: None,
        net_return_ticks: None,
        slippage_bps,
        fee_grid_bps: FEE_GRID_BPS,
    }
}

fn map_trial(err: TrialError) -> BatchError {
    match err {
        TrialError::DuplicateRunId(id) => BatchError::DuplicateRunId(id),
        other => BatchError::Append(other.to_string()),
    }
}

#[derive(serde::Serialize)]
struct RowJson<'a> {
    run_id: &'a str,
    trial_id: &'a str,
    config_id: &'a str,
    family: &'a str,
    attempt: u64,
    pinned_root: &'a str,
    status: &'a str,
    reason: Option<&'a str>,
    total_return_ticks: Option<i64>,
    sharpe_annual_ticks: Option<i64>,
    max_drawdown_ticks: Option<i64>,
    turnover_ticks: Option<i64>,
    flat_bars: Option<FlatJson>,
    net_return_ticks: Option<NetJson>,
    slippage_bps: u32,
    fee_grid_bps: &'a [u32],
    metrics_digest: Option<&'a str>,
}

#[derive(serde::Serialize)]
struct FlatJson {
    #[serde(rename = "FundingStale")]
    funding_stale: u64,
    #[serde(rename = "FundingSlotMissing")]
    funding_slot_missing: u64,
}

#[derive(serde::Serialize)]
struct NetJson {
    #[serde(rename = "2")]
    bps_2: i64,
    #[serde(rename = "4")]
    bps_4: i64,
    #[serde(rename = "5")]
    bps_5: i64,
    #[serde(rename = "6")]
    bps_6: i64,
}

fn write_row(
    path: &Path,
    spec: &TrialSpec,
    row: &ResultRow,
    pinned_root: &str,
    metrics_digest: Option<&str>,
) -> Result<(), BatchError> {
    let flat = row.flat_bars.map(|bars| FlatJson {
        funding_stale: bars.funding_stale,
        funding_slot_missing: bars.funding_slot_missing,
    });
    let nets = row.net_return_ticks.map(|ticks| NetJson {
        bps_2: ticks[0],
        bps_4: ticks[1],
        bps_5: ticks[2],
        bps_6: ticks[3],
    });
    let json = RowJson {
        run_id: &row.run_id,
        trial_id: &row.trial_id,
        config_id: &row.config_id,
        family: &spec.family,
        attempt: spec.attempt,
        pinned_root,
        status: &row.status,
        reason: row.reason.as_deref(),
        total_return_ticks: row.total_return_ticks,
        sharpe_annual_ticks: row.sharpe_annual_ticks,
        max_drawdown_ticks: row.max_drawdown_ticks,
        turnover_ticks: row.turnover_ticks,
        flat_bars: flat,
        net_return_ticks: nets,
        slippage_bps: row.slippage_bps,
        fee_grid_bps: &row.fee_grid_bps,
        metrics_digest,
    };
    let line = serde_json::to_string(&json).map_err(|err| BatchError::Append(err.to_string()))?;
    append_fsync(path, &line).map_err(map_trial)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn one_parameter_changes_the_canonical_config_and_the_trial_id() {
        let mut left = TrialSpec {
            run_id: "r".to_owned(),
            config_id: String::new(),
            family: "S01".to_owned(),
            params: BTreeMap::new(),
            attempt: 0,
            ts: UtcMicros::from_micros(1),
            run: RunConfig {
                initial_cash: Cash(100),
                start: UtcMicros::from_micros(1),
                end: UtcMicros::from_micros(2),
            },
        };
        left.params.insert("n".to_owned(), 24);
        assert_eq!(
            config_preimage(&left),
            "{\"end_us\":2,\"family\":\"S01\",\"initial_cash\":100,\"params\":{\"n\":24},\"start_us\":1}"
        );
        let mut right = left.clone();
        right.params.insert("n".to_owned(), 72);
        let left_id = canonical_config_id(&left);
        let right_id = canonical_config_id(&right);
        assert_ne!(left_id, right_id);
        assert_ne!(trial_id(&left_id, "pin", 0), trial_id(&right_id, "pin", 0));
    }

    #[test]
    fn sharpe_of_100_200_300_is_the_integer_golden() {
        let ticks = annual_sharpe_ticks(&[100, 200, 300]).unwrap();
        assert_eq!(ticks, Some(4_052_776_825));
    }

    #[test]
    fn sharpe_at_n_1350_with_large_returns_does_not_overflow() {
        let mut levels = Vec::with_capacity(1351);
        for i in 0..=1350 {
            levels.push(if i % 2 == 0 { 1_000_000 } else { 2_000_000 });
        }
        let ticks = annual_sharpe_ticks(&levels).unwrap();
        assert!(ticks.is_some());
    }

    #[test]
    fn constant_levels_have_no_sharpe() {
        assert_eq!(annual_sharpe_ticks(&[10, 10, 10, 10]).unwrap(), None);
    }

    #[test]
    fn turnover_round_trip_over_365_days_is_point_two() {
        let notional = 2 * 100 * SCALE;
        let equity = 1000 * 100_000_000_i64;
        let daily = vec![equity; 365];
        assert_eq!(
            turnover_ratio(notional, &daily).unwrap().ticks(),
            20_000_000
        );
    }

    #[test]
    fn metrics_digest_locks_the_canonical_body() {
        let metrics = TrialMetrics {
            total_return: Ratio::from_ticks(3),
            sharpe_annual_ticks: Some(5),
            max_drawdown: Ratio::from_ticks(0),
            turnover: Ratio::from_ticks(6),
            flat_bars: FlatBars {
                funding_stale: 3,
                funding_slot_missing: 2,
            },
            net_return: [
                Ratio::from_ticks(1),
                Ratio::from_ticks(2),
                Ratio::from_ticks(3),
                Ratio::from_ticks(4),
            ],
        };
        assert_eq!(
            metrics_preimage(&metrics),
            "{\"flat_funding_slot_missing\":2,\"flat_funding_stale\":3,\"max_drawdown_ticks\":0,\"net_2\":1,\"net_4\":2,\"net_5\":3,\"net_6\":4,\"sharpe_annual_ticks\":5,\"total_return_ticks\":3,\"turnover_ticks\":6}"
        );
        assert_eq!(
            metrics_digest(&metrics),
            "82eee788c3d38f931950de091bc8d3975abe2deee4c341c8827c79710e794048"
        );
    }

    proptest! {
        #[test]
        fn constant_equity_sharpe_is_absent(level in 1i64..1_000_000_000, days in 3usize..12) {
            let levels = vec![level; days];
            prop_assert_eq!(annual_sharpe_ticks(&levels).unwrap(), None);
        }

        #[test]
        fn zero_notional_turnover_is_zero(equity in 1i64..1_000_000_000, days in 1usize..8) {
            let daily = vec![equity; days];
            prop_assert_eq!(turnover_ratio(0, &daily).unwrap().ticks(), 0);
        }
    }
}
