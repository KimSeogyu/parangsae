//! Fixed execution/statistics settings are admitted before any DATA hash.

use serde_json::{json, Value};

use super::actor::{C1Replay, PolicyWindow};
use super::admission::{
    admit, AdmittedC1, C1AdmissionError, C1BatchRequest, C1RoleConfig, ChargedRole,
};
use super::metrics::{selection_statistics, series_metrics, SelectionStatistics, SeriesMetrics};
use super::report::{replay_diagnostic, ReplayResult};
use super::sizing::Scenario;
use crate::c1_source::C1SourceView;
use crate::canon::{json_string, sha256_hex};
use crate::trial_log::{trial_id, StartRecord, TrialLog, TrialRoleKind, TrialRoleMetadata};
use crate::UtcRange;
use parangsae_core::UtcMicros;
use parangsae_strategies::c1::C1MaxHold;
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Debug)]
pub struct C1BatchError(pub String);

impl std::fmt::Display for C1BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for C1BatchError {}

#[derive(Clone, Debug, Serialize)]
struct Identity {
    study_id: String,
    attempt: u64,
    config_id: String,
    trial_id: String,
    run_id: String,
    role: TrialRoleMetadata,
    scenario: &'static str,
    unit_kind: &'static str,
    fold: Option<u32>,
    prereg_sha256: String,
    pinned_root: String,
}

#[derive(Debug)]
enum Outcome {
    Complete {
        run: ReplayResult,
        metrics: SeriesMetrics,
    },
    Technical {
        error: String,
        prefix: Option<ReplayResult>,
        stop_error: Option<String>,
    },
    NotExecuted,
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", content = "error", rename_all = "SCREAMING_SNAKE_CASE")]
enum Persistence {
    Finished,
    Aborted,
    StartUncertain(String),
    TerminalUncertain(String),
}

#[derive(Debug)]
struct Unit {
    identity: Identity,
    range: UtcRange,
    outcome: Outcome,
    persistence: Persistence,
}

impl Unit {
    fn complete(&self) -> Result<(&ReplayResult, &SeriesMetrics), C1BatchError> {
        match &self.persistence {
            Persistence::StartUncertain(error) | Persistence::TerminalUncertain(error) => {
                return Err(C1BatchError(format!("ledger persistence: {error}")))
            }
            Persistence::Aborted => {
                return Err(C1BatchError(match &self.outcome {
                    Outcome::Technical { error, .. } => error.clone(),
                    _ => "unit has no acknowledged successful terminal".to_owned(),
                }));
            }
            Persistence::Finished => {}
        }
        match &self.outcome {
            Outcome::Complete { run, metrics } => Ok((run, metrics)),
            Outcome::Technical { error, .. } => Err(C1BatchError(error.clone())),
            Outcome::NotExecuted => Err(C1BatchError("unit execution was not started".to_owned())),
        }
    }
    fn diagnostic(&self) -> Value {
        match &self.outcome {
            Outcome::Complete { run, metrics } => json!({"identity":self.identity,
                "status":if matches!(self.persistence,Persistence::Finished){"COMPLETE"}else{"INCOMPLETE"},
                "execution_status":"COMPLETE","ledger_persistence":self.persistence,
                "metrics":metrics,"raw":replay_diagnostic(run)}),
            Outcome::Technical {
                error,
                prefix,
                stop_error,
            } => json!({"identity":self.identity,
                "status":"TECHNICAL_UNRESOLVED","ledger_persistence":self.persistence,"error":error,
                "control_stop_error":stop_error,"metrics":null,"raw_partial":prefix.as_ref().map(replay_diagnostic)}),
            Outcome::NotExecuted => json!({"identity":self.identity,"status":"INCOMPLETE",
                "execution_status":"NOT_EXECUTED","ledger_persistence":self.persistence,"metrics":null,"raw":null}),
        }
    }
}

fn scenario(role: ChargedRole) -> (Scenario, &'static str, TrialRoleMetadata) {
    let (actual, name, kind, label) = match role {
        ChargedRole::TrainMax168 => (
            Scenario::Headline,
            "HEADLINE",
            TrialRoleKind::TrainingCandidate,
            "TRAIN_MAX168",
        ),
        ChargedRole::TrainMax336 => (
            Scenario::Headline,
            "HEADLINE",
            TrialRoleKind::TrainingCandidate,
            "TRAIN_MAX336",
        ),
        ChargedRole::OosHeadline => (
            Scenario::Headline,
            "HEADLINE",
            TrialRoleKind::ContinuousPolicy,
            "OOS_HEADLINE",
        ),
        ChargedRole::OosHigherPerpFee => (
            Scenario::HigherPerpFee,
            "HIGHER_PERP_FEE",
            TrialRoleKind::ContinuousPolicy,
            "OOS_HIGHER_PERP_FEE",
        ),
        ChargedRole::OosSlippageStress => (
            Scenario::SlippageStress,
            "SLIPPAGE_STRESS",
            TrialRoleKind::ContinuousPolicy,
            "OOS_SLIPPAGE_STRESS",
        ),
    };
    (
        actual,
        name,
        TrialRoleMetadata {
            role_kind: kind,
            semantic_role: label.to_owned(),
            scenario: name.to_owned(),
        },
    )
}

fn identity(
    admitted: &AdmittedC1,
    request: &C1BatchRequest,
    role: &C1RoleConfig,
    fold: Option<u32>,
) -> Identity {
    let (_, name, metadata) = scenario(role.role);
    let kind = if fold.is_some() {
        "train"
    } else {
        "continuous_oos"
    };
    Identity {
        study_id: admitted.inputs.study_id.0.clone(),
        attempt: request.attempt,
        config_id: role.config_id.0.clone(),
        trial_id: trial_id(&role.config_id.0, &admitted.pin, request.attempt),
        run_id: run_unit_id(
            &admitted.inputs.study_id.0,
            &role.config_id.0,
            request.attempt,
            fold,
        ),
        role: metadata,
        scenario: name,
        unit_kind: kind,
        fold,
        prereg_sha256: admitted.prereg_identity.sha256.0.clone(),
        pinned_root: admitted.pin.clone(),
    }
}

fn run_unit_id(study: &str, config: &str, attempt: u64, fold: Option<u32>) -> String {
    let kind = if fold.is_some() {
        "train"
    } else {
        "continuous_oos"
    };
    let fold_json = fold.map_or_else(|| "null".to_owned(), |fold| fold.to_string());
    let preimage = format!("{{\"attempt\":{},\"config_id\":{},\"fold\":{},\"schema\":\"parangsae-c1-run-unit-v1\",\"study_id\":{},\"unit_kind\":{}}}",
        attempt,json_string(config),fold_json,json_string(study),json_string(kind));
    sha256_hex(preimage.as_bytes())
}

fn run_unit(
    source: &C1SourceView,
    log: &TrialLog,
    identity: Identity,
    actual: Scenario,
    policies: Vec<PolicyWindow>,
) -> Result<Unit, C1BatchError> {
    let range = UtcRange {
        start: UtcMicros::from_micros(
            policies
                .first()
                .ok_or_else(|| C1BatchError("empty policy schedule".to_owned()))?
                .start,
        ),
        end: UtcMicros::from_micros(
            policies
                .last()
                .ok_or_else(|| C1BatchError("empty policy schedule".to_owned()))?
                .end,
        ),
    };
    let started = log.begin(StartRecord {
        run_id: &identity.run_id,
        trial_id: &identity.trial_id,
        config_id: &identity.config_id,
        pinned_root: &identity.pinned_root,
        prereg_sha256: &identity.prereg_sha256,
        attempt: identity.attempt,
        family: "C1",
        ts: range.start.micros(),
        fold: identity.fold,
        segment: Some(identity.unit_kind),
        role: Some(&identity.role),
    });
    if let Err(error) = started {
        return Ok(Unit {
            identity,
            range,
            outcome: Outcome::NotExecuted,
            persistence: Persistence::StartUncertain(error.to_string()),
        });
    }
    let outcome = execute_unit(source, actual, policies, range);
    Ok(persist_execution(log, identity, range, outcome))
}

fn execute_unit(
    source: &C1SourceView,
    actual: Scenario,
    policies: Vec<PolicyWindow>,
    range: UtcRange,
) -> Outcome {
    match C1Replay::new(source, actual, policies) {
        Err(error) => Outcome::Technical {
            error: format!("replay setup: {error:?}"),
            prefix: None,
            stop_error: None,
        },
        Ok(replay) => match replay.run() {
            Err(failure) => Outcome::Technical {
                error: format!("{:?}", failure.error),
                prefix: Some(*failure.prefix),
                stop_error: failure.stop_error.map(|error| format!("{error:?}")),
            },
            Ok(run) => match series_metrics(Ok(&run), range) {
                Ok(metrics) => Outcome::Complete { run, metrics },
                Err(error) => Outcome::Technical {
                    error: error.to_string(),
                    prefix: Some(run),
                    stop_error: None,
                },
            },
        },
    }
}

fn persist_execution(
    log: &TrialLog,
    identity: Identity,
    range: UtcRange,
    outcome: Outcome,
) -> Unit {
    let persistence = match &outcome {
        Outcome::Complete { metrics, .. } => match serde_json::to_vec(metrics) {
            Ok(bytes) => match log.finish(&identity.run_id, &sha256_hex(&bytes)) {
                Ok(()) => Persistence::Finished,
                Err(error) => Persistence::TerminalUncertain(error.to_string()),
            },
            Err(error) => Persistence::TerminalUncertain(error.to_string()),
        },
        Outcome::Technical { error, .. } => match log.abort(&identity.run_id, error) {
            Ok(()) => Persistence::Aborted,
            Err(error) => Persistence::TerminalUncertain(error.to_string()),
        },
        Outcome::NotExecuted => {
            Persistence::StartUncertain("execution was not admitted".to_owned())
        }
    };
    Unit {
        identity,
        range,
        outcome,
        persistence,
    }
}

pub fn run_c1_batch(request: &C1BatchRequest) -> Result<(), C1BatchError> {
    let admitted = admit(request).map_err(|error| C1BatchError(error.to_string()))?;
    let _exclusive_ledger = &admitted.ledger_file;
    let source = match C1SourceView::load_exact(&admitted.source_manifest) {
        Ok(source) => source,
        Err(error) => {
            let message = format!("source admission: {error:?}");
            write_outputs(&admitted, &[], &[], None, Some(&message))?;
            return Err(C1BatchError(message));
        }
    };
    let folds = schedule().map_err(|error| C1BatchError(error.to_string()))?;
    let log = TrialLog::new(&admitted.ledger);
    let mut units = Vec::with_capacity(33);
    let mut training = Vec::with_capacity(15);
    let mut selected = Vec::with_capacity(15);
    for fold in &folds {
        let mut columns = Vec::with_capacity(2);
        let mut scores = [None, None];
        for (grid, hold) in [C1MaxHold::Hours168, C1MaxHold::Hours336]
            .into_iter()
            .enumerate()
        {
            let id = identity(
                &admitted,
                request,
                &admitted.inputs.roles[grid],
                Some(fold.index),
            );
            let unit = run_unit(
                &source,
                &log,
                id,
                Scenario::Headline,
                vec![PolicyWindow {
                    start: fold.train.start.micros(),
                    end: fold.train.end.micros(),
                    hold,
                    enabled: true,
                }],
            )?;
            units.push(unit);
            let (run, metrics) = match units
                .last()
                .ok_or_else(|| C1BatchError("missing completed unit".to_owned()))?
                .complete()
            {
                Ok(value) => value,
                Err(error) => {
                    write_outputs(&admitted, &units, &selected, None, Some(&error.0))?;
                    return Err(error);
                }
            };
            columns.push((
                fold.train.start.micros(),
                run.daily.iter().map(|day| day.return_ticks).collect(),
            ));
            scores[grid] = metrics.annual_sharpe_ticks;
        }
        training.push(columns);
        selected.push(select_hold(scores));
    }
    for role in &admitted.inputs.roles[2..] {
        let (actual, _, _) = scenario(role.role);
        let policies = folds
            .iter()
            .zip(&selected)
            .map(|(fold, chosen)| PolicyWindow {
                start: fold.test.start.micros(),
                end: fold.test.end.micros(),
                hold: chosen.unwrap_or(C1MaxHold::Hours168),
                enabled: chosen.is_some(),
            })
            .collect();
        let unit = run_unit(
            &source,
            &log,
            identity(&admitted, request, role, None),
            actual,
            policies,
        )?;
        units.push(unit);
        if let Err(error) = units
            .last()
            .ok_or_else(|| C1BatchError("missing policy unit".to_owned()))?
            .complete()
        {
            write_outputs(&admitted, &units, &selected, None, Some(&error.0))?;
            return Err(error);
        }
    }
    let headline = units[30].complete()?.0;
    let higher = units[31].complete()?.0;
    let statistics = match selection_statistics(&training, Ok(headline), Ok(higher), 202) {
        Ok(value) => value,
        Err(error) => {
            let message = error.to_string();
            write_outputs(&admitted, &units, &selected, None, Some(&message))?;
            return Err(C1BatchError(message));
        }
    };
    write_outputs(&admitted, &units, &selected, Some(&statistics), None)
}

fn select_hold(scores: [Option<i64>; 2]) -> Option<C1MaxHold> {
    let mut selected = None;
    for (score, hold) in scores
        .into_iter()
        .zip([C1MaxHold::Hours168, C1MaxHold::Hours336])
    {
        if let Some(score) = score.filter(|value| *value > 0) {
            if selected.is_none_or(|(previous, _)| score > previous) {
                selected = Some((score, hold));
            }
        }
    }
    selected.map(|(_, hold)| hold)
}

fn create_output(path: &Path, bytes: &[u8]) -> Result<(), C1BatchError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| C1BatchError(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| C1BatchError(error.to_string()))
}

fn write_outputs(
    admitted: &AdmittedC1,
    units: &[Unit],
    selected: &[Option<C1MaxHold>],
    statistics: Option<&SelectionStatistics>,
    fatal: Option<&str>,
) -> Result<(), C1BatchError> {
    let common = json!({"study_id":admitted.inputs.study_id.0,"attempt":4_u64,"pin":admitted.pin,"prereg":admitted.prereg_identity});
    let diagnostics: Vec<_> = units.iter().map(Unit::diagnostic).collect();
    let policy_windows = policy_windows(units)?;
    let report_schedule = schedule().map_err(|error| C1BatchError(error.to_string()))?;
    let failed_role = units.iter().rev().find_map(|unit| {
        unit.complete()
            .is_err()
            .then_some(unit.identity.role.semantic_role.as_str())
    });
    let recorded_starts = crate::trial_log::recorded_run_count(&admitted.ledger);
    let summary = json!({"schema":"parangsae-c1-summary-v1","identity":common,
        "status":if fatal.is_some(){"INCONCLUSIVE_TECHNICAL"}else{"COMPLETED_DECLARED_MODEL"},
        "completion":if fatal.is_some(){"INCOMPLETE"}else{"COMPLETE"},"failed_role":failed_role,
        "error":fatal,"actual_started_units":recorded_starts.as_ref().ok(),
        "ledger_count_error":recorded_starts.as_ref().err().map(|error|error.to_string()),
        "represented_unit_outcomes":units.len(),"declared_unit_cap":33,"declared_charge_cap":5,
        "statistics":statistics,"policy_fold_windows":policy_windows,
        "selection":selected.iter().map(|value|value.map(C1MaxHold::hours)).collect::<Vec<_>>(),
        "block_diagnostics_status":"DEFERRED_NOT_COMPUTED","trading_readiness_claim":false});
    let raw = json!({"schema":"parangsae-c1-diagnostics-v1","identity":common,"units":diagnostics});
    create_output(
        &admitted.out.join("C1-diagnostics.json"),
        &serde_json::to_vec_pretty(&raw).map_err(|error| C1BatchError(error.to_string()))?,
    )?;
    let mut table=String::from("study_id,run_id,config_id,trial_id,role_kind,semantic_role,scenario,fold,start_us,end_exclusive_us,calendar_days,net_return_ticks,annual_sharpe_ticks,daily_close_mdd_ticks,normal_round_trips,status,attempt,unit_kind,prereg_sha256,pinned_root\n");
    let mut daily=String::from("study_id,run_id,config_id,trial_id,role_kind,semantic_role,scenario,day_start_us,end_exclusive_us,known_at_us,equity_ticks,return_ticks,cash_only,report_fold,attempt,unit_kind,prereg_sha256,pinned_root\n");
    let mut run_log = String::new();
    for unit in units {
        let id = &unit.identity;
        let kind = match id.role.role_kind {
            TrialRoleKind::TrainingCandidate => "TrainingCandidate",
            TrialRoleKind::ContinuousPolicy => "ContinuousPolicy",
        };
        let (days, ret, sr, dd, pairs, status) = match &unit.outcome {
            Outcome::Complete { metrics, .. }
                if matches!(unit.persistence, Persistence::Finished) =>
            {
                (
                    metrics.calendar_days.to_string(),
                    metrics.total_return_ticks.to_string(),
                    metrics
                        .annual_sharpe_ticks
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    metrics.daily_close_max_drawdown_ticks.to_string(),
                    metrics.normal_round_trips.to_string(),
                    "COMPLETE",
                )
            }
            Outcome::Technical { .. } | Outcome::NotExecuted | Outcome::Complete { .. } => (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                "TECHNICAL_UNRESOLVED",
            ),
        };
        table.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            id.study_id,
            id.run_id,
            id.config_id,
            id.trial_id,
            kind,
            id.role.semantic_role,
            id.scenario,
            id.fold.map(|v| v.to_string()).unwrap_or_default(),
            unit.range.start.micros(),
            unit.range.end.micros(),
            days,
            ret,
            sr,
            dd,
            pairs,
            status,
            id.attempt,
            id.unit_kind,
            id.prereg_sha256,
            id.pinned_root
        ));
        if let Ok((run, _)) = unit.complete() {
            for row in &run.daily {
                let report_fold = match id.fold {
                    Some(fold) => fold,
                    None => report_schedule
                        .iter()
                        .find(|fold| {
                            row.day_start >= fold.test.start.micros()
                                && row.day_start < fold.test.end.micros()
                        })
                        .map(|fold| fold.index)
                        .ok_or_else(|| {
                            C1BatchError("daily policy row has no owning fold".to_owned())
                        })?,
                };
                daily.push_str(&format!(
                    "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                    id.study_id,
                    id.run_id,
                    id.config_id,
                    id.trial_id,
                    kind,
                    id.role.semantic_role,
                    id.scenario,
                    row.day_start,
                    row.economic_end_exclusive,
                    row.known_at,
                    row.equity,
                    row.return_ticks,
                    row.cash_only,
                    report_fold,
                    id.attempt,
                    id.unit_kind,
                    id.prereg_sha256,
                    id.pinned_root
                ));
            }
        }
        run_log.push_str(&format!(
            "{} {} {} {}\n",
            id.run_id, id.role.semantic_role, id.scenario, status
        ));
    }
    if let Some(error) = fatal {
        run_log.push_str(&format!("TECHNICAL_UNRESOLVED {error}\n"));
    }
    let mut matrix = String::from("study_id,run_id,trial_id,column_config_id,role_kind,semantic_role,scenario,date_us,source_train_fold,return_ticks,attempt,prereg_sha256,pinned_root\n");
    if let Some(stats) = statistics {
        for (row, at) in stats.is_dates_us.iter().enumerate() {
            for (column, values) in stats.is_return_columns.iter().enumerate() {
                let source_fold = u32::try_from(stats.is_source_folds[row])
                    .map_err(|_| C1BatchError("IS source fold overflow".to_owned()))?;
                let source_unit = units
                    .iter()
                    .find(|unit| {
                        unit.identity.fold == Some(source_fold)
                            && unit.identity.config_id == admitted.inputs.roles[column].config_id.0
                    })
                    .ok_or_else(|| C1BatchError("IS source unit is absent".to_owned()))?;
                let id = &source_unit.identity;
                matrix.push_str(&format!(
                    "{},{},{},{},TrainingCandidate,{},{},{},{},{},{},{},{}\n",
                    id.study_id,
                    id.run_id,
                    id.trial_id,
                    id.config_id,
                    id.role.semantic_role,
                    id.scenario,
                    at,
                    source_fold,
                    values[row],
                    id.attempt,
                    id.prereg_sha256,
                    id.pinned_root
                ));
            }
        }
    }
    for (name, value) in [
        ("table_C1.csv", table),
        ("fold_daily_returns.csv", daily),
        ("cscv_is_C1.csv", matrix),
        ("run_log.txt", run_log),
    ] {
        create_output(&admitted.out.join(name), value.as_bytes())?;
    }
    create_output(
        &admitted.out.join("C1.json"),
        &serde_json::to_vec_pretty(&summary).map_err(|error| C1BatchError(error.to_string()))?,
    )?;
    File::open(&admitted.out)
        .and_then(|file| file.sync_all())
        .map_err(|error| C1BatchError(error.to_string()))
}

fn policy_windows(units: &[Unit]) -> Result<Vec<Value>, C1BatchError> {
    use parangsae_sim::Cash;
    let folds = schedule().map_err(|error| C1BatchError(error.to_string()))?;
    let mut rows = Vec::new();
    for unit in units
        .iter()
        .filter(|unit| unit.identity.role.role_kind == TrialRoleKind::ContinuousPolicy)
    {
        let Ok((run, _)) = unit.complete() else {
            continue;
        };
        for fold in &folds {
            let from = usize::try_from(
                (fold.test.start.micros() - unit.range.start.micros()) / 86_400_000_000_i64,
            )
            .map_err(|_| {
                C1BatchError("policy report window starts before its continuous run".to_owned())
            })?;
            let count = usize::try_from(
                (fold.test.end.micros() - fold.test.start.micros()) / 86_400_000_000_i64,
            )
            .map_err(|_| C1BatchError("policy report window length".to_owned()))?;
            let end = from
                .checked_add(count)
                .ok_or_else(|| C1BatchError("policy report window overflow".to_owned()))?;
            let days = run
                .daily
                .get(from..end)
                .ok_or_else(|| C1BatchError("policy report window is incomplete".to_owned()))?;
            let opening = if from == 0 {
                100_000_000_000_000_i64
            } else {
                run.daily[from - 1].equity
            };
            let final_equity = days
                .last()
                .ok_or_else(|| C1BatchError("empty policy report window".to_owned()))?
                .equity;
            let returns: Vec<_> = days.iter().map(|day| day.return_ticks).collect();
            let annual = crate::metrics::sharpe_of_return_ticks(&returns)
                .map_err(|error| C1BatchError(error.to_string()))?
                .1;
            let window_return = crate::metrics::total_return(Cash(opening), Cash(final_equity))
                .map_err(|error| C1BatchError(error.to_string()))?;
            let mut equity = Vec::with_capacity(count + 1);
            equity.push(Cash(opening));
            equity.extend(days.iter().map(|day| Cash(day.equity)));
            let dd = crate::metrics::max_drawdown(&equity)
                .map_err(|error| C1BatchError(error.to_string()))?;
            rows.push(json!({"identity":unit.identity,"report_fold":fold.index,"is_new_run":false,
                "start_us":fold.test.start.micros(),"end_exclusive_us":fold.test.end.micros(),
                "calendar_days":count,"opening_equity_ticks":opening,"final_equity_ticks":final_equity,
                "cash_delta_ticks":final_equity.checked_sub(opening).ok_or_else(||C1BatchError("window cash overflow".to_owned()))?,
                "window_return_ticks":window_return.ticks(),"annual_sharpe_ticks":annual.map(|value|value.ticks()),
                "daily_close_max_drawdown_ticks":dd.ticks(),"account_reset":false}));
        }
    }
    Ok(rows)
}
use crate::{fold_schedule, Fold, ScheduleSpec, UtcDate};

pub(super) fn schedule() -> Result<Vec<Fold>, C1AdmissionError> {
    let date = |text| UtcDate::parse(text).ok_or_else(|| refusal("native calendar date"));
    fold_schedule(&ScheduleSpec {
        sample_start: date("2020-12-18")?,
        holdout_start: date("2025-10-01")?,
        warmup_days: 3,
        train_days: 365,
        purge_days: 1,
        test_days: 90,
        step_days: 90,
        remainder_own_fold_days: 45,
    })
    .map_err(|error| refusal(&format!("native calendar: {error}")))
}

fn refusal(path: &str) -> C1AdmissionError {
    C1AdmissionError {
        code: "RUNTIME_SETTINGS",
        detail: format!("selected native contract differs at {path}"),
    }
}

pub(super) fn validate_runtime_settings(actual: &Value) -> Result<(), C1AdmissionError> {
    // This binds the selected native profile; parsing this data grants no run permission.
    // Source profile names/calendars have one separate Data-owned validator.
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/c1/runtime-settings.json"
    ))
    .map_err(|_| refusal("compiled native profile"))?;
    let expected_fields = expected
        .as_object()
        .ok_or_else(|| refusal("native profile"))?;
    let actual_fields = actual
        .as_object()
        .ok_or_else(|| refusal("settings object"))?;
    if expected_fields.len() != actual_fields.len()
        || actual_fields
            .keys()
            .any(|key| !expected_fields.contains_key(key))
    {
        return Err(refusal("settings fields"));
    }
    let source = actual.get("sources").ok_or_else(|| refusal("sources"))?;
    crate::c1_source::validate_source_settings(source).map_err(|error| C1AdmissionError {
        code: "SOURCE_PROFILE",
        detail: format!("{error:?}"),
    })?;
    for (section, value) in expected_fields {
        if section != "sources" && actual_fields.get(section) != Some(value) {
            return Err(refusal(section));
        }
    }
    let declared = actual
        .pointer("/folds/schedule")
        .and_then(Value::as_array)
        .ok_or_else(|| refusal("folds.schedule"))?;
    let native = schedule()?;
    if declared.len() != native.len() {
        return Err(refusal("folds.schedule count"));
    }
    for (row, fold) in declared.iter().zip(native) {
        let expected = json!({
            "fold": fold.index,
            "test_days": (fold.test.end.micros() - fold.test.start.micros()) / 86_400_000_000_i64,
            "train_start_us": fold.train.start.micros(),
            "train_end_exclusive_us": fold.train.end.micros(),
            "test_start_us": fold.test.start.micros(),
            "test_end_exclusive_us": fold.test.end.micros(),
            "train_terminal_intent_us": fold.train.end.micros() - 600_000_000_i64,
            "test_terminal_intent_us": fold.test.end.micros() - 600_000_000_i64,
        });
        for (key, value) in expected.as_object().ok_or_else(|| refusal("native fold"))? {
            if row.get(key) != Some(value) {
                return Err(refusal(&format!("folds.schedule.{key}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_cash_unit(path: &Path) -> Unit {
        let source = C1SourceView::synthetic(Vec::new(), Vec::new(), Vec::new()).unwrap();
        let start = UtcDate::parse("2024-01-04").unwrap().start_us().micros();
        run_unit(
            &source,
            &TrialLog::new(path),
            Identity {
                study_id: "synthetic-study".to_owned(),
                attempt: 4,
                config_id: "synthetic-config".to_owned(),
                trial_id: "synthetic-trial".to_owned(),
                run_id: "synthetic-unit".to_owned(),
                role: TrialRoleMetadata {
                    role_kind: TrialRoleKind::ContinuousPolicy,
                    semantic_role: "OOS_HEADLINE".to_owned(),
                    scenario: "HEADLINE".to_owned(),
                },
                scenario: "HEADLINE",
                unit_kind: "continuous_oos",
                fold: None,
                prereg_sha256: "synthetic-prereg".to_owned(),
                pinned_root: "synthetic-pin".to_owned(),
            },
            Scenario::Headline,
            vec![PolicyWindow {
                start,
                end: start + 86_400_000_000_i64,
                hold: C1MaxHold::Hours168,
                enabled: false,
            }],
        )
        .unwrap()
    }

    #[test]
    fn undefined_or_nonpositive_selection_is_flat_and_ties_keep_the_shorter_horizon() {
        assert_eq!(select_hold([None, None]), None);
        assert_eq!(select_hold([Some(0), Some(-1)]), None);
        assert_eq!(select_hold([Some(1), Some(1)]), Some(C1MaxHold::Hours168));
        assert_eq!(select_hold([Some(1), Some(2)]), Some(C1MaxHold::Hours336));
    }

    #[test]
    fn thirty_three_unit_keys_preserve_only_five_distinct_charged_configurations() {
        use std::collections::BTreeSet;
        let mut units = BTreeSet::new();
        let mut trials = BTreeSet::new();
        for config in ["train168", "train336"] {
            for fold in 1..=15 {
                assert!(units.insert(run_unit_id("synthetic-study", config, 4, Some(fold))));
                trials.insert(trial_id(config, "synthetic-pin", 4));
            }
        }
        for config in ["headline-policy", "higher-fee-policy", "stress-policy"] {
            assert!(units.insert(run_unit_id("synthetic-study", config, 4, None)));
            trials.insert(trial_id(config, "synthetic-pin", 4));
        }
        assert_eq!(units.len(), 33);
        assert_eq!(trials.len(), 5);
        assert_ne!(
            run_unit_id("a", "c", 4, None),
            run_unit_id("b", "c", 4, None)
        );
    }

    #[test]
    fn a_real_synthetic_cash_unit_writes_one_start_and_one_role_bound_finish() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trials.jsonl");
        let unit = isolated_cash_unit(&path);
        let (run, metrics) = unit.complete().unwrap();
        assert_eq!(run.daily.len(), 1);
        assert_eq!(metrics.final_equity_ticks, 100_000_000_000_000);
        assert_eq!(metrics.annual_sharpe_ticks, None);
        let rows: Vec<Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["kind"], "RunStarted");
        assert_eq!(rows[1]["kind"], "RunFinished");
        assert_eq!(rows[0]["role"], rows[1]["role"]);
        assert_eq!(crate::distinct_trial_count(&path).unwrap(), 1);
        assert!(run.pairs.is_empty() && run.snapshot.is_none());
    }

    fn start_io_fault_evidence() -> Value {
        let directory = tempfile::tempdir().unwrap();
        let ledger = directory.path().join("trials.jsonl");
        let first = isolated_cash_unit(&ledger);
        let original = std::fs::read(&ledger).unwrap();
        let retained = directory.path().join("retained-ledger-before-fault.jsonl");
        // Test-only filesystem obstruction. Retain every original logical fact.
        std::fs::rename(&ledger, &retained).unwrap();
        std::fs::create_dir(&ledger).unwrap();
        let source = C1SourceView::synthetic(Vec::new(), Vec::new(), Vec::new()).unwrap();
        let mut next = first.identity.clone();
        next.run_id.push_str("-next");
        let second = run_unit(
            &source,
            &TrialLog::new(&ledger),
            next,
            Scenario::Headline,
            vec![PolicyWindow {
                start: first.range.start.micros(),
                end: first.range.end.micros(),
                hold: C1MaxHold::Hours168,
                enabled: false,
            }],
        )
        .expect("ledger IO failure must remain an owned reportable unit");
        assert!(second.complete().is_err());
        let records = json!([first.diagnostic(), second.diagnostic()]);
        let output = directory.path().join("retained-unit-evidence.json");
        create_output(&output, &serde_json::to_vec(&records).unwrap()).unwrap();
        assert_eq!(std::fs::read(retained).unwrap(), original);
        assert!(crate::trial_log::recorded_run_count(&ledger).is_err());
        serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap()
    }

    #[test]
    fn ledger_start_io_failure_keeps_prior_unit_evidence_without_executing_the_next_unit() {
        let records = start_io_fault_evidence();
        assert_eq!(records[0]["status"], "COMPLETE");
        assert_eq!(
            records[0]["metrics"]["final_equity_ticks"],
            100_000_000_000_000_i64
        );
        assert_eq!(records[1]["status"], "INCOMPLETE");
        assert_eq!(records[1]["execution_status"], "NOT_EXECUTED");
        assert_eq!(records[1]["ledger_persistence"]["state"], "START_UNCERTAIN");
        assert!(records[1]["raw"].is_null());
    }

    #[test]
    fn a_failed_terminal_append_retains_the_actual_completed_execution_and_metrics() {
        let directory = tempfile::tempdir().unwrap();
        let ledger = directory.path().join("trials.jsonl");
        let first = isolated_cash_unit(&ledger);
        let log = TrialLog::new(&ledger);
        let mut next = first.identity.clone();
        next.run_id.push_str("-terminal-fault");
        log.begin(StartRecord {
            run_id: &next.run_id,
            trial_id: &next.trial_id,
            config_id: &next.config_id,
            pinned_root: &next.pinned_root,
            prereg_sha256: &next.prereg_sha256,
            attempt: next.attempt,
            family: "C1",
            ts: first.range.start.micros(),
            fold: None,
            segment: Some(next.unit_kind),
            role: Some(&next.role),
        })
        .unwrap();
        let source = C1SourceView::synthetic(Vec::new(), Vec::new(), Vec::new()).unwrap();
        let execution = execute_unit(
            &source,
            Scenario::Headline,
            vec![PolicyWindow {
                start: first.range.start.micros(),
                end: first.range.end.micros(),
                hold: C1MaxHold::Hours168,
                enabled: false,
            }],
            first.range,
        );
        let original = std::fs::read(&ledger).unwrap();
        let retained = directory
            .path()
            .join("retained-ledger-before-terminal-fault.jsonl");
        std::fs::rename(&ledger, &retained).unwrap();
        std::fs::create_dir(&ledger).unwrap();
        let second = persist_execution(&log, next, first.range, execution);
        assert!(second.complete().is_err());
        let records = json!([first.diagnostic(), second.diagnostic()]);
        let output = directory.path().join("retained-unit-evidence.json");
        create_output(&output, &serde_json::to_vec(&records).unwrap()).unwrap();
        let recorded: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        assert_eq!(recorded[0]["status"], "COMPLETE");
        assert_eq!(recorded[1]["status"], "INCOMPLETE");
        assert_eq!(recorded[1]["execution_status"], "COMPLETE");
        assert_eq!(
            recorded[1]["ledger_persistence"]["state"],
            "TERMINAL_UNCERTAIN"
        );
        assert_eq!(
            recorded[1]["metrics"]["final_equity_ticks"],
            100_000_000_000_000_i64
        );
        assert_eq!(
            recorded[1]["raw"]["daily_capital"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(std::fs::read(retained).unwrap(), original);
        assert_eq!(std::str::from_utf8(&original).unwrap().lines().count(), 3);
    }

    #[derive(Debug, Default, cucumber::World)]
    struct AssemblyWorld {
        directory: Option<tempfile::TempDir>,
        unit: Option<Unit>,
        io_evidence: Option<Value>,
    }

    #[cucumber::when("a real isolated cash only C1 unit is assembled with an explicit flat policy")]
    fn assemble_cash(world: &mut AssemblyWorld) {
        let directory = tempfile::tempdir().unwrap();
        world.unit = Some(isolated_cash_unit(&directory.path().join("trials.jsonl")));
        world.directory = Some(directory);
    }

    #[cucumber::then(
        "its full capital day is retained and the shared ledger has one matched role bound finish"
    )]
    fn assembled_cash(world: &mut AssemblyWorld) {
        let unit = world.unit.as_ref().unwrap();
        let (run, metrics) = unit.complete().unwrap();
        assert_eq!(run.daily.len(), 1);
        assert_eq!(run.daily[0].return_ticks, 0);
        assert_eq!(metrics.final_equity_ticks, 100_000_000_000_000);
        assert!(metrics.annual_sharpe_ticks.is_none());
        let path = world
            .directory
            .as_ref()
            .unwrap()
            .path()
            .join("trials.jsonl");
        let rows: Vec<Value> = std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["kind"], "RunFinished");
        assert_eq!(rows[0]["role"], rows[1]["role"]);
    }

    #[cucumber::when("ledger append becomes unavailable after one completed synthetic unit")]
    fn obstruct_ledger(world: &mut AssemblyWorld) {
        world.io_evidence = Some(start_io_fault_evidence());
    }

    #[cucumber::then(
        "prior full unit evidence is written and the next unit is explicitly not executed"
    )]
    fn retained_ledger_error(world: &mut AssemblyWorld) {
        let records = world.io_evidence.as_ref().unwrap();
        assert_eq!(
            records[0]["metrics"]["final_equity_ticks"],
            100_000_000_000_000_i64
        );
        assert_eq!(
            records[0]["raw"]["daily_capital"].as_array().unwrap().len(),
            1
        );
        assert_eq!(records[1]["execution_status"], "NOT_EXECUTED");
        assert_eq!(records[1]["ledger_persistence"]["state"], "START_UNCERTAIN");
    }

    #[tokio::test]
    async fn c1_assembly_feature() {
        use cucumber::World as _;
        AssemblyWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/features/c1_assembly.feature"
            ))
            .await;
    }

    #[test]
    fn fixed_mark_registry_keys_match_the_declared_utc_day() {
        let value: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/c1/runtime-settings.json"
        ))
        .unwrap();
        crate::c1_source::validate_source_settings(&value["sources"]).unwrap();
        for key in value["sources"]["perp_mark_gap_keys_us"]
            .as_array()
            .unwrap()
        {
            assert_eq!(
                UtcDate::from_micros(UtcMicros::from_micros(key.as_i64().unwrap())).to_string(),
                "2024-08-12"
            );
        }
    }

    #[test]
    fn actual_selected_settings_and_calendar_match_the_native_binding() {
        let value: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/c1/runtime-settings.json"
        ))
        .unwrap();
        validate_runtime_settings(&value).unwrap();
        let folds = schedule().unwrap();
        assert_eq!(folds.len(), 15);
        assert_eq!(
            folds[0].train.start,
            UtcDate::parse("2020-12-21").unwrap().start_us()
        );
        assert_eq!(
            folds[0].test.start,
            UtcDate::parse("2021-12-22").unwrap().start_us()
        );
        assert_eq!(
            (folds.last().unwrap().test.end.micros() - folds[0].test.start.micros())
                / 86_400_000_000_i64,
            1379
        );
    }

    #[test]
    fn self_consistent_hashes_cannot_change_runtime_capital_costs_or_calendar() {
        let original: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/c1/runtime-settings.json"
        ))
        .unwrap();
        for path in [
            "/capital/initial_total_cash_ticks",
            "/risk/session_loss_bps_of_original_C0",
            "/cost_scenarios/HEADLINE/perp_fee_bps",
            "/clock/entry_decision_offset_us",
            "/statistics/CSCV_gap_call_literal",
            "/folds/schedule/0/train_end_exclusive_us",
        ] {
            let mut value = original.clone();
            *value.pointer_mut(path).unwrap() = json!(1);
            assert_eq!(
                validate_runtime_settings(&value).unwrap_err().code,
                "RUNTIME_SETTINGS"
            );
        }
    }
}
