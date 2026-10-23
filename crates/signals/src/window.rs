//! Capped causal window. The only constructor re-checks the decision gate.

use parangsae_core::{AvailableTs, Causal, DecisionTime, Observation};
use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroUsize;

use crate::feature::FeatureOutput;
pub use crate::output::WindowError;
use crate::output::{SignalError, SignalOutput};

/// The last `cap` causal rows at one decision, oldest first.
#[derive(Debug)]
pub struct CausalWindow<'a, T> {
    decision: DecisionTime,
    cap: NonZeroUsize,
    rows: VecDeque<Causal<'a, T>>,
}

impl<'a, T> CausalWindow<'a, T> {
    /// Keep the last `cap` views in availability order.
    ///
    /// Every view is checked against `decision`, including views the cap drops.
    pub fn from_views(
        decision: DecisionTime,
        cap: NonZeroUsize,
        views: impl IntoIterator<Item = Causal<'a, T>>,
    ) -> Result<Self, WindowError> {
        let mut rows = VecDeque::new();
        let mut previous: Option<AvailableTs> = None;
        for view in views {
            if view.available_ts().micros() > decision.micros() {
                return Err(WindowError::AfterDecision);
            }
            if let Some(prev) = previous {
                if view.available_ts() < prev {
                    return Err(WindowError::Unordered);
                }
            }
            previous = Some(view.available_ts());
            rows.push_back(view);
            if rows.len() > cap.get() {
                rows.pop_front();
            }
        }
        Ok(Self {
            decision,
            cap,
            rows,
        })
    }

    pub const fn decision(&self) -> DecisionTime {
        self.decision
    }

    pub const fn cap(&self) -> NonZeroUsize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Causal<'a, T>> {
        self.rows.iter()
    }

    pub fn last(&self) -> Option<&Causal<'a, T>> {
        self.rows.back()
    }
}

/// A pure target signal. `WINDOW_CAP` is the exact lookback, not a ceiling.
pub trait CausalSignal {
    const ID: &'static str;
    const WINDOW_CAP: NonZeroUsize;
    type Row: Copy + fmt::Debug + 'static;

    fn evaluate(
        &self,
        window: &CausalWindow<'_, Self::Row>,
    ) -> Result<Option<SignalOutput>, SignalError>;
}

/// A pure feature. There is no blanket impl to or from [`CausalSignal`].
pub trait CausalFeature {
    const ID: &'static str;
    const WINDOW_CAP: NonZeroUsize;
    type Row: Copy + fmt::Debug + 'static;

    fn evaluate(
        &self,
        window: &CausalWindow<'_, Self::Row>,
    ) -> Result<Option<FeatureOutput>, SignalError>;
}

/// Engine-facing adapter. Rows pass through `causal_at` before the window.
pub fn drive<S: CausalSignal>(
    signal: &S,
    rows: &[Observation<S::Row>],
    decision: DecisionTime,
) -> Result<Option<SignalOutput>, SignalError> {
    let views = rows.iter().filter_map(|row| row.causal_at(decision));
    let window = CausalWindow::from_views(decision, S::WINDOW_CAP, views)?;
    signal.evaluate(&window)
}

/// Same adapter as [`drive`], for a [`CausalFeature`].
pub fn drive_feature<F: CausalFeature>(
    feature: &F,
    rows: &[Observation<F::Row>],
    decision: DecisionTime,
) -> Result<Option<FeatureOutput>, SignalError> {
    let views = rows.iter().filter_map(|row| row.causal_at(decision));
    let window = CausalWindow::from_views(decision, F::WINDOW_CAP, views)?;
    feature.evaluate(&window)
}

pub(crate) const fn nonzero(cap: usize) -> NonZeroUsize {
    match NonZeroUsize::new(cap) {
        Some(value) => value,
        None => panic!("window cap is zero"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{
        AvailableTs, DecisionTime, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros,
    };
    use std::num::NonZeroUsize;

    fn obs(available: i64, value: i64) -> Observation<i64> {
        Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(UtcMicros::from_micros(available)),
                available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
                received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(available)),
            },
            value,
        )
    }

    fn at(micros: i64) -> DecisionTime {
        DecisionTime::from_micros(UtcMicros::from_micros(micros))
    }

    #[test]
    fn cap_keeps_the_newest_rows() {
        let rows = [obs(1, 10), obs(2, 20), obs(3, 30), obs(4, 40)];
        let views: Vec<_> = rows.iter().filter_map(|row| row.causal_at(at(4))).collect();
        let cap = NonZeroUsize::new(2).unwrap();
        let window = CausalWindow::from_views(at(4), cap, views).unwrap();
        let values: Vec<_> = window.iter().map(|view| *view.value()).collect();
        assert_eq!(values, vec![30, 40]);
        assert_eq!(window.last().unwrap().value(), &40);
        assert_eq!(window.len(), 2);
    }

    #[test]
    fn a_later_gate_does_not_admit_a_row_at_an_earlier_decision() {
        let row = obs(10, 1);
        let view = row.causal_at(at(10)).unwrap();
        let err = CausalWindow::from_views(at(5), NonZeroUsize::new(4).unwrap(), [view]);
        assert!(matches!(err, Err(WindowError::AfterDecision)));
    }

    #[test]
    fn decreasing_availability_is_unordered_and_ties_are_kept() {
        let rows = [obs(3, 1), obs(1, 2)];
        let views: Vec<_> = rows.iter().filter_map(|row| row.causal_at(at(3))).collect();
        let err = CausalWindow::from_views(at(3), NonZeroUsize::new(4).unwrap(), views);
        assert!(matches!(err, Err(WindowError::Unordered)));

        let tied = [obs(2, 1), obs(2, 2)];
        let views: Vec<_> = tied.iter().filter_map(|row| row.causal_at(at(2))).collect();
        let window = CausalWindow::from_views(at(2), NonZeroUsize::new(4).unwrap(), views).unwrap();
        assert_eq!(window.len(), 2);
    }
}
