use cucumber::{then, when, World as _};
use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Price, Qty, Ratio, ReceivedTs, Side,
    Timestamps, UtcMicros,
};
use parangsae_sim::{
    CapturedFundingError, CapturedFundingFailure, Cash, Costs, FeeBps, FundingCapture,
    FundingCaptureError, FundingEvent, FundingRate, Ledger, MarketOrder, PositionQty,
    SettlementMark, SimError, SlippageBps,
};

const U: i64 = 100_000_000;
const S: i64 = 1_704_096_000_000_000; // Invented fixture clock, not observed historical receipt.
const R: i64 = S + 60_000_000;
const NEXT_S: i64 = S + 8 * 3_600_000_000;

fn time(v: i64) -> UtcMicros {
    UtcMicros::from_micros(v)
}
fn decision(v: i64) -> DecisionTime {
    DecisionTime::from_micros(time(v))
}
fn px(v: i64) -> Price {
    Price::from_ticks(v).unwrap()
}
fn qty(v: i64) -> Qty {
    Qty::from_ticks(v).unwrap()
}
fn costs() -> Costs {
    Costs {
        taker_fee: FeeBps(0),
        slippage: SlippageBps(0),
    }
}
fn observed<T>(value: T, slot: i64, available: i64) -> Observation<T> {
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(time(slot)),
            available_ts: AvailableTs::from_micros(time(available)),
            received_ts: ReceivedTs::from_micros(time(available)),
        },
        value,
    )
}
fn fill(ledger: &mut Ledger, side: Side, amount: i64, price: i64, at: i64, costs: &Costs) {
    ledger
        .apply_fill(
            MarketOrder {
                side,
                qty: qty(amount),
            },
            px(price),
            costs,
            time(at),
        )
        .unwrap();
}
fn short() -> Ledger {
    let mut ledger = Ledger::new(Cash(10_000 * U));
    fill(
        &mut ledger,
        Side::Sell,
        U,
        100 * U,
        S - 60_000_000,
        &costs(),
    );
    ledger
}
fn terms(
    slot: i64,
    rate: i64,
    price: i64,
    available: i64,
) -> (Observation<FundingRate>, Observation<SettlementMark>) {
    (
        observed(
            FundingRate {
                settlement: time(slot),
                rate: Ratio::from_ticks(rate),
            },
            slot,
            available,
        ),
        observed(
            SettlementMark {
                source_open: time(slot),
                price: px(price),
            },
            slot,
            available,
        ),
    )
}
fn apply(
    ledger: &mut Ledger,
    capture: FundingCapture,
    rate: &Observation<FundingRate>,
    mark: &Observation<SettlementMark>,
    at: i64,
) -> Result<FundingEvent, CapturedFundingFailure> {
    // A later-minted view is deliberately possible. The native method must recheck `at`.
    let minted = at
        .max(rate.timestamps().available_ts.micros().micros())
        .max(mark.timestamps().available_ts.micros().micros());
    ledger.apply_captured_funding(
        capture,
        rate.causal_at(decision(minted)).unwrap(),
        mark.causal_at(decision(minted)).unwrap(),
        decision(at),
    )
}

// The old API correctly uses current position. This is the new receipt-contract regression.
#[test]
fn late_receipt_requires_the_boundary_position_after_the_account_closes() {
    let mut ledger = short();
    let capture = ledger.capture_funding(time(S)).unwrap();
    let boundary_position = capture.position();
    fill(&mut ledger, Side::Buy, U, 100 * U, S + 1, &costs());
    assert_eq!(ledger.position(), PositionQty(0));
    let (rate, mark) = terms(S, 1_000_000, 100 * U, R);
    let event = apply(&mut ledger, capture, &rate, &mark, R).unwrap();
    assert_eq!(event.position, boundary_position);
    assert_eq!(event.position, PositionQty(-U));
    assert_eq!(event.amount, Cash(U));
    assert_eq!(event.ts, time(S));
    assert_eq!(ledger.position(), PositionQty(0));
}

#[derive(Debug)]
struct LateTrace {
    side: Side,
    rate: i64,
    before: Ledger,
    after: Ledger,
    event: FundingEvent,
}
fn late(side: Side, rate_ticks: i64) -> LateTrace {
    let fee_costs = Costs {
        taker_fee: FeeBps(5),
        slippage: SlippageBps(2),
    };
    let mut ledger = Ledger::new(Cash(10_000 * U));
    let amount = 9_990_000_000;
    fill(
        &mut ledger,
        side,
        amount,
        100 * U,
        S - 60_000_000,
        &fee_costs,
    );
    let capture = ledger.capture_funding(time(S)).unwrap();
    let opposite = if side == Side::Buy {
        Side::Sell
    } else {
        Side::Buy
    };
    fill(&mut ledger, opposite, amount, 100 * U, S + 1, &fee_costs);
    let before = ledger.clone();
    let (rate, mark) = terms(S, rate_ticks, 100 * U + 1, R);
    let event = apply(&mut ledger, capture, &rate, &mark, R).unwrap();
    LateTrace {
        side,
        rate: rate_ticks,
        before,
        after: ledger,
        event,
    }
}
fn assert_late(t: &LateTrace) {
    let expected_position = if t.side == Side::Buy {
        9_990_000_000
    } else {
        -9_990_000_000
    };
    let receives = (t.side == Side::Sell) == (t.rate > 0);
    let amount = if receives { 9_990 } else { -9_991 };
    assert_eq!(t.before.position(), PositionQty(0));
    assert_eq!(t.after.position(), PositionQty(0));
    assert_eq!(t.before.cash(), Cash(998_601_400_000));
    assert_eq!(t.before.fees_paid(), Cash(999_000_000));
    assert_eq!(t.after.fees_paid(), t.before.fees_paid());
    assert_eq!(t.event.position, PositionQty(expected_position));
    assert_eq!(t.event.ts, time(S));
    assert_eq!(t.event.mark, px(100 * U + 1));
    assert_eq!(t.event.rate, Ratio::from_ticks(t.rate));
    assert_eq!(t.event.amount, Cash(amount));
    assert_eq!(t.after.cash(), Cash(998_601_400_000 + amount));
    assert_eq!(t.after.funding_net(), Cash(amount));
    assert_eq!(t.after.equity(px(100 * U)).unwrap(), t.after.cash());
}

#[derive(Debug)]
struct Refusal {
    before: Ledger,
    after: Ledger,
    error: CapturedFundingError,
    expected: CapturedFundingError,
    returned_slot: UtcMicros,
    returned_position: PositionQty,
    retry: FundingEvent,
}
fn source_refusals() -> Vec<Refusal> {
    let mut out = Vec::new();
    for which in 0..8 {
        let mut ledger = short();
        let capture = ledger.capture_funding(time(S)).unwrap();
        let before = ledger.clone();
        let (mut rate, mut mark) = terms(S, 1_000_000, 100 * U, R);
        let (at, expected) = match which {
            0 => {
                rate = observed(
                    FundingRate {
                        settlement: time(S + 1),
                        rate: Ratio::from_ticks(1_000_000),
                    },
                    S,
                    R,
                );
                (R, CapturedFundingError::RateSlotMismatch)
            }
            1 => {
                mark = observed(
                    SettlementMark {
                        source_open: time(S - 60_000_000),
                        price: px(100 * U),
                    },
                    S,
                    R,
                );
                (R, CapturedFundingError::MarkSlotMismatch)
            }
            2 => (S - 1, CapturedFundingError::BeforeSettlement),
            3 => (S + 30_000_000, CapturedFundingError::RateUnavailable),
            4 => {
                rate = observed(*rate.value(), S, S);
                (S + 30_000_000, CapturedFundingError::MarkUnavailable)
            }
            5 => {
                mark = observed(
                    SettlementMark {
                        source_open: time(S),
                        price: px(0),
                    },
                    S,
                    R,
                );
                (R, CapturedFundingError::InvalidMark)
            }
            6 => {
                rate = observed(
                    FundingRate {
                        settlement: time(S + 1),
                        rate: Ratio::from_ticks(1_000_000),
                    },
                    S,
                    R,
                );
                (S + 30_000_000, CapturedFundingError::RateUnavailable)
            }
            7 => {
                rate = observed(*rate.value(), S, S);
                mark = observed(
                    SettlementMark {
                        source_open: time(S - 1),
                        price: px(100 * U),
                    },
                    S,
                    R,
                );
                (S + 30_000_000, CapturedFundingError::MarkUnavailable)
            }
            _ => unreachable!(),
        };
        let failure = apply(&mut ledger, capture, &rate, &mark, at).unwrap_err();
        let after = ledger.clone();
        let returned_slot = failure.capture.settlement();
        let returned_position = failure.capture.position();
        let (rate, mark) = terms(S, 1_000_000, 100 * U, R);
        let retry = apply(&mut ledger, failure.capture, &rate, &mark, R).unwrap();
        out.push(Refusal {
            before,
            after,
            error: failure.error,
            expected,
            returned_slot,
            returned_position,
            retry,
        });
    }
    out
}
fn assert_refusals(rows: &[Refusal]) {
    assert_eq!(rows.len(), 8);
    for row in rows {
        assert_eq!(row.error, row.expected);
        assert_eq!(row.after, row.before);
        assert_eq!(row.returned_slot, time(S));
        assert_eq!(row.returned_position, PositionQty(-U));
        assert_eq!(row.retry.position, PositionQty(-U));
        assert_eq!(row.retry.amount, Cash(U));
    }
}

#[derive(Debug)]
struct OwnerTrace {
    before: Ledger,
    after: Ledger,
    error: CapturedFundingError,
    retained_slot: UtcMicros,
    retained_position: PositionQty,
    good_event: FundingEvent,
    other_event: Option<FundingEvent>,
}
fn wrong_issuer() -> OwnerTrace {
    let mut owner = short();
    let capture = owner.capture_funding(time(S)).unwrap();
    let mut other = short(); // Ledger::new creates a distinct financial account.
    let other_capture = other.capture_funding(time(S)).unwrap();
    let before = other.clone();
    let (rate, mark) = terms(S, 1_000_000, 100 * U, R);
    let failure = apply(&mut other, capture, &rate, &mark, R).unwrap_err();
    let retained_slot = failure.capture.settlement();
    let retained_position = failure.capture.position();
    let after = other.clone();
    let good_event = apply(&mut owner, failure.capture, &rate, &mark, R).unwrap();
    let other_event = Some(apply(&mut other, other_capture, &rate, &mark, R).unwrap());
    OwnerTrace {
        before,
        after,
        error: failure.error,
        retained_slot,
        retained_position,
        good_event,
        other_event,
    }
}
fn pre_capture_fork(extra_position: bool) -> OwnerTrace {
    let mut owner = short();
    let mut fork = owner.clone();
    if extra_position {
        fill(&mut fork, Side::Sell, U, 100 * U, S - 1, &costs());
    }
    let good_capture = owner.capture_funding(time(S)).unwrap();
    let foreign_capture = fork.capture_funding(time(S)).unwrap();
    let before = owner.clone();
    let (rate, mark) = terms(S, 1_000_000, 100 * U, R);
    let failure = apply(&mut owner, foreign_capture, &rate, &mark, R).unwrap_err();
    let after = owner.clone();
    let retained_slot = failure.capture.settlement();
    let retained_position = failure.capture.position();
    let good_event = apply(&mut owner, good_capture, &rate, &mark, R).unwrap();
    OwnerTrace {
        before,
        after,
        error: failure.error,
        retained_slot,
        retained_position,
        good_event,
        other_event: None,
    }
}
fn assert_owner(t: &OwnerTrace, expected: CapturedFundingError, position: i64) {
    assert_eq!(t.error, expected);
    assert_eq!(t.before, t.after);
    assert_eq!(t.retained_slot, time(S));
    assert_eq!(t.retained_position, PositionQty(position));
    assert_eq!(t.good_event.position, PositionQty(-U));
    assert_eq!(t.good_event.amount, Cash(U));
    if expected == CapturedFundingError::WrongIssuer {
        let independent = t.other_event.as_ref().unwrap();
        assert_eq!(independent.position, PositionQty(-U));
        assert_eq!(independent.amount, Cash(U));
    }
}

#[derive(Debug)]
struct ClockTrace {
    before: Ledger,
    after: Ledger,
    errors: Vec<FundingCaptureError>,
    event: FundingEvent,
    settled_duplicate: FundingCaptureError,
}
fn clock() -> ClockTrace {
    let mut ledger = short();
    let capture = ledger.capture_funding(time(S)).unwrap();
    let before = ledger.clone();
    let errors = [S, S - 1, NEXT_S]
        .into_iter()
        .map(|at| ledger.capture_funding(time(at)).unwrap_err())
        .collect();
    let after = ledger.clone();
    let (rate, mark) = terms(S, 0, 100 * U, R);
    let event = apply(&mut ledger, capture, &rate, &mark, R).unwrap();
    let settled_duplicate = ledger.capture_funding(time(S)).unwrap_err();
    ClockTrace {
        before,
        after,
        errors,
        event,
        settled_duplicate,
    }
}
fn assert_clock(t: &ClockTrace) {
    assert_eq!(t.after, t.before);
    assert_eq!(
        t.errors,
        vec![
            FundingCaptureError::NonIncreasingSlot {
                last: time(S),
                requested: time(S)
            },
            FundingCaptureError::NonIncreasingSlot {
                last: time(S),
                requested: time(S - 1)
            },
            FundingCaptureError::PendingCapture {
                settlement: time(S)
            },
        ]
    );
    assert_eq!(t.event.amount, Cash(0));
    assert_eq!(t.event.position, PositionQty(-U));
    assert_eq!(
        t.settled_duplicate,
        FundingCaptureError::NonIncreasingSlot {
            last: time(S),
            requested: time(S)
        }
    );
}

#[derive(Debug)]
struct OverflowTrace {
    before: Ledger,
    after: Ledger,
    error: CapturedFundingError,
    retained_slot: UtcMicros,
    retained_position: PositionQty,
    recovered: Option<FundingEvent>,
}
fn cash_overflow() -> OverflowTrace {
    let mut ledger = Ledger::new(Cash(100 * U));
    fill(&mut ledger, Side::Sell, U, 100 * U, S - 1, &costs());
    let capture = ledger.capture_funding(time(S)).unwrap();
    let before = ledger.clone();
    let (rate, mark) = terms(S, U, i64::MAX, R);
    let failure = apply(&mut ledger, capture, &rate, &mark, R).unwrap_err();
    let after = ledger.clone();
    let retained_slot = failure.capture.settlement();
    let retained_position = failure.capture.position();
    // A priced close reduces the position and cash before retrying the SAME receipt terms.
    // This is an arithmetic boundary fixture, not an Astra risk/venue feasibility claim.
    fill(&mut ledger, Side::Buy, U, 200 * U, R + 1, &costs());
    let recovered = Some(apply(&mut ledger, failure.capture, &rate, &mark, R + 2).unwrap());
    OverflowTrace {
        before,
        after,
        error: failure.error,
        retained_slot,
        retained_position,
        recovered,
    }
}
fn funding_net_overflow() -> OverflowTrace {
    let mut ledger = Ledger::new(Cash(0));
    fill(&mut ledger, Side::Sell, U, 1, S - 1, &costs());
    let capture = ledger.capture_funding(time(S)).unwrap();
    let (rate, mark) = terms(S, U, i64::MAX - 2, R);
    apply(&mut ledger, capture, &rate, &mark, R).unwrap();
    fill(&mut ledger, Side::Buy, U, i64::MAX - 1, R + 1, &costs());
    fill(&mut ledger, Side::Sell, U, 1, R + 2, &costs());
    let capture = ledger.capture_funding(time(NEXT_S)).unwrap();
    let before = ledger.clone();
    let (rate, mark) = terms(NEXT_S, U, 3, NEXT_S + 60_000_000);
    let failure = apply(&mut ledger, capture, &rate, &mark, NEXT_S + 60_000_000).unwrap_err();
    let after = ledger.clone();
    let retained_slot = failure.capture.settlement();
    let retained_position = failure.capture.position();
    // A repeated refusal still returns the same outstanding slot/quantity.
    let repeated = apply(
        &mut ledger,
        failure.capture,
        &rate,
        &mark,
        NEXT_S + 60_000_000,
    )
    .unwrap_err();
    assert_eq!(ledger, after);
    assert_eq!(repeated.capture.settlement(), retained_slot);
    assert_eq!(repeated.capture.position(), retained_position);
    OverflowTrace {
        before,
        after,
        error: repeated.error,
        retained_slot,
        retained_position,
        recovered: None,
    }
}
fn assert_overflows(rows: &[OverflowTrace]) {
    assert_eq!(rows.len(), 2);
    for t in rows {
        assert_eq!(t.after, t.before);
        assert_eq!(
            t.error,
            CapturedFundingError::Arithmetic(SimError::Overflow)
        );
        assert_eq!(t.retained_position, PositionQty(-U));
    }
    assert_eq!(rows[0].retained_slot, time(S));
    assert_eq!(
        rows[0].recovered.as_ref().unwrap().position,
        PositionQty(-U)
    );
    assert_eq!(rows[0].recovered.as_ref().unwrap().amount, Cash(i64::MAX));
    assert_eq!(rows[1].retained_slot, time(NEXT_S));
}

#[derive(Debug, Default, cucumber::World)]
struct FundingWorld {
    late: Option<LateTrace>,
    refusals: Vec<Refusal>,
    owners: Vec<OwnerTrace>,
    clock: Option<ClockTrace>,
    overflows: Vec<OverflowTrace>,
    staged: Option<Ledger>,
    staged_event: Option<FundingEvent>,
    legacy: Option<(Ledger, FundingEvent)>,
}
#[when(regex = r"^a captured (long|short) closes before a late (positive|negative) rate arrives$")]
fn late_case(w: &mut FundingWorld, side: String, rate: String) {
    w.late = Some(late(
        if side == "long" {
            Side::Buy
        } else {
            Side::Sell
        },
        if rate == "positive" { 1 } else { -1 },
    ));
}
#[then("the original signed quantity settles once with exact fees and adverse funding rounding")]
fn late_values(w: &mut FundingWorld) {
    assert_late(w.late.as_ref().unwrap());
}
#[when("wrong slot, future view and zero mark receipts try to spend captures")]
fn refusal_cases(w: &mut FundingWorld) {
    w.refusals = source_refusals();
}
#[then("each refusal returns its handle and cause without mutation and the corrected receipt can settle")]
fn refusal_values(w: &mut FundingWorld) {
    assert_refusals(&w.refusals);
}
#[when("a different account and two pre-capture forks try to spend another reservation")]
fn owner_cases(w: &mut FundingWorld) {
    w.owners = vec![
        wrong_issuer(),
        pre_capture_fork(false),
        pre_capture_fork(true),
    ];
}
#[then(
    "issuer and per-capture identity refuse the foreign handles while the correct owner can settle"
)]
fn owner_values(w: &mut FundingWorld) {
    assert_owner(&w.owners[0], CapturedFundingError::WrongIssuer, -U);
    assert_owner(&w.owners[1], CapturedFundingError::WrongCapture, -U);
    assert_owner(&w.owners[2], CapturedFundingError::WrongCapture, -2 * U);
}
#[when("duplicate, older and second pending slots request captures")]
fn clock_case(w: &mut FundingWorld) {
    w.clock = Some(clock());
}
#[then("the finite pending state stays unchanged and even a known zero rate consumes only the original slot")]
fn clock_values(w: &mut FundingWorld) {
    assert_clock(w.clock.as_ref().unwrap());
}
#[when("cash and cumulative funding overflow after valid capture admission")]
fn overflow_cases(w: &mut FundingWorld) {
    w.overflows = vec![cash_overflow(), funding_net_overflow()];
}
#[then("both overflow paths keep every ledger field and the still-unspent funding liability")]
fn overflow_values(w: &mut FundingWorld) {
    assert_overflows(&w.overflows);
}
#[when("a post-capture staging clone closes and settles as its final fallible operation")]
fn staging(w: &mut FundingWorld) {
    let mut owner = short();
    let capture = owner.capture_funding(time(S)).unwrap();
    let mut staged = owner.clone();
    fill(&mut staged, Side::Buy, U, 100 * U, S + 1, &costs());
    let (rate, mark) = terms(S, 1_000_000, 100 * U, R);
    let event = apply(&mut staged, capture, &rate, &mark, R).unwrap();
    owner = staged; // Infallible publication immediately follows successful consume.
    w.staged = Some(owner);
    w.staged_event = Some(event);
}
#[then("the published logical account has the captured payment and remains actually flat")]
fn staging_values(w: &mut FundingWorld) {
    let account = w.staged.as_ref().unwrap();
    let event = w.staged_event.as_ref().unwrap();
    assert_eq!(account.position(), PositionQty(0));
    assert_eq!(account.cash(), Cash(10_001 * U));
    assert_eq!(account.funding_net(), Cash(U));
    assert_eq!(event.position, PositionQty(-U));
    assert_eq!(event.ts, time(S));
}
#[when("the legacy current-position method is called only after a full close")]
fn legacy(w: &mut FundingWorld) {
    let mut ledger = short();
    fill(&mut ledger, Side::Buy, U, 100 * U, S + 1, &costs());
    let event = ledger
        .apply_funding(Ratio::from_ticks(1_000_000), px(100 * U), time(S))
        .unwrap();
    w.legacy = Some((ledger, event));
}
#[then("its existing current-position contract still records zero rather than captured exposure")]
fn legacy_values(w: &mut FundingWorld) {
    let (ledger, event) = w.legacy.as_ref().unwrap();
    assert_eq!(event.position, PositionQty(0));
    assert_eq!(event.amount, Cash(0));
    assert_eq!(ledger.cash(), Cash(10_000 * U));
    assert_eq!(ledger.funding_net(), Cash(0));
}
#[tokio::test]
async fn captured_funding_behavior() {
    FundingWorld::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/captured_funding.feature"
        ))
        .await;
}
