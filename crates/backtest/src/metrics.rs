//! Daily returns, drawdown, and Sharpe. Integer returns in, one f64 function out.

use std::ops::{Add, Div, Mul, Sub};

use parangsae_core::{Ratio, SCALE};
use parangsae_sim::{round_against_trader, Cash, SimError};

/// Half-even division. Ties resolve to the even quotient.
pub(crate) fn div_half_even(num: i128, den: i128) -> Result<i128, SimError> {
    if den <= 0 {
        return Err(SimError::Overflow);
    }
    let quot = num.checked_div(den).ok_or(SimError::Overflow)?;
    let rem = num.checked_rem(den).ok_or(SimError::Overflow)?;
    if rem == 0 {
        return Ok(quot);
    }
    let rem_abs = rem.checked_abs().ok_or(SimError::Overflow)?;
    let twice = rem_abs.checked_mul(2).ok_or(SimError::Overflow)?;
    let sign = if num > 0 { 1 } else { -1 };
    if twice < den || (twice == den && quot.rem_euclid(2) == 0) {
        Ok(quot)
    } else {
        quot.checked_add(sign).ok_or(SimError::Overflow)
    }
}

pub(crate) fn level_returns(levels_ticks: &[i64]) -> Result<Vec<i128>, SimError> {
    let mut out = Vec::new();
    for pair in levels_ticks.windows(2) {
        let prev = i128::from(pair[0]);
        if prev == 0 {
            return Err(SimError::Overflow);
        }
        let next = i128::from(pair[1]);
        let num = (next - prev)
            .checked_mul(1_000_000_000_000_000_000)
            .ok_or(SimError::Overflow)?;
        out.push(div_half_even(num, prev)?);
    }
    Ok(out)
}

/// `(end - initial) * 1e8 / initial`, half-even.
pub(crate) fn total_return(initial: Cash, end: Cash) -> Result<Ratio, SimError> {
    if initial.0 <= 0 {
        return Err(SimError::Negative);
    }
    let num = (i128::from(end.0) - i128::from(initial.0))
        .checked_mul(SCALE)
        .ok_or(SimError::Overflow)?;
    let ticks = div_half_even(num, i128::from(initial.0))?;
    Ok(Ratio::from_ticks(
        i64::try_from(ticks).map_err(|_| SimError::Overflow)?,
    ))
}

/// `ceil((peak - equity) * 1e8 / peak)` over the per-bar curve. Never understates drawdown.
pub fn max_drawdown(equity: &[Cash]) -> Result<Ratio, SimError> {
    let mut peak = i128::MIN;
    let mut worst = 0i128;
    for point in equity {
        let value = i128::from(point.0);
        if value > peak {
            peak = value;
        }
        if peak > 0 && value < peak {
            let num = (peak - value)
                .checked_mul(SCALE)
                .ok_or(SimError::Overflow)?;
            let drawdown = round_against_trader(num, peak, true)?;
            if drawdown > worst {
                worst = drawdown;
            }
        }
    }
    Ok(Ratio::from_ticks(
        i64::try_from(worst).map_err(|_| SimError::Overflow)?,
    ))
}

/// Sharpe of consecutive level returns.
///
/// `levels_ticks[0]` is the baseline (initial cash). Each later value is an
/// end-of-day equity. Fewer than two returns, a zero baseline, or a zero
/// sample standard deviation yields `None` rather than 0 or infinity.
///
pub fn sharpe_f64(levels_ticks: &[i64]) -> Result<(Option<f64>, Option<f64>), SimError> {
    let returns = level_returns(levels_ticks)?;
    if returns.len() < 2 {
        return Ok((None, None));
    }
    let n = f64_from_i128(i128::from(
        u32::try_from(returns.len()).map_err(|_| SimError::Overflow)?,
    ))?;
    let mut mean = 0.0_f64;
    for value in &returns {
        let sample = fdiv(f64_from_i128(*value)?, 1e18);
        mean = fadd(mean, sample);
    }
    mean = fdiv(mean, n);
    let mut variance = 0.0_f64;
    for value in &returns {
        let sample = fdiv(f64_from_i128(*value)?, 1e18);
        let delta = fsub(sample, mean);
        variance = fadd(variance, fmul(delta, delta));
    }
    variance = fdiv(variance, fsub(n, 1.0));
    if variance == 0.0 {
        return Ok((None, None));
    }
    let std = variance.sqrt();
    let daily = fdiv(mean, std);
    let annual = fmul(daily, 365.0_f64.sqrt());
    if daily.is_finite() && annual.is_finite() {
        Ok((Some(daily), Some(annual)))
    } else {
        Ok((None, None))
    }
}

fn fadd(left: f64, right: f64) -> f64 {
    <f64 as Add>::add(left, right)
}

fn fsub(left: f64, right: f64) -> f64 {
    <f64 as Sub>::sub(left, right)
}

fn fmul(left: f64, right: f64) -> f64 {
    <f64 as Mul>::mul(left, right)
}

fn fdiv(left: f64, right: f64) -> f64 {
    <f64 as Div>::div(left, right)
}

/// IEEE-754 round-to-nearest, ties to even. Same bits as an `as f64` cast.
fn f64_from_i128(value: i128) -> Result<f64, SimError> {
    let bits = f64_bits(value.unsigned_abs(), value < 0)?;
    Ok(f64::from_bits(bits))
}

fn f64_bits(mag: u128, sign: bool) -> Result<u64, SimError> {
    if mag == 0 {
        return Ok(0);
    }
    let shift = mag.ilog2();
    let mut exponent = u64::from(shift) + 1023;
    let (mut mantissa, round, sticky) = if shift <= 52 {
        (mag << (52 - shift), 0u128, 0u128)
    } else {
        let drop = shift - 52;
        let mantissa = mag >> drop;
        let round = (mag >> (drop - 1)) & 1;
        let sticky = if drop == 1 {
            0
        } else {
            mag & ((1u128 << (drop - 1)) - 1)
        };
        (mantissa, round, sticky)
    };
    if round == 1 && (sticky != 0 || (mantissa & 1) == 1) {
        mantissa = mantissa.saturating_add(1);
    }
    if mantissa >= (1u128 << 53) {
        mantissa >>= 1;
        exponent = exponent.saturating_add(1);
    }
    mantissa &= (1u128 << 52) - 1;
    let sign_bit = if sign { 1u64 << 63 } else { 0 };
    let mantissa_bits = u64::try_from(mantissa).map_err(|_| SimError::Overflow)?;
    Ok(sign_bit | (exponent << 52) | mantissa_bits)
}

#[cfg(test)]
mod tests {
    use super::div_half_even;

    #[test]
    fn half_even_resolves_ties_to_the_even_quotient() -> Result<(), String> {
        assert_eq!(div_half_even(1, 2).map_err(|err| err.to_string())?, 0);
        assert_eq!(div_half_even(3, 2).map_err(|err| err.to_string())?, 2);
        assert_eq!(div_half_even(5, 2).map_err(|err| err.to_string())?, 2);
        assert_eq!(div_half_even(-1, 2).map_err(|err| err.to_string())?, 0);
        assert_eq!(div_half_even(-3, 2).map_err(|err| err.to_string())?, -2);
        assert_eq!(div_half_even(-5, 2).map_err(|err| err.to_string())?, -2);
        Ok(())
    }
}
