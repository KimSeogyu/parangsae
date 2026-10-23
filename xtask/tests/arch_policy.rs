//! Positive and negative fixtures for each architecture rule.
//! These tests do not run SANY or TLC. `spec validate` is the CI job.

use std::path::{Path, PathBuf};

use xtask::{enforce, generate, parse_policy, path_diags_for, EnforceOptions};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/arch")
        .join(name)
}

fn enforce_at(root: &Path) -> Vec<xtask::Diagnostic> {
    enforce(&EnforceOptions {
        manifest: &root.join("Cargo.toml"),
        diff: None,
        as_bot: None,
        check_generated: false,
    })
    .unwrap_or_else(|err| panic!("enforce {}: {err}", root.display()))
}

fn ids(diags: &[xtask::Diagnostic]) -> Vec<&str> {
    diags.iter().map(|diag| diag.rule_id).collect()
}

#[test]
fn dep_direct_ok_is_empty() {
    let diags = enforce_at(&fixture("dep_direct_ok"));
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn dep_direct_forbidden_edge() {
    let diags = enforce_at(&fixture("dep_direct_bad"));
    assert_eq!(ids(&diags), ["ARCH-DEP-001"]);
    assert_eq!(diags[0].symbol, "a -> x");
    assert!(diags[0].file.ends_with("a/Cargo.toml"), "{}", diags[0].file);
    assert!(diags[0].spec_line > 1);
    assert!(!diags[0].allowed.is_empty());
}

#[test]
fn dep_indirect_listed_path_is_empty() {
    let diags = enforce_at(&fixture("dep_indirect_ok"));
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn dep_indirect_unlisted_edge() {
    let diags = enforce_at(&fixture("dep_indirect_bad"));
    let mut rules = ids(&diags);
    rules.sort_unstable();
    assert_eq!(rules, ["ARCH-DEP-001", "ARCH-DEP-002"]);
    assert!(diags.iter().any(|diag| diag.symbol == "d -> x"));
    assert!(diags
        .iter()
        .any(|diag| diag.symbol == "a -> d -> x" && diag.rule_id == "ARCH-DEP-002"));
}

#[test]
fn mut_owner_write_is_empty() {
    let diags = enforce_at(&fixture("mut_ok"));
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn mut_write_through_use_alias() {
    let diags = enforce_at(&fixture("mut_alias_bad"));
    assert_eq!(ids(&diags), ["ARCH-MUT-001"], "{diags:?}");
    assert_eq!(diags[0].symbol, "Ledger.cash");
    assert!(diags[0].file.contains("other/"), "{}", diags[0].file);
    assert!(diags[0].allowed.contains("owner"), "{}", diags[0].allowed);
}

#[test]
fn call_from_allowed_module_is_empty() {
    let diags = enforce_at(&fixture("call_ok"));
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn call_through_reexport() {
    let diags = enforce_at(&fixture("call_reexport_bad"));
    assert_eq!(ids(&diags), ["ARCH-CALL-001"], "{diags:?}");
    assert!(
        diags[0].symbol.contains("network_order_submit"),
        "{}",
        diags[0].symbol
    );
    assert!(diags[0].file.contains("bad/"), "{}", diags[0].file);
    assert!(diags[0].allowed.contains("exec"), "{}", diags[0].allowed);
}

#[test]
fn effect_net_in_allowed_crate_is_empty() {
    let diags = enforce_at(&fixture("effect_ok"));
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn effect_net_in_money_crate() {
    let diags = enforce_at(&fixture("effect_net_bad"));
    assert_eq!(ids(&diags), ["ARCH-EFFECT-001"], "{diags:?}");
    assert!(diags[0].symbol.contains("std::net"), "{}", diags[0].symbol);
    assert!(
        diags[0].message.contains("money") && diags[0].message.contains("net"),
        "{}",
        diags[0].message
    );
}

#[test]
fn float_in_allowed_crate_is_empty() {
    let diags = enforce_at(&fixture("float_ok"));
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn float_allow_attribute_does_not_silence_money_crate() {
    let root = fixture("float_bad");
    let source = std::fs::read_to_string(root.join("money/src/lib.rs")).unwrap();
    assert!(source.contains("#[allow(clippy::float_arithmetic)]"));
    let diags = enforce_at(&root);
    assert!(diags
        .iter()
        .any(|diag| diag.rule_id == "ARCH-FLOAT-001" && diag.symbol == "f64"));
    assert!(diags.iter().all(|diag| diag.file.contains("money/")));
}

#[test]
fn generated_file_hand_edit_fails_check() {
    let tla = fixture("dep_direct_ok").join("spec/arch/ArchitecturePolicy.tla");
    let dir = std::env::temp_dir().join(format!("arch-gen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("policy.json");
    generate(&tla, &out, false).unwrap();
    let first = std::fs::read(&out).unwrap();
    generate(&tla, &out, true).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), first);
    let mut edited = first.clone();
    edited.extend(b"\n ");
    std::fs::write(&out, &edited).unwrap();
    let err = generate(&tla, &out, true).expect_err("hand edit");
    assert_eq!(err.rule_id, "ARCH-GEN-001");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unsupported_tla_construct_fails_parse() {
    let path = fixture("unsupported").join("spec/arch/ArchitecturePolicy.tla");
    let text = std::fs::read_to_string(&path).unwrap();
    let err = parse_policy(&path.display().to_string(), &text).expect_err("CHOOSE");
    assert_eq!(err.rule_id, "ARCH-PARSE-001");
    assert!(err.message.contains("CHOOSE"), "{}", err.message);
}

#[test]
fn path_owner_rejects_a_file_outside_the_bot() {
    let policy = parse_policy(
        "path.tla",
        &policy_text(
            &[
                ("crates/sim/**", "Risk/Portfolio"),
                ("crates/**", "Ops/Release"),
            ],
            &["Risk/Portfolio", "Ops/Release"],
        ),
    )
    .unwrap();
    let owned = path_diags_for(
        &policy,
        &["crates/sim/src/ledger.rs".to_owned()],
        "Risk/Portfolio",
    );
    assert!(owned.is_empty(), "{owned:?}");
    let denied = path_diags_for(
        &policy,
        &["crates/sim/src/ledger.rs".to_owned()],
        "Ops/Release",
    );
    assert_eq!(ids(&denied), ["ARCH-PATH-001"]);
    assert!(
        denied[0].allowed.contains("crates/**"),
        "{}",
        denied[0].allowed
    );
}

#[test]
fn path_equal_specificity_is_ambiguous() {
    let policy = parse_policy(
        "path.tla",
        &policy_text(
            &[
                ("src/lib.rs", "Data Engineer"),
                ("src/lib.rs", "Signal Builder"),
            ],
            &["Data Engineer", "Signal Builder"],
        ),
    )
    .unwrap();
    let diags = path_diags_for(&policy, &["src/lib.rs".to_owned()], "Data Engineer");
    assert_eq!(ids(&diags), ["ARCH-PATH-002"]);
}

#[test]
fn workspace_enforce_has_no_false_positives() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    let diags = enforce(&EnforceOptions {
        manifest: &manifest,
        diff: None,
        as_bot: None,
        check_generated: true,
    })
    .unwrap_or_else(|err| panic!("{err}"));
    assert!(diags.is_empty(), "{diags:#?}");
}

#[test]
fn every_tracked_file_has_one_owner() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest)
        .other_options(["--offline".to_owned(), "--no-deps".to_owned()])
        .exec()
        .unwrap();
    let root = PathBuf::from(metadata.workspace_root.as_str());
    let (policy, _) = xtask::read_policy(&root.join("spec/arch/ArchitecturePolicy.tla")).unwrap();
    let missing = xtask::every_tracked_has_owner(&root, &policy).unwrap();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn explain_covers_the_stable_ids() {
    for id in [
        "ARCH-DEP-001",
        "ARCH-DEP-002",
        "ARCH-MUT-001",
        "ARCH-MUT-002",
        "ARCH-CALL-001",
        "ARCH-CALL-002",
        "ARCH-EFFECT-001",
        "ARCH-FLOAT-001",
        "ARCH-GEN-001",
        "ARCH-PATH-001",
        "ARCH-PATH-002",
        "ARCH-PARSE-001",
        "ARCH-WF-001",
        "ARCH-TOOL-001",
    ] {
        let text = xtask::explain(id).unwrap();
        assert!(text.starts_with(id), "{text}");
    }
    assert!(xtask::explain("ARCH-NOPE").is_err());
}

fn policy_text(paths: &[(&str, &str)], owners: &[&str]) -> String {
    let owners = owners
        .iter()
        .map(|owner| format!("  \"{owner}\""))
        .collect::<Vec<_>>()
        .join(",\n");
    let paths = paths
        .iter()
        .map(|(glob, owner)| format!("  <<\"{glob}\", \"{owner}\">>"))
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        r#"--------------------------- MODULE ArchitecturePolicy ---------------------------
EXTENDS FiniteSets
Modules == {{"a"}}
Owners == {{
{owners}
}}
Effects == {{"env", "fs_write", "net", "process"}}
Capabilities == {{"holdout_read"}}
FloatAllowed == {{}}
AllowedDependencies == {{}}
AllowedStateWriters == {{}}
AllowedCapabilityCallers == {{}}
AllowedEffects == {{}}
PathOwners == {{
{paths}
}}
WellFormed ==
  /\ \A dep \in AllowedDependencies : (
       dep[1] \in Modules /\ dep[2] \in Modules /\ dep[1] # dep[2])
  /\ \A w \in AllowedStateWriters : (
       w[1] \in Modules /\ Cardinality({{ w2 \in AllowedStateWriters : w2[2] = w[2] }}) = 1)
  /\ \A c \in AllowedCapabilityCallers : (
       c[1] \in Modules /\ c[2] \in Capabilities)
  /\ \A e \in AllowedEffects : (
       e[1] \in Modules /\ e[2] \in Effects)
  /\ FloatAllowed \subseteq Modules
  /\ \A p \in PathOwners : (p[2] \in Owners)
  /\ Effects = {{"env", "fs_write", "net", "process"}}
VARIABLE clock
Init == clock = 0
Next == (clock = 0 /\ clock' = 1) \/ (clock = 1 /\ clock' = 0)
Spec == Init /\ [][Next]_clock
=============================================================================
"#
    )
}
