//! Causal targets and one causal feature for BTCUSDT bars.
//!
//! `tsmom` and `funding_z` emit a [`SignalOutput`]. `volregime` emits a
//! [`FeatureOutput`] and does not implement [`CausalSignal`]. Every read goes
//! through [`parangsae_core::Causal`], which exists only when
//! `available_ts <= decision`.
//!
//! The proptests in this crate sample invariants. They are not a proof.

#![forbid(unsafe_code)]

mod bars;
mod feature;
mod funding;
mod funding_z;
mod output;
pub mod premium_input;
mod rank;
mod target;
mod tsmom;
mod volregime;
mod window;

pub use feature::FeatureOutput;
pub use funding::{funding_observation, FundingRow};
pub use funding_z::{drive_funding_z, FundingZ, PROD_MAX_AGE_US};
pub use output::{Decision, FlatReason, ParamError, SignalError, SignalOutput};
pub use rank::{Rank, RankError, RANK_SCALE};
pub use target::{Target, TargetError, TARGET_SCALE};
pub use tsmom::Tsmom;
pub use volregime::VolRegime;
pub use window::{drive, drive_feature, CausalFeature, CausalSignal, CausalWindow, WindowError};

/// Target signals the engine may trade.
pub const TARGET_IDS: &[&str] = &["tsmom", "funding_z"];

/// Features. The engine must not treat these as a position.
pub const FEATURE_IDS: &[&str] = &["volregime"];

/// Every id this crate ships. `TARGET_IDS` and `FEATURE_IDS` partition it.
pub const SIGNAL_IDS: &[&str] = &["tsmom", "volregime", "funding_z"];
