use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};
use parangsae_signals::sample_value;
use parangsae_sim::Driver;

#[test]
fn producer_reaches_a_signal_only_through_the_gate() -> Result<(), String> {
    let row = Observation::from_source(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(1)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(10)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(12)),
        },
        7_i64,
    )
    .map_err(|err| err.to_string())?;
    let decision = Driver::decision_at(UtcMicros::from_micros(10));
    let sample = row
        .causal_at(decision)
        .ok_or("row available at the decision is visible")?;
    if *sample_value(&sample) != 7 {
        return Err(format!("signal read {}", sample_value(&sample)));
    }
    let early = Driver::decision_at(UtcMicros::from_micros(9));
    if row.causal_at(early).is_some() {
        return Err("row was visible before available_ts".to_owned());
    }
    Ok(())
}
