//! One trade side for the whole workspace. Sim and backtest use this enum.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Buy => write!(f, "buy"),
            Self::Sell => write!(f, "sell"),
        }
    }
}
