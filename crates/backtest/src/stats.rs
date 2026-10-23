//! DSR, skew, kurtosis, fee slope, and CSCV PBO. Integer ratios only.
//!
//! The formulas are runner-inputs v2 §5.2 and the second-batch PBO reading.
//! Probabilities and the normal tail use a fixed scale of `10^15`. No `f64`.

use parangsae_core::SCALE;

use crate::metrics::sharpe_of_return_ticks;

const Q: i128 = 1_000_000_000_000_000;
const LN2_Q: i128 = 693_147_180_559_945;
const SQRT2_Q: i128 = 1_414_213_562_373_095;
const GAMMA_Q: i128 = 577_215_664_900_000;
const E_Q: i128 = 2_718_281_828_459_045;
const ERF_P_Q: i128 = 327_591_100_000_000;
const ERF_A1_Q: i128 = 254_829_592_000_000;
const ERF_A2_Q: i128 = -284_496_736_000_000;
const ERF_A3_Q: i128 = 1_421_413_741_000_000;
const ERF_A4_Q: i128 = -1_453_152_027_000_000;
const ERF_A5_Q: i128 = 1_061_405_429_000_000;
const SQRT365_Q: i128 = 19_104_973_174_542_200;

const CSCV_BLOCKS: usize = 16;
const CSCV_SPLITS: i128 = 12_870;
const VAR_FLOOR_TICKS: i64 = 25_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatsError;

impl std::fmt::Display for StatsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "report statistic overflowed")
    }
}

fn mul_q(a: i128, b: i128) -> Result<i128, StatsError> {
    let prod = a.checked_mul(b).ok_or(StatsError)?;
    let half = Q / 2;
    let rounded = if prod >= 0 {
        prod.checked_add(half)
    } else {
        prod.checked_sub(half)
    }
    .ok_or(StatsError)?;
    rounded.checked_div(Q).ok_or(StatsError)
}

fn div_q(num: i128, den: i128) -> Result<i128, StatsError> {
    if den == 0 {
        return Err(StatsError);
    }
    let prod = num.checked_mul(Q).ok_or(StatsError)?;
    let half = den.abs() / 2;
    let rounded = if prod >= 0 {
        prod.checked_add(half)
    } else {
        prod.checked_sub(half)
    }
    .ok_or(StatsError)?;
    rounded.checked_div(den).ok_or(StatsError)
}

fn ticks_to_q(ticks: i128) -> Result<i128, StatsError> {
    ticks.checked_mul(Q / SCALE).ok_or(StatsError)
}

fn q_to_ticks(value: i128) -> Result<i64, StatsError> {
    let half = Q / SCALE / 2;
    let rounded = if value >= 0 {
        value.checked_add(half)
    } else {
        value.checked_sub(half)
    }
    .ok_or(StatsError)?;
    i64::try_from(rounded / (Q / SCALE)).map_err(|_| StatsError)
}

fn exp_neg(x_q: i128) -> Result<i128, StatsError> {
    if x_q <= 0 {
        return Ok(Q);
    }
    if x_q > 40 * Q {
        return Ok(0);
    }
    let n = x_q / LN2_Q;
    let r = x_q - n * LN2_Q;
    let mut term = Q;
    let mut sum = Q;
    let mut neg = true;
    for k in 1..40 {
        term = mul_q(term, r)? / i128::from(k);
        if neg {
            sum -= term;
        } else {
            sum += term;
        }
        neg = !neg;
        if term == 0 {
            break;
        }
    }
    if n >= 62 {
        return Ok(0);
    }
    Ok(sum >> n)
}

fn erf_q(z_q: i128) -> Result<i128, StatsError> {
    if z_q < 0 {
        return Ok(-erf_q(-z_q)?);
    }
    let t = div_q(Q, Q + mul_q(ERF_P_Q, z_q)?)?;
    let mut poly = ERF_A5_Q;
    for coeff in [ERF_A4_Q, ERF_A3_Q, ERF_A2_Q, ERF_A1_Q] {
        poly = mul_q(poly, t)?.checked_add(coeff).ok_or(StatsError)?;
    }
    poly = mul_q(poly, t)?;
    let z2 = mul_q(z_q, z_q)?;
    let tail = mul_q(poly, exp_neg(z2)?)?;
    Ok(Q - tail)
}

fn phi_q(x_q: i128) -> Result<i128, StatsError> {
    let arg = div_q(x_q, SQRT2_Q)?;
    let erf = erf_q(arg)?;
    Ok((Q + erf) / 2)
}

fn inv_phi_q(p_q: i128) -> Result<i128, StatsError> {
    if p_q <= 0 || p_q >= Q {
        return Err(StatsError);
    }
    let mut lo = -8 * Q;
    let mut hi = 8 * Q;
    for _ in 0..60 {
        let mid = (lo + hi) / 2;
        if phi_q(mid)? < p_q {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok((lo + hi) / 2)
}

/// `(1 − γ) Φ⁻¹(1 − 1/N) + γ Φ⁻¹(1 − 1/(N·e))`, at scale `10^15`.
pub(crate) fn selection_bracket_q(n_global: u64) -> Result<i128, StatsError> {
    if n_global < 2 {
        return Err(StatsError);
    }
    let n = i128::from(n_global);
    let p1 = Q - Q / n;
    let ne = mul_q(n.checked_mul(Q).ok_or(StatsError)?, E_Q)?;
    let p2 = Q - div_q(Q, ne)?;
    let one_minus = Q - GAMMA_Q;
    let left = mul_q(one_minus, inv_phi_q(p1)?)?;
    let right = mul_q(GAMMA_Q, inv_phi_q(p2)?)?;
    left.checked_add(right).ok_or(StatsError)
}

fn sr0_ann_q(var_ann_ticks: i64, bracket_q: i128) -> Result<i128, StatsError> {
    let var_q = ticks_to_q(i128::from(var_ann_ticks))?;
    let sd_q = isqrt_q(var_q)?;
    mul_q(sd_q, bracket_q)
}

fn isqrt_q(value_q: i128) -> Result<i128, StatsError> {
    if value_q < 0 {
        return Err(StatsError);
    }
    let scaled = value_q.checked_mul(Q).ok_or(StatsError)?;
    let root = u128::try_from(scaled).map_err(|_| StatsError)?.isqrt();
    i128::try_from(root).map_err(|_| StatsError)
}

fn dsr_z_q(
    sr_ann_ticks: i64,
    days_t: u64,
    skew_ticks: i64,
    kurt_ticks: i64,
    sr0_ann_q: i128,
) -> Result<Option<i128>, StatsError> {
    if days_t < 2 {
        return Ok(None);
    }
    let sr_ann = ticks_to_q(i128::from(sr_ann_ticks))?;
    let sr_d = div_q(sr_ann, SQRT365_Q)?;
    let sr0_d = div_q(sr0_ann_q, SQRT365_Q)?;
    let g3 = ticks_to_q(i128::from(skew_ticks))?;
    let g4 = ticks_to_q(i128::from(kurt_ticks))?;
    let sr2 = mul_q(sr_d, sr_d)?;
    let skew_term = mul_q(g3, sr_d)?;
    let kurt_term = mul_q((g4 - Q) / 4, sr2)?;
    let inside = Q
        .checked_sub(skew_term)
        .and_then(|value| value.checked_add(kurt_term))
        .ok_or(StatsError)?;
    if inside <= 0 {
        return Ok(None);
    }
    let denom = isqrt_q(inside)?;
    let diff = sr_d.checked_sub(sr0_d).ok_or(StatsError)?;
    let t = i128::from(days_t - 1);
    let width = isqrt_q(t * Q)?;
    let num = mul_q(diff, width)?;
    Ok(Some(div_q(num, denom)?))
}

/// `Φ(z)` of §5.2, in `10^8` probability ticks. `None` when the variance term is not positive.
pub(crate) fn deflated_sharpe(
    sr_ann_ticks: i64,
    n_global: u64,
    var_ann_ticks: i64,
    days_t: u64,
    skew_ticks: i64,
    kurt_ticks: i64,
) -> Result<Option<i64>, StatsError> {
    let bracket = selection_bracket_q(n_global)?;
    let sr0 = sr0_ann_q(var_ann_ticks, bracket)?;
    let Some(z) = dsr_z_q(sr_ann_ticks, days_t, skew_ticks, kurt_ticks, sr0)? else {
        return Ok(None);
    };
    q_to_ticks(phi_q(z)?).map(Some)
}

/// Smallest annual Sharpe, in `10^8` ticks, whose DSR is at least 0.95.
pub(crate) fn dsr_hurdle_sr_ann(
    n_global: u64,
    var_ann_ticks: i64,
    days_t: u64,
    skew_ticks: i64,
    kurt_ticks: i64,
) -> Result<Option<i64>, StatsError> {
    let target = (Q * 95) / 100;
    let bracket = selection_bracket_q(n_global)?;
    let sr0 = sr0_ann_q(var_ann_ticks, bracket)?;
    let mut lo = 0i64;
    let mut hi = 10_000_000_000i64;
    let top = dsr_z_q(hi, days_t, skew_ticks, kurt_ticks, sr0)?;
    if top.is_none() || phi_q(top.unwrap_or(0))? < target {
        return Ok(None);
    }
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let pass = match dsr_z_q(mid, days_t, skew_ticks, kurt_ticks, sr0)? {
            Some(z) => phi_q(z)? >= target,
            None => false,
        };
        if pass {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok(Some(lo))
}

/// Sample moments of daily-return ticks. Kurtosis is non-excess (`3` for a normal).
pub(crate) fn skew_kurt(returns: &[i64]) -> Result<Option<(i64, i64)>, StatsError> {
    let n_us = returns.len();
    if n_us < 4 {
        return Ok(None);
    }
    let n = i128::try_from(n_us).map_err(|_| StatsError)?;
    let mut sum = 0i128;
    for value in returns {
        sum = sum.checked_add(i128::from(*value)).ok_or(StatsError)?;
    }
    let mut s2 = 0i128;
    let mut s3 = 0i128;
    let mut s4 = 0i128;
    for value in returns {
        let dev = (i128::from(*value)
            .checked_mul(n)
            .ok_or(StatsError)?
            .checked_sub(sum)
            .ok_or(StatsError)?)
            / n;
        let d2 = dev.checked_mul(dev).ok_or(StatsError)?;
        s2 = s2.checked_add(d2).ok_or(StatsError)?;
        s3 = s3
            .checked_add(d2.checked_mul(dev).ok_or(StatsError)?)
            .ok_or(StatsError)?;
        s4 = s4
            .checked_add(d2.checked_mul(d2).ok_or(StatsError)?)
            .ok_or(StatsError)?;
    }
    let m2 = s2 / n;
    let m3 = s3 / n;
    let m4 = s4 / n;
    if m2 <= 0 {
        return Ok(None);
    }
    let sd = u128::try_from(m2).map_err(|_| StatsError)?.isqrt();
    let sd = i128::try_from(sd).map_err(|_| StatsError)?;
    if sd == 0 {
        return Ok(None);
    }
    let sd3 = sd
        .checked_mul(sd)
        .and_then(|value| value.checked_mul(sd))
        .ok_or(StatsError)?;
    let skew_q = m3.checked_mul(Q).ok_or(StatsError)? / sd3;
    let kurt_q = m4
        .checked_div(m2)
        .and_then(|value| value.checked_mul(Q))
        .ok_or(StatsError)?
        / m2;
    Ok(Some((q_to_ticks(skew_q)?, q_to_ticks(kurt_q)?)))
}

/// `max(sample variance of the column annual Sharpes, 0.25)`, in `10^8` ticks.
/// An undefined Sharpe counts as 0. Fewer than two columns is the floor.
pub(crate) fn sr_var_ann_ticks(columns: &[Vec<i64>]) -> Result<i64, StatsError> {
    if columns.len() < 2 {
        return Ok(VAR_FLOOR_TICKS);
    }
    let mut sharpes = Vec::with_capacity(columns.len());
    for column in columns {
        let annual = sharpe_of_return_ticks(column)
            .map_err(|_| StatsError)?
            .1
            .map(|ratio| ratio.ticks())
            .unwrap_or(0);
        sharpes.push(i128::from(annual));
    }
    let n = i128::try_from(sharpes.len()).map_err(|_| StatsError)?;
    let sum: i128 = sharpes.iter().sum();
    let mut acc = 0i128;
    for value in sharpes {
        let diff = value.checked_mul(n).ok_or(StatsError)? - sum;
        acc = acc
            .checked_add(diff.checked_mul(diff).ok_or(StatsError)?)
            .ok_or(StatsError)?;
    }
    // `x` is already in `10^8` ticks, so the unitless variance is `var(x) / 1e8`.
    let den = n
        .checked_mul(n)
        .and_then(|value| value.checked_mul(n - 1))
        .and_then(|value| value.checked_mul(SCALE))
        .ok_or(StatsError)?;
    let var_ticks = acc / den;
    let floored = var_ticks.max(i128::from(VAR_FLOOR_TICKS));
    i64::try_from(floored).map_err(|_| StatsError)
}

/// OLS slope of annual Sharpe ticks against `{2, 4, 5, 6}` bps. Ticks of Sharpe per bps.
pub(crate) fn fee_slope_ticks(sharpes: [i64; 4]) -> Result<i64, StatsError> {
    let xs = [2i128, 4, 5, 6];
    let n = 4i128;
    let sum_x = 17i128;
    let sum_y: i128 = sharpes.iter().map(|value| i128::from(*value)).sum();
    let mut num = 0i128;
    let mut den = 0i128;
    for (x, y) in xs.iter().zip(sharpes) {
        let dx = *x * n - sum_x;
        let dy = i128::from(y) * n - sum_y;
        num = num
            .checked_add(dx.checked_mul(dy).ok_or(StatsError)?)
            .ok_or(StatsError)?;
        den = den
            .checked_add(dx.checked_mul(dx).ok_or(StatsError)?)
            .ok_or(StatsError)?;
    }
    if den == 0 {
        return Err(StatsError);
    }
    i64::try_from(num / den).map_err(|_| StatsError)
}

/// Fee, in `10^8` bps-ticks, where interpolated net Sharpe is 0.
/// `None` means the root is outside `{2, 4, 5, 6}`: `above` is `>6`.
pub(crate) fn breakeven_fee(sharpes: [Option<i64>; 4]) -> Option<Breakeven> {
    let xs = [2i128, 4, 5, 6];
    let ys: [i128; 4] = match sharpes {
        [Some(a), Some(b), Some(c), Some(d)] => {
            [i128::from(a), i128::from(b), i128::from(c), i128::from(d)]
        }
        _ => return None,
    };
    for index in 0..3 {
        let left = ys[index];
        let right = ys[index + 1];
        if left == 0 {
            return Some(Breakeven::Fee(xs[index] * SCALE));
        }
        if left.signum() != right.signum() && right != 0 {
            let span = xs[index + 1] - xs[index];
            let num = left.checked_mul(span)?;
            let den = left - right;
            let fee = xs[index] * SCALE + num * SCALE / den;
            return Some(Breakeven::Fee(fee));
        }
        if right == 0 {
            return Some(Breakeven::Fee(xs[index + 1] * SCALE));
        }
    }
    if ys[0] > 0 && ys[3] >= ys[0] {
        return Some(Breakeven::Below2);
    }
    if ys[0] < 0 && ys[3] <= ys[0] {
        return Some(Breakeven::Below2);
    }
    if ys[3] > 0 && ys[3] < ys[0] {
        return Some(Breakeven::Above6);
    }
    if ys[3] < 0 && ys[3] > ys[0] {
        return Some(Breakeven::Above6);
    }
    if ys[0] > 0 {
        Some(Breakeven::Above6)
    } else {
        Some(Breakeven::Below2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Breakeven {
    Fee(i128),
    Above6,
    Below2,
}

pub(crate) fn block_ranges(days: usize) -> Option<Vec<(usize, usize)>> {
    if days < CSCV_BLOCKS {
        return None;
    }
    let base = days / CSCV_BLOCKS;
    let mut cursor = 0;
    let mut out = Vec::with_capacity(CSCV_BLOCKS);
    for index in 0..CSCV_BLOCKS {
        let len = if index + 1 == CSCV_BLOCKS {
            days - cursor
        } else {
            base
        };
        out.push((cursor, cursor + len));
        cursor += len;
    }
    Some(out)
}

fn is_days(blocks: &[(usize, usize)], chosen: &[usize], gap: usize) -> Vec<usize> {
    let mut days = Vec::new();
    for (index, (start, end)) in blocks.iter().enumerate() {
        if chosen.binary_search(&index).is_err() {
            continue;
        }
        let mut lo = *start;
        let mut hi = *end;
        if index > 0 && chosen.binary_search(&(index - 1)).is_err() {
            lo = lo.saturating_add(gap).min(hi);
        }
        if index + 1 < blocks.len() && chosen.binary_search(&(index + 1)).is_err() {
            hi = hi.saturating_sub(gap).max(lo);
        }
        days.extend(lo..hi);
    }
    days
}

fn oos_days(blocks: &[(usize, usize)], chosen: &[usize]) -> Vec<usize> {
    let mut days = Vec::new();
    for (index, (start, end)) in blocks.iter().enumerate() {
        if chosen.binary_search(&index).is_err() {
            days.extend(*start..*end);
        }
    }
    days
}

fn column_sharpe(columns: &[Vec<i64>], days: &[usize]) -> Result<Vec<Option<i64>>, StatsError> {
    let mut scores = Vec::with_capacity(columns.len());
    for column in columns {
        let series: Vec<i64> = days.iter().map(|day| column[*day]).collect();
        let annual = sharpe_of_return_ticks(&series)
            .map_err(|_| StatsError)?
            .0
            .map(|ratio| ratio.ticks());
        scores.push(annual);
    }
    Ok(scores)
}

fn is_best(scores: &[Option<i64>]) -> usize {
    let mut best = 0;
    for index in 1..scores.len() {
        let better = match (scores[index], scores[best]) {
            (Some(left), Some(right)) => left > right,
            (Some(_), None) => true,
            _ => false,
        };
        if better {
            best = index;
        }
    }
    best
}

fn lamda_nonpositive(scores: &[Option<i64>], chosen: usize) -> bool {
    let mut worse = 0i128;
    let mut ties = 0i128;
    for score in scores {
        let order = match (score, scores[chosen]) {
            (None, None) => 0,
            (None, Some(_)) => -1,
            (Some(_), None) => 1,
            (Some(left), Some(right)) => match left.cmp(&right) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            },
        };
        if order < 0 {
            worse += 1;
        } else if order == 0 {
            ties += 1;
        }
    }
    let k = i128::try_from(scores.len()).unwrap_or(i128::MAX);
    worse * 2 + ties <= k
}

/// PBO in `10^8` ticks. `None` when there are fewer than 2 configs or 16 days.
pub(crate) fn pbo_ticks(columns: &[Vec<i64>], gap: usize) -> Result<Option<i64>, StatsError> {
    if columns.len() < 2 {
        return Ok(None);
    }
    let days = columns[0].len();
    if columns.iter().any(|column| column.len() != days) {
        return Err(StatsError);
    }
    let Some(blocks) = block_ranges(days) else {
        return Ok(None);
    };
    let mut failures = 0i128;
    for_each_is_split(&mut |chosen| {
        let is_rows = is_days(&blocks, chosen, gap);
        let oos_rows = oos_days(&blocks, chosen);
        let is_scores = column_sharpe(columns, &is_rows)?;
        let oos_scores = column_sharpe(columns, &oos_rows)?;
        let best = is_best(&is_scores);
        if lamda_nonpositive(&oos_scores, best) {
            failures += 1;
        }
        Ok(())
    })?;
    let ticks = failures.checked_mul(SCALE).ok_or(StatsError)? / CSCV_SPLITS;
    i64::try_from(ticks).map(Some).map_err(|_| StatsError)
}

fn for_each_is_split(
    body: &mut dyn FnMut(&[usize]) -> Result<(), StatsError>,
) -> Result<(), StatsError> {
    let mut pick = [0usize; 8];
    fn walk(
        start: usize,
        filled: usize,
        pick: &mut [usize],
        body: &mut dyn FnMut(&[usize]) -> Result<(), StatsError>,
    ) -> Result<(), StatsError> {
        if filled == 8 {
            return body(pick);
        }
        let last = CSCV_BLOCKS - (8 - filled);
        for index in start..=last {
            pick[filled] = index;
            walk(index + 1, filled + 1, pick, body)?;
        }
        Ok(())
    }
    walk(0, 0, &mut pick, body)
}

const DAY_US: i64 = 86_400_000_000;

/// `(day open microseconds, one column of daily-return ticks per config)`.
///
/// Fold 1 contributes its train days. Each later fold contributes the train
/// days that are not in the previous fold's train window.
pub(crate) fn is_matrix(
    folds: &[Vec<(i64, Vec<i64>)>],
) -> Result<(Vec<i64>, Vec<Vec<i64>>), StatsError> {
    if folds.is_empty() || folds[0].is_empty() {
        return Err(StatsError);
    }
    let configs = folds[0].len();
    let mut columns = vec![Vec::new(); configs];
    let mut dates = Vec::new();
    let (start, returns) = &folds[0][0];
    for day in 0..returns.len() {
        let offset = i64::try_from(day).map_err(|_| StatsError)? * DAY_US;
        dates.push(start.checked_add(offset).ok_or(StatsError)?);
    }
    for (index, (_start, returns)) in folds[0].iter().enumerate() {
        columns[index].extend_from_slice(returns);
    }
    for pair in folds.windows(2) {
        if pair[0].len() != configs || pair[1].len() != configs {
            return Err(StatsError);
        }
        let (start, returns) = &pair[1][0];
        let from = suffix_from(&pair[0][0], *start, returns)?;
        for day in from..returns.len() {
            let offset = i64::try_from(day).map_err(|_| StatsError)? * DAY_US;
            dates.push(start.checked_add(offset).ok_or(StatsError)?);
        }
        for (index, column) in columns.iter_mut().enumerate() {
            let from = suffix_from(&pair[0][index], pair[1][index].0, &pair[1][index].1)?;
            column.extend_from_slice(&pair[1][index].1[from..]);
        }
    }
    let width = columns[0].len();
    if columns.iter().any(|column| column.len() != width) || dates.len() != width {
        return Err(StatsError);
    }
    Ok((dates, columns))
}

fn suffix_from(prev: &(i64, Vec<i64>), start: i64, returns: &[i64]) -> Result<usize, StatsError> {
    let prev_end = prev
        .0
        .checked_add(i64::try_from(prev.1.len()).map_err(|_| StatsError)? * DAY_US)
        .ok_or(StatsError)?;
    let suffix = if start >= prev_end {
        returns.len()
    } else {
        let overlap = (prev_end - start) / DAY_US;
        returns
            .len()
            .saturating_sub(usize::try_from(overlap).unwrap_or(usize::MAX))
    };
    Ok(returns.len() - suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near_hurdle(got: i64, expect_hundredths: i64) {
        let got_hundredths = got / 1_000_000;
        assert!(
            (got_hundredths - expect_hundredths).abs() <= 1,
            "hurdle {got} is not {expect_hundredths}/100"
        );
    }

    #[test]
    fn the_n202_bracket_is_the_published_2_7688() {
        let bracket = selection_bracket_q(202).unwrap();
        let ten_thousandths = bracket * 10_000 / Q;
        assert!(
            (ten_thousandths - 27_688).abs() <= 2,
            "bracket {ten_thousandths}"
        );
    }

    #[test]
    fn planning_hurdles_match_section_5_2() {
        let var = |sd_hundredths: i64| sd_hundredths * sd_hundredths * 10_000;
        let normal = (0, 300_000_000);
        let fat = (-50_000_000, 800_000_000);
        near_hurdle(
            dsr_hurdle_sr_ann(202, var(25), 1351, normal.0, normal.1)
                .unwrap()
                .unwrap(),
            155,
        );
        near_hurdle(
            dsr_hurdle_sr_ann(202, var(50), 1351, normal.0, normal.1)
                .unwrap()
                .unwrap(),
            224,
        );
        near_hurdle(
            dsr_hurdle_sr_ann(202, var(75), 1351, normal.0, normal.1)
                .unwrap()
                .unwrap(),
            294,
        );
        near_hurdle(
            dsr_hurdle_sr_ann(202, var(25), 1351, fat.0, fat.1)
                .unwrap()
                .unwrap(),
            157,
        );
        near_hurdle(
            dsr_hurdle_sr_ann(202, var(25), 1321, normal.0, normal.1)
                .unwrap()
                .unwrap(),
            156,
        );
        near_hurdle(
            dsr_hurdle_sr_ann(202, var(50), 1321, fat.0, fat.1)
                .unwrap()
                .unwrap(),
            229,
        );
    }

    #[test]
    fn pbo_is_zero_when_the_same_config_wins_both_halves() {
        // A scalar multiple has the same Sharpe, so the weak column is the
        // opposite shape. Each 4-day block keeps a positive middle after the
        // 1-day embargo, and that middle has variance.
        let mut strong = Vec::new();
        let mut weak = Vec::new();
        for _ in 0..16 {
            strong.extend_from_slice(&[-100_000, 800_000, 1_200_000, 1_000_000]);
            weak.extend_from_slice(&[100_000, -800_000, -1_200_000, -1_000_000]);
        }
        let pbo = pbo_ticks(&[strong, weak], 1).unwrap().unwrap();
        assert_eq!(pbo, 0);
    }

    #[test]
    fn pbo_counts_a_tie_as_a_failure() {
        let column = vec![1_000_000i64, -500_000, 250_000, -100_000];
        let mut days = Vec::new();
        for _ in 0..8 {
            days.extend_from_slice(&column);
        }
        let pbo = pbo_ticks(&[days.clone(), days], 1).unwrap().unwrap();
        assert_eq!(pbo, 100_000_000);
    }

    #[test]
    fn cross_trial_variance_is_the_sharpe_sample_over_1e8() {
        let flat = sr_var_ann_ticks(&[vec![1, -1, 1, -1]]).unwrap();
        assert_eq!(flat, 25_000_000);
        let up = vec![1_000_000i64, -200_000, 1_000_000, -200_000];
        let down = vec![-1_000_000i64, 200_000, -1_000_000, 200_000];
        let s_up = sharpe_of_return_ticks(&up).unwrap().1.unwrap().ticks();
        let s_down = sharpe_of_return_ticks(&down).unwrap().1.unwrap().ticks();
        let diff = i128::from(s_up) - i128::from(s_down);
        let expected = diff * diff / 2 / SCALE;
        assert!(expected > 25_000_000);
        assert_eq!(
            sr_var_ann_ticks(&[up, down]).unwrap(),
            i64::try_from(expected).unwrap()
        );
    }

    #[test]
    fn one_config_has_no_pbo() {
        assert_eq!(pbo_ticks(&[vec![1, 2, 3]], 1).unwrap(), None);
    }

    #[test]
    fn embargo_drops_the_is_day_touching_an_oos_block() {
        let blocks = block_ranges(16).unwrap();
        let chosen: Vec<usize> = (0..8).collect();
        let days = is_days(&blocks, &chosen, 1);
        assert_eq!(days.len(), 7);
        assert!(!days.contains(&7));
    }

    #[test]
    fn breakeven_outside_the_grid_uses_the_prereg_tokens() {
        assert_eq!(
            breakeven_fee([Some(10), Some(8), Some(7), Some(6)]),
            Some(Breakeven::Above6)
        );
        assert_eq!(
            breakeven_fee([Some(-4), Some(-5), Some(-6), Some(-7)]),
            Some(Breakeven::Below2)
        );
        match breakeven_fee([Some(100_000_000), Some(0), Some(-1), Some(-2)]) {
            Some(Breakeven::Fee(ticks)) => assert_eq!(ticks, 4 * SCALE),
            other => panic!("{other:?}"),
        }
    }
}

// Family-specific CSCV policy is exercised independently of the walk-forward gap.
/// Frozen preregs §5: max hold plus one-day embargo. Separate from WF purge.
pub(crate) fn cscv_gap_days(family: &str) -> usize {
    match family {
        "H3" => 5,
        "H4" | "H5" => 8,
        _ => 1,
    }
}

#[cfg(test)]
mod qr_purge_regression {
    use super::*;
    #[test]
    fn qr2_preregistered_gap_excludes_near_boundary_days() {
        let blocks = vec![(0, 20), (20, 40), (40, 60)];
        for (family, gap) in [("H1", 1), ("H3", 5), ("H4", 8), ("H5", 8)] {
            let kept = is_days(&blocks, &[1], cscv_gap_days(family));
            let policy = cscv_gap_days(family);
            assert_eq!(
                (policy, kept.first().copied(), kept.last().copied()),
                (gap, Some(20 + gap), Some(39 - gap)),
                "{family}"
            );
        }
    }
}
