use std::ffi::OsString;
use std::fs::File;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::ExitCode;

use parangsae_backtest::{run_c1_batch, C1BatchRequest};

const USAGE: &str = "c1_batch --inputs <absolute C1 JSON> --data-root <absolute snapshot> --out <new absolute directory> --ledger <out>/trials.jsonl --attempt <integer>";

fn parse(args: impl Iterator<Item = OsString>) -> Result<C1BatchRequest, String> {
    let mut args = args;
    let mut inputs = None;
    let mut root = None;
    let mut out = None;
    let mut ledger = None;
    let mut attempt = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value: {}", flag.to_string_lossy()))?;
        let slot = match flag.to_str() {
            Some("--inputs") => &mut inputs,
            Some("--data-root") => &mut root,
            Some("--out") => &mut out,
            Some("--ledger") => &mut ledger,
            Some("--attempt") => {
                if attempt.is_some() {
                    return Err("duplicate --attempt".to_owned());
                }
                attempt = Some(
                    value
                        .to_str()
                        .ok_or("attempt is not UTF8")?
                        .parse::<u64>()
                        .map_err(|_| "attempt is not an unsigned integer")?,
                );
                continue;
            }
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        };
        if slot.is_some() {
            return Err(format!("duplicate argument: {}", flag.to_string_lossy()));
        }
        *slot = Some(PathBuf::from(value));
    }
    Ok(C1BatchRequest {
        inputs: inputs.ok_or("missing --inputs")?,
        data_root: root.ok_or("missing --data-root")?,
        out: out.ok_or("missing --out")?,
        ledger: ledger.ok_or("missing --ledger")?,
        attempt: attempt.ok_or("missing --attempt")?,
    })
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let request = match parse(args.into_iter()) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match run_c1_batch(&request) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let stderr = std::io::stderr();
            let _lock = stderr.lock();
            match stderr.as_fd().try_clone_to_owned() {
                Ok(fd) => {
                    // File preserves write errors that the standard stream can suppress.
                    let mut sink = File::from(fd);
                    match error.handoff(&mut sink) {
                        Ok(_retained) => ExitCode::FAILURE,
                        // Process exit cannot preserve an unacknowledged in-memory payload.
                        Err((_retained, _handoff_error)) => ExitCode::from(74),
                    }
                }
                Err(_handoff_error) => {
                    let _retained = error;
                    ExitCode::from(74)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_explicit_cli_values_reach_the_actual_request_without_defaults() {
        let request = parse(
            [
                "--attempt",
                "4",
                "--out",
                "/tmp/new-c1-output",
                "--inputs",
                "/tmp/c1-snapshot/c1-inputs-v1.json",
                "--ledger",
                "/tmp/new-c1-output/trials.jsonl",
                "--data-root",
                "/tmp/c1-snapshot",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .unwrap();
        assert_eq!(request.attempt, 4);
        assert_eq!(request.out, PathBuf::from("/tmp/new-c1-output"));
        assert_eq!(request.ledger, request.out.join("trials.jsonl"));
        assert!(parse(["--attempt", "4"].into_iter().map(OsString::from)).is_err());
        assert!(parse(
            ["--attempt", "4", "--attempt", "5"]
                .into_iter()
                .map(OsString::from)
        )
        .is_err());
        assert!(parse(["--unknown", "x"].into_iter().map(OsString::from)).is_err());
    }
}
