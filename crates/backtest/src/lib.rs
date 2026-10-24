//! Event-driven backtest for one BTCUSDT USD-M stream.
//!
//! `run` walks 1m bars, settles funding on the pre-fill position, fills the
//! previous decision at this bar's open, then marks equity at the mark price.
//! The strategy sees a [`BarView`] of bars whose stored `available_ts` is at or
//! before the current bar's stored `available_ts`.
//!
//! Um-kline `available_ts_us` in the store is `close_time + 1ms`. Comparing
//! against `close_time + 1µs` would hide the bar that just closed. The view
//! therefore uses the stored timestamp and includes bar t.
//!
//! `clippy::float_arithmetic` is denied. Money and Sharpe stay in integer ticks.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::cast_precision_loss)]

mod bar;
mod batch;
mod c1;
mod c1_source;
mod canon;
mod cash_view;
mod date;
mod engine;
mod error;
pub mod fixture;
pub mod h4_report;
mod metrics;
pub mod offline_replay;
mod report;
mod signal;
mod sizing;
mod stats;
mod sweep;
mod table;
mod tape;
mod trial_log;
mod walkforward;
mod wfbatch;

#[cfg(test)]
#[path = "../tests/common/paper_cases.rs"]
pub mod paper_cases;

pub use bar::{Bar, BarView, FundingStamp, PremiumMinute, Strategy, Tape};
pub use batch::{
    annual_sharpe_ticks, canonical_config_id, metrics_digest, metrics_from_grid, run_batch,
    turnover_ratio, BatchError, FeeGridRunner, FlatBars, ResultRow, SweepPins, TargetEngine,
    TargetRunner, TrialMetrics, TrialSpec, HOLDOUT_START_US,
};
pub use date::UtcDate;
pub use engine::{run, run_fee_grid, run_targets, MarkAbsence, RunConfig, UtcRange, FEE_GRID_BPS};
pub use error::{EngineError, TapeError};
pub use metrics::{max_drawdown, sharpe_ticks};
pub use report::{
    ForcedExit, ForcedExitReason, H4EntryDrop, H4EntryDropReason, H4Trace, H4Trade, Metrics,
    Report, RoundTrip,
};
pub use signal::{
    funding_visible, ActualHold, BookState, Decision, ExecutionDirective, ExecutionPhase,
    FlatReason, FundingObs, H4Event, H4Observation, PositionSide, SignalOutput, StrategyOutput,
    Target, TargetSignal,
};
pub use sizing::{abs_notional_ticks, target_qty};
pub use sweep::{run_sweep, FoldRunner, SweepRequest, WindowConfig, WindowOutput};
pub use tape::{funding_available, planned_paths, AppliedOverlay, Overlay, TapeConfig};
pub use trial_log::{distinct_trial_count, finish_trial, trial_id, TrialError};
pub use walkforward::{
    btcusdt_schedule, check_window_result, daily_return_tick, fold_schedule, forced_flat_at,
    run_walk_forward, selection_costs, window_result, FamilyGrid, FamilyOutput, FeeReplay, Fold,
    FoldOutcome, GridPoint, ScheduleError, ScheduleSpec, Segment, SegmentRun, StayFlatShadow,
    WalkContext, WalkError, WalkForward, WindowAccounting, WindowReplay, WindowResult, WindowShape,
    H1_WARMUP_DAYS, H3_WARMUP_DAYS, H4_WARMUP_DAYS, H5_WARMUP_DAYS, PURGE_DAYS,
    REMAINDER_OWN_FOLD_DAYS, SELECTION_FEE_BPS, SELECTION_SLIPPAGE_BPS, SESSION_WARMUP_DAYS,
    STEP_DAYS, STRESS_SLIPPAGE_BPS, TEST_DAYS, TRAIN_DAYS, WARMUP_DAYS,
};
pub use wfbatch::{run_wf_batch, WfBatchRequest, DEFAULT_DATA_ROOT};
