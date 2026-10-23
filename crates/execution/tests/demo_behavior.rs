use std::path::Path;

use cucumber::{given, then, when, World as _};
use parangsae_core::{Symbol, UtcMicros};
use parangsae_execution::demo::{Credentials, DemoClient, DemoError, Reply, RequestTiming};
use parangsae_execution::{Attempt, ClientOrderId, IntentId, NotFoundAuthority, OrderState};

#[derive(Debug, Default, cucumber::World)]
struct DemoWorld {
    fixture: String,
    result: Option<Result<Reply, DemoError>>,
}

#[given(regex = r"^the recorded (\w+) fixture$")]
fn recorded(world: &mut DemoWorld, name: String) {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/demo/responses.json")).unwrap();
    world.fixture = fixtures[&name].to_string();
}

#[when(regex = r"^it is parsed with HTTP status (\d+)$")]
fn parse(world: &mut DemoWorld, status: u16) {
    let credentials = Credentials::new("fixture", b"fixture").unwrap();
    let request = DemoClient::new().query_order(
        &Symbol::new("BTCUSDT").unwrap(),
        &ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST),
        RequestTiming::new(
            UtcMicros::from_micros(1_700_000_000_000_000),
            NotFoundAuthority::new(5000, 100).unwrap(),
        )
        .unwrap(),
        &credentials,
    );
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
