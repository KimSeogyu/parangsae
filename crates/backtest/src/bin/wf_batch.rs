//! Walk-forward batch. One trial per sweep config. OOS files only.
//!
//! ```text
//! cargo run -p parangsae-backtest --bin wf_batch -- \
//!   --inputs research/sweep-inputs-v2.json \
//!   --out runs/wf-batch-attempt1 \
//!   --attempt 1
//! ```
//!
//! `--attempt` is required. `--data-root` defaults to `/workspace/data/parangsae`.
//! The ledger defaults to `{out}/trials.jsonl`. Stdout names the files it wrote.
//! It does not print returns.
//! Both output and ledger paths must be absent, including empty paths. A failed
//! run keeps its claimed paths. Supported diagnostic runs use the checked
//! launcher, which requires `pinned_root_expected` in the inputs file and binds
//! the source and binary identity outside this program.

#![deny(clippy::float_arithmetic)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use parangsae_backtest::{run_wf_batch, WfBatchRequest, DEFAULT_DATA_ROOT};

fn main() -> ExitCode {
    match run() {
        Ok(paths) => {
            for line in paths {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("wf_batch: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<Vec<String>, String> {
    let request = parse_args(env::args().skip(1))?;
    run_wf_batch(&request)?;
    let mut lines = vec![
        format!("data_root={}", request.data_root.display()),
        format!("ledger={}", request.ledger.display()),
    ];
    let mut names: Vec<_> = std::fs::read_dir(&request.out_dir)
        .map_err(|err| format!("read {}: {err}", request.out_dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.ends_with(".json") || name == "fold_daily_returns.csv")
        .collect();
    names.sort();
    for name in names {
        lines.push(format!("{name}={}", request.out_dir.join(&name).display()));
    }
    Ok(lines)
}

fn parse_args(raw: impl Iterator<Item = String>) -> Result<WfBatchRequest, String> {
    let mut inputs = None;
    let mut data_root = None;
    let mut out_dir = None;
    let mut ledger = None;
    let mut attempt = None;
    let mut raw = raw;
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
            "--attempt" => {
                attempt = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| format!("attempt {value} is not a u64"))?,
                );
            }
            other => return Err(format!("unknown argument {other}\n{}", usage())),
        }
    }
    let out_dir = out_dir.ok_or_else(|| format!("missing --out\n{}", usage()))?;
    let ledger = ledger.unwrap_or_else(|| out_dir.join("trials.jsonl"));
    let attempt = attempt.ok_or_else(|| format!("missing --attempt\n{}", usage()))?;
    Ok(WfBatchRequest {
        inputs: inputs.ok_or_else(|| format!("missing --inputs\n{}", usage()))?,
        data_root: data_root.unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_ROOT)),
        out_dir,
        ledger,
        attempt,
    })
}

fn usage() -> String {
    format!(
        "usage: wf_batch --inputs <sweep-inputs-v2.json> --out <dir> --attempt <n> [--data-root <parquet-root>] [--ledger <trials.jsonl>]\n  --data-root defaults to {DEFAULT_DATA_ROOT}\n  --ledger defaults to <out>/trials.jsonl"
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::parse_args;
    use parangsae_backtest::DEFAULT_DATA_ROOT;

    fn owned(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn data_root_defaults_to_the_parangsae_layout() {
        let request = parse_args(
            owned(&["--inputs", "in.json", "--out", "out", "--attempt", "1"]).into_iter(),
        )
        .unwrap();
        assert_eq!(request.data_root, PathBuf::from(DEFAULT_DATA_ROOT));
        assert_eq!(request.ledger, PathBuf::from("out/trials.jsonl"));
        assert_eq!(request.inputs, PathBuf::from("in.json"));
        assert_eq!(request.attempt, 1);
        let rooted = parse_args(
            owned(&[
                "--inputs",
                "in.json",
                "--data-root",
                "data",
                "--out",
                "out",
                "--ledger",
                "log.jsonl",
                "--attempt=1",
            ])
            .into_iter(),
        )
        .unwrap();
        assert_eq!(rooted.data_root, PathBuf::from("data"));
        assert_eq!(rooted.ledger, PathBuf::from("log.jsonl"));
        assert_eq!(rooted.attempt, 1);
        let missing = parse_args(owned(&["--inputs", "in.json", "--out", "out"]).into_iter());
        match missing {
            Err(message) => assert!(message.contains("missing --attempt"), "{message}"),
            Ok(_) => panic!("missing --attempt was accepted"),
        }
    }
}
