//! The declared bar-based source supplies evidence, never account authority.

use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Price, Qty, ReceivedTs, Side, Timestamps,
    UtcMicros,
};
use parangsae_execution::carry::{
    CarryFill, CarryOrderId, CarryReport, CarryStage, CarryTerminal, CarryTerminalCut,
};
use parangsae_sim::carry::Market;
use parangsae_sim::{fill_price, SlippageBps};

use super::sizing::PriceBand;

const MILLIS: i64 = 1_000;
const MINUTE: i64 = 60_000_000;
const FUNDING_STEP: i64 = 28_800_000_000;
const FUNDING_EXCLUSION: i64 = 15_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VenueError {
    Clock,
    Price,
    Quantity,
    Overflow,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct OrderTerms {
    pub id: CarryOrderId,
    pub qty: Qty,
    pub native_upper: Price,
    pub fresh_band: PriceBand,
    pub dispatched: DecisionTime,
    pub deadline: DecisionTime,
    pub slippage: SlippageBps,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RawExecution {
    pub open: UtcMicros,
    pub reference_open: Price,
    pub count: i64,
    pub source_available: UtcMicros,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitReason {
    NotForward,
    NotVisible,
    ZeroCount,
    FundingExclusion,
    OutsideBand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Candidate {
    Wait(WaitReason),
    Filled(PreparedFill),
    Deadline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreparedFill {
    pub source: Observation<CarryFill>,
    pub expected_executed: Price,
    pub delivery: DecisionTime,
}

impl PreparedFill {
    pub(super) fn report(&self) -> Observation<CarryReport> {
        self.source
            .with_value(CarryReport::Fill(self.source.value().clone()))
    }

    /// Valid only for this declared all-remaining fill model after accepted accounting.
    pub(super) fn filled_cut(&self) -> Observation<CarryTerminalCut> {
        let fill = self.source.value();
        self.source.with_value(CarryTerminalCut {
            order: fill.order,
            evidence_id: 1,
            status: CarryTerminal::Filled,
            cumulative: fill.qty,
            receipts: vec![self.source.clone()],
            complete: true,
        })
    }
}

pub(super) fn consider(
    terms: OrderTerms,
    raw: RawExecution,
    at: DecisionTime,
) -> Result<Candidate, VenueError> {
    if terms.qty.ticks() <= 0 {
        return Err(VenueError::Quantity);
    }
    if at.micros().micros().rem_euclid(MILLIS) != 0 || raw.open.micros().rem_euclid(MINUTE) != 0 {
        return Err(VenueError::Clock);
    }
    if at > terms.deadline {
        return Ok(Candidate::Deadline);
    }
    if raw.open <= terms.dispatched.micros() {
        return Ok(Candidate::Wait(WaitReason::NotForward));
    }
    let delivery = ceil_ms(raw.source_available)?;
    if at.micros() < delivery {
        return Ok(Candidate::Wait(WaitReason::NotVisible));
    }
    if raw.count < 0 {
        return Err(VenueError::Quantity);
    }
    if raw.count == 0 {
        return Ok(Candidate::Wait(WaitReason::ZeroCount));
    }
    if raw.reference_open.ticks() <= 0 {
        return Err(VenueError::Price);
    }
    let (market, side) = market_side(terms.id.stage);
    if market == Market::UsdMPerp && funding_excluded(raw.open) {
        return Ok(Candidate::Wait(WaitReason::FundingExclusion));
    }
    // The computed price is for eligibility/audit. The core receives the ORIGINAL
    // raw open and applies the same slippage once to that unmodified reference.
    let executed =
        fill_price(raw.reference_open, side, terms.slippage).map_err(|_| VenueError::Overflow)?;
    if !terms.fresh_band.contains(executed) || executed > terms.native_upper {
        return Ok(Candidate::Wait(WaitReason::OutsideBand));
    }
    let source = Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(raw.open),
            available_ts: AvailableTs::from_micros(raw.source_available),
            received_ts: ReceivedTs::from_micros(delivery),
        },
        CarryFill {
            order: terms.id,
            fill_id: 1,
            market,
            qty: terms.qty,
            reference_price: raw.reference_open,
        },
    );
    Ok(Candidate::Filled(PreparedFill {
        source,
        expected_executed: executed,
        delivery: DecisionTime::from_micros(delivery),
    }))
}

pub(super) fn ceil_ms(value: UtcMicros) -> Result<UtcMicros, VenueError> {
    let raw = value.micros();
    let rem = raw.rem_euclid(MILLIS);
    Ok(UtcMicros::from_micros(if rem == 0 {
        raw
    } else {
        raw.checked_add(MILLIS - rem).ok_or(VenueError::Overflow)?
    }))
}

fn funding_excluded(open: UtcMicros) -> bool {
    let since = open.micros().rem_euclid(FUNDING_STEP);
    since <= FUNDING_EXCLUSION || FUNDING_STEP - since <= FUNDING_EXCLUSION
}

fn market_side(stage: CarryStage) -> (Market, Side) {
    match stage {
        CarryStage::SpotEntry => (Market::Spot, Side::Buy),
        CarryStage::PerpEntry => (Market::UsdMPerp, Side::Sell),
        CarryStage::PerpExit => (Market::UsdMPerp, Side::Buy),
        CarryStage::SpotExit => (Market::Spot, Side::Sell),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(raw: i64) -> UtcMicros {
        UtcMicros::from_micros(raw)
    }
    fn d(raw: i64) -> DecisionTime {
        DecisionTime::from_micros(ts(raw))
    }
    fn price(raw: i64) -> Price {
        Price::from_ticks(raw).expect("test price")
    }
    fn terms(stage: CarryStage) -> OrderTerms {
        OrderTerms {
            id: CarryOrderId {
                pair: 1,
                stage,
                attempt: 0,
            },
            qty: Qty::from_ticks(100_000_000).expect("oneBTC"),
            native_upper: price(10_100_000_000),
            fresh_band: PriceBand::at(price(10_000_000_000)).expect("band"),
            dispatched: d(1_000),
            deadline: d(600_000_000),
            slippage: SlippageBps(2),
        }
    }
    fn raw() -> RawExecution {
        RawExecution {
            open: ts(MINUTE),
            reference_open: price(10_001_000_000),
            count: 1,
            source_available: ts(2 * MINUTE + 999),
        }
    }

    #[test]
    fn no_fill_is_exposed_before_exact_ceil_ms_publication() {
        assert_eq!(
            ceil_ms(ts(2 * MINUTE + 999)).expect("ceil"),
            ts(2 * MINUTE + 1_000)
        );
        assert_eq!(
            consider(terms(CarryStage::SpotEntry), raw(), d(2 * MINUTE)).expect("pending"),
            Candidate::Wait(WaitReason::NotVisible)
        );
        let Candidate::Filled(fill) =
            consider(terms(CarryStage::SpotEntry), raw(), d(2 * MINUTE + 1_000)).expect("visible")
        else {
            panic!("must fill")
        };
        assert_eq!(fill.source.value().reference_price, raw().reference_open);
        assert_eq!(fill.expected_executed.ticks(), 10_003_000_200);
        assert_eq!(fill.source.timestamps().event_ts.micros(), ts(MINUTE));
        assert_eq!(
            fill.source.timestamps().available_ts.micros(),
            ts(2 * MINUTE + 999)
        );
        assert_eq!(
            fill.source.timestamps().received_ts.micros(),
            ts(2 * MINUTE + 1_000)
        );
        let cut = fill.filled_cut();
        assert_eq!(cut.value().cumulative, terms(CarryStage::SpotEntry).qty);
        assert_eq!(cut.value().receipts.len(), 1);
        assert!(cut.value().complete);
    }

    #[test]
    fn sell_lower_bound_and_immutable_native_upper_both_bind() {
        let mut below = raw();
        below.reference_open = price(9_899_999_900);
        assert_eq!(
            consider(terms(CarryStage::PerpEntry), below, d(2 * MINUTE + 1_000))
                .expect("below lower"),
            Candidate::Wait(WaitReason::OutsideBand)
        );
        let mut capped = terms(CarryStage::PerpEntry);
        capped.fresh_band = PriceBand::at(price(10_500_000_000)).expect("D1band");
        let mut risen = raw();
        risen.reference_open = price(10_400_000_000);
        assert_eq!(
            consider(capped, risen, d(2 * MINUTE + 1_000)).expect("oldD0upper"),
            Candidate::Wait(WaitReason::OutsideBand)
        );
    }

    #[test]
    fn zero_count_same_open_and_postdeadline_never_make_receipts() {
        let mut filler = raw();
        filler.count = 0;
        assert_eq!(
            consider(terms(CarryStage::SpotEntry), filler, d(2 * MINUTE + 1_000)).expect("filler"),
            Candidate::Wait(WaitReason::ZeroCount)
        );
        let mut same = terms(CarryStage::SpotEntry);
        same.dispatched = d(MINUTE);
        assert_eq!(
            consider(same, raw(), d(2 * MINUTE + 1_000)).expect("already processed"),
            Candidate::Wait(WaitReason::NotForward)
        );
        assert_eq!(
            consider(terms(CarryStage::SpotEntry), raw(), d(600_001_000)).expect("deadline"),
            Candidate::Deadline
        );
    }

    #[test]
    fn a_nominal_funding_open_cannot_receive_a_new_perp_fill() {
        let mut candidate = raw();
        candidate.open = ts(FUNDING_STEP);
        candidate.source_available = ts(FUNDING_STEP + MINUTE);
        let mut order = terms(CarryStage::PerpEntry);
        order.dispatched = d(FUNDING_STEP - MINUTE);
        order.deadline = d(FUNDING_STEP + 600_000_000);
        assert_eq!(
            consider(order, candidate, d(FUNDING_STEP + MINUTE)).expect("funding exclusion"),
            Candidate::Wait(WaitReason::FundingExclusion)
        );
    }
}
