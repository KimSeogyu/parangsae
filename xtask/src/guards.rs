//! Guards against a green run that did not exercise the check.
//!
//! Feature ownership, TLC config coverage, distinct-state floors, Kani harness
//! names, B2 measurements, and the ratchet in `regressions/floors.toml`.
//! The baseline commit comes from `PARANGSAE_FLOOR_BASE`. An all-zero
//! `before`, a `before` missing from the checkout, or a `before` that is
//! not an ancestor of `HEAD` is not a ratchet baseline. Those cases use
//! `merge-base` of `HEAD` and `refs/remotes/origin/main` and never fetch
//! the `before` commit. `origin/main` is present because the checkout uses
//! `fetch-depth: 0`; if it is missing, resolution fails closed. A missing
//! `regressions/floors.toml` is accepted only when that resolved commit is
//! exactly `BOOTSTRAP_BASE`, the raw sha256 of the current floors file is
//! `BOOTSTRAP_FLOORS_SHA256`, and `origin/main` does not already contain the
//! file. The landing push is the exception: on `main`, with that base, when
//! `origin/main` equals `HEAD`. Any other missing baseline fails.
//! `BOOTSTRAP_BASE`, `BOOTSTRAP_FLOORS_SHA256`, `EXPECTED_GUARD_RUNS`,
//! `CARGO_CONFIG_TOML`, `CARGO_CONFIG_SHA256`, `ALLOWED_ACTIONS`,
//! `REQUIRED_JOBS`, `JOB_ENV_ALLOW`, `STEP_ENV_ALLOW`, `RUST_TOOLCHAIN_TOML`,
//! `RUST_TOOLCHAIN_SHA256`, `DENY_TOML_SHA256`, `CARGO_LOCK_SHA256`,
//! `DENY_VERSION`, `DENY_TARBALL_SHA256`, `DENY_BIN_SHA256`, `CARGO_MANIFESTS`,
//! `CARGO_MANIFESTS_SHA256`, `CI_HEADER`, `CI_HEADER_SHA256`,
//! `ALLOWED_TRIGGERS`, `ALLOWED_WORKFLOW_FILES`, `ALLOWED_DEMO_SECRETS`,
//! `VISION_SMOKE_SHA256`, `GATE_RESULTS_SHA256`, `CLOSURE_PINS`,
//! `TEST_BINARY_COUNTS`, `ENV_I_TEMPLATES`, `ENV_I_OCCURRENCES`,
//! `PY_HEREDOC_TEMPLATE`, `PRINTF_GREP_STATEMENTS`, `CASE_LITERAL_MATCHES`,
//! `CUCUMBER_CRATE`, and `GHERKIN_CRATE`
//! are Reviewer-protected. Baseline resolution never skips the other guards.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Floors {
    values: BTreeMap<String, u64>,
    kani_names: Vec<String>,
    invariants: BTreeMap<String, Vec<String>>,
    ci_jobs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FeaturePolicy {
    runner: Vec<RunnerPolicy>,
}

#[derive(Debug, Deserialize)]
struct RunnerPolicy {
    name: String,
    glob: String,
    source: String,
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return check_workspace(root);
    }
    match args[0].as_str() {
        "features" => {
            let scan = match optional_path(args, "--root")? {
                Some(path) => path,
                None => root.to_path_buf(),
            };
            finish(Ok(feature_ownership(root, &scan, scan == root)?))
        }
        "tlc-cfg" => {
            let tla_path = required_path(args, "--tla")?;
            let cfg_path = required_path(args, "--cfg")?;
            let tla = read(&tla_path)?;
            let cfg = read(&cfg_path)?;
            let mut violations: Vec<String> = uncovered_invariants(&tla, &cfg)
                .into_iter()
                .map(|name| format!("missing from cfg: {name}"))
                .collect();
            if let Some(model) = optional_value(args, "--model")? {
                let floors = load_floors(&root.join("regressions/floors.toml"))?;
                violations.extend(floor_invariant_gaps(&model, &tla, &cfg, &floors));
            }
            finish(Ok(violations))
        }
        "tlc-output" => {
            let model = required_value(args, "--model")?;
            let file = required_path(args, "--file")?;
            let floors = load_floors(&root.join("regressions/floors.toml"))?;
            let minimum = floors.value(&format!("tlc.{model}.distinct_states"))?;
            let found = distinct_states(&read(&file)?)?;
            if found < minimum {
                Err(format!(
                    "tlc {model} distinct states {found} is below floor {minimum}"
                ))
            } else {
                Ok(())
            }
        }
        "kani-output" => {
            let file = required_path(args, "--file")?;
            let floors = load_floors(&root.join("regressions/floors.toml"))?;
            finish(Ok(kani_violations(&read(&file)?, &floors)?))
        }
        "ratchet" => {
            let current = load_floors(&required_path(args, "--current")?)?;
            let base = load_floors(&required_path(args, "--base")?)?;
            finish(Ok(ratchet(&current, &base)))
        }
        "ci-gate" => {
            let file = required_path(args, "--file")?;
            finish(Ok(ci_gate_gaps(&read(&file)?)?))
        }
        "gate-script" => {
            let file = required_path(args, "--file")?;
            finish(Ok(gate_results_pin(&read_bytes(&file)?)))
        }
        "gate-fixture" => {
            let name = required_value(args, "--name")?;
            let file = required_path(args, "--file")?;
            finish(Ok(gate_fixture_pin(&name, &read_bytes(&file)?)))
        }
        "toolchain-files" => {
            if args.len() != 1 {
                return Err("unexpected argument after toolchain-files".to_owned());
            }
            finish(toolchain_file_gaps(root))
        }
        "segment-fixture" => {
            let file = required_path(args, "--file")?;
            match segment_fixture_fault(&read(&file)?)? {
                Some(name) => Err(name.to_owned()),
                None => Ok(()),
            }
        }
        "test-output" => {
            let job = required_value(args, "--job")?;
            let step = required_value(args, "--step")?;
            let log = read_stdin()?;
            finish(Ok(test_binary_table_gaps(&log, &job, &step)?))
        }
        "cucumber-summary" => {
            let runner = required_value(args, "--runner")?;
            let log = read_stdin()?;
            finish(Ok(cucumber_summary_gaps(root, &runner, &log)?))
        }
        "command-inventory" => {
            if args.len() != 1 {
                return Err("unexpected argument after command-inventory".to_owned());
            }
            let ci = read(&root.join(".github/workflows/ci.yml"))?;
            let lines = command_listing(root, &ci)?;
            let mut out = std::io::stdout().lock();
            for line in lines {
                writeln!(out, "{line}").map_err(|err| format!("write inventory: {err}"))?;
            }
            Ok(())
        }
        other => Err(format!("unknown guards command {other}")),
    }
}

fn check_workspace(root: &Path) -> Result<(), String> {
    let base = std::env::var("PARANGSAE_FLOOR_BASE").ok();
    let reports = guard_reports(root, base.as_deref(), env_flag("PARANGSAE_ON_MAIN"));
    for report in &reports {
        emit_guard_line(&report.line())?;
    }
    emit_guard_line(&guard_summary(&reports))?;
    let mut errors = Vec::new();
    for report in &reports {
        if report.passed {
            continue;
        }
        if !report.ran {
            if report.detail.is_empty() {
                errors.push(format!("guard {} did not run", report.name));
            } else {
                errors.push(format!(
                    "guard {} did not run: {}",
                    report.name, report.detail
                ));
            }
            continue;
        }
        if report.detail.is_empty() {
            errors.push(format!("guard {} failed", report.name));
        } else {
            errors.push(report.detail.clone());
        }
    }
    errors.extend(guard_run_gaps(&reports));
    errors.extend(guard_inventory_gaps());
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

/// Reviewer-protected. Bootstrap applies only when the resolved base equals this commit.
const BOOTSTRAP_BASE: &str = "e71e0d75c88368260021d83a2a2b0cae3b1aeefe";

/// Reviewer-protected. The only `uses:` targets. Kani is installed with cargo, not an action.
const ALLOWED_ACTIONS: &[&str] = &[
    "actions/checkout@11d5960a326750d5838078e36cf38b85af677262",
    "actions/setup-java@cf277c60eb25467037889841efdb72551f06f6c3",
    "dtolnay/rust-toolchain@5059963b1731bf1580991b5f3b1eecfb5da406c0",
];

/// Reviewer-protected. Raw sha256 of `regressions/floors.toml` on the bootstrap path.
const BOOTSTRAP_FLOORS_SHA256: &str =
    "37268a2ef608080b929c1344e5768d119943ecfb86f0249d05bb45a4803e4aec";

/// Reviewer-protected. Non-ratchet guards plus the ratchet.
const EXPECTED_GUARD_RUNS: usize = 12;

const NON_RATCHET_GUARDS: &[&str] = &[
    "feature-ownership",
    "tla-models",
    "floor-keys",
    "b2-counts",
    "cucumber-counts",
    "ci-gate",
    "gate-script",
    "gate-fixtures",
    "workflow-permissions",
    "action-pins",
    "cargo-workspace-dir",
];

struct GuardReport {
    name: &'static str,
    ran: bool,
    passed: bool,
    detail: String,
}

impl GuardReport {
    fn line(&self) -> String {
        if !self.ran {
            return format!("guard {} did not run", self.name);
        }
        let status = if self.passed { "passed" } else { "failed" };
        format!("guard {} ran {status}", self.name)
    }
}

fn guard_inventory_gaps() -> Vec<String> {
    let mut names: Vec<&str> = NON_RATCHET_GUARDS.to_vec();
    names.push("ratchet");
    inventory_gaps(&names)
}

fn inventory_gaps(names: &[&str]) -> Vec<String> {
    let mut gaps = Vec::new();
    if names.len() != EXPECTED_GUARD_RUNS {
        gaps.push(format!(
            "guard inventory {} is not {EXPECTED_GUARD_RUNS}",
            names.len()
        ));
    }
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(*name) {
            gaps.push(format!("guard {name} is duplicated"));
        }
    }
    gaps
}

fn emit_guard_line(line: &str) -> Result<(), String> {
    writeln!(std::io::stdout().lock(), "{line}").map_err(|err| format!("write guard log: {err}"))
}

fn guard_summary(reports: &[GuardReport]) -> String {
    let ran = reports.iter().filter(|report| report.ran).count();
    let passed = reports
        .iter()
        .filter(|report| report.ran && report.passed)
        .count();
    format!("guards ran {ran} passed {passed}")
}

fn guard_run_gaps(reports: &[GuardReport]) -> Vec<String> {
    let mut gaps = Vec::new();
    let ran = reports.iter().filter(|report| report.ran).count();
    if ran < EXPECTED_GUARD_RUNS {
        gaps.push(format!(
            "guards ran {ran} which is below {EXPECTED_GUARD_RUNS}"
        ));
    }
    for name in NON_RATCHET_GUARDS {
        if !reports
            .iter()
            .any(|report| report.name == *name && report.ran)
        {
            gaps.push(format!("guard {name} did not run"));
        }
    }
    if !reports
        .iter()
        .any(|report| report.name == "ratchet" && report.ran)
    {
        gaps.push("guard ratchet did not run".to_owned());
    }
    gaps
}

fn guard_reports(root: &Path, floor_base: Option<&str>, on_main: bool) -> Vec<GuardReport> {
    let mut reports = Vec::new();
    reports.push(guard_from_result(
        "feature-ownership",
        feature_ownership(root, root, true),
    ));
    reports.push(guard_from_result("tla-models", tla_models(root)));
    let floors = load_floors(&root.join("regressions/floors.toml"));
    reports.push(match &floors {
        Ok(floors) => guard_from_violations("floor-keys", required_floor_keys(floors)),
        Err(err) => guard_failed("floor-keys", err.clone()),
    });
    reports.push(ratchet_report(root, floor_base, on_main, &floors));
    match &floors {
        Ok(floors) => {
            reports.push(guard_from_result("b2-counts", b2_violations(root, floors)));
            reports.push(guard_from_result(
                "cucumber-counts",
                cucumber_count_violations(root, floors),
            ));
        }
        Err(err) => {
            reports.push(guard_not_run("b2-counts", err.clone()));
            reports.push(guard_not_run("cucumber-counts", err.clone()));
        }
    }
    reports.push(guard_from_result("ci-gate", ci_gate_result(root)));
    reports.push(guard_from_result("gate-script", gate_script_result(root)));
    reports.push(guard_from_result(
        "gate-fixtures",
        gate_fixture_result(root),
    ));
    reports.push(guard_from_result(
        "workflow-permissions",
        workflow_permission_result(root),
    ));
    reports.push(guard_from_result("action-pins", action_pin_result(root)));
    reports.push(guard_from_violations(
        "cargo-workspace-dir",
        cargo_workspace_dir_violations(root),
    ));
    reports
}

fn guard_from_result(name: &'static str, result: Result<Vec<String>, String>) -> GuardReport {
    match result {
        Ok(violations) => guard_from_violations(name, violations),
        Err(err) => guard_failed(name, err),
    }
}

fn guard_from_violations(name: &'static str, violations: Vec<String>) -> GuardReport {
    if violations.is_empty() {
        GuardReport {
            name,
            ran: true,
            passed: true,
            detail: String::new(),
        }
    } else {
        GuardReport {
            name,
            ran: true,
            passed: false,
            detail: violations.join("\n"),
        }
    }
}

fn guard_failed(name: &'static str, detail: String) -> GuardReport {
    GuardReport {
        name,
        ran: true,
        passed: false,
        detail,
    }
}

fn guard_not_run(name: &'static str, detail: String) -> GuardReport {
    GuardReport {
        name,
        ran: false,
        passed: false,
        detail,
    }
}

fn ci_gate_result(root: &Path) -> Result<Vec<String>, String> {
    let text = read(&root.join(".github/workflows/ci.yml"))?;
    let mut gaps = ci_gate_gaps(&text)?;
    match load_floors(&root.join("regressions/floors.toml")) {
        Ok(floors) => gaps.extend(ci_job_inventory_gaps(&text, &floors)),
        Err(err) => gaps.push(err),
    }
    Ok(gaps)
}

fn gate_script_result(root: &Path) -> Result<Vec<String>, String> {
    Ok(gate_results_pin(&read_bytes(
        &root.join(".github/workflows/gate_results.py"),
    )?))
}

fn gate_fixture_result(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    for (name, _) in GATE_FIXTURE_SHA256 {
        violations.extend(gate_fixture_pin(name, &read_bytes(&root.join(name))?));
    }
    Ok(violations)
}

fn cargo_workspace_dir_violations(root: &Path) -> Vec<String> {
    let in_ci = crate::ci_is_set(
        std::env::var_os("GITHUB_ACTIONS").as_deref(),
        std::env::var_os("CI").as_deref(),
    );
    let mut gaps = match crate::reject_cargo_workspace_dir_in_ci(
        in_ci,
        std::env::var_os("CARGO_WORKSPACE_DIR").as_deref(),
    ) {
        Ok(()) => Vec::new(),
        Err(err) => vec![err],
    };
    match read_bytes(&root.join("rust-toolchain.toml")) {
        Ok(bytes) => gaps.extend(toolchain_bytes_gaps(&bytes)),
        Err(err) => gaps.push(format!("rust-toolchain.toml is missing: {err}")),
    }
    match toolchain_file_gaps(root) {
        Ok(found) => gaps.extend(found),
        Err(err) => gaps.push(err),
    }
    gaps.extend(sha_file_gaps(root, "deny.toml", DENY_TOML_SHA256));
    gaps.extend(sha_file_gaps(root, "Cargo.lock", CARGO_LOCK_SHA256));
    gaps.extend(cargo_manifests_gaps(root));
    gaps
}

const GATE_CHECK_STEP: &str = concat!(
    "      - name: fail unless every needed job succeeded\n",
    "        env:\n",
    "          NEEDS: ${{ toJSON(needs) }}\n",
    "        run: python3 .github/workflows/gate_results.py\n",
);

const GUARDS_STEP: &str = concat!(
    "      - name: cargo xtask guards\n",
    "        env:\n",
    "          PARANGSAE_FLOOR_BASE: ${{ github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.event.before }}\n",
    "          PARANGSAE_ON_MAIN: ${{ github.ref == 'refs/heads/main' && '1' || '' }}\n",
    "        run: |\n",
    "          set -o pipefail\n",
    "          cargo xtask guards | tee \"${{ runner.temp }}/guards.txt\"\n",
    "          grep -qx 'guards ran 12 passed 12' \"${{ runner.temp }}/guards.txt\"\n",
);

const ALL_ZERO_STEP: &str = concat!(
    "      - name: all-zero before compares with merge-base and does not skip\n",
    "        run: |\n",
    "          set -o pipefail\n",
    "          PARANGSAE_FLOOR_BASE=0000000000000000000000000000000000000000 cargo xtask guards | tee \"${{ runner.temp }}/guards-zero.txt\"\n",
    "          grep -qx 'guards ran 12 passed 12' \"${{ runner.temp }}/guards-zero.txt\"\n",
);

const KANI_RUSTUP_STABLE: &str = "          RUSTUP_TOOLCHAIN: 1.98.0";
const KANI_RUSTUP_NIGHTLY: &str = "          RUSTUP_TOOLCHAIN: nightly-2026-08-21";

/// Reviewer-protected.
const CARGO_CONFIG_TOML: &str = "[alias]\nxtask = \"run --package xtask --\"\n";
/// Reviewer-protected.
const CARGO_CONFIG_SHA256: &str =
    "783fdc447abb5b8f8b5652157aa9ede64da640813fa89679c29c5251c8e0cbe6";

/// Reviewer-protected. Exact bytes of `rust-toolchain.toml`. No `path=` key.
const RUST_TOOLCHAIN_TOML: &str =
    "[toolchain]\nchannel = \"1.98.0\"\ncomponents = [\"rustfmt\", \"clippy\"]\n";
/// Reviewer-protected.
const RUST_TOOLCHAIN_SHA256: &str =
    "07487db9d6b45de2f308f9fb578c9c46041381c3ee44ef782eca64150a394772";

/// Reviewer-protected. sha256 of `deny.toml`, re-checked in the deny job.
const DENY_TOML_SHA256: &str = "d8cec209fc719185e615794935ce8fecae69bf27e0f123ad85211d17857194c3";
/// Reviewer-protected. sha256 of `Cargo.lock`, re-checked in the deny job.
const CARGO_LOCK_SHA256: &str = "1441e1e59f0c8b75994c4ba04483930ab66826314efa5607e4e2ef6ab066b9e4";

/// Reviewer-protected. cargo-deny release the deny job downloads.
const DENY_VERSION: &str = "0.20.2";
/// Reviewer-protected. sha256 of `cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz`.
const DENY_TARBALL_SHA256: &str =
    "9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f";
/// Reviewer-protected. sha256 of the `cargo-deny` binary inside that tarball.
const DENY_BIN_SHA256: &str = "b329e25933d01c36dd7c47d84ea5716694f9b7caf53a5003d45674703a8ed54a";

/// Reviewer-protected. Workspace `Cargo.toml` plus every member manifest, sorted by path.
const CARGO_MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "crates/backtest/Cargo.toml",
    "crates/collector/Cargo.toml",
    "crates/core/Cargo.toml",
    "crates/execution/Cargo.toml",
    "crates/regime/Cargo.toml",
    "crates/signals/Cargo.toml",
    "crates/sim/Cargo.toml",
    "xtask/Cargo.toml",
];
/// Reviewer-protected. sha256 of the `sha256sum` text for `CARGO_MANIFESTS`.
const CARGO_MANIFESTS_SHA256: &str =
    "0d0ee519a2aa23cff0c8d6c587e97df2e5c90a7058853a696c2609586cc1f4ce";

/// Reviewer-protected. Exact non-job preamble of `.github/workflows/ci.yml`.
const CI_HEADER: &str = include_str!("pinned_jobs/ci.header");
/// Reviewer-protected.
const CI_HEADER_SHA256: &str = "e70f3eb0e8ab1ec9cb815c23ee523d4cf29bd82824f34cdaf07e83468bff5671";

/// Reviewer-protected. Canonical bytes and sha256 of each required job.
const REQUIRED_JOBS: &[(&str, &str, &str)] = &[
    (
        "changes",
        include_str!("pinned_jobs/changes.job"),
        "3c01b3bbc841341ebf4a8ad837048aadfcefd7914d40f515226c7639845c8bd2",
    ),
    (
        "fmt",
        include_str!("pinned_jobs/fmt.job"),
        "acccaa8b48519fcd2d982011ef693eb4b926388b7cc9f27d4d9f4ab60c1e84bf",
    ),
    (
        "clippy",
        include_str!("pinned_jobs/clippy.job"),
        "8ea34e7b3dc7a05f07669ba2eb74e294ecee5dad3fa8e9f3c44747578535a595",
    ),
    (
        "lint-fixture",
        include_str!("pinned_jobs/lint-fixture.job"),
        "e2fd8711baa3796507ef2011afa25bdf7d4baff51ed489df32d0a2aa8955862d",
    ),
    (
        "depgraph",
        include_str!("pinned_jobs/depgraph.job"),
        "548ac0a08768f080287437bdb53b314291eee5e85d6abd16851ca57e52dd9c58",
    ),
    (
        "test",
        include_str!("pinned_jobs/test.job"),
        "fa3dddbb33063df6a725f0f0d583ee2822d0aab80644f7480873af22e781aeaa",
    ),
    (
        "cucumber",
        include_str!("pinned_jobs/cucumber.job"),
        "6998c905a4900a0f86652775f7e6f0b1989c858f0782f7e2392e44fa91335be2",
    ),
    (
        "guards",
        include_str!("pinned_jobs/guards.job"),
        "6fd148f17910d86586d13a8435d809c07e46ea18c3db3e1f0f5341950d081e3d",
    ),
    (
        "guard-fixtures",
        include_str!("pinned_jobs/guard-fixtures.job"),
        "4ef405352108d9f22fcf9741dcb65dbc8f8451ab7cbb7a21f5af774fa93d4e59",
    ),
    (
        "tla",
        include_str!("pinned_jobs/tla.job"),
        "80a41530941e1ff9ff4e9f6439649db724ca8ab35ad5bfb96aa36fb16fe8b822",
    ),
    (
        "kani",
        include_str!("pinned_jobs/kani.job"),
        "72d4932b7331e414e6cf16f701c6b75462725ed6436967f15e32e616bec8dd64",
    ),
    (
        "deny",
        include_str!("pinned_jobs/deny.job"),
        "7093a5d12c7a3341f5381b8619c1f07b44e403e495c1f4fccdcdc07160c8a58e",
    ),
    (
        "gate",
        include_str!("pinned_jobs/gate.job"),
        "1af2588254a4e34454b5636408978164be1a78e728ea2002cd83cc71af313153",
    ),
];

/// Reviewer-protected. Job-level `env:` names on each required job.
const JOB_ENV_ALLOW: &[(&str, &[&str])] = &[
    ("changes", &[]),
    ("fmt", &[]),
    ("clippy", &[]),
    ("lint-fixture", &[]),
    ("depgraph", &[]),
    ("test", &[]),
    ("cucumber", &[]),
    ("guards", &[]),
    ("guard-fixtures", &[]),
    ("tla", &[]),
    ("kani", &[]),
    ("deny", &[]),
    ("gate", &[]),
];

/// Reviewer-protected. Step-level `env:` names on each required job.
const STEP_ENV_ALLOW: &[(&str, &[&str])] = &[
    (
        "changes",
        &["EVENT_NAME", "PUSH_BEFORE", "PR_BASE", "DEFAULT_BRANCH"],
    ),
    ("fmt", &[]),
    ("clippy", &[]),
    ("lint-fixture", &[]),
    ("depgraph", &[]),
    ("test", &[]),
    ("cucumber", &[]),
    ("guards", &["PARANGSAE_FLOOR_BASE", "PARANGSAE_ON_MAIN"]),
    ("guard-fixtures", &["PARANGSAE_FLOOR_BASE"]),
    ("tla", &[]),
    ("kani", &["RUSTUP_TOOLCHAIN"]),
    ("deny", &[]),
    ("gate", &["NEEDS"]),
];

const RUSTC_198_STEP: &str = concat!(
    "      - name: rustc is 1.98.0\n",
    "        run: |\n",
    "          expected=\"rustc 1.98.0 (88d9e12ae 2026-08-18)\"\n",
    "          actual=\"$(rustc -V)\"\n",
    "          if [ \"$actual\" != \"$expected\" ]; then\n",
    "            echo \"rustc -V was ${actual}\"\n",
    "            exit 1\n",
    "          fi\n",
);

const RUSTUP_WHICH_STEP: &str = concat!(
    "      - name: cargo and rustc are the 1.98.0 toolchain\n",
    "        run: |\n",
    "          cargo_bin=\"$(rustup which cargo)\"\n",
    "          rustc_bin=\"$(rustup which rustc)\"\n",
    "          if [ \"$cargo_bin\" != \"/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo\" ]; then\n",
    "            echo \"cargo is ${cargo_bin}\"\n",
    "            exit 1\n",
    "          fi\n",
    "          if [ \"$rustc_bin\" != \"/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc\" ]; then\n",
    "            echo \"rustc is ${rustc_bin}\"\n",
    "            exit 1\n",
    "          fi\n",
);

const GUARD_FIXTURE_UNREADABLE_STEP: &str = concat!(
    "      - name: unreadable floor baseline fails closed\n",
    "        env:\n",
    "          PARANGSAE_FLOOR_BASE: not-a-sha\n",
    "        run: |\n",
    "          set +e\n",
    "          output=$(cargo xtask guards 2>&1)\n",
    "          status=$?\n",
    "          set -e\n",
    "          printf '%s\\n' \"$output\"\n",
    "          if [ \"$status\" -eq 0 ]; then\n",
    "            echo \"unreadable baseline was accepted\"\n",
    "            exit 1\n",
    "          fi\n",
    "          printf '%s\\n' \"$output\" | grep -q 'unreadable'\n",
);

/// Reviewer-protected. Triggers a workflow file may name. Non-ci files are
/// narrower: `workflow_dispatch` only.
const ALLOWED_TRIGGERS: &[&str] = &["push", "pull_request", "workflow_dispatch"];

/// Reviewer-protected. The only secrets a non-ci workflow may reference.
const ALLOWED_DEMO_SECRETS: &[&str] = &["BINANCE_DEMO_API_KEY", "BINANCE_DEMO_API_SECRET"];

/// Reviewer-protected. Raw sha256 of `.github/workflows/vision-smoke.yml`.
const VISION_SMOKE_SHA256: &str =
    "6158b7cfad3cc94fc851d9f709975d8caf7b7d9422b422162c8f1846a05b10c4";

const FORBIDDEN_ENV: &[&str] = &[
    "CARGO_WORKSPACE_DIR",
    "GITHUB_ACTIONS",
    "CI",
    "PARANGSAE_FLOOR_BASE",
    "RUSTUP_TOOLCHAIN",
];

const GATE_FIXTURE_STEP: &str = include_str!("gate_fixture_step.txt");

/// Reviewer-protected. Raw sha256 of `.github/workflows/gate_results.py`.
const GATE_RESULTS_SHA256: &str =
    "67f96791665fc8b870c26e70c16d62a06cfa127e7e1dc9f86974654c0f567a24";

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkflowFilePin {
    Ci,
    WholeFile {
        message: &'static str,
        sha256: &'static str,
    },
}

/// Reviewer-protected. Files allowed under `.github/workflows/`, keyed to the pin.
const ALLOWED_WORKFLOW_FILES: &[(&str, WorkflowFilePin)] = &[
    (".github/workflows/ci.yml", WorkflowFilePin::Ci),
    (
        ".github/workflows/gate_results.py",
        WorkflowFilePin::WholeFile {
            message: "gate_results.py sha256 does not match",
            sha256: GATE_RESULTS_SHA256,
        },
    ),
    (
        ".github/workflows/vision-smoke.yml",
        WorkflowFilePin::WholeFile {
            message: "vision-smoke.yml sha256 does not match",
            sha256: VISION_SMOKE_SHA256,
        },
    ),
];

const GATE_FIXTURE_SHA256: &[(&str, &str)] = &[
    (
        "fixtures/guards/ci/needs-cancelled.json",
        "c8a35ed42b8f06c4f1f7505e6d99cf61107d563d8ddb9fc29c5a77d8749e7e1c",
    ),
    (
        "fixtures/guards/ci/needs-empty.json",
        "ca3d163bab055381827226140568f3bef7eaac187cebd76878e0b63e9e442356",
    ),
    (
        "fixtures/guards/ci/needs-entry-non-object.json",
        "0b4d8bc7dfef11cb91fd85d2ec04e692b5c4645ba6b8e0518a39997fbd78d51d",
    ),
    (
        "fixtures/guards/ci/needs-failure.json",
        "bf1e5a5938aa5b91beb5bd900b03e2df8251a41e743b062c774c56567572178b",
    ),
    (
        "fixtures/guards/ci/needs-malformed.json",
        "afd37634d06e2ec494d371a0d8a0290c8086f90d312545e7700bdd87345cd6ce",
    ),
    (
        "fixtures/guards/ci/needs-non-object.json",
        "37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570",
    ),
    (
        "fixtures/guards/ci/needs-path-skipped.json",
        "443b3ddf9b8158a9b9fc97e0f9c10bdc8daf8cbf61add5245454b1c1f4c9afab",
    ),
    (
        "fixtures/guards/ci/needs-null.json",
        "b304e324ab16bd0921ed2eaf0d7a2e6fbe9c3bf3e2bd20b6e2e721a0f8b6eef2",
    ),
    (
        "fixtures/guards/ci/needs-skipped.json",
        "e6592562a39006232d0965a99b9905dade1b73798e66361e3b2dbddd0d26a3c1",
    ),
    (
        "fixtures/guards/ci/needs-success.json",
        "22d1b135c8fc2cd56844481bd12ccb07f030b6c3a2e555018cc77eba4f53a52a",
    ),
];

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn gate_results_pin(bytes: &[u8]) -> Vec<String> {
    pinned_whole_file_gaps(".github/workflows/gate_results.py", bytes)
}

fn gate_fixture_pin(name: &str, bytes: &[u8]) -> Vec<String> {
    let Some((_, expected)) = GATE_FIXTURE_SHA256.iter().find(|(path, _)| *path == name) else {
        return vec![format!("gate fixture {name} is not pinned")];
    };
    if sha256_hex(bytes) == *expected {
        Vec::new()
    } else {
        vec![format!("gate fixture {name} sha256 does not match")]
    }
}

fn ci_gate_gaps(text: &str) -> Result<Vec<String>, String> {
    let jobs = ci_job_ids(text)?;
    let block = match gate_job_block(text) {
        Ok(block) => block,
        Err(err) => return Ok(vec![err]),
    };
    let mut gaps = Vec::new();
    if !block
        .lines()
        .any(|line| strip_yaml_comment(line).trim() == "if: always()")
    {
        gaps.push("gate job is missing if: always()".to_owned());
    }
    if gate_check_step(block) != GATE_CHECK_STEP {
        gaps.push("gate check step does not match".to_owned());
    }
    if gate_fixture_step(block) != GATE_FIXTURE_STEP {
        gaps.push("gate fixture step does not match".to_owned());
    }
    gaps.extend(gate_control_gaps(block));
    let needs = match gate_needs(block) {
        Ok(needs) => needs,
        Err(err) => {
            gaps.push(err);
            return Ok(gaps);
        }
    };
    let mut inventory = jobs;
    inventory.remove("gate");
    for name in inventory.difference(&needs) {
        gaps.push(format!("ci job {name} is missing from gate.needs"));
    }
    for name in needs.difference(&inventory) {
        gaps.push(format!("gate.needs lists {name} which is not a ci job"));
    }
    for name in &needs {
        let Some(job) = job_block(text, name) else {
            continue;
        };
        if block_sets_continue_on_error(job) {
            gaps.push(format!("ci job {name} sets continue-on-error"));
        }
    }
    gaps.extend(required_job_if_gaps(text, &needs));
    gaps.extend(required_step_directive_gaps(&needs, text));
    Ok(gaps)
}

fn required_step_directive_gaps(needs: &BTreeSet<String>, text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    for name in needs {
        let Some(block) = job_block(text, name) else {
            continue;
        };
        gaps.extend(step_directive_gaps(name, block));
    }
    gaps
}

fn step_directive_gaps(job: &str, block: &str) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    let mut gaps = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let indent = lines[idx].len() - lines[idx].trim_start().len();
        let trimmed = strip_yaml_comment(lines[idx]).trim();
        if let Some(rest) = trimmed.strip_prefix("run:") {
            let rest = rest.trim();
            if rest.is_empty() || matches!(rest, "|" | ">" | "|-" | ">-" | "|+" | ">+") {
                idx += 1;
                while idx < lines.len() {
                    if !lines[idx].trim().is_empty() {
                        let findent = lines[idx].len() - lines[idx].trim_start().len();
                        if findent <= indent {
                            break;
                        }
                    }
                    idx += 1;
                }
                continue;
            }
        }
        let key = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        if indent > 4 {
            if key.starts_with("shell:") {
                gaps.push(format!("ci job {job} sets shell:"));
            }
            if key.starts_with("working-directory:") {
                gaps.push(format!("ci job {job} sets working-directory:"));
            }
        }
        idx += 1;
    }
    gaps
}

/// `tla` and `kani` may carry the path-filter condition from `changes`.
/// Any other job-level `if:`, or a different condition, still fails.
fn path_filter_job_if(name: &str, block: &str) -> bool {
    let expected = match name {
        "tla" => "if: needs.changes.outputs.tla == 'true'",
        "kani" => "if: needs.changes.outputs.kani == 'true'",
        _ => return false,
    };
    let mut found = 0usize;
    let mut exact = false;
    for line in block.lines() {
        let indent = line.len() - line.trim_start().len();
        let key = strip_yaml_comment(line).trim();
        if indent == 4 && key.starts_with("if:") {
            found += 1;
            exact = key == expected;
        }
    }
    found == 1 && exact
}

fn required_job_if_gaps(text: &str, needs: &BTreeSet<String>) -> Vec<String> {
    let mut gaps = Vec::new();
    for name in needs {
        let Some(block) = job_block(text, name) else {
            continue;
        };
        if job_level_if(block) && !path_filter_job_if(name, block) {
            gaps.push(format!("ci job {name} sets if:"));
        }
        if step_level_if(block) {
            gaps.push(format!("ci job {name} sets step if:"));
        }
    }
    gaps
}

fn job_level_if(block: &str) -> bool {
    block.lines().any(|line| {
        let indent = line.len() - line.trim_start().len();
        let key = strip_yaml_comment(line).trim();
        indent == 4 && key.starts_with("if:")
    })
}

fn step_level_if(block: &str) -> bool {
    block.lines().any(|line| {
        let indent = line.len() - line.trim_start().len();
        let trimmed = strip_yaml_comment(line).trim();
        let key = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        indent > 4 && key.starts_with("if:")
    })
}

fn ci_job_inventory_gaps(text: &str, floors: &Floors) -> Vec<String> {
    let jobs = match ci_job_ids(text) {
        Ok(jobs) => jobs,
        Err(err) => return vec![err],
    };
    let needs = match gate_job_block(text).and_then(gate_needs) {
        Ok(needs) => needs,
        Err(err) => return vec![err],
    };
    let mut gaps = Vec::new();
    let mut listed = BTreeSet::new();
    for name in &floors.ci_jobs {
        if !listed.insert(name.as_str()) {
            gaps.push(format!("duplicate ci job {name}"));
        }
        if !jobs.contains(name) {
            gaps.push(format!("ci job {name} is not in the workflow"));
        }
        if !needs.contains(name) {
            gaps.push(format!("ci job {name} is not required by the gate"));
        }
    }
    for name in &needs {
        if !floors.ci_jobs.iter().any(|job| job == name) {
            gaps.push(format!("ci job {name} is missing from the floor inventory"));
        }
    }
    gaps
}

fn gate_control_gaps(block: &str) -> Vec<String> {
    let mut job_continue = false;
    let mut step_continue = false;
    let mut step_if = false;
    for line in block.lines() {
        let trimmed = strip_yaml_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        let indent = line.len() - line.trim_start().len();
        if key.starts_with("continue-on-error:") {
            if indent > 4 {
                step_continue = true;
            } else {
                job_continue = true;
            }
        }
        if indent > 4 && key.starts_with("if:") {
            step_if = true;
        }
    }
    let mut gaps = Vec::new();
    if job_continue {
        gaps.push("gate job sets continue-on-error".to_owned());
    }
    if step_continue {
        gaps.push("gate step sets continue-on-error".to_owned());
    }
    if step_if {
        gaps.push("gate step sets if:".to_owned());
    }
    gaps
}

fn block_sets_continue_on_error(block: &str) -> bool {
    block.lines().any(|line| {
        let trimmed = strip_yaml_comment(line).trim();
        let key = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        key.starts_with("continue-on-error:")
    })
}

fn ci_job_ids(text: &str) -> Result<BTreeSet<String>, String> {
    let lines: Vec<&str> = text.lines().collect();
    if jobs_region(&lines).is_none() {
        return Err("ci workflow has no jobs".to_owned());
    }
    let jobs: BTreeSet<String> = job_key_lines(&lines)
        .into_iter()
        .map(|(_, id)| id)
        .collect();
    if jobs.is_empty() {
        return Err("ci workflow has no jobs".to_owned());
    }
    Ok(jobs)
}

fn gate_job_block(text: &str) -> Result<&str, String> {
    job_block(text, "gate").ok_or_else(|| "ci.yml has no gate job".to_owned())
}

fn job_block<'a>(text: &'a str, id: &str) -> Option<&'a str> {
    let lines: Vec<&str> = text.lines().collect();
    let (_, jobs_end) = jobs_region(&lines)?;
    let keys = job_key_lines(&lines);
    let start_pos = keys.iter().position(|(_, key)| key == id)?;
    let start = keys[start_pos].0;
    let end = keys
        .get(start_pos + 1)
        .map(|(idx, _)| *idx)
        .unwrap_or(jobs_end);
    Some(&text[line_offset(text, start)..line_offset(text, end)])
}

fn jobs_region(lines: &[&str]) -> Option<(usize, usize)> {
    let jobs_at = lines
        .iter()
        .position(|line| is_top_level(line) && strip_yaml_comment(line).trim() == "jobs:")?;
    let mut jobs_end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(jobs_at + 1) {
        if is_top_level(line) {
            jobs_end = idx;
            break;
        }
    }
    Some((jobs_at, jobs_end))
}

fn job_key_lines(lines: &[&str]) -> Vec<(usize, String)> {
    let Some((jobs_at, jobs_end)) = jobs_region(lines) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut block = RunBlockScan::Outside;
    for (idx, line) in lines.iter().enumerate().take(jobs_end).skip(jobs_at + 1) {
        if line_inside_block(&mut block, line) {
            continue;
        }
        if let Some(next) = open_run_block(line) {
            block = next;
            continue;
        }
        if let Some(id) = job_id(line) {
            found.push((idx, id));
        }
    }
    found
}

enum RunBlockScan {
    Outside,
    /// Parent indent of an auto-detected block scalar. Content must be
    /// indented strictly past that parent; otherwise the block is empty.
    NeedIndent(usize),
    Inside(usize),
}

/// YAML block scalars end by indentation. An explicit `|n` or `>n` (the
/// digit and the chomp flag may be in either order) means the parent indent
/// plus n. A following line at a smaller indent, including a job key after
/// an empty `|`, is outside the block.
fn open_run_block(line: &str) -> Option<RunBlockScan> {
    let extra = block_scalar_content_indent(line)?;
    let parent = block_scalar_parent_indent(line);
    Some(match extra {
        Some(extra) => RunBlockScan::Inside(parent.saturating_add(extra)),
        None => RunBlockScan::NeedIndent(parent),
    })
}

fn block_scalar_parent_indent(line: &str) -> usize {
    let (indent, _) = leading_indent(line);
    let rest = line.get(indent..).unwrap_or("");
    if rest.starts_with("- ") {
        indent.saturating_add(2)
    } else {
        indent
    }
}

fn line_inside_block(block: &mut RunBlockScan, line: &str) -> bool {
    match *block {
        RunBlockScan::Outside => false,
        RunBlockScan::NeedIndent(parent) => {
            if strip_yaml_comment(line).trim().is_empty() {
                return true;
            }
            let (indent, _) = leading_indent(line);
            if indent > parent {
                *block = RunBlockScan::Inside(indent);
                true
            } else {
                *block = RunBlockScan::Outside;
                false
            }
        }
        RunBlockScan::Inside(min) => {
            if strip_yaml_comment(line).trim().is_empty() {
                return true;
            }
            let (indent, _) = leading_indent(line);
            if indent >= min {
                true
            } else {
                *block = RunBlockScan::Outside;
                false
            }
        }
    }
}

fn block_scalar_content_indent(line: &str) -> Option<Option<usize>> {
    let trimmed = strip_yaml_comment(line).trim();
    if trimmed.is_empty() {
        return None;
    }
    let body = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let (_, value) = split_yaml_key(body)?;
    let value = value.trim();
    if block_scalar_header(value) {
        return Some(None);
    }
    if indented_block_scalar(value) {
        return explicit_block_indent(value).map(Some);
    }
    None
}

fn explicit_block_indent(value: &str) -> Option<usize> {
    let digits: String = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    let extra = digits.parse::<usize>().ok()?;
    if (1..=9).contains(&extra) {
        Some(extra)
    } else {
        None
    }
}

fn gate_named_step<'a>(block: &'a str, header: &str) -> &'a str {
    let Some(start) = block.find(header) else {
        return "";
    };
    let rest = &block[start..];
    let end = rest.find("\n      - ").map_or(rest.len(), |idx| idx + 1);
    &rest[..end]
}

fn gate_check_step(block: &str) -> &str {
    gate_named_step(
        block,
        "      - name: fail unless every needed job succeeded\n",
    )
}

fn gate_fixture_step(block: &str) -> &str {
    gate_named_step(
        block,
        "      - name: non-success gate needs fail and success passes\n",
    )
}

fn gate_needs(block: &str) -> Result<BTreeSet<String>, String> {
    let lines: Vec<&str> = block.lines().collect();
    let Some(idx) = lines
        .iter()
        .position(|line| strip_yaml_comment(line).trim().starts_with("needs:"))
    else {
        return Err("gate job has no needs".to_owned());
    };
    let rest = strip_yaml_comment(lines[idx])
        .trim()
        .strip_prefix("needs:")
        .ok_or_else(|| "gate job has no needs".to_owned())?
        .trim();
    if rest.is_empty() {
        let mut names = BTreeSet::new();
        for line in lines.iter().skip(idx + 1) {
            let item = strip_yaml_comment(line).trim();
            let Some(name) = item.strip_prefix("- ") else {
                break;
            };
            let name = name.trim().trim_matches(|ch| ch == '"' || ch == '\'');
            if name.is_empty() {
                return Err("gate job has no needs".to_owned());
            }
            names.insert(name.to_owned());
        }
        if names.is_empty() {
            return Err("gate job has no needs".to_owned());
        }
        return Ok(names);
    }
    if !rest.starts_with('[') || !rest.ends_with(']') {
        return Err("gate needs must be a list".to_owned());
    }
    let inner = &rest[1..rest.len() - 1];
    let names = inner
        .split(',')
        .map(|item| item.trim().trim_matches(|ch| ch == '"' || ch == '\''))
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();
    Ok(names)
}

fn job_id(line: &str) -> Option<String> {
    if !line.starts_with("  ") || line.starts_with("   ") {
        return None;
    }
    let id = strip_yaml_comment(line).trim().strip_suffix(':')?;
    if id.is_empty() || id.contains([' ', ':']) {
        None
    } else {
        Some(id.to_owned())
    }
}

fn is_top_level(line: &str) -> bool {
    let code = strip_yaml_comment(line);
    !code.trim().is_empty() && !line.starts_with(' ') && !line.starts_with('#')
}

fn line_offset(text: &str, line_idx: usize) -> usize {
    let mut offset = 0;
    for (idx, line) in text.lines().enumerate() {
        if idx == line_idx {
            return offset;
        }
        offset += line.len();
        if text.as_bytes().get(offset) == Some(&b'\n') {
            offset += 1;
        }
    }
    text.len()
}

fn strip_yaml_comment(line: &str) -> &str {
    let mut quote = None;
    let mut prev_space = true;
    let mut chars = line.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if let Some(open) = quote {
            if yaml_escape_skips(&mut chars, open, ch) {
                prev_space = false;
                continue;
            }
            if ch == open {
                quote = None;
            }
            prev_space = false;
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                prev_space = false;
            }
            '#' if prev_space => return line[..idx].trim_end(),
            _ => prev_space = ch.is_whitespace(),
        }
    }
    line.trim_end()
}

fn yaml_escape_skips(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    open: char,
    ch: char,
) -> bool {
    if open == '"' && ch == '\\' {
        chars.next();
        return true;
    }
    if open == '\'' && ch == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
        chars.next();
        return true;
    }
    false
}

fn finish(violations: Result<Vec<String>, String>) -> Result<(), String> {
    let violations = violations?;
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("\n"))
    }
}

fn feature_ownership(
    workspace: &Path,
    scan_root: &Path,
    check_sources: bool,
) -> Result<Vec<String>, String> {
    let policy: FeaturePolicy = toml::from_str(&read(&workspace.join("policy/features.toml"))?)
        .map_err(|err| format!("parse policy/features.toml: {err}"))?;
    let mut violations = Vec::new();
    let mut claimed = BTreeSet::new();
    for runner in &policy.runner {
        let matched = expand_glob(scan_root, &runner.glob)?;
        if matched.is_empty() && scan_root == workspace {
            violations.push(format!(
                "runner {} glob {} matched no features",
                runner.name, runner.glob
            ));
        }
        for path in &matched {
            let rel = rel_path(scan_root, path)?;
            if !claimed.insert(rel.clone()) {
                violations.push(format!("feature {rel} matches more than one runner"));
            }
            if let Err(err) = parse_feature(&read(path)?) {
                violations.push(format!("malformed feature {rel}: {err}"));
            }
        }
        if check_sources {
            violations.extend(runner_source(workspace, runner)?);
        }
    }
    let specs = scan_root.join("specs/features");
    if !specs.is_dir() {
        violations.push(format!(
            "{} is missing",
            rel_path(scan_root, &specs).unwrap_or_else(|_| "specs/features".to_owned())
        ));
        return Ok(violations);
    }
    for path in walk_features(&specs)? {
        let rel = rel_path(scan_root, &path)?;
        if !claimed.contains(&rel) {
            violations.push(format!("unowned feature {rel}"));
        }
    }
    Ok(violations)
}

fn runner_source(workspace: &Path, runner: &RunnerPolicy) -> Result<Vec<String>, String> {
    let code = included_sources(workspace, &runner.source)?;
    let mut violations = Vec::new();
    if code.contains("PARANGSAE_CUCUMBER_FEATURES") {
        violations.push(format!(
            "{} reads PARANGSAE_CUCUMBER_FEATURES",
            runner.source
        ));
    }
    if !runner_runs_glob(&code, &runner.glob) {
        violations.push(format!("{} does not run {}", runner.source, runner.glob));
    }
    Ok(violations)
}

/// Main's runners call `run_and_exit` on the feature path. A runner may also
/// name the glob in `FEATURE_GLOB` and refuse skipped scenarios.
fn runner_runs_glob(code: &str, glob: &str) -> bool {
    let stack = code.contains(&format!("const FEATURE_GLOB: &str = \"{glob}\";"))
        && code.contains("expand_feature_glob(FEATURE_GLOB)")
        && code.contains(".fail_on_skipped()")
        && code.contains("execution_has_failed()");
    if stack {
        return true;
    }
    let path = glob.split("/**").next().unwrap_or(glob);
    code.contains("run_and_exit") && code.contains(path)
}

fn included_sources(workspace: &Path, source_rel: &str) -> Result<String, String> {
    let mut combined = String::new();
    let mut queue = vec![workspace.join(source_rel)];
    let mut seen = BTreeSet::new();
    while let Some(path) = queue.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let text = read(&path)?;
        for include in path_attributes(&text) {
            let parent = path
                .parent()
                .ok_or_else(|| format!("{} has no parent", path.display()))?;
            queue.push(parent.join(include));
        }
        combined.push_str(&text);
        combined.push('\n');
    }
    Ok(strip_line_comments(&combined))
}

fn path_attributes(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("#[path = \"") else {
            continue;
        };
        let Some(rel) = rest.strip_suffix("\"]") else {
            continue;
        };
        found.push(rel.to_owned());
    }
    found
}

fn tla_models(root: &Path) -> Result<Vec<String>, String> {
    let floors = load_floors(&root.join("regressions/floors.toml"))?;
    let mut violations = Vec::new();
    let mut found = Vec::new();
    for (model, tla_path, cfg_path) in tla_model_paths(root)? {
        found.push(model.clone());
        if !cfg_path.is_file() {
            violations.push(format!("{model}.tla has no cfg"));
            continue;
        }
        let tla = read(&tla_path)?;
        let cfg = read(&cfg_path)?;
        for name in uncovered_invariants(&tla, &cfg) {
            violations.push(format!("{model} missing from cfg: {name}"));
        }
        violations.extend(floor_invariant_gaps(&model, &tla, &cfg, &floors));
    }
    if found.is_empty() {
        violations.push("no TLC models under specs/tla".to_owned());
    }
    Ok(violations)
}

fn tla_model_paths(root: &Path) -> Result<Vec<(String, PathBuf, PathBuf)>, String> {
    let dir = root.join("specs/tla");
    let entries =
        std::fs::read_dir(&dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    let mut models = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(model) = name.strip_suffix(".tla") else {
            continue;
        };
        let cfg = dir.join(format!("{model}.cfg"));
        models.push((model.to_owned(), path, cfg));
    }
    models.sort();
    Ok(models)
}

/// Parameterless boolean or temporal operators must be an `INVARIANT` or
/// `PROPERTY` in the cfg. Actions (`Name(...)`), `Init`, `Next`, `Spec`,
/// `vars`, and set aliases such as `Ids == 1..Max` are not checks.
fn uncovered_invariants(tla: &str, cfg: &str) -> Vec<String> {
    let defined = invariant_names(tla);
    let listed = cfg_checks(cfg);
    defined.difference(&listed).cloned().collect()
}

fn floor_invariant_gaps(model: &str, tla: &str, cfg: &str, floors: &Floors) -> Vec<String> {
    let mut violations = Vec::new();
    let Some(required) = floors.invariants.get(model) else {
        violations.push(format!("missing floor invariants for {model}"));
        return violations;
    };
    if required.is_empty() {
        violations.push(format!("missing floor invariants for {model}"));
        return violations;
    }
    let defined = invariant_names(tla);
    let listed = cfg_checks(cfg);
    for name in required {
        if !defined.contains(name) {
            violations.push(format!(
                "floor invariant {name} is not defined in {model}.tla"
            ));
        }
        if !listed.contains(name) {
            violations.push(format!(
                "floor invariant {name} is missing from {model}.cfg"
            ));
        }
    }
    violations
}

fn invariant_names(tla: &str) -> BTreeSet<String> {
    let stripped: Vec<String> = tla
        .lines()
        .map(|line| match line.find("\\*") {
            Some(index) => line[..index].to_owned(),
            None => line.to_owned(),
        })
        .collect();
    let mut starts = Vec::new();
    for (index, line) in stripped.iter().enumerate() {
        if let Some((name, has_params, _)) = definition_at(line) {
            starts.push((index, name, has_params));
        }
    }
    let mut names = BTreeSet::new();
    for (position, (index, name, has_params)) in starts.iter().enumerate() {
        if *has_params || matches!(name.as_str(), "Init" | "Next" | "Spec" | "vars") {
            continue;
        }
        let end = starts
            .get(position + 1)
            .map(|(next, _, _)| *next)
            .unwrap_or(stripped.len());
        let body = stripped[*index..end].join("\n");
        let body = body.split_once("==").map(|(_, rest)| rest).unwrap_or("");
        if is_check_body(body) {
            names.insert(name.clone());
        }
    }
    names
}

fn definition_at(line: &str) -> Option<(String, bool, &str)> {
    let mut chars = line.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    let mut name = String::from(first);
    for ch in chars {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            name.push(ch);
        } else {
            break;
        }
    }
    let rest = line[name.len()..].trim_start();
    let (has_params, after) = if let Some(stripped) = rest.strip_prefix('(') {
        let end = stripped.find(')')?;
        (true, stripped[end + 1..].trim_start())
    } else {
        (false, rest)
    };
    let body = after.strip_prefix("==")?;
    Some((name, has_params, body))
}

fn is_check_body(body: &str) -> bool {
    [
        "==", "=>", "<=>", "/\\", "\\/", "\\A", "\\E", "[]", "<>", "TRUE", "FALSE", "=",
    ]
    .iter()
    .any(|token| body.contains(token))
}

fn cfg_checks(cfg: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in cfg.lines() {
        let line = match line.find("\\*") {
            Some(index) => &line[..index],
            None => line,
        };
        let mut parts = line.split_whitespace();
        let kind = parts.next();
        let name = parts.next();
        if matches!(kind, Some("INVARIANT" | "PROPERTY")) {
            if let Some(name) = name {
                names.insert(name.to_owned());
            }
        }
    }
    names
}

fn distinct_states(output: &str) -> Result<u64, String> {
    if !output.contains("Model checking completed. No error has been found.") {
        return Err("tlc output has no successful completion line".to_owned());
    }
    let mut found = None;
    for line in output.lines() {
        let Some(index) = line.find(" distinct states found") else {
            continue;
        };
        let head = line[..index].trim();
        let digits: String = head
            .chars()
            .rev()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if digits.is_empty() {
            return Err(format!("tlc distinct-state line has no count: {line}"));
        }
        found = Some(
            digits
                .parse()
                .map_err(|_| format!("tlc distinct-state count is not an integer: {digits}"))?,
        );
    }
    found.ok_or_else(|| "tlc output has no distinct-state count".to_owned())
}

fn kani_violations(output: &str, floors: &Floors) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    for line in output.lines() {
        let Some(start) = line.find("Checking harness ") else {
            continue;
        };
        let rest = &line[start + "Checking harness ".len()..];
        let name = rest.split("...").next().unwrap_or(rest).trim();
        if name.is_empty() {
            return Err(format!("kani harness line has no name: {line}"));
        }
        if found.iter().any(|existing: &String| existing == name) {
            return Err(format!("kani harness {name} was reported twice"));
        }
        found.push(name.to_owned());
    }
    let summary = output.lines().rev().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("Complete - ")?;
        let (verified, rest) = rest.split_once(" successfully verified harnesses, ")?;
        let (failures, rest) = rest.split_once(" failures, ")?;
        let total = rest.strip_suffix(" total.")?;
        Some((verified, failures, total))
    });
    let Some((verified, failures, total)) = summary else {
        return Err("kani output has no harness summary".to_owned());
    };
    let verified: u64 = verified
        .parse()
        .map_err(|_| format!("kani verified count is not an integer: {verified}"))?;
    let failures: u64 = failures
        .parse()
        .map_err(|_| format!("kani failure count is not an integer: {failures}"))?;
    let total: u64 = total
        .parse()
        .map_err(|_| format!("kani total count is not an integer: {total}"))?;
    let mut violations = Vec::new();
    if failures != 0 {
        violations.push(format!("kani failures {failures}"));
    }
    if verified.saturating_add(failures) != total {
        violations.push(format!("kani summary {verified} + {failures} != {total}"));
    }
    let minimum = floors.value("kani.min_harnesses")?;
    if verified < minimum {
        violations.push(format!(
            "kani harness count {verified} is below floor {minimum}"
        ));
    }
    let found_count = u64::try_from(found.len()).unwrap_or(u64::MAX);
    if found_count != verified.saturating_add(failures) {
        violations.push(format!(
            "kani checked {found_count} harnesses but the summary total is {}",
            verified.saturating_add(failures)
        ));
    }
    for name in &floors.kani_names {
        if !found.iter().any(|seen| harness_name_matches(seen, name)) {
            violations.push(format!("missing kani harness {name}"));
        }
    }
    for name in &found {
        if !floors
            .kani_names
            .iter()
            .any(|expected| harness_name_matches(name, expected))
        {
            violations.push(format!("unexpected kani harness {name}"));
        }
    }
    Ok(violations)
}

/// Kani prints `fixed::proofs::notional_rejects_negative`. The floor lists the
/// function name.
fn harness_name_matches(found: &str, expected: &str) -> bool {
    found == expected || found.ends_with(&format!("::{expected}"))
}

fn is_ceiling(key: &str) -> bool {
    key.ends_with("max_skipped") || key == "lint.suppressions"
}

fn ratchet(current: &Floors, base: &Floors) -> Vec<String> {
    let mut violations = Vec::new();
    for (key, base_value) in &base.values {
        match current.values.get(key) {
            None => violations.push(format!("floor {key} deleted")),
            Some(value) if is_ceiling(key) && value > base_value => {
                violations.push(format!(
                    "floor {key} increased from {base_value} to {value}"
                ));
            }
            Some(value) if !is_ceiling(key) && value < base_value => {
                violations.push(format!(
                    "floor {key} decreased from {base_value} to {value}"
                ));
            }
            Some(_) => {}
        }
    }
    for name in &base.kani_names {
        if !current
            .kani_names
            .iter()
            .any(|current_name| current_name == name)
        {
            violations.push(format!("kani harness {name} deleted"));
        }
    }
    for name in &base.ci_jobs {
        if !current
            .ci_jobs
            .iter()
            .any(|current_name| current_name == name)
        {
            violations.push(format!("ci job {name} deleted"));
        }
    }
    for (model, names) in &base.invariants {
        let current_names = current.invariants.get(model);
        for name in names {
            let kept = current_names
                .is_some_and(|current_names| current_names.iter().any(|item| item == name));
            if !kept {
                violations.push(format!("invariant {model}.{name} deleted"));
            }
        }
    }
    violations
}

fn required_floor_keys(floors: &Floors) -> Vec<String> {
    let mut violations = Vec::new();
    for key in [
        "cucumber.kline.scenarios",
        "cucumber.kline.steps",
        "cucumber.kline.max_skipped",
        "cucumber.vision.scenarios",
        "cucumber.vision.steps",
        "cucumber.vision.max_skipped",
        "cucumber.instrument_rules.scenarios",
        "cucumber.instrument_rules.steps",
        "cucumber.instrument_rules.max_skipped",
        "tlc.IngestSegment.distinct_states",
        "tlc.ManifestWrite.distinct_states",
        "kani.min_harnesses",
        "lint.suppressions",
        "proptest.count",
        "tests.count",
        "agents.rule_rows",
    ] {
        if !floors.values.contains_key(key) {
            violations.push(format!("missing floor {key}"));
        }
    }
    if floors.kani_names.is_empty() {
        violations.push("missing kani harness names".to_owned());
    }
    if let Some(minimum) = floors.values.get("kani.min_harnesses") {
        if u64::try_from(floors.kani_names.len()).ok() != Some(*minimum) {
            violations.push(format!(
                "kani.min_harnesses is {minimum} but {} names are listed",
                floors.kani_names.len()
            ));
        }
    }
    violations
}

fn b2_violations(root: &Path, floors: &Floors) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    let suppressions = suppression_count(root)?;
    let ceiling = floors.value("lint.suppressions")?;
    if suppressions > ceiling {
        violations.push(format!(
            "lint suppressions {suppressions} exceed ceiling {ceiling}"
        ));
    }
    push_minimum(
        &mut violations,
        floors,
        "proptest.count",
        count_needles(root, "proptest!")?,
    )?;
    push_minimum(
        &mut violations,
        floors,
        "tests.count",
        count_needles(root, "#[test]")?,
    )?;
    push_minimum(
        &mut violations,
        floors,
        "agents.rule_rows",
        rule_rows(&read(&root.join("AGENTS.md"))?)?,
    )?;
    Ok(violations)
}

fn push_minimum(
    violations: &mut Vec<String>,
    floors: &Floors,
    key: &str,
    found: u64,
) -> Result<(), String> {
    let minimum = floors.value(key)?;
    if found < minimum {
        violations.push(format!("floor {key} measured {found} is below {minimum}"));
    }
    Ok(())
}

fn cucumber_count_violations(root: &Path, floors: &Floors) -> Result<Vec<String>, String> {
    let policy: FeaturePolicy = toml::from_str(&read(&root.join("policy/features.toml"))?)
        .map_err(|err| format!("parse policy/features.toml: {err}"))?;
    let mut violations = Vec::new();
    for runner in &policy.runner {
        let files = expand_glob(root, &runner.glob)?;
        let (scenarios, steps) = gherkin_file_counts(&files)?;
        let scenario_key = format!("cucumber.{}.scenarios", runner.name);
        let step_key = format!("cucumber.{}.steps", runner.name);
        let min_scenarios = floors.value(&scenario_key)?;
        let min_steps = floors.value(&step_key)?;
        if u64::try_from(scenarios).unwrap_or(u64::MAX) < min_scenarios {
            violations.push(format!(
                "floor {scenario_key} measured {scenarios} is below {min_scenarios}"
            ));
        }
        if u64::try_from(steps).unwrap_or(u64::MAX) < min_steps {
            violations.push(format!(
                "floor {step_key} measured {steps} is below {min_steps}"
            ));
        }
    }
    let ci_text = read(&root.join(".github/workflows/ci.yml"))?;
    violations.extend(cucumber_runner_contract_gaps(root, &ci_text)?);
    Ok(violations)
}

fn gherkin_file_counts(files: &[PathBuf]) -> Result<(usize, usize), String> {
    let mut scenarios = 0usize;
    let mut steps = 0usize;
    for path in files {
        let (file_scenarios, file_steps) = parse_feature(&read(path)?)
            .map_err(|err| format!("malformed feature {}: {err}", path.display()))?;
        scenarios += file_scenarios;
        steps += file_steps;
    }
    Ok((scenarios, steps))
}

fn parse_feature(text: &str) -> Result<(usize, usize), String> {
    let feature = gherkin::Feature::parse(text, gherkin::GherkinEnv::default())
        .map_err(|err| err.to_string())?;
    let mut scenarios = feature.scenarios.len();
    let mut steps = background_steps(feature.background.as_ref());
    for scenario in &feature.scenarios {
        steps += scenario.steps.len();
    }
    for rule in &feature.rules {
        scenarios += rule.scenarios.len();
        steps += background_steps(rule.background.as_ref());
        for scenario in &rule.scenarios {
            steps += scenario.steps.len();
        }
    }
    Ok((scenarios, steps))
}

fn background_steps(background: Option<&gherkin::Background>) -> usize {
    match background {
        Some(background) => background.steps.len(),
        None => 0,
    }
}

fn load_floors(path: &Path) -> Result<Floors, String> {
    parse_floors(&read(path)?)
}

fn parse_floors(text: &str) -> Result<Floors, String> {
    let mut values = BTreeMap::new();
    let mut kani_names = Vec::new();
    let mut invariants: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut ci_jobs = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("floor line {} is not key = value", index + 1));
        };
        let key = key.trim();
        let value = value.trim();
        if key == "kani.name" {
            if kani_names.iter().any(|name: &String| name == value) {
                return Err(format!("duplicate kani harness {value}"));
            }
            kani_names.push(value.to_owned());
            continue;
        }
        if key == "ci.job" {
            if ci_jobs.iter().any(|name: &String| name == value) {
                return Err(format!("duplicate ci job {value}"));
            }
            ci_jobs.push(value.to_owned());
            continue;
        }
        if let Some(model) = invariant_key(key) {
            let names = invariants.entry(model.to_owned()).or_default();
            if names.iter().any(|name| name == value) {
                return Err(format!("duplicate invariant {model}.{value}"));
            }
            names.push(value.to_owned());
            continue;
        }
        let parsed = value
            .parse()
            .map_err(|_| format!("floor {key} is not an integer"))?;
        if values.insert(key.to_owned(), parsed).is_some() {
            return Err(format!("duplicate floor {key}"));
        }
    }
    Ok(Floors {
        values,
        kani_names,
        invariants,
        ci_jobs,
    })
}

fn invariant_key(key: &str) -> Option<&str> {
    let rest = key.strip_prefix("tlc.")?;
    let model = rest.strip_suffix(".invariant")?;
    if model.is_empty() || model.contains('.') {
        None
    } else {
        Some(model)
    }
}

impl Floors {
    fn value(&self, key: &str) -> Result<u64, String> {
        self.values
            .get(key)
            .copied()
            .ok_or_else(|| format!("missing floor {key}"))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum BaselineChoice {
    Unreadable,
    DirectPush,
    MergeBase,
    Sha(String),
}

fn filtered_cargo_test_gaps(name: &str, text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    for step in job_steps(text) {
        if step_runs_filtered_cargo_test(&step) && !step_asserts_passed_count(&step) {
            gaps.push(format!(
                "{name} filtered cargo test does not assert the passed count"
            ));
        }
    }
    gaps
}

fn job_steps(text: &str) -> Vec<String> {
    let mut steps = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    let mut block = RunBlockScan::Outside;
    for line in text.lines() {
        let inside = line_inside_block(&mut block, line);
        if !inside && line.starts_with("      - ") {
            if let Some(done) = current.take() {
                steps.push(done.join("\n"));
            }
            current = Some(vec![line]);
            if let Some(next) = open_run_block(line) {
                block = next;
            }
            continue;
        }
        if !inside && (job_id(line).is_some() || is_top_level(line)) {
            if let Some(done) = current.take() {
                steps.push(done.join("\n"));
            }
        }
        if !inside {
            if let Some(next) = open_run_block(line) {
                block = next;
            }
        }
        if let Some(step) = current.as_mut() {
            step.push(line);
        }
    }
    if let Some(done) = current {
        steps.push(done.join("\n"));
    }
    steps
}

fn step_runs_filtered_cargo_test(step: &str) -> bool {
    cargo_test_arg_lists(step)
        .iter()
        .any(|args| cargo_test_args_are_filtered(args))
}

fn step_asserts_passed_count(step: &str) -> bool {
    step.lines().any(line_asserts_at_least_one_pass)
}

fn line_asserts_at_least_one_pass(line: &str) -> bool {
    let Some((_, rest)) = line.split_once("test result: ok. ") else {
        return false;
    };
    let Some((count, _)) = rest.split_once(" passed") else {
        return false;
    };
    let Ok(value) = count.trim().parse::<u32>() else {
        return false;
    };
    value >= 1
}

fn compiling_tool_env_gaps(name: &str, text: &str) -> Vec<String> {
    let Ok(jobs) = ci_job_ids(text) else {
        return Vec::new();
    };
    let mut gaps = Vec::new();
    for job in jobs {
        let Some(block) = job_block(text, &job) else {
            continue;
        };
        if !job_invokes_cargo_or_rustc(block) {
            continue;
        }
        for script in run_scripts(block) {
            for line in script.lines() {
                let words = shell_words(line);
                let mut exported = Vec::new();
                for (idx, word) in words.iter().enumerate() {
                    if *word != "export" {
                        continue;
                    }
                    let mut cursor = idx + 1;
                    while words.get(cursor).is_some_and(|flag| {
                        flag.starts_with('-') && assignment_name(flag).is_none()
                    }) {
                        cursor += 1;
                    }
                    let Some(operand) = words.get(cursor) else {
                        continue;
                    };
                    let Some(key) = export_name(operand) else {
                        continue;
                    };
                    if !tool_env_key(key) {
                        continue;
                    }
                    push_gap(&mut gaps, format!("{name} job {job} exports {key}"));
                    exported.push(cursor);
                }
                for (key, value) in line_tool_assignments(line) {
                    if exported.iter().any(|idx| {
                        words
                            .get(*idx)
                            .and_then(|word| export_name(word))
                            .is_some_and(|exported| exported == key)
                    }) {
                        continue;
                    }
                    if pinned_tool_assignment(line, &key, &value) {
                        continue;
                    }
                    push_gap(&mut gaps, format!("{name} job {job} assigns {key}={value}"));
                }
            }
        }
    }
    gaps
}

fn tool_env_key(key: &str) -> bool {
    matches!(key, "CARGO" | "RUSTC" | "RUSTFLAGS")
}

fn pinned_tool_assignment(line: &str, key: &str, value: &str) -> bool {
    env_i_assignment_line(line) && pinned_tool_path(key) == Some(value)
}

fn pinned_tool_path(key: &str) -> Option<&'static str> {
    const CARGO: &str = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo";
    const RUSTC: &str = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc";
    match key {
        "CARGO" => Some(CARGO),
        "RUSTC" => Some(RUSTC),
        _ => None,
    }
}

fn env_i_assignment_line(line: &str) -> bool {
    let words = shell_words(line);
    !words.is_empty()
        && words.iter().all(|word| {
            *word == "env" || *word == "-i" || *word == "\\" || assignment_name(word).is_some()
        })
}

fn line_tool_assignments(line: &str) -> Vec<(String, String)> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut idx = 0usize;
    let mut quote = None;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if let Some(open) = quote {
            if byte == open {
                quote = None;
            }
            idx += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            idx += 1;
            continue;
        }
        let boundary = idx == 0 || is_assign_boundary(bytes[idx - 1]);
        if boundary && is_env_start(byte) {
            let start = idx;
            idx += 1;
            while idx < bytes.len() && is_env_continue(bytes[idx]) {
                idx += 1;
            }
            if bytes.get(idx) == Some(&b'=') {
                let key = &line[start..idx];
                idx += 1;
                if tool_env_key(key) {
                    let value_at = idx;
                    while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                        idx += 1;
                    }
                    let value = line[value_at..idx].trim_end_matches('\\');
                    if !found.iter().any(|(found_key, _)| found_key == key) {
                        found.push((key.to_owned(), value.to_owned()));
                    }
                }
                continue;
            }
            continue;
        }
        idx += 1;
    }
    found
}

fn cargo_test_arg_lists(step: &str) -> Vec<Vec<String>> {
    let bytes = step.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if word_at(bytes, i, b"cargo") {
            let mut j = i + 5;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if word_at(bytes, j, b"test") {
                let (args, next) = cargo_test_shell_words(bytes, j + 4);
                found.push(args);
                i = next;
                continue;
            }
        }
        i += 1;
    }
    found
}

fn word_at(bytes: &[u8], i: usize, word: &[u8]) -> bool {
    if i > 0 && is_word_byte(bytes[i - 1]) {
        return false;
    }
    let Some(end) = i.checked_add(word.len()) else {
        return false;
    };
    if bytes.get(i..end) != Some(word) {
        return false;
    }
    !bytes.get(end).is_some_and(|byte| is_word_byte(*byte))
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn cargo_test_shell_words(bytes: &[u8], mut i: usize) -> (Vec<String>, usize) {
    let mut words = Vec::new();
    loop {
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        match bytes[i] {
            b'\n' | b';' | b'|' | b'&' | b'(' | b')' | b'`' | b'<' | b'>' | b'#' => break,
            b'\'' | b'"' => {
                let (word, next) = cargo_test_quoted_word(bytes, i);
                words.push(word);
                i = next;
            }
            _ => {
                let start = i;
                while i < bytes.len() {
                    match bytes[i] {
                        b' ' | b'\t' | b'\n' | b';' | b'|' | b'&' | b'(' | b')' | b'`' | b'<'
                        | b'>' | b'#' | b'\'' | b'"' => break,
                        _ => i += 1,
                    }
                }
                words.push(String::from_utf8_lossy(&bytes[start..i]).into_owned());
            }
        }
    }
    (words, i)
}

fn cargo_test_quoted_word(bytes: &[u8], start: usize) -> (String, usize) {
    let quote = bytes[start];
    let mut i = start + 1;
    let mut word = String::new();
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == quote {
            return (word, i + 1);
        }
        if quote == b'"' && byte == b'\\' && i + 1 < bytes.len() {
            word.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }
        word.push(byte as char);
        i += 1;
    }
    (word, i)
}

fn cargo_test_args_are_filtered(args: &[String]) -> bool {
    let mut idx = 0;
    while idx < args.len() {
        let arg = &args[idx];
        if arg == "--" {
            idx += 1;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, _)) => (name, true),
            None => (arg.as_str(), false),
        };
        if name == "--exact" {
            return true;
        }
        if name.starts_with('-') {
            if !inline && flag_takes_value(name) {
                idx += 2;
            } else {
                idx += 1;
            }
            continue;
        }
        return true;
    }
    false
}

fn flag_takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "-p" | "--package"
            | "--test"
            | "--bin"
            | "--example"
            | "--bench"
            | "--manifest-path"
            | "--features"
            | "-F"
            | "--exclude"
            | "--target"
            | "--target-dir"
            | "-j"
            | "--jobs"
            | "--color"
            | "--profile"
            | "--message-format"
            | "--config"
            | "--skip"
            | "--format"
            | "--logfile"
            | "--test-threads"
            | "-Z"
    )
}

fn workflow_permission_result(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    violations.extend(allowlisted_yml_without_pin_gaps(
        &allowed_workflow_paths(),
        ALLOWED_WORKFLOW_FILES,
    ));
    for path in workflow_dir_files(root)? {
        let name = workflow_display(root, &path);
        let text = read(&path)?;
        if workflow_file_pin(&name).is_none() {
            violations.push(format!("{name} is not an allowed workflow file"));
        } else {
            violations.extend(workflow_byte_pin_gaps(&name, &text));
        }
        if workflow_yaml(&path) {
            violations.extend(workflow_file_gaps(&name, &text));
            if name == ".github/workflows/ci.yml" {
                violations.extend(compiling_job_order_gaps(&text));
            } else {
                violations.extend(foreign_workflow_gaps(&name, &text));
                if name == ".github/workflows/vision-smoke.yml" {
                    if let Some(block) = job_block(&text, "smoke") {
                        violations.extend(compile_step_order_gaps("vision-smoke", block));
                    }
                }
            }
        } else {
            violations.extend(hidden_workflow_gaps(&name, &text));
        }
    }
    violations.extend(cargo_config_gaps(root)?);
    let ci = read(&root.join(".github/workflows/ci.yml"))?;
    violations.extend(required_job_pin_gaps(&ci));
    violations.extend(deny_release_pin_gaps(&ci));
    violations.extend(required_jobs_needs_gaps(&ci)?);
    for (name, pinned, _) in REQUIRED_JOBS {
        violations.extend(filtered_cargo_test_gaps(name, pinned));
    }
    if !guards_fetch_depth_is_zero(&ci) {
        violations.push("guards checkout fetch-depth is not 0".to_owned());
    }
    violations.extend(pinned_guards_steps(&ci));
    violations.extend(command_inventory_gaps(root, &ci)?);
    Ok(violations)
}

fn pinned_guards_steps(text: &str) -> Vec<String> {
    let Some(block) = job_block(text, "guards") else {
        return vec!["ci.yml has no guards job".to_owned()];
    };
    let mut gaps = Vec::new();
    if !block.contains(GUARDS_STEP) {
        gaps.push("guards step does not match".to_owned());
    }
    if !block.contains(ALL_ZERO_STEP) {
        gaps.push("all-zero before step does not match".to_owned());
    }
    if gaps.is_empty() && !guards_steps_exact(block) {
        gaps.push(
            "guards job steps are not checkout, toolchain, rustup-which, guards, all-zero"
                .to_owned(),
        );
    }
    gaps
}

fn guards_job_steps() -> String {
    format!(
        "    steps:\n      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4\n        with:\n          fetch-depth: 0\n      - uses: dtolnay/rust-toolchain@5059963b1731bf1580991b5f3b1eecfb5da406c0 # 1.98.0\n{RUSTUP_WHICH_STEP}{GUARDS_STEP}{ALL_ZERO_STEP}"
    )
}

fn guards_steps_exact(block: &str) -> bool {
    let expected = guards_job_steps();
    let Some(idx) = block.find(&expected) else {
        return false;
    };
    let before = &block[..idx];
    let after = &block[idx + expected.len()..];
    !before.lines().any(|line| line.starts_with("      - "))
        && !after.lines().any(|line| line.starts_with("      - "))
}

fn strip_env_allowlist(text: &str) -> String {
    let text = map_job_block(text, "guards", |block| block.replace(GUARDS_STEP, ""));
    let text = map_job_block(&text, "guard-fixtures", |block| {
        block.replace(GUARD_FIXTURE_UNREADABLE_STEP, "")
    });
    map_job_block(&text, "kani", |block| {
        let mut out = String::new();
        for line in block.split_inclusive('\n') {
            let bare = line.trim_end_matches(['\n', '\r']);
            if bare == KANI_RUSTUP_STABLE || bare == KANI_RUSTUP_NIGHTLY {
                continue;
            }
            out.push_str(line);
        }
        out
    })
}

fn map_job_block(text: &str, id: &str, edit: impl FnOnce(&str) -> String) -> String {
    let Some(block) = job_block(text, id) else {
        return text.to_owned();
    };
    let Some(idx) = text.find(block) else {
        return text.to_owned();
    };
    let cleaned = edit(block);
    let mut out = String::with_capacity(text.len() - block.len() + cleaned.len());
    out.push_str(&text[..idx]);
    out.push_str(&cleaned);
    out.push_str(&text[idx + block.len()..]);
    out
}

fn workflow_dir_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = root.join(".github/workflows");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn workflow_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn workflow_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("yml" | "yaml")
    )
}

fn workflow_file_gaps(name: &str, text: &str) -> Vec<String> {
    let mut gaps = strict_yaml_gaps(name, text);
    if !permissions_are_contents_read(text) {
        gaps.push(format!("{name} permissions are not contents: read"));
    }
    gaps.extend(job_write_permission_gaps(name, text));
    gaps.extend(trigger_gaps(name, text));
    gaps.extend(forbidden_env_gaps(name, text));
    gaps.extend(rustc_pin_gaps(name, text));
    gaps.extend(run_injection_gaps(name, text));
    gaps.extend(github_env_path_gaps(name, text));
    gaps.extend(compiling_tool_env_gaps(name, text));
    gaps.extend(required_job_shape_gaps(name, text));
    if name.ends_with("vision-smoke.yml") && !text.contains("day must be YYYY-MM-DD") {
        gaps.push(format!("{name} does not validate the day"));
    }
    gaps
}

fn hidden_workflow_gaps(name: &str, text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    for line in text.lines() {
        let code = strip_yaml_comment(line).trim();
        if code.starts_with("uses:")
            || code.starts_with("- uses:")
            || code.starts_with("permissions:")
            || code == "on:"
            || code.starts_with("on:")
        {
            gaps.push(format!("{name} carries workflow keys"));
            break;
        }
    }
    gaps
}

fn permissions_are_contents_read(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = false;
    for (idx, line) in lines.iter().enumerate() {
        if !is_top_level(line) {
            continue;
        }
        let code = strip_yaml_comment(line).trim();
        if code == "permissions: contents: read" {
            if found {
                return false;
            }
            found = true;
            continue;
        }
        if code == "permissions:" {
            if found {
                return false;
            }
            let mut keys = Vec::new();
            for follow in lines.iter().skip(idx + 1) {
                if is_top_level(follow) {
                    break;
                }
                let indent = follow.len() - follow.trim_start().len();
                let child = strip_yaml_comment(follow).trim();
                if child.is_empty() || indent != 2 {
                    continue;
                }
                keys.push(child);
            }
            if keys != ["contents: read"] {
                return false;
            }
            found = true;
            continue;
        }
        if code.starts_with("permissions:") {
            return false;
        }
    }
    found
}

fn job_write_permission_gaps(name: &str, text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    let mut job: Option<String> = None;
    let mut in_perms = false;
    let mut perms_indent = 0usize;
    for line in text.lines() {
        if is_top_level(line) {
            job = None;
            in_perms = false;
            continue;
        }
        if let Some(id) = job_id(line) {
            job = Some(id);
            in_perms = false;
            continue;
        }
        let Some(job_name) = job.as_deref() else {
            continue;
        };
        let indent = line.len() - line.trim_start().len();
        let trimmed = strip_yaml_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }
        if in_perms && indent <= perms_indent {
            in_perms = false;
        }
        let key = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        if !in_perms && indent == 4 && key.starts_with("permissions:") {
            if permission_line_grants_write(key) {
                gaps.push(format!("{name} job {job_name} permissions grant write"));
            }
            if key == "permissions:" {
                in_perms = true;
                perms_indent = 4;
            }
        } else if in_perms && permission_line_grants_write(key) {
            gaps.push(format!("{name} job {job_name} permissions grant write"));
        }
    }
    gaps
}

fn permission_line_grants_write(key: &str) -> bool {
    if key.contains("write-all") {
        return true;
    }
    let Some((_, value)) = key.split_once(':') else {
        return false;
    };
    let value = value.trim().trim_matches(['"', '\'']);
    value == "write" || value == "write-all"
}

fn foreign_workflow_gaps(name: &str, text: &str) -> Vec<String> {
    if name.ends_with("/ci.yml") {
        return Vec::new();
    }
    let mut gaps = Vec::new();
    if workflow_triggers(text) != ["workflow_dispatch".to_owned()] {
        gaps.push(format!("{name} must be workflow_dispatch only"));
    }
    if let Ok(jobs) = ci_job_ids(text) {
        for job in jobs {
            let display = job_block(text, &job).and_then(job_display_name);
            let reused = REQUIRED_JOBS
                .iter()
                .any(|(required, _, _)| *required == job || display.as_deref() == Some(*required));
            if reused {
                gaps.push(format!("{name} job {job} reuses required check name"));
            }
        }
    }
    for secret in secret_names(text) {
        if !ALLOWED_DEMO_SECRETS.contains(&secret.as_str()) {
            gaps.push(format!("{name} secret {secret} is not allowlisted"));
        }
    }
    gaps
}

fn job_display_name(block: &str) -> Option<String> {
    for line in block.lines().skip(1) {
        let (indent, _) = leading_indent(line);
        if indent <= 2 && !strip_yaml_comment(line).trim().is_empty() {
            break;
        }
        let trimmed = strip_yaml_comment(line).trim();
        if indent == 4 {
            if let Some(rest) = trimmed.strip_prefix("name:") {
                let name = rest.trim().trim_matches(['"', '\'']);
                if !name.is_empty() {
                    return Some(name.to_owned());
                }
            }
        }
    }
    None
}

fn secret_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(found) = rest.find("secrets.") {
        rest = &rest[found + "secrets.".len()..];
        let name: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn vision_smoke_pin(bytes: &[u8]) -> Vec<String> {
    pinned_whole_file_gaps(".github/workflows/vision-smoke.yml", bytes)
}

fn workflow_file_pin(path: &str) -> Option<WorkflowFilePin> {
    ALLOWED_WORKFLOW_FILES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, pin)| *pin)
}

fn allowed_workflow_paths() -> Vec<&'static str> {
    ALLOWED_WORKFLOW_FILES
        .iter()
        .map(|(path, _)| *path)
        .collect()
}

fn allowlisted_yml_without_pin_gaps(
    allowed: &[&str],
    pins: &[(&str, WorkflowFilePin)],
) -> Vec<String> {
    let mut gaps = Vec::new();
    for path in allowed {
        let yml = path.ends_with(".yml") || path.ends_with(".yaml");
        let pinned = pins.iter().any(|(name, _)| name == path);
        if yml && !pinned {
            gaps.push(format!("{path} is allowlisted without a path-keyed pin"));
        }
    }
    gaps
}

fn workflow_byte_pin_gaps(name: &str, text: &str) -> Vec<String> {
    match name {
        ".github/workflows/ci.yml" => ci_header_pin_gaps(text),
        ".github/workflows/vision-smoke.yml" => vision_smoke_pin(text.as_bytes()),
        ".github/workflows/gate_results.py" => gate_results_pin(text.as_bytes()),
        _ => match workflow_file_pin(name) {
            Some(WorkflowFilePin::Ci) => ci_header_pin_gaps(text),
            Some(WorkflowFilePin::WholeFile { .. }) => {
                pinned_whole_file_gaps(name, text.as_bytes())
            }
            None => Vec::new(),
        },
    }
}

fn pinned_whole_file_gaps(path: &str, bytes: &[u8]) -> Vec<String> {
    match workflow_file_pin(path) {
        Some(WorkflowFilePin::WholeFile { message, sha256 }) => {
            if sha256_hex(bytes) == sha256 {
                Vec::new()
            } else {
                vec![message.to_owned()]
            }
        }
        _ => vec![format!("{path} is allowlisted without a path-keyed pin")],
    }
}

fn compiling_job_order_gaps(text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    for (job, _, _) in REQUIRED_JOBS {
        // gate checks results. changes only diffs paths. Neither compiles.
        if *job == "gate" || *job == "changes" {
            continue;
        }
        let Some(block) = job_block(text, job) else {
            gaps.push(format!("ci.yml has no {job} job"));
            continue;
        };
        gaps.extend(compile_step_order_gaps(job, block));
        if *job == "tla" {
            gaps.extend(tla_workspace_cargo_gaps(block));
        }
    }
    gaps
}

fn compile_step_order_gaps(name: &str, block: &str) -> Vec<String> {
    let Some(which_at) = block.find(RUSTUP_WHICH_STEP) else {
        return which_assert_shape_gaps(name, block);
    };
    let mut gaps = Vec::new();
    if let Some(rustc_at) = block.find(RUSTC_198_STEP) {
        if which_at > rustc_at {
            gaps.push(format!("{name} runs rustc -V before rustup which"));
        }
    }
    if tool_token_before(block, which_at) {
        gaps.push(format!("{name} runs cargo or rustc before rustup which"));
    }
    gaps
}

fn which_assert_shape_gaps(name: &str, block: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    if which_assert_uses_glob(block) {
        gaps.push(format!("{name} which-assert uses a glob"));
    }
    if which_assert_uses_rustup_home(block) {
        gaps.push(format!("{name} which-assert uses RUSTUP_HOME"));
    }
    if gaps.is_empty() {
        gaps.push(format!("{name} has no rustup which step"));
    }
    gaps
}

fn which_assert_uses_glob(block: &str) -> bool {
    block
        .lines()
        .any(|line| line.contains("toolchains/") && (line.contains('*') || line.contains('?')))
}

fn which_assert_uses_rustup_home(block: &str) -> bool {
    block.contains("$RUSTUP_HOME") || block.contains("${RUSTUP_HOME")
}

fn tool_token_before(block: &str, which_at: usize) -> bool {
    let prefix = block[..which_at].replace(RUSTC_198_STEP, "");
    let mut token = String::new();
    let mut saw = false;
    for ch in prefix.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if token == "cargo" || token == "rustc" {
                saw = true;
            }
            token.clear();
        }
    }
    saw || token == "cargo" || token == "rustc"
}

fn tla_workspace_cargo_gaps(block: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    let mut steps = Vec::new();
    let mut current = String::new();
    for line in block.split_inclusive('\n') {
        if line.starts_with("      - ") && !current.is_empty() {
            steps.push(std::mem::take(&mut current));
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        steps.push(current);
    }
    for step in &steps {
        if step.contains("cd specs/tla") && step.contains("cargo") {
            gaps.push("tla step runs cargo after cd specs/tla".to_owned());
        }
    }
    let workspace_cargo = "cd \"$GITHUB_WORKSPACE\"\n          cargo xtask guards tlc-output";
    if block.matches(workspace_cargo).count() != 2 {
        gaps.push("tla cargo does not run from GITHUB_WORKSPACE".to_owned());
    }
    gaps
}

fn trigger_gaps(name: &str, text: &str) -> Vec<String> {
    let triggers = workflow_triggers(text);
    if triggers.is_empty() {
        return vec![format!("{name} has no allowed trigger")];
    }
    let mut gaps = Vec::new();
    for trigger in triggers {
        if !ALLOWED_TRIGGERS.contains(&trigger.as_str()) {
            gaps.push(format!("{name} trigger {trigger} is not allowed"));
        }
    }
    gaps
}

fn workflow_triggers(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut triggers = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if !is_top_level(line) {
            continue;
        }
        let code = strip_yaml_comment(line).trim();
        let Some(rest) = code.strip_prefix("on:") else {
            continue;
        };
        let rest = rest.trim();
        if rest.is_empty() {
            for follow in lines.iter().skip(idx + 1) {
                if is_top_level(follow) {
                    break;
                }
                let indent = follow.len() - follow.trim_start().len();
                if indent != 2 {
                    continue;
                }
                let child = strip_yaml_comment(follow).trim();
                let Some(trigger) = child.strip_suffix(':') else {
                    continue;
                };
                let trigger = trigger.trim();
                if !trigger.is_empty() && !trigger.contains(' ') {
                    triggers.push(trigger.to_owned());
                }
            }
        } else if let Some(inner) = rest
            .strip_prefix('[')
            .and_then(|body| body.strip_suffix(']'))
        {
            for item in inner.split(',') {
                let item = item.trim().trim_matches(['"', '\'']);
                if !item.is_empty() {
                    triggers.push(item.to_owned());
                }
            }
        } else if let Some(trigger) = rest.split_whitespace().next() {
            let trigger = trigger.trim_matches(['"', '\'', '{', '}', ',']);
            let trigger = trigger.trim_end_matches(':');
            if !trigger.is_empty() {
                triggers.push(trigger.to_owned());
            }
        }
    }
    triggers
}

fn toolchain_bytes_gaps(bytes: &[u8]) -> Vec<String> {
    let mut gaps = Vec::new();
    if bytes != RUST_TOOLCHAIN_TOML.as_bytes() || sha256_hex(bytes) != RUST_TOOLCHAIN_SHA256 {
        gaps.push("rust-toolchain.toml does not match".to_owned());
    }
    if std::str::from_utf8(bytes).is_ok_and(toolchain_sets_path) {
        gaps.push("rust-toolchain.toml sets path=".to_owned());
    }
    gaps
}

fn toolchain_file_gaps(root: &Path) -> Result<Vec<String>, String> {
    let mut gaps = Vec::new();
    let root_file = root.join("rust-toolchain.toml");
    if std::fs::symlink_metadata(&root_file).is_ok_and(|meta| meta.file_type().is_symlink()) {
        gaps.push("root rust-toolchain.toml is a symlink".to_owned());
    }
    for path in tree_paths(root)? {
        let rel = path
            .strip_prefix(root)
            .map_err(|err| format!("toolchain path: {err}"))?;
        let name = rel.file_name().and_then(|part| part.to_str()).unwrap_or("");
        if name.starts_with("rust-toolchain") && rel != Path::new("rust-toolchain.toml") {
            gaps.push(format!(
                "{} is a toolchain override; only the root rust-toolchain.toml is allowed",
                rel.display()
            ));
        }
        let parent_is_cargo = rel
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|part| part.to_str())
            == Some(".cargo");
        if parent_is_cargo && name.starts_with("config") && rel != Path::new(".cargo/config.toml") {
            gaps.push(format!(
                "{} is a nested cargo config; only the root .cargo/config.toml is allowed",
                rel.display()
            ));
        }
    }
    gaps.sort();
    Ok(gaps)
}

fn tree_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let (paths, nested_git) = walk_tree(root)?;
    if !nested_git.is_empty() {
        return Err(nested_git.join("\n"));
    }
    Ok(paths)
}

fn walk_tree(root: &Path) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let mut paths = Vec::new();
    let mut nested_git = Vec::new();
    collect_tree_files(root, root, &mut paths, &mut nested_git)?;
    paths.sort();
    nested_git.sort();
    Ok((paths, nested_git))
}

fn collect_tree_files(
    root: &Path,
    dir: &Path,
    found: &mut Vec<PathBuf>,
    nested_git: &mut Vec<String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if dir == root && (name == ".git" || name == "target") {
            continue;
        }
        let path = entry.path();
        if name == ".git" {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let shown = rel.to_string_lossy().replace('\\', "/");
            nested_git.push(format!("{shown} is a nested git repository"));
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        if file_type.is_symlink() || !file_type.is_dir() {
            found.push(path);
            continue;
        }
        collect_tree_files(root, &path, found, nested_git)?;
    }
    Ok(())
}

fn toolchain_sets_path(text: &str) -> bool {
    text.lines().any(|line| {
        let line = strip_yaml_comment(line).trim();
        let Some((key, _)) = line.split_once('=') else {
            return false;
        };
        key.trim().trim_matches(['"', '\'']) == "path"
    })
}

fn sha_file_gaps(root: &Path, name: &str, expected: &str) -> Vec<String> {
    match read_bytes(&root.join(name)) {
        Ok(bytes) if sha256_hex(&bytes) == expected => Vec::new(),
        Ok(_) => vec![format!("{name} sha256 does not match")],
        Err(err) => vec![format!("{name} is missing: {err}")],
    }
}

fn workflow_preamble(text: &str) -> Option<&str> {
    let lines: Vec<&str> = text.lines().collect();
    let jobs_at = lines
        .iter()
        .position(|line| is_top_level(line) && strip_yaml_comment(line).trim() == "jobs:")?;
    let start = lines
        .iter()
        .enumerate()
        .skip(jobs_at + 1)
        .find(|(_, line)| job_id(line).is_some())
        .map(|(idx, _)| idx)
        .unwrap_or(lines.len());
    Some(&text[..line_offset(text, start)])
}

fn ci_header_pin_gaps(text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    if sha256_hex(CI_HEADER.as_bytes()) != CI_HEADER_SHA256 {
        gaps.push("ci header pin hash does not match its bytes".to_owned());
    }
    let Some(header) = workflow_preamble(text) else {
        gaps.push("ci.yml has no jobs header".to_owned());
        return gaps;
    };
    if header != CI_HEADER {
        let mut message = format!(
            "ci header bytes do not match\npinned sha256 {CI_HEADER_SHA256}\nworkflow sha256 {}\n",
            sha256_hex(header.as_bytes())
        );
        message.push_str(&unified_diff("header", CI_HEADER, header));
        gaps.push(message);
    }
    let mut built = String::from(CI_HEADER);
    for (_, pinned, _) in REQUIRED_JOBS {
        built.push_str(pinned);
    }
    if built != text {
        gaps.push("ci.yml is not the pinned header plus required jobs".to_owned());
    }
    gaps
}

fn cargo_manifests_preimage(root: &Path) -> Result<String, String> {
    let mut lines = String::new();
    for path in CARGO_MANIFESTS {
        let bytes = read_bytes(&root.join(path))?;
        lines.push_str(&sha256_hex(&bytes));
        lines.push_str("  ");
        lines.push_str(path);
        lines.push('\n');
    }
    Ok(lines)
}

#[derive(Deserialize)]
struct WorkspaceMeta {
    packages: Vec<WorkspacePkg>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
}

#[derive(Deserialize)]
struct WorkspacePkg {
    id: String,
    manifest_path: PathBuf,
}

fn workspace_member_manifests(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--locked")
        .arg("--offline")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|err| format!("cargo metadata: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let meta: WorkspaceMeta =
        serde_json::from_slice(&output.stdout).map_err(|err| format!("metadata json: {err}"))?;
    let mut paths = Vec::new();
    for id in &meta.workspace_members {
        let pkg = meta
            .packages
            .iter()
            .find(|pkg| pkg.id == *id)
            .ok_or_else(|| format!("workspace member {id} has no package"))?;
        let rel = pkg
            .manifest_path
            .strip_prefix(&meta.workspace_root)
            .map_err(|_| {
                format!(
                    "member manifest {} is outside the workspace",
                    pkg.manifest_path.display()
                )
            })?;
        paths.push(rel.to_string_lossy().replace('\\', "/"));
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn cargo_manifests_gaps(root: &Path) -> Vec<String> {
    let mut gaps = Vec::new();
    match workspace_member_manifests(root) {
        Ok(members) => {
            for path in members {
                if !CARGO_MANIFESTS.contains(&path.as_str()) {
                    gaps.push(format!("member manifest isn't pinned: {path}"));
                }
            }
        }
        Err(err) => gaps.push(err),
    }
    match cargo_manifests_preimage(root) {
        Ok(preimage) if sha256_hex(preimage.as_bytes()) == CARGO_MANIFESTS_SHA256 => {}
        Ok(_) => gaps.push("cargo manifests sha256 does not match".to_owned()),
        Err(err) => gaps.push(err),
    }
    gaps
}

fn deny_release_pin_gaps(text: &str) -> Vec<String> {
    let Some(block) = job_block(text, "deny") else {
        return vec!["ci.yml has no deny job".to_owned()];
    };
    let mut gaps = Vec::new();
    let tarball = format!("cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl.tar.gz");
    if !block.contains(&tarball) {
        gaps.push("deny job tarball name does not use DENY_VERSION".to_owned());
    }
    if !block.contains(DENY_TARBALL_SHA256) {
        gaps.push("deny job does not pin the cargo-deny tarball sha256".to_owned());
    }
    if !block.contains(DENY_BIN_SHA256) {
        gaps.push("deny job does not pin the cargo-deny binary sha256".to_owned());
    } else if deny_exec_missing_bin_recheck(block) {
        gaps.push(
            "deny job does not re-check the cargo-deny binary sha256 before execution".to_owned(),
        );
    }
    gaps.extend(deny_rehash_gaps(
        block,
        CARGO_CONFIG_SHA256,
        ".cargo/config.toml",
    ));
    gaps.extend(deny_rehash_gaps(block, DENY_TOML_SHA256, "deny.toml"));
    if deny_exec_missing_config_absence(block) {
        gaps.push("deny job does not reject an extensionless .cargo/config".to_owned());
    }
    if deny_exec_missing_tree_walk(block) {
        gaps.push("deny job does not reject unexpected cargo config or toolchain files".to_owned());
    }
    gaps.extend(deny_path_shim_gaps(block));
    gaps.extend(deny_cargo_env_gaps(block));
    gaps.extend(deny_toolchain_walk_gaps(block));
    gaps
}

fn deny_toolchain_walk_gaps(block: &str) -> Vec<String> {
    const WALK: &str = "cargo xtask guards toolchain-files";
    let total = block.matches(WALK).count();
    if total == 0 {
        return Vec::new();
    }
    let lines: Vec<&str> = block.lines().collect();
    let mut covered = 0usize;
    let mut idx = 0usize;
    while idx < lines.len() {
        if line_after_env_i(lines[idx]).is_none() {
            idx += 1;
            continue;
        }
        let start = idx;
        while idx + 1 < lines.len() && lines[idx].trim_end().ends_with('\\') {
            idx += 1;
        }
        let command = lines[start..=idx].join("\n");
        if command.contains(WALK) {
            covered += 1;
            if !pinned_walk_command(&command) {
                return vec!["deny job toolchain walk can auto-install through rustup".to_owned()];
            }
        }
        idx += 1;
    }
    if covered != total {
        vec!["deny job toolchain walk can auto-install through rustup".to_owned()]
    } else {
        Vec::new()
    }
}

fn pinned_walk_command(command: &str) -> bool {
    let cargo = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo";
    let rustc = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc";
    let toolchain = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin";
    let cargo_set = format!("CARGO={cargo}");
    let rustc_set = format!("RUSTC={rustc}");
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if !tokens.iter().any(|token| *token == cargo_set)
        || !tokens.iter().any(|token| *token == rustc_set)
    {
        return false;
    }
    let Some(path) = tokens.iter().find_map(|token| token.strip_prefix("PATH=")) else {
        return false;
    };
    if path.split(':').next() != Some(toolchain) {
        return false;
    }
    let words: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|token| {
            *token != "env" && *token != "-i" && *token != "\\" && assignment_name(token).is_none()
        })
        .collect();
    words.as_slice() == [cargo, "xtask", "guards", "toolchain-files"]
}

fn deny_path_shim_gaps(block: &str) -> Vec<String> {
    let toolchain = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin";
    let mut gaps = Vec::new();
    for line in block.lines() {
        let Some(rest) = line.split("PATH=").nth(1) else {
            continue;
        };
        let value = rest
            .split_whitespace()
            .next()
            .unwrap_or(rest)
            .trim_end_matches('\\')
            .trim_matches(['"', '\'']);
        let Some(first) = value.split(':').next() else {
            continue;
        };
        if first != toolchain {
            let gap = format!("deny job PATH entry {first} is before the toolchain bin");
            if !gaps.contains(&gap) {
                gaps.push(gap);
            }
        }
    }
    gaps
}

fn deny_cargo_env_gaps(block: &str) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    let mut gaps = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let code = strip_yaml_comment(lines[idx]).trim();
        let key = code.strip_prefix("- ").unwrap_or(code);
        let Some(rest) = key.strip_prefix("env:") else {
            idx += 1;
            continue;
        };
        let indent = lines[idx].len() - lines[idx].trim_start().len();
        record_flow_cargo(&mut gaps, rest);
        idx += 1;
        while idx < lines.len() {
            let follow = lines[idx];
            let child = strip_yaml_comment(follow).trim();
            if child.is_empty() {
                idx += 1;
                continue;
            }
            let findent = follow.len() - follow.trim_start().len();
            if findent <= indent {
                break;
            }
            let child_key = child.strip_prefix("- ").unwrap_or(child);
            if let Some((env_key, value)) = child_key.split_once(':') {
                if env_key.trim() == "CARGO" {
                    record_cargo_env(&mut gaps, value.trim());
                }
            }
            idx += 1;
        }
    }
    gaps.extend(deny_env_i_literal_gaps(block));
    gaps
}

fn deny_rehash_gaps(block: &str, hash: &str, label: &str) -> Vec<String> {
    if !block.contains(hash) {
        vec![format!("deny job does not re-hash {label}")]
    } else if deny_exec_missing_before(block, hash) {
        vec![format!(
            "deny job does not re-hash {label} before execution"
        )]
    } else {
        Vec::new()
    }
}

fn deny_env_i_literal_gaps(block: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    let mut pending = false;
    for line in block.lines() {
        let segment = if pending {
            line
        } else if let Some(rest) = line_after_env_i(line) {
            rest
        } else {
            continue;
        };
        let mut ended = false;
        for word in shell_words(segment) {
            if word == "\\" {
                continue;
            }
            if let Some(key) = assignment_name(word) {
                if matches!(key, "CARGO" | "RUSTC" | "RUSTFLAGS") {
                    let value = word
                        .split_once('=')
                        .map(|(_, value)| value.trim_end_matches('\\'))
                        .unwrap_or("");
                    if pinned_tool_path(key) != Some(value) {
                        push_gap(&mut gaps, format!("deny job sets {key}={value} in env -i"));
                    }
                }
                continue;
            }
            ended = true;
            break;
        }
        pending = !ended && line.trim_end().ends_with('\\');
    }
    gaps
}

fn line_after_env_i(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let needle = "env -i";
    let mut start = 0usize;
    while let Some(rel) = line[start..].find(needle) {
        let idx = start + rel;
        let before_ok =
            idx == 0 || (bytes[idx - 1] != b'_' && !bytes[idx - 1].is_ascii_alphanumeric());
        let after = idx + needle.len();
        let after_ok = after >= bytes.len() || bytes[after].is_ascii_whitespace();
        if before_ok && after_ok {
            return Some(&line[after..]);
        }
        start = idx + needle.len();
    }
    None
}

fn record_flow_cargo(gaps: &mut Vec<String>, rest: &str) {
    let rest = rest.trim();
    if rest.is_empty() {
        return;
    }
    let inner = rest.trim_matches(|ch: char| ch == '{' || ch == '}');
    for item in inner.split(',') {
        if let Some((key, value)) = item.split_once(':') {
            if key.trim() == "CARGO" {
                record_cargo_env(gaps, value.trim());
            }
        }
    }
}

fn record_cargo_env(gaps: &mut Vec<String>, value: &str) {
    let gap = format!("deny job sets CARGO={value} in the step or job env");
    if !gaps.contains(&gap) {
        gaps.push(gap);
    }
}

fn deny_exec_missing_bin_recheck(block: &str) -> bool {
    deny_exec_missing_before(block, DENY_BIN_SHA256)
}

fn deny_exec_missing_config_absence(block: &str) -> bool {
    deny_exec_missing_before(block, ".cargo/config must be absent")
}

fn deny_exec_missing_tree_walk(block: &str) -> bool {
    deny_exec_missing_before(block, "cargo xtask guards toolchain-files")
}

fn required_jobs_needs_gaps(text: &str) -> Result<Vec<String>, String> {
    let needs = gate_needs(gate_job_block(text)?)?;
    let names = required_job_names();
    let mut gaps = Vec::new();
    for name in names.difference(&needs) {
        gaps.push(format!("required job {name} is missing from gate.needs"));
    }
    for name in needs.difference(&names) {
        gaps.push(format!(
            "gate.needs lists {name} which is not a required job"
        ));
    }
    Ok(gaps)
}

fn required_job_names() -> BTreeSet<String> {
    REQUIRED_JOBS
        .iter()
        .filter(|(name, _, _)| *name != "gate")
        .map(|(name, _, _)| (*name).to_owned())
        .collect()
}

fn deny_exec_missing_before(block: &str, needle: &str) -> bool {
    let bin = format!("/tmp/cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl/cargo-deny");
    let lines: Vec<&str> = block.lines().collect();
    let mut step_starts = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if line.starts_with("      - ") {
            step_starts.push(idx);
        }
    }
    step_starts.push(lines.len());
    for pair in step_starts.windows(2) {
        let step = &lines[pair[0]..pair[1]];
        for (offset, line) in step.iter().enumerate() {
            if line.contains(&bin)
                && !line.contains("sha256sum")
                && !step[..offset].join("\n").contains(needle)
            {
                return true;
            }
        }
    }
    false
}

fn required_job_pin_gaps(text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    for (name, pinned, hash) in REQUIRED_JOBS {
        if sha256_hex(pinned.as_bytes()) != *hash {
            gaps.push(format!(
                "required job {name} pin hash does not match its bytes"
            ));
        }
        let Some(block) = job_block(text, name) else {
            gaps.push(format!("ci.yml has no {name} job"));
            continue;
        };
        if block != *pinned {
            let mut message = format!(
                "ci job {name} bytes do not match\npinned sha256 {hash}\nworkflow sha256 {}\n",
                sha256_hex(block.as_bytes())
            );
            message.push_str(&unified_diff(name, pinned, block));
            gaps.push(message);
        }
    }
    gaps
}

fn without_pinned_jobs(text: &str) -> String {
    let mut current = text.to_owned();
    for (name, pinned, _) in REQUIRED_JOBS {
        if job_block(&current, name) == Some(*pinned) {
            current = map_job_block(&current, name, |_| String::new());
        }
    }
    current
}

fn required_job_shape_gaps(name: &str, text: &str) -> Vec<String> {
    let Ok(jobs) = ci_job_ids(text) else {
        return Vec::new();
    };
    let mut gaps = Vec::new();
    for job in jobs {
        if !required_job(job.as_str()) {
            continue;
        }
        let Some(block) = job_block(text, &job) else {
            continue;
        };
        let pinned = pinned_job(&job).unwrap_or("");
        for key in ["container", "services", "strategy", "matrix"] {
            if job_sets_key(block, key) && !job_sets_key(pinned, key) {
                gaps.push(format!("{name} job {job} sets {key}:"));
            }
        }
        gaps.extend(job_env_allow_gaps(name, &job, block));
        gaps.extend(step_env_allow_gaps(name, &job, block));
    }
    gaps
}

fn required_job(name: &str) -> bool {
    REQUIRED_JOBS.iter().any(|(job, _, _)| *job == name)
}

fn pinned_job(name: &str) -> Option<&'static str> {
    REQUIRED_JOBS
        .iter()
        .find(|(job, _, _)| *job == name)
        .map(|(_, text, _)| *text)
}

fn job_sets_key(block: &str, key: &str) -> bool {
    let header = format!("{key}:");
    block.lines().any(|line| {
        let (indent, _) = leading_indent(line);
        let trimmed = strip_yaml_comment(line).trim();
        indent == 4 && (trimmed == header || trimmed.starts_with(&header))
    })
}

fn job_env_allow_gaps(name: &str, job: &str, block: &str) -> Vec<String> {
    let allowed = named_allow(JOB_ENV_ALLOW, job);
    env_allow_gaps(name, job, &mapping_keys(block, 4), allowed)
}

fn step_env_allow_gaps(name: &str, job: &str, block: &str) -> Vec<String> {
    let allowed = named_allow(STEP_ENV_ALLOW, job);
    env_allow_gaps(name, job, &step_env_keys(block), allowed)
}

fn named_allow<'a>(table: &'a [(&str, &[&str])], job: &str) -> &'a [&'a str] {
    table
        .iter()
        .find(|(name, _)| *name == job)
        .map(|(_, keys)| *keys)
        .unwrap_or(&[])
}

fn env_allow_gaps(name: &str, job: &str, keys: &[String], allowed: &[&str]) -> Vec<String> {
    let mut gaps = Vec::new();
    for key in keys {
        if !allowed.contains(&key.as_str()) || env_name_disallowed(key) {
            gaps.push(format!("{name} job {job} env {key} is not allowlisted"));
        }
    }
    for key in allowed {
        if !keys.iter().any(|found| found == key) {
            gaps.push(format!("{name} job {job} env is missing {key}"));
        }
    }
    gaps
}

fn env_name_disallowed(key: &str) -> bool {
    key.starts_with("CARGO")
        || key.starts_with("RUSTC")
        || key == "RUSTDOCFLAGS"
        || key == "CARGO_ENCODED_RUSTFLAGS"
        || key.starts_with("CARGO_SOURCE_")
        || key == "BASH_ENV"
}

fn mapping_keys(block: &str, parent_indent: usize) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    let mut keys = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let (indent, _) = leading_indent(lines[idx]);
        let trimmed = strip_yaml_comment(lines[idx]).trim();
        if indent == parent_indent && trimmed == "env:" {
            idx += 1;
            while idx < lines.len() {
                let (findent, _) = leading_indent(lines[idx]);
                let child = strip_yaml_comment(lines[idx]).trim();
                if child.is_empty() {
                    idx += 1;
                    continue;
                }
                if findent <= parent_indent {
                    break;
                }
                if let Some((key, _)) = child.split_once(':') {
                    let key = key.trim().trim_matches(['"', '\'']);
                    if !key.is_empty() && !keys.iter().any(|found| found == key) {
                        keys.push(key.to_owned());
                    }
                }
                idx += 1;
            }
            continue;
        }
        idx += 1;
    }
    keys
}

fn step_env_keys(block: &str) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    let mut keys = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let (indent, _) = leading_indent(lines[idx]);
        let trimmed = strip_yaml_comment(lines[idx]).trim();
        let key = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        if indent > 4 && key == "env:" {
            idx += 1;
            while idx < lines.len() {
                let (findent, _) = leading_indent(lines[idx]);
                let child = strip_yaml_comment(lines[idx]).trim();
                if child.is_empty() {
                    idx += 1;
                    continue;
                }
                if findent <= indent {
                    break;
                }
                if let Some((env_key, _)) = child.split_once(':') {
                    let env_key = env_key.trim().trim_matches(['"', '\'']);
                    if !env_key.is_empty() && !keys.iter().any(|found| found == env_key) {
                        keys.push(env_key.to_owned());
                    }
                }
                idx += 1;
            }
            continue;
        }
        idx += 1;
    }
    keys
}

enum LineOp<'a> {
    Same(&'a str),
    Del(&'a str),
    Add(&'a str),
}

fn unified_diff(job: &str, expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let ops = line_ops(&expected_lines, &actual_lines);
    let mut changed = Vec::new();
    for (idx, op) in ops.iter().enumerate() {
        if !matches!(op, LineOp::Same(_)) {
            changed.push(idx);
        }
    }
    let mut out = format!("--- pinned {job}\n+++ workflow {job}\n");
    if changed.is_empty() {
        out.push_str("texts differ past the shared lines\n");
        return out;
    }
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for idx in changed {
        if let Some(last) = groups.last_mut() {
            if idx <= last.1 + 6 {
                last.1 = idx;
                continue;
            }
        }
        groups.push((idx, idx));
    }
    for (first, last) in groups {
        let from = first.saturating_sub(3);
        let to = (last + 4).min(ops.len());
        let (mut old_line, mut new_line) = (1usize, 1usize);
        for op in ops.iter().take(from) {
            match op {
                LineOp::Same(_) => {
                    old_line += 1;
                    new_line += 1;
                }
                LineOp::Del(_) => old_line += 1,
                LineOp::Add(_) => new_line += 1,
            }
        }
        let (mut old_count, mut new_count) = (0usize, 0usize);
        for op in ops.iter().take(to).skip(from) {
            match op {
                LineOp::Same(_) | LineOp::Del(_) => old_count += 1,
                LineOp::Add(_) => {}
            }
            match op {
                LineOp::Same(_) | LineOp::Add(_) => new_count += 1,
                LineOp::Del(_) => {}
            }
        }
        out.push_str(&format!(
            "@@ -{old_line},{old_count} +{new_line},{new_count} @@\n"
        ));
        for op in ops.iter().take(to).skip(from) {
            match op {
                LineOp::Same(line) => out.push_str(&format!(" {line}\n")),
                LineOp::Del(line) => out.push_str(&format!("-{line}\n")),
                LineOp::Add(line) => out.push_str(&format!("+{line}\n")),
            }
        }
    }
    out
}

fn line_ops<'a>(expected: &[&'a str], actual: &[&'a str]) -> Vec<LineOp<'a>> {
    let rows = expected.len();
    let cols = actual.len();
    let mut score = vec![0u32; (rows + 1) * (cols + 1)];
    let at = |row: usize, col: usize| row * (cols + 1) + col;
    for row in (0..rows).rev() {
        for col in (0..cols).rev() {
            score[at(row, col)] = if expected[row] == actual[col] {
                score[at(row + 1, col + 1)] + 1
            } else {
                score[at(row + 1, col)].max(score[at(row, col + 1)])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut row, mut col) = (0usize, 0usize);
    while row < rows && col < cols {
        if expected[row] == actual[col] {
            ops.push(LineOp::Same(expected[row]));
            row += 1;
            col += 1;
        } else if score[at(row + 1, col)] >= score[at(row, col + 1)] {
            ops.push(LineOp::Del(expected[row]));
            row += 1;
        } else {
            ops.push(LineOp::Add(actual[col]));
            col += 1;
        }
    }
    while row < rows {
        ops.push(LineOp::Del(expected[row]));
        row += 1;
    }
    while col < cols {
        ops.push(LineOp::Add(actual[col]));
        col += 1;
    }
    ops
}

fn forbidden_env_gaps(name: &str, text: &str) -> Vec<String> {
    let text = without_pinned_jobs(text);
    let stripped = strip_env_allowlist(&text);
    let lines: Vec<&str> = stripped.lines().collect();
    let mut gaps = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let code = strip_yaml_comment(lines[idx]).trim();
        let key = code.strip_prefix("- ").unwrap_or(code);
        if let Some(rest) = key.strip_prefix("env:") {
            let indent = lines[idx].len() - lines[idx].trim_start().len();
            let rest = rest.trim();
            if !rest.is_empty() {
                gaps.extend(flow_env_gaps(name, rest));
            }
            idx += 1;
            while idx < lines.len() {
                let follow = lines[idx];
                let child = strip_yaml_comment(follow).trim();
                if child.is_empty() {
                    idx += 1;
                    continue;
                }
                let findent = follow.len() - follow.trim_start().len();
                if findent <= indent {
                    break;
                }
                if let Some((env_key, _)) = child.split_once(':') {
                    let env_key = env_key.trim();
                    if env_key_forbidden(env_key) {
                        gaps.push(format!("{name} env sets {env_key}"));
                    }
                }
                idx += 1;
            }
            continue;
        }
        idx += 1;
    }
    gaps
}

fn env_key_forbidden(key: &str) -> bool {
    let key = key.trim().trim_matches(['"', '\'']);
    if key.is_empty() {
        return false;
    }
    if FORBIDDEN_ENV.contains(&key) {
        return true;
    }
    if key.starts_with("CARGO_ALIAS_") || key.starts_with("CARGO_BUILD_RUSTC") {
        return true;
    }
    if key.starts_with("CARGO_TARGET_")
        && key.ends_with("_RUNNER")
        && key.len() > "CARGO_TARGET_".len() + "_RUNNER".len()
    {
        return true;
    }
    if key.starts_with("LD_") || key.starts_with("DYLD_") {
        return true;
    }
    matches!(
        key,
        "RUSTC_WRAPPER"
            | "RUSTC_WORKSPACE_WRAPPER"
            | "CARGO_HOME"
            | "RUSTFLAGS"
            | "PATH"
            | "RUSTUP_HOME"
            | "BASH_ENV"
            | "ENV"
    )
}

fn flow_env_gaps(name: &str, rest: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    let inner = rest.trim().trim_matches(|ch| ch == '{' || ch == '}');
    for item in inner.split(',') {
        let key = item
            .split_once(':')
            .map(|(key, _)| key.trim())
            .unwrap_or("");
        if env_key_forbidden(key) {
            gaps.push(format!("{name} env sets {key}"));
        }
    }
    gaps
}

fn rustc_pin_gaps(name: &str, text: &str) -> Vec<String> {
    let Ok(jobs) = ci_job_ids(text) else {
        return Vec::new();
    };
    let mut gaps = Vec::new();
    for job in jobs {
        let Some(block) = job_block(text, &job) else {
            continue;
        };
        if job_invokes_cargo_or_rustc(block) && !guards_steps_exact(block) {
            if !block.contains(RUSTC_198_STEP) {
                gaps.push(format!("{name} job {job} does not assert rustc 1.98.0"));
            }
            if !block.contains(RUSTUP_WHICH_STEP) {
                gaps.push(format!(
                    "{name} job {job} does not assert rustup which 1.98.0"
                ));
            }
        }
    }
    gaps
}

fn job_invokes_cargo_or_rustc(block: &str) -> bool {
    block.lines().any(|line| {
        let code = strip_yaml_comment(line);
        yaml_token(code, "cargo") || yaml_token(code, "rustc")
    })
}

fn yaml_token(line: &str, token: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = token.as_bytes();
    let mut start = 0usize;
    while let Some(rel) = line[start..].find(token) {
        let idx = start + rel;
        let before_ok = idx == 0 || !bytes[idx - 1].is_ascii_alphanumeric();
        let after = idx + needle.len();
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = idx + needle.len();
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum YamlFrameKind {
    On,
    Permissions,
    Env,
    Steps,
}

#[derive(Clone, Copy)]
struct YamlFrame {
    kind: YamlFrameKind,
    indent: usize,
}

fn strict_yaml_gaps(name: &str, text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    if text.contains('\u{feff}') {
        push_gap(&mut gaps, format!("{name} has a byte order mark"));
    }
    if text.contains('\0') {
        push_gap(&mut gaps, format!("{name} contains NUL"));
    }
    if text.contains('\r') {
        push_gap(&mut gaps, format!("{name} uses crlf line endings"));
    }
    let stripped = text.replace('\u{feff}', "");
    let text = stripped.as_str();
    let mut block_parent: Option<usize> = None;
    let mut stack: Vec<YamlFrame> = Vec::new();
    let mut maps: Vec<(usize, Vec<String>)> = Vec::new();
    let mut open_key: Option<(String, usize)> = None;
    for line in text.lines() {
        let (indent, tab) = leading_indent(line);
        if tab {
            push_gap(&mut gaps, format!("{name} uses a tab in indentation"));
        }
        if let Some(parent) = block_parent {
            if line.trim().is_empty() || indent > parent {
                continue;
            }
            block_parent = None;
        }
        let code = strip_yaml_comment(line);
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "---" || trimmed == "..." || trimmed.starts_with("--- ") {
            push_gap(&mut gaps, format!("{name} has multiple documents"));
            block_parent = None;
            stack.clear();
            maps.clear();
            open_key = None;
            continue;
        }
        let marks = structural_marks(trimmed);
        if marks.merge {
            push_gap(&mut gaps, format!("{name} uses a merge key"));
        }
        if marks.tag {
            push_gap(&mut gaps, format!("{name} uses a tag"));
        }
        if marks.anchor {
            push_gap(&mut gaps, format!("{name} uses an anchor"));
        }
        if marks.alias {
            push_gap(&mut gaps, format!("{name} uses an alias"));
        }
        if open_key.as_ref().is_some_and(|(_, at)| indent <= *at) {
            open_key = None;
        }
        while stack.last().is_some_and(|frame| indent <= frame.indent) {
            stack.pop();
        }
        let parent = stack.last().copied();
        let seq = trimmed.starts_with("- ");
        let body = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
        let key_indent = if seq { indent + 2 } else { indent };
        if seq {
            while maps.last().is_some_and(|(at, _)| *at >= key_indent) {
                maps.pop();
            }
        }
        if body.starts_with('?') || body.starts_with(':') {
            push_gap(&mut gaps, format!("{name} uses an explicit yaml key"));
            continue;
        }
        if yaml_tag(body) {
            push_gap(&mut gaps, format!("{name} uses a tag"));
            continue;
        }
        if body.starts_with('{') || body.starts_with('[') {
            if let Some(role) = flow_node_role(seq, parent, indent) {
                push_gap(
                    &mut gaps,
                    format!(
                        "{name} uses a flow {} as {role}",
                        flow_kind(body.starts_with('{'))
                    ),
                );
            } else {
                push_gap(
                    &mut gaps,
                    format!("{name} uses a flow {}", flow_kind(body.starts_with('{'))),
                );
            }
            continue;
        }
        let Some((key, value)) = split_yaml_key(body) else {
            if indented_block_scalar(body) {
                push_gap(&mut gaps, format!("{name} uses an indented block scalar"));
                block_parent = Some(indent);
                open_key = None;
                continue;
            }
            if block_scalar_header(body) {
                if let Some((key, at)) = open_key.clone() {
                    if indent > at && step_run(&key, parent, at) {
                        block_parent = Some(at);
                        open_key = None;
                        continue;
                    }
                }
                push_gap(&mut gaps, format!("{name} uses a block scalar outside run"));
                block_parent = Some(indent);
                open_key = None;
                continue;
            }
            if seq && parent.is_some_and(|frame| direct_child(frame, indent, YamlFrameKind::On)) {
                push_gap(&mut gaps, format!("{name} uses a sequence under on"));
            }
            continue;
        };
        if key_is_quoted(key) {
            push_gap(&mut gaps, format!("{name} has a quoted key"));
        }
        if !key.is_ascii() {
            push_gap(&mut gaps, format!("{name} has a non-ascii key"));
        }
        if key == "defaults" {
            push_gap(&mut gaps, format!("{name} sets defaults"));
        }
        if key != key.trim_end() {
            push_gap(&mut gaps, format!("{name} has whitespace before a colon"));
        }
        if repeats_key(&mut maps, key_indent, key) {
            push_gap(
                &mut gaps,
                format!(
                    "{name} repeats key {}",
                    key.trim().trim_matches(['"', '\''])
                ),
            );
        }
        let value = value.trim();
        if yaml_tag(value) {
            push_gap(&mut gaps, format!("{name} uses a tag"));
        }
        if (value.starts_with('{') || value.starts_with('['))
            && !allowed_needs_list(key, value)
            && !allowed_runner_list(key, value)
        {
            if let Some(role) = flow_value_role(key, parent, indent) {
                push_gap(
                    &mut gaps,
                    format!(
                        "{name} uses a flow {} as {role}",
                        flow_kind(value.starts_with('{'))
                    ),
                );
            } else {
                push_gap(
                    &mut gaps,
                    format!("{name} uses a flow {}", flow_kind(value.starts_with('{'))),
                );
            }
        }
        if indented_block_scalar(value) {
            push_gap(&mut gaps, format!("{name} uses an indented block scalar"));
            block_parent = Some(indent);
            open_key = None;
            continue;
        }
        if block_scalar_header(value) {
            if !step_run(key, parent, indent) {
                push_gap(&mut gaps, format!("{name} uses a block scalar outside run"));
            }
            block_parent = Some(indent);
            open_key = None;
            continue;
        }
        if value.is_empty() {
            open_key = Some((key.trim().trim_matches(['"', '\'']).to_owned(), key_indent));
            if let Some(kind) = frame_kind(key) {
                stack.push(YamlFrame { kind, indent });
            }
        } else {
            open_key = None;
        }
    }
    gaps
}

fn push_gap(gaps: &mut Vec<String>, gap: String) {
    if !gaps.contains(&gap) {
        gaps.push(gap);
    }
}

fn leading_indent(line: &str) -> (usize, bool) {
    let mut indent = 0usize;
    let mut tab = false;
    for ch in line.chars() {
        if ch == ' ' {
            indent += 1;
        } else if ch == '\t' {
            tab = true;
            indent += 1;
        } else {
            break;
        }
    }
    (indent, tab)
}

struct StructuralMarks {
    anchor: bool,
    alias: bool,
    merge: bool,
    tag: bool,
}

fn structural_marks(line: &str) -> StructuralMarks {
    let bytes = line.as_bytes();
    let mut quote = None;
    let mut marks = StructuralMarks {
        anchor: false,
        alias: false,
        merge: false,
        tag: false,
    };
    let mut idx = 0usize;
    while idx < bytes.len() {
        let ch = bytes[idx];
        if let Some(open) = quote {
            if open == b'"' && ch == b'\\' {
                idx += 1;
                if idx < bytes.len() {
                    idx += 1;
                }
                continue;
            }
            if open == b'\'' && ch == b'\'' && bytes.get(idx + 1) == Some(&b'\'') {
                idx += 2;
                continue;
            }
            if ch == open {
                quote = None;
            }
            idx += 1;
            continue;
        }
        match ch {
            b'"' | b'\'' => {
                quote = Some(ch);
                idx += 1;
            }
            b'&' => {
                let prev_amp = idx > 0 && bytes[idx - 1] == b'&';
                let next = bytes.get(idx + 1).copied();
                if !prev_amp
                    && next != Some(b'&')
                    && next.is_some_and(|next| next.is_ascii_alphanumeric() || next == b'_')
                {
                    marks.anchor = true;
                }
                idx += 1;
            }
            b'*' => {
                let next = bytes.get(idx + 1).copied();
                if next.is_some_and(|next| next.is_ascii_alphanumeric() || next == b'_') {
                    marks.alias = true;
                }
                idx += 1;
            }
            b'<' if bytes.get(idx + 1) == Some(&b'<') => {
                marks.merge = true;
                idx += 2;
            }
            b'!' if bytes.get(idx + 1) == Some(&b'!') => {
                marks.tag = true;
                idx += 2;
            }
            _ => idx += 1,
        }
    }
    marks
}

fn split_yaml_key(body: &str) -> Option<(&str, &str)> {
    let mut quote = None;
    let mut chars = body.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if let Some(open) = quote {
            if yaml_escape_skips(&mut chars, open, ch) {
                continue;
            }
            if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            ':' => return Some((&body[..idx], &body[idx + 1..])),
            _ => {}
        }
    }
    None
}

fn key_is_quoted(key: &str) -> bool {
    key.contains(['"', '\''])
}

fn yaml_tag(value: &str) -> bool {
    let mut chars = value.trim().chars();
    if chars.next() != Some('!') {
        return false;
    }
    !matches!(chars.next(), Some('='))
}

fn block_scalar_header(value: &str) -> bool {
    let mut chars = value.trim().chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '|' && first != '>' {
        return false;
    }
    matches!(chars.as_str(), "" | "-" | "+")
}

fn indented_block_scalar(value: &str) -> bool {
    let mut chars = value.trim().chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '|' && first != '>' {
        return false;
    }
    let rest = chars.as_str();
    rest.chars().any(|ch| ch.is_ascii_digit())
        && rest
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '+' || ch == '-')
}

fn step_run(key: &str, parent: Option<YamlFrame>, indent: usize) -> bool {
    key == "run"
        && parent.is_some_and(|frame| frame.kind == YamlFrameKind::Steps && indent > frame.indent)
}

/// Main's self-hosted label list. Any other `runs-on` flow sequence still fails.
fn allowed_runner_list(key: &str, value: &str) -> bool {
    key == "runs-on" && value.trim() == "[self-hosted, linux, x64, parangsae]"
}

fn allowed_needs_list(key: &str, value: &str) -> bool {
    if key != "needs" {
        return false;
    }
    let Some(inner) = value
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return false;
    };
    if inner.contains(['{', '}', '[', ']', ':', '!', '&', '*', '|', '>', '?', '#']) {
        return false;
    }
    inner.split(',').all(|item| {
        let item = item.trim().trim_matches(['"', '\'']);
        !item.is_empty()
            && item
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    })
}

fn repeats_key(maps: &mut Vec<(usize, Vec<String>)>, key_indent: usize, key: &str) -> bool {
    let key = key.trim().trim_matches(['"', '\'']).to_owned();
    while maps.last().is_some_and(|(indent, _)| *indent > key_indent) {
        maps.pop();
    }
    if let Some((indent, keys)) = maps.last_mut() {
        if *indent == key_indent {
            if keys.iter().any(|found| found == &key) {
                return true;
            }
            keys.push(key);
            return false;
        }
    }
    maps.push((key_indent, vec![key]));
    false
}

fn frame_kind(key: &str) -> Option<YamlFrameKind> {
    match key {
        "on" => Some(YamlFrameKind::On),
        "permissions" => Some(YamlFrameKind::Permissions),
        "env" => Some(YamlFrameKind::Env),
        "steps" => Some(YamlFrameKind::Steps),
        _ => None,
    }
}

fn direct_child(frame: YamlFrame, indent: usize, kind: YamlFrameKind) -> bool {
    frame.kind == kind && indent == frame.indent + 2
}

fn flow_kind(mapping: bool) -> &'static str {
    if mapping {
        "mapping"
    } else {
        "sequence"
    }
}

fn flow_node_role(seq: bool, parent: Option<YamlFrame>, indent: usize) -> Option<&'static str> {
    let frame = parent?;
    match frame.kind {
        YamlFrameKind::Steps => seq.then_some("a step"),
        YamlFrameKind::On => direct_child(frame, indent, YamlFrameKind::On).then_some("on"),
        YamlFrameKind::Permissions => (indent > frame.indent).then_some("permissions"),
        YamlFrameKind::Env => (indent > frame.indent).then_some("env"),
    }
}

fn flow_value_role(key: &str, parent: Option<YamlFrame>, indent: usize) -> Option<&'static str> {
    match key {
        "on" => Some("on"),
        "permissions" => Some("permissions"),
        "env" => Some("env"),
        _ => parent.and_then(|frame| {
            direct_child(frame, indent, YamlFrameKind::On).then_some("a trigger")
        }),
    }
}

fn github_env_path_gaps(name: &str, text: &str) -> Vec<String> {
    let text = without_pinned_jobs(text);
    let text = text.replace(GUARDS_STEP, "").replace(ALL_ZERO_STEP, "");
    let mut gaps = Vec::new();
    for script in run_scripts(&text) {
        if script.contains("GITHUB_ENV") {
            push_gap(&mut gaps, format!("{name} references GITHUB_ENV"));
        }
        if script.contains("GITHUB_PATH") {
            push_gap(&mut gaps, format!("{name} references GITHUB_PATH"));
        }
        gaps.extend(run_assignment_gaps(name, &script));
    }
    gaps
}

fn run_scripts(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut scripts = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let indent = lines[idx].len() - lines[idx].trim_start().len();
        let trimmed = strip_yaml_comment(lines[idx]).trim();
        let Some(rest) = run_prefix(trimmed) else {
            idx += 1;
            continue;
        };
        let rest = rest.trim();
        if rest.is_empty() || block_scalar_header(rest) {
            let mut script = String::new();
            idx += 1;
            while idx < lines.len() {
                let child = lines[idx];
                if !child.trim().is_empty() {
                    let findent = child.len() - child.trim_start().len();
                    if findent <= indent {
                        break;
                    }
                }
                script.push_str(child);
                script.push('\n');
                idx += 1;
            }
            scripts.push(script);
            continue;
        }
        scripts.push(rest.to_owned());
        idx += 1;
    }
    scripts
}

fn run_prefix(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("- ")
        .unwrap_or(trimmed)
        .strip_prefix("run:")
}

fn run_assignment_gaps(name: &str, script: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    for line in script.lines() {
        let words = shell_words(line);
        let mut exported = Vec::new();
        for (idx, word) in words.iter().enumerate() {
            if *word != "export" {
                continue;
            }
            let mut cursor = idx + 1;
            while words
                .get(cursor)
                .is_some_and(|flag| flag.starts_with('-') && assignment_name(flag).is_none())
            {
                cursor += 1;
            }
            let Some(operand) = words.get(cursor) else {
                continue;
            };
            let Some(key) = export_name(operand) else {
                continue;
            };
            if env_key_forbidden(key) {
                push_gap(&mut gaps, format!("{name} exports {key}"));
            }
            exported.push(cursor);
        }
        for key in assignment_keys(line) {
            if exported.iter().any(|idx| {
                words
                    .get(*idx)
                    .and_then(|word| export_name(word))
                    .is_some_and(|exported| exported == key)
            }) {
                continue;
            }
            push_gap(&mut gaps, format!("{name} assigns {key}"));
        }
    }
    gaps
}

fn assignment_keys(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut keys = Vec::new();
    let mut idx = 0usize;
    let mut quote = None;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if let Some(open) = quote {
            if byte == open {
                quote = None;
            }
            idx += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            idx += 1;
            continue;
        }
        let boundary = idx == 0 || is_assign_boundary(bytes[idx - 1]);
        if boundary && is_env_start(byte) {
            let start = idx;
            idx += 1;
            while idx < bytes.len() && is_env_continue(bytes[idx]) {
                idx += 1;
            }
            if bytes.get(idx) == Some(&b'=') {
                let key = line[start..idx].to_owned();
                if env_key_forbidden(&key) && !keys.contains(&key) {
                    keys.push(key);
                }
                idx += 1;
                continue;
            }
            continue;
        }
        idx += 1;
    }
    keys
}

fn is_assign_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'(' | b';' | b'|' | b'&' | b'`')
}

fn shell_words(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut words = Vec::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= bytes.len() || bytes[idx] == b'#' {
            break;
        }
        let start = idx;
        let mut quote = None;
        while idx < bytes.len() {
            let byte = bytes[idx];
            if let Some(open) = quote {
                if byte == open {
                    quote = None;
                }
                idx += 1;
                continue;
            }
            match byte {
                b'"' | b'\'' => {
                    quote = Some(byte);
                    idx += 1;
                }
                byte if byte.is_ascii_whitespace() => break,
                _ => idx += 1,
            }
        }
        if start < idx {
            words.push(&line[start..idx]);
        }
    }
    words
}

fn assignment_name(word: &str) -> Option<&str> {
    let bytes = word.as_bytes();
    if bytes.first().is_none_or(|byte| !is_env_start(*byte)) {
        return None;
    }
    let mut idx = 1usize;
    while idx < bytes.len() && is_env_continue(bytes[idx]) {
        idx += 1;
    }
    if bytes.get(idx) == Some(&b'=') {
        Some(&word[..idx])
    } else {
        None
    }
}

fn export_name(word: &str) -> Option<&str> {
    if let Some(name) = assignment_name(word) {
        return Some(name);
    }
    if !word.is_empty() && is_env_start(word.as_bytes()[0]) && word.bytes().all(is_env_continue) {
        Some(word)
    } else {
        None
    }
}

fn is_env_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_env_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn cargo_config_gaps(root: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    collect_cargo_configs(root, &mut files)?;
    files.sort();
    let allowed = root.join(".cargo").join("config.toml");
    let mut gaps = Vec::new();
    let mut saw_allowed = false;
    for file in &files {
        if file == &allowed {
            saw_allowed = true;
            continue;
        }
        gaps.push(format!(
            "unexpected cargo config {}",
            workflow_display(root, file)
        ));
    }
    if !saw_allowed {
        gaps.push("missing .cargo/config.toml".to_owned());
        return Ok(gaps);
    }
    let bytes = read_bytes(&allowed)?;
    if bytes.as_slice() != CARGO_CONFIG_TOML.as_bytes() || sha256_hex(&bytes) != CARGO_CONFIG_SHA256
    {
        gaps.push(".cargo/config.toml does not match the pinned xtask alias".to_owned());
        if let Ok(text) = std::str::from_utf8(&bytes) {
            gaps.extend(cargo_alias_gaps(text));
        }
    }
    Ok(gaps)
}

fn collect_cargo_configs(root: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    for path in tree_paths(root)? {
        let rel = path
            .strip_prefix(root)
            .map_err(|err| format!("cargo config path: {err}"))?;
        let name = rel.file_name().and_then(|part| part.to_str()).unwrap_or("");
        let parent_is_cargo = rel
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|part| part.to_str())
            == Some(".cargo");
        if parent_is_cargo && name.starts_with("config") {
            found.push(path);
            continue;
        }
        let symlink =
            std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink());
        if symlink && (name == ".cargo" || name.starts_with("config")) {
            found.push(path);
        }
    }
    Ok(())
}

fn cargo_alias_gaps(text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    let mut section = String::new();
    let mut saw_xtask = false;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            let name = name.trim();
            if name != "alias" {
                gaps.push(format!("cargo config section {name} is not allowed"));
            }
            section = name.to_owned();
            continue;
        }
        if section != "alias" {
            gaps.push(format!(
                "cargo config line {} is outside [alias]",
                index + 1
            ));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            gaps.push(format!("cargo config line {} is not an alias", index + 1));
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key != "xtask" {
            gaps.push(format!("cargo alias {key} is not allowed"));
            continue;
        }
        if saw_xtask {
            gaps.push("cargo alias xtask is duplicated".to_owned());
        }
        saw_xtask = true;
        if value != "\"run --package xtask --\"" {
            gaps.push(format!("cargo alias xtask is {value}"));
        }
    }
    if !saw_xtask {
        gaps.push("cargo alias xtask is missing".to_owned());
    }
    gaps
}

fn run_injection_gaps(name: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut gaps = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let indent = lines[idx].len() - lines[idx].trim_start().len();
        let trimmed = strip_yaml_comment(lines[idx]).trim();
        let Some(rest) = run_prefix(trimmed) else {
            idx += 1;
            continue;
        };
        if rest.contains("${{ inputs.day }}") || rest.contains("${{inputs.day}}") {
            gaps.push(format!("{name} interpolates inputs.day in run"));
        }
        let rest = rest.trim();
        if rest.is_empty() || matches!(rest, "|" | ">" | "|-" | ">-") {
            idx += 1;
            while idx < lines.len() {
                let child = lines[idx];
                if !child.trim().is_empty() {
                    let findent = child.len() - child.trim_start().len();
                    if findent <= indent {
                        break;
                    }
                }
                if child.contains("${{ inputs.day }}") || child.contains("${{inputs.day}}") {
                    gaps.push(format!("{name} interpolates inputs.day in run"));
                }
                idx += 1;
            }
            continue;
        }
        idx += 1;
    }
    gaps
}

fn guards_fetch_depth_is_zero(text: &str) -> bool {
    job_block(text, "guards").is_some_and(|block| {
        block
            .lines()
            .any(|line| strip_yaml_comment(line).trim() == "fetch-depth: 0")
    })
}

fn action_pin_result(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    for path in workflow_dir_files(root)? {
        if !workflow_yaml(&path) {
            continue;
        }
        let text = read(&path)?;
        violations.extend(action_pin_gaps(&text));
    }
    Ok(violations)
}

fn action_pin_gaps(text: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    for line in text.lines() {
        let code = strip_yaml_comment(line);
        let indent = line.len() - line.trim_start().len();
        let job_uses = indent == 4 && code.trim().starts_with("uses:");
        for uses in uses_values(line) {
            if job_uses {
                push_gap(
                    &mut gaps,
                    format!("reusable workflow {uses} is not allowed"),
                );
            }
            if !ALLOWED_ACTIONS.contains(&uses.as_str()) {
                gaps.push(format!("action {uses} is not allowlisted"));
            }
            if !action_is_sha_pinned(&uses) {
                gaps.push(format!("action {uses} is not pinned to a sha"));
            }
        }
    }
    gaps
}

fn uses_values(line: &str) -> Vec<String> {
    let code = strip_yaml_comment(line);
    let mut found = Vec::new();
    let mut rest = code;
    while let Some(idx) = find_uses_key(rest) {
        let after = &rest[idx + "uses:".len()..];
        let value = take_uses_value(after);
        if !value.is_empty() {
            found.push(value);
        }
        rest = &rest[idx + "uses:".len()..];
    }
    found
}

fn find_uses_key(text: &str) -> Option<usize> {
    let mut start = 0usize;
    while let Some(rel) = text[start..].find("uses:") {
        let idx = start + rel;
        let ok = match text[..idx].chars().next_back() {
            None => true,
            Some(ch) if ch.is_whitespace() || matches!(ch, '{' | ',' | '-') => true,
            Some(_) => false,
        };
        if ok {
            return Some(idx);
        }
        start = idx + "uses:".len();
    }
    None
}

fn take_uses_value(after: &str) -> String {
    let after = after.trim_start().trim_start_matches(['"', '\'']);
    let mut end = after.len();
    for (idx, ch) in after.char_indices() {
        if ch == ',' || ch == '}' || ch == '#' || ch.is_whitespace() {
            end = idx;
            break;
        }
    }
    after[..end].trim().trim_matches(['"', '\'']).to_owned()
}

fn action_is_sha_pinned(uses: &str) -> bool {
    let Some((repo, sha)) = uses.rsplit_once('@') else {
        return false;
    };
    !repo.is_empty()
        && repo.contains('/')
        && sha.len() == 40
        && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

enum ResolvedBaseline {
    Present(Floors),
    Missing { sha: String },
    Error(String),
}

fn ratchet_report(
    root: &Path,
    floor_base: Option<&str>,
    on_main: bool,
    floors: &Result<Floors, String>,
) -> GuardReport {
    match resolve_floor_base(root, floor_base, on_main) {
        ResolvedBaseline::Present(base) => match floors {
            Ok(current) => guard_from_violations("ratchet", ratchet(current, &base)),
            Err(err) => guard_failed("ratchet", err.clone()),
        },
        ResolvedBaseline::Missing { sha } if sha == BOOTSTRAP_BASE => {
            bootstrap_report(root, floors, on_main, &sha)
        }
        ResolvedBaseline::Missing { sha } => guard_failed(
            "ratchet",
            format!("baseline floors.toml is missing at {sha}"),
        ),
        ResolvedBaseline::Error(err) => guard_failed("ratchet", err),
    }
}

fn bootstrap_report(
    root: &Path,
    floors: &Result<Floors, String>,
    on_main: bool,
    base: &str,
) -> GuardReport {
    if let Err(err) = floors {
        return guard_failed("ratchet", err.clone());
    }
    let origin = match origin_main(root) {
        Ok(origin) => origin,
        Err(err) => return guard_failed("ratchet", err),
    };
    let landing = match bootstrap_landing(root, on_main, base, &origin.sha) {
        Ok(landing) => landing,
        Err(err) => return guard_failed("ratchet", err),
    };
    if origin.has_floors && !landing {
        return guard_failed(
            "ratchet",
            "bootstrap path is closed: origin/main contains regressions/floors.toml".to_owned(),
        );
    }
    match read_bytes(&root.join("regressions/floors.toml")) {
        Ok(bytes) if sha256_hex(&bytes) == BOOTSTRAP_FLOORS_SHA256 => {
            guard_from_violations("ratchet", Vec::new())
        }
        Ok(_) => guard_failed(
            "ratchet",
            "bootstrap floors.toml sha256 does not match".to_owned(),
        ),
        Err(err) => guard_failed("ratchet", err),
    }
}

struct OriginMain {
    sha: String,
    has_floors: bool,
}

fn origin_main(root: &Path) -> Result<OriginMain, String> {
    git(
        root,
        &[
            "fetch",
            "--no-tags",
            "origin",
            "main:refs/remotes/origin/main",
        ],
    )?;
    let sha = git(root, &["rev-parse", "refs/remotes/origin/main"])?;
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "cat-file",
            "-e",
            "refs/remotes/origin/main:regressions/floors.toml",
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|err| format!("git: {err}"))?;
    Ok(OriginMain {
        sha,
        has_floors: output.status.success(),
    })
}

fn bootstrap_landing(
    root: &Path,
    on_main: bool,
    base: &str,
    origin_sha: &str,
) -> Result<bool, String> {
    if !on_main || base != BOOTSTRAP_BASE {
        return Ok(false);
    }
    let head = git(root, &["rev-parse", "HEAD"])?;
    Ok(origin_sha == head)
}

fn resolve_floor_base(root: &Path, floor_base: Option<&str>, on_main: bool) -> ResolvedBaseline {
    let sha = match choose_baseline(floor_base.map(str::trim), on_main) {
        BaselineChoice::DirectPush => {
            return ResolvedBaseline::Error("direct push has no floor baseline".to_owned());
        }
        BaselineChoice::Unreadable => {
            return ResolvedBaseline::Error("floor baseline is unreadable".to_owned());
        }
        BaselineChoice::MergeBase => match merge_base_origin_main(root) {
            Ok(sha) => sha,
            Err(err) => return ResolvedBaseline::Error(err),
        },
        BaselineChoice::Sha(sha) => match trusted_floor_sha(root, &sha) {
            Ok(sha) => sha,
            Err(err) => return ResolvedBaseline::Error(err),
        },
    };
    if first_push_sentinel(&sha) {
        return ResolvedBaseline::Error("all-zero before is not a baseline".to_owned());
    }
    if baseline_sha_unreadable(&sha) {
        return ResolvedBaseline::Error("floor baseline is unreadable".to_owned());
    }
    let text = match git_show(root, &sha, "regressions/floors.toml") {
        Ok(text) => text,
        Err(err) => return ResolvedBaseline::Error(err),
    };
    if text.is_none() && sha == BOOTSTRAP_BASE {
        return ResolvedBaseline::Missing { sha };
    }
    match baseline_from_show(on_main, &sha, text.as_deref()) {
        Ok(floors) => ResolvedBaseline::Present(floors),
        Err(_) if text.is_none() && !on_main => ResolvedBaseline::Missing { sha },
        Err(err) => ResolvedBaseline::Error(err),
    }
}

fn choose_baseline(env_sha: Option<&str>, on_main: bool) -> BaselineChoice {
    match env_sha.map(str::trim) {
        None if on_main => BaselineChoice::DirectPush,
        None => BaselineChoice::MergeBase,
        Some(sha) if first_push_sentinel(sha) => BaselineChoice::MergeBase,
        Some(sha) if baseline_sha_unreadable(sha) => BaselineChoice::Unreadable,
        Some(sha) => BaselineChoice::Sha(sha.to_owned()),
    }
}

fn first_push_sentinel(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| byte == b'0')
}

fn baseline_sha_unreadable(sha: &str) -> bool {
    sha.is_empty()
        || !sha.chars().all(|ch| ch.is_ascii_hexdigit())
        || (sha.bytes().all(|byte| byte == b'0') && !first_push_sentinel(sha))
}

fn baseline_from_show(on_main: bool, sha: &str, text: Option<&str>) -> Result<Floors, String> {
    match text {
        Some(text) => parse_floors(text),
        None if on_main => Err(format!(
            "direct push baseline floors.toml is missing at {sha}"
        )),
        None => Err(format!("baseline floors.toml is missing at {sha}")),
    }
}

fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !value.is_empty() && value != "0",
        Err(_) => false,
    }
}

fn merge_base_origin_main(root: &Path) -> Result<String, String> {
    if git(root, &["rev-parse", "--is-shallow-repository"])? == "true" {
        git(root, &["fetch", "--no-tags", "--unshallow", "origin"])?;
    }
    git(
        root,
        &[
            "fetch",
            "--no-tags",
            "origin",
            "main:refs/remotes/origin/main",
        ],
    )?;
    let sha = git(root, &["merge-base", "HEAD", "refs/remotes/origin/main"])?;
    if sha.is_empty() || baseline_sha_unreadable(&sha) {
        return Err("git merge-base HEAD origin/main failed: no baseline commit".to_owned());
    }
    Ok(sha)
}

fn command_failure_detail(stderr: &str, stdout: &str, code: Option<i32>) -> String {
    let stderr = stderr.trim();
    let stdout = stdout.trim();
    if !stderr.is_empty() {
        stderr.to_owned()
    } else if !stdout.is_empty() {
        stdout.to_owned()
    } else {
        match code {
            Some(code) => format!("exit {code}"),
            None => "exit signal".to_owned(),
        }
    }
}

fn trusted_floor_sha(root: &Path, sha: &str) -> Result<String, String> {
    if first_push_sentinel(sha) {
        return Err("all-zero before is not a baseline".to_owned());
    }
    if baseline_sha_unreadable(sha) {
        return Err("floor baseline is unreadable".to_owned());
    }
    if trusted_ancestor(root, sha) {
        Ok(sha.to_owned())
    } else {
        merge_base_origin_main(root)
    }
}

fn trusted_ancestor(root: &Path, base: &str) -> bool {
    commit_present(root, base) && commit_is_ancestor(root, base)
}

fn commit_is_ancestor(root: &Path, base: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", base, "HEAD"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn commit_present(root: &Path, base: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", &format!("{base}^{{commit}}")])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|err| format!("git: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            command_failure_detail(
                &String::from_utf8_lossy(&output.stderr),
                &String::from_utf8_lossy(&output.stdout),
                output.status.code(),
            )
        ));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|err| format!("git output: {err}"))
}

fn git_show(root: &Path, base: &str, path: &str) -> Result<Option<String>, String> {
    if !commit_present(root, base) {
        return Err(format!("baseline commit {base} is not in this checkout"));
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", &format!("{base}:{path}")])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|err| format!("git show: {err}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    String::from_utf8(output.stdout)
        .map(Some)
        .map_err(|err| format!("git show output: {err}"))
}

fn expand_glob(root: &Path, glob: &str) -> Result<Vec<PathBuf>, String> {
    if glob.ends_with(".feature") && !glob.contains('*') {
        let path = root.join(glob);
        if path.is_file() {
            return Ok(vec![path]);
        }
        return Ok(Vec::new());
    }
    let (dir, recursive) = if let Some(dir) = glob.strip_suffix("/**/*.feature") {
        (dir, true)
    } else if let Some(dir) = glob.strip_suffix("/*.feature") {
        (dir, false)
    } else {
        return Err(format!("unsupported feature glob {glob}"));
    };
    let start = root.join(dir);
    if !start.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_features(&start, recursive, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_features(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|err| format!("read {}: {err}", dir.display()))?
            .path();
        if path.is_dir() {
            if recursive {
                collect_features(&path, true, out)?;
            }
        } else if path.extension().is_some_and(|ext| ext == "feature") {
            out.push(path);
        }
    }
    Ok(())
}

fn walk_features(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_features(dir, true, &mut files)?;
    files.sort();
    Ok(files)
}

fn rel_path(root: &Path, path: &Path) -> Result<String, String> {
    let root = root
        .canonicalize()
        .map_err(|err| format!("canonicalize {}: {err}", root.display()))?;
    let path = path
        .canonicalize()
        .map_err(|err| format!("canonicalize {}: {err}", path.display()))?;
    let rel = path
        .strip_prefix(&root)
        .map_err(|_| format!("{} is outside {}", path.display(), root.display()))?;
    Ok(rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))
}

fn required_path(args: &[String], flag: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(required_value(args, flag)?))
}

fn optional_path(args: &[String], flag: &str) -> Result<Option<PathBuf>, String> {
    Ok(optional_value(args, flag)?.map(PathBuf::from))
}

fn required_value(args: &[String], flag: &str) -> Result<String, String> {
    optional_value(args, flag)?.ok_or_else(|| format!("missing {flag}"))
}

fn optional_value(args: &[String], flag: &str) -> Result<Option<String>, String> {
    let Some(position) = args.iter().position(|arg| arg == flag) else {
        return Ok(None);
    };
    args.get(position + 1)
        .cloned()
        .map(Some)
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn count_needles(root: &Path, needle: &str) -> Result<u64, String> {
    let mut count = 0u64;
    for dir in ["crates", "xtask"] {
        count = count.saturating_add(count_in_dir(&root.join(dir), needle)?);
    }
    Ok(count)
}

fn count_in_dir(dir: &Path, needle: &str) -> Result<u64, String> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut count = 0u64;
    let entries = std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name == ".git" {
            continue;
        }
        if path.is_dir() {
            count = count.saturating_add(count_in_dir(&path, needle)?);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = read(&path)?;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains(needle) {
                count = count.saturating_add(1);
            }
        }
    }
    Ok(count)
}

/// Allow lines and clippy allow-keys that already exist on `origin/main`.
/// A file counts when it has a suppression outside this budget. Adding another
/// copy of a pinned line, or a new `allow-*` key, still counts.
const MAIN_SUPPRESSION_LINES: &[(&str, &str, usize)] = &[
    (
        "crates/backtest/src/metrics.rs",
        "#[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]",
        1,
    ),
    (
        "crates/backtest/tests/support.rs",
        "#![allow(dead_code)]",
        1,
    ),
    (
        "crates/collector/src/run.rs",
        "#[allow(clippy::too_many_arguments)]",
        2,
    ),
    (
        "crates/collector/src/store.rs",
        "#[allow(clippy::too_many_arguments)]",
        2,
    ),
];

const MAIN_CLIPPY_ALLOW_KEYS: &[(&str, &str)] =
    &[("crates/signals/clippy.toml", "allow-unwrap-in-tests")];

fn suppression_count(root: &Path) -> Result<u64, String> {
    let mut count = 0u64;
    count_suppressions(root, root, &mut count)?;
    Ok(count)
}

fn count_suppressions(root: &Path, dir: &Path, count: &mut u64) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name == ".git" {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if relative.starts_with("fixtures/lint") {
            continue;
        }
        if path.is_dir() {
            count_suppressions(root, &path, count)?;
            continue;
        }
        if name == "clippy.toml" {
            let parsed: toml::Value = toml::from_str(&read(&path)?)
                .map_err(|err| format!("parse {}: {err}", path.display()))?;
            let mut keys = Vec::new();
            allow_keys(&parsed, &mut keys);
            let rel = path_slash(relative);
            keys.retain(|key| !main_clippy_allow(&rel, key));
            if !keys.is_empty() {
                *count = count.saturating_add(1);
            }
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = read(&path)?;
        if file_has_unpinned_suppression(&path_slash(relative), &text) {
            *count = count.saturating_add(1);
        }
    }
    Ok(())
}

fn allow_keys(value: &toml::Value, found: &mut Vec<String>) {
    let toml::Value::Table(table) = value else {
        return;
    };
    for (key, child) in table {
        if key.starts_with("allow-") {
            found.push(key.clone());
        }
        allow_keys(child, found);
    }
}

fn line_has_suppression(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with("//") {
        return false;
    }
    let compact: String = strip_quotes(trimmed)
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    compact.contains("#[allow")
        || compact.contains("#![allow")
        || compact.contains("#[expect")
        || compact.contains("#![expect")
}

fn path_slash(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn main_clippy_allow(relative: &str, key: &str) -> bool {
    MAIN_CLIPPY_ALLOW_KEYS
        .iter()
        .any(|(path, allowed)| *path == relative && *allowed == key)
}

/// Before/after segment fixture. `Ok(Some(name))` is the invariant that broke
/// and matched `expect`. `Ok(None)` means `expect=none` and the pair holds.
fn segment_fixture_fault(text: &str) -> Result<Option<&'static str>, String> {
    let fields = segment_fields(text)?;
    let before = segment_side(&fields, "before")?;
    let after = segment_side(&fields, "after")?;
    let broken = segment_step_fault(&before, &after);
    let expect = segment_required(&fields, "expect")?;
    match broken {
        Some(name) if name == expect => Ok(Some(name)),
        Some(_) => Err("segment fixture: expected invariant was not the one that broke".to_owned()),
        None if expect == "none" => Ok(None),
        None => Err("segment fixture: expected invariant was not the one that broke".to_owned()),
    }
}

struct SegmentSide {
    max: u64,
    gap: bool,
    last_u: u64,
    accepted: Vec<u64>,
    gap_ids: Vec<u64>,
}

fn segment_fields(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut fields = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| "segment fixture: missing field".to_owned())?;
        fields.push((key.trim().to_owned(), value.trim().to_owned()));
    }
    Ok(fields)
}

fn segment_required(fields: &[(String, String)], key: &str) -> Result<String, String> {
    fields
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| "segment fixture: missing field".to_owned())
}

fn segment_u64(fields: &[(String, String)], key: &str) -> Result<u64, String> {
    segment_required(fields, key)?
        .parse()
        .map_err(|_| "segment fixture: bad integer".to_owned())
}

fn segment_ids(fields: &[(String, String)], key: &str) -> Result<Vec<u64>, String> {
    let text = segment_required(fields, key)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|part| {
            part.parse()
                .map_err(|_| "segment fixture: bad integer".to_owned())
        })
        .collect()
}

fn segment_side(fields: &[(String, String)], prefix: &str) -> Result<SegmentSide, String> {
    let max = segment_u64(fields, "max")?;
    if max < 2 {
        return Err("segment fixture: max is below 2".to_owned());
    }
    let phase = segment_required(fields, &format!("{prefix}.phase"))?;
    let gap = match phase.as_str() {
        "open" => false,
        "gap" => true,
        _ => return Err("segment fixture: phase is not open or gap".to_owned()),
    };
    Ok(SegmentSide {
        max,
        gap,
        last_u: segment_u64(fields, &format!("{prefix}.last_u"))?,
        accepted: segment_ids(fields, &format!("{prefix}.accepted"))?,
        gap_ids: segment_ids(fields, &format!("{prefix}.gap"))?,
    })
}

fn segment_state_fault(side: &SegmentSide) -> Option<&'static str> {
    if side.last_u > side.max {
        return Some("TypeOK");
    }
    if side
        .accepted
        .iter()
        .chain(side.gap_ids.iter())
        .any(|id| *id == 0 || *id > side.max)
    {
        return Some("TypeOK");
    }
    let mut previous: Option<u64> = None;
    for id in &side.accepted {
        if let Some(prev) = previous {
            match prev.checked_add(1) {
                Some(next) if next == *id => {}
                _ => return Some("Contiguous"),
            }
        }
        if side.accepted.iter().filter(|other| *other == id).count() > 1 {
            return Some("NoRepeatedId");
        }
        previous = Some(*id);
    }
    if side.gap && side.accepted != side.gap_ids {
        return Some("GapHoldsSnapshot");
    }
    None
}

fn segment_step_fault(before: &SegmentSide, after: &SegmentSide) -> Option<&'static str> {
    if let Some(broken) = segment_state_fault(before) {
        return Some(broken);
    }
    if let Some(broken) = segment_state_fault(after) {
        return Some(broken);
    }
    if before.gap && after.gap && before.accepted != after.accepted {
        return Some("NoAppendWhileGap");
    }
    None
}

fn file_has_unpinned_suppression(relative: &str, text: &str) -> bool {
    let mut budget: Vec<(&str, usize)> = MAIN_SUPPRESSION_LINES
        .iter()
        .filter(|(path, _, _)| *path == relative)
        .map(|(_, line, count)| (*line, *count))
        .collect();
    for line in text.lines() {
        if !line_has_suppression(line) {
            continue;
        }
        let trimmed = line.trim();
        let Some(slot) = budget
            .iter_mut()
            .find(|(pinned, left)| *pinned == trimmed && *left > 0)
        else {
            return true;
        };
        slot.1 = slot.1.saturating_sub(1);
    }
    false
}

fn strip_quotes(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            while let Some(inner) = chars.next() {
                if inner == '\\' {
                    let _ = chars.next();
                    continue;
                }
                if inner == '"' {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn rule_rows(text: &str) -> Result<u64, String> {
    let start = text
        .find("## Rule → enforcement")
        .ok_or("AGENTS.md has no rule table")?;
    let rest = &text[start..];
    let body = rest.split("\n## ").next().unwrap_or(rest);
    let mut count = 0u64;
    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.contains("---") {
            continue;
        }
        if line.contains("Where it is forced") {
            continue;
        }
        count = count.saturating_add(1);
    }
    Ok(count)
}

/// Reviewer-protected. sha256 of every closure file except the trust root
/// `xtask/src/guards.rs`. TLA models are inputs to TLC, not executed scripts,
/// so they are not pins.
const CLOSURE_PINS: &[(&str, &str)] = &[
    (
        ".cargo/config.toml",
        "783fdc447abb5b8f8b5652157aa9ede64da640813fa89679c29c5251c8e0cbe6",
    ),
    (
        ".github/workflows/gate_results.py",
        "67f96791665fc8b870c26e70c16d62a06cfa127e7e1dc9f86974654c0f567a24",
    ),
    (
        "Cargo.toml",
        "aa107d267f220f4269ddefd0005043d1a6cd9e505de4e0d057deeb63c80aa63b",
    ),
    (
        "xtask/Cargo.toml",
        "42a9ecbae2259630a8dd6daf2796cd04aaf65f0e926986f96aa1b624a0130896",
    ),
    (
        "xtask/src/depgraph.rs",
        "2a6dcebaa2bfb17c594b281dc667488d6c742a7bf04a70e2ae8510d881f4b0ef",
    ),
    (
        "xtask/src/gate_fixture_step.txt",
        "f89ee58483375c617c8cb37522e99b3c3586addb9cf6b71a7aad19bedaca0f0f",
    ),
    (
        "xtask/src/lib.rs",
        "66a1876f3cb3728f267db7b0ab3ac1b9841ad2634b030507d1e2b2e11b326683",
    ),
    (
        "xtask/src/lints.rs",
        "62fe583b2c533dd5a5d8a62a6738ee537af1fc08b7f1fe995b092bbcbddbfa68",
    ),
    (
        "xtask/src/main.rs",
        "de0aa384a5f0100cd490f29b6ae0ea44d69395b5f9e9067c7d7faa546de1fea5",
    ),
    (
        "xtask/src/pinned_jobs/ci.header",
        "e70f3eb0e8ab1ec9cb815c23ee523d4cf29bd82824f34cdaf07e83468bff5671",
    ),
    (
        "xtask/src/pinned_jobs/changes.job",
        "3c01b3bbc841341ebf4a8ad837048aadfcefd7914d40f515226c7639845c8bd2",
    ),
    (
        "xtask/src/pinned_jobs/clippy.job",
        "8ea34e7b3dc7a05f07669ba2eb74e294ecee5dad3fa8e9f3c44747578535a595",
    ),
    (
        "xtask/src/pinned_jobs/cucumber.job",
        "6998c905a4900a0f86652775f7e6f0b1989c858f0782f7e2392e44fa91335be2",
    ),
    (
        "xtask/src/pinned_jobs/deny.job",
        "7093a5d12c7a3341f5381b8619c1f07b44e403e495c1f4fccdcdc07160c8a58e",
    ),
    (
        "xtask/src/pinned_jobs/depgraph.job",
        "548ac0a08768f080287437bdb53b314291eee5e85d6abd16851ca57e52dd9c58",
    ),
    (
        "xtask/src/pinned_jobs/fmt.job",
        "acccaa8b48519fcd2d982011ef693eb4b926388b7cc9f27d4d9f4ab60c1e84bf",
    ),
    (
        "xtask/src/pinned_jobs/gate.job",
        "1af2588254a4e34454b5636408978164be1a78e728ea2002cd83cc71af313153",
    ),
    (
        "xtask/src/pinned_jobs/guard-fixtures.job",
        "4ef405352108d9f22fcf9741dcb65dbc8f8451ab7cbb7a21f5af774fa93d4e59",
    ),
    (
        "xtask/src/pinned_jobs/guards.job",
        "6fd148f17910d86586d13a8435d809c07e46ea18c3db3e1f0f5341950d081e3d",
    ),
    (
        "xtask/src/pinned_jobs/kani.job",
        "72d4932b7331e414e6cf16f701c6b75462725ed6436967f15e32e616bec8dd64",
    ),
    (
        "xtask/src/pinned_jobs/lint-fixture.job",
        "e2fd8711baa3796507ef2011afa25bdf7d4baff51ed489df32d0a2aa8955862d",
    ),
    (
        "xtask/src/pinned_jobs/test.job",
        "fa3dddbb33063df6a725f0f0d583ee2822d0aab80644f7480873af22e781aeaa",
    ),
    (
        "xtask/src/pinned_jobs/tla.job",
        "80a41530941e1ff9ff4e9f6439649db724ca8ab35ad5bfb96aa36fb16fe8b822",
    ),
];

/// Reviewer-protected. Count of `printf '%s\n' "$var" | grep -q '<lit>'`
/// statements that are not under `pipefail`.
const PRINTF_GREP_STATEMENTS: usize = 35;

/// Reviewer-protected.
const CUCUMBER_CRATE: (&str, &str, &str) = (
    "cucumber",
    "0.22.1",
    "16cbb27bc2064274afa3a3d8bc9a0e71333589850573aa632ec4520e4af14d94",
);

/// Reviewer-protected.
const GHERKIN_CRATE: (&str, &str, &str) = (
    "gherkin",
    "0.15.0",
    "70197ce7751bfe8bc828e3a855502d3a869a1e9416b58b10c4bde5cf8a0a3cb3",
);

const PINNED_CARGO: &str =
    "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo";

/// Reviewer-protected. Both P3 `case` shapes on the final head. Pending Reviewer review.
const CASE_LITERAL_MATCHES: usize = 2;

const ENUMERATED_MATCH_FORMS: &[&str] = &[
    "grep -qF -- '<lit>' \"$f\"",
    "case \"$v\" in *'<lit>'*) body ;; esac",
    "case \"$v\" in *'<lit>'*) ;; *) body ;; esac",
];

const API_SNAPSHOT_POSTCHECK: &str = "\
test \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\n\
git diff --exit-code -- crates/core/tests/snapshots/\n\
test -z \"$(git status --porcelain -- crates/core/tests/snapshots/)\"\n";

const H2_INSTALL: &str = "\
rustup toolchain install \"$PUBLIC_API_NIGHTLY\" --profile minimal\n\
for i in 1 2; do\n\
  sleep 30\n\
done\n";

macro_rules! env_a {
    ($($tail:literal)*) => {
        concat!(
            "env -i HOME=/home/runner PATH=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin CARGO_HOME=/home/runner/.cargo RUSTUP_HOME=/home/runner/.rustup",
            $($tail)*
        )
    };
}

macro_rules! env_b {
    ($($tail:literal)*) => {
        concat!(
            "env -i HOME=/home/runner PATH=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin CARGO_HOME=/home/runner/.cargo RUSTUP_HOME=/home/runner/.rustup CARGO=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo RUSTC=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc",
            $($tail)*
        )
    };
}

/// Reviewer-protected. The distinct `env -i` commands, one normalized line each.
const ENV_I_TEMPLATES: &[&str] = &[
    env_a!(" /usr/bin/curl --fail --location --silent --show-error --output \"$tarball\" https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz"),
    env_a!(" /usr/bin/sha256sum \"$tarball\""),
    env_a!(" /usr/bin/tar -xzf \"$tarball\" -C /tmp"),
    env_a!(" /usr/bin/sha256sum /tmp/cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny"),
    env_a!(" /usr/bin/sha256sum deny.toml"),
    env_a!(" /usr/bin/sha256sum Cargo.lock"),
    env_a!(" /usr/bin/sha256sum rust-toolchain.toml"),
    env_a!(" /usr/bin/sha256sum Cargo.toml crates/collector/Cargo.toml crates/core/Cargo.toml crates/execution/Cargo.toml crates/regime/Cargo.toml crates/signals/Cargo.toml crates/sim/Cargo.toml xtask/Cargo.toml"),
    env_a!(" /usr/bin/sha256sum"),
    env_a!(" /usr/bin/sha256sum .cargo/config.toml"),
    env_b!(" /tmp/cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny check"),
    env_b!(" /tmp/cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny --manifest-path fixtures/deny/banned-reqwest/Cargo.toml --config deny.toml check bans 2>&1"),
    env_b!(" /home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo xtask guards toolchain-files"),
];

/// Reviewer-protected. `(trailer, count)` aligned with `ENV_I_TEMPLATES` in E1–E13 order.
/// Trailers are the shell bytes after the command, with continuation indent removed.
/// E13 is the stack head's thirteenth template; its count is a protected change.
const ENV_I_OCCURRENCES: &[(&str, usize)] = &[
    ("", 1),
    (")\"", 1),
    ("", 1),
    (")\"", 3),
    (")\"", 2),
    (")\"", 1),
    (")\"", 1),
    (" \\\n| ", 1),
    (")\"", 1),
    (")\"", 2),
    ("", 1),
    (" 2>&1)", 1),
    ("", 2),
];

/// Reviewer-protected. The T2 heredoc from the `python3` line through `PY`,
/// with the 10-space YAML indent removed and the relative indent kept.
const PY_HEREDOC_TEMPLATE: &str = r#"python3 - "$copy" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
path = root / ".github/workflows/ci.yml"
text = path.read_text()
needle = "    name: fmt\n"
at = text.find(needle)
if at < 0:
    raise SystemExit("fmt job marker missing")
byte_at = at + len("    name: fm")
chars = list(text)
if chars[byte_at] != "t":
    raise SystemExit("fmt name byte is not t")
chars[byte_at] = "u"
path.write_text("".join(chars))
PY
"#;

/// Reviewer-protected. `(step, package, kind, target, N)`.
/// `step` is `(job id, step name)`. A `0` count is a binary whose log says
/// `running 0 tests`.
type StepPair = (&'static str, &'static str);
type TestBinaryCount = (StepPair, &'static str, &'static str, &'static str, usize);
const TEST_BINARY_COUNTS: &[TestBinaryCount] = &[
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-collector",
        "lib",
        "unittests src/lib.rs (target/debug/deps/parangsae_collector-",
        47,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-collector",
        "bin",
        "unittests src/bin/parangsae-collect.rs",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-collector",
        "bin",
        "unittests src/bin/segment-invariants.rs",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-collector",
        "test",
        "tests/no_order_client.rs",
        1,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-collector",
        "test",
        "tests/verify_filters.rs",
        5,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-collector",
        "test",
        "tests/vision_integrity.rs",
        29,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-core",
        "lib",
        "unittests src/lib.rs (target/debug/deps/parangsae_core-",
        19,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-core",
        "test",
        "tests/feature_reject.rs",
        1,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-core",
        "test",
        "tests/instrument_rules.rs",
        3,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-core",
        "test",
        "tests/instrument_spec.rs",
        13,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-core",
        "test",
        "tests/kline_integrity.rs",
        12,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-core",
        "test",
        "tests/workspace_lints.rs",
        4,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-execution",
        "lib",
        "unittests src/lib.rs (target/debug/deps/parangsae_execution-",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-execution",
        "test",
        "tests/no_live_client.rs",
        1,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-regime",
        "lib",
        "unittests src/lib.rs (target/debug/deps/parangsae_regime-",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-signals",
        "lib",
        "unittests src/lib.rs (target/debug/deps/parangsae_signals-",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-signals",
        "test",
        "tests/producer_gate.rs",
        1,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-sim",
        "lib",
        "unittests src/lib.rs (target/debug/deps/parangsae_sim-",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "xtask",
        "lib",
        "unittests src/lib.rs (target/debug/deps/xtask-",
        93,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "xtask",
        "bin",
        "unittests src/main.rs (target/debug/deps/xtask-",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-collector",
        "doctest",
        "Doc-tests parangsae_collector",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-core",
        "doctest",
        "Doc-tests parangsae_core",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-execution",
        "doctest",
        "Doc-tests parangsae_execution",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-regime",
        "doctest",
        "Doc-tests parangsae_regime",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-signals",
        "doctest",
        "Doc-tests parangsae_signals",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "parangsae-sim",
        "doctest",
        "Doc-tests parangsae_sim",
        0,
    ),
    (
        (
            "test",
            "cargo test (unit, proptest, cucumber, execution lock)",
        ),
        "xtask",
        "doctest",
        "Doc-tests xtask",
        0,
    ),
    (
        ("test", "test-support constructors"),
        "parangsae-core",
        "lib",
        "unittests src/lib.rs (target/debug/deps/parangsae_core-",
        20,
    ),
    (
        ("test", "causal gate compile-fail"),
        "closed-gate",
        "lib",
        "deps/closed_gate-",
        0,
    ),
    (
        ("test", "causal gate compile-fail"),
        "closed-gate",
        "test",
        "tests/gate_closed.rs",
        1,
    ),
    (
        ("test", "causal gate compile-fail"),
        "closed-gate",
        "doctest",
        "Doc-tests closed_gate",
        0,
    ),
];

#[derive(Clone, Copy)]
struct BinaryRow {
    job: &'static str,
    step: &'static str,
    package: &'static str,
    kind: &'static str,
    target: &'static str,
    count: usize,
}

fn binary_rows() -> Vec<BinaryRow> {
    TEST_BINARY_COUNTS
        .iter()
        .map(|(step, package, kind, target, count)| BinaryRow {
            job: step.0,
            step: step.1,
            package,
            kind,
            target,
            count: *count,
        })
        .collect()
}

#[derive(Default)]
struct ShellReport {
    gaps: Vec<String>,
    listing: Vec<String>,
    env_forms: Vec<String>,
    printf_grep: usize,
    heredocs: Vec<String>,
    facts: Vec<CmdFact>,
    p3_body: usize,
    p3_default: usize,
}

#[derive(Clone, Copy)]
struct CmdFact {
    test_running: bool,
    self_checked: bool,
    kani: bool,
    teed: bool,
    captured: bool,
    in_pipeline: bool,
    suppressed: bool,
    pipefail: bool,
}

struct Cmd {
    argv: Vec<String>,
    display: String,
}

struct Cur<'a> {
    src: &'a str,
    i: usize,
}

impl<'a> Cur<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, i: 0 }
    }

    fn eof(&self) -> bool {
        self.i >= self.src.len()
    }

    fn rest(&self) -> &'a str {
        self.src.get(self.i..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn starts(&self, token: &str) -> bool {
        self.rest().starts_with(token)
    }

    fn bump(&mut self) {
        if let Some(ch) = self.peek() {
            self.i = self.i.saturating_add(ch.len_utf8());
        }
    }

    fn bump_str(&mut self, token: &str) {
        if self.starts(token) {
            self.i = self.i.saturating_add(token.len());
        }
    }

    fn skip_inline(&mut self) {
        loop {
            if self.starts("\\\n") {
                self.bump_str("\\\n");
                continue;
            }
            if self.starts("\\\r\n") {
                self.bump_str("\\\r\n");
                continue;
            }
            match self.peek() {
                Some(' ' | '\t' | '\r') => self.bump(),
                _ => break,
            }
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some('#') {
            while self.peek().is_some_and(|ch| ch != '\n') {
                self.bump();
            }
        }
    }

    fn starts_kw(&self, kw: &str) -> bool {
        let rest = self.rest();
        let Some(after) = rest.strip_prefix(kw) else {
            return false;
        };
        after
            .chars()
            .next()
            .is_none_or(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r' | ';' | '|' | '&' | ')' | '('))
    }
}

struct Ctx<'a> {
    step_key: &'a str,
    file_line: usize,
    pipefail: bool,
    captured: bool,
    in_if_cond: bool,
    status_vars: BTreeSet<String>,
    report: ShellReport,
}

impl<'a> Ctx<'a> {
    fn gap(&mut self, message: &str) {
        push_gap(&mut self.report.gaps, message.to_owned());
    }

    fn line_of(&self, src: &str, index: usize) -> usize {
        let prefix = src.get(..index).unwrap_or("");
        self.file_line
            .saturating_add(prefix.bytes().filter(|byte| *byte == b'\n').count())
    }
}

fn parse_run_script(script: &str, step_key: &str, file_line: usize) -> ShellReport {
    let mut ctx = Ctx {
        step_key,
        file_line,
        pipefail: false,
        captured: false,
        in_if_cond: false,
        status_vars: BTreeSet::new(),
        report: ShellReport::default(),
    };
    let mut cur = Cur::new(script);
    parse_body(&mut cur, &mut ctx);
    finish_script(script, &mut ctx);
    ctx.report
}

fn parse_body(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    loop {
        skip_separators(cur);
        if cur.eof() {
            break;
        }
        let before = cur.i;
        parse_stmt(cur, ctx);
        if cur.i == before {
            ctx.gap("unparseable run step");
            cur.bump();
        }
    }
}

fn skip_separators(cur: &mut Cur<'_>) {
    loop {
        cur.skip_inline();
        if cur.peek() == Some('#') {
            cur.skip_comment();
            continue;
        }
        if cur.peek() == Some('\n') {
            cur.bump();
            continue;
        }
        break;
    }
}

fn parse_stmt(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    cur.skip_inline();
    if cur.eof() || cur.peek() == Some('\n') {
        return;
    }
    if cur.starts_kw("if") {
        parse_if(cur, ctx);
        return;
    }
    if cur.starts_kw("for") {
        parse_for(cur, ctx);
        return;
    }
    if cur.starts_kw("case") {
        parse_case(cur, ctx);
        return;
    }
    if cur.starts_kw("while") || cur.starts_kw("until") {
        ctx.gap("unclassified command while");
        skip_line(cur);
        return;
    }
    parse_list(cur, ctx);
}

fn parse_if(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    cur.bump_str("if");
    ctx.in_if_cond = true;
    parse_condition(cur, ctx);
    ctx.in_if_cond = false;
    cur.skip_inline();
    if cur.peek() == Some(';') && !cur.starts(";;") {
        cur.bump();
    }
    skip_separators(cur);
    if !consume_kw(cur, "then") {
        ctx.gap("unparseable run step");
        return;
    }
    parse_until(cur, ctx, &["fi", "else", "elif"]);
    if cur.starts_kw("else") {
        cur.bump_str("else");
        parse_until(cur, ctx, &["fi"]);
    }
    while cur.starts_kw("elif") {
        cur.bump_str("elif");
        ctx.in_if_cond = true;
        parse_condition(cur, ctx);
        ctx.in_if_cond = false;
        cur.skip_inline();
        if cur.peek() == Some(';') && !cur.starts(";;") {
            cur.bump();
        }
        skip_separators(cur);
        if !consume_kw(cur, "then") {
            ctx.gap("unparseable run step");
            return;
        }
        parse_until(cur, ctx, &["fi", "else", "elif"]);
    }
    if !consume_kw(cur, "fi") {
        ctx.gap("unparseable run step");
    }
}

fn parse_for(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    cur.bump_str("for");
    cur.skip_inline();
    let _name = scan_word(cur, ctx);
    cur.skip_inline();
    if !consume_kw(cur, "in") {
        ctx.gap("unparseable run step");
        return;
    }
    loop {
        cur.skip_inline();
        if cur.eof() || cur.starts_kw("do") || (cur.peek() == Some(';') && !cur.starts(";;")) {
            break;
        }
        if cur.peek() == Some('\n') {
            break;
        }
        let before = cur.i;
        let _word = scan_word(cur, ctx);
        if cur.i == before {
            ctx.gap("unparseable run step");
            cur.bump();
            break;
        }
    }
    cur.skip_inline();
    if cur.peek() == Some(';') && !cur.starts(";;") {
        cur.bump();
    }
    skip_separators(cur);
    if !consume_kw(cur, "do") {
        ctx.gap("unparseable run step");
        return;
    }
    parse_until(cur, ctx, &["done"]);
    if !consume_kw(cur, "done") {
        ctx.gap("unparseable run step");
    }
}

fn parse_case(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    let line = ctx.line_of(cur.src, cur.i);
    cur.bump_str("case");
    cur.skip_inline();
    let subject = scan_word(cur, ctx);
    skip_separators(cur);
    if !consume_kw(cur, "in") || !is_p3_subject(&subject) {
        ctx.gap("case form is not a p3 match");
        skip_until_esac(cur);
        return;
    }
    skip_separators(cur);
    let pattern = scan_word(cur, ctx);
    cur.skip_inline();
    if cur.peek() != Some(')') || !is_p3_pattern(&pattern) {
        ctx.gap("case form is not a p3 match");
        skip_until_esac(cur);
        return;
    }
    cur.bump();
    cur.skip_inline();
    if cur.starts(";;") {
        cur.bump_str(";;");
        if !parse_p3_default_arm(cur, ctx) {
            ctx.gap("case form is not a p3 match");
            skip_until_esac(cur);
            return;
        }
        ctx.report.p3_default = ctx.report.p3_default.saturating_add(1);
        ctx.report
            .listing
            .push(format!("{} L{line} case :: match:p3-default", ctx.step_key));
        return;
    }
    parse_until(cur, ctx, &["esac"]);
    cur.skip_inline();
    if !cur.starts(";;") {
        ctx.gap("case form is not a p3 match");
        skip_until_esac(cur);
        return;
    }
    cur.bump_str(";;");
    skip_separators(cur);
    if !consume_kw(cur, "esac") {
        ctx.gap("case form is not a p3 match");
        skip_until_esac(cur);
        return;
    }
    ctx.report.p3_body = ctx.report.p3_body.saturating_add(1);
    ctx.report
        .listing
        .push(format!("{} L{line} case :: match:p3-body", ctx.step_key));
}

fn is_p3_subject(word: &str) -> bool {
    let Some(inner) = word
        .strip_prefix("\"$")
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return false;
    };
    !inner.is_empty()
        && inner
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_p3_pattern(word: &str) -> bool {
    let Some(inner) = word.strip_prefix("*'") else {
        return false;
    };
    let Some(inner) = inner.strip_suffix("'*") else {
        return false;
    };
    !inner.is_empty() && !inner.contains('\'')
}

fn parse_p3_default_arm(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) -> bool {
    skip_separators(cur);
    let star = scan_word(cur, ctx);
    cur.skip_inline();
    if star != "*" || cur.peek() != Some(')') {
        return false;
    }
    cur.bump();
    parse_until(cur, ctx, &["esac"]);
    cur.skip_inline();
    if !cur.starts(";;") {
        return false;
    }
    cur.bump_str(";;");
    skip_separators(cur);
    consume_kw(cur, "esac")
}

fn skip_until_esac(cur: &mut Cur<'_>) {
    while !cur.eof() && !cur.starts_kw("esac") {
        cur.bump();
    }
    if cur.starts_kw("esac") {
        cur.bump_str("esac");
    }
}

fn parse_until(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>, stop: &[&str]) {
    loop {
        skip_separators(cur);
        if cur.eof() || stop.iter().any(|kw| cur.starts_kw(kw)) || cur.starts(";;") {
            return;
        }
        let before = cur.i;
        parse_stmt(cur, ctx);
        if cur.i == before {
            ctx.gap("unparseable run step");
            cur.bump();
        }
    }
}

fn parse_condition(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    parse_pipeline(cur, ctx);
    loop {
        cur.skip_inline();
        if cur.starts("&&") || cur.starts("||") {
            cur.bump_str(if cur.starts("&&") { "&&" } else { "||" });
            parse_pipeline(cur, ctx);
            continue;
        }
        break;
    }
}

fn parse_list(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    parse_pipeline(cur, ctx);
    loop {
        cur.skip_inline();
        if cur.starts(";;") {
            break;
        }
        if cur.starts("&&") || cur.starts("||") {
            let op = if cur.starts("&&") { "&&" } else { "||" };
            cur.bump_str(op);
            parse_pipeline(cur, ctx);
            continue;
        }
        if cur.peek() == Some(';') {
            cur.bump();
            cur.skip_inline();
            if cur.eof()
                || cur.peek() == Some('\n')
                || cur.starts_kw("then")
                || cur.starts_kw("do")
                || cur.starts_kw("fi")
                || cur.starts_kw("done")
                || cur.starts_kw("else")
                || cur.starts_kw("elif")
                || cur.starts_kw("esac")
            {
                break;
            }
            parse_pipeline(cur, ctx);
            continue;
        }
        break;
    }
}

fn parse_pipeline(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    cur.skip_inline();
    let negated = at_bang(cur);
    if negated {
        cur.bump();
        cur.skip_inline();
    }
    let fact_at = ctx.report.facts.len();
    let mut stages = Vec::new();
    stages.push(parse_simple(cur, ctx));
    loop {
        cur.skip_inline();
        if cur.peek() == Some('|') && !cur.starts("||") {
            cur.bump();
            stages.push(parse_simple(cur, ctx));
            continue;
        }
        break;
    }
    classify_pipeline(ctx, negated, fact_at, &stages);
}

fn parse_simple(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) -> Cmd {
    let mut argv = Vec::new();
    let mut redirs = Vec::new();
    let mut heredoc: Option<(String, usize)> = None;
    let mut started = false;
    loop {
        cur.skip_inline();
        if simple_stops(cur) {
            break;
        }
        if cur.starts("<<<") {
            cur.bump_str("<<<");
            cur.skip_inline();
            if !simple_stops(cur) {
                let _body = scan_word(cur, ctx);
            }
            ctx.gap("here-string is not accepted");
            continue;
        }
        if cur.starts("<<") {
            let mark = cur.i;
            let delim = scan_heredoc_delim(cur);
            heredoc = Some((delim, mark));
            continue;
        }
        if at_redir(cur) {
            redirs.push(scan_redir(cur));
            continue;
        }
        let before = cur.i;
        let word = scan_word(cur, ctx);
        if cur.i == before {
            ctx.gap("unparseable run step");
            cur.bump();
            break;
        }
        if argv.is_empty() && is_name_assignment(&word) {
            note_status_assignment(ctx, &word);
            continue;
        }
        if argv.is_empty() && cur.peek() == Some('(') {
            ctx.gap("functions defined in the step are not accepted");
            skip_line(cur);
            break;
        }
        if !started {
            started = true;
        }
        argv.push(word);
    }
    let mut heredoc_text = None;
    if let Some((delim, _)) = heredoc {
        cur.skip_inline();
        if cur.peek() == Some('\n') {
            cur.bump();
        }
        let body_from = argv_origin(cur, &argv);
        consume_heredoc_body(cur, &delim);
        if let Some(from) = body_from {
            heredoc_text = cur.src.get(from..cur.i).map(str::to_owned);
        }
    }
    let display = command_display(&argv, &redirs);
    let cmd = Cmd { argv, display };
    classify_simple(ctx, &cmd, heredoc_text.as_deref(), cur);
    cmd
}

fn argv_origin(cur: &Cur<'_>, argv: &[String]) -> Option<usize> {
    let first = argv.first()?;
    cur.src
        .get(..cur.i)
        .and_then(|prefix| prefix.rfind(first.as_str()))
}

fn consume_heredoc_body(cur: &mut Cur<'_>, delim: &str) {
    loop {
        if cur.eof() {
            return;
        }
        let start = cur.i;
        while cur.peek().is_some_and(|ch| ch != '\n') {
            cur.bump();
        }
        let line = cur.src.get(start..cur.i).unwrap_or("");
        let newline = cur.peek() == Some('\n');
        if newline {
            cur.bump();
        }
        if line.trim() == delim {
            return;
        }
        if !newline {
            return;
        }
    }
}

fn simple_stops(cur: &Cur<'_>) -> bool {
    if cur.eof() {
        return true;
    }
    if cur.starts(";;") || cur.starts("&&") || cur.starts("||") {
        return true;
    }
    if matches!(cur.peek(), Some('\n' | ';' | '|' | ')' | '&')) {
        return true;
    }
    ["then", "do", "done", "fi", "else", "elif", "esac"]
        .iter()
        .any(|kw| cur.starts_kw(kw))
}

fn at_bang(cur: &Cur<'_>) -> bool {
    let rest = cur.rest();
    let Some(after) = rest.strip_prefix('!') else {
        return false;
    };
    after
        .chars()
        .next()
        .is_none_or(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r'))
}

fn at_redir(cur: &Cur<'_>) -> bool {
    let rest = cur.rest();
    if rest.starts_with("<<") {
        return false;
    }
    let bytes = rest.as_bytes();
    let mut index = 0usize;
    while bytes.get(index).is_some_and(|byte| byte.is_ascii_digit()) {
        index = index.saturating_add(1);
    }
    matches!(bytes.get(index), Some(b'>' | b'<'))
}

fn scan_redir(cur: &mut Cur<'_>) -> String {
    let start = cur.i;
    while cur.peek().is_some_and(|ch| ch.is_ascii_digit()) {
        cur.bump();
    }
    if cur.starts(">>") {
        cur.bump_str(">>");
    } else if cur.peek() == Some('>') || cur.peek() == Some('<') {
        cur.bump();
    }
    if cur.peek() == Some('&') {
        cur.bump();
    }
    while cur
        .peek()
        .is_some_and(|ch| !matches!(ch, ' ' | '\t' | '\n' | ';' | '|' | ')' | '&'))
    {
        cur.bump();
    }
    cur.src.get(start..cur.i).unwrap_or("").to_owned()
}

fn scan_heredoc_delim(cur: &mut Cur<'_>) -> String {
    cur.bump_str("<<");
    let quote = cur.peek().filter(|ch| *ch == '\'' || *ch == '"');
    if quote.is_some() {
        cur.bump();
    }
    let start = cur.i;
    while cur
        .peek()
        .is_some_and(|ch| !matches!(ch, ' ' | '\t' | '\n' | ';' | '|' | '\'' | '"' | ')'))
    {
        cur.bump();
    }
    let delim = cur.src.get(start..cur.i).unwrap_or("").to_owned();
    if quote.is_some() && cur.peek() == quote {
        cur.bump();
    }
    delim
}

fn scan_word(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) -> String {
    let start = cur.i;
    while let Some(ch) = cur.peek() {
        if cur.starts("\\\n") {
            cur.bump_str("\\\n");
            continue;
        }
        match ch {
            '\'' => scan_single(cur),
            '"' => scan_double(cur, ctx),
            '`' => {
                ctx.gap("backticks are not accepted");
                cur.bump();
                while cur.peek().is_some_and(|next| next != '`' && next != '\n') {
                    cur.bump();
                }
                if cur.peek() == Some('`') {
                    cur.bump();
                }
            }
            '$' => scan_dollar(cur, ctx),
            '>' | '<' => {
                let so_far = cur.src.get(start..cur.i).unwrap_or("");
                if !so_far.is_empty() && so_far.chars().all(|item| item.is_ascii_digit()) {
                    cur.bump();
                    while cur
                        .peek()
                        .is_some_and(|next| !matches!(next, ' ' | '\t' | '\n' | ';' | '|' | ')'))
                    {
                        cur.bump();
                    }
                    continue;
                }
                break;
            }
            ' ' | '\t' | '\n' | '\r' | ';' | '|' | '&' | '(' | ')' | '#' => break,
            _ => cur.bump(),
        }
    }
    cur.src.get(start..cur.i).unwrap_or("").to_owned()
}

fn scan_single(cur: &mut Cur<'_>) {
    cur.bump();
    while let Some(ch) = cur.peek() {
        cur.bump();
        if ch == '\'' {
            break;
        }
    }
}

fn scan_double(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    cur.bump();
    while let Some(ch) = cur.peek() {
        if ch == '\\' {
            cur.bump();
            cur.bump();
            continue;
        }
        if ch == '"' {
            cur.bump();
            break;
        }
        if ch == '$' {
            scan_dollar(cur, ctx);
            continue;
        }
        if ch == '`' {
            ctx.gap("backticks are not accepted");
            cur.bump();
            continue;
        }
        cur.bump();
    }
}

fn scan_dollar(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    if cur.starts("$(") {
        let inner = take_subst(cur);
        let mut child = Ctx {
            step_key: ctx.step_key,
            file_line: ctx.line_of(cur.src, cur.i),
            pipefail: ctx.pipefail,
            captured: true,
            in_if_cond: false,
            status_vars: BTreeSet::new(),
            report: ShellReport::default(),
        };
        parse_body(&mut Cur::new(&inner), &mut child);
        ctx.report.gaps.extend(child.report.gaps);
        ctx.report.listing.extend(child.report.listing);
        ctx.report.env_forms.extend(child.report.env_forms);
        ctx.report.printf_grep = ctx
            .report
            .printf_grep
            .saturating_add(child.report.printf_grep);
        ctx.report.heredocs.extend(child.report.heredocs);
        ctx.report.facts.extend(child.report.facts);
        ctx.report.p3_body = ctx.report.p3_body.saturating_add(child.report.p3_body);
        ctx.report.p3_default = ctx
            .report
            .p3_default
            .saturating_add(child.report.p3_default);
        return;
    }
    if cur.starts("${{") {
        cur.bump_str("${{");
        while !cur.eof() && !cur.starts("}}") {
            cur.bump();
        }
        if cur.starts("}}") {
            cur.bump_str("}}");
        }
        return;
    }
    if cur.starts("${") {
        cur.bump_str("${");
        while cur.peek().is_some_and(|ch| ch != '}') {
            cur.bump();
        }
        if cur.peek() == Some('}') {
            cur.bump();
        }
        return;
    }
    if cur.starts("$?") {
        cur.bump_str("$?");
        return;
    }
    cur.bump();
    while cur
        .peek()
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        cur.bump();
    }
}

fn take_subst(cur: &mut Cur<'_>) -> String {
    cur.bump_str("$(");
    let start = cur.i;
    let mut depth = 1i32;
    let mut quote: Option<char> = None;
    while let Some(ch) = cur.peek() {
        if let Some(open) = quote {
            if open == '"' && ch == '\\' {
                cur.bump();
                cur.bump();
                continue;
            }
            cur.bump();
            if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                cur.bump();
            }
            '(' => {
                depth = depth.saturating_add(1);
                cur.bump();
            }
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let inner = cur.src.get(start..cur.i).unwrap_or("").to_owned();
                    cur.bump();
                    return inner;
                }
                cur.bump();
            }
            _ => cur.bump(),
        }
    }
    cur.src.get(start..cur.i).unwrap_or("").to_owned()
}

fn consume_kw(cur: &mut Cur<'_>, kw: &str) -> bool {
    cur.skip_inline();
    if !cur.starts_kw(kw) {
        return false;
    }
    cur.bump_str(kw);
    true
}

fn skip_line(cur: &mut Cur<'_>) {
    while cur.peek().is_some_and(|ch| ch != '\n') {
        cur.bump();
    }
}

fn is_name_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn note_status_assignment(ctx: &mut Ctx<'_>, word: &str) {
    let Some((name, rhs)) = word.split_once('=') else {
        return;
    };
    if rhs == "$?" {
        ctx.status_vars.insert(name.to_owned());
    }
}

fn command_display(argv: &[String], redirs: &[String]) -> String {
    let mut parts = argv.to_vec();
    parts.extend(redirs.iter().cloned());
    parts.join(" ")
}

fn classify_simple(ctx: &mut Ctx<'_>, cmd: &Cmd, heredoc: Option<&str>, cur: &Cur<'_>) {
    let Some(argv0) = cmd.argv.first() else {
        return;
    };
    let line = ctx.line_of(cur.src, cur.i);
    if let Some(text) = heredoc {
        ctx.report.heredocs.push(text.to_owned());
        if let Some(message) = heredoc_gap(argv0, text) {
            ctx.gap(&message);
            return;
        }
    }
    if swallowed_wrapper(argv0, &cmd.argv) {
        if let Some(message) = wrapper_message(argv0, &cmd.argv) {
            ctx.gap(&message);
        }
        return;
    }
    if argv0_is_var(argv0) {
        ctx.gap("argv0 comes from a variable");
        return;
    }
    if argv0.contains("${{") {
        ctx.gap("github expression in argv0 or test selector");
        return;
    }
    if let Some(message) = command_gap(cmd) {
        ctx.gap(&message);
        return;
    }
    let class = command_class(cmd);
    if class.test_running && selector_has_github(&cmd.argv) {
        ctx.gap("github expression in argv0 or test selector");
        return;
    }
    if let Some(message) = summary_outside(ctx.step_key, cmd) {
        ctx.gap(&message);
    }
    if argv0 == "env" {
        if let Some(message) = env_command_gap(&cmd.display) {
            ctx.gap(&message);
        } else if ENV_I_TEMPLATES.contains(&cmd.display.as_str()) {
            ctx.report.env_forms.push(cmd.display.clone());
        }
    }
    if let Some(path) = executed_repo_path(&cmd.argv) {
        if !CLOSURE_PINS.iter().any(|(pin, _)| *pin == path) {
            ctx.gap(&format!("unpinned callee {path}"));
        }
    }
    ctx.report.listing.push(format!(
        "{} L{line} {} :: {}",
        ctx.step_key, cmd.display, class.label
    ));
    ctx.report.facts.push(CmdFact {
        test_running: class.test_running,
        self_checked: class.self_checked,
        kani: class.kani,
        teed: false,
        captured: ctx.captured,
        in_pipeline: false,
        suppressed: false,
        pipefail: ctx.pipefail,
    });
    if argv0 == "set" && sets_pipefail(&cmd.argv) {
        ctx.pipefail = true;
    }
    if argv0 == "exit" {
        if let Some(message) = exit_gap(&cmd.argv, &ctx.status_vars) {
            ctx.gap(&message);
        }
    }
}

struct Class {
    label: &'static str,
    test_running: bool,
    self_checked: bool,
    kani: bool,
}

fn classify_pipeline(ctx: &mut Ctx<'_>, negated: bool, fact_at: usize, stages: &[Cmd]) {
    let multi = stages.len() > 1;
    if multi {
        for fact in ctx.report.facts.iter_mut().skip(fact_at) {
            fact.in_pipeline = true;
        }
    }
    if ctx.pipefail && pipeline_ends_blocked(stages) && !test_piped_to_grep(stages) {
        ctx.gap(&format!(
            "{}: early-exit reader in a pipeline under pipefail",
            ctx.step_key
        ));
        for fact in ctx.report.facts.iter_mut().skip(fact_at) {
            fact.suppressed = true;
        }
        return;
    }
    if test_piped_to_grep(stages) || grep_pattern_is_test_result(stages) {
        ctx.gap(&format!("{}: test output is piped into grep", ctx.step_key));
        for fact in ctx.report.facts.iter_mut().skip(fact_at) {
            fact.suppressed = true;
        }
        return;
    }
    if is_printf_grep(stages) {
        if ctx.in_if_cond && negated {
            ctx.report.listing.push(format!(
                "{} printf-grep :: match:printf-grep-bang",
                ctx.step_key
            ));
            return;
        }
        if ctx.in_if_cond && !negated {
            ctx.gap("positive printf grep is not accepted");
            return;
        }
        if !negated && printf_grep_is_literal(stages) {
            ctx.report.printf_grep = ctx.report.printf_grep.saturating_add(1);
            ctx.report.listing.push(format!(
                "{} printf-grep :: match:printf-grep-literal",
                ctx.step_key
            ));
            return;
        }
        ctx.gap("positive printf grep is not accepted");
        return;
    }
    if multi
        && stages
            .iter()
            .any(|stage| stage.argv.first().is_some_and(|arg| arg == "tee"))
    {
        for fact in ctx.report.facts.iter_mut().skip(fact_at) {
            if fact.test_running || fact.kani {
                fact.teed = true;
            }
        }
    }
    if multi && !ctx.pipefail {
        ctx.gap("pipeline without pipefail");
    }
}

fn pipeline_ends_blocked(stages: &[Cmd]) -> bool {
    stages.len() >= 2 && stages.last().is_some_and(early_exit_reader)
}

fn early_exit_reader(cmd: &Cmd) -> bool {
    let Some(argv0) = cmd.argv.first() else {
        return false;
    };
    match argv0.as_str() {
        "head" => true,
        "sed" | "awk" => true,
        "grep" => cmd.argv.iter().skip(1).any(|arg| {
            arg == "-q"
                || arg == "-m"
                || arg.starts_with("-q")
                || (arg.starts_with("-m") && arg.len() > 2)
        }),
        _ => false,
    }
}

fn test_piped_to_grep(stages: &[Cmd]) -> bool {
    let Some(grep_at) = stages
        .iter()
        .position(|stage| stage.argv.first().is_some_and(|arg| arg == "grep"))
    else {
        return false;
    };
    stages.iter().take(grep_at).any(stage_is_test_running)
}

fn stage_is_test_running(stage: &Cmd) -> bool {
    command_class(stage).test_running || command_class(stage).kani
}

fn grep_pattern_is_test_result(stages: &[Cmd]) -> bool {
    stages.iter().any(|stage| {
        stage.argv.first().is_some_and(|arg| arg == "grep")
            && stage.argv.iter().any(|arg| arg.contains("test result:"))
    }) && stages
        .iter()
        .any(|stage| stage.argv.first().is_some_and(|arg| arg == "printf"))
}

fn is_printf_grep(stages: &[Cmd]) -> bool {
    let Some(printf) = stages.first() else {
        return false;
    };
    let Some(grep) = stages.get(1) else {
        return false;
    };
    stages.len() == 2
        && printf.argv.first().is_some_and(|arg| arg == "printf")
        && printf.argv.get(1).is_some_and(|arg| arg == "'%s\\n'")
        && grep.argv.first().is_some_and(|arg| arg == "grep")
        && grep.argv.get(1).is_some_and(|arg| arg == "-q")
}

fn printf_grep_is_literal(stages: &[Cmd]) -> bool {
    stages
        .get(1)
        .and_then(|grep| grep.argv.get(2))
        .is_some_and(|arg| arg.starts_with('\'') && arg.ends_with('\'') && arg.len() >= 2)
}

fn finish_script(script: &str, ctx: &mut Ctx<'_>) {
    let file_grep = script.contains("grep -qF") || script.contains("grep -qxF");
    let piped_grep = script.contains("| grep -qF") || script.contains("| grep -qxF");
    if file_grep
        && !piped_grep
        && !(script.contains("-eq 0") && script.contains("-eq 1") && script.contains("-gt 1"))
    {
        ctx.gap(&format!(
            "{}: grep rc above 1 is treated as absent",
            ctx.step_key
        ));
    }
    let checker = script.contains("guards test-output") || script.contains("cucumber-summary");
    let mut assert_gap = false;
    let mut mask_gap = false;
    for fact in &ctx.report.facts {
        if fact.suppressed || fact.self_checked {
            continue;
        }
        if fact.kani {
            if !fact.teed && !fact.captured {
                assert_gap = true;
            }
            continue;
        }
        if !fact.test_running {
            continue;
        }
        if fact.in_pipeline && !fact.teed && !fact.pipefail {
            mask_gap = true;
            continue;
        }
        if fact.teed {
            continue;
        }
        if fact.captured {
            if !checker {
                assert_gap = true;
            }
            continue;
        }
        assert_gap = true;
    }
    if assert_gap {
        ctx.gap(&format!(
            "{}: test step does not assert its output",
            ctx.step_key
        ));
    }
    if mask_gap {
        ctx.gap(&format!("{}: test step masks its exit code", ctx.step_key));
    }
    let specific: Vec<String> = ctx
        .report
        .gaps
        .iter()
        .filter(|gap| {
            gap.contains("test output is piped into grep")
                || gap.contains("test summary outside a binary section")
        })
        .cloned()
        .collect();
    if !specific.is_empty() {
        ctx.report.gaps = specific;
    }
}

fn heredoc_gap(argv0: &str, text: &str) -> Option<String> {
    if argv0 == "sh" || argv0 == "bash" {
        return Some("heredoc into a shell is not accepted".to_owned());
    }
    if argv0 == "python3" && text.starts_with("python3 - ") {
        if normalize_heredoc(text) == PY_HEREDOC_TEMPLATE {
            return None;
        }
        return Some("heredoc body is not the pinned template".to_owned());
    }
    Some("unlisted heredoc".to_owned())
}

fn swallowed_wrapper(argv0: &str, argv: &[String]) -> bool {
    match argv0 {
        "sh" => argv.get(1).is_some_and(|arg| arg == "-c"),
        "bash" => argv.get(1).is_some_and(|arg| arg == "-c"),
        "source" | "." | "exec" | "timeout" | "command" | "xargs" | "eval" => true,
        "find" => argv.iter().any(|arg| arg == "-exec"),
        _ => false,
    }
}

fn wrapper_message(argv0: &str, argv: &[String]) -> Option<String> {
    let message = match argv0 {
        "sh" if argv.get(1).is_some_and(|arg| arg == "-c") => "sh -c is not accepted",
        "bash" if argv.get(1).is_some_and(|arg| arg == "-c") => "bash -c is not accepted",
        "source" | "." => "source is not accepted",
        "exec" => "exec is not accepted",
        "timeout" => "timeout is not accepted",
        "command" => "command is not accepted",
        "xargs" => "xargs is not accepted",
        "eval" => "eval is not accepted",
        "find" if argv.iter().any(|arg| arg == "-exec") => "find -exec is not accepted",
        _ => return None,
    };
    Some(message.to_owned())
}

fn argv0_is_var(argv0: &str) -> bool {
    if let Some(name) = argv0.strip_prefix("$") {
        return !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    }
    if let Some(inner) = argv0.strip_prefix("${") {
        return inner.ends_with('}') && !inner.contains("${{");
    }
    false
}

fn command_gap(cmd: &Cmd) -> Option<String> {
    let argv0 = cmd.argv.first().map(String::as_str).unwrap_or("");
    if argv0.ends_with("/bin/cargo") && argv0 != PINNED_CARGO && argv0 != "cargo" {
        return Some("absolute cargo path is not the pinned toolchain".to_owned());
    }
    if argv0 == "env" {
        return None;
    }
    if known_command(&cmd.argv) {
        return None;
    }
    Some(format!("unclassified command {}", cmd.display))
}

fn known_command(argv: &[String]) -> bool {
    let Some(argv0) = argv.first().map(String::as_str) else {
        return false;
    };
    match argv0 {
        "cd" | "set" | "exit" | "test" | "[" | "true" | "false" | "echo" | "printf" | "grep"
        | "head" | "sed" | "awk" | "tee" | "cat" | "curl" | "sha256sum" | "tar" | "sleep"
        | "mkdir" | "mktemp" | "jq" | "/usr/bin/curl" | "/usr/bin/sha256sum" | "/usr/bin/tar" => {
            true
        }
        "cargo" | "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo" => {
            known_cargo(argv)
        }
        "rustup" => known_rustup(argv),
        "rustc" => true,
        "git" => known_git(argv),
        "python3" => known_python(argv),
        "bash" => known_bash(argv),
        "java" => known_java(argv),
        "/tmp/cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny" => true,
        other => other.contains("/deps/"),
    }
}

fn known_cargo(argv: &[String]) -> bool {
    let Some(sub) = argv.get(1).map(String::as_str) else {
        return false;
    };
    match sub {
        "fmt" | "clippy" | "test" | "check" | "install" | "run" | "nextest" | "metadata" => true,
        "xtask" => matches!(
            argv.get(2).map(String::as_str),
            Some("guards" | "depgraph" | "run-exact" | "api-snapshot")
        ),
        "kani" => true,
        _ => false,
    }
}

fn known_rustup(argv: &[String]) -> bool {
    matches!(
        (
            argv.get(1).map(String::as_str),
            argv.get(2).map(String::as_str),
        ),
        (Some("which"), Some("cargo" | "rustc")) | (Some("toolchain"), Some("install"))
    )
}

fn known_git(argv: &[String]) -> bool {
    let mut args = argv.iter().skip(1);
    if args.clone().next().is_some_and(|arg| arg == "-C") {
        let _dir = args.next();
        let _path = args.next();
    }
    matches!(
        args.next().map(String::as_str),
        Some("clone" | "rev-parse" | "diff" | "status" | "checkout")
    )
}

fn known_python(argv: &[String]) -> bool {
    argv.get(1)
        .is_some_and(|arg| arg.ends_with("gate_results.py") || arg == "-")
}

fn known_bash(argv: &[String]) -> bool {
    matches!(
        argv.get(1).map(String::as_str),
        Some("scripts/load-versions.sh" | "scripts/api-snapshot-postcheck.sh")
    )
}

fn known_java(argv: &[String]) -> bool {
    let has_cp = argv.iter().any(|arg| arg == "-cp");
    let has_jar = argv.iter().any(|arg| arg.contains("tla2tools.jar"));
    let has_tlc = argv.iter().any(|arg| arg.contains("tlc2.TLC"));
    has_cp && has_jar && has_tlc
}

fn command_class(cmd: &Cmd) -> Class {
    let argv0 = cmd.argv.first().map(String::as_str).unwrap_or("");
    let cargo = argv0 == "cargo" || argv0 == PINNED_CARGO;
    if cargo
        && cmd
            .argv
            .get(1)
            .is_some_and(|arg| arg == "test" || arg == "nextest")
    {
        return Class {
            label: "test-running",
            test_running: true,
            self_checked: false,
            kani: false,
        };
    }
    if cargo && cmd.argv.get(1).is_some_and(|arg| arg == "kani") {
        if cmd.argv.get(2).is_some_and(|arg| arg == "setup") {
            return Class {
                label: "plain",
                test_running: false,
                self_checked: false,
                kani: false,
            };
        }
        return Class {
            label: "kani",
            test_running: false,
            self_checked: false,
            kani: true,
        };
    }
    if cargo
        && cmd.argv.get(1).is_some_and(|arg| arg == "xtask")
        && cmd.argv.get(2).is_some_and(|arg| arg == "run-exact")
    {
        return Class {
            label: "self-checked",
            test_running: true,
            self_checked: true,
            kani: false,
        };
    }
    if argv0.contains("/deps/") {
        return Class {
            label: "test-running",
            test_running: true,
            self_checked: false,
            kani: false,
        };
    }
    let label = if is_grep_qf(&cmd.argv) {
        "match:grep-qF"
    } else {
        "plain"
    };
    Class {
        label,
        test_running: false,
        self_checked: false,
        kani: false,
    }
}

fn is_grep_qf(argv: &[String]) -> bool {
    argv.len() >= 4
        && argv.first().is_some_and(|arg| arg == "grep")
        && argv.get(1).is_some_and(|arg| arg == "-qF" || arg == "-qxF")
        && argv.get(2).is_some_and(|arg| arg == "--")
        && argv
            .get(3)
            .is_some_and(|arg| arg.starts_with('\'') && arg.ends_with('\''))
}

fn selector_has_github(argv: &[String]) -> bool {
    let Some(split) = argv.iter().position(|arg| arg == "--") else {
        return false;
    };
    argv.iter()
        .skip(split.saturating_add(1))
        .any(|arg| arg.contains("${{"))
}

fn summary_outside(step_key: &str, cmd: &Cmd) -> Option<String> {
    let argv0 = cmd.argv.first().map(String::as_str)?;
    if argv0 != "echo" && argv0 != "printf" {
        return None;
    }
    if cmd.argv.iter().any(|arg| arg.contains("test result:")) {
        Some(format!("{step_key}: test summary outside a binary section"))
    } else {
        None
    }
}

fn sets_pipefail(argv: &[String]) -> bool {
    let has_name = argv.iter().any(|arg| arg == "pipefail");
    let has_o = argv.iter().any(|arg| {
        arg == "-o" || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains('o'))
    });
    has_name && has_o
}

fn exit_gap(argv: &[String], status_vars: &BTreeSet<String>) -> Option<String> {
    let word = argv.get(1).map(String::as_str).unwrap_or("0");
    if word.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let name = exit_var_name(word);
    if name.is_some_and(|item| status_vars.contains(item)) {
        return None;
    }
    Some("exit status was not captured".to_owned())
}

fn exit_var_name(word: &str) -> Option<&str> {
    if let Some(name) = word.strip_prefix("$") {
        if !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Some(name);
        }
    }
    word.strip_prefix("\"$")
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            word.strip_prefix("${")
                .and_then(|inner| inner.strip_suffix('}'))
        })
}

fn executed_repo_path(argv: &[String]) -> Option<&str> {
    let argv0 = argv.first().map(String::as_str)?;
    let path = match argv0 {
        "python3" | "bash" | "source" => argv.get(1).map(String::as_str)?,
        _ => return None,
    };
    if path.ends_with(".py") || path.ends_with(".sh") || path.starts_with("scripts/") {
        Some(path)
    } else {
        None
    }
}

fn env_command_gap(display: &str) -> Option<String> {
    if ENV_I_TEMPLATES.contains(&display) {
        return None;
    }
    let words: Vec<&str> = display.split(' ').filter(|word| !word.is_empty()).collect();
    if words.first() != Some(&"env") || words.get(1) != Some(&"-i") {
        return Some("env is not accepted".to_owned());
    }
    let (keys, cmd) = split_env_words(&words[2..]);
    let names = key_names(&keys);
    let mut same_keys = Vec::new();
    for template in ENV_I_TEMPLATES {
        let template_words: Vec<&str> = template
            .split(' ')
            .filter(|word| !word.is_empty())
            .collect();
        let (template_keys, template_cmd) = split_env_words(&template_words[2..]);
        if key_names(&template_keys) == names {
            same_keys.push((template_keys, template_cmd));
        }
    }
    if same_keys.is_empty() {
        return Some("env -i key was added or removed".to_owned());
    }
    if !same_keys
        .iter()
        .any(|(template_keys, _)| template_keys == &keys)
    {
        return Some("env -i value is not an accepted deny form".to_owned());
    }
    let binary = cmd.first().copied().unwrap_or("");
    let same_bin: Vec<_> = same_keys
        .into_iter()
        .filter(|(_, template_cmd)| template_cmd.first().copied() == Some(binary))
        .collect();
    if same_bin.is_empty() {
        return Some("env -i binary path is not an accepted deny form".to_owned());
    }
    Some("env -i argument is not an accepted deny form".to_owned())
}

fn split_env_words<'a>(words: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut keys = Vec::new();
    let mut rest = words;
    while let Some(word) = rest.first() {
        if !is_name_assignment(word) {
            break;
        }
        keys.push(*word);
        rest = rest.get(1..).unwrap_or(&[]);
    }
    (keys, rest.to_vec())
}

fn key_names<'a>(keys: &[&'a str]) -> Vec<&'a str> {
    keys.iter()
        .filter_map(|word| word.split_once('=').map(|(name, _)| name))
        .collect()
}

struct RunStep {
    job: String,
    name: String,
    script: String,
    line: usize,
}

fn ci_run_steps(text: &str) -> Vec<RunStep> {
    let lines: Vec<&str> = text.lines().collect();
    let mut job = String::new();
    let mut step_name = String::new();
    let mut out = Vec::new();
    let mut idx = 0usize;
    let mut block = RunBlockScan::Outside;
    while idx < lines.len() {
        let line = lines[idx];
        let inside = line_inside_block(&mut block, line);
        if !inside {
            if let Some(id) = job_id(line) {
                job = id;
                step_name.clear();
            }
            let trimmed = strip_yaml_comment(line).trim();
            let name_body = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            if let Some((key, value)) = split_yaml_key(name_body) {
                if key.trim() == "name" {
                    step_name = value.trim().trim_matches('"').trim_matches('\'').to_owned();
                }
            }
            if let Some(rest) = run_prefix(trimmed) {
                let rest = rest.trim();
                if rest.is_empty() || block_scalar_header(rest) {
                    let indent = line.len().saturating_sub(line.trim_start().len());
                    idx = idx.saturating_add(1);
                    let start_line = idx.saturating_add(1);
                    let mut script = String::new();
                    while idx < lines.len() {
                        let child = lines[idx];
                        if !child.trim().is_empty() {
                            let findent = child.len().saturating_sub(child.trim_start().len());
                            if findent <= indent {
                                break;
                            }
                        }
                        script.push_str(child);
                        script.push('\n');
                        idx = idx.saturating_add(1);
                    }
                    block = RunBlockScan::Outside;
                    out.push(RunStep {
                        job: job.clone(),
                        name: step_name.clone(),
                        script,
                        line: start_line,
                    });
                    continue;
                }
                out.push(RunStep {
                    job: job.clone(),
                    name: step_name.clone(),
                    script: format!("{rest}\n"),
                    line: idx.saturating_add(1),
                });
            }
            if let Some(next) = open_run_block(line) {
                block = next;
            }
        }
        idx = idx.saturating_add(1);
    }
    out
}

fn step_key(step: &RunStep) -> String {
    format!("{} {}", step.job, step.name)
}

struct Inventory {
    gaps: Vec<String>,
    listing: Vec<String>,
}

fn inventory_report(root: &Path, ci: &str) -> Result<Inventory, String> {
    let mut gaps = Vec::new();
    let mut listing = Vec::new();
    let mut env_forms = Vec::new();
    let mut printf_grep = 0usize;
    let mut heredocs = Vec::new();
    let mut p3_body = 0usize;
    let mut p3_default = 0usize;
    let mut steps = Vec::new();
    for step in ci_run_steps(ci) {
        let key = step_key(&step);
        let report = parse_run_script(&step.script, &key, step.line);
        gaps.extend(report.gaps);
        listing.extend(report.listing);
        let trailers = env_trailer_list(&step.script);
        if trailers.len() != report.env_forms.len() {
            push_gap(
                &mut gaps,
                format!("{key}: env -i trailer not in ENV_I_OCCURRENCES"),
            );
        }
        for (display, trailer) in report.env_forms.iter().zip(trailers.iter()) {
            env_forms.push((key.clone(), display.clone(), trailer.clone()));
        }
        printf_grep = printf_grep.saturating_add(report.printf_grep);
        heredocs.extend(report.heredocs);
        p3_body = p3_body.saturating_add(report.p3_body);
        p3_default = p3_default.saturating_add(report.p3_default);
        steps.push((step, report.facts));
    }
    gaps.extend(env_occurrence_gaps(&env_forms));
    if printf_grep != PRINTF_GREP_STATEMENTS {
        gaps.push(format!(
            "printf grep statements counted {printf_grep} does not match {PRINTF_GREP_STATEMENTS}"
        ));
    }
    let case_matches = p3_body.saturating_add(p3_default);
    if case_matches != CASE_LITERAL_MATCHES || p3_body != 1 || p3_default != 1 {
        gaps.push(format!(
            "case literal matches counted {case_matches} does not match {CASE_LITERAL_MATCHES}"
        ));
    }
    let matched = heredocs
        .iter()
        .filter(|text| normalize_heredoc(text) == PY_HEREDOC_TEMPLATE)
        .count();
    if matched != 1 {
        push_gap(
            &mut gaps,
            "heredoc body is not the pinned template".to_owned(),
        );
    }
    gaps.extend(closure_gaps(root)?);
    gaps.extend(crate_pin_gaps(&read(&root.join("Cargo.lock"))?)?);
    gaps.extend(binary_placement_gaps(&steps));
    gaps.extend(cucumber_runner_wiring_gaps(ci));
    gaps.extend(embedded_sample_gaps(root));
    Ok(Inventory { gaps, listing })
}

fn embedded_sample_gaps(root: &Path) -> Vec<String> {
    let mut gaps = parse_run_script(API_SNAPSHOT_POSTCHECK, "api-snapshot postcheck", 1).gaps;
    gaps.extend(parse_run_script(H2_INSTALL, "h2 install", 1).gaps);
    if ENUMERATED_MATCH_FORMS.len() != 3 {
        gaps.push("match forms are not enumerated".to_owned());
    }
    let bespoke = root.join("fixtures/guards/bespoke/cases.txt");
    if bespoke.is_file() {
        gaps.extend(bespoke_fixture_gaps(
            root,
            "fixtures/guards/bespoke/cases.txt",
            "control ok\nmutant fail\n2\n",
        ));
    }
    gaps
}

fn command_inventory_gaps(root: &Path, ci: &str) -> Result<Vec<String>, String> {
    Ok(inventory_report(root, ci)?.gaps)
}

fn command_listing(root: &Path, ci: &str) -> Result<Vec<String>, String> {
    let report = inventory_report(root, ci)?;
    if report.gaps.is_empty() {
        Ok(report.listing)
    } else {
        Err(report.gaps.join("\n"))
    }
}

fn env_occurrence_gaps(found: &[(String, String, String)]) -> Vec<String> {
    if ENV_I_TEMPLATES.len() != ENV_I_OCCURRENCES.len() {
        return vec!["env -i occurrence count does not match".to_owned()];
    }
    let mut gaps = Vec::new();
    for (template, (trailer, expected)) in ENV_I_TEMPLATES.iter().zip(ENV_I_OCCURRENCES.iter()) {
        let got = found
            .iter()
            .filter(|(_, display, found_trailer)| display == template && found_trailer == trailer)
            .count();
        if got != *expected {
            push_gap(
                &mut gaps,
                "env -i occurrence count does not match".to_owned(),
            );
        }
    }
    for (step_key, display, trailer) in found {
        if env_row_known(display, trailer) {
            continue;
        }
        if ENV_I_TEMPLATES.contains(&display.as_str()) {
            push_gap(
                &mut gaps,
                format!("{step_key}: env -i trailer not in ENV_I_OCCURRENCES"),
            );
        } else {
            push_gap(&mut gaps, "env -i is not an accepted deny form".to_owned());
        }
    }
    gaps
}

fn env_row_known(display: &str, trailer: &str) -> bool {
    ENV_I_TEMPLATES
        .iter()
        .zip(ENV_I_OCCURRENCES.iter())
        .any(|(template, (row_trailer, _))| display == *template && trailer == *row_trailer)
}

fn normalize_heredoc(text: &str) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("          ") {
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
    }
    out
}

fn env_trailer_list(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < script.len() {
        if env_i_at(script, index) {
            let (trailer, next) = read_env_trailer(script, index);
            out.push(trailer);
            index = next.max(index.saturating_add(1));
            continue;
        }
        index = index.saturating_add(1);
    }
    out
}

fn env_i_at(script: &str, index: usize) -> bool {
    if index > 0 {
        let prev = script.as_bytes()[index - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'/' {
            return false;
        }
    }
    script[index..].starts_with("env -i")
}

fn read_env_trailer(script: &str, start: usize) -> (String, usize) {
    let bytes = script.as_bytes();
    let mut index = start;
    let mut last_word = String::new();
    loop {
        let mut look = index;
        let mut continued = false;
        loop {
            if look >= bytes.len() {
                return (String::new(), look);
            }
            match bytes[look] {
                b' ' | b'\t' => look = look.saturating_add(1),
                b'\\' if script[look..].starts_with("\\\n") => {
                    continued = true;
                    look = look.saturating_add(2);
                }
                _ => break,
            }
        }
        if look >= bytes.len() {
            return (String::new(), look);
        }
        if bytes[look] == b'|' && continued {
            return (" \\\n| ".to_owned(), look);
        }
        index = look;
        match bytes[index] {
            b')' => {
                let trailer = if last_word == "2>&1" {
                    " 2>&1)".to_owned()
                } else if script[index.saturating_add(1)..].starts_with('"') {
                    ")\"".to_owned()
                } else {
                    ")".to_owned()
                };
                return (trailer, index.saturating_add(1));
            }
            b'\n' | b';' => return (String::new(), index),
            b'\'' | b'"' => {
                let word_start = index;
                index = skip_trailer_quote(script, index);
                last_word = script.get(word_start..index).unwrap_or("").to_owned();
            }
            _ => {
                let word_start = index;
                while index < bytes.len() {
                    let ch = bytes[index];
                    if matches!(ch, b' ' | b'\t' | b'\n' | b'\\' | b'|' | b')' | b';') {
                        break;
                    }
                    index = index.saturating_add(1);
                }
                last_word = script.get(word_start..index).unwrap_or("").to_owned();
            }
        }
    }
}

fn skip_trailer_quote(script: &str, start: usize) -> usize {
    let bytes = script.as_bytes();
    let quote = bytes[start];
    let mut index = start.saturating_add(1);
    while index < bytes.len() {
        if bytes[index] == quote {
            return index.saturating_add(1);
        }
        index = index.saturating_add(1);
    }
    index
}

fn cucumber_runner_wiring_gaps(ci: &str) -> Vec<String> {
    let runners = ["kline", "vision", "instrument_rules", "feature_reject"];
    let mut gaps = Vec::new();
    for runner in runners {
        let needle = format!("cucumber-summary --runner {runner}");
        if !ci.contains(&needle) {
            gaps.push(format!(
                "cucumber step does not invoke cucumber-summary for {runner}"
            ));
        }
    }
    gaps
}

fn read_stdin() -> Result<String, String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|err| format!("read stdin: {err}"))?;
    Ok(buf)
}

fn closure_gaps(root: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    collect_closure_files(root, &mut files)?;
    let mut owned = Vec::new();
    for path in &files {
        let bytes = read_bytes(&root.join(path))?;
        owned.push((path.clone(), bytes));
    }
    let views: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    Ok(assess_closure(&views, CLOSURE_PINS))
}

fn collect_closure_files(root: &Path, out: &mut Vec<String>) -> Result<(), String> {
    walk_closure_dir(root, &root.join("xtask/src"), out)?;
    out.retain(|path| path != "xtask/src/guards.rs");
    for path in [
        "xtask/Cargo.toml",
        "Cargo.toml",
        ".github/workflows/gate_results.py",
    ] {
        let full = root.join(path);
        if full.is_file() {
            out.push(path.to_owned());
        }
    }
    let cargo_dir = root.join(".cargo");
    if cargo_dir.is_dir() {
        let entries = std::fs::read_dir(&cargo_dir)
            .map_err(|err| format!("read {}: {err}", cargo_dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| format!("read {}: {err}", cargo_dir.display()))?;
            let path = entry.path();
            if path.is_file() {
                out.push(rel_string(root, &path)?);
            }
        }
    }
    find_build_rs(root, root, out)?;
    out.sort();
    out.dedup();
    Ok(())
}

fn walk_closure_dir(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_closure_dir(root, &path, out)?;
            continue;
        }
        out.push(rel_string(root, &path)?);
    }
    Ok(())
}

fn find_build_rs(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == "target" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            find_build_rs(root, &path, out)?;
            continue;
        }
        if name == "build.rs" {
            out.push(rel_string(root, &path)?);
        }
    }
    Ok(())
}

fn rel_string(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("path {} is outside {}", path.display(), root.display()))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn assess_closure(files: &[(&str, &[u8])], pins: &[(&str, &str)]) -> Vec<String> {
    let mut gaps = Vec::new();
    let file_names: BTreeSet<&str> = files.iter().map(|(path, _)| *path).collect();
    for (path, _) in pins {
        if *path == "xtask/src/guards.rs" {
            push_gap(&mut gaps, "closure pin names guards.rs".to_owned());
        } else if !file_names.contains(path) {
            push_gap(&mut gaps, format!("{path} is outside the closure"));
        }
    }
    for (path, bytes) in files {
        match pins.iter().find(|(pin, _)| pin == path) {
            None => push_gap(&mut gaps, format!("{path} is not pinned")),
            Some((_, hash)) => {
                if sha256_hex(bytes) != *hash {
                    push_gap(&mut gaps, format!("{path} sha256 does not match"));
                }
            }
        }
    }
    gaps
}

fn crate_pin_gaps(lock: &str) -> Result<Vec<String>, String> {
    let mut gaps = Vec::new();
    for (name, version, checksum) in [CUCUMBER_CRATE, GHERKIN_CRATE] {
        match lock_package(lock, name) {
            Some((found_version, found_checksum))
                if found_version == version && found_checksum == checksum => {}
            _ => gaps.push("cucumber/gherkin crate pin does not match Cargo.lock".to_owned()),
        }
    }
    Ok(gaps)
}

fn lock_package<'a>(lock: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let mut version = None;
    let mut checksum = None;
    let mut in_package = false;
    let mut saw_name = false;
    for line in lock.lines() {
        if line.starts_with("[[package]]") {
            if saw_name && version.is_some() && checksum.is_some() {
                return Some((version?, checksum?));
            }
            in_package = true;
            saw_name = false;
            version = None;
            checksum = None;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(value) = lock_value(line, "name") {
            saw_name = value == name;
            if value != name {
                in_package = false;
            }
        } else if saw_name && version.is_none() {
            if let Some(value) = lock_value(line, "version") {
                version = Some(value);
            }
        } else if saw_name && checksum.is_none() {
            if let Some(value) = lock_value(line, "checksum") {
                checksum = Some(value);
            }
        }
    }
    if saw_name {
        Some((version?, checksum?))
    } else {
        None
    }
}

fn lock_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim().strip_prefix(key)?;
    let rest = rest.trim().strip_prefix('=')?.trim();
    rest.strip_prefix('"')?.strip_suffix('"')
}

fn binary_placement_gaps(steps: &[(RunStep, Vec<CmdFact>)]) -> Vec<String> {
    let rows = binary_rows();
    let mut gaps = Vec::new();
    let mut seen = BTreeSet::new();
    for (step, facts) in steps {
        let cucumber = step.script.contains("cucumber-summary");
        let needs = !cucumber
            && facts.iter().any(|fact| {
                fact.test_running && !fact.self_checked && !fact.kani && !fact.suppressed
            });
        if !needs {
            continue;
        }
        let key = (step.job.as_str(), step.name.as_str());
        seen.insert((step.job.clone(), step.name.clone()));
        if !rows.iter().any(|row| row.job == key.0 && row.step == key.1) {
            gaps.push(format!(
                "{} {}: libtest invocation has no binary rows",
                step.job, step.name
            ));
        }
    }
    for row in &rows {
        if !seen
            .iter()
            .any(|(job, step)| job == row.job && step == row.step)
        {
            push_gap(
                &mut gaps,
                format!(
                    "{} {}: test binary step key matches no step",
                    row.job, row.step
                ),
            );
        }
    }
    gaps
}

fn test_binary_table_gaps(log: &str, job: &str, step: &str) -> Result<Vec<String>, String> {
    Ok(test_binary_table_with(log, job, step, &binary_rows()))
}

fn test_binary_table_with(log: &str, job: &str, step: &str, rows: &[BinaryRow]) -> Vec<String> {
    let mine: Vec<&BinaryRow> = rows
        .iter()
        .filter(|row| row.job == job && row.step == step)
        .collect();
    let mut gaps = Vec::new();
    if mine.is_empty() {
        gaps.push(format!(
            "{job} {step}: libtest invocation has no binary rows"
        ));
        return gaps;
    }
    let sections = log_sections(log);
    let mut matched = vec![false; mine.len()];
    for section in &sections {
        if !section.mappable {
            gaps.push(format!("{job} {step}: unmappable Running line"));
            continue;
        }
        let own = best_row(&section.header, &mine);
        if let Some((index, row)) = own {
            if let Some(slot) = matched.get_mut(index) {
                *slot = true;
            }
            push_count_gap(&mut gaps, job, step, row, section.count);
            continue;
        }
        if best_row(&section.header, &rows.iter().collect::<Vec<_>>()).is_some() {
            gaps.push(format!("{job} {step}: test binary row is on another step"));
        } else {
            gaps.push(format!("{job} {step}: test binary row removed"));
        }
    }
    for (index, row) in mine.iter().enumerate() {
        if matched.get(index).is_some_and(|flag| *flag) {
            continue;
        }
        gaps.push(format!(
            "{job} {step} {} {}: test binary row extra",
            row.package, row.target
        ));
    }
    gaps
}

fn push_count_gap(gaps: &mut Vec<String>, job: &str, step: &str, row: &BinaryRow, observed: usize) {
    if observed == row.count {
        return;
    }
    if row.count == 0 && observed > 0 {
        gaps.push(format!(
            "{job} {step} {} {}: test binary count is 0 for a binary that runs tests",
            row.package, row.kind
        ));
        return;
    }
    if row.count < observed {
        gaps.push(format!(
            "{job} {step} {} {}: test binary count lowered",
            row.package, row.kind
        ));
    }
}

fn best_row<'a>(header: &str, rows: &[&'a BinaryRow]) -> Option<(usize, &'a BinaryRow)> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| header.contains(row.target))
        .max_by_key(|(_, row)| row.target.len())
        .map(|(index, row)| (index, *row))
}

struct LogSection {
    header: String,
    count: usize,
    mappable: bool,
}

fn log_sections(log: &str) -> Vec<LogSection> {
    let lines: Vec<&str> = log.lines().collect();
    let mut starts = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("Running ") || trimmed.starts_with("Doc-tests ") {
            starts.push(index);
        }
    }
    let mut sections = Vec::new();
    for (offset, start) in starts.iter().enumerate() {
        let end = starts
            .get(offset.saturating_add(1))
            .copied()
            .unwrap_or(lines.len());
        let header = lines.get(*start).unwrap_or(&"").trim().to_owned();
        let body = lines.get(*start..end).unwrap_or(&[]);
        let mappable = section_mappable(&header);
        let count = section_count(body).unwrap_or(0);
        sections.push(LogSection {
            header,
            count,
            mappable,
        });
    }
    sections
}

fn section_mappable(header: &str) -> bool {
    if let Some(rest) = header.strip_prefix("Doc-tests ") {
        return !rest.is_empty() && !rest.contains(' ');
    }
    header.starts_with("Running ") && header.contains('(')
}

fn section_count(lines: &[&str]) -> Option<usize> {
    for line in lines {
        if let Some(count) = running_count(line.trim()) {
            return Some(count);
        }
    }
    let mut scenarios = 0usize;
    let mut saw = false;
    for line in lines {
        if let Some(count) = scenario_count_line(line.trim()) {
            scenarios = scenarios.saturating_add(count);
            saw = true;
        }
    }
    if saw {
        Some(scenarios)
    } else {
        None
    }
}

fn running_count(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("running ")?;
    let (count, suffix) = rest.split_once(' ')?;
    if suffix != "test" && suffix != "tests" {
        return None;
    }
    count.parse().ok()
}

fn scenario_count_line(line: &str) -> Option<usize> {
    let (count, rest) = line.split_once(' ')?;
    if rest.starts_with("scenario") {
        count.parse().ok()
    } else {
        None
    }
}

fn cucumber_summary_gaps(root: &Path, runner: &str, log: &str) -> Result<Vec<String>, String> {
    let plain = strip_ansi(log);
    if runner == "feature_reject" {
        return Ok(feature_reject_summary_gaps(&plain));
    }
    let policy: FeaturePolicy = toml::from_str(&read(&root.join("policy/features.toml"))?)
        .map_err(|err| format!("parse policy/features.toml: {err}"))?;
    let Some(item) = policy.runner.iter().find(|item| item.name == runner) else {
        return Ok(vec![format!(
            "cucumber runner {runner} is not in the policy"
        )]);
    };
    let files = expand_glob(root, &item.glob)?;
    let mut gaps = Vec::new();
    if files.is_empty() {
        gaps.push(format!("cucumber runner {runner} has no features"));
        return Ok(gaps);
    }
    for path in &files {
        let (scenarios, steps, rules) = contract_counts(&read(path)?)
            .map_err(|err| format!("malformed feature {}: {err}", path.display()))?;
        let block = summary_block(1, rules, scenarios, steps, 0, 0, 0);
        if !plain.contains(&block) {
            gaps.push(format!(
                "cucumber summary does not match {}",
                rel_string(root, path)?
            ));
        }
    }
    Ok(gaps)
}

fn feature_reject_summary_gaps(log: &str) -> Vec<String> {
    let broken = "[Summary]\n0 features\n0 scenarios\n0 steps\n1 parsing error\n";
    let undefined = "[Summary]\n1 feature\n1 scenario (1 failed)\n1 step (1 failed)\n";
    let mut gaps = Vec::new();
    if !log.contains(broken) || !log.contains(undefined) {
        gaps.push("cucumber summary does not match feature_reject".to_owned());
    }
    gaps
}

fn cucumber_runner_contract_gaps(root: &Path, ci: &str) -> Result<Vec<String>, String> {
    let mut gaps = cucumber_runner_wiring_gaps(ci);
    let policy: FeaturePolicy = toml::from_str(&read(&root.join("policy/features.toml"))?)
        .map_err(|err| format!("parse policy/features.toml: {err}"))?;
    for runner in &policy.runner {
        let files = expand_glob(root, &runner.glob)?;
        for path in files {
            if let Err(err) = contract_counts(&read(&path)?) {
                gaps.push(format!("malformed feature {}: {err}", path.display()));
            }
        }
    }
    for relative in [
        "fixtures/guards/cucumber-count/background.feature",
        "fixtures/guards/cucumber-count/rule-background.feature",
        "fixtures/guards/cucumber-count/examples.feature",
    ] {
        let path = root.join(relative);
        if path.is_file() {
            contract_counts(&read(&path)?)
                .map_err(|err| format!("malformed feature {relative}: {err}"))?;
        }
    }
    Ok(gaps)
}

fn contract_counts(text: &str) -> Result<(usize, usize, usize), String> {
    let mut owned = text.to_owned();
    if !owned.ends_with('\n') {
        owned.push('\n');
    }
    let feature = gherkin::Feature::parse(&owned, gherkin::GherkinEnv::default())
        .map_err(|err| err.to_string())?;
    let feature_bg = background_steps(feature.background.as_ref());
    let mut scenarios = 0usize;
    let mut steps = 0usize;
    for scenario in &feature.scenarios {
        let (extra_scenarios, extra_steps) = scenario_weight(scenario, feature_bg, 0);
        scenarios = scenarios.saturating_add(extra_scenarios);
        steps = steps.saturating_add(extra_steps);
    }
    for rule in &feature.rules {
        let rule_bg = background_steps(rule.background.as_ref());
        for scenario in &rule.scenarios {
            let (extra_scenarios, extra_steps) = scenario_weight(scenario, feature_bg, rule_bg);
            scenarios = scenarios.saturating_add(extra_scenarios);
            steps = steps.saturating_add(extra_steps);
        }
    }
    Ok((scenarios, steps, feature.rules.len()))
}

fn scenario_weight(
    scenario: &gherkin::Scenario,
    feature_bg: usize,
    rule_bg: usize,
) -> (usize, usize) {
    let multiplier = if scenario.examples.is_empty() {
        1
    } else {
        scenario.examples.iter().fold(0usize, |total, examples| {
            let rows = examples
                .table
                .as_ref()
                .map(|table| table.rows.len().saturating_sub(1))
                .unwrap_or(0);
            total.saturating_add(rows)
        })
    };
    let per = feature_bg
        .saturating_add(rule_bg)
        .saturating_add(scenario.steps.len());
    (multiplier, multiplier.saturating_mul(per))
}

fn summary_block(
    features: usize,
    rules: usize,
    scenarios: usize,
    steps: usize,
    parsing_errors: usize,
    failed_scenarios: usize,
    failed_steps: usize,
) -> String {
    let mut lines = vec![
        "[Summary]".to_owned(),
        count_noun(features, "feature", "features"),
    ];
    if rules > 0 {
        lines.push(count_noun(rules, "rule", "rules"));
    }
    lines.push(outcome_noun(
        scenarios,
        "scenario",
        "scenarios",
        failed_scenarios,
    ));
    lines.push(outcome_noun(steps, "step", "steps", failed_steps));
    if parsing_errors > 0 {
        lines.push(count_noun(
            parsing_errors,
            "parsing error",
            "parsing errors",
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn count_noun(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn outcome_noun(count: usize, singular: &str, plural: &str, failed: usize) -> String {
    let head = count_noun(count, singular, plural);
    if count == 0 {
        return head;
    }
    if failed == 0 {
        format!("{head} ({count} passed)")
    } else {
        format!("{head} ({failed} failed)")
    }
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn bespoke_fixture_gaps(root: &Path, cases_rel: &str, output: &str) -> Vec<String> {
    let path = root.join(cases_rel);
    let text = match read(&path) {
        Ok(text) => text,
        Err(_) => return vec!["bespoke harness fixture is missing".to_owned()],
    };
    bespoke_case_gaps(&text, output)
}

fn bespoke_case_gaps(cases: &str, output: &str) -> Vec<String> {
    let parsed: Vec<(&str, &str)> = cases
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                None
            } else {
                line.split_once(' ')
            }
        })
        .collect();
    if parsed.is_empty() {
        return vec!["bespoke harness has no cases".to_owned()];
    }
    let mut gaps = Vec::new();
    let output_lines: BTreeSet<&str> = output.lines().map(str::trim).collect();
    for (name, outcome) in &parsed {
        if *outcome == "fail" && output_lines.contains(format!("{name} ok").as_str()) {
            push_gap(
                &mut gaps,
                "bespoke harness case unexpectedly passed".to_owned(),
            );
        }
        let expected = format!("{name} {outcome}");
        if !output_lines.contains(expected.as_str())
            && !gaps
                .iter()
                .any(|gap| gap == "bespoke harness case unexpectedly passed")
        {
            push_gap(
                &mut gaps,
                "bespoke harness case unexpectedly passed".to_owned(),
            );
        }
    }
    let total = parsed.len().to_string();
    if !output_lines.contains(total.as_str()) {
        push_gap(
            &mut gaps,
            "bespoke harness case unexpectedly passed".to_owned(),
        );
    }
    gaps
}

#[cfg(test)]
fn glob_matches(glob: &str, rel: &str) -> bool {
    if glob.ends_with(".feature") && !glob.contains('*') {
        glob == rel
    } else if let Some(dir) = glob.strip_suffix("/**/*.feature") {
        rel.starts_with(&format!("{dir}/")) && rel.ends_with(".feature")
    } else if let Some(dir) = glob.strip_suffix("/*.feature") {
        Path::new(rel).parent().and_then(|parent| parent.to_str()) == Some(dir)
            && rel.ends_with(".feature")
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use super::*;

    #[test]
    fn main_allow_lines_do_not_count_and_a_new_allow_does() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("parangsae-suppress-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [
            "crates/backtest/src",
            "crates/backtest/tests",
            "crates/collector/src",
            "crates/signals",
        ] {
            std::fs::create_dir_all(root.join(dir)).map_err(|err| format!("mkdir {dir}: {err}"))?;
        }
        std::fs::write(
            root.join("crates/backtest/src/metrics.rs"),
            "#[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]\n",
        )
        .map_err(|err| format!("write metrics: {err}"))?;
        std::fs::write(
            root.join("crates/backtest/tests/support.rs"),
            "#![allow(dead_code)]\n",
        )
        .map_err(|err| format!("write support: {err}"))?;
        std::fs::write(
            root.join("crates/collector/src/run.rs"),
            "#[allow(clippy::too_many_arguments)]\n#[allow(clippy::too_many_arguments)]\n",
        )
        .map_err(|err| format!("write run: {err}"))?;
        std::fs::write(
            root.join("crates/collector/src/store.rs"),
            "#[allow(clippy::too_many_arguments)]\n#[allow(clippy::too_many_arguments)]\n",
        )
        .map_err(|err| format!("write store: {err}"))?;
        std::fs::write(
            root.join("crates/signals/clippy.toml"),
            "allow-unwrap-in-tests = true\n",
        )
        .map_err(|err| format!("write clippy.toml: {err}"))?;
        let pinned = suppression_count(&root)?;
        if pinned != 0 {
            let _ = std::fs::remove_dir_all(&root);
            return Err(format!("main allows counted as {pinned}"));
        }
        std::fs::write(
            root.join("crates/backtest/src/extra.rs"),
            "#[allow(clippy::unwrap_used)]\n",
        )
        .map_err(|err| format!("write extra: {err}"))?;
        let extra = suppression_count(&root)?;
        std::fs::remove_file(root.join("crates/backtest/src/extra.rs"))
            .map_err(|err| format!("remove extra: {err}"))?;
        if extra != 1 {
            let _ = std::fs::remove_dir_all(&root);
            return Err(format!("new allow counted as {extra}"));
        }
        std::fs::write(
            root.join("crates/collector/src/run.rs"),
            "#[allow(clippy::too_many_arguments)]\n#[allow(clippy::too_many_arguments)]\n#[allow(clippy::too_many_arguments)]\n",
        )
        .map_err(|err| format!("write third allow: {err}"))?;
        let third = suppression_count(&root)?;
        std::fs::write(
            root.join("crates/collector/src/run.rs"),
            "#[allow(clippy::too_many_arguments)]\n#[allow(clippy::too_many_arguments)]\n",
        )
        .map_err(|err| format!("restore run: {err}"))?;
        if third != 1 {
            let _ = std::fs::remove_dir_all(&root);
            return Err(format!("third identical allow counted as {third}"));
        }
        std::fs::write(
            root.join("crates/signals/clippy.toml"),
            "allow-unwrap-in-tests = true\nallow-print-in-tests = true\n",
        )
        .map_err(|err| format!("write extra clippy key: {err}"))?;
        let clippy_extra = suppression_count(&root)?;
        let _ = std::fs::remove_dir_all(&root);
        if clippy_extra != 1 {
            return Err(format!("new clippy allow key counted as {clippy_extra}"));
        }
        Ok(())
    }

    #[test]
    fn append_while_gap_fixture_breaks_no_append_while_gap() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let text = read(&root.join("fixtures/guards/segment/append_while_gap.txt"))?;
        match segment_fixture_fault(&text)? {
            Some("NoAppendWhileGap") => {}
            other => return Err(format!("{other:?}")),
        }
        let held = text.replace("expect=NoAppendWhileGap", "expect=none");
        match segment_fixture_fault(&held) {
            Err(err) if err.contains("expected invariant") => Ok(()),
            other => Err(format!("{other:?}")),
        }
    }

    #[test]
    fn globs_do_not_overlap_and_nested_collector_files_are_owned() {
        let kline = "specs/features/kline_integrity.feature";
        let instrument = "specs/features/instrument_rules.feature";
        let vision = "specs/features/collector/**/*.feature";
        assert!(glob_matches(kline, kline));
        assert!(!glob_matches(kline, instrument));
        assert!(!glob_matches(
            kline,
            "specs/features/collector/vision_integrity.feature"
        ));
        assert!(glob_matches(
            vision,
            "specs/features/collector/vision_integrity.feature"
        ));
        assert!(glob_matches(
            vision,
            "specs/features/collector/nested/x.feature"
        ));
        assert!(glob_matches(instrument, instrument));
        assert!(!glob_matches(instrument, kline));
        assert!(!glob_matches(vision, kline));
    }

    #[test]
    fn comment_does_not_declare_an_invariant_and_ids_is_not_one() {
        let tla = "\
\\* Fake == TRUE
Ids == 1..Max
Init ==
  /\\ phase = \"open\"
Contiguous ==
  \\A i \\in 1..Len(accepted) : accepted[i] = i
";
        let names = invariant_names(tla);
        assert!(names.contains("Contiguous"));
        assert!(!names.contains("Fake"));
        assert!(!names.contains("Ids"));
        assert!(!names.contains("Init"));
        let cfg = "INVARIANT Contiguous\n";
        assert!(uncovered_invariants(tla, cfg).is_empty());
        assert_eq!(
            uncovered_invariants(tla, "SPECIFICATION Spec\n"),
            vec!["Contiguous".to_owned()]
        );
    }

    #[test]
    fn deleting_an_invariant_from_the_model_and_the_cfg_still_fails() -> Result<(), String> {
        let tla = "TypeOK == TRUE\n";
        let cfg = "INVARIANT TypeOK\n";
        assert!(uncovered_invariants(tla, cfg).is_empty());
        let floors = parse_floors(
            "\
tlc.IngestSegment.invariant=TypeOK
tlc.IngestSegment.invariant=Contiguous
",
        )?;
        let gaps = floor_invariant_gaps("IngestSegment", tla, cfg, &floors);
        if !gaps
            .iter()
            .any(|line| line.contains("Contiguous") && line.contains("not defined"))
        {
            return Err(format!("{gaps:?}"));
        }
        Ok(())
    }

    #[test]
    fn lowered_floor_and_deleted_harness_fail_the_ratchet() -> Result<(), String> {
        let base = parse_floors(
            "\
cucumber.kline.scenarios=12
cucumber.kline.max_skipped=0
lint.suppressions=0
kani.min_harnesses=1
kani.name=notional_rejects_negative
tlc.IngestSegment.invariant=Contiguous
",
        )?;
        let lowered = parse_floors(
            "\
cucumber.kline.scenarios=11
cucumber.kline.max_skipped=1
lint.suppressions=1
kani.min_harnesses=1
",
        )?;
        let violations = ratchet(&lowered, &base);
        if !violations
            .iter()
            .any(|line| line.contains("scenarios") && line.contains("decreased"))
        {
            return Err(format!("{violations:?}"));
        }
        if !violations
            .iter()
            .any(|line| line.contains("max_skipped") && line.contains("increased"))
        {
            return Err(format!("{violations:?}"));
        }
        if !violations
            .iter()
            .any(|line| line.contains("lint.suppressions") && line.contains("increased"))
        {
            return Err(format!("{violations:?}"));
        }
        if !violations
            .iter()
            .any(|line| line.contains("notional_rejects_negative") && line.contains("deleted"))
        {
            return Err(format!("{violations:?}"));
        }
        if !violations
            .iter()
            .any(|line| line.contains("Contiguous") && line.contains("deleted"))
        {
            return Err(format!("{violations:?}"));
        }
        Ok(())
    }

    #[test]
    fn missing_or_unreadable_baseline_fails_closed() -> Result<(), String> {
        if !baseline_sha_unreadable("") {
            return Err("empty sha was readable".to_owned());
        }
        if !baseline_sha_unreadable("not-a-sha") {
            return Err("non-hex sha was readable".to_owned());
        }
        if !baseline_sha_unreadable("0000") {
            return Err("short zero sha was readable".to_owned());
        }
        let zeros = "0000000000000000000000000000000000000000";
        if choose_baseline(Some(zeros), false) != BaselineChoice::MergeBase {
            return Err("all-zero before did not use merge-base".to_owned());
        }
        if choose_baseline(Some(zeros), true) != BaselineChoice::MergeBase {
            return Err("all-zero before on main did not use merge-base".to_owned());
        }
        if choose_baseline(None, false) != BaselineChoice::MergeBase {
            return Err("missing base did not use merge-base".to_owned());
        }
        if choose_baseline(None, true) != BaselineChoice::DirectPush {
            return Err("direct push was not fail-closed".to_owned());
        }
        if choose_baseline(Some("not-a-sha"), false) != BaselineChoice::Unreadable {
            return Err("non-hex sha was accepted".to_owned());
        }
        match baseline_from_show(false, "4db8a75a7bd0886f4cdeefa050a2d89e3a36118d", None) {
            Err(err) if err.contains("baseline floors.toml is missing") => {}
            other => return Err(format!("merge-base without floors: {other:?}")),
        }
        let sha = "a84508c798fa3e2f08f71d4be988da43ef1c5472";
        let missing = baseline_from_show(false, sha, None);
        match missing {
            Err(err) if err.contains("baseline floors.toml is missing") => {}
            other => return Err(format!("missing baseline: {other:?}")),
        }
        let direct = baseline_from_show(true, sha, None);
        match direct {
            Err(err) if err.contains("direct push") && err.contains("missing") => {}
            other => return Err(format!("direct push: {other:?}")),
        }
        Ok(())
    }

    fn assert_guards_ran_except_unloaded_counts(reports: &[GuardReport]) -> Result<(), String> {
        for name in NON_RATCHET_GUARDS {
            let report = report_named(reports, name)?;
            if matches!(*name, "b2-counts" | "cucumber-counts") {
                if report.ran {
                    return Err(format!("{name} ran without a loaded floors file"));
                }
                if !report.detail.contains("floors.toml") {
                    return Err(format!("{name} detail: {}", report.detail));
                }
                continue;
            }
            if !report.ran {
                return Err(format!("guard {name} did not run"));
            }
        }
        if !reports
            .iter()
            .any(|report| report.name == "ratchet" && report.ran)
        {
            return Err("guard ratchet did not run".to_owned());
        }
        Ok(())
    }

    fn assert_non_ratchet_ran(reports: &[GuardReport]) -> Result<(), String> {
        for name in NON_RATCHET_GUARDS {
            match reports.iter().find(|report| report.name == *name) {
                Some(report) if report.ran => {}
                Some(_) => return Err(format!("guard {name} did not run")),
                None => return Err(format!("guard {name} missing")),
            }
        }
        if !reports
            .iter()
            .any(|report| report.name == "ratchet" && report.ran)
        {
            return Err("guard ratchet did not run".to_owned());
        }
        let ran = reports.iter().filter(|report| report.ran).count();
        if ran < EXPECTED_GUARD_RUNS {
            return Err(format!("guards ran {ran}"));
        }
        Ok(())
    }

    fn report_named<'a>(reports: &'a [GuardReport], name: &str) -> Result<&'a GuardReport, String> {
        reports
            .iter()
            .find(|report| report.name == name)
            .ok_or_else(|| format!("guard {name} missing"))
    }

    #[test]
    fn all_zero_before_with_a_non_bootstrap_base_fails_and_still_runs_guards() -> Result<(), String>
    {
        let root = TempGit::new("nonzero-base")?;
        let origin = root.path.join("origin");
        std::fs::create_dir_all(&origin).map_err(|err| format!("mkdir: {err}"))?;
        git_fixture(&origin, &["init", "-b", "main"])?;
        std::fs::write(origin.join("file"), "main\n").map_err(|err| format!("write: {err}"))?;
        git_fixture(&origin, &["add", "file"])?;
        git_fixture(&origin, &["commit", "-m", "main"])?;
        let main_sha = git_stdout(&origin, &["rev-parse", "HEAD"])?;
        git_fixture(&origin, &["checkout", "-b", "feature"])?;
        std::fs::write(origin.join("file"), "feature\n").map_err(|err| format!("write: {err}"))?;
        git_fixture(&origin, &["add", "file"])?;
        git_fixture(&origin, &["commit", "-m", "feature"])?;
        let origin_url = format!(
            "file://{}",
            origin.to_str().ok_or("origin path is not utf-8")?
        );
        git_fixture(
            &root.path,
            &[
                "clone",
                "--depth",
                "1",
                "--single-branch",
                "--branch",
                "feature",
                &origin_url,
                "clone",
            ],
        )?;
        let clone = root.path.join("clone");
        let zeros = "0000000000000000000000000000000000000000";
        let reports = guard_reports(&clone, Some(zeros), false);
        assert_guards_ran_except_unloaded_counts(&reports)?;
        let ratchet = report_named(&reports, "ratchet")?;
        if ratchet.passed {
            return Err("non-bootstrap missing floors passed".to_owned());
        }
        if !ratchet.detail.contains(&main_sha) || ratchet.detail.contains(zeros) {
            return Err(format!(
                "resolved base was not merge-base: {}",
                ratchet.detail
            ));
        }
        if main_sha == BOOTSTRAP_BASE {
            return Err("fixture main collided with the bootstrap base".to_owned());
        }
        Ok(())
    }

    #[test]
    fn baseline_paths_run_every_non_ratchet_guard() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let head = git_stdout(&root, &["rev-parse", "HEAD"])?;
        let normal = guard_reports(&root, Some(head.trim()), false);
        assert_non_ratchet_ran(&normal)?;
        if normal.iter().any(|report| !report.passed) {
            return Err(normal
                .iter()
                .filter(|report| !report.passed)
                .map(|report| format!("{}: {}", report.name, report.detail))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        let bootstrap_repo = shared_workspace("bootstrap-open")?;
        overlay_workspace_checks(&root, &bootstrap_repo.path)?;
        point_origin_at_main_without_floors(&bootstrap_repo.path)?;
        let bootstrap = guard_reports(&bootstrap_repo.path, Some(BOOTSTRAP_BASE), false);
        assert_non_ratchet_ran(&bootstrap)?;
        if bootstrap.iter().any(|report| !report.passed) {
            return Err(format!(
                "bootstrap: {}",
                bootstrap
                    .iter()
                    .filter(|report| !report.passed)
                    .map(|report| format!("{}: {}", report.name, report.detail))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        let zeros = "0000000000000000000000000000000000000000";
        let first_push = guard_reports(&root, Some(zeros), false);
        assert_non_ratchet_ran(&first_push)?;
        if first_push.iter().any(|report| !report.passed) {
            return Err(format!(
                "first push: {}",
                first_push
                    .iter()
                    .filter(|report| !report.passed)
                    .map(|report| format!("{}: {}", report.name, report.detail))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        let floors = read_bytes(&root.join("regressions/floors.toml"))?;
        if sha256_hex(&floors) != BOOTSTRAP_FLOORS_SHA256 {
            return Err(
                "BOOTSTRAP_FLOORS_SHA256 does not match regressions/floors.toml".to_owned(),
            );
        }
        Ok(())
    }

    #[test]
    fn bootstrap_fails_on_a_mutated_gate_pin_or_a_closed_origin_main() -> Result<(), String> {
        let source = crate::workspace_root()?;
        let mutated = shared_workspace("mutated-pin")?;
        overlay_workspace_checks(&source, &mutated.path)?;
        std::fs::write(
            mutated.path.join(".github/workflows/gate_results.py"),
            "import sys\nsys.exit(0)\n",
        )
        .map_err(|err| format!("write gate script: {err}"))?;
        point_origin_at_main_without_floors(&mutated.path)?;
        let reports = guard_reports(&mutated.path, Some(BOOTSTRAP_BASE), false);
        assert_non_ratchet_ran(&reports)?;
        let pin = report_named(&reports, "gate-script")?;
        if pin.passed || !pin.detail.contains("sha256 does not match") {
            return Err(format!("mutated pin: {}", pin.detail));
        }
        if reports.iter().all(|report| report.passed) {
            return Err("mutated pin bootstrap passed".to_owned());
        }

        let closed = shared_workspace("closed-main")?;
        overlay_workspace_checks(&source, &closed.path)?;
        let remote = closed.path.join("remote-main");
        std::fs::create_dir_all(&remote).map_err(|err| format!("mkdir: {err}"))?;
        git_fixture(&remote, &["init", "-b", "main"])?;
        std::fs::create_dir_all(remote.join("regressions"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(
            remote.join("regressions/floors.toml"),
            "lint.suppressions=0\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        git_fixture(&remote, &["add", "regressions/floors.toml"])?;
        git_fixture(&remote, &["commit", "-m", "floors"])?;
        let remote_url = format!(
            "file://{}",
            remote.to_str().ok_or("remote path is not utf-8")?
        );
        git_fixture(&closed.path, &["remote", "set-url", "origin", &remote_url])?;
        clear_origin_main(&closed.path)?;
        let reports = guard_reports(&closed.path, Some(BOOTSTRAP_BASE), false);
        assert_non_ratchet_ran(&reports)?;
        let ratchet = report_named(&reports, "ratchet")?;
        if ratchet.passed || !ratchet.detail.contains("origin/main contains") {
            return Err(format!("closed bootstrap: {}", ratchet.detail));
        }
        Ok(())
    }

    #[test]
    fn a_bootstrap_floors_sha_mismatch_fails() -> Result<(), String> {
        let source = crate::workspace_root()?;
        let cloned = shared_workspace("floors-sha")?;
        overlay_workspace_checks(&source, &cloned.path)?;
        let mut floors = read_bytes(&source.join("regressions/floors.toml"))?;
        floors.push(b'\n');
        std::fs::write(cloned.path.join("regressions/floors.toml"), floors)
            .map_err(|err| format!("write floors: {err}"))?;
        point_origin_at_main_without_floors(&cloned.path)?;
        let reports = guard_reports(&cloned.path, Some(BOOTSTRAP_BASE), false);
        assert_non_ratchet_ran(&reports)?;
        let ratchet = report_named(&reports, "ratchet")?;
        if ratchet.passed || !ratchet.detail.contains("bootstrap floors.toml sha256") {
            return Err(format!("sha mismatch: {}", ratchet.detail));
        }
        Ok(())
    }

    #[test]
    fn permissions_and_action_shas_are_required() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let missing = read(&root.join("fixtures/guards/ci/no-permissions.yml"))?;
        if permissions_are_contents_read(&missing) {
            return Err("missing permissions was accepted".to_owned());
        }
        let unpinned = read(&root.join("fixtures/guards/ci/unpinned-action.yml"))?;
        let gaps = action_pin_gaps(&unpinned);
        if gaps
            != [
                "action actions/checkout@v4 is not allowlisted".to_owned(),
                "action actions/checkout@v4 is not pinned to a sha".to_owned(),
            ]
        {
            return Err(format!("{gaps:?}"));
        }
        if !permissions_are_contents_read(&read(&root.join(".github/workflows/ci.yml"))?) {
            return Err("ci.yml permissions".to_owned());
        }
        if !action_pin_gaps(&read(&root.join(".github/workflows/ci.yml"))?).is_empty() {
            return Err(format!(
                "{:?}",
                action_pin_gaps(&read(&root.join(".github/workflows/ci.yml"))?)
            ));
        }
        Ok(())
    }

    #[test]
    fn bootstrap_base_missing_from_the_checkout_does_not_bootstrap() -> Result<(), String> {
        let root = TempGit::new("missing-object")?;
        git_fixture(&root.path, &["init", "-b", "feature"])?;
        std::fs::write(root.path.join("file"), "feature\n")
            .map_err(|err| format!("write: {err}"))?;
        git_fixture(&root.path, &["add", "file"])?;
        git_fixture(&root.path, &["commit", "-m", "feature"])?;
        let reports = guard_reports(&root.path, Some(BOOTSTRAP_BASE), false);
        assert_guards_ran_except_unloaded_counts(&reports)?;
        let ratchet = report_named(&reports, "ratchet")?;
        if ratchet.passed
            || ratchet.detail.contains("is not in this checkout")
            || ratchet.detail.contains(BOOTSTRAP_BASE)
            || !ratchet.detail.contains("refs/remotes/origin/main")
            || !ratchet.detail.contains("failed")
        {
            return Err(format!("missing object: {}", ratchet.detail));
        }
        Ok(())
    }

    #[test]
    fn git_show_does_not_fetch_a_missing_baseline() -> Result<(), String> {
        let root = TempGit::new("replaced-tip")?;
        let origin = root.path.join("origin");
        std::fs::create_dir_all(origin.join("regressions"))
            .map_err(|err| format!("mkdir: {err}"))?;
        git_fixture(&origin, &["init", "-b", "kept"])?;
        std::fs::write(origin.join("regressions/floors.toml"), "old-floors\n")
            .map_err(|err| format!("write: {err}"))?;
        git_fixture(&origin, &["add", "regressions/floors.toml"])?;
        git_fixture(&origin, &["commit", "-m", "old floors"])?;
        let old = git_stdout(&origin, &["rev-parse", "HEAD"])?;
        git_fixture(&origin, &["checkout", "--orphan", "rewritten"])?;
        git_fixture(&origin, &["rm", "-rf", "."])?;
        std::fs::write(origin.join("file"), "new\n").map_err(|err| format!("write: {err}"))?;
        git_fixture(&origin, &["add", "file"])?;
        git_fixture(&origin, &["commit", "-m", "rewritten"])?;
        git_fixture(&origin, &["update-ref", "refs/keep/old", &old])?;
        git_fixture(&origin, &["branch", "-D", "kept"])?;
        git_fixture(
            &origin,
            &["config", "uploadpack.allowReachableSHA1InWant", "true"],
        )?;
        git_fixture(
            &origin,
            &["config", "uploadpack.allowAnySHA1InWant", "true"],
        )?;
        let origin_url = format!(
            "file://{}",
            origin.to_str().ok_or("origin path is not utf-8")?
        );
        git_fixture(
            &root.path,
            &[
                "clone",
                "--single-branch",
                "--branch",
                "rewritten",
                &origin_url,
                "clone",
            ],
        )?;
        let clone = root.path.join("clone");
        let already = git_command(&clone, &["cat-file", "-e", &format!("{old}^{{commit}}")])?;
        if already.status.success() {
            return Err("replaced tip was already in the clone".to_owned());
        }
        match git_show(&clone, &old, "regressions/floors.toml") {
            Err(err) if err.contains("is not in this checkout") => {}
            other => return Err(format!("missing baseline was fetched: {other:?}")),
        }
        let still_missing = git_command(&clone, &["cat-file", "-e", &format!("{old}^{{commit}}")])?;
        if still_missing.status.success() {
            return Err("git show fetched the replaced tip".to_owned());
        }
        Ok(())
    }

    #[test]
    fn force_push_over_a_lowered_floor_ratchets_against_main() -> Result<(), String> {
        let repo = lowered_floor_repo()?;
        git_fixture(&repo.clone, &["checkout", "--quiet", "tip-b"])?;
        let ancestor = git_command(
            &repo.clone,
            &["merge-base", "--is-ancestor", &repo.tip_a, "HEAD"],
        )?;
        if ancestor.status.success() {
            return Err("lowered tip is an ancestor of the force-pushed head".to_owned());
        }
        let reports = guard_reports(&repo.clone, Some(&repo.tip_a), false);
        let ratchet = report_named(&reports, "ratchet")?;
        if ratchet.passed
            || !ratchet
                .detail
                .contains("floor cucumber.kline.scenarios decreased from 12 to 11")
            || ratchet.detail.contains(&repo.tip_a)
        {
            return Err(format!("force-push baseline: {}", ratchet.detail));
        }
        Ok(())
    }

    #[test]
    fn all_zero_before_on_a_new_branch_ratchets_against_main() -> Result<(), String> {
        let repo = lowered_floor_repo()?;
        git_fixture(&repo.clone, &["checkout", "--quiet", "tip-a"])?;
        let zeros = "0000000000000000000000000000000000000000";
        let reports = guard_reports(&repo.clone, Some(zeros), false);
        let ratchet = report_named(&reports, "ratchet")?;
        if ratchet.passed
            || !ratchet
                .detail
                .contains("floor cucumber.kline.scenarios decreased from 12 to 11")
            || ratchet.detail.contains(zeros)
        {
            return Err(format!("new-branch baseline: {}", ratchet.detail));
        }
        Ok(())
    }

    #[test]
    fn cargo_workspace_dir_in_ci_fails() -> Result<(), String> {
        let set = std::ffi::OsStr::new("/tmp/other");
        let empty = std::ffi::OsStr::new("");
        match crate::reject_cargo_workspace_dir_in_ci(true, Some(set)) {
            Err(err) if err == "CARGO_WORKSPACE_DIR is set in CI" => {}
            other => return Err(format!("CI override: {other:?}")),
        }
        if crate::reject_cargo_workspace_dir_in_ci(true, Some(empty)).is_err() {
            return Err("empty override was rejected".to_owned());
        }
        if crate::reject_cargo_workspace_dir_in_ci(true, None).is_err() {
            return Err("unset override was rejected".to_owned());
        }
        if crate::reject_cargo_workspace_dir_in_ci(false, Some(set)).is_err() {
            return Err("local override was rejected".to_owned());
        }
        let zero = std::ffi::OsStr::new("0");
        let truth = std::ffi::OsStr::new("true");
        if !crate::ci_is_set(Some(empty), None) {
            return Err("empty GITHUB_ACTIONS was not CI".to_owned());
        }
        if !crate::ci_is_set(Some(zero), None) {
            return Err("GITHUB_ACTIONS=0 was not CI".to_owned());
        }
        if !crate::ci_is_set(None, Some(truth)) {
            return Err("CI was not CI".to_owned());
        }
        if crate::ci_is_set(None, None) {
            return Err("unset environment was CI".to_owned());
        }
        Ok(())
    }

    #[test]
    fn workflow_guards_reject_each_bypass() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let extra = read(&root.join("fixtures/guards/ci/permissions-extra.yml"))?;
        if permissions_are_contents_read(&extra) {
            return Err("extra permission was accepted".to_owned());
        }
        let write = read(&root.join("fixtures/guards/ci/job-permissions-write.yml"))?;
        let write_gaps = job_write_permission_gaps("job-permissions-write.yml", &write);
        if write_gaps != ["job-permissions-write.yml job fmt permissions grant write".to_owned()] {
            return Err(format!("write: {write_gaps:?}"));
        }
        let write_all = read(&root.join("fixtures/guards/ci/job-permissions-write-all.yml"))?;
        let write_all_gaps = job_write_permission_gaps("job-permissions-write-all.yml", &write_all);
        if write_all_gaps
            != ["job-permissions-write-all.yml job fmt permissions grant write".to_owned()]
        {
            return Err(format!("write-all: {write_all_gaps:?}"));
        }
        let flow = read(&root.join("fixtures/guards/ci/flow-uses.yml"))?;
        let flow_gaps = action_pin_gaps(&flow);
        if flow_gaps
            != [
                "action actions/checkout@v4 is not allowlisted".to_owned(),
                "action actions/checkout@v4 is not pinned to a sha".to_owned(),
            ]
        {
            return Err(format!("flow uses: {flow_gaps:?}"));
        }
        let target = read(&root.join("fixtures/guards/ci/pull-request-target.yml"))?;
        let target_gaps = trigger_gaps("pull-request-target.yml", &target);
        if target_gaps
            != ["pull-request-target.yml trigger pull_request_target is not allowed".to_owned()]
        {
            return Err(format!("pull_request_target: {target_gaps:?}"));
        }
        let workflow_run = read(&root.join("fixtures/guards/ci/workflow-run.yml"))?;
        let run_gaps = trigger_gaps("workflow-run.yml", &workflow_run);
        if run_gaps != ["workflow-run.yml trigger workflow_run is not allowed".to_owned()] {
            return Err(format!("workflow_run: {run_gaps:?}"));
        }
        let env_text = read(&root.join("fixtures/guards/ci/env-overrides.yml"))?;
        let env_gaps = forbidden_env_gaps("env-overrides.yml", &env_text);
        let expected_env = [
            "env-overrides.yml env sets CARGO_WORKSPACE_DIR".to_owned(),
            "env-overrides.yml env sets GITHUB_ACTIONS".to_owned(),
            "env-overrides.yml env sets CI".to_owned(),
            "env-overrides.yml env sets PARANGSAE_FLOOR_BASE".to_owned(),
        ];
        if env_gaps != expected_env {
            return Err(format!("env: {env_gaps:?}"));
        }
        let day = read(&root.join("fixtures/guards/ci/vision-day-injected.yml"))?;
        let day_gaps = run_injection_gaps("vision-day-injected.yml", &day);
        if day_gaps != ["vision-day-injected.yml interpolates inputs.day in run".to_owned()] {
            return Err(format!("day: {day_gaps:?}"));
        }
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        if !pinned_guards_steps(&ci).is_empty() {
            return Err(format!("guards step pin: {:?}", pinned_guards_steps(&ci)));
        }
        if !strict_yaml_gaps("ci.yml", &ci).is_empty() {
            return Err(format!(
                "ci.yml subset: {:?}",
                strict_yaml_gaps("ci.yml", &ci)
            ));
        }
        if !ci.contains(RUSTC_198_STEP) {
            return Err("rustc step pin".to_owned());
        }
        let smoke = read(&root.join(".github/workflows/vision-smoke.yml"))?;
        let smoke_gaps = workflow_file_gaps(".github/workflows/vision-smoke.yml", &smoke);
        if !smoke_gaps.is_empty() {
            return Err(format!("vision-smoke: {smoke_gaps:?}"));
        }
        expect_gaps(
            "smoke push",
            foreign_workflow_gaps(".github/workflows/vision-smoke.yml", &smoke),
            &[],
        )?;
        Ok(())
    }

    #[test]
    fn b2_and_cucumber_do_not_run_when_floors_fail_to_load() -> Result<(), String> {
        let root = TempGit::new("no-floors")?;
        let reports = guard_reports(&root.path, None, false);
        assert_guards_ran_except_unloaded_counts(&reports)?;
        for name in ["b2-counts", "cucumber-counts"] {
            let report = report_named(&reports, name)?;
            if report.line() != format!("guard {name} did not run") {
                return Err(report.line());
            }
            if report.passed {
                return Err(format!("{name} passed without floors.toml"));
            }
        }
        let floor = report_named(&reports, "floor-keys")?;
        if !floor.ran || floor.passed {
            return Err(format!(
                "floor-keys ran={} passed={}",
                floor.ran, floor.passed
            ));
        }
        let gaps = guard_run_gaps(&reports);
        if !gaps.iter().any(|gap| gap.contains("below")) {
            return Err(format!("run count: {gaps:?}"));
        }
        Ok(())
    }

    fn expect_gaps(label: &str, gaps: Vec<String>, expected: &[&str]) -> Result<(), String> {
        let expected: Vec<String> = expected.iter().map(|gap| (*gap).to_owned()).collect();
        if gaps == expected {
            Ok(())
        } else {
            Err(format!("{label}: {gaps:?}"))
        }
    }

    #[test]
    fn guards_steps_are_pinned_inside_the_guards_job() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("ci.yml", pinned_guards_steps(&ci), &[])?;
        let probes = [
            (
                "fixtures/guards/ci/guards-step-echo.yml",
                &["guards step does not match"][..],
            ),
            (
                "fixtures/guards/ci/guards-step-or-true.yml",
                &["guards step does not match"][..],
            ),
            (
                "fixtures/guards/ci/all-zero-step-echo.yml",
                &["all-zero before step does not match"][..],
            ),
            (
                "fixtures/guards/ci/all-zero-step-or-true.yml",
                &["all-zero before step does not match"][..],
            ),
        ];
        for (path, expected) in probes {
            let text = read(&root.join(path))?;
            expect_gaps(path, pinned_guards_steps(&text), expected)?;
        }
        let decoy = format!(
            "name: ci\non:\n  push:\njobs:\n  fmt:\n    steps:\n{GUARDS_STEP}  guards:\n    runs-on: ubuntu-latest\n    steps:\n      - name: cargo xtask guards\n        env:\n          PARANGSAE_FLOOR_BASE: kept\n          PARANGSAE_ON_MAIN: kept\n        run: echo 'guards ran 12 passed 12'\n{ALL_ZERO_STEP}"
        );
        expect_gaps(
            "decoy",
            pinned_guards_steps(&decoy),
            &["guards step does not match"],
        )?;
        Ok(())
    }

    #[test]
    fn workflow_yaml_outside_the_subset_fails_closed() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        let smoke = read(&root.join(".github/workflows/vision-smoke.yml"))?;
        expect_gaps("ci.yml", strict_yaml_gaps("ci.yml", &ci), &[])?;
        expect_gaps(
            "vision-smoke.yml",
            strict_yaml_gaps("vision-smoke.yml", &smoke),
            &[],
        )?;
        expect_gaps("ci env", forbidden_env_gaps("ci.yml", &ci), &[])?;
        expect_gaps("ci rustc", rustc_pin_gaps("ci.yml", &ci), &[])?;
        expect_gaps(
            "smoke rustc",
            rustc_pin_gaps("vision-smoke.yml", &smoke),
            &[],
        )?;
        let probes = [
            (
                "fixtures/guards/ci/b1-quoted-if-step.yml",
                "b1-quoted-if-step.yml has a quoted key",
            ),
            (
                "fixtures/guards/ci/b2-space-before-colon.yml",
                "b2-space-before-colon.yml has whitespace before a colon",
            ),
            (
                "fixtures/guards/ci/b3-quoted-if-job.yml",
                "b3-quoted-if-job.yml has a quoted key",
            ),
            (
                "fixtures/guards/ci/b4-flow-step-if.yml",
                "b4-flow-step-if.yml uses a flow mapping as a step",
            ),
            (
                "fixtures/guards/ci/b5-flow-on.yml",
                "b5-flow-on.yml uses a flow mapping as on",
            ),
            (
                "fixtures/guards/ci/b7-flow-permissions.yml",
                "b7-flow-permissions.yml uses a flow mapping as permissions",
            ),
            (
                "fixtures/guards/ci/b8-quoted-uses.yml",
                "b8-quoted-uses.yml has a quoted key",
            ),
            (
                "fixtures/guards/ci/b9-quoted-env-key.yml",
                "b9-quoted-env-key.yml has a quoted key",
            ),
            (
                "fixtures/guards/ci/b17-quoted-continue-on-error.yml",
                "b17-quoted-continue-on-error.yml has a quoted key",
            ),
            ("fixtures/guards/ci/yaml-tag.yml", "yaml-tag.yml uses a tag"),
            (
                "fixtures/guards/ci/multi-doc.yml",
                "multi-doc.yml has multiple documents",
            ),
            (
                "fixtures/guards/ci/tab-indent.yml",
                "tab-indent.yml uses a tab in indentation",
            ),
            (
                "fixtures/guards/ci/merge-key.yml",
                "merge-key.yml uses a merge key",
            ),
        ];
        for (path, expected) in probes {
            let Some(name) = path.rsplit('/').next() else {
                return Err(format!("{path} has no file name"));
            };
            let text = read(&root.join(path))?;
            expect_gaps(path, strict_yaml_gaps(name, &text), &[expected])?;
        }
        let anchor = read(&root.join("fixtures/guards/ci/anchor-alias.yml"))?;
        expect_gaps(
            "anchor-alias.yml",
            strict_yaml_gaps("anchor-alias.yml", &anchor),
            &[
                "anchor-alias.yml uses an anchor",
                "anchor-alias.yml uses an alias",
            ],
        )?;
        let block = read(&root.join("fixtures/guards/ci/b6-block-pull-request-target.yml"))?;
        expect_gaps(
            "b6 strict",
            strict_yaml_gaps("b6-block-pull-request-target.yml", &block),
            &[],
        )?;
        expect_gaps(
            "b6 trigger",
            workflow_file_gaps("b6-block-pull-request-target.yml", &block),
            &["b6-block-pull-request-target.yml trigger pull_request_target is not allowed"],
        )?;
        let cargo = read(&root.join("fixtures/guards/ci/cargo-without-rustc.yml"))?;
        expect_gaps(
            "cargo without rustc",
            rustc_pin_gaps("cargo-without-rustc.yml", &cargo),
            &[
                "cargo-without-rustc.yml job fmt does not assert rustc 1.98.0",
                "cargo-without-rustc.yml job fmt does not assert rustup which 1.98.0",
            ],
        )?;
        let toolchain = "\
name: ci
on:
  push:
permissions:
  contents: read
jobs:
  docs:
    runs-on: ubuntu-latest
    steps:
      - uses: dtolnay/rust-toolchain@5059963b1731bf1580991b5f3b1eecfb5da406c0
      - run: echo docs
";
        expect_gaps(
            "toolchain without cargo",
            rustc_pin_gaps("toolchain.yml", toolchain),
            &[],
        )?;
        let rustup = read(&root.join("fixtures/guards/ci/rustup-toolchain.yml"))?;
        expect_gaps(
            "rustup outside kani",
            forbidden_env_gaps("rustup-toolchain.yml", &rustup),
            &["rustup-toolchain.yml env sets RUSTUP_TOOLCHAIN"],
        )?;
        let unpinned = read(&root.join("fixtures/guards/ci/kani-rustup-unpinned.yml"))?;
        expect_gaps(
            "kani rustup",
            forbidden_env_gaps("kani-rustup-unpinned.yml", &unpinned),
            &["kani-rustup-unpinned.yml env sets RUSTUP_TOOLCHAIN"],
        )?;
        let allowed = "\
name: ci
on:
  push:
permissions:
  contents: read
jobs:
  kani:
    runs-on: ubuntu-latest
    steps:
      - env:
          RUSTUP_TOOLCHAIN: 1.98.0
        run: cargo install kani-verifier
      - env:
          RUSTUP_TOOLCHAIN: nightly-2026-08-21
        run: cargo kani
";
        expect_gaps("kani pins", forbidden_env_gaps("kani.yml", allowed), &[])?;
        let sequence = "\
name: ci
on:
  - pull_request_target
permissions:
  contents: read
jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - run: echo fmt
";
        expect_gaps(
            "on sequence",
            strict_yaml_gaps("on-sequence.yml", sequence),
            &["on-sequence.yml uses a sequence under on"],
        )?;
        expect_gaps(
            "on flow sequence",
            strict_yaml_gaps(
                "on-flow.yml",
                "name: ci\non: [push, pull_request_target]\npermissions:\n  contents: read\n",
            ),
            &["on-flow.yml uses a flow sequence as on"],
        )?;
        expect_gaps(
            "trigger flow",
            strict_yaml_gaps(
                "trigger-flow.yml",
                "name: ci\non:\n  push:\n  pull_request_target: {types: [opened]}\npermissions:\n  contents: read\n",
            ),
            &["trigger-flow.yml uses a flow mapping as a trigger"],
        )?;
        expect_gaps(
            "env flow",
            strict_yaml_gaps(
                "env-flow.yml",
                "name: ci\non:\n  push:\npermissions:\n  contents: read\nenv:\n  {CI: \"false\"}\n",
            ),
            &["env-flow.yml uses a flow mapping as env"],
        )?;
        expect_gaps(
            "step flow sequence",
            strict_yaml_gaps(
                "step-flow.yml",
                "name: ci\non:\n  push:\npermissions:\n  contents: read\njobs:\n  fmt:\n    steps:\n      - [echo]\n",
            ),
            &["step-flow.yml uses a flow sequence as a step"],
        )?;
        let grammar = [
            (
                "fixtures/guards/ci/yaml-single-tag.yml",
                &["yaml-single-tag.yml uses a tag"][..],
            ),
            (
                "fixtures/guards/ci/block-scalar-pipe.yml",
                &["block-scalar-pipe.yml uses a block scalar outside run"][..],
            ),
            (
                "fixtures/guards/ci/block-scalar-fold.yml",
                &["block-scalar-fold.yml uses a block scalar outside run"][..],
            ),
            (
                "fixtures/guards/ci/block-scalar-next-line.yml",
                &["block-scalar-next-line.yml uses a block scalar outside run"][..],
            ),
            (
                "fixtures/guards/ci/flow-runs-on.yml",
                &["flow-runs-on.yml uses a flow sequence"][..],
            ),
            (
                "fixtures/guards/ci/flow-mapping.yml",
                &["flow-mapping.yml uses a flow mapping"][..],
            ),
            (
                "fixtures/guards/ci/crlf-workflow.yml",
                &["crlf-workflow.yml uses crlf line endings"][..],
            ),
            (
                "fixtures/guards/ci/bom-workflow.yml",
                &["bom-workflow.yml has a byte order mark"][..],
            ),
            (
                "fixtures/guards/ci/duplicate-key.yml",
                &["duplicate-key.yml repeats key name"][..],
            ),
        ];
        for (path, expected) in grammar {
            let Some(name) = path.rsplit('/').next() else {
                return Err(format!("{path} has no file name"));
            };
            let text = read(&root.join(path))?;
            expect_gaps(path, strict_yaml_gaps(name, &text), expected)?;
        }
        let needs_list = "\
name: ci
on:
  push:
permissions:
  contents: read
jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - run: echo fmt
  gate:
    needs: [fmt]
    runs-on: ubuntu-latest
    steps:
      - run: echo gate
";
        expect_gaps(
            "needs scalar list",
            strict_yaml_gaps("needs.yml", needs_list),
            &[],
        )?;
        Ok(())
    }

    #[test]
    fn cargo_aliases_defaults_and_explicit_keys_fail() -> Result<(), String> {
        let root = crate::workspace_root()?;
        expect_gaps("repo cargo config", cargo_config_gaps(&root)?, &[])?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("guards steps", pinned_guards_steps(&ci), &[])?;
        expect_gaps("ci github", github_env_path_gaps("ci.yml", &ci), &[])?;
        let needs = gate_needs(gate_job_block(&ci)?)?;
        expect_gaps(
            "ci directives",
            required_step_directive_gaps(&needs, &ci),
            &[],
        )?;
        let alias = read(&root.join("fixtures/guards/cargo/alias-xtask.toml"))?;
        expect_gaps(
            "alias text",
            cargo_alias_gaps(&alias),
            &["cargo alias xtask is \"test --no-run\""],
        )?;
        let planted = TempGit::new("alias-xtask")?;
        std::fs::create_dir_all(planted.path.join(".cargo"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(planted.path.join(".cargo/config.toml"), &alias)
            .map_err(|err| format!("write: {err}"))?;
        expect_gaps(
            "planted alias",
            cargo_config_gaps(&planted.path)?,
            &[
                ".cargo/config.toml does not match the pinned xtask alias",
                "cargo alias xtask is \"test --no-run\"",
            ],
        )?;
        let extra = TempGit::new("extra-cargo")?;
        std::fs::create_dir_all(extra.path.join("nested/.cargo"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::create_dir_all(extra.path.join(".cargo"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(extra.path.join(".cargo/config.toml"), CARGO_CONFIG_TOML)
            .map_err(|err| format!("write: {err}"))?;
        std::fs::copy(
            root.join("fixtures/guards/cargo/extra.toml"),
            extra.path.join("nested/.cargo/config.toml"),
        )
        .map_err(|err| format!("copy: {err}"))?;
        expect_gaps(
            "nested cargo config",
            cargo_config_gaps(&extra.path)?,
            &["unexpected cargo config nested/.cargo/config.toml"],
        )?;
        std::fs::copy(
            root.join("fixtures/guards/cargo/extra.toml"),
            extra.path.join(".cargo/config"),
        )
        .map_err(|err| format!("copy: {err}"))?;
        expect_gaps(
            "bare cargo config",
            cargo_config_gaps(&extra.path)?,
            &[
                "unexpected cargo config .cargo/config",
                "unexpected cargo config nested/.cargo/config.toml",
            ],
        )?;
        let env_cases = [
            (
                "fixtures/guards/ci/cargo-alias-xtask.yml",
                &[
                    "cargo-alias-xtask.yml env sets CARGO_ALIAS_XTASK",
                    "cargo-alias-xtask.yml env sets CARGO_ALIAS_XTASK",
                    "cargo-alias-xtask.yml env sets CARGO_ALIAS_XTASK",
                ][..],
            ),
            (
                "fixtures/guards/ci/rustc-wrapper.yml",
                &["rustc-wrapper.yml env sets RUSTC_WRAPPER"][..],
            ),
            (
                "fixtures/guards/ci/rustc-workspace-wrapper.yml",
                &["rustc-workspace-wrapper.yml env sets RUSTC_WORKSPACE_WRAPPER"][..],
            ),
            (
                "fixtures/guards/ci/cargo-build-rustc.yml",
                &["cargo-build-rustc.yml env sets CARGO_BUILD_RUSTC_WRAPPER"][..],
            ),
            (
                "fixtures/guards/ci/cargo-target-runner.yml",
                &["cargo-target-runner.yml env sets CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER"]
                    [..],
            ),
            (
                "fixtures/guards/ci/cargo-home.yml",
                &["cargo-home.yml env sets CARGO_HOME"][..],
            ),
            (
                "fixtures/guards/ci/rustflags.yml",
                &["rustflags.yml env sets RUSTFLAGS"][..],
            ),
            (
                "fixtures/guards/ci/env-path.yml",
                &[
                    "env-path.yml env sets PATH",
                    "env-path.yml env sets PATH",
                    "env-path.yml env sets PATH",
                ][..],
            ),
            (
                "fixtures/guards/ci/env-ld.yml",
                &["env-ld.yml env sets LD_PRELOAD"][..],
            ),
            (
                "fixtures/guards/ci/env-dyld.yml",
                &["env-dyld.yml env sets DYLD_INSERT_LIBRARIES"][..],
            ),
            (
                "fixtures/guards/ci/env-rustup-home.yml",
                &["env-rustup-home.yml env sets RUSTUP_HOME"][..],
            ),
            (
                "fixtures/guards/ci/env-bash-env.yml",
                &["env-bash-env.yml env sets BASH_ENV"][..],
            ),
            (
                "fixtures/guards/ci/env-env.yml",
                &["env-env.yml env sets ENV"][..],
            ),
        ];
        for (path, expected) in env_cases {
            let name = path.rsplit('/').next().unwrap_or(path);
            let text = read(&root.join(path))?;
            expect_gaps(path, forbidden_env_gaps(name, &text), expected)?;
        }
        let github_cases = [
            (
                "fixtures/guards/ci/github-env-alias.yml",
                &["github-env-alias.yml references GITHUB_ENV"][..],
            ),
            (
                "fixtures/guards/ci/github-env-heredoc.yml",
                &[
                    "github-env-heredoc.yml references GITHUB_ENV",
                    "github-env-heredoc.yml assigns CARGO_HOME",
                ][..],
            ),
            (
                "fixtures/guards/ci/github-env-file.yml",
                &["github-env-file.yml references GITHUB_ENV"][..],
            ),
            (
                "fixtures/guards/ci/github-path.yml",
                &["github-path.yml references GITHUB_PATH"][..],
            ),
            (
                "fixtures/guards/ci/run-compact-github-env.yml",
                &["run-compact-github-env.yml references GITHUB_ENV"][..],
            ),
            (
                "fixtures/guards/ci/run-indirect-github-env.yml",
                &["run-indirect-github-env.yml references GITHUB_ENV"][..],
            ),
            (
                "fixtures/guards/ci/run-python-github-path.yml",
                &["run-python-github-path.yml references GITHUB_PATH"][..],
            ),
            (
                "fixtures/guards/ci/run-export-path.yml",
                &["run-export-path.yml exports PATH"][..],
            ),
            (
                "fixtures/guards/ci/run-inline-path.yml",
                &["run-inline-path.yml assigns PATH"][..],
            ),
            (
                "fixtures/guards/ci/run-inline-rustflags.yml",
                &["run-inline-rustflags.yml assigns RUSTFLAGS"][..],
            ),
            (
                "fixtures/guards/ci/run-export-bash-env.yml",
                &["run-export-bash-env.yml exports BASH_ENV"][..],
            ),
            (
                "fixtures/guards/ci/run-inline-ld.yml",
                &["run-inline-ld.yml assigns LD_PRELOAD"][..],
            ),
        ];
        for (path, expected) in github_cases {
            let name = path.rsplit('/').next().unwrap_or(path);
            let text = read(&root.join(path))?;
            expect_gaps(path, github_env_path_gaps(name, &text), expected)?;
        }
        let shell = read(&root.join("fixtures/guards/ci/step-shell.yml"))?;
        let shell_needs = gate_needs(gate_job_block(&shell)?)?;
        expect_gaps(
            "shell",
            required_step_directive_gaps(&shell_needs, &shell),
            &["ci job fmt sets shell:"],
        )?;
        let directory = read(&root.join("fixtures/guards/ci/step-working-directory.yml"))?;
        let directory_needs = gate_needs(gate_job_block(&directory)?)?;
        expect_gaps(
            "working-directory",
            required_step_directive_gaps(&directory_needs, &directory),
            &["ci job fmt sets working-directory:"],
        )?;
        let syntax_cases = [
            (
                "fixtures/guards/ci/defaults-workflow.yml",
                &["defaults-workflow.yml sets defaults"][..],
            ),
            (
                "fixtures/guards/ci/defaults-job.yml",
                &["defaults-job.yml sets defaults"][..],
            ),
            (
                "fixtures/guards/ci/explicit-if.yml",
                &["explicit-if.yml uses an explicit yaml key"][..],
            ),
            (
                "fixtures/guards/ci/non-ascii-key.yml",
                &["non-ascii-key.yml has a non-ascii key"][..],
            ),
        ];
        for (path, expected) in syntax_cases {
            let name = path.rsplit('/').next().unwrap_or(path);
            let text = read(&root.join(path))?;
            expect_gaps(path, strict_yaml_gaps(name, &text), expected)?;
        }
        let bom = read(&root.join("fixtures/guards/ci/bom-workflow.yml"))?;
        expect_gaps(
            "bom",
            strict_yaml_gaps("bom-workflow.yml", &bom),
            &["bom-workflow.yml has a byte order mark"],
        )?;
        let nul = read(&root.join("fixtures/guards/ci/nul-workflow.yml"))?;
        expect_gaps(
            "nul",
            strict_yaml_gaps("nul-workflow.yml", &nul),
            &["nul-workflow.yml contains NUL"],
        )?;
        let uses_cases = [
            (
                "fixtures/guards/ci/uses-cache.yml",
                &["action actions/cache@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is not allowlisted"]
                    [..],
            ),
            (
                "fixtures/guards/ci/uses-upload-artifact.yml",
                &["action actions/upload-artifact@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is not allowlisted"]
                    [..],
            ),
            (
                "fixtures/guards/ci/uses-download-artifact.yml",
                &["action actions/download-artifact@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is not allowlisted"]
                    [..],
            ),
            (
                "fixtures/guards/ci/uses-local.yml",
                &[
                    "action ./.github/actions/plant is not allowlisted",
                    "action ./.github/actions/plant is not pinned to a sha",
                ][..],
            ),
            (
                "fixtures/guards/ci/uses-docker.yml",
                &[
                    "action docker://alpine:3.20 is not allowlisted",
                    "action docker://alpine:3.20 is not pinned to a sha",
                ][..],
            ),
            (
                "fixtures/guards/ci/uses-reusable.yml",
                &[
                    "reusable workflow KimSeogyu/parangsae/.github/workflows/ci.yml@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is not allowed",
                    "action KimSeogyu/parangsae/.github/workflows/ci.yml@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is not allowlisted",
                ][..],
            ),
        ];
        for (path, expected) in uses_cases {
            let text = read(&root.join(path))?;
            expect_gaps(path, action_pin_gaps(&text), expected)?;
        }
        let smoke = read(&root.join(".github/workflows/vision-smoke.yml"))?;
        expect_gaps("smoke actions", action_pin_gaps(&smoke), &[])?;
        expect_gaps("ci actions", action_pin_gaps(&ci), &[])?;
        let extra_step = read(&root.join("fixtures/guards/ci/guards-extra-step.yml"))?;
        expect_gaps(
            "extra step",
            pinned_guards_steps(&extra_step),
            &["guards job steps are not checkout, toolchain, rustup-which, guards, all-zero"],
        )?;
        Ok(())
    }

    #[test]
    fn expected_guard_runs_matches_the_registry_and_the_pinned_grep() -> Result<(), String> {
        if NON_RATCHET_GUARDS.len() + 1 != EXPECTED_GUARD_RUNS {
            return Err(format!(
                "registry {} is not {EXPECTED_GUARD_RUNS}",
                NON_RATCHET_GUARDS.len() + 1
            ));
        }
        let line = format!("guards ran {EXPECTED_GUARD_RUNS} passed {EXPECTED_GUARD_RUNS}");
        if !GUARDS_STEP.contains(&line) || !ALL_ZERO_STEP.contains(&line) {
            return Err(format!("grep literal {line} is not in the guards steps"));
        }
        let Some((_, guards, _)) = REQUIRED_JOBS.iter().find(|(name, _, _)| *name == "guards")
        else {
            return Err("guards job is not pinned".to_owned());
        };
        if !guards.contains(&line) {
            return Err(format!("pinned guards job is missing {line}"));
        }
        Ok(())
    }

    #[test]
    fn required_jobs_are_pinned_and_hostile_bytes_diff() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("required jobs", required_job_pin_gaps(&ci), &[])?;
        expect_gaps("ci header", ci_header_pin_gaps(&ci), &[])?;
        expect_gaps("deny release", deny_release_pin_gaps(&ci), &[])?;
        expect_gaps("ci shape", required_job_shape_gaps("ci.yml", &ci), &[])?;
        if !ci.contains(RUSTUP_WHICH_STEP) {
            return Err("rustup which step is not in ci.yml".to_owned());
        }
        if !ci.contains(GUARD_FIXTURE_UNREADABLE_STEP) {
            return Err("unreadable floor step is not in ci.yml".to_owned());
        }
        let deny = pinned_job("deny").ok_or("deny job is not pinned")?;
        if !deny.contains(DENY_TOML_SHA256) || !deny.contains(CARGO_LOCK_SHA256) {
            return Err("deny job does not re-hash deny.toml and Cargo.lock".to_owned());
        }
        if !deny.contains("env -i") {
            return Err("deny job does not use env -i".to_owned());
        }
        let cargo_bin = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo";
        let rustc_bin = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc";
        if !RUSTUP_WHICH_STEP.contains(cargo_bin) || !RUSTUP_WHICH_STEP.contains(rustc_bin) {
            return Err("toolchain check does not use the runner's literal paths".to_owned());
        }
        if RUSTUP_WHICH_STEP.contains("RUSTUP_HOME")
            || ci.contains("$RUSTUP_HOME")
            || ci.contains("${RUSTUP_HOME:-")
            || ci.contains("${CARGO_HOME:-")
        {
            return Err("toolchain check reads RUSTUP_HOME or CARGO_HOME".to_owned());
        }
        if deny.matches(&format!("CARGO={cargo_bin}")).count() != 4
            || deny.matches(&format!("RUSTC={rustc_bin}")).count() != 4
        {
            return Err("deny execs do not set absolute CARGO and RUSTC".to_owned());
        }
        if deny.contains("cargo install") || deny.contains("cargo deny") {
            return Err("deny job still invokes cargo install or cargo deny".to_owned());
        }
        if !deny.contains(DENY_TARBALL_SHA256) || !deny.contains(DENY_BIN_SHA256) {
            return Err("deny job does not pin the release tarball and binary".to_owned());
        }
        if deny.matches(DENY_BIN_SHA256).count() != 3 {
            return Err(format!(
                "deny job binary sha checks: {}",
                deny.matches(DENY_BIN_SHA256).count()
            ));
        }
        if deny.matches(CARGO_CONFIG_SHA256).count() != 2 {
            return Err(format!(
                "deny job cargo config sha checks: {}",
                deny.matches(CARGO_CONFIG_SHA256).count()
            ));
        }
        if deny.matches(DENY_TOML_SHA256).count() != 2 {
            return Err(format!(
                "deny job deny.toml sha checks: {}",
                deny.matches(DENY_TOML_SHA256).count()
            ));
        }
        let deny_bin_path =
            format!("/tmp/cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl/cargo-deny");
        let deny_lines: Vec<&str> = deny.lines().collect();
        let mut deny_execs = 0usize;
        for (idx, line) in deny_lines.iter().enumerate() {
            if line.contains(&deny_bin_path) && !line.contains("sha256sum") {
                deny_execs += 1;
                let from = idx.saturating_sub(40);
                let window = deny_lines[from..idx].join("\n");
                if !window.contains(DENY_BIN_SHA256)
                    || !window.contains(CARGO_CONFIG_SHA256)
                    || !window.contains(".cargo/config must be absent")
                    || !window.contains("cargo xtask guards toolchain-files")
                {
                    return Err(format!(
                        "cargo-deny execution is not immediately preceded by the binary and config rechecks: {line}"
                    ));
                }
            }
        }
        if deny_execs != 2 {
            return Err(format!("cargo-deny executions: {deny_execs}"));
        }
        let bin_parts: Vec<&str> = deny.split(DENY_BIN_SHA256).collect();
        if bin_parts.len() != 4 {
            return Err(format!("binary sha splits: {}", bin_parts.len()));
        }
        let install_only = format!(
            "name: ci\non:\n  push:\njobs:\n{}{DENY_BIN_SHA256}{}{}{}",
            bin_parts[0], bin_parts[1], bin_parts[2], bin_parts[3]
        );
        expect_gaps(
            "install-only binary sha",
            deny_release_pin_gaps(&install_only),
            &["deny job does not re-check the cargo-deny binary sha256 before execution"],
        )?;
        if !deny.contains(&format!(
            "cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl.tar.gz"
        )) {
            return Err("deny job tarball name does not use DENY_VERSION".to_owned());
        }
        if !deny.contains("/usr/bin/curl")
            || !deny.contains("/usr/bin/sha256sum")
            || !deny.contains("/usr/bin/tar")
        {
            return Err("deny job does not use absolute curl, sha256sum, and tar".to_owned());
        }
        let deny_bin =
            format!("/tmp/cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl/cargo-deny");
        if deny.matches(&deny_bin).count() < 2 {
            return Err("deny job does not run the extracted binary by absolute path".to_owned());
        }
        if deny.contains("/home/runner/.cargo/bin") {
            return Err("deny job PATH still includes cargo's bin".to_owned());
        }
        let toolchain_bin = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin";
        let toolchain_first = format!(
            "PATH={toolchain_bin}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
        );
        let system_first = format!(
            "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:{toolchain_bin}"
        );
        if !deny.contains(&toolchain_first) || deny.contains(&system_first) {
            return Err("deny env -i PATH does not put the toolchain bin first".to_owned());
        }
        if !deny.contains("pre-exists") || !deny.contains("[ -L \"$p\" ]") {
            return Err("deny job does not reject a pre-existing extraction path".to_owned());
        }
        let planted_path = ci.replace(&toolchain_first, &system_first);
        let planted_gaps = required_job_pin_gaps(&planted_path);
        if !planted_gaps.iter().any(|gap| {
            gap.contains("ci job deny bytes do not match") && gap.contains(&system_first)
        }) {
            return Err(format!("system-first PATH was accepted: {planted_gaps:?}"));
        }
        if !deny.contains(RUST_TOOLCHAIN_SHA256) || !deny.contains(CARGO_MANIFESTS_SHA256) {
            return Err(
                "deny job does not re-hash rust-toolchain.toml and the crate manifests".to_owned(),
            );
        }
        for path in CARGO_MANIFESTS {
            if !deny.contains(path) {
                return Err(format!("deny job does not re-hash {path}"));
            }
        }
        let preimage = cargo_manifests_preimage(&root)?;
        if sha256_hex(preimage.as_bytes()) != CARGO_MANIFESTS_SHA256 {
            return Err("cargo manifests sha256 does not match".to_owned());
        }
        let mut flipped = preimage.into_bytes();
        flipped[0] ^= 0x01;
        if sha256_hex(&flipped) == CARGO_MANIFESTS_SHA256 {
            return Err("flipped cargo manifests still matched the pin".to_owned());
        }
        expect_gaps(
            "toolchain",
            toolchain_bytes_gaps(&read_bytes(&root.join("rust-toolchain.toml"))?),
            &[],
        )?;
        expect_gaps("workspace pins", cargo_workspace_dir_violations(&root), &[])?;
        let planted = read(&root.join("fixtures/guards/toolchain/absolute-path.toml"))?;
        expect_gaps(
            "toolchain path",
            toolchain_bytes_gaps(planted.as_bytes()),
            &[
                "rust-toolchain.toml does not match",
                "rust-toolchain.toml sets path=",
            ],
        )?;
        let structural = [
            ("fixtures/guards/ci/run-printf-github-env.yml", "printf"),
            ("fixtures/guards/ci/run-eval.yml", "eval"),
            ("fixtures/guards/ci/run-python-c.yml", "python -c"),
            ("fixtures/guards/ci/run-source.yml", "source"),
        ];
        for (path, needle) in structural {
            let text = read(&root.join(path))?;
            let gaps = required_job_pin_gaps(&text);
            let Some(diff) = gaps
                .iter()
                .find(|gap| gap.contains("ci job fmt bytes do not match"))
            else {
                return Err(format!("{path} did not diff fmt: {gaps:?}"));
            };
            if !diff.contains("--- pinned fmt") || !diff.contains(needle) {
                return Err(format!("{path} diff missed {needle}: {diff}"));
            }
            let grammar = github_env_path_gaps(path, &text);
            if grammar.iter().any(|gap| gap.contains(needle)) {
                return Err(format!("{path} was caught by a text denylist: {grammar:?}"));
            }
        }
        let escaped = read(&root.join("fixtures/guards/ci/run-escaped-quote.yml"))?;
        expect_gaps(
            "escaped quote",
            github_env_path_gaps("run-escaped-quote.yml", &escaped),
            &["run-escaped-quote.yml references GITHUB_ENV"],
        )?;
        let indented = read(&root.join("fixtures/guards/ci/block-scalar-indent.yml"))?;
        expect_gaps(
            "indent scalar",
            strict_yaml_gaps("block-scalar-indent.yml", &indented),
            &["block-scalar-indent.yml uses an indented block scalar"],
        )?;
        let shape_cases = [
            (
                "fixtures/guards/ci/job-container.yml",
                &["job-container.yml job fmt sets container:"][..],
            ),
            (
                "fixtures/guards/ci/job-services.yml",
                &["job-services.yml job fmt sets services:"],
            ),
            (
                "fixtures/guards/ci/env-cargo-encoded.yml",
                &["env-cargo-encoded.yml job fmt env CARGO_ENCODED_RUSTFLAGS is not allowlisted"],
            ),
        ];
        for (path, expected) in shape_cases {
            let name = path.rsplit('/').next().unwrap_or(path);
            let text = read(&root.join(path))?;
            expect_gaps(path, required_job_shape_gaps(name, &text), expected)?;
        }
        let strategy = ci.replacen(
            "  fmt:\n",
            "  fmt:\n    strategy:\n      fail-fast: false\n",
            1,
        );
        let strategy_gaps = required_job_pin_gaps(&strategy);
        let strategy_diff = strategy_gaps
            .iter()
            .find(|gap| gap.contains("ci job fmt bytes do not match"))
            .ok_or_else(|| format!("strategy pin: {strategy_gaps:?}"))?;
        if !strategy_diff.contains("\n+    strategy:\n") {
            return Err(format!("strategy diff: {strategy_diff}"));
        }
        expect_gaps(
            "strategy shape",
            required_job_shape_gaps("ci.yml", &strategy),
            &["ci.yml job fmt sets strategy:"],
        )?;
        let matrix = ci.replacen("  fmt:\n", "  fmt:\n    matrix:\n      rust: [stable]\n", 1);
        expect_gaps(
            "matrix shape",
            required_job_shape_gaps("ci.yml", &matrix),
            &["ci.yml job fmt sets matrix:"],
        )?;
        let matrix_gaps = required_job_pin_gaps(&matrix);
        if !matrix_gaps
            .iter()
            .any(|gap| gap.contains("\n+    matrix:\n"))
        {
            return Err(format!("matrix diff: {matrix_gaps:?}"));
        }
        let header_cases = [
            (
                "fixtures/guards/ci/workflow-env-cargo.yml",
                "  CARGO: /tmp/fake",
            ),
            (
                "fixtures/guards/ci/workflow-env-rustflags.yml",
                "  RUSTFLAGS: -Awarnings",
            ),
            (
                "fixtures/guards/ci/workflow-env-rustc.yml",
                "  RUSTC: /tmp/rustc",
            ),
            (
                "fixtures/guards/ci/workflow-env-encoded-rustflags.yml",
                "  CARGO_ENCODED_RUSTFLAGS: -Awarnings",
            ),
            (
                "fixtures/guards/ci/workflow-env-rustdocflags.yml",
                "  RUSTDOCFLAGS: -Awarnings",
            ),
            (
                "fixtures/guards/ci/workflow-env-target-dir.yml",
                "  CARGO_TARGET_DIR: /tmp/target",
            ),
            ("fixtures/guards/ci/workflow-defaults.yml", "defaults:"),
        ];
        for (path, needle) in header_cases {
            let text = read(&root.join(path))?;
            let gaps = ci_header_pin_gaps(&text);
            let Some(diff) = gaps
                .iter()
                .find(|gap| gap.contains("ci header bytes do not match"))
            else {
                return Err(format!("{path} did not diff the header: {gaps:?}"));
            };
            if !diff.contains("--- pinned header") || !diff.contains(needle) {
                return Err(format!("{path} diff missed {needle}: {diff}"));
            }
        }
        let cargo_env = read(&root.join("fixtures/guards/ci/workflow-env-cargo.yml"))?;
        let cargo_grammar = forbidden_env_gaps("workflow-env-cargo.yml", &cargo_env);
        if cargo_grammar.iter().any(|gap| gap.contains("CARGO")) {
            return Err(format!(
                "workflow CARGO was caught by a text denylist: {cargo_grammar:?}"
            ));
        }
        let missing_bin = read(&root.join("fixtures/guards/ci/deny-no-bin-sha.yml"))?;
        let missing_gaps = required_job_pin_gaps(&missing_bin);
        let Some(missing_diff) = missing_gaps
            .iter()
            .find(|gap| gap.contains("ci job deny bytes do not match"))
        else {
            return Err(format!("deny pin: {missing_gaps:?}"));
        };
        if !missing_diff.contains(DENY_BIN_SHA256) {
            return Err(format!("deny diff missed the binary sha: {missing_diff}"));
        }
        expect_gaps(
            "missing bin sha",
            deny_release_pin_gaps(&missing_bin),
            &["deny job does not pin the cargo-deny binary sha256"],
        )?;
        let smoke = read(&root.join(".github/workflows/vision-smoke.yml"))?;
        let smoke_triggers = workflow_triggers(&smoke);
        if smoke_triggers != ["workflow_dispatch".to_owned()] {
            return Err(format!("vision-smoke triggers: {smoke_triggers:?}"));
        }
        expect_gaps(
            "smoke push",
            foreign_workflow_gaps(".github/workflows/vision-smoke.yml", &smoke),
            &[],
        )?;
        let side = "name: side\non:\n  push:\npermissions:\n  contents: read\njobs:\n  sidejob:\n    runs-on: ubuntu-latest\n    steps:\n      - run: true\n";
        expect_gaps(
            "side push",
            foreign_workflow_gaps(".github/workflows/side.yml", side),
            &[".github/workflows/side.yml must be workflow_dispatch only"],
        )?;
        expect_gaps("order", compiling_job_order_gaps(&ci), &[])?;
        let smoke_block = job_block(&smoke, "smoke").ok_or("vision-smoke has no smoke job")?;
        expect_gaps(
            "smoke order",
            compile_step_order_gaps("vision-smoke", smoke_block),
            &[],
        )?;
        expect_gaps(
            "smoke pin",
            pinned_whole_file_gaps(".github/workflows/vision-smoke.yml", smoke.as_bytes()),
            &[],
        )?;
        expect_gaps(
            "extra workflow",
            if workflow_file_pin(".github/workflows/side.yml").is_some() {
                Vec::new()
            } else {
                vec![".github/workflows/side.yml is not an allowed workflow file".to_owned()]
            },
            &[".github/workflows/side.yml is not an allowed workflow file"],
        )?;
        let pull = read(&root.join("fixtures/guards/ci/side-pull-request.yml"))?;
        expect_gaps(
            "side pull",
            foreign_workflow_gaps("side-pull-request.yml", &pull),
            &["side-pull-request.yml must be workflow_dispatch only"],
        )?;
        let spoof = read(&root.join("fixtures/guards/ci/side-gate-name.yml"))?;
        expect_gaps(
            "side gate",
            foreign_workflow_gaps("side-gate-name.yml", &spoof),
            &["side-gate-name.yml job spoof reuses required check name"],
        )?;
        let foreign_secret = read(&root.join("fixtures/guards/ci/smoke-github-token.yml"))?;
        expect_gaps(
            "foreign secret",
            foreign_workflow_gaps("smoke-github-token.yml", &foreign_secret),
            &["smoke-github-token.yml secret GITHUB_TOKEN is not allowlisted"],
        )?;
        let demo = "name: side\non:\n  workflow_dispatch:\npermissions:\n  contents: read\njobs:\n  smoke:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo ${{ secrets.BINANCE_DEMO_API_KEY }} ${{ secrets.BINANCE_DEMO_API_SECRET }}\n";
        expect_gaps("demo secrets", foreign_workflow_gaps("side.yml", demo), &[])?;
        let other = demo.replace("BINANCE_DEMO_API_SECRET", "BINANCE_DEMO_OTHER");
        expect_gaps(
            "other demo secret",
            foreign_workflow_gaps("side.yml", &other),
            &["side.yml secret BINANCE_DEMO_OTHER is not allowlisted"],
        )?;
        Ok(())
    }

    #[test]
    fn toolchain_override_files_are_rejected() -> Result<(), String> {
        let dir = TempGit::new("toolchain-files")?;
        git_fixture(&dir.path, &["init"])?;
        std::fs::write(dir.path.join("rust-toolchain"), "nightly\n")
            .map_err(|err| format!("write: {err}"))?;
        std::fs::create_dir_all(dir.path.join("specs/tla"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(
            dir.path.join("specs/tla/rust-toolchain.toml"),
            "path = \"/tmp/fake\"\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        std::fs::create_dir_all(dir.path.join("crates/x"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(dir.path.join("crates/x/rust-toolchain"), "1.98.0\n")
            .map_err(|err| format!("write: {err}"))?;
        git_fixture(&dir.path, &["add", "specs/tla/rust-toolchain.toml"])?;
        let gaps = toolchain_file_gaps(&dir.path)?;
        let expected = [
            "crates/x/rust-toolchain is a toolchain override; only the root rust-toolchain.toml is allowed",
            "rust-toolchain is a toolchain override; only the root rust-toolchain.toml is allowed",
            "specs/tla/rust-toolchain.toml is a toolchain override; only the root rust-toolchain.toml is allowed",
        ];
        expect_gaps("overrides", gaps, &expected)?;
        let link = TempGit::new("toolchain-link")?;
        git_fixture(&link.path, &["init"])?;
        std::fs::write(
            link.path.join("real.toml"),
            "[toolchain]\nchannel = \"1.98.0\"\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        std::os::unix::fs::symlink("real.toml", link.path.join("rust-toolchain.toml"))
            .map_err(|err| format!("symlink: {err}"))?;
        expect_gaps(
            "symlink",
            toolchain_file_gaps(&link.path)?,
            &["root rust-toolchain.toml is a symlink"],
        )?;
        Ok(())
    }

    #[test]
    fn required_job_names_equal_pinned_gate_needs() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("live needs", required_jobs_needs_gaps(&ci)?, &[])?;
        let missing = read(&root.join("fixtures/guards/ci/needs-missing-required.yml"))?;
        expect_gaps(
            "missing required",
            required_jobs_needs_gaps(&missing)?,
            &["required job deny is missing from gate.needs"],
        )?;
        let extra = read(&root.join("fixtures/guards/ci/needs-extra-required.yml"))?;
        expect_gaps(
            "extra required",
            required_jobs_needs_gaps(&extra)?,
            &["gate.needs lists phantom which is not a required job"],
        )?;
        Ok(())
    }

    #[test]
    fn ignored_untracked_toolchain_and_cargo_config_are_rejected() -> Result<(), String> {
        let dir = TempGit::new("ignored-tree")?;
        git_fixture(&dir.path, &["init"])?;
        std::fs::write(dir.path.join(".gitignore"), "hidden/\n")
            .map_err(|err| format!("write: {err}"))?;
        std::fs::create_dir_all(dir.path.join("hidden/nested/.cargo"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(dir.path.join("hidden/rust-toolchain"), "nightly\n")
            .map_err(|err| format!("write: {err}"))?;
        std::fs::write(
            dir.path.join("hidden/nested/.cargo/config.toml"),
            "[alias]\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        std::fs::write(dir.path.join("hidden/nested/.cargo/config"), "paths = []\n")
            .map_err(|err| format!("write: {err}"))?;
        git_fixture(&dir.path, &["add", ".gitignore"])?;
        git_fixture(&dir.path, &["commit", "-m", "ignore hidden"])?;
        expect_gaps(
            "ignored tree",
            toolchain_file_gaps(&dir.path)?,
            &[
                "hidden/nested/.cargo/config is a nested cargo config; only the root .cargo/config.toml is allowed",
                "hidden/nested/.cargo/config.toml is a nested cargo config; only the root .cargo/config.toml is allowed",
                "hidden/rust-toolchain is a toolchain override; only the root rust-toolchain.toml is allowed",
            ],
        )?;
        Ok(())
    }

    #[test]
    fn nested_target_is_scanned_and_nested_git_is_rejected() -> Result<(), String> {
        let scanned = TempGit::new("nested-target")?;
        git_fixture(&scanned.path, &["init"])?;
        std::fs::create_dir_all(scanned.path.join("target"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(scanned.path.join("target/rust-toolchain"), "nightly\n")
            .map_err(|err| format!("write: {err}"))?;
        std::fs::create_dir_all(scanned.path.join("crates/vendor/target"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(
            scanned.path.join("crates/vendor/target/rust-toolchain"),
            "nightly\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        expect_gaps(
            "nested target",
            toolchain_file_gaps(&scanned.path)?,
            &["crates/vendor/target/rust-toolchain is a toolchain override; only the root rust-toolchain.toml is allowed"],
        )?;
        let embedded = TempGit::new("nested-git")?;
        git_fixture(&embedded.path, &["init"])?;
        std::fs::create_dir_all(embedded.path.join("crates/vendor/.git"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(
            embedded.path.join("crates/vendor/.git/HEAD"),
            "ref: refs/heads/main\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        match toolchain_file_gaps(&embedded.path) {
            Err(err) if err == "crates/vendor/.git is a nested git repository" => Ok(()),
            other => Err(format!("nested git: {other:?}")),
        }
    }

    #[test]
    fn deny_step_rejects_extensionless_cargo_config() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("live deny", deny_release_pin_gaps(&ci), &[])?;
        let bare = read(&root.join("fixtures/guards/ci/deny-extensionless-config.yml"))?;
        expect_gaps(
            "extensionless config",
            deny_release_pin_gaps(&bare),
            &["deny job does not reject an extensionless .cargo/config"],
        )?;
        let deny = pinned_job("deny").ok_or("deny job is not pinned")?;
        if deny.matches(".cargo/config must be absent").count() != 2 {
            return Err(format!(
                "absence checks: {}",
                deny.matches(".cargo/config must be absent").count()
            ));
        }
        Ok(())
    }

    #[test]
    fn deny_step_walks_for_unexpected_toolchain_files() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("live deny walk", deny_release_pin_gaps(&ci), &[])?;
        let bare = read(&root.join("fixtures/guards/ci/deny-no-tree-walk.yml"))?;
        expect_gaps(
            "no tree walk",
            deny_release_pin_gaps(&bare),
            &["deny job does not reject unexpected cargo config or toolchain files"],
        )?;
        let planted = TempGit::new("toolchain-prefix")?;
        git_fixture(&planted.path, &["init"])?;
        std::fs::create_dir_all(planted.path.join("vendor/nested/.cargo"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::create_dir_all(planted.path.join("vendor/.cargo"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(
            planted.path.join("vendor/.cargo/config.extra"),
            "paths = []\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        std::fs::write(
            planted.path.join("vendor/rust-toolchain.extra"),
            "nightly\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        std::fs::write(
            planted.path.join("vendor/nested/.cargo/config.toml"),
            "[alias]\n",
        )
        .map_err(|err| format!("write: {err}"))?;
        expect_gaps(
            "prefix files",
            toolchain_file_gaps(&planted.path)?,
            &[
                "vendor/.cargo/config.extra is a nested cargo config; only the root .cargo/config.toml is allowed",
                "vendor/nested/.cargo/config.toml is a nested cargo config; only the root .cargo/config.toml is allowed",
                "vendor/rust-toolchain.extra is a toolchain override; only the root rust-toolchain.toml is allowed",
            ],
        )?;
        let deny = pinned_job("deny").ok_or("deny job is not pinned")?;
        if deny.matches("cargo xtask guards toolchain-files").count() != 2 {
            return Err(format!(
                "tree walks: {}",
                deny.matches("cargo xtask guards toolchain-files").count()
            ));
        }
        Ok(())
    }

    #[test]
    fn toolchain_walk_outside_env_i_can_auto_install() -> Result<(), String> {
        let bare = "          /home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo xtask guards toolchain-files\n";
        expect_gaps(
            "bare walk",
            deny_toolchain_walk_gaps(bare),
            &["deny job toolchain walk can auto-install through rustup"],
        )?;
        let root = crate::workspace_root()?;
        let missing = read(&root.join("fixtures/guards/ci/deny-no-tree-walk.yml"))?;
        expect_gaps("absent walk", deny_toolchain_walk_gaps(&missing), &[])?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("live walk", deny_toolchain_walk_gaps(&ci), &[])?;
        Ok(())
    }

    #[test]
    fn swapped_deny_binary_fails_the_recheck() -> Result<(), String> {
        let dir = TempGit::new("deny-bin")?;
        let tarball = cached_deny_tarball()?;
        let extract = Command::new("/usr/bin/tar")
            .arg("-xzf")
            .arg(&tarball)
            .arg("-C")
            .arg(&dir.path)
            .output()
            .map_err(|err| format!("tar: {err}"))?;
        if !extract.status.success() {
            return Err(format!(
                "tar failed: {}",
                String::from_utf8_lossy(&extract.stderr)
            ));
        }
        let bin = dir.path.join(format!(
            "cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl/cargo-deny"
        ));
        let script = deny_binary_recheck_script(&bin)?;
        let (code, output) = run_shell(&script)?;
        if code != 0 {
            return Err(format!("provisioned binary failed the recheck: {output}"));
        }
        let mut bytes = std::fs::read(&bin).map_err(|err| format!("read binary: {err}"))?;
        let Some(last) = bytes.last_mut() else {
            return Err("cargo-deny binary is empty".to_owned());
        };
        *last ^= 0xff;
        std::fs::write(&bin, &bytes).map_err(|err| format!("write binary: {err}"))?;
        let (code, output) = run_shell(&script)?;
        if code == 0 || !output.contains("cargo-deny binary sha256 does not match") {
            return Err(format!("swapped binary recheck: code {code}: {output}"));
        }
        Ok(())
    }

    #[test]
    fn changed_deny_input_fails_the_rehash() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let dir = TempGit::new("deny-inputs")?;
        provision_deny_inputs(&root, &dir.path)?;
        let script = deny_input_rehash_script()?;
        let (code, output) = run_shell_at(&script, &dir.path)?;
        if code != 0 {
            return Err(format!("provisioned inputs failed the rehash: {output}"));
        }
        let cases = [
            (
                "deny.toml",
                "deny.toml sha256 does not match",
                "Cargo.lock sha256 does not match",
            ),
            (
                "Cargo.lock",
                "Cargo.lock sha256 does not match",
                "rust-toolchain.toml sha256 does not match",
            ),
            (
                "rust-toolchain.toml",
                "rust-toolchain.toml sha256 does not match",
                "cargo manifests sha256 does not match",
            ),
            (
                "xtask/Cargo.toml",
                "cargo manifests sha256 does not match",
                ".cargo/config.toml sha256 does not match",
            ),
            (
                ".cargo/config.toml",
                ".cargo/config.toml sha256 does not match",
                "cargo-deny binary sha256 does not match",
            ),
        ];
        for (rel, message, later) in cases {
            let path = dir.path.join(rel);
            let mut bytes = std::fs::read(&path).map_err(|err| format!("read {rel}: {err}"))?;
            let Some(last) = bytes.last_mut() else {
                return Err(format!("{rel} is empty"));
            };
            let saved = *last;
            *last ^= 0xff;
            std::fs::write(&path, &bytes).map_err(|err| format!("write {rel}: {err}"))?;
            let (code, output) = run_shell_at(&script, &dir.path)?;
            bytes = std::fs::read(&path).map_err(|err| format!("reread {rel}: {err}"))?;
            let Some(last) = bytes.last_mut() else {
                return Err(format!("{rel} is empty"));
            };
            *last = saved;
            std::fs::write(&path, &bytes).map_err(|err| format!("restore {rel}: {err}"))?;
            if code == 0 || !output.contains(message) || output.contains(later) {
                return Err(format!("{rel}: code {code}: {output}"));
            }
        }
        Ok(())
    }

    #[test]
    fn compiling_jobs_reject_inline_tool_assignments() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("ci tools", compiling_tool_env_gaps("ci.yml", &ci), &[])?;
        let smoke = read(&root.join(".github/workflows/vision-smoke.yml"))?;
        expect_gaps(
            "smoke tools",
            compiling_tool_env_gaps("vision-smoke.yml", &smoke),
            &[],
        )?;
        let fixture = read(&root.join("fixtures/guards/ci/run-inline-tool-env.yml"))?;
        expect_gaps(
            "run-inline-tool-env.yml",
            compiling_tool_env_gaps("run-inline-tool-env.yml", &fixture),
            &[
                "run-inline-tool-env.yml job fmt assigns CARGO=/tmp/fake",
                "run-inline-tool-env.yml job fmt exports RUSTC",
                "run-inline-tool-env.yml job fmt assigns RUSTFLAGS=-Awarnings",
            ],
        )?;
        let prefix = read(&root.join("fixtures/guards/ci/deny-prefix-cargo.yml"))?;
        let cargo = "/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo";
        expect_gaps(
            "deny-prefix-cargo.yml",
            compiling_tool_env_gaps("deny-prefix-cargo.yml", &prefix),
            &[
                "deny-prefix-cargo.yml job deny assigns CARGO=x",
                &format!("deny-prefix-cargo.yml job deny assigns CARGO={cargo}"),
            ],
        )?;
        Ok(())
    }

    #[test]
    fn job_block_ignores_header_and_run_body() -> Result<(), String> {
        let text = "\
name: ci
on:
  push:
  gate:
jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - name: show
        run: |
          echo '  gate:'
          echo \"  push:\"
  gate:
    runs-on: ubuntu-latest
    steps:
      - run: echo real-gate
";
        let gate = job_block(text, "gate").ok_or("gate job missing")?;
        if !gate.contains("echo real-gate") || gate.contains("echo '  gate:'") {
            return Err(format!(
                "header or run body was treated as the gate job:\n{gate}"
            ));
        }
        if job_block(text, "push").is_some() {
            return Err("header id push was treated as a job".to_owned());
        }
        let fmt = job_block(text, "fmt").ok_or("fmt job missing")?;
        if !fmt.contains("echo '  gate:'") || fmt.contains("echo real-gate") {
            return Err(format!("fmt job:\n{fmt}"));
        }
        Ok(())
    }

    fn job_key_findings(name: &str, text: &str) -> Vec<String> {
        let mut gaps = strict_yaml_gaps(name, text);
        match ci_job_ids(text) {
            Ok(ids) => {
                for id in ids {
                    gaps.push(format!("{name} job {id}"));
                }
            }
            Err(err) => gaps.push(format!("{name} {err}")),
        }
        if let Some(fmt) = job_block(text, "fmt") {
            if fmt.contains("echo clippy-job") || fmt.contains("\n  clippy:\n") {
                gaps.push(format!("{name} job fmt includes clippy"));
            } else if fmt.contains("echo still-fmt") {
                gaps.push(format!("{name} job fmt keeps echo still-fmt"));
            }
            if job_steps(fmt)
                .iter()
                .any(|step| step.contains("\n  clippy:\n"))
            {
                gaps.push(format!("{name} step includes clippy"));
            }
        }
        gaps
    }

    #[test]
    fn job_block_fixtures_are_not_jobs() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let header = read(&root.join("fixtures/guards/ci/job-id-header.yml"))?;
        let fmt = job_block(&header, "fmt").ok_or("header fixture has no fmt job")?;
        if !fmt.contains("echo real-fmt") || !fmt.contains("runs-on:") {
            return Err(format!("header key was extracted as fmt:\n{fmt}"));
        }
        if job_block(&header, "push").is_some() {
            return Err("header id push was extracted as a job".to_owned());
        }
        let run_block = read(&root.join("fixtures/guards/ci/job-id-run-block.yml"))?;
        assert_yaml_parses("fixtures/guards/ci/job-id-run-block.yml", &run_block)?;
        expect_gaps(
            "run block",
            job_key_findings("job-id-run-block.yml", &run_block),
            &[
                "job-id-run-block.yml uses an indented block scalar",
                "job-id-run-block.yml job clippy",
                "job-id-run-block.yml job fmt",
            ],
        )?;
        let empty = read(&root.join("fixtures/guards/ci/job-id-empty-block.yml"))?;
        assert_yaml_parses("fixtures/guards/ci/job-id-empty-block.yml", &empty)?;
        expect_gaps(
            "empty block",
            job_key_findings("job-id-empty-block.yml", &empty),
            &[
                "job-id-empty-block.yml job clippy",
                "job-id-empty-block.yml job fmt",
            ],
        )?;
        let boundary = read(&root.join("fixtures/guards/ci/job-id-block-boundary.yml"))?;
        assert_yaml_parses("fixtures/guards/ci/job-id-block-boundary.yml", &boundary)?;
        expect_gaps(
            "block boundary",
            job_key_findings("job-id-block-boundary.yml", &boundary),
            &[
                "job-id-block-boundary.yml job clippy",
                "job-id-block-boundary.yml job fmt",
                "job-id-block-boundary.yml job fmt keeps echo still-fmt",
            ],
        )?;
        for indicator in ["|2-", "|-2"] {
            let text = format!(
                "\
name: ci
on:
  push:
jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - run: {indicator}
          echo kept
  clippy:
    runs-on: ubuntu-latest
    steps:
      - run: echo clippy-job
"
            );
            assert_yaml_parses(indicator, &text)?;
            let clippy =
                job_block(&text, "clippy").ok_or_else(|| format!("{indicator} hid clippy"))?;
            if !clippy.contains("echo clippy-job") {
                return Err(format!("{indicator} clippy job:\n{clippy}"));
            }
        }
        Ok(())
    }

    #[test]
    fn deny_run_step_fixtures_fail() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let shim = read(&root.join("fixtures/guards/ci/deny-path-shim.yml"))?;
        expect_gaps(
            "path shim",
            deny_release_pin_gaps(&shim),
            &["deny job PATH entry /tmp/shim is before the toolchain bin"],
        )?;
        let job_cargo = read(&root.join("fixtures/guards/ci/deny-job-cargo-env.yml"))?;
        expect_gaps(
            "job cargo",
            deny_release_pin_gaps(&job_cargo),
            &["deny job sets CARGO=/path/to/fake in the step or job env"],
        )?;
        let step_cargo = read(&root.join("fixtures/guards/ci/deny-step-cargo-env.yml"))?;
        expect_gaps(
            "step cargo",
            deny_release_pin_gaps(&step_cargo),
            &["deny job sets CARGO=/path/to/fake in the step or job env"],
        )?;
        let swapped = read(&root.join("fixtures/guards/ci/deny-swapped-bin.yml"))?;
        expect_gaps(
            "swapped bin",
            deny_release_pin_gaps(&swapped),
            &["deny job does not re-check the cargo-deny binary sha256 before execution"],
        )?;
        Ok(())
    }

    fn which_job(path: &str) -> Result<String, String> {
        let text = read(&crate::workspace_root()?.join(path))?;
        job_block(&text, "fmt")
            .map(str::to_owned)
            .ok_or_else(|| format!("{path} has no fmt job"))
    }

    #[test]
    fn which_assert_fixtures_name_the_gap() -> Result<(), String> {
        let missing = "  fmt:\n    steps:\n      - run: cargo test\n";
        expect_gaps(
            "no which step",
            compile_step_order_gaps("fmt", missing),
            &["fmt has no rustup which step"],
        )?;
        let rustc_first = format!("  fmt:\n    steps:\n{RUSTC_198_STEP}{RUSTUP_WHICH_STEP}");
        expect_gaps(
            "rustc -V before which",
            compile_step_order_gaps("fmt", &rustc_first),
            &["fmt runs rustc -V before rustup which"],
        )?;
        let cargo_first = format!(
            "  fmt:\n    steps:\n      - run: cargo test\n{RUSTUP_WHICH_STEP}{RUSTC_198_STEP}"
        );
        expect_gaps(
            "cargo before which",
            compile_step_order_gaps("fmt", &cargo_first),
            &["fmt runs cargo or rustc before rustup which"],
        )?;
        let cargo_before = which_job("fixtures/guards/ci/which-cargo-before.yml")?;
        let rustc_at = cargo_before
            .find(RUSTC_198_STEP)
            .ok_or("which-cargo-before.yml dropped the rustc -V step")?;
        let which_at = cargo_before
            .find(RUSTUP_WHICH_STEP)
            .ok_or("which-cargo-before.yml dropped the which-assert")?;
        if rustc_at > which_at {
            return Err("which-cargo-before.yml puts which before rustc -V".to_owned());
        }
        let between = &cargo_before[rustc_at + RUSTC_198_STEP.len()..which_at];
        if !between.contains("\n        run: cargo test\n") {
            return Err(format!(
                "which-cargo-before.yml does not run cargo between rustc -V and which: {between}"
            ));
        }
        expect_gaps(
            "which-cargo-before.yml",
            compile_step_order_gaps("which-cargo-before.yml", &cargo_before),
            &[
                "which-cargo-before.yml runs rustc -V before rustup which",
                "which-cargo-before.yml runs cargo or rustc before rustup which",
            ],
        )?;
        let glob = which_job("fixtures/guards/ci/which-glob.yml")?;
        if glob.contains(RUSTUP_WHICH_STEP) {
            return Err("which-glob.yml still has the literal toolchain path".to_owned());
        }
        expect_gaps(
            "which-glob.yml",
            compile_step_order_gaps("which-glob.yml", &glob),
            &["which-glob.yml which-assert uses a glob"],
        )?;
        let question = glob.replace(
            "/home/runner/.rustup/toolchains/\"*\"",
            "/home/runner/.rustup/toolchains/1.98.?",
        );
        expect_gaps(
            "question glob",
            compile_step_order_gaps("which-glob.yml", &question),
            &["which-glob.yml which-assert uses a glob"],
        )?;
        let home = which_job("fixtures/guards/ci/which-rustup-home.yml")?;
        if home.contains(RUSTUP_WHICH_STEP) || !home.contains("$RUSTUP_HOME/toolchains/") {
            return Err("which-rustup-home.yml does not use $RUSTUP_HOME".to_owned());
        }
        expect_gaps(
            "which-rustup-home.yml",
            compile_step_order_gaps("which-rustup-home.yml", &home),
            &["which-rustup-home.yml which-assert uses RUSTUP_HOME"],
        )?;
        let braced = home.replace("$RUSTUP_HOME/", "${RUSTUP_HOME}/");
        expect_gaps(
            "braced RUSTUP_HOME",
            compile_step_order_gaps("which-rustup-home.yml", &braced),
            &["which-rustup-home.yml which-assert uses RUSTUP_HOME"],
        )?;
        Ok(())
    }

    #[test]
    fn allowlisted_yml_without_a_path_keyed_pin_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let mut on_disk = Vec::new();
        let dir = root.join(".github/workflows");
        for entry in std::fs::read_dir(&dir).map_err(|err| format!("read workflows: {err}"))? {
            let entry = entry.map_err(|err| format!("read workflows: {err}"))?;
            let path = entry.path();
            if path.is_file() {
                on_disk.push(workflow_display(&root, &path));
            }
        }
        on_disk.sort();
        let mut pinned: Vec<String> = ALLOWED_WORKFLOW_FILES
            .iter()
            .map(|(path, _)| (*path).to_owned())
            .collect();
        pinned.sort();
        if on_disk != pinned {
            return Err(format!(
                "workflow directory {on_disk:?} is not the pin table {pinned:?}"
            ));
        }
        let notes = ".github/workflows/notes.txt".to_owned();
        if pinned.iter().any(|path| path == &notes) {
            return Err("notes.txt is in the pin table".to_owned());
        }
        let mut with_notes = on_disk.clone();
        with_notes.push(notes);
        let note_gaps: Vec<String> = with_notes
            .iter()
            .filter(|path| workflow_file_pin(path).is_none())
            .map(|path| format!("{path} is not an allowed workflow file"))
            .collect();
        expect_gaps(
            "notes.txt",
            note_gaps,
            &[".github/workflows/notes.txt is not an allowed workflow file"],
        )?;
        let live = allowed_workflow_paths();
        expect_gaps(
            "live allowlist",
            allowlisted_yml_without_pin_gaps(&live, ALLOWED_WORKFLOW_FILES),
            &[],
        )?;
        if workflow_file_pin(".github/workflows/ci.yml") != Some(WorkflowFilePin::Ci) {
            return Err("ci.yml pin is not keyed by its path".to_owned());
        }
        if workflow_file_pin("nested/ci.yml").is_some()
            || workflow_file_pin(".github/workflows/not-vision-smoke.yml").is_some()
        {
            return Err("a workflow pin matched by suffix".to_owned());
        }
        let mut fourth = live.clone();
        fourth.push(".github/workflows/fourth.yml");
        expect_gaps(
            "fourth yml",
            allowlisted_yml_without_pin_gaps(&fourth, ALLOWED_WORKFLOW_FILES),
            &[".github/workflows/fourth.yml is allowlisted without a path-keyed pin"],
        )?;
        expect_gaps(
            "unpinned script",
            allowlisted_yml_without_pin_gaps(
                &[".github/workflows/extra.py"],
                ALLOWED_WORKFLOW_FILES,
            ),
            &[],
        )?;
        expect_gaps(
            "unpinned script pin",
            pinned_whole_file_gaps(".github/workflows/fourth.yml", b"name: fourth\n"),
            &[".github/workflows/fourth.yml is allowlisted without a path-keyed pin"],
        )?;
        let root = crate::workspace_root()?;
        let env = read(&root.join("fixtures/guards/ci/env-overrides.yml"))?;
        expect_gaps(
            "env-overrides.yml",
            forbidden_env_gaps("env-overrides.yml", &env),
            &[
                "env-overrides.yml env sets CARGO_WORKSPACE_DIR",
                "env-overrides.yml env sets GITHUB_ACTIONS",
                "env-overrides.yml env sets CI",
                "env-overrides.yml env sets PARANGSAE_FLOOR_BASE",
            ],
        )?;
        let github = read(&root.join("fixtures/guards/ci/github-env-file.yml"))?;
        let github_gaps = workflow_file_gaps("github-env-file.yml", &github);
        if !github_gaps
            .iter()
            .any(|gap| gap == "github-env-file.yml references GITHUB_ENV")
        {
            return Err(format!("github env layer: {github_gaps:?}"));
        }
        expect_gaps(
            "deny.toml",
            sha_file_gaps(&root, "deny.toml", DENY_TOML_SHA256),
            &[],
        )?;
        let planted = TempGit::new("deny-toml")?;
        let mut bytes = read_bytes(&root.join("deny.toml"))?;
        let first = bytes.first_mut().ok_or("deny.toml is empty")?;
        *first ^= 0x01;
        std::fs::write(planted.path.join("deny.toml"), &bytes)
            .map_err(|err| format!("write deny.toml: {err}"))?;
        expect_gaps(
            "mutated deny.toml",
            sha_file_gaps(&planted.path, "deny.toml", DENY_TOML_SHA256),
            &["deny.toml sha256 does not match"],
        )?;
        Ok(())
    }

    #[test]
    fn explicit_yaml_keys_are_rejected() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let fixture = read(&root.join("fixtures/guards/ci/explicit-key.yml"))?;
        expect_gaps(
            "explicit-key.yml",
            strict_yaml_gaps("explicit-key.yml", &fixture),
            &["explicit-key.yml uses an explicit yaml key"],
        )?;
        let header = "\
name: ci
on:
  push:
? owner: ops
permissions:
  contents: read
jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - run: echo fmt
";
        expect_gaps(
            "explicit header",
            strict_yaml_gaps("header.yml", header),
            &["header.yml uses an explicit yaml key"],
        )?;
        let job = "\
name: ci
on:
  push:
permissions:
  contents: read
jobs:
  fmt:
    ? runs-on
    : ubuntu-latest
    steps:
      - run: echo fmt
";
        expect_gaps(
            "explicit job",
            strict_yaml_gaps("job.yml", job),
            &["job.yml uses an explicit yaml key"],
        )?;
        Ok(())
    }

    #[test]
    fn libtest_one_pass_rejects_other_counts() -> Result<(), String> {
        let one = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s";
        if !crate::libtest_summary_is_one_pass(one) {
            return Err(format!("one pass was rejected: {one}"));
        }
        for line in [
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 60 filtered out",
            "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            "test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            "test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out",
        ] {
            if crate::libtest_summary_is_one_pass(line) {
                return Err(format!("accepted {line}"));
            }
        }
        Ok(())
    }

    #[test]
    fn filtered_cargo_tests_assert_the_passed_count() -> Result<(), String> {
        for (name, pinned, _) in REQUIRED_JOBS {
            expect_gaps(name, filtered_cargo_test_gaps(name, pinned), &[])?;
        }
        let root = crate::workspace_root()?;
        let fixture = read(&root.join("fixtures/guards/ci/filtered-cargo-test.yml"))?;
        expect_gaps(
            "filtered-cargo-test.yml",
            filtered_cargo_test_gaps("filtered-cargo-test.yml", &fixture),
            &["filtered-cargo-test.yml filtered cargo test does not assert the passed count"],
        )?;
        let zero = read(&root.join("fixtures/guards/ci/filtered-zero-passed.yml"))?;
        expect_gaps(
            "filtered-zero-passed.yml",
            filtered_cargo_test_gaps("filtered-zero-passed.yml", &zero),
            &["filtered-zero-passed.yml filtered cargo test does not assert the passed count"],
        )?;
        let ten = "\
  fmt:
    steps:
      - run: |
          cargo test -p xtask --lib some::test -- --exact
          grep -q 'test result: ok. 10 passed'
";
        expect_gaps("ten passed", filtered_cargo_test_gaps("fmt", ten), &[])?;
        let run_exact = "\
  fmt:
    steps:
      - run: cargo xtask run-exact guards::tests::missing_or_unreadable_baseline_fails_closed
";
        expect_gaps("run-exact", filtered_cargo_test_gaps("fmt", run_exact), &[])?;
        let asserted = "\
  fmt:
    steps:
      - name: one test
        run: |
          output=$(cargo test -p xtask --lib guards::tests::missing_or_unreadable_baseline_fails_closed -- --exact)
          printf '%s\\n' \"$output\" | grep -q 'test result: ok. 1 passed'
";
        expect_gaps("asserted", filtered_cargo_test_gaps("fmt", asserted), &[])?;
        let workspace = "\
  test:
    steps:
      - run: cargo test --workspace
      - run: cargo test -p parangsae-core --features test-support --lib
      - run: cargo test --manifest-path fixtures/causal/closed-gate/Cargo.toml --locked
      - run: cargo test -p parangsae-core --test kline_integrity
";
        expect_gaps(
            "unfiltered",
            filtered_cargo_test_gaps("test", workspace),
            &[],
        )?;
        Ok(())
    }

    #[test]
    fn the_landing_push_keeps_bootstrap_and_a_later_branch_does_not() -> Result<(), String> {
        let source = crate::workspace_root()?;
        let landing = shared_workspace("landing-push")?;
        overlay_workspace_checks(&source, &landing.path)?;
        point_origin_at_head(&landing.path)?;
        let reports = guard_reports(&landing.path, Some(BOOTSTRAP_BASE), true);
        assert_non_ratchet_ran(&reports)?;
        if reports.iter().any(|report| !report.passed) {
            return Err(reports
                .iter()
                .filter(|report| !report.passed)
                .map(|report| format!("{}: {}", report.name, report.detail))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        let later = shared_workspace("later-branch")?;
        overlay_workspace_checks(&source, &later.path)?;
        let remote = later.path.join("remote-main");
        std::fs::create_dir_all(&remote).map_err(|err| format!("mkdir: {err}"))?;
        git_fixture(&remote, &["init", "-b", "main"])?;
        std::fs::create_dir_all(remote.join("regressions"))
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::copy(
            source.join("regressions/floors.toml"),
            remote.join("regressions/floors.toml"),
        )
        .map_err(|err| format!("copy floors: {err}"))?;
        git_fixture(&remote, &["add", "regressions/floors.toml"])?;
        git_fixture(&remote, &["commit", "-m", "floors"])?;
        let remote_url = format!(
            "file://{}",
            remote.to_str().ok_or("remote path is not utf-8")?
        );
        git_fixture(&later.path, &["remote", "set-url", "origin", &remote_url])?;
        clear_origin_main(&later.path)?;
        let reports = guard_reports(&later.path, Some(BOOTSTRAP_BASE), false);
        assert_non_ratchet_ran(&reports)?;
        let ratchet = report_named(&reports, "ratchet")?;
        if ratchet.passed || !ratchet.detail.contains("origin/main contains") {
            return Err(format!("later branch: {}", ratchet.detail));
        }
        Ok(())
    }

    struct SharedWorkspace {
        _keep: TempGit,
        path: PathBuf,
    }

    fn clear_origin_main(repo: &Path) -> Result<(), String> {
        let present = git_command(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "refs/remotes/origin/main",
            ],
        )?;
        if present.status.success() {
            git_fixture(repo, &["update-ref", "-d", "refs/remotes/origin/main"])?;
        }
        Ok(())
    }

    fn point_origin_at_main_without_floors(repo: &Path) -> Result<(), String> {
        let parent = repo.parent().ok_or("clone has no parent")?;
        let remote = parent.join("empty-main");
        std::fs::create_dir_all(&remote).map_err(|err| format!("mkdir: {err}"))?;
        git_fixture(&remote, &["init", "-b", "main"])?;
        std::fs::write(remote.join("keep"), "keep\n").map_err(|err| format!("write: {err}"))?;
        git_fixture(&remote, &["add", "keep"])?;
        git_fixture(&remote, &["commit", "-m", "main"])?;
        let url = format!(
            "file://{}",
            remote.to_str().ok_or("remote path is not utf-8")?
        );
        git_fixture(repo, &["remote", "set-url", "origin", &url])?;
        clear_origin_main(repo)?;
        Ok(())
    }

    fn point_origin_at_head(repo: &Path) -> Result<(), String> {
        let parent = repo.parent().ok_or("clone has no parent")?;
        let remote = parent.join("head-main.git");
        git_fixture(
            parent,
            &["init", "--bare", "--quiet", "-b", "main", "head-main.git"],
        )?;
        let url = format!(
            "file://{}",
            remote.to_str().ok_or("remote path is not utf-8")?
        );
        git_fixture(repo, &["remote", "set-url", "origin", &url])?;
        clear_origin_main(repo)?;
        git_fixture(repo, &["push", "--quiet", "origin", "HEAD:refs/heads/main"])?;
        Ok(())
    }

    fn overlay_workspace_checks(source: &Path, dest: &Path) -> Result<(), String> {
        let from = source.join(".github/workflows");
        let to = dest.join(".github/workflows");
        std::fs::create_dir_all(&to).map_err(|err| format!("mkdir: {err}"))?;
        for entry in std::fs::read_dir(&from).map_err(|err| format!("read workflows: {err}"))? {
            let entry = entry.map_err(|err| format!("read workflows: {err}"))?;
            if !entry
                .file_type()
                .map_err(|err| format!("stat workflow: {err}"))?
                .is_file()
            {
                continue;
            }
            std::fs::copy(entry.path(), to.join(entry.file_name()))
                .map_err(|err| format!("copy workflow: {err}"))?;
        }
        std::fs::create_dir_all(dest.join("regressions")).map_err(|err| format!("mkdir: {err}"))?;
        std::fs::copy(
            source.join("regressions/floors.toml"),
            dest.join("regressions/floors.toml"),
        )
        .map_err(|err| format!("copy floors: {err}"))?;
        Ok(())
    }

    fn shared_workspace(label: &str) -> Result<SharedWorkspace, String> {
        let source = crate::workspace_root()?;
        let temp = TempGit::new(label)?;
        let source_text = source.to_str().ok_or("workspace path is not utf-8")?;
        git_fixture(
            &temp.path,
            &["clone", "--shared", "--quiet", source_text, "repo"],
        )?;
        let path = temp.path.join("repo");
        Ok(SharedWorkspace { _keep: temp, path })
    }

    #[test]
    fn git_failure_detail_is_never_blank() -> Result<(), String> {
        if command_failure_detail("", "", Some(128)) != "exit 128" {
            return Err("blank git failure".to_owned());
        }
        if command_failure_detail("fatal: Not a valid object name origin/main", "", Some(128))
            != "fatal: Not a valid object name origin/main"
        {
            return Err("stderr dropped".to_owned());
        }
        if command_failure_detail("", "stdout only", Some(1)) != "stdout only" {
            return Err("stdout dropped".to_owned());
        }
        Ok(())
    }

    #[test]
    fn branch_only_clone_fetches_origin_main_before_merge_base() -> Result<(), String> {
        let root = TempGit::new("branch-only")?;
        let origin = root.path.join("origin");
        std::fs::create_dir_all(&origin).map_err(|err| format!("mkdir: {err}"))?;
        git_fixture(&origin, &["init", "-b", "main"])?;
        std::fs::write(origin.join("file"), "main\n").map_err(|err| format!("write: {err}"))?;
        git_fixture(&origin, &["add", "file"])?;
        git_fixture(&origin, &["commit", "-m", "main"])?;
        let main_sha = git_stdout(&origin, &["rev-parse", "HEAD"])?;
        git_fixture(&origin, &["checkout", "-b", "feature"])?;
        std::fs::write(origin.join("file"), "feature\n").map_err(|err| format!("write: {err}"))?;
        git_fixture(&origin, &["add", "file"])?;
        git_fixture(&origin, &["commit", "-m", "feature"])?;
        let origin_url = format!(
            "file://{}",
            origin.to_str().ok_or("origin path is not utf-8")?
        );
        git_fixture(
            &root.path,
            &[
                "clone",
                "--depth",
                "1",
                "--single-branch",
                "--branch",
                "feature",
                &origin_url,
                "clone",
            ],
        )?;
        let clone = root.path.join("clone");
        let shallow = git_stdout(&clone, &["rev-parse", "--is-shallow-repository"])?;
        if shallow != "true" {
            return Err(format!("branch clone was not depth 1: {shallow}"));
        }
        let missing = Command::new("git")
            .arg("-C")
            .arg(&clone)
            .args(["rev-parse", "--verify", "refs/remotes/origin/main"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|err| format!("git: {err}"))?;
        if missing.status.success() {
            return Err("branch-only clone already had origin/main".to_owned());
        }
        let base = merge_base_origin_main(&clone)?;
        if base != main_sha {
            return Err(format!("merge-base {base} != main {main_sha}"));
        }
        Ok(())
    }

    #[test]
    fn merge_base_without_main_fails_closed() -> Result<(), String> {
        let root = TempGit::new("no-main")?;
        let origin = root.path.join("origin");
        std::fs::create_dir_all(&origin).map_err(|err| format!("mkdir: {err}"))?;
        git_fixture(&origin, &["init", "-b", "feature"])?;
        std::fs::write(origin.join("file"), "feature\n").map_err(|err| format!("write: {err}"))?;
        git_fixture(&origin, &["add", "file"])?;
        git_fixture(&origin, &["commit", "-m", "feature"])?;
        git_fixture(
            &root.path,
            &[
                "clone",
                "--single-branch",
                "--branch",
                "feature",
                origin.to_str().ok_or("origin path is not utf-8")?,
                "clone",
            ],
        )?;
        match merge_base_origin_main(&root.path.join("clone")) {
            Ok(sha) => Err(format!("missing main returned a baseline {sha}")),
            Err(err) if err.contains("failed") && !err.is_empty() => Ok(()),
            Err(err) => Err(format!("missing main: {err}")),
        }
    }

    #[test]
    fn tlc_count_below_the_floor_fails_and_a_missing_success_line_fails() -> Result<(), String> {
        let output = "\
Model checking completed. No error has been found.
115 states generated, 21 distinct states found, 0 states left on queue.
";
        if distinct_states(output)? != 21 {
            return Err("distinct state count".to_owned());
        }
        if distinct_states(output)? >= 22 {
            return Err("floor comparison".to_owned());
        }
        if distinct_states(
            "115 states generated, 21 distinct states found, 0 states left on queue.\n",
        )
        .is_ok()
        {
            return Err("missing success line was accepted".to_owned());
        }
        Ok(())
    }

    #[test]
    fn kani_summary_with_a_failure_or_a_missing_name_fails() -> Result<(), String> {
        let floors = parse_floors(
            "\
kani.min_harnesses=1
kani.name=notional_rejects_negative
",
        )?;
        let failed = "\
Checking harness notional_rejects_negative...
Verification failed for - notional_rejects_negative
Complete - 0 successfully verified harnesses, 1 failures, 1 total.
";
        let violations = kani_violations(failed, &floors)?;
        if !violations.iter().any(|line| line.contains("failures")) {
            return Err(format!("{violations:?}"));
        }
        let missing = "\
Checking harness widening_mul_fits_in_i128...
Complete - 1 successfully verified harnesses, 0 failures, 1 total.
";
        let violations = kani_violations(missing, &floors)?;
        if !violations
            .iter()
            .any(|line| line.contains("missing kani harness notional_rejects_negative"))
        {
            return Err(format!("{violations:?}"));
        }
        let qualified = "\
Checking harness fixed::proofs::notional_rejects_negative...
Complete - 1 successfully verified harnesses, 0 failures, 1 total.
";
        let violations = kani_violations(qualified, &floors)?;
        if !violations.is_empty() {
            return Err(format!("{violations:?}"));
        }
        Ok(())
    }

    #[test]
    fn unowned_feature_is_rejected() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let scan =
            std::env::temp_dir().join(format!("parangsae-guards-unowned-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scan);
        let dir = scan.join("specs/features/orphan");
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        std::fs::write(dir.join("orphan.feature"), "Feature: Orphan\n")
            .map_err(|err| err.to_string())?;
        let violations = feature_ownership(&root, &scan, false)?;
        let _ = std::fs::remove_dir_all(&scan);
        if !violations
            .iter()
            .any(|line| line.contains("unowned feature") && line.contains("orphan.feature"))
        {
            return Err(format!("{violations:?}"));
        }
        Ok(())
    }

    #[test]
    fn a_malformed_feature_on_an_owned_path_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let scan =
            std::env::temp_dir().join(format!("parangsae-guards-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scan);
        let dir = scan.join("specs/features");
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        std::fs::write(
            dir.join("kline_integrity.feature"),
            "this is not a feature\n",
        )
        .map_err(|err| err.to_string())?;
        let violations = feature_ownership(&root, &scan, false)?;
        let _ = std::fs::remove_dir_all(&scan);
        if !violations.iter().any(|line| {
            line.contains("malformed feature") && line.contains("kline_integrity.feature")
        }) {
            return Err(format!("{violations:?}"));
        }
        Ok(())
    }

    #[test]
    fn workspace_features_are_owned_and_tla_cfgs_list_invariants() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let violations = feature_ownership(&root, &root, true)?;
        if !violations.is_empty() {
            return Err(violations.join("\n"));
        }
        let violations = tla_models(&root)?;
        if !violations.is_empty() {
            return Err(violations.join("\n"));
        }
        let floors = load_floors(&root.join("regressions/floors.toml"))?;
        let missing = required_floor_keys(&floors);
        if !missing.is_empty() {
            return Err(missing.join("\n"));
        }
        let measured = b2_violations(&root, &floors)?;
        if !measured.is_empty() {
            return Err(measured.join("\n"));
        }
        let counts = cucumber_count_violations(&root, &floors)?;
        if !counts.is_empty() {
            return Err(counts.join("\n"));
        }
        let gaps = ci_gate_gaps(&read(&root.join(".github/workflows/ci.yml"))?)?;
        if !gaps.is_empty() {
            return Err(gaps.join("\n"));
        }
        let pin = gate_results_pin(&read_bytes(
            &root.join(".github/workflows/gate_results.py"),
        )?);
        if !pin.is_empty() {
            return Err(pin.join("\n"));
        }
        for (name, _) in GATE_FIXTURE_SHA256 {
            let pin = gate_fixture_pin(name, &read_bytes(&root.join(name))?);
            if !pin.is_empty() {
                return Err(pin.join("\n"));
            }
        }
        Ok(())
    }

    #[test]
    fn a_job_omitted_from_gate_needs_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let omitted = ci_gate_gaps(&read(&root.join("fixtures/guards/ci/omitted-job.yml"))?)?;
        if omitted != ["ci job clippy is missing from gate.needs".to_owned()] {
            return Err(format!("{omitted:?}"));
        }
        Ok(())
    }

    #[test]
    fn a_trailing_comment_on_jobs_does_not_hide_the_inventory() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let text = read(&root.join("fixtures/guards/ci/jobs-comment.yml"))?;
        let jobs = ci_job_ids(&text)?;
        let expected = BTreeSet::from(["clippy".to_owned(), "fmt".to_owned(), "gate".to_owned()]);
        if jobs != expected {
            return Err(format!("job inventory {jobs:?}"));
        }
        let gaps = ci_gate_gaps(&text)?;
        if gaps != ["ci job clippy is missing from gate.needs".to_owned()] {
            return Err(format!("{gaps:?}"));
        }
        Ok(())
    }

    #[test]
    fn an_empty_job_set_fails_closed() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let text = read(&root.join("fixtures/guards/ci/jobs-empty.yml"))?;
        match ci_gate_gaps(&text) {
            Ok(gaps) => Err(format!("empty jobs passed: {gaps:?}")),
            Err(err) if err.contains("no jobs") => Ok(()),
            Err(err) => Err(format!("empty jobs: {err}")),
        }
    }

    #[test]
    fn gate_without_if_always_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let gaps = ci_gate_gaps(&read(&root.join("fixtures/guards/ci/no-always.yml"))?)?;
        if gaps != ["gate job is missing if: always()".to_owned()] {
            return Err(format!("{gaps:?}"));
        }
        Ok(())
    }

    #[test]
    fn an_exit_zero_gate_check_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let gaps = ci_gate_gaps(&read(&root.join("fixtures/guards/ci/exit-zero.yml"))?)?;
        if gaps != ["gate check step does not match".to_owned()] {
            return Err(format!("{gaps:?}"));
        }
        Ok(())
    }

    #[test]
    fn an_extra_gate_need_is_not_the_inventory() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let gaps = ci_gate_gaps(&read(&root.join("fixtures/guards/ci/extra-need.yml"))?)?;
        if gaps != ["gate.needs lists phantom which is not a ci job".to_owned()] {
            return Err(format!("{gaps:?}"));
        }
        Ok(())
    }

    #[test]
    fn a_weakened_gate_fixture_step_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let gaps = ci_gate_gaps(&read(
            &root.join("fixtures/guards/ci/fixture-step-weakened.yml"),
        )?)?;
        if gaps != ["gate fixture step does not match".to_owned()] {
            return Err(format!("{gaps:?}"));
        }
        Ok(())
    }

    #[test]
    fn gate_continue_on_error_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let job = ci_gate_gaps(&read(
            &root.join("fixtures/guards/ci/gate-continue-on-error.yml"),
        )?)?;
        if job != ["gate job sets continue-on-error".to_owned()] {
            return Err(format!("job: {job:?}"));
        }
        let step = ci_gate_gaps(&read(
            &root.join("fixtures/guards/ci/gate-step-continue-on-error.yml"),
        )?)?;
        if step != ["gate step sets continue-on-error".to_owned()] {
            return Err(format!("step: {step:?}"));
        }
        let needed = ci_gate_gaps(&read(
            &root.join("fixtures/guards/ci/needed-continue-on-error.yml"),
        )?)?;
        if needed != ["ci job fmt sets continue-on-error".to_owned()] {
            return Err(format!("needed: {needed:?}"));
        }
        Ok(())
    }

    #[test]
    fn a_step_level_if_inside_the_gate_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let gaps = ci_gate_gaps(&read(&root.join("fixtures/guards/ci/gate-step-if.yml"))?)?;
        if gaps != ["gate step sets if:".to_owned()] {
            return Err(format!("{gaps:?}"));
        }
        let forced = read(&root.join("fixtures/guards/ci/gate-step-if.yml"))?
            .replace("if: success()", "if: false");
        let forced_gaps = ci_gate_gaps(&forced)?;
        if forced_gaps != ["gate step sets if:".to_owned()] {
            return Err(format!("if false: {forced_gaps:?}"));
        }
        let needed = ci_gate_gaps(&read(&root.join("fixtures/guards/ci/needed-step-if.yml"))?)?;
        if needed != ["ci job fmt sets step if:".to_owned()] {
            return Err(format!("needed step if: {needed:?}"));
        }
        let job = ci_gate_gaps(&read(&root.join("fixtures/guards/ci/job-if.yml"))?)?;
        if job != ["ci job fmt sets if:".to_owned()] {
            return Err(format!("job if: {job:?}"));
        }
        Ok(())
    }

    #[test]
    fn self_hosted_runs_on_is_the_only_runner_flow() -> Result<(), String> {
        let root = crate::workspace_root()?;
        for rel in [
            ".github/workflows/ci.yml",
            ".github/workflows/vision-smoke.yml",
        ] {
            let gaps = strict_yaml_gaps(rel, &read(&root.join(rel))?);
            if gaps.iter().any(|gap| gap.contains("flow")) {
                return Err(format!("{rel}: {gaps:?}"));
            }
        }
        let ubuntu = strict_yaml_gaps(
            "flow-runs-on.yml",
            &read(&root.join("fixtures/guards/ci/flow-runs-on.yml"))?,
        );
        if ubuntu != ["flow-runs-on.yml uses a flow sequence".to_owned()] {
            return Err(format!("ubuntu flow: {ubuntu:?}"));
        }
        Ok(())
    }

    #[test]
    fn path_filter_if_is_only_allowed_on_tla_and_kani() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        let gaps = ci_gate_gaps(&ci)?;
        if !gaps.is_empty() {
            return Err(format!("ci gate: {gaps:?}"));
        }
        let tla_false = ci.replace("if: needs.changes.outputs.tla == 'true'", "if: false");
        let tla_gaps = ci_gate_gaps(&tla_false)?;
        if !tla_gaps.iter().any(|gap| gap == "ci job tla sets if:") {
            return Err(format!("tla if false: {tla_gaps:?}"));
        }
        let kani_wrong = ci.replace(
            "if: needs.changes.outputs.kani == 'true'",
            "if: needs.changes.outputs.tla == 'true'",
        );
        let kani_gaps = ci_gate_gaps(&kani_wrong)?;
        if !kani_gaps.iter().any(|gap| gap == "ci job kani sets if:") {
            return Err(format!("kani wrong if: {kani_gaps:?}"));
        }
        Ok(())
    }

    #[test]
    fn renaming_a_required_job_fails_the_ratchet() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let base = load_floors(&root.join("regressions/floors.toml"))?;
        let renamed = load_floors(&root.join("fixtures/guards/floors-renamed-job.toml"))?;
        let violations = ratchet(&renamed, &base);
        if violations != ["ci job fmt deleted".to_owned()] {
            return Err(format!("{violations:?}"));
        }
        let inventory =
            ci_job_inventory_gaps(&read(&root.join(".github/workflows/ci.yml"))?, &renamed);
        if !inventory
            .iter()
            .any(|gap| gap.contains("fmt") && gap.contains("missing"))
        {
            return Err(format!("inventory: {inventory:?}"));
        }
        if !inventory
            .iter()
            .any(|gap| gap.contains("format") && gap.contains("not required"))
        {
            return Err(format!("added name: {inventory:?}"));
        }
        Ok(())
    }

    #[test]
    fn a_guard_that_did_not_run_fails_the_count() -> Result<(), String> {
        let mut reports = Vec::new();
        for name in NON_RATCHET_GUARDS {
            reports.push(GuardReport {
                name,
                ran: true,
                passed: true,
                detail: String::new(),
            });
        }
        reports.push(GuardReport {
            name: "ratchet",
            ran: false,
            passed: false,
            detail: String::new(),
        });
        let gaps = guard_run_gaps(&reports);
        if !gaps.iter().any(|gap| gap.contains("below 12")) {
            return Err(format!("{gaps:?}"));
        }
        let duplicated = inventory_gaps(&["feature-ownership", "feature-ownership", "ratchet"]);
        if duplicated.len() != 2 {
            return Err(format!("{duplicated:?}"));
        }
        if !guard_inventory_gaps().is_empty() {
            return Err(format!("{:?}", guard_inventory_gaps()));
        }
        Ok(())
    }

    #[test]
    fn a_replaced_gate_script_fails_the_sha256_pin() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let pinned = gate_results_pin(&read_bytes(
            &root.join(".github/workflows/gate_results.py"),
        )?);
        if !pinned.is_empty() {
            return Err(format!("real script: {pinned:?}"));
        }
        let replaced = gate_results_pin(&read_bytes(
            &root.join("fixtures/guards/ci/gate_results_exit_zero.py"),
        )?);
        if replaced != ["gate_results.py sha256 does not match".to_owned()] {
            return Err(format!("{replaced:?}"));
        }
        Ok(())
    }

    #[test]
    fn non_object_needs_are_rejected() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let cases = [
            (
                "fixtures/guards/ci/needs-non-object.json",
                "gate needs must be an object",
            ),
            (
                "fixtures/guards/ci/needs-entry-non-object.json",
                "not-object",
            ),
        ];
        for (name, needle) in cases {
            let needs = read(&root.join(name))?;
            let output = Command::new("python3")
                .arg(root.join(".github/workflows/gate_results.py"))
                .env("NEEDS", &needs)
                .output()
                .map_err(|err| format!("python3: {err}"))?;
            let detail = command_failure_detail(
                &String::from_utf8_lossy(&output.stderr),
                &String::from_utf8_lossy(&output.stdout),
                output.status.code(),
            );
            if output.status.success() || !detail.contains(needle) {
                return Err(format!("{name}: {detail}"));
            }
        }
        Ok(())
    }

    #[test]
    fn a_weakened_gate_fixture_file_fails_the_sha256_pin() -> Result<(), String> {
        let root = crate::workspace_root()?;
        for (name, _) in GATE_FIXTURE_SHA256 {
            let pin = gate_fixture_pin(name, &read_bytes(&root.join(name))?);
            if !pin.is_empty() {
                return Err(format!("{name}: {pin:?}"));
            }
        }
        let weakened = gate_fixture_pin(
            "fixtures/guards/ci/needs-success.json",
            &read_bytes(&root.join("fixtures/guards/ci/needs-success-weakened.json"))?,
        );
        if weakened
            != [
                "gate fixture fixtures/guards/ci/needs-success.json sha256 does not match"
                    .to_owned(),
            ]
        {
            return Err(format!("{weakened:?}"));
        }
        Ok(())
    }

    #[test]
    fn a_bom_or_crlf_change_fails_the_sha256_pin() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let original = std::fs::read(root.join("fixtures/guards/ci/needs-success.json"))
            .map_err(|err| format!("read original: {err}"))?;
        let bom = std::fs::read(root.join("fixtures/guards/ci/needs-success-bom.json"))
            .map_err(|err| format!("read bom: {err}"))?;
        let crlf = std::fs::read(root.join("fixtures/guards/ci/needs-success-crlf.json"))
            .map_err(|err| format!("read crlf: {err}"))?;
        let mut expected_bom = vec![0xEF, 0xBB, 0xBF];
        expected_bom.extend_from_slice(&original);
        if bom != expected_bom {
            return Err("bom fixture is not the pinned bytes with a utf-8 bom".to_owned());
        }
        let mut expected_crlf = Vec::with_capacity(original.len());
        for byte in &original {
            if *byte == b'\n' {
                expected_crlf.push(b'\r');
            }
            expected_crlf.push(*byte);
        }
        if crlf != expected_crlf {
            return Err("crlf fixture is not the pinned bytes with crlf".to_owned());
        }
        for (label, bytes) in [("bom", bom.as_slice()), ("crlf", crlf.as_slice())] {
            let pin = gate_fixture_pin("fixtures/guards/ci/needs-success.json", bytes);
            if pin
                != [
                    "gate fixture fixtures/guards/ci/needs-success.json sha256 does not match"
                        .to_owned(),
                ]
            {
                return Err(format!("{label}: {pin:?}"));
            }
        }
        let pinned = gate_fixture_pin("fixtures/guards/ci/needs-success.json", &original);
        if !pinned.is_empty() {
            return Err(format!("original bytes: {pinned:?}"));
        }
        Ok(())
    }

    #[test]
    fn tracked_nested_target_toolchain_files_fail_and_the_old_skip_missed_them(
    ) -> Result<(), String> {
        let repo = TempGit::new("tracked-nested-target")?;
        git_fixture(&repo.path, &["init", "-b", "main"])?;
        let toolchain = repo.path.join("crates/core/target/rust-toolchain.toml");
        let config = repo.path.join("crates/core/target/.cargo/config");
        std::fs::create_dir_all(config.parent().ok_or("config has no parent")?)
            .map_err(|err| format!("mkdir: {err}"))?;
        std::fs::write(&toolchain, "[toolchain]\nchannel = \"nightly\"\n")
            .map_err(|err| format!("write: {err}"))?;
        std::fs::write(&config, "paths = []\n").map_err(|err| format!("write: {err}"))?;
        git_fixture(
            &repo.path,
            &["add", "-f", "crates/core/target/rust-toolchain.toml"],
        )?;
        git_fixture(
            &repo.path,
            &["add", "-f", "crates/core/target/.cargo/config"],
        )?;
        git_fixture(&repo.path, &["commit", "-m", "track nested target"])?;
        let listed = git_stdout(&repo.path, &["ls-files"])?;
        if !listed.contains("crates/core/target/rust-toolchain.toml")
            || !listed.contains("crates/core/target/.cargo/config")
        {
            return Err(format!("nested target files are not tracked: {listed}"));
        }
        expect_gaps(
            "production walk",
            toolchain_file_gaps(&repo.path)?,
            &[
                "crates/core/target/.cargo/config is a nested cargo config; only the root .cargo/config.toml is allowed",
                "crates/core/target/rust-toolchain.toml is a toolchain override; only the root rust-toolchain.toml is allowed",
            ],
        )?;
        expect_gaps(
            "bbe4674 skip",
            toolchain_gaps_skipping_every_target(&repo.path)?,
            &[],
        )?;
        Ok(())
    }

    #[test]
    fn deny_rehash_is_checked_before_every_exec() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("live deny rehash", deny_release_pin_gaps(&ci), &[])?;
        let config_late = zero_nth_hash(&ci, CARGO_CONFIG_SHA256, 1)?;
        expect_gaps(
            "config hash after the first exec",
            deny_release_pin_gaps(&config_late),
            &["deny job does not re-hash .cargo/config.toml before execution"],
        )?;
        let toml_late = zero_nth_hash(&ci, DENY_TOML_SHA256, 1)?;
        expect_gaps(
            "deny.toml hash after the first exec",
            deny_release_pin_gaps(&toml_late),
            &["deny job does not re-hash deny.toml before execution"],
        )?;
        let toml_gone = ci.replace(DENY_TOML_SHA256, &"0".repeat(DENY_TOML_SHA256.len()));
        expect_gaps(
            "deny.toml hash absent",
            deny_release_pin_gaps(&toml_gone),
            &["deny job does not re-hash deny.toml"],
        )?;
        Ok(())
    }

    #[test]
    fn deny_env_i_rejects_unpinned_tool_arguments() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        let deny = job_block(&ci, "deny").ok_or("ci.yml has no deny job")?;
        expect_gaps("live env -i", deny_cargo_env_gaps(deny), &[])?;
        let fixture = read(&root.join("fixtures/guards/ci/deny-env-i-cargo.yml"))?;
        let block = job_block(&fixture, "deny").ok_or("fixture has no deny job")?;
        expect_gaps(
            "env -i cargo",
            deny_cargo_env_gaps(block),
            &[
                "deny job sets CARGO=/tmp/fake in env -i",
                "deny job sets RUSTC=/tmp/rustc in env -i",
                "deny job sets RUSTFLAGS=-Awarnings in env -i",
            ],
        )?;
        Ok(())
    }

    const E13: &str = concat!(
        "          env -i \\\n",
        "            HOME=/home/runner \\\n",
        "            PATH=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \\\n",
        "            CARGO_HOME=/home/runner/.cargo \\\n",
        "            RUSTUP_HOME=/home/runner/.rustup \\\n",
        "            CARGO=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo \\\n",
        "            RUSTC=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc \\\n",
        "            /home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo xtask guards toolchain-files",
    );

    fn e13_uses(root: &std::path::Path) -> Result<Vec<(String, usize)>, String> {
        let mut found = Vec::new();
        for dir in [
            ".github/workflows",
            "fixtures/guards/ci",
            "xtask/src/pinned_jobs",
        ] {
            let mut paths = Vec::new();
            for entry in
                std::fs::read_dir(root.join(dir)).map_err(|err| format!("read {dir}: {err}"))?
            {
                let entry = entry.map_err(|err| format!("read {dir}: {err}"))?;
                paths.push(entry.path());
            }
            paths.sort();
            for path in paths {
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.ends_with(".yml") && !name.ends_with(".job") {
                    continue;
                }
                let count = read(&path)?.matches(E13).count();
                if count > 0 {
                    found.push((format!("{dir}/{name}"), count));
                }
            }
        }
        found.sort();
        Ok(found)
    }

    fn plant_e13(command: &str, planted: &str) -> Result<String, String> {
        if command.matches(E13).count() != 2 {
            return Err(format!(
                "deny job has {} E13 walks",
                command.matches(E13).count()
            ));
        }
        Ok(command.replacen(E13, planted, 1))
    }

    #[test]
    fn toolchain_walk_env_i_rejects_unpinned_tool_literals() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        let deny = job_block(&ci, "deny").ok_or("ci.yml has no deny job")?;
        let uses = e13_uses(&root)?;
        let expected = [
            (".github/workflows/ci.yml", 2usize),
            ("fixtures/guards/ci/deny-extensionless-config.yml", 2),
            ("fixtures/guards/ci/deny-job-cargo-env.yml", 2),
            ("fixtures/guards/ci/deny-no-bin-sha.yml", 2),
            ("fixtures/guards/ci/deny-path-shim.yml", 2),
            ("fixtures/guards/ci/deny-step-cargo-env.yml", 2),
            ("fixtures/guards/ci/deny-swapped-bin.yml", 2),
            ("xtask/src/pinned_jobs/deny.job", 2),
        ];
        if uses.len() != expected.len()
            || uses.iter().zip(expected.iter()).any(
                |((path, count), (expected_path, expected_count))| {
                    path != expected_path || count != expected_count
                },
            )
        {
            return Err(format!("E13 uses: {uses:?}"));
        }
        expect_gaps("live E13", deny_env_i_literal_gaps(deny), &[])?;
        let home = deny.replacen("CARGO_HOME=/home/runner/.cargo", "CARGO_HOME=/tmp", 1);
        expect_gaps(
            "CARGO_HOME is not CARGO",
            deny_env_i_literal_gaps(&home),
            &[],
        )?;
        let cargo =
            "CARGO=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo";
        let rustc =
            "RUSTC=/home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc";
        let assigned = E13
            .replace(cargo, "CARGO=/tmp/fake")
            .replace(rustc, "RUSTC=/tmp/rustc")
            .replace(
                "RUSTC=/tmp/rustc \\\n",
                "RUSTC=/tmp/rustc \\\n            RUSTFLAGS=-Awarnings \\\n",
            );
        if !assigned.contains("RUSTFLAGS=-Awarnings") {
            return Err("E13 assignment plant missed RUSTFLAGS".to_owned());
        }
        let literals = [
            "deny job sets CARGO=/tmp/fake in env -i",
            "deny job sets RUSTC=/tmp/rustc in env -i",
            "deny job sets RUSTFLAGS=-Awarnings in env -i",
        ];
        expect_gaps(
            "E13 assignment literals",
            deny_env_i_literal_gaps(&plant_e13(deny, &assigned)?),
            &literals,
        )?;
        let last = "            /home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo xtask guards toolchain-files";
        let inline = E13.replacen(
            last,
            "            CARGO=/tmp/fake RUSTC=/tmp/rustc RUSTFLAGS=-Awarnings /home/runner/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo xtask guards toolchain-files",
            1,
        );
        if inline == E13 {
            return Err("E13 command line did not take literal args".to_owned());
        }
        expect_gaps(
            "E13 inline literals",
            deny_env_i_literal_gaps(&plant_e13(deny, &inline)?),
            &literals,
        )?;
        Ok(())
    }

    fn byte_check_between(text: &str, marker: &str, end: &str) -> Result<String, String> {
        let start = text
            .find(marker)
            .ok_or_else(|| format!("byte check is missing {marker}"))?;
        let rest = &text[start..];
        let end_at = rest
            .find(end)
            .ok_or_else(|| format!("byte check has no end {end}"))?;
        Ok(rest[..end_at + 1].to_owned())
    }

    fn assert_file_grep(label: &str, check: &str, rc_name: &str) -> Result<(), String> {
        if check.contains("<<<")
            || check.contains("case ")
            || check.contains("esac")
            || check.contains('|')
        {
            return Err(format!("{label} is not a file grep: {check}"));
        }
        let error_branch = format!("if [ \"${rc_name}\" -gt 1 ]; then");
        if !check.contains("grep -qF -- 'ci job fmt bytes do not match' \"$f\"")
            || !check.contains("write_rc=$?")
            || !check.contains("if [ \"$write_rc\" -ne 0 ]; then")
            || !check.contains(&error_branch)
        {
            return Err(format!("{label} does not classify grep rc: {check}"));
        }
        Ok(())
    }

    fn guard_byte_script(
        check: &str,
        copy: &Path,
        data: &Path,
        path_prefix: &str,
    ) -> Result<String, String> {
        Ok(format!(
            "{path_prefix}set -euo pipefail\nclean=$(cat {data})\noutput=\"$clean\"\ncopy={copy}\n{check}echo reached-end\n",
            data = shell_quote(data)?,
            copy = shell_quote(copy)?,
        ))
    }

    fn require_outcome(
        label: &str,
        code: i32,
        output: &str,
        want_code: i32,
        present: &str,
        absent: &[&str],
    ) -> Result<(), String> {
        if code != want_code || !output.contains(present) {
            return Err(format!(
                "{label}: code {code}, wanted {want_code} and {present}: {output}"
            ));
        }
        for needle in absent {
            if output.contains(needle) {
                return Err(format!("{label} contained {needle}: {output}"));
            }
        }
        Ok(())
    }

    #[test]
    fn clean_byte_check_matches_a_large_output() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        let clean = byte_check_between(
            &ci,
            "          f=\"$copy/unmutated-guards.txt\"\n",
            "\n          python3 - \"$copy\" <<'PY'\n",
        )?;
        let mutated = byte_check_between(
            &ci,
            "          f=\"$copy/mutated-guards.txt\"\n",
            "\n  tla:\n",
        )?;
        assert_file_grep("clean", &clean, "clean_rc")?;
        assert_file_grep("mutated", &mutated, "match_rc")?;
        let dir = TempGit::new("large-clean")?;
        let mut matched = b"ci job fmt bytes do not match\n".to_vec();
        matched.extend(std::iter::repeat_n(b'x', 200_000));
        if matched.len() < 200_000 {
            return Err(format!("large output is {} bytes", matched.len()));
        }
        let matched_path = dir.path.join("matched.txt");
        std::fs::write(&matched_path, &matched).map_err(|err| format!("write matched: {err}"))?;
        let copy = dir.path.join("copy");
        std::fs::create_dir(&copy).map_err(|err| format!("mkdir copy: {err}"))?;
        let hit = guard_byte_script(&clean, &copy, &matched_path, "")?;
        let (code, output) = run_shell(&hit)?;
        require_outcome(
            "large match",
            code,
            &output,
            1,
            "unmutated copy reported a byte mismatch",
            &["reached-end", "unmutated guards grep failed"],
        )?;
        let pipe = format!(
            "set -o pipefail\nclean=$(cat {quoted})\nif printf '%s\\n' \"$clean\" | grep -q 'ci job fmt bytes do not match'; then echo pipe-hit; else echo pipe-miss; fi\n",
            quoted = shell_quote(&matched_path)?,
        );
        let (pipe_code, pipe_output) = run_shell(&pipe)?;
        if pipe_code != 0 || !pipe_output.contains("pipe-miss") || pipe_output.contains("pipe-hit")
        {
            return Err(format!(
                "200 KB pipe no longer misses, so this positive would not catch the bug: code {pipe_code}: {pipe_output}"
            ));
        }
        let absent_path = dir.path.join("absent.txt");
        std::fs::write(&absent_path, vec![b'x'; 200_000])
            .map_err(|err| format!("write absent: {err}"))?;
        let miss = guard_byte_script(&clean, &copy, &absent_path, "")?;
        let (miss_code, miss_output) = run_shell(&miss)?;
        require_outcome(
            "large miss",
            miss_code,
            &miss_output,
            0,
            "reached-end",
            &[
                "unmutated copy reported a byte mismatch",
                "unmutated guards grep failed",
            ],
        )?;
        let found = guard_byte_script(&mutated, &copy, &matched_path, "")?;
        let (found_code, found_output) = run_shell(&found)?;
        require_outcome(
            "mutated large match",
            found_code,
            &found_output,
            0,
            "reached-end",
            &[
                "mutated copy missed the byte mismatch",
                "mutated guards grep failed",
            ],
        )?;
        let missed = guard_byte_script(&mutated, &copy, &absent_path, "")?;
        let (missed_code, missed_output) = run_shell(&missed)?;
        require_outcome(
            "mutated large miss",
            missed_code,
            &missed_output,
            1,
            "mutated copy missed the byte mismatch",
            &["reached-end", "mutated guards grep failed"],
        )?;
        let blocked = dir.path.join("not-a-directory");
        std::fs::write(&blocked, b"file\n").map_err(|err| format!("write blocked: {err}"))?;
        let unwritten = guard_byte_script(&clean, &blocked, &absent_path, "")?;
        let (write_code, write_output) = run_shell(&unwritten)?;
        require_outcome(
            "write failure",
            write_code,
            &write_output,
            1,
            "unmutated guards output was not written",
            &["reached-end", "unmutated copy reported a byte mismatch"],
        )?;
        let as_dir = dir.path.join("grep-target");
        std::fs::create_dir(&as_dir).map_err(|err| format!("mkdir grep target: {err}"))?;
        let bin = dir.path.join("bin");
        std::fs::create_dir(&bin).map_err(|err| format!("mkdir bin: {err}"))?;
        let wrapper = bin.join("grep");
        let grep_dir = shell_quote(&as_dir)?;
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nexec /usr/bin/grep -qF -- 'ci job fmt bytes do not match' {grep_dir}\n"
            ),
        )
        .map_err(|err| format!("write grep: {err}"))?;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
            .map_err(|err| format!("chmod grep: {err}"))?;
        let prefix = format!("export PATH={}:\"$PATH\"\n", shell_quote(&bin)?);
        let errored = guard_byte_script(&clean, &copy, &absent_path, &prefix)?;
        let (error_code, error_output) = run_shell(&errored)?;
        require_outcome(
            "grep on a directory",
            error_code,
            &error_output,
            2,
            "unmutated guards grep failed",
            &["reached-end", "unmutated copy reported a byte mismatch"],
        )?;
        let mutated_error = guard_byte_script(&mutated, &copy, &matched_path, &prefix)?;
        let (mut_error_code, mut_error_output) = run_shell(&mutated_error)?;
        require_outcome(
            "mutated grep on a directory",
            mut_error_code,
            &mut_error_output,
            2,
            "mutated guards grep failed",
            &["reached-end", "mutated copy missed the byte mismatch"],
        )?;
        Ok(())
    }

    fn split_outside_quotes(line: &str, pipe: bool) -> Vec<String> {
        let mut parts = Vec::new();
        let mut cur = String::new();
        let mut chars = line.chars().peekable();
        let mut quote = None;
        while let Some(ch) = chars.next() {
            if let Some(open) = quote {
                cur.push(ch);
                if ch == open {
                    quote = None;
                }
                continue;
            }
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
                cur.push(ch);
                continue;
            }
            if ch == '|' {
                if chars.peek() == Some(&'|') {
                    cur.push('|');
                    cur.push('|');
                    chars.next();
                    continue;
                }
                if pipe {
                    if chars.peek() == Some(&'&') {
                        chars.next();
                    }
                    parts.push(std::mem::take(&mut cur));
                    continue;
                }
            }
            if !pipe && (ch == ';' || ch == '\n' || (ch == '&' && chars.peek() == Some(&'&'))) {
                if ch == '&' {
                    chars.next();
                }
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            cur.push(ch);
        }
        parts.push(cur);
        if pipe {
            parts.retain(|part| !part.trim().is_empty());
        }
        parts
    }

    fn updates_pipefail(part: &str) -> Option<bool> {
        let words: Vec<&str> = part.split_whitespace().collect();
        if words.first().copied() != Some("set") {
            return None;
        }
        let mut idx = 1usize;
        let mut result = None;
        while idx < words.len() {
            let word = words[idx];
            if word == "-o" || word == "+o" {
                if words.get(idx + 1) == Some(&"pipefail") {
                    result = Some(word == "-o");
                }
                idx += 2;
                continue;
            }
            let disable = word.starts_with('+');
            let flags = word.strip_prefix(['-', '+']).unwrap_or("");
            if !flags.is_empty() && flags.contains('o') {
                if words.get(idx + 1) == Some(&"pipefail") {
                    result = Some(!disable);
                }
                idx += 2;
                continue;
            }
            idx += 1;
        }
        result
    }

    fn command_exits_early(stage: &str) -> bool {
        let words = shell_words(stage);
        let mut idx = 0usize;
        while idx < words.len() {
            let bare =
                words[idx].trim_matches(|ch: char| matches!(ch, '"' | '\'' | '(' | ')' | '`'));
            if matches!(bare, "env" | "-i" | "command" | "exec" | "\\")
                || assignment_name(bare).is_some()
            {
                idx += 1;
                continue;
            }
            break;
        }
        let Some(cmd) = words.get(idx) else {
            return false;
        };
        let cmd = cmd.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '(' | ')' | '`'));
        let name = cmd.rsplit('/').next().unwrap_or(cmd);
        if name == "head" {
            return true;
        }
        if name != "grep" {
            return false;
        }
        for arg in words.iter().skip(idx + 1) {
            let arg = arg.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '(' | ')' | '`'));
            if arg == "--" {
                break;
            }
            if let Some(long) = arg.strip_prefix("--") {
                if long == "quiet" || long == "max-count" || long.starts_with("max-count=") {
                    return true;
                }
                continue;
            }
            if let Some(short) = arg.strip_prefix('-') {
                if !short.is_empty() && (short.contains('q') || short.contains('m')) {
                    return true;
                }
            }
        }
        false
    }

    fn pipeline_ends_early(part: &str) -> bool {
        let stages = split_outside_quotes(part, true);
        stages.len() >= 2 && stages.iter().any(|stage| command_exits_early(stage))
    }

    fn script_early_exits(script: &str) -> Vec<String> {
        let mut pipefail = false;
        let mut hits = Vec::new();
        let mut current = String::new();
        for line in script.lines() {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
            if line.trim_end().ends_with('\\') {
                continue;
            }
            let logical = std::mem::take(&mut current);
            for part in split_outside_quotes(&logical, false) {
                if let Some(enabled) = updates_pipefail(&part) {
                    pipefail = enabled;
                }
                if pipefail && pipeline_ends_early(&part) {
                    hits.push(part);
                }
            }
        }
        hits
    }

    fn run_scripts(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let mut scripts = Vec::new();
        let mut idx = 0usize;
        while idx < lines.len() {
            if lines[idx].trim() != "run: |" {
                idx += 1;
                continue;
            }
            let run_indent = lines[idx].chars().take_while(|ch| *ch == ' ').count();
            idx += 1;
            let mut body = Vec::new();
            while idx < lines.len() {
                let line = lines[idx];
                if !line.trim().is_empty()
                    && line.chars().take_while(|ch| *ch == ' ').count() <= run_indent
                {
                    break;
                }
                body.push(line);
                idx += 1;
            }
            scripts.push(body.join("\n"));
        }
        scripts
    }

    #[test]
    fn pipefail_scripts_do_not_end_in_grep_or_head() -> Result<(), String> {
        if !pipeline_ends_early("printf '%s\\n' \"$v\" | grep -qF -- 'lit'") {
            return Err("detector missed grep -qF".to_owned());
        }
        if !pipeline_ends_early("printf '%s\\n' \"$v\" | grep -m 1 lit") {
            return Err("detector missed grep -m".to_owned());
        }
        if !pipeline_ends_early("printf '%s\\n' \"$v\" | head -n 1") {
            return Err("detector missed head".to_owned());
        }
        if !pipeline_ends_early("printf '%s\\n' \"$v\" | grep -q 'bad' | cat") {
            return Err("detector missed a middle grep -q".to_owned());
        }
        if !pipeline_ends_early("if printf '%s\\n' \"$v\" | grep -q 'bad' | cat; then exit 1; fi") {
            return Err("detector missed a middle grep -q before cat".to_owned());
        }
        if !pipeline_ends_early("printf '%s\\n' \"$v\" | head -n 1 | cat") {
            return Err("detector missed a middle head".to_owned());
        }
        if !pipeline_ends_early("printf '%s\\n' \"$v\" | grep -m 1 lit | cat") {
            return Err("detector missed a middle grep -m".to_owned());
        }
        if pipeline_ends_early("grep -qF -- 'lit' file")
            || pipeline_ends_early("cargo test | tee out")
        {
            return Err("detector flagged a file grep or tee".to_owned());
        }
        let sample = "        run: |\n          set -euo pipefail\n          printf '%s\\n' \"$v\" | grep -q hit\n          if printf '%s\\n' \"$v\" | grep -q 'bad' | cat; then exit 1; fi\n          printf '%s\\n' \"$v\" | head -n 1 | cat\n        run: |\n          printf '%s\\n' \"$v\" | head -n 1\n        run: |\n          set -o pipefail\n          cargo xtask guards | tee out\n          grep -qx 'guards ran 12 passed 12' out\n";
        let sample_hits = run_scripts(sample)
            .into_iter()
            .flat_map(|script| script_early_exits(&script))
            .collect::<Vec<_>>();
        if sample_hits.len() != 3
            || !sample_hits.iter().any(|hit| hit.contains("grep -q hit"))
            || !sample_hits.iter().any(|hit| hit.contains("grep -q 'bad'"))
            || !sample_hits
                .iter()
                .any(|hit| hit.contains("head -n 1 | cat"))
        {
            return Err(format!("sample hits: {sample_hits:?}"));
        }
        let root = crate::workspace_root()?;
        for dir in [".github/workflows", "fixtures"] {
            let mut paths = Vec::new();
            collect_yml(&root.join(dir), &mut paths)?;
            for path in paths {
                let text = read(&path)?;
                let hits = run_scripts(&text)
                    .iter()
                    .flat_map(|script| script_early_exits(script))
                    .collect::<Vec<_>>();
                if !hits.is_empty() {
                    return Err(format!("{}: {hits:?}", path.display()));
                }
            }
        }
        Ok(())
    }

    fn trim_token(word: &str) -> &str {
        word.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '(' | ')' | '`'))
    }

    fn command_at(stage: &str) -> Option<(String, Vec<String>)> {
        let words: Vec<String> = shell_words(stage)
            .into_iter()
            .map(|word| trim_token(word).to_owned())
            .collect();
        let mut idx = 0usize;
        while idx < words.len() {
            if matches!(
                words[idx].as_str(),
                "env"
                    | "-i"
                    | "command"
                    | "exec"
                    | "\\"
                    | "if"
                    | "elif"
                    | "while"
                    | "until"
                    | "!"
                    | "then"
                    | "do"
                    | "{"
                    | "time"
            ) || assignment_name(&words[idx]).is_some()
            {
                idx += 1;
                continue;
            }
            break;
        }
        let name = words.get(idx)?.rsplit('/').next()?.to_owned();
        let args = words.iter().skip(idx + 1).cloned().collect();
        Some((name, args))
    }

    fn args_are_quiet_grep(args: &[String]) -> bool {
        for arg in args {
            if arg == "--" {
                break;
            }
            if let Some(long) = arg.strip_prefix("--") {
                if long == "quiet" {
                    return true;
                }
                continue;
            }
            if let Some(short) = arg.strip_prefix('-') {
                if !short.is_empty() && short.contains('q') {
                    return true;
                }
            }
        }
        false
    }

    fn is_printf_grep_q(part: &str) -> bool {
        let stages = split_outside_quotes(part, true);
        if stages.len() < 2 {
            return false;
        }
        let mut printf = false;
        let mut quiet = false;
        for stage in stages {
            let Some((name, args)) = command_at(&stage) else {
                continue;
            };
            if name == "printf" {
                printf = true;
            }
            if name == "grep" && args_are_quiet_grep(&args) {
                quiet = true;
            }
        }
        printf && quiet
    }

    fn statement_has_printf_grep_q(text: &str) -> bool {
        split_outside_quotes(text, false)
            .iter()
            .any(|part| is_printf_grep_q(part))
    }

    fn updates_errexit(part: &str) -> Option<bool> {
        let words: Vec<&str> = part.split_whitespace().collect();
        if words.first().copied() != Some("set") {
            return None;
        }
        let mut result = None;
        for word in words.iter().skip(1) {
            if let Some(flags) = word.strip_prefix('+') {
                if flags.contains('e') {
                    result = Some(false);
                }
                continue;
            }
            if let Some(flags) = word.strip_prefix('-') {
                if flags.contains('e') {
                    result = Some(true);
                }
            }
        }
        result
    }

    fn note_grep_q_statement(statement: &str, errexit: &mut bool, hits: &mut Vec<String>) {
        if statement.trim().is_empty() {
            return;
        }
        if let Some(enabled) = updates_errexit(statement) {
            *errexit = enabled;
            return;
        }
        if !(*errexit) && statement_has_printf_grep_q(statement) {
            hits.push(statement.trim().to_owned());
        }
    }

    fn grep_q_statement_violations(script: &str) -> Vec<String> {
        let chars: Vec<char> = script.chars().collect();
        let mut hits = Vec::new();
        let mut errexit = true;
        let mut quote = None;
        let mut quote_stack = Vec::new();
        let mut depth = 0usize;
        let mut outer = String::new();
        let mut inners: Vec<String> = Vec::new();
        let mut idx = 0usize;
        while idx < chars.len() {
            let ch = chars[idx];
            let in_double = quote == Some('"');
            if quote.is_some() && !(in_double && ch == '$' && chars.get(idx + 1) == Some(&'(')) {
                if depth == 0 {
                    outer.push(ch);
                } else if let Some(buf) = inners.last_mut() {
                    buf.push(ch);
                }
                if Some(ch) == quote {
                    quote = None;
                }
                idx += 1;
                continue;
            }
            if ch == '$' && chars.get(idx + 1) == Some(&'(') {
                depth += 1;
                quote_stack.push(quote);
                quote = None;
                inners.push(String::new());
                idx += 2;
                continue;
            }
            if ch == ')' && depth > 0 {
                depth -= 1;
                quote = quote_stack.pop().unwrap_or(None);
                let inner = inners.pop().unwrap_or_default();
                if statement_has_printf_grep_q(&inner) {
                    hits.push(inner.trim().to_owned());
                }
                idx += 1;
                continue;
            }
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
                if depth == 0 {
                    outer.push(ch);
                } else if let Some(buf) = inners.last_mut() {
                    buf.push(ch);
                }
                idx += 1;
                continue;
            }
            let separator = depth == 0
                && (ch == '\n'
                    || ch == ';'
                    || (ch == '&' && chars.get(idx + 1) == Some(&'&'))
                    || (ch == '|' && chars.get(idx + 1) == Some(&'|')));
            if separator {
                note_grep_q_statement(&outer, &mut errexit, &mut hits);
                outer.clear();
                if ch == '&' || ch == '|' {
                    idx += 2;
                } else {
                    idx += 1;
                }
                continue;
            }
            if depth == 0 {
                outer.push(ch);
            } else if let Some(buf) = inners.last_mut() {
                buf.push(ch);
            }
            idx += 1;
        }
        note_grep_q_statement(&outer, &mut errexit, &mut hits);
        hits
    }

    #[test]
    fn printf_grep_q_statement_requires_set_e() -> Result<(), String> {
        let top = "printf '%s\\n' \"$v\" | grep -q 'bad'\n";
        let top_hits = grep_q_statement_violations(top);
        if !top_hits.is_empty() {
            return Err(format!(
                "bash -e rejected a top-level grep -q: {top_hits:?}"
            ));
        }
        let disabled = "set +e\nprintf '%s\\n' \"$v\" | grep -q 'bad'\n";
        let disabled_hits = grep_q_statement_violations(disabled);
        if disabled_hits.len() != 1 || !disabled_hits[0].contains("grep -q") {
            return Err(format!("set +e: {disabled_hits:?}"));
        }
        let nested = "set -e\nhit=$(printf '%s\\n' \"$v\" | grep -q 'bad')\n";
        let nested_hits = grep_q_statement_violations(nested);
        if nested_hits.len() != 1 || !nested_hits[0].contains("grep -q") {
            return Err(format!("command substitution: {nested_hits:?}"));
        }
        let restored = "set +e\noutput=$(printf '%s\\n' \"$v\")\nset -e\nprintf '%s\\n' \"$output\" | grep -q 'bad'\n";
        let restored_hits = grep_q_statement_violations(restored);
        if !restored_hits.is_empty() {
            return Err(format!("restored set -e: {restored_hits:?}"));
        }
        let conditional =
            "set -e\nif ! printf '%s\\n' \"$output\" | grep -q \"$lint\"; then\n  exit 1\nfi\n";
        let conditional_hits = grep_q_statement_violations(conditional);
        if !conditional_hits.is_empty() {
            return Err(format!("if grep -q: {conditional_hits:?}"));
        }
        let root = crate::workspace_root()?;
        for dir in [".github/workflows", "fixtures"] {
            let mut paths = Vec::new();
            collect_yml(&root.join(dir), &mut paths)?;
            for path in paths {
                let text = read(&path)?;
                let hits = run_scripts(&text)
                    .iter()
                    .flat_map(|script| grep_q_statement_violations(script))
                    .collect::<Vec<_>>();
                if !hits.is_empty() {
                    return Err(format!("{}: {hits:?}", path.display()));
                }
            }
        }
        Ok(())
    }

    fn collect_yml(dir: &Path, paths: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in
            std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?
        {
            let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                collect_yml(&path, paths)?;
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("yml") {
                paths.push(path);
            }
        }
        Ok(())
    }

    fn shell_quote(path: &Path) -> Result<String, String> {
        let text = path.to_str().ok_or("path is not utf-8")?;
        if text.contains('\'') {
            return Err("path contains a single quote".to_owned());
        }
        Ok(format!("'{text}'"))
    }

    #[test]
    fn inert_member_manifest_edit_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let dir = TempGit::new("manifest-inert")?;
        let mut files: Vec<&str> = CARGO_MANIFESTS.to_vec();
        files.push("Cargo.lock");
        files.push("crates/backtest/Cargo.toml");
        for rel in files {
            let from = root.join(rel);
            let to = dir.path.join(rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|err| format!("mkdir {rel}: {err}"))?;
            }
            std::fs::copy(&from, &to).map_err(|err| format!("copy {rel}: {err}"))?;
        }
        for member in [
            "crates/backtest",
            "crates/collector",
            "crates/core",
            "crates/execution",
            "crates/regime",
            "crates/signals",
            "crates/sim",
            "xtask",
        ] {
            let lib = dir.path.join(member).join("src/lib.rs");
            if let Some(parent) = lib.parent() {
                std::fs::create_dir_all(parent).map_err(|err| format!("mkdir {member}: {err}"))?;
            }
            std::fs::write(&lib, "").map_err(|err| format!("write {member} lib: {err}"))?;
        }
        std::fs::write(dir.path.join("xtask/src/main.rs"), "fn main() {}\n")
            .map_err(|err| format!("write xtask main: {err}"))?;
        let clean = cargo_manifests_gaps(&dir.path);
        if !clean.is_empty() {
            return Err(format!(
                "unedited member manifests were rejected: {clean:?}"
            ));
        }
        let manifest = dir.path.join("crates/backtest/Cargo.toml");
        let mut text = std::fs::read_to_string(&manifest)
            .map_err(|err| format!("read backtest manifest: {err}"))?;
        text.push_str("# inert\n");
        std::fs::write(&manifest, text).map_err(|err| format!("write backtest manifest: {err}"))?;
        let gaps = cargo_manifests_gaps(&dir.path);
        if !gaps
            .iter()
            .any(|gap| gap == "cargo manifests sha256 does not match")
        {
            return Err(format!("inert member manifest edit was accepted: {gaps:?}"));
        }
        Ok(())
    }

    #[test]
    fn manifest_pipeline_fails_when_the_first_stage_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let script = manifest_pipeline_script()?;
        if !script.contains("set -o pipefail")
            || !script.contains("sha256sum \\\n            Cargo.toml")
        {
            return Err("manifest pipeline script dropped pipefail or the first stage".to_owned());
        }
        let dir = TempGit::new("manifest-pipe")?;
        provision_deny_inputs(&root, &dir.path)?;
        std::fs::remove_file(dir.path.join("xtask/Cargo.toml"))
            .map_err(|err| format!("remove manifest: {err}"))?;
        let output = Command::new("bash")
            .arg("-e")
            .arg("-c")
            .arg(&script)
            .current_dir(&dir.path)
            .output()
            .map_err(|err| format!("bash: {err}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success()
            || !text.contains("No such file")
            || text.contains("cargo manifests sha256 does not match")
        {
            let code = match output.status.code() {
                Some(code) => code.to_string(),
                None => "signal".to_owned(),
            };
            return Err(format!("first pipe stage: code {code}: {text}"));
        }
        Ok(())
    }

    #[test]
    fn each_required_job_byte_mutation_fails_its_pin() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        expect_gaps("unmutated jobs", required_job_pin_gaps(&ci), &[])?;
        for (name, _, _) in REQUIRED_JOBS {
            let block = job_block(&ci, name).ok_or_else(|| format!("ci.yml has no {name} job"))?;
            let name_at = block
                .find("\n    name:")
                .ok_or_else(|| format!("{name} name field is missing"))?;
            let line_end = block[name_at + 1..]
                .find('\n')
                .ok_or_else(|| format!("{name} name line has no end"))?
                + name_at
                + 1;
            if line_end == 0 {
                return Err(format!("{name} name line is empty"));
            }
            let block_at = ci
                .find(block)
                .ok_or_else(|| format!("{name} block is missing from ci.yml"))?;
            let byte_at = block_at + line_end - 1;
            let mut bytes = ci.as_bytes().to_vec();
            let last = bytes[byte_at];
            bytes[byte_at] = if last == b'z' { b'y' } else { last + 1 };
            let mutated = String::from_utf8(bytes).map_err(|err| format!("utf-8: {err}"))?;
            let gaps = required_job_pin_gaps(&mutated);
            let message = format!("ci job {name} bytes do not match");
            let hits: Vec<&String> = gaps.iter().filter(|gap| gap.contains(&message)).collect();
            if hits.len() != 1 || gaps.len() != 1 {
                return Err(format!("{name} mutation: {gaps:?}"));
            }
        }
        Ok(())
    }

    fn assert_yaml_parses(label: &str, text: &str) -> Result<(), String> {
        let mut child = Command::new("python3")
            .args(["-c", "import sys, yaml; yaml.safe_load(sys.stdin.read())"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|err| format!("python: {err}"))?;
        let mut stdin = child.stdin.take().ok_or("python stdin missing")?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|err| format!("write yaml: {err}"))?;
        drop(stdin);
        let output = child
            .wait_with_output()
            .map_err(|err| format!("python: {err}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{label} is not valid yaml: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    fn zero_nth_hash(text: &str, hash: &str, nth: usize) -> Result<String, String> {
        let mut from = 0usize;
        for _ in 0..nth {
            let found = text[from..]
                .find(hash)
                .ok_or_else(|| format!("hash occurrence missing: {hash}"))?;
            from += found + hash.len();
        }
        let found = text[from..]
            .find(hash)
            .ok_or_else(|| format!("hash occurrence missing: {hash}"))?;
        let at = from + found;
        let mut out = text.to_owned();
        out.replace_range(at..at + hash.len(), &"0".repeat(hash.len()));
        Ok(out)
    }

    fn manifest_pipeline_script() -> Result<String, String> {
        let deny = pinned_job("deny").ok_or("deny job is not pinned")?;
        let marker = "          set -o pipefail\n";
        let start = deny
            .find(marker)
            .ok_or("deny job manifest pipeline has no pipefail")?;
        let rest = &deny[start..];
        let message = "echo \"cargo manifests sha256 does not match\"\n";
        let message_at = rest
            .find(message)
            .ok_or("deny job manifest pipeline has no message")?;
        let after = &rest[message_at..];
        let close = after
            .find("\n          fi\n")
            .ok_or("deny job manifest pipeline has no end")?;
        Ok(rest[..message_at + close + "\n          fi".len()].to_owned())
    }

    fn toolchain_gaps_skipping_every_target(root: &Path) -> Result<Vec<String>, String> {
        let mut paths = Vec::new();
        collect_skipping_every_target(root, &mut paths)?;
        paths.sort();
        let mut gaps = Vec::new();
        for path in paths {
            let rel = path
                .strip_prefix(root)
                .map_err(|err| format!("toolchain path: {err}"))?;
            let name = rel.file_name().and_then(|part| part.to_str()).unwrap_or("");
            let shown = rel.to_string_lossy().replace('\\', "/");
            if name.starts_with("rust-toolchain") && shown != "rust-toolchain.toml" {
                gaps.push(format!(
                    "{shown} is a toolchain override; only the root rust-toolchain.toml is allowed"
                ));
            }
            let parent_is_cargo = rel
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|part| part.to_str())
                == Some(".cargo");
            if parent_is_cargo && name.starts_with("config") && shown != ".cargo/config.toml" {
                gaps.push(format!(
                    "{shown} is a nested cargo config; only the root .cargo/config.toml is allowed"
                ));
            }
        }
        gaps.sort();
        Ok(gaps)
    }

    fn collect_skipping_every_target(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries =
            std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" || name == "target" {
                continue;
            }
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|err| format!("read {}: {err}", path.display()))?;
            if file_type.is_symlink() || !file_type.is_dir() {
                found.push(path);
                continue;
            }
            collect_skipping_every_target(&path, found)?;
        }
        Ok(())
    }

    fn cached_deny_tarball() -> Result<PathBuf, String> {
        let path = std::env::temp_dir().join(format!(
            "cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl.tar.gz"
        ));
        if path.is_file() && sha256sum_file(&path).ok().as_deref() == Some(DENY_TARBALL_SHA256) {
            return Ok(path);
        }
        let url = format!(
            "https://github.com/EmbarkStudios/cargo-deny/releases/download/{DENY_VERSION}/cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl.tar.gz"
        );
        let status = Command::new("/usr/bin/curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(&path)
            .arg(&url)
            .status()
            .map_err(|err| format!("curl: {err}"))?;
        if !status.success() {
            let code = match status.code() {
                Some(code) => code.to_string(),
                None => "signal".to_owned(),
            };
            return Err(format!("curl exit {code}"));
        }
        let got = sha256sum_file(&path)?;
        if got != DENY_TARBALL_SHA256 {
            return Err(format!("cargo-deny tarball sha256 does not match: {got}"));
        }
        Ok(path)
    }

    fn sha256sum_file(path: &Path) -> Result<String, String> {
        let output = Command::new("/usr/bin/sha256sum")
            .arg(path)
            .output()
            .map_err(|err| format!("sha256sum: {err}"))?;
        if !output.status.success() {
            return Err(format!(
                "sha256sum failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        text.split_whitespace()
            .next()
            .map(str::to_owned)
            .ok_or_else(|| "sha256sum returned no hash".to_owned())
    }

    fn deny_binary_recheck_script(bin: &Path) -> Result<String, String> {
        let deny = pinned_job("deny").ok_or("deny job is not pinned")?;
        let marker = "bin_line=\"$(env -i \\";
        let start = deny.find(marker).ok_or("deny job has no binary recheck")?;
        let rest = &deny[start..];
        let message_at = rest
            .find("echo \"cargo-deny binary sha256 does not match\"\n")
            .ok_or("deny job binary recheck has no message")?;
        let after = &rest[message_at..];
        let close = after
            .find("\n          fi\n")
            .ok_or("deny job binary recheck has no end")?;
        let mut script = rest[..message_at + close + "\n          fi".len()].to_owned();
        let pinned = format!("/tmp/cargo-deny-{DENY_VERSION}-x86_64-unknown-linux-musl/cargo-deny");
        if !script.contains(&pinned) {
            return Err("deny job binary recheck does not name the pinned path".to_owned());
        }
        let shown = bin.to_string_lossy().replace('\\', "/");
        script = script.replacen(&pinned, &shown, 1);
        Ok(format!("set -euo pipefail\n{script}"))
    }

    fn provision_deny_inputs(root: &Path, dir: &Path) -> Result<(), String> {
        let mut files = vec![
            "deny.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            ".cargo/config.toml",
        ];
        files.extend(CARGO_MANIFESTS.iter().copied());
        for rel in files {
            let from = root.join(rel);
            let to = dir.join(rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|err| format!("mkdir {rel}: {err}"))?;
            }
            std::fs::copy(&from, &to).map_err(|err| format!("copy {rel}: {err}"))?;
        }
        Ok(())
    }

    fn deny_input_rehash_script() -> Result<String, String> {
        let deny = pinned_job("deny").ok_or("deny job is not pinned")?;
        let marker = "deny_line=\"$(env -i \\";
        let start = deny.find(marker).ok_or("deny job has no input rehash")?;
        let rest = &deny[start..];
        let message = "echo \".cargo/config.toml sha256 does not match\"\n";
        let message_at = rest
            .find(message)
            .ok_or("deny job input rehash has no config message")?;
        let after = &rest[message_at..];
        let close = after
            .find("\n          fi\n")
            .ok_or("deny job input rehash has no end")?;
        let script = rest[..message_at + close + "\n          fi".len()].to_owned();
        for needle in [
            "echo \"deny.toml sha256 does not match\"",
            "echo \"Cargo.lock sha256 does not match\"",
            "echo \"rust-toolchain.toml sha256 does not match\"",
            "echo \"cargo manifests sha256 does not match\"",
            "echo \".cargo/config.toml sha256 does not match\"",
            "sha256sum \\\n            Cargo.toml",
        ] {
            if !script.contains(needle) {
                return Err(format!("deny job input rehash is missing {needle}"));
            }
        }
        Ok(format!("set -euo pipefail\n{script}"))
    }

    fn run_shell(script: &str) -> Result<(i32, String), String> {
        run_shell_at(
            script,
            &std::env::current_dir().map_err(|err| format!("cwd: {err}"))?,
        )
    }

    fn run_shell_at(script: &str, dir: &Path) -> Result<(i32, String), String> {
        let output = Command::new("bash")
            .arg("-c")
            .arg(script)
            .current_dir(dir)
            .output()
            .map_err(|err| format!("bash: {err}"))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        match output.status.code() {
            Some(code) => Ok((code, text)),
            None => Err(format!("bash signal: {text}")),
        }
    }

    struct LoweredFloorRepo {
        _keep: TempGit,
        clone: PathBuf,
        tip_a: String,
    }

    fn lowered_floor_repo() -> Result<LoweredFloorRepo, String> {
        let root = crate::workspace_root()?;
        let high = read(&root.join("regressions/floors.toml"))?;
        let marker = "cucumber.kline.scenarios=12\n";
        if !high.contains(marker) {
            return Err("floors fixture is missing cucumber.kline.scenarios=12".to_owned());
        }
        let low = high.replace(marker, "cucumber.kline.scenarios=11\n");
        let keep = TempGit::new("lowered-floor")?;
        let origin = keep.path.join("origin");
        std::fs::create_dir_all(origin.join("regressions"))
            .map_err(|err| format!("mkdir: {err}"))?;
        git_fixture(&origin, &["init", "-b", "main"])?;
        std::fs::write(origin.join("regressions/floors.toml"), &high)
            .map_err(|err| format!("write: {err}"))?;
        git_fixture(&origin, &["add", "regressions/floors.toml"])?;
        git_fixture(&origin, &["commit", "-m", "main"])?;
        let origin_url = format!(
            "file://{}",
            origin.to_str().ok_or("origin path is not utf-8")?
        );
        git_fixture(&keep.path, &["clone", "--quiet", &origin_url, "clone"])?;
        let clone = keep.path.join("clone");
        git_fixture(&clone, &["checkout", "--quiet", "-b", "tip-a"])?;
        std::fs::write(clone.join("regressions/floors.toml"), &low)
            .map_err(|err| format!("write: {err}"))?;
        git_fixture(&clone, &["add", "regressions/floors.toml"])?;
        git_fixture(&clone, &["commit", "-m", "lower A"])?;
        let tip_a = git_stdout(&clone, &["rev-parse", "HEAD"])?;
        git_fixture(&clone, &["checkout", "--quiet", "main"])?;
        git_fixture(&clone, &["checkout", "--quiet", "-b", "tip-b"])?;
        std::fs::write(clone.join("regressions/floors.toml"), &low)
            .map_err(|err| format!("write: {err}"))?;
        git_fixture(&clone, &["add", "regressions/floors.toml"])?;
        git_fixture(&clone, &["commit", "-m", "lower B"])?;
        Ok(LoweredFloorRepo {
            _keep: keep,
            clone,
            tip_a,
        })
    }

    struct TempGit {
        path: PathBuf,
    }

    impl TempGit {
        fn new(label: &str) -> Result<Self, String> {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir()
                .join(format!("parangsae-{label}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&path).map_err(|err| format!("mkdir: {err}"))?;
            Ok(Self { path })
        }
    }

    impl Drop for TempGit {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn git_fixture(dir: &Path, args: &[&str]) -> Result<(), String> {
        let output = git_command(dir, args)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "git {} failed: {}",
                args.join(" "),
                command_failure_detail(
                    &String::from_utf8_lossy(&output.stderr),
                    &String::from_utf8_lossy(&output.stdout),
                    output.status.code(),
                )
            ))
        }
    }

    fn git_stdout(dir: &Path, args: &[&str]) -> Result<String, String> {
        let output = git_command(dir, args)?;
        if !output.status.success() {
            return Err(format!(
                "git {} failed: {}",
                args.join(" "),
                command_failure_detail(
                    &String::from_utf8_lossy(&output.stderr),
                    &String::from_utf8_lossy(&output.stdout),
                    output.status.code(),
                )
            ));
        }
        String::from_utf8(output.stdout)
            .map(|text| text.trim().to_owned())
            .map_err(|err| format!("git output: {err}"))
    }

    fn git_command(dir: &Path, args: &[&str]) -> Result<std::process::Output, String> {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "parangsae")
            .env("GIT_AUTHOR_EMAIL", "parangsae@example.com")
            .env("GIT_COMMITTER_NAME", "parangsae")
            .env("GIT_COMMITTER_EMAIL", "parangsae@example.com")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|err| format!("git: {err}"))
    }

    fn script_gaps(text: &str) -> Vec<String> {
        let mut gaps = Vec::new();
        for step in ci_run_steps(text) {
            let report = parse_run_script(&step.script, &step_key(&step), step.line);
            gaps.extend(report.gaps);
        }
        gaps
    }

    fn fixture_script_gaps(rel: &str) -> Result<Vec<String>, String> {
        let root = crate::workspace_root()?;
        let text = read(&root.join(rel))?;
        Ok(script_gaps(&text))
    }

    #[test]
    fn item13_head_ci_parses_and_match_forms_are_enumerated() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let ci = read(&root.join(".github/workflows/ci.yml"))?;
        let report = inventory_report(&root, &ci)?;
        expect_gaps("head ci", report.gaps, &[])?;
        assert_eq!(
            ENUMERATED_MATCH_FORMS,
            &[
                "grep -qF -- '<lit>' \"$f\"",
                "case \"$v\" in *'<lit>'*) body ;; esac",
                "case \"$v\" in *'<lit>'*) ;; *) body ;; esac",
            ]
        );
        assert_eq!(ENV_I_TEMPLATES.len(), 13);
        let mut occurrences = 0usize;
        for count in ENV_I_OCCURRENCES {
            occurrences = occurrences.saturating_add(count.1);
        }
        assert_eq!(occurrences, 18);
        assert_eq!(PRINTF_GREP_STATEMENTS, 35);
        assert_eq!(CASE_LITERAL_MATCHES, 2);
        for needle in [
            "match:p3-body",
            "match:p3-default",
            "match:printf-grep-literal",
        ] {
            if !report.listing.iter().any(|line| line.contains(needle)) {
                return Err(format!("listing missing {needle}"));
            }
        }
        let steps = ci_run_steps(&ci);
        if steps.is_empty() {
            return Err("head ci has no run steps".to_owned());
        }
        for step in &steps {
            let key = step_key(step);
            let listed = report.listing.iter().any(|line| line.starts_with(&key));
            if !listed {
                return Err(format!("{key} L{} was not classified", step.line));
            }
        }
        let post = parse_run_script(API_SNAPSHOT_POSTCHECK, "api-snapshot postcheck", 1);
        expect_gaps("postcheck", post.gaps, &[])?;
        let install = parse_run_script(H2_INSTALL, "h2 install", 1);
        expect_gaps("h2", install.gaps, &[])?;
        let qf = fixture_script_gaps("fixtures/guards/ci/item13-grep-qf.yml")?;
        expect_gaps("grep -qF", qf, &[])?;
        let qf_report = parse_run_script(
            &ci_run_steps(&read(&root.join("fixtures/guards/ci/item13-grep-qf.yml"))?)
                .first()
                .ok_or("grep -qF fixture has no step")?
                .script,
            "sample probe",
            1,
        );
        if !qf_report
            .listing
            .iter()
            .any(|line| line.contains("match:grep-qF"))
        {
            return Err(format!(
                "grep -qF was not classified: {:?}",
                qf_report.listing
            ));
        }
        expect_gaps(
            "p3 body",
            fixture_script_gaps("fixtures/guards/ci/item13-p3-body.yml")?,
            &[],
        )?;
        expect_gaps(
            "p3 default",
            fixture_script_gaps("fixtures/guards/ci/item13-p3-default.yml")?,
            &[],
        )?;
        expect_gaps(
            "pipefail plain",
            fixture_script_gaps("fixtures/guards/ci/item13-pipefail-plain.yml")?,
            &[],
        )?;
        expect_gaps(
            "exit captured",
            fixture_script_gaps("fixtures/guards/ci/item13-exit-captured.yml")?,
            &[],
        )?;
        Ok(())
    }

    #[test]
    fn item13_named_rejections() -> Result<(), String> {
        let cases: &[(&str, &[&str])] = &[
            (
                "fixtures/guards/ci/item13-variable-argv0.yml",
                &["argv0 comes from a variable"],
            ),
            (
                "fixtures/guards/ci/item13-eval.yml",
                &["eval is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-backticks.yml",
                &["backticks are not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-github-argv0.yml",
                &["github expression in argv0 or test selector"],
            ),
            (
                "fixtures/guards/ci/item13-github-selector.yml",
                &["github expression in argv0 or test selector"],
            ),
            (
                "fixtures/guards/ci/item13-bash-c.yml",
                &["bash -c is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-sh-c.yml",
                &["sh -c is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-source.yml",
                &["source is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-dot-source.yml",
                &["source is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-function.yml",
                &["functions defined in the step are not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-unlisted-heredoc.yml",
                &["unlisted heredoc"],
            ),
            (
                "fixtures/guards/ci/item13-heredoc-shell.yml",
                &["heredoc into a shell is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-heredoc-body.yml",
                &["heredoc body is not the pinned template"],
            ),
            (
                "fixtures/guards/ci/item13-env.yml",
                &["env is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-env-key.yml",
                &["env -i key was added or removed"],
            ),
            (
                "fixtures/guards/ci/item13-env-value.yml",
                &["env -i value is not an accepted deny form"],
            ),
            (
                "fixtures/guards/ci/item13-env-binary.yml",
                &["env -i binary path is not an accepted deny form"],
            ),
            (
                "fixtures/guards/ci/item13-env-argument.yml",
                &["env -i argument is not an accepted deny form"],
            ),
            (
                "fixtures/guards/ci/item13-pipeline-without-pipefail.yml",
                &["pipeline without pipefail"],
            ),
            (
                "fixtures/guards/ci/item13-positive-printf-grep.yml",
                &["positive printf grep is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-test-piped-grep.yml",
                &["sample probe: test output is piped into grep"],
            ),
            (
                "fixtures/guards/ci/item13-test-piped-grep-pipefail.yml",
                &["sample probe: test output is piped into grep"],
            ),
            (
                "fixtures/guards/ci/item13-faked-summary.yml",
                &["sample probe: test summary outside a binary section"],
            ),
            (
                "fixtures/guards/ci/item13-faked-printf.yml",
                &["sample probe: test summary outside a binary section"],
            ),
            (
                "fixtures/guards/ci/item13-exec.yml",
                &["exec is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-timeout.yml",
                &["timeout is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-command.yml",
                &["command is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-xargs.yml",
                &["xargs is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-find-exec.yml",
                &["find -exec is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-here-string.yml",
                &["here-string is not accepted"],
            ),
            (
                "fixtures/guards/ci/item13-absolute-cargo.yml",
                &["absolute cargo path is not the pinned toolchain"],
            ),
            (
                "fixtures/guards/ci/item13-exit-uncaptured.yml",
                &["exit status was not captured"],
            ),
            (
                "fixtures/guards/ci/item13-grep-q-pipefail.yml",
                &["sample probe: early-exit reader in a pipeline under pipefail"],
            ),
            (
                "fixtures/guards/ci/item13-grep-m-pipefail.yml",
                &["sample probe: early-exit reader in a pipeline under pipefail"],
            ),
            (
                "fixtures/guards/ci/item13-head-pipefail.yml",
                &["sample probe: early-exit reader in a pipeline under pipefail"],
            ),
            (
                "fixtures/guards/ci/item13-sed-pipefail.yml",
                &["sample probe: early-exit reader in a pipeline under pipefail"],
            ),
            (
                "fixtures/guards/ci/item13-awk-pipefail.yml",
                &["sample probe: early-exit reader in a pipeline under pipefail"],
            ),
            (
                "fixtures/guards/ci/item13-grep-qf-absent.yml",
                &["sample probe: grep rc above 1 is treated as absent"],
            ),
            (
                "fixtures/guards/ci/item13-grep-qf-pipe.yml",
                &["sample probe: early-exit reader in a pipeline under pipefail"],
            ),
            (
                "fixtures/guards/ci/item13-case-double-quote.yml",
                &["case form is not a p3 match"],
            ),
            (
                "fixtures/guards/ci/item13-unpinned-callee.yml",
                &["unpinned callee fixtures/not-pinned/gate_results.py"],
            ),
            (
                "fixtures/guards/ci/item13-unclassified.yml",
                &["unclassified command not-a-command"],
            ),
            (
                "fixtures/guards/ci/item13-bare-cargo-test.yml",
                &["sample probe: test step does not assert its output"],
            ),
            (
                "fixtures/guards/ci/item13-test-mask.yml",
                &[
                    "pipeline without pipefail",
                    "sample probe: test step masks its exit code",
                ],
            ),
        ];
        for (path, expected) in cases {
            expect_gaps(path, fixture_script_gaps(path)?, expected)?;
        }
        Ok(())
    }

    #[test]
    fn item13_env_occurrence_and_closure_and_bespoke() -> Result<(), String> {
        let mut short = Vec::new();
        for (template, (trailer, _)) in ENV_I_TEMPLATES.iter().zip(ENV_I_OCCURRENCES.iter()) {
            short.push((
                "step".to_owned(),
                (*template).to_owned(),
                (*trailer).to_owned(),
            ));
        }
        expect_gaps(
            "env short",
            env_occurrence_gaps(&short),
            &["env -i occurrence count does not match"],
        )?;
        short.push((
            "step".to_owned(),
            "env -i HOME=/tmp /bin/true".to_owned(),
            String::new(),
        ));
        if !env_occurrence_gaps(&short)
            .iter()
            .any(|gap| gap == "env -i is not an accepted deny form")
        {
            return Err("unknown env form was accepted".to_owned());
        }
        let mut trailed = Vec::new();
        for (template, (trailer, count)) in ENV_I_TEMPLATES.iter().zip(ENV_I_OCCURRENCES.iter()) {
            for _ in 0..*count {
                trailed.push((
                    "step".to_owned(),
                    (*template).to_owned(),
                    (*trailer).to_owned(),
                ));
            }
        }
        trailed.push((
            "deny cargo-deny check".to_owned(),
            ENV_I_TEMPLATES[4].to_owned(),
            " 2>/dev/null)".to_owned(),
        ));
        expect_gaps(
            "env trailer",
            env_occurrence_gaps(&trailed),
            &["deny cargo-deny check: env -i trailer not in ENV_I_OCCURRENCES"],
        )?;
        expect_gaps(
            "pin guards.rs",
            assess_closure(&[], &[("xtask/src/guards.rs", "abc")]),
            &["closure pin names guards.rs"],
        )?;
        expect_gaps(
            "outside",
            assess_closure(&[], &[("not/a/file", "abc")]),
            &["not/a/file is outside the closure"],
        )?;
        expect_gaps(
            "unpinned",
            assess_closure(&[("a.rs", b"hi")], &[]),
            &["a.rs is not pinned"],
        )?;
        expect_gaps(
            "sha",
            assess_closure(&[("a.rs", b"hi")], &[("a.rs", "deadbeef")]),
            &["a.rs sha256 does not match"],
        )?;
        let root = crate::workspace_root()?;
        expect_gaps(
            "bespoke missing",
            bespoke_fixture_gaps(&root, "fixtures/guards/bespoke/missing.txt", ""),
            &["bespoke harness fixture is missing"],
        )?;
        expect_gaps(
            "bespoke empty",
            bespoke_fixture_gaps(&root, "fixtures/guards/bespoke/empty.txt", ""),
            &["bespoke harness has no cases"],
        )?;
        expect_gaps(
            "bespoke pass",
            bespoke_fixture_gaps(
                &root,
                "fixtures/guards/bespoke/cases.txt",
                "control ok\nmutant ok\n2\n",
            ),
            &["bespoke harness case unexpectedly passed"],
        )?;
        expect_gaps(
            "bespoke held",
            bespoke_fixture_gaps(
                &root,
                "fixtures/guards/bespoke/cases.txt",
                "control ok\nmutant fail\n2\n",
            ),
            &[],
        )?;
        Ok(())
    }

    #[test]
    fn item13_binary_table_negatives() -> Result<(), String> {
        let own = BinaryRow {
            job: "test",
            step: "alpha",
            package: "pkg",
            kind: "lib",
            target: "deps/aaa-",
            count: 1,
        };
        let other = BinaryRow {
            job: "test",
            step: "beta",
            package: "pkg",
            kind: "lib",
            target: "deps/bbb-",
            count: 1,
        };
        let rows = [own, other];
        let log_ok = "\
Running unittests src/lib.rs (target/debug/deps/aaa-hash)
running 1 test
test result: ok. 1 passed
";
        expect_gaps(
            "match",
            test_binary_table_with(log_ok, "test", "alpha", &rows),
            &[],
        )?;
        let lowered = BinaryRow { count: 1, ..own };
        let log_high = "\
Running unittests src/lib.rs (target/debug/deps/aaa-hash)
running 2 tests
";
        expect_gaps(
            "lowered",
            test_binary_table_with(log_high, "test", "alpha", &[lowered, other]),
            &["test alpha pkg lib: test binary count lowered"],
        )?;
        let zero = BinaryRow { count: 0, ..own };
        expect_gaps(
            "zero",
            test_binary_table_with(log_ok, "test", "alpha", &[zero, other]),
            &["test alpha pkg lib: test binary count is 0 for a binary that runs tests"],
        )?;
        let log_removed = "\
Running unittests src/lib.rs (target/debug/deps/aaa-hash)
running 1 test
Running unittests src/lib.rs (target/debug/deps/unknown-)
running 1 test
";
        expect_gaps(
            "removed",
            test_binary_table_with(log_removed, "test", "alpha", &rows),
            &["test alpha: test binary row removed"],
        )?;
        let log_other = "\
Running unittests src/lib.rs (target/debug/deps/aaa-hash)
running 1 test
Running unittests src/lib.rs (target/debug/deps/bbb-hash)
running 1 test
";
        expect_gaps(
            "other step",
            test_binary_table_with(log_other, "test", "alpha", &rows),
            &["test alpha: test binary row is on another step"],
        )?;
        expect_gaps(
            "extra",
            test_binary_table_with("", "test", "alpha", &rows),
            &["test alpha pkg deps/aaa-: test binary row extra"],
        )?;
        let log_unmap = "\
Running unittests src/lib.rs (target/debug/deps/aaa-hash)
running 1 test
Running tests
";
        expect_gaps(
            "unmappable",
            test_binary_table_with(log_unmap, "test", "alpha", &rows),
            &["test alpha: unmappable Running line"],
        )?;
        expect_gaps(
            "no rows",
            test_binary_table_with(log_ok, "test", "missing", &rows),
            &["test missing: libtest invocation has no binary rows"],
        )?;
        let running = CmdFact {
            test_running: true,
            self_checked: false,
            kani: false,
            teed: false,
            captured: false,
            in_pipeline: false,
            suppressed: false,
            pipefail: true,
        };
        let mut steps = Vec::new();
        let mut seen = BTreeSet::new();
        for row in binary_rows() {
            if seen.insert((row.job, row.step)) {
                steps.push((
                    RunStep {
                        job: row.job.to_owned(),
                        name: row.step.to_owned(),
                        script: String::new(),
                        line: 1,
                    },
                    vec![running],
                ));
            }
        }
        expect_gaps("placed", binary_placement_gaps(&steps), &[])?;
        let dropped = steps.pop().ok_or("no binary steps")?;
        let dropped_key = format!("{} {}", dropped.0.job, dropped.0.name);
        expect_gaps(
            "missing step",
            binary_placement_gaps(&steps),
            &[&format!(
                "{dropped_key}: test binary step key matches no step"
            )],
        )?;
        steps.push((
            RunStep {
                job: "test".to_owned(),
                name: "not-in-the-table".to_owned(),
                script: String::new(),
                line: 1,
            },
            vec![running],
        ));
        steps.push(dropped);
        expect_gaps(
            "extra step",
            binary_placement_gaps(&steps),
            &["test not-in-the-table: libtest invocation has no binary rows"],
        )?;
        Ok(())
    }

    #[test]
    fn item13_cucumber_contract_counts() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let samples = [
            (
                "fixtures/guards/cucumber-count/background.feature",
                2usize,
                8usize,
                0usize,
            ),
            (
                "fixtures/guards/cucumber-count/rule-background.feature",
                3,
                11,
                1,
            ),
            ("fixtures/guards/cucumber-count/examples.feature", 6, 12, 0),
        ];
        for (rel, scenarios, steps, rules) in samples {
            let counts = contract_counts(&read(&root.join(rel))?)?;
            if counts != (scenarios, steps, rules) {
                return Err(format!("{rel}: {counts:?}"));
            }
        }
        let mut log = String::new();
        let policy: FeaturePolicy = toml::from_str(&read(&root.join("policy/features.toml"))?)
            .map_err(|err| format!("parse policy: {err}"))?;
        for runner in &policy.runner {
            for path in expand_glob(&root, &runner.glob)? {
                let (scenarios, steps, rules) = contract_counts(&read(&path)?)?;
                log.push_str(&summary_block(1, rules, scenarios, steps, 0, 0, 0));
            }
        }
        for runner in ["kline", "vision", "instrument_rules"] {
            expect_gaps(runner, cucumber_summary_gaps(&root, runner, &log)?, &[])?;
            let missing = cucumber_summary_gaps(&root, runner, "")?;
            if missing.is_empty() {
                return Err(format!("{runner} accepted an empty log"));
            }
        }
        let reject = "\
[Summary]
0 features
0 scenarios
0 steps
1 parsing error
[Summary]
1 feature
1 scenario (1 failed)
1 step (1 failed)
";
        expect_gaps(
            "feature_reject",
            cucumber_summary_gaps(&root, "feature_reject", reject)?,
            &[],
        )?;
        let wrong = cucumber_summary_gaps(&root, "feature_reject", "")?;
        expect_gaps(
            "feature_reject empty",
            wrong,
            &["cucumber summary does not match feature_reject"],
        )?;
        let lock = "name = \"not-cucumber\"\n";
        expect_gaps(
            "crate pin",
            crate_pin_gaps(lock)?,
            &[
                "cucumber/gherkin crate pin does not match Cargo.lock",
                "cucumber/gherkin crate pin does not match Cargo.lock",
            ],
        )?;
        Ok(())
    }

    #[test]
    fn item13_early_exit_positions_use_two_hundred_kilobytes() -> Result<(), String> {
        let blob = "x".repeat(200_000);
        let scripts = [
            format!("set -o pipefail\nprintf '%s\\n' '{blob}' | grep -q 'z'\n"),
            format!("set -o pipefail\nif printf '%s\\n' '{blob}' | grep -q 'z'; then exit 1; fi\n"),
            format!(
                "set -o pipefail\nif ! printf '%s\\n' '{blob}' | grep -m 1 'z'; then exit 1; fi\n"
            ),
            format!("set -o pipefail\nprintf '%s\\n' '{blob}' | grep -q 'z' && true\n"),
            format!("set -o pipefail\nprintf '%s\\n' '{blob}' | head -n 1 || true\n"),
        ];
        for script in scripts {
            if script.len() < 200_000 {
                return Err("early-exit negative was under 200KB".to_owned());
            }
            let report = parse_run_script(&script, "sample probe", 1);
            if !report
                .gaps
                .iter()
                .any(|gap| gap == "sample probe: early-exit reader in a pipeline under pipefail")
            {
                return Err(format!("{:?}", report.gaps));
            }
        }
        Ok(())
    }

    #[test]
    fn item13_run_exact_decision() {
        assert!(
            crate::run_exact_decision("test result: ok. 1 passed; 0 failed\n", "", true).is_ok()
        );
        assert!(
            crate::run_exact_decision("test result: ok. 1 passed; 0 failed\n", "", false).is_err()
        );
        assert!(
            crate::run_exact_decision("test result: ok. 0 passed; 0 failed\n", "", true).is_err()
        );
        assert!(crate::run_exact_decision("", "error: no tests matched", false).is_err());
    }
}
