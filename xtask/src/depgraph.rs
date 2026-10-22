//! Allowed-edge check over `cargo metadata --locked --format-version 1`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Policy {
    #[serde(rename = "crate", default)]
    crates: Vec<CratePolicy>,
    #[serde(default)]
    strategy: Vec<StrategyRule>,
}

#[derive(Debug, Deserialize)]
struct CratePolicy {
    name: String,
    #[serde(default, rename = "edge")]
    edges: Vec<EdgePolicy>,
    /// Path dependencies whose package name starts with one of these are
    /// allowed. Research uses this for strategy crates that do not exist yet.
    #[serde(default)]
    allow_path_prefixes: BTreeSet<String>,
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
    let mut violations = Vec::new();
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
    if rule.forbid_build_script && pkg.manifest_path.with_file_name("build.rs").is_file() {
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
        let prefixed = source == "path"
            && kind == "normal"
            && dep.features.is_empty()
            && rules
                .allow_path_prefixes
                .iter()
                .any(|prefix| dep.name.starts_with(prefix));
        if prefixed {
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
        let dir =
            std::env::temp_dir().join(format!("parangsae-strategy-bad-{}", std::process::id()));
        let manifest_dir = dir.join("crates/strategies/peek");
        std::fs::create_dir_all(&manifest_dir).map_err(|err| err.to_string())?;
        let manifest = manifest_dir.join("Cargo.toml");
        std::fs::write(&manifest, "").map_err(|err| err.to_string())?;
        std::fs::write(manifest_dir.join("build.rs"), "// bad\n").map_err(|err| err.to_string())?;
        let raw = include_str!("../../fixtures/depgraph/strategy-build.json");
        let meta_text = raw.replace("WORKSPACE_ROOT", &dir.display().to_string());
        let meta: Meta = serde_json::from_str(&meta_text).map_err(|err| err.to_string())?;
        let violations = check(&meta, &policy()?);
        let _ = std::fs::remove_dir_all(&dir);
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
