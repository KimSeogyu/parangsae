//! Offline allocation proposals, not RiskGate approvals.
//!
//! Inverse-volatility weights equalize risk only under the explicitly diagonal
//! (zero-correlation) covariance assumption. They are not a general ERC solver.
//! No portfolio, account or execution state is changed by an allocation.
use parangsae_core::{Causal, DecisionTime, Symbol};

const SCALE: i64 = 100_000_000;
const RECIPROCAL_SCALE: i128 = 1_000_000_000_000;
const MAX_ASSETS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationError {
    Capacity,
    DuplicateSymbol,
    Volatility,
    Future,
    Clock,
    Threshold,
    Resolution,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiskEstimates {
    assets: Vec<(Symbol, u32)>,
}
impl RiskEstimates {
    /// Supplied forecast volatility in basis points over one common horizon.
    /// The caller must establish the common horizon and provenance; no estimator
    /// or missing-value imputation is hidden here.
    pub fn new(mut assets: Vec<(Symbol, u32)>) -> Result<Self, AllocationError> {
        if assets.is_empty() || assets.len() > MAX_ASSETS {
            return Err(AllocationError::Capacity);
        }
        if assets.iter().any(|x| x.1 == 0 || x.1 > 1_000_000) {
            return Err(AllocationError::Volatility);
        }
        assets.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        if assets.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(AllocationError::DuplicateSymbol);
        }
        Ok(Self { assets })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    weights: Vec<(Symbol, i64)>,
}
impl Allocation {
    pub fn weights(&self) -> &[(Symbol, i64)] {
        &self.weights
    }
}

/// Quantized inverse-volatility allocation. Positive weights sum exactly to 1e8.
/// Refuses `Resolution` if quantization would silently remove an admitted asset.
/// Reciprocal volatilities use 1e12 fixed precision, then largest remainders are
/// assigned deterministically (symbol breaks ties). This is a proposal only.
pub fn inverse_volatility(
    input: &Causal<'_, RiskEstimates>,
    decision: DecisionTime,
) -> Result<Allocation, AllocationError> {
    if input.available_ts().micros() > decision.micros() {
        return Err(AllocationError::Future);
    }
    let assets = &input.value().assets;
    let scores: Vec<i128> = assets
        .iter()
        .map(|x| RECIPROCAL_SCALE / i128::from(x.1))
        .collect();
    let total: i128 = scores.iter().sum(); // <=128 * 1e12 by construction
    let mut weights: Vec<(Symbol, i64)> = assets
        .iter()
        .zip(&scores)
        .map(|(a, s)| {
            (
                a.0.clone(),
                i64::try_from(s * i128::from(SCALE) / total).expect("bounded unit weight"),
            )
        })
        .collect();
    let mut remainders: Vec<(usize, i128)> = scores
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s * i128::from(SCALE) % total))
        .collect();
    remainders.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let leftover = SCALE - weights.iter().map(|x| x.1).sum::<i64>();
    for (i, _) in remainders
        .into_iter()
        .take(usize::try_from(leftover).expect("nonnegative remainder"))
    {
        weights[i].1 += 1;
    }
    if weights.iter().any(|(_, weight)| *weight == 0) {
        return Err(AllocationError::Resolution);
    }
    Ok(Allocation { weights })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volatility(u32);
impl Volatility {
    pub fn new(bps: u32) -> Result<Self, AllocationError> {
        if bps > 1_000_000 {
            Err(AllocationError::Volatility)
        } else {
            Ok(Self(bps))
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Regime {
    Normal,
    Defensive,
}
impl Regime {
    /// Illustrative risk-budget multiplier; no risk permission is minted.
    pub const fn budget_ticks(self) -> i64 {
        match self {
            Self::Normal => SCALE,
            Self::Defensive => SCALE / 2,
        }
    }
}
#[derive(Debug)]
pub struct VolatilityRegime {
    low: u32,
    high: u32,
    current: Regime,
    last: Option<DecisionTime>,
}
impl VolatilityRegime {
    pub fn new(low: u32, high: u32) -> Result<Self, AllocationError> {
        if low >= high || high > 1_000_000 {
            return Err(AllocationError::Threshold);
        }
        Ok(Self {
            low,
            high,
            current: Regime::Normal,
            last: None,
        })
    }
    pub const fn regime(&self) -> Regime {
        self.current
    }
    /// Hysteresis on a supplied causal forecast. Each strictly later decision
    /// requires a newly published observation; stale reuse cannot change state.
    pub fn observe(
        &mut self,
        input: &Causal<'_, Volatility>,
        decision: DecisionTime,
    ) -> Result<Regime, AllocationError> {
        if input.available_ts().micros() > decision.micros() {
            return Err(AllocationError::Future);
        }
        if self
            .last
            .is_some_and(|last| decision <= last || input.available_ts().micros() <= last.micros())
        {
            return Err(AllocationError::Clock);
        }
        let next = match self.current {
            Regime::Normal if input.value().0 >= self.high => Regime::Defensive,
            Regime::Defensive if input.value().0 <= self.low => Regime::Normal,
            other => other,
        };
        self.current = next;
        self.last = Some(decision);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};
    fn at(t: i64) -> DecisionTime {
        DecisionTime::from_micros(UtcMicros::from_micros(t))
    }
    fn obs<T>(t: i64, v: T) -> Observation<T> {
        Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(UtcMicros::from_micros(t)),
                available_ts: AvailableTs::from_micros(UtcMicros::from_micros(t)),
                received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(t)),
            },
            v,
        )
    }
    #[test]
    fn diagonal_equal_risk_is_not_equal_capital() {
        let rows = RiskEstimates::new(vec![
            (Symbol::new("BTCUSDT").unwrap(), 100),
            (Symbol::new("ETHUSDT").unwrap(), 200),
        ])
        .unwrap();
        let o = obs(1, rows);
        let a = inverse_volatility(&o.causal_at(at(1)).unwrap(), at(1)).unwrap();
        assert_eq!(a.weights()[0].1, 66_666_667);
        assert_eq!(a.weights()[1].1, 33_333_333);
        assert_eq!(a.weights().iter().map(|x| x.1).sum::<i64>(), 100_000_000);
    }
    #[test]
    fn unrepresentable_positive_weights_are_explicitly_refused() {
        let vols: Vec<u32> = std::iter::repeat_n(1, 100)
            .chain(std::iter::repeat_n(2, 22))
            .chain(std::iter::repeat_n(3, 5))
            .chain([1_000_000])
            .collect();
        let assets = vols
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Symbol::new(&format!("A{i:03}")).unwrap(), v))
            .collect();
        let o = obs(1, RiskEstimates::new(assets).unwrap());
        let result = inverse_volatility(&o.causal_at(at(1)).unwrap(), at(1));
        assert_eq!(result, Err(AllocationError::Resolution));
    }

    #[test]
    fn bad_estimates_and_future_gate_refuse() {
        assert_eq!(
            RiskEstimates::new(vec![(Symbol::new("BTCUSDT").unwrap(), 0)]),
            Err(AllocationError::Volatility)
        );
        let s = Symbol::new("BTCUSDT").unwrap();
        assert_eq!(
            RiskEstimates::new(vec![(s.clone(), 10), (s, 20)]),
            Err(AllocationError::DuplicateSymbol)
        );
        let o = obs(
            2,
            RiskEstimates::new(vec![(Symbol::new("BTCUSDT").unwrap(), 100)]).unwrap(),
        );
        assert_eq!(
            inverse_volatility(&o.causal_at(at(2)).unwrap(), at(1)),
            Err(AllocationError::Future)
        );
    }
    #[test]
    fn regime_hysteresis_and_refusal_preserve_state() {
        let mut r = VolatilityRegime::new(100, 200).unwrap();
        assert_eq!(
            r.observe(
                &obs(1, Volatility::new(250).unwrap())
                    .causal_at(at(1))
                    .unwrap(),
                at(1)
            )
            .unwrap(),
            Regime::Defensive
        );
        assert_eq!(
            r.observe(
                &obs(2, Volatility::new(150).unwrap())
                    .causal_at(at(2))
                    .unwrap(),
                at(2)
            )
            .unwrap(),
            Regime::Defensive
        );
        assert_eq!(
            r.observe(
                &obs(3, Volatility::new(100).unwrap())
                    .causal_at(at(3))
                    .unwrap(),
                at(3)
            )
            .unwrap(),
            Regime::Normal
        );
        assert_eq!(
            r.observe(
                &obs(2, Volatility::new(300).unwrap())
                    .causal_at(at(2))
                    .unwrap(),
                at(2)
            ),
            Err(AllocationError::Clock)
        );
        assert_eq!(r.regime(), Regime::Normal);
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn allocation_conserves_and_is_permutation_invariant(v in proptest::collection::vec(1u32..1_000_001,1..30)) {
            let rows:Vec<_>=v.iter().enumerate().map(|(i,x)|(Symbol::new(&format!("A{i}")).unwrap(),*x)).collect();
            let mut reverse=rows.clone();reverse.reverse();
            let stamp=UtcMicros::from_micros(1);let decision=DecisionTime::from_micros(stamp);
            let ts=Timestamps{event_ts:EventTs::from_micros(stamp),available_ts:AvailableTs::from_micros(stamp),received_ts:ReceivedTs::from_micros(stamp)};
            let a=Observation::new(ts,RiskEstimates::new(rows).unwrap());
            let b=Observation::new(ts,RiskEstimates::new(reverse).unwrap());
            let result=inverse_volatility(&a.causal_at(decision).unwrap(),decision);
            prop_assert_eq!(&result,&inverse_volatility(&b.causal_at(decision).unwrap(),decision));
            match result {
                Ok(weights) => {
                    prop_assert_eq!(weights.weights().iter().map(|x|x.1).sum::<i64>(),SCALE);
                    prop_assert!(weights.weights().iter().all(|x|x.1>0));
                }
                Err(error) => prop_assert_eq!(error, AllocationError::Resolution),
            }
        }
    }
}
