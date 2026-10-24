//! Offline USD-M demo protocol codec. No transport or URL resolution exists.
//!
//! The existing sealed `Venue` remains a refusing paper stub. The real risk
//! crate and its `ApprovedOrder` do not exist in this tree, so this module
//! exposes no order-submit API. New-order encoding is compiled only for unit
//! fixtures. Binding it to `Venue` requires the real risk token and a current
//! risk-gate check; a paper order is not approval.
//!
//! ```compile_fail
//! use parangsae_execution::demo::DemoClient;
//! use parangsae_execution::PaperOrder;
//! fn bypass(order: PaperOrder) {
//!     DemoClient::new().submit(order);
//! }
//! ```
//!
//! The order method does not exist in a production build, including release:
//!
//! ```compile_fail
//! let _ = parangsae_execution::demo::Method::Post;
//! ```

mod response;

use std::fmt;

use parangsae_core::{Symbol, UtcMicros};
use sha2::{Digest, Sha256};

use crate::{ClientOrderId, NotFoundAuthority};

pub use response::{OrderReport, Reply};

/// Only a demo environment marker. It cannot select a caller-provided host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    UsdMDemo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Delete,
    #[cfg(test)]
    Post,
}

/// Explicit caller-injected signing material. Never reads a file or a clock.
/// Debug formatting omits both values. Do not use real material in fixtures.
pub struct Credentials<'a> {
    api_key: &'a str,
    signing_bytes: &'a [u8],
}

impl<'a> Credentials<'a> {
    pub fn new(api_key: &'a str, signing_bytes: &'a [u8]) -> Result<Self, DemoError> {
        if api_key.is_empty()
            || !api_key.bytes().all(|byte| byte.is_ascii_graphic())
            || signing_bytes.is_empty()
        {
            return Err(DemoError::InvalidCredentials);
        }
        Ok(Self {
            api_key,
            signing_bytes,
        })
    }
}

impl fmt::Debug for Credentials<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credentials([redacted])")
    }
}

/// Injected UTC time, exactly representable in wire milliseconds. recvWindow
/// comes from the same authority bound used by the lifecycle's NotFound rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestTiming {
    timestamp_ms: i64,
    recv_window_ms: u32,
}

impl RequestTiming {
    pub fn new(now: UtcMicros, authority: NotFoundAuthority) -> Result<Self, DemoError> {
        let micros = now.micros();
        if micros < 0 || micros % 1000 != 0 {
            return Err(DemoError::InvalidTime);
        }
        Ok(Self {
            timestamp_ms: micros / 1000,
            recv_window_ms: authority.recv_window_ms(),
        })
    }
}

/// Immutable request data, with identity retained for response binding.
/// There is deliberately no `send`, arbitrary path, host, or method setter.
pub struct SignedRequest {
    method: Method,
    symbol: Symbol,
    client_order_id: ClientOrderId,
    unsigned_query: String,
    query: String,
    api_key: String,
}

impl SignedRequest {
    pub const fn environment(&self) -> Environment {
        Environment::UsdMDemo
    }

    pub const fn method(&self) -> Method {
        self.method
    }

    pub const fn path(&self) -> &'static str {
        "/fapi/v1/order"
    }

    pub fn unsigned_query(&self) -> &str {
        &self.unsigned_query
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn api_key_header(&self) -> (&'static str, &str) {
        ("X-MBX-APIKEY", &self.api_key)
    }
}

impl fmt::Debug for SignedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedRequest")
            .field("environment", &self.environment())
            .field("method", &self.method)
            .field("path", &self.path())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct DemoClient;

impl DemoClient {
    pub const fn new() -> Self {
        Self
    }

    pub fn query_order(
        &self,
        symbol: &Symbol,
        id: &ClientOrderId,
        timing: RequestTiming,
        credentials: &Credentials<'_>,
    ) -> SignedRequest {
        self.order_request(Method::Get, symbol, id, timing, credentials)
    }

    /// Cancels need no risk approval. This returns data and performs no I/O.
    pub fn cancel_order(
        &self,
        symbol: &Symbol,
        id: &ClientOrderId,
        timing: RequestTiming,
        credentials: &Credentials<'_>,
    ) -> SignedRequest {
        self.order_request(Method::Delete, symbol, id, timing, credentials)
    }

    fn order_request(
        &self,
        method: Method,
        symbol: &Symbol,
        id: &ClientOrderId,
        timing: RequestTiming,
        credentials: &Credentials<'_>,
    ) -> SignedRequest {
        // Both newtypes contain only unreserved ASCII, so neither can inject
        // query delimiters. The returned wire bytes are exactly the signed bytes.
        let params = format!("symbol={symbol}&origClientOrderId={id}");
        signed_request(method, symbol, id, params, timing, credentials)
    }
}

fn signed_request(
    method: Method,
    symbol: &Symbol,
    id: &ClientOrderId,
    params: String,
    timing: RequestTiming,
    credentials: &Credentials<'_>,
) -> SignedRequest {
    let unsigned_query = format!(
        "{params}&recvWindow={}&timestamp={}",
        timing.recv_window_ms, timing.timestamp_ms
    );
    let signature = hex(&hmac_sha256(
        credentials.signing_bytes,
        unsigned_query.as_bytes(),
    ));
    let query = format!("{unsigned_query}&signature={signature}");
    SignedRequest {
        method,
        symbol: symbol.clone(),
        client_order_id: id.clone(),
        unsigned_query,
        query,
        api_key: credentials.api_key.to_owned(),
    }
}

/// RFC 2104 composition over the existing SHA-256 dependency. Fixture vectors
/// cover short keys and keys longer than SHA-256's 64-byte block.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for ((inner, outer), byte) in inner_pad.iter_mut().zip(&mut outer_pad).zip(block) {
        *inner ^= byte;
        *outer ^= byte;
    }
    let inner = Sha256::new()
        .chain_update(inner_pad)
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(outer_pad)
        .chain_update(inner)
        .finalize()
        .into()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoError {
    InvalidCredentials,
    InvalidTime,
    InvalidResponse,
    IdentityMismatch,
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidCredentials => "empty signing material or invalid header value",
            Self::InvalidTime => "timestamp must be nonnegative whole milliseconds",
            Self::InvalidResponse => "invalid or unsupported demo response",
            Self::IdentityMismatch => "response belongs to another order",
        })
    }
}

impl std::error::Error for DemoError {}

#[cfg(test)]
mod tests;
