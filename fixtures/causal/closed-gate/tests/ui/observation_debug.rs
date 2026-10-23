use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};

#[derive(Debug)]
struct Secret;

fn main() {
    let row = Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(1)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(2)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(3)),
        },
        Secret,
    );
    let _rendered = format!("{row:?}");
}
