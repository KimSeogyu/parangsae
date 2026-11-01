//! Executable synthetic observers for the public catalogue kernels. No tape or study.
use std::num::NonZeroUsize;
use std::path::Path;

use cucumber::{given, then, when, World as _};
use parangsae_backtest::catalog::{self, InputUniverse, Output, Request};
use parangsae_core::{
    AvailableTs, BarInterval, DecisionTime, EventTs, Observation, Price, Qty, ReceivedTs,
    ResampledBar, Symbol, Timestamps, UtcMicros,
};
use parangsae_signals::CausalWindow;
use parangsae_strategies::catalog::{panel, price};

const HOUR: i64 = 3_600_000_000;

#[derive(Debug, Default, cucumber::World)]
struct CatalogWorld {
    family: String,
    rows: Vec<Observation<price::PriceBar>>,
    price: Option<Result<price::Assessment, price::Error>>,
    pair: Option<Result<panel::PairAssessment, panel::Error>>,
    allocation: Option<Result<panel::Allocation, panel::Error>>,
}
fn at(t: i64) -> DecisionTime {
    DecisionTime::from_micros(UtcMicros::from_micros(t))
}
fn observed<T>(time: i64, value: T) -> Observation<T> {
    let t = UtcMicros::from_micros(time);
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(t),
            available_ts: AvailableTs::from_micros(t),
            received_ts: ReceivedTs::from_micros(t),
        },
        value,
    )
}
fn bar(index: i64, price_ticks: i64, volume_ticks: i64) -> price::PriceBar {
    let p = Price::from_ticks(price_ticks).expect("synthetic price");
    price::PriceBar::new(
        ResampledBar::new(
            UtcMicros::from_micros(index * HOUR),
            BarInterval::H1,
            p,
            p,
            p,
            p,
        )
        .expect("synthetic closed bar"),
        Qty::from_ticks(volume_ticks).expect("synthetic volume"),
    )
    .expect("positive bar")
}
fn window<'a, T>(rows: &'a [Observation<T>]) -> CausalWindow<'a, T> {
    let decision = at(3 * HOUR);
    CausalWindow::from_views(
        decision,
        NonZeroUsize::new(16).expect("cap"),
        rows.iter().filter_map(|r| r.causal_at(decision)),
    )
    .expect("availability ordered")
}
#[given(expr = "a synthetic catalogue price fixture for {word}")]
fn price_fixture(world: &mut CatalogWorld, family: String) {
    world.family = family;
    world.rows = vec![
        observed(HOUR, bar(0, 100, 10)),
        observed(2 * HOUR, bar(1, 100, 10)),
        observed(3 * HOUR, bar(2, 120, 21)),
    ];
}
#[given("a later extreme bar is appended after the decision")]
fn future(world: &mut CatalogWorld) {
    world.rows.push(observed(4 * HOUR, bar(3, i64::MAX, 1)));
}
#[given("the candidate price is exactly at the one percent threshold")]
fn equality(world: &mut CatalogWorld) {
    world.rows[2] = observed(3 * HOUR, bar(2, 101, 21));
}
#[given("the candidate is absent")]
fn missing(world: &mut CatalogWorld) {
    world.rows.pop();
}
#[given("the first bar claims availability before its canonical close")]
fn invalid_clock(world: &mut CatalogWorld) {
    world.rows[0] = observed(HOUR - 1, bar(0, 100, 10));
}
#[when("the catalogue price kernel evaluates at the third close")]
fn evaluate_price(world: &mut CatalogWorld) {
    let rule = match world.family.as_str() {
        "momentum" => price::PriceRule::Momentum {
            lookback: 2,
            threshold_bps: 100,
        },
        "mean_reversion" => price::PriceRule::MeanReversion {
            lookback: 2,
            threshold_bps: 100,
        },
        "trend_following" => price::PriceRule::TrendFollowing { fast: 1, slow: 3 },
        "short_term_reversal" => price::PriceRule::ShortTermReversal {
            lookback: 2,
            threshold_bps: 100,
            volume_multiple: 2,
        },
        "breakout" => price::PriceRule::Breakout { lookback: 2 },
        _ => panic!("unknown test family"),
    };
    world.price = Some(
        match catalog::evaluate(
            InputUniverse::SuppliedOffline,
            Request::Price {
                rule,
                window: &window(&world.rows),
            },
        ) {
            Ok(Output::Price(result)) => Ok(result),
            Err(catalog::Error::Price(error)) => Err(error),
            other => panic!("wrong catalogue price route: {other:?}"),
        },
    );
}
#[then(expr = "the catalogue price direction is {word}")]
fn direction(world: &mut CatalogWorld, expected: String) {
    let direction = match expected.as_str() {
        "long" => price::Direction::Long,
        "short" => price::Direction::Short,
        "neutral" => price::Direction::Neutral,
        _ => panic!("unknown test direction"),
    };
    assert_eq!(world.price, Some(Ok(price::Assessment::Ready(direction))));
}
#[then("the catalogue result is typed unavailable history, not a flat signal")]
fn unavailable(world: &mut CatalogWorld) {
    assert_eq!(
        world.price,
        Some(Ok(price::Assessment::Unavailable(
            price::Unavailable::Warmup
        )))
    );
}
#[then("the catalogue price input is refused with Clock")]
fn clock_refusal(world: &mut CatalogWorld) {
    assert_eq!(world.price, Some(Err(price::Error::Clock)));
}
#[when("the supplied pair residual rises from zero to twenty price ticks")]
fn evaluate_pair(world: &mut CatalogWorld) {
    let rows: Vec<_> = [100, 100, 120]
        .into_iter()
        .enumerate()
        .map(|(i, left)| {
            let i = i64::try_from(i).expect("bounded index");
            observed(
                (i + 1) * HOUR,
                panel::PairBar::new(
                    Symbol::new("AAA").expect("symbol"),
                    Symbol::new("BBB").expect("symbol"),
                    bar(i, left, 10),
                    bar(i, 100, 10),
                )
                .expect("synchronized pair"),
            )
        })
        .collect();
    world.pair = Some(
        match catalog::evaluate(
            InputUniverse::SuppliedOffline,
            Request::Pairs {
                rule: panel::PairRule::new(2, 100_000_000, 10).expect("frozen beta"),
                window: &window(&rows),
            },
        ) {
            Ok(Output::Pairs(result)) => Ok(result),
            Err(catalog::Error::Panel(error)) => Err(error),
            other => panic!("wrong catalogue pair route: {other:?}"),
        },
    );
}
#[then("the pair assessment shorts left and buys right with the frozen unit hedge")]
fn pair_direction(world: &mut CatalogWorld) {
    assert_eq!(
        world.pair,
        Some(Ok(panel::PairAssessment::Ready {
            left: price::Direction::Short,
            right: price::Direction::Long,
            hedge_ratio_ticks: 100_000_000
        }))
    );
}
fn synthetic_panel() -> panel::Panel {
    panel::Panel::new(
        UtcMicros::from_micros(1),
        vec![
            panel::PanelRow::new(Symbol::new("AAA").expect("symbol"), [10, -10, 0, 0]),
            panel::PanelRow::new(Symbol::new("BBB").expect("symbol"), [0; 4]),
            panel::PanelRow::new(Symbol::new("CCC").expect("symbol"), [-10, 10, 0, 0]),
        ],
    )
    .expect("unique bounded panel")
}
#[when(expr = "the supplied panel is evaluated by {word}")]
fn evaluate_panel(world: &mut CatalogWorld, family: String) {
    let row = observed(1, synthetic_panel());
    let view = row.causal_at(at(1)).expect("causal snapshot");
    let weights = panel::FactorWeights::new([0, 1, 0, 0]).expect("loadings");
    let request = match family.as_str() {
        "cross_sectional_alpha" => Request::CrossSection {
            panel: &view,
            decision: at(1),
            per_side: 1,
        },
        "factor_investing" => Request::Factor {
            panel: &view,
            decision: at(1),
            weights: &weights,
            per_side: 1,
        },
        _ => panic!("unknown panel family"),
    };
    world.allocation = Some(
        match catalog::evaluate(InputUniverse::SuppliedOffline, request) {
            Ok(Output::Panel(result)) => Ok(result),
            Err(catalog::Error::Panel(error)) => Err(error),
            other => panic!("wrong catalogue panel route: {other:?}"),
        },
    );
}
#[then(expr = "the exact panel is long {word} and short {word}")]
fn allocation(world: &mut CatalogWorld, long: String, short: String) {
    let allocation = world
        .allocation
        .as_ref()
        .expect("evaluated")
        .as_ref()
        .expect("admitted");
    let actual: Vec<_> = allocation
        .legs()
        .iter()
        .map(|l| (l.symbol().as_str(), l.weight_ticks()))
        .collect();
    assert_eq!(
        actual,
        vec![(long.as_str(), 100_000_000), (short.as_str(), -100_000_000)]
    );
}
#[when("a later causal panel view is reused for an earlier decision")]
fn earlier(world: &mut CatalogWorld) {
    let row = observed(2, synthetic_panel());
    world.allocation = Some(panel::rank(
        &row.causal_at(at(2)).expect("later gate"),
        at(1),
        1,
    ));
}
#[then("the panel is refused with Future")]
fn panel_future(world: &mut CatalogWorld) {
    assert_eq!(world.allocation, Some(Err(panel::Error::Future)));
}

#[tokio::main]
async fn main() {
    CatalogWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/features/catalog_price_panel.feature"),
        )
        .await;
}
