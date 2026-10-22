//! Binance Vision backfill. Public archives only. No API key and no order route.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use parangsae_collector::civil::{parse_day, DayRange};
use parangsae_collector::fetch::HttpGet;
use parangsae_collector::run::{self, RunRequest};
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
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Vision(args) => vision(args),
        Command::Verify(args) => verify(args),
    }
}

fn vision(args: VisionArgs) -> ExitCode {
    let datasets = match datasets(&args.dataset) {
        Ok(datasets) => datasets,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let symbol = match Symbol::new(&args.symbol) {
        Ok(symbol) => symbol,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let from = match parse_day(&args.from) {
        Ok(day) => day,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let to = match parse_day(&args.to) {
        Ok(day) => day,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let range = match DayRange::new(from, to) {
        Ok(range) => range,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let fetched_at = match run::received_now() {
        Ok(ts) => ts,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let client = match HttpGet::new() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
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
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn verify(args: VerifyArgs) -> ExitCode {
    match run::verify(&args.data_dir) {
        Ok(summary) => {
            println!("verify {summary}");
            if summary.ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn print_rows(rows: &[(VisionDataset, run::RunSummary)]) -> ExitCode {
    let mut failed = false;
    for (dataset, summary) in rows {
        println!("{dataset} {summary}");
        if !summary.ok() {
            failed = true;
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
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
