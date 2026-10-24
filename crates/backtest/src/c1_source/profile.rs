//! Fixed C1 archived source profile. Metadata validation does not admit market facts.

pub(super) const SOURCE_POLICY_ID: &str = "C1_ARCHIVED_SPARSE_KEY_CERTIFIED_V2";
pub(super) const REBUILT: [&str; 6] = [
    "2020-12-21",
    "2021-02-11",
    "2021-04-25",
    "2021-08-13",
    "2021-12-24",
    "2023-03-24",
];
pub(super) const STATUS_DAYS: [&str; 10] = [
    "2020-12-21",
    "2020-12-25",
    "2021-02-11",
    "2021-03-06",
    "2021-04-20",
    "2021-04-25",
    "2021-08-13",
    "2021-09-29",
    "2021-12-24",
    "2023-03-24",
];
pub(super) const CERTIFIED_NORMAL_GAPS: [(&str, i64, i64); 4] = [
    ("2020-12-25", 1_608_861_600_000_000, 60),
    ("2021-03-06", 1_614_996_000_000_000, 90),
    ("2021-04-20", 1_618_884_000_000_000, 150),
    ("2021-09-29", 1_632_898_800_000_000, 120),
];
pub(super) const MARK_GAP_DAY: &str = "2024-08-12";
pub(super) const MARK_GAP_KEYS: [i64; 2] = [1_723_456_920_000_000, 1_723_456_980_000_000];
pub(super) const FUNDING_FLOOR_US: i64 = 60_000_000;
pub(super) const BAR_INTERVAL_US: i64 = 60_000_000;
pub(super) const FUNDING_CADENCE_US: i64 = 28_800_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceProfileError {
    UnsupportedPolicy,
    DailyRebuiltRegistry,
    SpotStatusRegistry,
    SourceSettings,
}

/// Fixed metadata projection, not caller-selectable source behavior.
fn canonical_source_settings() -> serde_json::Value {
    let normal_days: Vec<_> = CERTIFIED_NORMAL_GAPS
        .iter()
        .map(|(day, _, _)| *day)
        .collect();
    let source_days = 1_748_usize;
    serde_json::json!({
        "spot_precedence":"NORMAL1742_STORED_BYTES_UNCHANGED_SIX_DAILY_ONLY_REBUILDS_TEN_EXACT_SPOT_STATUS_DAYS_MONTHLY_AUDIT_ONLY",
        "missing_statuses":["PENDING","GAP","QUARANTINED","ZERO_COUNT"],
        "funding_cadence_us":FUNDING_CADENCE_US,
        "funding_effective_available_floor_us":FUNDING_FLOOR_US,
        "funding_notional":"UM_MARK_1M_OPEN_AT_SETTLEMENT_KEY_PROXY",
        "risk_price":"UM_MARK_1M_CLOSE_ORIGINAL_AVAILABLE",
        "spot_value":"SPOT_1M_CLOSE_ORIGINAL_AVAILABLE",
        "entry_count":"VALID_POSITIVE_COUNT_BOTH_ALIGNED_TRADE_ROWS",
        "new_entry_on_gap":false,
        "owned_required_missing_value":"WHOLE_TRIAL_TECHNICAL_UNRESOLVED",
        "flat_no_due_action_missing":"SUPPRESS_NEW_EXPOSURE_KEEP_CALENDAR",
        "source_policy_id":SOURCE_POLICY_ID,
        "daily_rebuilt_days":REBUILT,
        "spot_status_days":STATUS_DAYS,
        "spot_normal_gap_days":normal_days,
        "spot_normal_gap_key_policy":"EXACT_ROOT_SELECTED_MACHINE_KEYSETS_DATA_OWNED_PROFILE_NOT_GENERIC_MISSING_TO_GAP",
        "um_allowed_gap_keys_us":[],
        "um_allowed_quarantined_keys_us":[],
        "funding_allowed_gap_keys_us":[],
        "funding_allowed_quarantined_keys_us":[],
        "perp_mark_gap_keys_us":MARK_GAP_KEYS,
        "perp_mark_gap_notice_us":MARK_GAP_KEYS.map(|key|key+BAR_INTERVAL_US),
        "perp_mark_allowed_quarantined_keys_us":[],
        "perp_mark_gap_carry":false,
        "financial_source_profile_id":"C1_FUNDING_FLOOR60_SUPPORTED_V1",
        "global_funding_and_mark_proxy_support_offset_us":FUNDING_FLOOR_US,
        "global_funding_support_independent_of_position":true,
        "exact_captured_funding_receipt_attempt_offset_us":FUNDING_FLOOR_US,
        "funding_or_proxy_unavailable_at_exact_floor":"WHOLE_TRIAL_TECHNICAL_NO_TIMESTAMP_CLAMP_OR_LATE_RESCUE",
        "source_catalog_logical_path":"c1-source-manifest.json",
        "catalog_is_additional_pin_metadata_not_DATA":true,
        "planned_DATA_object_count":source_days*4+STATUS_DAYS.len()+1,
        "planned_DATA_composition":{
            "common_core":source_days*3+1,
            "normal_spot":source_days-REBUILT.len(),
            "rebuilt_spot":REBUILT.len(),
            "spot_status":STATUS_DAYS.len()
        },
        "planned_count_is_financial_admission":false
    })
}

/// Rejects drift, omitted fields and unknown fields before any data IO.
pub(crate) fn validate_source_settings(
    settings: &serde_json::Value,
) -> Result<(), SourceProfileError> {
    if *settings != canonical_source_settings() {
        return Err(SourceProfileError::SourceSettings);
    }
    Ok(())
}

/// Checks the fixed profile before data IO. The named profile also fixes Mark2,
/// no mark carry and the global S+60s funding-source requirement.
pub(crate) fn validate_source_profile(
    source_policy_id: &str,
    daily_rebuilt_days: &[String],
    spot_status_days: &[String],
) -> Result<(), SourceProfileError> {
    if source_policy_id != SOURCE_POLICY_ID {
        return Err(SourceProfileError::UnsupportedPolicy);
    }
    if !daily_rebuilt_days.iter().map(String::as_str).eq(REBUILT) {
        return Err(SourceProfileError::DailyRebuiltRegistry);
    }
    if !spot_status_days.iter().map(String::as_str).eq(STATUS_DAYS) {
        return Err(SourceProfileError::SpotStatusRegistry);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_named_profile_is_accepted_but_caller_replacement_is_not() {
        let rebuilt = REBUILT.map(str::to_owned);
        let statuses = STATUS_DAYS.map(str::to_owned);
        assert_eq!(
            validate_source_profile(SOURCE_POLICY_ID, &rebuilt, &statuses),
            Ok(())
        );
        assert_eq!(
            validate_source_profile("UNSUPPORTED_CALLER_POLICY", &rebuilt, &statuses),
            Err(SourceProfileError::UnsupportedPolicy)
        );
        assert_eq!(
            validate_source_profile(SOURCE_POLICY_ID, &rebuilt[..5], &statuses),
            Err(SourceProfileError::DailyRebuiltRegistry)
        );
        assert_eq!(
            validate_source_profile(SOURCE_POLICY_ID, &rebuilt, &statuses[..9]),
            Err(SourceProfileError::SpotStatusRegistry)
        );
    }

    #[test]
    fn reordered_or_repeated_status_registry_is_not_the_canonical_profile() {
        let rebuilt = REBUILT.map(str::to_owned);
        let mut statuses = STATUS_DAYS.map(str::to_owned);
        statuses.swap(0, 1);
        assert_eq!(
            validate_source_profile(SOURCE_POLICY_ID, &rebuilt, &statuses),
            Err(SourceProfileError::SpotStatusRegistry)
        );
        statuses[0] = statuses[1].clone();
        assert_eq!(
            validate_source_profile(SOURCE_POLICY_ID, &rebuilt, &statuses),
            Err(SourceProfileError::SpotStatusRegistry)
        );
    }

    #[test]
    fn complete_source_descriptor_refuses_contradictory_or_unknown_metadata() {
        let expected = canonical_source_settings();
        assert_eq!(validate_source_settings(&expected), Ok(()));
        for (key, value) in [
            ("funding_effective_available_floor_us", serde_json::json!(0)),
            ("perp_mark_gap_carry", serde_json::json!(true)),
            (
                "global_funding_support_independent_of_position",
                serde_json::json!(false),
            ),
            ("unknown_source_field", serde_json::json!(0)),
        ] {
            let mut changed = expected.clone();
            changed[key] = value;
            assert_eq!(
                validate_source_settings(&changed),
                Err(SourceProfileError::SourceSettings)
            );
        }
        let mut missing = expected;
        missing
            .as_object_mut()
            .expect("source object")
            .remove("funding_notional");
        assert_eq!(
            validate_source_settings(&missing),
            Err(SourceProfileError::SourceSettings)
        );
    }
}
