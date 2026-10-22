//! Binance Vision backfill. Public archives only. No API key and no order route.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use parangsae_collector::civil::{parse_day, DayRange};
use parangsae_collector::fetch::HttpGet;
use parangsae_collector::run::{self, RunRequest, VerifyFilter};
use parangsae_collector::vision::VisionDataset;
use parangsae_core::Symbol;

#[derive(Parser)]
#[command(name = "parangsae-collect")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Download and ingest Binance Vision archives for a closed date range.
    Vision(VisionArgs),
    /// Re-check local checksums, parquet rows, and invariants. Does not use the network.
    Verify(VerifyArgs),
}

#[derive(Parser)]
struct VisionArgs {
    /// Dataset name, a Vision folder alias, or `all`.
    #[arg(long)]
    dataset: String,
    /// First UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    from: String,
    /// Last UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    to: String,
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    #[arg(long, default_value = "BTCUSDT")]
    symbol: String,
    #[arg(long, default_value_t = 2)]
    jobs: usize,
    #[arg(long, default_value_t = 200)]
    min_interval_ms: u64,
}

#[derive(Parser)]
struct VerifyArgs {
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    /// Dataset name, repeatable. Same names as `vision --dataset`.
    #[arg(long = "dataset")]
    dataset: Vec<String>,
    /// First UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    from: Option<String>,
    /// Last UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    to: Option<String>,
}

fn main() -> Result<ExitCode, io::Error> {
    match Cli::parse().command {
        Command::Vision(args) => vision(args),
        Command::Verify(args) => verify(args),
    }
}

fn fail(err: impl std::fmt::Display) -> Result<ExitCode, io::Error> {
    writeln!(io::stderr().lock(), "{err}")?;
    Ok(ExitCode::from(2))
}

fn vision(args: VisionArgs) -> Result<ExitCode, io::Error> {
    let datasets = match datasets(&args.dataset) {
        Ok(datasets) => datasets,
        Err(err) => return fail(err),
    };
    let symbol = match Symbol::new(&args.symbol) {
        Ok(symbol) => symbol,
        Err(err) => return fail(err),
    };
    let from = match parse_day(&args.from) {
        Ok(day) => day,
        Err(err) => return fail(err),
    };
    let to = match parse_day(&args.to) {
        Ok(day) => day,
        Err(err) => return fail(err),
    };
    let range = match DayRange::new(from, to) {
        Ok(range) => range,
        Err(err) => return fail(err),
    };
    let fetched_at = match run::received_now() {
        Ok(ts) => ts,
        Err(err) => return fail(err),
    };
    let client = match HttpGet::new() {
        Ok(client) => client,
        Err(err) => return fail(err),
    };
    let request = RunRequest {
        datasets,
        symbol,
        range,
        data_dir: args.data_dir,
        jobs: args.jobs,
        min_interval: Duration::from_millis(args.min_interval_ms),
        fetched_at,
    };
    match run::run_vision(&request, &client) {
        Ok(rows) => print_rows(&rows),
        Err(err) => fail(err),
    }
}

fn verify(args: VerifyArgs) -> Result<ExitCode, io::Error> {
    let filter = match VerifyFilter::parse(&args.dataset, args.from.as_deref(), args.to.as_deref())
    {
        Ok(filter) => filter,
        Err(err) => return fail(err),
    };
    match run::verify(&args.data_dir, &filter) {
        Ok(report) => {
            writeln!(io::stdout().lock(), "verify {report}")?;
            if report.ok() {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Err(err) => fail(err),
    }
}

fn print_rows(rows: &[(VisionDataset, run::RunSummary)]) -> Result<ExitCode, io::Error> {
    let mut failed = false;
    let mut stdout = io::stdout().lock();
    for (dataset, summary) in rows {
        writeln!(stdout, "{dataset} {summary}")?;
        if !summary.ok() {
            failed = true;
        }
    }
    if failed {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn datasets(text: &str) -> Result<Vec<VisionDataset>, String> {
    if text == "all" {
        return Ok(VisionDataset::ALL.to_vec());
    }
    VisionDataset::parse(text)
        .map(|dataset| vec![dataset])
        .ok_or_else(|| format!("unknown dataset {text}"))
}
