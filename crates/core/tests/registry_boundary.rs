//! The production registry cannot be selected from outside this crate.

use std::fs;

#[test]
fn core_has_no_features_and_the_classifier_has_no_override() {
    let root = env!("CARGO_MANIFEST_DIR");
    let manifest = fs::read_to_string(format!("{root}/Cargo.toml")).expect("manifest");
    assert!(
        !manifest.contains("[features]"),
        "parangsae-core must not declare features"
    );
    let source = fs::read_to_string(format!("{root}/src/version_status.rs")).expect("source");
    let banned = [
        "env::var",
        "env!",
        "option_env!",
        "static mut",
        "Cell",
        "RefCell",
        "OnceLock",
        "Mutex",
        "RwLock",
    ];
    for token in banned {
        assert!(
            !source.contains(token),
            "version_status.rs contains {token}"
        );
    }
    assert!(
        !source.contains("non_exhaustive"),
        "RuleVersion must stay exhaustive"
    );
}
