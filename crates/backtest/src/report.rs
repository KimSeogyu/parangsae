//! Equity, fills, round trips, and the on-disk report.
//! Integer fields are written as integer strings so a tick is not a float.

use std::fs;
use std::io;
use std::path::Path;

use parangsae_core::{Ratio, Side, UtcMicros};
use parangsae_sim::{Cash, Costs, Fill, FundingEvent};
use parangsae_strategies::{ActualHold, H4Event, H4Observation};

use crate::date::UtcDate;
use crate::engine::{MarkAbsence, UtcRange};
use crate::signal::{Decision, FlatReason, SignalOutput};
use crate::tape::AppliedOverlay;

/// A position that left zero. `close_ts` is empty while the trip is still open.
/// `pnl` is the sum of cash flows in the trip: fills, fees, and funding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundTrip {
    pub open_ts: UtcMicros,
    pub close_ts: Option<UtcMicros>,
    pub side: Side,
    pub qty: parangsae_core::Qty,
    pub pnl: Cash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForcedExitReason {
    Terminal,
    Ruin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForcedExit {
    pub trigger: UtcMicros,
    pub filled_at: UtcMicros,
    pub reason: ForcedExitReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct H4Trade {
    pub trade: RoundTrip,
    pub event: H4Event,
    pub hold: ActualHold,
    pub slippage_cost: Cash,
    pub funding: Vec<FundingEvent>,
    pub fills: Vec<Fill>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H4EntryDropReason {
    DevelopmentBoundary,
    ZeroLots,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct H4EntryDrop {
    pub event: H4Event,
    pub at: UtcMicros,
    pub reason: H4EntryDropReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct H4Trace {
    pub scored: UtcRange,
    pub observations: Vec<H4Observation>,
    pub trades: Vec<H4Trade>,
    pub entry_drops: Vec<H4EntryDrop>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metrics {
    pub days: u32,
    pub total_return: Ratio,
    pub max_drawdown: Ratio,
    pub sharpe_daily: Option<Ratio>,
    pub sharpe_annual: Option<Ratio>,
    pub turnover: Ratio,
    pub fees_paid: Cash,
    pub funding_net: Cash,
    pub fills: u32,
    pub trades: u32,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub equity_curve: Vec<(UtcMicros, Cash)>,
    pub daily_equity: Vec<(UtcDate, Cash)>,
    pub fills: Vec<Fill>,
    pub funding: Vec<FundingEvent>,
    pub trades: Vec<RoundTrip>,
    pub metrics: Metrics,
    pub costs: Costs,
    pub bars: u64,
    pub ruined: Option<UtcMicros>,
    pub dropped_orders: u64,
    pub warmup_bars: u64,
    pub flat_funding_stale: u64,
    pub flat_funding_slot_missing: u64,
    /// Minutes a resting order skipped because `count` was 0.
    pub n_refused_fills: u64,
    /// Cash lost to a worse fill price. The fee is not included.
    pub slippage_cost: Cash,
    /// Strategy-initiated settlements that changed the held quantity.
    pub n_rebalances: u64,
    /// Realized held-position sign flips. Both sides are nonzero.
    pub n_sign_changes: u64,
    /// Realized forced closes at a fold/data end, forced-flat window, or ruin.
    pub n_forced_exits: u64,
    pub forced_exits: Vec<ForcedExit>,
    pub h4: Option<H4Trace>,
    pub signal_outputs: Vec<SignalOutput>,
    pub overlays: Vec<AppliedOverlay>,
    pub mark_gaps: Vec<MarkAbsence>,
    pub forced_flat: Vec<UtcRange>,
}

impl Report {
    pub fn equity_end(&self) -> Option<Cash> {
        self.equity_curve.last().map(|(_, cash)| *cash)
    }

    /// Writes `equity.csv`, `fills.csv`, `trades.csv`, `funding.csv`, and `metrics.json`.
    pub fn write_dir(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)?;
        fs::write(path.join("equity.csv"), equity_csv(&self.equity_curve))?;
        fs::write(path.join("fills.csv"), fills_csv(&self.fills))?;
        fs::write(path.join("trades.csv"), trades_csv(&self.trades))?;
        fs::write(path.join("funding.csv"), funding_csv(&self.funding))?;
        fs::write(
            path.join("signal_outputs.csv"),
            signal_csv(&self.signal_outputs),
        )?;
        fs::write(path.join("metrics.json"), metrics_json(self))?;
        Ok(())
    }
}

pub(crate) fn daily_equity(curve: &[(UtcMicros, Cash)]) -> Vec<(UtcDate, Cash)> {
    let mut out = Vec::new();
    for (ts, cash) in curve {
        let day = UtcDate::from_micros(*ts);
        if let Some((last_day, last_cash)) = out.last_mut() {
            if *last_day == day {
                *last_cash = *cash;
                continue;
            }
        }
        out.push((day, *cash));
    }
    out
}

fn equity_csv(curve: &[(UtcMicros, Cash)]) -> String {
    let mut out = String::from("close_time_us,equity_ticks\n");
    for (ts, cash) in curve {
        out.push_str(&format!("{},{}\n", ts.micros(), cash.0));
    }
    out
}

fn fills_csv(fills: &[Fill]) -> String {
    let mut out = String::from("ts_us,side,qty_ticks,price_ticks,fee_ticks\n");
    for fill in fills {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            fill.ts.micros(),
            fill.side,
            fill.qty.ticks(),
            fill.price.ticks(),
            fill.fee.0
        ));
    }
    out
}

fn trades_csv(trades: &[RoundTrip]) -> String {
    let mut out = String::from("open_ts_us,close_ts_us,side,qty_ticks,pnl_ticks\n");
    for trade in trades {
        let close = trade
            .close_ts
            .map(|ts| ts.micros().to_string())
            .unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            trade.open_ts.micros(),
            close,
            trade.side,
            trade.qty.ticks(),
            trade.pnl.0
        ));
    }
    out
}

fn funding_csv(events: &[FundingEvent]) -> String {
    let mut out = String::from("ts_us,rate_ticks,mark_ticks,position_ticks,amount_ticks\n");
    for event in events {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            event.ts.micros(),
            event.rate.ticks(),
            event.mark.ticks(),
            event.position.0,
            event.amount.0
        ));
    }
    out
}

fn metrics_json(report: &Report) -> String {
    let metrics = &report.metrics;
    let fields = vec![
        format!("  \"days\": \"{}\"", metrics.days),
        format!(
            "  \"total_return_ticks\": \"{}\"",
            metrics.total_return.ticks()
        ),
        format!(
            "  \"max_drawdown_ticks\": \"{}\"",
            metrics.max_drawdown.ticks()
        ),
        json_ratio("sharpe_daily_ticks", metrics.sharpe_daily),
        json_ratio("sharpe_annual_ticks", metrics.sharpe_annual),
        format!("  \"turnover_ticks\": \"{}\"", metrics.turnover.ticks()),
        format!("  \"fees_paid_ticks\": \"{}\"", metrics.fees_paid.0),
        format!("  \"funding_net_ticks\": \"{}\"", metrics.funding_net.0),
        format!("  \"fills\": \"{}\"", metrics.fills),
        format!("  \"trades\": \"{}\"", metrics.trades),
        format!("  \"taker_fee_bps\": \"{}\"", report.costs.taker_fee.0),
        format!("  \"slippage_bps\": \"{}\"", report.costs.slippage.0),
        format!("  \"bars\": \"{}\"", report.bars),
        "  \"scale\": \"100000000\"".to_owned(),
        match report.ruined {
            Some(ts) => format!("  \"ruined_us\": \"{}\"", ts.micros()),
            None => "  \"ruined_us\": null".to_owned(),
        },
        format!("  \"dropped_orders\": \"{}\"", report.dropped_orders),
        format!("  \"warmup_bars\": \"{}\"", report.warmup_bars),
        format!(
            "  \"flat_funding_stale\": \"{}\"",
            report.flat_funding_stale
        ),
        format!(
            "  \"flat_funding_slot_missing\": \"{}\"",
            report.flat_funding_slot_missing
        ),
        format!("  \"n_refused_fills\": \"{}\"", report.n_refused_fills),
        format!("  \"overlays\": {}", overlays_json(&report.overlays)),
        format!("  \"mark_gaps\": {}", gaps_json(&report.mark_gaps)),
        format!("  \"forced_flat\": {}", ranges_json(&report.forced_flat)),
    ];
    format!("{{\n{}\n}}\n", fields.join(",\n"))
}

fn json_ratio(key: &str, value: Option<Ratio>) -> String {
    match value {
        Some(ratio) => format!("  \"{key}\": \"{}\"", ratio.ticks()),
        None => format!("  \"{key}\": null"),
    }
}

fn signal_csv(outputs: &[SignalOutput]) -> String {
    let mut out = String::from(
        "decision_ts_us,bar_close_time_us,interval_us,decision,flat_reason,target_ticks\n",
    );
    for output in outputs {
        let (decision, reason, target) = match output.decision {
            Decision::Target(target) => ("target", "", target.ticks().to_string()),
            Decision::Flat(FlatReason::FundingStale) => ("flat", "funding_stale", "0".to_owned()),
            Decision::Flat(FlatReason::FundingSlotMissing) => {
                ("flat", "funding_slot_missing", "0".to_owned())
            }
        };
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            output.decision_ts.micros().micros(),
            output.bar_close_time_us.micros(),
            output.interval.micros(),
            decision,
            reason,
            target
        ));
    }
    out
}

fn overlays_json(overlays: &[AppliedOverlay]) -> String {
    if overlays.is_empty() {
        return "[]".to_owned();
    }
    let parts: Vec<String> = overlays
        .iter()
        .map(|overlay| {
            format!(
                "{{\"dataset\":\"{}\",\"path\":\"{}\",\"sha256\":\"{}\"}}",
                json_escape(&overlay.dataset),
                json_escape(&overlay.path),
                overlay.sha256
            )
        })
        .collect();
    format!("[{}]", parts.join(","))
}

fn gaps_json(gaps: &[MarkAbsence]) -> String {
    if gaps.is_empty() {
        return "[]".to_owned();
    }
    let parts: Vec<String> = gaps
        .iter()
        .map(|gap| {
            format!(
                "{{\"first_open_us\":\"{}\",\"last_open_us\":\"{}\",\"known_source_absence\":{}}}",
                gap.first_open.micros(),
                gap.last_open.micros(),
                if gap.known_source_absence {
                    "true"
                } else {
                    "false"
                }
            )
        })
        .collect();
    format!("[{}]", parts.join(","))
}

fn ranges_json(ranges: &[UtcRange]) -> String {
    if ranges.is_empty() {
        return "[]".to_owned();
    }
    let parts: Vec<String> = ranges
        .iter()
        .map(|range| {
            format!(
                "{{\"start_us\":\"{}\",\"end_us\":\"{}\"}}",
                range.start.micros(),
                range.end.micros()
            )
        })
        .collect();
    format!("[{}]", parts.join(","))
}

fn json_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}
