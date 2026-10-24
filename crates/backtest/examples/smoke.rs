//! Buy-and-hold smoke run. Prints the costs it was given, including slippage.
//!
//! ```text
//! cargo run -p parangsae-backtest --example smoke -- \
//!   --root /workspace/data/parangsae --from 2024-01-01 --to 2024-01-07 --slippage-bps 1
//! ```

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use parangsae_backtest::fixture::BuyAndHold;
use parangsae_backtest::{run, RunConfig, Tape, TapeConfig, UtcDate};
use parangsae_core::{Interval, Qty, Ratio, Symbol};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};

const SCALE: i64 = 100_000_000;

fn main() -> ExitCode {
    match run_smoke() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("smoke: {message}");
            ExitCode::from(1)
        }
    }
}

fn run_smoke() -> Result<(), String> {
    let args = Args::parse()?;
    let symbol = Symbol::new("BTCUSDT").expect("BTCUSDT is a valid symbol");
    let from = UtcDate::parse(&args.from).ok_or_else(|| format!("bad --from {}", args.from))?;
    let to = UtcDate::parse(&args.to).ok_or_else(|| format!("bad --to {}", args.to))?;
    let holdout = UtcDate::parse("2025-10-01")
        .expect("holdout date")
        .start_us();
    let tape = Tape::load(
        &args.root,
        &symbol,
        from,
        to,
        &TapeConfig {
            holdout_start: holdout,
            overlays: Vec::new(),
            funding_root: args.root.clone(),
            include_premium: false,
        },
    )
    .map_err(|err| err.to_string())?;
    let costs = Costs {
        taker_fee: FeeBps(5),
        slippage: SlippageBps(args.slippage_bps),
    };
    let cfg = RunConfig {
        initial_cash: Cash(1_000_000 * SCALE),
        warmup_start: from.start_us(),
        start: from.start_us(),
        end: to.next().start_us(),
        holdout_start: holdout,
        forced_flat: Vec::new(),
        flatten_before: None,
        mark_gaps: Vec::new(),
        interval: Interval::ONE_HOUR,
        lot_ticks: 100_000,
    };
    println!("slippage_bps={}", costs.slippage.0);
    println!("taker_fee_bps={}", costs.taker_fee.0);
    println!("scale={SCALE}");
    let mut strategy = BuyAndHold::new(Qty::from_ticks(SCALE).expect("1 BTC fits"));
    let report = run(&tape, &mut strategy, &costs, &cfg).map_err(|err| err.to_string())?;
    report
        .write_dir(&args.out)
        .map_err(|err| format!("write {}: {err}", args.out.display()))?;
    let end = report.equity_end().map(|cash| cash.0).unwrap_or(0);
    println!("bars={}", report.bars);
    println!("fills={}", report.fills.len());
    println!("fees_paid_ticks={}", report.metrics.fees_paid.0);
    println!("funding_net_ticks={}", report.metrics.funding_net.0);
    println!("funding_events={}", report.funding.len());
    println!("equity_end_ticks={end}");
    println!(
        "sharpe_daily_ticks={}",
        fmt_ratio(report.metrics.sharpe_daily)
    );
    println!(
        "sharpe_annual_ticks={}",
        fmt_ratio(report.metrics.sharpe_annual)
    );
    println!("max_drawdown_ticks={}", report.metrics.max_drawdown.ticks());
    println!("dropped_orders={}", report.dropped_orders);
    if let Some(ts) = report.ruined {
        println!("ruined_us={}", ts.micros());
    }
    println!("report_dir={}", args.out.display());
    Ok(())
}

fn fmt_ratio(value: Option<Ratio>) -> String {
    match value {
        Some(ratio) => ratio.ticks().to_string(),
        None => "none".to_owned(),
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
