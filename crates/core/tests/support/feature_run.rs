//! Shared cucumber execution for the production runners and `feature_reject`.
//!
//! A malformed feature increments `parsing_errors`. An undefined step is
//! skipped until `.fail_on_skipped()` turns it into a failure. `run_and_exit`
//! panics only when `execution_has_failed` is true, so this function returns
//! that same failure. The production binaries and the reject fixture both
//! call `run_feature`.

use std::path::{Path, PathBuf};

use cucumber::writer::Stats;
use cucumber::World;

pub struct Counts {
    pub passed_steps: usize,
    pub failed_steps: usize,
    pub skipped_steps: usize,
    pub parsing_errors: usize,
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub async fn run_feature<W>(path: &Path) -> Result<Counts, String>
where
    W: World + std::fmt::Debug + cucumber::codegen::WorldInventory,
{
    let writer = W::cucumber()
        .fail_on_skipped()
        .with_default_cli()
        .run(path)
        .await;
    let counts = Counts {
        passed_steps: writer.passed_steps(),
        failed_steps: writer.failed_steps(),
        skipped_steps: writer.skipped_steps(),
        parsing_errors: writer.parsing_errors(),
    };
    if writer.execution_has_failed() || counts.skipped_steps > 0 {
        return Err(format!(
            "cucumber step was not executed: failed={} skipped={} parsing_errors={}",
            counts.failed_steps, counts.skipped_steps, counts.parsing_errors
        ));
    }
    Ok(counts)
}
