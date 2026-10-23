//! Architecture policy engine. The normative text is `spec/arch/ArchitecturePolicy.tla`.

#![forbid(unsafe_code)]

mod effects;
mod error;
mod json;
mod parse;
mod rules;
mod scan;
mod tokens;

pub use error::{Diagnostic, ToolError};
pub use parse::{parse_policy, read_policy, Policy};
pub use rules::{
    absolutize_jar, diff_names, enforce, every_tracked_has_owner, explain, generate, glob_match,
    impact, inspect, path_diags_for, unowned_path_diags, validate, EnforceOptions,
};
