use std::path::Path;

use cucumber::{given, then, when, World as _};
use parangsae_core::{Symbol, UtcMicros};
use parangsae_execution::demo::{Credentials, DemoClient, DemoError, Reply, RequestTiming};
use parangsae_execution::{Attempt, ClientOrderId, IntentId, NotFoundAuthority, OrderState};

#[derive(Debug, Default, cucumber::World)]
struct DemoWorld {
    fixture: String,
    cancel: bool,
    result: Option<Result<Reply, DemoError>>,
}

#[given(regex = r"^the recorded (\w+) fixture$")]
fn recorded(world: &mut DemoWorld, name: String) {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/demo/responses.json")).unwrap();
    world.fixture = fixtures[&name].to_string();
}

#[given("a canceled snapshot without an average price")]
fn canceled_without_average_price(world: &mut DemoWorld) {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/demo/responses.json")).unwrap();
    let mut order = fixtures["order"].clone();
    order["type"] = serde_json::json!("LIMIT");
    order["status"] = serde_json::json!("CANCELED");
    order["price"] = serde_json::json!("30000");
    order["executedQty"] = serde_json::json!("0.0005");
    order.as_object_mut().unwrap().remove("avgPrice");
    world.fixture = order.to_string();
    world.cancel = true;
}

#[when(regex = r"^it is parsed with HTTP status (\d+)$")]
fn parse(world: &mut DemoWorld, status: u16) {
    let credentials = Credentials::new("fixture", b"fixture").unwrap();
    let symbol = Symbol::new("BTCUSDT").unwrap();
    let id = ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST);
    let timing = RequestTiming::new(
        UtcMicros::from_micros(1_700_000_000_000_000),
        NotFoundAuthority::new(5000, 100).unwrap(),
    )
    .unwrap();
    let client = DemoClient::new();
    let request = if world.cancel {
        client.cancel_order(&symbol, &id, timing, &credentials)
    } else {
        client.query_order(&symbol, &id, timing, &credentials)
    };
    world.result = Some(request.parse_response(status, &world.fixture));
}

#[then(regex = r"^the reply is ([a-z-]+)$")]
fn reply(world: &mut DemoWorld, expected: String) {
    let actual = match world
        .result
        .as_ref()
        .unwrap()
        .as_ref()
        .expect("valid fixture response")
    {
        Reply::Order(report) => {
            assert_eq!(report.state, OrderState::Filled);
            "filled"
        }
        Reply::NotFound => "not-found",
        Reply::Unknown => "unknown",
        Reply::Refused { .. } => "refused",
        Reply::RateLimited => "rate-limited",
        Reply::Banned => "banned",
    };
    assert_eq!(actual, expected);
}

#[then("the canceled snapshot retains cumulative quantity without a fill price")]
fn canceled_snapshot(world: &mut DemoWorld) {
    let Some(Ok(Reply::Order(report))) = &world.result else {
        panic!("expected canceled snapshot");
    };
    assert_eq!(report.state, OrderState::Canceled);
    assert_eq!(report.cumulative.ticks(), 50_000);
    assert_eq!(report.average_price, None);
}

#[test]
fn offline_demo_behavior() {
    futures::executor::block_on(
        DemoWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/demo/client.feature"
            ))),
    );
}
