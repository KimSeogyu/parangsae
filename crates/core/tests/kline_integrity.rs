//! Cucumber runner for `specs/features`. A failed or undefined step panics the binary.

use std::path::PathBuf;

use cucumber::{gherkin::Step, given, then, when, World as _};
use parangsae_core::{
    expected_close, require_declared_unit, require_regular_grid, validate_close_time, Interval,
    KlineSpan, TimeUnit, UtcMicros,
};

#[derive(Debug, Default, cucumber::World)]
struct KlineWorld {
    raw: Option<i64>,
    read: Option<Result<UtcMicros, String>>,
    opens: Vec<UtcMicros>,
    span_open: Option<UtcMicros>,
    span_close: Option<UtcMicros>,
}

#[given(regex = r"^the raw timestamp (\d+)$")]
fn raw_timestamp(world: &mut KlineWorld, raw: i64) {
    world.raw = Some(raw);
}

#[when("it is read as milliseconds")]
fn read_millis(world: &mut KlineWorld) {
    let raw = world.raw.expect("Given the raw timestamp");
    world.read = Some(require_declared_unit(raw, TimeUnit::Millis).map_err(|err| err.to_string()));
}

#[when("it is read as microseconds")]
fn read_micros(world: &mut KlineWorld) {
    let raw = world.raw.expect("Given the raw timestamp");
    world.read = Some(require_declared_unit(raw, TimeUnit::Micros).map_err(|err| err.to_string()));
}

#[then(regex = r"^the normalized timestamp is (\d+) microseconds$")]
fn normalized_is(world: &mut KlineWorld, expected: i64) {
    match world.read.as_ref() {
        Some(Ok(timestamp)) => assert_eq!(timestamp.micros(), expected),
        other => panic!("expected a normalized timestamp, got {other:?}"),
    }
}

#[then(regex = r"^the 1m close time is (\d+) microseconds$")]
fn close_time_is(world: &mut KlineWorld, expected: i64) {
    let open = match world.read.as_ref() {
        Some(Ok(timestamp)) => *timestamp,
        other => panic!("expected a normalized open time, got {other:?}"),
    };
    let close = expected_close(open, Interval::ONE_MINUTE).expect("close time fits");
    assert_eq!(close.micros(), expected);
}

#[then("the read fails as a unit mismatch")]
fn read_fails(world: &mut KlineWorld) {
    match world.read.as_ref() {
        Some(Err(message)) => {
            assert!(
                message.contains("unit mismatch"),
                "error did not report a unit mismatch: {message}"
            );
        }
        other => panic!("expected a unit mismatch, got {other:?}"),
    }
}

#[given("1m open times in microseconds")]
fn open_times(world: &mut KlineWorld, step: &Step) {
    let table = step.table.as_ref().expect("open-time table");
    world.opens = table
        .rows
        .iter()
        .skip(1)
        .map(|row| {
            let raw = row[0].trim().parse::<i64>().expect("open_us integer");
            UtcMicros::from_micros(raw)
        })
        .collect();
}

#[then("the open-time grid is continuous")]
fn grid_continuous(world: &mut KlineWorld) {
    require_regular_grid(&world.opens, Interval::ONE_MINUTE).expect("grid should be continuous");
}

#[then("the open-time grid has a gap")]
fn grid_gap(world: &mut KlineWorld) {
    let result = require_regular_grid(&world.opens, Interval::ONE_MINUTE);
    assert!(result.is_err(), "expected a grid gap, got {result:?}");
}

#[given(regex = r"^a 1m span opening at (\d+) microseconds$")]
fn span_open(world: &mut KlineWorld, open: i64) {
    world.span_open = Some(UtcMicros::from_micros(open));
}

#[given(regex = r"^closing at (\d+) microseconds$")]
fn span_close(world: &mut KlineWorld, close: i64) {
    world.span_close = Some(UtcMicros::from_micros(close));
}

fn span(world: &KlineWorld) -> KlineSpan {
    KlineSpan {
        open_time: world.span_open.expect("opening timestamp"),
        close_time: world.span_close.expect("closing timestamp"),
        interval: Interval::ONE_MINUTE,
    }
}

#[then("the span matches the interval")]
fn span_matches(world: &mut KlineWorld) {
    validate_close_time(span(world)).expect("span should match the 1m rule");
}

#[then("the span does not match the interval")]
fn span_rejects(world: &mut KlineWorld) {
    let result = validate_close_time(span(world));
    assert!(
        result.is_err(),
        "expected close_time rejection, got {result:?}"
    );
}

#[tokio::main]
async fn main() {
    let features = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/features/kline_integrity.feature");
    KlineWorld::cucumber().run_and_exit(features).await;
}
