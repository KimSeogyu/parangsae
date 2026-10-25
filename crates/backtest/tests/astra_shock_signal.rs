use cucumber::{then, when, World as _};
use parangsae_core::{
    AvailableTs, BarInterval, DecisionTime, EventTs, Observation, Price, ReceivedTs, ResampledBar,
    Timestamps, UtcMicros,
};
use parangsae_signals::CausalWindow;
use parangsae_strategies::astra::shock::{
    assess_shock, ShockAssessment, ShockRejection, ShockUnavailable, SHOCK_BAR_CAP,
};
use parangsae_strategies::{PositionSide, StrategyError};

const SCALE: i64 = 100_000_000;
const HOUR: i64 = 3_600_000_000;
const ORIGIN: i64 = 1_704_067_200_000_000;

fn price(ticks: i64) -> Price {
    Price::from_ticks(ticks).expect("valid synthetic fixed-point price")
}

fn decimal_price(raw: &str) -> Price {
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    assert!(fraction.len() <= 8 && fraction.bytes().all(|byte| byte.is_ascii_digit()));
    let whole = whole.parse::<i64>().expect("Gherkin integer price");
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<i64>().expect("Gherkin fractional price")
            * 10_i64.pow(8 - u32::try_from(fraction.len()).expect("bounded price precision"))
    };
    price(
        whole
            .checked_mul(SCALE)
            .and_then(|value| value.checked_add(fraction))
            .expect("bounded Gherkin price"),
    )
}

fn row(index: usize, open: i64, close: i64) -> Observation<ResampledBar> {
    let at = ORIGIN + i64::try_from(index).expect("bounded synthetic hour") * HOUR;
    let bar = ResampledBar::new(
        UtcMicros::from_micros(at),
        BarInterval::H1,
        price(open),
        price(open.max(close) + SCALE),
        price(open.min(close) - SCALE),
        price(close),
    )
    .expect("consistent synthetic OHLC");
    let available = UtcMicros::from_micros(at + HOUR);
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(bar.close_time()),
            available_ts: AvailableTs::from_micros(available),
            received_ts: ReceivedTs::from_micros(available),
        },
        bar,
    )
}

fn input(
    shock: Price,
    confirmation: Price,
    flat: bool,
    count: usize,
) -> Vec<Observation<ResampledBar>> {
    let mut rows = Vec::with_capacity(count);
    let mut previous = 100 * SCALE;
    for index in 0..count {
        let close = match index {
            0..=672 if !flat && index % 2 == 1 => 100 * SCALE + SCALE / 5,
            674 => shock.ticks(),
            675 => (shock.ticks() + confirmation.ticks()) / 2,
            676 => confirmation.ticks(),
            _ => 100 * SCALE,
        };
        rows.push(row(index, previous, close));
        previous = close;
    }
    rows
}

fn decision() -> DecisionTime {
    DecisionTime::from_micros(UtcMicros::from_micros(ORIGIN + 677 * HOUR + 1_000))
}

fn evaluate(
    rows: &[Observation<ResampledBar>],
    at: DecisionTime,
) -> Result<ShockAssessment, StrategyError> {
    let window = CausalWindow::from_views(
        at,
        SHOCK_BAR_CAP,
        rows.iter().filter_map(|row| row.causal_at(at)),
    )
    .expect("ordered causal synthetic rows");
    assess_shock(&window)
}

#[derive(Debug, Default, cucumber::World)]
struct ShockWorld {
    actual: Option<Result<ShockAssessment, StrategyError>>,
    earlier: Option<ShockAssessment>,
}

#[when(
    regex = r"^the actual shock kernel sees a (\d+(?:\.\d+)?) USDT shock close and a (\d+(?:\.\d+)?) USDT confirmation close$"
)]
fn price_example(world: &mut ShockWorld, shock: String, confirmation: String) {
    world.actual = Some(evaluate(
        &input(
            decimal_price(&shock),
            decimal_price(&confirmation),
            false,
            677,
        ),
        decision(),
    ));
}

#[then(
    regex = r"^it emits a causal (short|long) opportunity for the shock hour two hours before confirmation$"
)]
fn active_side(world: &mut ShockWorld, side: String) {
    let actual = world
        .actual
        .as_ref()
        .expect("actual kernel call")
        .as_ref()
        .expect("valid feature");
    let ShockAssessment::Ready(event) = actual else {
        panic!("a real opportunity is required: {actual:?}");
    };
    assert_eq!(
        event.side,
        if side == "short" {
            PositionSide::Short
        } else {
            PositionSide::Long
        }
    );
    assert_eq!(event.facts.shock_end.micros(), ORIGIN + 675 * HOUR);
    assert_eq!(event.facts.confirmation_end.micros(), ORIGIN + 677 * HOUR);
    assert_eq!(event.facts.decision_ts, decision());
}

#[when("the same qualifying shock follows a constant 672 return baseline")]
fn no_scale(world: &mut ShockWorld) {
    world.actual = Some(evaluate(
        &input(decimal_price("110"), decimal_price("104.5"), true, 677),
        decision(),
    ));
}

#[then("zero scale is unavailable and is not a known false rearming observation")]
fn unavailable_scale(world: &mut ShockWorld) {
    assert_eq!(
        world.actual.as_ref().expect("actual kernel call"),
        &Ok(ShockAssessment::Unavailable(ShockUnavailable::ZeroScale))
    );
}

#[when(
    "a past baseline close becomes 200 USDT after the original frame reached an active decision"
)]
fn changed_past(world: &mut ShockWorld) {
    let mut rows = input(decimal_price("110"), decimal_price("104.5"), false, 677);
    let before = evaluate(&rows, decision()).expect("valid active frame");
    assert!(matches!(before, ShockAssessment::Ready(_)));
    world.earlier = Some(before);
    rows[300] = row(300, rows[299].value().close().ticks(), 200 * SCALE);
    world.actual = Some(evaluate(&rows, decision()));
}

#[then("the real past perturbation changes the decision to known false below the shock threshold")]
fn changed_decision(world: &mut ShockWorld) {
    assert!(matches!(
        world.actual.as_ref().expect("actual call"),
        Ok(ShockAssessment::KnownFalse {
            reason: ShockRejection::BelowShockThreshold,
            ..
        })
    ));
}

#[when("future closes become 500 USDT after an earlier active shock decision")]
fn changed_future(world: &mut ShockWorld) {
    let mut rows = input(decimal_price("110"), decimal_price("104.5"), false, 690);
    let before = evaluate(&rows[..677], decision()).expect("valid active frame");
    assert!(matches!(before, ShockAssessment::Ready(_)));
    world.earlier = Some(before);
    for (index, value) in rows.iter_mut().enumerate().skip(677) {
        *value = row(index, value.value().open().ticks(), 500 * SCALE);
    }
    world.actual = Some(evaluate(&rows, decision()));
}

#[then("the active earlier opportunity is exactly unchanged by future mutation and truncation")]
fn unchanged_prefix(world: &mut ShockWorld) {
    assert_eq!(
        world
            .actual
            .as_ref()
            .expect("actual call")
            .as_ref()
            .expect("valid feature"),
        world
            .earlier
            .as_ref()
            .expect("actually active earlier output")
    );
}

#[when("the qualifying shock is evaluated one millisecond after its declared decision")]
fn wrong_decision(world: &mut ShockWorld) {
    let later =
        DecisionTime::from_micros(UtcMicros::from_micros(decision().micros().micros() + 1_000));
    world.actual = Some(evaluate(
        &input(decimal_price("110"), decimal_price("104.5"), false, 677),
        later,
    ));
}

#[then("the actual kernel reports the typed clock error")]
fn clock_error(world: &mut ShockWorld) {
    assert_eq!(
        world.actual.as_ref().expect("actual call"),
        &Err(StrategyError::Clock)
    );
}

#[tokio::main]
async fn main() {
    ShockWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/astra_shock_signal.feature"
        ))
        .await;
}
