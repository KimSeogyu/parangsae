//! Pure offline plans, never RiskGate approvals or venue orders.
//!
//! Quantities are integer venue lots whose size the caller must establish.
//! Plans do not assume fills, queue position, fees, forecast accuracy or live
//! eligibility. The normal paper risk/order lifecycle remains mandatory.
use parangsae_core::{Causal, DecisionTime, Price};
const MAX_SLICES: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanningError {
    Market,
    Inventory,
    Quantity,
    Tick,
    Clock,
    Future,
    Stale,
    Capacity,
    Profile,
    Overflow,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteState {
    bid: Price,
    ask: Price,
    inventory: i64,
    limit: i64,
}
impl QuoteState {
    pub fn new(
        bid_ticks: i64,
        ask_ticks: i64,
        inventory_lots: i64,
        limit_lots: i64,
    ) -> Result<Self, PlanningError> {
        if bid_ticks <= 0 || ask_ticks <= bid_ticks {
            return Err(PlanningError::Market);
        }
        if limit_lots <= 0 || i128::from(inventory_lots).abs() > i128::from(limit_lots) {
            return Err(PlanningError::Inventory);
        }
        Ok(Self {
            bid: Price::from_ticks(bid_ticks).map_err(|_| PlanningError::Market)?,
            ask: Price::from_ticks(ask_ticks).map_err(|_| PlanningError::Market)?,
            inventory: inventory_lots,
            limit: limit_lots,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteConfig {
    tick: i64,
    lots: i64,
    max_age: i64,
}
impl QuoteConfig {
    pub fn new(
        price_tick: i64,
        quote_lots: i64,
        max_age_micros: i64,
    ) -> Result<Self, PlanningError> {
        if price_tick <= 0 {
            return Err(PlanningError::Tick);
        }
        if quote_lots <= 0 {
            return Err(PlanningError::Quantity);
        }
        if max_age_micros < 0 {
            return Err(PlanningError::Clock);
        }
        Ok(Self {
            tick: price_tick,
            lots: quote_lots,
            max_age: max_age_micros,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotePlan {
    bid: Option<(i64, i64)>,
    ask: Option<(i64, i64)>,
}
impl QuotePlan {
    pub const fn bid(&self) -> Option<(i64, i64)> {
        self.bid
    }
    pub const fn ask(&self) -> Option<(i64, i64)> {
        self.ask
    }
}
/// One tick wider than an admitted top of book, rounded away from crossing.
/// A side is omitted if its full fill would exceed the inventory cap.
pub fn quotes(
    input: &Causal<'_, QuoteState>,
    decision: DecisionTime,
    config: QuoteConfig,
) -> Result<QuotePlan, PlanningError> {
    let age = decision
        .micros()
        .micros()
        .checked_sub(input.available_ts().micros().micros())
        .ok_or(PlanningError::Overflow)?;
    if age < 0 {
        return Err(PlanningError::Future);
    }
    if age > config.max_age {
        return Err(PlanningError::Stale);
    }
    let s = input.value();
    let tick = i128::from(config.tick);
    let bid = (i128::from(s.bid.ticks()) / tick - 1) * tick;
    let ask = ((i128::from(s.ask.ticks()) + tick - 1) / tick + 1) * tick;
    if bid <= 0 || ask > i128::from(i64::MAX) {
        return Err(PlanningError::Tick);
    }
    let can_buy = i128::from(s.inventory) + i128::from(config.lots) <= i128::from(s.limit);
    let can_sell = i128::from(s.inventory) - i128::from(config.lots) >= -i128::from(s.limit);
    Ok(QuotePlan {
        bid: can_buy.then_some((
            i64::try_from(bid).map_err(|_| PlanningError::Overflow)?,
            config.lots,
        )),
        ask: can_sell.then_some((
            i64::try_from(ask).map_err(|_| PlanningError::Overflow)?,
            config.lots,
        )),
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeProfile {
    weights: Vec<u32>,
}
impl VolumeProfile {
    /// Historical/forecast relative volume for future slices, not future realized
    /// volume. All weights positive; zero-volume slots must be excluded upstream.
    pub fn new(weights: Vec<u32>) -> Result<Self, PlanningError> {
        if weights.is_empty() || weights.len() > MAX_SLICES {
            return Err(PlanningError::Capacity);
        }
        if weights.contains(&0) {
            return Err(PlanningError::Profile);
        }
        Ok(Self { weights })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    slices: Vec<(i64, u64)>,
}
impl Schedule {
    pub fn slices(&self) -> &[(i64, u64)] {
        &self.slices
    }
}
pub fn twap(
    lots: u64,
    start_micros: i64,
    end_micros: i64,
    count: usize,
) -> Result<Schedule, PlanningError> {
    if count == 0 || count > MAX_SLICES {
        return Err(PlanningError::Capacity);
    }
    schedule(lots, start_micros, end_micros, &vec![1; count])
}
pub fn vwap(
    lots: u64,
    start_micros: i64,
    end_micros: i64,
    profile: &Causal<'_, VolumeProfile>,
    decision: DecisionTime,
) -> Result<Schedule, PlanningError> {
    if profile.available_ts().micros() > decision.micros() {
        return Err(PlanningError::Future);
    }
    if start_micros < decision.micros().micros() {
        return Err(PlanningError::Clock);
    }
    schedule(lots, start_micros, end_micros, &profile.value().weights)
}
fn schedule(lots: u64, start: i64, end: i64, weights: &[u32]) -> Result<Schedule, PlanningError> {
    let n = weights.len();
    let count = i128::try_from(n).map_err(|_| PlanningError::Capacity)?;
    if i128::from(lots) < count {
        return Err(PlanningError::Quantity);
    }
    let duration = i128::from(end) - i128::from(start);
    if start < 0 || duration < count {
        return Err(PlanningError::Clock);
    }
    let total: u128 = weights.iter().map(|v| u128::from(*v)).sum();
    let mut quantities: Vec<u64> = weights
        .iter()
        .map(|w| {
            u64::try_from(u128::from(lots) * u128::from(*w) / total).expect("bounded allocation")
        })
        .collect();
    let remainder = lots - quantities.iter().sum::<u64>();
    let mut ranks: Vec<(usize, u128)> = weights
        .iter()
        .enumerate()
        .map(|(i, w)| (i, u128::from(lots) * u128::from(*w) % total))
        .collect();
    ranks.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (i, _) in ranks
        .into_iter()
        .take(usize::try_from(remainder).map_err(|_| PlanningError::Overflow)?)
    {
        quantities[i] += 1;
    }
    if quantities.contains(&0) {
        return Err(PlanningError::Quantity);
    }
    let mut slices = Vec::with_capacity(n);
    for (i, q) in quantities.into_iter().enumerate() {
        let time = i128::from(start)
            + duration * i128::try_from(i).map_err(|_| PlanningError::Capacity)? / count;
        slices.push((i64::try_from(time).map_err(|_| PlanningError::Overflow)?, q));
    }
    Ok(Schedule { slices })
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
    fn quotes_never_cross_and_inventory_disables_increasing_side() {
        let o = obs(1, QuoteState::new(100, 102, 10, 10).unwrap());
        let plan = quotes(
            &o.causal_at(at(1)).unwrap(),
            at(1),
            QuoteConfig::new(2, 2, 5).unwrap(),
        )
        .unwrap();
        assert_eq!(plan.bid(), None);
        assert_eq!(plan.ask(), Some((104, 2)));
    }
    #[test]
    fn twap_and_historical_profile_conserve_exact_quantity() {
        let t = twap(10, 1, 10, 3).unwrap();
        assert_eq!(t.slices(), &[(1, 4), (4, 3), (7, 3)]);
        let o = obs(0, VolumeProfile::new(vec![1, 2, 1]).unwrap());
        let v = vwap(10, 1, 10, &o.causal_at(at(0)).unwrap(), at(0)).unwrap();
        assert_eq!(v.slices(), &[(1, 3), (4, 5), (7, 2)]);
    }
    #[test]
    fn impossible_inputs_are_refused() {
        assert_eq!(twap(2, 1, 3, 3), Err(PlanningError::Quantity));
        assert_eq!(VolumeProfile::new(vec![0, 0]), Err(PlanningError::Profile));
        assert_eq!(QuoteState::new(102, 100, 0, 10), Err(PlanningError::Market));
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
     #[test]
     fn twap_conserves_and_has_strictly_increasing_bounded_times(n in 1usize..100,lots in 100u64..u64::MAX) {
      let plan=twap(lots,1,10000,n).unwrap();
      prop_assert_eq!(plan.slices().iter().map(|x|u128::from(x.1)).sum::<u128>(),u128::from(lots));
      prop_assert!(plan.slices().iter().all(|x|x.0>=1 && x.0<10000 && x.1>0));
      prop_assert!(plan.slices().windows(2).all(|x|x[0].0<x[1].0));
     }
    }
}
