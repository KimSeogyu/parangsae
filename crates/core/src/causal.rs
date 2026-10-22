//! Type-level lookahead gate.
//!
//! [`Causal`] has private fields and no public constructor. The only way to
//! obtain one is [`Observation::causal_at`], which returns `Some` only when
//! `available_ts <= decision`. Signal code takes `Causal` and therefore cannot
//! name a row that is not yet available.

use crate::time::UtcMicros;
use crate::timestamps::{AvailableTs, Timestamps};

/// Time at which a signal is evaluated. Compared with `available_ts` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DecisionTime(UtcMicros);

impl DecisionTime {
    pub const fn from_micros(ts: UtcMicros) -> Self {
        Self(ts)
    }

    pub const fn micros(self) -> UtcMicros {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation<T> {
    timestamps: Timestamps,
    value: T,
}

impl<T> Observation<T> {
    pub const fn new(timestamps: Timestamps, value: T) -> Self {
        Self { timestamps, value }
    }

    pub const fn timestamps(&self) -> Timestamps {
        self.timestamps
    }

    pub const fn value(&self) -> &T {
        &self.value
    }

    pub const fn with_value<U>(&self, value: U) -> Observation<U> {
        Observation {
            timestamps: self.timestamps,
            value,
        }
    }

    pub fn causal_at(&self, decision: DecisionTime) -> Option<Causal<'_, T>> {
        if self.timestamps.available_ts.micros() <= decision.micros() {
            Some(Causal {
                available_ts: self.timestamps.available_ts,
                value: &self.value,
            })
        } else {
            None
        }
    }
}

/// A value a signal is allowed to read at the decision time that produced it.
#[derive(Debug, PartialEq, Eq)]
pub struct Causal<'a, T> {
    available_ts: AvailableTs,
    value: &'a T,
}

impl<'a, T> Causal<'a, T> {
    pub const fn available_ts(&self) -> AvailableTs {
        self.available_ts
    }

    pub const fn value(&self) -> &'a T {
        self.value
    }
}
