//! Daily returns, drawdown, turnover, and Sharpe. All of them are integer ratios.

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

/// Sharpe of consecutive level returns, in `10^8` ticks.
///
/// `levels_ticks[0]` is the baseline (initial cash). Each later value is an
/// end-of-day equity. Daily returns are `trunc((next - prev) * 1e8 / prev)`.
/// `S1 = Σr`, `S2 = Σr²`, `D = n·S2 − S1²`, all in `i128`.
/// `sharpe = sign(S1) · isqrt(S1² · (n−1) · 1e16 · factor / (n·D))`.
/// The annual factor is 365 inside the root. `n < 2` or `D = 0` is `None`.
/// A product that does not fit is [`SimError::Overflow`].
pub fn sharpe_ticks(levels_ticks: &[i64]) -> Result<(Option<Ratio>, Option<Ratio>), SimError> {
    let returns = daily_returns(levels_ticks)?;
    let n_us = returns.len();
    if n_us < 2 {
        return Ok((None, None));
    }
    let n = i128::try_from(n_us).map_err(|_| SimError::Overflow)?;
    let mut s1: i128 = 0;
    let mut s2: i128 = 0;
    for value in returns {
        s1 = s1.checked_add(value).ok_or(SimError::Overflow)?;
        s2 = s2
            .checked_add(value.checked_mul(value).ok_or(SimError::Overflow)?)
            .ok_or(SimError::Overflow)?;
    }
    let square = s1.checked_mul(s1).ok_or(SimError::Overflow)?;
    let d = n
        .checked_mul(s2)
        .and_then(|value| value.checked_sub(square))
        .ok_or(SimError::Overflow)?;
    if d == 0 {
        return Ok((None, None));
    }
    if d < 0 {
        return Err(SimError::Overflow);
    }
    let daily = sharpe_from_sums(s1, n, d, 1)?;
    let annual = sharpe_from_sums(s1, n, d, 365)?;
    Ok((Some(daily), Some(annual)))
}

fn daily_returns(levels_ticks: &[i64]) -> Result<Vec<i128>, SimError> {
    let mut out = Vec::new();
    for pair in levels_ticks.windows(2) {
        let prev = i128::from(pair[0]);
        if prev == 0 {
            return Err(SimError::Overflow);
        }
        let next = i128::from(pair[1]);
        let num = (next - prev).checked_mul(SCALE).ok_or(SimError::Overflow)?;
        out.push(num / prev);
    }
    Ok(out)
}

fn sharpe_from_sums(s1: i128, n: i128, d: i128, factor: i128) -> Result<Ratio, SimError> {
    let quotient = sharpe_quotient(s1, n, d, factor)?;
    let root = u128::try_from(quotient)
        .map_err(|_| SimError::Overflow)?
        .isqrt();
    let signed = if s1 < 0 {
        i128::try_from(root)
            .map_err(|_| SimError::Overflow)?
            .checked_neg()
            .ok_or(SimError::Overflow)?
    } else {
        i128::try_from(root).map_err(|_| SimError::Overflow)?
    };
    Ok(Ratio::from_ticks(
        i64::try_from(signed).map_err(|_| SimError::Overflow)?,
    ))
}

/// `floor(S1² · (n−1) · factor · 1e16 / (n·D))`.
///
/// The `1e16` scale is two `1e8` steps so a long sample stays inside `u128`.
/// The division is the same floor as one ratio.
fn sharpe_quotient(s1: i128, n: i128, d: i128, factor: i128) -> Result<i128, SimError> {
    let squares = n.checked_mul(d).ok_or(SimError::Overflow)?;
    if squares <= 0 {
        return Err(SimError::Overflow);
    }
    let periods = (n - 1).checked_mul(factor).ok_or(SimError::Overflow)?;
    if periods <= 0 {
        return Err(SimError::Overflow);
    }
    let sum2 = s1
        .unsigned_abs()
        .checked_mul(s1.unsigned_abs())
        .ok_or(SimError::Overflow)?;
    let periods = u128::try_from(periods).map_err(|_| SimError::Overflow)?;
    let squares = u128::try_from(squares).map_err(|_| SimError::Overflow)?;
    let scaled = floor_times_scale_sq(sum2, periods, squares)?;
    i128::try_from(scaled).map_err(|_| SimError::Overflow)
}

/// `floor(base * 1e16 / squares)` with `base = sum2 * periods`.
fn floor_times_scale_sq(sum2: u128, periods: u128, squares: u128) -> Result<u128, SimError> {
    let scale = 100_000_000u128;
    let base = sum2.checked_mul(periods).ok_or(SimError::Overflow)?;
    let (quot, rem) = (base / squares, base % squares);
    let rem_scaled = rem.checked_mul(scale).ok_or(SimError::Overflow)?;
    let once = quot
        .checked_mul(scale)
        .ok_or(SimError::Overflow)?
        .checked_add(rem_scaled / squares)
        .ok_or(SimError::Overflow)?;
    let rem = rem_scaled % squares;
    let rem_scaled = rem.checked_mul(scale).ok_or(SimError::Overflow)?;
    once.checked_mul(scale)
        .ok_or(SimError::Overflow)?
        .checked_add(rem_scaled / squares)
        .ok_or(SimError::Overflow)
}

/// Annualised one-way turnover in multiples of equity, at `10^8`.
///
/// `sum(|fill notional|) * 365 / sum(daily equity)`.
pub fn turnover_ticks(notional_ticks: i128, daily_equity: &[i64]) -> Result<Ratio, SimError> {
    if daily_equity.is_empty() || notional_ticks == 0 {
        return Ok(Ratio::from_ticks(0));
    }
    let mut equity_sum: i128 = 0;
    for value in daily_equity {
        equity_sum = equity_sum
            .checked_add(i128::from(*value))
            .ok_or(SimError::Overflow)?;
    }
    if equity_sum <= 0 {
        return Ok(Ratio::from_ticks(0));
    }
    let num = notional_ticks
        .checked_mul(365)
        .and_then(|value| value.checked_mul(SCALE))
        .ok_or(SimError::Overflow)?;
    let ticks = num / equity_sum;
    Ok(Ratio::from_ticks(
        i64::try_from(ticks).map_err(|_| SimError::Overflow)?,
    ))
}

/// `|price * qty| / 1e8` in cash ticks.
pub fn fill_notional_ticks(price_ticks: i64, qty_ticks: i64) -> Result<i128, SimError> {
    let product = i128::from(price_ticks)
        .checked_mul(i128::from(qty_ticks))
        .ok_or(SimError::Overflow)?;
    Ok(product.checked_abs().ok_or(SimError::Overflow)? / SCALE)
}

#[cfg(test)]
mod tests {
    use super::{div_half_even, turnover_ticks};

    #[test]
    fn half_even_resolves_ties_to_the_even_quotient() {
        assert_eq!(div_half_even(1, 2).unwrap(), 0);
        assert_eq!(div_half_even(3, 2).unwrap(), 2);
        assert_eq!(div_half_even(5, 2).unwrap(), 2);
        assert_eq!(div_half_even(-1, 2).unwrap(), 0);
        assert_eq!(div_half_even(-3, 2).unwrap(), -2);
        assert_eq!(div_half_even(-5, 2).unwrap(), -2);
    }

    /// T8. One 1 BTC round trip at 100 USDT, equity 1_000 for 365 days, is 0.2x.
    #[test]
    fn t8_round_trip_turnover_is_point_two() {
        let tick = 100_000_000i64;
        let notional = 200 * i128::from(tick);
        let daily = vec![1_000 * tick; 365];
        assert_eq!(
            turnover_ticks(notional, &daily).unwrap().ticks(),
            20_000_000
        );
    }
}
