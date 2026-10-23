//! New-order construction exists in the unit-test build only. There is no
//! production constructor until the real risk crate supplies ApprovedOrder.

use super::*;
use crate::{Attempt, IntentId, MaxAttempts};
use parangsae_core::{Price, Qty, Side};
use serde_json::Value;

struct FixtureOrder {
    symbol: Symbol,
    id: ClientOrderId,
    side: Side,
    qty: Qty,
    limit: Option<Price>,
    reduce_only: bool,
}

impl FixtureOrder {
    fn encode(
        &self,
        timing: RequestTiming,
        credentials: &Credentials<'_>,
    ) -> Result<SignedRequest, DemoError> {
        if self.qty.ticks() == 0 || self.limit.is_some_and(|price| price.ticks() == 0) {
            return Err(DemoError::InvalidResponse);
        }
        let side = match self.side {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        };
        let kind = if self.limit.is_some() {
            "LIMIT"
        } else {
            "MARKET"
        };
        let mut params = format!(
            "symbol={}&side={side}&type={kind}&quantity={}&reduceOnly={}&positionSide=BOTH&newClientOrderId={}&newOrderRespType=RESULT",
            self.symbol, fixed(self.qty.ticks()), self.reduce_only, self.id
        );
        if let Some(price) = self.limit {
            params.push_str(&format!("&price={}&timeInForce=GTC", fixed(price.ticks())));
        }
        Ok(signed_request(
            Method::Post,
            &self.symbol,
            &self.id,
            params,
            timing,
            credentials,
        ))
    }
}

fn fixed(ticks: i64) -> String {
    format!("{}.{:08}", ticks / 100_000_000, ticks % 100_000_000)
}

#[test]
fn hmac_matches_public_short_and_long_key_vectors() {
    let vectors: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/demo/hmac.json")).unwrap();
    for vector in vectors.as_array().unwrap() {
        let byte = u8::try_from(vector["key_byte"].as_u64().unwrap()).unwrap();
        let length = usize::try_from(vector["key_length"].as_u64().unwrap()).unwrap();
        let key = vec![byte; length];
        assert_eq!(
            hex(&hmac_sha256(
                &key,
                vector["message"].as_str().unwrap().as_bytes()
            )),
            vector["sha256"].as_str().unwrap()
        );
    }
}

#[test]
fn offline_new_order_fixture_uses_canonical_qty_signature_and_fresh_attempt_ids() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/demo/responses.json")).unwrap();
    let credentials = Credentials::new(
        fixture["api_key"].as_str().unwrap(),
        fixture["signing_bytes"].as_str().unwrap().as_bytes(),
    )
    .unwrap();
    let timing = RequestTiming::new(
        UtcMicros::from_micros(fixture["timestamp_us"].as_i64().unwrap()),
        NotFoundAuthority::new(5000, 100).unwrap(),
    )
    .unwrap();
    let mut order = FixtureOrder {
        symbol: Symbol::new("BTCUSDT").unwrap(),
        id: ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST),
        side: Side::Buy,
        qty: Qty::from_ticks(100_000).unwrap(),
        limit: None,
        reduce_only: false,
    };
    let first = order.encode(timing, &credentials).unwrap();
    assert_eq!(
        first.unsigned_query(),
        fixture["new_market"].as_str().unwrap()
    );
    assert_eq!(
        first.query(),
        fixture["new_market_signed"].as_str().unwrap()
    );
    assert_eq!(
        first.query(),
        order.encode(timing, &credentials).unwrap().query()
    );
    let next = Attempt::FIRST.next(MaxAttempts::new(2).unwrap()).unwrap();
    order.id = ClientOrderId::from_parts(order.id.intent(), next);
    let retry = order.encode(timing, &credentials).unwrap();
    assert_ne!(first.query(), retry.query());
    assert!(retry
        .unsigned_query()
        .contains(&format!("newClientOrderId={}", order.id)));
}

#[test]
fn fixed_point_limit_fixture_and_quantity_boundaries() {
    assert_eq!(fixed(i64::MAX), "92233720368.54775807");
    assert_eq!(fixed(1), "0.00000001");
    let credentials = Credentials::new("fixture", b"fixture").unwrap();
    let timing = RequestTiming::new(
        UtcMicros::from_micros(1_700_000_000_000_000),
        NotFoundAuthority::new(5000, 100).unwrap(),
    )
    .unwrap();
    let mut order = FixtureOrder {
        symbol: Symbol::new("BTCUSDT").unwrap(),
        id: ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST),
        side: Side::Sell,
        qty: Qty::from_ticks(100_000).unwrap(),
        limit: Some(Price::from_ticks(3_000_012_345_678).unwrap()),
        reduce_only: true,
    };
    let request = order.encode(timing, &credentials).unwrap();
    assert!(request
        .unsigned_query()
        .contains("side=SELL&type=LIMIT&quantity=0.00100000&reduceOnly=true"));
    assert!(request
        .unsigned_query()
        .contains("&price=30000.12345678&timeInForce=GTC&recvWindow=5000"));
    order.qty = Qty::from_ticks(0).unwrap();
    assert!(order.encode(timing, &credentials).is_err());
    order.qty = Qty::from_ticks(1).unwrap();
    order.limit = Some(Price::from_ticks(0).unwrap());
    assert!(order.encode(timing, &credentials).is_err());
}
