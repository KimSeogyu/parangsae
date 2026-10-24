#[path = "support/carry_cases.rs"]
mod cases;

use cases::*;
use parangsae_core::{DecisionTime, Observation, Ratio, ReceivedTs};
use parangsae_execution::carry::*;
use parangsae_execution::HaltCause;
use parangsae_sim::carry::{Allocation, Market};
use parangsae_sim::{Cash, FeeBps};

#[test]
fn no_hedge_before_visible_fee_net_spot_and_a_new_decision() {
    let mut h = Harness::new();
    h.act(CarryAction::Plan(plan())).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .unwrap();
    assert_eq!(
        h.act(CarryAction::PlanHedge { pair: 1 }),
        Err(CarryError::Phase)
    );
    let before = h.engine.snapshot().account;
    let fill = CarryFill {
        order: id(CarryStage::SpotEntry),
        fill_id: 1,
        market: Market::Spot,
        qty: qty(U),
        reference_price: price(1_000 * U),
    };
    assert_eq!(
        h.act(CarryAction::Deliver(observed(
            CarryReport::Fill(fill),
            3,
            100
        ))),
        Err(CarryError::FutureObservation)
    );
    assert_eq!(h.engine.snapshot().account, before);
    let mut valid = Harness::new();
    valid.spot();
    assert_eq!(valid.engine.snapshot().account.spot_btc, qty(99_900_000));
    assert!(matches!(
        valid.act(CarryAction::PlanHedge { pair: 1 }),
        Ok(CarryOutcome::Planned(_))
    ));
    valid
        .act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
        .unwrap();
}

#[test]
fn tiny_isolated_wallet_cannot_approve_a_fee_affordable_short() {
    let mut s = session();
    s.allocation.perp_collateral = Cash(U);
    s.allocation.idle = Cash(5_999 * U);
    let mut h = Harness::with_session(s);
    assert_eq!(
        h.act(CarryAction::Plan(plan())),
        Err(CarryError::CapitalEnvelope)
    );
    assert_eq!(h.engine.snapshot().account.perp_wallet, Cash(U));
    assert!(h.engine.snapshot().orders.is_empty());
}

#[test]
fn complete_four_fill_cycle_counts_each_wallet_once() {
    let mut h = Harness::new();
    h.hedged();
    assert_eq!(
        h.engine.snapshot().account.perp_wallet,
        Cash(499_950_050_000)
    );
    h.close();
    let state = h.engine.snapshot();
    assert_eq!(state.phase, CarryPhase::Closed);
    assert_eq!(state.account.spot_btc, qty(0));
    assert_eq!(state.account.perp_short, qty(0));
    assert_eq!(state.account.spot_quote, Cash(399_800_100_000));
    assert_eq!(state.account.perp_wallet, Cash(499_900_100_000));
    assert_eq!(
        state
            .account
            .equity(price(1_000 * U), price(1_000 * U))
            .unwrap(),
        Cash(999_700_200_000)
    );
}

#[test]
fn late_funding_uses_retained_boundary_quantity_after_the_position_closes() {
    let mut h = Harness::new();
    h.hedged();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let before_receipt = h.engine.snapshot().account.perp_wallet;
    h.close();
    assert_eq!(h.engine.snapshot().account.perp_short, qty(0));
    assert_ne!(h.engine.snapshot().phase, CarryPhase::Closed);
    let source = observed(
        CarryFundingReceipt {
            key: 1,
            effective,
            rate: Ratio::from_ticks(10_000),
            charge_mark: price(1_000 * U),
        },
        u64::try_from(effective.micros() - AT).unwrap(),
        h.now + 1,
    );
    let action = CarryAction::FundingReceipt(source.clone());
    assert_eq!(
        h.act(action.clone()),
        Ok(CarryOutcome::FundingApplied {
            key: 1,
            amount: Cash(9_990_000)
        })
    );
    let after = h.engine.snapshot();
    assert_eq!(after.funding[0].captured_short, qty(99_900_000));
    assert_eq!(after.account.funding_net, Cash(9_990_000));
    assert!(after.account.perp_wallet.0 < before_receipt.0);
    assert_eq!(after.phase, CarryPhase::Closed);
    assert_eq!(h.act(action), Ok(CarryOutcome::Duplicate));
    assert_eq!(h.engine.snapshot().account, after.account);
}

#[test]
fn dispatched_out_of_envelope_fact_is_unresolved_and_retained() {
    let mut h = Harness::new();
    h.act(CarryAction::Plan(plan())).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .unwrap();
    let before = h.engine.snapshot().account;
    let at = h.now + 1;
    let fill = CarryFill {
        order: id(CarryStage::SpotEntry),
        fill_id: 1,
        market: Market::Spot,
        qty: qty(U),
        reference_price: price(1_300 * U),
    };
    assert_eq!(
        h.act(CarryAction::Deliver(observed(
            CarryReport::Fill(fill),
            at,
            at
        ))),
        Err(CarryError::PriceEnvelope)
    );
    let after = h.engine.snapshot();
    assert_eq!(after.account, before);
    assert_eq!(after.phase, CarryPhase::Unresolved);
    assert!(after.unsupported.is_some());
    assert!(after.halted);
    assert!(after.spot_quote_reserved.0 > 0);
    assert!(after.perp_collateral_reserved.0 > 0);
}

#[test]
fn positive_pair_equity_cannot_top_up_a_breached_isolated_wallet() {
    let mut h = Harness::new();
    h.hedged();
    let receipt = h.receipts[1].clone();
    let before = h.engine.snapshot().account;
    h.marked(
        CarryAction::Deliver(receipt.with_value(CarryReport::Fill(receipt.value().clone()))),
        10_000 * U,
        10_000 * U,
    )
    .unwrap();
    let after = h.engine.snapshot();
    assert!(
        after
            .account
            .equity(price(10_000 * U), price(10_000 * U))
            .unwrap()
            .0
            > 0
    );
    assert!(after.margin_latched && after.halted);
    assert!(!after.loss_latched);
    assert_eq!(after.account, before);
}

#[test]
fn a_price_drift_refusal_consumes_the_old_dispatch_authority() {
    let mut h = Harness::new();
    h.act(CarryAction::Plan(plan())).unwrap();
    assert_eq!(
        h.marked(
            CarryAction::Dispatch(id(CarryStage::SpotEntry)),
            1_300 * U,
            1_000 * U
        ),
        Err(CarryError::PriceEnvelope)
    );
    assert!(h
        .act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .is_err());
    assert!(!h.engine.snapshot().orders[0].progress.sent());
}

#[test]
fn a_canceled_empty_short_cut_allows_known_spot_unwind_after_explicit_recovery() {
    let mut h = Harness::new();
    h.spot();
    h.act(CarryAction::PlanHedge { pair: 1 }).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
        .unwrap();
    h.act(CarryAction::Cancel(id(CarryStage::PerpEntry)))
        .unwrap();
    let at = h.now + 1;
    h.act(CarryAction::Deliver(observed(
        CarryReport::CancelDenied(id(CarryStage::PerpEntry)),
        at,
        at,
    )))
    .unwrap();
    let unknown = h.engine.snapshot();
    assert!(unknown.halted);
    assert!(unknown.perp_collateral_reserved.0 > 0);
    assert!(h
        .act(CarryAction::PlanSpotExit {
            pair: 1,
            max_spot_price: price(1_200 * U)
        })
        .is_err());
    h.cut(CarryStage::PerpEntry, CarryTerminal::Canceled);
    h.act(CarryAction::ReconcilePortfolio(h.engine.snapshot().account))
        .unwrap();
    let epoch = h.engine.snapshot().epoch.get();
    assert_eq!(
        h.act(CarryAction::Resume {
            expected_epoch: epoch - 1
        }),
        Err(CarryError::EpochMismatch)
    );
    h.act(CarryAction::Resume {
        expected_epoch: epoch,
    })
    .unwrap();
    h.act(CarryAction::BeginExit {
        pair: 1,
        max_perp_price: price(1_200 * U),
    })
    .unwrap();
    h.act(CarryAction::PlanSpotExit {
        pair: 1,
        max_spot_price: price(1_200 * U),
    })
    .unwrap();
}

#[test]
fn unique_funding_boundary_over_capacity_is_a_retained_refusal() {
    let mut s = session();
    s.limits.max_funding_events = 1;
    let mut h = Harness::with_session(s);
    h.hedged();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let next = time(h.now + 1);
    assert_eq!(
        h.act(CarryAction::FundingBoundary {
            key: 2,
            effective: next
        }),
        Err(CarryError::Capacity)
    );
    assert_eq!(h.engine.snapshot().phase, CarryPhase::Unresolved);
    assert!(h.engine.snapshot().unsupported.is_some());
}

#[test]
fn a_later_backdated_unique_fill_cannot_rewrite_captured_funding_quantity() {
    let mut h = Harness::new();
    h.hedged();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let before = h.engine.snapshot();
    let at = u64::try_from(effective.micros() - AT).unwrap();
    let fill = CarryFill {
        order: id(CarryStage::PerpEntry),
        fill_id: 999,
        market: Market::UsdMPerp,
        qty: qty(1),
        reference_price: price(1_000 * U),
    };
    assert_eq!(
        h.act(CarryAction::Deliver(observed(
            CarryReport::Fill(fill),
            at,
            h.now + 1
        ))),
        Err(CarryError::InvalidEvidence)
    );
    let after = h.engine.snapshot();
    assert_eq!(
        after.funding[0].captured_short,
        before.funding[0].captured_short
    );
    assert_eq!(after.account, before.account);
    assert_eq!(after.phase, CarryPhase::Unresolved);
}

#[test]
fn a_halt_does_not_turn_another_engines_order_record_into_authority() {
    let mut first = Harness::new();
    first.act(CarryAction::Plan(plan())).unwrap();
    let mut second = Harness::new();
    assert_eq!(
        second.act(CarryAction::Dispatch(id(CarryStage::SpotEntry))),
        Err(CarryError::UnknownOrder)
    );
    first.act(CarryAction::Halt(HaltCause::Kill)).unwrap();
    assert_eq!(
        first.act(CarryAction::Dispatch(id(CarryStage::SpotEntry))),
        Err(CarryError::Halted)
    );
}

#[test]
fn candidate_valuation_failure_retains_the_dispatched_fact_without_half_commit() {
    let mut s = session();
    let capital = i64::MAX - 1_000;
    s.allocation = Allocation {
        total: Cash(capital),
        spot_quote: Cash(capital / 2),
        perp_collateral: Cash(capital - capital / 2),
        idle: Cash(0),
    };
    s.fees.spot_bps = FeeBps(0);
    s.fees.perp_bps = FeeBps(0);
    s.limits.max_unhedged_notional = Cash(i64::MAX);
    let mut h = Harness::with_session(s);
    let mut intent = plan();
    intent.net_btc = qty(U);
    h.act(CarryAction::Plan(intent)).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .unwrap();
    let before = h.engine.snapshot();
    let at = h.now + 1;
    let fill = CarryFill {
        order: id(CarryStage::SpotEntry),
        fill_id: 1,
        market: Market::Spot,
        qty: qty(U),
        reference_price: price(1_000 * U),
    };
    assert!(h
        .marked(
            CarryAction::Deliver(observed(CarryReport::Fill(fill), at, at)),
            i64::MAX,
            1_000 * U
        )
        .is_err());
    let after = h.engine.snapshot();
    assert_eq!(after.account, before.account);
    assert_eq!(after.fills, before.fills);
    assert_eq!(after.phase, CarryPhase::Unresolved);
    assert!(matches!(
        after.unsupported,
        Some(UnsupportedEvidence::Fill(_))
    ));
}

#[test]
fn expired_one_leg_deadline_keeps_the_owned_inventory_and_unresolved_liability() {
    let mut h = Harness::new();
    h.spot();
    let before = h.engine.snapshot().account;
    h.now = 10_004;
    assert_eq!(
        h.act(CarryAction::NoExecutionEvidence {
            order: id(CarryStage::SpotEntry),
            kind: MissingExecution::ZeroCount
        }),
        Err(CarryError::UnsupportedFact)
    );
    let after = h.engine.snapshot();
    assert_eq!(after.account, before);
    assert_eq!(after.phase, CarryPhase::Unresolved);
    assert!(after.perp_collateral_reserved.0 > 0);
    assert!(after.unsupported.is_some());
}

#[test]
fn one_settlement_cannot_be_rebound_under_another_key() {
    let mut h = Harness::new();
    h.hedged();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let at = u64::try_from(effective.micros() - AT).unwrap();
    let mut command = h.command(
        CarryAction::FundingBoundary { key: 2, effective },
        1_000 * U,
        1_000 * U,
    );
    command.decision = DecisionTime::from_micros(effective);
    command.spot = observed(price(1_000 * U), at, at);
    command.perp = observed(price(1_000 * U), at, at);
    assert_eq!(h.engine.step(&command), Err(CarryError::ReceiptConflict));
    assert_eq!(h.engine.snapshot().funding.len(), 1);
    assert_eq!(h.engine.snapshot().account.funding_net, Cash(0));
    assert_eq!(h.engine.snapshot().phase, CarryPhase::Unresolved);
}

#[test]
fn a_new_dispatch_cannot_fill_on_an_already_captured_settlement_tie() {
    let mut h = Harness::new();
    h.hedged();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let at = u64::try_from(effective.micros() - AT).unwrap();
    for action in [
        CarryAction::BeginExit {
            pair: 1,
            max_perp_price: price(1_200 * U),
        },
        CarryAction::Dispatch(id(CarryStage::PerpExit)),
    ] {
        let mut command = h.command(action, 1_000 * U, 1_000 * U);
        command.decision = DecisionTime::from_micros(effective);
        command.spot = observed(price(1_000 * U), at, at);
        command.perp = observed(price(1_000 * U), at, at);
        h.now += 1;
        h.engine.step(&command).unwrap();
    }
    let before = h.engine.snapshot().account;
    let fill = CarryFill {
        order: id(CarryStage::PerpExit),
        fill_id: 99,
        market: Market::UsdMPerp,
        qty: qty(99_900_000),
        reference_price: price(1_000 * U),
    };
    let source = observed(CarryReport::Fill(fill), at, at);
    assert_eq!(
        h.act(CarryAction::Deliver(source)),
        Err(CarryError::InvalidEvidence)
    );
    assert_eq!(h.engine.snapshot().account, before);
    assert_eq!(h.engine.snapshot().phase, CarryPhase::Unresolved);
}

#[test]
fn distinct_halts_at_equal_monotonic_time_each_invalidate_the_epoch() {
    let mut h = Harness::new();
    let first = h.command(CarryAction::Halt(HaltCause::Kill), 1_000 * U, 1_000 * U);
    h.engine.step(&first).unwrap();
    let epoch = h.engine.snapshot().epoch.get();
    let mut second = first;
    second.action = CarryAction::Halt(HaltCause::Ban);
    h.engine.step(&second).unwrap();
    assert_eq!(h.engine.snapshot().epoch.get(), epoch + 1);
}

#[test]
fn a_price_budget_does_not_prove_minimum_notional_at_the_known_mark() {
    let mut s = session();
    s.spot_grid.min_notional = Cash(1_000 * U);
    s.fees.spot_bps = FeeBps(0);
    let mut h = Harness::with_session(s);
    let mut intent = plan();
    intent.gross_spot = qty(U / 10);
    intent.net_btc = qty(U / 10);
    intent.max_spot_price = price(12_000 * U);
    assert_eq!(
        h.act(CarryAction::Plan(intent)),
        Err(CarryError::GridMismatch)
    );
    assert!(h.engine.snapshot().orders.is_empty());
}

#[test]
fn a_future_effective_rule_refuses_session_creation() {
    let mut future = session();
    future.spot_grid.provenance = RuleProvenance::VerifiedHistorical {
        effective_from: time(1),
        source: "declared-fixture".into(),
    };
    assert!(matches!(
        CarryEngine::new(future),
        Err(CarryError::InvalidSession)
    ));
}

#[test]
fn an_unsupported_sell_asset_refuses_session_creation() {
    let mut unsupported = session();
    unsupported.fees.spot_sell_asset = SpotFeeAsset::Btc;
    assert!(matches!(
        CarryEngine::new(unsupported),
        Err(CarryError::InvalidSession)
    ));
}

#[test]
fn partial_base_fee_dust_is_retained_instead_of_forcing_a_hedge() {
    let mut h = Harness::new();
    h.act(CarryAction::Plan(plan())).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .unwrap();
    h.fill(CarryStage::SpotEntry, 1, 50_100_000);
    h.act(CarryAction::Cancel(id(CarryStage::SpotEntry)))
        .unwrap();
    let at = h.now + 1;
    h.act(CarryAction::Deliver(observed(
        CarryReport::CancelConfirmed(id(CarryStage::SpotEntry)),
        at,
        at,
    )))
    .unwrap();
    h.cut(CarryStage::SpotEntry, CarryTerminal::Canceled);
    assert_eq!(
        h.act(CarryAction::PlanHedge { pair: 1 }),
        Err(CarryError::GridMismatch)
    );
    let state = h.engine.snapshot();
    assert_eq!(state.account.spot_btc, qty(50_049_900));
    assert_eq!(state.account.spot_quote, Cash(3_499 * U));
    assert_eq!(state.account.spot_btc_fees, qty(50_100));
    assert_eq!(state.account.perp_short, qty(0));
    assert_ne!(state.phase, CarryPhase::Hedged);
    assert_ne!(state.phase, CarryPhase::Flat);
}

#[test]
fn a_negative_funding_receipt_during_partial_close_uses_the_pre_s_quantity() {
    let mut h = Harness::new();
    h.hedged();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    h.act(CarryAction::BeginExit {
        pair: 1,
        max_perp_price: price(1_200 * U),
    })
    .unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpExit)))
        .unwrap();
    h.fill(CarryStage::PerpExit, 3, 40_000_000);
    assert_eq!(h.engine.snapshot().account.perp_short, qty(59_900_000));
    let at = u64::try_from(effective.micros() - AT).unwrap();
    let source = observed(
        CarryFundingReceipt {
            key: 1,
            effective,
            rate: Ratio::from_ticks(-500_000),
            charge_mark: price(990 * U),
        },
        at,
        h.now + 1,
    );
    assert_eq!(
        h.act(CarryAction::FundingReceipt(source)),
        Ok(CarryOutcome::FundingApplied {
            key: 1,
            amount: Cash(-494_505_000)
        })
    );
    let state = h.engine.snapshot();
    assert_eq!(state.funding[0].captured_short, qty(99_900_000));
    assert_eq!(state.account.perp_short, qty(59_900_000));
    assert_eq!(state.account.funding_net, Cash(-494_505_000));
}

#[test]
fn an_unfunded_settlement_is_retained_unapplied_and_blocks_recovery() {
    let mut h = Harness::new();
    h.hedged();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let before = h.engine.snapshot().account;
    let at = u64::try_from(effective.micros() - AT).unwrap();
    let source = observed(
        CarryFundingReceipt {
            key: 1,
            effective,
            rate: Ratio::from_ticks(-U),
            charge_mark: price(100_000 * U),
        },
        at,
        h.now + 1,
    );
    assert!(matches!(
        h.act(CarryAction::FundingReceipt(source)),
        Err(CarryError::Accounting(_))
    ));
    let state = h.engine.snapshot();
    assert_eq!(state.account, before);
    assert!(matches!(
        state.unsupported,
        Some(UnsupportedEvidence::Funding(_))
    ));
    assert_eq!(state.phase, CarryPhase::Unresolved);
    assert_eq!(state.funding[0].cashflow, None);
    assert!(h.act(CarryAction::ReconcilePortfolio(before)).is_err());
    assert!(h
        .act(CarryAction::Resume {
            expected_epoch: state.epoch.get()
        })
        .is_err());
}

#[test]
fn exact_economic_replay_is_free_at_capacity_but_changed_funding_halts() {
    let mut s = session();
    s.limits.max_receipts = 2;
    s.limits.max_funding_events = 1;
    let mut h = Harness::with_session(s);
    h.hedged();
    let old = h.receipts[0].clone();
    let before = h.engine.snapshot().account;
    assert_eq!(
        h.act(CarryAction::Deliver(
            old.with_value(CarryReport::Fill(old.value().clone()))
        )),
        Ok(CarryOutcome::Duplicate)
    );
    assert_eq!(h.engine.snapshot().account, before);
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let at = u64::try_from(effective.micros() - AT).unwrap();
    let source = observed(
        CarryFundingReceipt {
            key: 1,
            effective,
            rate: Ratio::from_ticks(10_000),
            charge_mark: price(1_000 * U),
        },
        at,
        h.now + 1,
    );
    h.act(CarryAction::FundingReceipt(source.clone())).unwrap();
    let accounted = h.engine.snapshot().account;
    assert_eq!(
        h.act(CarryAction::FundingReceipt(source.clone())),
        Ok(CarryOutcome::Duplicate)
    );
    let mut changed = source.value().clone();
    changed.rate = Ratio::from_ticks(20_000);
    assert_eq!(
        h.act(CarryAction::FundingReceipt(source.with_value(changed))),
        Err(CarryError::ReceiptConflict)
    );
    assert_eq!(h.engine.snapshot().account, accounted);
    assert!(h.engine.snapshot().halted);
}

#[test]
fn receipt_clocks_are_original_and_future_receipt_marks_cannot_plan() {
    let mut h = Harness::new();
    let before = h.engine.snapshot().account;
    let mut command = h.command(CarryAction::Plan(plan()), 1_000 * U, 1_000 * U);
    let mut clocks = command.spot.timestamps();
    clocks.received_ts = ReceivedTs::from_micros(time(2));
    command.spot = Observation::new(clocks, *command.spot.value());
    assert_eq!(h.engine.step(&command), Err(CarryError::FutureObservation));
    assert_eq!(h.engine.snapshot().account, before);
    assert!(h.engine.snapshot().orders.is_empty());
    let mut valid = Harness::new();
    valid.spot();
    assert_eq!(valid.engine.snapshot().fills[0].source, valid.receipts[0]);
}

#[test]
fn approval_age_equality_is_fresh_and_expiry_does_not_recover_with_price() {
    let mut fresh = Harness::new();
    fresh.act(CarryAction::Plan(plan())).unwrap();
    fresh.now = 1_000;
    fresh
        .act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .unwrap();
    let mut expired = Harness::new();
    expired.act(CarryAction::Plan(plan())).unwrap();
    expired.now = 1_001;
    assert_eq!(
        expired.act(CarryAction::Dispatch(id(CarryStage::SpotEntry))),
        Err(CarryError::ApprovalExpired)
    );
    assert!(expired
        .act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
        .is_err());
    assert!(!expired.engine.snapshot().orders[0].progress.sent());
}

#[test]
fn a_transient_loss_mark_on_an_exact_duplicate_latches_once_and_stays_latched() {
    let mut h = Harness::new();
    h.hedged();
    let receipt = h.receipts[0].clone();
    let action =
        CarryAction::Deliver(receipt.with_value(CarryReport::Fill(receipt.value().clone())));
    let before = h.engine.snapshot();
    h.marked(action.clone(), 500 * U, 1_000 * U).unwrap();
    let loss = h.engine.snapshot();
    assert_eq!(loss.account, before.account);
    assert!(loss.loss_latched);
    assert_eq!(loss.epoch.get(), before.epoch.get() + 1);
    h.act(action).unwrap();
    assert_eq!(h.engine.snapshot().epoch, loss.epoch);
    h.act(CarryAction::ReconcilePortfolio(loss.account))
        .unwrap();
    assert_eq!(
        h.act(CarryAction::Resume {
            expected_epoch: loss.epoch.get()
        }),
        Err(CarryError::LossLatched)
    );
}

#[test]
fn missing_exit_source_and_zero_trades_never_invent_a_flat_position() {
    let mut h = Harness::new();
    h.hedged();
    h.act(CarryAction::BeginExit {
        pair: 1,
        max_perp_price: price(1_200 * U),
    })
    .unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpExit)))
        .unwrap();
    assert_eq!(
        h.act(CarryAction::PlanSpotExit {
            pair: 1,
            max_spot_price: price(1_200 * U)
        }),
        Err(CarryError::Phase)
    );
    h.fill(CarryStage::PerpExit, 3, 99_900_000);
    h.cut(CarryStage::PerpExit, CarryTerminal::Filled);
    h.act(CarryAction::PlanSpotExit {
        pair: 1,
        max_spot_price: price(1_200 * U),
    })
    .unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::SpotExit)))
        .unwrap();
    let before = h.engine.snapshot().account;
    assert_eq!(
        h.act(CarryAction::NoExecutionEvidence {
            order: id(CarryStage::SpotExit),
            kind: MissingExecution::ZeroCount
        }),
        Ok(CarryOutcome::Waiting)
    );
    assert_eq!(h.engine.snapshot().account, before);
    assert!(h
        .act(CarryAction::NoExecutionEvidence {
            order: id(CarryStage::SpotExit),
            kind: MissingExecution::MissingSource
        })
        .is_err());
    assert!(h.act(CarryAction::EndOfInput).is_err());
    let state = h.engine.snapshot();
    assert_eq!(state.account, before);
    assert_eq!(state.account.spot_btc, qty(99_900_000));
    assert_eq!(state.phase, CarryPhase::Unresolved);
}

#[test]
fn a_fresh_engine_reconstructs_unknown_liabilities_from_data_commands_only() {
    let mut h = Harness::new();
    h.spot();
    h.act(CarryAction::PlanHedge { pair: 1 }).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
        .unwrap();
    h.act(CarryAction::Timeout(id(CarryStage::PerpEntry)))
        .unwrap();
    let mut fresh = CarryEngine::new(h.initial.clone()).unwrap();
    for command in &h.commands {
        fresh.step(command).unwrap();
    }
    assert_eq!(fresh.snapshot(), h.engine.snapshot());
    assert!(fresh.snapshot().halted);
    assert!(fresh.snapshot().perp_collateral_reserved.0 > 0);
}

#[test]
fn spot_only_boundary_cannot_earn_from_a_hedge_created_later() {
    let mut h = Harness::new();
    h.spot();
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    h.act(CarryAction::PlanHedge { pair: 1 }).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
        .unwrap();
    h.fill(CarryStage::PerpEntry, 2, 99_900_000);
    h.cut(CarryStage::PerpEntry, CarryTerminal::Filled);
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
    assert_eq!(
        h.act(CarryAction::FundingReceipt(receipt)),
        Ok(CarryOutcome::FundingApplied {
            key: 1,
            amount: Cash(0)
        })
    );
    let state = h.engine.snapshot();
    assert_eq!(state.funding[0].captured_short, qty(0));
    assert_eq!(state.account.perp_short, qty(99_900_000));
    assert_eq!(state.account.funding_net, Cash(0));
}

#[test]
fn cancel_race_is_priced_before_a_complete_partial_short_can_capture_funding() {
    let mut h = Harness::new();
    h.spot();
    h.act(CarryAction::PlanHedge { pair: 1 }).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
        .unwrap();
    h.fill(CarryStage::PerpEntry, 2, 40_000_000);
    h.act(CarryAction::Cancel(id(CarryStage::PerpEntry)))
        .unwrap();
    let at = h.now + 1;
    h.act(CarryAction::Deliver(observed(
        CarryReport::CancelConfirmed(id(CarryStage::PerpEntry)),
        at,
        at,
    )))
    .unwrap();
    let reserved = h.engine.snapshot().perp_collateral_reserved;
    assert!(reserved.0 > 0);
    h.fill(CarryStage::PerpEntry, 3, 20_000_000);
    assert_eq!(h.engine.snapshot().account.perp_short, qty(60_000_000));
    assert_eq!(h.engine.snapshot().account.perp_fees, Cash(30_000_000));
    h.cut(CarryStage::PerpEntry, CarryTerminal::Canceled);
    let effective = time(h.now + 1);
    h.act(CarryAction::FundingBoundary { key: 1, effective })
        .unwrap();
    let at = u64::try_from(effective.micros() - AT).unwrap();
    let receipt = observed(
        CarryFundingReceipt {
            key: 1,
            effective,
            rate: Ratio::from_ticks(-10_000),
            charge_mark: price(1_000 * U),
        },
        at,
        h.now + 1,
    );
    assert_eq!(
        h.act(CarryAction::FundingReceipt(receipt)),
        Ok(CarryOutcome::FundingApplied {
            key: 1,
            amount: Cash(-6_000_000)
        })
    );
    assert_eq!(
        h.engine.snapshot().funding[0].captured_short,
        qty(60_000_000)
    );
}

#[test]
fn complete_unknown_cut_accounts_new_priced_receipts_before_explicit_resume() {
    let mut h = Harness::new();
    h.spot();
    h.act(CarryAction::PlanHedge { pair: 1 }).unwrap();
    h.act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
        .unwrap();
    h.act(CarryAction::Timeout(id(CarryStage::PerpEntry)))
        .unwrap();
    let epoch = h.engine.snapshot().epoch.get();
    let at = h.now + 1;
    let fill = observed(
        CarryFill {
            order: id(CarryStage::PerpEntry),
            fill_id: 2,
            market: Market::UsdMPerp,
            qty: qty(99_900_000),
            reference_price: price(1_000 * U),
        },
        at - 1,
        at,
    );
    let cut = CarryTerminalCut {
        order: id(CarryStage::PerpEntry),
        evidence_id: 99,
        status: CarryTerminal::Filled,
        cumulative: qty(99_900_000),
        receipts: vec![fill],
        complete: true,
    };
    h.act(CarryAction::Reconcile(observed(cut, at, at)))
        .unwrap();
    let known = h.engine.snapshot();
    assert_eq!(known.account.perp_short, qty(99_900_000));
    assert_eq!(known.account.perp_wallet, Cash(499_950_050_000));
    assert_eq!(known.phase, CarryPhase::Hedged);
    assert!(known.halted);
    h.act(CarryAction::ReconcilePortfolio(known.account))
        .unwrap();
    assert_eq!(
        h.act(CarryAction::Resume {
            expected_epoch: epoch - 1
        }),
        Err(CarryError::EpochMismatch)
    );
    h.act(CarryAction::Resume {
        expected_epoch: epoch,
    })
    .unwrap();
    assert!(!h.engine.snapshot().halted);
}

#[test]
fn acknowledged_and_priced_partial_cancel_denials_do_not_create_unknown() {
    for partial in [false, true] {
        let mut h = Harness::new();
        h.spot();
        h.act(CarryAction::PlanHedge { pair: 1 }).unwrap();
        h.act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
            .unwrap();
        if partial {
            h.fill(CarryStage::PerpEntry, 2, 40_000_000);
        } else {
            let at = h.now + 1;
            h.act(CarryAction::Deliver(observed(
                CarryReport::Ack(id(CarryStage::PerpEntry)),
                at,
                at,
            )))
            .unwrap();
        }
        h.act(CarryAction::Cancel(id(CarryStage::PerpEntry)))
            .unwrap();
        let at = h.now + 1;
        h.act(CarryAction::Deliver(observed(
            CarryReport::CancelDenied(id(CarryStage::PerpEntry)),
            at,
            at,
        )))
        .unwrap();
        let state = h.engine.snapshot();
        let short = state
            .orders
            .iter()
            .find(|o| o.id == id(CarryStage::PerpEntry))
            .unwrap();
        assert_eq!(
            short.progress.state(),
            if partial {
                parangsae_execution::OrderState::PartiallyFilled
            } else {
                parangsae_execution::OrderState::Acked
            }
        );
        assert!(!state.halted);
        assert!(state.perp_collateral_reserved.0 > 0);
    }
}
