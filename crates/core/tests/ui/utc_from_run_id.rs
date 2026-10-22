use parangsae_core::{Receipt, RunId, UtcMicros};

fn f(run_id: RunId, _r: Receipt) -> UtcMicros {
    UtcMicros::from(run_id)
}

fn main() {
    let _ = f;
}
