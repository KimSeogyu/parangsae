//! Position, cash, fees, and funding. State changes only after every check fits.

use parangsae_core::{Price, Qty, Ratio, Side, UtcMicros, SCALE};

use crate::round::{div_floor, fit_i64, round_against_trader};
use crate::types::{Cash, Costs, FeeBps, MarketOrder, PositionQty, SimError, SlippageBps};

const BPS_DENOM: i128 = 10_000;

/// A fill at the slipped price. `fee` is what the trader paid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub ts: UtcMicros,
    pub side: Side,
    pub qty: Qty,
    pub price: Price,
    pub fee: Cash,
}

/// One funding settlement. `amount` is signed cash: negative means the trader paid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundingEvent {
    pub ts: UtcMicros,
    pub rate: Ratio,
    pub mark: Price,
    pub position: PositionQty,
    pub amount: Cash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    cash: Cash,
    position: PositionQty,
    fees_paid: Cash,
    funding_net: Cash,
}

impl Ledger {
    pub fn new(initial: Cash) -> Self {
        Self {
            cash: initial,
            position: PositionQty(0),
            fees_paid: Cash(0),
            funding_net: Cash(0),
        }
    }

    pub fn cash(&self) -> Cash {
        self.cash
    }

    pub fn position(&self) -> PositionQty {
        self.position
    }

    pub fn fees_paid(&self) -> Cash {
        self.fees_paid
    }

    pub fn funding_net(&self) -> Cash {
        self.funding_net
    }

    /// Fill `order` against `open`. Buy cash is ceil(notional) + fee.
    /// Sell cash is floor(notional) − fee. The position has no average price.
    pub fn apply_fill(
        &mut self,
        order: MarketOrder,
        open: Price,
        costs: &Costs,
        ts: UtcMicros,
    ) -> Result<Fill, SimError> {
        if order.qty.ticks() == 0 {
            return Err(SimError::ZeroQty);
        }
        let price = fill_price(open, order.side, costs.slippage)?;
        let fee = taker_fee(price, order.qty, costs.taker_fee)?;
        let notion = signed_notional(order.side, price, order.qty)?;
        let cash_delta = notion.checked_sub(fee.0).ok_or(SimError::Overflow)?;
        let new_cash = self
            .cash
            .0
            .checked_add(cash_delta)
            .ok_or(SimError::Overflow)?;
        let new_pos = match order.side {
            Side::Buy => self.position.0.checked_add(order.qty.ticks()),
            Side::Sell => self.position.0.checked_sub(order.qty.ticks()),
        }
        .ok_or(SimError::Overflow)?;
        let new_fees = self
            .fees_paid
            .0
            .checked_add(fee.0)
            .ok_or(SimError::Overflow)?;
        self.cash.0 = new_cash;
        self.position.0 = new_pos;
        self.fees_paid.0 = new_fees;
        Ok(Fill {
            ts,
            side: order.side,
            qty: order.qty,
            price,
            fee,
        })
    }

    /// `amount = -(position * mark * rate) / 1e16`, rounded against the trader.
    /// A flat position records zero and does not change cash.
    pub fn apply_funding(
        &mut self,
        rate: Ratio,
        mark: Price,
        ts: UtcMicros,
    ) -> Result<FundingEvent, SimError> {
        let position = self.position;
        let amount = funding_amount(position.0, mark.ticks(), rate.ticks())?;
        let new_cash = self.cash.0.checked_add(amount).ok_or(SimError::Overflow)?;
        let new_net = self
            .funding_net
            .0
            .checked_add(amount)
            .ok_or(SimError::Overflow)?;
        self.cash.0 = new_cash;
        self.funding_net.0 = new_net;
        Ok(FundingEvent {
            ts,
            rate,
            mark,
            position,
            amount: Cash(amount),
        })
    }

    /// `cash + floor(position * mark / 1e8)` toward −∞.
    pub fn equity(&self, mark: Price) -> Result<Cash, SimError> {
        let num = i128::from(self.position.0)
            .checked_mul(i128::from(mark.ticks()))
            .ok_or(SimError::Overflow)?;
        let mtm = div_floor(num, SCALE)?;
        let total = mtm
            .checked_add(i128::from(self.cash.0))
            .ok_or(SimError::Overflow)?;
        Ok(Cash(fit_i64(total)?))
    }
}

/// `ceil(|qty * price| * bps / (1e8 * 10_000))`.
pub fn taker_fee(price: Price, qty: Qty, bps: FeeBps) -> Result<Cash, SimError> {
    let num = i128::from(qty.ticks())
        .checked_mul(i128::from(price.ticks()))
        .and_then(|value| value.checked_mul(i128::from(bps.0)))
        .ok_or(SimError::Overflow)?;
    let den = SCALE.checked_mul(BPS_DENOM).ok_or(SimError::Overflow)?;
    let ticks = round_against_trader(num, den, true)?;
    Ok(Cash(fit_i64(ticks)?))
}

pub(crate) fn fill_price(open: Price, side: Side, slip: SlippageBps) -> Result<Price, SimError> {
    let slip = i128::from(slip.0);
    let (factor, pays) = match side {
        Side::Buy => (BPS_DENOM + slip, true),
        Side::Sell => {
            if slip > BPS_DENOM {
                return Err(SimError::Negative);
            }
            (BPS_DENOM - slip, false)
        }
    };
    let num = i128::from(open.ticks())
        .checked_mul(factor)
        .ok_or(SimError::Overflow)?;
    let ticks = round_against_trader(num, BPS_DENOM, pays)?;
    if ticks < 0 {
        return Err(SimError::Negative);
    }
    Price::from_ticks(fit_i64(ticks)?).map_err(|_| SimError::Negative)
}

fn signed_notional(side: Side, price: Price, qty: Qty) -> Result<i64, SimError> {
    let num = i128::from(price.ticks())
        .checked_mul(i128::from(qty.ticks()))
        .ok_or(SimError::Overflow)?;
    let pays = matches!(side, Side::Buy);
    let magnitude = round_against_trader(num, SCALE, pays)?;
    let signed = if pays {
        magnitude.checked_neg().ok_or(SimError::Overflow)?
    } else {
        magnitude
    };
    fit_i64(signed)
}

pub(crate) fn funding_amount(position: i64, mark: i64, rate: i64) -> Result<i64, SimError> {
    let product = i128::from(position)
        .checked_mul(i128::from(mark))
        .and_then(|value| value.checked_mul(i128::from(rate)))
        .ok_or(SimError::Overflow)?;
    let num = product.checked_neg().ok_or(SimError::Overflow)?;
    let den = SCALE.checked_mul(SCALE).ok_or(SimError::Overflow)?;
    let pays = num < 0;
    fit_i64(round_against_trader(num, den, pays)?)
}

#[cfg(test)]
mod tests {
    use parangsae_core::{Price, Qty, UtcMicros};

    use super::taker_fee;
    use crate::types::{FeeBps, SimError};

    #[test]
    fn one_lot_at_one_hundred_usdt_charges_one_tick() {
        let price = Price::from_ticks(100 * 100_000_000).unwrap();
        let qty = Qty::from_ticks(1).unwrap();
        let fee = taker_fee(price, qty, FeeBps(5)).unwrap();
        assert_eq!(fee.0, 1);
    }

    #[test]
    fn zero_qty_is_rejected_without_moving_cash() {
        use parangsae_core::Side;

        use crate::{Cash, Costs, Ledger, MarketOrder, SlippageBps};
        let mut ledger = Ledger::new(Cash(1_000));
        let err = ledger
            .apply_fill(
                MarketOrder {
                    side: Side::Buy,
                    qty: Qty::from_ticks(0).unwrap(),
                },
                Price::from_ticks(1).unwrap(),
                &Costs {
                    taker_fee: FeeBps(5),
                    slippage: SlippageBps(0),
                },
                UtcMicros::from_micros(0),
            )
            .unwrap_err();
        assert_eq!(err, SimError::ZeroQty);
        assert_eq!(ledger.cash().0, 1_000);
        assert_eq!(ledger.position().0, 0);
    }
}
