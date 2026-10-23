//! Single-order book used to halt on an illegal cell.
//!
//! Dispatch, reconciliation, and kill wait on `crates/risk`. This book applies
//! [`transition`](crate::transition) and, on failure or on entry to `Unknown`,
//! reports the cause through [`HaltSink`]. Nothing here clears the latch.

use crate::lifecycle::{
    transition, CellContext, Event, OrderProgress, OrderState, TransitionError, ValidatedEvent,
};

/// How many alerts the book retains. Further alerts still latch; they are not evicted.
const MAX_ALERTS: usize = 64;

/// Halt generation. The sink returns the new value. This crate does not reset it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HaltEpoch(u64);

impl HaltEpoch {
    pub const ZERO: Self = Self(0);

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

/// Causes the book can report. Commit 2 passes the same set to `RiskGate::latch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HaltCause {
    ReconMismatch,
    UnknownOrder,
    Kill,
    Ban,
    RiskTrip,
    IllegalTransition,
    PayloadError,
}

/// Execution-local halt port. Commit 2 binds this to `RiskGate::latch`.
pub trait HaltSink {
    fn latch(&mut self, cause: HaltCause) -> HaltEpoch;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub cause: HaltCause,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookError {
    Halted(TransitionError),
}

impl std::fmt::Display for BookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Halted(error) => write!(f, "book halted: {error}"),
        }
    }
}

impl std::error::Error for BookError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderBook {
    progress: OrderProgress,
    halted: bool,
    alerts: Vec<Alert>,
    epoch: HaltEpoch,
}

impl OrderBook {
    pub fn new(progress: OrderProgress) -> Self {
        Self {
            progress,
            halted: false,
            alerts: Vec::new(),
            epoch: HaltEpoch::ZERO,
        }
    }

    pub fn progress(&self) -> &OrderProgress {
        &self.progress
    }

    pub const fn is_halted(&self) -> bool {
        self.halted
    }

    pub fn alerts(&self) -> &[Alert] {
        &self.alerts
    }

    pub const fn epoch(&self) -> HaltEpoch {
        self.epoch
    }

    pub fn apply<S: HaltSink>(
        &mut self,
        event: &ValidatedEvent,
        cx: &CellContext<'_>,
        sink: &mut S,
    ) -> Result<(), BookError> {
        let before = self.progress.clone();
        match transition(&before, event, cx) {
            Ok(next) => {
                let entered_unknown =
                    before.state() != OrderState::Unknown && next.state() == OrderState::Unknown;
                let timeout = event.kind() == Event::Timeout;
                self.progress = next;
                if timeout || entered_unknown {
                    self.latch(HaltCause::UnknownOrder, sink);
                }
                Ok(())
            }
            Err(error) => {
                self.latch(cause_of(error), sink);
                Err(BookError::Halted(error))
            }
        }
    }

    fn latch<S: HaltSink>(&mut self, cause: HaltCause, sink: &mut S) {
        self.epoch = sink.latch(cause);
        self.halted = true;
        if self.alerts.len() < MAX_ALERTS {
            self.alerts.push(Alert { cause });
        }
    }
}

fn cause_of(error: TransitionError) -> HaltCause {
    match error {
        TransitionError::IllegalTransition { .. } => HaltCause::IllegalTransition,
        TransitionError::Overfill
        | TransitionError::ReportConflict
        | TransitionError::MissingEvidence
        | TransitionError::InvalidQuantity
        | TransitionError::InconsistentQuery => HaltCause::PayloadError,
    }
}
