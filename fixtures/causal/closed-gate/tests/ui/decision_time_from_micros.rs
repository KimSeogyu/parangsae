use parangsae_core::{DecisionTime, UtcMicros};

fn main() {
    let _decision = DecisionTime::from_micros(UtcMicros::from_micros(1));
}
