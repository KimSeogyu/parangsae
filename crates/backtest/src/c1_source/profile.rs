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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceProfileError {
    UnsupportedPolicy,
    DailyRebuiltRegistry,
    SpotStatusRegistry,
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
}
