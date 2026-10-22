//! Buy-and-hold smoke run. Prints the costs it was given, including slippage.
//!
//! ```text
//! cargo run -p parangsae-backtest --example smoke -- \
//!   --root /workspace/data/parangsae --from 2024-01-01 --to 2024-01-07 --slippage-bps 1
//! ```

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use parangsae_backtest::fixture::BuyAndHold;
use parangsae_backtest::{run, RunConfig, Tape, UtcDate};
use parangsae_core::{Qty, Symbol};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};

const SCALE: i64 = 100_000_000;

fn main() -> ExitCode {
    match run_smoke() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(io::stderr(), "smoke: {message}");
            ExitCode::from(1)
        }
    }
}

fn run_smoke() -> Result<(), String> {
    let args = Args::parse()?;
    let symbol = Symbol::new("BTCUSDT").map_err(|err| err.to_string())?;
    let from = UtcDate::parse(&args.from).ok_or_else(|| format!("bad --from {}", args.from))?;
    let to = UtcDate::parse(&args.to).ok_or_else(|| format!("bad --to {}", args.to))?;
    let tape = Tape::load(&args.root, &symbol, from, to).map_err(|err| err.to_string())?;
    let costs = Costs {
        taker_fee: FeeBps(5),
        slippage: SlippageBps(args.slippage_bps),
    };
    let cfg = RunConfig {
        initial_cash: Cash(1_000_000 * SCALE),
        start: from.start_us().map_err(|err| err.to_string())?,
        end: to.next().start_us().map_err(|err| err.to_string())?,
    };
    line(format!("slippage_bps={}", costs.slippage.0))?;
    line(format!("taker_fee_bps={}", costs.taker_fee.0))?;
    line(format!("scale={SCALE}"))?;
    let qty = Qty::from_ticks(SCALE).map_err(|err| err.to_string())?;
    let mut strategy = BuyAndHold::new(qty);
    let report = run(&tape, &mut strategy, &costs, &cfg).map_err(|err| err.to_string())?;
    report
        .write_dir(&args.out)
        .map_err(|err| format!("write {}: {err}", args.out.display()))?;
    let end = report.equity_end().map(|cash| cash.0).unwrap_or(0);
    line(format!("bars={}", report.bars))?;
    line(format!("fills={}", report.fills.len()))?;
    line(format!("fees_paid_ticks={}", report.metrics.fees_paid.0))?;
    line(format!(
        "funding_net_ticks={}",
        report.metrics.funding_net.0
    ))?;
    line(format!("funding_events={}", report.funding.len()))?;
    line(format!("equity_end_ticks={end}"))?;
    line(format!(
        "sharpe_daily={}",
        fmt_opt(report.metrics.sharpe_daily)
    ))?;
    line(format!(
        "sharpe_annual={}",
        fmt_opt(report.metrics.sharpe_annual)
    ))?;
    line(format!(
        "max_drawdown_ticks={}",
        report.metrics.max_drawdown.ticks()
    ))?;
    line(format!("dropped_orders={}", report.dropped_orders))?;
    if let Some(ts) = report.ruined {
        line(format!("ruined_us={}", ts.micros()))?;
    }
    line(format!("report_dir={}", args.out.display()))?;
    Ok(())
}

fn line(text: impl std::fmt::Display) -> Result<(), String> {
    writeln!(io::stdout(), "{text}").map_err(|err| err.to_string())
}

fn fmt_opt(value: Option<f64>) -> String {
    match value {
        Some(sample) if sample.is_finite() => sample.to_string(),
        Some(_) | None => "none".to_owned(),
    }
}

struct Args {
    root: PathBuf,
    from: String,
    to: String,
    slippage_bps: u32,
    out: PathBuf,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut root = None;
        let mut from = None;
        let mut to = None;
        let mut slippage = None;
        let mut out = None;
        let mut raw = env::args().skip(1);
        while let Some(arg) = raw.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => {
                    (flag.to_owned(), Some(value.to_owned()))
                }
                _ => (arg, None),
            };
            let value = match inline {
                Some(value) => value,
                None => raw
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?,
            };
            match flag.as_str() {
                "--root" => root = Some(PathBuf::from(value)),
                "--from" => from = Some(value),
                "--to" => to = Some(value),
                "--slippage-bps" => {
                    slippage = Some(
                        value
                            .parse::<u32>()
                            .map_err(|_| format!("bad --slippage-bps {value}"))?,
                    );
                }
                "--out" => out = Some(PathBuf::from(value)),
                other => return Err(format!("unknown argument {other}")),
            }
        }
        Ok(Self {
            root: root.ok_or("missing --root")?,
            from: from.ok_or("missing --from")?,
            to: to.ok_or("missing --to")?,
            slippage_bps: slippage.ok_or("missing --slippage-bps")?,
            out: out.unwrap_or_else(|| PathBuf::from("smoke-report")),
        })
    }
}
