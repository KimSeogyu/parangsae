use std::path::PathBuf;

use cucumber::{given, then, when, World as _};
use parangsae_backtest::astra_premium_source::{
    AstraPremiumObservation, PremiumModelClock, PremiumSourceError, PremiumSourceProjection,
};
use parangsae_core::{AvailableTs, DecisionTime, EventTs, ReceivedTs, Symbol, UtcMicros};

const START: i64 = 1_608_249_600_000_000;
const MINUTE: i64 = 60_000_000;
const RECORDED: i64 = 1_792_776_118_572_302;

#[derive(Debug, Default, cucumber::World)]
struct PremiumSourceWorld {
    row: Option<PremiumSourceProjection>,
    result: Option<Result<AstraPremiumObservation, PremiumSourceError>>,
    visible: Option<i64>,
}

fn symbol() -> Symbol {
    Symbol::new("BTCUSDT").expect("symbol")
}

fn row(start: i64, ticks: i64) -> PremiumSourceProjection {
    PremiumSourceProjection {
        symbol: symbol(),
        interval_us: MINUTE,
        open_time: UtcMicros::from_micros(start),
        close_time: UtcMicros::from_micros(start + MINUTE - 1_000),
        close_ticks: ticks,
        event_ts: EventTs::from_micros(UtcMicros::from_micros(start)),
        stored_available_ts: AvailableTs::from_micros(UtcMicros::from_micros(start + MINUTE)),
        recorded_received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(RECORDED)),
        schema_version: 1,
        tick_domain: "ratio".to_owned(),
    }
}

#[given("a synthetic ten-field premium projection")]
fn projection(world: &mut PremiumSourceWorld) {
    world.row = Some(row(START, -1));
}

#[given("a synthetic premium projection with price domain")]
fn bad_domain(world: &mut PremiumSourceWorld) {
    world.row = Some(PremiumSourceProjection {
        tick_domain: "price".to_owned(),
        ..row(START, -1)
    });
}

#[when("the projection is checked under the named Astra minute-end model")]
fn checked(world: &mut PremiumSourceWorld) {
    world.result = Some(AstraPremiumObservation::from_projection(
        &symbol(),
        world.row.clone().expect("projection"),
        PremiumModelClock::AstraProtocolV1ClosedMinuteEnd,
    ));
}

fn observe(world: &mut PremiumSourceWorld, at: i64) {
    world.visible = world
        .result
        .as_ref()
        .expect("checked")
        .as_ref()
        .expect("valid")
        .causal_at(DecisionTime::from_micros(UtcMicros::from_micros(at)))
        .map(|view| view.value().close().ticks());
}

#[when("a view is requested just before the complete minute")]
fn before(world: &mut PremiumSourceWorld) {
    observe(world, START + MINUTE - 1);
}

#[when("a view is requested at the complete minute")]
fn on_time(world: &mut PremiumSourceWorld) {
    observe(world, START + MINUTE);
}

#[then("no signed ratio is visible yet")]
fn hidden(world: &mut PremiumSourceWorld) {
    assert_eq!(world.visible, None);
}

#[then("the signed ratio is visible without rewriting the original clocks")]
fn original(world: &mut PremiumSourceWorld) {
    let value = world
        .result
        .as_ref()
        .expect("checked")
        .as_ref()
        .expect("valid");
    assert_eq!(world.visible, Some(-1));
    assert_eq!(
        value.original_timestamps().event_ts.micros().micros(),
        START
    );
    assert_eq!(
        value.original_timestamps().received_ts.micros().micros(),
        RECORDED
    );
    assert_eq!(value.model_delivery().known_at().micros(), START + MINUTE);
    assert_eq!(
        value.model_delivery().clock(),
        PremiumModelClock::AstraProtocolV1ClosedMinuteEnd
    );
}

#[then("the wrong domain is a typed refusal")]
fn domain_refused(world: &mut PremiumSourceWorld) {
    assert_eq!(
        world.result,
        Some(Err(PremiumSourceError::TickDomainMismatch))
    );
}

#[when("a future ratio is changed after the earlier decision")]
fn future_changed(world: &mut PremiumSourceWorld) {
    let current = AstraPremiumObservation::from_projection(
        &symbol(),
        row(START, -1),
        PremiumModelClock::AstraProtocolV1ClosedMinuteEnd,
    )
    .expect("current");
    for changed in [-99, 0, 99] {
        let future = AstraPremiumObservation::from_projection(
            &symbol(),
            row(START + MINUTE, changed),
            PremiumModelClock::AstraProtocolV1ClosedMinuteEnd,
        )
        .expect("future");
        let earlier: Vec<_> = [&current, &future]
            .into_iter()
            .filter_map(|value| {
                value.causal_at(DecisionTime::from_micros(UtcMicros::from_micros(
                    START + MINUTE,
                )))
            })
            .map(|view| view.value().close().ticks())
            .collect();
        assert_eq!(earlier, [-1]);
        assert_eq!(
            future
                .causal_at(DecisionTime::from_micros(UtcMicros::from_micros(
                    START + 2 * MINUTE
                )))
                .expect("later")
                .value()
                .close()
                .ticks(),
            changed
        );
    }
    world.visible = Some(-1);
}

#[then("only the earlier delivered ratio is observed")]
fn earlier_only(world: &mut PremiumSourceWorld) {
    assert_eq!(world.visible, Some(-1));
}

#[tokio::main]
async fn main() {
    let feature = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/features/astra_premium_source.feature");
    PremiumSourceWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit(feature)
        .await;
}
