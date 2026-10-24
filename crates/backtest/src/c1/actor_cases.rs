#[path = "actor_risk_cases.rs"]
mod risk_cases;

use super::*;
use crate::c1_source::{PriceSource, SourceRef};
use cucumber::{given, then, when, World as _};
use parangsae_core::{Bar, Price, Ratio, TimeUnit};

const START: i64 = 1_704_326_400_000_000; // 2024-01-04, well inside the sample.

fn rate_source(key: i64, available: i64) -> SourceRef {
    SourceRef {
        object: "synthetic/funding".into(),
        sha256: "0".repeat(64),
        row: 0,
        original: Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(key)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(1_792_800_000_000_000)),
        },
        raw_unit: TimeUnit::Millis,
    }
}

fn bar(source: PriceSource, key: i64, price_ticks: i64) -> (PriceSource, Bar, SourceRef) {
    let price = Price::from_ticks(price_ticks).unwrap();
    let mut provenance = rate_source(key, key + MINUTE);
    provenance.object = format!("synthetic/{source:?}/{key}");
    (
        source,
        Bar {
            open_time: UtcMicros::from_micros(key),
            close_time: UtcMicros::from_micros(key + MINUTE - MS),
            available: UtcMicros::from_micros(key + MINUTE),
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

fn source_with_rates(
    rates: Vec<(i64, i64, i64)>,
    bars: Vec<(PriceSource, Bar, SourceRef)>,
) -> C1SourceView {
    C1SourceView::synthetic(
        bars,
        vec![],
        rates
            .into_iter()
            .map(|(key, rate, available)| {
                (
                    UtcMicros::from_micros(key),
                    Ratio::from_ticks(rate),
                    8,
                    rate_source(key, available),
                )
            })
            .collect(),
    )
    .unwrap()
}

fn window() -> Vec<PolicyWindow> {
    vec![PolicyWindow {
        start: START,
        end: START + 9 * DAY,
        hold: C1MaxHold::Hours168,
        enabled: true,
    }]
}

fn two_cycles_source(missing_mark: bool) -> C1SourceView {
    let mut bars = Vec::new();
    let mut key = START - MINUTE;
    while key <= START + 44 * HOUR {
        for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
            if !(missing_mark
                && source == PriceSource::UmMark
                && key == START + 9 * HOUR + 5 * MINUTE)
            {
                bars.push(bar(source, key, 10_000_000_000));
            }
        }
        key += MINUTE;
    }
    let mut rates = Vec::new();
    for index in -8..=5 {
        let key = START + index * SLOT;
        let value = if matches!(index, 0 | 2 | 3 | 5) {
            -1_000
        } else {
            50_000
        };
        rates.push((key, value, key + MINUTE));
    }
    source_with_rates(rates, bars)
}

fn max_source() -> C1SourceView {
    let mut bars = Vec::new();
    let mut key = START - MINUTE;
    while key <= START + 15 * DAY {
        for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
            bars.push(bar(source, key, 10_000_000_000));
        }
        key += MINUTE;
    }
    let rates = (-8..=45)
        .map(|index| {
            let key = START + index * SLOT;
            (key, if index == 0 { -1_000 } else { 50_000 }, key + MINUTE)
        })
        .collect();
    source_with_rates(rates, bars)
}

fn run_max(source: &C1SourceView, hold: C1MaxHold) -> ReplayResult {
    C1Replay::new(
        source,
        Scenario::Headline,
        vec![PolicyWindow {
            start: START,
            end: START + 18 * DAY,
            hold,
            enabled: true,
        }],
    )
    .unwrap()
    .run()
    .unwrap()
}

fn assert_actual_max(value: &ReplayResult, hold: C1MaxHold) {
    assert_eq!(value.pairs.len(), 1);
    let pair = &value.pairs[0];
    let observed = START + 9 * HOUR + 4 * MINUTE;
    assert_eq!(pair.observed_paired_at, Some(observed));
    assert_eq!(pair.physical_paired_at, Some(observed - MINUTE));
    let expected_exit = START + (hold.hours() + 10) * HOUR + MS;
    assert_eq!(pair.exit_reason, Some(ExitReason::MaxHold));
    assert_eq!(pair.exit_decision, Some(expected_exit));
    assert_eq!(pair.max_exit_decision, Some(expected_exit));
    assert!(expected_exit >= observed + hold.hours() * HOUR);
    assert!(expected_exit - HOUR < observed + hold.hours() * HOUR);
    let captures = match hold {
        C1MaxHold::Hours168 => 21,
        C1MaxHold::Hours336 => 42,
    };
    assert_eq!(value.snapshot.as_ref().unwrap().funding.len(), captures);
    assert_eq!(
        value.final_equity,
        match hold {
            C1MaxHold::Hours168 => 100_006_692_299_800,
            C1MaxHold::Hours336 => 100_017_181_799_800,
        }
    );
    assert_dependent_decisions(value);
}

#[test]
fn both_real_max_horizons_trigger_the_first_scheduled_exit_after_observed_completion() {
    let source = max_source();
    for hold in [C1MaxHold::Hours168, C1MaxHold::Hours336] {
        assert_actual_max(&run_max(&source, hold), hold);
    }
}

fn assert_dependent_decisions(value: &ReplayResult) {
    for pair in &value.pairs {
        for (fill_stage, order_stage) in [("spot_entry", "perp_entry"), ("perp_exit", "spot_exit")]
        {
            let receipt_at = value
                .records
                .iter()
                .find_map(|record| match record {
                    ActorRecord::Fill {
                        at,
                        pair: id,
                        stage,
                        ..
                    } if id == &pair.pair && *stage == fill_stage => Some(*at),
                    _ => None,
                })
                .unwrap();
            let decision_at = value
                .records
                .iter()
                .find_map(|record| match record {
                    ActorRecord::Order {
                        at,
                        pair: id,
                        stage,
                        ..
                    } if id == &pair.pair && *stage == order_stage => Some(*at),
                    _ => None,
                })
                .unwrap();
            assert_eq!(decision_at, receipt_at + MS);
        }
    }
}

fn assert_entry_prefix(snapshot: &CarrySnapshot) {
    assert_eq!(snapshot.account.spot_quote.0, 46_499_800_000_000);
    assert_eq!(snapshot.account.perp_wallet.0, 47_499_500_599_900);
    assert_eq!(snapshot.account.spot_btc.ticks(), 9_990_000_000);
    assert_eq!(snapshot.account.perp_short.ticks(), 9_990_000_000);
    assert_eq!(snapshot.account.spot_btc_fees.ticks(), 10_000_000);
    assert_eq!(snapshot.account.perp_fees.0, 499_400_100);
    assert_eq!(snapshot.account.spot_quote_fees.0, 0);
    assert_eq!(snapshot.account.funding_net.0, 0);
}

fn refusal_preserves_arm(prior_false: bool) -> bool {
    let mut bars = Vec::new();
    for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
        bars.push(bar(source, START + HOUR - MINUTE, 10_000_000_000));
        let value = match source {
            PriceSource::Spot => 10_000_000_000,
            PriceSource::UmTrade => 9_999_000_000,
            PriceSource::UmMark => 10_100_000_000,
        };
        bars.push(bar(source, START + 9 * HOUR - MINUTE, value));
    }
    let rates = (-8..=1)
        .map(|index| {
            let key = START + index * SLOT;
            (key, if index == 0 { -1_000 } else { 50_000 }, key + MINUTE)
        })
        .collect();
    let source = source_with_rates(rates, bars);
    let mut replay = C1Replay::new(&source, Scenario::Headline, window()).unwrap();
    if prior_false {
        replay.hour(START + HOUR + MS).unwrap();
        assert!(replay.flag("armed"));
    }
    replay.hour(START + 9 * HOUR + MS).unwrap();
    assert_eq!(replay.committed.unwrap().key().micros(), START + SLOT);
    assert!(replay.pairs.is_empty());
    assert_eq!(replay.snapshot().unwrap().account.spot_btc.ticks(), 0);
    replay.flag("armed")
}

#[test]
fn a_known_money_refusal_does_not_become_an_economic_false() {
    assert!(!refusal_preserves_arm(false));
    assert!(refusal_preserves_arm(true));
}

fn zero_count_then_valid() -> (bool, Option<i64>, usize, i64) {
    let mut bars = Vec::new();
    for hour in [1, 9, 10] {
        for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
            let mut row = bar(source, START + hour * HOUR - MINUTE, 10_000_000_000);
            if hour == 9 && source == PriceSource::Spot {
                row.1.count = 0;
            }
            bars.push(row);
        }
    }
    let rates = (-8..=1)
        .map(|index| {
            let key = START + index * SLOT;
            (key, if index == 0 { -1_000 } else { 50_000 }, key + MINUTE)
        })
        .collect();
    let source = source_with_rates(rates, bars);
    let mut replay = C1Replay::new(&source, Scenario::Headline, window()).unwrap();
    replay.hour(START + HOUR + MS).unwrap();
    assert!(replay.flag("armed"));
    replay.hour(START + 9 * HOUR + MS).unwrap();
    let armed = replay.flag("armed");
    let key = replay.committed.map(|rate| rate.key().micros());
    assert!(replay.pairs.is_empty());
    replay.hour(START + 10 * HOUR + MS).unwrap();
    assert_eq!(replay.committed.unwrap().key().micros(), START + SLOT);
    assert_eq!(replay.snapshot().unwrap().orders.len(), 1);
    (
        armed,
        key,
        replay.pairs.len(),
        replay.pairs[0].opened_decision,
    )
}

#[test]
fn zero_count_preserves_arm_and_allows_the_first_valid_same_key_entry() {
    assert_eq!(
        zero_count_then_valid(),
        (true, Some(START), 1, START + 10 * HOUR + MS)
    );
}

fn epoch_does_not_rearm_from_a_pre_epoch_seen_key() -> (bool, bool) {
    let mut bars = Vec::new();
    for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
        bars.push(bar(source, START + DAY + HOUR - MINUTE, 10_000_000_000));
    }
    let rates = (-6..=3)
        .map(|index| {
            let key = START + index * SLOT;
            (key, -1_000, key + MINUTE)
        })
        .collect();
    let source = source_with_rates(rates, bars);
    let mut replay = C1Replay::new(
        &source,
        Scenario::Headline,
        vec![
            PolicyWindow {
                start: START,
                end: START + DAY,
                hold: C1MaxHold::Hours168,
                enabled: false,
            },
            PolicyWindow {
                start: START + DAY,
                end: START + 9 * DAY,
                hold: C1MaxHold::Hours168,
                enabled: true,
            },
        ],
    )
    .unwrap();
    replay.hour(START + 23 * HOUR + MS).unwrap();
    assert_eq!(replay.committed, None);
    replay.activate_policy(START + DAY).unwrap();
    assert_eq!(replay.entry_epoch_start, START + DAY);
    let revision = replay.control.read().revision;
    replay.hour(START + DAY + MS).unwrap();
    assert_eq!(replay.control.read().revision, revision);
    assert_eq!(replay.committed, None);
    let old_armed = replay.flag("armed");
    replay.hour(START + DAY + HOUR + MS).unwrap();
    assert_eq!(replay.committed.unwrap().key().micros(), START + DAY);
    assert!(replay.pairs.is_empty());
    (old_armed, replay.flag("armed"))
}

#[test]
fn a_new_epoch_ignores_old_seen_keys_without_fake_consumption() {
    assert_eq!(
        epoch_does_not_rearm_from_a_pre_epoch_seen_key(),
        (false, true)
    );
}

#[test]
fn two_cycles_keep_the_same_capital_history_and_actual_completion_anchor() {
    let source = two_cycles_source(false);
    let result = C1Replay::new(&source, Scenario::Headline, window())
        .unwrap()
        .run()
        .unwrap();
    assert_eq!(result.pairs.len(), 2);
    assert_eq!(result.daily.len(), 9);
    assert_eq!(result.final_equity, 99_992_385_619_600);
    let snap = result.snapshot.as_ref().unwrap();
    assert_eq!(snap.orders.len(), 8);
    assert_eq!(snap.fills.len(), 8);
    assert_eq!(snap.funding.len(), 2);
    assert_eq!(snap.account.funding_net.0, -19_980_000);
    assert_eq!(snap.account.spot_btc.ticks(), 0);
    assert_eq!(snap.account.perp_short.ticks(), 0);
    assert!(!snap.loss_latched);
    assert!(result.risk_ticks > 0);
    assert_dependent_decisions(&result);
    for pair in &result.pairs {
        let physical = pair.physical_paired_at.unwrap();
        let observed = pair.observed_paired_at.unwrap();
        assert_eq!(observed - physical, MINUTE);
        assert!(pair.max_exit_decision.unwrap() >= observed + 168 * HOUR);
        assert_eq!(pair.exit_reason, Some(ExitReason::NonpositiveFunding));
        assert!(pair.closed_observed_at.unwrap() < pair.closing_deadline.unwrap());
    }
}

fn split_windows() -> Vec<PolicyWindow> {
    vec![
        PolicyWindow {
            start: START,
            end: START + 2 * DAY,
            hold: C1MaxHold::Hours168,
            enabled: true,
        },
        PolicyWindow {
            start: START + 2 * DAY,
            end: START + 9 * DAY,
            hold: C1MaxHold::Hours336,
            enabled: false,
        },
    ]
}

#[test]
fn a_later_flat_policy_fold_preserves_the_same_account_and_old_history() {
    let source = two_cycles_source(false);
    let whole = C1Replay::new(&source, Scenario::Headline, window())
        .unwrap()
        .run()
        .unwrap();
    let split = C1Replay::new(&source, Scenario::Headline, split_windows())
        .unwrap()
        .run()
        .unwrap();
    assert_eq!(split.final_equity, whole.final_equity);
    assert_eq!(split.snapshot, whole.snapshot);
    assert_eq!(split.daily, whole.daily);
    assert_eq!(split.daily.len(), 9);
    assert!(split.records.iter().any(|record| matches!(record,
        ActorRecord::Control { at, event: "NewEpoch", .. } if *at == START + 2 * DAY)));
}

#[test]
fn an_owned_required_mark_gap_refuses_and_retains_actual_money_prefix() {
    let source = two_cycles_source(true);
    let failure = C1Replay::new(&source, Scenario::Headline, window())
        .unwrap()
        .run()
        .unwrap_err();
    assert!(
        failure.stop_error.is_none(),
        "secondary Stop failure: {:?}",
        failure.stop_error
    );
    assert_stopped_prefix(&failure.prefix);
    assert!(matches!(failure.error, ReplayError::Source {
        at,
        error: SourceError::Unavailable { source: PriceSource::UmMark, key, health: SourceHealth::Gap }
    } if at == START + 9 * HOUR + 6 * MINUTE + MS && key.micros() == START + 9 * HOUR + 5 * MINUTE));
    assert_eq!(failure.prefix.pairs.len(), 1);
    assert_eq!(failure.prefix.snapshot.as_ref().unwrap().fills.len(), 2);
    assert_entry_prefix(failure.prefix.snapshot.as_ref().unwrap());
    assert!(failure.prefix.records.iter().any(|record| matches!(
        record,
        ActorRecord::Fill {
            stage: "perp_entry",
            ..
        }
    )));
    assert_eq!(failure.prefix.daily.len(), 0);
}

const TERMINAL_FOLD_END: i64 = START + DAY;
const TERMINAL_AT: i64 = TERMINAL_FOLD_END - LEGGING;
const TERMINAL_REPLAY_END: i64 = START + 9 * DAY;

fn terminal_fixture_source(zero_closing_window: bool) -> C1SourceView {
    let mut bars = Vec::new();
    let mut key = START - MINUTE;
    while key < TERMINAL_REPLAY_END {
        for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
            let zero_count = zero_closing_window
                && source == PriceSource::UmTrade
                && (TERMINAL_AT..TERMINAL_FOLD_END).contains(&key);
            let mut row = bar(source, key, 10_000_000_000);
            if zero_count {
                row.1.count = 0;
            }
            bars.push(row);
        }
        key += MINUTE;
    }
    let rates = (-8..27)
        .map(|index| {
            let key = START + index * SLOT;
            let value = if matches!(index, 0 | 3) {
                -1_000
            } else {
                50_000
            };
            (key, value, key + MINUTE)
        })
        .collect();
    source_with_rates(rates, bars)
}

fn terminal_fixture_windows() -> Vec<PolicyWindow> {
    vec![
        PolicyWindow {
            start: START,
            end: TERMINAL_FOLD_END,
            hold: C1MaxHold::Hours168,
            enabled: true,
        },
        PolicyWindow {
            start: TERMINAL_FOLD_END,
            end: TERMINAL_REPLAY_END,
            hold: C1MaxHold::Hours168,
            enabled: false,
        },
    ]
}

fn run_terminal_fixture(zero_closing_window: bool) -> Result<ReplayResult, ReplayFailure> {
    let source = terminal_fixture_source(zero_closing_window);
    C1Replay::new(&source, Scenario::Headline, terminal_fixture_windows())
        .unwrap()
        .run()
}

fn record_clock(record: &ActorRecord) -> i64 {
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

fn assert_stopped_prefix(value: &ReplayResult) {
    let values = &value.control.values;
    assert!(matches!(values.get("mode"), Some(FiniteValue::String(mode)) if mode == "Stopped"));
    assert!(matches!(
        values.get("stoppedEver"),
        Some(FiniteValue::Boolean(true))
    ));
    assert!(matches!(
        values.get("healthy"),
        Some(FiniteValue::Boolean(false))
    ));
    assert!(matches!(
        values.get("armed"),
        Some(FiniteValue::Boolean(false))
    ));
}

fn assert_terminal_closed(value: &ReplayResult) {
    assert_eq!(value.pairs.len(), 1);
    let pair = &value.pairs[0];
    assert_eq!(pair.observed_paired_at, Some(START + 9 * HOUR + 4 * MINUTE));
    assert_eq!(pair.exit_reason, Some(ExitReason::Terminal));
    assert_eq!(pair.exit_decision, Some(TERMINAL_AT));
    assert_eq!(pair.closing_deadline, Some(TERMINAL_FOLD_END));
    assert_eq!(pair.closed_observed_at, Some(TERMINAL_AT + 4 * MINUTE));
    assert!(pair.closed_observed_at.unwrap() < TERMINAL_FOLD_END);
    assert!(pair.max_exit_decision.unwrap() > TERMINAL_FOLD_END);
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
    assert_eq!(snapshot.account.spot_btc_fees.ticks(), 10_000_000);
    assert_eq!(snapshot.account.spot_quote_fees.0, 998_800_200);
    assert_eq!(snapshot.account.perp_fees.0, 999_000_000);
    assert_eq!(snapshot.account.realized_perp.0, -399_600_000);
    assert_eq!(snapshot.account.funding_net.0, 499_500_000);
    assert_eq!(snapshot.funding.len(), 1);
    assert_eq!(snapshot.funding[0].effective.micros(), START + 2 * SLOT);
    assert_eq!(snapshot.funding[0].captured_short.ticks(), 9_990_000_000);
    assert_eq!(snapshot.funding[0].cashflow, Some(Cash(499_500_000)));
    assert!(snapshot.funding[0].receipt.is_some());
    assert_eq!(value.final_equity, 99_996_702_299_800);
    assert_eq!(value.daily.len(), 9);
    assert_eq!(value.daily[0].day_start, START);
    assert_eq!(value.daily[0].economic_end_exclusive, TERMINAL_FOLD_END);
    assert_eq!(value.daily[0].known_at, TERMINAL_FOLD_END + MS);
    assert_eq!(value.daily[0].equity, 99_996_702_299_800);
    assert_eq!(value.daily[0].return_ticks, -3_297);
    assert!(value.daily.iter().skip(1).all(|day| day.return_ticks == 0));
    assert!(value.records.iter().any(|record| matches!(record,
        ActorRecord::Order { at, stage: "perp_exit", deadline, .. }
            if *at == TERMINAL_AT && *deadline == TERMINAL_FOLD_END)));
}

fn assert_terminal_blocked(failure: &ReplayFailure) {
    assert!(matches!(&failure.error,
        ReplayError::UnclosedBoundary { at } if *at == TERMINAL_FOLD_END));
    assert!(
        failure.stop_error.is_none(),
        "secondary Stop failure: {:?}",
        failure.stop_error
    );
    let value = &failure.prefix;
    assert_stopped_prefix(value);
    assert_eq!(value.pairs.len(), 1);
    let pair = &value.pairs[0];
    assert_eq!(pair.exit_reason, Some(ExitReason::Terminal));
    assert_eq!(pair.exit_decision, Some(TERMINAL_AT));
    assert_eq!(pair.closing_deadline, Some(TERMINAL_FOLD_END));
    assert_eq!(pair.closed_observed_at, None);
    assert!(pair.max_exit_decision.unwrap() > TERMINAL_FOLD_END);
    let snapshot = value.snapshot.as_ref().unwrap();
    assert!(!matches!(
        snapshot.phase,
        CarryPhase::Flat | CarryPhase::Closed
    ));
    assert_eq!(snapshot.account.spot_quote.0, 46_499_800_000_000);
    assert_eq!(snapshot.account.perp_wallet.0, 47_500_000_099_900);
    assert_eq!(snapshot.account.idle.0, 5_000_000_000_000);
    assert_eq!(snapshot.account.spot_btc.ticks(), 9_990_000_000);
    assert_eq!(snapshot.account.perp_short.ticks(), 9_990_000_000);
    assert_eq!(snapshot.account.spot_btc_fees.ticks(), 10_000_000);
    assert_eq!(snapshot.account.spot_quote_fees.0, 0);
    assert_eq!(snapshot.account.perp_fees.0, 499_400_100);
    assert_eq!(snapshot.account.funding_net.0, 499_500_000);
    assert_eq!(snapshot.account.realized_perp.0, 0);
    assert_eq!(snapshot.fills.len(), 2);
    assert_eq!(snapshot.orders.len(), 3);
    let actual_ids: BTreeSet<_> = snapshot
        .orders
        .iter()
        .map(|order| (order.id.pair, order.id.stage, order.id.attempt))
        .collect();
    assert_eq!(
        actual_ids,
        BTreeSet::from([
            (1, CarryStage::SpotEntry, 0),
            (1, CarryStage::PerpEntry, 0),
            (1, CarryStage::PerpExit, 0),
        ])
    );
    assert!(snapshot
        .orders
        .iter()
        .filter(|order| order.id.stage != CarryStage::PerpExit)
        .all(|order| order.complete_cut));
    let close_order = snapshot
        .orders
        .iter()
        .find(|order| order.id.stage == CarryStage::PerpExit)
        .unwrap();
    assert_eq!(close_order.qty.ticks(), 9_990_000_000);
    assert_eq!(close_order.progress.cum().ticks(), 0);
    assert!(snapshot.perp_collateral_reserved.0 > 0);
    let paid = snapshot
        .funding
        .iter()
        .find(|funding| funding.effective.micros() == START + 2 * SLOT)
        .unwrap();
    assert_eq!(paid.captured_short.ticks(), 9_990_000_000);
    assert_eq!(paid.cashflow, Some(Cash(499_500_000)));
    assert!(paid.receipt.is_some());
    for unpaid in snapshot
        .funding
        .iter()
        .filter(|funding| funding.effective.micros() >= TERMINAL_FOLD_END)
    {
        assert_eq!(unpaid.effective.micros(), TERMINAL_FOLD_END);
        assert_eq!(unpaid.captured_short.ticks(), 9_990_000_000);
        assert!(unpaid.receipt.is_none());
        assert!(unpaid.cashflow.is_none());
    }
    assert!(
        value.daily.is_empty(),
        "a technical prefix is not a zero-return day"
    );
    assert!(value.records.iter().any(|record| matches!(record,
        ActorRecord::Order { at, stage: "perp_exit", deadline, .. }
            if *at == TERMINAL_AT && *deadline == TERMINAL_FOLD_END)));
    assert!(!value.records.iter().any(|record| matches!(
        record,
        ActorRecord::Fill {
            stage: "perp_exit" | "spot_exit",
            ..
        }
    )));
    assert!(value
        .records
        .iter()
        .all(|record| record_clock(record) <= TERMINAL_FOLD_END));
    let healthy = run_terminal_fixture(false).unwrap();
    let before_terminal = |result: &ReplayResult| {
        result
            .records
            .iter()
            .filter(|record| record_clock(record) < TERMINAL_AT)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        before_terminal(value),
        before_terminal(&healthy),
        "future zero counts must not change an earlier financial prefix"
    );
}

#[test]
fn an_active_pair_closes_at_the_fixed_fold_terminal_with_all_costs() {
    assert_terminal_closed(&run_terminal_fixture(false).unwrap());
}

#[test]
fn zero_closing_liquidity_retains_a_technical_prefix_at_the_original_end() {
    assert_terminal_blocked(&run_terminal_fixture(true).unwrap_err());
}

#[derive(Debug, Default, cucumber::World)]
struct C1World {
    case: Option<&'static str>,
    projections: Option<(i128, i128, i128)>,
    evaluation: Option<C1FundingEvaluation>,
    incomplete_committed: Option<i64>,
    full_committed: Option<i64>,
    false_armed: bool,
    duplicate_armed: bool,
    cycles: Option<ReplayResult>,
    failure: Option<ReplayFailure>,
    refusal_preserved: Option<(bool, bool)>,
    zero_count_entry: Option<(bool, Option<i64>, usize, i64)>,
    max: Option<ReplayResult>,
    epoch_arming: Option<(bool, bool)>,
}

#[given("nine causal past funding values of one basis point")]
fn nine(world: &mut C1World) {
    world.case = Some("nine");
}

#[given("an older funding value without the expected latest value")]
fn missing_latest(world: &mut C1World) {
    world.case = Some("missing");
}

#[given("a valid nonpositive latest rate and incomplete history")]
fn latest_only(world: &mut C1World) {
    world.case = Some("latest");
}

#[when("the actual C1 feature is evaluated")]
fn feature(world: &mut C1World) {
    let rates = match world.case.unwrap() {
        "nine" => (-8..=0)
            .map(|i| (START + i * SLOT, 10_000, START + i * SLOT + MINUTE))
            .collect(),
        "missing" => vec![(START - SLOT, 10_000, START - SLOT + MINUTE)],
        "latest" => vec![(START, -10_000, START + MINUTE)],
        _ => panic!("known feature case"),
    };
    let source = source_with_rates(rates, vec![]);
    let frame = source.rate_frame_at(decision(START + HOUR + MS)).unwrap();
    let evaluated = evaluate_c1_funding(
        decision(START + HOUR + MS),
        frame.expected_latest.unwrap(),
        &frame.views(),
    )
    .unwrap();
    if let C1FundingEvaluation::Latest {
        history: C1History::Complete(value),
        ..
    } = evaluated
    {
        let short = value.project(C1MaxHold::Hours168).unwrap();
        let long = value.project(C1MaxHold::Hours336).unwrap();
        assert_eq!(short.denominator(), long.denominator());
        world.projections = Some((short.numerator(), long.numerator(), short.denominator()));
    }
    world.evaluation = Some(evaluated);
}

#[then("the projections are exactly twenty one and forty two basis points")]
fn projected(world: &mut C1World) {
    let (short, long, denominator) = world.projections.unwrap();
    assert_eq!(short * 10_000, 21 * denominator);
    assert_eq!(long * 10_000, 42 * denominator);
}

#[then("the expected latest value stays unavailable without stale substitution")]
fn no_stale(world: &mut C1World) {
    assert!(
        matches!(world.evaluation, Some(C1FundingEvaluation::ExpectedUnavailable { expected_key }) if expected_key.micros() == START)
    );
}

#[then("the latest exit rate remains available without a complete entry feature")]
fn latest_exit(world: &mut C1World) {
    assert!(
        matches!(world.evaluation, Some(C1FundingEvaluation::Latest { rate, history: C1History::Incomplete { .. } }) if rate.rate().ticks() == -10_000)
    );
}

#[given("the same key becomes complete after a pending generated evaluation")]
fn pending_case(world: &mut C1World) {
    world.case = Some("pending");
}

#[when("the real generated owner processes pending complete and duplicate evaluations")]
fn pending_complete(world: &mut C1World) {
    let first = START + HOUR + MS;
    let full = START + 2 * HOUR + MS;
    let mut rates = Vec::new();
    for index in -8..=0 {
        let key = START + index * SLOT;
        rates.push((
            key,
            if index == 0 { -1_000 } else { 10_000 },
            if index == -8 { full } else { key + MINUTE },
        ));
    }
    let mut bars = Vec::new();
    for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
        bars.push(bar(source, START + 2 * HOUR - MINUTE, 10_000_000_000));
    }
    let source = source_with_rates(rates, bars);
    let mut replay = C1Replay::new(&source, Scenario::Headline, window()).unwrap();
    replay.hour(first).unwrap();
    world.incomplete_committed = replay.committed.map(|value| value.key().micros());
    assert!(
        matches!(replay.control.read().values.get("evaluation"), Some(FiniteValue::String(v)) if v == "Pending")
    );
    replay.hour(full).unwrap();
    world.full_committed = replay.committed.map(|value| value.key().micros());
    world.false_armed = replay.flag("armed");
    replay.hour(START + 3 * HOUR + MS).unwrap();
    world.duplicate_armed = replay.flag("armed");
    assert!(replay.pairs.is_empty());
    assert_eq!(
        replay.committed.map(|value| value.key().micros()),
        Some(START)
    );
}

#[then("pending keeps the key and the first complete evaluation consumes it once")]
fn consumed(world: &mut C1World) {
    assert_eq!(world.incomplete_committed, None);
    assert_eq!(world.full_committed, Some(START));
    assert!(world.false_armed && world.duplicate_armed);
}

#[given("two funded carry cycles with no capital reset")]
fn cycles_case(world: &mut C1World) {
    world.case = Some("cycles");
}

#[given("two completed cycles followed by a flat selected policy fold")]
fn fold_case(world: &mut C1World) {
    world.case = Some("fold");
}

#[given("a held pair whose latest required mark is missing")]
fn gap_case(world: &mut C1World) {
    world.case = Some("gap");
}

#[given("a known sizing refusal that overlaps a negative basis")]
fn refused_false(world: &mut C1World) {
    world.case = Some("refused_false");
}

#[when("the actual actor evaluates both initial arming states")]
fn refused_arm(world: &mut C1World) {
    world.refusal_preserved = Some((refusal_preserves_arm(false), refusal_preserves_arm(true)));
}

#[then("the key is consumed and the prior arm is preserved without a new false")]
fn refused_result(world: &mut C1World) {
    assert_eq!(world.refusal_preserved, Some((false, true)));
}

#[given("an armed flat account and a zero count reference for a new positive key")]
fn zero_count_case(world: &mut C1World) {
    world.case = Some("zero_count");
}

#[when("the actual actor receives a valid later frame for the same key")]
fn zero_count_later(world: &mut C1World) {
    world.zero_count_entry = Some(zero_count_then_valid());
}

#[then("the unavailable frame preserves the arm and the valid frame dispatches once")]
fn zero_count_result(world: &mut C1World) {
    assert_eq!(
        world.zero_count_entry,
        Some((true, Some(START), 1, START + 10 * HOUR + MS))
    );
}

#[given("a fully observed pair with positive funding through the real 336 hour hold")]
fn max_case(world: &mut C1World) {
    world.case = Some("max336");
}

#[when("the actor reaches the first scheduled MAX exit after admitted completion")]
fn max_run(world: &mut C1World) {
    world.max = Some(run_max(&max_source(), C1MaxHold::Hours336));
}

#[then("the actual exit uses the unchanged 336 hour timer and preserves all cashflows")]
fn max_result(world: &mut C1World) {
    assert_actual_max(world.max.as_ref().unwrap(), C1MaxHold::Hours336);
}

#[given("a flat fold observed a negative key before a later active epoch")]
fn epoch_case(world: &mut C1World) {
    world.case = Some("epoch");
}

#[when("the actor sees the old key and then the first post activation key")]
fn epoch_run(world: &mut C1World) {
    world.epoch_arming = Some(epoch_does_not_rearm_from_a_pre_epoch_seen_key());
}

#[then("the old key changes no control or consumption and only the new false arms")]
fn epoch_result(world: &mut C1World) {
    assert_eq!(world.epoch_arming, Some((false, true)));
}

#[when("the actual C1 actor replays the bounded synthetic source")]
fn actor(world: &mut C1World) {
    let source = two_cycles_source(world.case == Some("gap"));
    let windows = if world.case == Some("fold") {
        split_windows()
    } else {
        window()
    };
    match C1Replay::new(&source, Scenario::Headline, windows)
        .unwrap()
        .run()
    {
        Ok(value) => world.cycles = Some(value),
        Err(value) => world.failure = Some(value),
    }
}

#[then("both cycles retain cash fees funding history and completion clocks")]
fn cycles_result(world: &mut C1World) {
    let value = world.cycles.as_ref().unwrap();
    assert_eq!(value.pairs.len(), 2);
    assert_eq!(value.final_equity, 99_992_385_619_600);
    assert_eq!(value.snapshot.as_ref().unwrap().fills.len(), 8);
    assert_eq!(value.snapshot.as_ref().unwrap().funding.len(), 2);
    assert_eq!(value.daily.len(), 9);
    if world.case == Some("fold") {
        assert!(value.records.iter().any(|record| matches!(record,
            ActorRecord::Control { at, event: "NewEpoch", .. } if *at == START + 2 * DAY)));
    }
    assert_dependent_decisions(value);
    assert!(value
        .pairs
        .iter()
        .all(|pair| pair.closed_observed_at.is_some()
            && pair.physical_paired_at.unwrap() < pair.observed_paired_at.unwrap()));
}

#[then("the whole replay is technical with its actual financial prefix retained")]
fn gap_result(world: &mut C1World) {
    let value = world.failure.as_ref().unwrap();
    assert!(
        value.stop_error.is_none(),
        "secondary Stop failure: {:?}",
        value.stop_error
    );
    assert_stopped_prefix(&value.prefix);
    assert!(matches!(
        value.error,
        ReplayError::Source {
            error: SourceError::Unavailable {
                source: PriceSource::UmMark,
                health: SourceHealth::Gap,
                ..
            },
            ..
        }
    ));
    assert_eq!(value.prefix.snapshot.as_ref().unwrap().fills.len(), 2);
    assert_entry_prefix(value.prefix.snapshot.as_ref().unwrap());
    assert_eq!(value.prefix.daily.len(), 0);
    assert!(world.cycles.is_none());
}

#[tokio::test]
async fn c1_actor_feature() {
    C1World::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/c1_actor.feature"
        ))
        .await;
}

#[given("a paired position held until the predeclared fold Terminal")]
fn terminal_case(world: &mut C1World) {
    world.case = Some("terminal");
}

#[when("the Terminal source has positive count closing minutes")]
fn terminal_liquid(world: &mut C1World) {
    world.cycles = Some(run_terminal_fixture(false).unwrap());
}

#[when("the Terminal source has zero count closing minutes")]
fn terminal_zero(world: &mut C1World) {
    world.failure = Some(run_terminal_fixture(true).unwrap_err());
}

#[then("Terminal closes before the original fold end with all costs retained")]
fn terminal_completed(world: &mut C1World) {
    assert_terminal_closed(world.cycles.as_ref().unwrap());
    assert!(world.failure.is_none());
}

#[then("the replay fails at the original fold end with its financial prefix retained")]
fn terminal_failed(world: &mut C1World) {
    assert_terminal_blocked(world.failure.as_ref().unwrap());
    assert!(world.cycles.is_none());
}
