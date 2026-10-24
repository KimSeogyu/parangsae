#[path = "support/carry_cases.rs"]
mod cases;

use cases::{id, plan, price, Harness, U};
use cucumber::{then, when, World as _};
use parangsae_execution::carry::*;
use parangsae_sim::Cash;
use std::path::Path;

fn assert_financial_evidence_unchanged(before: &CarrySnapshot, after: &CarrySnapshot) {
    assert_eq!(after.account, before.account);
    assert_eq!(after.pair, before.pair);
    assert_eq!(after.orders, before.orders);
    assert_eq!(after.fills, before.fills);
    assert_eq!(after.funding, before.funding);
    assert_eq!(after.spot_quote_reserved, before.spot_quote_reserved);
    assert_eq!(
        after.perp_collateral_reserved,
        before.perp_collateral_reserved
    );
    assert_eq!(after.hedged_at, before.hedged_at);
    assert_eq!(after.reconciled_epoch, before.reconciled_epoch);
}

#[derive(Debug)]
struct LossTrace {
    before: CarrySnapshot,
    adverse_outcome: CarryOutcome,
    adverse: CarrySnapshot,
    recovered_outcome: CarryOutcome,
    recovered: CarrySnapshot,
}

fn observe_loss() -> LossTrace {
    let mut initial = cases::session();
    initial.limits.max_session_loss = Cash(5 * U);
    let mut h = Harness::with_session(initial);
    h.hedged();
    let before = h.engine.snapshot();
    let adverse_outcome = h.marked(CarryAction::Observe, 990 * U, 1_000 * U).unwrap();
    let adverse = h.engine.snapshot();
    let recovered_outcome = h.act(CarryAction::Observe).unwrap();
    LossTrace {
        before,
        adverse_outcome,
        adverse,
        recovered_outcome,
        recovered: h.engine.snapshot(),
    }
}

#[derive(Debug)]
struct DeadlineTrace {
    before: CarrySnapshot,
    equality_outcome: CarryOutcome,
    equality: CarrySnapshot,
    expired_outcome: Result<CarryOutcome, CarryError>,
    expired: CarrySnapshot,
}

fn observe_deadline() -> DeadlineTrace {
    let mut h = Harness::new();
    h.act(CarryAction::Plan(plan())).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .unwrap();
    let before = h.engine.snapshot();
    let deadline = h.now + h.initial.limits.max_legging_us;
    h.now = deadline - 1;
    let equality_outcome = h.act(CarryAction::Observe).unwrap();
    let equality = h.engine.snapshot();
    let expired_outcome = h.act(CarryAction::Observe);
    DeadlineTrace {
        before,
        equality_outcome,
        equality,
        expired_outcome,
        expired: h.engine.snapshot(),
    }
}

#[derive(Debug, Default, cucumber::World)]
struct ObserveWorld {
    loss: Option<LossTrace>,
    deadline: Option<DeadlineTrace>,
}

#[when("one matched pair observes an adverse spot mark and then the recovered marks")]
fn loss_trace(world: &mut ObserveWorld) {
    world.loss = Some(observe_loss());
}

#[then("each admitted observation returns Observed without changing financial evidence")]
fn unchanged_economics(world: &mut ObserveWorld) {
    let t = world.loss.as_ref().unwrap();
    assert_eq!(t.adverse_outcome, CarryOutcome::Observed);
    assert_eq!(t.recovered_outcome, CarryOutcome::Observed);
    assert_financial_evidence_unchanged(&t.before, &t.adverse);
    assert_financial_evidence_unchanged(&t.before, &t.recovered);
    assert_eq!(t.before.reconciled_epoch, None);
    assert_eq!(t.before.account.spot_quote, Cash(3_000 * U));
    assert_eq!(t.before.account.perp_wallet, Cash(499_950_050_000));
    assert_eq!(t.before.account.spot_btc.ticks(), 99_900_000);
    assert_eq!(
        t.before
            .account
            .equity(price(1_000 * U), price(1_000 * U))
            .unwrap(),
        Cash(999_850_050_000)
    );
    assert_eq!(
        t.adverse
            .account
            .equity(price(990 * U), price(1_000 * U))
            .unwrap(),
        Cash(998_851_050_000)
    );
}

#[then("the observed loss remains latched after recovery with exactly one new epoch")]
fn sticky_loss(world: &mut ObserveWorld) {
    let t = world.loss.as_ref().unwrap();
    assert!(!t.before.halted);
    assert!(!t.before.loss_latched);
    assert!(t.adverse.halted);
    assert!(t.adverse.loss_latched);
    assert_eq!(t.adverse.epoch.get(), t.before.epoch.get() + 1);
    assert!(t.recovered.halted);
    assert!(t.recovered.loss_latched);
    assert_eq!(t.recovered.epoch, t.adverse.epoch);
    assert_eq!(t.adverse.phase, CarryPhase::Hedged);
    assert_eq!(t.recovered.phase, CarryPhase::Hedged);
}

#[when("a dispatched unfilled spot entry is observed at its deadline and one microsecond later")]
fn deadline_trace(world: &mut ObserveWorld) {
    world.deadline = Some(observe_deadline());
}

#[then("deadline equality is Observed and preserves the complete public snapshot")]
fn deadline_equality(world: &mut ObserveWorld) {
    let t = world.deadline.as_ref().unwrap();
    assert_eq!(t.equality_outcome, CarryOutcome::Observed);
    assert_eq!(t.equality, t.before);
}

#[then("the later observation retains LeggingDeadline and refuses with UnsupportedFact")]
fn deadline_refusal(world: &mut ObserveWorld) {
    let t = world.deadline.as_ref().unwrap();
    assert_eq!(t.expired_outcome, Err(CarryError::UnsupportedFact));
    assert_eq!(
        t.expired.unsupported,
        Some(UnsupportedEvidence::LeggingDeadline)
    );
    assert_eq!(t.expired.phase, CarryPhase::Unresolved);
    assert!(t.expired.halted);
    assert_eq!(t.expired.epoch.get(), t.equality.epoch.get() + 1);
    assert_financial_evidence_unchanged(&t.equality, &t.expired);
}

#[test]
fn healthy_observe_is_not_a_reconciliation_or_a_financial_action() {
    let mut h = Harness::new();
    h.hedged();
    let before = h.engine.snapshot();
    assert_eq!(h.act(CarryAction::Observe), Ok(CarryOutcome::Observed));
    assert_eq!(h.engine.snapshot(), before);
    h.close();
    assert_eq!(h.engine.snapshot().phase, CarryPhase::Closed);
}

#[test]
fn carry_observe_behavior() {
    futures::executor::block_on(
        ObserveWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/carry/observe.feature"
            ))),
    );
}
