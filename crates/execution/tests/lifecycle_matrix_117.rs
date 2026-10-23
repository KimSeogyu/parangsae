mod support;

use std::collections::BTreeSet;

use parangsae_execution::{Event, OrderState};
use support::{
    assert_every_cell_matches, assert_illegal_cells_halt, each_event, each_order_state,
    event_index, load_fixture, state_index, FIXTURE_SHA256,
};

#[test]
fn fixture_sha_is_pinned() {
    let fixture = load_fixture();
    assert_eq!(fixture.sha256, FIXTURE_SHA256);
}

#[test]
fn fixture_has_117_cells() {
    let fixture = load_fixture();
    assert_eq!(fixture.cells.len(), 117);
    assert_eq!(
        fixture.cells.len(),
        each_order_state().len() * each_event().len()
    );
    let pairs: BTreeSet<_> = fixture
        .cells
        .iter()
        .map(|cell| (cell.state.as_str(), cell.event.as_str()))
        .collect();
    assert_eq!(pairs.len(), 117);
}

#[test]
fn fixture_and_enums_agree_both_ways() {
    let fixture = load_fixture();
    let json_states: BTreeSet<_> = fixture
        .cells
        .iter()
        .map(|cell| cell.state.as_str())
        .collect();
    let json_events: BTreeSet<_> = fixture
        .cells
        .iter()
        .map(|cell| cell.event.as_str())
        .collect();
    let rust_states: BTreeSet<_> = each_order_state()
        .into_iter()
        .map(OrderState::name)
        .collect();
    let rust_events: BTreeSet<_> = each_event().into_iter().map(Event::name).collect();
    assert_eq!(json_states, rust_states);
    assert_eq!(json_events, rust_events);
}

#[test]
fn all_is_exhaustive() {
    for (index, state) in each_order_state().into_iter().enumerate() {
        assert_eq!(state_index(state), index);
    }
    for (index, event) in each_event().into_iter().enumerate() {
        assert_eq!(event_index(event), index);
    }
}

#[test]
fn every_cell_matches_model() {
    let fixture = load_fixture();
    let counts = assert_every_cell_matches(&fixture);
    assert_eq!(counts.cells, 117);
    assert_eq!(counts.legal, 30);
    assert_eq!(counts.illegal, 87);
    println!(
        "replay: {} cells, {} legal, {} illegal",
        counts.cells, counts.legal, counts.illegal
    );
}

#[test]
fn illegal_cell_halts_book() {
    let fixture = load_fixture();
    assert_illegal_cells_halt(&fixture);
}
