//! Pure Astra feature inputs. Execution and money remain with the Backtest owner.

pub mod range;

use parangsae_core::{BarInterval, ResampledBar};
use parangsae_signals::{CausalWindow, SignalError};

use crate::StrategyError;

const HOUR_US: i64 = 3_600_000_000;
const MILLISECOND_US: i64 = 1_000;

/// Admit the exact newest complete H1 dependency window at end plus one millisecond.
///
/// `None` means unavailable history, candidate or a known key gap. It is never a
/// false entry condition. Causal views carry availability, not original event or
/// receipt provenance; those source contracts belong to input admission.
pub(super) fn prepare_h1<'a>(
    window: &CausalWindow<'a, ResampledBar>,
    count: usize,
) -> Result<Option<Vec<&'a ResampledBar>>, StrategyError> {
    if count == 0 || count > window.cap().get() {
        return Err(StrategyError::Param("window_cap"));
    }
    let end = window
        .decision()
        .micros()
        .micros()
        .checked_sub(MILLISECOND_US)
        .ok_or(StrategyError::Overflow)?;
    if end.rem_euclid(HOUR_US) != 0 {
        return Err(StrategyError::Clock);
    }
    let expected_candidate = end.checked_sub(HOUR_US).ok_or(StrategyError::Overflow)?;
    let mut rows = Vec::with_capacity(count);
    let mut previous_open: Option<i64> = None;
    let mut gap = false;
    for view in window.iter().skip(window.len().saturating_sub(count)) {
        let row = view.value();
        if row.interval() != BarInterval::H1 {
            return Err(StrategyError::Interval);
        }
        if [row.open(), row.high(), row.low(), row.close()]
            .iter()
            .any(|price| price.ticks() <= 0)
        {
            return Err(StrategyError::Price);
        }
        let canonical = row
            .close_time()
            .checked_add_micros(MILLISECOND_US)
            .ok_or(StrategyError::Overflow)?;
        if view.available_ts().micros() != canonical {
            return Err(StrategyError::Clock);
        }
        let open = row.open_time().micros();
        if let Some(previous) = previous_open {
            if open == previous {
                return Err(StrategyError::Signal(SignalError::DuplicateBar));
            }
            let expected = previous
                .checked_add(HOUR_US)
                .ok_or(StrategyError::Overflow)?;
            if open < expected {
                return Err(StrategyError::Clock);
            }
            gap |= open != expected;
        }
        previous_open = Some(open);
        rows.push(row);
    }
    let Some(candidate) = rows.last() else {
        return Ok(None);
    };
    if candidate.open_time().micros() > expected_candidate {
        return Err(StrategyError::Clock);
    }
    if rows.len() != count || candidate.open_time().micros() != expected_candidate || gap {
        return Ok(None);
    }
    Ok(Some(rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{
        AvailableTs, DecisionTime, EventTs, Observation, Price, ReceivedTs, Timestamps, UtcMicros,
    };
    use std::num::NonZeroUsize;

    fn row(hour: i64) -> Observation<ResampledBar> {
        row_with_price(hour, 10)
    }

    fn row_with_price(hour: i64, ticks: i64) -> Observation<ResampledBar> {
        let price = Price::from_ticks(ticks).expect("nonnegative fixture price");
        let bar = ResampledBar::new(
            UtcMicros::from_micros(hour * HOUR_US),
            BarInterval::H1,
            price,
            price,
            price,
            price,
        )
        .expect("aligned fixture");
        let end = bar.close_time().checked_add_micros(1_000).expect("end");
        Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(bar.close_time()),
                available_ts: AvailableTs::from_micros(end),
                received_ts: ReceivedTs::from_micros(end),
            },
            bar,
        )
    }

    fn window<'a>(
        rows: &'a [Observation<ResampledBar>],
        end_hour: i64,
    ) -> CausalWindow<'a, ResampledBar> {
        let decision =
            DecisionTime::from_micros(UtcMicros::from_micros(end_hour * HOUR_US + 1_000));
        CausalWindow::from_views(
            decision,
            NonZeroUsize::new(2).expect("cap"),
            rows.iter().filter_map(|row| row.causal_at(decision)),
        )
        .expect("causal fixture window")
    }

    #[test]
    fn exact_candidate_and_capped_dependencies_are_required() {
        let rows = [row(0), row(1), row(2)];
        let view = window(&rows, 3);
        let prepared = prepare_h1(&view, 2).expect("contract").expect("ready");
        assert_eq!(prepared[0].open_time().micros(), HOUR_US);
        assert_eq!(prepared[1].open_time().micros(), 2 * HOUR_US);
        assert!(prepare_h1(&window(&rows[..2], 3), 2)
            .expect("contract")
            .is_none());
        assert!(prepare_h1(&window(&rows[..1], 1), 2)
            .expect("contract")
            .is_none());
    }

    #[test]
    fn gap_is_unavailable_and_duplicate_is_a_contract_error() {
        let rows = [row(0), row(2)];
        assert!(prepare_h1(&window(&rows, 3), 2)
            .expect("contract")
            .is_none());
        let duplicate = [row(2), row(2)];
        assert_eq!(
            prepare_h1(&window(&duplicate, 3), 2),
            Err(StrategyError::Signal(SignalError::DuplicateBar))
        );
    }

    #[test]
    fn a_zero_price_is_not_a_false_entry_condition() {
        let rows = [row(0), row_with_price(1, 0)];
        assert_eq!(prepare_h1(&window(&rows, 2), 2), Err(StrategyError::Price));
    }
}
