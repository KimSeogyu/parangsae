//! Guards against a green run that did not exercise the check.
//!
//! Feature ownership, TLC config coverage, distinct-state floors, Kani harness
//! names, B2 measurements, and the ratchet in `regressions/floors.toml`.
//! The baseline commit comes from `PARANGSAE_FLOOR_BASE`. A missing file,
//! an unreadable sha, or a direct push to main with no floors file fails.
//! GitHub's all-zero `before` on a branch's first push is not a skip: the
//! comparison unshallows a depth-1 checkout, fetches
//! `refs/remotes/origin/main`, and uses `merge-base` with that ref. A failed
//! fetch, a failed merge-base, or a missing file still fails. There is no
//! fallback to the branch tip or to no baseline.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Floors {
    values: BTreeMap<String, u64>,
    kani_names: Vec<String>,
    invariants: BTreeMap<String, Vec<String>>,
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
        other => Err(format!("unknown guards command {other}")),
    }
}

fn check_workspace(root: &Path) -> Result<(), String> {
    let base = resolve_baseline(root)?;
    let mut violations = feature_ownership(root, root, true)?;
    violations.extend(tla_models(root)?);
    let floors = load_floors(&root.join("regressions/floors.toml"))?;
    violations.extend(required_floor_keys(&floors));
    violations.extend(ratchet(&floors, &base));
    violations.extend(b2_violations(root, &floors)?);
    violations.extend(cucumber_count_violations(root, &floors)?);
    violations.extend(ci_gate_gaps(&read(
        &root.join(".github/workflows/ci.yml"),
    )?)?);
    finish(Ok(violations))
}

const GATE_CHECK_STEP: &str = concat!(
    "      - name: fail unless every needed job succeeded\n",
    "        env:\n",
    "          NEEDS: ${{ toJSON(needs) }}\n",
    "        run: python3 .github/workflows/gate_results.py\n",
);

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
    Ok(gaps)
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
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| job_id(line).as_deref() == Some("gate"))
        .ok_or_else(|| "ci.yml has no gate job".to_owned())?;
    let mut end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start + 1) {
        if job_id(line).is_some() || is_top_level(line) {
            end = idx;
            break;
        }
    }
    Ok(&text[line_offset(text, start)..line_offset(text, end)])
}

fn gate_check_step(block: &str) -> &str {
    let header = "      - name: fail unless every needed job succeeded\n";
    let Some(start) = block.find(header) else {
        return "";
    };
    let rest = &block[start..];
    let end = rest.find("\n      - ").map_or(rest.len(), |idx| idx + 1);
    &rest[..end]
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

fn resolve_baseline(root: &Path) -> Result<Floors, String> {
    let on_main = env_flag("PARANGSAE_ON_MAIN");
    let env_sha = std::env::var("PARANGSAE_FLOOR_BASE").ok();
    let sha = match choose_baseline(env_sha.as_deref(), on_main) {
        BaselineChoice::DirectPush => {
            return Err("direct push has no floor baseline".to_owned());
        }
        BaselineChoice::Unreadable => return Err("floor baseline is unreadable".to_owned()),
        BaselineChoice::MergeBase => merge_base_origin_main(root)?,
        BaselineChoice::Sha(sha) => sha,
    };
    if baseline_sha_unreadable(&sha) {
        return Err("floor baseline is unreadable".to_owned());
    }
    let text = git_show(root, &sha, "regressions/floors.toml")?;
    baseline_from_show(on_main, &sha, text.as_deref())
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
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", &format!("{base}:{path}")])
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
        Ok(())
    }

    #[test]
    fn a_job_omitted_from_gate_needs_fails() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let omitted = ci_gate_gaps(&read(&root.join("fixtures/guards/ci/omitted-job.yml"))?)?;
        if !omitted
            .iter()
            .any(|line| line == "ci job clippy is missing from gate.needs")
        {
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
        let text = "\
name: ci
on: push
jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - run: echo fmt
  gate:
    if: always()
    needs: [fmt, phantom]
    runs-on: ubuntu-latest
    steps:
      - name: fail unless every needed job succeeded
        env:
          NEEDS: ${{ toJSON(needs) }}
        run: python3 .github/workflows/gate_results.py
";
        let gaps = ci_gate_gaps(text)?;
        if gaps != ["gate.needs lists phantom which is not a ci job".to_owned()] {
            return Err(format!("{gaps:?}"));
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
