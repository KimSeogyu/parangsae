use parangsae_core::{Receipt, RunId, UtcMicros};

fn f(_run_id: RunId, r: Receipt) -> UtcMicros {
    match r {
        Receipt::Archive { fetched_at_run } => fetched_at_run.micros(),
        Receipt::Live { received_ts } => received_ts,
    }
}

fn main() {
    let _ = f;
}
