use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Ratio, ReceivedTs, Timestamps, UtcMicros,
};
use parangsae_signals::FundingRow;
use parangsae_strategies::c1::{
    classify_c1_key, evaluate_c1_funding, C1FeatureError, C1FundingEvaluation, C1FundingFeature,
    C1History, C1KeyClass, C1MaxHold, C1SettledRate,
};
use proptest::prelude::*;

const HOUR: i64 = 3_600_000_000;
const SLOT: i64 = 8 * HOUR;
const LATEST: i64 = 8 * SLOT;
const LAG: i64 = 60_000_000;
const DECISION: i64 = LATEST + HOUR + 1_000;

fn at(us: i64) -> UtcMicros {
    UtcMicros::from_micros(us)
}

fn row(key: i64, rate: i64, interval_hours: u8, available: i64) -> Observation<FundingRow> {
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(at(key)),
            available_ts: AvailableTs::from_micros(at(available)),
            received_ts: ReceivedTs::from_micros(at(0)),
        },
        FundingRow::new(at(key), Ratio::from_ticks(rate), interval_hours),
    )
}

fn complete() -> Vec<Observation<FundingRow>> {
    (0..9)
        .map(|i| row(i * SLOT, 10_000, 8, i * SLOT + LAG))
        .collect()
}

fn evaluate(
    rows: &[Observation<FundingRow>],
    decision: i64,
    expected: i64,
) -> Result<C1FundingEvaluation, C1FeatureError> {
    let decision = DecisionTime::from_micros(at(decision));
    let views: Vec<_> = rows.iter().filter_map(|r| r.causal_at(decision)).collect();
    evaluate_c1_funding(decision, at(expected), &views)
}

fn feature(value: C1FundingEvaluation) -> C1FundingFeature {
    match value {
        C1FundingEvaluation::Latest {
            history: C1History::Complete(feature),
            ..
        } => feature,
        other => panic!("expected complete synthetic history, got {other:?}"),
    }
}

fn latest(value: C1FundingEvaluation) -> C1SettledRate {
    match value {
        C1FundingEvaluation::Latest { rate, .. } => rate,
        other => panic!("expected visible synthetic latest rate, got {other:?}"),
    }
}

#[test]
fn nine_payments_use_seventy_two_hours_while_the_keys_span_sixty_four() {
    let f = feature(evaluate(&complete(), DECISION, LATEST).expect("valid synthetic window"));
    assert_eq!(f.oldest_key(), at(0));
    assert_eq!(f.latest().key(), at(64 * HOUR));
    assert_eq!(f.sum9_ticks(), 90_000);
    assert_eq!(f.latest().interval_hours(), 8);
    assert_eq!(f.available_through().micros(), at(LATEST + LAG));
    for (horizon, bps) in [(C1MaxHold::Hours168, 21), (C1MaxHold::Hours336, 42)] {
        let projection = f.project(horizon).expect("bounded rational projection");
        assert_eq!(
            projection.numerator() * 10_000,
            i128::from(bps) * projection.denominator()
        );
        assert!(projection.denominator() > 0);
    }
}

#[test]
fn a_missing_expected_latest_does_not_fall_back_to_the_previous_rate() {
    let mut rows = complete();
    rows.pop();
    assert_eq!(
        evaluate(&rows, DECISION, LATEST),
        Ok(C1FundingEvaluation::ExpectedUnavailable {
            expected_key: at(LATEST)
        })
    );
}

#[test]
fn a_valid_nonpositive_latest_survives_incomplete_entry_history() {
    let mut rows = complete();
    rows.remove(4);
    rows[7] = row(LATEST, -10_000, 8, LATEST + LAG);
    match evaluate(&rows, DECISION, LATEST).expect("latest is valid") {
        C1FundingEvaluation::Latest {
            rate,
            history: C1History::Incomplete { missing_mask },
        } => {
            assert_eq!(rate.key(), at(LATEST));
            assert_eq!(rate.rate().ticks(), -10_000);
            assert_eq!(missing_mask, 1 << 4);
        }
        other => panic!("missing history was hidden: {other:?}"),
    }
}

#[test]
fn availability_equality_is_admitted_without_an_additional_delay() {
    let rows = complete();
    assert_eq!(
        evaluate(&rows, LATEST + LAG - 1, LATEST),
        Ok(C1FundingEvaluation::ExpectedUnavailable {
            expected_key: at(LATEST)
        })
    );
    let equal = feature(evaluate(&rows, LATEST + LAG, LATEST).expect("equality is available"));
    let after = feature(evaluate(&rows, LATEST + LAG + 1, LATEST).expect("still available"));
    assert_eq!(equal, after);
    assert_eq!(equal.latest().available_ts().micros(), at(LATEST + LAG));
}

#[test]
fn a_view_made_for_a_later_decision_is_refused_before_its_payload_is_read() {
    let future = row(LATEST + SLOT + 1, i64::MAX, 4, DECISION + 1);
    let view = future
        .causal_at(DecisionTime::from_micros(at(DECISION + 1)))
        .expect("visible only at the later decision");
    assert_eq!(
        evaluate_c1_funding(DecisionTime::from_micros(at(DECISION)), at(LATEST), &[view]),
        Err(C1FeatureError::AfterDecision {
            available: AvailableTs::from_micros(at(DECISION + 1))
        })
    );
}

#[test]
fn wrong_cadence_and_off_grid_facts_are_not_zero_filled_or_ignored() {
    let key = 4 * SLOT;
    let mut rows = complete();
    rows[4] = row(key, 10_000, 4, key + LAG);
    assert_eq!(
        evaluate(&rows, DECISION, LATEST),
        Err(C1FeatureError::WrongCadence {
            key: at(key),
            interval_hours: 4
        })
    );
    rows[4] = row(key + 1, 10_000, 8, key + LAG);
    assert_eq!(
        evaluate(&rows, DECISION, LATEST),
        Err(C1FeatureError::OffGrid { key: at(key + 1) })
    );
}

#[test]
fn expected_grid_and_required_window_are_checked() {
    assert_eq!(
        evaluate(&[], DECISION, LATEST + 1),
        Err(C1FeatureError::BadExpectedKey {
            key: at(LATEST + 1)
        })
    );
    let mut rows = complete();
    rows.push(row(-SLOT, 10_000, 8, -SLOT + LAG));
    assert_eq!(
        evaluate(&rows, DECISION, LATEST),
        Err(C1FeatureError::UnexpectedKey { key: at(-SLOT) })
    );
}

#[test]
fn an_available_but_future_settlement_is_still_invalid() {
    let future = row(LATEST + SLOT, 10_000, 8, DECISION);
    assert_eq!(
        evaluate(&[future], DECISION, LATEST),
        Err(C1FeatureError::FutureSettlement {
            key: at(LATEST + SLOT)
        })
    );
}

#[test]
fn identical_duplicates_are_idempotent_but_conflicting_rates_are_refused() {
    let mut rows = complete();
    let expected = evaluate(&rows, DECISION, LATEST).expect("valid synthetic window");
    rows.push(rows[4].clone());
    assert_eq!(evaluate(&rows, DECISION, LATEST), Ok(expected));
    rows.push(row(4 * SLOT, 10_001, 8, 4 * SLOT + LAG));
    assert_eq!(
        evaluate(&rows, DECISION, LATEST),
        Err(C1FeatureError::ConflictingKey { key: at(4 * SLOT) })
    );
}

#[test]
fn a_missing_latest_does_not_hide_conflicting_older_input() {
    let mut rows = complete();
    rows.pop();
    rows.push(row(4 * SLOT, 99, 8, 4 * SLOT + LAG));
    assert_eq!(
        evaluate(&rows, DECISION, LATEST),
        Err(C1FeatureError::ConflictingKey { key: at(4 * SLOT) })
    );
}

#[test]
fn key_classification_is_stateless_and_detects_changed_latest_facts() {
    let rows = complete();
    let current = latest(evaluate(&rows, DECISION, LATEST).expect("valid latest"));
    let mut older_rows = rows.clone();
    older_rows.pop();
    let older = latest(
        evaluate(&older_rows, DECISION, LATEST - SLOT).expect("valid preceding synthetic window"),
    );
    assert_eq!(classify_c1_key(&current, None), Ok(C1KeyClass::Fresh));
    assert_eq!(
        classify_c1_key(&current, Some(&older)),
        Ok(C1KeyClass::Fresh)
    );
    assert_eq!(
        classify_c1_key(&current, Some(&current)),
        Ok(C1KeyClass::Duplicate)
    );
    assert_eq!(
        classify_c1_key(&older, Some(&current)),
        Ok(C1KeyClass::Superseded)
    );
    let changed = latest(
        evaluate(&[row(LATEST, 10_001, 8, LATEST + LAG)], DECISION, LATEST)
            .expect("valid changed latest fact"),
    );
    assert_eq!(
        classify_c1_key(&changed, Some(&current)),
        Err(C1FeatureError::ConflictingKey { key: at(LATEST) })
    );
}

#[test]
fn later_history_completion_cannot_reopen_an_actor_consumed_latest_key() {
    let rows = complete();
    let partial = latest(evaluate(&rows[8..], DECISION, LATEST).expect("valid latest only"));
    let full = feature(evaluate(&rows, DECISION, LATEST).expect("complete window"));
    assert_eq!(
        classify_c1_key(&full.latest(), Some(&partial)),
        Ok(C1KeyClass::Duplicate)
    );
    assert_eq!(
        classify_c1_key(&full.latest(), None),
        Ok(C1KeyClass::Fresh),
        "an actor that kept the incomplete event pending can evaluate it later"
    );
}

#[test]
fn signed_extreme_rates_stay_exact_without_truncating_a_fractional_tick() {
    for rate in [i64::MIN, i64::MAX, -1, 1] {
        let rows: Vec<_> = (0..9)
            .map(|i| row(i * SLOT, rate, 8, i * SLOT + LAG))
            .collect();
        let f = feature(evaluate(&rows, DECISION, LATEST).expect("valid signed rates"));
        assert_eq!(f.sum9_ticks(), 9 * i128::from(rate));
        let p = f
            .project(C1MaxHold::Hours168)
            .expect("wide signed rational");
        assert_eq!(p.numerator(), 168 * 9 * i128::from(rate));
        assert_eq!(p.denominator(), 72 * 100_000_000);
    }
    let mut rows = complete();
    for r in &mut rows {
        *r = r.with_value(r.value().with_rate(Ratio::from_ticks(0)));
    }
    rows[0] = row(0, 1, 8, LAG);
    let p = feature(evaluate(&rows, DECISION, LATEST).expect("one rate tick"))
        .project(C1MaxHold::Hours168)
        .expect("exact fractional rate");
    assert_eq!(p.numerator(), 168);
    assert_eq!(p.denominator(), 72 * 100_000_000);
}

#[test]
fn a_history_range_that_cannot_fit_the_timestamp_type_is_refused() {
    let lowest_grid = i64::MIN + (SLOT - i64::MIN.rem_euclid(SLOT));
    assert_eq!(
        evaluate(&[], DECISION, lowest_grid),
        Err(C1FeatureError::Overflow)
    );
}

proptest! {
    #[test]
    fn future_rate_mutation_is_inert_but_the_legal_past_control_changes_the_feature(
        future_a in any::<i64>(), future_b in any::<i64>(), past in -100_000_000_i64..100_000_000
    ) {
        prop_assume!(future_a != future_b);
        let mut a = complete();
        a.push(row(LATEST + SLOT, future_a, 4, DECISION + 1));
        let mut b = a.clone();
        b[9] = row(LATEST + SLOT, future_b, 4, DECISION + 1);
        let before = feature(evaluate(&a, DECISION, LATEST).expect("future row is gated"));
        let after = feature(evaluate(&b, DECISION, LATEST).expect("mutated future is still gated"));
        prop_assert_eq!(before, after);
        let ungated_a: i128 = a.iter().map(|r| i128::from(r.value().rate().ticks())).sum();
        let ungated_b: i128 = b.iter().map(|r| i128::from(r.value().rate().ticks())).sum();
        prop_assert_ne!(ungated_a, ungated_b, "negative control must detect an ungated future read");
        a[0] = row(0, past, 8, LAG);
        b[0] = row(0, past + 1, 8, LAG);
        let legal_a = feature(evaluate(&a, DECISION, LATEST).expect("legal past control"));
        let legal_b = feature(evaluate(&b, DECISION, LATEST).expect("changed legal past control"));
        prop_assert_ne!(legal_a, legal_b);
        prop_assert_eq!(legal_b.sum9_ticks() - legal_a.sum9_ticks(), 1);
    }

    #[test]
    fn arrival_order_does_not_replace_the_latest_semantic_key(
        order in prop::collection::vec(any::<u16>(), 9)
    ) {
        let rows = complete();
        let expected = evaluate(&rows, DECISION, LATEST).expect("valid window");
        let mut indexed: Vec<_> = rows.into_iter().enumerate().collect();
        indexed.sort_by_key(|(i, _)| order[*i]);
        let shuffled: Vec<_> = indexed.into_iter().map(|(_, r)| r).collect();
        prop_assert_eq!(evaluate(&shuffled, DECISION, LATEST), Ok(expected));
    }
}
