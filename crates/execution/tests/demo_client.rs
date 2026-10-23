use parangsae_core::{Symbol, UtcMicros};
use parangsae_execution::demo::{Credentials, DemoClient, DemoError, Method, Reply, RequestTiming};
use parangsae_execution::{Attempt, ClientOrderId, IntentId, NotFoundAuthority, OrderState};
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/demo/responses.json")).unwrap()
}

fn timing() -> RequestTiming {
    RequestTiming::new(
        UtcMicros::from_micros(1_700_000_000_000_000),
        NotFoundAuthority::new(5000, 100).unwrap(),
    )
    .unwrap()
}

#[test]
fn recorded_requests_keep_the_same_id_and_sign_exact_bytes() {
    let f = fixture();
    let credentials = Credentials::new(
        f["api_key"].as_str().unwrap(),
        f["signing_bytes"].as_str().unwrap().as_bytes(),
    )
    .unwrap();
    let id = ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST);
    let symbol = Symbol::new("BTCUSDT").unwrap();
    let client = DemoClient::new();
    let first = client.query_order(&symbol, &id, timing(), &credentials);
    let again = client.query_order(&symbol, &id, timing(), &credentials);
    let cancel = client.cancel_order(&symbol, &id, timing(), &credentials);
    assert_eq!(first.method(), Method::Get);
    assert_eq!(cancel.method(), Method::Delete);
    assert_eq!(first.path(), "/fapi/v1/order");
    assert_eq!(first.query(), again.query());
    assert_eq!(first.unsigned_query(), f["query"].as_str().unwrap());
    assert_eq!(first.query(), f["query_signed"].as_str().unwrap());
    assert_eq!(cancel.unsigned_query(), f["cancel"].as_str().unwrap());
    assert_eq!(cancel.query(), f["cancel_signed"].as_str().unwrap());
    assert_eq!(
        first.api_key_header(),
        ("X-MBX-APIKEY", "fixture-only-public-id")
    );
    let redacted = format!("{first:?} {credentials:?}");
    assert!(!redacted.contains(f["api_key"].as_str().unwrap()));
    assert!(!redacted.contains(f["signing_bytes"].as_str().unwrap()));
    assert!(!redacted.contains(first.query()));
}

#[test]
fn parses_fixed_point_report_and_binds_it_to_the_request() {
    let f = fixture();
    let credentials = Credentials::new("fixture", b"fixture").unwrap();
    let id = ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST);
    let request = DemoClient::new().query_order(
        &Symbol::new("BTCUSDT").unwrap(),
        &id,
        timing(),
        &credentials,
    );
    let report = match request
        .parse_response(200, &f["order"].to_string())
        .unwrap()
    {
        Reply::Order(report) => report,
        other => panic!("expected order, got {other:?}"),
    };
    assert_eq!(report.client_order_id, id);
    assert_eq!(report.state, OrderState::Filled);
    assert_eq!(report.qty.ticks(), 100_000);
    assert_eq!(report.cumulative.ticks(), 100_000);
    assert_eq!(
        report.average_price.map(|price| price.ticks()),
        Some(3_000_012_345_678)
    );
    assert_eq!(report.updated_at.micros().micros(), 1_700_000_000_123_000);

    for (field, value) in [
        ("symbol", json!("ETHUSDT")),
        (
            "clientOrderId",
            json!(ClientOrderId::from_parts(IntentId::from_raw(2), Attempt::FIRST).as_str()),
        ),
    ] {
        let mut wrong = f["order"].clone();
        wrong[field] = value;
        assert_eq!(
            request.parse_response(200, &wrong.to_string()),
            Err(DemoError::IdentityMismatch)
        );
    }
}

#[test]
fn incomplete_or_unknown_reports_fail_closed() {
    let f = fixture();
    let credentials = Credentials::new("fixture", b"fixture").unwrap();
    let request = DemoClient::new().query_order(
        &Symbol::new("BTCUSDT").unwrap(),
        &ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST),
        timing(),
        &credentials,
    );
    for (field, value) in [
        ("status", json!("FUTURE_STATUS")),
        ("type", json!("STOP_MARKET")),
        ("type", json!("LIMIT")),
        ("positionSide", json!("LONG")),
        ("executedQty", json!("0.002")),
        ("origQty", json!("0")),
        ("executedQty", json!("0.000000001")),
        ("executedQty", serde_json::from_str("0.001").unwrap()),
        ("avgPrice", json!("NaN")),
        ("price", json!("-1")),
        ("price", json!("1e3")),
        ("avgPrice", json!("9223372036854775808")),
        ("origQty", json!(" 0.001")),
        ("orderId", json!(0)),
        ("updateTime", json!(u64::MAX)),
        ("reduceOnly", json!("false")),
    ] {
        let mut bad = f["order"].clone();
        bad[field] = value;
        assert_eq!(
            request.parse_response(200, &bad.to_string()),
            Err(DemoError::InvalidResponse),
            "{field}"
        );
    }
    for body in ["{}", "{", "[]", "null"] {
        assert_eq!(
            request.parse_response(200, body),
            Err(DemoError::InvalidResponse)
        );
    }
    let original = f["order"].to_string();
    let duplicate = format!(
        "{},\"symbol\":\"BTCUSDT\"}}",
        original.trim_end_matches('}')
    );
    assert_eq!(
        request.parse_response(200, &duplicate),
        Err(DemoError::InvalidResponse)
    );
    assert_eq!(
        request.parse_response(200, &format!("{original}{}", " ".repeat(65_536))),
        Err(DemoError::InvalidResponse)
    );
}

#[test]
fn reports_keep_partial_and_terminal_cumulative_quantities() {
    let f = fixture();
    let credentials = Credentials::new("fixture", b"fixture").unwrap();
    let request = DemoClient::new().query_order(
        &Symbol::new("BTCUSDT").unwrap(),
        &ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST),
        timing(),
        &credentials,
    );
    for (status, qty, expected) in [
        ("NEW", "0", OrderState::Acked),
        ("PARTIALLY_FILLED", "0.0005", OrderState::PartiallyFilled),
        ("CANCELED", "0.0005", OrderState::Canceled),
        ("EXPIRED", "0", OrderState::Expired),
        ("EXPIRED_IN_MATCH", "0", OrderState::Expired),
        ("REJECTED", "0", OrderState::Rejected),
    ] {
        let mut wire = f["order"].clone();
        wire["status"] = json!(status);
        wire["executedQty"] = json!(qty);
        if qty == "0" {
            wire["avgPrice"] = json!("0");
        }
        let Reply::Order(report) = request.parse_response(200, &wire.to_string()).unwrap() else {
            panic!("expected snapshot");
        };
        assert_eq!(report.state, expected);
        assert_eq!(
            report.cumulative.ticks(),
            if qty == "0" { 0 } else { 50_000 }
        );
    }
}

#[test]
fn ambiguous_failures_and_absence_are_never_success_or_retry() {
    let f = fixture();
    let credentials = Credentials::new("fixture", b"fixture").unwrap();
    let symbol = Symbol::new("BTCUSDT").unwrap();
    let id = ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST);
    let query = DemoClient::new().query_order(&symbol, &id, timing(), &credentials);
    let cancel = DemoClient::new().cancel_order(&symbol, &id, timing(), &credentials);
    assert_eq!(
        query.parse_response(400, &f["not_found"].to_string()),
        Ok(Reply::NotFound)
    );
    assert_eq!(
        cancel.parse_response(400, &f["not_found"].to_string()),
        Ok(Reply::Unknown)
    );
    assert_eq!(
        cancel.parse_response(400, &f["cancel_unknown"].to_string()),
        Ok(Reply::Unknown)
    );
    assert_eq!(
        query.parse_response(400, &f["timeout"].to_string()),
        Ok(Reply::Unknown)
    );
    assert_eq!(
        query.parse_response(503, "<html>unavailable</html>"),
        Ok(Reply::Unknown)
    );
    assert_eq!(query.parse_response(429, ""), Ok(Reply::RateLimited));
    assert_eq!(query.parse_response(418, ""), Ok(Reply::Banned));
    assert_eq!(
        query.parse_response(400, &f["rejected"].to_string()),
        Ok(Reply::Refused { code: -1021 })
    );
}

#[test]
fn cancel_success_without_average_price_preserves_snapshot() {
    let f = fixture();
    let credentials = Credentials::new("fixture", b"fixture").unwrap();
    let symbol = Symbol::new("BTCUSDT").unwrap();
    let id = ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST);
    let client = DemoClient::new();
    let cancel = client.cancel_order(&symbol, &id, timing(), &credentials);
    let query = client.query_order(&symbol, &id, timing(), &credentials);
    for executed in ["0", "0.0005"] {
        let mut wire = f["order"].clone();
        wire["type"] = json!("LIMIT");
        wire["status"] = json!("CANCELED");
        wire["price"] = json!("30000");
        wire["executedQty"] = json!(executed);
        wire.as_object_mut().unwrap().remove("avgPrice");
        let body = wire.to_string();
        assert_eq!(
            query.parse_response(200, &body),
            Err(DemoError::InvalidResponse)
        );
        let Reply::Order(report) = cancel.parse_response(200, &body).unwrap() else {
            panic!("expected canceled snapshot");
        };
        assert_eq!(report.state, OrderState::Canceled);
        assert_eq!(report.average_price, None);
        assert_eq!(
            report.cumulative.ticks(),
            if executed == "0" { 0 } else { 50_000 }
        );
        for average_price in [serde_json::Value::Null, json!(30000)] {
            wire["avgPrice"] = average_price;
            assert_eq!(
                cancel.parse_response(200, &wire.to_string()),
                Err(DemoError::InvalidResponse)
            );
        }
    }
}

#[test]
fn timestamp_and_header_inputs_are_checked_without_a_clock_or_environment() {
    let authority = NotFoundAuthority::new(5000, 100).unwrap();
    for micros in [-1, 1_700_000_000_000_001] {
        assert_eq!(
            RequestTiming::new(UtcMicros::from_micros(micros), authority),
            Err(DemoError::InvalidTime)
        );
    }
    for key in ["", "fixture\r\nInjected: yes", "fixture key"] {
        assert!(Credentials::new(key, b"fixture").is_err());
    }
    assert!(Credentials::new("fixture", b"").is_err());
}
