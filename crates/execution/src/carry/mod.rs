//! Pure synthetic two-wallet carry. No bar oracle, account access or order transport.
//!
//! A session allocates one capital pool to funded spot, isolated futures and
//! idle cash. The engine owns both wallets and consumes its private leg
//! authority. A short never credits sale notional to spendable cash. Fees,
//! grids and margin are explicit scenario inputs, not verified account terms.
//! The first scope supports BTC or USDT spot-buy fees and USDT spot-sell fees.
//!
//! Spot execution and its complete priced cut must be visible before a new
//! hedge decision. The short must have a complete flat cut before spot exit.
//! Partial fee dust remains owned; it is never rounded away to claim a hedge.
//!
//! A rate-free boundary captures the known short quantity at settlement.
//! The later visible receipt uses that retained quantity and preserves its
//! economic timestamp. Decision cash recognizes the admitted receipt; this
//! module does not reconstruct intraminute historical margin or liquidity.
//!
//! An unsupported dispatched fact remains in the snapshot with Unresolved
//! state and retained liabilities. Its account values contain only applied
//! facts and are not a complete PnL sample or automatic recovery result.
//! Fresh reconstruction replays original data commands; no approval is saved.

mod engine;
mod types;

pub use engine::CarryEngine;
pub use types::*;
