//! `cargo xtask depgraph` checks `policy/deps.toml` against `cargo metadata`.
//! `cargo xtask guards` checks feature ownership, TLC config coverage, and floors.
//! `cargo xtask run-exact <full::path>` runs one xtask lib test and fails unless it passed.

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> io::Result<ExitCode> {
    match run() {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(err) => {
            writeln!(io::stderr().lock(), "{err}")?;
            Ok(ExitCode::from(1))
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args
        .next()
        .ok_or_else(|| "usage: cargo xtask <depgraph|guards|run-exact>".to_owned())?;
    let root = xtask::workspace_root()?;
    match command.as_str() {
        "depgraph" => {
            if args.next().is_some() {
                return Err("unexpected argument after depgraph".to_owned());
            }
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
        "guards" => xtask::guards::run(&root, &args.collect::<Vec<_>>()),
        "run-exact" => {
            let test = args
                .next()
                .ok_or_else(|| "usage: cargo xtask run-exact <test-path>".to_owned())?;
            if args.next().is_some() {
                return Err("unexpected argument after run-exact".to_owned());
            }
            xtask::run_exact(&root, &test)
        }
        other => Err(format!("unknown command {other}")),
    }
}
