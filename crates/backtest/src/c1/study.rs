//! Fixed execution/statistics settings are admitted before any DATA hash.

use serde_json::{json, Value};

use super::admission::C1AdmissionError;
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
