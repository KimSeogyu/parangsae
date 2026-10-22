use parangsae_core::{Receipt, RunId, UtcMicros};

fn f(_run_id: RunId, r: Receipt) -> UtcMicros {
    match r {
        Receipt::Archive { .. } => UtcMicros::from_micros(0),
        Receipt::Live { received_ts } => received_ts,
    }
}

fn main() {
    let _ = f;
}
