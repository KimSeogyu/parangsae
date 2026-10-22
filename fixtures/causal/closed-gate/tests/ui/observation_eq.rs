use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};

fn main() {
    let Ok(row) = Observation::from_source(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(1)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(1)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(1)),
        },
        1_i64,
    ) else {
        return;
    };
    let _same = row == row;
}
