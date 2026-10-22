//! Checks the workspace can run from `cargo xtask`.

pub mod depgraph;
pub mod guards;
pub mod lints;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `CARGO_WORKSPACE_DIR` would point every check at another tree. CI must not set it.
pub fn reject_cargo_workspace_dir_in_ci(
    in_ci: bool,
    cargo_workspace_dir: Option<&std::ffi::OsStr>,
) -> Result<(), String> {
    if in_ci {
        if let Some(dir) = cargo_workspace_dir {
            if !dir.is_empty() {
                return Err("CARGO_WORKSPACE_DIR is set in CI".to_owned());
            }
        }
    }
    Ok(())
}

/// Any set value counts. `GITHUB_ACTIONS=0` and an empty `CI` are still CI.
pub fn ci_is_set(github_actions: Option<&std::ffi::OsStr>, ci: Option<&std::ffi::OsStr>) -> bool {
    github_actions.is_some() || ci.is_some()
}

fn running_in_ci() -> bool {
    ci_is_set(
        std::env::var_os("GITHUB_ACTIONS").as_deref(),
        std::env::var_os("CI").as_deref(),
    )
}

/// Workspace root from `CARGO_WORKSPACE_DIR` or from `cargo metadata`.
/// Metadata walks up from the current directory, so a caller in `specs/tla`
/// still finds this repository. In CI the environment override is refused.
pub fn workspace_root() -> Result<PathBuf, String> {
    let in_ci = running_in_ci();
    reject_cargo_workspace_dir_in_ci(in_ci, std::env::var_os("CARGO_WORKSPACE_DIR").as_deref())?;
    if !in_ci {
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

/// True when a libtest summary line is exactly one passing test.
pub fn libtest_summary_is_one_pass(line: &str) -> bool {
    line.starts_with("test result: ok. 1 passed")
}

/// Run one `xtask` library test by its full path.
/// Fails unless the libtest summary is `test result: ok. 1 passed`.
pub fn run_exact(root: &Path, test: &str) -> Result<(), String> {
    if test.is_empty() || test.contains(char::is_whitespace) || !test.contains("::") {
        return Err(format!("run-exact needs one full test path, got {test}"));
    }
    let output = Command::new("cargo")
        .args(["test", "-p", "xtask", "--lib", test, "--", "--exact"])
        .current_dir(root)
        .output()
        .map_err(|err| format!("cargo test: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut out = std::io::stdout().lock();
    out.write_all(stdout.as_bytes())
        .map_err(|err| format!("write test output: {err}"))?;
    drop(out);
    let mut err = std::io::stderr().lock();
    err.write_all(stderr.as_bytes())
        .map_err(|err| format!("write test output: {err}"))?;
    drop(err);
    run_exact_decision(&stdout, &stderr, output.status.success())
}

/// Decide a `run-exact` result from cargo's streams and status.
/// A selector that matches nothing has no `test result: ok. 1 passed` line.
/// A nonzero cargo status fails even when the summary line says one passed.
pub fn run_exact_decision(stdout: &str, stderr: &str, success: bool) -> Result<(), String> {
    let summary = stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| line.starts_with("test result:"));
    match summary {
        Some(line) if libtest_summary_is_one_pass(line) && success => Ok(()),
        Some(line) => Err(format!(
            "run-exact expected test result: ok. 1 passed, got {line}"
        )),
        None => Err("run-exact saw no libtest summary".to_owned()),
    }
}
