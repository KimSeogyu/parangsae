//! Position, cash, fees, and funding. State changes only after every check fits.

use std::sync::Arc;

use parangsae_core::{Causal, DecisionTime, Price, Qty, Ratio, Side, UtcMicros, SCALE};

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

/// A rate source bound to one economic settlement. Source provenance remains the caller's duty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundingRate {
    pub settlement: UtcMicros,
    pub rate: Ratio,
}

/// An explicit settlement-mark proxy, not a certificate of an actual venue charge mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementMark {
    pub source_open: UtcMicros,
    pub price: Price,
}

/// One retained position issued only by its ledger at the caller-declared boundary.
/// This is live in-process authority with no serialization interface.
/// It is not a general restart checkpoint.
///
/// It cannot be cloned (and therefore cannot implement Copy):
/// ```compile_fail
/// use parangsae_core::UtcMicros;
/// use parangsae_sim::{Cash, Ledger};
/// let mut ledger = Ledger::new(Cash(0));
/// let capture = ledger.capture_funding(UtcMicros::from_micros(0)).unwrap();
/// let duplicate = capture.clone();
/// ```
#[derive(Debug)]
#[must_use = "an outstanding funding capture retains a settlement liability"]
pub struct FundingCapture {
    issuer: Arc<()>,
    reservation: Arc<()>,
    settlement: UtcMicros,
    position: PositionQty,
}

impl FundingCapture {
    pub const fn settlement(&self) -> UtcMicros {
        self.settlement
    }
    pub const fn position(&self) -> PositionQty {
        self.position
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FundingCaptureError {
    NonIncreasingSlot {
        last: UtcMicros,
        requested: UtcMicros,
    },
    PendingCapture {
        settlement: UtcMicros,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturedFundingError {
    WrongIssuer,
    WrongCapture,
    BeforeSettlement,
    RateSlotMismatch,
    MarkSlotMismatch,
    RateUnavailable,
    MarkUnavailable,
    InvalidMark,
    Arithmetic(SimError),
}

/// Every refusal returns the original, still-unspent handle and its cause.
#[derive(Debug)]
#[must_use = "retain both the funding capture and the settlement failure"]
pub struct CapturedFundingFailure {
    pub capture: FundingCapture,
    pub error: CapturedFundingError,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CaptureClock {
    Empty,
    Pending {
        reservation: Arc<()>,
        settlement: UtcMicros,
        position: PositionQty,
    },
    Settled {
        settlement: UtcMicros,
    },
}

/// A clone is a staged snapshot of the SAME logical account and reservation.
/// Use Ledger::new for a distinct financial account. Arbitrary clone publication
/// and mixing legacy funding with this capability are not globally prevented.
/// Derived equality compares values, not Arc identity; capture guards use ptr_eq.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    issuer: Arc<()>,
    capture_clock: CaptureClock,
    cash: Cash,
    position: PositionQty,
    fees_paid: Cash,
    funding_net: Cash,
}

impl Ledger {
    pub fn new(initial: Cash) -> Self {
        Self {
            issuer: Arc::new(()),
            capture_clock: CaptureClock::Empty,
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
        self.apply_funding_position(self.position, rate, mark, ts)
    }

    fn apply_funding_position(
        &mut self,
        position: PositionQty,
        rate: Ratio,
        mark: Price,
        ts: UtcMicros,
    ) -> Result<FundingEvent, SimError> {
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

    /// Capture the actual position without posting money. Only one slot may be pending.
    /// The caller must admit the economic boundary clock; this method does not observe a venue.
    pub fn capture_funding(
        &mut self,
        settlement: UtcMicros,
    ) -> Result<FundingCapture, FundingCaptureError> {
        let last = match &self.capture_clock {
            CaptureClock::Empty => None,
            CaptureClock::Pending { settlement, .. } | CaptureClock::Settled { settlement } => {
                Some(*settlement)
            }
        };
        if let Some(last) = last {
            if settlement <= last {
                return Err(FundingCaptureError::NonIncreasingSlot {
                    last,
                    requested: settlement,
                });
            }
        }
        if let CaptureClock::Pending { settlement, .. } = self.capture_clock {
            return Err(FundingCaptureError::PendingCapture { settlement });
        }
        let reservation = Arc::new(());
        let capture = FundingCapture {
            issuer: Arc::clone(&self.issuer),
            reservation: Arc::clone(&reservation),
            settlement,
            position: self.position,
        };
        self.capture_clock = CaptureClock::Pending {
            reservation,
            settlement,
            position: self.position,
        };
        Ok(capture)
    }

    /// Atomically settle one retained position, consuming its handle only on success.
    ///
    /// This immediate operation MUST be the caller's last fallible step before
    /// infallible publication of a staged ledger. It does not make later caller
    /// rollback safe. Every refusal returns the original handle and changes nothing.
    /// The successful handle cannot be spent again:
    /// ```compile_fail
    /// use parangsae_core::{AvailableTs, DecisionTime, EventTs, Observation, Price, Ratio,
    ///     ReceivedTs, Timestamps, UtcMicros};
    /// use parangsae_sim::{Cash, FundingRate, Ledger, SettlementMark};
    /// let s = UtcMicros::from_micros(0);
    /// let d = DecisionTime::from_micros(s);
    /// let clocks = Timestamps { event_ts: EventTs::from_micros(s),
    ///     available_ts: AvailableTs::from_micros(s), received_ts: ReceivedTs::from_micros(s) };
    /// let rate = Observation::new(clocks, FundingRate { settlement: s, rate: Ratio::from_ticks(0) });
    /// let mark = Observation::new(clocks, SettlementMark { source_open: s, price: Price::from_ticks(1).unwrap() });
    /// let mut ledger = Ledger::new(Cash(0));
    /// let capture = ledger.capture_funding(s).unwrap();
    /// ledger.apply_captured_funding(capture, rate.causal_at(d).unwrap(), mark.causal_at(d).unwrap(), d).unwrap();
    /// ledger.apply_captured_funding(capture, rate.causal_at(d).unwrap(), mark.causal_at(d).unwrap(), d).unwrap();
    /// ```
    ///
    /// Causal views are rechecked at `at`; they do not certify EventTs, ReceivedTs,
    /// symbol/source identity, historical arrival or the truth of a charge mark.
    pub fn apply_captured_funding(
        &mut self,
        capture: FundingCapture,
        rate: Causal<'_, FundingRate>,
        mark: Causal<'_, SettlementMark>,
        at: DecisionTime,
    ) -> Result<FundingEvent, CapturedFundingFailure> {
        let checked = self.check_capture(&capture, &rate, &mark, at);
        let result = checked.and_then(|()| {
            self.apply_funding_position(
                capture.position,
                rate.value().rate,
                mark.value().price,
                capture.settlement,
            )
            .map_err(CapturedFundingError::Arithmetic)
        });
        match result {
            Ok(event) => {
                self.capture_clock = CaptureClock::Settled {
                    settlement: capture.settlement,
                };
                Ok(event)
            }
            Err(error) => Err(CapturedFundingFailure { capture, error }),
        }
    }

    fn check_capture(
        &self,
        capture: &FundingCapture,
        rate: &Causal<'_, FundingRate>,
        mark: &Causal<'_, SettlementMark>,
        at: DecisionTime,
    ) -> Result<(), CapturedFundingError> {
        if !Arc::ptr_eq(&self.issuer, &capture.issuer) {
            return Err(CapturedFundingError::WrongIssuer);
        }
        match &self.capture_clock {
            CaptureClock::Pending {
                reservation,
                settlement,
                position,
            } if Arc::ptr_eq(reservation, &capture.reservation)
                && *settlement == capture.settlement
                && *position == capture.position => {}
            _ => return Err(CapturedFundingError::WrongCapture),
        }
        if at.micros() < capture.settlement {
            return Err(CapturedFundingError::BeforeSettlement);
        }
        if rate.available_ts().micros() > at.micros() {
            return Err(CapturedFundingError::RateUnavailable);
        }
        if mark.available_ts().micros() > at.micros() {
            return Err(CapturedFundingError::MarkUnavailable);
        }
        if rate.value().settlement != capture.settlement {
            return Err(CapturedFundingError::RateSlotMismatch);
        }
        if mark.value().source_open != capture.settlement {
            return Err(CapturedFundingError::MarkSlotMismatch);
        }
        if mark.value().price.ticks() <= 0 {
            return Err(CapturedFundingError::InvalidMark);
        }
        Ok(())
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

/// Cash the trader loses because the fill price is worse than `open`.
///
/// The fee is not included. A buy pays more notional; a sell receives less.
pub fn slippage_cost(
    open: Price,
    side: Side,
    qty: Qty,
    slip: SlippageBps,
) -> Result<Cash, SimError> {
    let slipped = fill_price(open, side, slip)?;
    let raw = signed_notional(side, open, qty)?;
    let paid = signed_notional(side, slipped, qty)?;
    let cost = i128::from(raw)
        .checked_sub(i128::from(paid))
        .ok_or(SimError::Overflow)?;
    if cost < 0 {
        return Err(SimError::Negative);
    }
    Ok(Cash(fit_i64(cost)?))
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

/// Adverse execution quote shared by fill sizing and ledger settlement.
pub fn fill_price(open: Price, side: Side, slip: SlippageBps) -> Result<Price, SimError> {
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
    use parangsae_core::{Price, Qty, Side, UtcMicros};

    use super::{slippage_cost, taker_fee};
    use crate::types::{FeeBps, SimError, SlippageBps};

    #[test]
    fn two_bps_on_one_btc_at_one_hundred_costs_two_cents() {
        let open = Price::from_ticks(100 * 100_000_000).unwrap();
        let qty = Qty::from_ticks(100_000_000).unwrap();
        let cost = slippage_cost(open, Side::Buy, qty, SlippageBps(2)).unwrap();
        assert_eq!(cost.0, 2_000_000);
        let cost = slippage_cost(open, Side::Sell, qty, SlippageBps(2)).unwrap();
        assert_eq!(cost.0, 2_000_000);
    }

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
