use std::num::NonZeroUsize;
use std::path::Path;

use cucumber::{given, then, when, World as _};
use parangsae_core::{
    AvailableTs, BarInterval, DecisionTime, EventTs, Observation, Price, ReceivedTs, ResampledBar,
    Timestamps, UtcMicros,
};
use parangsae_signals::{CausalWindow, SignalError};
use parangsae_strategies::astra::range::{
    evaluate, RangeAssessment, RangeUnavailable, RANGE_WINDOW_CAP,
};
use parangsae_strategies::{PositionSide, StrategyError};

const HOUR: i64 = 3_600_000_000;
const T0: i64 = 1_704_067_200_000_000;
const DECISION: i64 = T0 + 26 * HOUR + 1_000;

#[derive(Debug, Default, cucumber::World)]
struct RangeWorld {
    rows: Vec<Observation<ResampledBar>>,
    decision: i64,
    result: Option<Result<RangeAssessment, StrategyError>>,
    baseline: Option<RangeAssessment>,
}

fn bar(index: i64, high: i64, low: i64, close: i64) -> ResampledBar {
    ResampledBar::new(
        UtcMicros::from_micros(T0 + index * HOUR),
        BarInterval::H1,
        Price::from_ticks(close).expect("synthetic open"),
        Price::from_ticks(high).expect("synthetic high"),
        Price::from_ticks(low).expect("synthetic low"),
        Price::from_ticks(close).expect("synthetic close"),
    )
    .expect("valid synthetic H1 bar")
}

fn observed(bar: ResampledBar, available: i64) -> Observation<ResampledBar> {
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(bar.open_time()),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
            // Synthetic receipt, not an assertion about historical publication.
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(available)),
        },
        bar,
    )
}

fn canonical(bar: ResampledBar) -> Observation<ResampledBar> {
    observed(bar, bar.close_time().micros() + 1_000)
}

fn assessed(world: &RangeWorld) -> Result<RangeAssessment, StrategyError> {
    let decision = DecisionTime::from_micros(UtcMicros::from_micros(world.decision));
    let window = CausalWindow::from_views(
        decision,
        NonZeroUsize::new(RANGE_WINDOW_CAP).expect("fixed nonzero cap"),
        world.rows.iter().filter_map(|row| row.causal_at(decision)),
    )
    .map_err(SignalError::from)?;
    evaluate(&window)
}

#[given("a complete synthetic range window has 24 prior ranges from 976 to 1024")]
fn complete(world: &mut RangeWorld) {
    world.rows = (0..26)
        .map(|index| canonical(bar(index, 1_024, 976, 1_000)))
        .collect();
    world.rows[25] = canonical(bar(25, 1_037, 980, 1_000));
    world.decision = DECISION;
    let baseline = assessed(world).expect("complete active synthetic window");
    assert!(matches!(
        baseline,
        RangeAssessment::Ready {
            side: PositionSide::Short,
            ..
        }
    ));
    world.baseline = Some(baseline);
}

#[when(regex = r"^the candidate has high (\d+) low (\d+) and close (\d+)$")]
fn candidate(world: &mut RangeWorld, high: i64, low: i64, close: i64) {
    world.rows[25] = canonical(bar(25, high, low, close));
    world.result = Some(assessed(world));
}

#[then(regex = r"^the pure range result is (short|long|known_false)$")]
fn condition(world: &mut RangeWorld, expected: String) {
    let value = world
        .result
        .as_ref()
        .expect("range was evaluated")
        .as_ref()
        .expect("valid complete window");
    let facts = match (expected.as_str(), value) {
        (
            "short",
            RangeAssessment::Ready {
                side: PositionSide::Short,
                facts,
            },
        )
        | (
            "long",
            RangeAssessment::Ready {
                side: PositionSide::Long,
                facts,
            },
        )
        | ("known_false", RangeAssessment::KnownFalse(facts)) => facts,
        _ => panic!("expected {expected}, got {value:?}"),
    };
    assert_eq!(facts.true_range_sum_ticks(), 1_152);
    assert_eq!((facts.lower().ticks(), facts.upper().ticks()), (976, 1_024));
    assert_eq!(facts.decision().micros().micros(), DECISION);
    assert_eq!(facts.bar_close_time().micros(), DECISION - 2_000);
}

#[when(
    regex = r"^the range window has (short_history|missing_candidate|known_gap|zero_atr|degenerate_range)$"
)]
fn unavailable(world: &mut RangeWorld, kind: String) {
    match kind.as_str() {
        "short_history" => {
            world.rows.remove(0);
        }
        "missing_candidate" => {
            world.rows[25] = observed(*world.rows[25].value(), DECISION + 1);
        }
        "known_gap" => {
            world.rows.remove(12);
        }
        "zero_atr" | "degenerate_range" => {
            world.rows = (0..26)
                .map(|index| canonical(bar(index, 1_000, 1_000, 1_000)))
                .collect();
            if kind == "degenerate_range" {
                world.rows[0] = canonical(bar(0, 1_500, 1_500, 1_500));
            }
        }
        _ => panic!("unknown synthetic missing-input case"),
    }
    world.result = Some(assessed(world));
}

#[then(
    regex = r"^the result is unavailable for (window_incomplete|zero_atr|degenerate_range) and is not observed false$"
)]
fn not_false(world: &mut RangeWorld, expected: String) {
    let reason = match expected.as_str() {
        "window_incomplete" => RangeUnavailable::WindowIncomplete,
        "zero_atr" => RangeUnavailable::ZeroAtr,
        "degenerate_range" => RangeUnavailable::DegenerateRange,
        _ => panic!("unknown unavailable reason"),
    };
    assert_eq!(
        world.result.as_ref().expect("evaluated"),
        &Ok(RangeAssessment::Unavailable(reason))
    );
}

#[when(
    regex = r"^the otherwise active range input has (duplicate|wrong_interval|wrong_availability|wrong_decision) metadata$"
)]
fn malformed(world: &mut RangeWorld, kind: String) {
    match kind.as_str() {
        "duplicate" => {
            world.rows.insert(13, world.rows[12].clone());
        }
        "wrong_interval" => {
            let old = *world.rows[12].value();
            let wrong = ResampledBar::new(
                old.open_time(),
                BarInterval::H4,
                old.open(),
                old.high(),
                old.low(),
                old.close(),
            )
            .expect("synthetic aligned H4 bar");
            world.rows[12] = world.rows[12].with_value(wrong);
        }
        "wrong_availability" => {
            let old = *world.rows[12].value();
            world.rows[12] = observed(old, old.close_time().micros() + 1_001);
        }
        "wrong_decision" => {
            world.decision += 1;
        }
        _ => panic!("unknown malformed-input case"),
    }
    world.result = Some(assessed(world));
}

#[then("the pure API returns a typed refusal instead of a trading condition")]
fn refused(world: &mut RangeWorld) {
    world
        .result
        .as_ref()
        .expect("evaluated")
        .as_ref()
        .expect_err("malformed metadata must be refused");
}

#[when("unavailable future OHLC changes while the admitted seed remains fixed")]
fn future(world: &mut RangeWorld) {
    world.rows.push(canonical(bar(26, 5_000, 100, 2_000)));
    assert_eq!(
        assessed(world).expect("future is gated"),
        world.baseline.expect("active baseline")
    );
    world.rows[26] = canonical(bar(26, 9_000, 50, 3_000));
    world.result = Some(assessed(world));
}

#[then("the same active short result survives but changing the admitted seed changes the result")]
fn causal_control(world: &mut RangeWorld) {
    assert_eq!(
        world.result.as_ref().expect("evaluated"),
        &Ok(world.baseline.expect("active baseline"))
    );
    world.rows[0] = canonical(bar(0, 5_000, 5_000, 5_000));
    let changed = assessed(world).expect("admitted past stays causal");
    assert!(matches!(changed, RangeAssessment::KnownFalse(_)));
    let RangeAssessment::KnownFalse(facts) = changed else {
        unreachable!("checked above")
    };
    assert_eq!(facts.true_range_sum_ticks(), 5_128);
}

#[tokio::main]
async fn main() {
    RangeWorld::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/astra_range_signal.feature"
        )))
        .await;
}
