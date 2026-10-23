use parangsae_core::{AvailableTs, Causal, UtcMicros};

fn main() {
    let value = 1_i64;
    let _row = Causal {
        available_ts: AvailableTs::from_micros(UtcMicros::from_micros(1)),
        value: &value,
    };
}
