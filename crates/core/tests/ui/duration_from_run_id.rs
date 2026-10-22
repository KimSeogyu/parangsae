use parangsae_core::{Receipt, RunId, UtcMicros};

fn f(run_id: RunId, _r: Receipt) -> UtcMicros {
    let _duration = std::time::Duration::from(run_id);
    UtcMicros::from_micros(0)
}

fn main() {
    let _ = f;
}
