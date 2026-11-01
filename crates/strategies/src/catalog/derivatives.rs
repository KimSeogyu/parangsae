//! Supplied-data carry/option research calculations, never executable orders.
//! All rates are same-horizon basis points. Options are European, one whole unit,
//! with no early exercise, contract multiplier, margin, path or fill simulation.

use parangsae_core::{Causal, DecisionTime, Price};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivativeError {
    InvalidTerms,
    AfterDecision,
    Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarryAssessment {
    Positive { net_bps: i64 },
    NonPositive { net_bps: i64 },
}

/// Expected same-horizon income, financing and total round-trip trading costs.
/// Funding receipts are not known income: this is a supplied forecast, not C1 admission.
#[derive(Clone, Copy, Debug)]
pub struct CarryTerms {
    income_bps: i64,
    financing_bps: i64,
    costs_bps: i64,
}
impl CarryTerms {
    pub fn new(
        income_bps: i64,
        financing_bps: i64,
        costs_bps: i64,
    ) -> Result<Self, DerivativeError> {
        if financing_bps < 0 || costs_bps < 0 {
            return Err(DerivativeError::InvalidTerms);
        }
        Ok(Self {
            income_bps,
            financing_bps,
            costs_bps,
        })
    }
}

pub fn carry(
    terms: &Causal<'_, CarryTerms>,
    decision: DecisionTime,
) -> Result<CarryAssessment, DerivativeError> {
    visible(terms, decision)?;
    let terms = terms.value();
    let net_bps = terms
        .income_bps
        .checked_sub(terms.financing_bps)
        .and_then(|n| n.checked_sub(terms.costs_bps))
        .ok_or(DerivativeError::Overflow)?;
    Ok(if net_bps > 0 {
        CarryAssessment::Positive { net_bps }
    } else {
        CarryAssessment::NonPositive { net_bps }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolatilityAssessment {
    LongOptionDeltaHedged { variance_edge: i64 },
    ShortOptionDeltaHedged { variance_edge: i64 },
    Neutral { variance_edge: i64 },
}
/// Both variances use the same caller-declared horizon and squared-return scale.
/// Cost buffer is in variance units, not currency. No volatility model is fitted.
#[derive(Clone, Copy, Debug)]
pub struct VolatilityTerms {
    implied_variance: i64,
    forecast_variance: i64,
    cost_buffer: i64,
}
impl VolatilityTerms {
    pub fn new(
        implied_variance: i64,
        forecast_variance: i64,
        cost_buffer: i64,
    ) -> Result<Self, DerivativeError> {
        if implied_variance < 0 || forecast_variance < 0 || cost_buffer < 0 {
            return Err(DerivativeError::InvalidTerms);
        }
        Ok(Self {
            implied_variance,
            forecast_variance,
            cost_buffer,
        })
    }
}
pub fn volatility_arbitrage(
    terms: &Causal<'_, VolatilityTerms>,
    decision: DecisionTime,
) -> Result<VolatilityAssessment, DerivativeError> {
    visible(terms, decision)?;
    let terms = terms.value();
    let variance_edge = terms.forecast_variance - terms.implied_variance;
    Ok(if variance_edge > terms.cost_buffer {
        VolatilityAssessment::LongOptionDeltaHedged { variance_edge }
    } else if variance_edge < -terms.cost_buffer {
        VolatilityAssessment::ShortOptionDeltaHedged { variance_edge }
    } else {
        VolatilityAssessment::Neutral { variance_edge }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    CoveredCall,
    CashSecuredPut,
}
/// One-unit payoff template assuming covered underlying or strike cash.
/// The returned collateral requirement is not evidence that a portfolio holds it.
#[derive(Clone, Copy, Debug)]
pub struct CoveredOption {
    kind: OptionKind,
    strike: Price,
    entry_spot: Price,
    premium: Price,
    cost: Price,
}
impl CoveredOption {
    pub fn new(
        kind: OptionKind,
        strike: Price,
        entry_spot: Price,
        premium: Price,
        cost: Price,
    ) -> Result<Self, DerivativeError> {
        if strike.ticks() <= 0 || entry_spot.ticks() <= 0 {
            return Err(DerivativeError::InvalidTerms);
        }
        Ok(Self {
            kind,
            strike,
            entry_spot,
            premium,
            cost,
        })
    }
    pub fn required_collateral_ticks(&self) -> i64 {
        match self.kind {
            OptionKind::CoveredCall => self.entry_spot.ticks(),
            OptionKind::CashSecuredPut => self.strike.ticks(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionPayoff {
    pub pnl_ticks: i64,
    pub required_collateral_ticks: i64,
}
/// Expiry scenario only; supplied scenario price must itself be visible at decision.
/// This does not identify a listed contract or validate its actual expiry/calendar;
/// no order, inventory, cash reservation or exercise instruction is constructed.
pub fn option_payoff(
    contract: &Causal<'_, CoveredOption>,
    expiry: &Causal<'_, Price>,
    decision: DecisionTime,
) -> Result<OptionPayoff, DerivativeError> {
    visible(contract, decision)?;
    visible(expiry, decision)?;
    if expiry.available_ts() < contract.available_ts() {
        return Err(DerivativeError::InvalidTerms);
    }
    let contract = contract.value();
    let spot = expiry.value().ticks();
    let pnl = match contract.kind {
        OptionKind::CoveredCall => {
            i128::from(spot.min(contract.strike.ticks())) - i128::from(contract.entry_spot.ticks())
        }
        OptionKind::CashSecuredPut => -i128::from((contract.strike.ticks() - spot).max(0)),
    } + i128::from(contract.premium.ticks())
        - i128::from(contract.cost.ticks());
    Ok(OptionPayoff {
        pnl_ticks: i64::try_from(pnl).map_err(|_| DerivativeError::Overflow)?,
        required_collateral_ticks: contract.required_collateral_ticks(),
    })
}
fn visible<T>(view: &Causal<'_, T>, decision: DecisionTime) -> Result<(), DerivativeError> {
    if view.available_ts().micros() > decision.micros() {
        Err(DerivativeError::AfterDecision)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};
    fn at(t: i64) -> DecisionTime {
        DecisionTime::from_micros(UtcMicros::from_micros(t))
    }
    fn obs<T>(t: i64, value: T) -> Observation<T> {
        Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(UtcMicros::from_micros(t)),
                available_ts: AvailableTs::from_micros(UtcMicros::from_micros(t)),
                received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(t)),
            },
            value,
        )
    }
    fn price(n: i64) -> Price {
        Price::from_ticks(n).expect("nonnegative price")
    }
    #[test]
    fn carry_subtracts_financing_and_roundtrip_costs() {
        let terms = obs(1, CarryTerms::new(90, 20, 10).expect("terms"));
        assert_eq!(
            carry(&terms.causal_at(at(1)).expect("visible"), at(1)),
            Ok(CarryAssessment::Positive { net_bps: 60 })
        );
    }
    #[test]
    fn volatility_uses_forecast_minus_implied_with_strict_cost_boundary() {
        let terms = obs(1, VolatilityTerms::new(100, 140, 20).expect("terms"));
        assert_eq!(
            volatility_arbitrage(&terms.causal_at(at(1)).expect("visible"), at(1)),
            Ok(VolatilityAssessment::LongOptionDeltaHedged { variance_edge: 40 })
        );
    }
    #[test]
    fn covered_call_caps_upside_and_secured_put_retains_downside() {
        let call = obs(
            1,
            CoveredOption::new(
                OptionKind::CoveredCall,
                price(110),
                price(100),
                price(5),
                price(1),
            )
            .expect("contract"),
        );
        let expiry = obs(2, price(150));
        assert_eq!(
            option_payoff(
                &call.causal_at(at(2)).expect("visible"),
                &expiry.causal_at(at(2)).expect("visible"),
                at(2)
            ),
            Ok(OptionPayoff {
                pnl_ticks: 14,
                required_collateral_ticks: 100
            })
        );
    }
    #[test]
    fn costs_can_eliminate_carry_and_overflow_is_an_error() {
        for (income, expected) in [(30, 0), (29, -1)] {
            let terms = obs(1, CarryTerms::new(income, 20, 10).expect("terms"));
            assert_eq!(
                carry(&terms.causal_at(at(1)).expect("visible"), at(1)),
                Ok(CarryAssessment::NonPositive { net_bps: expected })
            );
        }
        let terms = obs(1, CarryTerms::new(i64::MIN, 1, 0).expect("terms"));
        assert_eq!(
            carry(&terms.causal_at(at(1)).expect("visible"), at(1)),
            Err(DerivativeError::Overflow)
        );
        assert!(CarryTerms::new(0, -1, 0).is_err());
    }
    #[test]
    fn strict_variance_boundary_and_future_reuse_are_enforced() {
        for (forecast, expected) in [
            (120, VolatilityAssessment::Neutral { variance_edge: 20 }),
            (80, VolatilityAssessment::Neutral { variance_edge: -20 }),
            (
                79,
                VolatilityAssessment::ShortOptionDeltaHedged { variance_edge: -21 },
            ),
        ] {
            let terms = obs(10, VolatilityTerms::new(100, forecast, 20).expect("terms"));
            let view = terms.causal_at(at(10)).expect("visible");
            assert_eq!(volatility_arbitrage(&view, at(10)), Ok(expected));
            assert_eq!(
                volatility_arbitrage(&view, at(9)),
                Err(DerivativeError::AfterDecision)
            );
        }
    }
    #[test]
    fn puts_are_cash_secured_not_risk_free() {
        let put = obs(
            1,
            CoveredOption::new(
                OptionKind::CashSecuredPut,
                price(100),
                price(100),
                price(5),
                price(1),
            )
            .expect("contract"),
        );
        let expiry = obs(2, price(0));
        assert_eq!(
            option_payoff(
                &put.causal_at(at(2)).expect("visible"),
                &expiry.causal_at(at(2)).expect("visible"),
                at(2)
            ),
            Ok(OptionPayoff {
                pnl_ticks: -96,
                required_collateral_ticks: 100
            })
        );
        let large = obs(
            1,
            CoveredOption::new(
                OptionKind::CoveredCall,
                price(i64::MAX),
                price(1),
                price(i64::MAX),
                price(0),
            )
            .expect("contract"),
        );
        let high = obs(2, price(i64::MAX));
        assert_eq!(
            option_payoff(
                &large.causal_at(at(2)).expect("visible"),
                &high.causal_at(at(2)).expect("visible"),
                at(2)
            ),
            Err(DerivativeError::Overflow)
        );
    }
    proptest::proptest! {
        #[test]
        fn call_upside_is_capped_and_put_loss_is_bounded(spot in 0_i64..1_000_000) {
            let expiry = obs(2, price(spot));
            let call = obs(1, CoveredOption::new(OptionKind::CoveredCall, price(110), price(100), price(5), price(1)).expect("contract"));
            let put = obs(1, CoveredOption::new(OptionKind::CashSecuredPut, price(100), price(100), price(5), price(1)).expect("contract"));
            let cv = option_payoff(&call.causal_at(at(2)).expect("visible"), &expiry.causal_at(at(2)).expect("visible"), at(2)).expect("bounded payoff");
            let pv = option_payoff(&put.causal_at(at(2)).expect("visible"), &expiry.causal_at(at(2)).expect("visible"), at(2)).expect("bounded payoff");
            proptest::prop_assert!(cv.pnl_ticks <= 14);
            proptest::prop_assert!((-96..=4).contains(&pv.pnl_ticks));
        }
    }
}
