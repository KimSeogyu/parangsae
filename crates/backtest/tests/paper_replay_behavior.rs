#[path = "common/paper_cases.rs"]
mod cases;

use cases::*;
use cucumber::{given, then, when, World as _};
use parangsae_backtest::offline_replay::{OfflineReplay, ReplayError};
use parangsae_core::{Amount, Observation};
use parangsae_execution::{
    HaltCause, OrderState, PaperAction, PaperEngine, PaperError, PaperOutcome, PaperReport,
    PaperSession, PaperSnapshot, PaperTerminal, PaperTerminalCut,
};
use parangsae_risk::{ExposureState, Limits, RiskError};

#[derive(Debug, Default, cucumber::World)]
struct PaperWorld {
    engine: Option<PaperEngine>,
    initial: Option<PaperSession>,
    now: u64,
    receipts: Vec<Observation<parangsae_execution::PaperFill>>,
    result: Option<Result<PaperOutcome, PaperError>>,
    before: Option<PaperSnapshot>,
    continuous: Option<PaperSnapshot>,
    restarted: Option<PaperSnapshot>,
    files: Option<tempfile::TempDir>,
    damaged: Vec<u8>,
    recovery_refused: bool,
}
impl PaperWorld {
    fn flat(&mut self) {
        let initial = session();
        self.engine = Some(PaperEngine::new(initial.clone()).unwrap());
        self.initial = Some(initial);
        self.now = 0;
    }
    fn dispatched(&mut self) {
        self.flat();
        for command in prefix() {
            self.apply(command);
            self.ok();
        }
    }
    fn apply(&mut self, command: parangsae_execution::PaperCommand) {
        self.now = command.now.micros();
        self.result = Some(self.engine.as_mut().unwrap().step(&command));
    }
    fn act(&mut self, action: PaperAction) {
        self.apply(command(self.now + 1, action));
    }
    fn ok(&self) {
        self.result.as_ref().unwrap().as_ref().unwrap();
    }
    fn state(&self) -> PaperSnapshot {
        self.engine.as_ref().unwrap().snapshot()
    }
    fn fill(&mut self, number: u64, ticks: i64) {
        let receipt = fill(self.now + 1, number, ticks);
        self.apply(deliver(self.now + 1, &receipt));
        self.ok();
        self.receipts.push(receipt);
    }
}

#[given("an explicit flat BTCUSDT paper session with 10000 USDT")]
fn flat(world: &mut PaperWorld) {
    world.flat();
}
#[given("taker fees are 5 bps and slippage is 2 bps")]
fn costs(world: &mut PaperWorld) {
    assert_eq!(world.initial.as_ref().unwrap().costs.taker_fee.0, 5);
    assert_eq!(world.initial.as_ref().unwrap().costs.slippage.0, 2);
}
#[given("a paper-dispatched buy of 0.004 BTC")]
fn dispatched(world: &mut PaperWorld) {
    world.dispatched();
}

#[given("a paper-dispatched buy waiting for its first acknowledgment")]
fn awaiting_ack(world: &mut PaperWorld) {
    world.flat();
    for command in prefix().into_iter().take(2) {
        world.apply(command);
        world.ok();
    }
}

#[when("the paper source definitively rejects the order")]
fn source_reject(world: &mut PaperWorld) {
    world.act(PaperAction::Deliver(observation(
        PaperReport::Rejected {
            client_order_id: id(1),
        },
        world.now + 1,
    )));
    world.ok();
}

#[then("the order is Rejected and its reservation is released")]
fn rejection_released(world: &mut PaperWorld) {
    assert_eq!(
        world.state().orders[0].progress.state(),
        OrderState::Rejected
    );
    assert!(world.state().risk.orders.is_empty());
}
#[given("paper fill 1 has settled 0.001 BTC at reference price 30000 USDT")]
fn settled(world: &mut PaperWorld) {
    world.fill(1, 100_000);
}
#[given("a paper-dispatched buy of 0.004 BTC that timed out")]
fn unknown_start(world: &mut PaperWorld) {
    world.dispatched();
    timeout(world);
}
#[given(
    regex = "^(?:a planned buy with the explicit approval lifetime|a planned buy at the current halt epoch|a planned buy that has not been paper-dispatched)$"
)]
fn planned(world: &mut PaperWorld) {
    world.flat();
    world.apply(plan(1, 1));
    world.ok();
}
#[given("a paper session with an explicit session loss limit")]
fn loss_start(world: &mut PaperWorld) {
    let mut initial = session();
    initial.limits = Limits::new(
        initial.limits.max_order(),
        initial.limits.max_position(),
        initial.limits.max_notional(),
        Amount::from_ticks(1).unwrap(),
        initial.limits.max_approval_age_us(),
        initial.limits.max_mark_age_us(),
        initial.limits.capacity(),
    )
    .unwrap();
    world.engine = Some(PaperEngine::new(initial.clone()).unwrap());
    world.initial = Some(initial);
}
#[given("the scripted paper session has a durable ingress journal")]
fn journal(world: &mut PaperWorld) {
    let dir = tempfile::tempdir().unwrap();
    let (initial, commands) = full_fill();
    let mut run = OfflineReplay::create(&dir.path().join("base.journal"), initial).unwrap();
    for command in commands {
        run.apply(command).unwrap().outcome.unwrap();
    }
    drop(run);
    world.files = Some(dir);
}

#[when("a scripted buy of 0.004 BTC is approved and paper-dispatched")]
fn submit(world: &mut PaperWorld) {
    for command in prefix() {
        world.apply(command);
        world.ok();
    }
}
#[when(
    regex = "^(?:paper|race) fill ([12]) reports (?:another )?(0\\.001|0\\.003|0\\.004) BTC at reference price 30000 USDT$"
)]
fn fill_step(world: &mut PaperWorld, number: u64, quantity: String) {
    let ticks = match quantity.as_str() {
        "0.001" => 100_000,
        "0.003" => 300_000,
        "0.004" => 400_000,
        _ => unreachable!(),
    };
    world.fill(number, ticks);
}
#[when("the exact first receipt is delivered again")]
fn duplicate(world: &mut PaperWorld) {
    world.before = Some(world.state());
    world.apply(deliver(world.now + 1, &world.receipts[0]));
    assert_eq!(world.result, Some(Ok(PaperOutcome::DuplicateReceipt)));
    assert_eq!(world.state(), *world.before.as_ref().unwrap());
}
#[when("fill 1 is delivered with a changed reference price")]
fn changed(world: &mut PaperWorld) {
    world.before = Some(world.state());
    let old = &world.receipts[0];
    let mut value = old.value().clone();
    value.reference_price = price(30_001 * UNIT);
    let changed = Observation::new(old.timestamps(), value);
    world.apply(deliver(world.now + 1, &changed));
    assert_eq!(world.result, Some(Err(PaperError::ReceiptConflict)));
}
#[when("a future-received fill is presented before its delivery time")]
fn future(world: &mut PaperWorld) {
    world.before = Some(world.state());
    world.apply(deliver(world.now + 1, &fill(100, 1, 400_000)));
    assert_eq!(world.result, Some(Err(PaperError::FutureReport)));
}
#[when("cancellation is requested and confirmed")]
fn cancel_confirm(world: &mut PaperWorld) {
    world.act(PaperAction::Cancel {
        client_order_id: id(1),
    });
    world.ok();
    world.act(PaperAction::Deliver(observation(
        PaperReport::CancelConfirmed {
            client_order_id: id(1),
        },
        world.now + 1,
    )));
    world.ok();
}
#[when("cancellation is requested")]
fn local_cancel(world: &mut PaperWorld) {
    world.act(PaperAction::Cancel {
        client_order_id: id(1),
    });
    world.ok();
}
#[when("a complete canceled cut lists both priced fills")]
fn canceled_cut(world: &mut PaperWorld) {
    world.apply(cut(
        world.now + 1,
        PaperTerminal::Canceled,
        world.receipts.clone(),
    ));
    world.ok();
}
#[when("the order times out")]
fn timeout(world: &mut PaperWorld) {
    world.act(PaperAction::Timeout {
        client_order_id: id(1),
    });
    world.ok();
}
#[when("a Filled cut reports 0.004 BTC without priced receipts")]
fn unpriced(world: &mut PaperWorld) {
    world.before = Some(world.state());
    let at = world.now + 1;
    world.act(PaperAction::Reconcile(observation(
        PaperTerminalCut {
            client_order_id: id(1),
            evidence_id: at,
            status: PaperTerminal::Filled,
            cumulative: qty(400_000),
            receipts: Vec::new(),
            complete: true,
        },
        at,
    )));
    assert_eq!(world.result, Some(Err(PaperError::IncompleteCut)));
}
#[when("a complete Filled cut supplies the 0.004 BTC priced receipt")]
fn priced_cut(world: &mut PaperWorld) {
    let receipt = fill(world.now + 1, 1, 400_000);
    world.apply(cut(world.now + 2, PaperTerminal::Filled, vec![receipt]));
    world.ok();
}
#[when("dispatch occurs strictly after that lifetime")]
fn expired(world: &mut PaperWorld) {
    world.apply(command(
        10_002,
        PaperAction::Dispatch {
            client_order_id: id(1),
        },
    ));
    world.ok();
}
#[when("a halt is latched before dispatch")]
fn halt_before(world: &mut PaperWorld) {
    world.act(PaperAction::Halt(HaltCause::RiskTrip));
    world.ok();
    world.act(PaperAction::Dispatch {
        client_order_id: id(1),
    });
    world.ok();
}
#[when("a priced fill breaches the frozen initial-equity loss limit")]
fn loss_fill(world: &mut PaperWorld) {
    submit(world);
    world.fill(1, 400_000);
    assert!(world.state().risk.loss_latched);
}
#[when("a matching reconciliation is followed by resume")]
fn loss_resume(world: &mut PaperWorld) {
    world.apply(cut(
        world.now + 1,
        PaperTerminal::Filled,
        world.receipts.clone(),
    ));
    world.ok();
    world.act(PaperAction::ReconcilePortfolio);
    world.ok();
    world.act(PaperAction::Resume);
    assert_eq!(
        world.result,
        Some(Err(PaperError::Risk(RiskError::LossLatched)))
    );
}
#[when(
    regex = "^the process stops at the (partial-fill|cancel-race|unknown|halt) boundary and resumes$"
)]
fn restart(world: &mut PaperWorld, boundary: String) {
    let ((initial, commands), split) = match boundary.as_str() {
        "partial-fill" => (partial(), 4),
        "cancel-race" => (cancel_race(), 6),
        "unknown" => (unknown(), 4),
        "halt" => (halted(), 2),
        _ => unreachable!(),
    };
    let mut continuous = PaperEngine::new(initial.clone()).unwrap();
    for command in &commands {
        continuous.step(command).unwrap();
    }
    let path = world.files.as_ref().unwrap().path().join("restart.journal");
    let mut run = OfflineReplay::create(&path, initial).unwrap();
    for command in &commands[..split] {
        run.apply(command.clone()).unwrap().outcome.unwrap();
    }
    drop(run);
    let mut resumed = OfflineReplay::resume(&path).unwrap();
    for command in &commands[split..] {
        resumed.apply(command.clone()).unwrap().outcome.unwrap();
    }
    world.continuous = Some(continuous.snapshot());
    world.restarted = Some(resumed.snapshot());
}
#[when("a completed input record is corrupted")]
fn corrupt(world: &mut PaperWorld) {
    let path = world.files.as_ref().unwrap().path().join("base.journal");
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.len() - 8;
    bytes[at] ^= 1;
    std::fs::write(&path, &bytes).unwrap();
    world.recovery_refused = matches!(
        OfflineReplay::resume(&path),
        Err(ReplayError::CorruptJournal { .. })
    );
    world.damaged = bytes;
}

#[then(regex = "^the order is (Filled|Canceled) with (0\\.004|0\\.002) BTC cumulative fill$")]
fn order_state(world: &mut PaperWorld, state: String, quantity: String) {
    let state_expected = if state == "Filled" {
        OrderState::Filled
    } else {
        OrderState::Canceled
    };
    let cumulative = if quantity == "0.004" {
        400_000
    } else {
        200_000
    };
    assert_eq!(world.state().orders[0].progress.state(), state_expected);
    assert_eq!(world.state().orders[0].progress.cum().ticks(), cumulative);
}
#[then("cash is 9879.915988 USDT and fees are 0.060012 USDT")]
fn money(world: &mut PaperWorld) {
    assert_eq!(world.state().cash.0, 987_991_598_800);
    assert_eq!(world.state().fees_paid.0, 6_001_200);
}
#[then("only two economic fills were applied")]
fn fills(world: &mut PaperWorld) {
    assert_eq!(world.state().fills.len(), 2);
}
#[then("execution is halted for conflicting evidence")]
fn conflict_halt(world: &mut PaperWorld) {
    assert!(world.state().risk.halted);
    assert_eq!(world.result, Some(Err(PaperError::ReceiptConflict)));
}
#[then("cash, fees, position and cumulative quantity are unchanged")]
fn unchanged(world: &mut PaperWorld) {
    let before = world.before.as_ref().unwrap();
    let after = world.state();
    assert_eq!(before.cash, after.cash);
    assert_eq!(before.fees_paid, after.fees_paid);
    assert_eq!(before.position, after.position);
    assert_eq!(before.orders, after.orders);
}
#[then("no fill or fee is applied")]
fn no_fills(world: &mut PaperWorld) {
    assert!(world.state().fills.is_empty());
    assert_eq!(world.state().fees_paid.0, 0);
}
#[then("the residual reservation is retained before the complete terminal cut")]
fn retained(world: &mut PaperWorld) {
    assert_eq!(world.state().risk.orders[0].remaining.ticks(), 200_000);
}
#[then(regex = "^(?:the residual reservation is released|its real Risk reservation is released)$")]
fn released(world: &mut PaperWorld) {
    assert!(world.state().risk.orders.is_empty());
}
#[then("exactly 0.002 BTC is held")]
fn position(world: &mut PaperWorld) {
    assert_eq!(world.state().position.0, 200_000);
}
#[then("the order is Unknown and exposure is unknown")]
fn unknown_state(world: &mut PaperWorld) {
    assert_eq!(
        world.state().orders[0].progress.state(),
        OrderState::Unknown
    );
    assert_eq!(world.state().risk.exposure, ExposureState::Unknown);
}
#[then("a second exposure-increasing signal cannot dispatch")]
fn blocked(world: &mut PaperWorld) {
    world.apply(plan(world.now + 1, 2));
    assert_eq!(
        world.result,
        Some(Err(PaperError::Risk(RiskError::ExposureUnknown)))
    );
}
#[then("accounting and cumulative quantity remain unchanged")]
fn unknown_unchanged(world: &mut PaperWorld) {
    unchanged(world);
}
#[then(
    regex = "^(?:execution remains halted|execution remains halted until explicit matching reconciliation and resume|the session loss halt remains latched)$"
)]
fn remains_halted(world: &mut PaperWorld) {
    assert!(world.state().risk.halted);
}
#[then("the unsent order is definitively refused")]
fn refused(world: &mut PaperWorld) {
    assert!(matches!(
        world.result,
        Some(Ok(PaperOutcome::Refused {
            reason: RiskError::ExpiredApproval,
            ..
        }))
    ));
    assert_eq!(
        world.state().orders[0].progress.state(),
        OrderState::Rejected
    );
}
#[then("no paper submission or fill occurred")]
fn no_submit(world: &mut PaperWorld) {
    assert!(!world.state().orders[0].progress.sent());
    assert!(world.state().fills.is_empty());
}
#[then("the old approval cannot dispatch")]
fn stale(world: &mut PaperWorld) {
    assert!(matches!(
        world.result,
        Some(Ok(PaperOutcome::Refused {
            reason: RiskError::StaleApproval,
            ..
        }))
    ));
    no_submit(world);
}
#[then("the order is Canceled without a paper submission")]
fn local_canceled(world: &mut PaperWorld) {
    assert_eq!(
        world.state().orders[0].progress.state(),
        OrderState::Canceled
    );
    no_submit(world);
}
#[then("the final cash, position, fees, fills and order states match uninterrupted execution")]
fn replay_equal(world: &mut PaperWorld) {
    assert_eq!(world.continuous, world.restarted);
}
#[then("halt epoch, exposure and reservations match uninterrupted execution")]
fn replay_risk(world: &mut PaperWorld) {
    assert_eq!(
        world.continuous.as_ref().unwrap().risk,
        world.restarted.as_ref().unwrap().risk
    );
}
#[then("an already settled receipt is not applied twice")]
fn replay_once(world: &mut PaperWorld) {
    assert_eq!(
        world.continuous.as_ref().unwrap().fills,
        world.restarted.as_ref().unwrap().fills
    );
}
#[then("recovery is refused without repairing the completed records")]
fn corrupt_refused(world: &mut PaperWorld) {
    assert!(world.recovery_refused);
    assert_eq!(
        std::fs::read(world.files.as_ref().unwrap().path().join("base.journal")).unwrap(),
        world.damaged
    );
}

#[tokio::main]
async fn main() {
    PaperWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../specs/features/paper_replay.feature"
        )))
        .await;
}
