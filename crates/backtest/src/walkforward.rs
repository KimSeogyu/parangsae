//! Walk-forward folds for one family grid.
//!
//! [`fold_schedule`] is a pure date function. [`run_walk_forward`] calls the
//! strategy closure once per config per fold, on the train window and again on
//! the test window, then replays the selected test windows on the fee grid.
//! It does not run a full-sample window.
//!
//! The closure is `FnMut(&Config, warmup_start_us, start_us, end_us, &Costs)`.
//! Bars from `warmup_start_us` are fed to the signal. There is no decision,
//! fill, funding settlement, or equity point before `start_us`.
//!
//! Forced flat fills at the last scored minute's open, not at that minute's
//! mark close. [`forced_flat_at`] is `[end - 1 minute, end)`. The engine
//! flattens on the first non-filler open in that minute and pays `costs`.
//!
//! Train selection uses only that fold's train annual Sharpe from
//! [`sharpe_ticks`] at the costs from [`selection_costs`] (5 bps fee, 2 bps
//! slippage). The closure settles funding. Ties, including two undefined
//! Sharpes, keep the earlier grid point.
//!
//! `config_id` is [`canonical_config_id`] of the grid point: family, params,
//! initial cash, and the family's outer window (first warmup start through the
//! last test end). The fold and the segment are fields on `RunStarted`, so one
//! config is one trial across every fold. S01's 12 configs stay 12 trials.
//!
//! Stitched OOS is the selected config's test equity, concatenated. Each fold
//! starts from its own opening equity. Daily returns are
//! `trunc((next - prev) * 1e8 / prev)`. The fee grid reruns that stitched test
//! path at 2/4/5/6 bps with 2 bps slippage, plus a 5 bps fee and 5 bps
//! slippage stress row. Those reruns are not trials.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use parangsae_core::{Interval, Ratio, UtcMicros, SCALE};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};

use crate::bar::ONE_MINUTE_US;
use crate::batch::{canonical_config_id, SweepPins, TrialSpec};
use crate::canon::sha256_hex;
use crate::date::UtcDate;
use crate::engine::{RunConfig, UtcRange, FEE_GRID_BPS};
use crate::metrics::{sharpe_of_return_ticks, sharpe_ticks};
use crate::report::Report;
use crate::trial_log::{trial_id, StartRecord, TrialError, TrialLog};

/// Train length of the BTCUSDT walk-forward, in UTC days.
pub const TRAIN_DAYS: i64 = 365;
/// Gap between a train end and the following test start, in UTC days.
pub const PURGE_DAYS: i64 = 2;
/// Nominal test length, in UTC days.
pub const TEST_DAYS: i64 = 90;
/// Distance from one test start to the next, in UTC days.
pub const STEP_DAYS: i64 = 90;
/// A remainder of at least this many days is its own fold. A shorter remainder
/// is appended to the previous test.
pub const REMAINDER_OWN_FOLD_DAYS: i64 = 45;
/// Feature warmup for families whose longest lookback is 30 days.
pub const WARMUP_DAYS: i64 = 30;
/// Feature warmup for the session family (S06).
pub const SESSION_WARMUP_DAYS: i64 = 365;
/// Headline taker fee used to pick a config. Fee-grid replays are not trials.
pub const SELECTION_FEE_BPS: u32 = 5;
/// Slippage, in bps, on every selection fill.
pub const SELECTION_SLIPPAGE_BPS: u32 = 2;
/// Slippage, in bps, on the stress replay of the selected path. Not a trial.
pub const STRESS_SLIPPAGE_BPS: u32 = 5;

const DAY_US: i64 = 86_400_000_000;

/// Costs the selector is allowed to see. There is no default of zero.
pub fn selection_costs() -> Costs {
    Costs {
        taker_fee: FeeBps(SELECTION_FEE_BPS),
        slippage: SlippageBps(SELECTION_SLIPPAGE_BPS),
    }
}

/// Inclusive start, exclusive end, on UTC midnights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleSpec {
    pub sample_start: UtcDate,
    pub holdout_start: UtcDate,
    pub warmup_days: i64,
    pub train_days: i64,
    pub purge_days: i64,
    pub test_days: i64,
    pub step_days: i64,
    pub remainder_own_fold_days: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// The named day count is outside the range the schedule accepts.
    NonPositive(&'static str),
    /// `step_days` and `test_days` differ, so OOS would gap or overlap.
    StepMismatch,
    /// The first test starts at or after the holdout.
    NoFold,
    Overflow,
    /// A fold left `[sample_start, holdout_start)`.
    OutOfRange,
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonPositive(name) => write!(f, "{name} is outside its range"),
            Self::StepMismatch => write!(f, "step_days must equal test_days"),
            Self::NoFold => write!(f, "no test window fits before the holdout"),
            Self::Overflow => write!(f, "schedule day count overflowed"),
            Self::OutOfRange => write!(f, "a fold leaves the sample"),
        }
    }
}

impl std::error::Error for ScheduleError {}

/// Which scored window a fold run covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Train,
    Test,
}

impl std::fmt::Display for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Train => write!(f, "train"),
            Self::Test => write!(f, "test"),
        }
    }
}

impl Segment {
    fn as_str(self) -> &'static str {
        match self {
            Self::Train => "train",
            Self::Test => "test",
        }
    }
}

/// One walk-forward fold. Ranges are inclusive at `start` and exclusive at `end`.
///
/// `train_warmup` and `test_warmup` are feature lookback only. The runner scores
/// `train` and `test`. `train_flat` and `test_flat` are the last minute of those
/// scored windows: the closure flattens there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    /// 1-based index in schedule order.
    pub index: u32,
    pub train_warmup: UtcRange,
    pub train: UtcRange,
    pub purge: UtcRange,
    pub test_warmup: UtcRange,
    pub test: UtcRange,
    pub train_flat: UtcRange,
    pub test_flat: UtcRange,
}

impl Fold {
    pub fn scored(self, segment: Segment) -> UtcRange {
        match segment {
            Segment::Train => self.train,
            Segment::Test => self.test,
        }
    }

    pub fn warmup(self, segment: Segment) -> UtcRange {
        match segment {
            Segment::Train => self.train_warmup,
            Segment::Test => self.test_warmup,
        }
    }

    pub fn forced_flat(self, segment: Segment) -> UtcRange {
        match segment {
            Segment::Train => self.train_flat,
            Segment::Test => self.test_flat,
        }
    }
}

/// Last minute of a scored window. The fill is that minute's open, not its
/// mark close. The engine flattens on the first non-filler open at or after
/// `start`, which is that minute when the tape is a 1m grid.
pub fn forced_flat_at(end: UtcMicros) -> Result<UtcRange, ScheduleError> {
    let start = end
        .micros()
        .checked_sub(ONE_MINUTE_US)
        .ok_or(ScheduleError::Overflow)?;
    Ok(UtcRange {
        start: UtcMicros::from_micros(start),
        end,
    })
}

/// Folds covering `[sample_start, holdout_start)`.
///
/// The first test starts `warmup_days + train_days + purge_days` after
/// `sample_start`. Later tests step by `step_days`. `train_k` is the
/// `train_days` immediately before the purge that precedes `test_k`. The last
/// test's exclusive end is `holdout_start`. A final remainder shorter than
/// `remainder_own_fold_days` is appended to the previous test; a longer
/// remainder is its own fold.
pub fn fold_schedule(spec: &ScheduleSpec) -> Result<Vec<Fold>, ScheduleError> {
    require_at_least_zero("warmup_days", spec.warmup_days)?;
    require_at_least_zero("purge_days", spec.purge_days)?;
    require_positive("train_days", spec.train_days)?;
    require_positive("test_days", spec.test_days)?;
    require_positive("step_days", spec.step_days)?;
    require_positive("remainder_own_fold_days", spec.remainder_own_fold_days)?;
    if spec.step_days != spec.test_days {
        return Err(ScheduleError::StepMismatch);
    }
    let first_train = add_days(spec.sample_start, spec.warmup_days)?;
    let first_test = add_days(
        first_train,
        spec.train_days
            .checked_add(spec.purge_days)
            .ok_or(ScheduleError::Overflow)?,
    )?;
    let span = days_between(first_test, spec.holdout_start)?;
    if span <= 0 {
        return Err(ScheduleError::NoFold);
    }
    let (n_folds, last_len) = fold_lengths(span, spec.test_days, spec.remainder_own_fold_days)?;
    let mut folds =
        Vec::with_capacity(usize::try_from(n_folds).map_err(|_| ScheduleError::Overflow)?);
    for k in 0..n_folds {
        let test_start = add_days(
            first_test,
            k.checked_mul(spec.step_days)
                .ok_or(ScheduleError::Overflow)?,
        )?;
        let test_end = if k + 1 == n_folds {
            spec.holdout_start
        } else {
            add_days(test_start, spec.test_days)?
        };
        let length = days_between(test_start, test_end)?;
        let expected = if k + 1 == n_folds {
            last_len
        } else {
            spec.test_days
        };
        if length != expected || test_end > spec.holdout_start {
            return Err(ScheduleError::OutOfRange);
        }
        let train_end = add_days(test_start, -spec.purge_days)?;
        let train_start = add_days(train_end, -spec.train_days)?;
        if train_start < spec.sample_start || train_end > test_start {
            return Err(ScheduleError::OutOfRange);
        }
        let index = u32::try_from(k + 1).map_err(|_| ScheduleError::Overflow)?;
        folds.push(Fold {
            index,
            train_warmup: warmup_range(train_start, spec.warmup_days, spec.sample_start)?,
            train: midnight_range(train_start, train_end),
            purge: midnight_range(train_end, test_start),
            test_warmup: warmup_range(test_start, spec.warmup_days, spec.sample_start)?,
            test: midnight_range(test_start, test_end),
            train_flat: forced_flat_at(train_end.start_us())?,
            test_flat: forced_flat_at(test_end.start_us())?,
        });
    }
    if folds.last().map(|fold| fold.test.end) != Some(spec.holdout_start.start_us()) {
        return Err(ScheduleError::OutOfRange);
    }
    Ok(folds)
}

/// BTCUSDT in-sample schedule. `warmup_days` is 30 for the price families and
/// [`SESSION_WARMUP_DAYS`] for the session family. The exclusive end is the
/// holdout instant 2025-10-01T00:00:00Z, so the last scored minute opens at
/// 2025-09-30T23:59:00Z.
pub fn btcusdt_schedule(warmup_days: i64) -> Result<Vec<Fold>, ScheduleError> {
    fold_schedule(&ScheduleSpec {
        sample_start: date("2020-12-18")?,
        holdout_start: date("2025-10-01")?,
        warmup_days,
        train_days: TRAIN_DAYS,
        purge_days: PURGE_DAYS,
        test_days: TEST_DAYS,
        step_days: STEP_DAYS,
        remainder_own_fold_days: REMAINDER_OWN_FOLD_DAYS,
    })
}

fn date(text: &str) -> Result<UtcDate, ScheduleError> {
    UtcDate::parse(text).ok_or(ScheduleError::OutOfRange)
}

fn require_positive(name: &'static str, value: i64) -> Result<(), ScheduleError> {
    if value <= 0 {
        Err(ScheduleError::NonPositive(name))
    } else {
        Ok(())
    }
}

fn require_at_least_zero(name: &'static str, value: i64) -> Result<(), ScheduleError> {
    if value < 0 {
        Err(ScheduleError::NonPositive(name))
    } else {
        Ok(())
    }
}

fn add_days(day: UtcDate, days: i64) -> Result<UtcDate, ScheduleError> {
    let delta = days.checked_mul(DAY_US).ok_or(ScheduleError::Overflow)?;
    let micros = day
        .start_us()
        .micros()
        .checked_add(delta)
        .ok_or(ScheduleError::Overflow)?;
    Ok(UtcDate::from_micros(UtcMicros::from_micros(micros)))
}

fn days_between(start: UtcDate, end: UtcDate) -> Result<i64, ScheduleError> {
    let delta = end
        .start_us()
        .micros()
        .checked_sub(start.start_us().micros())
        .ok_or(ScheduleError::Overflow)?;
    if delta % DAY_US != 0 {
        return Err(ScheduleError::Overflow);
    }
    Ok(delta / DAY_US)
}

fn midnight_range(start: UtcDate, end: UtcDate) -> UtcRange {
    UtcRange {
        start: start.start_us(),
        end: end.start_us(),
    }
}

fn warmup_range(
    segment_start: UtcDate,
    warmup_days: i64,
    sample: UtcDate,
) -> Result<UtcRange, ScheduleError> {
    let raw = add_days(segment_start, -warmup_days)?;
    let start = if raw < sample { sample } else { raw };
    Ok(midnight_range(start, segment_start))
}

/// `(fold count, last test length in days)`.
fn fold_lengths(
    span: i64,
    test_days: i64,
    remainder_own_fold_days: i64,
) -> Result<(i64, i64), ScheduleError> {
    let full = span / test_days;
    let rem = span % test_days;
    if rem == 0 {
        return Ok((full, test_days));
    }
    if rem >= remainder_own_fold_days {
        return Ok((full.checked_add(1).ok_or(ScheduleError::Overflow)?, rem));
    }
    if full == 0 {
        return Ok((1, span));
    }
    Ok((
        full,
        test_days.checked_add(rem).ok_or(ScheduleError::Overflow)?,
    ))
}

/// One grid point, in selection order. `params` keys are ASCII; the map keeps
/// them in byte order for [`canonical_config_id`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridPoint<C> {
    pub config: C,
    pub params: BTreeMap<String, i64>,
    pub attempt: u64,
}

/// One family. `folds` come from [`fold_schedule`] at this family's warmup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyGrid<C> {
    pub family: String,
    pub folds: Vec<Fold>,
    pub points: Vec<GridPoint<C>>,
}

/// Equity path and the integer daily returns derived from it.
///
/// `equity_ticks[0]` is this run's opening equity. Each later value is the
/// end-of-day equity. `daily_returns[i]` is [`daily_return_tick`] of
/// `equity_ticks[i]` and `equity_ticks[i + 1]`. `net_result_ticks` is the last
/// equity minus the opening equity, after fees, slippage, and funding.
/// `round_trips` counts trips that closed inside the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowResult {
    pub daily_returns: Vec<i64>,
    pub equity_ticks: Vec<i64>,
    pub net_result_ticks: i64,
    pub round_trips: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowShape {
    Empty,
    Equity,
    Return,
    Net,
    Overflow,
}

impl std::fmt::Display for WindowShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "window has no equity"),
            Self::Equity => write!(f, "opening equity is not the fold cash"),
            Self::Return => write!(f, "daily returns do not match the equity path"),
            Self::Net => write!(f, "net result is not the equity change"),
            Self::Overflow => write!(f, "window arithmetic overflowed"),
        }
    }
}

impl std::error::Error for WindowShape {}

/// `trunc((next - prev) * 1e8 / prev)`. The same quotient the Sharpe path uses.
pub fn daily_return_tick(prev: i64, next: i64) -> Result<i64, WindowShape> {
    if prev <= 0 {
        return Err(WindowShape::Equity);
    }
    let num = (i128::from(next) - i128::from(prev))
        .checked_mul(SCALE)
        .ok_or(WindowShape::Overflow)?;
    let ticks = num / i128::from(prev);
    i64::try_from(ticks).map_err(|_| WindowShape::Overflow)
}

/// Build a [`WindowResult`] from a report. Closed trips are the round-trip count.
/// The opening equity is `initial`, not the first marked bar.
pub fn window_result(initial: Cash, report: &Report) -> Result<WindowResult, WindowShape> {
    let end = report.equity_end().ok_or(WindowShape::Empty)?;
    let mut equity_ticks = Vec::with_capacity(report.daily_equity.len().saturating_add(1));
    equity_ticks.push(initial.0);
    for (_, cash) in &report.daily_equity {
        equity_ticks.push(cash.0);
    }
    if equity_ticks.last().copied() != Some(end.0) {
        return Err(WindowShape::Equity);
    }
    let mut daily_returns = Vec::with_capacity(equity_ticks.len().saturating_sub(1));
    for pair in equity_ticks.windows(2) {
        daily_returns.push(daily_return_tick(pair[0], pair[1])?);
    }
    let net_result_ticks = end.0.checked_sub(initial.0).ok_or(WindowShape::Overflow)?;
    let closed = report
        .trades
        .iter()
        .filter(|trade| trade.close_ts.is_some())
        .count();
    let round_trips = u64::try_from(closed).map_err(|_| WindowShape::Overflow)?;
    Ok(WindowResult {
        daily_returns,
        equity_ticks,
        net_result_ticks,
        round_trips,
    })
}

/// `days` is the number of UTC midnights in the scored window.
pub fn check_window_result(
    initial_ticks: i64,
    days: usize,
    result: &WindowResult,
) -> Result<(), WindowShape> {
    if result.equity_ticks.first().copied() != Some(initial_ticks) {
        return Err(WindowShape::Equity);
    }
    if result.daily_returns.len() != days || result.equity_ticks.len() != days.saturating_add(1) {
        return Err(WindowShape::Return);
    }
    for (index, pair) in result.equity_ticks.windows(2).enumerate() {
        if daily_return_tick(pair[0], pair[1])? != result.daily_returns[index] {
            return Err(WindowShape::Return);
        }
    }
    let end = result
        .equity_ticks
        .last()
        .copied()
        .ok_or(WindowShape::Empty)?;
    let net = end
        .checked_sub(initial_ticks)
        .ok_or(WindowShape::Overflow)?;
    if net != result.net_result_ticks {
        return Err(WindowShape::Net);
    }
    Ok(())
}

/// Ledger and cash for one walk-forward call.
pub struct WalkContext<'a> {
    pub pins: &'a SweepPins,
    pub trials_path: &'a Path,
    pub initial_cash: Cash,
    pub holdout_start: UtcMicros,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentRun {
    pub grid_index: usize,
    pub run_id: String,
    pub trial_id: String,
    pub config_id: String,
    pub daily_returns: Vec<i64>,
    pub equity_ticks: Vec<i64>,
    pub net_result_ticks: i64,
    pub round_trips: u64,
    /// Annual Sharpe of this segment's equity, from [`sharpe_ticks`].
    pub sharpe_annual_ticks: Option<i64>,
}

/// One fee-grid or stress replay of a family's stitched test path.
///
/// `daily_returns` concatenates the selected config's test windows. Each fold
/// still starts from its own opening equity, so `equity_ticks` restarts and
/// `net_result_ticks` is the sum of the fold nets. `sharpe_annual_ticks` is
/// the Sharpe of the concatenated daily returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeeReplay {
    pub fee_bps: u32,
    pub slippage_bps: u32,
    pub daily_returns: Vec<i64>,
    pub equity_ticks: Vec<i64>,
    pub net_result_ticks: i64,
    pub round_trips: u64,
    pub sharpe_annual_ticks: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldOutcome {
    pub fold_index: u32,
    pub train: Vec<SegmentRun>,
    pub test: Vec<SegmentRun>,
    pub selected_index: usize,
    pub selected_config_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyOutput {
    pub family: String,
    pub folds: Vec<FoldOutcome>,
    pub stitched_oos_daily_returns: Vec<i64>,
    pub stitched_oos_equity: Vec<i64>,
    pub oos_round_trips: u64,
    /// Distinct `trial_id` values begun for this family. Folds do not multiply it.
    pub n_trials: u64,
    /// 2/4/5/6 bps at 2 bps slippage, then 5 bps fee at 5 bps slippage.
    pub fee_replays: Vec<FeeReplay>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkForward {
    pub families: Vec<FamilyOutput>,
    /// Sum of each family's [`FamilyOutput::n_trials`].
    pub n_trials: u64,
}

#[derive(Debug)]
pub enum WalkError<E> {
    Holdout {
        run_id: String,
        start_us: i64,
        end_us: i64,
    },
    BadWindow {
        run_id: String,
    },
    EmptyFamily(String),
    DuplicateFamily(String),
    SelectionCosts {
        fee_bps: u32,
        slippage_bps: u32,
    },
    Trial(String),
    Run {
        run_id: String,
        source: E,
    },
    Window {
        run_id: String,
        detail: WindowShape,
    },
    Overflow,
}

impl<E: std::fmt::Display> std::fmt::Display for WalkError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Holdout {
                run_id,
                start_us,
                end_us,
            } => write!(f, "holdout window for {run_id}: {start_us}..{end_us}"),
            Self::BadWindow { run_id } => write!(f, "window end is before start for {run_id}"),
            Self::EmptyFamily(name) => write!(f, "family {name} has no folds or no configs"),
            Self::DuplicateFamily(name) => write!(f, "duplicate family {name}"),
            Self::SelectionCosts {
                fee_bps,
                slippage_bps,
            } => write!(
                f,
                "selection costs are {fee_bps} bps fee and {slippage_bps} bps slippage"
            ),
            Self::Trial(msg) => write!(f, "{msg}"),
            Self::Run { run_id, source } => write!(f, "run {run_id} failed: {source}"),
            Self::Window { run_id, detail } => write!(f, "run {run_id}: {detail}"),
            Self::Overflow => write!(f, "walk-forward count overflowed"),
        }
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for WalkError<E> {}

/// Run every fold of every family.
///
/// `run` is
/// `FnMut(&Config, warmup_start_us, start_us, end_us, &Costs) -> Result<WindowResult, E>`.
/// `warmup_start_us` and `start_us` are inclusive. `end_us` is exclusive. All
/// three are UTC midnight microseconds. The signal reads bars from
/// `warmup_start_us`. Fills, funding, and scoring start at `start_us`. The
/// closure flattens at `end_us`, at that minute's open.
///
/// Selection reads the train annual Sharpe only. The test result is stored
/// and, for the selected config, appended to that family's OOS series. The
/// selected test path is then replayed at 2/4/5/6 bps and at the 5+5 stress
/// row. Replays are not trials.
///
/// Every scored call is one `RunStarted`, fsynced before `run`. `config_id`
/// does not include the fold. `fold` and `segment` do. A holdout window
/// (`warmup_start >= holdout`, `start >= holdout`, or `end > holdout`) is
/// aborted and `run` is not called. `end == holdout` stays in sample.
pub fn run_walk_forward<C, F, E>(
    families: &[FamilyGrid<C>],
    ctx: &WalkContext<'_>,
    costs: &Costs,
    mut run: F,
) -> Result<WalkForward, WalkError<E>>
where
    F: FnMut(&C, i64, i64, i64, &Costs) -> Result<WindowResult, E>,
    E: std::fmt::Display,
{
    if costs.taker_fee.0 != SELECTION_FEE_BPS || costs.slippage.0 != SELECTION_SLIPPAGE_BPS {
        return Err(WalkError::SelectionCosts {
            fee_bps: costs.taker_fee.0,
            slippage_bps: costs.slippage.0,
        });
    }
    let mut seen = BTreeSet::new();
    for family in families {
        if family.family.is_empty() || family.folds.is_empty() || family.points.is_empty() {
            return Err(WalkError::EmptyFamily(family.family.clone()));
        }
        if !seen.insert(family.family.as_str()) {
            return Err(WalkError::DuplicateFamily(family.family.clone()));
        }
    }
    let log = TrialLog::new(ctx.trials_path);
    let mut out = Vec::with_capacity(families.len());
    for family in families {
        let mut drive = Drive {
            ctx,
            costs,
            log: &log,
            run: &mut run,
        };
        out.push(drive.family(family)?);
    }
    let n_trials = out.iter().try_fold(0u64, |sum, family| {
        sum.checked_add(family.n_trials).ok_or(WalkError::Overflow)
    })?;
    Ok(WalkForward {
        families: out,
        n_trials,
    })
}

struct Drive<'a, F> {
    ctx: &'a WalkContext<'a>,
    costs: &'a Costs,
    log: &'a TrialLog,
    run: &'a mut F,
}

impl<F> Drive<'_, F> {
    fn family<C, E>(&mut self, family: &FamilyGrid<C>) -> Result<FamilyOutput, WalkError<E>>
    where
        F: FnMut(&C, i64, i64, i64, &Costs) -> Result<WindowResult, E>,
        E: std::fmt::Display,
    {
        let bounds = config_bounds(&family.folds);
        let mut identities = Vec::with_capacity(family.points.len());
        for point in &family.points {
            identities.push(config_identity(family, self.ctx, point, bounds));
        }
        let mut folds = Vec::with_capacity(family.folds.len());
        let mut stitched_oos_daily_returns = Vec::new();
        let mut stitched_oos_equity = Vec::new();
        let mut oos_round_trips = 0u64;
        let mut trial_ids = BTreeSet::new();
        for fold in &family.folds {
            let mut train = Vec::with_capacity(family.points.len());
            for (grid_index, point) in family.points.iter().enumerate() {
                let row = self.segment(SegmentCall {
                    family,
                    fold,
                    point,
                    identity: &identities[grid_index],
                    grid_index,
                    segment: Segment::Train,
                    costs: self.costs,
                    trial: true,
                })?;
                trial_ids.insert(row.trial_id.clone());
                train.push(row);
            }
            let selected_index = Self::select_train(&train);
            let mut test = Vec::with_capacity(family.points.len());
            for (grid_index, point) in family.points.iter().enumerate() {
                let row = self.segment(SegmentCall {
                    family,
                    fold,
                    point,
                    identity: &identities[grid_index],
                    grid_index,
                    segment: Segment::Test,
                    costs: self.costs,
                    trial: true,
                })?;
                trial_ids.insert(row.trial_id.clone());
                test.push(row);
            }
            let chosen = &test[selected_index];
            stitched_oos_daily_returns.extend_from_slice(&chosen.daily_returns);
            stitched_oos_equity.extend_from_slice(&chosen.equity_ticks);
            oos_round_trips = oos_round_trips
                .checked_add(chosen.round_trips)
                .ok_or(WalkError::Overflow)?;
            folds.push(FoldOutcome {
                fold_index: fold.index,
                selected_config_id: chosen.config_id.clone(),
                selected_index,
                train,
                test,
            });
        }
        let fee_replays = self.fee_replays(family, &identities, &folds)?;
        let n_trials = u64::try_from(trial_ids.len()).map_err(|_| WalkError::Overflow)?;
        Ok(FamilyOutput {
            family: family.family.clone(),
            folds,
            stitched_oos_daily_returns,
            stitched_oos_equity,
            oos_round_trips,
            n_trials,
            fee_replays,
        })
    }

    /// Highest train annual Sharpe. An equal Sharpe keeps the earlier grid index.
    /// An undefined Sharpe loses to a defined one and ties with another undefined one.
    fn select_train(train: &[SegmentRun]) -> usize {
        let mut best = 0usize;
        for (index, row) in train.iter().enumerate().skip(1) {
            if sharpe_beats(row.sharpe_annual_ticks, train[best].sharpe_annual_ticks) {
                best = index;
            }
        }
        best
    }

    fn fee_replays<C, E>(
        &mut self,
        family: &FamilyGrid<C>,
        identities: &[Identity],
        folds: &[FoldOutcome],
    ) -> Result<Vec<FeeReplay>, WalkError<E>>
    where
        F: FnMut(&C, i64, i64, i64, &Costs) -> Result<WindowResult, E>,
        E: std::fmt::Display,
    {
        let mut out = Vec::with_capacity(FEE_GRID_BPS.len() + 1);
        for (fee_bps, slippage_bps) in replay_grid() {
            if fee_bps == SELECTION_FEE_BPS && slippage_bps == SELECTION_SLIPPAGE_BPS {
                out.push(replay_from_selection(fee_bps, slippage_bps, folds)?);
                continue;
            }
            let costs = Costs {
                taker_fee: FeeBps(fee_bps),
                slippage: SlippageBps(slippage_bps),
            };
            let mut daily_returns = Vec::new();
            let mut equity_ticks = Vec::new();
            let mut net_result_ticks = 0i64;
            let mut round_trips = 0u64;
            for (fold, outcome) in family.folds.iter().zip(folds) {
                let index = outcome.selected_index;
                let row = self.segment(SegmentCall {
                    family,
                    fold,
                    point: &family.points[index],
                    identity: &identities[index],
                    grid_index: index,
                    segment: Segment::Test,
                    costs: &costs,
                    trial: false,
                })?;
                daily_returns.extend_from_slice(&row.daily_returns);
                equity_ticks.extend_from_slice(&row.equity_ticks);
                net_result_ticks = net_result_ticks
                    .checked_add(row.net_result_ticks)
                    .ok_or(WalkError::Overflow)?;
                round_trips = round_trips
                    .checked_add(row.round_trips)
                    .ok_or(WalkError::Overflow)?;
            }
            out.push(FeeReplay {
                fee_bps,
                slippage_bps,
                sharpe_annual_ticks: annual_sharpe(&daily_returns)?,
                daily_returns,
                equity_ticks,
                net_result_ticks,
                round_trips,
            });
        }
        Ok(out)
    }

    fn segment<C, E>(&mut self, call: SegmentCall<'_, C>) -> Result<SegmentRun, WalkError<E>>
    where
        F: FnMut(&C, i64, i64, i64, &Costs) -> Result<WindowResult, E>,
        E: std::fmt::Display,
    {
        let SegmentCall {
            family,
            fold,
            point,
            identity,
            grid_index,
            segment,
            costs,
            trial,
        } = call;
        let scored = fold.scored(segment);
        let warmup = fold.warmup(segment).start.micros();
        let start = scored.start.micros();
        let end = scored.end.micros();
        let run_id = format!(
            "{}-f{:02}-{}-c{}",
            family.family,
            fold.index,
            segment.as_str(),
            grid_index
        );
        if trial {
            self.log
                .begin(StartRecord {
                    run_id: &run_id,
                    trial_id: &identity.trial_id,
                    config_id: &identity.config_id,
                    pinned_root: self.ctx.pins.pinned_root(),
                    attempt: point.attempt,
                    family: &family.family,
                    ts: start,
                    fold: Some(fold.index),
                    segment: Some(segment.as_str()),
                })
                .map_err(map_trial)?;
        }
        if let Some(reason) = gate(warmup, start, end, self.ctx.holdout_start.micros()) {
            if trial {
                self.log.abort(&run_id, reason).map_err(map_trial)?;
            }
            return Err(gate_error(reason, &run_id, start, end));
        }
        let computed = panic::catch_unwind(AssertUnwindSafe(|| {
            (self.run)(&point.config, warmup, start, end, costs)
        }));
        match computed {
            Ok(Ok(result)) => {
                let days = window_days(&run_id, start, end)?;
                if let Err(detail) = check_window_result(self.ctx.initial_cash.0, days, &result) {
                    let message = detail.to_string();
                    if trial {
                        self.log.abort(&run_id, &message).map_err(map_trial)?;
                    }
                    return Err(WalkError::Window { run_id, detail });
                }
                let sharpe_annual_ticks = match sharpe_ticks(&result.equity_ticks) {
                    Ok((_, annual)) => annual.map(Ratio::ticks),
                    Err(err) => {
                        if trial {
                            let message = err.to_string();
                            self.log.abort(&run_id, &message).map_err(map_trial)?;
                        }
                        return Err(WalkError::Overflow);
                    }
                };
                if trial {
                    let digest = window_digest(&result);
                    self.log.finish(&run_id, &digest).map_err(map_trial)?;
                }
                Ok(SegmentRun {
                    grid_index,
                    run_id,
                    trial_id: identity.trial_id.clone(),
                    config_id: identity.config_id.clone(),
                    daily_returns: result.daily_returns,
                    equity_ticks: result.equity_ticks,
                    net_result_ticks: result.net_result_ticks,
                    round_trips: result.round_trips,
                    sharpe_annual_ticks,
                })
            }
            Ok(Err(source)) => {
                let message = source.to_string();
                if trial {
                    self.log.abort(&run_id, &message).map_err(map_trial)?;
                }
                Err(WalkError::Run { run_id, source })
            }
            Err(payload) => {
                if trial {
                    let _ = self.log.abort(&run_id, "panic");
                }
                panic::resume_unwind(payload);
            }
        }
    }
}

struct Identity {
    config_id: String,
    trial_id: String,
}

struct SegmentCall<'a, C> {
    family: &'a FamilyGrid<C>,
    fold: &'a Fold,
    point: &'a GridPoint<C>,
    identity: &'a Identity,
    grid_index: usize,
    segment: Segment,
    costs: &'a Costs,
    trial: bool,
}

fn config_bounds(folds: &[Fold]) -> UtcRange {
    UtcRange {
        start: folds[0].train_warmup.start,
        end: folds[folds.len() - 1].test.end,
    }
}

fn config_identity<C>(
    family: &FamilyGrid<C>,
    ctx: &WalkContext<'_>,
    point: &GridPoint<C>,
    bounds: UtcRange,
) -> Identity {
    let spec = TrialSpec {
        run_id: String::new(),
        config_id: String::new(),
        family: family.family.clone(),
        params: point.params.clone(),
        attempt: point.attempt,
        ts: bounds.start,
        run: RunConfig {
            initial_cash: ctx.initial_cash,
            warmup_start: bounds.start,
            start: bounds.start,
            end: bounds.end,
            holdout_start: ctx.holdout_start,
            forced_flat: Vec::new(),
            mark_gaps: Vec::new(),
            interval: Interval::ONE_HOUR,
            lot_ticks: 100_000,
        },
    };
    let config_id = canonical_config_id(&spec);
    let trial_id = trial_id(&config_id, ctx.pins.pinned_root(), point.attempt);
    Identity {
        config_id,
        trial_id,
    }
}

fn replay_grid() -> [(u32, u32); 5] {
    [
        (FEE_GRID_BPS[0], SELECTION_SLIPPAGE_BPS),
        (FEE_GRID_BPS[1], SELECTION_SLIPPAGE_BPS),
        (FEE_GRID_BPS[2], SELECTION_SLIPPAGE_BPS),
        (FEE_GRID_BPS[3], SELECTION_SLIPPAGE_BPS),
        (SELECTION_FEE_BPS, STRESS_SLIPPAGE_BPS),
    ]
}

fn replay_from_selection<E>(
    fee_bps: u32,
    slippage_bps: u32,
    folds: &[FoldOutcome],
) -> Result<FeeReplay, WalkError<E>> {
    let mut daily_returns = Vec::new();
    let mut equity_ticks = Vec::new();
    let mut net_result_ticks = 0i64;
    let mut round_trips = 0u64;
    for fold in folds {
        let chosen = &fold.test[fold.selected_index];
        daily_returns.extend_from_slice(&chosen.daily_returns);
        equity_ticks.extend_from_slice(&chosen.equity_ticks);
        net_result_ticks = net_result_ticks
            .checked_add(chosen.net_result_ticks)
            .ok_or(WalkError::Overflow)?;
        round_trips = round_trips
            .checked_add(chosen.round_trips)
            .ok_or(WalkError::Overflow)?;
    }
    Ok(FeeReplay {
        fee_bps,
        slippage_bps,
        sharpe_annual_ticks: annual_sharpe(&daily_returns)?,
        daily_returns,
        equity_ticks,
        net_result_ticks,
        round_trips,
    })
}

fn annual_sharpe<E>(returns: &[i64]) -> Result<Option<i64>, WalkError<E>> {
    match sharpe_of_return_ticks(returns) {
        Ok((_, annual)) => Ok(annual.map(Ratio::ticks)),
        Err(_) => Err(WalkError::Overflow),
    }
}

fn sharpe_beats(candidate: Option<i64>, incumbent: Option<i64>) -> bool {
    match (candidate, incumbent) {
        (Some(left), Some(right)) => left > right,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn gate(warmup: i64, start: i64, end: i64, holdout: i64) -> Option<&'static str> {
    if warmup >= holdout || start >= holdout || end > holdout {
        return Some("holdout");
    }
    if warmup > start || end < start {
        return Some("bad window");
    }
    None
}

fn gate_error<E>(reason: &str, run_id: &str, start: i64, end: i64) -> WalkError<E> {
    match reason {
        "holdout" => WalkError::Holdout {
            run_id: run_id.to_owned(),
            start_us: start,
            end_us: end,
        },
        _ => WalkError::BadWindow {
            run_id: run_id.to_owned(),
        },
    }
}

fn window_days<E>(run_id: &str, start: i64, end: i64) -> Result<usize, WalkError<E>> {
    if start % DAY_US != 0 || end % DAY_US != 0 || end <= start {
        return Err(WalkError::BadWindow {
            run_id: run_id.to_owned(),
        });
    }
    let days = (end - start) / DAY_US;
    usize::try_from(days).map_err(|_| WalkError::Overflow)
}

fn window_digest(result: &WindowResult) -> String {
    sha256_hex(window_preimage(result).as_bytes())
}

fn window_preimage(result: &WindowResult) -> String {
    format!(
        "{{\"daily_returns\":{},\"equity_ticks\":{},\"net_result_ticks\":{},\"round_trips\":{}}}",
        json_i64s(&result.daily_returns),
        json_i64s(&result.equity_ticks),
        result.net_result_ticks,
        result.round_trips
    )
}

fn json_i64s(values: &[i64]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&value.to_string());
    }
    out.push(']');
    out
}

fn map_trial<E>(err: TrialError) -> WalkError<E> {
    match err {
        TrialError::DuplicateRunId(id) => WalkError::Trial(format!("duplicate run_id {id}")),
        other => WalkError::Trial(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn day(text: &str) -> UtcDate {
        UtcDate::parse(text).expect(text)
    }

    fn ymd(range: UtcRange) -> (String, String) {
        (
            UtcDate::from_micros(range.start).to_string(),
            UtcDate::from_micros(range.end).to_string(),
        )
    }

    fn span_days(range: UtcRange) -> i64 {
        (range.end.micros() - range.start.micros()) / DAY_US
    }

    const PRICE_FOLDS: &[(&str, &str, &str, &str)] = &[
        ("2021-01-17", "2022-01-17", "2022-01-19", "2022-04-19"),
        ("2021-04-17", "2022-04-17", "2022-04-19", "2022-07-18"),
        ("2021-07-16", "2022-07-16", "2022-07-18", "2022-10-16"),
        ("2021-10-14", "2022-10-14", "2022-10-16", "2023-01-14"),
        ("2022-01-12", "2023-01-12", "2023-01-14", "2023-04-14"),
        ("2022-04-12", "2023-04-12", "2023-04-14", "2023-07-13"),
        ("2022-07-11", "2023-07-11", "2023-07-13", "2023-10-11"),
        ("2022-10-09", "2023-10-09", "2023-10-11", "2024-01-09"),
        ("2023-01-07", "2024-01-07", "2024-01-09", "2024-04-08"),
        ("2023-04-07", "2024-04-06", "2024-04-08", "2024-07-07"),
        ("2023-07-06", "2024-07-05", "2024-07-07", "2024-10-05"),
        ("2023-10-04", "2024-10-03", "2024-10-05", "2025-01-03"),
        ("2024-01-02", "2025-01-01", "2025-01-03", "2025-04-03"),
        ("2024-04-01", "2025-04-01", "2025-04-03", "2025-07-02"),
        ("2024-06-30", "2025-06-30", "2025-07-02", "2025-10-01"),
    ];

    const SESSION_FOLDS: &[(&str, &str, &str, &str)] = &[
        ("2021-12-18", "2022-12-18", "2022-12-20", "2023-03-20"),
        ("2022-03-18", "2023-03-18", "2023-03-20", "2023-06-18"),
        ("2022-06-16", "2023-06-16", "2023-06-18", "2023-09-16"),
        ("2022-09-14", "2023-09-14", "2023-09-16", "2023-12-15"),
        ("2022-12-13", "2023-12-13", "2023-12-15", "2024-03-14"),
        ("2023-03-13", "2024-03-12", "2024-03-14", "2024-06-12"),
        ("2023-06-11", "2024-06-10", "2024-06-12", "2024-09-10"),
        ("2023-09-09", "2024-09-08", "2024-09-10", "2024-12-09"),
        ("2023-12-08", "2024-12-07", "2024-12-09", "2025-03-09"),
        ("2024-03-07", "2025-03-07", "2025-03-09", "2025-06-07"),
        ("2024-06-05", "2025-06-05", "2025-06-07", "2025-10-01"),
    ];

    fn assert_table(folds: &[Fold], table: &[(&str, &str, &str, &str)], warmup_days: i64) {
        assert_eq!(folds.len(), table.len());
        let sample = day("2020-12-18").start_us();
        let holdout = day("2025-10-01").start_us();
        for (fold, (train_start, train_end, test_start, test_end)) in folds.iter().zip(table) {
            assert_eq!(
                ymd(fold.train),
                (train_start.to_string(), train_end.to_string())
            );
            assert_eq!(
                ymd(fold.test),
                (test_start.to_string(), test_end.to_string())
            );
            assert_eq!(span_days(fold.train), TRAIN_DAYS);
            assert_eq!(fold.purge.start, fold.train.end);
            assert_eq!(fold.purge.end, fold.test.start);
            assert_eq!(span_days(fold.purge), PURGE_DAYS);
            assert_eq!(fold.train_warmup.end, fold.train.start);
            assert_eq!(fold.test_warmup.end, fold.test.start);
            assert_eq!(span_days(fold.train_warmup), warmup_days);
            assert_eq!(span_days(fold.test_warmup), warmup_days);
            assert!(fold.train_warmup.start >= sample);
            assert!(fold.test_warmup.start >= sample);
            assert!(fold.test.end <= holdout);
            assert!(fold.train.start < holdout);
            assert_eq!(fold.train_flat.end, fold.train.end);
            assert_eq!(fold.test_flat.end, fold.test.end);
            assert_eq!(
                fold.train_flat.start.micros(),
                fold.train.end.micros() - ONE_MINUTE_US
            );
        }
        for pair in folds.windows(2) {
            assert_eq!(pair[1].test.start, pair[0].test.end);
            assert_eq!(
                pair[1].test.start.micros() - pair[0].test.start.micros(),
                STEP_DAYS * DAY_US
            );
        }
    }

    #[test]
    fn thirty_day_warmup_matches_the_price_family_table() {
        let folds = btcusdt_schedule(WARMUP_DAYS).unwrap();
        assert_table(&folds, PRICE_FOLDS, WARMUP_DAYS);
        assert_eq!(ymd(folds[0].train_warmup).0, "2020-12-18");
        let oos: i64 = folds.iter().map(|fold| span_days(fold.test)).sum();
        assert_eq!(oos, 1_351);
        assert_eq!(span_days(folds.last().unwrap().test), 91);
        assert_eq!(folds.len(), 15);
    }

    #[test]
    fn session_warmup_matches_the_s06_table() {
        let folds = btcusdt_schedule(SESSION_WARMUP_DAYS).unwrap();
        assert_table(&folds, SESSION_FOLDS, SESSION_WARMUP_DAYS);
        assert_eq!(ymd(folds[0].train).0, "2021-12-18");
        assert_eq!(ymd(folds[0].test).0, "2022-12-20");
        let oos: i64 = folds.iter().map(|fold| span_days(fold.test)).sum();
        assert_eq!(oos, 1_016);
        assert_eq!(span_days(folds.last().unwrap().test), 116);
        assert_eq!(folds.len(), 11);
    }

    #[test]
    fn last_scored_minute_is_2025_09_30_2359_utc() {
        let folds = btcusdt_schedule(WARMUP_DAYS).unwrap();
        let last = folds.last().unwrap();
        let holdout = day("2025-10-01").start_us();
        assert_eq!(last.test.end, holdout);
        assert_eq!(holdout.micros(), crate::batch::HOLDOUT_START_US);
        let last_open = last.test_flat.start;
        assert_eq!(last_open.micros(), holdout.micros() - ONE_MINUTE_US);
        let day = UtcDate::from_micros(last_open);
        let minute_of_day = (last_open.micros() - day.start_us().micros()) / ONE_MINUTE_US;
        assert_eq!(day.to_string(), "2025-09-30");
        assert_eq!(minute_of_day, 23 * 60 + 59);
        assert!(folds.iter().all(|fold| fold.test.end <= holdout));
        assert!(folds.iter().all(|fold| fold.train.end < holdout));
    }

    #[test]
    fn warmup_is_a_parameter() {
        let folds = btcusdt_schedule(10).unwrap();
        assert_eq!(ymd(folds[0].train).0, "2020-12-28");
        assert_eq!(span_days(folds[0].train_warmup), 10);
        assert_eq!(ymd(folds[0].train_warmup).0, "2020-12-18");
    }

    #[test]
    fn remainder_below_45_appends_and_45_is_its_own_fold() {
        let appended = fold_schedule(&ScheduleSpec {
            sample_start: day("2021-01-01"),
            holdout_start: day("2021-05-17"),
            warmup_days: 0,
            train_days: 1,
            purge_days: 1,
            test_days: 90,
            step_days: 90,
            remainder_own_fold_days: 45,
        })
        .unwrap();
        assert_eq!(appended.len(), 1);
        assert_eq!(
            ymd(appended[0].test),
            ("2021-01-03".to_string(), "2021-05-17".to_string())
        );
        assert_eq!(span_days(appended[0].test), 134);

        let own = fold_schedule(&ScheduleSpec {
            sample_start: day("2021-01-01"),
            holdout_start: day("2021-05-18"),
            warmup_days: 0,
            train_days: 1,
            purge_days: 1,
            test_days: 90,
            step_days: 90,
            remainder_own_fold_days: 45,
        })
        .unwrap();
        assert_eq!(own.len(), 2);
        assert_eq!(span_days(own[0].test), 90);
        assert_eq!(span_days(own[1].test), 45);
        assert_eq!(own[1].test.start, own[0].test.end);
        assert_eq!(ymd(own[1].test).1, "2021-05-18");
    }

    #[test]
    fn a_short_span_below_the_remainder_is_one_fold() {
        let folds = fold_schedule(&ScheduleSpec {
            sample_start: day("2021-01-01"),
            holdout_start: day("2021-02-16"),
            warmup_days: 0,
            train_days: 1,
            purge_days: 1,
            test_days: 90,
            step_days: 90,
            remainder_own_fold_days: 45,
        })
        .unwrap();
        assert_eq!(folds.len(), 1);
        assert_eq!(span_days(folds[0].test), 44);
        assert_eq!(ymd(folds[0].test).1, "2021-02-16");
    }

    #[test]
    fn no_fold_when_warmup_passes_the_holdout() {
        let err = fold_schedule(&ScheduleSpec {
            sample_start: day("2021-01-01"),
            holdout_start: day("2021-01-02"),
            warmup_days: 30,
            train_days: TRAIN_DAYS,
            purge_days: PURGE_DAYS,
            test_days: TEST_DAYS,
            step_days: STEP_DAYS,
            remainder_own_fold_days: REMAINDER_OWN_FOLD_DAYS,
        })
        .unwrap_err();
        assert_eq!(err, ScheduleError::NoFold);
    }

    #[test]
    fn step_must_equal_the_test_length() {
        let err = fold_schedule(&ScheduleSpec {
            sample_start: day("2021-01-01"),
            holdout_start: day("2021-06-01"),
            warmup_days: 0,
            train_days: 1,
            purge_days: 1,
            test_days: 10,
            step_days: 7,
            remainder_own_fold_days: 5,
        })
        .unwrap_err();
        assert_eq!(err, ScheduleError::StepMismatch);
    }

    #[test]
    fn daily_return_is_the_truncating_quotient() {
        assert_eq!(daily_return_tick(100, 110).unwrap(), 10_000_000);
        assert_eq!(daily_return_tick(3, 4).unwrap(), 33_333_333);
        assert_eq!(daily_return_tick(100, 90).unwrap(), -10_000_000);
        assert_eq!(daily_return_tick(0, 1), Err(WindowShape::Equity));
    }

    proptest::proptest! {
        #[test]
        fn schedule_ends_on_the_holdout_and_purges(
            warmup in 0i64..20,
            train in 1i64..30,
            purge in 0i64..5,
            test in 1i64..20,
            rem_min in 1i64..20,
            extra in 0i64..40,
        ) {
            let sample = day("2021-01-01");
            let offset = warmup + train + purge + test + extra;
            let holdout = UtcDate::from_micros(UtcMicros::from_micros(
                sample.start_us().micros() + offset * DAY_US,
            ));
            let folds = fold_schedule(&ScheduleSpec {
                sample_start: sample,
                holdout_start: holdout,
                warmup_days: warmup,
                train_days: train,
                purge_days: purge,
                test_days: test,
                step_days: test,
                remainder_own_fold_days: rem_min,
            })
            .unwrap();
            prop_assert!(!folds.is_empty());
            prop_assert_eq!(folds.last().unwrap().test.end, holdout.start_us());
            let mut previous: Option<Fold> = None;
            for fold in &folds {
                prop_assert_eq!(span_days(fold.train), train);
                prop_assert_eq!(fold.purge.start, fold.train.end);
                prop_assert_eq!(fold.purge.end, fold.test.start);
                prop_assert_eq!(span_days(fold.purge), purge);
                prop_assert!(fold.train.end <= fold.test.start);
                prop_assert!(fold.train_warmup.start >= sample.start_us());
                prop_assert!(fold.test_warmup.start >= sample.start_us());
                prop_assert_eq!(fold.train_warmup.end, fold.train.start);
                prop_assert_eq!(fold.test_warmup.end, fold.test.start);
                prop_assert!(fold.test.end <= holdout.start_us());
                prop_assert!(fold.test.start < holdout.start_us());
                prop_assert_eq!(fold.test_flat.end, fold.test.end);
                prop_assert_eq!(
                    fold.test_flat.start.micros(),
                    fold.test.end.micros() - ONE_MINUTE_US
                );
                if let Some(prev) = previous {
                    prop_assert_eq!(fold.test.start, prev.test.end);
                }
                previous = Some(*fold);
            }
        }

        #[test]
        fn btcusdt_purge_is_two_days(warmup in 0i64..80) {
            let folds = btcusdt_schedule(warmup).unwrap();
            prop_assert!(!folds.is_empty());
            for fold in &folds {
                prop_assert_eq!(span_days(fold.purge), PURGE_DAYS);
                prop_assert_eq!(
                    fold.purge.end.micros() - fold.purge.start.micros(),
                    PURGE_DAYS * DAY_US
                );
            }
        }
    }
}
