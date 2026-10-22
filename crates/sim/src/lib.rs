//! Fixed-point accounting for one USD-M position, and the paper clock.
//!
//! Prices, quantities, fees, funding, and cash are integers at scale `10^8`.
//! Products are computed in `i128`. A result that does not fit in `i64` is
//! [`SimError::Overflow`], never a wrapped value.
//!
//! This crate denies `clippy::float_arithmetic` and `clippy::cast_precision_loss`.
//! A float in the money path fails CI. [`Costs`] has no [`Default`]: zero fee
//! and zero slippage are explicit inputs.
//!
//! `decision_at` is the only public mint of [`DecisionTime`]. The `driver-clock`
//! feature on `parangsae-core` is enabled by this crate's default feature and
//! by no other crate's normal build. This crate does not submit an order.

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

#[cfg(feature = "driver-clock")]
use parangsae_core::{DecisionTime, UtcMicros};

/// Mints a decision time. Downstream crates call this instead of `from_micros`.
#[cfg(feature = "driver-clock")]
pub struct Driver;

#[cfg(feature = "driver-clock")]
impl Driver {
    pub const fn decision_at(ts: UtcMicros) -> DecisionTime {
        DecisionTime::from_micros(ts)
    }
}
