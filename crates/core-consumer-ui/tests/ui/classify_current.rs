use parangsae_core::version_status::{classify, ManifestHead, VersionStatus};

fn main() {
    let head = ManifestHead {
        dataset: "metrics",
        spec: "parangsae/dataset-manifest/v2.2",
        payload_version: Some("payload/v3"),
        metric_missing_rules: Some(
            "metric_missing_rules/v1 merge=across_utc_days long_null_run_slots=12",
        ),
        metrics_only_key: false,
    };
    assert!(matches!(classify(&head), VersionStatus::Current));
}
