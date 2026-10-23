use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};

fn main() {
    let row = Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(1)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(1)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(1)),
        },
        1_i64,
    );
    let _other = row.with_value(2_i64);
}
