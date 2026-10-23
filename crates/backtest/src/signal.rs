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

/// Called once per closed 1h or 4h bar.
///
/// `None` is warmup. After the first `Some`, `None` or a mismatched
/// `bar_close_time_us` aborts the run.
pub trait TargetSignal {
    fn on_bar(
        &mut self,
        view: Causal<'_, &[ResampledBar]>,
        funding: Causal<'_, &[FundingObs]>,
    ) -> Option<SignalOutput>;
}
