//! Decision money derived from the economic ledger and bounded funding dependencies.

use parangsae_core::{AvailableTs, Bar, Price, UtcMicros};
use parangsae_sim::{Cash, Ledger};

use crate::bar::FundingStamp;
use crate::error::EngineError;

#[derive(Clone, Copy)]
pub(crate) struct FundingDependency {
    settlement: UtcMicros,
    original_mark: UtcMicros,
    available: AvailableTs,
}

impl FundingDependency {
    pub(crate) fn from_sources(rate: &FundingStamp, original_mark: &Bar) -> Self {
        Self {
            settlement: rate.funding_time,
            original_mark: original_mark.open_time,
            available: AvailableTs::from_micros(rate.available.max(original_mark.available)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisibleMoney {
    cash: Cash,
    equity: Cash,
}

impl VisibleMoney {
    pub(crate) fn equity(self) -> VisibleEquity {
        VisibleEquity(self.equity)
    }

    pub(crate) fn cash(self) -> Cash {
        self.cash
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisibleEquity(Cash);

impl VisibleEquity {
    pub(crate) fn ticks(self) -> i64 {
        self.0 .0
    }
}

#[derive(Clone, Copy, Debug)]
struct UnavailableCash {
    settlement: UtcMicros,
    original_mark: UtcMicros,
    available: UtcMicros,
    amount: Cash,
}

pub(crate) struct CashVisibility {
    pending: Vec<UnavailableCash>,
    capacity: usize,
    last_settlement: Option<UtcMicros>,
    last_view: Option<UtcMicros>,
}

pub(crate) struct CashReservation<'a> {
    owner: &'a mut CashVisibility,
    settlement: UtcMicros,
    original_mark: UtcMicros,
    available: UtcMicros,
}

impl CashReservation<'_> {
    pub(crate) fn commit(self, amount: Cash) {
        self.owner.pending.push(UnavailableCash {
            settlement: self.settlement,
            original_mark: self.original_mark,
            available: self.available,
            amount,
        });
        self.owner.last_settlement = Some(self.settlement);
    }
}

impl CashVisibility {
    /// The validated tape's finite funding-slot count bounds every reservation.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            pending: Vec::with_capacity(capacity),
            capacity,
            last_settlement: None,
            last_view: None,
        }
    }

    /// Validate metadata and capacity before any economic arithmetic or mutation.
    pub(crate) fn reserve(
        &mut self,
        dependency: FundingDependency,
    ) -> Result<CashReservation<'_>, EngineError> {
        let settlement = dependency.settlement;
        if self.last_settlement.is_some_and(|last| last >= settlement) {
            return Err(EngineError::MoneyClock { at: settlement });
        }
        if self.pending.len() >= self.capacity {
            return Err(EngineError::MoneyCapacity { at: settlement });
        }
        Ok(CashReservation {
            owner: self,
            settlement,
            original_mark: dependency.original_mark,
            available: dependency.available.micros(),
        })
    }

    pub(crate) fn project(
        &mut self,
        ledger: &Ledger,
        price: Price,
        at: UtcMicros,
    ) -> Result<VisibleMoney, EngineError> {
        // Even if subtraction could make the visible value fit, an unrepresentable
        // economic ledger/equity aborts the complete run. It is never a return.
        let economic = ledger
            .equity(price)
            .map_err(|_| EngineError::EconomicArithmetic { at })?;
        let hidden = self.hidden(at)?;
        let cash = i128::from(ledger.cash().0)
            .checked_sub(hidden)
            .and_then(|value| i64::try_from(value).ok())
            .map(Cash)
            .ok_or(EngineError::EconomicArithmetic { at })?;
        let equity = i128::from(economic.0)
            .checked_sub(hidden)
            .and_then(|value| i64::try_from(value).ok())
            .map(Cash)
            .ok_or(EngineError::EconomicArithmetic { at })?;
        self.pending.retain(|row| row.available > at);
        self.last_view = Some(at);
        Ok(VisibleMoney { cash, equity })
    }

    fn hidden(&self, at: UtcMicros) -> Result<i128, EngineError> {
        if self.last_view.is_some_and(|last| last > at) {
            return Err(EngineError::MoneyClock { at });
        }
        let hidden = self
            .pending
            .iter()
            .filter(|row| row.available > at)
            .try_fold(0i128, |sum, row| sum.checked_add(i128::from(row.amount.0)))
            .ok_or(EngineError::EconomicArithmetic { at })?;
        Ok(hidden)
    }

    pub(crate) fn unavailable_dependencies(
        &self,
        at: UtcMicros,
    ) -> impl Iterator<Item = (UtcMicros, UtcMicros)> + '_ {
        self.pending
            .iter()
            .filter(move |row| row.available > at)
            .map(|row| (row.settlement, row.original_mark))
    }
}
