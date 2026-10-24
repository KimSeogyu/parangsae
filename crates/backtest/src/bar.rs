//! Bars, the causal view, and the in-memory tape.

use parangsae_core::{Ratio, UtcMicros};
use parangsae_sim::MarketOrder;

pub use parangsae_core::Bar;

/// A funding settlement timestamp. `interval_hours` is the declared grid, 8 for USD-M BTCUSDT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundingStamp {
    pub funding_time: UtcMicros,
    pub available: UtcMicros,
    pub rate: Ratio,
    pub interval_hours: i64,
}

/// One premium-index minute. `close_ticks` is a ratio at `10^8` and may be negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PremiumMinute {
    pub open_time: UtcMicros,
    pub close_ticks: i64,
    pub count: i64,
    pub available: UtcMicros,
}

/// Um bars, mark bars, funding stamps, and premium minutes. `run` rejects gaps and missing coverage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tape {
    pub bars: Vec<Bar>,
    pub marks: Vec<Bar>,
    pub funding: Vec<FundingStamp>,
    pub premium: Vec<PremiumMinute>,
    pub overlays: Vec<crate::tape::AppliedOverlay>,
}

impl Tape {
    pub fn from_parts(bars: Vec<Bar>, marks: Vec<Bar>, funding: Vec<FundingStamp>) -> Self {
        Self {
            bars,
            marks,
            funding,
            premium: Vec::new(),
            overlays: Vec::new(),
        }
    }

    pub fn with_premium(mut self, mut premium: Vec<PremiumMinute>) -> Self {
        premium.sort_by_key(|row| (row.available, row.open_time));
        self.premium = premium;
        self
    }
}

/// Bars a strategy may read at one decision.
///
/// There is no public constructor. The slice stops at the current bar, and
/// `get` returns [`None`] past that prefix. Indexing off the end is not offered.
#[derive(Clone, Copy, Debug)]
pub struct BarView<'a> {
    bars: &'a [Bar],
    decision: UtcMicros,
    visible: usize,
}

impl<'a> BarView<'a> {
    pub(crate) fn new(bars: &'a [Bar], decision: UtcMicros) -> Self {
        let visible = bars
            .iter()
            .position(|bar| bar.available > decision)
            .unwrap_or(bars.len());
        Self {
            bars,
            decision,
            visible,
        }
    }

    pub fn decision(self) -> UtcMicros {
        self.decision
    }

    pub fn len(self) -> usize {
        self.visible
    }

    pub fn is_empty(self) -> bool {
        self.visible == 0
    }

    pub fn get(self, index: usize) -> Option<&'a Bar> {
        if index < self.visible {
            Some(&self.bars[index])
        } else {
            None
        }
    }

    pub fn iter(self) -> impl Iterator<Item = &'a Bar> {
        self.bars[..self.visible].iter()
    }
}

pub trait Strategy {
    fn on_bar(&mut self, view: BarView<'_>) -> Option<MarketOrder>;
}

pub(crate) const ONE_MINUTE_US: i64 = 60_000_000;
