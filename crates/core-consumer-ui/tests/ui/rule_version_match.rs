use parangsae_core::version_status::RuleVersion;

fn name(rule: RuleVersion) -> &'static str {
    match rule {
        RuleVersion::MetricsSpecV2_1 => "MetricsSpecV2_1",
        RuleVersion::PayloadV2 => "PayloadV2",
        RuleVersion::MetricMissingRulesV1 => "MetricMissingRulesV1",
    }
}

fn main() {
    let _ = name(RuleVersion::MetricsSpecV2_1);
    let _ = name(RuleVersion::PayloadV2);
    let _ = name(RuleVersion::MetricMissingRulesV1);
}
