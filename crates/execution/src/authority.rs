//! Monotonic clock and the NotFound authority bound.
//!
//! `Mono` is injected by the caller. This module does not read the operating
//! system clock. A NotFound is authoritative only when the elapsed time since
//! the last send of that client order id is strictly greater than the bound.

use crate::error::ConfigError;

pub use parangsae_risk::Mono;

/// Microseconds since the last send of one client order id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Elapsed(u64);

impl Elapsed {
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    pub const fn us(self) -> u64 {
        self.0
    }
}

/// `recvWindow + margin`, in milliseconds, checked into microseconds.
///
/// `recv_window_ms` is the window a future adapter would send. v0 sends
/// nothing, so the value is configuration only. The margin covers clock skew:
/// once `elapsed > bound`, no NEW for that id is still in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotFoundAuthority {
    recv_window_ms: u32,
    margin_ms: u32,
    bound_us: u64,
}

impl NotFoundAuthority {
    pub fn new(recv_window_ms: u32, margin_ms: u32) -> Result<Self, ConfigError> {
        if !(1..=60_000).contains(&recv_window_ms) {
            return Err(ConfigError::RecvWindowOutOfRange);
        }
        if margin_ms < 1 {
            return Err(ConfigError::MarginOutOfRange);
        }
        let sum_ms = recv_window_ms
            .checked_add(margin_ms)
            .ok_or(ConfigError::BoundOverflow)?;
        let bound_us = u64::from(sum_ms)
            .checked_mul(1_000)
            .ok_or(ConfigError::BoundOverflow)?;
        Ok(Self {
            recv_window_ms,
            margin_ms,
            bound_us,
        })
    }

    pub const fn recv_window_ms(self) -> u32 {
        self.recv_window_ms
    }

    pub const fn margin_ms(self) -> u32 {
        self.margin_ms
    }

    pub const fn bound_us(self) -> u64 {
        self.bound_us
    }

    /// Authoritative when the elapsed time is strictly past the bound.
    pub const fn is_authoritative(self, since_last_send: Elapsed) -> bool {
        since_last_send.us() > self.bound_us
    }
}
