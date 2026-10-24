#[path = "../../backtest/tests/common/paper_cases.rs"]
mod cases;

use cases::*;
use parangsae_core::{Amount, DecisionTime, Observation, UtcMicros};
use parangsae_execution::{
    OrderState, PaperAction, PaperEngine, PaperError, PaperOutcome, PaperTerminal, PaperTerminalCut,
};
use parangsae_risk::{ExposureState, Limits, RiskError};

fn run(
    session: parangsae_execution::PaperSession,
    commands: &[parangsae_execution::PaperCommand],
) -> PaperEngine {
    let mut engine = PaperEngine::new(session).unwrap();
    for command in commands {
        engine.step(command).unwrap();
    }
    engine
}

#[test]
fn actual_risk_approved_fill_changes_fixed_point_accounting() {
    let (session, commands) = full_fill();
    let engine = run(session, &commands);
    let state = engine.snapshot();
    assert_eq!(state.position.0, 400_000);
    assert_eq!(state.cash.0, 987_991_598_800);
    assert_eq!(state.fees_paid.0, 6_001_200);
    assert_eq!(state.fills.len(), 1);
    assert_eq!(state.orders[0].progress.state(), OrderState::Filled);
    assert!(state.orders[0].progress.sent());
    assert!(state.orders[0].progress.last_sent_at().is_some());
    assert!(state.risk.orders.is_empty());
}

#[test]
fn exact_receipts_are_noops_and_changed_receipts_halt_without_accounting_change() {
    let (session, commands) = partial();
    let mut engine = run(session, &commands);
    let before = engine.snapshot();
    assert_eq!(before.fills.len(), 2);
    assert_eq!(before.cash.0, 987_991_598_800);
    assert_eq!(
        engine.step(&deliver(9, &fill(4, 1, 100_000))),
        Ok(PaperOutcome::DuplicateReceipt)
    );
    assert_eq!(engine.snapshot(), before);
    let mut changed = fill(4, 1, 100_000);
    let mut value = changed.value().clone();
    value.reference_price = price(30_001 * UNIT);
    changed = Observation::new(changed.timestamps(), value);
    assert_eq!(
        engine.step(&deliver(10, &changed)),
        Err(PaperError::ReceiptConflict)
    );
    let after = engine.snapshot();
    assert!(after.risk.halted);
    assert_eq!(after.cash, before.cash);
    assert_eq!(after.position, before.position);
    assert_eq!(after.fees_paid, before.fees_paid);
    assert_eq!(after.orders, before.orders);
    assert_eq!(after.fills, before.fills);
}

#[test]
fn cancel_race_keeps_reservation_until_the_complete_terminal_cut() {
    let (session, commands) = cancel_race();
    let mut engine = run(session, &commands[..7]);
    let before = engine.snapshot();
    assert_eq!(before.orders[0].progress.state(), OrderState::Canceled);
    assert_eq!(before.orders[0].progress.cum().ticks(), 200_000);
    assert_eq!(before.risk.orders[0].remaining.ticks(), 200_000);
    for command in &commands[7..] {
        engine.step(command).unwrap();
    }
    let after = engine.snapshot();
    assert_eq!(after.position.0, 200_000);
    assert!(after.risk.orders.is_empty());
    assert_eq!(after.fills.len(), 2);
}

#[test]
fn unknown_requires_complete_priced_evidence_and_explicit_portfolio_resume() {
    let (session, commands) = unknown();
    let mut engine = run(session, &commands[..4]);
    let before = engine.snapshot();
    assert!(before.risk.halted);
    assert_eq!(before.risk.exposure, ExposureState::Unknown);
    assert_eq!(
        engine.step(&plan(5, 2)),
        Err(PaperError::Risk(RiskError::ExposureUnknown))
    );
    let missing = command(
        6,
        PaperAction::Reconcile(observation(
            PaperTerminalCut {
                client_order_id: id(1),
                evidence_id: 6,
                status: PaperTerminal::Filled,
                cumulative: qty(400_000),
                receipts: Vec::new(),
                complete: true,
            },
            6,
        )),
    );
    assert_eq!(engine.step(&missing), Err(PaperError::IncompleteCut));
    assert_eq!(engine.snapshot().cash, before.cash);
    assert_eq!(engine.snapshot().orders, before.orders);
    engine.step(&commands[4]).unwrap();
    assert!(engine.snapshot().risk.halted);
    assert_eq!(engine.snapshot().position.0, 400_000);
    engine.step(&commands[5]).unwrap();
    engine.step(&commands[6]).unwrap();
    assert!(!engine.snapshot().risk.halted);
}

#[test]
fn expired_and_old_epoch_approvals_never_paper_dispatch() {
    let mut engine = run(session(), &[plan(1, 1)]);
    let expired = engine
        .step(&command(
            10_002,
            PaperAction::Dispatch {
                client_order_id: id(1),
            },
        ))
        .unwrap();
    assert!(matches!(
        expired,
        PaperOutcome::Refused {
            reason: RiskError::ExpiredApproval,
            ..
        }
    ));
    assert!(!engine.snapshot().orders[0].progress.sent());
    assert!(engine.snapshot().fills.is_empty());
    let (session, commands) = halted();
    let engine = run(session, &commands);
    assert!(!engine.snapshot().orders[0].progress.sent());
    assert!(engine.snapshot().fills.is_empty());
}

#[test]
fn a_future_received_report_never_changes_accounting() {
    let mut engine = run(session(), &prefix());
    let before = engine.snapshot();
    let future = fill(100, 1, 400_000);
    assert_eq!(
        engine.step(&deliver(4, &future)),
        Err(PaperError::FutureReport)
    );
    let after = engine.snapshot();
    assert_eq!(after.cash, before.cash);
    assert_eq!(after.position, before.position);
    assert_eq!(after.fills, before.fills);
}

#[test]
fn late_terminal_failure_drops_every_staged_fill() {
    let mut engine = run(session(), &prefix());
    engine
        .step(&command(
            4,
            PaperAction::Timeout {
                client_order_id: id(1),
            },
        ))
        .unwrap();
    let before = engine.snapshot();
    // The priced partial receipt is valid, but Filled requires the full order.
    let invalid = cut(10, PaperTerminal::Filled, vec![fill(5, 1, 100_000)]);
    assert!(engine.step(&invalid).is_err());
    let after = engine.snapshot();
    assert_eq!(after.cash, before.cash);
    assert_eq!(after.position, before.position);
    assert_eq!(after.fees_paid, before.fees_paid);
    assert_eq!(after.orders, before.orders);
    assert_eq!(after.fills, before.fills);
    assert!(after.risk.halted);
}

#[test]
fn loss_anchor_stays_latched_after_matching_reconciliation() {
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
    let (_, commands) = full_fill();
    let mut engine = run(initial, &commands);
    assert!(engine.snapshot().risk.loss_latched);
    assert_eq!(
        engine.step(&command(7, PaperAction::Resume)),
        Err(PaperError::Risk(RiskError::LossLatched))
    );
    assert!(engine.snapshot().risk.halted);
}

#[test]
fn command_journal_codec_preserves_canonical_ids_and_all_three_clocks() {
    for (_, commands) in [full_fill(), partial(), cancel_race(), unknown(), halted()] {
        for command in commands {
            let bytes = serde_json::to_vec(&command).unwrap();
            let restored = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(command, restored);
        }
    }
    let mut value = serde_json::to_value(plan(1, 1)).unwrap();
    value["action"]["approval"] = serde_json::json!("forged");
    assert!(serde_json::from_value::<parangsae_execution::PaperCommand>(value).is_err());
    let mut engine = PaperEngine::new(session()).unwrap();
    let mut backward = plan(1, 1);
    backward.decision = DecisionTime::from_micros(UtcMicros::from_micros(AT - 1));
    assert_eq!(engine.step(&backward), Err(PaperError::ClockWentBackwards));
}

#[test]
fn an_unsent_cancel_is_local_and_releases_the_actual_risk_reservation() {
    let mut engine = run(session(), &[plan(1, 1)]);
    let outcome = engine
        .step(&command(
            2,
            PaperAction::Cancel {
                client_order_id: id(1),
            },
        ))
        .unwrap();
    assert!(matches!(
        outcome,
        PaperOutcome::StateChanged {
            state: OrderState::Canceled,
            ..
        }
    ));
    let state = engine.snapshot();
    assert!(!state.orders[0].progress.sent());
    assert_eq!(state.orders[0].progress.state(), OrderState::Canceled);
    assert!(state.risk.orders.is_empty());
    assert!(state.fills.is_empty());
    assert_eq!(state.cash.0, 10_000 * UNIT);
    assert!(engine
        .step(&command(
            3,
            PaperAction::Dispatch {
                client_order_id: id(1)
            }
        ))
        .is_err());
}

#[test]
fn a_complete_known_terminal_cut_fits_at_full_receipt_retention() {
    let mut initial = session();
    initial.limits = Limits::new(
        initial.limits.max_order(),
        initial.limits.max_position(),
        initial.limits.max_notional(),
        initial.limits.max_session_loss(),
        initial.limits.max_approval_age_us(),
        initial.limits.max_mark_age_us(),
        1,
    )
    .unwrap();
    let (_, commands) = full_fill();
    let mut engine = run(initial, &commands[..4]);
    engine.step(&commands[4]).unwrap();
    assert!(engine.snapshot().risk.orders.is_empty());
    assert_eq!(
        engine.step(&deliver(6, &fill(4, 1, 400_000))),
        Ok(PaperOutcome::DuplicateReceipt)
    );
}

#[test]
fn retry_cid_attempt_is_preserved_by_initial_progress_and_bounded_by_configuration() {
    use parangsae_execution::{
        Attempt, ClientOrderId, ExecutionConfig, IntentId, MaxAttempts, PositiveMicros,
    };
    let retry = ClientOrderId::from_parts(
        IntentId::from_raw(1),
        Attempt::from_index(1, MaxAttempts::new(3).unwrap()).unwrap(),
    );
    let mut retry_plan = plan(3, 1);
    if let PaperAction::Plan {
        client_order_id, ..
    } = &mut retry_plan.action
    {
        *client_order_id = retry.clone();
    }
    let mut engine = run(
        session(),
        &[
            plan(1, 1),
            command(
                2,
                PaperAction::Cancel {
                    client_order_id: id(1),
                },
            ),
        ],
    );
    engine.step(&retry_plan).unwrap();
    assert_eq!(
        engine.snapshot().orders[1].progress.attempt(),
        retry.attempt()
    );
    let mut initial = session();
    initial.execution = ExecutionConfig::new(
        initial.execution.timing(),
        *initial.execution.authority(),
        MaxAttempts::new(1).unwrap(),
        PositiveMicros::new(initial.limits.max_approval_age_us()).unwrap(),
    )
    .unwrap();
    let mut bounded = run(
        initial,
        &[
            plan(1, 1),
            command(
                2,
                PaperAction::Cancel {
                    client_order_id: id(1),
                },
            ),
        ],
    );
    assert_eq!(
        bounded.step(&retry_plan),
        Err(PaperError::Progress(
            parangsae_execution::ProgressError::AttemptOutOfRange
        ))
    );
    assert_eq!(bounded.snapshot().orders.len(), 1);
}

#[test]
fn expired_late_receipt_recovery_is_refused_without_inventing_a_transition() {
    use parangsae_execution::PaperReport;
    let mut engine = run(session(), &prefix());
    engine
        .step(&command(
            4,
            PaperAction::Deliver(observation(
                PaperReport::Expired {
                    client_order_id: id(1),
                },
                4,
            )),
        ))
        .unwrap();
    let before = engine.snapshot();
    let late = cut(10, PaperTerminal::Expired, vec![fill(3, 1, 100_000)]);
    assert!(engine.step(&late).is_err());
    let after = engine.snapshot();
    assert!(after.risk.halted);
    assert_eq!(after.cash, before.cash);
    assert_eq!(after.orders, before.orders);
    assert_eq!(after.fills, before.fills);
    assert_eq!(after.risk.orders[0].remaining.ticks(), 400_000);
}

#[test]
fn a_definitive_paper_rejection_releases_its_unfilled_reservation() {
    use parangsae_execution::PaperReport;
    let mut engine = run(
        session(),
        &[
            plan(1, 1),
            command(
                2,
                PaperAction::Dispatch {
                    client_order_id: id(1),
                },
            ),
        ],
    );
    engine
        .step(&command(
            3,
            PaperAction::Deliver(observation(
                PaperReport::Rejected {
                    client_order_id: id(1),
                },
                3,
            )),
        ))
        .unwrap();
    let state = engine.snapshot();
    assert_eq!(state.orders[0].progress.state(), OrderState::Rejected);
    assert!(state.risk.orders.is_empty());
    assert!(state.fills.is_empty());
    assert_eq!(state.cash.0, 10_000 * UNIT);
}
