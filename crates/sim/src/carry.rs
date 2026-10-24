//! Separate funded-spot and USD-M wallets for a bounded synthetic carry session.

use parangsae_core::{Price, Qty, Ratio, Side, SCALE};

use crate::round::{div_floor, fit_i64};
use crate::{Cash, SimError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub total: Cash,
    pub spot_quote: Cash,
    pub perp_collateral: Cash,
    pub idle: Cash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Market {
    Spot,
    UsdMPerp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeeCharge {
    Usdt(Cash),
    Btc(Qty),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountFill {
    pub market: Market,
    pub side: Side,
    pub qty: Qty,
    pub price: Price,
    pub fee: FeeCharge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountEvent {
    Fill(AccountFill),
    Funding(Cash),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarryAccountingError {
    InvalidAllocation,
    InvalidFee,
    WrongFeeAsset,
    ZeroQuantity,
    FeeExceedsFill,
    InsufficientSpotCash,
    InsufficientPerpCash,
    InsufficientBtc,
    NotShort,
    Overflow,
}

impl From<SimError> for CarryAccountingError {
    fn from(_: SimError) -> Self {
        Self::Overflow
    }
}

/// A copied value, never a capability to mutate the owned ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryAccountState {
    pub spot_quote: Cash,
    pub spot_btc: Qty,
    pub perp_wallet: Cash,
    pub perp_short: Qty,
    pub idle: Cash,
    pub spot_quote_fees: Cash,
    pub spot_btc_fees: Qty,
    pub perp_fees: Cash,
    pub funding_net: Cash,
    pub realized_perp: Cash,
    pub perp_entry_cost_scaled: i128,
    pub realized_perp_scaled: i128,
}

impl CarryAccountState {
    pub fn unrealized_perp(&self, mark: Price) -> Result<Cash, CarryAccountingError> {
        let closing = product(self.perp_short.ticks(), mark.ticks())?;
        let pnl = self
            .perp_entry_cost_scaled
            .checked_sub(closing)
            .ok_or(CarryAccountingError::Overflow)?;
        Ok(Cash(fit_i64(div_floor(pnl, SCALE)?)?))
    }

    pub fn equity(&self, spot: Price, perp: Price) -> Result<Cash, CarryAccountingError> {
        let inventory = div_floor(product(self.spot_btc.ticks(), spot.ticks())?, SCALE)?;
        let unrealized = self.unrealized_perp(perp)?;
        let value = i128::from(self.spot_quote.0)
            .checked_add(inventory)
            .and_then(|v| v.checked_add(i128::from(self.perp_wallet.0)))
            .and_then(|v| v.checked_add(i128::from(self.idle.0)))
            .and_then(|v| v.checked_add(i128::from(unrealized.0)))
            .ok_or(CarryAccountingError::Overflow)?;
        Ok(Cash(fit_i64(value)?))
    }
}

pub fn funding_credit(short: Qty, mark: Price, rate: Ratio) -> Result<Cash, CarryAccountingError> {
    Ok(Cash(crate::ledger::funding_amount(
        -short.ticks(),
        mark.ticks(),
        rate.ticks(),
    )?))
}

/// The sole monetary writer. Engine users receive snapshots and prepared updates.
#[derive(Debug)]
pub struct CarryLedger {
    account_state: CarryAccountState,
}

impl CarryLedger {
    pub fn new(allocation: Allocation) -> Result<Self, CarryAccountingError> {
        let values = [
            allocation.spot_quote.0,
            allocation.perp_collateral.0,
            allocation.idle.0,
        ];
        let sum = values
            .into_iter()
            .try_fold(0i128, |sum, value| sum.checked_add(i128::from(value)))
            .ok_or(CarryAccountingError::Overflow)?;
        if allocation.total.0 <= 0
            || values.into_iter().any(|value| value < 0)
            || sum != i128::from(allocation.total.0)
        {
            return Err(CarryAccountingError::InvalidAllocation);
        }
        Ok(Self {
            account_state: CarryAccountState {
                spot_quote: allocation.spot_quote,
                spot_btc: Qty::from_ticks(0).map_err(|_| CarryAccountingError::Overflow)?,
                perp_wallet: allocation.perp_collateral,
                perp_short: Qty::from_ticks(0).map_err(|_| CarryAccountingError::Overflow)?,
                idle: allocation.idle,
                spot_quote_fees: Cash(0),
                spot_btc_fees: Qty::from_ticks(0).map_err(|_| CarryAccountingError::Overflow)?,
                perp_fees: Cash(0),
                funding_net: Cash(0),
                realized_perp: Cash(0),
                perp_entry_cost_scaled: 0,
                realized_perp_scaled: 0,
            },
        })
    }

    pub fn snapshot(&self) -> CarryAccountState {
        self.account_state.clone()
    }

    pub fn prepare(
        &mut self,
        events: &[AccountEvent],
    ) -> Result<PreparedCarryUpdate<'_>, CarryAccountingError> {
        let mut next = self.account_state.clone();
        for event in events {
            apply(&mut next, *event)?;
        }
        Ok(PreparedCarryUpdate { ledger: self, next })
    }

    fn replace(&mut self, next: CarryAccountState) {
        self.account_state = next;
    }
}

#[derive(Debug)]
pub struct PreparedCarryUpdate<'a> {
    ledger: &'a mut CarryLedger,
    next: CarryAccountState,
}

impl PreparedCarryUpdate<'_> {
    pub fn snapshot(&self) -> &CarryAccountState {
        &self.next
    }

    pub fn commit(self) {
        self.ledger.replace(self.next);
    }
}

fn apply(state: &mut CarryAccountState, event: AccountEvent) -> Result<(), CarryAccountingError> {
    match event {
        AccountEvent::Fill(fill) => {
            if fill.qty.ticks() == 0 {
                return Err(CarryAccountingError::ZeroQuantity);
            }
            match fill.market {
                Market::Spot => spot(state, fill),
                Market::UsdMPerp => perp(state, fill),
            }
        }
        AccountEvent::Funding(amount) => {
            let wallet = add(state.perp_wallet.0, amount.0)?;
            if wallet < 0 {
                return Err(CarryAccountingError::InsufficientPerpCash);
            }
            state.perp_wallet = Cash(wallet);
            state.funding_net = Cash(add(state.funding_net.0, amount.0)?);
            Ok(())
        }
    }
}

fn spot(state: &mut CarryAccountState, fill: AccountFill) -> Result<(), CarryAccountingError> {
    let (quote_fee, btc_fee) = match fill.fee {
        FeeCharge::Usdt(value) if value.0 >= 0 => (value.0, 0),
        FeeCharge::Usdt(_) => return Err(CarryAccountingError::InvalidFee),
        FeeCharge::Btc(value) => (0, value.ticks()),
    };
    let lots = fill.qty.ticks();
    match fill.side {
        Side::Buy => {
            if btc_fee > lots {
                return Err(CarryAccountingError::FeeExceedsFill);
            }
            let paid = add(notional(fill.price, fill.qty, true)?, quote_fee)?;
            let cash = sub(state.spot_quote.0, paid)?;
            if cash < 0 {
                return Err(CarryAccountingError::InsufficientSpotCash);
            }
            state.spot_quote = Cash(cash);
            state.spot_btc = quantity(add(state.spot_btc.ticks(), sub(lots, btc_fee)?)?)?;
        }
        Side::Sell => {
            let debit = add(lots, btc_fee)?;
            if debit > state.spot_btc.ticks() {
                return Err(CarryAccountingError::InsufficientBtc);
            }
            let received = sub(notional(fill.price, fill.qty, false)?, quote_fee)?;
            if received < 0 {
                return Err(CarryAccountingError::InvalidFee);
            }
            state.spot_quote = Cash(add(state.spot_quote.0, received)?);
            state.spot_btc = quantity(sub(state.spot_btc.ticks(), debit)?)?;
        }
    }
    state.spot_quote_fees = Cash(add(state.spot_quote_fees.0, quote_fee)?);
    state.spot_btc_fees = quantity(add(state.spot_btc_fees.ticks(), btc_fee)?)?;
    Ok(())
}

fn perp(state: &mut CarryAccountState, fill: AccountFill) -> Result<(), CarryAccountingError> {
    let fee = match fill.fee {
        FeeCharge::Usdt(value) if value.0 >= 0 => value.0,
        FeeCharge::Usdt(_) => return Err(CarryAccountingError::InvalidFee),
        FeeCharge::Btc(_) => return Err(CarryAccountingError::WrongFeeAsset),
    };
    let lots = fill.qty.ticks();
    let mut wallet = sub(state.perp_wallet.0, fee)?;
    match fill.side {
        Side::Sell => {
            state.perp_entry_cost_scaled = state
                .perp_entry_cost_scaled
                .checked_add(product(lots, fill.price.ticks())?)
                .ok_or(CarryAccountingError::Overflow)?;
            state.perp_short = quantity(add(state.perp_short.ticks(), lots)?)?;
        }
        Side::Buy => {
            let old = state.perp_short.ticks();
            if lots > old || old == 0 {
                return Err(CarryAccountingError::NotShort);
            }
            let basis = proportional(state.perp_entry_cost_scaled, lots, old)?;
            let pnl = basis
                .checked_sub(product(lots, fill.price.ticks())?)
                .ok_or(CarryAccountingError::Overflow)?;
            let realized = state
                .realized_perp_scaled
                .checked_add(pnl)
                .ok_or(CarryAccountingError::Overflow)?;
            let rounded = fit_i64(div_floor(realized, SCALE)?)?;
            wallet = add(wallet, sub(rounded, state.realized_perp.0)?)?;
            state.realized_perp_scaled = realized;
            state.realized_perp = Cash(rounded);
            state.perp_entry_cost_scaled = state
                .perp_entry_cost_scaled
                .checked_sub(basis)
                .ok_or(CarryAccountingError::Overflow)?;
            state.perp_short = quantity(sub(old, lots)?)?;
        }
    }
    if wallet < 0 {
        return Err(CarryAccountingError::InsufficientPerpCash);
    }
    state.perp_wallet = Cash(wallet);
    state.perp_fees = Cash(add(state.perp_fees.0, fee)?);
    Ok(())
}

/// Floor `(value * part) / whole` without widening the already-scaled value.
fn proportional(value: i128, part: i64, whole: i64) -> Result<i128, CarryAccountingError> {
    let (part, whole) = (i128::from(part), i128::from(whole));
    (value / whole)
        .checked_mul(part)
        .and_then(|a| (value % whole).checked_mul(part).map(|r| (a, r)))
        .and_then(|(a, r)| a.checked_add(r / whole))
        .ok_or(CarryAccountingError::Overflow)
}

pub fn notional(price: Price, qty: Qty, round_up: bool) -> Result<i64, CarryAccountingError> {
    let value = product(price.ticks(), qty.ticks())?;
    let rounded = value / SCALE;
    let rounded = if round_up && value % SCALE != 0 {
        rounded
            .checked_add(1)
            .ok_or(CarryAccountingError::Overflow)?
    } else {
        rounded
    };
    Ok(fit_i64(rounded)?)
}

fn product(a: i64, b: i64) -> Result<i128, CarryAccountingError> {
    i128::from(a)
        .checked_mul(i128::from(b))
        .ok_or(CarryAccountingError::Overflow)
}
fn add(a: i64, b: i64) -> Result<i64, CarryAccountingError> {
    a.checked_add(b).ok_or(CarryAccountingError::Overflow)
}
fn sub(a: i64, b: i64) -> Result<i64, CarryAccountingError> {
    a.checked_sub(b).ok_or(CarryAccountingError::Overflow)
}
fn quantity(value: i64) -> Result<Qty, CarryAccountingError> {
    Qty::from_ticks(value).map_err(|_| CarryAccountingError::Overflow)
}
