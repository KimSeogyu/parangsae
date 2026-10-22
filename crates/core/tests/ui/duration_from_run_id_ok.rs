use parangsae_core::{Receipt, RunId, UtcMicros};

fn f(_run_id: RunId, r: Receipt) -> UtcMicros {
    let _duration = std::time::Duration::from_micros(0);
    match r {
        Receipt::Archive { .. } => UtcMicros::from_micros(0),
        Receipt::Live { received_ts } => received_ts,
    }
}

fn main() {
    let _ = f;
}
