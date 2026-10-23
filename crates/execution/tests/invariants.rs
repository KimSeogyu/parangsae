mod support;

use parangsae_core::Qty;
use parangsae_execution::{
    transition, Attempt, BookError, Checkpoint, ClientOrderId, HaltCause, IntentId, MaxAttempts,
    OrderBook, OrderProgress, OrderState, ProgressError, TransitionError, ValidatedEvent,
};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use support::{assert_every_cell_matches, load_fixture, proptest_cases, MemSink, World};

fn prop_config() -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(proptest_cases());
    config.failure_persistence = Some(Box::new(FileFailurePersistence::Direct(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/proptest-regressions/invariants.txt"
    ))));
    config
}

#[test]
fn type_ok() {
    println!("type_ok proptest cases: {}", proptest_cases());
    for budget in 1..=16 {
        let max = MaxAttempts::new(budget).expect("budget");
        let mut attempt = Attempt::FIRST;
        let mut seen = 1u32;
        assert!(attempt.index() < max.get());
        while let Some(next) = attempt.next(max) {
            assert!(next.index() < max.get());
            assert_eq!(next.index(), attempt.index() + 1);
            attempt = next;
            seen += 1;
            assert!(seen <= u32::from(max.get()));
        }
        assert_eq!(seen, u32::from(max.get()));
        assert_eq!(attempt.index(), max.get() - 1);
    }

    let wide = MaxAttempts::new(16).expect("wide");
    let narrow = MaxAttempts::new(2).expect("narrow");
    let rejected = OrderProgress::from_checkpoint(Checkpoint {
        state: OrderState::PendingNew,
        qty: Qty::from_ticks(2).expect("qty"),
        cum: Qty::from_ticks(0).expect("cum"),
        attempt: wide.last_attempt(),
        max_attempts: narrow,
        sent: false,
        acked: false,
        last_sent_at: None,
        fill_ids: Vec::new(),
    });
    assert_eq!(rejected, Err(ProgressError::AttemptOutOfRange));
}

proptest! {
    #![proptest_config(prop_config())]
    #[test]
    fn type_ok_sampled_fills(
        qty_ticks in 1i64..=32,
        fills in proptest::collection::vec(1i64..=8, 0..6usize)
    ) {
        let qty = Qty::from_ticks(qty_ticks).expect("qty");
        let mut progress = OrderProgress::new_pending(qty).expect("pending");
        let world = World::qty2();
        assert_domain(&progress, world.max_attempts);
        for (index, fill) in fills.iter().copied().enumerate() {
            let event = ValidatedEvent::Fill {
                qty: Qty::from_ticks(fill).expect("fill"),
                fill_id: u64::try_from(index).expect("id") + 1,
            };
            match transition(&progress, &event, &world.context()) {
                Ok(next) => {
                    assert_domain(&next, world.max_attempts);
                    progress = next;
                }
                Err(
                    TransitionError::Overfill
                    | TransitionError::InvalidQuantity
                    | TransitionError::IllegalTransition { .. },
                ) => break,
                Err(other) => panic!("unexpected {other}"),
            }
        }
    }
}

fn assert_domain(progress: &OrderProgress, max: MaxAttempts) {
    assert!(progress.cum().ticks() <= progress.qty().ticks());
    assert!(progress.attempt().index() < max.get());
    match progress.state() {
        OrderState::Canceled | OrderState::Expired => {
            let remaining = progress.cancelled_remaining().expect("remaining");
            assert!(remaining.ticks() <= progress.qty().ticks());
            assert_eq!(progress.frozen_cum(), Some(progress.cum()));
        }
        OrderState::Filled | OrderState::Rejected => {
            assert_eq!(progress.cancelled_remaining(), None);
            assert_eq!(progress.frozen_cum(), Some(progress.cum()));
        }
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel
        | OrderState::Unknown => {
            assert_eq!(progress.cancelled_remaining(), None);
            assert_eq!(progress.frozen_cum(), None);
        }
    }
}

#[test]
fn filled_qty_never_exceeds_order_qty() {
    let world = World::qty2();
    let progress = world.order(
        OrderState::PendingNew,
        1,
        true,
        false,
        0,
        world.authority.bound_us(),
    );
    let before = progress.clone();
    let too_big = ValidatedEvent::Fill {
        qty: Qty::from_ticks(2).expect("fill"),
        fill_id: 7,
    };
    assert_eq!(
        transition(&progress, &too_big, &world.context()),
        Err(TransitionError::Overfill)
    );
    assert_eq!(progress, before);

    let exact = ValidatedEvent::Fill {
        qty: Qty::from_ticks(1).expect("fill"),
        fill_id: 7,
    };
    let filled = transition(&progress, &exact, &world.context()).expect("exact fill");
    assert_eq!(filled.state(), OrderState::Filled);
    assert!(filled.cum().ticks() <= filled.qty().ticks());

    let huge = Qty::from_ticks(i64::MAX).expect("max qty");
    let near = OrderProgress::from_checkpoint(Checkpoint {
        state: OrderState::Acked,
        qty: huge,
        cum: Qty::from_ticks(i64::MAX - 1).expect("cum"),
        attempt: Attempt::FIRST,
        max_attempts: world.max_attempts,
        sent: true,
        acked: true,
        last_sent_at: Some(world.sent_at(1)),
        fill_ids: Vec::new(),
    })
    .expect("checkpoint");
    let overflow = ValidatedEvent::Fill {
        qty: Qty::from_ticks(2).expect("fill"),
        fill_id: 9,
    };
    assert_eq!(
        transition(&near, &overflow, &world.context()),
        Err(TransitionError::Overfill)
    );

    let mut book = OrderBook::new(progress);
    let mut sink = MemSink::new();
    let error = book
        .apply(&too_big, &world.context(), &mut sink)
        .expect_err("overfill");
    assert!(book.is_halted());
    assert_eq!(sink.causes, vec![HaltCause::PayloadError]);
    assert_eq!(book.progress().cum().ticks(), 1);
    assert!(matches!(
        error,
        BookError::Halted(TransitionError::Overfill)
    ));
}

#[test]
fn duplicate_fill_is_report_conflict() {
    let world = World::qty2();
    let progress = world.order(OrderState::PendingNew, 0, true, false, 0, 1);
    let fill = ValidatedEvent::Fill {
        qty: Qty::from_ticks(1).expect("fill"),
        fill_id: 4,
    };
    let once = transition(&progress, &fill, &world.context()).expect("first fill");
    assert_eq!(
        transition(&once, &fill, &world.context()),
        Err(TransitionError::ReportConflict)
    );
}

#[test]
fn checkpoint_rejects_ghost_acked() {
    let world = World::qty2();
    let ghost = OrderProgress::from_checkpoint(Checkpoint {
        state: OrderState::Acked,
        qty: world.qty,
        cum: Qty::from_ticks(0).expect("cum"),
        attempt: Attempt::FIRST,
        max_attempts: world.max_attempts,
        sent: true,
        acked: false,
        last_sent_at: Some(world.sent_at(1)),
        fill_ids: Vec::new(),
    });
    assert_eq!(ghost, Err(ProgressError::InconsistentState));
}

#[test]
fn restored_fill_id_is_not_replayed() {
    let world = World::qty2();
    let progress = OrderProgress::from_checkpoint(Checkpoint {
        state: OrderState::PartiallyFilled,
        qty: world.qty,
        cum: Qty::from_ticks(1).expect("cum"),
        attempt: Attempt::FIRST,
        max_attempts: world.max_attempts,
        sent: true,
        acked: false,
        last_sent_at: Some(world.sent_at(1)),
        fill_ids: vec![11],
    })
    .expect("checkpoint");
    let replay = ValidatedEvent::Fill {
        qty: Qty::from_ticks(1).expect("fill"),
        fill_id: 11,
    };
    assert_eq!(
        transition(&progress, &replay, &world.context()),
        Err(TransitionError::ReportConflict)
    );
    assert_eq!(progress.cum().ticks(), 1);
}

#[test]
fn query_cum_cannot_decrease() {
    let world = World::qty2();
    let progress = world.order(OrderState::Unknown, 1, true, true, 0, 1);
    let event = ValidatedEvent::QueryExpired {
        cum: Qty::from_ticks(0).expect("reported"),
        evidence: 1,
    };
    assert_eq!(
        transition(&progress, &event, &world.context()),
        Err(TransitionError::InconsistentQuery)
    );
    assert_eq!(progress.cum().ticks(), 1);
    assert_eq!(progress.state(), OrderState::Unknown);
}

#[test]
fn no_ghost_acked() {
    let world = World::qty2();
    let elapsed = world.authority.bound_us();
    let denied = ValidatedEvent::CancelDenied;

    let unacked = world.order(OrderState::PendingCancel, 0, true, false, 0, elapsed);
    let unknown = transition(&unacked, &denied, &world.context()).expect("deny");
    assert_eq!(unknown.state(), OrderState::Unknown);
    assert!(!unknown.acked());

    let acked = world.order(OrderState::PendingCancel, 0, true, true, 0, elapsed);
    let back = transition(&acked, &denied, &world.context()).expect("deny acked");
    assert_eq!(back.state(), OrderState::Acked);
    assert!(back.acked());

    let partial = world.order(OrderState::PendingCancel, 1, true, false, 0, elapsed);
    let stayed = transition(&partial, &denied, &world.context()).expect("partial deny");
    assert_eq!(stayed.state(), OrderState::PartiallyFilled);
    assert!(!stayed.acked());

    let pending = OrderProgress::new_pending(world.qty).expect("pending");
    let acked_new = transition(&pending, &ValidatedEvent::Ack, &world.context()).expect("ack");
    assert_eq!(acked_new.state(), OrderState::Acked);
    assert!(acked_new.acked());
}

#[test]
fn cells_follow_matrix() {
    let fixture = load_fixture();
    let counts = assert_every_cell_matches(&fixture);
    assert_eq!(counts.cells, 117);
    assert_eq!(counts.legal, 30);
    assert_eq!(counts.illegal, 87);
}

proptest! {
    #![proptest_config(prop_config())]
    #[test]
    fn cid_injective(
        left_intent in any::<u128>(),
        right_intent in any::<u128>(),
        left_attempt in 0u8..16u8,
        right_attempt in 0u8..16u8
    ) {
        let max = MaxAttempts::new(16).expect("max");
        let left = ClientOrderId::from_parts(
            IntentId::from_raw(left_intent),
            Attempt::from_index(left_attempt, max).expect("left"),
        );
        let right = ClientOrderId::from_parts(
            IntentId::from_raw(right_intent),
            Attempt::from_index(right_attempt, max).expect("right"),
        );
        if (left_intent, left_attempt) != (right_intent, right_attempt) {
            prop_assert_ne!(left.as_str(), right.as_str());
        } else {
            prop_assert_eq!(left.as_str(), right.as_str());
        }
        let parsed = ClientOrderId::parse(left.as_str()).expect("parse");
        prop_assert_eq!(&parsed, &left);
        prop_assert_eq!(parsed.intent().raw(), left_intent);
        prop_assert_eq!(parsed.attempt().index(), left_attempt);
        prop_assert_eq!(left.as_str().len(), 36);
        prop_assert!(binance_charset(left.as_str()));
    }
}

#[test]
fn attempt_fifteen_id_differs_from_attempt_zero() {
    let max = MaxAttempts::new(16).expect("max");
    let intent = IntentId::from_raw(0);
    let first = ClientOrderId::from_parts(intent, Attempt::FIRST);
    let last = ClientOrderId::from_parts(intent, max.last_attempt());
    assert_eq!(last.attempt().index(), 15);
    assert_ne!(first.as_str(), last.as_str());
}

#[test]
fn client_id_vectors() {
    let max = MaxAttempts::new(16).expect("max");
    let vectors = [
        (0u128, 0u8, "eAAAAAAAAAAAAAAAAAAAAAAAAAA-00000000"),
        (1u128, 1u8, "eAAAAAAAAAAAAAAAAAAAAAAAAAB-00000001"),
        (u128::MAX, 15u8, "eH7777777777777777777777777-0000000f"),
        (
            0x0123_4567_89AB_CDEF_0123_4567_89AB_CDEF,
            1u8,
            "eABENCWPCNLZXXQCI2FM6E2XTPP-00000001",
        ),
    ];
    for (intent, attempt, expected) in vectors {
        assert_eq!(expected.len(), 36, "{expected}");
        let id = ClientOrderId::from_parts(
            IntentId::from_raw(intent),
            Attempt::from_index(attempt, max).expect("attempt"),
        );
        assert_eq!(id.as_str(), expected);
        assert_eq!(ClientOrderId::parse(expected).expect("parse"), id);
        assert!(binance_charset(expected));
    }
}

#[test]
fn not_found_backwards_clock_is_inconsistent_query() {
    let world = World::qty2();
    let mut progress = world.order(OrderState::Unknown, 0, true, false, 0, 1);
    let future = world.now.micros() + 5;
    progress = OrderProgress::from_checkpoint(Checkpoint {
        state: progress.state(),
        qty: progress.qty(),
        cum: progress.cum(),
        attempt: progress.attempt(),
        max_attempts: world.max_attempts,
        sent: true,
        acked: false,
        last_sent_at: Some(parangsae_execution::Mono::from_micros(future)),
        fill_ids: Vec::new(),
    })
    .expect("checkpoint");
    let event = ValidatedEvent::QueryNotFound {
        elapsed_since_last_send: parangsae_execution::Elapsed::from_micros(1),
    };
    assert_eq!(
        transition(&progress, &event, &world.context()),
        Err(TransitionError::InconsistentQuery)
    );
    let mut book = OrderBook::new(progress);
    let mut sink = MemSink::new();
    book.apply(&event, &world.context(), &mut sink)
        .expect_err("backwards");
    assert!(book.is_halted());
    assert_eq!(sink.causes, vec![HaltCause::PayloadError]);
}

fn binance_charset(text: &str) -> bool {
    text.len() <= 36
        && text.bytes().all(|byte| {
            matches!(
                byte,
                b'.' | b'A'..=b'Z' | b':' | b'/' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'
            )
        })
}

#[test]
fn proptest_case_budget_defaults_to_512() {
    let cases = proptest_cases();
    println!("PROPTEST_CASES effective: {cases}");
    if std::env::var("PROPTEST_CASES").is_err() {
        assert_eq!(cases, 512);
    }
}
