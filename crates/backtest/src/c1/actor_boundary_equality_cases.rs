use super::*;

const POST_2025_START: i64 = 1_735_948_800_000_000;
const SPOT_MICROSECOND_ERA: i64 = 1_735_689_600_000_000;

fn equality_bar(source: PriceSource, key: i64) -> (PriceSource, Bar, SourceRef) {
    let micro_spot = source == PriceSource::Spot && key >= SPOT_MICROSECOND_ERA;
    let end = key + MINUTE;
    let available = if micro_spot { end + 999 } else { end };
    let close = if micro_spot { end - 1 } else { end - MS };
    let price = Price::from_ticks(10_000_000_000).unwrap();
    let mut provenance = rate_source(key, available);
    provenance.object = format!("synthetic/equality/{source:?}/{key}");
    provenance.raw_unit = if micro_spot {
        TimeUnit::Micros
    } else {
        TimeUnit::Millis
    };
    (
        source,
        Bar {
            open_time: UtcMicros::from_micros(key),
            close_time: UtcMicros::from_micros(close),
            available: UtcMicros::from_micros(available),
            open: price,
            high: price,
            low: price,
            close: price,
            volume: Qty::from_ticks(100_000_000).unwrap(),
            count: 1,
        },
        provenance,
    )
}

fn equality_source(start: i64) -> C1SourceView {
    let end = start + DAY;
    let terminal = end - LEGGING;
    let first_possible_spot_open = terminal + 3 * MINUTE;
    let final_pre_end_open = end - MINUTE;
    let mut bars = Vec::new();
    let mut key = start - MINUTE;
    while key < start + 9 * DAY {
        for kind in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
            let zero_count = kind == PriceSource::Spot
                && (first_possible_spot_open..final_pre_end_open).contains(&key);
            let mut row = equality_bar(kind, key);
            if zero_count {
                row.1.count = 0;
            }
            bars.push(row);
        }
        key += MINUTE;
    }
    let rates = (-8..27)
        .map(|index| {
            let key = start + index * SLOT;
            (
                key,
                if matches!(index, 0 | 3) {
                    -1_000
                } else {
                    50_000
                },
                key + MINUTE,
            )
        })
        .collect();
    source_with_rates(rates, bars)
}

fn equality_windows(start: i64) -> Vec<PolicyWindow> {
    vec![
        PolicyWindow {
            start,
            end: start + DAY,
            hold: C1MaxHold::Hours168,
            enabled: true,
        },
        PolicyWindow {
            start: start + DAY,
            end: start + 9 * DAY,
            hold: C1MaxHold::Hours168,
            enabled: false,
        },
    ]
}

fn equality_record_at(record: &ActorRecord) -> i64 {
    match record {
        ActorRecord::Control { at, .. }
        | ActorRecord::Evaluation { at, .. }
        | ActorRecord::Order { at, .. }
        | ActorRecord::Fill { at, .. }
        | ActorRecord::FundingBoundary { at, .. }
        | ActorRecord::FundingReceipt { at, .. }
        | ActorRecord::PairClosed { at, .. }
        | ActorRecord::RiskStop { at, .. }
        | ActorRecord::ZeroExposureBoundary { at } => *at,
    }
}

fn equality_funding_preserved(snapshot: &CarrySnapshot, start: i64) {
    let paid = snapshot
        .funding
        .iter()
        .find(|row| row.effective.micros() == start + 2 * SLOT)
        .unwrap();
    assert_eq!(paid.captured_short.ticks(), 9_990_000_000);
    assert_eq!(paid.cashflow, Some(Cash(499_500_000)));
    assert!(paid.receipt.is_some());
    assert_eq!(snapshot.account.funding_net.0, 499_500_000);
}

fn pre_era_equality_replay() -> ReplayResult {
    let source = equality_source(START);
    let end = START + DAY;
    let physical = UtcMicros::from_micros(end - MINUTE);
    let ExecutionState::Valid {
        timestamps, count, ..
    } = source
        .execution_at(C1Instrument::Spot, physical, decision(end))
        .unwrap()
    else {
        panic!("the declared pre-era final spot row must be admitted exactly at E")
    };
    assert_eq!(count, 1);
    assert_eq!(timestamps.event_ts.micros(), physical);
    assert_eq!(timestamps.available_ts.micros().micros(), end);
    assert_eq!(timestamps.received_ts.micros().micros(), end);
    C1Replay::new(&source, Scenario::Headline, equality_windows(START))
        .unwrap()
        .run()
        .unwrap()
}

fn assert_pre_era_equality(value: &ReplayResult) {
    let end = START + DAY;
    assert_eq!(value.pairs.len(), 1);
    let pair = &value.pairs[0];
    assert_eq!(pair.exit_reason, Some(ExitReason::Terminal));
    assert_eq!(pair.exit_decision, Some(end - LEGGING));
    assert_eq!(pair.closing_deadline, Some(end));
    assert_eq!(pair.closed_observed_at, Some(end));
    assert_dependent_decisions(value);
    let snapshot = value.snapshot.as_ref().unwrap();
    assert_eq!(snapshot.phase, CarryPhase::Closed);
    assert!(snapshot.orders.iter().all(|order| order.complete_cut));
    assert_eq!(snapshot.orders.len(), 4);
    assert_eq!(snapshot.fills.len(), 4);
    assert_eq!(snapshot.account.spot_btc.ticks(), 0);
    assert_eq!(snapshot.account.perp_short.ticks(), 0);
    assert_eq!(snapshot.account.spot_quote.0, 47_497_601_399_800);
    assert_eq!(snapshot.account.perp_wallet.0, 47_499_100_900_000);
    assert_eq!(snapshot.account.idle.0, 5_000_000_000_000);
    equality_funding_preserved(snapshot, START);
    assert_eq!(value.final_equity, 99_996_702_299_800);
    assert_eq!(value.daily.len(), 9);
    assert_eq!(value.daily[0].economic_end_exclusive, end);
    assert_eq!(value.daily[0].known_at, end + MS);
    assert!(value.records.iter().all(|record| match record {
        ActorRecord::Order { at, reference, .. } => *at < end && reference.original_event_us < end,
        ActorRecord::Fill {
            physical, source, ..
        } => *physical < end && source.original_event_us < end,
        ActorRecord::FundingReceipt {
            effective,
            rate_source,
            mark_source,
            ..
        } =>
            *effective < end
                && rate_source.original_event_us < end
                && mark_source.original_event_us < end,
        _ => true,
    }));
    let (observed, physical) = value
        .records
        .iter()
        .find_map(|record| match record {
            ActorRecord::Fill {
                at,
                physical,
                stage: "spot_exit",
                ..
            } => Some((*at, *physical)),
            _ => None,
        })
        .unwrap();
    assert_eq!((observed, physical), (end, end - MINUTE));
}

fn post_era_unpublished_replay() -> ReplayFailure {
    let source = equality_source(POST_2025_START);
    let end = POST_2025_START + DAY;
    let physical = UtcMicros::from_micros(end - MINUTE);
    assert!(
        matches!(source.execution_at(C1Instrument::Spot, physical, decision(end)).unwrap(),
        ExecutionState::Pending { until } if until.micros() == end + MS)
    );
    // This is a source-fixture check, not an actor action after its deadline.
    let ExecutionState::Valid {
        timestamps,
        count,
        source: provenance,
        ..
    } = source
        .execution_at(C1Instrument::Spot, physical, decision(end + MS))
        .unwrap()
    else {
        panic!("the canonical post-era row must first be admitted at E+1ms")
    };
    assert_eq!(count, 1);
    assert_eq!(timestamps.event_ts.micros(), physical);
    assert_eq!(timestamps.available_ts.micros().micros(), end + 999);
    assert_eq!(timestamps.received_ts.micros().micros(), end + MS);
    assert_eq!(provenance.raw_unit, TimeUnit::Micros);
    assert_eq!(
        provenance.original.available_ts.micros().micros(),
        end + 999
    );
    assert!(provenance.original.received_ts.micros().micros() > end + MS);
    C1Replay::new(
        &source,
        Scenario::Headline,
        equality_windows(POST_2025_START),
    )
    .unwrap()
    .run()
    .unwrap_err()
}

fn assert_post_era_unpublished(failure: &ReplayFailure) {
    let end = POST_2025_START + DAY;
    assert!(matches!(&failure.error, ReplayError::UnclosedBoundary { at } if *at == end));
    assert!(
        failure.stop_error.is_none(),
        "secondary Stop failure: {:?}",
        failure.stop_error
    );
    let value = &failure.prefix;
    assert!(
        matches!(value.control.values.get("mode"), Some(FiniteValue::String(mode)) if mode == "Stopped")
    );
    assert_eq!(value.pairs.len(), 1);
    let pair = &value.pairs[0];
    assert_eq!(pair.exit_reason, Some(ExitReason::Terminal));
    assert_eq!(pair.exit_decision, Some(end - LEGGING));
    assert_eq!(pair.closing_deadline, Some(end));
    assert_eq!(pair.closed_observed_at, None);
    let snapshot = value.snapshot.as_ref().unwrap();
    assert!(!matches!(
        snapshot.phase,
        CarryPhase::Flat | CarryPhase::Closed
    ));
    assert_eq!(snapshot.account.spot_btc.ticks(), 9_990_000_000);
    assert_eq!(snapshot.account.perp_short.ticks(), 0);
    assert_eq!(snapshot.account.spot_quote.0, 46_499_800_000_000);
    assert_eq!(snapshot.account.perp_wallet.0, 47_499_100_900_000);
    assert_eq!(snapshot.account.idle.0, 5_000_000_000_000);
    assert_eq!(snapshot.account.spot_quote_fees.0, 0);
    assert_eq!(snapshot.account.spot_btc_fees.ticks(), 10_000_000);
    assert_eq!(snapshot.account.perp_fees.0, 999_000_000);
    assert_eq!(snapshot.account.realized_perp.0, -399_600_000);
    assert_eq!(snapshot.orders.len(), 4);
    assert_eq!(snapshot.fills.len(), 3);
    equality_funding_preserved(snapshot, POST_2025_START);
    assert!(
        value.daily.is_empty(),
        "unpublished owned value must not become a zero-return day"
    );
    assert!(!value.records.iter().any(|record| matches!(
        record,
        ActorRecord::Fill {
            stage: "spot_exit",
            ..
        }
    )));
    assert!(value
        .records
        .iter()
        .all(|record| equality_record_at(record) <= end));
    assert!(value.records.iter().all(|record| match record {
        ActorRecord::Order { at, reference, .. } => *at < end && reference.original_event_us < end,
        ActorRecord::Fill {
            physical, source, ..
        } => *physical < end && source.original_event_us < end,
        ActorRecord::FundingReceipt {
            effective,
            rate_source,
            mark_source,
            ..
        } =>
            *effective < end
                && rate_source.original_event_us < end
                && mark_source.original_event_us < end,
        _ => true,
    }));
}

#[test]
fn an_already_admitted_final_spot_cut_at_e_finishes_the_fold() {
    assert_pre_era_equality(&pre_era_equality_replay());
}

#[test]
fn a_canonical_post_era_spot_receipt_after_e_cannot_rescue_the_fold() {
    assert_post_era_unpublished(&post_era_unpublished_replay());
}

#[given("the same predetermined closing spot liquidity at the last pre end minute")]
fn equality_clock_case(world: &mut C1World) {
    world.case = Some("boundary_equality");
}

#[when("the canonical pre 2025 spot clock is replayed")]
fn equality_clock_pre(world: &mut C1World) {
    world.cycles = Some(pre_era_equality_replay());
}

#[when("the canonical post 2025 spot clock is replayed")]
fn equality_clock_post(world: &mut C1World) {
    world.failure = Some(post_era_unpublished_replay());
}

#[then("the admitted cut at E completes the fold using only pre E physical facts")]
fn equality_clock_finished(world: &mut C1World) {
    assert_pre_era_equality(world.cycles.as_ref().unwrap());
    assert!(world.failure.is_none());
}

#[then("the unpublished E plus 1ms receipt leaves a technical failure at E")]
fn equality_clock_unpublished(world: &mut C1World) {
    assert_post_era_unpublished(world.failure.as_ref().unwrap());
    assert!(world.cycles.is_none());
}
