//! Dependency, path, generation, and the enforce entry point.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use cargo_metadata::{CargoOpt, MetadataCommand};

use crate::effects::{
    dep_effect, effect_diags, effects_in_source, float_diags, float_tokens, package_rs_files, Hit,
};
use crate::error::{
    explain_rule, Diagnostic, ToolError, DEP_DIRECT, DEP_TRANSITIVE, GEN, PATH, PATH_AMBIG,
};
use crate::json::{canonical_json, sha256_hex};
use crate::parse::{read_policy, Policy};
use crate::scan::{rel_path, scan_ast};

pub struct EnforceOptions<'a> {
    pub manifest: &'a Path,
    pub diff: Option<&'a str>,
    pub as_bot: Option<&'a str>,
    pub check_generated: bool,
}

pub fn generate(policy_path: &Path, out: &Path, check: bool) -> Result<String, ToolError> {
    let (policy, text) = read_policy(policy_path)?;
    let json = canonical_json(&policy, &sha256_hex(text.as_bytes()));
    if check {
        let current = std::fs::read_to_string(out)
            .map_err(|err| ToolError::tool(format!("read {}: {err}", out.display())))?;
        if current != json {
            return Err(ToolError {
                rule_id: GEN,
                message: format!(
                    "{} is not the bytes generated from {} (drift or a hand edit)",
                    out.display(),
                    policy_path.display()
                ),
                file: out.display().to_string(),
                line: 1,
            });
        }
        return Ok(json);
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| ToolError::tool(format!("create {}: {err}", parent.display())))?;
    }
    std::fs::write(out, &json)
        .map_err(|err| ToolError::tool(format!("write {}: {err}", out.display())))?;
    Ok(json)
}

pub fn enforce(opts: &EnforceOptions<'_>) -> Result<Vec<Diagnostic>, ToolError> {
    let metadata = metadata(opts.manifest)?;
    let root = PathBuf::from(metadata.workspace_root.as_str());
    let policy_path = root.join("spec/arch/ArchitecturePolicy.tla");
    let generated = root.join("spec/arch/generated/policy.json");
    let (mut policy, text) = read_policy(&policy_path)?;
    policy.file = rel_path(&root, &policy_path);
    let mut diags = Vec::new();
    if opts.check_generated {
        if let Some(diag) = generated_diag(&policy, &text, &generated, &root) {
            diags.push(diag);
        }
    }
    diags.extend(dep_diags(&metadata, &policy, &root)?);
    let targets = target_roots(&metadata, &root);
    scan_ast(&root, &targets, &policy, &mut diags)?;
    diags.extend(package_effects(&metadata, &policy, &root)?);
    if let Some(range) = opts.diff {
        match opts.as_bot {
            Some(bot) => diags.extend(path_diags(&root, &policy, range, bot)?),
            None => {
                let files = diff_names(&root, range)?;
                diags.extend(unowned_path_diags(&policy, &files));
            }
        }
    }
    diags.sort_by(|a, b| {
        (a.rule_id, &a.file, a.line, &a.symbol).cmp(&(b.rule_id, &b.file, b.line, &b.symbol))
    });
    diags.dedup();
    Ok(diags)
}

pub fn inspect(manifest: &Path, symbol: &str) -> Result<String, ToolError> {
    let metadata = metadata(manifest)?;
    let root = PathBuf::from(metadata.workspace_root.as_str());
    let policy_path = root.join("spec/arch/ArchitecturePolicy.tla");
    let (mut policy, _) = read_policy(&policy_path)?;
    policy.file = rel_path(&root, &policy_path);
    if let Some((ty, field)) = symbol.split_once('.') {
        let key = format!("{ty}.{field}");
        let rows: Vec<_> = policy
            .writers
            .iter()
            .filter(|pair| pair.right == key)
            .collect();
        if rows.is_empty() {
            return Err(ToolError::tool(format!(
                "{symbol} is not in AllowedStateWriters"
            )));
        }
        let mut out = format!("state {key}\n");
        for row in rows {
            out.push_str(&format!(
                "  writer: {} ({}:{})\n",
                row.left, policy.file, row.line
            ));
        }
        return Ok(out);
    }
    if policy.capabilities.contains_key(symbol) {
        let callers: Vec<_> = policy
            .capability_callers
            .iter()
            .filter(|pair| pair.right == symbol)
            .map(|pair| format!("{} ({}:{})", pair.left, policy.file, pair.line))
            .collect();
        return Ok(format!(
            "capability {symbol}\n  callers: {}\n",
            if callers.is_empty() {
                "(none)".to_owned()
            } else {
                callers.join(", ")
            }
        ));
    }
    if !policy.modules.contains_key(symbol) {
        return Err(ToolError::tool(format!(
            "{symbol} is not a module, Type.field, or capability in the policy"
        )));
    }
    let mut out = format!("crate {symbol}\n");
    out.push_str("  dependencies:\n");
    for pair in policy
        .dependencies
        .iter()
        .filter(|pair| pair.left == symbol)
    {
        out.push_str(&format!(
            "    {} -> {} ({}:{})\n",
            pair.left, pair.right, policy.file, pair.line
        ));
    }
    out.push_str("  state writers:\n");
    for pair in policy.writers.iter().filter(|pair| pair.left == symbol) {
        out.push_str(&format!(
            "    {} ({}:{})\n",
            pair.right, policy.file, pair.line
        ));
    }
    out.push_str("  capabilities:\n");
    for pair in policy
        .capability_callers
        .iter()
        .filter(|pair| pair.left == symbol)
    {
        out.push_str(&format!(
            "    {} ({}:{})\n",
            pair.right, policy.file, pair.line
        ));
    }
    out.push_str("  effects:\n");
    for pair in policy
        .allowed_effects
        .iter()
        .filter(|pair| pair.left == symbol)
    {
        out.push_str(&format!(
            "    {} ({}:{})\n",
            pair.right, policy.file, pair.line
        ));
    }
    out.push_str(&format!(
        "  float: {}\n",
        if policy.float_allowed.contains_key(symbol) {
            "allowed"
        } else {
            "denied"
        }
    ));
    out.push_str("  path owners:\n");
    for pair in &policy.path_owners {
        out.push_str(&format!(
            "    {} {} ({}:{})\n",
            pair.left, pair.right, policy.file, pair.line
        ));
    }
    Ok(out)
}

pub fn impact(manifest: &Path, module: &str, capability: &str) -> Result<String, ToolError> {
    let metadata = metadata(manifest)?;
    let root = PathBuf::from(metadata.workspace_root.as_str());
    let policy_path = root.join("spec/arch/ArchitecturePolicy.tla");
    let (mut policy, _) = read_policy(&policy_path)?;
    policy.file = rel_path(&root, &policy_path);
    if !policy.capabilities.contains_key(capability) {
        return Err(ToolError::tool(format!(
            "{capability} is not in Capabilities"
        )));
    }
    let allowed = policy
        .capability_callers
        .iter()
        .filter(|pair| pair.right == capability)
        .map(|pair| pair.left.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let yes = policy
        .capability_callers
        .iter()
        .any(|pair| pair.left == module && pair.right == capability);
    let mut diags = Vec::new();
    let targets = target_roots(&metadata, &root);
    let _reached = scan_ast(&root, &targets, &policy, &mut diags)?;
    let calls: Vec<_> = diags
        .iter()
        .filter(|diag| diag.rule_id == crate::error::CALL && diag.symbol.contains(capability))
        .map(|diag| format!("{}:{}", diag.file, diag.line))
        .collect();
    Ok(format!(
        "capability {capability}\n  module {module}: {}\n  allowed callers: {}\n  violating call sites: {}\n",
        if yes { "allowed" } else { "not allowed" },
        if allowed.is_empty() { "(none)".to_owned() } else { allowed },
        if calls.is_empty() { "(none)".to_owned() } else { calls.join(", ") }
    ))
}

pub fn explain(rule: &str) -> Result<&'static str, ToolError> {
    explain_rule(rule).ok_or_else(|| ToolError::tool(format!("unknown rule id {rule}")))
}

pub fn validate(manifest: &Path, jar: Option<&Path>) -> Result<String, ToolError> {
    let metadata = metadata(manifest)?;
    let root = PathBuf::from(metadata.workspace_root.as_str());
    let jar = resolve_jar(&root, jar)?;
    let dir = root.join("spec/arch");
    let sany = Command::new("java")
        .arg("-cp")
        .arg(&jar)
        .arg("tla2sany.SANY")
        .arg("ArchitecturePolicy.tla")
        .current_dir(&dir)
        .output()
        .map_err(|err| ToolError::tool(format!("run SANY: {err}")))?;
    if !sany.status.success() {
        return Err(ToolError::tool(format!(
            "SANY failed\n{}",
            tail(&sany.stdout, &sany.stderr)
        )));
    }
    let metadir = std::env::temp_dir().join(format!("tlc-arch-{}", std::process::id()));
    let tlc = Command::new("java")
        .args(["-XX:+UseParallelGC", "-Xmx512m", "-cp"])
        .arg(&jar)
        .args(["tlc2.TLC", "-workers", "1", "-metadir"])
        .arg(&metadir)
        .args([
            "-config",
            "ArchitecturePolicy.cfg",
            "ArchitecturePolicy.tla",
        ])
        .current_dir(&dir)
        .output()
        .map_err(|err| ToolError::tool(format!("run TLC: {err}")))?;
    let combined = tail(&tlc.stdout, &tlc.stderr);
    if !tlc.status.success() {
        return Err(ToolError::tool(format!("TLC failed\n{combined}")));
    }
    Ok(format!("SANY ok\n{combined}"))
}

fn resolve_jar(root: &Path, flag: Option<&Path>) -> Result<PathBuf, ToolError> {
    let raw = if let Some(path) = flag {
        path.to_path_buf()
    } else if let Ok(env) = std::env::var("TLA2TOOLS_JAR") {
        PathBuf::from(env)
    } else {
        PathBuf::from("specs/tla/tla2tools.jar")
    };
    let absolute = absolutize_jar(root, &raw);
    if absolute.is_file() {
        return Ok(absolute);
    }
    Err(ToolError::tool(format!(
        "tla2tools.jar not found at {}. Pass --jar, set TLA2TOOLS_JAR, or download the pinned jar into specs/tla/tla2tools.jar",
        absolute.display()
    )))
}

/// A relative jar path is rooted at the workspace, not at `spec/arch` where
/// SANY and TLC run.
pub fn absolutize_jar(root: &Path, raw: &Path) -> PathBuf {
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    }
}

fn tail(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).to_string();
    let err = String::from_utf8_lossy(stderr);
    if !err.is_empty() {
        text.push('\n');
        text.push_str(&err);
    }
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(40);
    lines[start..].join("\n")
}

fn metadata(manifest: &Path) -> Result<cargo_metadata::Metadata, ToolError> {
    let mut cmd = MetadataCommand::new();
    cmd.manifest_path(manifest);
    cmd.features(CargoOpt::AllFeatures);
    cmd.other_options(["--offline".to_owned()]);
    match cmd.exec() {
        Ok(metadata) => Ok(metadata),
        Err(err) => Err(ToolError::tool(format!(
            "cargo metadata --offline --all-features --manifest-path {}: {err}",
            manifest.display()
        ))),
    }
}

fn generated_diag(
    policy: &Policy,
    text: &str,
    generated: &Path,
    root: &Path,
) -> Option<Diagnostic> {
    let expected = canonical_json(policy, &sha256_hex(text.as_bytes()));
    let current = std::fs::read_to_string(generated).unwrap_or_default();
    if current == expected {
        return None;
    }
    Some(Diagnostic {
        rule_id: GEN,
        message: "generated policy.json does not match ArchitecturePolicy.tla".to_owned(),
        symbol: rel_path(root, generated),
        file: rel_path(root, generated),
        line: 1,
        allowed: "byte-identical regeneration, including spec_sha256".to_owned(),
        spec_file: policy.file.clone(),
        spec_line: 1,
    })
}

fn dep_diags(
    metadata: &cargo_metadata::Metadata,
    policy: &Policy,
    root: &Path,
) -> Result<Vec<Diagnostic>, ToolError> {
    let members: BTreeSet<_> = metadata.workspace_members.iter().cloned().collect();
    let mut edges = Vec::new();
    let mut seen = BTreeSet::new();
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // Edges come from the manifest, not the resolve graph. An optional dep
    // that is off by default, a dev or build dep, and a target-specific dep
    // are still edges. `--all-features` is requested above so the package
    // list includes path crates that cargo only loads for a feature.
    for pkg in &metadata.packages {
        if pkg.source.is_some() {
            continue;
        }
        graph.entry(pkg.name.to_string()).or_default();
        let manifest = std::fs::read_to_string(pkg.manifest_path.as_str()).unwrap_or_default();
        for dep in &pkg.dependencies {
            if !declared_local(dep) {
                continue;
            }
            let target = find_local(&metadata.packages, &members, &dep.name);
            let to = target
                .map(|target| target.name.clone())
                .unwrap_or_else(|| dep.name.clone());
            let from = pkg.name.to_string();
            if !seen.insert((from.clone(), to.clone())) {
                continue;
            }
            let outside = match target {
                Some(target) => !members.contains(&pkg.id) || !members.contains(&target.id),
                None => true,
            };
            graph.entry(from.clone()).or_default().insert(to.clone());
            let key = dep.rename.as_deref().unwrap_or(dep.name.as_str());
            edges.push(Edge {
                from,
                to,
                file: rel_path(root, Path::new(pkg.manifest_path.as_str())),
                line: manifest_line(&manifest, key),
                outside,
            });
        }
    }
    let allowed: BTreeSet<(String, String)> = policy
        .dependencies
        .iter()
        .map(|pair| (pair.left.clone(), pair.right.clone()))
        .collect();
    let mut diags = Vec::new();
    for edge in &edges {
        if edge.outside {
            diags.push(Diagnostic {
                rule_id: DEP_DIRECT,
                message: format!(
                    "path dependency {} -> {} is an unknown crate outside the workspace",
                    edge.from, edge.to
                ),
                symbol: format!("{} -> {}", edge.from, edge.to),
                file: edge.file.clone(),
                line: edge.line,
                allowed: allowed_from(&allowed, &edge.from),
                spec_file: policy.file.clone(),
                spec_line: policy.line_of("AllowedDependencies"),
            });
            continue;
        }
        if allowed.contains(&(edge.from.clone(), edge.to.clone())) {
            continue;
        }
        diags.push(Diagnostic {
            rule_id: DEP_DIRECT,
            message: format!(
                "workspace dependency {} -> {} is not allowed",
                edge.from, edge.to
            ),
            symbol: format!("{} -> {}", edge.from, edge.to),
            file: edge.file.clone(),
            line: edge.line,
            allowed: allowed_from(&allowed, &edge.from),
            spec_file: policy.file.clone(),
            spec_line: policy.line_of("AllowedDependencies"),
        });
    }
    let mut paths = BTreeSet::new();
    for start in graph.keys() {
        let mut path = vec![start.clone()];
        dfs(&graph, start, &mut path, &mut paths);
    }
    for path in paths {
        if path.len() < 3 {
            continue;
        }
        let mut bad = None;
        for pair in path.windows(2) {
            if !allowed.contains(&(pair[0].clone(), pair[1].clone())) {
                bad = Some((pair[0].clone(), pair[1].clone()));
                break;
            }
        }
        let Some((from, to)) = bad else {
            continue;
        };
        let edge = edges.iter().find(|edge| edge.from == from && edge.to == to);
        diags.push(Diagnostic {
            rule_id: DEP_TRANSITIVE,
            message: format!(
                "workspace path {} goes through an unlisted edge",
                path.join(" -> ")
            ),
            symbol: path.join(" -> "),
            file: edge.map(|edge| edge.file.clone()).unwrap_or_default(),
            line: edge.map(|edge| edge.line).unwrap_or(1),
            allowed: allowed_from(&allowed, &from),
            spec_file: policy.file.clone(),
            spec_line: policy.line_of("AllowedDependencies"),
        });
    }
    Ok(diags)
}

fn declared_local(dep: &cargo_metadata::Dependency) -> bool {
    dep.path.is_some() || dep.source.is_none()
}

fn find_local<'a>(
    packages: &'a [cargo_metadata::Package],
    members: &BTreeSet<cargo_metadata::PackageId>,
    name: &str,
) -> Option<&'a cargo_metadata::Package> {
    let mut fallback = None;
    for pkg in packages {
        if pkg.source.is_some() || pkg.name != name {
            continue;
        }
        if members.contains(&pkg.id) {
            return Some(pkg);
        }
        fallback = Some(pkg);
    }
    fallback
}

struct Edge {
    from: String,
    to: String,
    file: String,
    line: u32,
    outside: bool,
}

fn allowed_from(allowed: &BTreeSet<(String, String)>, from: &str) -> String {
    let rows: Vec<String> = allowed
        .iter()
        .filter(|(left, _)| left == from)
        .map(|(left, right)| format!("{left} -> {right}"))
        .collect();
    if rows.is_empty() {
        format!("(no outgoing edge from {from})")
    } else {
        rows.join(", ")
    }
}

fn dfs(
    graph: &BTreeMap<String, BTreeSet<String>>,
    current: &str,
    path: &mut Vec<String>,
    out: &mut BTreeSet<Vec<String>>,
) {
    let Some(nexts) = graph.get(current) else {
        return;
    };
    for next in nexts {
        if path.contains(next) {
            continue;
        }
        path.push(next.clone());
        if path.len() >= 3 {
            out.insert(path.clone());
        }
        dfs(graph, next, path, out);
        path.pop();
    }
}

fn manifest_line(text: &str, dep: &str) -> u32 {
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("{dep} "))
            || trimmed.starts_with(&format!("{dep}="))
            || trimmed.starts_with(&format!("name = \"{dep}\""))
        {
            return index as u32 + 1;
        }
    }
    1
}

fn target_roots(
    metadata: &cargo_metadata::Metadata,
    _root: &Path,
) -> Vec<(String, String, String, bool, PathBuf)> {
    let members: BTreeSet<_> = metadata.workspace_members.iter().cloned().collect();
    let mut out = Vec::new();
    for pkg in &metadata.packages {
        if !members.contains(&pkg.id) {
            continue;
        }
        let lib_name = pkg
            .targets
            .iter()
            .find(|target| target.kind.iter().any(|kind| kind.to_string() == "lib"))
            .map(|target| target.name.clone())
            .unwrap_or_else(|| pkg.name.replace('-', "_"));
        for target in &pkg.targets {
            let kind = target
                .kind
                .iter()
                .map(|k| k.to_string())
                .collect::<Vec<_>>();
            if kind.iter().any(|k| k == "custom-build") {
                continue;
            }
            let is_lib = kind.iter().any(|k| k == "lib");
            out.push((
                pkg.name.to_string(),
                target.name.clone(),
                if is_lib {
                    target.name.clone()
                } else {
                    lib_name.clone()
                },
                is_lib,
                PathBuf::from(target.src_path.as_str()),
            ));
        }
    }
    out
}

fn package_effects(
    metadata: &cargo_metadata::Metadata,
    policy: &Policy,
    root: &Path,
) -> Result<Vec<Diagnostic>, ToolError> {
    let mut diags = Vec::new();
    let mut float_hits = Vec::new();
    for pkg in &metadata.packages {
        if pkg.source.is_some() {
            continue;
        }
        let mut hits = Vec::new();
        for dep in &pkg.dependencies {
            if let Some(effect) = dep_effect(&dep.name, &dep.features, dep.uses_default_features) {
                let manifest = rel_path(root, Path::new(pkg.manifest_path.as_str()));
                let text = std::fs::read_to_string(pkg.manifest_path.as_str()).unwrap_or_default();
                hits.push(Hit {
                    effect,
                    symbol: dep.name.to_string(),
                    file: manifest,
                    line: manifest_line(&text, &dep.name),
                });
            }
        }
        let files = package_rs_files(root, Path::new(pkg.manifest_path.as_str()));
        for (rel, text) in &files {
            hits.extend(effects_in_source(rel, text)?);
            for hit in float_tokens(rel, text) {
                float_hits.push((pkg.name.to_string(), hit));
            }
        }
        hits.sort_by(|a, b| (&a.file, a.line, a.effect).cmp(&(&b.file, b.line, b.effect)));
        diags.extend(effect_diags(policy, &pkg.name, &hits));
    }
    diags.extend(float_diags(policy, &float_hits));
    Ok(diags)
}

pub fn diff_names(root: &Path, range: &str) -> Result<Vec<String>, ToolError> {
    let output = Command::new("git")
        .args(["diff", "--name-only", "--no-renames", range])
        .current_dir(root)
        .output()
        .map_err(|err| ToolError::tool(format!("git diff: {err}")))?;
    if !output.status.success() {
        return Err(ToolError::tool(format!(
            "git diff --no-renames {range} failed\n{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect())
}

pub fn path_diags(
    root: &Path,
    policy: &Policy,
    range: &str,
    bot: &str,
) -> Result<Vec<Diagnostic>, ToolError> {
    if !policy.owners.contains_key(bot) {
        return Err(ToolError::tool(format!("{bot} is not in Owners")));
    }
    let files = diff_names(root, range)?;
    Ok(path_diags_for(policy, &files, bot))
}

pub fn unowned_path_diags(policy: &Policy, files: &[String]) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for file in files {
        match owner_of(policy, file) {
            OwnerMatch::One(_) => {}
            OwnerMatch::None => diags.push(Diagnostic {
                rule_id: PATH,
                message: format!("{file} has no PathOwners entry"),
                symbol: file.clone(),
                file: file.clone(),
                line: 1,
                allowed: "(one owner)".to_owned(),
                spec_file: policy.file.clone(),
                spec_line: policy.line_of("PathOwners"),
            }),
            OwnerMatch::Ambiguous(left, right) => diags.push(Diagnostic {
                rule_id: PATH_AMBIG,
                message: format!("{file} matches {left} and {right}"),
                symbol: file.clone(),
                file: file.clone(),
                line: 1,
                allowed: format!("{left}, {right}"),
                spec_file: policy.file.clone(),
                spec_line: policy.line_of("PathOwners"),
            }),
        }
    }
    diags
}

pub fn path_diags_for(policy: &Policy, files: &[String], bot: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for file in files {
        match owner_of(policy, file) {
            OwnerMatch::One(owner) if owner == bot => {}
            OwnerMatch::One(owner) => {
                let spec_line = policy
                    .path_owners
                    .iter()
                    .find(|pair| pair.right == owner && glob_match(&pair.left, file))
                    .map(|pair| pair.line)
                    .unwrap_or_else(|| policy.line_of("PathOwners"));
                diags.push(Diagnostic {
                    rule_id: PATH,
                    message: format!("{file} is owned by {owner}"),
                    symbol: file.clone(),
                    file: file.clone(),
                    line: 1,
                    allowed: globs_for(policy, bot),
                    spec_file: policy.file.clone(),
                    spec_line,
                });
            }
            OwnerMatch::None => diags.push(Diagnostic {
                rule_id: PATH,
                message: format!("{file} has no PathOwners entry"),
                symbol: file.clone(),
                file: file.clone(),
                line: 1,
                allowed: globs_for(policy, bot),
                spec_file: policy.file.clone(),
                spec_line: policy.line_of("PathOwners"),
            }),
            OwnerMatch::Ambiguous(left, right) => diags.push(Diagnostic {
                rule_id: PATH_AMBIG,
                message: format!("{file} matches {left} and {right}"),
                symbol: file.clone(),
                file: file.clone(),
                line: 1,
                allowed: format!("{left}, {right}"),
                spec_file: policy.file.clone(),
                spec_line: policy.line_of("PathOwners"),
            }),
        }
    }
    diags
}

fn globs_for(policy: &Policy, bot: &str) -> String {
    let rows: Vec<_> = policy
        .path_owners
        .iter()
        .filter(|pair| pair.right == bot)
        .map(|pair| pair.left.as_str())
        .collect();
    if rows.is_empty() {
        "(none)".to_owned()
    } else {
        rows.join(", ")
    }
}

enum OwnerMatch<'a> {
    One(&'a str),
    None,
    Ambiguous(&'a str, &'a str),
}

fn owner_of<'a>(policy: &'a Policy, file: &str) -> OwnerMatch<'a> {
    let mut best: Option<(&str, &str, usize)> = None;
    let mut tie = false;
    let mut other = "";
    for pair in &policy.path_owners {
        if !glob_match(&pair.left, file) {
            continue;
        }
        let score = specificity(&pair.left);
        match best {
            None => best = Some((pair.right.as_str(), pair.left.as_str(), score)),
            Some((_owner, _, prev)) if score > prev => {
                best = Some((pair.right.as_str(), pair.left.as_str(), score));
                tie = false;
            }
            Some((owner, _, prev)) if score == prev && owner != pair.right => {
                tie = true;
                other = pair.right.as_str();
            }
            _ => {}
        }
    }
    if tie {
        if let Some((owner, _, _)) = best {
            return OwnerMatch::Ambiguous(owner, other);
        }
    }
    match best {
        Some((owner, _, _)) => OwnerMatch::One(owner),
        None => OwnerMatch::None,
    }
}

fn specificity(glob: &str) -> usize {
    glob.chars().filter(|c| *c != '*').count()
}

pub fn glob_match(glob: &str, path: &str) -> bool {
    let pat: Vec<&str> = glob.split('/').filter(|s| !s.is_empty()).collect();
    let text: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match_segs(&pat, &text)
}

fn match_segs(pat: &[&str], text: &[&str]) -> bool {
    if pat.is_empty() {
        return text.is_empty();
    }
    if pat[0] == "**" {
        if match_segs(&pat[1..], text) {
            return true;
        }
        if text.is_empty() {
            return false;
        }
        return match_segs(pat, &text[1..]);
    }
    if text.is_empty() {
        return false;
    }
    seg_match(pat[0], text[0]) && match_segs(&pat[1..], &text[1..])
}

fn seg_match(pat: &str, seg: &str) -> bool {
    let pb = pat.as_bytes();
    let sb = seg.as_bytes();
    fn rec(pat: &[u8], seg: &[u8]) -> bool {
        if pat.is_empty() {
            return seg.is_empty();
        }
        if pat[0] == b'*' {
            if rec(&pat[1..], seg) {
                return true;
            }
            return !seg.is_empty() && rec(pat, &seg[1..]);
        }
        !seg.is_empty() && pat[0] == seg[0] && rec(&pat[1..], &seg[1..])
    }
    rec(pb, sb)
}

pub fn every_tracked_has_owner(root: &Path, policy: &Policy) -> Result<Vec<String>, ToolError> {
    let output = Command::new("git")
        .args(["ls-files"])
        .current_dir(root)
        .output()
        .map_err(|err| ToolError::tool(format!("git ls-files: {err}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut missing = Vec::new();
    for line in text.lines() {
        if !matches!(owner_of(policy, line), OwnerMatch::One(_)) {
            missing.push(line.to_owned());
        }
    }
    Ok(missing)
}
