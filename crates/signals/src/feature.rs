//! Volatility rank as a feature. It is not a target and not a signal output.
//!
//! ```compile_fail,E0277
//! use parangsae_core::{BarInterval, DecisionTime, Observation, ResampledBar, UtcMicros};
//! use parangsae_signals::{drive, VolRegime};
//! let feature = VolRegime::<2, 3>::new(BarInterval::H1).unwrap();
//! let rows: Vec<Observation<ResampledBar>> = Vec::new();
//! let decision = DecisionTime::from_micros(UtcMicros::from_micros(0));
//! let _ = drive(&feature, &rows, decision);
//! ```
//!
//! ```
//! use parangsae_core::{BarInterval, DecisionTime, Observation, ResampledBar, UtcMicros};
//! use parangsae_signals::{drive, Tsmom};
//! let signal = Tsmom::<3>::new(BarInterval::H1, 0.05).unwrap();
//! let rows: Vec<Observation<ResampledBar>> = Vec::new();
//! let decision = DecisionTime::from_micros(UtcMicros::from_micros(0));
//! let _ = drive(&signal, &rows, decision);
//! ```
//!
//! ```compile_fail,E0277
//! use parangsae_signals::{Rank, Target};
//! let rank = Rank::of(1.0, &[0.0]).unwrap();
//! let _via_into: Target = rank.into();
//! let _via_from = Target::from(rank);
//! ```
//!
//! ```
//! use parangsae_signals::Rank;
//! let rank = Rank::of(1.0, &[0.0]).unwrap();
//! let ticks: i64 = rank.ticks();
//! assert_eq!(ticks, 100_000_000);
//! ```
//!
//! ```compile_fail,E0277
//! use parangsae_core::{BarInterval, DecisionTime, UtcMicros};
//! use parangsae_signals::{FeatureOutput, Rank, SignalOutput};
//! let rank = Rank::of(1.0, &[0.0]).unwrap();
//! let feature_out = FeatureOutput {
//!     decision_ts: DecisionTime::from_micros(UtcMicros::from_micros(0)),
//!     bar_close_time_us: UtcMicros::from_micros(0),
//!     interval: BarInterval::H1,
//!     value: rank,
//! };
//! let _out: SignalOutput = feature_out.into();
//! ```
//!
//! ```
//! use parangsae_core::{BarInterval, DecisionTime, UtcMicros};
//! use parangsae_signals::{FeatureOutput, Rank};
//! let rank = Rank::of(0.0, &[1.0]).unwrap();
//! let feature_out = FeatureOutput {
//!     decision_ts: DecisionTime::from_micros(UtcMicros::from_micros(0)),
//!     bar_close_time_us: UtcMicros::from_micros(0),
//!     interval: BarInterval::H1,
//!     value: rank,
//! };
//! let copied: FeatureOutput = feature_out;
//! assert_eq!(copied.value.ticks(), feature_out.value.ticks());
//! ```
//!
//! ```compile_fail,E0308
//! use parangsae_core::{BarInterval, DecisionTime, UtcMicros};
//! use parangsae_signals::{FeatureOutput, Rank, SignalOutput};
//! let rank = Rank::of(1.0, &[0.0]).unwrap();
//! let feature_out = FeatureOutput {
//!     decision_ts: DecisionTime::from_micros(UtcMicros::from_micros(0)),
//!     bar_close_time_us: UtcMicros::from_micros(0),
//!     interval: BarInterval::H1,
//!     value: rank,
//! };
//! fn engine_accepts(_: SignalOutput) {}
//! engine_accepts(feature_out);
//! ```
//!
//! ```
//! use parangsae_core::{BarInterval, DecisionTime, UtcMicros};
//! use parangsae_signals::{Decision, SignalOutput, Target};
//! fn engine_accepts(_: SignalOutput) {}
//! let signal_out = SignalOutput {
//!     decision_ts: DecisionTime::from_micros(UtcMicros::from_micros(0)),
//!     bar_close_time_us: UtcMicros::from_micros(0),
//!     interval: BarInterval::H1,
//!     decision: Decision::Target(Target::FLAT),
//! };
//! engine_accepts(signal_out);
//! ```

use parangsae_core::{BarInterval, DecisionTime, UtcMicros};

use crate::rank::Rank;

/// One feature value for one closed bar. `value` is a rank, not a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureOutput {
    pub decision_ts: DecisionTime,
    pub bar_close_time_us: UtcMicros,
    pub interval: BarInterval,
    pub value: Rank,
}

#[cfg(test)]
mod tests {
    use crate::{FEATURE_IDS, SIGNAL_IDS, TARGET_IDS};
    use std::collections::BTreeSet;

    #[test]
    fn catalogs_partition_signal_ids() {
        let targets: BTreeSet<&str> = TARGET_IDS.iter().copied().collect();
        let features: BTreeSet<&str> = FEATURE_IDS.iter().copied().collect();
        let mut union = targets.clone();
        union.extend(features.iter().copied());
        let all: BTreeSet<&str> = SIGNAL_IDS.iter().copied().collect();
        assert!(targets.is_disjoint(&features));
        assert_eq!(union, all);
        assert!(features.contains("volregime"));
        assert!(!targets.contains("volregime"));
    }
}
