//! Fixed-point accounting for one USD-M position.
//!
//! Prices, quantities, fees, funding, and cash are integers at scale `10^8`.
//! Products are computed in `i128`. A result that does not fit in `i64` is
//! [`SimError::Overflow`], never a wrapped value.
//!
//! This crate denies `clippy::float_arithmetic` and `clippy::cast_precision_loss`.
//! A float in the money path fails CI. [`Costs`] has no [`Default`]: zero fee
//! and zero slippage are explicit inputs.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::cast_precision_loss)]

mod ledger;
mod round;
mod types;

pub use ledger::{taker_fee, Fill, FundingEvent, Ledger};
pub use round::round_against_trader;
pub use types::{Cash, Costs, FeeBps, MarketOrder, PositionQty, Side, SimError, SlippageBps};

const _: () = assert!(parangsae_core::SCALE == 100_000_000);
