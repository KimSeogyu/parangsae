//! Name resolution for owned-field writes and capability calls.
//!
//! Each cargo target is its own `crate::` namespace. Policy identity is the
//! package name. `use` aliases and `pub use` re-exports are followed. A write
//! or call the resolver cannot classify fails closed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    Fields, FnArg, ImplItem, Item, ItemFn, ItemMod, Local, Member, Pat, Type, UseTree, Visibility,
};

use crate::error::{Diagnostic, ToolError, CALL, CALL_UNRESOLVED, MUT, MUT_UNRESOLVED};
use crate::parse::Policy;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Struct {
        package: String,
        name: String,
        fields: Vec<String>,
    },
    Fn {
        name: String,
    },
    Module {
        path: Vec<String>,
    },
}

#[derive(Clone, Debug)]
struct Binding {
    is_pub: bool,
    local: bool,
    kind: Kind,
}

struct Target {
    package: String,
    key: String,
    rust_name: String,
    is_lib: bool,
    bindings: HashMap<Vec<String>, HashMap<String, Binding>>,
}

struct SourceFile {
    rel: String,
    target_key: String,
    module: Vec<String>,
    ast: syn::File,
}

struct PendingUse {
    target_key: String,
    module: Vec<String>,
    is_pub: bool,
    is_glob: bool,
    alias: String,
    path: Vec<String>,
    resolved: bool,
}

pub fn scan_ast(
    root: &Path,
    targets: &[(String, String, String, bool, PathBuf)],
    policy: &Policy,
    diags: &mut Vec<Diagnostic>,
) -> Result<Vec<PathBuf>, ToolError> {
    let mut reached = Vec::new();
    let mut loaded = Vec::new();
    for (package, target_name, rust_name, is_lib, src) in targets {
        let key = format!("{package}\u{0}{target_name}");
        let rel = rel_path(root, src);
        let mut batch = Vec::new();
        load_file(
            &mut LoadOut {
                root,
                target_key: &key,
                out: &mut batch,
                reached: &mut reached,
            },
            src,
            &rel,
            Vec::new(),
            true,
        )?;
        loaded.push((package.clone(), key, rust_name.clone(), *is_lib, batch));
    }
    let mut uses = Vec::new();
    let mut target_objs = Vec::new();
    let mut all_files = Vec::new();
    for (package, key, rust_name, is_lib, batch) in loaded {
        let mut bindings = HashMap::new();
        for file in &batch {
            collect_items(
                &file.ast.items,
                &file.module,
                &package,
                &key,
                &mut bindings,
                &mut uses,
            );
        }
        all_files.extend(batch);
        target_objs.push(Target {
            package,
            key,
            rust_name,
            is_lib,
            bindings,
        });
    }
    resolve_uses(&mut target_objs, &mut uses)?;
    for file in &all_files {
        let target = target_objs
            .iter()
            .find(|t| t.key == file.target_key)
            .expect("target inserted with its files");
        let mut walker = Walker {
            target,
            targets: &target_objs,
            policy,
            rel: &file.rel,
            module: file.module.clone(),
            self_ty: None,
            ns_stack: Vec::new(),
            value_stack: Vec::new(),
            diags,
        };
        walker.visit_file(&file.ast);
    }
    Ok(reached)
}

struct LoadOut<'a> {
    root: &'a Path,
    target_key: &'a str,
    out: &'a mut Vec<SourceFile>,
    reached: &'a mut Vec<PathBuf>,
}

fn load_file(
    files: &mut LoadOut<'_>,
    path: &Path,
    rel: &str,
    module: Vec<String>,
    crate_root: bool,
) -> Result<(), ToolError> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| ToolError::tool(format!("read {}: {err}", path.display())))?;
    let ast = syn::parse_file(&text).map_err(|err| {
        ToolError::parse(
            rel,
            err.span().start().line as u32,
            format!("unsupported Rust syntax: {err}"),
        )
    })?;
    if let Ok(canon) = path.canonicalize() {
        files.reached.push(canon);
    } else {
        files.reached.push(path.to_path_buf());
    }
    let file_module = module.clone();
    let target_key = files.target_key.to_owned();
    files.out.push(SourceFile {
        rel: rel.to_owned(),
        target_key,
        module: file_module,
        ast: ast.clone(),
    });
    for item in &ast.items {
        let Item::Mod(item_mod) = item else {
            continue;
        };
        if item_mod.content.is_some() {
            continue;
        }
        let child_path = child_file(
            path,
            &item_mod.ident.to_string(),
            path_attr(&item_mod.attrs),
            crate_root,
        )?;
        let child_rel = rel_path(files.root, &child_path);
        let mut child_mod = module.clone();
        child_mod.push(item_mod.ident.to_string());
        load_file(files, &child_path, &child_rel, child_mod, false)?;
    }
    Ok(())
}

fn child_file(
    current: &Path,
    name: &str,
    path_attr: Option<String>,
    crate_root: bool,
) -> Result<PathBuf, ToolError> {
    if let Some(rel) = path_attr {
        let joined = current.parent().unwrap_or(Path::new(".")).join(rel);
        if joined.is_file() {
            return Ok(joined);
        }
        return Err(ToolError::tool(format!(
            "missing #[path] module {}",
            joined.display()
        )));
    }
    let file_name = current
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    // A cargo target root keeps its modules beside the file, even when the
    // file is not named lib.rs. Nested files use the file-stem directory.
    let dir =
        if crate_root || file_name == "mod.rs" || file_name == "lib.rs" || file_name == "main.rs" {
            current.parent().unwrap_or(Path::new(".")).to_path_buf()
        } else {
            current
                .parent()
                .unwrap_or(Path::new("."))
                .join(current.file_stem().unwrap_or_default())
        };
    let flat = dir.join(format!("{name}.rs"));
    if flat.is_file() {
        return Ok(flat);
    }
    let nested = dir.join(name).join("mod.rs");
    if nested.is_file() {
        return Ok(nested);
    }
    Err(ToolError::tool(format!(
        "missing module {name} from {}",
        current.display()
    )))
}

fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("path") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(expr) = &nv.value {
                if let syn::Lit::Str(lit) = &expr.lit {
                    return Some(lit.value());
                }
            }
        }
    }
    None
}

fn collect_items(
    items: &[Item],
    module: &[String],
    package: &str,
    target_key: &str,
    bindings: &mut HashMap<Vec<String>, HashMap<String, Binding>>,
    uses: &mut Vec<PendingUse>,
) {
    for item in items {
        match item {
            Item::Struct(item) => {
                let fields = match &item.fields {
                    Fields::Named(named) => named
                        .named
                        .iter()
                        .filter_map(|field| field.ident.as_ref().map(|id| id.to_string()))
                        .collect(),
                    Fields::Unnamed(_) | Fields::Unit => Vec::new(),
                };
                insert_local(
                    bindings,
                    module,
                    item.ident.to_string(),
                    Binding {
                        is_pub: is_pub(&item.vis),
                        local: true,
                        kind: Kind::Struct {
                            package: package.to_owned(),
                            name: item.ident.to_string(),
                            fields,
                        },
                    },
                );
            }
            Item::Fn(item) => {
                insert_local(
                    bindings,
                    module,
                    item.sig.ident.to_string(),
                    Binding {
                        is_pub: is_pub(&item.vis),
                        local: true,
                        kind: Kind::Fn {
                            name: item.sig.ident.to_string(),
                        },
                    },
                );
            }
            Item::Use(item) => {
                let mut flat = Vec::new();
                flatten_use(&item.tree, &[], &mut flat);
                for (alias, path, is_glob) in flat {
                    uses.push(PendingUse {
                        target_key: target_key.to_owned(),
                        module: module.to_vec(),
                        is_pub: is_pub(&item.vis),
                        is_glob,
                        alias,
                        path,
                        resolved: false,
                    });
                }
            }
            Item::Mod(item) => {
                let mut child = module.to_vec();
                child.push(item.ident.to_string());
                insert_local(
                    bindings,
                    module,
                    item.ident.to_string(),
                    Binding {
                        is_pub: is_pub(&item.vis),
                        local: true,
                        kind: Kind::Module {
                            path: child.clone(),
                        },
                    },
                );
                if let Some((_, inline)) = &item.content {
                    collect_items(inline, &child, package, target_key, bindings, uses);
                }
            }
            _ => {}
        }
    }
}

fn insert_local(
    bindings: &mut HashMap<Vec<String>, HashMap<String, Binding>>,
    module: &[String],
    name: String,
    binding: Binding,
) {
    bindings
        .entry(module.to_vec())
        .or_default()
        .entry(name)
        .or_insert(binding);
}

fn flatten_use(tree: &UseTree, prefix: &[String], out: &mut Vec<(String, Vec<String>, bool)>) {
    match tree {
        UseTree::Path(path) => {
            let mut next = prefix.to_vec();
            next.push(path.ident.to_string());
            flatten_use(&path.tree, &next, out);
        }
        UseTree::Name(name) => {
            if name.ident == "self" {
                let alias = prefix.last().cloned().unwrap_or_else(|| "self".to_owned());
                out.push((alias, prefix.to_vec(), false));
            } else {
                let mut path = prefix.to_vec();
                path.push(name.ident.to_string());
                out.push((name.ident.to_string(), path, false));
            }
        }
        UseTree::Rename(rename) => {
            let mut path = prefix.to_vec();
            path.push(rename.ident.to_string());
            out.push((rename.rename.to_string(), path, false));
        }
        UseTree::Glob(_) => out.push((String::new(), prefix.to_vec(), true)),
        UseTree::Group(group) => {
            for item in &group.items {
                flatten_use(item, prefix, out);
            }
        }
    }
}

fn resolve_uses(targets: &mut [Target], uses: &mut [PendingUse]) -> Result<(), ToolError> {
    for _ in 0..64 {
        let mut progress = false;
        for pending in uses.iter_mut() {
            if pending.resolved {
                continue;
            }
            let Some(target_index) = targets.iter().position(|t| t.key == pending.target_key)
            else {
                continue;
            };
            if pending.is_glob {
                let imported = glob_bindings(targets, target_index, &pending.module, &pending.path);
                if imported.is_empty() && !is_external_prelude(&pending.path, targets) {
                    continue;
                }
                let is_pub = pending.is_pub;
                let module = pending.module.clone();
                let path = pending.path.clone();
                let mut added = false;
                for (name, mut binding) in imported {
                    binding.is_pub = is_pub && binding.is_pub;
                    binding.local = false;
                    if insert_imported(
                        &mut targets[target_index].bindings,
                        &module,
                        name,
                        binding,
                        true,
                    ) {
                        added = true;
                    }
                }
                if added || is_external_prelude(&path, targets) {
                    pending.resolved = true;
                    progress |= added;
                }
                continue;
            }
            let resolved =
                resolve_path(targets, target_index, &pending.module, &pending.path, false);
            if let Some(mut binding) = resolved {
                binding.is_pub = pending.is_pub;
                binding.local = false;
                let alias = pending.alias.clone();
                let module = pending.module.clone();
                let _inserted = insert_imported(
                    &mut targets[target_index].bindings,
                    &module,
                    alias,
                    binding,
                    false,
                );
                pending.resolved = true;
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    Ok(())
}

fn is_external_prelude(path: &[String], targets: &[Target]) -> bool {
    let Some(first) = path.first() else {
        return false;
    };
    !matches!(first.as_str(), "crate" | "self" | "super")
        && !targets
            .iter()
            .any(|t| t.is_lib && (t.rust_name == *first || t.package == *first))
}

fn glob_bindings(
    targets: &[Target],
    target_index: usize,
    module: &[String],
    path: &[String],
) -> Vec<(String, Binding)> {
    let source = if path == ["super"] {
        let parent = match module.split_last() {
            Some((_, parent)) => parent.to_vec(),
            None => return Vec::new(),
        };
        targets[target_index].bindings.get(&parent)
    } else if path == ["crate"] {
        targets[target_index].bindings.get(&Vec::new())
    } else if path == ["self"] {
        targets[target_index].bindings.get(module)
    } else if let Some(binding) = resolve_path(targets, target_index, module, path, true) {
        match binding.kind {
            Kind::Module { path } => targets[target_index].bindings.get(&path),
            _ => None,
        }
    } else if let Some(first) = path.first() {
        if let Some(lib) = lib_target(targets, first) {
            if path.len() == 1 {
                lib.bindings.get(&Vec::new())
            } else if let Some(binding) = walk(lib, &[], &path[1..], true) {
                match binding.kind {
                    Kind::Module { path } => lib.bindings.get(&path),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    source
        .map(|map| {
            map.iter()
                .filter(|(_, binding)| {
                    binding.is_pub || path == ["super"] || path == ["self"] || path == ["crate"]
                })
                .map(|(name, binding)| (name.clone(), binding.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn insert_imported(
    bindings: &mut HashMap<Vec<String>, HashMap<String, Binding>>,
    module: &[String],
    name: String,
    binding: Binding,
    glob: bool,
) -> bool {
    let map = bindings.entry(module.to_vec()).or_default();
    if let Some(existing) = map.get(&name) {
        if existing.local || glob {
            return false;
        }
        if existing.kind == binding.kind && existing.is_pub == binding.is_pub {
            return false;
        }
    }
    map.insert(name, binding);
    true
}

fn resolve_path(
    targets: &[Target],
    target_index: usize,
    module: &[String],
    path: &[String],
    external: bool,
) -> Option<Binding> {
    let first = path.first()?;
    match first.as_str() {
        "crate" => walk(&targets[target_index], &[], &path[1..], external),
        "self" => walk(&targets[target_index], module, &path[1..], external),
        "super" => {
            let mut current = module.to_vec();
            let mut rest = &path[1..];
            current.pop()?;
            while rest.first().map(String::as_str) == Some("super") {
                current.pop()?;
                rest = &rest[1..];
            }
            walk(&targets[target_index], &current, rest, external)
        }
        _ => {
            if let Some(binding) = lookup(&targets[target_index], module, first, false) {
                if path.len() == 1 {
                    return Some(binding);
                }
                if let Kind::Module { path: child } = binding.kind {
                    return walk(&targets[target_index], &child, &path[1..], external);
                }
            }
            let lib = lib_target(targets, first)?;
            walk(lib, &[], &path[1..], true)
        }
    }
}

fn walk(target: &Target, module: &[String], segs: &[String], external: bool) -> Option<Binding> {
    let first = segs.first()?;
    let binding = lookup(target, module, first, external)?;
    if segs.len() == 1 {
        return Some(binding);
    }
    match &binding.kind {
        Kind::Module { path } => walk(target, path, &segs[1..], external),
        _ => None,
    }
}

fn lookup(target: &Target, module: &[String], name: &str, external: bool) -> Option<Binding> {
    let binding = target.bindings.get(module)?.get(name)?.clone();
    if external && !binding.is_pub {
        return None;
    }
    Some(binding)
}

fn lib_target<'a>(targets: &'a [Target], name: &str) -> Option<&'a Target> {
    targets
        .iter()
        .find(|target| target.is_lib && (target.rust_name == name || target.package == name))
}

fn is_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

struct Walker<'a> {
    target: &'a Target,
    targets: &'a [Target],
    policy: &'a Policy,
    rel: &'a str,
    module: Vec<String>,
    self_ty: Option<TypeRef>,
    ns_stack: Vec<HashMap<String, Kind>>,
    value_stack: Vec<HashMap<String, Val>>,
    diags: &'a mut Vec<Diagnostic>,
}

#[derive(Clone)]
struct TypeRef {
    package: String,
    name: String,
}

#[derive(Clone)]
enum Val {
    Ty(TypeRef),
    Fn(String),
}

impl<'a> Walker<'a> {
    fn lookup_kind(&self, name: &str) -> Option<Kind> {
        for frame in self.ns_stack.iter().rev() {
            if let Some(kind) = frame.get(name) {
                return Some(kind.clone());
            }
        }
        self.target
            .bindings
            .get(&self.module)
            .and_then(|map| map.get(name))
            .map(|binding| binding.kind.clone())
    }

    fn lookup_val(&self, name: &str) -> Option<Val> {
        for frame in self.value_stack.iter().rev() {
            if let Some(val) = frame.get(name) {
                return Some(val.clone());
            }
        }
        None
    }

    fn resolve_kind(&self, segs: &[String]) -> Option<Kind> {
        if segs.len() == 1 {
            return self.lookup_kind(&segs[0]);
        }
        let target_index = self
            .targets
            .iter()
            .position(|target| target.key == self.target.key)?;
        resolve_path(self.targets, target_index, &self.module, segs, false)
            .map(|binding| binding.kind)
    }

    fn type_from_kind(&self, kind: Kind) -> Option<TypeRef> {
        match kind {
            Kind::Struct { package, name, .. } => Some(TypeRef { package, name }),
            _ => None,
        }
    }

    fn resolve_type(&self, ty: &Type) -> Option<TypeRef> {
        match ty {
            Type::Reference(inner) => self.resolve_type(&inner.elem),
            Type::Paren(inner) => self.resolve_type(&inner.elem),
            Type::Group(inner) => self.resolve_type(&inner.elem),
            Type::Path(path) => {
                let segs: Vec<String> = path
                    .path
                    .segments
                    .iter()
                    .map(|seg| seg.ident.to_string())
                    .collect();
                if segs.len() == 1 && segs[0] == "Self" {
                    return self.self_ty.clone();
                }
                self.resolve_kind(&segs)
                    .and_then(|kind| self.type_from_kind(kind))
            }
            _ => None,
        }
    }

    fn type_of_expr(&self, expr: &syn::Expr) -> Option<TypeRef> {
        match expr {
            syn::Expr::Path(path) if path.path.segments.len() == 1 => {
                let name = path.path.segments[0].ident.to_string();
                if name == "self" || name == "Self" {
                    return self.self_ty.clone();
                }
                match self.lookup_val(&name) {
                    Some(Val::Ty(ty)) => Some(ty),
                    _ => self
                        .lookup_kind(&name)
                        .and_then(|kind| self.type_from_kind(kind)),
                }
            }
            syn::Expr::Paren(inner) => self.type_of_expr(&inner.expr),
            syn::Expr::Group(inner) => self.type_of_expr(&inner.expr),
            syn::Expr::Reference(inner) => self.type_of_expr(&inner.expr),
            syn::Expr::Unary(inner) if matches!(inner.op, syn::UnOp::Deref(_)) => {
                self.type_of_expr(&inner.expr)
            }
            _ => None,
        }
    }

    fn owned_field(&self, field: &str) -> bool {
        self.policy.writers.iter().any(|pair| {
            pair.right
                .rsplit_once('.')
                .is_some_and(|(_, name)| name == field)
        })
    }

    fn note_write(&mut self, ty: Option<TypeRef>, field: &str, line: u32) {
        let Some(ty) = ty else {
            if self.owned_field(field) {
                self.diags
                    .push(unresolved_write(self.policy, self.rel, line, field));
            }
            return;
        };
        let key = format!("{}.{}", ty.name, field);
        let Some(writer) = self
            .policy
            .writers
            .iter()
            .find(|pair| pair.left == ty.package && pair.right == key)
        else {
            return;
        };
        if self.target.package == writer.left {
            return;
        }
        let allowed = self
            .policy
            .writers
            .iter()
            .filter(|pair| pair.right == key)
            .map(|pair| pair.left.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        self.diags.push(Diagnostic {
            rule_id: MUT,
            message: format!("{key} is written outside {}", writer.left),
            symbol: key,
            file: self.rel.to_owned(),
            line,
            allowed: if allowed.is_empty() {
                "(none)".to_owned()
            } else {
                allowed
            },
            spec_file: self.policy.file.clone(),
            spec_line: writer.line,
        });
    }

    fn check_place(&mut self, expr: &syn::Expr) {
        let mut cursor = expr;
        loop {
            cursor = match cursor {
                syn::Expr::Paren(inner) => &inner.expr,
                syn::Expr::Group(inner) => &inner.expr,
                syn::Expr::Field(field) => {
                    let name = member_name(&field.member);
                    let ty = self.type_of_expr(&field.base);
                    let line = line_of_span(field.member.span());
                    self.note_write(ty, &name, line);
                    &field.base
                }
                _ => break,
            };
        }
    }

    fn note_call(&mut self, path: &syn::Path, line: u32) {
        let segs: Vec<String> = path
            .segments
            .iter()
            .map(|seg| seg.ident.to_string())
            .collect();
        let fn_name = if segs.len() == 1 {
            match self.lookup_val(&segs[0]) {
                Some(Val::Fn(name)) => Some(name),
                _ => match self.lookup_kind(&segs[0]) {
                    Some(Kind::Fn { name }) => Some(name),
                    _ => None,
                },
            }
        } else {
            match self.resolve_kind(&segs) {
                Some(Kind::Fn { name }) => Some(name),
                _ => None,
            }
        };
        let Some(fn_name) = fn_name else {
            if segs
                .last()
                .is_some_and(|seg| self.policy.capabilities.contains_key(seg))
            {
                self.diags.push(unresolved_call(
                    self.policy,
                    self.rel,
                    line,
                    segs.last().unwrap(),
                ));
            }
            return;
        };
        if !self.policy.capabilities.contains_key(&fn_name) {
            return;
        }
        let allowed: Vec<&str> = self
            .policy
            .capability_callers
            .iter()
            .filter(|pair| pair.right == fn_name)
            .map(|pair| pair.left.as_str())
            .collect();
        if allowed.contains(&self.target.package.as_str()) {
            return;
        }
        let spec_line = self
            .policy
            .capability_callers
            .iter()
            .find(|pair| pair.right == fn_name)
            .map(|pair| pair.line)
            .unwrap_or_else(|| self.policy.line_of("AllowedCapabilityCallers"));
        let shown = if segs.len() == 1 && segs[0] != fn_name {
            format!("{} -> {fn_name}", segs[0])
        } else {
            format!("{} -> {fn_name}", segs.join("::"))
        };
        self.diags.push(Diagnostic {
            rule_id: CALL,
            message: format!(
                "capability {fn_name} is called from {}",
                self.target.package
            ),
            symbol: shown,
            file: self.rel.to_owned(),
            line,
            allowed: if allowed.is_empty() {
                "(none)".to_owned()
            } else {
                allowed.join(", ")
            },
            spec_file: self.policy.file.clone(),
            spec_line,
        });
    }

    fn bind_pat(&mut self, pat: &Pat, ty: Option<TypeRef>, slot: &mut HashMap<String, Val>) {
        match pat {
            Pat::Type(inner) => {
                let resolved = self.resolve_type(&inner.ty).or(ty);
                self.bind_pat(&inner.pat, resolved, slot);
            }
            Pat::Ident(ident) => {
                if let Some(ty) = ty {
                    slot.insert(ident.ident.to_string(), Val::Ty(ty));
                }
            }
            Pat::Reference(inner) => self.bind_pat(&inner.pat, ty, slot),
            _ => {}
        }
    }

    fn bind_sig(&mut self, sig: &syn::Signature) -> HashMap<String, Val> {
        let mut slot = HashMap::new();
        for input in &sig.inputs {
            let FnArg::Typed(typed) = input else {
                continue;
            };
            let ty = self.resolve_type(&typed.ty);
            self.bind_pat(&typed.pat, ty, &mut slot);
        }
        slot
    }
}

fn member_name(member: &Member) -> String {
    match member {
        Member::Named(ident) => ident.to_string(),
        Member::Unnamed(index) => index.index.to_string(),
    }
}

fn unresolved_write(policy: &Policy, file: &str, line: u32, field: &str) -> Diagnostic {
    Diagnostic {
        rule_id: MUT_UNRESOLVED,
        message: format!(
            "write to field {field} has no resolved type; an owned field of that name cannot pass silently"
        ),
        symbol: field.to_owned(),
        file: file.to_owned(),
        line,
        allowed: policy
            .writers
            .iter()
            .filter(|pair| pair.right.ends_with(&format!(".{field}")))
            .map(|pair| format!("{} {}", pair.left, pair.right))
            .collect::<Vec<_>>()
            .join(", "),
        spec_file: policy.file.clone(),
        spec_line: policy.line_of("AllowedStateWriters"),
    }
}

fn unresolved_call(policy: &Policy, file: &str, line: u32, name: &str) -> Diagnostic {
    Diagnostic {
        rule_id: CALL_UNRESOLVED,
        message: format!("call to capability {name} did not resolve"),
        symbol: name.to_owned(),
        file: file.to_owned(),
        line,
        allowed: "(unresolved)".to_owned(),
        spec_file: policy.file.clone(),
        spec_line: policy.line_of("Capabilities"),
    }
}

impl<'ast> Visit<'ast> for Walker<'_> {
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if let Some((_, items)) = &item.content {
            let saved_module = self.module.clone();
            let saved_ns = std::mem::take(&mut self.ns_stack);
            let saved_vals = std::mem::take(&mut self.value_stack);
            let saved_self = self.self_ty.clone();
            self.module.push(item.ident.to_string());
            for child in items {
                self.visit_item(child);
            }
            self.module = saved_module;
            self.ns_stack = saved_ns;
            self.value_stack = saved_vals;
            self.self_ty = saved_self;
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let saved = self.self_ty.clone();
        self.self_ty = self.resolve_type(&item.self_ty);
        syn::visit::visit_item_impl(self, item);
        self.self_ty = saved;
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let frame = self.bind_sig(&item.sig);
        self.value_stack.push(frame);
        syn::visit::visit_item_fn(self, item);
        self.value_stack.pop();
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if let ImplItem::Fn(func) = item {
            let frame = self.bind_sig(&func.sig);
            self.value_stack.push(frame);
            syn::visit::visit_impl_item(self, item);
            self.value_stack.pop();
            return;
        }
        syn::visit::visit_impl_item(self, item);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.ns_stack.push(HashMap::new());
        self.value_stack.push(HashMap::new());
        syn::visit::visit_block(self, block);
        self.value_stack.pop();
        self.ns_stack.pop();
    }

    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        if let syn::Stmt::Local(local) = stmt {
            self.visit_local(local);
            return;
        }
        if let syn::Stmt::Item(Item::Use(item)) = stmt {
            self.import_fn_use(item);
        }
        syn::visit::visit_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        match expr {
            syn::Expr::Assign(assign) => self.check_place(&assign.left),
            syn::Expr::Binary(bin) if is_assign_op(&bin.op) => self.check_place(&bin.left),
            syn::Expr::Reference(reference) if reference.mutability.is_some() => {
                self.check_place(&reference.expr);
            }
            syn::Expr::Call(call) => {
                if let syn::Expr::Path(path) = &*call.func {
                    let line = line_of_span(path.path.span());
                    self.note_call(&path.path, line);
                }
            }
            syn::Expr::MethodCall(call) => {
                let name = call.method.to_string();
                if self.policy.capabilities.contains_key(&name)
                    && !self
                        .policy
                        .capability_callers
                        .iter()
                        .any(|pair| pair.right == name && pair.left == self.target.package)
                {
                    let line = line_of_span(call.method.span());
                    self.diags.push(Diagnostic {
                        rule_id: CALL,
                        message: format!(
                            "capability {name} is called from {}",
                            self.target.package
                        ),
                        symbol: name.clone(),
                        file: self.rel.to_owned(),
                        line,
                        allowed: self
                            .policy
                            .capability_callers
                            .iter()
                            .filter(|pair| pair.right == name)
                            .map(|pair| pair.left.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        spec_file: self.policy.file.clone(),
                        spec_line: self.policy.line_of("AllowedCapabilityCallers"),
                    });
                }
            }
            syn::Expr::Struct(item) => {
                let segs: Vec<String> = item
                    .path
                    .segments
                    .iter()
                    .map(|seg| seg.ident.to_string())
                    .collect();
                let ty = if segs.len() == 1 && segs[0] == "Self" {
                    self.self_ty.clone()
                } else {
                    self.resolve_kind(&segs)
                        .and_then(|kind| self.type_from_kind(kind))
                };
                for field in &item.fields {
                    if let Member::Named(name) = &field.member {
                        self.note_write(ty.clone(), &name.to_string(), line_of_span(name.span()));
                    }
                }
            }
            syn::Expr::Closure(closure) => {
                let mut frame = HashMap::new();
                for pat in &closure.inputs {
                    self.bind_pat(pat, None, &mut frame);
                }
                self.value_stack.push(frame);
                syn::visit::visit_expr_closure(self, closure);
                self.value_stack.pop();
                return;
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
}

impl Walker<'_> {
    fn visit_local(&mut self, local: &Local) {
        if let Some(init) = &local.init {
            self.visit_expr(&init.expr);
            if let syn::Expr::Path(path) = &*init.expr {
                let segs: Vec<String> = path
                    .path
                    .segments
                    .iter()
                    .map(|seg| seg.ident.to_string())
                    .collect();
                if let Some(Kind::Fn { name }) = self.resolve_kind(&segs) {
                    if let Pat::Ident(ident) = &local.pat {
                        if let Some(frame) = self.value_stack.last_mut() {
                            frame.insert(ident.ident.to_string(), Val::Fn(name));
                        }
                    }
                }
            }
        }
        let mut frame_extra = HashMap::new();
        self.bind_pat(&local.pat, None, &mut frame_extra);
        if let Some(frame) = self.value_stack.last_mut() {
            frame.extend(frame_extra);
        }
    }

    fn import_fn_use(&mut self, item: &syn::ItemUse) {
        let mut flat = Vec::new();
        flatten_use(&item.tree, &[], &mut flat);
        let target_index = self
            .targets
            .iter()
            .position(|target| target.key == self.target.key);
        let Some(target_index) = target_index else {
            return;
        };
        for (alias, path, is_glob) in flat {
            if is_glob {
                continue;
            }
            if let Some(binding) =
                resolve_path(self.targets, target_index, &self.module, &path, false)
            {
                if let Some(frame) = self.ns_stack.last_mut() {
                    frame.insert(alias, binding.kind);
                }
            }
        }
    }
}

fn is_assign_op(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
            | syn::BinOp::BitXorAssign(_)
            | syn::BinOp::BitAndAssign(_)
            | syn::BinOp::BitOrAssign(_)
            | syn::BinOp::ShlAssign(_)
            | syn::BinOp::ShrAssign(_)
    )
}

pub fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn line_of_span(span: proc_macro2::Span) -> u32 {
    let line = u32::try_from(span.start().line).unwrap_or(u32::MAX);
    if line == 0 {
        1
    } else {
        line
    }
}
