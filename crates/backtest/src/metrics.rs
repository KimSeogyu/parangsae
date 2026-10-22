//! Daily returns, drawdown, and Sharpe. Integer returns in, one f64 function out.

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
/// The allow is the only float arithmetic in this crate.
#[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]
pub fn sharpe_f64(levels_ticks: &[i64]) -> Result<(Option<f64>, Option<f64>), SimError> {
    let returns = level_returns(levels_ticks)?;
    if returns.len() < 2 {
        return Ok((None, None));
    }
    let n = returns.len() as f64;
    let mut mean = 0.0_f64;
    for value in &returns {
        mean += *value as f64 / 1e18;
    }
    mean /= n;
    let mut variance = 0.0_f64;
    for value in &returns {
        let sample = *value as f64 / 1e18;
        let delta = sample - mean;
        variance += delta * delta;
    }
    variance /= n - 1.0;
    if variance == 0.0 {
        return Ok((None, None));
    }
    let std = variance.sqrt();
    let daily = mean / std;
    let annual = daily * 365.0_f64.sqrt();
    if daily.is_finite() && annual.is_finite() {
        Ok((Some(daily), Some(annual)))
    } else {
        Ok((None, None))
    }
}

#[cfg(test)]
mod tests {
    use super::div_half_even;

    #[test]
    fn half_even_resolves_ties_to_the_even_quotient() {
        assert_eq!(div_half_even(1, 2).unwrap(), 0);
        assert_eq!(div_half_even(3, 2).unwrap(), 2);
        assert_eq!(div_half_even(5, 2).unwrap(), 2);
        assert_eq!(div_half_even(-1, 2).unwrap(), 0);
        assert_eq!(div_half_even(-3, 2).unwrap(), -2);
        assert_eq!(div_half_even(-5, 2).unwrap(), -2);
    }
}
