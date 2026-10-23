//! Event-driven backtest for one BTCUSDT USD-M stream.
//!
//! `run` walks 1m bars, settles funding on the pre-fill position, fills the
//! previous decision at this bar's open, then marks equity on the mark close.
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
mod canon;
mod date;
mod engine;
mod error;
pub mod fixture;
mod metrics;
mod report;
mod signal;
mod sizing;
mod sweep;
mod tape;
mod trial_log;

pub use bar::{Bar, BarView, FundingStamp, Strategy, Tape};
pub use batch::{
    annual_sharpe_ticks, canonical_config_id, metrics_digest, metrics_from_grid, run_batch,
    turnover_ratio, BatchError, FeeGridRunner, FlatBars, ResultRow, SweepPins, TargetEngine,
    TargetRunner, TrialMetrics, TrialSpec, HOLDOUT_START_US,
};
pub use date::UtcDate;
pub use engine::{run, run_fee_grid, run_targets, MarkAbsence, RunConfig, UtcRange, FEE_GRID_BPS};
pub use error::{EngineError, TapeError};
pub use metrics::{max_drawdown, sharpe_ticks};
pub use report::{Metrics, Report, RoundTrip};
pub use signal::{
    funding_visible, BookState, Decision, FlatReason, FundingObs, SignalOutput, Target,
    TargetSignal,
};
pub use sizing::{abs_notional_ticks, target_qty};
pub use sweep::{run_sweep, FoldRunner, SweepRequest, WindowConfig, WindowOutput};
pub use tape::{funding_available, planned_paths, AppliedOverlay, Overlay, TapeConfig};
pub use trial_log::{finish_trial, trial_id, TrialError};
