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
    let expected_epoch = engine.snapshot().risk.epoch.get();
    engine
        .step(&command(12, PaperAction::Resume { expected_epoch }))
        .unwrap();
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
        engine.step(&command(
            7,
            PaperAction::Resume {
                expected_epoch: engine.snapshot().risk.epoch.get()
            }
        )),
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

#[test]
fn stale_resume_intent_cannot_borrow_the_latest_reconciled_halt_epoch() {
    let (initial, commands) = halted();
    let mut engine = run(initial, &commands[..4]);
    let before = engine.snapshot();
    assert!(before.risk.halted);
    assert_eq!(before.risk.epoch.get(), 1);
    assert_eq!(
        engine.step(&command(5, PaperAction::Resume { expected_epoch: 0 })),
        Err(PaperError::Risk(RiskError::EpochMismatch))
    );
    assert_eq!(engine.snapshot(), before);
    assert!(matches!(
        engine.step(&command(6, PaperAction::Resume { expected_epoch: 1 })),
        Ok(PaperOutcome::Resumed(_))
    ));
    assert!(!engine.snapshot().risk.halted);
    let mut legacy =
        serde_json::to_value(command(7, PaperAction::Resume { expected_epoch: 1 })).unwrap();
    legacy["action"]
        .as_object_mut()
        .unwrap()
        .remove("expected_epoch");
    assert!(serde_json::from_value::<parangsae_execution::PaperCommand>(legacy).is_err());
}

#[test]
fn future_received_mark_cannot_initialize_active_paper_authority() {
    use parangsae_core::ReceivedTs;
    let mut initial = session();
    let mut clocks = initial.initial_mark.timestamps();
    clocks.received_ts = ReceivedTs::from_micros(UtcMicros::from_micros(AT + 100));
    initial.initial_mark = Observation::new(clocks, *initial.initial_mark.value());
    assert!(matches!(
        PaperEngine::new(initial),
        Err(PaperError::FutureMarkReceipt)
    ));
    let mut arrived = session();
    arrived.initial_decision = DecisionTime::from_micros(UtcMicros::from_micros(AT + 100));
    let mut clocks = arrived.initial_mark.timestamps();
    clocks.received_ts = ReceivedTs::from_micros(UtcMicros::from_micros(AT + 50));
    arrived.initial_mark = Observation::new(clocks, *arrived.initial_mark.value());
    assert!(PaperEngine::new(arrived).is_ok());
}

#[test]
fn future_received_mark_cannot_revalue_existing_paper_risk() {
    use parangsae_core::ReceivedTs;
    let (initial, commands) = full_fill();
    let mut engine = run(initial, &commands);
    engine.step(&plan(7, 2)).unwrap();
    engine
        .step(&command(
            8,
            PaperAction::Dispatch {
                client_order_id: id(2),
            },
        ))
        .unwrap();
    let before = engine.snapshot();
    assert!(!before.risk.loss_latched);
    assert_eq!(before.risk.orders.len(), 1);
    let mut future = command(9, PaperAction::ReconcilePortfolio);
    let mut clocks = future.mark.timestamps();
    clocks.received_ts = ReceivedTs::from_micros(UtcMicros::from_micros(AT + 100));
    future.mark = Observation::new(clocks, price(UNIT));
    assert_eq!(engine.step(&future), Err(PaperError::FutureMarkReceipt));
    let after = engine.snapshot();
    assert_eq!(after.cash, before.cash);
    assert_eq!(after.position, before.position);
    assert_eq!(after.fees_paid, before.fees_paid);
    assert_eq!(after.orders, before.orders);
    assert_eq!(after.fills, before.fills);
    assert_eq!(after.risk.orders, before.risk.orders);
    assert!(!after.risk.loss_latched);
    assert!(after.risk.halted);
    assert_eq!(after.risk.epoch.get(), before.risk.epoch.get() + 1);
    // A normally delayed mark is usable once it has actually arrived.
    let mut delayed = command(10, PaperAction::ReconcilePortfolio);
    let mut clocks = delayed.mark.timestamps();
    clocks.received_ts = ReceivedTs::from_micros(UtcMicros::from_micros(AT + 10));
    delayed.mark = Observation::new(clocks, *delayed.mark.value());
    assert_eq!(engine.step(&delayed), Ok(PaperOutcome::PortfolioReconciled));
    assert!(!engine.snapshot().risk.loss_latched);
    // Availability remains the first visibility error.
    let mut unavailable = delayed;
    let mut clocks = unavailable.mark.timestamps();
    clocks.available_ts =
        parangsae_core::AvailableTs::from_micros(UtcMicros::from_micros(AT + 200));
    clocks.received_ts = ReceivedTs::from_micros(UtcMicros::from_micros(AT + 200));
    unavailable.mark = Observation::new(clocks, *unavailable.mark.value());
    assert_eq!(
        engine.step(&unavailable),
        Err(PaperError::Risk(RiskError::FutureMark))
    );
}

#[test]
fn an_exact_retained_receipt_still_observes_new_portfolio_loss() {
    let (initial, commands) = full_fill();
    let mut engine = run(initial, &commands);
    let before = engine.snapshot();
    let same = deliver(7, &fill(4, 1, 400_000));
    assert_eq!(engine.step(&same), Ok(PaperOutcome::DuplicateReceipt));
    assert_eq!(engine.snapshot(), before);
    let mut loss = deliver(8, &fill(4, 1, 400_000));
    loss.mark = observation(price(UNIT), 8);
    assert_eq!(engine.step(&loss), Ok(PaperOutcome::DuplicateReceipt));
    let latched = engine.snapshot();
    assert!(latched.risk.loss_latched);
    assert!(latched.risk.halted);
    assert_eq!(latched.risk.epoch.get(), before.risk.epoch.get() + 1);
    assert_eq!(latched.cash, before.cash);
    assert_eq!(latched.position, before.position);
    assert_eq!(latched.fees_paid, before.fees_paid);
    assert_eq!(latched.orders, before.orders);
    assert_eq!(latched.fills, before.fills);
    assert_eq!(latched.risk.orders, before.risk.orders);
    let mut recovered = deliver(9, &fill(4, 1, 400_000));
    recovered.mark = observation(price(30_000 * UNIT), 9);
    assert_eq!(engine.step(&recovered), Ok(PaperOutcome::DuplicateReceipt));
    assert!(engine.snapshot().risk.loss_latched);
    assert_eq!(engine.snapshot().risk.epoch, latched.risk.epoch);
}

#[test]
fn future_event_marks_cannot_initialize_paper_authority() {
    use parangsae_core::EventTs;
    let mut initial = session();
    let mut clocks = initial.initial_mark.timestamps();
    clocks.event_ts = EventTs::from_micros(UtcMicros::from_micros(AT + 100));
    initial.initial_mark = Observation::new(clocks, *initial.initial_mark.value());
    assert!(matches!(
        PaperEngine::new(initial),
        Err(PaperError::FutureMarkEvent)
    ));
}

#[test]
fn future_event_marks_cannot_revalue_existing_paper_risk() {
    use parangsae_core::EventTs;
    let (initial, commands) = full_fill();
    let mut engine = run(initial, &commands);
    let before = engine.snapshot();
    let mut future = command(7, PaperAction::ReconcilePortfolio);
    let mut clocks = future.mark.timestamps();
    clocks.event_ts = EventTs::from_micros(UtcMicros::from_micros(AT + 100));
    future.mark = Observation::new(clocks, price(UNIT));
    assert_eq!(engine.step(&future), Err(PaperError::FutureMarkEvent));
    let after = engine.snapshot();
    assert_eq!(after.cash, before.cash);
    assert_eq!(after.position, before.position);
    assert_eq!(after.fees_paid, before.fees_paid);
    assert_eq!(after.orders, before.orders);
    assert_eq!(after.fills, before.fills);
    assert_eq!(after.risk.orders, before.risk.orders);
    assert!(!after.risk.loss_latched);
    assert_eq!(after.risk.epoch.get(), before.risk.epoch.get() + 1);
    // These three source times are independently past the decision. Their
    // internal ordering is not a global research or clock-skew invariant.
    let mut arrived = command(8, PaperAction::ReconcilePortfolio);
    let mut clocks = arrived.mark.timestamps();
    clocks.event_ts = EventTs::from_micros(UtcMicros::from_micros(AT + 6));
    clocks.available_ts = parangsae_core::AvailableTs::from_micros(UtcMicros::from_micros(AT + 2));
    clocks.received_ts = parangsae_core::ReceivedTs::from_micros(UtcMicros::from_micros(AT + 8));
    arrived.mark = Observation::new(clocks, *arrived.mark.value());
    assert_eq!(engine.step(&arrived), Ok(PaperOutcome::PortfolioReconciled));
}

#[test]
fn unresolved_expired_receipts_cannot_be_cleared_by_portfolio_resume() {
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
    let late = cut(10, PaperTerminal::Expired, vec![fill(3, 1, 100_000)]);
    assert_eq!(
        engine.step(&late),
        Err(PaperError::ExpiredReceiptRecoveryUnsupported)
    );
    let before = engine.snapshot();
    let reconcile = engine.step(&command(11, PaperAction::ReconcilePortfolio));
    let epoch = engine.snapshot().risk.epoch.get();
    let resume = engine.step(&command(
        12,
        PaperAction::Resume {
            expected_epoch: epoch,
        },
    ));
    let after = engine.snapshot();
    assert_eq!(
        reconcile,
        Err(PaperError::IncompleteCut),
        "reconcile={reconcile:?}; resume={resume:?}; halted={}",
        after.risk.halted
    );
    assert_eq!(
        resume,
        Err(PaperError::Risk(RiskError::ResetRequiresReconciliation))
    );
    assert!(after.risk.halted);
    assert_eq!(after.risk.exposure, ExposureState::Unknown);
    assert_eq!(after.cash, before.cash);
    assert_eq!(after.position, before.position);
    assert_eq!(after.fees_paid, before.fees_paid);
    assert_eq!(after.orders, before.orders);
    assert_eq!(after.fills, before.fills);
    assert_eq!(after.risk.orders, before.risk.orders);
    assert_eq!(after.risk.orders[0].remaining.ticks(), 400_000);
    // A separate source with no missing receipts can complete its Expired cut.
    let mut known = run(session(), &prefix());
    known
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
    known
        .step(&cut(5, PaperTerminal::Expired, Vec::new()))
        .unwrap();
    known
        .step(&command(6, PaperAction::ReconcilePortfolio))
        .unwrap();
    assert!(known.snapshot().risk.orders.is_empty());
}

#[test]
fn existing_plan_and_stale_resume_still_observe_their_independent_mark() {
    let actions = [plan(7, 1).action, PaperAction::Resume { expected_epoch: 0 }];
    for action in actions {
        let (initial, commands) = full_fill();
        let mut engine = run(initial, &commands);
        let before = engine.snapshot();
        let mut input = command(7, action);
        input.mark = observation(price(UNIT), 7);
        let result = engine.step(&input);
        assert!(matches!(
            result,
            Err(PaperError::AlreadyPlanned | PaperError::Risk(RiskError::EpochMismatch))
        ));
        let after = engine.snapshot();
        assert!(after.risk.loss_latched);
        assert!(after.risk.halted);
        assert_eq!(after.risk.epoch.get(), before.risk.epoch.get() + 1);
        assert_eq!(after.cash, before.cash);
        assert_eq!(after.position, before.position);
        assert_eq!(after.fees_paid, before.fees_paid);
        assert_eq!(after.orders, before.orders);
        assert_eq!(after.fills, before.fills);
    }
}

#[test]
fn exact_receipt_observation_latches_a_new_notional_breach() {
    let (initial, commands) = full_fill();
    let mut engine = run(initial, &commands);
    let before = engine.snapshot();
    let mut large = deliver(7, &fill(4, 1, 400_000));
    large.mark = observation(price(3_000_000 * UNIT), 7);
    assert_eq!(engine.step(&large), Ok(PaperOutcome::DuplicateReceipt));
    let latched = engine.snapshot();
    assert!(latched.risk.halted);
    assert!(!latched.risk.loss_latched);
    assert_eq!(latched.risk.epoch.get(), before.risk.epoch.get() + 1);
    assert_eq!(latched.cash, before.cash);
    assert_eq!(latched.position, before.position);
    assert_eq!(latched.fees_paid, before.fees_paid);
    assert_eq!(latched.fills, before.fills);
    let mut recovered = deliver(8, &fill(4, 1, 400_000));
    recovered.mark = observation(price(30_000 * UNIT), 8);
    assert_eq!(engine.step(&recovered), Ok(PaperOutcome::DuplicateReceipt));
    assert!(engine.snapshot().risk.halted);
    assert_eq!(engine.snapshot().risk.epoch, latched.risk.epoch);
}

#[test]
fn cancel_denied_without_ack_commits_unknown_risk_authority() {
    use parangsae_risk::ReservationPhase;
    let (initial, commands) = cancel_denied_unknown();
    let mut engine = run(initial, &commands[..3]);
    let before = engine.snapshot();
    engine.step(&commands[3]).unwrap();
    let unknown = engine.snapshot();
    let second_plan = engine.step(&plan(5, 2));
    let second_dispatch = if second_plan.is_ok() {
        Some(engine.step(&command(
            6,
            PaperAction::Dispatch {
                client_order_id: id(2),
            },
        )))
    } else {
        None
    };
    assert_eq!(unknown.orders[0].progress.state(), OrderState::Unknown);
    assert_eq!(
        unknown.risk.exposure,
        ExposureState::Unknown,
        "Risk={:?}; second_plan={second_plan:?}; second_dispatch={second_dispatch:?}",
        unknown.risk
    );
    assert!(unknown.risk.halted);
    assert_eq!(unknown.risk.epoch.get(), before.risk.epoch.get() + 1);
    assert_eq!(unknown.risk.orders[0].phase, ReservationPhase::Unknown);
    assert_eq!(unknown.risk.orders[0].remaining.ticks(), 400_000);
    assert_eq!(unknown.cash, before.cash);
    assert_eq!(unknown.position, before.position);
    assert_eq!(unknown.fees_paid, before.fees_paid);
    assert_eq!(unknown.fills, before.fills);
    assert_eq!(
        second_plan,
        Err(PaperError::Risk(RiskError::ExposureUnknown))
    );
    assert!(second_dispatch.is_none());
}

#[test]
fn cancel_denied_invalidates_old_approval_then_requires_complete_recovery() {
    use parangsae_execution::PaperReport;
    let mut engine = run(session(), &prefix().into_iter().take(2).collect::<Vec<_>>());
    engine.step(&plan(3, 2)).unwrap();
    engine
        .step(&command(
            4,
            PaperAction::Cancel {
                client_order_id: id(1),
            },
        ))
        .unwrap();
    engine
        .step(&command(
            5,
            PaperAction::Deliver(observation(
                PaperReport::CancelDenied {
                    client_order_id: id(1),
                },
                5,
            )),
        ))
        .unwrap();
    let unknown = engine.snapshot();
    let old = engine
        .step(&command(
            6,
            PaperAction::Dispatch {
                client_order_id: id(2),
            },
        ))
        .unwrap();
    assert!(
        matches!(
            old,
            PaperOutcome::Refused {
                reason: RiskError::StaleApproval,
                ..
            }
        ),
        "old approval outcome={old:?}"
    );
    assert_eq!(
        engine.step(&plan(7, 3)),
        Err(PaperError::Risk(RiskError::ExposureUnknown))
    );
    assert!(engine.snapshot().risk.halted);
    assert_eq!(unknown.risk.epoch.get(), 1);
    engine
        .step(&cut(8, PaperTerminal::Filled, vec![fill(7, 1, 400_000)]))
        .unwrap();
    assert!(engine.snapshot().risk.halted);
    assert_eq!(
        engine.step(&command(9, PaperAction::Resume { expected_epoch: 1 })),
        Err(PaperError::Risk(RiskError::ResetRequiresReconciliation))
    );
    engine
        .step(&command(10, PaperAction::ReconcilePortfolio))
        .unwrap();
    engine
        .step(&command(11, PaperAction::Resume { expected_epoch: 1 }))
        .unwrap();
    assert!(!engine.snapshot().risk.halted);
    assert_eq!(engine.snapshot().position.0, 400_000);
    // A refused request identity remains a tombstone after recovery.
    assert_eq!(
        engine.step(&plan(12, 3)),
        Err(PaperError::Risk(RiskError::TerminalOrder))
    );
    engine.step(&plan(13, 4)).unwrap();
    assert!(matches!(
        engine.step(&command(
            14,
            PaperAction::Dispatch {
                client_order_id: id(4)
            }
        )),
        Ok(PaperOutcome::Dispatched(_))
    ));
}

#[test]
fn cancel_denied_after_ack_or_partial_fill_preserves_known_authority() {
    use parangsae_execution::PaperReport;
    use parangsae_risk::ReservationPhase;
    for partial_without_ack in [false, true] {
        let mut engine = if partial_without_ack {
            let mut engine = run(session(), &prefix().into_iter().take(2).collect::<Vec<_>>());
            engine.step(&deliver(3, &fill(3, 1, 100_000))).unwrap();
            engine
        } else {
            run(session(), &prefix())
        };
        let before = engine.snapshot();
        engine
            .step(&command(
                4,
                PaperAction::Cancel {
                    client_order_id: id(1),
                },
            ))
            .unwrap();
        engine
            .step(&command(
                5,
                PaperAction::Deliver(observation(
                    PaperReport::CancelDenied {
                        client_order_id: id(1),
                    },
                    5,
                )),
            ))
            .unwrap();
        let after = engine.snapshot();
        let state = if partial_without_ack {
            OrderState::PartiallyFilled
        } else {
            OrderState::Acked
        };
        assert_eq!(after.orders[0].progress.state(), state);
        assert_eq!(after.risk.exposure, ExposureState::Known);
        assert!(!after.risk.halted);
        assert_eq!(after.risk.epoch, before.risk.epoch);
        assert_eq!(after.risk.orders, before.risk.orders);
        assert_eq!(after.risk.orders[0].phase, ReservationPhase::Submitted);
        assert_eq!(after.cash, before.cash);
        assert_eq!(after.position, before.position);
        engine.step(&plan(6, 2)).unwrap();
        assert!(matches!(
            engine.step(&command(
                7,
                PaperAction::Dispatch {
                    client_order_id: id(2)
                }
            )),
            Ok(PaperOutcome::Dispatched(_))
        ));
    }
}
