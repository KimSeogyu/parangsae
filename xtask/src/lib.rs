//! Architecture policy engine. The normative text is `spec/arch/ArchitecturePolicy.tla`.

#![forbid(unsafe_code)]

mod effects;
mod error;
mod json;
mod parse;
mod rules;
mod scan;

pub use error::{Diagnostic, ToolError};
pub use parse::{parse_policy, read_policy, Policy};
pub use rules::{
    enforce, every_tracked_has_owner, explain, generate, glob_match, impact, inspect,
    path_diags_for, validate, EnforceOptions,
};
