//! Fixed-point price and quantity. Scale is `10^8` minor units per whole unit.
//!
//! Notional is `price_ticks * qty_lots / SCALE`, truncating toward zero.
//! The product is computed in `i128`. A result that does not fit in `i64` is
//! [`FixedError::Overflow`], never a wrapped value.

use std::fmt;

/// Minor units per whole unit (8 decimal places).
pub const SCALE: i128 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(i64);

impl Price {
    pub fn from_ticks(ticks: i64) -> Result<Self, FixedError> {
        if ticks < 0 {
            Err(FixedError::Negative)
        } else {
            Ok(Self(ticks))
        }
    }

    pub const fn ticks(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Qty(i64);

impl Qty {
    pub fn from_ticks(ticks: i64) -> Result<Self, FixedError> {
        if ticks < 0 {
            Err(FixedError::Negative)
        } else {
            Ok(Self(ticks))
        }
    }

    pub const fn ticks(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Notional(i64);

impl Notional {
    pub const fn ticks(self) -> i64 {
        self.0
    }
}

/// Signed scale-`10^8` value. Premium ratios, funding rates, and book
/// percentages are not prices: [`Price`] rejects negatives, and these do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ratio(i64);

impl Ratio {
    pub const fn from_ticks(ticks: i64) -> Self {
        Self(ticks)
    }

    pub const fn ticks(self) -> i64 {
        self.0
    }
}

/// Non-negative scale-`10^8` amount the exchange stated.
///
/// Quote volume, open-interest value, and book notional are not the product
/// [`notional`] computes, and they are not base-asset quantities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Amount(i64);

impl Amount {
    pub fn from_ticks(ticks: i64) -> Result<Self, FixedError> {
        if ticks < 0 {
            Err(FixedError::Negative)
        } else {
            Ok(Self(ticks))
        }
    }

    pub const fn ticks(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixedError {
    Negative,
    Overflow,
}

impl fmt::Display for FixedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Negative => write!(f, "negative fixed-point value"),
            Self::Overflow => write!(f, "fixed-point overflow"),
        }
    }
}

/// `price * qty / SCALE` in minor units of notional.
pub fn notional(price: Price, qty: Qty) -> Result<Notional, FixedError> {
    notional_ticks(price.ticks(), qty.ticks()).map(Notional)
}

fn notional_ticks(price_ticks: i64, qty_lots: i64) -> Result<i64, FixedError> {
    if price_ticks < 0 || qty_lots < 0 {
        return Err(FixedError::Negative);
    }
    // Two i64 magnitudes always fit in i128. checked_mul keeps a release build,
    // which enables overflow checks, from depending on that fact silently.
    let product = i128::from(price_ticks)
        .checked_mul(i128::from(qty_lots))
        .ok_or(FixedError::Overflow)?;
    let scaled = product / SCALE;
    i64::try_from(scaled).map_err(|_| FixedError::Overflow)
}

#[cfg(kani)]
mod proofs {
    use super::*;

    /// Every non-negative i64 pair widens into i128. This is the multiply that
    /// `notional_ticks` uses; it must not overflow.
    #[kani::proof]
    fn widening_mul_fits_in_i128() {
        let price_ticks: i64 = kani::any();
        let qty_lots: i64 = kani::any();
        kani::assume(price_ticks >= 0);
        kani::assume(qty_lots >= 0);
        let product = i128::from(price_ticks).checked_mul(i128::from(qty_lots));
        assert!(product.is_some());
        assert_eq!(
            product.unwrap(),
            i128::from(price_ticks) * i128::from(qty_lots)
        );
    }

    /// End-to-end quotient for every pair in `0..=1_000_000` ticks.
    /// The full i64×i64 division is not in this harness: CBMC did not finish it.
    /// Inside this square the quotient is `price * qty / SCALE` and it fits in i64.
    #[kani::proof]
    fn notional_matches_below_one_million() {
        let price_ticks: i64 = kani::any();
        let qty_lots: i64 = kani::any();
        kani::assume(price_ticks >= 0 && price_ticks <= 1_000_000);
        kani::assume(qty_lots >= 0 && qty_lots <= 1_000_000);
        let expected = (i128::from(price_ticks) * i128::from(qty_lots)) / SCALE;
        match notional_ticks(price_ticks, qty_lots) {
            Ok(ticks) => assert_eq!(i128::from(ticks), expected),
            Err(_) => panic!("this square cannot overflow"),
        }
    }

    /// A family of products past i64, not a single literal. `bump` covers 1..=255.
    #[kani::proof]
    fn notional_overflow_does_not_wrap() {
        let bump: u8 = kani::any();
        kani::assume(bump > 0);
        let ticks = 1_000_000_000_000_000 + i64::from(bump);
        assert_eq!(notional_ticks(ticks, ticks), Err(FixedError::Overflow));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_whole_unit_times_one_whole_unit() {
        let price = Price::from_ticks(SCALE as i64).unwrap();
        let qty = Qty::from_ticks(SCALE as i64).unwrap();
        assert_eq!(notional(price, qty).unwrap().ticks(), SCALE as i64);
    }

    #[test]
    fn product_below_one_minor_unit_truncates_toward_zero() {
        let price = Price::from_ticks(1).unwrap();
        let qty = Qty::from_ticks(SCALE as i64 - 1).unwrap();
        assert_eq!(notional(price, qty).unwrap().ticks(), 0);
    }

    #[test]
    fn extreme_product_returns_overflow() {
        let ticks = 1_000_000_000_000_000;
        let price = Price::from_ticks(ticks).unwrap();
        let qty = Qty::from_ticks(ticks).unwrap();
        assert_eq!(notional(price, qty), Err(FixedError::Overflow));
    }

    #[test]
    fn negative_ticks_are_rejected() {
        assert_eq!(Price::from_ticks(-1), Err(FixedError::Negative));
        assert_eq!(Qty::from_ticks(-1), Err(FixedError::Negative));
        assert_eq!(Amount::from_ticks(-1), Err(FixedError::Negative));
        assert_eq!(Ratio::from_ticks(-42_342).ticks(), -42_342);
    }
}
