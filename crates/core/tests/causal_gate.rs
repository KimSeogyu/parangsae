//! The public gate: a row is visible iff `available_ts <= decision`, and rows
//! that fail that test cannot change the visible values.

use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros,
};
use proptest::prelude::*;

fn observation(event: i64, available: i64, received: i64, value: i64) -> Observation<i64> {
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(event)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(received)),
        },
        value,
    )
}

fn visible(rows: &[Observation<i64>], decision: DecisionTime) -> Vec<i64> {
    rows.iter()
        .filter_map(|row| row.causal_at(decision).map(|sample| *sample.value()))
        .collect()
}

#[test]
fn decision_instant_is_inclusive() {
    let decision = DecisionTime::from_micros(UtcMicros::from_micros(10));
    let at = observation(1, 10, 12, 7);
    let after = observation(1, 11, 12, 9);
    assert_eq!(at.causal_at(decision).unwrap().value(), &7);
    assert!(after.causal_at(decision).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn visible_rows_are_exactly_those_available_by_decision(
        decision in 0i64..5_000,
        rows in prop::collection::vec((0i64..5_000, 0i64..5_000, 0i64..8_000, -50i64..50), 0..16),
        salt in 1i64..1_000,
    ) {
        let decision_time = DecisionTime::from_micros(UtcMicros::from_micros(decision));
        let rows: Vec<Observation<i64>> = rows
            .into_iter()
            .map(|(event, lag, recv_lag, value)| {
                observation(event, event + lag, event + recv_lag, value)
            })
            .collect();

        for row in &rows {
            let available = row.timestamps().available_ts.micros().micros();
            let gated = row.causal_at(decision_time);
            prop_assert_eq!(gated.is_some(), available <= decision);
            if let Some(sample) = gated {
                prop_assert_eq!(*sample.value(), *row.value());
                prop_assert!(sample.available_ts().micros().micros() <= decision);
            }
        }

        let before = visible(&rows, decision_time);
        let mutated: Vec<Observation<i64>> = rows
            .iter()
            .map(|row| {
                let available = row.timestamps().available_ts.micros().micros();
                if available > decision {
                    row.with_value(row.value().wrapping_add(salt))
                } else {
                    row.clone()
                }
            })
            .collect();
        prop_assert_eq!(before, visible(&mutated, decision_time));
    }
}
