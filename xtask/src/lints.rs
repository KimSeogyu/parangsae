//! A workspace member inherits `[workspace.lints]`, or its manifest bytes are
//! the pinned `origin/main` copy. Editing a main manifest without opting into
//! the workspace lints fails. A manifest that does neither fails.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// sha256 of each member manifest on `origin/main`. `xtask` is not in that tree.
const MAIN_MANIFEST_SHA256: &[(&str, &str)] = &[
    (
        "crates/backtest/Cargo.toml",
        "8346a490756b526bc67bb8760fb39486ad197e5d6fdf6ad71ee63960c85d044c",
    ),
    (
        "crates/collector/Cargo.toml",
        "a3c99b94a61726a9986693777db6ad38ecbcbf718754bdae00b542a50c07ff6d",
    ),
    (
        "crates/core/Cargo.toml",
        "c572b4f17419b7ea2c3dee614e0efbcb36132f804234fe585c1268da2f28336f",
    ),
    (
        "crates/execution/Cargo.toml",
        "4fafe3f33828be6a7eeca1a556e7120568e89f299070b8f5fb80d80bf33058f4",
    ),
    (
        "crates/regime/Cargo.toml",
        "35cf21b207b47f6af59f3440de0e76e003be5d88bfecad5c1ed21822f70fad54",
    ),
    (
        "crates/signals/Cargo.toml",
        "8cdba4d3d4d484b0f8aef3c6c2441c737109432a3603194c1faac91b2789adcd",
    ),
    (
        "crates/sim/Cargo.toml",
        "5b4d263a71f7af1481167c5d0379fa7deb0de63fd866ff3ef301a403a3b7c861",
    ),
];

pub fn workspace_inheritance(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    for manifest in member_manifests(root)? {
        let text = read(&manifest)?;
        let relative = manifest
            .strip_prefix(root)
            .unwrap_or(manifest.as_path())
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if member_lints_ok(&relative, &text)? {
            continue;
        }
        violations.push(format!(
            "{relative} does not set lints.workspace = true and does not match the pinned main manifest"
        ));
    }
    Ok(violations)
}

fn member_lints_ok(relative: &str, text: &str) -> Result<bool, String> {
    if inherits_workspace_lints(text)? {
        return Ok(true);
    }
    let Some(expected) = MAIN_MANIFEST_SHA256
        .iter()
        .find(|(path, _)| *path == relative)
        .map(|(_, hash)| *hash)
    else {
        return Ok(false);
    };
    Ok(sha256_hex(text.as_bytes()) == expected)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
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
    fn a_manifest_that_neither_inherits_nor_matches_main_is_rejected() -> Result<(), String> {
        let text = "[package]\nname = \"x\"\nversion = \"0.0.0\"\n";
        if member_lints_ok("crates/backtest/Cargo.toml", text)? {
            return Err("edited main manifest was accepted".to_owned());
        }
        if member_lints_ok("crates/new/Cargo.toml", text)? {
            return Err("unpinned member was accepted".to_owned());
        }
        let inherited = "[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n";
        if !member_lints_ok("crates/new/Cargo.toml", inherited)? {
            return Err("workspace inheritance was rejected".to_owned());
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
