//! Cucumber runner for `specs/features/kline_integrity.feature`. A failed step returns `Err`.

#[path = "support/feature_floors.rs"]
mod feature_floors;
#[path = "support/feature_run.rs"]
mod feature_run;

use cucumber::{gherkin::Step, given, then, when};
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
    unit: Option<TimeUnit>,
}

#[given(regex = r"^the raw timestamp (\d+)$")]
fn raw_timestamp(world: &mut KlineWorld, raw: i64) {
    world.raw = Some(raw);
}

#[when("it is read as milliseconds")]
fn read_millis(world: &mut KlineWorld) -> Result<(), String> {
    let raw = world.raw.ok_or("Given the raw timestamp")?;
    world.unit = Some(TimeUnit::Millis);
    world.read = Some(require_declared_unit(raw, TimeUnit::Millis).map_err(|err| err.to_string()));
    Ok(())
}

#[when("it is read as microseconds")]
fn read_micros(world: &mut KlineWorld) -> Result<(), String> {
    let raw = world.raw.ok_or("Given the raw timestamp")?;
    world.unit = Some(TimeUnit::Micros);
    world.read = Some(require_declared_unit(raw, TimeUnit::Micros).map_err(|err| err.to_string()));
    Ok(())
}

#[then(regex = r"^the normalized timestamp is (\d+) microseconds$")]
fn normalized_is(world: &mut KlineWorld, expected: i64) -> Result<(), String> {
    match world.read.as_ref() {
        Some(Ok(timestamp)) => {
            assert_eq!(timestamp.micros(), expected);
            Ok(())
        }
        Some(Err(message)) => Err(format!("expected a normalized timestamp, got {message}")),
        None => Err("expected a normalized timestamp, missing read".to_owned()),
    }
}

#[then(regex = r"^the 1m close time is (\d+) microseconds$")]
fn close_time_is(world: &mut KlineWorld, expected: i64) -> Result<(), String> {
    let open = match world.read.as_ref() {
        Some(Ok(timestamp)) => *timestamp,
        Some(Err(message)) => {
            return Err(format!("expected a normalized open time, got {message}"))
        }
        None => return Err("expected a normalized open time, missing read".to_owned()),
    };
    let unit = world.unit.ok_or("declared unit")?;
    let close = expected_close(open, Interval::ONE_MINUTE, unit).map_err(|err| err.to_string())?;
    assert_eq!(close.micros(), expected);
    Ok(())
}

#[then("the read fails as a unit mismatch")]
fn read_fails(world: &mut KlineWorld) -> Result<(), String> {
    match world.read.as_ref() {
        Some(Err(message)) => {
            if message.contains("unit mismatch") {
                Ok(())
            } else {
                Err(format!("error did not report a unit mismatch: {message}"))
            }
        }
        Some(Ok(timestamp)) => Err(format!("expected a unit mismatch, got {timestamp}")),
        None => Err("expected a unit mismatch, missing read".to_owned()),
    }
}

#[given("1m open times in microseconds")]
fn open_times(world: &mut KlineWorld, step: &Step) -> Result<(), String> {
    let table = step.table.as_ref().ok_or("open-time table")?;
    let mut opens = Vec::new();
    for row in table.rows.iter().skip(1) {
        let raw = row[0]
            .trim()
            .parse::<i64>()
            .map_err(|err| format!("open_us integer: {err}"))?;
        opens.push(UtcMicros::from_micros(raw));
    }
    world.opens = opens;
    Ok(())
}

#[then("the open-time grid is continuous")]
fn grid_continuous(world: &mut KlineWorld) -> Result<(), String> {
    require_regular_grid(&world.opens, Interval::ONE_MINUTE).map_err(|err| err.to_string())?;
    Ok(())
}

#[then("the open-time grid has a gap")]
fn grid_gap(world: &mut KlineWorld) -> Result<(), String> {
    let result = require_regular_grid(&world.opens, Interval::ONE_MINUTE);
    if result.is_err() {
        Ok(())
    } else {
        Err(format!("expected a grid gap, got {result:?}"))
    }
}

#[given(regex = r"^a 1m span opening at (\d+) microseconds$")]
fn span_open(world: &mut KlineWorld, open: i64) {
    world.span_open = Some(UtcMicros::from_micros(open));
}

#[given(regex = r"^closing at (\d+) microseconds$")]
fn span_close(world: &mut KlineWorld, close: i64) {
    world.span_close = Some(UtcMicros::from_micros(close));
}

#[given(regex = r"^the declared close tick is (milliseconds|microseconds)$")]
fn declared_tick(world: &mut KlineWorld, text: String) -> Result<(), String> {
    world.unit = Some(match text.as_str() {
        "milliseconds" => TimeUnit::Millis,
        "microseconds" => TimeUnit::Micros,
        _ => return Err(format!("declared close tick {text}")),
    });
    Ok(())
}

fn span(world: &KlineWorld) -> Result<KlineSpan, String> {
    Ok(KlineSpan {
        open_time: world.span_open.ok_or("opening timestamp")?,
        close_time: world.span_close.ok_or("closing timestamp")?,
        interval: Interval::ONE_MINUTE,
        unit: world.unit.ok_or("declared close tick")?,
    })
}

#[then("the span matches the interval")]
fn span_matches(world: &mut KlineWorld) -> Result<(), String> {
    validate_close_time(span(world)?).map_err(|err| err.to_string())?;
    Ok(())
}

#[then("the span does not match the interval")]
fn span_rejects(world: &mut KlineWorld) -> Result<(), String> {
    let result = validate_close_time(span(world)?);
    if result.is_err() {
        Ok(())
    } else {
        Err(format!("expected close_time rejection, got {result:?}"))
    }
}

const FEATURE_GLOB: &str = "specs/features/kline_integrity.feature";

#[tokio::main]
async fn main() -> Result<(), String> {
    let files = feature_floors::expand_feature_glob(FEATURE_GLOB)?;
    let mut passed_steps = 0usize;
    for path in &files {
        let counts = feature_run::run_feature::<KlineWorld>(path).await?;
        passed_steps += counts.passed_steps;
    }
    feature_floors::enforce_floors("kline", &files, passed_steps)
}
