use std::path::PathBuf;

use cucumber::{given, then, when, World as _};
use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Ratio, ReceivedTs, Timestamps, UtcMicros,
};
use parangsae_signals::premium_input::{PremiumInputError, PremiumMinuteInput};

const MINUTE_US: i64 = 60_000_000;

#[derive(Debug, Default, cucumber::World)]
struct PremiumWorld {
    minute: Option<UtcMicros>,
    input: Option<Result<PremiumMinuteInput, PremiumInputError>>,
    observation: Option<Observation<PremiumMinuteInput>>,
    original_timestamps: Option<Timestamps>,
    visible_ticks: Option<i64>,
}

#[given(regex = r"^a source minute time of (-?\d+) microseconds$")]
fn source_minute(world: &mut PremiumWorld, micros: i64) {
    world.minute = Some(UtcMicros::from_micros(micros));
}

#[when(regex = r"^a premium input is constructed with close ticks (-?\d+)$")]
fn construct(world: &mut PremiumWorld, ticks: i64) {
    world.input = Some(PremiumMinuteInput::try_new(
        world.minute.expect("source minute"),
        Ratio::from_ticks(ticks),
    ));
}

#[then(regex = r"^the premium close ticks remain (-?\d+)$")]
fn preserved_close(world: &mut PremiumWorld, ticks: i64) {
    assert_eq!(
        world
            .input
            .expect("construction")
            .expect("valid minute")
            .close()
            .ticks(),
        ticks
    );
}

#[then(regex = r"^the minute time remains (-?\d+) microseconds$")]
fn preserved_minute(world: &mut PremiumWorld, micros: i64) {
    assert_eq!(
        world
            .input
            .expect("construction")
            .expect("valid minute")
            .minute_start()
            .micros(),
        micros
    );
}

#[then("the off-minute input is refused without rounding")]
fn refused(world: &mut PremiumWorld) {
    assert_eq!(
        world.input,
        Some(Err(PremiumInputError::OffMinuteGrid {
            minute_start: world.minute.expect("source minute"),
        }))
    );
}

#[given("an archived premium input wrapped in its unchanged source clocks")]
fn archived(world: &mut PremiumWorld) {
    let minute = UtcMicros::from_micros(1_608_249_600_000_000);
    let end = minute.checked_add_micros(MINUTE_US).expect("end");
    let timestamps = Timestamps {
        event_ts: EventTs::from_micros(minute),
        available_ts: AvailableTs::from_micros(end),
        received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(1_792_776_118_572_302)),
    };
    let value = PremiumMinuteInput::try_new(minute, Ratio::from_ticks(-1)).expect("minute");
    world.observation = Some(Observation::new(timestamps, value));
    world.original_timestamps = Some(timestamps);
}

fn request(world: &mut PremiumWorld, offset: i64) {
    let observation = world.observation.as_ref().expect("source observation");
    let at = observation
        .timestamps()
        .available_ts
        .micros()
        .checked_add_micros(offset)
        .expect("decision");
    world.visible_ticks = observation
        .causal_at(DecisionTime::from_micros(at))
        .map(|view| view.value().close().ticks());
}

#[when("a causal view is requested one microsecond before availability")]
fn before(world: &mut PremiumWorld) {
    request(world, -1);
}

#[when("a causal view is requested at availability")]
fn available(world: &mut PremiumWorld) {
    request(world, 0);
}

#[then("no premium value is visible")]
fn invisible(world: &mut PremiumWorld) {
    assert_eq!(world.visible_ticks, None);
}

#[then(regex = r"^the visible premium close ticks are (-?\d+)$")]
fn visible(world: &mut PremiumWorld, ticks: i64) {
    assert_eq!(world.visible_ticks, Some(ticks));
}

#[then("the original source timestamps were not rewritten")]
fn unchanged(world: &mut PremiumWorld) {
    assert_eq!(
        world
            .observation
            .as_ref()
            .expect("observation")
            .timestamps(),
        world.original_timestamps.expect("original clocks")
    );
}

#[tokio::main]
async fn main() {
    let feature = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/features/signals/premium_input.feature");
    PremiumWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit(feature)
        .await;
}
