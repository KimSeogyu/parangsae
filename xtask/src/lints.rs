//! Every workspace member must inherit `[workspace.lints]`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn workspace_inheritance(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    for manifest in member_manifests(root)? {
        let text = read(&manifest)?;
        if !inherits_workspace_lints(&text)? {
            violations.push(format!(
                "{} does not set lints.workspace = true",
                manifest.display()
            ));
        }
    }
    Ok(violations)
}

pub fn inherits_workspace_lints(cargo_toml: &str) -> Result<bool, String> {
    let value: toml::Value = toml::from_str(cargo_toml).map_err(|err| err.to_string())?;
    Ok(value
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(|flag| flag.as_bool())
        == Some(true))
}

fn member_manifests(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|err| format!("cargo metadata: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let meta: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|err| format!("metadata json: {err}"))?;
    let members = meta
        .get("workspace_members")
        .and_then(|value| value.as_array())
        .ok_or("cargo metadata has no workspace_members")?;
    let mut ids = Vec::new();
    for id in members {
        ids.push(
            id.as_str()
                .ok_or("workspace member id is not a string")?
                .to_owned(),
        );
    }
    let packages = meta
        .get("packages")
        .and_then(|value| value.as_array())
        .ok_or("cargo metadata has no packages")?;
    let mut paths = Vec::new();
    for package in packages {
        let id = package
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or("package id is missing")?;
        if !ids.iter().any(|member| member == id) {
            continue;
        }
        let manifest = package
            .get("manifest_path")
            .and_then(|value| value.as_str())
            .ok_or("manifest_path is missing")?;
        paths.push(PathBuf::from(manifest));
    }
    if paths.len() != ids.len() {
        return Err(format!(
            "metadata lists {} members and {} manifests",
            ids.len(),
            paths.len()
        ));
    }
    Ok(paths)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opting_out_of_workspace_lints_is_rejected() -> Result<(), String> {
        let text = include_str!("../../fixtures/lint/missing-inheritance/Cargo.toml");
        if inherits_workspace_lints(text)? {
            return Err("missing-inheritance fixture was accepted".to_owned());
        }
        let commented = "# [lints]\n# workspace = true\n[package]\nname = \"x\"\n";
        if inherits_workspace_lints(commented)? {
            return Err("a comment satisfied lints.workspace".to_owned());
        }
        Ok(())
    }

    #[test]
    fn this_workspace_inherits_lints() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let violations = workspace_inheritance(&root)?;
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations.join("\n"))
        }
    }
}
