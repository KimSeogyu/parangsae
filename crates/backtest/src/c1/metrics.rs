//! Metrics for completed C1 replays. Failed prefixes never supply a verdict.

use std::collections::{BTreeMap, BTreeSet};

use parangsae_execution::carry::CarryPhase;
use parangsae_sim::Cash;
use serde::Serialize;

use crate::{daily_return_tick, metrics, stats, UtcRange};

use super::actor::ReplayFailure;
use super::report::{AccountRecord, ActorRecord, ExitReason, ReplayResult};

const DAY: i64 = 86_400_000_000;
const MS: i64 = 1_000;
const C0: i64 = 100_000_000_000_000;
const TRAIN_START: i64 = 1_608_508_800_000_000;
const OOS_START: i64 = 1_640_131_200_000_000;
const OOS_END: i64 = 1_759_276_800_000_000;
const FOLDS: usize = 15;
const TRAIN_DAYS: usize = 365;
const STEP_DAYS: i64 = 90;
const IS_DAYS: usize = 1_625;
const OOS_DAYS: usize = 1_379;
const CSCV_GAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MetricError {
    Technical,
    Window,
    Calendar,
    Return,
    Unsettled,
    Reconciliation,
    Pair,
    Statistics,
    HistoryFloor,
}

impl std::fmt::Display for MetricError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "C1 metrics refused {:?} input", self)
    }
}

impl std::error::Error for MetricError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct SeriesMetrics {
    pub start_us: i64,
    pub end_exclusive_us: i64,
    pub calendar_days: usize,
    pub initial_capital_ticks: i64,
    pub final_equity_ticks: i64,
    pub net_cash_ticks: i64,
    pub total_return_ticks: i64,
    pub daily_sharpe_ticks: Option<i64>,
    pub annual_sharpe_ticks: Option<i64>,
    pub daily_close_max_drawdown_ticks: i64,
    pub skew_ticks: Option<i64>,
    pub nonexcess_kurtosis_ticks: Option<i64>,
    pub cash_only_days: usize,
    pub closed_pairs: usize,
    pub normal_round_trips: usize,
    pub terminal_pairs: usize,
    pub risk_pairs: usize,
    pub hedge_unwind_pairs: usize,
    pub normal_close_times_us: Vec<i64>,
    pub funding_net_ticks: i64,
    pub spot_quote_fees_ticks: i64,
    pub spot_btc_fee_qty_ticks: i64,
    pub perp_fees_ticks: i64,
    pub sampled_risk_ticks: u64,
    pub sharpe_denominator: &'static str,
    pub drawdown_sampling: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Verdict {
    RejectOnDeclaredModelSnapshot,
    InconclusiveSparseNoPromotion,
    InconclusiveHistory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Gates {
    pub headline_positive_return: bool,
    pub headline_positive_sharpe: bool,
    pub higher_fee_positive_sharpe: bool,
    pub dsr_at_least_95: bool,
    pub pbo_at_most_20: bool,
    pub coverage_50_pairs_10_folds: bool,
}

impl Gates {
    fn verdict(self) -> Verdict {
        if !(self.headline_positive_return
            && self.headline_positive_sharpe
            && self.higher_fee_positive_sharpe
            && self.dsr_at_least_95
            && self.pbo_at_most_20)
        {
            Verdict::RejectOnDeclaredModelSnapshot
        } else if !self.coverage_50_pairs_10_folds {
            Verdict::InconclusiveSparseNoPromotion
        } else {
            Verdict::InconclusiveHistory
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct SelectionStatistics {
    pub headline: SeriesMetrics,
    pub higher_fee: SeriesMetrics,
    pub candidate_config_count: usize,
    pub declared_charged_role_cap: usize,
    pub implemented_history_floor: u64,
    pub exact_global_n: Option<u64>,
    pub history_complete: bool,
    pub is_dates_us: Vec<i64>,
    pub is_source_folds: Vec<usize>,
    pub is_return_columns: Vec<Vec<i64>>,
    pub cscv_gap_rows: usize,
    pub cscv_splits: usize,
    pub cross_config_variance_ticks: i64,
    pub dsr_probability_ticks: Option<i64>,
    pub pbo_probability_ticks: Option<i64>,
    pub active_test_folds: usize,
    pub gates: Gates,
    pub verdict: Verdict,
}

pub(super) fn series_metrics(
    run: Result<&ReplayResult, &ReplayFailure>,
    expected: UtcRange,
) -> Result<SeriesMetrics, MetricError> {
    let result = run.map_err(|_| MetricError::Technical)?;
    let start = expected.start.micros();
    let end = expected.end.micros();
    let span = end.checked_sub(start).ok_or(MetricError::Window)?;
    if span <= 0 || start.rem_euclid(DAY) != 0 || span % DAY != 0 {
        return Err(MetricError::Window);
    }
    let days = usize::try_from(span / DAY).map_err(|_| MetricError::Window)?;
    if result.daily.len() != days {
        return Err(MetricError::Calendar);
    }

    let mut levels = Vec::with_capacity(days + 1);
    levels.push(Cash(C0));
    let mut returns = Vec::with_capacity(days);
    let mut previous = C0;
    let mut cursor = start;
    for day in &result.daily {
        let day_end = cursor.checked_add(DAY).ok_or(MetricError::Calendar)?;
        if day.day_start != cursor
            || day.economic_end_exclusive != day_end
            || day.known_at != day_end.checked_add(MS).ok_or(MetricError::Calendar)?
            || day.pending_funding != 0
        {
            return Err(MetricError::Calendar);
        }
        if day.equity <= 0
            || daily_return_tick(previous, day.equity).map_err(|_| MetricError::Return)?
                != day.return_ticks
        {
            return Err(MetricError::Return);
        }
        if day.cash_only
            && (day.account.spot_btc != 0
                || day.account.perp_short != 0
                || flat_cash(&day.account)? != day.equity)
        {
            return Err(MetricError::Reconciliation);
        }
        levels.push(Cash(day.equity));
        returns.push(day.return_ticks);
        previous = day.equity;
        cursor = day_end;
    }
    if cursor != end || previous != result.final_equity {
        return Err(MetricError::Reconciliation);
    }
    let final_account = final_account(result)?;
    if flat_cash(&final_account)? != result.final_equity {
        return Err(MetricError::Reconciliation);
    }

    let (daily_sr, annual_sr) =
        metrics::sharpe_of_return_ticks(&returns).map_err(|_| MetricError::Statistics)?;
    let moments = stats::skew_kurt(&returns).map_err(|_| MetricError::Statistics)?;
    let mut out = SeriesMetrics {
        start_us: start,
        end_exclusive_us: end,
        calendar_days: days,
        initial_capital_ticks: C0,
        final_equity_ticks: result.final_equity,
        net_cash_ticks: result
            .final_equity
            .checked_sub(C0)
            .ok_or(MetricError::Statistics)?,
        total_return_ticks: metrics::total_return(Cash(C0), Cash(result.final_equity))
            .map_err(|_| MetricError::Statistics)?
            .ticks(),
        daily_sharpe_ticks: daily_sr.map(|value| value.ticks()),
        annual_sharpe_ticks: annual_sr.map(|value| value.ticks()),
        daily_close_max_drawdown_ticks: metrics::max_drawdown(&levels)
            .map_err(|_| MetricError::Statistics)?
            .ticks(),
        skew_ticks: moments.map(|value| value.0),
        nonexcess_kurtosis_ticks: moments.map(|value| value.1),
        cash_only_days: result.daily.iter().filter(|day| day.cash_only).count(),
        closed_pairs: result.pairs.len(),
        normal_round_trips: 0,
        terminal_pairs: 0,
        risk_pairs: 0,
        hedge_unwind_pairs: 0,
        normal_close_times_us: Vec::new(),
        funding_net_ticks: final_account.funding_net,
        spot_quote_fees_ticks: final_account.spot_quote_fees,
        spot_btc_fee_qty_ticks: final_account.spot_btc_fees,
        perp_fees_ticks: final_account.perp_fees,
        sampled_risk_ticks: result.risk_ticks,
        sharpe_denominator: "full-capital daily 00:00Z net; ddof1; sqrt365; calendar days",
        drawdown_sampling: "daily economic session closes including original C0; not intraminute",
    };
    if result.daily.last().map(|day| &day.account) != Some(&final_account) {
        return Err(MetricError::Reconciliation);
    }
    count_pairs(result, start, end, &final_account, &mut out)?;
    let funding = result.records.iter().try_fold(0i64, |sum, record| {
        if let ActorRecord::FundingReceipt { cashflow, .. } = record {
            sum.checked_add(*cashflow)
                .ok_or(MetricError::Reconciliation)
        } else {
            Ok(sum)
        }
    })?;
    if funding != out.funding_net_ticks {
        return Err(MetricError::Reconciliation);
    }
    Ok(out)
}

fn flat_cash(account: &AccountRecord) -> Result<i64, MetricError> {
    if account.spot_btc != 0 || account.perp_short != 0 {
        return Err(MetricError::Unsettled);
    }
    account
        .spot_quote
        .checked_add(account.perp_wallet)
        .and_then(|value| value.checked_add(account.idle))
        .ok_or(MetricError::Reconciliation)
}

fn final_account(result: &ReplayResult) -> Result<AccountRecord, MetricError> {
    if let Some(snapshot) = &result.snapshot {
        if !matches!(snapshot.phase, CarryPhase::Flat | CarryPhase::Closed)
            || snapshot.unsupported.is_some()
            || snapshot.orders.iter().any(|order| !order.complete_cut)
            || snapshot
                .funding
                .iter()
                .any(|funding| funding.receipt.is_none() || funding.cashflow.is_none())
            || snapshot.spot_quote_reserved.0 != 0
            || snapshot.perp_collateral_reserved.0 != 0
        {
            return Err(MetricError::Unsettled);
        }
        return Ok((&snapshot.account).into());
    }
    let financial = result.records.iter().any(|record| {
        !matches!(
            record,
            ActorRecord::Control { .. }
                | ActorRecord::Evaluation { .. }
                | ActorRecord::ZeroExposureBoundary { .. }
        )
    });
    if financial
        || !result.pairs.is_empty()
        || result.final_equity != C0
        || result.daily.iter().any(|day| {
            !day.cash_only
                || day.equity != C0
                || day.return_ticks != 0
                || day.account.funding_net != 0
                || day.account.spot_quote_fees != 0
                || day.account.spot_btc_fees != 0
                || day.account.perp_fees != 0
        })
    {
        return Err(MetricError::Unsettled);
    }
    result
        .daily
        .last()
        .map(|day| day.account.clone())
        .ok_or(MetricError::Calendar)
}

fn count_pairs(
    result: &ReplayResult,
    start: i64,
    end: i64,
    final_account: &AccountRecord,
    out: &mut SeriesMetrics,
) -> Result<(), MetricError> {
    let mut closed = BTreeMap::new();
    for record in &result.records {
        if let ActorRecord::PairClosed { at, pair, account } = record {
            if closed.insert(pair.as_str(), (*at, account)).is_some() {
                return Err(MetricError::Pair);
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut previous_account = initial_account_record();
    let mut previous_close = start;
    for pair in &result.pairs {
        let at = pair.closed_observed_at.ok_or(MetricError::Pair)?;
        let (record_at, account) = closed.get(pair.pair.as_str()).ok_or(MetricError::Pair)?;
        if !seen.insert(pair.pair.as_str())
            || pair.opened_decision < start
            || pair.opened_decision < previous_close
            || pair.opened_decision >= end
            || at < pair.opened_decision
            || at >= end
            || *record_at != at
            || pair.gross_spot <= 0
            || pair.net_btc <= 0
        {
            return Err(MetricError::Pair);
        }
        if pair.proposal_account != previous_account {
            return Err(MetricError::Reconciliation);
        }
        flat_cash(account)?;
        let reason = pair.exit_reason.ok_or(MetricError::Pair)?;
        match reason {
            ExitReason::MaxHold | ExitReason::NonpositiveFunding => {
                let physical = pair.physical_paired_at.ok_or(MetricError::Pair)?;
                let observed = pair.observed_paired_at.ok_or(MetricError::Pair)?;
                let exit = pair.exit_decision.ok_or(MetricError::Pair)?;
                if pair.opened_decision > physical
                    || physical > observed
                    || observed > exit
                    || exit > at
                {
                    return Err(MetricError::Pair);
                }
                out.normal_round_trips += 1;
                out.normal_close_times_us.push(at);
            }
            ExitReason::Terminal => out.terminal_pairs += 1,
            ExitReason::Risk => out.risk_pairs += 1,
            ExitReason::HedgeEnvelopeUnwind => out.hedge_unwind_pairs += 1,
        }
        previous_account = (*account).clone();
        previous_close = at;
    }
    if closed.len() != result.pairs.len() {
        return Err(MetricError::Pair);
    }
    if &previous_account != final_account {
        return Err(MetricError::Reconciliation);
    }
    Ok(())
}

fn initial_account_record() -> AccountRecord {
    AccountRecord {
        spot_quote: 47_500_000_000_000,
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

pub(super) fn selection_statistics(
    train_folds: &[Vec<(i64, Vec<i64>)>],
    headline: Result<&ReplayResult, &ReplayFailure>,
    higher_fee: Result<&ReplayResult, &ReplayFailure>,
    n_floor: u64,
) -> Result<SelectionStatistics, MetricError> {
    if n_floor < 202 {
        return Err(MetricError::HistoryFloor);
    }
    let expected = UtcRange {
        start: parangsae_core::UtcMicros::from_micros(OOS_START),
        end: parangsae_core::UtcMicros::from_micros(OOS_END),
    };
    let headline_result = headline.map_err(|_| MetricError::Technical)?;
    let higher_fee_result = higher_fee.map_err(|_| MetricError::Technical)?;
    let headline = series_metrics(Ok(headline_result), expected)?;
    let higher_fee = series_metrics(Ok(higher_fee_result), expected)?;
    if headline.calendar_days != OOS_DAYS {
        return Err(MetricError::Calendar);
    }
    let active_folds = active_test_folds(headline_result)?;
    active_test_folds(higher_fee_result)?;
    validate_train_shape(train_folds)?;
    let (dates, columns) = stats::is_matrix(train_folds).map_err(|_| MetricError::Statistics)?;
    if dates.len() != IS_DAYS || columns.len() != 2 {
        return Err(MetricError::Calendar);
    }
    let source_folds = matrix_source_folds(&dates, &columns, train_folds)?;
    let variance = stats::sr_var_ann_ticks(&columns).map_err(|_| MetricError::Statistics)?;
    let dsr = match (
        headline.annual_sharpe_ticks,
        headline.skew_ticks,
        headline.nonexcess_kurtosis_ticks,
    ) {
        (Some(sr), Some(skew), Some(kurt)) => stats::deflated_sharpe(
            sr,
            n_floor,
            variance,
            u64::try_from(headline.calendar_days).map_err(|_| MetricError::Statistics)?,
            skew,
            kurt,
        )
        .map_err(|_| MetricError::Statistics)?,
        _ => None,
    };
    let pbo = stats::pbo_ticks(&columns, CSCV_GAP).map_err(|_| MetricError::Statistics)?;
    let gates = Gates {
        headline_positive_return: headline.total_return_ticks > 0,
        headline_positive_sharpe: headline.annual_sharpe_ticks.is_some_and(|value| value > 0),
        higher_fee_positive_sharpe: higher_fee
            .annual_sharpe_ticks
            .is_some_and(|value| value > 0),
        dsr_at_least_95: dsr.is_some_and(|value| value >= 95_000_000),
        pbo_at_most_20: pbo.is_some_and(|value| value <= 20_000_000),
        coverage_50_pairs_10_folds: headline.normal_round_trips >= 50 && active_folds >= 10,
    };
    Ok(SelectionStatistics {
        headline,
        higher_fee,
        candidate_config_count: 2,
        declared_charged_role_cap: 5,
        implemented_history_floor: n_floor,
        exact_global_n: None,
        history_complete: false,
        is_dates_us: dates,
        is_source_folds: source_folds,
        is_return_columns: columns,
        cscv_gap_rows: CSCV_GAP,
        cscv_splits: 12_870,
        cross_config_variance_ticks: variance,
        dsr_probability_ticks: dsr,
        pbo_probability_ticks: pbo,
        active_test_folds: active_folds,
        gates,
        verdict: gates.verdict(),
    })
}

fn active_test_folds(result: &ReplayResult) -> Result<usize, MetricError> {
    let mut active = [false; FOLDS];
    for pair in &result.pairs {
        let closed = pair.closed_observed_at.ok_or(MetricError::Pair)?;
        let open_fold = test_fold(pair.opened_decision)?;
        let close_fold = test_fold(closed)?;
        if open_fold != close_fold {
            return Err(MetricError::Pair);
        }
        match pair.exit_reason.ok_or(MetricError::Pair)? {
            ExitReason::MaxHold | ExitReason::NonpositiveFunding => active[close_fold] = true,
            ExitReason::Terminal | ExitReason::Risk | ExitReason::HedgeEnvelopeUnwind => {}
        }
    }
    Ok(active.iter().filter(|value| **value).count())
}

fn test_fold(at: i64) -> Result<usize, MetricError> {
    if !(OOS_START..OOS_END).contains(&at) {
        return Err(MetricError::Calendar);
    }
    let day = (at - OOS_START) / DAY;
    let index = usize::try_from(day / STEP_DAYS).map_err(|_| MetricError::Calendar)?;
    Ok(index.min(FOLDS - 1))
}

fn validate_train_shape(folds: &[Vec<(i64, Vec<i64>)>]) -> Result<(), MetricError> {
    if folds.len() != FOLDS {
        return Err(MetricError::Calendar);
    }
    for (index, fold) in folds.iter().enumerate() {
        let index = i64::try_from(index).map_err(|_| MetricError::Calendar)?;
        let start = TRAIN_START
            .checked_add(index * STEP_DAYS * DAY)
            .ok_or(MetricError::Calendar)?;
        if fold.len() != 2
            || fold
                .iter()
                .any(|(at, returns)| *at != start || returns.len() != TRAIN_DAYS)
        {
            return Err(MetricError::Calendar);
        }
    }
    Ok(())
}

fn matrix_source_folds(
    dates: &[i64],
    columns: &[Vec<i64>],
    folds: &[Vec<(i64, Vec<i64>)>],
) -> Result<Vec<usize>, MetricError> {
    let mut sources = Vec::with_capacity(dates.len());
    let mut expected = TRAIN_START;
    for (row, at) in dates.iter().enumerate() {
        if *at != expected {
            return Err(MetricError::Calendar);
        }
        let (source, fold) = folds
            .iter()
            .enumerate()
            .find(|(_, fold)| *at >= fold[0].0 && *at < fold[0].0 + 365 * DAY)
            .ok_or(MetricError::Calendar)?;
        let offset = usize::try_from((*at - fold[0].0) / DAY).map_err(|_| MetricError::Calendar)?;
        for (column, values) in columns.iter().enumerate() {
            if values.get(row) != fold[column].1.get(offset) {
                return Err(MetricError::Reconciliation);
            }
        }
        sources.push(source + 1);
        expected = expected.checked_add(DAY).ok_or(MetricError::Calendar)?;
    }
    Ok(sources)
}

#[cfg(test)]
#[path = "metrics_cases.rs"]
mod tests;
