mod support;

use cucumber::{given, then, when, World as _};
use parangsae_core::{Observation, Price, Side, UtcMicros};
use parangsae_risk::*;
use support::*;

#[derive(Debug, Default, cucumber::World)]
struct World {
    gate: Option<RiskGate>,
    mark: Option<Observation<Price>>,
    approvals: Vec<ApprovedOrder>,
    decision: Option<DispatchDecision>,
    error: Option<RiskError>,
}

#[given("a flat declared paper portfolio")]
fn initial(world: &mut World) {
    world.gate = Some(gate(0));
    world.mark = Some(mark(MARK));
}

#[given("a declared one BTC long paper portfolio")]
fn initial_long(world: &mut World) {
    world.gate = Some(gate(UNIT));
    world.mark = Some(mark(MARK));
}

#[when("a higher causal mark breaches its notional bound")]
fn mark_breach(world: &mut World) {
    world.mark = Some(mark(30_000 * UNIT));
    world
        .gate
        .as_mut()
        .unwrap()
        .observe(&context(world.mark.as_ref().unwrap(), UNIT, 1))
        .unwrap();
}

#[then("a fresh reduce-only half close is approved while risk remains halted")]
fn protective_close(world: &mut World) {
    let cx = context(world.mark.as_ref().unwrap(), UNIT, 1);
    let gate = world.gate.as_mut().unwrap();
    let approval = gate
        .check(request(1, Side::Sell, UNIT / 2, true), &cx)
        .unwrap();
    assert!(gate.is_halted());
    assert!(matches!(
        gate.prepare_dispatch(approval, &cx).unwrap().commit(),
        DispatchDecision::Authorized(_)
    ));
    assert!(gate.is_halted());
}

#[when("one BTC buy and one BTC sell are approved")]
fn opposing(world: &mut World) {
    let gate = world.gate.as_mut().unwrap();
    let cx = context(world.mark.as_ref().unwrap(), 0, 0);
    world
        .approvals
        .push(gate.check(request(1, Side::Buy, UNIT, false), &cx).unwrap());
    world.approvals.push(
        gate.check(request(2, Side::Sell, UNIT, false), &cx)
            .unwrap(),
    );
}

#[then("a further buy is refused by the position limit")]
fn cannot_net(world: &mut World) {
    let cx = context(world.mark.as_ref().unwrap(), 0, 0);
    assert_eq!(
        world
            .gate
            .as_mut()
            .unwrap()
            .check(request(3, Side::Buy, 100_000, false), &cx)
            .unwrap_err(),
        RiskError::PositionLimit
    );
}

#[when("an approved buy is followed by a kill latch")]
fn kill(world: &mut World) {
    let cx = context(world.mark.as_ref().unwrap(), 0, 0);
    let gate = world.gate.as_mut().unwrap();
    let approval = gate.check(request(1, Side::Buy, UNIT, false), &cx).unwrap();
    gate.latch(HaltCause::Kill);
    world.decision = Some(gate.prepare_dispatch(approval, &cx).unwrap().commit());
}

#[then("its dispatch is refused without a fill")]
fn stale(world: &mut World) {
    assert!(matches!(
        world.decision,
        Some(DispatchDecision::Refused {
            reason: RiskError::StaleApproval,
            ..
        })
    ));
    assert_eq!(world.gate.as_ref().unwrap().snapshot().position_ticks, 0);
}

#[when("a dispatched buy receives a priced half fill and an inconsistent terminal cut")]
fn bad_cut(world: &mut World) {
    let cx = context(world.mark.as_ref().unwrap(), 0, 0);
    let gate = world.gate.as_mut().unwrap();
    let req = request(1, Side::Buy, UNIT, false);
    let id = req.id();
    let approval = gate.check(req, &cx).unwrap();
    assert!(matches!(
        gate.prepare_dispatch(approval, &cx).unwrap().commit(),
        DispatchDecision::Authorized(_)
    ));
    let fill = FillReceipt {
        key: FillKey {
            order: id,
            fill_id: 1,
        },
        qty: qty(UNIT / 2),
        price: Price::from_ticks(MARK).unwrap(),
        fee: amount(5),
        at: UtcMicros::from_micros(AT),
    };
    let cut = TerminalEvidence {
        order: id,
        status: TerminalStatus::Canceled,
        cumulative: qty(UNIT),
        receipts: vec![fill.key],
        complete: true,
    };
    world.error = gate
        .prepare_settlement(
            &[Settlement::Fill(fill), Settlement::Terminal(cut)],
            &cx,
            PortfolioSnapshot {
                position_ticks: UNIT / 2,
                equity_ticks: EQUITY - 5,
            },
        )
        .err();
}

#[then("position remains flat and its reservation remains held")]
fn unchanged(world: &mut World) {
    assert_eq!(world.error, Some(RiskError::IncompleteTerminal));
    let snapshot = world.gate.as_ref().unwrap().snapshot();
    assert_eq!(snapshot.position_ticks, 0);
    assert_eq!(snapshot.orders[0].remaining.ticks(), UNIT);
    assert!(snapshot.halted);
}

fn main() {
    futures::executor::block_on(World::run("../../specs/features/risk"));
}
