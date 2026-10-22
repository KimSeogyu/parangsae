//! Closed payload and rule classification for a dataset manifest.
//!
//! No I/O. [`classify`] always uses the production registry. A test registry
//! exists only in this crate's unit tests.

/// Why a metrics root is superseded. Closed: a new variant breaks every exhaustive match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleVersion {
    MetricsSpecV2_1,
    PayloadV2,
    MetricMissingRulesV1,
}

/// Standing of one registry row. Public so a superseded row is not a dead variant
/// while the production table has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleStanding {
    Current,
    Superseded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowId {
    Rule(RuleVersion),
    #[cfg(test)]
    SyntheticTest,
}

#[derive(Clone, Copy)]
pub(crate) struct RuleRow {
    id: RowId,
    text: &'static str,
    standing: RuleStanding,
}

pub(crate) struct RuleRegistry<'a> {
    rows: &'a [RuleRow],
}

pub(crate) const METRIC_MISSING_RULES: &str =
    "metric_missing_rules/v1 merge=across_utc_days long_null_run_slots=12";

const SPEC_V2_1: &str = "parangsae/dataset-manifest/v2.1";
const SPEC_V2_2: &str = "parangsae/dataset-manifest/v2.2";
const PAYLOAD_V2: &str = "payload/v2";
const PAYLOAD_V3: &str = "payload/v3";

const PRODUCTION: RuleRegistry<'static> = RuleRegistry {
    rows: &[RuleRow {
        id: RowId::Rule(RuleVersion::MetricMissingRulesV1),
        text: METRIC_MISSING_RULES,
        standing: RuleStanding::Current,
    }],
};

/// Manifest fields the step-2 rules read. After structural checks every version
/// string is absent or an exact JSON string.
pub struct ManifestHead<'a> {
    pub dataset: &'a str,
    pub spec: &'a str,
    pub payload_version: Option<&'a str>,
    pub metric_missing_rules: Option<&'a str>,
    pub metrics_only_key: bool,
}

/// Step-2 outcome. Structure faults stay in the collector: they need the file bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionStatus {
    Current,
    Superseded(RuleVersion),
    UnknownPayload(Option<String>),
    UnknownRule {
        field: &'static str,
        found: Option<String>,
    },
    SpecMismatch {
        dataset: String,
        found: String,
    },
}

/// Classify `head` with the production registry.
pub fn classify(head: &ManifestHead<'_>) -> VersionStatus {
    classify_with(head, &PRODUCTION)
}

fn classify_with(head: &ManifestHead<'_>, registry: &RuleRegistry<'_>) -> VersionStatus {
    if head.dataset == "metrics" && head.spec == SPEC_V2_1 {
        return VersionStatus::Superseded(RuleVersion::MetricsSpecV2_1);
    }
    let expected = spec_for(head.dataset);
    if expected != Some(head.spec) || (head.dataset != "metrics" && head.metrics_only_key) {
        return VersionStatus::SpecMismatch {
            dataset: head.dataset.to_owned(),
            found: head.spec.to_owned(),
        };
    }
    if head.dataset != "metrics" {
        return VersionStatus::Current;
    }
    match head.payload_version {
        None => VersionStatus::UnknownPayload(None),
        Some(payload) if payload == PAYLOAD_V2 => VersionStatus::Superseded(RuleVersion::PayloadV2),
        Some(payload) if payload == PAYLOAD_V3 => {
            classify_rule(head.metric_missing_rules, registry)
        }
        Some(payload) => VersionStatus::UnknownPayload(Some(payload.to_owned())),
    }
}

fn classify_rule(found: Option<&str>, registry: &RuleRegistry<'_>) -> VersionStatus {
    let Some(text) = found else {
        return VersionStatus::UnknownRule {
            field: "metric_missing_rules",
            found: None,
        };
    };
    match registry.rows.iter().find(|row| row.text == text) {
        Some(row) if row.standing == RuleStanding::Current => VersionStatus::Current,
        Some(row) if row.standing == RuleStanding::Superseded => match rule_of(row.id) {
            Some(rule) => VersionStatus::Superseded(rule),
            None => VersionStatus::UnknownRule {
                field: "metric_missing_rules",
                found: Some(text.to_owned()),
            },
        },
        _ => VersionStatus::UnknownRule {
            field: "metric_missing_rules",
            found: Some(text.to_owned()),
        },
    }
}

fn rule_of(id: RowId) -> Option<RuleVersion> {
    match id {
        RowId::Rule(rule) => Some(rule),
        #[cfg(test)]
        RowId::SyntheticTest => None,
    }
}

fn spec_for(dataset: &str) -> Option<&'static str> {
    match dataset {
        "um-kline"
        | "mark-price-kline"
        | "index-price-kline"
        | "premium-index-kline"
        | "spot-kline"
        | "funding-rate"
        | "book-depth" => Some(SPEC_V2_1),
        "metrics" => Some(SPEC_V2_2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const SYNTHETIC: &str = "metric_missing_rules/v2 merge=across_utc_days long_null_run_slots=24";

    fn test_registry() -> RuleRegistry<'static> {
        RuleRegistry { rows: &TEST_ROWS }
    }

    static TEST_ROWS: [RuleRow; 2] = [
        RuleRow {
            id: RowId::Rule(RuleVersion::MetricMissingRulesV1),
            text: METRIC_MISSING_RULES,
            standing: RuleStanding::Superseded,
        },
        RuleRow {
            id: RowId::SyntheticTest,
            text: SYNTHETIC,
            standing: RuleStanding::Current,
        },
    ];

    fn head<'a>(
        dataset: &'a str,
        spec: &'a str,
        payload: Option<&'a str>,
        rule: Option<&'a str>,
        metrics_only_key: bool,
    ) -> ManifestHead<'a> {
        ManifestHead {
            dataset,
            spec,
            payload_version: payload,
            metric_missing_rules: rule,
            metrics_only_key,
        }
    }

    fn metrics<'a>(payload: Option<&'a str>, rule: Option<&'a str>) -> ManifestHead<'a> {
        head("metrics", SPEC_V2_2, payload, rule, false)
    }

    #[test]
    fn production_v1_string_is_current() {
        assert_eq!(METRIC_MISSING_RULES.len(), 68);
        let status = classify(&metrics(Some(PAYLOAD_V3), Some(METRIC_MISSING_RULES)));
        assert_eq!(status, VersionStatus::Current);
    }

    #[test]
    fn absent_payload_is_unknown_and_v2_is_superseded() {
        assert_eq!(
            classify(&metrics(None, Some(METRIC_MISSING_RULES))),
            VersionStatus::UnknownPayload(None)
        );
        assert_eq!(
            classify(&metrics(Some(PAYLOAD_V2), Some(METRIC_MISSING_RULES))),
            VersionStatus::Superseded(RuleVersion::PayloadV2)
        );
        assert_eq!(
            classify(&metrics(Some("payload/v4"), Some(METRIC_MISSING_RULES))),
            VersionStatus::UnknownPayload(Some("payload/v4".to_owned()))
        );
    }

    #[test]
    fn metrics_v2_1_is_superseded_before_payload() {
        let status = classify(&head("metrics", SPEC_V2_1, None, None, false));
        assert_eq!(
            status,
            VersionStatus::Superseded(RuleVersion::MetricsSpecV2_1)
        );
    }

    #[test]
    fn unknown_and_absent_rules_fail_closed() {
        assert_eq!(
            classify(&metrics(Some(PAYLOAD_V3), None)),
            VersionStatus::UnknownRule {
                field: "metric_missing_rules",
                found: None,
            }
        );
        let odd = "metric_missing_rules/v1 merge=across_utc_days long_null_run_slots=13";
        assert_eq!(
            classify(&metrics(Some(PAYLOAD_V3), Some(odd))),
            VersionStatus::UnknownRule {
                field: "metric_missing_rules",
                found: Some(odd.to_owned()),
            }
        );
    }

    #[test]
    fn a_metrics_only_key_on_a_v2_1_dataset_mismatches() {
        let status = classify(&head("um-kline", SPEC_V2_1, None, None, true));
        assert_eq!(
            status,
            VersionStatus::SpecMismatch {
                dataset: "um-kline".to_owned(),
                found: SPEC_V2_1.to_owned(),
            }
        );
        assert_eq!(
            classify(&head("um-kline", SPEC_V2_1, None, None, false)),
            VersionStatus::Current
        );
    }

    #[test]
    fn test_registry_marks_v1_superseded_and_the_synthetic_string_current() {
        let registry = test_registry();
        let v1 = metrics(Some(PAYLOAD_V3), Some(METRIC_MISSING_RULES));
        assert_eq!(
            classify_with(&v1, &registry),
            VersionStatus::Superseded(RuleVersion::MetricMissingRulesV1)
        );
        let synthetic = metrics(Some(PAYLOAD_V3), Some(SYNTHETIC));
        assert_eq!(classify_with(&synthetic, &registry), VersionStatus::Current);
        assert!(registry_ok(&registry, false));
        assert!(registry_ok(&PRODUCTION, true));
    }

    #[test]
    fn production_registry_invariants_reject_two_current_rows() {
        let current = RuleRow {
            id: RowId::Rule(RuleVersion::MetricMissingRulesV1),
            text: METRIC_MISSING_RULES,
            standing: RuleStanding::Current,
        };
        let second = RuleRow {
            id: RowId::SyntheticTest,
            text: SYNTHETIC,
            standing: RuleStanding::Current,
        };
        assert!(!registry_ok(
            &RuleRegistry {
                rows: &[current, second],
            },
            true
        ));
        let duplicated = RuleRow {
            id: RowId::Rule(RuleVersion::PayloadV2),
            text: METRIC_MISSING_RULES,
            standing: RuleStanding::Superseded,
        };
        assert!(!registry_ok(
            &RuleRegistry {
                rows: &[current, duplicated],
            },
            true
        ));
        let missing_variant = RuleRow {
            id: RowId::Rule(RuleVersion::PayloadV2),
            text: SYNTHETIC,
            standing: RuleStanding::Current,
        };
        assert!(!registry_ok(
            &RuleRegistry {
                rows: &[missing_variant],
            },
            true
        ));
    }

    fn registry_ok(registry: &RuleRegistry<'_>, per_variant: bool) -> bool {
        if registry
            .rows
            .iter()
            .filter(|row| row.standing == RuleStanding::Current)
            .count()
            != 1
        {
            return false;
        }
        let mut seen = HashSet::new();
        for row in registry.rows {
            if !seen.insert(row.text) {
                return false;
            }
        }
        if !per_variant {
            return true;
        }
        let count = registry
            .rows
            .iter()
            .filter(|row| matches!(row.id, RowId::Rule(RuleVersion::MetricMissingRulesV1)))
            .count();
        count == 1
    }
}
