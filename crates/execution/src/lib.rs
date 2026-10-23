//! Order lifecycle for paper execution.
//!
//! State changes go through [`transition`]. [`demo`] builds signed offline
//! query/cancel data without a transport. `crates/risk` is not
//! linked, so order dispatch and kill are absent.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::cast_precision_loss)]

mod authority;
mod book;
mod config;
pub mod demo;
mod error;
mod ids;
mod lifecycle;
mod paper;

pub use authority::{Elapsed, Mono, NotFoundAuthority};
pub use book::{Alert, BookError, HaltCause, HaltEpoch, HaltSink, OrderBook};
pub use config::{ApprovalTiming, ExecutionConfig, PositiveMicros};
pub use error::ConfigError;
pub use ids::{Attempt, ClientOrderId, IdError, IntentId, MaxAttempts};
pub use lifecycle::{
    transition, CellContext, Checkpoint, Event, OrderProgress, OrderState, ProgressError,
    TransitionError, ValidatedEvent,
};
pub use paper::{
    PaperAck, PaperOrder, PaperRefusal, PaperVenue, Reservation, SimKey, Venue, VenueError,
};
