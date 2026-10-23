//! Saturating position in `1e8` ticks.
//!
//! NaN and infinity are [`TargetError::NonFinite`]. They are never clamped and
//! never become zero. The clamp applies only to finite values: it is the
//! formula's saturating map, not a hide for a bad input.
//!
//! `from_f64` totality is sampled by proptest. That test is not a proof.

use libm::trunc;

/// 1.0 position, in ticks.
pub const TARGET_SCALE: i64 = 100_000_000;

const TARGET_SCALE_F: f64 = 100_000_000.0;

/// A position in `[-1, 1]`. The only constructors are [`Target::from_f64`] and
/// [`Target::FLAT`]. There is no `From<f64>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Target(i64);

impl Target {
    pub const FLAT: Self = Self(0);

    pub fn from_f64(value: f64) -> Result<Self, TargetError> {
        if !value.is_finite() {
            return Err(TargetError::NonFinite);
        }
        let clamped = value.clamp(-1.0, 1.0);
        let truncated = trunc(clamped * TARGET_SCALE_F);
        let ticks = trunc_to_i64(truncated)?;
        if !(-TARGET_SCALE..=TARGET_SCALE).contains(&ticks) {
            return Err(TargetError::NonFinite);
        }
        Ok(Self(ticks))
    }

    pub const fn ticks(self) -> i64 {
        self.0
    }
}

/// Convert an already-truncated finite f64 into i64 without a float cast.
///
/// The value is integral and, for [`Target::from_f64`], inside `[-1e8, 1e8]`.
fn trunc_to_i64(value: f64) -> Result<i64, TargetError> {
    let bits = value.to_bits();
    let negative = (bits >> 63) == 1;
    let exponent = match i32::try_from((bits >> 52) & 0x7ff) {
        Ok(exponent) => exponent,
        Err(_) => return Err(TargetError::NonFinite),
    };
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent == 0 {
        return Ok(0);
    }
    if exponent == 0x7ff {
        return Err(TargetError::NonFinite);
    }
    let unbiased = exponent - 1023;
    if unbiased < 0 {
        return Ok(0);
    }
    if unbiased > 62 {
        return Err(TargetError::NonFinite);
    }
    let shift = match u32::try_from(unbiased) {
        Ok(shift) => shift,
        Err(_) => return Err(TargetError::NonFinite),
    };
    let mantissa = (1_u64 << 52) | fraction;
    let magnitude = if shift >= 52 {
        mantissa << (shift - 52)
    } else {
        mantissa >> (52 - shift)
    };
    let ticks = i64::try_from(magnitude).map_err(|_| TargetError::NonFinite)?;
    if negative {
        Ok(-ticks)
    } else {
        Ok(ticks)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetError {
    NonFinite,
}

impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite => write!(f, "target is not finite"),
        }
    }
}
