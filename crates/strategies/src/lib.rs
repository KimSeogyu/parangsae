//! First-batch families S01, S02, S03, S05, and S06.
//!
//! Each family emits one [`parangsae_signals::SignalOutput`] per closed bar once
//! its lookback and the prereg warmup instant have both passed. Flat positions
//! are [`parangsae_signals::Decision::Target`] of [`parangsae_signals::Target::FLAT`].
//! [`parangsae_signals::Decision::Flat`] stays the funding-only reason from
//! `signals`. Targets are built with [`parangsae_signals::Target::from_f64`].
//!
//! A bar is closed only when `available_ts` equals `close_time + 1 ms`. The
//! decision for that bar is that timestamp. `Cursor::admit` returns the
//! [`parangsae_core::Causal`] view from
//! [`parangsae_core::Observation::causal_at`] and rejects anything earlier than
//! 2020-12-18 00:00 UTC. Families read that view. History buffers have a fixed cap.
//!
//! # Readings where the prereg left a choice
//!
//! * **S01 `w_cur`.** With a [`DecisionBook`], `w_cur` is the signed notional
//!   over equity at the decision close: `position_lots * mark_ticks /
//!   equity_ticks`, toward zero. The deadband is `|w_t - w_cur| >= 0.25`
//!   (`25_000_000` ticks). Inside the band the last emitted target is repeated
//!   so the engine does not trade. An accepted rebalance increments `accept`
//!   even when the clipped target ticks stay the same, and the engine resizes.
//!   A replay with no book uses that last emitted target as `w_cur`.
//! * **S01 scale.** `r_scale = (c / 1e8) · r0 · sqrt(n / n0)` with H1
//!   `(r0, n0) = (0.05, 24)` and H4 `(0.10, 42)`, via `libm::sqrt`. `c` is the
//!   config's `1e8` tick count.
//! * **Warmup silence.** No output before 2021-01-17 00:00 UTC for S01, S02,
//!   S03, and S05, or before 2021-12-18 00:00 UTC for S06. Buffers still
//!   update. Position state does not. This is the protocol's "no decisions in
//!   the warmup period" on the full-window batch.
//! * **Time stop.** At the first decision `>= entry + H`, the position is flat
//!   before that bar's entry signal is considered, so the exit bar can open a
//!   new trade. S02 flips on an opposite break and does not extend on a
//!   same-side break. S03 and S05 ignore signals during the hold, including
//!   opposite breaks.
//! * **No new hold past 23:59.** An entry whose hold end is after
//!   2025-09-30 23:59:00 UTC is refused, and that refusal increments
//!   [`Strategy::dropped_entries`]. S02 does the same on a flip: the old side
//!   is closed and the new side is not opened. S01 has no fixed hold. S06 uses
//!   the same end instant through `hold_fits` and is not part of this count.
//! * **S03 / S05 rank.** Compared in rank ticks. `θ` and `k` and the quiet
//!   gate `0.5` are the config's `1e8` ticks. S03 uses `p_{t-1}`. S05 uses
//!   `p_t`. Both call `VolRegime<24, 168>` through `drive_feature`.
//! * **S05 `σ_L`.** `sqrt(L) ·` sample sd (ddof 1) of the 720 H1 log returns
//!   ending at `t` (721 closes). That is the queue's "previous 720 h", one
//!   return longer than the 719 returns inside a 720-bar window. A
//!   non-positive sd or a non-finite `z` does not enter.
//! * **S06 blocks.** Open-to-close log return of each H4 bar. `m_b` and `sd_b`
//!   use the previous `W` occurrences of block `b` only, which on a gap-free
//!   grid are the previous `W` calendar days. A zero sd emits target 0. The
//!   signal still reports the held target when the next block has the same
//!   sign; the order layer is what skips the re-trade.
//! * **Sizing.** The engine sizes `w × equity / decision close` when the
//!   emitted target changes, and holds that quantity until the next change.
//!   Equity is marked at the decision close. That close stands in for the
//!   unknown next 1m open, which is the fill price. Quantity is floored toward
//!   zero to 0.001 BTC. A `count = 0` minute does not fill; the order waits
//!   for the next minute with `count > 0`, and each refused minute is
//!   `n_refused_fills`. The open of 2025-09-30 23:59 UTC force-flats, so the
//!   final exit pays costs. A repeated target has delta quantity 0, so S06
//!   does not re-trade a same-sign block.

#![forbid(unsafe_code)]

mod donchian;
mod error;
mod fade;
mod season;
mod series;
mod squeeze;
mod tsmom;
mod vol;

use std::collections::BTreeMap;

use parangsae_core::{BarInterval, Observation, ResampledBar};
use parangsae_signals::SignalOutput;

pub use error::StrategyError;
pub use series::{DecisionBook, EARLIEST_US};

enum Inner {
    S01(tsmom::TsmomSlow),
    S02(donchian::Donchian),
    S03(squeeze::Squeeze),
    S05(fade::Fade),
    S06(season::Season),
}

/// One configured family. `push` is causal: it only reads the bar it is given
/// plus the capped history of bars already pushed.
pub struct Strategy {
    inner: Inner,
}

impl Strategy {
    pub fn from_params(
        family: &str,
        params: &BTreeMap<String, i64>,
    ) -> Result<Self, StrategyError> {
        let inner = match family {
            "S01" => Inner::S01(tsmom::TsmomSlow::from_params(params)?),
            "S02" => Inner::S02(donchian::Donchian::from_params(params)?),
            "S03" => Inner::S03(squeeze::Squeeze::from_params(params)?),
            "S05" => Inner::S05(fade::Fade::from_params(params)?),
            "S06" => Inner::S06(season::Season::from_params(params)?),
            _ => return Err(StrategyError::Param("family")),
        };
        Ok(Self { inner })
    }

    /// Holds `Hold::arm` refused because they would end after 2025-09-30 23:59 UTC.
    pub fn dropped_entries(&self) -> u64 {
        match &self.inner {
            Inner::S01(_) | Inner::S06(_) => 0,
            Inner::S02(strategy) => strategy.dropped_entries(),
            Inner::S03(strategy) => strategy.dropped_entries(),
            Inner::S05(strategy) => strategy.dropped_entries(),
        }
    }

    pub fn interval(&self) -> BarInterval {
        match self.inner {
            Inner::S01(ref strategy) => strategy.interval(),
            Inner::S02(_) | Inner::S03(_) | Inner::S05(_) => BarInterval::H1,
            Inner::S06(_) => BarInterval::H4,
        }
    }

    pub fn push(
        &mut self,
        row: &Observation<ResampledBar>,
        book: Option<DecisionBook>,
    ) -> Result<Option<SignalOutput>, StrategyError> {
        match &mut self.inner {
            Inner::S01(strategy) => strategy.push(row, book),
            Inner::S02(strategy) => strategy.push(row, book),
            Inner::S03(strategy) => strategy.push(row, book),
            Inner::S05(strategy) => strategy.push(row, book),
            Inner::S06(strategy) => strategy.push(row, book),
        }
    }
}

/// Replay `bars` in order. A prefix replay matches the full replay up to that prefix.
pub fn replay(
    family: &str,
    params: &BTreeMap<String, i64>,
    bars: &[Observation<ResampledBar>],
) -> Result<Vec<SignalOutput>, StrategyError> {
    let mut strategy = Strategy::from_params(family, params)?;
    let mut out = Vec::new();
    for bar in bars {
        if let Some(signal) = strategy.push(bar, None)? {
            out.push(signal);
        }
    }
    Ok(out)
}
