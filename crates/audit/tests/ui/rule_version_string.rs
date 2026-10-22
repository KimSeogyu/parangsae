use parangsae_collector::version::{LoadError, VersionRoot};

fn bad() -> LoadError {
    LoadError::SupersededVersion {
        dataset_id: String::new(),
        root: VersionRoot::from_bytes([0; 32]),
        successor_root: None,
        rule: "metrics_spec_v2.1",
    }
}

fn main() {
    let _ = bad;
}
