//! Scale-`10^8` decimal. The same minor unit as [`crate::Price`]. Not `f64`.

use crate::{Price, Qty, SCALE};
use std::fmt;

/// Fixed-point decimal. One whole unit is [`Self::SCALE`] minor units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Decimal(i64);

impl Decimal {
    pub const SCALE: i64 = 100_000_000;
    pub const ZERO: Self = Self(0);

    pub const fn from_ticks(ticks: i64) -> Self {
        Self(ticks)
    }

    pub const fn ticks(self) -> i64 {
        self.0
    }

    pub fn as_price(self) -> Result<Price, DecimalError> {
        Price::from_ticks(self.0).map_err(DecimalError::from_fixed)
    }

    pub fn as_qty(self) -> Result<Qty, DecimalError> {
        Qty::from_ticks(self.0).map_err(DecimalError::from_fixed)
    }

    /// Plain base-10 literal. No exponent and no binary float.
    /// More than 8 fractional digits fail unless those digits are `0`.
    pub fn parse(raw: &str) -> Result<Self, DecimalError> {
        let text = raw.trim();
        if text.is_empty() {
            return Err(DecimalError::Empty);
        }
        let (neg, digits) = if let Some(rest) = text.strip_prefix('-') {
            if rest.is_empty() {
                return Err(DecimalError::Charset);
            }
            (true, rest)
        } else {
            (false, text)
        };
        if digits.starts_with('+') || digits.starts_with('-') {
            return Err(DecimalError::Charset);
        }
        let (int_part, frac_part) = match digits.split_once('.') {
            Some((int_part, frac_part)) => {
                if int_part.is_empty() || frac_part.is_empty() || frac_part.contains('.') {
                    return Err(DecimalError::Charset);
                }
                (int_part, frac_part)
            }
            None => (digits, ""),
        };
        if !int_part.bytes().all(|byte| byte.is_ascii_digit())
            || !frac_part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(DecimalError::Charset);
        }
        let frac_bytes = frac_part.as_bytes();
        if frac_bytes.len() > 8 && frac_bytes[8..].iter().any(|byte| *byte != b'0') {
            return Err(DecimalError::Scale);
        }
        let mut frac_pad = [b'0'; 8];
        let take = frac_bytes.len().min(8);
        frac_pad[..take].copy_from_slice(&frac_bytes[..take]);
        let mut frac_val: i128 = 0;
        for byte in frac_pad {
            let digit = byte.checked_sub(b'0').ok_or(DecimalError::Charset)?;
            frac_val = frac_val
                .checked_mul(10)
                .ok_or(DecimalError::Overflow)?
                .checked_add(i128::from(digit))
                .ok_or(DecimalError::Overflow)?;
        }
        let int_trim = int_part.trim_start_matches('0');
        let int_trim = if int_trim.is_empty() { "0" } else { int_trim };
        let int_val: i128 = int_trim.parse().map_err(|_| DecimalError::Overflow)?;
        let mut scaled = int_val
            .checked_mul(SCALE)
            .ok_or(DecimalError::Overflow)?
            .checked_add(frac_val)
            .ok_or(DecimalError::Overflow)?;
        if neg {
            scaled = scaled.checked_neg().ok_or(DecimalError::Overflow)?;
        }
        let ticks = i64::try_from(scaled).map_err(|_| DecimalError::Overflow)?;
        Ok(Self(ticks))
    }

    pub fn is_multiple_of(self, step: Self) -> bool {
        match self.0.checked_rem(step.0) {
            Some(0) => true,
            Some(_) | None => false,
        }
    }

    pub fn whole_units(self) -> Option<i64> {
        if self.0.checked_rem(Self::SCALE) != Some(0) {
            return None;
        }
        self.0.checked_div(Self::SCALE)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecimalError {
    Empty,
    Charset,
    Scale,
    Overflow,
    Negative,
}

impl DecimalError {
    fn from_fixed(err: crate::FixedError) -> Self {
        match err {
            crate::FixedError::Negative => Self::Negative,
            crate::FixedError::Overflow => Self::Overflow,
        }
    }
}

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty decimal"),
            Self::Charset => write!(f, "decimal is not a plain base-10 literal"),
            Self::Scale => write!(f, "more than 8 fractional digits"),
            Self::Overflow => write!(f, "decimal does not fit"),
            Self::Negative => write!(f, "negative decimal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rule_literals_and_rejects_floats() -> Result<(), DecimalError> {
        assert_eq!(Decimal::parse("0.01")?.ticks(), 1_000_000);
        assert_eq!(Decimal::parse("0.10")?.ticks(), 10_000_000);
        assert_eq!(Decimal::parse("0.0002")?.ticks(), 20_000);
        assert_eq!(Decimal::parse("0.0005")?.ticks(), 50_000);
        assert_eq!(Decimal::parse("81903.30")?.ticks(), 8_190_330_000_000);
        assert_eq!(Decimal::parse("1000")?.ticks(), 100_000_000_000);
        assert_eq!(Decimal::parse("1e-3"), Err(DecimalError::Charset));
        assert_eq!(Decimal::parse("1.000000009"), Err(DecimalError::Scale));
        assert_eq!(i128::from(Decimal::SCALE), SCALE);
        Ok(())
    }
}
