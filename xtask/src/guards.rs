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
//! `BOOTSTRAP_BASE`, `BOOTSTRAP_FLOORS_SHA256`, and `EXPECTED_GUARD_RUNS`
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
        if report.ran && !report.passed {
            if report.detail.is_empty() {
                errors.push(format!("guard {} failed", report.name));
            } else {
                errors.push(report.detail.clone());
            }
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

/// Reviewer-protected. Raw sha256 of `regressions/floors.toml` on the bootstrap path.
const BOOTSTRAP_FLOORS_SHA256: &str =
    "e9588b557ea0b3ce615fa0e469573279235647bcca7a93e2d3b5c91a75778528";

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
            reports.push(guard_failed("b2-counts", err.clone()));
            reports.push(guard_failed("cucumber-counts", err.clone()));
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
        cargo_workspace_dir_violations(),
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

fn cargo_workspace_dir_violations() -> Vec<String> {
    let in_ci = crate::ci_is_set(
        std::env::var_os("GITHUB_ACTIONS").as_deref(),
        std::env::var_os("CI").as_deref(),
    );
    match crate::reject_cargo_workspace_dir_in_ci(
        in_ci,
        std::env::var_os("CARGO_WORKSPACE_DIR").as_deref(),
    ) {
        Ok(()) => Vec::new(),
        Err(err) => vec![err],
    }
}

const GATE_CHECK_STEP: &str = concat!(
    "      - name: fail unless every needed job succeeded\n",
    "        env:\n",
    "          NEEDS: ${{ toJSON(needs) }}\n",
    "        run: python3 .github/workflows/gate_results.py\n",
);

const GUARDS_STEP_ENV: &str = concat!(
    "        env:\n",
    "          PARANGSAE_FLOOR_BASE: ${{ github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.event.before }}\n",
    "          PARANGSAE_ON_MAIN: ${{ github.ref == 'refs/heads/main' && '1' || '' }}\n",
);

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

const ALLOWED_TRIGGERS: &[&str] = &["push", "pull_request", "workflow_dispatch"];

const FORBIDDEN_ENV: &[&str] = &[
    "CARGO_WORKSPACE_DIR",
    "GITHUB_ACTIONS",
    "CI",
    "PARANGSAE_FLOOR_BASE",
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
    Ok(gaps)
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
    for (idx, ch) in line.char_indices() {
        if let Some(open) = quote {
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
    let ci = read(&root.join(".github/workflows/ci.yml"))?;
    if !guards_fetch_depth_is_zero(&ci) {
        violations.push("guards checkout fetch-depth is not 0".to_owned());
    }
    if !ci.contains(GUARDS_STEP_ENV) {
        violations.push("guards step env does not match".to_owned());
    }
    Ok(violations)
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
    let mut gaps = Vec::new();
    if !permissions_are_contents_read(text) {
        gaps.push(format!("{name} permissions are not contents: read"));
    }
    gaps.extend(job_write_permission_gaps(name, text));
    gaps.extend(trigger_gaps(name, text));
    gaps.extend(forbidden_env_gaps(name, text));
    gaps.extend(rustc_pin_gaps(name, text));
    gaps.extend(run_injection_gaps(name, text));
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

fn forbidden_env_gaps(name: &str, text: &str) -> Vec<String> {
    let stripped = match text.find(GUARDS_STEP_ENV) {
        Some(idx) => {
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..idx]);
            out.push_str(&text[idx + GUARDS_STEP_ENV.len()..]);
            out
        }
        None => text.to_owned(),
    };
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
                    if FORBIDDEN_ENV.contains(&env_key) {
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

fn flow_env_gaps(name: &str, rest: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    let inner = rest.trim().trim_matches(|ch| ch == '{' || ch == '}');
    for item in inner.split(',') {
        let key = item
            .split_once(':')
            .map(|(key, _)| key.trim())
            .unwrap_or("");
        if FORBIDDEN_ENV.contains(&key) {
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
        if block.contains("dtolnay/rust-toolchain@") && !block.contains(RUSTC_198_STEP) {
            gaps.push(format!("{name} job {job} does not assert rustc 1.98.0"));
        }
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
        let Some(rest) = trimmed.strip_prefix("run:") else {
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
        for uses in uses_values(line) {
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
        assert_non_ratchet_ran(&reports)?;
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
        if gaps != ["action actions/checkout@v4 is not pinned to a sha".to_owned()] {
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
        assert_non_ratchet_ran(&reports)?;
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
        if flow_gaps != ["action actions/checkout@v4 is not pinned to a sha".to_owned()] {
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
        if !ci.contains(GUARDS_STEP_ENV) {
            return Err("guards step env pin".to_owned());
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
