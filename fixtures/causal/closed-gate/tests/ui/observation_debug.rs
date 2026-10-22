use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};

struct Secret;

fn main() {
    let Ok(row) = Observation::from_source(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(1)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(2)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(3)),
        },
        Secret,
    ) else {
        return;
    };
    let _rendered = format!("{row:?}");
}
