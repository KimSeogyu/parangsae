//! The paper clock. This crate does not submit an order.
//!
//! `decision_at` is the only public mint of [`DecisionTime`]. The `driver-clock`
//! feature on `parangsae-core` is enabled by this crate's normal dependency
//! and by no other one.

use parangsae_core::{DecisionTime, UtcMicros};

/// Mints a decision time. Downstream crates call this instead of `from_micros`.
pub struct Driver;

impl Driver {
    pub const fn decision_at(ts: UtcMicros) -> DecisionTime {
        DecisionTime::from_micros(ts)
    }
}
