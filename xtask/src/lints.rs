//! Every workspace member must inherit `[workspace.lints]`.

use std::fs;
use std::path::Path;

pub fn workspace_inheritance(root: &Path) -> Result<Vec<String>, String> {
    let workspace = read(&root.join("Cargo.toml"))?;
    let mut violations = Vec::new();
    for member in members(&workspace)? {
        let path = root.join(&member).join("Cargo.toml");
        let text = read(&path)?;
        if !inherits_workspace_lints(&text) {
            violations.push(format!("{member} does not set lints.workspace = true"));
        }
    }
    Ok(violations)
}

pub fn inherits_workspace_lints(cargo_toml: &str) -> bool {
    let mut in_lints = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_lints = trimmed == "[lints]";
            continue;
        }
        if in_lints && trimmed == "workspace = true" {
            return true;
        }
    }
    false
}

fn members(cargo_toml: &str) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    let mut in_members = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if !in_members && trimmed.starts_with("members") && trimmed.contains('[') {
            in_members = true;
            continue;
        }
        if in_members {
            if trimmed.starts_with(']') {
                break;
            }
            let name = trimmed.trim_matches(|c: char| c == '"' || c == ',' || c.is_whitespace());
            if !name.is_empty() && !name.starts_with('#') {
                found.push(name.to_owned());
            }
        }
    }
    if found.is_empty() {
        return Err("workspace members list is empty".to_owned());
    }
    Ok(found)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opting_out_of_workspace_lints_is_rejected() {
        let text = include_str!("../../fixtures/lint/missing-inheritance/Cargo.toml");
        assert!(!inherits_workspace_lints(text));
    }

    #[test]
    fn this_workspace_inherits_lints() -> Result<(), String> {
        let root = crate::workspace_root()?;
        let violations = workspace_inheritance(&root)?;
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations.join("\n"))
        }
    }
}
