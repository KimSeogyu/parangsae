//! `cargo xtask spec ...` reads ArchitecturePolicy.tla and checks the workspace.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use xtask::{enforce, explain, generate, impact, inspect, validate, EnforceOptions};

#[derive(Parser)]
#[command(name = "xtask", about = "parangsae maintenance tasks")]
struct Cli {
    /// Workspace Cargo.toml. Defaults to ./Cargo.toml.
    #[arg(long, global = true)]
    manifest: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Architecture policy sourced from spec/arch/ArchitecturePolicy.tla.
    Spec {
        #[command(subcommand)]
        action: SpecCmd,
    },
}

#[derive(Subcommand)]
enum SpecCmd {
    /// Run SANY, then TLC on the well-formedness spec.
    Validate {
        /// Path to tla2tools.jar. Defaults to $TLA2TOOLS_JAR or specs/tla/tla2tools.jar.
        #[arg(long)]
        jar: Option<PathBuf>,
    },
    /// Write spec/arch/generated/policy.json from the .tla file.
    Generate {
        /// Fail if the committed JSON is not the bytes this command would write.
        #[arg(long)]
        check: bool,
    },
    /// Run DEP, MUT, CALL, EFFECT, FLOAT, and GEN.
    /// `--diff` also runs PATH. `--as` names the bot. Without `--as`, every
    /// changed path must have exactly one owner.
    Enforce {
        /// Git diff range, for example main..HEAD. Renames are compared with
        /// `--no-renames`, so both sides are checked.
        #[arg(long)]
        diff: Option<String>,
        /// Bot name from Owners. With `--diff`, the diff must stay inside that
        /// bot's globs. Omit it to fail only unowned or ambiguous paths.
        #[arg(long = "as")]
        as_bot: Option<String>,
    },
    /// Show the policy facts for a crate, a Type.field, or a capability.
    Inspect { symbol: String },
    /// Show whether a module may call a capability, and any violating call sites.
    Impact { module: String, capability: String },
    /// Print the stable meaning of a rule id.
    Explain { rule: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let manifest = cli.manifest.unwrap_or_else(|| PathBuf::from("Cargo.toml"));
    let result = match cli.cmd {
        Cmd::Spec { action } => dispatch(&manifest, action),
    };
    match result {
        Ok(text) => {
            if !text.is_empty() {
                print!("{text}");
                if !text.ends_with('\n') {
                    println!();
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            match err {
                RunError::Tool(err) => eprintln!("{err}"),
                RunError::Violations(text) => eprint!("{text}"),
            }
            ExitCode::from(1)
        }
    }
}

enum RunError {
    Tool(xtask::ToolError),
    Violations(String),
}

impl From<xtask::ToolError> for RunError {
    fn from(err: xtask::ToolError) -> Self {
        Self::Tool(err)
    }
}

fn dispatch(manifest: &std::path::Path, action: SpecCmd) -> Result<String, RunError> {
    match action {
        SpecCmd::Validate { jar } => validate(manifest, jar.as_deref()).map_err(RunError::from),
        SpecCmd::Generate { check } => {
            let root = workspace_root(manifest)?;
            let policy = root.join("spec/arch/ArchitecturePolicy.tla");
            let out = root.join("spec/arch/generated/policy.json");
            generate(&policy, &out, check)?;
            if check {
                Ok(format!("policy.json matches {}\n", policy.display()))
            } else {
                Ok(format!("wrote {}\n", out.display()))
            }
        }
        SpecCmd::Enforce { diff, as_bot } => {
            if as_bot.is_some() && diff.is_none() {
                return Err(RunError::Tool(xtask::ToolError::tool(
                    "--as requires --diff",
                )));
            }
            let diags = enforce(&EnforceOptions {
                manifest,
                diff: diff.as_deref(),
                as_bot: as_bot.as_deref(),
                check_generated: true,
            })?;
            if diags.is_empty() {
                return Ok("spec enforce: no violations\n".to_owned());
            }
            let mut text = String::new();
            for diag in &diags {
                text.push_str(&diag.to_string());
                text.push('\n');
            }
            Err(RunError::Violations(text))
        }
        SpecCmd::Inspect { symbol } => inspect(manifest, &symbol).map_err(RunError::from),
        SpecCmd::Impact { module, capability } => {
            impact(manifest, &module, &capability).map_err(RunError::from)
        }
        SpecCmd::Explain { rule } => Ok(format!("{}\n", explain(&rule)?)),
    }
}

fn workspace_root(manifest: &std::path::Path) -> Result<PathBuf, xtask::ToolError> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest)
        .other_options(["--offline".to_owned(), "--no-deps".to_owned()])
        .exec()
        .map_err(|err| xtask::ToolError::tool(format!("cargo metadata: {err}")))?;
    Ok(PathBuf::from(metadata.workspace_root.as_str()))
}
