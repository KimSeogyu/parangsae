//! Hyndman-Fan type-7 quantile. The same interpolation R uses by default.

use libm::{ceil, floor};

use crate::error::StrategyError;

/// `h = (n - 1) * p`, then the linear blend of the floor and ceil order stats.
pub(crate) fn type7(values: &[f64], p: f64) -> Result<f64, StrategyError> {
    if values.is_empty() || !(0.0..1.0).contains(&p) {
        return Err(StrategyError::Param("quantile"));
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let n = sorted.len();
    if n == 1 {
        return Ok(sorted[0]);
    }
    let width = u32::try_from(n - 1).map_err(|_| StrategyError::Overflow)?;
    let h = f64::from(width) * p;
    if !h.is_finite() {
        return Err(StrategyError::Param("quantile"));
    }
    let lo = floor(h);
    let hi = ceil(h);
    let lo_ix = index_of(lo, n)?;
    let hi_ix = index_of(hi, n)?;
    let frac = h - lo;
    let value = sorted[lo_ix] + frac * (sorted[hi_ix] - sorted[lo_ix]);
    if value.is_finite() {
        Ok(value)
    } else {
        Err(StrategyError::Param("quantile"))
    }
}

fn index_of(value: f64, len: usize) -> Result<usize, StrategyError> {
    if value < 0.0 || !value.is_finite() {
        return Err(StrategyError::Param("quantile"));
    }
    let mut index = 0usize;
    let mut cursor = 0.0;
    while cursor < value {
        index = index.checked_add(1).ok_or(StrategyError::Overflow)?;
        cursor += 1.0;
        if index > len {
            return Err(StrategyError::Param("quantile"));
        }
    }
    if index >= len {
        Err(StrategyError::Param("quantile"))
    } else {
        Ok(index)
    }
}
