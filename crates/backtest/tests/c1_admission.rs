#[path = "../src/canon.rs"]
mod canon;
pub use parangsae_backtest::{fold_schedule, Fold, ScheduleSpec, UtcDate};
mod date {
    pub use super::UtcDate;
}
#[path = "../src/c1_source/profile.rs"]
mod source_profile;
mod c1_source {
    pub(crate) use super::source_profile::{validate_source_profile, validate_source_settings};
}
#[path = "../src/c1/admission.rs"]
mod admission;
#[path = "../src/c1/study.rs"]
mod study;

#[test]
fn fixed_mark_registry_keys_match_the_declared_utc_day() {
    for key in source_profile::MARK_GAP_KEYS {
        assert_eq!(
            UtcDate::from_micros(parangsae_core::UtcMicros::from_micros(key)).to_string(),
            source_profile::MARK_GAP_DAY
        );
    }
}
