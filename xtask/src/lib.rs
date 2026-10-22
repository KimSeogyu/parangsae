//! Checks the workspace can run from `cargo xtask`.

pub mod depgraph;
pub mod guards;
pub mod lints;

use std::path::PathBuf;
use std::process::Command;

/// `CARGO_WORKSPACE_DIR` would point every check at another tree. CI must not set it.
pub fn reject_cargo_workspace_dir_in_ci(
    github_actions: bool,
    cargo_workspace_dir: Option<&std::ffi::OsStr>,
) -> Result<(), String> {
    if github_actions {
        if let Some(dir) = cargo_workspace_dir {
            if !dir.is_empty() {
                return Err("CARGO_WORKSPACE_DIR is set in CI".to_owned());
            }
        }
    }
    Ok(())
}

fn github_actions_set() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some_and(|value| {
        let value = value.to_string_lossy();
        !value.is_empty() && value != "0"
    })
}

/// Workspace root from `CARGO_WORKSPACE_DIR` or from `cargo metadata`.
/// Metadata walks up from the current directory, so a caller in `specs/tla`
/// still finds this repository. In CI the environment override is refused.
pub fn workspace_root() -> Result<PathBuf, String> {
    let github_actions = github_actions_set();
    reject_cargo_workspace_dir_in_ci(
        github_actions,
        std::env::var_os("CARGO_WORKSPACE_DIR").as_deref(),
    )?;
    if !github_actions {
        if let Some(dir) = std::env::var_os("CARGO_WORKSPACE_DIR") {
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir));
            }
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
