//! `cargo xtask depgraph` checks `policy/deps.toml` against `cargo metadata`.

#![allow(
    clippy::print_stderr,
    reason = "xtask prints dependency and lint violations for the operator"
)]

use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args
        .next()
        .ok_or_else(|| "usage: cargo xtask depgraph".to_owned())?;
    if args.next().is_some() {
        return Err(format!("unexpected argument after {command}"));
    }
    let root = xtask::workspace_root()?;
    match command.as_str() {
        "depgraph" => {
            let violations = xtask::lints::workspace_inheritance(&root)?;
            if !violations.is_empty() {
                return Err(violations.join("\n"));
            }
            let violations = xtask::depgraph::workspace(&root)?;
            if violations.is_empty() {
                Ok(())
            } else {
                Err(violations.join("\n"))
            }
        }
        other => Err(format!("unknown command {other}")),
    }
}
