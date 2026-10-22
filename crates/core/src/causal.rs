//! Type-level lookahead gate.
//!
//! [`Causal`] has private fields and no public constructor. The only way to
//! obtain one is [`Observation::causal_at`], which returns `Some` only when
//! `available_ts <= decision`. [`Observation::from_source`] is the public
//! producer. [`Observation::value`] stays crate-private, so another crate
//! reads the payload only through [`Causal::value`].
//!
//! `from_source` rejects `available_ts < event_ts`. [`DecisionTime::from_micros`]
//! is public only with the `driver-clock` feature, which `parangsae-sim` is
//! the only normal dependency allowed to enable. `test-support` exposes
//! [`Observation::for_test`] and [`DecisionTime::for_test`] for tests and is
//! not a default feature.

use std::fmt;

use crate::time::UtcMicros;
use crate::timestamps::{AvailableTs, Timestamps};

/// Time at which a signal is evaluated. Compared with `available_ts` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DecisionTime(UtcMicros);

impl DecisionTime {
    /// Public only for `parangsae-sim`, which enables `driver-clock`.
    /// Unit tests see the same constructor; other crates do not.
    #[cfg(any(feature = "driver-clock", test))]
    pub const fn from_micros(ts: UtcMicros) -> Self {
        Self(ts)
    }

    /// Same clock as [`Self::from_micros`], available only under `test-support`.
    #[cfg(feature = "test-support")]
    pub const fn for_test(ts: UtcMicros) -> Self {
        Self(ts)
    }

    pub const fn micros(self) -> UtcMicros {
        self.0
    }
}

/// `available_ts` is earlier than `event_ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationError {
    AvailableBeforeEvent,
}

impl fmt::Display for ObservationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AvailableBeforeEvent => write!(f, "available_ts is before event_ts"),
        }
    }
}

/// A sourced row. `Debug` prints the timestamps and not `value`. There is no
/// `PartialEq`/`Eq`: equality would compare the private payload.
#[derive(Clone)]
pub struct Observation<T> {
    timestamps: Timestamps,
    value: T,
}

impl<T> fmt::Debug for Observation<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Observation")
            .field("timestamps", &self.timestamps)
            .finish_non_exhaustive()
    }
}

impl<T> Observation<T> {
    pub(crate) fn new(timestamps: Timestamps, value: T) -> Result<Self, ObservationError> {
        if timestamps.available_ts.micros().micros() < timestamps.event_ts.micros().micros() {
            return Err(ObservationError::AvailableBeforeEvent);
        }
        Ok(Self { timestamps, value })
    }

    /// Trusted producer. The payload stays private; readers use [`Self::causal_at`].
    pub fn from_source(timestamps: Timestamps, value: T) -> Result<Self, ObservationError> {
        Self::new(timestamps, value)
    }

    /// Test constructor. Still rejects `available_ts < event_ts`.
    #[cfg(feature = "test-support")]
    pub fn for_test(timestamps: Timestamps, value: T) -> Result<Self, ObservationError> {
        Self::new(timestamps, value)
    }

    pub const fn timestamps(&self) -> Timestamps {
        self.timestamps
    }

    pub(crate) const fn value(&self) -> &T {
        &self.value
    }

    pub fn causal_at(&self, decision: DecisionTime) -> Option<Causal<'_, T>> {
        if self.timestamps.available_ts.micros() <= decision.micros() {
            Some(Causal {
                available_ts: self.timestamps.available_ts,
                value: self.value(),
            })
        } else {
            None
        }
    }
}

/// A value a signal is allowed to read at the decision time that produced it.
#[derive(Debug, PartialEq, Eq)]
pub struct Causal<'a, T> {
    available_ts: AvailableTs,
    value: &'a T,
}

impl<'a, T> Causal<'a, T> {
    pub const fn available_ts(&self) -> AvailableTs {
        self.available_ts
    }

    pub const fn value(&self) -> &'a T {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timestamps::{EventTs, ReceivedTs};
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;

    fn timestamps(event: i64, available: i64, received: i64) -> Timestamps {
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(event)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(received)),
        }
    }

    fn observation(
        event: i64,
        available: i64,
        received: i64,
        value: i64,
    ) -> Result<Observation<i64>, String> {
        Observation::from_source(timestamps(event, available, received), value)
            .map_err(|err| err.to_string())
    }

    fn visible(rows: &[Observation<i64>], decision: DecisionTime) -> Vec<i64> {
        rows.iter()
            .filter_map(|row| row.causal_at(decision).map(|sample| *sample.value()))
            .collect()
    }

    /// Spec of "not yet available", independent of [`Observation::causal_at`].
    fn not_yet_available(available_us: i64, decision_us: i64) -> bool {
        available_us > decision_us
    }

    #[test]
    fn available_before_event_fixture_is_rejected() -> Result<(), String> {
        let text = include_str!("../../../fixtures/causal/available_before_event.txt");
        let mut event = None;
        let mut available = None;
        let mut received = None;
        let mut expect = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("fixture line has no '=': {line}"))?;
            match key.trim() {
                "event_us" => event = Some(parse_i64(value)?),
                "available_us" => available = Some(parse_i64(value)?),
                "received_us" => received = Some(parse_i64(value)?),
                "expect" => expect = Some(value.trim().to_owned()),
                other => return Err(format!("unknown fixture key {other}")),
            }
        }
        let event = event.ok_or("fixture missing event_us")?;
        let available = available.ok_or("fixture missing available_us")?;
        let received = received.ok_or("fixture missing received_us")?;
        let expect = expect.ok_or("fixture missing expect")?;
        match observation(event, available, received, 1) {
            Err(message) => {
                if message.contains(&expect) {
                    Ok(())
                } else {
                    Err(format!("error {message:?} did not contain {expect:?}"))
                }
            }
            Ok(_) => Err("available_ts before event_ts was accepted".to_owned()),
        }
    }

    fn parse_i64(value: &str) -> Result<i64, String> {
        value.trim().parse::<i64>().map_err(|err| err.to_string())
    }

    #[test]
    fn available_equal_to_event_is_visible_at_that_decision() -> Result<(), String> {
        let row = observation(4, 4, 9, 3)?;
        let decision = DecisionTime::from_micros(UtcMicros::from_micros(4));
        let sample = row
            .causal_at(decision)
            .ok_or("available_ts equal to event_ts is a valid row")?;
        if sample.value() == &3 {
            Ok(())
        } else {
            Err(format!("visible value {}", sample.value()))
        }
    }

    #[test]
    fn decision_instant_is_inclusive() -> Result<(), String> {
        let decision = DecisionTime::from_micros(UtcMicros::from_micros(10));
        let at = observation(1, 10, 12, 7)?;
        let after = observation(1, 11, 12, 9)?;
        let sample = at
            .causal_at(decision)
            .ok_or("row available at the decision is visible")?;
        if sample.value() != &7 {
            return Err(format!("visible value {}", sample.value()));
        }
        if after.causal_at(decision).is_some() {
            return Err("row available after the decision was visible".to_owned());
        }
        Ok(())
    }

    #[test]
    fn appending_a_row_one_micros_later_does_not_change_the_view() -> Result<(), String> {
        let decision_us = 10;
        let decision = DecisionTime::from_micros(UtcMicros::from_micros(decision_us));
        let row = observation(1, decision_us, decision_us, 4)?;
        let later_us = decision_us + 1;
        if !not_yet_available(later_us, decision_us) {
            return Err("the appended row is already available under the spec".to_owned());
        }
        let later = observation(1, later_us, later_us, 99)?;
        let before = visible(std::slice::from_ref(&row), decision);
        let after = visible(&[row, later], decision);
        if before == after {
            Ok(())
        } else {
            Err(format!("view changed from {before:?} to {after:?}"))
        }
    }

    #[test]
    fn debug_prints_timestamps_and_not_the_payload() -> Result<(), String> {
        let row = observation(1, 2, 3, 0x5E47_1A11_i64)?;
        let rendered = format!("{row:?}");
        if rendered.contains("5e471a11") || rendered.contains("1581718033") {
            return Err(format!("payload leaked into Debug: {rendered}"));
        }
        if rendered.contains("timestamps") {
            Ok(())
        } else {
            Err(format!("timestamps missing from Debug: {rendered}"))
        }
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn for_test_rejects_available_before_event() -> Result<(), String> {
        match Observation::for_test(timestamps(10, 9, 9), 1_i64) {
            Err(ObservationError::AvailableBeforeEvent) => {}
            Ok(_) => return Err("for_test accepted available_ts before event_ts".to_owned()),
        }
        let row =
            Observation::for_test(timestamps(4, 4, 4), 8_i64).map_err(|err| err.to_string())?;
        let decision = DecisionTime::for_test(UtcMicros::from_micros(4));
        let sample = row
            .causal_at(decision)
            .ok_or("for_test row is not visible at available_ts")?;
        if sample.value() == &8 {
            Ok(())
        } else {
            Err(format!("for_test visible value {}", sample.value()))
        }
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
            let mut built = Vec::new();
            for (event, lag, recv_lag, value) in rows {
                let available = event + lag;
                let received = event + recv_lag;
                match observation(event, available, received, value) {
                    Ok(row) => built.push(row),
                    Err(err) => return Err(TestCaseError::fail(err)),
                }
            }

            for row in &built {
                let available = row.timestamps().available_ts.micros().micros();
                let gated = row.causal_at(decision_time);
                prop_assert_eq!(gated.is_some(), available <= decision);
                if let Some(sample) = gated {
                    prop_assert_eq!(*sample.value(), *row.value());
                    prop_assert!(sample.available_ts().micros().micros() <= decision);
                }
            }

            let before = visible(&built, decision_time);
            let mut mutated = Vec::new();
            for row in &built {
                let available = row.timestamps().available_ts.micros().micros();
                if not_yet_available(available, decision) {
                    match Observation::from_source(row.timestamps(), row.value().wrapping_add(salt)) {
                        Ok(rewritten) => mutated.push(rewritten),
                        Err(err) => return Err(TestCaseError::fail(err.to_string())),
                    }
                } else {
                    mutated.push(row.clone());
                }
            }
            prop_assert_eq!(before, visible(&mutated, decision_time));
        }

        #[test]
        fn dropping_or_appending_not_yet_available_rows_leaves_the_view(
            decision in 0i64..5_000,
            rows in prop::collection::vec((0i64..5_000, 0i64..5_000, 0i64..8_000, -50i64..50), 0..16),
            extra in prop::collection::vec((0i64..20, -50i64..50), 0..8),
        ) {
            let decision_time = DecisionTime::from_micros(UtcMicros::from_micros(decision));
            let mut built = Vec::new();
            for (event, lag, recv_lag, value) in rows {
                match observation(event, event + lag, event + recv_lag, value) {
                    Ok(row) => built.push(row),
                    Err(err) => return Err(TestCaseError::fail(err)),
                }
            }
            let before = visible(&built, decision_time);

            let kept: Vec<Observation<i64>> = built
                .iter()
                .filter(|row| {
                    let available = row.timestamps().available_ts.micros().micros();
                    !not_yet_available(available, decision)
                })
                .cloned()
                .collect();
            prop_assert_eq!(before.clone(), visible(&kept, decision_time));

            let mut appended = built.clone();
            for (lag, value) in extra {
                let available = decision + 1 + lag;
                prop_assert!(not_yet_available(available, decision));
                match observation(0, available, available, value) {
                    Ok(row) => appended.push(row),
                    Err(err) => return Err(TestCaseError::fail(err)),
                }
            }
            prop_assert_eq!(before, visible(&appended, decision_time));
        }
    }
}
