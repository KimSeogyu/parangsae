//! Collector boundaries for Binance USD-M market data.
//!
//! There is no HTTP or WebSocket client in this scaffold. Modules name the
//! stages from the mid-frequency data plan so later work has a place to land:
//! manifest, declared clock, Vision bulk, public REST, public WS, sequenced
//! segment, and external venues.

#![forbid(unsafe_code)]

pub mod external;
pub mod manifest;
pub mod normalize;
pub mod rest;
pub mod segment;
pub mod vision;
pub mod ws;
