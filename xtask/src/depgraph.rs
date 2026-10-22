//! Allowed-edge check over `cargo metadata --locked --format-version 1`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Policy {
    #[serde(rename = "crate", default)]
    crates: Vec<CratePolicy>,
    /// Crates that are not workspace members yet. Edges here are not checked.
    /// A planned name that is also a member is a collision.
    #[serde(default)]
    planned: Vec<PlannedCrate>,
    #[serde(default)]
    strategy: Vec<StrategyRule>,
}

#[derive(Debug, Deserialize)]
struct PlannedCrate {
    name: String,
}

#[derive(Debug, Deserialize)]
struct CratePolicy {
    name: String,
    #[serde(default, rename = "edge")]
    edges: Vec<EdgePolicy>,
}

#[derive(Debug, Deserialize)]
struct EdgePolicy {
    name: String,
    kind: String,
    source: String,
    default_features: bool,
    #[serde(default)]
    features: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
struct StrategyRule {
    path_prefix: String,
    internal: BTreeSet<String>,
    forbid_build_script: bool,
    forbid_proc_macro: bool,
}

#[derive(Debug, Deserialize)]
struct Meta {
    packages: Vec<Pkg>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
    #[serde(default)]
    resolve: Resolve,
}

#[derive(Debug, Default, Deserialize)]
struct Resolve {
    #[serde(default)]
    nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    #[serde(default)]
    deps: Vec<ResolvedDep>,
}

#[derive(Debug, Deserialize)]
struct ResolvedDep {
    pkg: String,
    #[serde(default)]
    dep_kinds: Vec<DepKind>,
}

#[derive(Debug, Deserialize)]
struct DepKind {
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Pkg {
    name: String,
    id: String,
    manifest_path: PathBuf,
    #[serde(default)]
    dependencies: Vec<Dep>,
    #[serde(default)]
    targets: Vec<Target>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Dep {
    name: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    rename: Option<String>,
    #[serde(default)]
    uses_default_features: bool,
    #[serde(default)]
    features: BTreeSet<String>,
    #[serde(default)]
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Target {
    kind: Vec<String>,
    #[serde(default)]
    crate_types: Vec<String>,
}

pub fn workspace(root: &Path) -> Result<Vec<String>, String> {
    let meta = load_metadata(root)?;
    let policy = load_policy(&root.join("policy/deps.toml"))?;
    Ok(check(&meta, &policy))
}

fn load_policy(path: &Path) -> Result<Policy, String> {
    let text =
        std::fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    toml::from_str(&text).map_err(|err| format!("parse {}: {err}", path.display()))
}

fn load_metadata(root: &Path) -> Result<Meta, String> {
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--locked")
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
    serde_json::from_slice(&output.stdout).map_err(|err| format!("metadata json: {err}"))
}

fn check(meta: &Meta, policy: &Policy) -> Vec<String> {
    let members: BTreeSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    let member_names = member_names(meta, &members);
    let mut violations = Vec::new();
    violations.extend(policy_membership(policy, &member_names));
    for pkg in &meta.packages {
        violations.extend(source_violation(pkg, &members));
        if !members.contains(pkg.id.as_str()) {
            continue;
        }
        let rel = relative_manifest(&meta.workspace_root, &pkg.manifest_path);
        if let Some(rule) = strategy_rule(policy, rel.as_deref()) {
            violations.extend(strategy_violations(pkg, rule, &rel));
        } else {
            violations.extend(crate_violations(pkg, policy));
        }
    }
    violations.extend(transitive_bans(meta, &members));
    violations
}

fn member_names<'a>(meta: &'a Meta, members: &BTreeSet<&str>) -> BTreeSet<&'a str> {
    meta.packages
        .iter()
        .filter(|pkg| members.contains(pkg.id.as_str()))
        .map(|pkg| pkg.name.as_str())
        .collect()
}

fn policy_membership(policy: &Policy, member_names: &BTreeSet<&str>) -> Vec<String> {
    let mut violations = Vec::new();
    for item in &policy.crates {
        if !member_names.contains(item.name.as_str()) {
            violations.push(format!(
                "policy lists {} but it is not a workspace member",
                item.name
            ));
        }
    }
    for item in &policy.planned {
        if member_names.contains(item.name.as_str()) {
            violations.push(format!(
                "{} is listed as planned and is a workspace member",
                item.name
            ));
        }
    }
    violations
}

fn source_violation(pkg: &Pkg, members: &BTreeSet<&str>) -> Option<String> {
    match &pkg.source {
        None => {
            if members.contains(pkg.id.as_str()) {
                None
            } else {
                Some(format!(
                    "path package {} is not a workspace member",
                    pkg.name
                ))
            }
        }
        Some(source) if is_crates_io(source) => None,
        Some(source) if source.starts_with("git+") => {
            Some(format!("git source is forbidden: {source}"))
        }
        Some(source) => Some(format!("registry source is not crates.io: {source}")),
    }
}

fn is_crates_io(source: &str) -> bool {
    source.starts_with("registry+https://github.com/rust-lang/crates.io-index")
        || source.starts_with("sparse+https://index.crates.io/")
}

fn strategy_rule<'a>(policy: &'a Policy, rel: Option<&str>) -> Option<&'a StrategyRule> {
    let rel = rel?;
    policy
        .strategy
        .iter()
        .find(|rule| rel.starts_with(&rule.path_prefix))
}

fn strategy_violations(pkg: &Pkg, rule: &StrategyRule, rel: &Option<String>) -> Vec<String> {
    let mut violations = Vec::new();
    let where_ = rel.as_deref().unwrap_or(pkg.name.as_str());
    if rule.forbid_build_script && has_build_script(pkg) {
        violations.push(format!(
            "{where_} has a build.rs, which strategy crates cannot"
        ));
    }
    if rule.forbid_proc_macro && is_proc_macro(pkg) {
        violations.push(format!(
            "{where_} is a proc-macro, which strategy crates cannot be"
        ));
    }
    for dep in &pkg.dependencies {
        let kind = kind_name(dep.kind.as_deref());
        let source = match classify_dep(dep) {
            Ok(source) => source,
            Err(err) => {
                violations.push(format!("{where_} -> {}: {err}", dep.name));
                continue;
            }
        };
        let allowed = source == "path"
            && kind == "normal"
            && rule.internal.contains(&dep.name)
            && dep.features.is_empty()
            && dep.target.is_none();
        if !allowed {
            violations.push(format!(
                "{where_} -> {} ({kind}, {source}) is not allowed for a strategy crate",
                dep.name
            ));
        }
    }
    violations
}

fn crate_violations(pkg: &Pkg, policy: &Policy) -> Vec<String> {
    let mut violations = Vec::new();
    let Some(rules) = policy.crates.iter().find(|item| item.name == pkg.name) else {
        violations.push(format!("{} is not in policy/deps.toml", pkg.name));
        return violations;
    };
    for dep in &pkg.dependencies {
        let kind = kind_name(dep.kind.as_deref());
        let source = match classify_dep(dep) {
            Ok(source) => source,
            Err(err) => {
                violations.push(format!("{} -> {}: {err}", pkg.name, dep.name));
                continue;
            }
        };
        if dep.target.is_some() {
            violations.push(format!(
                "{} -> {} has a target cfg, which the policy does not list",
                pkg.name, dep.name
            ));
            continue;
        }
        let Some(edge) = rules
            .edges
            .iter()
            .find(|edge| edge.name == dep.name && edge.kind == kind && edge.source == source)
        else {
            let renamed = dep
                .rename
                .as_deref()
                .map(|name| format!(" renamed to {name}"))
                .unwrap_or_default();
            violations.push(format!(
                "{} -> {} ({kind}, {source}){renamed} is not an allowed edge",
                pkg.name, dep.name
            ));
            continue;
        };
        if dep.uses_default_features && !edge.default_features {
            violations.push(format!(
                "{} -> {} enables default features",
                pkg.name, dep.name
            ));
        }
        for feature in &dep.features {
            if !edge.features.contains(feature) {
                violations.push(format!(
                    "{} -> {} enables feature {feature}",
                    pkg.name, dep.name
                ));
            }
        }
    }
    violations
}

fn classify_dep(dep: &Dep) -> Result<&'static str, String> {
    match &dep.source {
        None => Ok("path"),
        Some(source) if is_crates_io(source) => Ok("crates.io"),
        Some(source) if source.starts_with("git+") => {
            Err(format!("git source is forbidden: {source}"))
        }
        Some(source) => Err(format!("registry source is not crates.io: {source}")),
    }
}

fn kind_name(kind: Option<&str>) -> String {
    kind.unwrap_or("normal").to_owned()
}

fn has_build_script(pkg: &Pkg) -> bool {
    pkg.targets
        .iter()
        .any(|target| target.kind.iter().any(|kind| kind == "custom-build"))
}

const GUARDED: &[&str] = &[
    "parangsae-signals",
    "parangsae-regime",
    "parangsae-research",
];
const BANNED_TRANSITIVE: &[&str] = &["reqwest", "hyper", "tokio-tungstenite"];

fn transitive_bans(meta: &Meta, members: &BTreeSet<&str>) -> Vec<String> {
    let packages: BTreeMap<&str, &Pkg> = meta
        .packages
        .iter()
        .map(|pkg| (pkg.id.as_str(), pkg))
        .collect();
    let nodes: BTreeMap<&str, &Node> = meta
        .resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut violations = Vec::new();
    for pkg in &meta.packages {
        if !members.contains(pkg.id.as_str()) || !is_guarded(meta, pkg) {
            continue;
        }
        let mut stack = vec![pkg.id.as_str()];
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(node) = nodes.get(id) else {
                continue;
            };
            for dep in &node.deps {
                if !dep.dep_kinds.iter().any(is_normal_kind) {
                    continue;
                }
                let Some(target) = packages.get(dep.pkg.as_str()) else {
                    continue;
                };
                if BANNED_TRANSITIVE.contains(&target.name.as_str()) {
                    violations.push(format!(
                        "{} transitively depends on {}",
                        pkg.name, target.name
                    ));
                }
                stack.push(dep.pkg.as_str());
            }
        }
    }
    violations
}

fn is_guarded(meta: &Meta, pkg: &Pkg) -> bool {
    if GUARDED.contains(&pkg.name.as_str()) {
        return true;
    }
    relative_manifest(&meta.workspace_root, &pkg.manifest_path)
        .map(|path| path.starts_with("crates/strategies/"))
        .unwrap_or(false)
}

fn is_normal_kind(kind: &DepKind) -> bool {
    kind.kind.is_none()
}

fn is_proc_macro(pkg: &Pkg) -> bool {
    pkg.targets.iter().any(|target| {
        target.kind.iter().any(|kind| kind == "proc-macro")
            || target.crate_types.iter().any(|kind| kind == "proc-macro")
    })
}

fn relative_manifest(root: &Path, manifest: &Path) -> Option<String> {
    let rel = manifest.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Result<Policy, String> {
        let root = crate::workspace_root()?;
        load_policy(&root.join("policy/deps.toml"))
    }

    #[test]
    fn forbidden_edge_fixture_is_rejected() -> Result<(), String> {
        let meta: Meta = serde_json::from_str(include_str!(
            "../../fixtures/depgraph/signals-on-collector.json"
        ))
        .map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        assert!(
            violations.iter().any(|item| {
                item.contains("parangsae-signals -> parangsae-collector")
                    && item.contains("not an allowed edge")
            }),
            "{violations:?}"
        );
        Ok(())
    }

    #[test]
    fn a_rename_does_not_hide_the_package_name() -> Result<(), String> {
        let meta: Meta = serde_json::from_str(include_str!(
            "../../fixtures/depgraph/renamed-collector.json"
        ))
        .map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        assert!(
            violations
                .iter()
                .any(|item| item.contains("parangsae-collector") && item.contains("renamed")),
            "{violations:?}"
        );
        Ok(())
    }

    #[test]
    fn git_source_is_rejected() -> Result<(), String> {
        let meta: Meta =
            serde_json::from_str(include_str!("../../fixtures/depgraph/git-source.json"))
                .map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        assert!(
            violations
                .iter()
                .any(|item| item.contains("git source is forbidden")),
            "{violations:?}"
        );
        Ok(())
    }

    #[test]
    fn an_unlisted_feature_is_rejected() -> Result<(), String> {
        let meta: Meta = serde_json::from_str(include_str!(
            "../../fixtures/depgraph/unexpected-feature.json"
        ))
        .map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        assert!(
            violations
                .iter()
                .any(|item| item.contains("enables feature foo")),
            "{violations:?}"
        );
        Ok(())
    }

    #[test]
    fn a_strategy_build_script_and_forbidden_edge_are_rejected() -> Result<(), String> {
        let raw = include_str!("../../fixtures/depgraph/strategy-build.json");
        let meta: Meta = serde_json::from_str(raw).map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        assert!(
            violations.iter().any(|item| item.contains("build.rs")),
            "{violations:?}"
        );
        assert!(
            violations.iter().any(|item| item.contains("parangsae-sim")),
            "{violations:?}"
        );
        assert!(
            violations.iter().any(|item| item.contains("proc-macro")),
            "{violations:?}"
        );
        let without_build = raw.replace("\"custom-build\"", "\"lib\"");
        let meta: Meta = serde_json::from_str(&without_build).map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        assert!(
            violations.iter().all(|item| !item.contains("build.rs")),
            "{violations:?}"
        );
        Ok(())
    }

    #[test]
    fn an_unlisted_parangsae_foo_edge_into_research_is_rejected() -> Result<(), String> {
        let policy_text = r#"
            [[crate]]
            name = "parangsae-research"
            allow_path_prefixes = ["parangsae-"]

            [[crate.edge]]
            name = "parangsae-core"
            kind = "normal"
            source = "path"
            default_features = true
        "#;
        let rules: Policy = toml::from_str(policy_text).map_err(|err| err.to_string())?;
        let meta: Meta = serde_json::from_str(include_str!(
            "../../fixtures/depgraph/research-unlisted-foo.json"
        ))
        .map_err(|err| err.to_string())?;
        let violations = check(&meta, &rules);
        assert!(
            violations.iter().any(|item| {
                item.contains("parangsae-research -> parangsae-foo")
                    && item.contains("not an allowed edge")
            }),
            "{violations:?}"
        );
        Ok(())
    }

    #[test]
    fn a_planned_name_that_is_a_member_and_a_stale_policy_crate_are_rejected() -> Result<(), String>
    {
        let policy_text = r#"
            [[crate]]
            name = "parangsae-ghost"

            [[planned]]
            name = "parangsae-signals"
        "#;
        let rules: Policy = toml::from_str(policy_text).map_err(|err| err.to_string())?;
        let meta: Meta = serde_json::from_str(include_str!(
            "../../fixtures/depgraph/signals-on-collector.json"
        ))
        .map_err(|err| err.to_string())?;
        let violations = check(&meta, &rules);
        assert!(
            violations
                .iter()
                .any(|item| item.contains("parangsae-ghost")
                    && item.contains("not a workspace member")),
            "{violations:?}"
        );
        assert!(
            violations
                .iter()
                .any(|item| item.contains("parangsae-signals") && item.contains("planned")),
            "{violations:?}"
        );
        Ok(())
    }

    #[test]
    fn a_resolved_hyper_edge_into_signals_is_rejected() -> Result<(), String> {
        let raw = include_str!("../../fixtures/depgraph/signals-transitive-hyper.json");
        let meta: Meta = serde_json::from_str(raw).map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        assert!(
            violations
                .iter()
                .any(|item| item.contains("parangsae-signals transitively depends on hyper")),
            "{violations:?}"
        );
        let dev_only = raw.replace(
            "\"pkg\": \"hyper\", \"dep_kinds\": [{ \"kind\": null }]",
            "\"pkg\": \"hyper\", \"dep_kinds\": [{ \"kind\": \"dev\" }]",
        );
        let meta: Meta = serde_json::from_str(&dev_only).map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        assert!(
            violations
                .iter()
                .all(|item| !item.contains("transitively depends on hyper")),
            "{violations:?}"
        );
        Ok(())
    }

    #[test]
    fn this_workspace_matches_the_policy() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let violations = workspace(&root)?;
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations.join("\n"))
        }
    }
}
