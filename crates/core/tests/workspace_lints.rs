//! Workspace lint inheritance, and the fixtures that prove the lints can fail.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))
}

fn parse_toml(text: &str) -> Result<toml::Value, String> {
    toml::from_str(text).map_err(|err| err.to_string())
}

fn inherits_workspace_lints(cargo_toml: &str) -> Result<bool, String> {
    let value = parse_toml(cargo_toml)?;
    Ok(value
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(|flag| flag.as_bool())
        == Some(true))
}

fn workspace_member_manifests(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .map_err(|err| format!("cargo metadata: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let meta: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("cargo metadata json: {err}"))?;
    let members = meta
        .get("workspace_members")
        .and_then(|value| value.as_array())
        .ok_or("cargo metadata has no workspace_members")?;
    let mut ids = BTreeSet::new();
    for id in members {
        ids.insert(
            id.as_str()
                .ok_or("workspace member id is not a string")?
                .to_owned(),
        );
    }
    let packages = meta
        .get("packages")
        .and_then(|value| value.as_array())
        .ok_or("cargo metadata has no packages")?;
    let mut paths = Vec::new();
    for package in packages {
        let id = package
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or("package id is missing")?;
        if !ids.contains(id) {
            continue;
        }
        let manifest = package
            .get("manifest_path")
            .and_then(|value| value.as_str())
            .ok_or("manifest_path is missing")?;
        paths.push(PathBuf::from(manifest));
    }
    if paths.len() != ids.len() {
        return Err(format!(
            "metadata lists {} members and {} manifests",
            ids.len(),
            paths.len()
        ));
    }
    Ok(paths)
}

fn toml_at<'a>(value: &'a toml::Value, path: &[&str]) -> Result<&'a toml::Value, String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor
            .get(*key)
            .ok_or_else(|| format!("missing table {key}"))?;
    }
    Ok(cursor)
}

fn denies_arithmetic_side_effects(source: &str) -> bool {
    let stripped = strip_comments_and_strings(source);
    stripped.contains("#![deny(clippy::arithmetic_side_effects)]")
}

fn strip_comments_and_strings(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if let Some(end) = raw_string_end(bytes, i) {
            i = end;
            out.push_str("\"\"");
            continue;
        }
        if bytes[i] == b'"' || (bytes[i] == b'b' && i + 1 < bytes.len() && bytes[i + 1] == b'"') {
            if bytes[i] == b'b' {
                i += 1;
            }
            i = cooked_string_end(bytes, i);
            out.push_str("\"\"");
            continue;
        }
        let ch = src[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    if i < bytes.len() && (bytes[i] == b'b' || bytes[i] == b'c') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'r' {
        return None;
    }
    let ident_before = start > 0 && is_ident(bytes[start - 1]);
    if ident_before {
        return None;
    }
    i += 1;
    let mut hashes = 0;
    while i < bytes.len() && bytes[i] == b'#' {
        hashes += 1;
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'"' {
        return None;
    }
    i += 1;
    let closer = format!("\"{}", "#".repeat(hashes));
    let rest = std::str::from_utf8(&bytes[i..]).ok()?;
    let found = rest.find(&closer)?;
    Some(i + found + closer.len())
}

fn cooked_string_end(bytes: &[u8], quote: usize) -> usize {
    let mut i = quote + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'"' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn has_suppression_attr(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            if j < bytes.len() && bytes[j] == b'!' {
                j += 1;
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'[' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let rest = &src[j..];
                if starts_with_keyword(rest, "allow") || starts_with_keyword(rest, "expect") {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    let Some(tail) = text.strip_prefix(keyword) else {
        return false;
    };
    tail.chars()
        .next()
        .map(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
        .unwrap_or(true)
}

fn allow_keys(value: &toml::Value, found: &mut Vec<String>) {
    let toml::Value::Table(table) = value else {
        return;
    };
    for (key, child) in table {
        if key.starts_with("allow-") {
            found.push(key.clone());
        }
        allow_keys(child, found);
    }
}

fn suppression_hits(root: &Path) -> Result<Vec<String>, String> {
    let mut hits = Vec::new();
    walk(root, root, &mut hits)?;
    hits.sort();
    Ok(hits)
}

fn walk(root: &Path, dir: &Path, hits: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name == ".git" {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if relative.starts_with("fixtures/lint") {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, hits)?;
            continue;
        }
        if name == "clippy.toml" {
            let text = read(&path)?;
            let parsed = parse_toml(&text)?;
            let mut keys = Vec::new();
            allow_keys(&parsed, &mut keys);
            if !keys.is_empty() {
                hits.push(format!(
                    "{} clippy.toml {}",
                    relative.display(),
                    keys.join(",")
                ));
            }
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = read(&path)?;
        if has_suppression_attr(&strip_comments_and_strings(&text)) {
            hits.push(format!("{} suppression attribute", relative.display()));
        }
    }
    Ok(())
}

fn copy_probe(name: &str, dest_name: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!(
        "parangsae-lint-{}-{}",
        std::process::id(),
        name.replace('/', "-")
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|err| format!("remove {}: {err}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|err| format!("mkdir {}: {err}", dir.display()))?;
    let source = repo_root().join("fixtures/lint/suppressions").join(name);
    fs::copy(&source, dir.join(dest_name))
        .map_err(|err| format!("copy {}: {err}", source.display()))?;
    Ok(dir)
}

#[test]
fn every_member_inherits_workspace_lints() -> Result<(), String> {
    let root = repo_root();
    let manifests = workspace_member_manifests(&root)?;
    if manifests.is_empty() {
        return Err("cargo metadata returned no workspace members".to_owned());
    }
    for path in manifests {
        let text = read(&path)?;
        if !inherits_workspace_lints(&text)? {
            return Err(format!(
                "{} does not set lints.workspace = true",
                path.display()
            ));
        }
    }
    Ok(())
}

#[test]
fn inheritance_check_rejects_a_package_that_opts_out() -> Result<(), String> {
    let text = read(&repo_root().join("fixtures/lint/missing-inheritance/Cargo.toml"))?;
    if inherits_workspace_lints(&text)? {
        return Err("missing-inheritance fixture was accepted".to_owned());
    }
    let commented = "# [lints]\n# workspace = true\n[package]\nname = \"x\"\n";
    if inherits_workspace_lints(commented)? {
        return Err("a comment satisfied lints.workspace".to_owned());
    }
    Ok(())
}

#[test]
fn violation_fixture_denies_the_workspace_clippy_lints() -> Result<(), String> {
    let root = repo_root();
    let workspace = parse_toml(&read(&root.join("Cargo.toml"))?)?;
    let fixture = parse_toml(&read(&root.join("fixtures/lint/violation/Cargo.toml"))?)?;
    let wanted = toml_at(&workspace, &["workspace", "lints", "clippy"])?;
    let actual = toml_at(&fixture, &["lints", "clippy"])?;
    if wanted != actual {
        return Err(format!(
            "clippy deny list drifted\nworkspace: {wanted}\nfixture: {actual}"
        ));
    }
    let unsafe_code = toml_at(&workspace, &["workspace", "lints", "rust", "unsafe_code"])?;
    if unsafe_code.as_str() != Some("forbid") {
        return Err("workspace does not forbid unsafe_code".to_owned());
    }
    let unsafe_fixture = parse_toml(&read(&root.join("fixtures/lint/unsafe-forbid/Cargo.toml"))?)?;
    if toml_at(&unsafe_fixture, &["lints", "rust", "unsafe_code"])?.as_str() != Some("forbid") {
        return Err("unsafe fixture does not forbid unsafe_code".to_owned());
    }
    let fixed = read(&root.join("crates/core/src/fixed.rs"))?;
    if !denies_arithmetic_side_effects(&fixed) {
        return Err("core::fixed does not deny arithmetic_side_effects".to_owned());
    }
    let commented = "// #![deny(clippy::arithmetic_side_effects)]\nfn demo() {}\n";
    if denies_arithmetic_side_effects(commented) {
        return Err("a comment satisfied the arithmetic deny".to_owned());
    }
    let quoted = "const TEXT: &str = \"#![deny(clippy::arithmetic_side_effects)]\";\n";
    if denies_arithmetic_side_effects(quoted) {
        return Err("a string satisfied the arithmetic deny".to_owned());
    }
    let violation = read(&root.join("fixtures/lint/violation/src/lib.rs"))?;
    if !denies_arithmetic_side_effects(&violation) {
        return Err("violation fixture does not deny arithmetic_side_effects".to_owned());
    }
    Ok(())
}

#[test]
fn suppressions_outside_fixtures_fail_and_comments_do_not() -> Result<(), String> {
    let repo_hits = suppression_hits(&repo_root())?;
    if !repo_hits.is_empty() {
        return Err(format!("suppression outside fixtures/lint: {repo_hits:?}"));
    }
    for (probe, dest) in [
        ("allow.rs", "allow.rs"),
        ("expect.rs", "expect.rs"),
        ("inner_allow.rs", "inner_allow.rs"),
        ("clippy-allow.toml", "clippy.toml"),
    ] {
        let dir = copy_probe(probe, dest)?;
        let hits = suppression_hits(&dir)?;
        fs::remove_dir_all(&dir).map_err(|err| format!("remove {}: {err}", dir.display()))?;
        if hits.is_empty() {
            return Err(format!("{probe} copied outside fixtures/lint was accepted"));
        }
    }
    for (probe, dest) in [
        ("comment.rs", "comment.rs"),
        ("quoted.rs", "quoted.rs"),
        ("clippy-comment.toml", "clippy.toml"),
    ] {
        let dir = copy_probe(probe, dest)?;
        let hits = suppression_hits(&dir)?;
        fs::remove_dir_all(&dir).map_err(|err| format!("remove {}: {err}", dir.display()))?;
        if !hits.is_empty() {
            return Err(format!("{probe} was flagged: {hits:?}"));
        }
    }
    Ok(())
}
