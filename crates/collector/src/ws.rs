//! Public market WebSocket routes after the 2026-04-23 split.
//!
//! `/private` (user data) is not a variant. This scaffold opens no socket.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicWsRoute {
    /// `wss://fstream.binance.com/public` — depth, bookTicker.
    Public,
    /// `wss://fstream.binance.com/market` — aggTrade, kline, markPrice, forceOrder.
    Market,
}
