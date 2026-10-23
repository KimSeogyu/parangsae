//! Target decisions the engine can trade.
//!
//! [`Decision`] and [`SignalOutput`] are the signal crate's types. `Flat`
//! carries a reason and no size, so a nonzero target cannot be paired with a
//! flat reason.
//!
//! ```compile_fail
//! use parangsae_signals::{Decision, FlatReason};
//! fn flatten_with_target(reason: FlatReason, target_ticks: i64) -> Decision {
//!     Decision::Flat(reason, target_ticks)
//! }
//! ```

use parangsae_core::{Causal, Ratio, ResampledBar, UtcMicros};

pub use parangsae_signals::{Decision, FlatReason, SignalOutput, Target};

/// Funding row a signal may read. Settlement still uses `funding_time`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundingObs {
    pub funding_time: UtcMicros,
    pub available: UtcMicros,
    pub rate: Ratio,
}

/// Rows whose `available` is at or before `decision`, in stored order.
pub fn funding_visible(rows: &[FundingObs], decision: UtcMicros) -> &[FundingObs] {
    let end = rows.partition_point(|row| row.available <= decision);
    &rows[..end]
}

/// Position at the decision. `mark_ticks` is the closed bar's close.
///
/// The engine freezes equity at the decision and sizes at the actual slipped
/// price of the next eligible 1m open with `count > 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookState {
    pub equity_ticks: i64,
    pub position_lots: i64,
    pub mark_ticks: i64,
}

/// Called once per closed 1h or 4h bar.
///
/// `None` is warmup. After the first `Some`, `None` or a mismatched
/// `bar_close_time_us` aborts the run. `book` is the position at this decision.
///
/// [`TargetSignal::on_warmup_bar`] runs for bars before the scored start.
/// The engine does not size, fill, fund, or score those bars. A stateful
/// signal updates lookback there and does not arm a hold.
pub trait TargetSignal {
    /// Additional delay after closed-bar availability (H4/H5 use 1 ms).
    fn decision_delay_us(&self) -> i64 {
        0
    }
    fn on_bar(
        &mut self,
        view: Causal<'_, &[ResampledBar]>,
        funding: Causal<'_, &[FundingObs]>,
        book: BookState,
    ) -> Option<SignalOutput>;

    /// Feature lookback before the scored start. The default reads nothing.
    fn on_warmup_bar(
        &mut self,
        _view: Causal<'_, &[ResampledBar]>,
        _funding: Causal<'_, &[FundingObs]>,
    ) {
    }
}
