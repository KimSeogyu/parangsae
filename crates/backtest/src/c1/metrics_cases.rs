use super::*;

use cucumber::{given, then, when, World as _};
use parangsae_core::{Qty, UtcMicros};
use parangsae_execution::carry::CarrySnapshot;
use parangsae_risk::HaltEpoch;
use parangsae_sim::carry::CarryAccountState;
use sgd_owned_c1opportunitycontrol_50dd0338899d::C1ControlMachine;

use crate::c1::actor::{ReplayError, ReplayFailure};
use crate::c1::report::{DailyCapital, PairRecord, SourceRecord};

const START: i64 = 1_704_326_400_000_000;
const UNIT: i64 = 100_000_000;

fn account(equity: i64) -> AccountRecord {
    AccountRecord {
        spot_quote: equity - 52_500_000_000_000,
        spot_btc: 0,
        perp_wallet: 47_500_000_000_000,
        perp_short: 0,
        idle: 5_000_000_000_000,
        spot_quote_fees: 0,
        spot_btc_fees: 0,
        perp_fees: 0,
        funding_net: 0,
        realized_perp: 0,
        perp_entry_cost_scaled: "0".into(),
        realized_perp_scaled: "0".into(),
    }
}

fn snapshot(value: &AccountRecord) -> CarrySnapshot {
    CarrySnapshot {
        account: CarryAccountState {
            spot_quote: Cash(value.spot_quote),
            spot_btc: Qty::from_ticks(0).unwrap(),
            perp_wallet: Cash(value.perp_wallet),
            perp_short: Qty::from_ticks(0).unwrap(),
            idle: Cash(value.idle),
            spot_quote_fees: Cash(0),
            spot_btc_fees: Qty::from_ticks(0).unwrap(),
            perp_fees: Cash(0),
            funding_net: Cash(0),
            realized_perp: Cash(0),
            perp_entry_cost_scaled: 0,
            realized_perp_scaled: 0,
        },
        phase: CarryPhase::Closed,
        pair: None,
        orders: Vec::new(),
        fills: Vec::new(),
        funding: Vec::new(),
        spot_quote_reserved: Cash(0),
        perp_collateral_reserved: Cash(0),
        halted: false,
        epoch: HaltEpoch::ZERO,
        loss_latched: false,
        margin_latched: false,
        unsupported: None,
        reconciled_epoch: None,
        hedged_at: None,
    }
}

fn source() -> SourceRecord {
    SourceRecord {
        object: "synthetic/metric-report".into(),
        sha256: "0".repeat(64),
        row: 0,
        original_event_us: START,
        original_available_us: START,
        original_received_us: START,
        raw_unit: "micros".into(),
    }
}

fn pair(index: usize, equity: i64, reason: ExitReason) -> (PairRecord, ActorRecord) {
    let day = i64::try_from(index).unwrap();
    let opened = START + day * DAY + MS;
    let at = opened + 60_000_000;
    let name = format!("synthetic-pair-{index}");
    (
        PairRecord {
            pair: name.clone(),
            origin_key: opened,
            opened_decision: opened,
            entry_epoch_start: START,
            opening_deadline: opened + 600_000_000,
            physical_paired_at: Some(opened + MS),
            observed_paired_at: Some(opened + 2 * MS),
            max_exit_decision: Some(at - MS),
            exit_reason: Some(reason),
            exit_decision: Some(at - MS),
            closing_deadline: Some(at + 600_000_000),
            closed_observed_at: Some(at),
            gross_spot: UNIT,
            net_btc: 99_900_000,
            forecast_numerator: "1".into(),
            forecast_denominator: "1".into(),
            headline_c4: 1,
            sum9_ticks: "1".into(),
            latest_rate_ticks: 1,
            decision_spot_reference: UNIT,
            decision_perp_reference: UNIT,
            spot_reference_source: source(),
            perp_reference_source: source(),
            proposal_account: account(C0),
        },
        ActorRecord::PairClosed {
            at,
            pair: name,
            account: account(equity),
        },
    )
}

// Completed synthetic account reports. These fixtures do not claim an actor replay.
fn report(levels: &[i64], reasons: &[ExitReason]) -> ReplayResult {
    let mut previous = C0;
    let daily = levels
        .iter()
        .enumerate()
        .map(|(index, equity)| {
            let at = START + i64::try_from(index).unwrap() * DAY;
            let value = DailyCapital {
                day_start: at,
                economic_end_exclusive: at + DAY,
                known_at: at + DAY + MS,
                equity: *equity,
                return_ticks: daily_return_tick(previous, *equity).unwrap(),
                cash_only: true,
                pending_funding: 0,
                account: account(*equity),
            };
            previous = *equity;
            value
        })
        .collect();
    let (pairs, records): (Vec<_>, Vec<_>) = reasons
        .iter()
        .enumerate()
        .map(|(index, reason)| {
            let (mut pair, record) = pair(index, levels[index], *reason);
            pair.proposal_account = account(if index == 0 { C0 } else { levels[index - 1] });
            (pair, record)
        })
        .unzip();
    ReplayResult {
        records,
        pairs,
        daily,
        snapshot: (!reasons.is_empty()).then(|| snapshot(&account(previous))),
        risk_ticks: 0,
        final_equity: previous,
        control: C1ControlMachine::new().read(),
    }
}

fn range(days: i64) -> UtcRange {
    UtcRange {
        start: UtcMicros::from_micros(START),
        end: UtcMicros::from_micros(START + days * DAY),
    }
}

fn moving_report() -> ReplayResult {
    report(
        &[1_100_000 * UNIT, 990_000 * UNIT, 1_089_000 * UNIT],
        &[ExitReason::HedgeEnvelopeUnwind; 3],
    )
}

#[test]
fn continuous_equity_preserves_forced_cost_results_and_counts_only_calendar_days() {
    let value = moving_report();
    let out = series_metrics(Ok(&value), range(3)).unwrap();
    assert_eq!(out.calendar_days, 3);
    assert_eq!(out.total_return_ticks, 8_900_000);
    assert_eq!(out.daily_close_max_drawdown_ticks, 10_000_000);
    assert_eq!(out.net_cash_ticks, 89_000 * UNIT);
    assert_eq!(out.normal_round_trips, 0);
    assert_eq!(out.hedge_unwind_pairs, 3);
    assert_eq!(out.initial_capital_ticks, C0);
}

#[test]
fn flat_cash_retains_all_days_and_undefined_sharpe() {
    let value = report(&[C0; 4], &[]);
    let out = series_metrics(Ok(&value), range(4)).unwrap();
    assert_eq!(out.calendar_days, 4);
    assert_eq!(out.cash_only_days, 4);
    assert_eq!(out.total_return_ticks, 0);
    assert_eq!(out.annual_sharpe_ticks, None);
}

#[test]
fn a_cycle_or_fold_cannot_reset_cash_or_fractional_history() {
    let mut reset = moving_report();
    reset.pairs[1].proposal_account = account(C0);
    assert_eq!(
        series_metrics(Ok(&reset), range(3)),
        Err(MetricError::Reconciliation)
    );
    let mut residue = moving_report();
    residue.pairs[1].proposal_account.realized_perp_scaled = "1".into();
    assert_eq!(
        series_metrics(Ok(&residue), range(3)),
        Err(MetricError::Reconciliation)
    );
}

#[test]
fn failed_prefix_and_missing_or_changed_days_never_supply_returns() {
    let mut value = report(&[C0; 4], &[]);
    value.daily.remove(1);
    assert_eq!(
        series_metrics(Ok(&value), range(4)),
        Err(MetricError::Calendar)
    );
    let mut changed = report(&[C0; 4], &[]);
    changed.daily[1].return_ticks = 1;
    assert_eq!(
        series_metrics(Ok(&changed), range(4)),
        Err(MetricError::Return)
    );
    let failure = ReplayFailure {
        error: ReplayError::UnknownDailyLiability { at: START + DAY },
        stop_error: None,
        prefix: report(&[C0; 4], &[]),
    };
    assert_eq!(
        series_metrics(Err(&failure), range(4)),
        Err(MetricError::Technical)
    );
}

#[test]
fn only_completed_normal_pairs_contribute_activity() {
    let value = report(
        &[C0; 5],
        &[
            ExitReason::MaxHold,
            ExitReason::NonpositiveFunding,
            ExitReason::Terminal,
            ExitReason::Risk,
            ExitReason::HedgeEnvelopeUnwind,
        ],
    );
    let out = series_metrics(Ok(&value), range(5)).unwrap();
    assert_eq!(out.normal_round_trips, 2);
    assert_eq!(
        (out.terminal_pairs, out.risk_pairs, out.hedge_unwind_pairs),
        (1, 1, 1)
    );
}

#[test]
fn a_pair_cannot_cross_a_declared_test_terminal_boundary() {
    let mut value = report(&[C0; 1], &[ExitReason::MaxHold]);
    value.pairs[0].opened_decision = OOS_START + 90 * DAY - 60_000_000;
    value.pairs[0].closed_observed_at = Some(OOS_START + 90 * DAY);
    assert_eq!(active_test_folds(&value), Err(MetricError::Pair));
}

fn train_folds() -> Vec<Vec<(i64, Vec<i64>)>> {
    (0..FOLDS)
        .map(|fold| {
            let start = TRAIN_START + i64::try_from(fold).unwrap() * STEP_DAYS * DAY;
            vec![
                (start, vec![i64::try_from(fold + 1).unwrap(); 365]),
                (start, vec![0; 365]),
            ]
        })
        .collect()
}

#[test]
fn is_matrix_keeps_earliest_train_path_without_claiming_overlap_equality() {
    let folds = train_folds();
    validate_train_shape(&folds).unwrap();
    let (dates, columns) = stats::is_matrix(&folds).unwrap();
    let source = matrix_source_folds(&dates, &columns, &folds).unwrap();
    assert_eq!(dates.len(), 1_625);
    assert_eq!(columns[0][364], 1);
    assert_eq!(columns[0][365], 2);
    assert_eq!(columns[0][1_624], 15);
    assert_eq!(source[364], 1);
    assert_eq!(source[365], 2);
    assert_eq!(source[1_624], 15);
}

#[test]
fn actual_selection_statistics_uses_sixteen_boundary_rows_not_the_legacy_one() {
    // Every 101-row block has 32 edge returns of -3 and 69 middle returns of +1.
    // The last 110-row block has 78 middle returns. Column B is exactly -A.
    // OOS always prefers B. With gap16, IS prefers A iff at least five of the
    // fifteen block boundaries change half. There are 12462 such 8-of-16 layouts.
    // With gap1, IS always prefers B. This is a combinatorial synthetic oracle.
    let blocks = stats::block_ranges(IS_DAYS).unwrap();
    let mut a = Vec::with_capacity(IS_DAYS);
    for (start, end) in blocks {
        for row in start..end {
            a.push(if row - start < 16 || end - row <= 16 {
                -3
            } else {
                1
            });
        }
    }
    let b: Vec<i64> = a.iter().map(|value| -*value).collect();
    let folds: Vec<_> = (0..FOLDS)
        .map(|fold| {
            let from = fold * 90;
            let start = TRAIN_START + i64::try_from(from).unwrap() * DAY;
            vec![
                (start, a[from..from + 365].to_vec()),
                (start, b[from..from + 365].to_vec()),
            ]
        })
        .collect();
    let mut flat = report(&vec![C0; OOS_DAYS], &[]);
    let offset = OOS_START - START;
    for day in &mut flat.daily {
        day.day_start += offset;
        day.economic_end_exclusive += offset;
        day.known_at += offset;
    }
    let out = selection_statistics(&folds, Ok(&flat), Ok(&flat), 202).unwrap();
    assert_eq!(out.pbo_probability_ticks, Some(96_829_836));
    assert_eq!(
        stats::pbo_ticks(&out.is_return_columns, 1).unwrap(),
        Some(0)
    );
    assert_eq!(out.candidate_config_count, 2);
    assert_eq!(out.declared_charged_role_cap, 5);
    assert_eq!(out.headline.calendar_days, 1_379);
    assert_eq!(out.is_dates_us.len(), 1_625);
    assert_eq!(out.exact_global_n, None);
    assert!(!out.history_complete);
    assert_eq!(out.verdict, Verdict::RejectOnDeclaredModelSnapshot);
}

#[test]
fn verdict_precedence_never_promotes_favorable_results_or_hides_numeric_failure() {
    let mut gates = Gates {
        headline_positive_return: true,
        headline_positive_sharpe: true,
        higher_fee_positive_sharpe: true,
        dsr_at_least_95: true,
        pbo_at_most_20: true,
        coverage_50_pairs_10_folds: true,
    };
    assert_eq!(gates.verdict(), Verdict::InconclusiveHistory);
    gates.coverage_50_pairs_10_folds = false;
    assert_eq!(gates.verdict(), Verdict::InconclusiveSparseNoPromotion);
    gates.headline_positive_return = false;
    assert_eq!(gates.verdict(), Verdict::RejectOnDeclaredModelSnapshot);
}

#[derive(Debug, Default, cucumber::World)]
struct MetricWorld {
    case: String,
    json: Option<serde_json::Value>,
}

#[given(expr = "a completed C1 metrics fixture named {word}")]
fn named_fixture(world: &mut MetricWorld, name: String) {
    world.case = name;
}

#[when("the metrics result is serialized")]
fn serialize_result(world: &mut MetricWorld) {
    let out = match world.case.as_str() {
        "continuous" => series_metrics(Ok(&moving_report()), range(3)),
        "flat" => series_metrics(Ok(&report(&[C0; 4], &[])), range(4)),
        "missing" => {
            let mut value = report(&[C0; 4], &[]);
            value.daily.remove(1);
            series_metrics(Ok(&value), range(4))
        }
        "technical" => {
            let failure = ReplayFailure {
                error: ReplayError::UnknownDailyLiability { at: START + DAY },
                stop_error: None,
                prefix: report(&[C0; 4], &[]),
            };
            series_metrics(Err(&failure), range(4))
        }
        other => panic!("unknown synthetic fixture {other}"),
    };
    world.json = Some(serde_json::to_value(out).unwrap());
}

#[then(expr = "the reported {word} is {int}")]
fn number(world: &mut MetricWorld, field: String, expected: i64) {
    assert_eq!(world.json.as_ref().unwrap()["Ok"][&field], expected);
}

#[then("the annual Sharpe is undefined")]
fn undefined(world: &mut MetricWorld) {
    assert!(world.json.as_ref().unwrap()["Ok"]["annual_sharpe_ticks"].is_null());
}

#[then(expr = "the metrics refusal is {word}")]
fn refusal(world: &mut MetricWorld, expected: String) {
    assert_eq!(world.json.as_ref().unwrap()["Err"], expected);
    assert!(world.json.as_ref().unwrap().get("Ok").is_none());
}

#[tokio::test]
async fn c1_metrics_gherkin() {
    MetricWorld::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit("tests/features/c1_metrics.feature")
        .await;
}
