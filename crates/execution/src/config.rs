//! Execution configuration that is known before any order exists.
//!
//! `ExecutionConfig::new` takes the approval age the risk limit would report.
//! This crate does not depend on `crates/risk`; the caller passes that age in.

use crate::authority::NotFoundAuthority;
use crate::error::ConfigError;
use crate::ids::MaxAttempts;

/// A non-zero duration in microseconds. Zero is rejected, not substituted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositiveMicros(u64);

impl PositiveMicros {
    pub fn new(micros: u64) -> Result<Self, ConfigError> {
        if micros == 0 {
            Err(ConfigError::NonPositiveMicros)
        } else {
            Ok(Self(micros))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Approval lifetime must be strictly longer than the dispatch-latency budget.
///
/// A reapproval is dispatched in the same `submit` that receives it, so the
/// internal gap is zero. `max_dispatch_latency` bounds only the venue round trip.
/// That submit path is not in this crate until risk exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApprovalTiming {
    max_approval_age: PositiveMicros,
    max_dispatch_latency: PositiveMicros,
}

impl ApprovalTiming {
    pub fn new(
        max_approval_age: PositiveMicros,
        max_dispatch_latency: PositiveMicros,
    ) -> Result<Self, ConfigError> {
        if max_approval_age.get() > max_dispatch_latency.get() {
            Ok(Self {
                max_approval_age,
                max_dispatch_latency,
            })
        } else {
            Err(ConfigError::ApprovalTtlNotAboveDispatchLatency)
        }
    }

    pub const fn max_approval_age(self) -> PositiveMicros {
        self.max_approval_age
    }

    pub const fn max_dispatch_latency(self) -> PositiveMicros {
        self.max_dispatch_latency
    }
}

/// Static execution bounds. Startup fails when the risk approval age disagrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionConfig {
    timing: ApprovalTiming,
    authority: NotFoundAuthority,
    max_attempts: MaxAttempts,
}

impl ExecutionConfig {
    /// `risk_max_approval_age` is the value `Limits::max_approval_age()` returns.
    /// A mismatch is [`ConfigError::ApprovalAgeMismatch`], so a book is not built.
    pub fn new(
        timing: ApprovalTiming,
        authority: NotFoundAuthority,
        max_attempts: MaxAttempts,
        risk_max_approval_age: PositiveMicros,
    ) -> Result<Self, ConfigError> {
        if risk_max_approval_age.get() != timing.max_approval_age().get() {
            return Err(ConfigError::ApprovalAgeMismatch);
        }
        Ok(Self {
            timing,
            authority,
            max_attempts,
        })
    }

    pub const fn timing(self) -> ApprovalTiming {
        self.timing
    }

    pub const fn authority(self) -> NotFoundAuthority {
        self.authority
    }

    pub const fn max_attempts(self) -> MaxAttempts {
        self.max_attempts
    }
}
