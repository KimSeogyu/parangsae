//! Checks the workspace can run from `cargo xtask`.

pub mod depgraph;
pub mod lints;

use std::path::PathBuf;
use std::process::Command;

/// Workspace root from `CARGO_WORKSPACE_DIR` or from `cargo metadata`.
/// Metadata walks up from the current directory, so a caller in `specs/tla`
/// still finds this repository.
pub fn workspace_root() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("CARGO_WORKSPACE_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
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
    let root = meta
        .get("workspace_root")
        .and_then(|value| value.as_str())
        .ok_or("cargo metadata has no workspace_root")?;
    Ok(PathBuf::from(root))
}
