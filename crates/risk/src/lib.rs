//! Pure risk authority for one BTCUSDT paper session.
//!
//! The gate owns approvals and reservations. Execution owns accounting and
//! lifecycle state and commits checked candidates together. Nothing here opens
//! files, reads a clock, or contacts an exchange. The bounded RiskGate TLA+
//! model is design evidence, not a proof of this runtime implementation.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::cast_precision_loss)]

mod gate;
mod types;

pub use gate::{
    upper_notional, ApprovedOrder, DispatchDecision, PreparedDispatch, PreparedSettlement,
    RiskGate, SettlementOutcome, SpentApproval,
};
pub use types::*;
