//! Cash, position, and cost newtypes. `Costs` is never defaulted.

use std::fmt;

use parangsae_core::{Qty, Side};

/// Signed USDT at `10^8` ticks per whole unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cash(pub i64);

/// Signed base-asset lots at `10^8` per BTC. Negative is short.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositionQty(pub i64);

/// Taker fee in basis points. `5` is 5 bps = 0.0005.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeeBps(pub u32);

/// Fill slippage in basis points, applied against the trader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlippageBps(pub u32);

/// Explicit trading costs. There is no `Default`: a zero must be written down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Costs {
    pub taker_fee: FeeBps,
    pub slippage: SlippageBps,
}

/// Market order. `qty` must be positive; zero is rejected at the ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MarketOrder {
    pub side: Side,
    pub qty: Qty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimError {
    Overflow,
    Negative,
    ZeroQty,
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => write!(f, "fixed-point overflow"),
            Self::Negative => write!(f, "negative price or cash base"),
            Self::ZeroQty => write!(f, "order quantity is zero"),
        }
    }
}

impl std::error::Error for SimError {}
