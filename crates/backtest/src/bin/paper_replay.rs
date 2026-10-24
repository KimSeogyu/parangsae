//! Scripted engineering replay. This binary reads no operational market data.

use parangsae_backtest::offline_replay::{OfflineReplay, PaperFixture};
use std::io::Read;
use std::path::Path;

fn main() {
    if let Err(error) = run() {
        eprintln!("paper replay refused: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 && args.len() != 6 {
        return Err("usage: paper_replay <run|resume> <fixture.json> <journal> [--stop-after <total-input-count>]".into());
    }
    let stop = if args.len() == 6 {
        if args[4] != "--stop-after" {
            return Err("expected --stop-after".into());
        }
        Some(args[5].parse::<usize>()?)
    } else {
        None
    };
    let file = std::fs::File::open(&args[2])?;
    const MAX_FIXTURE: u64 = 67_108_864;
    let mut bytes = Vec::new();
    file.take(MAX_FIXTURE + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len())? > MAX_FIXTURE {
        return Err("paper fixture exceeds 64 MiB".into());
    }
    let fixture: PaperFixture = serde_json::from_slice(&bytes)?;
    let limit = stop.unwrap_or(fixture.commands.len());
    if limit > fixture.commands.len() {
        return Err("stop count is outside the remaining fixture".into());
    }
    let path = Path::new(&args[3]);
    let mut run = match args[1].as_str() {
        "run" => OfflineReplay::create(path, fixture.session.clone())?,
        "resume" => OfflineReplay::resume(path)?,
        _ => return Err("expected run or resume".into()),
    };
    run.validate_fixture(&fixture)?;
    let completed = run.command_count();
    if limit < completed {
        return Err("stop count is outside the remaining fixture".into());
    }
    for command in fixture.commands.iter().take(limit).skip(completed) {
        let result = run.apply(command.clone())?;
        println!("input {}: {:?}", run.command_count(), result.outcome);
    }
    let state = run.snapshot();
    println!(
        "cash_ticks={} position_ticks={} fee_ticks={} economic_fills={} halted={} exposure={:?}",
        state.cash.0,
        state.position.0,
        state.fees_paid.0,
        state.fills.len(),
        state.risk.halted,
        state.risk.exposure
    );
    for order in state.orders {
        println!(
            "order {} state={} cumulative_ticks={}",
            order.client_order_id,
            order.progress.state(),
            order.progress.cum().ticks()
        );
    }
    Ok(())
}
