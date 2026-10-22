use parangsae_core::{DecisionTime, UtcMicros};

fn main() {
    let _decision = DecisionTime::for_test(UtcMicros::from_micros(1));
}
