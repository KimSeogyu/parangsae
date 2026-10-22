//! Binance Vision backfill. Public archives only. No API key and no order route.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use parangsae_collector::civil::{parse_day, DayRange};
use parangsae_collector::fetch::HttpGet;
use parangsae_collector::run::{self, Provenance, RunRequest, VerifyFilter};
use parangsae_collector::version::migrate_receipts;
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
    /// Map rows/v1 `received_ts_us` values onto archive runs. Default is a dry run.
    MigrateReceipts(MigrateArgs),
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

#[derive(Parser)]
struct MigrateArgs {
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    /// Rewrite v1 partitions. Omit for a dry run, which writes nothing.
    #[arg(long, default_value_t = false)]
    apply: bool,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Vision(args) => vision(args),
        Command::Verify(args) => verify(args),
        Command::MigrateReceipts(args) => migrate(args),
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
    let run_id = match run::generate_run_id() {
        Ok(run_id) => run_id,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let started_at = match run::wall_now() {
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
        run: run_id,
        started_at,
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
    let filter = match VerifyFilter::parse(&args.dataset, args.from.as_deref(), args.to.as_deref())
    {
        Ok(filter) => filter,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    match run::verify(&args.data_dir, &filter) {
        Ok(report) => {
            println!("verify {report}");
            if report.ok() {
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

fn migrate(args: MigrateArgs) -> ExitCode {
    let provenance = Provenance::from_build();
    match migrate_receipts(&args.data_dir, args.apply, &provenance) {
        Ok(report) => {
            let mode = if args.apply { "apply" } else { "dry-run" };
            print!("migrate-receipts {mode} {report}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            if err.check_name().is_some() {
                ExitCode::from(1)
            } else {
                ExitCode::from(2)
            }
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
