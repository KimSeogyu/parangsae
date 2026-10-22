//! Workspace lint inheritance, and the fixtures that prove the lints can fail.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn inherits_workspace_lints(cargo_toml: &str) -> bool {
    let mut in_lints = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_lints = trimmed == "[lints]";
            continue;
        }
        if in_lints && trimmed == "workspace = true" {
            return true;
        }
    }
    false
}

fn workspace_members(cargo_toml: &str) -> Result<Vec<String>, String> {
    let mut members = Vec::new();
    let mut in_members = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if !in_members && trimmed.starts_with("members") && trimmed.contains('[') {
            in_members = true;
            continue;
        }
        if in_members {
            if trimmed.starts_with(']') {
                break;
            }
            let name = trimmed.trim_matches(|c: char| c == '"' || c == ',' || c.is_whitespace());
            if !name.is_empty() && !name.starts_with('#') {
                members.push(name.to_string());
            }
        }
    }
    if members.is_empty() {
        return Err("workspace members list is empty".to_owned());
    }
    Ok(members)
}

fn table_entries(text: &str, header: &str) -> BTreeSet<String> {
    let mut entries = BTreeSet::new();
    let mut on = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            on = trimmed == header;
            continue;
        }
        if on && trimmed.contains('=') && !trimmed.starts_with('#') {
            entries.insert(trimmed.to_owned());
        }
    }
    entries
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))
}

#[test]
fn every_member_inherits_workspace_lints() -> Result<(), String> {
    let root = repo_root();
    let workspace = read(&root.join("Cargo.toml"))?;
    for member in workspace_members(&workspace)? {
        let path = root.join(&member).join("Cargo.toml");
        let text = read(&path)?;
        if !inherits_workspace_lints(&text) {
            return Err(format!("{member} does not set lints.workspace = true"));
        }
    }
    Ok(())
}

#[test]
fn inheritance_check_rejects_a_package_that_opts_out() -> Result<(), String> {
    let text = read(&repo_root().join("fixtures/lint/missing-inheritance/Cargo.toml"))?;
    if inherits_workspace_lints(&text) {
        return Err("missing-inheritance fixture was accepted".to_owned());
    }
    Ok(())
}

#[test]
fn violation_fixture_denies_the_workspace_clippy_lints() -> Result<(), String> {
    let root = repo_root();
    let workspace = read(&root.join("Cargo.toml"))?;
    let fixture = read(&root.join("fixtures/lint/violation/Cargo.toml"))?;
    let wanted = table_entries(&workspace, "[workspace.lints.clippy]");
    let actual = table_entries(&fixture, "[lints.clippy]");
    if wanted != actual {
        return Err(format!(
            "clippy deny list drifted\nworkspace: {wanted:?}\nfixture: {actual:?}"
        ));
    }
    if !workspace.contains("unsafe_code = \"forbid\"") {
        return Err("workspace does not forbid unsafe_code".to_owned());
    }
    let unsafe_fixture = read(&root.join("fixtures/lint/unsafe-forbid/Cargo.toml"))?;
    if !unsafe_fixture.contains("unsafe_code = \"forbid\"") {
        return Err("unsafe fixture does not forbid unsafe_code".to_owned());
    }
    let fixed = read(&root.join("crates/core/src/fixed.rs"))?;
    if !fixed.contains("#![deny(clippy::arithmetic_side_effects)]") {
        return Err("core::fixed does not deny arithmetic_side_effects".to_owned());
    }
    if !fixture.contains("#![deny(clippy::arithmetic_side_effects)]")
        && !read(&root.join("fixtures/lint/violation/src/lib.rs"))?
            .contains("#![deny(clippy::arithmetic_side_effects)]")
    {
        return Err("violation fixture does not deny arithmetic_side_effects".to_owned());
    }
    Ok(())
}
