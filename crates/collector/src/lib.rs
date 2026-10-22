//! Collector boundaries for Binance USD-M market data.
//!
//! The Vision path downloads public `data.binance.vision` objects and the
//! public S3 listing. It does not sign requests, read API keys, or open a
//! market-data WebSocket. REST and WebSocket modules stay boundaries.

#![forbid(unsafe_code)]

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
pub mod version;
pub mod vision;
pub mod ws;

pub use error::Error;
