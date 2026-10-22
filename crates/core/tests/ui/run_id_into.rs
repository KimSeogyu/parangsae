use parangsae_core::{Receipt, RunId, UtcMicros};

fn f(run_id: RunId, _r: Receipt) -> UtcMicros {
    run_id.into()
}

fn main() {
    let _ = f;
}
