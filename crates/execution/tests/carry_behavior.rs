#[path = "support/carry_cases.rs"]
mod cases;

use std::path::Path;

use cases::*;
use cucumber::{given, then, when, World as _};
use parangsae_core::Ratio;
use parangsae_execution::carry::*;
use parangsae_execution::OrderState;
use parangsae_sim::Cash;

#[derive(Debug, Default, cucumber::World)]
struct CarryWorld {
    h: Option<Harness>,
    before: Option<CarrySnapshot>,
    result: Option<Result<CarryOutcome, CarryError>>,
    restarted: Option<CarrySnapshot>,
}

#[given("one explicit capital allocation to spot, isolated futures and idle cash")]
fn flat(world: &mut CarryWorld) {
    world.h = Some(Harness::new());
}

#[given("a visible completed spot purchase with 0.999 BTC after fees")]
fn spot(world: &mut CarryWorld) {
    let mut h = Harness::new();
    h.spot();
    world.h = Some(h);
}

#[given("a separately completed short hedge of the owned 0.999 BTC")]
fn hedged(world: &mut CarryWorld) {
    let mut h = Harness::new();
    h.hedged();
    world.h = Some(h);
}

#[given("a hedged session with capacity for one funding identity")]
fn one_slot(world: &mut CarryWorld) {
    let mut initial = session();
    initial.limits.max_funding_events = 1;
    let mut h = Harness::with_session(initial);
    h.hedged();
    world.h = Some(h);
}

#[when("all four entry and exit executions complete at 1000 USDT")]
fn four_fills(world: &mut CarryWorld) {
    let h = world.h.as_mut().unwrap();
    h.hedged();
    h.close();
}

#[then("the closed wallets total 9997.002 USDT after every execution fee")]
fn four_cashflows(world: &mut CarryWorld) {
    let state = world.h.as_ref().unwrap().engine.snapshot();
    assert_eq!(state.phase, CarryPhase::Closed);
    assert_eq!(state.account.spot_quote, Cash(399_800_100_000));
    assert_eq!(state.account.perp_wallet, Cash(499_900_100_000));
    assert_eq!(state.account.idle, Cash(1_000 * U));
    assert_eq!(
        state
            .account
            .equity(price(1_000 * U), price(1_000 * U))
            .unwrap(),
        Cash(999_700_200_000)
    );
    assert_eq!(state.account.spot_btc, qty(0));
    assert_eq!(state.account.perp_short, qty(0));
}

#[then("the unhedged spot leg has spent 1000 USDT and has created no futures cash")]
fn no_principal(world: &mut CarryWorld) {
    let state = world.h.as_ref().unwrap().engine.snapshot();
    assert_eq!(state.account.spot_quote, Cash(3_000 * U));
    assert_eq!(state.account.perp_wallet, Cash(5_000 * U));
    assert_eq!(state.account.spot_btc, qty(99_900_000));
    assert_eq!(state.account.perp_short, qty(0));
    assert_eq!(state.phase, CarryPhase::OpeningHedge);
}

#[when("a one-basis-point funding receipt arrives only after both legs have closed")]
fn late_funding(world: &mut CarryWorld) {
    let h = world.h.as_mut().unwrap();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    h.close();
    let at = u64::try_from(effective.micros() - AT).unwrap();
    let receipt = observed(
        CarryFundingReceipt {
            key: 1,
            effective,
            rate: Ratio::from_ticks(10_000),
            charge_mark: price(1_000 * U),
        },
        at,
        h.now + 1,
    );
    world.result = Some(h.act(CarryAction::FundingReceipt(receipt)));
}

#[then("funding is 0.0999 USDT from the retained quantity rather than the zero current short")]
fn retained_funding(world: &mut CarryWorld) {
    assert_eq!(
        world.result,
        Some(Ok(CarryOutcome::FundingApplied {
            key: 1,
            amount: Cash(9_990_000)
        }))
    );
    let state = world.h.as_ref().unwrap().engine.snapshot();
    assert_eq!(state.funding[0].captured_short, qty(99_900_000));
    assert_eq!(state.account.perp_short, qty(0));
    assert_eq!(state.account.funding_net, Cash(9_990_000));
    assert_eq!(
        state
            .account
            .equity(price(1_000 * U), price(1_000 * U))
            .unwrap(),
        Cash(999_710_190_000)
    );
    assert_eq!(state.phase, CarryPhase::Closed);
}

#[when("the short close times out before any priced close receipt")]
fn unknown_close(world: &mut CarryWorld) {
    let h = world.h.as_mut().unwrap();
    h.act(CarryAction::BeginExit {
        pair: 1,
        max_perp_price: price(1_200 * U),
    })
    .unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpExit)))
        .unwrap();
    h.act(CarryAction::Timeout(id(CarryStage::PerpExit)))
        .unwrap();
    world.result = Some(h.act(CarryAction::PlanSpotExit {
        pair: 1,
        max_spot_price: price(1_200 * U),
    }));
}

#[then("the spot exit is refused and both leg liabilities remain visible")]
fn unknown_keeps_spot(world: &mut CarryWorld) {
    assert_eq!(world.result, Some(Err(CarryError::Phase)));
    let state = world.h.as_ref().unwrap().engine.snapshot();
    assert_eq!(state.account.spot_btc, qty(99_900_000));
    assert_eq!(state.account.perp_short, qty(99_900_000));
    assert!(state.halted);
    assert!(state.perp_collateral_reserved.0 > 0);
    assert!(state
        .orders
        .iter()
        .any(|o| o.progress.state() == OrderState::Unknown));
    assert_eq!(state.phase, CarryPhase::Unresolved);
}

#[when("both visible marks rally enough to breach the isolated margin scenario")]
fn margin_breach(world: &mut CarryWorld) {
    let h = world.h.as_mut().unwrap();
    world.before = Some(h.engine.snapshot());
    let receipt = h.receipts[0].clone();
    world.result = Some(h.marked(
        CarryAction::Deliver(receipt.with_value(CarryReport::Fill(receipt.value().clone()))),
        10_000 * U,
        10_000 * U,
    ));
}

#[then("positive pair equity does not move spot gains or idle cash into isolated collateral")]
fn independent_margin(world: &mut CarryWorld) {
    let state = world.h.as_ref().unwrap().engine.snapshot();
    assert_eq!(state.account, world.before.as_ref().unwrap().account);
    assert!(
        state
            .account
            .equity(price(10_000 * U), price(10_000 * U))
            .unwrap()
            .0
            > 0
    );
    assert!(state.margin_latched && state.halted);
    assert!(!state.loss_latched);
}

#[when("the one-leg deadline expires while no hedge can be evidenced")]
fn deadline(world: &mut CarryWorld) {
    let h = world.h.as_mut().unwrap();
    world.before = Some(h.engine.snapshot());
    h.now = 10_004;
    world.result = Some(h.act(CarryAction::NoExecutionEvidence {
        order: id(CarryStage::SpotEntry),
        kind: MissingExecution::ZeroCount,
    }));
}

#[then("the trial is unresolved with the paid spot inventory and reserved hedge capital")]
fn stranded(world: &mut CarryWorld) {
    assert_eq!(world.result, Some(Err(CarryError::UnsupportedFact)));
    let state = world.h.as_ref().unwrap().engine.snapshot();
    assert_eq!(state.account, world.before.as_ref().unwrap().account);
    assert_eq!(state.account.spot_btc, qty(99_900_000));
    assert!(state.perp_collateral_reserved.0 > 0);
    assert_eq!(state.phase, CarryPhase::Unresolved);
}

#[when("the same accounted funding receipt is replayed at full retention capacity")]
fn duplicate(world: &mut CarryWorld) {
    let h = world.h.as_mut().unwrap();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let at = u64::try_from(effective.micros() - AT).unwrap();
    let receipt = observed(
        CarryFundingReceipt {
            key: 1,
            effective,
            rate: Ratio::from_ticks(10_000),
            charge_mark: price(1_000 * U),
        },
        at,
        h.now + 1,
    );
    let action = CarryAction::FundingReceipt(receipt);
    h.act(action.clone()).unwrap();
    world.before = Some(h.engine.snapshot());
    world.result = Some(h.act(action));
}

#[then("no second funding credit or identity is created")]
fn no_double_credit(world: &mut CarryWorld) {
    assert_eq!(world.result, Some(Ok(CarryOutcome::Duplicate)));
    let state = world.h.as_ref().unwrap().engine.snapshot();
    assert_eq!(state.account, world.before.as_ref().unwrap().account);
    assert_eq!(state.funding.len(), 1);
    assert_eq!(state.account.funding_net, Cash(9_990_000));
}

#[when("only the original session and data commands reconstruct a fresh engine")]
fn replay(world: &mut CarryWorld) {
    let h = world.h.as_mut().unwrap();
    h.act(CarryAction::BeginExit {
        pair: 1,
        max_perp_price: price(1_200 * U),
    })
    .unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpExit)))
        .unwrap();
    h.act(CarryAction::Timeout(id(CarryStage::PerpExit)))
        .unwrap();
    let mut fresh = CarryEngine::new(h.initial.clone()).unwrap();
    for command in &h.commands {
        fresh.step(command).unwrap();
    }
    world.restarted = Some(fresh.snapshot());
}

#[then(
    "wallets, order progress, retained liabilities and halt epoch match the uninterrupted engine"
)]
fn reconstructed(world: &mut CarryWorld) {
    let expected = world.h.as_ref().unwrap().engine.snapshot();
    assert_eq!(world.restarted, Some(expected));
    assert!(world.restarted.as_ref().unwrap().halted);
}

#[test]
fn synthetic_carry_behavior() {
    futures::executor::block_on(
        CarryWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/carry/accounting.feature"
            ))),
    );
}
