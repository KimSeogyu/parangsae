//! 1x sizing. Risk and portfolio can replace this module later.
//!
//! `target_qty` is one truncating `i128` division, then a toward-zero step
//! down to the lot. `|qty * mark| / 1e8` stays at or under equity.

use parangsae_core::SCALE;
use parangsae_sim::SimError;

/// Signed lots. `0` when the target is flat or equity cannot fund new exposure.
///
/// `target_ticks * equity_ticks / mark_ticks`, toward zero, then toward zero
/// to a multiple of `lot_ticks`.
pub fn target_qty(
    target_ticks: i64,
    equity_ticks: i64,
    mark_ticks: i64,
    lot_ticks: i64,
) -> Result<i64, SimError> {
    if lot_ticks <= 0 || mark_ticks <= 0 {
        return Err(SimError::Negative);
    }
    if equity_ticks <= 0 || target_ticks == 0 {
        return Ok(0);
    }
    let raw = i128::from(target_ticks)
        .checked_mul(i128::from(equity_ticks))
        .ok_or(SimError::Overflow)?
        / i128::from(mark_ticks);
    let lot = i128::from(lot_ticks);
    let stepped = raw / lot * lot;
    i64::try_from(stepped).map_err(|_| SimError::Overflow)
}

/// `|qty * mark| / 1e8` in cash ticks. Used by the 1x check.
pub fn abs_notional_ticks(qty_lots: i64, mark_ticks: i64) -> Result<i128, SimError> {
    let product = i128::from(qty_lots)
        .checked_mul(i128::from(mark_ticks))
        .ok_or(SimError::Overflow)?;
    let magnitude = product.checked_abs().ok_or(SimError::Overflow)?;
    Ok(magnitude / SCALE)
}
