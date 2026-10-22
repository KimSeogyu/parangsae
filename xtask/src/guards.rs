//! Guards against a green run that did not exercise the check.
//!
//! Feature ownership, TLC config coverage, distinct-state floors, Kani harness
//! names, B2 measurements, and the ratchet in `regressions/floors.toml`.
//! The baseline commit comes from `PARANGSAE_FLOOR_BASE`. An all-zero
//! `before` is not a base: it unshallows a shallow checkout, fetches
//! `refs/remotes/origin/main`, and uses `merge-base`. A missing
//! `regressions/floors.toml` is accepted only when that resolved commit is
//! exactly `BOOTSTRAP_BASE`, the raw sha256 of the current floors file is
//! `BOOTSTRAP_FLOORS_SHA256`, and `origin/main` does not already contain the
//! file. The landing push is the exception: on `main`, with that base, when
//! `origin/main` equals `HEAD`. Any other missing baseline fails.
//! `BOOTSTRAP_BASE`, `BOOTSTRAP_FLOORS_SHA256`, `EXPECTED_GUARD_RUNS`,
//! `CARGO_CONFIG_TOML`, `CARGO_CONFIG_SHA256`, `ALLOWED_ACTIONS`,
//! `REQUIRED_JOBS`, `JOB_ENV_ALLOW`, `STEP_ENV_ALLOW`, `RUST_TOOLCHAIN_TOML`,
//! `RUST_TOOLCHAIN_SHA256`, `DENY_TOML_SHA256`, and `CARGO_LOCK_SHA256`
//! are Reviewer-protected. Baseline resolution never skips the other guards.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
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
const BOOTSTRAP_BASE: &str = "4db8a75a7bd0886f4cdeefa050a2d89e3a36118d";

/// Reviewer-protected. The only `uses:` targets. Kani is installed with cargo, not an action.
const ALLOWED_ACTIONS: &[&str] = &[
    "actions/checkout@11d5960a326750d5838078e36cf38b85af677262",
    "actions/setup-java@cf277c60eb25467037889841efdb72551f06f6c3",
    "dtolnay/rust-toolchain@5059963b1731bf1580991b5f3b1eecfb5da406c0",
];

/// Reviewer-protected. Raw sha256 of `regressions/floors.toml` on the bootstrap path.
const BOOTSTRAP_FLOORS_SHA256: &str =
    "7c677efda602168f65e7bd98c4594a149ebe72ecc0de18c884224818bff76a07";

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
    gaps.extend(sha_file_gaps(root, "deny.toml", DENY_TOML_SHA256));
    gaps.extend(sha_file_gaps(root, "Cargo.lock", CARGO_LOCK_SHA256));
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
const DENY_TOML_SHA256: &str = "9a54da72f5bf1f6a00695a4348f76941bdfae24e84b86d445ccfced828e06c86";
/// Reviewer-protected. sha256 of `Cargo.lock`, re-checked in the deny job.
const CARGO_LOCK_SHA256: &str = "6b617ee34a33bac5754ced2bb940ea529d0ca34f409f71fb352351db72b31d04";

/// Reviewer-protected. Canonical bytes and sha256 of each required job.
const REQUIRED_JOBS: &[(&str, &str, &str)] = &[
    (
        "fmt",
        include_str!("pinned_jobs/fmt.job"),
        "d10bca32292ea0b139dba9715c5b6d7ab936d9bb52efc0ca677f3561ab35d805",
    ),
    (
        "clippy",
        include_str!("pinned_jobs/clippy.job"),
        "a2f3141767afe1d64f04ff28f7dbf7f43b5595591c33fabd44ea4ecd10cb3a51",
    ),
    (
        "lint-fixture",
        include_str!("pinned_jobs/lint-fixture.job"),
        "6cd3dfb44474100ac3c4d1d6c44b31db96261c1f99ecbc74b0dcdc279c9aaf53",
    ),
    (
        "depgraph",
        include_str!("pinned_jobs/depgraph.job"),
        "f7b305a41a10c7e9c6c21f9f74a0c90c868921ac741f7c73246ef62853082696",
    ),
    (
        "test",
        include_str!("pinned_jobs/test.job"),
        "05d0811e99913dcb19ab2edf11748c132308428e709ab772d979721221475ca6",
    ),
    (
        "cucumber",
        include_str!("pinned_jobs/cucumber.job"),
        "375b16a6f71b69d349a933442b57b368e8a3c56d21acd7eb1d8a5e609ccd388f",
    ),
    (
        "guards",
        include_str!("pinned_jobs/guards.job"),
        "8137c034f0aa49a7616a3d7cef04794ee4fd87e2876938f8dc3dfad6739baf4d",
    ),
    (
        "guard-fixtures",
        include_str!("pinned_jobs/guard-fixtures.job"),
        "a416c01e2879f63927f1eff3c1209630abd79731221c630297c883d1658352a7",
    ),
    (
        "tla",
        include_str!("pinned_jobs/tla.job"),
        "8aa93b38ccd696187890fac5a44fdfdc24152d5ef605b72d6322b00c566d95e7",
    ),
    (
        "kani",
        include_str!("pinned_jobs/kani.job"),
        "8f6c91cb840cbb8c36b00cacd98e97950a06f614cb01c38451b72294af7ca1a2",
    ),
    (
        "deny",
        include_str!("pinned_jobs/deny.job"),
        "233007703a72370e5967fdf89c51867fe67694b8c0e47ceba47d194ed1752341",
    ),
    (
        "gate",
        include_str!("pinned_jobs/gate.job"),
        "280b2f8e38a9b4e8db437ddccc58b7114804db2a857ba2f8cb04093e2bbb506a",
    ),
];

/// Reviewer-protected. Job-level `env:` names on each required job.
const JOB_ENV_ALLOW: &[(&str, &[&str])] = &[
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
    "          case \"$cargo_bin\" in\n",
    "            \"$RUSTUP_HOME\"/toolchains/1.98.0-*/bin/cargo) ;;\n",
    "            *) echo \"cargo is ${cargo_bin}\"; exit 1 ;;\n",
    "          esac\n",
    "          case \"$rustc_bin\" in\n",
    "            \"$RUSTUP_HOME\"/toolchains/1.98.0-*/bin/rustc) ;;\n",
    "            *) echo \"rustc is ${rustc_bin}\"; exit 1 ;;\n",
    "          esac\n",
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

const ALLOWED_TRIGGERS: &[&str] = &["push", "pull_request", "workflow_dispatch"];

const FORBIDDEN_ENV: &[&str] = &[
    "CARGO_WORKSPACE_DIR",
    "GITHUB_ACTIONS",
    "CI",
    "PARANGSAE_FLOOR_BASE",
    "RUSTUP_TOOLCHAIN",
];

const GATE_FIXTURE_STEP: &str = include_str!("gate_fixture_step.txt");

const GATE_RESULTS_SHA256: &str =
    "7e0f2561876477d8516e7642d8f3be7aa938cbd571273babe7bc0fd80a891666";

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
    if sha256_hex(bytes) == GATE_RESULTS_SHA256 {
        Vec::new()
    } else {
        vec!["gate_results.py sha256 does not match".to_owned()]
    }
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

fn required_job_if_gaps(text: &str, needs: &BTreeSet<String>) -> Vec<String> {
    let mut gaps = Vec::new();
    for name in needs {
        let Some(block) = job_block(text, name) else {
            continue;
        };
        if job_level_if(block) {
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
    let mut in_jobs = false;
    let mut jobs = BTreeSet::new();
    for line in text.lines() {
        if !in_jobs {
            if !line.starts_with(' ') && strip_yaml_comment(line).trim() == "jobs:" {
                in_jobs = true;
            }
            continue;
        }
        if is_top_level(line) {
            break;
        }
        let Some(id) = job_id(line) else {
            continue;
        };
        jobs.insert(id);
    }
    if !in_jobs || jobs.is_empty() {
        return Err("ci workflow has no jobs".to_owned());
    }
    Ok(jobs)
}

fn gate_job_block(text: &str) -> Result<&str, String> {
    job_block(text, "gate").ok_or_else(|| "ci.yml has no gate job".to_owned())
}

fn job_block<'a>(text: &'a str, id: &str) -> Option<&'a str> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| job_id(line).as_deref() == Some(id))?;
    let mut end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start + 1) {
        if job_id(line).is_some() || is_top_level(line) {
            end = idx;
            break;
        }
    }
    Some(&text[line_offset(text, start)..line_offset(text, end)])
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
    let glob_const = format!("const FEATURE_GLOB: &str = \"{}\";", runner.glob);
    if !code.contains(&glob_const) {
        violations.push(format!("{} does not declare {}", runner.source, glob_const));
    }
    if !code.contains("expand_feature_glob(FEATURE_GLOB)") {
        violations.push(format!(
            "{} does not discover features with expand_feature_glob(FEATURE_GLOB)",
            runner.source
        ));
    }
    if !code.contains(".fail_on_skipped()") {
        violations.push(format!(
            "{} does not call .fail_on_skipped()",
            runner.source
        ));
    }
    if !code.contains("execution_has_failed()") {
        violations.push(format!(
            "{} does not call execution_has_failed()",
            runner.source
        ));
    }
    if code.contains("PARANGSAE_CUCUMBER_FEATURES") {
        violations.push(format!(
            "{} reads PARANGSAE_CUCUMBER_FEATURES",
            runner.source
        ));
    }
    Ok(violations)
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

fn workflow_permission_result(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    for path in workflow_dir_files(root)? {
        let name = workflow_display(root, &path);
        let text = read(&path)?;
        if workflow_yaml(&path) {
            violations.extend(workflow_file_gaps(&name, &text));
        } else {
            violations.extend(hidden_workflow_gaps(&name, &text));
        }
    }
    violations.extend(cargo_config_gaps(root)?);
    let ci = read(&root.join(".github/workflows/ci.yml"))?;
    violations.extend(required_job_pin_gaps(&ci));
    if !guards_fetch_depth_is_zero(&ci) {
        violations.push("guards checkout fetch-depth is not 0".to_owned());
    }
    violations.extend(pinned_guards_steps(&ci));
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
        if (value.starts_with('{') || value.starts_with('[')) && !allowed_needs_list(key, value) {
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

fn collect_cargo_configs(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name == ".git" {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|err| format!("read {}: {err}", entry.path().display()))?;
        let path = entry.path();
        if file_type.is_symlink() {
            if name == ".cargo" || name.starts_with("config") {
                found.push(path);
            }
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        if name == ".cargo" {
            let children = std::fs::read_dir(&path)
                .map_err(|err| format!("read {}: {err}", path.display()))?;
            for child in children {
                let child = child.map_err(|err| format!("read {}: {err}", path.display()))?;
                let child_name = child.file_name();
                let child_name = child_name.to_string_lossy();
                if child_name.starts_with("config") {
                    found.push(child.path());
                }
            }
        }
        collect_cargo_configs(&path, found)?;
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
        BaselineChoice::Sha(sha) => sha,
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
    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", &format!("{base}^{{commit}}")])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|err| format!("git show: {err}"))?;
    if !commit.status.success() {
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
            if !keys.is_empty() {
                *count = count.saturating_add(1);
            }
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = read(&path)?;
        if text.lines().any(line_has_suppression) {
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
    use std::process::Command;

    use super::*;

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
        let ancestor = "a84508c798fa3e2f08f71d4be988da43ef1c5472";
        let missing = guard_reports(&root, Some(ancestor), false);
        assert_non_ratchet_ran(&missing)?;
        let ratchet = report_named(&missing, "ratchet")?;
        if ratchet.passed || !ratchet.detail.contains(ancestor) {
            return Err(format!("ancestor baseline: {}", ratchet.detail));
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
            || !ratchet.detail.contains("is not in this checkout")
            || ratchet.detail.contains("sha256")
        {
            return Err(format!("missing object: {}", ratchet.detail));
        }
        Ok(())
    }

    #[test]
    fn cargo_workspace_dir_in_ci_fails() -> Result<(), String> {
        let set = std::ffi::OsStr::new("/tmp/other");
        let empty = std::ffi::OsStr::new("");
        if crate::reject_cargo_workspace_dir_in_ci(true, Some(set)).is_ok() {
            return Err("CI override was accepted".to_owned());
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
        if !deny.contains("env -i") || !deny.contains("rustup which cargo") {
            return Err("deny job does not use env -i and rustup which cargo".to_owned());
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
}
