//! Finding traces the pure transition can show without a venue.
//!
//! F2's venue submit count, F8's risk-epoch table, and the kill driver for F9
//! need `crates/risk` and MockVenue. Those are not in this crate.

mod support;

use parangsae_core::Qty;
use parangsae_execution::{
    transition, Attempt, ClientOrderId, Elapsed, HaltCause, IntentId, OrderBook, OrderProgress,
    OrderState, ValidatedEvent,
};
use support::{MemSink, World};

#[test]
fn f1_timeout_then_venue_expired_resolves_via_query_expired() {
    let world = World::qty2();
    let pending = world.order(OrderState::PendingNew, 0, true, false, 0, 1);
    let unknown =
        transition(&pending, &ValidatedEvent::Timeout, &world.context()).expect("timeout");
    assert_eq!(unknown.state(), OrderState::Unknown);
    let expired = transition(
        &unknown,
        &ValidatedEvent::QueryExpired {
            cum: Qty::from_ticks(0).expect("cum"),
            evidence: 1,
        },
        &world.context(),
    )
    .expect("query expired");
    assert_eq!(expired.state(), OrderState::Expired);
    assert_eq!(expired.cum().ticks(), 0);
    assert_eq!(expired.frozen_cum().map(Qty::ticks), Some(0));
}

#[test]
fn f3_not_found_before_bound_stays_unknown_and_halted() {
    let world = World::qty2();
    let bound = world.authority.bound_us();
    let pending = world.order(OrderState::PendingNew, 0, true, false, 0, bound);
    let mut book = OrderBook::new(pending);
    let mut sink = MemSink::new();
    book.apply(&ValidatedEvent::Timeout, &world.context(), &mut sink)
        .expect("timeout");
    assert_eq!(book.progress().state(), OrderState::Unknown);
    assert!(book.is_halted());
    assert_eq!(sink.causes, vec![HaltCause::UnknownOrder]);
    let epoch = book.epoch();

    book.apply(
        &ValidatedEvent::QueryNotFound {
            elapsed_since_last_send: Elapsed::from_micros(bound),
        },
        &world.context(),
        &mut sink,
    )
    .expect("early not found");
    assert_eq!(book.progress().state(), OrderState::Unknown);
    assert!(book.is_halted());
    assert_eq!(book.epoch(), epoch);

    let late_pending = world.order(OrderState::Unknown, 0, true, false, 0, bound + 1);
    let mut late_book = OrderBook::new(late_pending);
    let mut late_sink = MemSink::new();
    late_book
        .apply(&ValidatedEvent::Timeout, &world.context(), &mut late_sink)
        .expect("late timeout");
    late_book
        .apply(
            &ValidatedEvent::QueryNotFound {
                elapsed_since_last_send: Elapsed::from_micros(bound + 1),
            },
            &world.context(),
            &mut late_sink,
        )
        .expect("authoritative not found");
    assert_eq!(late_book.progress().state(), OrderState::PendingNew);
    assert_eq!(late_book.progress().attempt().index(), 1);
    assert!(!late_book.progress().sent());
    assert!(!late_book.progress().acked());
    assert_eq!(late_book.progress().last_sent_at(), None);
    assert!(late_book.is_halted());
}

#[test]
fn f5_cancel_denied_without_ack_goes_unknown_and_queries() {
    let world = World::qty2();
    let progress = world.order(OrderState::PendingCancel, 0, true, false, 0, 1);
    let mut book = OrderBook::new(progress);
    let mut sink = MemSink::new();
    book.apply(&ValidatedEvent::CancelDenied, &world.context(), &mut sink)
        .expect("deny");
    assert_eq!(book.progress().state(), OrderState::Unknown);
    assert!(!book.progress().acked());
    assert_eq!(book.progress().cum().ticks(), 0);
    assert!(book.is_halted());
    assert_eq!(sink.causes, vec![HaltCause::UnknownOrder]);
}

#[test]
fn f7_retry_bumps_attempt_then_rejects_when_exhausted() {
    let world = World::qty2();
    let bound = world.authority.bound_us();
    let intent = IntentId::from_raw(0x11);
    let first = world.order(OrderState::Unknown, 0, true, true, 0, bound + 1);
    let retry = transition(
        &first,
        &ValidatedEvent::QueryNotFound {
            elapsed_since_last_send: Elapsed::from_micros(bound + 1),
        },
        &world.context(),
    )
    .expect("retry");
    assert_eq!(retry.state(), OrderState::PendingNew);
    assert_eq!(
        retry.attempt(),
        Attempt::from_index(1, world.max_attempts).expect("attempt 1")
    );
    assert!(!retry.sent());
    assert!(!retry.acked());
    assert_eq!(retry.last_sent_at(), None);
    let original = ClientOrderId::from_parts(intent, Attempt::FIRST);
    let fresh = ClientOrderId::from_parts(intent, retry.attempt());
    assert_ne!(original.as_str(), fresh.as_str());

    let second = world.order(OrderState::Unknown, 0, true, false, 1, bound + 1);
    let rejected = transition(
        &second,
        &ValidatedEvent::QueryNotFound {
            elapsed_since_last_send: Elapsed::from_micros(bound + 1),
        },
        &world.context(),
    )
    .expect("exhausted");
    assert_eq!(rejected.state(), OrderState::Rejected);
    assert_eq!(rejected.attempt().index(), 1);
    assert!(!rejected.sent());
}

#[test]
fn f9_unsent_cancel_is_local_canceled() {
    let world = World::qty2();
    let progress = OrderProgress::new_pending(world.qty).expect("pending");
    let canceled = transition(
        &progress,
        &ValidatedEvent::CancelRequested,
        &world.context(),
    )
    .expect("local cancel");
    assert_eq!(canceled.state(), OrderState::Canceled);
    assert!(!canceled.sent());
    assert_eq!(canceled.cancelled_remaining().map(Qty::ticks), Some(2));
}

#[test]
fn authoritative_not_found_with_fill_is_inconsistent() {
    let world = World::qty2();
    let bound = world.authority.bound_us();
    let progress = world.order(OrderState::Unknown, 1, true, true, 0, bound + 1);
    assert_eq!(
        transition(
            &progress,
            &ValidatedEvent::QueryNotFound {
                elapsed_since_last_send: Elapsed::from_micros(bound + 1),
            },
            &world.context(),
        ),
        Err(parangsae_execution::TransitionError::InconsistentQuery)
    );
}
