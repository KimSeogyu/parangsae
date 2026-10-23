//! Shared fixture, grid, and clock for the execution integration tests.
//!
//! Each integration test binary compiles this module on its own, so a helper
//! used by one binary is unused in the others.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use parangsae_core::Qty;
use parangsae_execution::{
    transition, Attempt, BookError, CellContext, Checkpoint, Elapsed, Event, HaltCause, HaltEpoch,
    HaltSink, MaxAttempts, Mono, NotFoundAuthority, OrderBook, OrderProgress, OrderState,
    TransitionError, ValidatedEvent,
};
use sha2::{Digest, Sha256};

pub const FIXTURE_SHA256: &str = "dc39b071806026f93c1c7f50f7211b80e3be6282ecd458091c7b974de82caeae";

pub fn fixture_path() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../specs/tla/execution/transitions.json"
    ))
}

pub struct CellRow {
    pub state: String,
    pub event: String,
    pub model_allowed: BTreeSet<String>,
}

pub struct Fixture {
    pub sha256: String,
    pub cells: Vec<CellRow>,
}

pub fn load_fixture() -> Fixture {
    let path = fixture_path();
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let sha256 = sha256_hex(&bytes);
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));
    let cells_json = value["cells"]
        .as_array()
        .unwrap_or_else(|| panic!("transitions.json has no cells array"));
    let mut seen = BTreeSet::new();
    let mut cells = Vec::with_capacity(cells_json.len());
    for cell in cells_json {
        let state = cell["state"]
            .as_str()
            .unwrap_or_else(|| panic!("cell missing state"))
            .to_string();
        let event = cell["event"]
            .as_str()
            .unwrap_or_else(|| panic!("cell missing event"))
            .to_string();
        assert!(
            seen.insert((state.clone(), event.clone())),
            "duplicate cell {state} {event}"
        );
        let allowed = cell["model_allowed"]
            .as_array()
            .unwrap_or_else(|| panic!("cell {state} {event} missing model_allowed"));
        let model_allowed = allowed
            .iter()
            .map(|item| {
                item.as_str()
                    .unwrap_or_else(|| panic!("model_allowed entry is not a string"))
                    .to_string()
            })
            .collect();
        cells.push(CellRow {
            state,
            event,
            model_allowed,
        });
    }
    Fixture { sha256, cells }
}

pub struct World {
    pub authority: NotFoundAuthority,
    pub max_attempts: MaxAttempts,
    pub now: Mono,
    pub qty: Qty,
}

impl World {
    pub fn qty2() -> Self {
        Self {
            authority: NotFoundAuthority::new(1, 1).expect("recv window"),
            max_attempts: MaxAttempts::new(2).expect("attempts"),
            now: Mono::from_micros(1_000_000_000),
            qty: Qty::from_ticks(2).expect("qty"),
        }
    }

    pub fn context(&self) -> CellContext<'_> {
        CellContext {
            now: self.now,
            authority: &self.authority,
            max_attempts: self.max_attempts,
        }
    }

    pub fn sent_at(&self, elapsed_us: u64) -> Mono {
        Mono::from_micros(self.now.micros() - elapsed_us)
    }

    pub fn order(
        &self,
        state: OrderState,
        cum: i64,
        sent: bool,
        acked: bool,
        attempt_index: u8,
        elapsed_us: u64,
    ) -> OrderProgress {
        let attempt = Attempt::from_index(attempt_index, self.max_attempts)
            .unwrap_or_else(|| panic!("attempt {attempt_index}"));
        OrderProgress::from_checkpoint(Checkpoint {
            state,
            qty: self.qty,
            cum: Qty::from_ticks(cum).expect("cum"),
            attempt,
            max_attempts: self.max_attempts,
            sent,
            acked: acked_flag(state, acked),
            last_sent_at: Some(self.sent_at(elapsed_us)),
            fill_ids: Vec::new(),
        })
        .expect("checkpoint")
    }
}

pub struct GridPoint {
    pub cum: i64,
    pub acked: bool,
    pub sent: bool,
    pub attempt_index: u8,
    pub elapsed_us: u64,
    pub fill_qty: i64,
    pub query_cum: i64,
}

pub fn grid_points(bound_us: u64) -> Vec<GridPoint> {
    let mut points = Vec::new();
    for cum in [0_i64, 1] {
        for acked in [false, true] {
            for sent in [false, true] {
                for attempt_index in [0_u8, 1] {
                    for elapsed_us in [bound_us, bound_us + 1] {
                        for fill_qty in [1_i64, 2] {
                            for query_cum in [0_i64, 1, 2] {
                                points.push(GridPoint {
                                    cum,
                                    acked,
                                    sent,
                                    attempt_index,
                                    elapsed_us,
                                    fill_qty,
                                    query_cum,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    points
}

pub fn event_at(event: Event, point: &GridPoint) -> ValidatedEvent {
    let fill_qty = Qty::from_ticks(point.fill_qty).expect("fill qty");
    let query_cum = Qty::from_ticks(point.query_cum).expect("query cum");
    match event {
        Event::Ack => ValidatedEvent::Ack,
        Event::Fill => ValidatedEvent::Fill {
            qty: fill_qty,
            fill_id: 1,
        },
        Event::CancelRequested => ValidatedEvent::CancelRequested,
        Event::CancelDenied => ValidatedEvent::CancelDenied,
        Event::CancelConfirmed => ValidatedEvent::CancelConfirmed,
        Event::Reject => ValidatedEvent::Reject,
        Event::Expire => ValidatedEvent::Expire,
        Event::Timeout => ValidatedEvent::Timeout,
        Event::QueryFilled => ValidatedEvent::QueryFilled {
            cum: query_cum,
            evidence: 1,
        },
        Event::QueryCanceled => ValidatedEvent::QueryCanceled {
            cum: query_cum,
            evidence: 1,
        },
        Event::QueryExpired => ValidatedEvent::QueryExpired {
            cum: query_cum,
            evidence: 1,
        },
        Event::QueryNotFound => ValidatedEvent::QueryNotFound {
            elapsed_since_last_send: Elapsed::from_micros(point.elapsed_us),
        },
        Event::RaceFill => ValidatedEvent::RaceFill {
            qty: fill_qty,
            fill_id: 1,
        },
    }
}

pub fn state_by_name(name: &str) -> OrderState {
    each_order_state()
        .into_iter()
        .find(|state| state.name() == name)
        .unwrap_or_else(|| panic!("unknown state {name}"))
}

pub fn event_by_name(name: &str) -> Event {
    each_event()
        .into_iter()
        .find(|event| event.name() == name)
        .unwrap_or_else(|| panic!("unknown event {name}"))
}

/// Every `OrderState`. The match is exhaustive, so a new variant fails to compile.
pub fn each_order_state() -> [OrderState; 9] {
    let states = [
        OrderState::PendingNew,
        OrderState::Acked,
        OrderState::PartiallyFilled,
        OrderState::Filled,
        OrderState::PendingCancel,
        OrderState::Canceled,
        OrderState::Rejected,
        OrderState::Expired,
        OrderState::Unknown,
    ];
    for state in states {
        let _ = match state {
            OrderState::PendingNew => 0,
            OrderState::Acked => 1,
            OrderState::PartiallyFilled => 2,
            OrderState::Filled => 3,
            OrderState::PendingCancel => 4,
            OrderState::Canceled => 5,
            OrderState::Rejected => 6,
            OrderState::Expired => 7,
            OrderState::Unknown => 8,
        };
    }
    states
}

/// Every `Event`. The match is exhaustive, so a new variant fails to compile.
pub fn each_event() -> [Event; 13] {
    let events = [
        Event::Ack,
        Event::Fill,
        Event::CancelRequested,
        Event::CancelDenied,
        Event::CancelConfirmed,
        Event::Reject,
        Event::Expire,
        Event::Timeout,
        Event::QueryFilled,
        Event::QueryCanceled,
        Event::QueryExpired,
        Event::QueryNotFound,
        Event::RaceFill,
    ];
    for event in events {
        let _ = match event {
            Event::Ack => 0,
            Event::Fill => 1,
            Event::CancelRequested => 2,
            Event::CancelDenied => 3,
            Event::CancelConfirmed => 4,
            Event::Reject => 5,
            Event::Expire => 6,
            Event::Timeout => 7,
            Event::QueryFilled => 8,
            Event::QueryCanceled => 9,
            Event::QueryExpired => 10,
            Event::QueryNotFound => 11,
            Event::RaceFill => 12,
        };
    }
    events
}

/// `Acked` without the ack flag is not a checkpoint. Other states keep the flag.
fn acked_flag(state: OrderState, acked: bool) -> bool {
    match state {
        OrderState::Acked => true,
        OrderState::PendingNew
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => acked,
    }
}

pub struct ReplayCounts {
    pub cells: usize,
    pub legal: usize,
    pub illegal: usize,
}

pub fn assert_every_cell_matches(fixture: &Fixture) -> ReplayCounts {
    let world = World::qty2();
    let points = grid_points(world.authority.bound_us());
    let mut legal = 0;
    let mut illegal = 0;
    for cell in &fixture.cells {
        let state = state_by_name(&cell.state);
        let event = event_by_name(&cell.event);
        let mut got = BTreeSet::new();
        for point in &points {
            let progress = world.order(
                state,
                point.cum,
                point.sent,
                point.acked,
                point.attempt_index,
                point.elapsed_us,
            );
            let cx = CellContext {
                now: world.now,
                authority: &world.authority,
                max_attempts: world.max_attempts,
            };
            match transition(&progress, &event_at(event, point), &cx) {
                Ok(next) => {
                    assert!(
                        !cell.model_allowed.is_empty(),
                        "illegal cell {} {} returned {}",
                        cell.state,
                        cell.event,
                        next.state().name()
                    );
                    got.insert(next.state().name().to_string());
                }
                Err(TransitionError::IllegalTransition {
                    from,
                    event: got_event,
                }) => {
                    assert!(
                        cell.model_allowed.is_empty(),
                        "legal cell {} {} returned IllegalTransition",
                        cell.state,
                        cell.event
                    );
                    assert_eq!(from, state);
                    assert_eq!(got_event, event);
                }
                Err(other) => {
                    assert!(
                        !cell.model_allowed.is_empty(),
                        "illegal cell {} {} returned {other}",
                        cell.state,
                        cell.event
                    );
                }
            }
        }
        if cell.model_allowed.is_empty() {
            illegal += 1;
        } else {
            legal += 1;
            assert_eq!(
                got, cell.model_allowed,
                "cell {} {} ok-set",
                cell.state, cell.event
            );
        }
    }
    ReplayCounts {
        cells: fixture.cells.len(),
        legal,
        illegal,
    }
}

pub fn assert_illegal_cells_halt(fixture: &Fixture) {
    let world = World::qty2();
    let point = GridPoint {
        cum: 0,
        acked: false,
        sent: false,
        attempt_index: 0,
        elapsed_us: world.authority.bound_us(),
        fill_qty: 1,
        query_cum: 0,
    };
    let mut illegal = 0;
    for cell in &fixture.cells {
        if !cell.model_allowed.is_empty() {
            continue;
        }
        illegal += 1;
        let state = state_by_name(&cell.state);
        let event = event_by_name(&cell.event);
        let progress = world.order(
            state,
            point.cum,
            point.sent,
            point.acked,
            point.attempt_index,
            point.elapsed_us,
        );
        let before = progress.clone();
        let mut book = OrderBook::new(progress);
        let mut sink = MemSink::new();
        let error = book
            .apply(&event_at(event, &point), &world.context(), &mut sink)
            .expect_err("illegal cell");
        assert_eq!(book.progress(), &before, "{} {}", cell.state, cell.event);
        assert!(book.is_halted(), "{} {}", cell.state, cell.event);
        assert_eq!(sink.causes, vec![HaltCause::IllegalTransition]);
        assert_eq!(book.epoch().get(), 1);
        assert_eq!(book.alerts().len(), 1);
        assert_eq!(book.alerts()[0].cause, HaltCause::IllegalTransition);
        assert!(matches!(
            error,
            BookError::Halted(TransitionError::IllegalTransition { .. })
        ));
    }
    assert_eq!(illegal, 87);
}

pub struct MemSink {
    pub epoch: HaltEpoch,
    pub causes: Vec<HaltCause>,
}

impl MemSink {
    pub fn new() -> Self {
        Self {
            epoch: HaltEpoch::ZERO,
            causes: Vec::new(),
        }
    }
}

impl HaltSink for MemSink {
    fn latch(&mut self, cause: HaltCause) -> HaltEpoch {
        self.epoch = self.epoch.checked_next().expect("halt epoch fits in u64");
        self.causes.push(cause);
        self.epoch
    }
}

pub fn proptest_cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(512)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

pub fn state_index(state: OrderState) -> usize {
    match state {
        OrderState::PendingNew => 0,
        OrderState::Acked => 1,
        OrderState::PartiallyFilled => 2,
        OrderState::Filled => 3,
        OrderState::PendingCancel => 4,
        OrderState::Canceled => 5,
        OrderState::Rejected => 6,
        OrderState::Expired => 7,
        OrderState::Unknown => 8,
    }
}

pub fn event_index(event: Event) -> usize {
    match event {
        Event::Ack => 0,
        Event::Fill => 1,
        Event::CancelRequested => 2,
        Event::CancelDenied => 3,
        Event::CancelConfirmed => 4,
        Event::Reject => 5,
        Event::Expire => 6,
        Event::Timeout => 7,
        Event::QueryFilled => 8,
        Event::QueryCanceled => 9,
        Event::QueryExpired => 10,
        Event::QueryNotFound => 11,
        Event::RaceFill => 12,
    }
}
