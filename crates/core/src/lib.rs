//! Domain types shared by the collector, signals, regime, and execution crates.
//!
//! This crate does no I/O. Binance payloads are parsed at the collector boundary
//! into these types. Signal code reads a [`Causal`] view, which exists only for
//! rows whose `available_ts` is at or before the decision time.

pub mod bar;
pub mod causal;
pub mod fixed;
pub mod instrument_spec;
pub mod symbol;
pub mod time;
pub mod timestamps;

pub use bar::{BarError, BarInterval, ResampledBar};
pub use causal::{Causal, DecisionTime, Observation};
pub use fixed::{notional, Amount, FixedError, Notional, Price, Qty, Ratio, SCALE};
pub use symbol::{Symbol, SymbolError};
pub use time::{
    expected_close, require_declared_unit, require_regular_grid, source_tick_us,
    validate_close_time, Interval, KlineSpan, TimeError, TimeUnit, UnitError, UtcMicros,
    MICROS_PER_MILLIS,
};
pub use timestamps::{AvailableTs, EventTs, ReceivedTs, Timestamps};
