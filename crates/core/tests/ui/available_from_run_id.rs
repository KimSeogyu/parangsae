use parangsae_core::{AvailableTs, Receipt, RunId, UtcMicros};

fn f(run_id: RunId, _r: Receipt) -> UtcMicros {
    AvailableTs::from_micros(run_id).micros()
}

fn main() {
    let _ = f;
}
