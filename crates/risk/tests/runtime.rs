mod support;

use parangsae_core::{Price, Side, UtcMicros};
use parangsae_risk::*;
use support::*;

fn dispatch(
    gate: &mut RiskGate,
    req: OrderRequest,
    mark: &parangsae_core::Observation<Price>,
    position: i64,
) {
    let approval = gate.check(req, &context(mark, position, 0)).unwrap();
    let prepared = gate
        .prepare_dispatch(approval, &context(mark, position, 0))
        .unwrap();
    assert!(matches!(prepared.commit(), DispatchDecision::Authorized(_)));
}

fn receipt(id: OrderId, fill_id: u64, ticks: i64) -> FillReceipt {
    FillReceipt {
        key: FillKey { order: id, fill_id },
        qty: qty(ticks),
        price: Price::from_ticks(MARK).unwrap(),
        fee: amount(5),
        at: UtcMicros::from_micros(AT),
    }
}

#[test]
fn opposing_reservations_cannot_net_away_a_third_order() {
    let mut gate = gate(0);
    let mark = mark(MARK);
    let _buy = gate
        .check(request(1, Side::Buy, UNIT, false), &context(&mark, 0, 0))
        .unwrap();
    let _sell = gate
        .check(request(2, Side::Sell, UNIT, false), &context(&mark, 0, 0))
        .unwrap();
    assert_eq!(
        gate.snapshot().envelope,
        ExposureEnvelope {
            low_ticks: -i128::from(UNIT),
            high_ticks: i128::from(UNIT)
        }
    );
    assert_eq!(
        gate.check(request(3, Side::Buy, 100_000, false), &context(&mark, 0, 0))
            .unwrap_err(),
        RiskError::PositionLimit
    );
}

#[test]
fn another_gate_cannot_accept_a_token_with_the_same_numeric_ids() {
    let mut first = gate(0);
    let mut second = gate(0);
    let mark = mark(MARK);
    let token = first
        .check(request(1, Side::Buy, UNIT, false), &context(&mark, 0, 0))
        .unwrap();
    let _other = second
        .check(request(1, Side::Buy, UNIT, false), &context(&mark, 0, 0))
        .unwrap();
    let before = second.snapshot();
    assert!(matches!(
        second.prepare_dispatch(token, &context(&mark, 0, 0)),
        Err(RiskError::ForeignApproval)
    ));
    assert_eq!(second.snapshot(), before);
}

#[test]
fn each_halt_invalidates_approvals_and_reset_cannot_restore_them() {
    for cause in [
        HaltCause::ReconMismatch,
        HaltCause::UnknownOrder,
        HaltCause::Kill,
        HaltCause::Ban,
        HaltCause::RiskTrip,
        HaltCause::IllegalTransition,
        HaltCause::PayloadError,
    ] {
        let mut gate = gate(0);
        let mark = mark(MARK);
        let token = gate
            .check(request(1, Side::Buy, UNIT, false), &context(&mark, 0, 0))
            .unwrap();
        let old = gate.halt_epoch();
        gate.latch(cause);
        assert!(gate.halt_epoch() > old);
        let cut = Reconciliation {
            position_ticks: 0,
            orders: gate.snapshot().orders,
        };
        gate.reconcile(&cut, &context(&mark, 0, 0)).unwrap();
        gate.resume(gate.halt_epoch(), &context(&mark, 0, 0))
            .unwrap();
        let decision = gate
            .prepare_dispatch(token, &context(&mark, 0, 0))
            .unwrap()
            .commit();
        assert!(matches!(
            decision,
            DispatchDecision::Refused {
                reason: RiskError::StaleApproval,
                ..
            }
        ));
        let epoch = gate.halt_epoch();
        gate.latch(cause);
        assert!(gate.halt_epoch() > epoch);
    }
}

#[test]
fn approval_age_equal_to_the_limit_is_fresh_and_one_microsecond_later_expires() {
    for (now, authorized) in [(10, true), (11, false)] {
        let mut gate = gate(0);
        let mark = mark(MARK);
        let token = gate
            .check(request(1, Side::Buy, UNIT, false), &context(&mark, 0, 0))
            .unwrap();
        let outcome = gate
            .prepare_dispatch(token, &context(&mark, 0, now))
            .unwrap()
            .commit();
        assert_eq!(
            matches!(outcome, DispatchDecision::Authorized(_)),
            authorized
        );
        if !authorized {
            assert!(gate.snapshot().orders.is_empty());
        }
    }
}

#[test]
fn reapproval_replaces_authority_without_duplicating_its_reservation() {
    let mut gate = gate(0);
    let mark = mark(MARK);
    let old = gate
        .check(request(1, Side::Buy, UNIT, false), &context(&mark, 0, 0))
        .unwrap();
    let fresh = gate
        .check(request(1, Side::Buy, UNIT, false), &context(&mark, 0, 1))
        .unwrap();
    assert_eq!(gate.snapshot().envelope.high_ticks, i128::from(UNIT));
    assert!(matches!(
        gate.prepare_dispatch(old, &context(&mark, 0, 1)),
        Err(RiskError::StaleApproval)
    ));
    assert!(matches!(
        gate.prepare_dispatch(fresh, &context(&mark, 0, 1))
            .unwrap()
            .commit(),
        DispatchDecision::Authorized(_)
    ));
}

#[test]
fn late_terminal_mismatch_discards_all_new_partial_fills() {
    let mut gate = gate(0);
    let mark = mark(MARK);
    let req = request(1, Side::Buy, UNIT, false);
    let id = req.id();
    dispatch(&mut gate, req, &mark, 0);
    let fill = receipt(id, 1, UNIT / 2);
    let bad = TerminalEvidence {
        order: id,
        status: TerminalStatus::Canceled,
        cumulative: qty(UNIT),
        receipts: vec![fill.key],
        complete: true,
    };
    assert!(matches!(
        gate.prepare_settlement(
            &[Settlement::Fill(fill), Settlement::Terminal(bad)],
            &context(&mark, 0, 1),
            PortfolioSnapshot {
                position_ticks: UNIT / 2,
                equity_ticks: EQUITY - 5
            }
        ),
        Err(RiskError::IncompleteTerminal)
    ));
    assert_eq!(gate.snapshot().position_ticks, 0);
    assert_eq!(gate.snapshot().orders[0].remaining.ticks(), UNIT);
    assert!(gate.is_halted());
}

#[test]
fn partial_fill_unknown_terminal_drain_and_explicit_resume_preserve_accounting() {
    let mut gate = gate(0);
    let mark = mark(MARK);
    let req = request(1, Side::Buy, UNIT, false);
    let id = req.id();
    dispatch(&mut gate, req, &mark, 0);
    let first = receipt(id, 1, UNIT / 4);
    gate.prepare_settlement(
        &[Settlement::Fill(first.clone()), Settlement::Unknown(id)],
        &context(&mark, 0, 1),
        PortfolioSnapshot {
            position_ticks: UNIT / 4,
            equity_ticks: EQUITY - 5,
        },
    )
    .unwrap()
    .commit();
    assert_eq!(gate.snapshot().exposure, ExposureState::Unknown);
    assert_eq!(gate.snapshot().orders[0].remaining.ticks(), UNIT * 3 / 4);
    assert_eq!(
        gate.check(
            request(2, Side::Sell, UNIT / 4, true),
            &context(&mark, UNIT / 4, 1)
        )
        .unwrap_err(),
        RiskError::ExposureUnknown
    );
    let second = receipt(id, 2, UNIT / 4);
    let cut = TerminalEvidence {
        order: id,
        status: TerminalStatus::Canceled,
        cumulative: qty(UNIT / 2),
        receipts: vec![first.key, second.key],
        complete: true,
    };
    gate.prepare_settlement(
        &[Settlement::Fill(second), Settlement::Terminal(cut)],
        &context(&mark, UNIT / 4, 2),
        PortfolioSnapshot {
            position_ticks: UNIT / 2,
            equity_ticks: EQUITY - 10,
        },
    )
    .unwrap()
    .commit();
    assert_eq!(gate.snapshot().position_ticks, UNIT / 2);
    assert!(gate.snapshot().orders.is_empty());
    assert!(gate.is_halted());
    assert_eq!(
        gate.resume(gate.halt_epoch(), &context(&mark, UNIT / 2, 2)),
        Err(RiskError::ResetRequiresReconciliation)
    );
    gate.reconcile(
        &Reconciliation {
            position_ticks: UNIT / 2,
            orders: vec![],
        },
        &context(&mark, UNIT / 2, 2),
    )
    .unwrap();
    gate.resume(gate.halt_epoch(), &context(&mark, UNIT / 2, 2))
        .unwrap();
    assert!(!gate.is_halted());
}

#[test]
fn unknown_attempt_cannot_be_replaced_by_a_fresh_attempt() {
    let mut gate = gate(0);
    let mark = mark(MARK);
    let req = request(1, Side::Buy, UNIT, false);
    let id = req.id();
    dispatch(&mut gate, req, &mark, 0);
    gate.prepare_settlement(
        &[Settlement::Unknown(id)],
        &context(&mark, 0, 1),
        PortfolioSnapshot {
            position_ticks: 0,
            equity_ticks: EQUITY,
        },
    )
    .unwrap()
    .commit();
    let retry = OrderRequest::new(
        OrderId::new(1, 1).unwrap(),
        symbol(),
        Side::Buy,
        qty(UNIT),
        false,
    )
    .unwrap();
    assert_eq!(
        gate.check(retry, &context(&mark, 0, 1)).unwrap_err(),
        RiskError::InvalidRetry
    );
    assert_eq!(gate.snapshot().orders[0].remaining.ticks(), UNIT);
}

#[test]
fn a_small_notional_remainder_rounds_up_instead_of_disappearing() {
    assert_eq!(
        upper_notional(UNIT as i128 - 1, Price::from_ticks(1).unwrap()),
        Ok(1)
    );
    assert_eq!(
        upper_notional(-(i128::from(UNIT) - 1), Price::from_ticks(1).unwrap()),
        Ok(1)
    );
}

#[test]
fn a_mark_breach_latches_and_session_loss_does_not_clear_after_recovery() {
    let mut gate = gate(UNIT);
    let high = mark(30_000 * UNIT);
    gate.observe(&context(&high, UNIT, 1)).unwrap();
    assert!(gate.is_halted());
    let mark = mark(MARK);
    let mut cx = context(&mark, UNIT, 2);
    cx.equity_ticks = EQUITY - 1_000 * UNIT;
    gate.observe(&cx).unwrap();
    gate.observe(&context(&mark, UNIT, 3)).unwrap();
    assert!(gate.snapshot().loss_latched);
    gate.reconcile(
        &Reconciliation {
            position_ticks: UNIT,
            orders: vec![],
        },
        &context(&mark, UNIT, 3),
    )
    .unwrap();
    assert_eq!(
        gate.resume(gate.halt_epoch(), &context(&mark, UNIT, 3)),
        Err(RiskError::LossLatched)
    );
}

#[test]
fn drop_prepared_dispatch_or_settlement_does_not_change_economic_state() {
    let mut gate = gate(0);
    let mark = mark(MARK);
    let req = request(1, Side::Buy, UNIT, false);
    let id = req.id();
    let token = gate.check(req.clone(), &context(&mark, 0, 0)).unwrap();
    let before = gate.snapshot();
    drop(gate.prepare_dispatch(token, &context(&mark, 0, 0)).unwrap());
    assert_eq!(gate.snapshot(), before);
    dispatch(&mut gate, req, &mark, 0);
    let before = gate.snapshot();
    drop(
        gate.prepare_settlement(
            &[Settlement::Fill(receipt(id, 1, UNIT / 2))],
            &context(&mark, 0, 1),
            PortfolioSnapshot {
                position_ticks: UNIT / 2,
                equity_ticks: EQUITY - 5,
            },
        )
        .unwrap(),
    );
    assert_eq!(gate.snapshot(), before);
}

proptest::proptest! {
    #[test]
    fn every_subset_of_two_opposing_reserved_fills_stays_in_the_envelope(buy in 10i64..=1_000, sell in 10i64..=1_000) {
        let buy = buy * 100_000;
        let sell = sell * 100_000;
        let mut gate = gate(0);
        let mark = mark(MARK);
        let _buy = gate.check(request(1, Side::Buy, buy, false), &context(&mark, 0, 0)).unwrap();
        let _sell = gate.check(request(2, Side::Sell, sell, false), &context(&mark, 0, 0)).unwrap();
        let bound = gate.snapshot().envelope;
        for position in [0, buy, -sell, buy - sell] {
            proptest::prop_assert!(i128::from(position) >= bound.low_ticks && i128::from(position) <= bound.high_ticks);
            proptest::prop_assert!(position.abs() <= UNIT);
        }
    }
}
