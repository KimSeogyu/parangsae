//! Checks the workspace can run from `cargo xtask`.

pub mod depgraph;
pub mod lints;

use std::path::{Path, PathBuf};

pub fn workspace_root() -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask has no parent directory".to_owned())
}
