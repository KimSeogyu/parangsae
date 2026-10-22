//! Dated BTCUSDT USD-M instrument rules.
//!
//! Look up `(symbol, field, at)`. There is no network and no file I/O in this
//! crate. Values are scale-`10^8` decimals, never `f64`. Tick size is its own
//! field: nothing in this module derives it from `pricePrecision`.
//!
//! Unknown bounds are `Boundary::Bracketed`. The default policy is conservative:
//! higher fee, smaller max quantity, larger min notional, coarser tick or step.
//!
//! # Resolved ambiguities
//!
//! * Bracket endpoints for the two exchangeInfo snapshots are the `serverTime`
//!   millisecond instants. The CSV truncates those to seconds. Using the
//!   truncated second as the bracket edge would put the snapshot itself inside
//!   the unresolved window.
//! * FAQ brackets use the 14-digit Wayback capture time in the source URL.
//!   `UNKNOWN(<=2021-01-28)` is `(listing, 2021-01-28 06:41:21)`, not the
//!   whole civil day. The same shape is used for the BNB discount, which the
//!   CSV marks unknown and section (c)'s two-window example does not name.
//! * The min-notional "by 10:00" and the 2026 rollout stay exact CSV instants.
//!   They are listed in [`UNRESOLVED_ITEMS`] and are not widened into brackets.
//! * `verified-doc+verified-archive` on the 5 USDT notional is stored as
//!   [`Confidence::VerifiedArchive`]: a 2022-03-18 snapshot shows `"notional":"5"`.
//! * After a tick coarsening, resting-order prints may use the old tick for
//!   two UTC days ([`TICK_COARSEN_GRACE_US`]). The note reports prints on
//!   2022-02-15 and 2022-02-16 and does not give a longer window. Orders
//!   constructed after the switch use the new tick immediately.
//! * `Decimal` is the crate's scale-`10^8` newtype, not `rust_decimal`.
//!   `UtcDateTime` wraps [`crate::UtcMicros`]. Core takes no extra crates.

mod clock;
mod decimal;
mod guard;
mod lookup;
mod table;
mod types;

pub use clock::{UtcDate, UtcDateTime};
pub use decimal::{Decimal, DecimalError};
pub use guard::{
    audit_continuity, audit_grid, audit_provenance, audit_snapshot, check_paper_order,
    classify_print, is_paper_blocked, ContinuityError, GridError, GridReport, GridSample,
    OrderError, PaperOrder, PrintFit, ProvenanceError, SnapshotError, SnapshotFields,
};
pub use lookup::{lookup, rule_at, LookupError};
pub use table::{
    BACKTEST_START, BACKTEST_START_US, BNB_DISCOUNT_START, BTCUSDT_RULES, FEE_BNB, FEE_FAQ_2020,
    FEE_FAQ_2020_US, FEE_FAQ_2021, FEE_FAQ_2021_US, FEE_FAQ_2023, FEE_FAQ_2023_US, FEE_FAQ_2024,
    FEE_FAQ_2024_US, FEE_MAKER, FEE_TAKER_4, FEE_TAKER_5, HOURS_8, KNOWN_COVERAGE_GAPS, LISTING,
    LISTING_US, MARKET_MAX_CHANGE, NOTIONAL_1, NOTIONAL_100, NOTIONAL_100_AT, NOTIONAL_1_AT,
    NOTIONAL_5, NOTIONAL_50, NOTIONAL_50_AT, NOTIONAL_5_AT, ONBOARD_MS, PAPER_BLOCKED, PRECISION_2,
    PRECISION_3, QTY_1000, QTY_120, STEP_0_001, TAKER_FEE_CHANGE, TICK_0_01, TICK_0_10,
    TICK_COARSEN_GRACE_DAYS, TICK_COARSEN_GRACE_US, TICK_SWITCH, TICK_SWITCH_US, UNRESOLVED_ITEMS,
    VIP0_FEE_START, WB2021, WB2021_US, WB2022, WB2022_US,
};
pub use types::{
    Boundary, BracketPolicy, Confidence, CoverageGap, Field, InstrumentRule, PaperBlock, RuleValue,
    UnresolvedItem,
};
