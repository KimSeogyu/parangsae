//! Intent, attempt, and the client order id derived from both.
//!
//! The id is `e` + 26 uppercase base32 characters of the intent + `-` + 8
//! lowercase hex characters of the attempt. That is 36 ASCII bytes. A retry
//! after an authoritative NotFound uses the next attempt, so the id changes.

use std::fmt;

use crate::error::ConfigError;

/// Caller-supplied intent. The value is opaque and is not a hash of the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntentId(u128);

impl IntentId {
    pub const fn from_raw(raw: u128) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u128 {
        self.0
    }
}

/// How many attempts one intent may send. There is no `Default`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaxAttempts(u8);

impl MaxAttempts {
    pub fn new(attempts: u8) -> Result<Self, ConfigError> {
        if (1..=16).contains(&attempts) {
            Ok(Self(attempts))
        } else {
            Err(ConfigError::MaxAttemptsOutOfRange)
        }
    }

    /// Widest budget the client-order-id text can carry. Attempt indexes are `0..=15`.
    const ENCODED_DOMAIN: Self = Self(16);

    pub const fn get(self) -> u8 {
        self.0
    }

    /// The last attempt index this budget allows. The first index is zero.
    pub const fn last_attempt(self) -> Attempt {
        Attempt(self.0 - 1)
    }
}

/// Zero-based attempt index. Constructed only by [`Attempt::FIRST`],
/// [`Attempt::next`], [`Attempt::from_index`], and [`MaxAttempts::last_attempt`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Attempt(u8);

impl Attempt {
    pub const FIRST: Self = Self(0);

    pub const fn index(self) -> u8 {
        self.0
    }

    pub fn from_index(index: u8, max: MaxAttempts) -> Option<Self> {
        if index < max.0 {
            Some(Self(index))
        } else {
            None
        }
    }

    /// The next attempt, if it is still inside `max`.
    pub fn next(self, max: MaxAttempts) -> Option<Self> {
        let next = self.0.checked_add(1)?;
        if next < max.0 {
            Some(Self(next))
        } else {
            None
        }
    }
}

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const CID_LEN: usize = 36;

/// Deterministic client order id. The same pair always encodes the same text,
/// and distinct pairs encode distinct text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClientOrderId {
    raw: String,
    intent: IntentId,
    attempt: Attempt,
}

impl ClientOrderId {
    pub fn from_parts(intent: IntentId, attempt: Attempt) -> Self {
        let body = encode_intent(intent.raw());
        let bits = attempt_bits(attempt);
        let raw = format!("e{body}-{bits:08x}");
        debug_assert_eq!(raw.len(), CID_LEN);
        Self {
            raw,
            intent,
            attempt,
        }
    }

    pub fn parse(text: &str) -> Result<Self, IdError> {
        let bytes = text.as_bytes();
        if bytes.len() != CID_LEN {
            return Err(IdError::Length);
        }
        if bytes[0] != b'e' || bytes[27] != b'-' {
            return Err(IdError::Charset);
        }
        let intent_raw = decode_intent(&bytes[1..27])?;
        let attempt_index = decode_attempt_hex(&bytes[28..36])?;
        let attempt = Attempt::from_index(attempt_index, MaxAttempts::ENCODED_DOMAIN)
            .ok_or(IdError::AttemptOutOfRange)?;
        let intent = IntentId::from_raw(intent_raw);
        let canonical = Self::from_parts(intent, attempt);
        if canonical.raw != text {
            return Err(IdError::NotCanonical);
        }
        Ok(canonical)
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub const fn intent(&self) -> IntentId {
        self.intent
    }

    pub const fn attempt(&self) -> Attempt {
        self.attempt
    }
}

impl fmt::Display for ClientOrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdError {
    Length,
    Charset,
    Padding,
    AttemptOutOfRange,
    NotCanonical,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => write!(f, "client order id length is not 36"),
            Self::Charset => write!(f, "client order id has a character outside the encoding"),
            Self::Padding => write!(f, "client order id padding bits are not zero"),
            Self::AttemptOutOfRange => write!(f, "attempt index is outside 0..=15"),
            Self::NotCanonical => write!(f, "client order id is not the canonical encoding"),
        }
    }
}

impl std::error::Error for IdError {}

/// Full attempt index, written as 8 lowercase hex digits.
fn attempt_bits(attempt: Attempt) -> u8 {
    attempt.index()
}

/// 26 base32 symbols. 26 × 5 = 130 bits, so two zero pad bits sit above the u128.
fn encode_intent(intent: u128) -> String {
    let mut out = String::with_capacity(26);
    for symbol in 0..26 {
        let start = 129 - 5 * symbol;
        let mut value = 0u8;
        for step in 0..5 {
            let pos = start - step;
            let bit = u8::from(pos < 128 && ((intent >> pos) & 1) == 1);
            value = (value << 1) | bit;
        }
        out.push(char::from(BASE32[usize::from(value)]));
    }
    out
}

fn decode_intent(body: &[u8]) -> Result<u128, IdError> {
    if body.len() != 26 {
        return Err(IdError::Length);
    }
    let mut intent = 0u128;
    for (symbol, byte) in body.iter().copied().enumerate() {
        let value = base32_value(byte).ok_or(IdError::Charset)?;
        let start = 129 - 5 * symbol;
        for step in 0..5 {
            let pos = start - step;
            let bit = (value >> (4 - step)) & 1;
            if pos >= 128 {
                if bit != 0 {
                    return Err(IdError::Padding);
                }
            } else if bit == 1 {
                intent |= 1u128 << pos;
            }
        }
    }
    Ok(intent)
}

fn decode_attempt_hex(hex: &[u8]) -> Result<u8, IdError> {
    if hex.len() != 8 {
        return Err(IdError::Length);
    }
    let mut value = 0u32;
    for byte in hex {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            _ => return Err(IdError::Charset),
        };
        value = value
            .checked_mul(16)
            .and_then(|acc| acc.checked_add(u32::from(digit)))
            .ok_or(IdError::AttemptOutOfRange)?;
    }
    u8::try_from(value).map_err(|_| IdError::AttemptOutOfRange)
}

fn base32_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'2'..=b'7' => Some(byte - b'2' + 26),
        _ => None,
    }
}
