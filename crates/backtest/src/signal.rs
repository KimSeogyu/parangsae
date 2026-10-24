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

/// The longest stored prefix whose rows are all available by `decision`.
/// Funding-time order does not imply availability order when a row is delayed.
pub fn funding_visible(rows: &[FundingObs], decision: UtcMicros) -> &[FundingObs] {
    let end = rows
        .iter()
        .position(|row| row.available > decision)
        .unwrap_or(rows.len());
    &rows[..end]
}

/// A read-only engine-created decision book. Money is admitted before construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookState {
    money: crate::cash_view::VisibleMoney,
    position_lots: i64,
    mark_ticks: i64,
    execution: ExecutionPhase,
}

impl BookState {
    pub(crate) fn new(
        money: crate::cash_view::VisibleMoney,
        position_lots: i64,
        mark_ticks: i64,
        execution: ExecutionPhase,
    ) -> Self {
        Self {
            money,
            position_lots,
            mark_ticks,
            execution,
        }
    }
    pub fn equity_ticks(self) -> i64 {
        self.money.equity().ticks()
    }
    pub fn position_lots(self) -> i64 {
        self.position_lots
    }
    pub fn mark_ticks(self) -> i64 {
        self.mark_ticks
    }
    pub fn execution(self) -> ExecutionPhase {
        self.execution
    }
}

pub use parangsae_strategies::{
    ActualHold, ExecutionDirective, ExecutionPhase, H4Event, H4Observation, PositionSide,
    StrategyOutput,
};

/// Called once per closed 1h or 4h bar.
///
/// `None` is warmup. After the first `Some`, `None` or a mismatched
/// `bar_close_time_us` aborts the run. `book` is the position at this decision.
///
/// [`TargetSignal::on_warmup_bar`] runs for bars before the scored start.
/// The engine does not size, fill, fund, or score those bars. A stateful
/// signal updates lookback there and does not arm a hold.
pub trait TargetSignal {
    fn take_h4_observations(&mut self) -> Vec<H4Observation> {
        Vec::new()
    }
    fn h4_observation_capacity(&self) -> usize {
        0
    }

    /// Additional delay after closed-bar availability (H4/H5 use 1 ms).
    fn decision_delay_us(&self) -> i64 {
        0
    }
    fn on_bar(
        &mut self,
        view: Causal<'_, &[ResampledBar]>,
        funding: Causal<'_, &[FundingObs]>,
        book: BookState,
    ) -> Option<StrategyOutput>;

    /// Feature lookback before the scored start. The default reads nothing.
    fn on_warmup_bar(
        &mut self,
        _view: Causal<'_, &[ResampledBar]>,
        _funding: Causal<'_, &[FundingObs]>,
    ) {
    }
}
