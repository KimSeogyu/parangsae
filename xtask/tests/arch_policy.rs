//! Positive and negative fixtures for each architecture rule.
//! These tests do not run SANY or TLC. `spec validate` is the CI job.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::{
    absolutize_jar, diff_names, enforce, generate, parse_policy, path_diags_for,
    unowned_path_diags, EnforceOptions,
};

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
fn optional_feature_dep_is_an_edge() {
    let diags = enforce_at(&fixture("dep_optional_bad"));
    assert!(
        diags.iter().any(|diag| {
            diag.rule_id == "ARCH-DEP-001" && diag.symbol == "sim -> parangsae-execution"
        }),
        "{diags:#?}"
    );
    assert!(
        diags
            .iter()
            .all(|diag| diag.rule_id == "ARCH-DEP-001" && diag.file.contains("sim/Cargo.toml")),
        "{diags:#?}"
    );
}

#[test]
fn registry_patch_dep_is_an_edge() {
    // G2: version req plus [patch.crates-io] onto a workspace path.
    let root = fixture("dep_patch_crates_bad");
    cargo_check(&root);
    let diags = enforce_at(&root);
    assert_execution_edge(&diags);
    assert_path_override(&diags, "patch");
}

#[test]
fn git_patch_dep_is_an_edge() {
    // G4: git req plus [patch."<url>"] onto a workspace path.
    let root = prepare_git_fixture("dep_patch_git_bad");
    cargo_check(&root);
    let diags = enforce_at(&root);
    assert_execution_edge(&diags);
    assert_path_override(&diags, "patch");
}

#[test]
fn git_member_dep_is_an_edge() {
    // G1: git req of this workspace's own execution crate, with no patch.
    let root = prepare_git_fixture("dep_git_member_bad");
    cargo_check(&root);
    assert_execution_edge(&enforce_at(&root));
}

#[test]
fn replace_path_is_a_direct_violation() {
    let root = fixture("dep_replace_bad");
    cargo_check(&root);
    let diags = enforce_at(&root);
    assert_eq!(ids(&diags), ["ARCH-DEP-001"]);
    assert_eq!(diags[0].symbol, "replace parangsae-execution:0.0.0");
    assert!(diags[0].file.ends_with("Cargo.toml"), "{}", diags[0].file);
}

fn assert_execution_edge(diags: &[xtask::Diagnostic]) {
    assert!(
        diags.iter().any(|diag| {
            diag.rule_id == "ARCH-DEP-001"
                && diag.symbol == "sim -> parangsae-execution"
                && diag.message.contains("is not allowed")
        }),
        "{diags:#?}"
    );
}

fn assert_path_override(diags: &[xtask::Diagnostic], kind: &str) {
    assert!(
        diags.iter().any(|diag| {
            diag.rule_id == "ARCH-DEP-001"
                && diag.symbol == format!("{kind} parangsae-execution")
                && diag.file.ends_with("Cargo.toml")
        }),
        "{diags:#?}"
    );
}

fn cargo_check(root: &Path) {
    let output = Command::new("cargo")
        .args(["check", "--offline", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .env(
            "CARGO_TARGET_DIR",
            std::env::temp_dir().join("parangsae-arch-check"),
        )
        .output()
        .expect("cargo check");
    assert!(
        output.status.success(),
        "cargo check {}\n{}{}",
        root.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// file:// git URLs are absolute, so the fixture keeps `__GIT_URL__` and this
/// copies it, commits `upstream`, and rewrites the URL before enforce.
fn prepare_git_fixture(name: &str) -> PathBuf {
    let src = fixture(name);
    let dst = std::env::temp_dir().join(format!("parangsae-{name}-{}", std::process::id()));
    if dst.exists() {
        fs::remove_dir_all(&dst).unwrap();
    }
    copy_dir(&src, &dst);
    let upstream = dst.join("upstream");
    git(&upstream, &["init"]);
    git(&upstream, &["add", "."]);
    git(
        &upstream,
        &[
            "-c",
            "user.email=arch@example.com",
            "-c",
            "user.name=arch",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "init",
        ],
    );
    let url = format!("file://{}", upstream.display());
    rewrite_token(&dst, "__GIT_URL__", &url);
    let output = Command::new("cargo")
        .args(["generate-lockfile", "--manifest-path"])
        .arg(dst.join("Cargo.toml"))
        .output()
        .expect("cargo generate-lockfile");
    assert!(
        output.status.success(),
        "lockfile {}\n{}{}",
        dst.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    dst
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} in {}\n{}{}",
        dir.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

fn rewrite_token(dir: &Path, token: &str, value: &str) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            rewrite_token(&path, token, value);
            continue;
        }
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            let text = fs::read_to_string(&path).unwrap();
            fs::write(&path, text.replace(token, value)).unwrap();
        }
    }
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
    assert!(
        diags.iter().any(|diag| diag.rule_id == "ARCH-CALL-001"
            && diag.file.contains("bad/")
            && diag.symbol.contains("network_order_submit")),
        "{diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|diag| diag.rule_id == "ARCH-CALL-001" && diag.file.contains("mid/")),
        "{diags:?}"
    );
    assert!(
        diags.iter().all(|diag| diag.rule_id == "ARCH-CALL-001"),
        "{diags:?}"
    );
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
fn float_literals_and_f64_methods_outside_float_allowed() {
    let diags = enforce_at(&fixture("float_literal_bad"));
    let symbols: Vec<&str> = diags.iter().map(|diag| diag.symbol.as_str()).collect();
    for symbol in ["0.5", "3.0", "1e-3", "1.5", "as_secs_f64"] {
        assert!(
            symbols.contains(&symbol),
            "{symbol} missing from {symbols:?}"
        );
    }
    assert!(diags
        .iter()
        .all(|diag| diag.rule_id == "ARCH-FLOAT-001" && diag.file.contains("money/")));
}

#[test]
fn effect_probes_are_env_and_fs_write() {
    let diags = enforce_at(&fixture("effect_probes_bad"));
    assert!(diags
        .iter()
        .any(|diag| { diag.rule_id == "ARCH-EFFECT-001" && diag.message.contains("env") }));
    assert!(diags
        .iter()
        .any(|diag| { diag.rule_id == "ARCH-EFFECT-001" && diag.message.contains("fs_write") }));
    assert!(diags
        .iter()
        .all(|diag| diag.rule_id == "ARCH-EFFECT-001" && diag.file.contains("money/")));
}

#[test]
fn call_mentions_flag_every_probe() {
    let root = fixture("call_mentions_bad");
    let diags = enforce_at(&root);
    let text = std::fs::read_to_string(root.join("bad/src/lib.rs")).unwrap();
    for marker in [
        "probe: use-alias",
        "probe: alias-call",
        "probe: mod-alias",
        "probe: typed",
        "probe: typed-call",
        "probe: argument",
        "probe: paren",
        "probe: vec",
        "probe: macro_rules",
    ] {
        let line = line_of(&text, marker);
        assert!(
            diags.iter().any(|diag| {
                diag.line == line
                    && diag.file.contains("bad/")
                    && diag.rule_id.starts_with("ARCH-CALL")
            }),
            "{marker} at {line} not in {diags:#?}"
        );
    }
    assert!(
        diags.iter().all(|diag| diag.file.contains("bad/")),
        "{diags:#?}"
    );
}

#[test]
fn path_dep_outside_the_workspace_is_an_unknown_crate() {
    let diags = enforce_at(&fixture("dep_outside_bad"));
    assert!(
        diags.iter().any(|diag| {
            diag.rule_id == "ARCH-DEP-001"
                && diag.symbol == "a -> outside"
                && diag.message.contains("unknown")
        }),
        "{diags:#?}"
    );
    assert!(diags
        .iter()
        .any(|diag| { diag.rule_id == "ARCH-FLOAT-001" && diag.file.contains("outside/") }));
    assert!(diags.iter().any(|diag| {
        diag.rule_id == "ARCH-EFFECT-001"
            && diag.message.contains("outside")
            && diag.message.contains("env")
    }));
}

#[test]
fn rename_diff_lists_both_paths() {
    let dir = std::env::temp_dir().join(format!("arch-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();
        assert!(status.success(), "{args:?}");
    };
    git(&["init", "-b", "main"]);
    std::fs::write(dir.join("old.rs"), "fn old() {}\n").unwrap();
    git(&["add", "old.rs"]);
    git(&["commit", "-m", "add"]);
    git(&["mv", "old.rs", "new.rs"]);
    git(&["commit", "-m", "move"]);
    let names = diff_names(&dir, "HEAD~1..HEAD").unwrap();
    assert!(names.iter().any(|name| name == "old.rs"), "{names:?}");
    assert!(names.iter().any(|name| name == "new.rs"), "{names:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn diff_without_a_bot_fails_a_path_with_no_owner() {
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
    let owned = unowned_path_diags(&policy, &["crates/sim/src/ledger.rs".to_owned()]);
    assert!(owned.is_empty(), "{owned:?}");
    let missing = unowned_path_diags(&policy, &["notes/unowned.md".to_owned()]);
    assert_eq!(ids(&missing), ["ARCH-PATH-001"]);
    assert!(missing[0].message.contains("no PathOwners"));
}

#[test]
fn jar_flag_is_resolved_from_the_workspace_root() {
    let relative = absolutize_jar(Path::new("/repo"), Path::new("specs/tla/tla2tools.jar"));
    assert_eq!(relative, PathBuf::from("/repo/specs/tla/tla2tools.jar"));
    let absolute = absolutize_jar(Path::new("/repo"), Path::new("/opt/tla2tools.jar"));
    assert_eq!(absolute, PathBuf::from("/opt/tla2tools.jar"));
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

fn line_of(text: &str, marker: &str) -> u32 {
    text.lines()
        .position(|line| line.contains(marker))
        .unwrap_or_else(|| panic!("missing {marker}")) as u32
        + 1
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
