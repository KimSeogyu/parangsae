//! H4 policy uses runtime state; its opportunity stream assumes no executions.

mod support;

use parangsae_core::{BarInterval, Observation, ResampledBar, UtcMicros};
use parangsae_strategies::{
    replay, ActualHold, DecisionBook, ExecutionDirective, ExecutionPhase, FundingPrint,
    H4ObservationKind, PositionSide, Strategy, StrategyError,
};
use support::{flat, params, usdt, SCALE, T0};

const HOUR: i64 = 3_600_000_000;
const DAY: i64 = 24 * HOUR;

fn configured() -> Strategy {
    Strategy::from_params(
        "H4",
        &params(&[
            ("q", 10_000_000),
            ("sum_settlements", 3),
            ("confirm_h", 24),
            ("hold_h", 72),
        ]),
    )
    .expect("valid H4 fixture parameters")
}

fn book(execution: ExecutionPhase) -> DecisionBook {
    let side = match execution {
        ExecutionPhase::Open { side, .. }
        | ExecutionPhase::ExitPending { side, .. }
        | ExecutionPhase::Adjusting { side, .. } => side.target_ticks() / SCALE,
        ExecutionPhase::Flat | ExecutionPhase::EntryPending { .. } => 0,
    };
    DecisionBook {
        equity_ticks: usdt(1_000_000),
        position_lots: side,
        mark_ticks: usdt(100),
        execution,
    }
}

fn confirmed() -> (Strategy, Observation<ResampledBar>) {
    let mut strategy = configured();
    let mut rows = Vec::new();
    let mut time = T0 - 365 * DAY - 16 * HOUR;
    while time <= T0 {
        rows.push(FundingPrint {
            funding_time_us: time,
            available_us: time + 60_000_000,
            rate_ticks: if time == T0 { SCALE } else { 0 },
        });
        time += 8 * HOUR;
    }
    for hour in -1..23 {
        let row = flat(hour, BarInterval::H1, usdt(100));
        let decision = row.timestamps().available_ts.micros().micros() + 1_000;
        let visible: Vec<_> = rows
            .iter()
            .copied()
            .filter(|rate| rate.available_us <= decision)
            .collect();
        strategy
            .ingest_funding(&visible)
            .expect("causal funding fixture");
        strategy.observe(&row).expect("closed history fixture");
    }
    (strategy, flat(23, BarInterval::H1, usdt(90)))
}

#[test]
fn opportunity_is_the_same_causal_event_used_by_the_flat_policy() {
    let (mut pure, row) = confirmed();
    let event = pure.h4_opportunity(&row).unwrap().expect("confirmed event");
    assert_eq!(event.event.settlement.micros(), T0);
    assert_eq!(event.side(), PositionSide::Short);
    assert_eq!(event.decision_ts.micros().micros(), T0 + DAY + 1_000);

    let (mut policy, row) = confirmed();
    let output = policy
        .push(&row, Some(book(ExecutionPhase::Flat)))
        .unwrap()
        .expect("ready policy");
    assert_eq!(output.signal, event.signal().unwrap());
    assert_eq!(
        output.directive,
        ExecutionDirective::Enter {
            event: event.event,
            hold: ActualHold::from_hours(72).unwrap()
        }
    );
    assert_eq!(
        policy.dropped_entries(),
        0,
        "runtime owns actual-entry expiry drops"
    );
}

#[test]
fn missing_or_adjusting_execution_context_has_no_bar_only_fallback() {
    let (mut strategy, row) = confirmed();
    assert_eq!(
        strategy.push(&row, None),
        Err(StrategyError::MissingExecutionContext)
    );
    assert!(
        strategy.h4_opportunity(&row).unwrap().is_some(),
        "refusing a missing context consumed the bar"
    );
    assert_eq!(
        replay(
            "H4",
            &params(&[
                ("q", 10_000_000),
                ("sum_settlements", 3),
                ("confirm_h", 24),
                ("hold_h", 72)
            ]),
            &[]
        ),
        Err(StrategyError::MissingExecutionContext)
    );

    let (mut adjusting, row) = confirmed();
    let phase = ExecutionPhase::Adjusting {
        side: PositionSide::Long,
        entered_at: UtcMicros::from_micros(T0),
    };
    assert_eq!(
        adjusting.push(&row, Some(book(phase))),
        Err(StrategyError::AdjustingExecutionPhase)
    );
}

#[test]
fn pending_requests_keep_the_runtime_order_even_when_an_opposite_event_is_ready() {
    for (phase, target) in [
        (
            ExecutionPhase::EntryPending {
                side: PositionSide::Long,
            },
            SCALE,
        ),
        (
            ExecutionPhase::ExitPending {
                side: PositionSide::Long,
                entered_at: UtcMicros::from_micros(T0),
            },
            0,
        ),
    ] {
        let (mut strategy, row) = confirmed();
        let output = strategy.push(&row, Some(book(phase))).unwrap().unwrap();
        assert_eq!(output.directive, ExecutionDirective::Keep);
        assert_eq!(output.signal.decision.target_ticks(), target);
    }

    let mut waiting = configured();
    for hour in 0..100 {
        let row = flat(hour, BarInterval::H1, usdt(100));
        let output = waiting
            .push(
                &row,
                Some(book(ExecutionPhase::EntryPending {
                    side: PositionSide::Short,
                })),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            output.directive,
            ExecutionDirective::Keep,
            "a pending entry invented its own hold deadline"
        );
        assert_eq!(output.signal.decision.target_ticks(), -SCALE);
    }
}

#[test]
fn same_crowd_tail_events_keep_one_episode_and_record_skips_without_a_fill() {
    let (mut strategy, row) = confirmed();
    strategy
        .ingest_funding(&[
            FundingPrint {
                funding_time_us: T0 + 8 * HOUR,
                available_us: T0 + 8 * HOUR + 60_000_000,
                rate_ticks: SCALE,
            },
            FundingPrint {
                funding_time_us: T0 + 16 * HOUR,
                available_us: T0 + 16 * HOUR + 60_000_000,
                rate_ticks: SCALE,
            },
        ])
        .unwrap();
    let pending = ExecutionPhase::EntryPending {
        side: PositionSide::Long,
    };
    let output = strategy.push(&row, Some(book(pending))).unwrap().unwrap();
    assert_eq!(output.directive, ExecutionDirective::Keep);
    let observations = strategy.take_h4_observations();
    let tails: Vec<_> = observations
        .iter()
        .filter(|item| item.kind == H4ObservationKind::Tail)
        .collect();
    assert_eq!(tails.len(), 3);
    assert!(tails.iter().all(
        |item| item.event.episode.micros() == T0 && item.event.crowd_side == PositionSide::Long
    ));
    assert!(observations
        .iter()
        .any(|item| item.kind == H4ObservationKind::Confirmation { confirmed: true }));
    assert!(observations
        .iter()
        .any(|item| item.kind == H4ObservationKind::OccupiedSkip));
    assert!(observations
        .iter()
        .all(|item| item.observed_at.micros().micros() <= T0 + DAY + 1_000));
    assert!(strategy.take_h4_observations().is_empty());
}

#[test]
fn neutral_and_opposite_tail_observations_end_the_previous_crowd_episode() {
    let mut strategy = configured();
    let mut rates = Vec::new();
    let mut time = T0 - 365 * DAY - 16 * HOUR;
    while time < T0 {
        rates.push(FundingPrint {
            funding_time_us: time,
            available_us: time + 60_000_000,
            rate_ticks: 0,
        });
        time += 8 * HOUR;
    }
    for slot in 0..=12 {
        let ticks = match slot {
            0..=2 | 12 => SCALE,
            6..=8 => -SCALE,
            _ => 0,
        };
        let time = T0 + slot * 8 * HOUR;
        rates.push(FundingPrint {
            funding_time_us: time,
            available_us: time + 60_000_000,
            rate_ticks: ticks,
        });
    }
    let mut tails = Vec::new();
    for hour in -1..100 {
        let row = flat(hour, BarInterval::H1, usdt(100));
        let decision = row.timestamps().available_ts.micros().micros() + 1_000;
        let visible: Vec<_> = rates
            .iter()
            .copied()
            .filter(|rate| rate.available_us <= decision)
            .collect();
        strategy.ingest_funding(&visible).unwrap();
        assert!(
            strategy.h4_opportunity(&row).unwrap().is_none(),
            "flat confirmation prices invented an entry"
        );
        tails.extend(
            strategy
                .take_h4_observations()
                .into_iter()
                .filter(|item| item.kind == H4ObservationKind::Tail),
        );
    }
    for (settlement, episode, side) in [
        (0, 0, PositionSide::Long),
        (32, 0, PositionSide::Long),
        (48, 48, PositionSide::Short),
        (80, 48, PositionSide::Short),
        (96, 96, PositionSide::Long),
    ] {
        let event = tails
            .iter()
            .find(|item| item.event.settlement.micros() == T0 + settlement * HOUR)
            .expect("tail event")
            .event;
        assert_eq!(event.episode.micros(), T0 + episode * HOUR);
        assert_eq!(event.crowd_side, side);
    }
    assert!(!tails
        .iter()
        .any(|item| item.event.settlement.micros() == T0 + 40 * HOUR));
    assert!(!tails
        .iter()
        .any(|item| item.event.settlement.micros() == T0 + 88 * HOUR));
}

#[test]
fn an_undrained_tail_stream_fails_instead_of_silently_evicting_records() {
    let mut strategy = configured();
    let tail_count =
        strategy.h4_observation_capacity() / parangsae_strategies::H4_OBSERVATIONS_PER_EVENT + 1;
    let mut rows = Vec::new();
    let mut time = T0 - 365 * DAY - 16 * HOUR;
    while time < T0 {
        rows.push(FundingPrint {
            funding_time_us: time,
            available_us: time + 60_000_000,
            rate_ticks: 0,
        });
        time += 8 * HOUR;
    }
    for index in 0..tail_count {
        let index = i64::try_from(index).unwrap();
        let time = T0 + index * 8 * HOUR;
        rows.push(FundingPrint {
            funding_time_us: time,
            available_us: time + 60_000_000,
            rate_ticks: (index + 1) * SCALE,
        });
    }
    assert_eq!(
        strategy.ingest_funding(&rows),
        Err(StrategyError::H4ObservationCapacity)
    );
}

#[test]
fn a_tail_unavailable_at_confirmation_cannot_bridge_observed_episodes() {
    for late_by in [0, 1] {
        let mut strategy = configured();
        let mut rows = Vec::new();
        let mut time = T0 - 365 * DAY - 16 * HOUR;
        while time < T0 {
            rows.push(FundingPrint {
                funding_time_us: time,
                available_us: time + 60_000_000,
                rate_ticks: 0,
            });
            time += 8 * HOUR;
        }
        for slot in 0..3 {
            let time = T0 + slot * 8 * HOUR;
            let available = if slot == 1 {
                time + 24 * HOUR + 1_000 + late_by
            } else {
                time + 60_000_000
            };
            rows.push(FundingPrint {
                funding_time_us: time,
                available_us: available,
                rate_ticks: SCALE,
            });
        }
        // All history is admitted at this decision. Its original use-time remains
        // explicit; late records do not become earlier confirmation evidence.
        strategy.ingest_funding(&rows).unwrap();
        let row = flat(50, BarInterval::H1, usdt(100));
        assert!(strategy.h4_opportunity(&row).unwrap().is_none());
        let observations = strategy.take_h4_observations();
        let tail = |slot: i64| {
            observations
                .iter()
                .find(|item| {
                    item.kind == H4ObservationKind::Tail
                        && item.event.settlement.micros() == T0 + slot * 8 * HOUR
                })
                .expect("tail metadata")
        };
        assert_eq!(tail(0).event.episode.micros(), T0);
        if late_by == 0 {
            assert_eq!(tail(1).event.episode.micros(), T0);
            assert_eq!(tail(2).event.episode.micros(), T0);
        } else {
            assert_eq!(tail(1).event.episode.micros(), T0 + 8 * HOUR);
            assert_eq!(tail(2).event.episode.micros(), T0 + 16 * HOUR);
            assert!(tail(1).event.gap_boundary && tail(2).event.gap_boundary);
        }
        assert!(observations
            .iter()
            .all(|item| item.observed_at.micros().micros() == T0 + 51 * HOUR + 1_000));
    }
}

#[test]
fn actual_entry_feedback_changes_exit_without_changing_feature_history() {
    let mut early = configured();
    let mut late = configured();
    let early_entry = UtcMicros::from_micros(T0 + 6 * 60_000_000);
    let late_entry = UtcMicros::from_micros(T0 + HOUR + 6 * 60_000_000);
    for hour in 1..=72 {
        let row = flat(hour, BarInterval::H1, usdt(100));
        let a = early
            .push(
                &row,
                Some(book(ExecutionPhase::Open {
                    side: PositionSide::Short,
                    entered_at: early_entry,
                })),
            )
            .unwrap()
            .unwrap();
        let b = late
            .push(
                &row,
                Some(book(ExecutionPhase::Open {
                    side: PositionSide::Short,
                    entered_at: late_entry,
                })),
            )
            .unwrap()
            .unwrap();
        assert_eq!(b.directive, ExecutionDirective::Keep);
        if hour == 72 {
            assert_eq!(a.directive, ExecutionDirective::SetTarget);
            assert_eq!(a.signal.decision.target_ticks(), 0);
            assert_eq!(b.signal.decision.target_ticks(), -SCALE);
        } else {
            assert_eq!(a.directive, ExecutionDirective::Keep);
        }
    }
    let row = flat(73, BarInterval::H1, usdt(100));
    let pending = early
        .push(
            &row,
            Some(book(ExecutionPhase::ExitPending {
                side: PositionSide::Short,
                entered_at: early_entry,
            })),
        )
        .unwrap()
        .unwrap();
    assert_eq!(pending.directive, ExecutionDirective::Keep);
    let due = late
        .push(
            &row,
            Some(book(ExecutionPhase::Open {
                side: PositionSide::Short,
                entered_at: late_entry,
            })),
        )
        .unwrap()
        .unwrap();
    assert_eq!(due.directive, ExecutionDirective::SetTarget);
    assert_eq!(due.signal.decision.target_ticks(), 0);
}
