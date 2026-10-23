//! Diagnostics the enforcement engine prints. Rule ids stay stable.

use std::fmt;

pub const DEP_DIRECT: &str = "ARCH-DEP-001";
pub const DEP_TRANSITIVE: &str = "ARCH-DEP-002";
pub const MUT: &str = "ARCH-MUT-001";
pub const MUT_UNRESOLVED: &str = "ARCH-MUT-002";
pub const CALL: &str = "ARCH-CALL-001";
pub const CALL_UNRESOLVED: &str = "ARCH-CALL-002";
pub const EFFECT: &str = "ARCH-EFFECT-001";
pub const FLOAT: &str = "ARCH-FLOAT-001";
pub const GEN: &str = "ARCH-GEN-001";
pub const PATH: &str = "ARCH-PATH-001";
pub const PATH_AMBIG: &str = "ARCH-PATH-002";
pub const PARSE: &str = "ARCH-PARSE-001";
pub const WF: &str = "ARCH-WF-001";

/// A policy violation an observer can read without the engine source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub rule_id: &'static str,
    pub message: String,
    pub symbol: String,
    pub file: String,
    pub line: u32,
    pub allowed: String,
    pub spec_file: String,
    pub spec_line: u32,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{id}: {msg}\n  symbol: {symbol}\n  source: {file}:{line}\n  allowed: {allowed}\n  spec: {spec}:{spec_line}",
            id = self.rule_id,
            msg = self.message,
            symbol = self.symbol,
            file = self.file,
            line = self.line,
            allowed = self.allowed,
            spec = self.spec_file,
            spec_line = self.spec_line,
        )
    }
}

/// The engine could not run the check. This is not a policy violation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolError {
    pub rule_id: &'static str,
    pub message: String,
    pub file: String,
    pub line: u32,
}

impl ToolError {
    pub fn parse(file: impl Into<String>, line: u32, message: impl Into<String>) -> Self {
        Self {
            rule_id: PARSE,
            message: message.into(),
            file: file.into(),
            line,
        }
    }

    pub fn wf(file: impl Into<String>, line: u32, message: impl Into<String>) -> Self {
        Self {
            rule_id: WF,
            message: message.into(),
            file: file.into(),
            line,
        }
    }

    pub fn tool(message: impl Into<String>) -> Self {
        Self {
            rule_id: "ARCH-TOOL-001",
            message: message.into(),
            file: String::new(),
            line: 0,
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}", self.rule_id, self.message)
        } else {
            write!(
                f,
                "{}: {} ({}:{})",
                self.rule_id, self.message, self.file, self.line
            )
        }
    }
}

impl std::error::Error for ToolError {}

pub fn explain_rule(id: &str) -> Option<&'static str> {
    Some(match id {
        DEP_DIRECT => {
            "ARCH-DEP-001\n\
             A declared dependency between local packages is not in\n\
             AllowedDependencies, or one end is a path package outside the\n\
             workspace (an unknown crate). The edge is taken from the manifest:\n\
             normal, dev, build, optional, and target-specific deps all count,\n\
             whether or not the resolver activated them. symbol is `from -> to`.\n\
             source is the depending crate's Cargo.toml line. Registry crates\n\
             are not edges."
        }
        DEP_TRANSITIVE => {
            "ARCH-DEP-002\n\
             A workspace path of two or more edges contains an edge that is not in\n\
             AllowedDependencies. symbol is the full crate path. source is the\n\
             Cargo.toml line of the unlisted edge. Dev, build, optional, and\n\
             target-specific deps are edges when they are path crates."
        }
        MUT => {
            "ARCH-MUT-001\n\
             An assignment or `&mut` field access writes Type.field outside the\n\
             package named in AllowedStateWriters. `#[allow]` and comments do not\n\
             suppress this. symbol is `Type.field`. Private fields remain the\n\
             compiler's check; this scan is the backstop."
        }
        MUT_UNRESOLVED => {
            "ARCH-MUT-002\n\
             A write to a field name that the policy owns could not be given a type,\n\
             or a Rust source file could not be parsed or reached from a cargo target.\n\
             The check fails instead of treating the write as allowed."
        }
        CALL => {
            "ARCH-CALL-001\n\
             A path mentions a function whose name is in Capabilities, and the\n\
             package is not paired with that capability in AllowedCapabilityCallers.\n\
             A mention is a call, a `use` or `pub use`, an alias (`y::safe` when `y`\n\
             is the function), a function passed as a value, a typed function pointer,\n\
             or the same path inside a macro. symbol is `path -> function`."
        }
        CALL_UNRESOLVED => {
            "ARCH-CALL-002\n\
             A path, `use`, or macro token names a capability and the path did not\n\
             resolve to that function. The check fails instead of ignoring it."
        }
        EFFECT => {
            "ARCH-EFFECT-001\n\
             A local package uses an effect the engine knows (net, fs_write, process,\n\
             env) without an AllowedEffects pair. net is reqwest, hyper, tungstenite,\n\
             socket2, tokio's net or default features, std::net, and tokio::net.\n\
             fs_write is the std::fs write and remove APIs, OpenOptions,\n\
             File::create, File::options, and std::os::{unix,windows}::fs::symlink.\n\
             process is std::process::{Command, Child, exit, Stdio, Output}. env is\n\
             the std::env readers and writers, including args. A glob of one of those\n\
             modules is the effect. extern crate aliases are expanded. Macro token\n\
             streams are parsed as expressions when they can be, and otherwise any\n\
             effect-module path in the tokens fails closed. ExitCode is not process.\n\
             A parsed File::open or read is not fs_write."
        }
        FLOAT => {
            "ARCH-FLOAT-001\n\
             A float outside FloatAllowed: an f64 or f32 identifier, a float literal\n\
             (decimal point or exponent, with or without an f32/f64 suffix), or a\n\
             method or call whose name ends in _f64 or _f32. Comments and string\n\
             literals are not tokens. `#[allow]` does not suppress the token.\n\
             Crates outside FloatAllowed also deny clippy::float_arithmetic."
        }
        GEN => {
            "ARCH-GEN-001\n\
             spec/arch/generated/policy.json is missing or is not the bytes the\n\
             generator emits for the current ArchitecturePolicy.tla, including the\n\
             sha256 of that file. Hand edits and drift fail."
        }
        PATH => {
            "ARCH-PATH-001\n\
             A changed path is outside the globs PathOwners grants the named bot, or\n\
             `--diff` was given without `--as` and the path has no owner. git diff\n\
             uses --no-renames, so a move is checked on both sides. The most specific\n\
             matching glob wins."
        }
        PATH_AMBIG => {
            "ARCH-PATH-002\n\
             Two PathOwners globs of equal specificity match one path and name\n\
             different bots. The spec must make the owner unique."
        }
        PARSE => {
            "ARCH-PARSE-001\n\
             The policy module uses a TLA+ construct outside the restricted profile:\n\
             finite sets of strings and of string pairs, plus the fixed clock\n\
             scaffold and the WellFormed shapes the engine evaluates. The generator\n\
             does not drop the construct."
        }
        WF => {
            "ARCH-WF-001\n\
             WellFormed is false for these sets: a referenced module is missing,\n\
             a dependency is a self-edge, a state field has more than one writer,\n\
             a capability or effect is outside its vocabulary, a float crate is\n\
             not a module, or a path owner is not in Owners."
        }
        "ARCH-TOOL-001" => {
            "ARCH-TOOL-001\n\
             The engine could not read the workspace, run git, or run the TLA+ tools.\n\
             This is not a policy decision."
        }
        _ => return None,
    })
}
