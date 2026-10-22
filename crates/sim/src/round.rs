//! Rounding that never favors the trader.

use crate::types::SimError;

/// Divide `num / den`.
///
/// When `pays` is set, the trader pays and the magnitude rounds away from zero.
/// When `pays` is clear, the trader receives and the magnitude rounds toward zero.
/// `den` must be positive.
pub fn round_against_trader(num: i128, den: i128, pays: bool) -> Result<i128, SimError> {
    if den <= 0 {
        return Err(SimError::Overflow);
    }
    let quot = num.checked_div(den).ok_or(SimError::Overflow)?;
    let rem = num.checked_rem(den).ok_or(SimError::Overflow)?;
    if rem == 0 || !pays {
        return Ok(quot);
    }
    let step = if num > 0 { 1 } else { -1 };
    quot.checked_add(step).ok_or(SimError::Overflow)
}

/// Truncate toward negative infinity. Used for mark-to-market.
pub(crate) fn div_floor(num: i128, den: i128) -> Result<i128, SimError> {
    if den <= 0 {
        return Err(SimError::Overflow);
    }
    let quot = num.checked_div(den).ok_or(SimError::Overflow)?;
    let rem = num.checked_rem(den).ok_or(SimError::Overflow)?;
    if rem != 0 && num < 0 {
        quot.checked_sub(1).ok_or(SimError::Overflow)
    } else {
        Ok(quot)
    }
}

pub(crate) fn fit_i64(value: i128) -> Result<i64, SimError> {
    i64::try_from(value).map_err(|_| SimError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::round_against_trader;

    #[test]
    fn pays_away_from_zero_and_receipts_toward_zero() -> Result<(), String> {
        assert_eq!(
            round_against_trader(1, 2, true).map_err(|err| err.to_string())?,
            1
        );
        assert_eq!(
            round_against_trader(1, 2, false).map_err(|err| err.to_string())?,
            0
        );
        assert_eq!(
            round_against_trader(-1, 2, true).map_err(|err| err.to_string())?,
            -1
        );
        assert_eq!(
            round_against_trader(-1, 2, false).map_err(|err| err.to_string())?,
            0
        );
        assert_eq!(
            round_against_trader(-3, 2, true).map_err(|err| err.to_string())?,
            -2
        );
        assert_eq!(
            round_against_trader(-3, 2, false).map_err(|err| err.to_string())?,
            -1
        );
        assert_eq!(
            round_against_trader(4, 2, true).map_err(|err| err.to_string())?,
            2
        );
        assert_eq!(
            round_against_trader(5, 2, false).map_err(|err| err.to_string())?,
            2
        );
        Ok(())
    }
}
