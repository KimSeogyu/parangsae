//! Effect and float scans. `#[allow]` and comments are not exemptions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use syn::visit::Visit;
use syn::{Expr, Type, UseTree};

use crate::error::{Diagnostic, EFFECT, FLOAT};
use crate::parse::Policy;
use crate::scan::rel_path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    Net,
    FsWrite,
    Process,
    Env,
}

impl Effect {
    pub fn name(self) -> &'static str {
        match self {
            Self::Net => "net",
            Self::FsWrite => "fs_write",
            Self::Process => "process",
            Self::Env => "env",
        }
    }
}

pub struct Hit {
    pub effect: Effect,
    pub symbol: String,
    pub file: String,
    pub line: u32,
}

pub fn dep_effect(name: &str, features: &[String], default_features: bool) -> Option<Effect> {
    if matches!(
        name,
        "reqwest" | "hyper" | "hyper-util" | "tungstenite" | "tokio-tungstenite" | "socket2"
    ) {
        return Some(Effect::Net);
    }
    if name == "tokio"
        && (default_features
            || features
                .iter()
                .any(|feature| feature == "net" || feature == "full"))
    {
        return Some(Effect::Net);
    }
    None
}

pub fn effects_in_source(rel: &str, text: &str) -> Result<Vec<Hit>, crate::error::ToolError> {
    let ast = syn::parse_file(text).map_err(|err| {
        crate::error::ToolError::parse(rel, 1, format!("unsupported Rust syntax: {err}"))
    })?;
    let mut aliases = BTreeMap::new();
    let mut collector = UseCollector {
        aliases: &mut aliases,
    };
    collector.visit_file(&ast);
    let mut walker = PathWalker {
        aliases: &aliases,
        rel,
        hits: Vec::new(),
    };
    walker.visit_file(&ast);
    Ok(walker.hits)
}

pub fn float_tokens(rel: &str, text: &str) -> Vec<Hit> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut line = 1u32;
    let mut hits = Vec::new();
    while index < bytes.len() {
        let c = bytes[index];
        if c == b'\n' {
            line += 1;
            index += 1;
            continue;
        }
        if c == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if c == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            let mut depth = 1;
            while index < bytes.len() && depth > 0 {
                if bytes[index] == b'\n' {
                    line += 1;
                }
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    depth += 1;
                    index += 2;
                    continue;
                }
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    depth -= 1;
                    index += 2;
                    continue;
                }
                index += 1;
            }
            continue;
        }
        if c == b'b' && is_raw_prefix(bytes, index) {
            if let Some(next) = skip_raw(bytes, index, &mut line) {
                index = next;
                continue;
            }
        }
        if c == b'r' && (bytes.get(index + 1) == Some(&b'"') || bytes.get(index + 1) == Some(&b'#'))
        {
            if let Some(next) = skip_raw(bytes, index, &mut line) {
                index = next;
                continue;
            }
        }
        if c == b'"' {
            index = skip_string(bytes, index + 1, &mut line);
            continue;
        }
        if c == b'\'' {
            index = skip_char_or_lifetime(bytes, index, &mut line);
            continue;
        }
        if c.is_ascii_digit() {
            let start_line = line;
            let (next, suffix) = scan_number(bytes, index);
            if suffix == Some("f64") || suffix == Some("f32") {
                hits.push(Hit {
                    effect: Effect::Net,
                    symbol: suffix.unwrap_or("f64").to_owned(),
                    file: rel.to_owned(),
                    line: start_line,
                });
            }
            index = next;
            continue;
        }
        if is_ident_start(c) {
            let start = index;
            index += 1;
            while index < bytes.len() && is_ident_continue(bytes[index]) {
                index += 1;
            }
            let ident = &text[start..index];
            if ident == "f64" || ident == "f32" {
                hits.push(Hit {
                    effect: Effect::Net,
                    symbol: ident.to_owned(),
                    file: rel.to_owned(),
                    line,
                });
            }
            continue;
        }
        index += 1;
    }
    hits
}

pub fn float_diags(policy: &Policy, hits: &[(String, Hit)]) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (package, hit) in hits {
        if policy.float_allowed.contains_key(package) {
            continue;
        }
        diags.push(Diagnostic {
            rule_id: FLOAT,
            message: format!("{package} uses {} outside FloatAllowed", hit.symbol),
            symbol: hit.symbol.clone(),
            file: hit.file.clone(),
            line: hit.line,
            allowed: if policy.float_allowed.is_empty() {
                "(none)".to_owned()
            } else {
                policy
                    .float_allowed
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            spec_file: policy.file.clone(),
            spec_line: policy.line_of("FloatAllowed"),
        });
    }
    diags.sort_by(|a, b| (&a.file, a.line, &a.symbol).cmp(&(&b.file, b.line, &b.symbol)));
    diags.dedup();
    diags
}

pub fn effect_diags(policy: &Policy, package: &str, hits: &[Hit]) -> Vec<Diagnostic> {
    let mut seen = BTreeSet::new();
    let mut diags = Vec::new();
    for hit in hits {
        if !seen.insert(hit.effect) {
            continue;
        }
        let name = hit.effect.name();
        if policy
            .allowed_effects
            .iter()
            .any(|pair| pair.left == package && pair.right == name)
        {
            continue;
        }
        let allowed = policy
            .allowed_effects
            .iter()
            .filter(|pair| pair.right == name)
            .map(|pair| pair.left.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let spec_line = policy
            .allowed_effects
            .iter()
            .find(|pair| pair.right == name)
            .map(|pair| pair.line)
            .unwrap_or_else(|| policy.line_of("AllowedEffects"));
        diags.push(Diagnostic {
            rule_id: EFFECT,
            message: format!("{package} uses effect {name}"),
            symbol: hit.symbol.clone(),
            file: hit.file.clone(),
            line: hit.line,
            allowed: if allowed.is_empty() {
                "(none)".to_owned()
            } else {
                allowed
            },
            spec_file: policy.file.clone(),
            spec_line,
        });
    }
    diags
}

struct UseCollector<'a> {
    aliases: &'a mut BTreeMap<String, Vec<String>>,
}

impl Visit<'_> for UseCollector<'_> {
    fn visit_item_use(&mut self, item: &syn::ItemUse) {
        let mut flat = Vec::new();
        flatten(&item.tree, &[], &mut flat);
        for (alias, path) in flat {
            self.aliases.insert(alias, path);
        }
    }
}

fn flatten(tree: &UseTree, prefix: &[String], out: &mut Vec<(String, Vec<String>)>) {
    match tree {
        UseTree::Path(path) => {
            let mut next = prefix.to_vec();
            next.push(path.ident.to_string());
            flatten(&path.tree, &next, out);
        }
        UseTree::Name(name) => {
            if name.ident == "self" {
                if let Some(alias) = prefix.last() {
                    out.push((alias.clone(), prefix.to_vec()));
                }
            } else {
                let mut path = prefix.to_vec();
                path.push(name.ident.to_string());
                out.push((name.ident.to_string(), path));
            }
        }
        UseTree::Rename(rename) => {
            let mut path = prefix.to_vec();
            path.push(rename.ident.to_string());
            out.push((rename.rename.to_string(), path));
        }
        UseTree::Glob(_) => {}
        UseTree::Group(group) => {
            for item in &group.items {
                flatten(item, prefix, out);
            }
        }
    }
}

struct PathWalker<'a> {
    aliases: &'a BTreeMap<String, Vec<String>>,
    rel: &'a str,
    hits: Vec<Hit>,
}

impl PathWalker<'_> {
    fn note_path(&mut self, segments: &[String], line: usize) {
        if segments.is_empty() {
            return;
        }
        let expanded = if let Some(prefix) = self.aliases.get(&segments[0]) {
            let mut full = prefix.clone();
            full.extend(segments.iter().skip(1).cloned());
            full
        } else {
            segments.to_vec()
        };
        if let Some((effect, symbol)) = classify(&expanded) {
            self.hits.push(Hit {
                effect,
                symbol,
                file: self.rel.to_owned(),
                line: u32::try_from(line).unwrap_or(u32::MAX),
            });
        }
    }
}

impl<'ast> Visit<'ast> for PathWalker<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let Expr::Path(path) = expr {
            let segs: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|seg| seg.ident.to_string())
                .collect();
            let line = path
                .path
                .segments
                .first()
                .map(|seg| seg.ident.span().start().line)
                .unwrap_or(1);
            self.note_path(&segs, line);
        }
        syn::visit::visit_expr(self, expr);
    }

    fn visit_type(&mut self, ty: &'ast Type) {
        if let Type::Path(path) = ty {
            let segs: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|seg| seg.ident.to_string())
                .collect();
            let line = path
                .path
                .segments
                .first()
                .map(|seg| seg.ident.span().start().line)
                .unwrap_or(1);
            self.note_path(&segs, line);
        }
        syn::visit::visit_type(self, ty);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut flat = Vec::new();
        flatten(&item.tree, &[], &mut flat);
        let line = item.use_token.span.start().line;
        for (_alias, path) in flat {
            self.note_path(&path, line);
        }
        syn::visit::visit_item_use(self, item);
    }
}

fn classify(segs: &[String]) -> Option<(Effect, String)> {
    let symbol = segs.join("::");
    let head = segs.first().map(String::as_str)?;
    if head == "reqwest" || head == "hyper" || head == "tungstenite" {
        return Some((Effect::Net, symbol));
    }
    if segs.len() >= 2 && segs[0] == "tokio" && segs[1] == "net" {
        return Some((Effect::Net, symbol));
    }
    if segs.len() >= 2 && segs[0] == "std" && segs[1] == "net" {
        return Some((Effect::Net, symbol));
    }
    if segs.len() >= 2 && segs[0] == "std" && segs[1] == "fs" {
        return fs_effect(&segs[2..]).map(|effect| (effect, symbol));
    }
    if segs.len() >= 2 && segs[0] == "std" && segs[1] == "process" {
        return process_effect(&segs[2..]).map(|effect| (effect, symbol));
    }
    if segs.len() >= 2 && segs[0] == "std" && segs[1] == "env" {
        return env_effect(&segs[2..]).map(|effect| (effect, symbol));
    }
    None
}

fn fs_effect(rest: &[String]) -> Option<Effect> {
    let name = rest.first().map(String::as_str)?;
    const WRITE: &[&str] = &[
        "write",
        "create_dir",
        "create_dir_all",
        "remove_file",
        "remove_dir",
        "remove_dir_all",
        "copy",
        "rename",
        "hard_link",
        "soft_link",
        "set_permissions",
    ];
    if WRITE.contains(&name) || name == "OpenOptions" {
        return Some(Effect::FsWrite);
    }
    if name == "File"
        && rest
            .get(1)
            .map(String::as_str)
            .is_some_and(|method| method == "create" || method == "create_new")
    {
        return Some(Effect::FsWrite);
    }
    None
}

fn process_effect(rest: &[String]) -> Option<Effect> {
    match rest.first().map(String::as_str) {
        Some("Command" | "Child" | "Stdio" | "Output" | "exit") => Some(Effect::Process),
        _ => None,
    }
}

fn env_effect(rest: &[String]) -> Option<Effect> {
    match rest.first().map(String::as_str) {
        Some(
            "var" | "var_os" | "set_var" | "remove_var" | "vars" | "vars_os" | "args" | "args_os"
            | "current_dir" | "set_current_dir" | "temp_dir" | "current_exe" | "consts",
        ) => Some(Effect::Env),
        _ => None,
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn scan_number(bytes: &[u8], start: usize) -> (usize, Option<&str>) {
    let mut index = start;
    while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
        index += 1;
    }
    if index < bytes.len()
        && bytes[index] == b'.'
        && bytes.get(index + 1).is_some_and(|c| c.is_ascii_digit())
    {
        index += 1;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
            index += 1;
        }
    }
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        let mut probe = index + 1;
        if bytes.get(probe) == Some(&b'+') || bytes.get(probe) == Some(&b'-') {
            probe += 1;
        }
        if bytes.get(probe).is_some_and(|c| c.is_ascii_digit()) {
            index = probe;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
                index += 1;
            }
        }
    }
    let suffix_start = index;
    if index < bytes.len() && is_ident_start(bytes[index]) {
        index += 1;
        while index < bytes.len() && is_ident_continue(bytes[index]) {
            index += 1;
        }
        let suffix = std::str::from_utf8(&bytes[suffix_start..index]).unwrap_or("");
        return (index, Some(suffix));
    }
    (index, None)
}

fn skip_string(bytes: &[u8], mut index: usize, line: &mut u32) -> usize {
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            *line += 1;
        }
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == b'"' {
            return index + 1;
        }
        index += 1;
    }
    index
}

fn skip_char_or_lifetime(bytes: &[u8], index: usize, line: &mut u32) -> usize {
    let next = index + 1;
    if bytes.get(next).is_some_and(|c| is_ident_start(*c)) {
        let mut end = next + 1;
        while end < bytes.len() && is_ident_continue(bytes[end]) {
            end += 1;
        }
        if bytes.get(end) != Some(&b'\'') {
            return end;
        }
    }
    let mut end = next;
    if bytes.get(end) == Some(&b'\\') {
        end += 2;
    } else {
        end += 1;
    }
    if bytes.get(end) == Some(&b'\'') {
        end += 1;
    }
    if bytes.get(index + 1) == Some(&b'\n') {
        *line += 1;
    }
    end
}

fn is_raw_prefix(bytes: &[u8], index: usize) -> bool {
    bytes.get(index) == Some(&b'b')
        && (bytes.get(index + 1) == Some(&b'r') || bytes.get(index + 1) == Some(&b'"'))
}

fn skip_raw(bytes: &[u8], index: usize, line: &mut u32) -> Option<usize> {
    let mut cursor = index;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        if bytes.get(cursor) == Some(&b'"') {
            return Some(skip_string(bytes, cursor + 1, line));
        }
        return None;
    }
    cursor += 1;
    let mut hashes = 0;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    cursor += 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\n' {
            *line += 1;
        }
        if bytes[cursor] == b'"' {
            let mut ok = true;
            for step in 1..=hashes {
                if bytes.get(cursor + step) != Some(&b'#') {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Some(cursor + 1 + hashes);
            }
        }
        cursor += 1;
    }
    Some(cursor)
}

pub fn package_rs_files(root: &Path, manifest: &Path) -> Vec<(String, String)> {
    let dir = manifest.parent().unwrap_or(root);
    let mut out = Vec::new();
    walk_rs(dir, root, &mut out);
    out.sort();
    out
}

fn walk_rs(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_rs(&path, root, out);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            let rel = rel_path(root, &path);
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            out.push((rel, text));
        }
    }
}
