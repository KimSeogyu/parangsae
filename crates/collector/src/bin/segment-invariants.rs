//! Report whether a before/after segment fixture breaks an IngestSegment invariant.
//!
//! Exit 1 when the fixture's expected invariant breaks. Exit 0 when the pair
//! satisfies the invariants. A weakened checker that accepts the append-while-gap
//! fixture exits 0, and CI treats that as a failure.

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use parangsae_collector::segment::pair_fault;

fn main() -> Result<ExitCode, io::Error> {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        writeln!(io::stderr().lock(), "usage: segment-invariants <fixture>")?;
        return Ok(ExitCode::from(2));
    };
    if args.next().is_some() {
        writeln!(io::stderr().lock(), "usage: segment-invariants <fixture>")?;
        return Ok(ExitCode::from(2));
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            writeln!(io::stderr().lock(), "read {path}: {err}")?;
            return Ok(ExitCode::from(2));
        }
    };
    match pair_fault(&text) {
        Ok(Some(invariant)) => {
            writeln!(io::stderr().lock(), "{invariant}")?;
            Ok(ExitCode::from(1))
        }
        Ok(None) => Ok(ExitCode::SUCCESS),
        Err(err) => {
            writeln!(io::stderr().lock(), "{err}")?;
            Ok(ExitCode::from(2))
        }
    }
}
