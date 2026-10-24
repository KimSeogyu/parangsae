//! Order lifecycle for paper execution.
//!
//! State changes go through [`transition`]. [`demo`] builds signed offline
//! query/cancel data without a transport. The offline paper engine binds the
//! actual RiskGate to lifecycle and fixed-point accounting.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::cast_precision_loss)]

mod authority;
mod book;
pub mod carry;
mod config;
pub mod demo;
mod error;
mod ids;
mod lifecycle;
mod paper;
mod paper_event;

pub use authority::{Elapsed, Mono, NotFoundAuthority};
pub use book::{Alert, BookError, HaltCause, HaltEpoch, HaltSink, OrderBook};
pub use config::{ApprovalTiming, ExecutionConfig, PositiveMicros};
pub use error::ConfigError;
pub use ids::{Attempt, ClientOrderId, IdError, IntentId, MaxAttempts};
pub use lifecycle::{
    transition, CellContext, Checkpoint, Event, OrderProgress, OrderState, ProgressError,
    TransitionError, ValidatedEvent,
};
pub use paper::{PaperEngine, PaperError, PaperOutcome, PaperSession, PaperSnapshot};
pub use paper_event::{
    PaperAction, PaperCommand, PaperFill, PaperInputError, PaperOrderSnapshot, PaperReport,
    PaperTerminal, PaperTerminalCut,
};
