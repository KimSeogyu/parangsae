use parangsae_core::instrument_spec::Decimal;
use parangsae_core::{require_declared_unit, EventTs, Price, Qty, Side, Symbol, TimeUnit};
use serde::{Deserialize, Deserializer};

use super::{DemoError, Method, SignedRequest};
use crate::{ClientOrderId, OrderState};

/// A validated snapshot, not a lifecycle event. The existing book decides
/// which event is legal; cumulative fills must never be treated as a delta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderReport {
    pub symbol: Symbol,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: u64,
    pub side: Side,
    pub state: OrderState,
    pub qty: Qty,
    pub cumulative: Qty,
    pub price: Price,
    /// Cancel snapshots may omit this value; no fill price is inferred.
    pub average_price: Option<Price>,
    pub reduce_only: bool,
    pub updated_at: EventTs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Order(OrderReport),
    /// Absence evidence only. The caller must apply NotFoundAuthority before
    /// it can retry with a fresh id; this codec never advances an attempt.
    NotFound,
    /// The request was refused, not proof that an existing order is Rejected.
    Refused {
        code: i64,
    },
    Unknown,
    RateLimited,
    Banned,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireOrder {
    symbol: String,
    client_order_id: String,
    order_id: u64,
    side: String,
    #[serde(rename = "type")]
    kind: String,
    status: String,
    orig_qty: String,
    executed_qty: String,
    price: String,
    #[serde(default, deserialize_with = "deserialize_average_price")]
    avg_price: Option<String>,
    reduce_only: bool,
    position_side: String,
    update_time: u64,
}

fn deserialize_average_price<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
struct WireError {
    code: i64,
    // Validate the error envelope without retaining or logging its raw message.
    #[serde(rename = "msg")]
    _message: String,
}

impl SignedRequest {
    pub fn parse_response(&self, status: u16, body: &str) -> Result<Reply, DemoError> {
        match status {
            418 => return Ok(Reply::Banned),
            429 => return Ok(Reply::RateLimited),
            408 | 500..=599 => return Ok(Reply::Unknown),
            _ => {}
        }
        if body.len() > 65_536 {
            return Err(DemoError::InvalidResponse);
        }
        if status != 200 {
            if !(400..=499).contains(&status) {
                return Err(DemoError::InvalidResponse);
            }
            let error: WireError =
                serde_json::from_str(body).map_err(|_| DemoError::InvalidResponse)?;
            return Ok(match error.code {
                -2013 if self.method == Method::Get => Reply::NotFound,
                -1021 | -1022 | -1100 | -1102 | -1111 | -2015 | -2019 | -2022 => {
                    Reply::Refused { code: error.code }
                }
                _ => Reply::Unknown,
            });
        }
        let wire: WireOrder = serde_json::from_str(body).map_err(|_| DemoError::InvalidResponse)?;
        let report = wire.validate(self.method)?;
        if report.symbol != self.symbol || report.client_order_id != self.client_order_id {
            return Err(DemoError::IdentityMismatch);
        }
        Ok(Reply::Order(report))
    }
}

impl WireOrder {
    fn validate(self, method: Method) -> Result<OrderReport, DemoError> {
        let invalid = DemoError::InvalidResponse;
        let symbol = Symbol::new(&self.symbol).map_err(|_| invalid)?;
        let client_order_id = ClientOrderId::parse(&self.client_order_id).map_err(|_| invalid)?;
        let qty = decimal(&self.orig_qty)?.as_qty().map_err(|_| invalid)?;
        let cumulative = decimal(&self.executed_qty)?.as_qty().map_err(|_| invalid)?;
        let price = decimal(&self.price)?.as_price().map_err(|_| invalid)?;
        let average_price = match self.avg_price {
            Some(raw) => Some(decimal(&raw)?.as_price().map_err(|_| invalid)?),
            None if method == Method::Delete => None,
            None => return Err(invalid),
        };
        if self.order_id == 0
            || self.position_side != "BOTH"
            || !matches!(self.kind.as_str(), "MARKET" | "LIMIT")
            || (self.kind == "LIMIT" && price.ticks() == 0)
            || qty.ticks() == 0
            || cumulative > qty
            || (cumulative.ticks() > 0 && average_price.is_some_and(|price| price.ticks() == 0))
        {
            return Err(invalid);
        }
        let state = match self.status.as_str() {
            "NEW" if cumulative.ticks() == 0 => OrderState::Acked,
            "PARTIALLY_FILLED" if cumulative.ticks() > 0 && cumulative < qty => {
                OrderState::PartiallyFilled
            }
            "FILLED" if cumulative == qty => OrderState::Filled,
            "CANCELED" => OrderState::Canceled,
            "EXPIRED" | "EXPIRED_IN_MATCH" => OrderState::Expired,
            "REJECTED" if cumulative.ticks() == 0 => OrderState::Rejected,
            _ => return Err(invalid),
        };
        let side = match self.side.as_str() {
            "BUY" => Side::Buy,
            "SELL" => Side::Sell,
            _ => return Err(invalid),
        };
        let millis = i64::try_from(self.update_time).map_err(|_| invalid)?;
        let updated_at = EventTs::from_micros(
            require_declared_unit(millis, TimeUnit::Millis).map_err(|_| invalid)?,
        );
        Ok(OrderReport {
            symbol,
            client_order_id,
            venue_order_id: self.order_id,
            side,
            state,
            qty,
            cumulative,
            price,
            average_price,
            reduce_only: self.reduce_only,
            updated_at,
        })
    }
}

fn decimal(raw: &str) -> Result<Decimal, DemoError> {
    if raw.trim() != raw {
        return Err(DemoError::InvalidResponse);
    }
    Decimal::parse(raw).map_err(|_| DemoError::InvalidResponse)
}
