//! The clocks on one observation, and the receipt that says how it was obtained.
//!
//! * `event_ts` — the exchange event time.
//! * `available_ts` — the earliest time a signal may use the value.
//! * `receipt` — an archive ingest run, or the live receive time.
//!
//! An archive receipt has no timestamp. Archive rows are often fetched long
//! after `available_ts`, so this module does not order the receipt against the
//! two clocks.

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Timestamps {
    pub event_ts: EventTs,
    pub available_ts: AvailableTs,
    pub receipt: Receipt,
}

/// Opaque id of one collector ingest run.
///
/// 128 bits from the OS RNG, shown as 32 lowercase hex characters. It is not
/// time-derived: no ULID, no UUIDv7, and no conversion to a time type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId([u8; 16]);

/// `parse_hex` rejected a string that is not 32 lowercase hex characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunIdError {
    /// The string is not exactly 32 characters.
    Length,
    /// A hex letter was uppercase. Only lowercase is accepted.
    Uppercase,
    /// A character is not a hex digit.
    Digit,
}

impl fmt::Display for RunIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => write!(f, "run id must be 32 lowercase hex characters"),
            Self::Uppercase => write!(f, "run id hex must be lowercase"),
            Self::Digit => write!(f, "run id contains a non-hex character"),
        }
    }
}

impl std::error::Error for RunIdError {}

impl RunId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Exactly 32 lowercase hex characters. Uppercase hex is an error.
    pub fn parse_hex(text: &str) -> Result<Self, RunIdError> {
        let bytes = text.as_bytes();
        if bytes.len() != 32 {
            return Err(RunIdError::Length);
        }
        let mut out = [0u8; 16];
        for index in 0..16 {
            let hi = hex_nibble(bytes[index * 2])?;
            let lo = hex_nibble(bytes[index * 2 + 1])?;
            out[index] = (hi << 4) | lo;
        }
        Ok(Self(out))
    }

    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(32);
        for byte in self.0 {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        out
    }
}

fn hex_nibble(byte: u8) -> Result<u8, RunIdError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Err(RunIdError::Uppercase),
        _ => Err(RunIdError::Digit),
    }
}

/// How a row was obtained.
///
/// `Archive` names the ingest run and has no timestamp. `Live` names the time
/// this process received the row. There is no accessor that turns an archive
/// receipt into a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Receipt {
    Archive { fetched_at_run: RunId },
    Live { received_ts: UtcMicros },
}

impl Receipt {
    /// Only live receipts expose a time. An archive receipt is `None`.
    pub const fn live_received_ts(&self) -> Option<UtcMicros> {
        match self {
            Self::Live { received_ts } => Some(*received_ts),
            Self::Archive { .. } => None,
        }
    }

    /// The ingest run, only for an archive receipt.
    pub const fn archive_run(&self) -> Option<RunId> {
        match self {
            Self::Archive { fetched_at_run } => Some(*fetched_at_run),
            Self::Live { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_receipt_exposes_no_time() -> Result<(), RunIdError> {
        let run = RunId::parse_hex("0123456789abcdef0123456789abcdef")?;
        let archive = Receipt::Archive {
            fetched_at_run: run,
        };
        assert!(archive.live_received_ts().is_none());
        assert_eq!(archive.archive_run(), Some(run));
        assert_eq!(run.to_hex(), "0123456789abcdef0123456789abcdef");
        let live = Receipt::Live {
            received_ts: UtcMicros::from_micros(5),
        };
        assert_eq!(live.live_received_ts(), Some(UtcMicros::from_micros(5)));
        assert!(live.archive_run().is_none());
        Ok(())
    }

    #[test]
    fn parse_hex_rejects_uppercase_and_the_wrong_length() {
        assert_eq!(
            RunId::parse_hex("0123456789ABCDEF0123456789abcdef"),
            Err(RunIdError::Uppercase)
        );
        assert_eq!(RunId::parse_hex("abc"), Err(RunIdError::Length));
        assert_eq!(
            RunId::parse_hex("0123456789abcdef0123456789abcdeg"),
            Err(RunIdError::Digit)
        );
    }
}
