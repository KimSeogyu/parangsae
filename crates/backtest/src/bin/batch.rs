//! First-batch driver. One trial per config in the sweep inputs.
//!
//! ```text
//! cargo run -p parangsae-backtest --bin batch -- \
//!   --inputs research/sweep-inputs-v2.json \
//!   --data-root /workspace/data/parangsae \
//!   --out runs/first-batch \
//!   --slippage-bps 2
//! ```
//!
//! `--slippage-bps` is required and must equal `slippage_bps` in the inputs.
//! Fees stay the engine grid 2/4/5/6 bps. The logged headline is the 5 bps run.
//! The ledger defaults to `research/trials.jsonl` (`--ledger` overrides it).
//! `{out}/results.jsonl` is one row per config. `{out}/stress.jsonl` is the
//! 5 bps fee plus 5 bps slippage case and is not a trial. Returns are not
//! printed.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use parangsae_backtest::{run_sweep, SweepRequest};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("batch: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let request = args()?;
    let _rows = run_sweep(&request)?;
    println!("ledger={}", request.ledger.display());
    println!("out={}", request.out_dir.display());
    Ok(())
}

fn args() -> Result<SweepRequest, String> {
    let mut inputs = None;
    let mut data_root = None;
    let mut out_dir = None;
    let mut ledger = None;
    let mut slippage_bps = None;
    let mut raw = env::args().skip(1);
    while let Some(arg) = raw.next() {
        if arg == "--help" {
            return Err(usage());
        }
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
            "--inputs" => inputs = Some(PathBuf::from(value)),
            "--data-root" => data_root = Some(PathBuf::from(value)),
            "--out" => out_dir = Some(PathBuf::from(value)),
            "--ledger" => ledger = Some(PathBuf::from(value)),
            "--slippage-bps" => {
                slippage_bps = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| format!("bad --slippage-bps {value}"))?,
                );
            }
            other => return Err(format!("unknown argument {other}\n{}", usage())),
        }
    }
    Ok(SweepRequest {
        inputs: inputs.ok_or_else(|| format!("missing --inputs\n{}", usage()))?,
        data_root: data_root.ok_or_else(|| format!("missing --data-root\n{}", usage()))?,
        out_dir: out_dir.ok_or_else(|| format!("missing --out\n{}", usage()))?,
        ledger: ledger.unwrap_or_else(|| PathBuf::from("research/trials.jsonl")),
        slippage_bps: slippage_bps.ok_or_else(|| format!("missing --slippage-bps\n{}", usage()))?,
    })
}

fn usage() -> String {
    "usage: batch --inputs <sweep-inputs-v2.json> --data-root <parquet-root> --out <dir> --slippage-bps <u32> [--ledger <trials.jsonl>]"
        .to_owned()
}
