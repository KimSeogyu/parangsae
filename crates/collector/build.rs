//! Bake the git revision that built this binary into `PARANGSAE_GIT_SHA`.
//!
//! Publish-time working-tree status is not the provenance. If git is unavailable
//! the sha is `unknown` and the tree is treated as dirty so publish fails closed.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => dir,
        Err(_) => {
            export("unknown", true);
            return;
        }
    };
    let repo = PathBuf::from(manifest_dir).join("../..");
    let sha = git_output(&repo, &["rev-parse", "HEAD"]);
    let status = git_output(&repo, &["status", "--porcelain", "--untracked-files=no"]);
    let (sha, dirty) = match (sha, status) {
        (Some(sha), Some(status)) => {
            let sha = sha.trim().to_owned();
            let dirty = !status.trim().is_empty();
            if sha.len() == 40
                && sha
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !matches!(byte, b'A'..=b'F'))
            {
                (sha, dirty)
            } else {
                ("unknown".to_owned(), true)
            }
        }
        _ => ("unknown".to_owned(), true),
    };
    export(&sha, dirty);
    let git_dir = repo.join(".git");
    rerun(&git_dir.join("HEAD"));
    if let Some(ref_path) = head_ref(&git_dir) {
        rerun(&ref_path);
    }
    rerun(&git_dir.join("index"));
    rerun(&repo.join("crates"));
}

fn export(sha: &str, dirty: bool) {
    let dirty_flag = if dirty { "1" } else { "0" };
    let _ = writeln!(io::stdout(), "cargo:rustc-env=PARANGSAE_GIT_SHA={sha}");
    let _ = writeln!(
        io::stdout(),
        "cargo:rustc-env=PARANGSAE_GIT_DIRTY={dirty_flag}"
    );
}

fn rerun(path: &Path) {
    let _ = writeln!(io::stdout(), "cargo:rerun-if-changed={}", path.display());
}

fn git_output(repo: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn head_ref(git_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let rest = text.trim().strip_prefix("ref: ")?;
    if rest.is_empty() || rest.contains("..") {
        return None;
    }
    Some(git_dir.join(rest))
}
