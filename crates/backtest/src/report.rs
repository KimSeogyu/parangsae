//! Equity, fills, round trips, and the on-disk report.
//! Integer fields are written as integer strings so a tick is not a float.

use std::fs;
use std::io;
use std::path::Path;

use parangsae_core::{Ratio, UtcMicros};
use parangsae_sim::{Cash, Costs, Fill, FundingEvent, Side};

use crate::date::UtcDate;

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

#[derive(Clone, Debug, PartialEq)]
pub struct Metrics {
    pub days: u32,
    pub total_return: Ratio,
    pub max_drawdown: Ratio,
    pub sharpe_daily: Option<f64>,
    pub sharpe_annual: Option<f64>,
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
        fs::write(path.join("metrics.json"), metrics_json(self))?;
        Ok(())
    }
}

pub(crate) fn daily_equity(
    curve: &[(UtcMicros, Cash)],
) -> Result<Vec<(UtcDate, Cash)>, crate::date::DateError> {
    let mut out = Vec::new();
    for (ts, cash) in curve {
        let day = UtcDate::from_micros(*ts)?;
        if let Some((last_day, last_cash)) = out.last_mut() {
            if *last_day == day {
                *last_cash = *cash;
                continue;
            }
        }
        out.push((day, *cash));
    }
    Ok(out)
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
        json_f64("sharpe_daily", metrics.sharpe_daily),
        json_f64("sharpe_annual", metrics.sharpe_annual),
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
    ];
    format!("{{\n{}\n}}\n", fields.join(",\n"))
}

fn json_f64(key: &str, value: Option<f64>) -> String {
    match value {
        Some(sample) if sample.is_finite() => format!("  \"{key}\": {sample}"),
        Some(_) | None => format!("  \"{key}\": null"),
    }
}
