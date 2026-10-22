//! The negative guard in `notional_ticks` deleted. `(-1) * 1 / SCALE` is `Ok(0)`.
//! The proof asserts that result is an error, so Kani must fail.

fn notional_ticks_without_negative_guard(
    price_ticks: i64,
    qty_lots: i64,
) -> Result<i64, &'static str> {
    let product = i128::from(price_ticks)
        .checked_mul(i128::from(qty_lots))
        .ok_or("overflow")?;
    let scaled = product.checked_div(100_000_000).ok_or("overflow")?;
    i64::try_from(scaled).map_err(|_| "overflow")
}

#[cfg(kani)]
mod proof {
    use super::notional_ticks_without_negative_guard;

    #[kani::proof]
    fn unguarded_notional_accepts_a_negative() {
        let price_ticks: i64 = -1;
        let qty_lots: i64 = 1;
        assert!(notional_ticks_without_negative_guard(price_ticks, qty_lots).is_err());
    }
}
