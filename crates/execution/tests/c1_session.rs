#[path = "support/carry_cases.rs"]
mod cases;

use cases::{observed, plan, price, qty, time, Harness, U};
use cucumber::{then, when, World as _};
use parangsae_core::{Observation, Ratio, UtcMicros};
use parangsae_execution::carry::*;
use parangsae_sim::carry::{CarryAccountState, Market};
use parangsae_sim::Cash;
use std::path::Path;

const NET_BTC: i64 = 99_900_000;
const FIRST_FUNDING: i64 = 9_990_000;
const LATE_FUNDING: i64 = -19_980_000;

fn order(pair: u128, stage: CarryStage) -> CarryOrderId {
    CarryOrderId {
        pair,
        stage,
        attempt: 0,
    }
}

fn fill(h: &mut Harness, pair: u128, stage: CarryStage, fill_id: u64, amount: i64) {
    let at = h.now + 1;
    let market = match stage {
        CarryStage::SpotEntry | CarryStage::SpotExit => Market::Spot,
        CarryStage::PerpEntry | CarryStage::PerpExit => Market::UsdMPerp,
    };
    let source = observed(
        CarryFill {
            order: order(pair, stage),
            fill_id,
            market,
            qty: qty(amount),
            reference_price: price(1_000 * U),
        },
        at,
        at,
    );
    h.act(CarryAction::Deliver(
        source.with_value(CarryReport::Fill(source.value().clone())),
    ))
    .unwrap();
    h.receipts.push(source);
}

fn cut(h: &mut Harness, pair: u128, stage: CarryStage) {
    let receipts: Vec<_> = h
        .receipts
        .iter()
        .filter(|receipt| receipt.value().order == order(pair, stage))
        .cloned()
        .collect();
    let cumulative = receipts.iter().map(|r| r.value().qty.ticks()).sum();
    let at = h.now + 1;
    h.act(CarryAction::Reconcile(observed(
        CarryTerminalCut {
            order: order(pair, stage),
            evidence_id: 1,
            status: CarryTerminal::Filled,
            cumulative: qty(cumulative),
            receipts,
            complete: true,
        },
        at,
        at,
    )))
    .unwrap();
}

fn finish_entry(h: &mut Harness, pair: u128) {
    h.act(CarryAction::Dispatch(order(pair, CarryStage::SpotEntry)))
        .unwrap();
    fill(h, pair, CarryStage::SpotEntry, 1, U);
    cut(h, pair, CarryStage::SpotEntry);
    h.act(CarryAction::PlanHedge { pair }).unwrap();
    h.act(CarryAction::Dispatch(order(pair, CarryStage::PerpEntry)))
        .unwrap();
    fill(h, pair, CarryStage::PerpEntry, 2, NET_BTC);
    cut(h, pair, CarryStage::PerpEntry);
}

fn close(h: &mut Harness, pair: u128) {
    h.act(CarryAction::BeginExit {
        pair,
        max_perp_price: price(1_200 * U),
    })
    .unwrap();
    h.act(CarryAction::Dispatch(order(pair, CarryStage::PerpExit)))
        .unwrap();
    fill(h, pair, CarryStage::PerpExit, 3, NET_BTC);
    cut(h, pair, CarryStage::PerpExit);
    h.act(CarryAction::PlanSpotExit {
        pair,
        max_spot_price: price(1_200 * U),
    })
    .unwrap();
    h.act(CarryAction::Dispatch(order(pair, CarryStage::SpotExit)))
        .unwrap();
    fill(h, pair, CarryStage::SpotExit, 4, NET_BTC);
    cut(h, pair, CarryStage::SpotExit);
}

fn boundary(h: &mut Harness, key: u64) -> UtcMicros {
    let effective = time(h.now + 1);
    assert_eq!(
        h.act(CarryAction::FundingBoundary { key, effective }),
        Ok(CarryOutcome::BoundaryCaptured {
            key,
            short: qty(NET_BTC),
        })
    );
    effective
}

fn funding(
    h: &Harness,
    key: u64,
    effective: UtcMicros,
    rate_ticks: i64,
) -> Observation<CarryFundingReceipt> {
    let event = u64::try_from(effective.micros() - cases::AT).unwrap();
    observed(
        CarryFundingReceipt {
            key,
            effective,
            rate: Ratio::from_ticks(rate_ticks),
            charge_mark: price(1_000 * U),
        },
        event,
        h.now + 1,
    )
}

#[derive(Debug)]
struct DuplicateObservation {
    before: CarrySnapshot,
    outcome: CarryOutcome,
    after: CarrySnapshot,
}

#[derive(Debug)]
struct SessionTrace {
    initial: CarrySession,
    first_closed: CarrySnapshot,
    second_opening: CarrySnapshot,
    second_hedged: CarrySnapshot,
    pending_before: CarrySnapshot,
    pending_plan: Result<CarryOutcome, CarryError>,
    pending_after: CarrySnapshot,
    final_state: CarrySnapshot,
    late_source: Observation<CarryFundingReceipt>,
    late_outcome: CarryOutcome,
    duplicates: Vec<DuplicateObservation>,
}

fn two_cycles() -> SessionTrace {
    let mut initial = cases::session();
    initial.limits.max_session_loss = Cash(6 * U);
    initial.limits.max_orders = 8;
    initial.limits.max_receipts = 8;
    initial.limits.max_funding_events = 2;
    let mut h = Harness::with_session(initial.clone());
    h.hedged();
    let first_s = boundary(&mut h, 1);
    let first_source = funding(&h, 1, first_s, 10_000);
    assert_eq!(
        h.act(CarryAction::FundingReceipt(first_source.clone())),
        Ok(CarryOutcome::FundingApplied {
            key: 1,
            amount: Cash(FIRST_FUNDING),
        })
    );
    h.close();
    let first_closed = h.engine.snapshot();

    let mut second_plan = plan();
    second_plan.pair = 2;
    h.act(CarryAction::Plan(second_plan)).unwrap();
    let second_opening = h.engine.snapshot();
    finish_entry(&mut h, 2);
    let second_hedged = h.engine.snapshot();
    let second_s = boundary(&mut h, 2);
    close(&mut h, 2);
    let pending_before = h.engine.snapshot();
    let mut third_plan = plan();
    third_plan.pair = 3;
    let pending_plan = h.act(CarryAction::Plan(third_plan));
    let pending_after = h.engine.snapshot();

    let late_source = funding(&h, 2, second_s, -20_000);
    let late_outcome = h
        .act(CarryAction::FundingReceipt(late_source.clone()))
        .unwrap();
    let final_state = h.engine.snapshot();
    let first_fill = h.receipts[0].clone();
    let actions = [
        CarryAction::Deliver(first_fill.with_value(CarryReport::Fill(first_fill.value().clone()))),
        CarryAction::FundingBoundary {
            key: 1,
            effective: first_s,
        },
        CarryAction::FundingReceipt(first_source),
        CarryAction::FundingReceipt(late_source.clone()),
    ];
    let duplicates = actions
        .into_iter()
        .map(|action| {
            let before = h.engine.snapshot();
            let outcome = h.act(action).unwrap();
            DuplicateObservation {
                before,
                outcome,
                after: h.engine.snapshot(),
            }
        })
        .collect();
    SessionTrace {
        initial,
        first_closed,
        second_opening,
        second_hedged,
        pending_before,
        pending_plan,
        pending_after,
        final_state,
        late_source,
        late_outcome,
        duplicates,
    }
}

fn expected_closed(cycles: i64, funding_net: i64) -> CarryAccountState {
    // The specified equal-price cycle pays 1.999 USDT in spot cash and
    // 0.999 USDT in perpetual commissions. It realizes no price PnL.
    CarryAccountState {
        spot_quote: Cash(4_000 * U - cycles * 199_900_000),
        spot_btc: qty(0),
        perp_wallet: Cash(5_000 * U - cycles * 99_900_000 + funding_net),
        perp_short: qty(0),
        idle: Cash(1_000 * U),
        spot_quote_fees: Cash(cycles * 99_900_000),
        spot_btc_fees: qty(cycles * 100_000),
        perp_fees: Cash(cycles * 99_900_000),
        funding_net: Cash(funding_net),
        realized_perp: Cash(0),
        perp_entry_cost_scaled: 0,
        realized_perp_scaled: 0,
    }
}

fn assert_carryover(actual: &CarryAccountState, expected: &CarryAccountState) {
    assert_eq!(actual, expected, "CapitalCarryoverMismatch");
}

#[derive(Debug, Default, cucumber::World)]
struct SessionWorld {
    trace: Option<SessionTrace>,
}

#[when(
    "one engine completes two pairs and receives the second funding payment after physical flat"
)]
fn run(world: &mut SessionWorld) {
    world.trace = Some(two_cycles());
}

#[then("the second pair retains the first cash, fees and original loss anchor")]
fn retained_accounts(world: &mut SessionWorld) {
    let t = world.trace.as_ref().unwrap();
    assert_eq!(t.first_closed.phase, CarryPhase::Closed);
    assert_eq!(t.first_closed.account, expected_closed(1, FIRST_FUNDING));
    assert_carryover(&t.second_opening.account, &t.first_closed.account);
    assert_eq!(t.second_opening.phase, CarryPhase::OpeningSpot);
    assert_eq!(t.second_opening.hedged_at, None);
    assert_eq!(t.second_hedged.phase, CarryPhase::Hedged);
    assert!(t.second_hedged.hedged_at > t.first_closed.hedged_at);
    assert!(!t.first_closed.loss_latched);
    assert!(!t.pending_before.loss_latched);
    assert_eq!(t.initial.limits.max_session_loss, Cash(6 * U));
    let final_account = expected_closed(2, FIRST_FUNDING + LATE_FUNDING);
    assert_eq!(t.final_state.account, final_account);
    assert_eq!(
        t.final_state
            .account
            .equity(price(1_000 * U), price(1_000 * U))
            .unwrap(),
        Cash(999_390_410_000)
    );
    assert!(t.final_state.loss_latched);
    assert!(t.final_state.halted);
    assert_eq!(t.final_state.phase, CarryPhase::Closed);
}

#[then("zero positions cannot rearm while the captured settlement is undrained")]
fn pending_is_not_closed(world: &mut SessionWorld) {
    let t = world.trace.as_ref().unwrap();
    assert_eq!(t.pending_before.account.spot_btc, qty(0));
    assert_eq!(t.pending_before.account.perp_short, qty(0));
    assert!(t.pending_before.orders.iter().all(|o| o.complete_cut));
    assert!(!t.pending_before.halted);
    assert_eq!(t.pending_before.phase, CarryPhase::ClosingSpot);
    assert_eq!(t.pending_plan, Err(CarryError::Phase));
    assert_eq!(t.pending_before, t.pending_after);
    assert_eq!(t.pending_before.account, expected_closed(2, FIRST_FUNDING));
    assert_eq!(t.pending_before.funding.len(), 2);
    assert!(t.pending_before.funding[1].receipt.is_none());
    assert!(t.pending_before.funding[1].cashflow.is_none());
}

#[then("the late negative payment retains its original settlement and captured quantity")]
fn original_settlement(world: &mut SessionWorld) {
    let t = world.trace.as_ref().unwrap();
    let recorded = &t.final_state.funding[1];
    assert_eq!(recorded.key, 2);
    assert_eq!(recorded.effective, t.late_source.value().effective);
    assert_eq!(recorded.captured_short, qty(NET_BTC));
    assert_eq!(recorded.receipt, Some(t.late_source.clone()));
    assert_eq!(recorded.cashflow, Some(Cash(LATE_FUNDING)));
    assert_eq!(
        t.late_source.timestamps().event_ts.micros(),
        recorded.effective
    );
    assert!(t.late_source.timestamps().available_ts.micros() > recorded.effective);
    assert!(t.late_source.timestamps().received_ts.micros() > recorded.effective);
    assert_eq!(
        t.late_outcome,
        CarryOutcome::FundingApplied {
            key: 2,
            amount: Cash(LATE_FUNDING),
        }
    );
    assert_eq!(
        t.final_state.account.perp_wallet.0 - t.pending_before.account.perp_wallet.0,
        LATE_FUNDING
    );
}

#[then("duplicate old identities preserve both pairs at full retention capacity")]
fn tombstones(world: &mut SessionWorld) {
    let t = world.trace.as_ref().unwrap();
    assert_eq!(t.final_state.orders.len(), 8);
    assert_eq!(t.final_state.fills.len(), 8);
    assert_eq!(t.final_state.funding.len(), 2);
    for pair in [1, 2] {
        assert_eq!(
            t.final_state
                .orders
                .iter()
                .filter(|o| o.id.pair == pair)
                .count(),
            4
        );
        assert_eq!(
            t.final_state
                .fills
                .iter()
                .filter(|f| f.source.value().order.pair == pair)
                .count(),
            4
        );
    }
    assert_eq!(t.duplicates.len(), 4);
    for duplicate in &t.duplicates {
        assert_eq!(duplicate.outcome, CarryOutcome::Duplicate);
        assert_eq!(duplicate.before, duplicate.after);
        assert_eq!(duplicate.after, t.final_state);
    }
}

#[test]
fn c1_session_behavior() {
    futures::executor::block_on(
        SessionWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/carry/c1_session.feature"
            ))),
    );
}

#[test]
#[should_panic(expected = "CapitalCarryoverMismatch")]
fn a_fresh_engine_reset_is_a_detected_negative_control() {
    let trace = two_cycles();
    let fresh = Harness::new();
    assert_eq!(fresh.initial.allocation, trace.initial.allocation);
    assert_carryover(
        &fresh.engine.snapshot().account,
        &expected_closed(1, FIRST_FUNDING),
    );
}

#[test]
#[should_panic(expected = "FundingQuantityMismatch")]
fn delivery_current_quantity_is_a_detected_negative_control() {
    let trace = two_cycles();
    let current_short = i128::from(trace.pending_before.account.perp_short.ticks());
    let receipt = trace.late_source.value();
    let wrong_amount =
        current_short * i128::from(receipt.charge_mark.ticks()) * i128::from(receipt.rate.ticks())
            / (i128::from(U) * i128::from(U));
    let actual =
        trace.final_state.account.perp_wallet.0 - trace.pending_before.account.perp_wallet.0;
    assert_eq!(i128::from(actual), wrong_amount, "FundingQuantityMismatch");
}
