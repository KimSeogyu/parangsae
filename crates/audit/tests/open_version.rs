use std::fs;

use parangsae_audit::{open_version, runner_depends_on_audit, AuditError};
use parangsae_collector::version::encode::{
    self, canonical_manifest, merkle_tree_head, ManifestDoc, UM_KLINE_COLUMNS,
};
use parangsae_collector::version::{ManifestDigest, VersionRoot};

const DATASET: &str = "binance-vision.um.metrics.BTCUSDT";

#[test]
fn a_refused_metrics_root_opens_for_audit() {
    let root_bytes = merkle_tree_head(&[]);
    let zeros = [0u8; 32];
    let built = canonical_manifest(&ManifestDoc {
        dataset_id: DATASET,
        columns: UM_KLINE_COLUMNS,
        schema_hash: &zeros,
        version_root: &root_bytes,
        first_date: "2021-06-01",
        last_date: "2021-06-01",
        partitions: &[],
        rows: 0,
    })
    .expect("manifest");
    let mut value: serde_json::Value = serde_json::from_slice(&built).expect("json");
    value.as_object_mut().expect("object").insert(
        "spec".to_owned(),
        serde_json::Value::String("parangsae/dataset-manifest/v2.1".to_owned()),
    );
    let bytes = serde_json::to_vec(&value).expect("bytes");
    let root = VersionRoot::from_bytes(root_bytes);
    let dir = tempfile::tempdir().expect("dir");
    let path = dir
        .path()
        .join("manifest")
        .join("versions")
        .join(DATASET)
        .join(format!("{}.json", root.to_hex()));
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(&path, &bytes).expect("write");
    let digest = ManifestDigest::from_bytes(encode::sha256(&bytes));
    let view = open_version(dir.path(), DATASET, root, digest).expect("audit");
    assert_eq!(view.dataset_id(), DATASET);
    assert_eq!(view.root(), root);
    assert_eq!(view.partitions(), 0);

    let wrong = ManifestDigest::from_bytes([9; 32]);
    match open_version(dir.path(), DATASET, root, wrong) {
        Err(AuditError::DigestMismatch) => {}
        Err(err) => panic!("{err}"),
        Ok(_) => panic!("opened a digest mismatch"),
    }
}

#[test]
fn audit_open_fails_when_the_root_does_not_match() {
    let zeros = [0u8; 32];
    let built = canonical_manifest(&ManifestDoc {
        dataset_id: DATASET,
        columns: UM_KLINE_COLUMNS,
        schema_hash: &zeros,
        version_root: &zeros,
        first_date: "2021-06-01",
        last_date: "2021-06-01",
        partitions: &[],
        rows: 0,
    })
    .expect("manifest");
    let root = VersionRoot::from_bytes(zeros);
    let dir = tempfile::tempdir().expect("dir");
    let path = dir
        .path()
        .join("manifest")
        .join("versions")
        .join(DATASET)
        .join(format!("{}.json", root.to_hex()));
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(&path, &built).expect("write");
    let digest = ManifestDigest::from_bytes(encode::sha256(&built));
    match open_version(dir.path(), DATASET, root, digest) {
        Err(AuditError::RootMismatch) => {}
        Err(err) => panic!("{err}"),
        Ok(_) => panic!("opened a root mismatch"),
    }
}

#[test]
fn a_runner_dependency_on_audit_is_rejected() {
    let metadata = serde_json::json!({
        "packages": [
            {
                "name": "parangsae-runner",
                "dependencies": [{ "name": "parangsae-collector", "kind": null }]
            },
            {
                "name": "parangsae-collector",
                "dependencies": [{ "name": "parangsae-audit", "kind": "dev" }]
            },
            {
                "name": "parangsae-audit",
                "dependencies": []
            }
        ]
    });
    let err = runner_depends_on_audit(&metadata).expect_err("mutant");
    assert!(err.contains("parangsae-runner"), "{err}");
    assert!(err.contains("parangsae-audit"), "{err}");
}

#[test]
fn this_workspace_does_not_let_research_crates_depend_on_audit() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--offline"])
        .current_dir(root)
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).expect("metadata");
    runner_depends_on_audit(&metadata).expect("depgraph");
}
