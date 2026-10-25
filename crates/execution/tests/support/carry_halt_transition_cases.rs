// Private authored observer content. Not compiled or executed by the author.
// Parent registration: #[path = "support/carry_halt_transition_cases.rs"]
// mod halt_transition_cases; in tests/carry_accounting.rs (which owns `cases`).
use super::cases::{id, observed, plan, price, qty, time, Harness, U};
use cucumber::{then, when, World as _};
use parangsae_core::DecisionTime;
use parangsae_execution::carry::*;
use parangsae_execution::{HaltCause, Mono};
use parangsae_sim::carry::{FeeCharge, Market};
use parangsae_sim::Cash;
use std::path::Path;

const NET: i64 = 99_900_000;
const CAP: i64 = 1_000 * U;

#[derive(Debug)]
struct TransitionTrace {
    close_price: i64,
    cap: i64,
    fill_now: u64,
    before_exit: CarrySnapshot,
    after_perp_fill: CarrySnapshot,
    after_perp_cut: CarrySnapshot,
    after_later_mark: CarrySnapshot,
    closed: CarrySnapshot,
    recovered: CarrySnapshot,
    later_halt: CarrySnapshot,
}

fn fill_at(h: &mut Harness, stage: CarryStage, fill_id: u64, amount: i64, px: i64) {
    let at = h.now + 1;
    let source = observed(
        CarryFill {
            order: id(stage),
            fill_id,
            market: match stage {
                CarryStage::SpotEntry | CarryStage::SpotExit => Market::Spot,
                CarryStage::PerpEntry | CarryStage::PerpExit => Market::UsdMPerp,
            },
            qty: qty(amount),
            reference_price: price(px),
        },
        at,
        at,
    );
    let result = h
        .marked(
            CarryAction::Deliver(source.with_value(CarryReport::Fill(source.value().clone()))),
            px,
            px,
        )
        .unwrap();
    assert!(matches!(result, CarryOutcome::FillApplied(_)));
    h.receipts.push(source);
}

fn cut_at(h: &mut Harness, stage: CarryStage, px: i64) {
    let at = h.now + 1;
    let receipts: Vec<_> = h
        .receipts
        .iter()
        .filter(|source| source.value().order == id(stage))
        .cloned()
        .collect();
    let cumulative = receipts
        .iter()
        .map(|source| source.value().qty.ticks())
        .sum();
    let outcome = h
        .marked(
            CarryAction::Reconcile(observed(
                CarryTerminalCut {
                    order: id(stage),
                    evidence_id: 1,
                    status: CarryTerminal::Filled,
                    cumulative: qty(cumulative),
                    receipts,
                    complete: true,
                },
                at,
                at,
            )),
            px,
            px,
        )
        .unwrap();
    assert_eq!(outcome, CarryOutcome::TerminalReconciled(id(stage)));
}

fn transition(close_price: i64, cap: i64, later_mark: i64) -> TransitionTrace {
    let mut initial = super::cases::session();
    initial.limits.max_net_btc = qty(NET);
    initial.limits.max_unhedged_notional = Cash(cap);
    let mut h = Harness::with_session(initial);
    // The standard helper's1200 upper would fail the1000 notional admission.
    // Freeze explicit1000 entry uppers before exposure exists.
    let mut entry = plan();
    entry.max_spot_price = price(1_000 * U);
    entry.max_perp_price = price(1_000 * U);
    h.act(CarryAction::Plan(entry)).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .unwrap();
    h.fill(CarryStage::SpotEntry, 1, U);
    h.cut(CarryStage::SpotEntry, CarryTerminal::Filled);
    h.act(CarryAction::PlanHedge { pair: 1 }).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
        .unwrap();
    h.fill(CarryStage::PerpEntry, 2, NET);
    h.cut(CarryStage::PerpEntry, CarryTerminal::Filled);
    let before_exit = h.engine.snapshot();
    assert_eq!(before_exit.phase, CarryPhase::Hedged);
    assert!(!before_exit.halted);
    h.marked(
        CarryAction::BeginExit {
            pair: 1,
            max_perp_price: price(close_price),
        },
        close_price,
        close_price,
    )
    .unwrap();
    h.marked(
        CarryAction::Dispatch(id(CarryStage::PerpExit)),
        close_price,
        close_price,
    )
    .unwrap();
    fill_at(&mut h, CarryStage::PerpExit, 3, NET, close_price);
    let fill_now = h.now;
    let after_perp_fill = h.engine.snapshot();
    cut_at(&mut h, CarryStage::PerpExit, close_price);
    let after_perp_cut = h.engine.snapshot();
    h.marked(CarryAction::Observe, later_mark, later_mark)
        .unwrap();
    let after_later_mark = h.engine.snapshot();
    h.marked(
        CarryAction::PlanSpotExit {
            pair: 1,
            max_spot_price: price(close_price),
        },
        close_price,
        close_price,
    )
    .unwrap();
    h.marked(
        CarryAction::Dispatch(id(CarryStage::SpotExit)),
        close_price,
        close_price,
    )
    .unwrap();
    fill_at(&mut h, CarryStage::SpotExit, 4, NET, close_price);
    cut_at(&mut h, CarryStage::SpotExit, close_price);
    let closed = h.engine.snapshot();
    h.act(CarryAction::Observe).unwrap();
    let recovered = h.engine.snapshot();
    h.act(CarryAction::Halt(HaltCause::ReconMismatch)).unwrap();
    TransitionTrace {
        close_price,
        cap,
        fill_now,
        before_exit,
        after_perp_fill,
        after_perp_cut,
        after_later_mark,
        closed,
        recovered,
        later_halt: h.engine.snapshot(),
    }
}

fn assert_economics(t: &TransitionTrace) {
    let (spot, perp, equity, perp_fee, spot_fee, realized) = match t.close_price / U {
        1_001 => (
            399_899_900_100,
            499_800_150_050,
            999_700_050_150,
            99_949_950,
            99_999_900,
            -99_900_000,
        ),
        1_002 => (
            399_999_700_200,
            499_700_200_100,
            999_699_900_300,
            99_999_900,
            100_099_800,
            -199_800_000,
        ),
        _ => panic!("unsupported fixed synthetic oracle"),
    };
    assert_eq!(t.after_perp_fill.phase, CarryPhase::ClosingPerp);
    assert_eq!(t.after_perp_cut.phase, CarryPhase::ClosingSpot);
    assert_eq!(t.after_perp_fill.account.spot_btc, qty(NET));
    assert_eq!(t.after_perp_fill.account.perp_short, qty(0));
    assert_eq!(t.after_perp_fill.fills.len(), 3);
    assert_eq!(t.closed.phase, CarryPhase::Closed);
    assert_eq!(t.closed.account.spot_btc, qty(0));
    assert_eq!(t.closed.account.perp_short, qty(0));
    assert_eq!(t.closed.account.spot_quote, Cash(spot));
    assert_eq!(t.closed.account.perp_wallet, Cash(perp));
    assert_eq!(t.closed.account.idle, Cash(1_000 * U));
    assert_eq!(t.closed.account.spot_btc_fees, qty(100_000));
    assert_eq!(t.closed.account.spot_quote_fees, Cash(spot_fee));
    assert_eq!(t.closed.account.perp_fees, Cash(perp_fee));
    assert_eq!(t.closed.account.realized_perp, Cash(realized));
    assert_eq!(t.closed.account.funding_net, Cash(0));
    assert!(t.closed.funding.is_empty());
    assert_eq!(t.closed.fills.len(), 4);
    assert!(t.closed.orders.iter().all(|order| order.complete_cut));
    assert_eq!(t.closed.spot_quote_reserved, Cash(0));
    assert_eq!(t.closed.perp_collateral_reserved, Cash(0));
    assert_eq!(
        t.closed
            .account
            .equity(price(t.close_price), price(t.close_price))
            .unwrap(),
        Cash(equity)
    );
    let exit = t
        .closed
        .fills
        .iter()
        .find(|fill| fill.source.value().order == id(CarryStage::PerpExit))
        .unwrap();
    assert_eq!(exit.price, price(t.close_price));
    assert_eq!(exit.fee, FeeCharge::Usdt(Cash(perp_fee - 49_950_000)));
}

fn assert_healthy(t: &TransitionTrace) {
    assert_economics(t);
    for state in [
        &t.before_exit,
        &t.after_perp_fill,
        &t.after_perp_cut,
        &t.after_later_mark,
        &t.closed,
        &t.recovered,
    ] {
        assert!(!state.halted);
        assert!(!state.loss_latched);
        assert!(!state.margin_latched);
        assert!(state.first_halt.is_none());
    }
}

fn assert_notional_only_first_evidence(t: &TransitionTrace) {
    assert_economics(t);
    assert!(t.after_perp_fill.halted);
    assert!(!t.after_perp_fill.loss_latched);
    assert!(!t.after_perp_fill.margin_latched);
    let first = t
        .after_perp_fill
        .first_halt
        .as_ref()
        .expect("actual Fill must retain first halt evidence");
    assert_eq!(first.cause, HaltCause::RiskTrip);
    assert_eq!(first.epoch.get(), 1);
    assert_eq!(first.now, Mono::from_micros(t.fill_now));
    assert_eq!(first.decision, DecisionTime::from_micros(time(t.fill_now)));
    assert_eq!(first.phase_before_refresh, CarryPhase::ClosingPerp);
    let assessment = first
        .assessment
        .as_ref()
        .expect("risk halt must retain assessment");
    assert_eq!(
        assessment.point,
        CarryAssessmentPoint::Fill(id(CarryStage::PerpExit))
    );
    assert_eq!(assessment.account, t.after_perp_fill.account);
    assert_eq!(assessment.account.perp_short, qty(0));
    assert_eq!(assessment.account.spot_btc, qty(NET));
    assert_eq!(assessment.spot, price(1_002 * U));
    assert_eq!(assessment.perp, price(1_002 * U));
    assert_eq!(assessment.equity, Cash(999_800_000_100));
    assert_eq!(assessment.session_loss_ticks, 199_999_900_i128);
    assert_eq!(assessment.max_session_loss, Cash(500 * U));
    assert_eq!(assessment.margin_balance_ticks, None);
    assert_eq!(assessment.margin_required, None);
    assert_eq!(assessment.unhedged_qty, qty(NET));
    assert_eq!(assessment.max_net_btc, qty(NET));
    assert_eq!(assessment.unhedged_notional_ticks, 100_099_800_000_i128);
    assert_eq!(assessment.max_unhedged_notional, Cash(t.cap));
    assert!(assessment.notional_guard_evaluated);
    assert!(!assessment.flags.loss);
    assert!(!assessment.flags.margin);
    assert!(!assessment.flags.net_quantity);
    assert!(assessment.flags.unhedged_notional);
}

fn assert_first_evidence_survives(t: &TransitionTrace) {
    assert_notional_only_first_evidence(t);
    for state in [
        &t.after_perp_cut,
        &t.after_later_mark,
        &t.closed,
        &t.recovered,
        &t.later_halt,
    ] {
        assert_eq!(state.first_halt, t.after_perp_fill.first_halt);
        assert!(state.halted);
    }
    assert_eq!(t.later_halt.account, t.closed.account);
    assert_eq!(t.later_halt.fills, t.closed.fills);
    assert_eq!(t.later_halt.orders, t.closed.orders);
    assert_eq!(t.later_halt.funding, t.closed.funding);
}

#[test]
fn within_bound_perp_exit_then_spot_exit_does_not_manufacture_a_halt() {
    assert_healthy(&transition(1_001 * U, CAP, 1_001 * U));
}
#[test]
fn exactly_equal_notional_and_quantity_bounds_remain_healthy() {
    assert_healthy(&transition(1_001 * U, 99_999_900_000, 1_001 * U));
}
#[test]
fn actual_perp_close_records_notional_only_risk_and_legal_spot_liquidation() {
    assert_notional_only_first_evidence(&transition(1_002 * U, CAP, 1_003 * U));
}
#[test]
fn first_halt_evidence_survives_later_marks_flat_recovery_and_a_second_cause() {
    assert_first_evidence_survives(&transition(1_002 * U, CAP, 1_003 * U));
}

#[derive(Debug, Default, cucumber::World)]
struct HaltTransitionWorld {
    trace: Option<TransitionTrace>,
}

#[when(
    regex = r"^a paired carry closes at (1001|1002) with a declared (inside|equal|over) notional boundary$"
)]
fn run_trace(world: &mut HaltTransitionWorld, px: i64, boundary: String) {
    let cap = if boundary == "equal" {
        99_999_900_000
    } else {
        CAP
    };
    let later = if boundary == "over" {
        1_003 * U
    } else {
        px * U
    };
    world.trace = Some(transition(px * U, cap, later));
}
#[then("all four actual fills and complete cuts retain exact fee inclusive flat wallets")]
fn values(world: &mut HaltTransitionWorld) {
    assert_economics(world.trace.as_ref().unwrap());
}
#[then("no halt or first evidence precedes the later explicit manual halt")]
fn healthy(world: &mut HaltTransitionWorld) {
    assert_healthy(world.trace.as_ref().unwrap());
}
#[then("the first Fill assessment distinguishes notional excess from quantity loss and margin")]
fn notional(world: &mut HaltTransitionWorld) {
    assert_notional_only_first_evidence(world.trace.as_ref().unwrap());
}
#[then("the original evidence survives legal liquidation later marks and a second halt cause")]
fn retained(world: &mut HaltTransitionWorld) {
    assert_first_evidence_survives(world.trace.as_ref().unwrap());
}
#[test]
fn carry_halt_transition_behavior() {
    futures::executor::block_on(
        HaltTransitionWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/carry/halt_transition.feature"
            ))),
    );
}
