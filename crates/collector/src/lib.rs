//! Collector boundaries for Binance USD-M market data.
//!
//! The Vision path downloads public `data.binance.vision` objects and the
//! public S3 listing. It does not sign requests, read API keys, or open a
//! market-data WebSocket. REST and WebSocket modules stay boundaries.

#[cfg(test)]
mod pipeline_test;

pub mod civil;
pub mod error;
pub mod external;
pub mod fetch;
pub mod manifest;
pub mod normalize;
pub mod rest;
pub mod run;
pub mod scale;
pub mod segment;
pub mod store;
pub mod vision;
pub mod ws;

pub use error::Error;

pub(crate) fn u64_from_usize(value: usize) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| Error::check("overflow", "count does not fit in u64"))
}
