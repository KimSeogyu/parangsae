//! Proves `feature_run::run_feature` rejects a malformed feature and an
//! undefined step. The production runners call that same function.

#[path = "support/feature_run.rs"]
mod feature_run;

#[derive(Debug, Default, cucumber::World)]
struct RejectWorld;

#[tokio::main]
async fn main() -> Result<(), String> {
    expect_rejection("fixtures/guards/broken.feature", "parsing_errors=").await?;
    expect_rejection("fixtures/guards/undefined_step.feature", "failed=").await?;
    Ok(())
}

async fn expect_rejection(rel: &str, marker: &str) -> Result<(), String> {
    let path = feature_run::workspace_root().join(rel);
    match feature_run::run_feature::<RejectWorld>(&path).await {
        Ok(counts) => Err(format!(
            "{rel} was accepted with {} passed steps",
            counts.passed_steps
        )),
        Err(err) => {
            let zero = format!("{marker}0");
            if err.contains(marker) && !err.contains(&zero) {
                Ok(())
            } else {
                Err(format!("{rel} failed for the wrong reason: {err}"))
            }
        }
    }
}
