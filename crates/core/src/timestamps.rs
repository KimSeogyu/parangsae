//! The three clocks on one observation.
//!
//! * `event_ts` — the exchange event time.
//! * `available_ts` — the earliest time a signal may use the value.
//! * `received_ts` — when this process actually received it.
//!
//! The data-plan name `ingest_ts` is `received_ts` here. Archive rows are often
//! received long after `available_ts`, so this module does not order the three.

use crate::time::UtcMicros;
use std::fmt;

macro_rules! ts_newtype {
    ($name:ident, $label:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(UtcMicros);

        impl $name {
            pub const fn from_micros(ts: UtcMicros) -> Self {
                Self(ts)
            }

            pub const fn micros(self) -> UtcMicros {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($label, "={}"), self.0)
            }
        }
    };
}

ts_newtype!(EventTs, "event_ts");
ts_newtype!(AvailableTs, "available_ts");
ts_newtype!(ReceivedTs, "received_ts");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Timestamps {
    pub event_ts: EventTs,
    pub available_ts: AvailableTs,
    pub received_ts: ReceivedTs,
}
