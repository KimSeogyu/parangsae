use super::*;
use cucumber::{given, then, when, World as _};

#[derive(Debug, Default, cucumber::World)]
struct C1SizingWorld {
    input: Option<(CarryAccountState, EntryPrices)>,
    scenario: Option<Scenario>,
    observed: Option<Result<EntrySizing, SizingError>>,
}

#[given("a full-capital closed account whose current net spot close is below minimum")]
fn net_minimum_fixture(world: &mut C1SizingWorld) {
    let mut account = initial();
    account.spot_quote = Cash(2_510_100_000_000);
    account.perp_wallet = Cash(92_489_900_000_000);
    let mut prices = refs(10_000_000_000);
    prices.perp_reference = Price::from_ticks(10_100_000_000).expect("101USDT");
    prices.perp_risk = prices.perp_reference;
    world.input = Some((account, prices));
}

#[given("a richer perp wallet with asymmetric references near its fixed entry cap")]
fn fixed_cap_fixture(world: &mut C1SizingWorld) {
    let mut account = initial();
    account.perp_wallet = Cash(50_000_000_000_000);
    let mut prices = refs(10_000_000_000_000);
    prices.perp_reference = Price::from_ticks(11_149_000_000_000).expect("111490USDT");
    prices.perp_risk = prices.perp_reference;
    world.input = Some((account, prices));
}

#[given(expr = "the selected actual perp entry fee is {int} bps")]
fn actual_fee(world: &mut C1SizingWorld, fee: i64) {
    world.scenario = Some(match fee {
        5 => Scenario::Headline,
        6 => Scenario::HigherPerpFee,
        other => panic!("unselected test fee {other}"),
    });
}

#[when("the actual private C1 sizer evaluates those current terms")]
fn actual_quote(world: &mut C1SizingWorld) {
    let (account, prices) = world.input.as_ref().expect("declared current terms");
    let scenario = world.scenario.expect("declared selected scenario");
    world.observed = Some(quote_entry(account, *prices, scenario));
}

#[then("the actual result is Refused NoCompatibleSize rather than a sized pair")]
fn refused_quote(world: &mut C1SizingWorld) {
    assert_eq!(
        world.observed,
        Some(Ok(EntrySizing::Refused(Refusal::NoCompatibleSize)))
    );
}

#[then(expr = "the actual sized pair has gross {int} and net {int} BTC ticks")]
fn actual_quantities(world: &mut C1SizingWorld, gross: i64, net: i64) {
    let Some(Ok(EntrySizing::Sized(quote))) = world.observed.as_ref() else {
        panic!("expected a real sized result, got {:?}", world.observed);
    };
    assert_eq!(quote.gross_spot.ticks(), gross);
    assert_eq!(quote.net_btc.ticks(), net);
    assert_eq!(quote.net_btc.ticks() % SPOT_STEP, 0);
    assert_eq!(quote.net_btc.ticks() % PERP_STEP, 0);
}

#[then(
    expr = "its actual perp entry requirement excluding reserves is {int} cash ticks within the original cap"
)]
fn actual_all_in_requirement(world: &mut C1SizingWorld, expected: i64) {
    let Some(Ok(EntrySizing::Sized(quote))) = world.observed.as_ref() else {
        panic!("expected a real sized result, got {:?}", world.observed);
    };
    let entry = quote.perp_requirement.0 - PERP_CLOSE_AND_FUNDING;
    assert_eq!(entry, expected);
    assert!(entry <= EACH_CAP);
}

#[tokio::test]
async fn c1_sizing_admission_feature() {
    C1SizingWorld::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/c1_sizing_admission.feature"
        ))
        .await;
}
