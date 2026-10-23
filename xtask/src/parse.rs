//! Restricted TLA+ profile for architecture policy modules.
//!
//! Accepted: a module header, `EXTENDS` of a known standard module, operators
//! whose right-hand side is a finite set of strings or of string pairs, one
//! clock scaffold, and a `WellFormed` predicate built from the shapes this
//! file evaluates. Any other construct is [`crate::error::PARSE`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::error::ToolError;

const EXTENDS_OK: &[&str] = &["FiniteSets", "Integers", "Naturals", "Sequences", "TLC"];
const DATA_OPS: &[&str] = &[
    "AllowedCapabilityCallers",
    "AllowedDependencies",
    "AllowedEffects",
    "AllowedStateWriters",
    "Capabilities",
    "Effects",
    "FloatAllowed",
    "Modules",
    "Owners",
    "PathOwners",
];
const STRING_OPS: &[&str] = &[
    "Capabilities",
    "Effects",
    "FloatAllowed",
    "Modules",
    "Owners",
];
const PAIR_OPS: &[&str] = &[
    "AllowedCapabilityCallers",
    "AllowedDependencies",
    "AllowedEffects",
    "AllowedStateWriters",
    "PathOwners",
];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pair {
    pub left: String,
    pub right: String,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub file: String,
    pub modules: BTreeMap<String, u32>,
    pub owners: BTreeMap<String, u32>,
    pub effects: BTreeMap<String, u32>,
    pub capabilities: BTreeMap<String, u32>,
    pub float_allowed: BTreeMap<String, u32>,
    pub dependencies: Vec<Pair>,
    pub writers: Vec<Pair>,
    pub capability_callers: Vec<Pair>,
    pub allowed_effects: Vec<Pair>,
    pub path_owners: Vec<Pair>,
    pub operator_line: BTreeMap<String, u32>,
}

impl Policy {
    pub fn line_of(&self, op: &str) -> u32 {
        self.operator_line.get(op).copied().unwrap_or(1)
    }

    pub fn string_set(&self, name: &str) -> Option<&BTreeMap<String, u32>> {
        match name {
            "Modules" => Some(&self.modules),
            "Owners" => Some(&self.owners),
            "Effects" => Some(&self.effects),
            "Capabilities" => Some(&self.capabilities),
            "FloatAllowed" => Some(&self.float_allowed),
            _ => None,
        }
    }

    pub fn pair_set(&self, name: &str) -> Option<&[Pair]> {
        match name {
            "AllowedDependencies" => Some(&self.dependencies),
            "AllowedStateWriters" => Some(&self.writers),
            "AllowedCapabilityCallers" => Some(&self.capability_callers),
            "AllowedEffects" => Some(&self.allowed_effects),
            "PathOwners" => Some(&self.path_owners),
            _ => None,
        }
    }
}

pub fn parse_policy(file: &str, text: &str) -> Result<Policy, ToolError> {
    let tokens = tokenize(file, text)?;
    let mut p = Parser {
        file,
        toks: &tokens,
        i: 0,
    };
    let mut policy = p.parse_module()?;
    fill_element_lines(text, &mut policy);
    Ok(policy)
}

#[derive(Clone, Debug)]
enum Tok {
    Dash,
    Footer,
    Ident(String),
    Str(String),
    Num(i64),
    EqEq,
    Eq,
    Ne,
    In,
    SubsetEq,
    And,
    Or,
    Forall,
    Prime,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBrack,
    RBrack,
    Box,
    LTuple,
    RTuple,
    Comma,
    Colon,
    Under,
}

#[derive(Clone, Debug)]
enum Expr {
    Ident(String),
    Str(String),
    Num(i64),
    Prime(String),
    Index(Box<Expr>, i64),
    Eq(Box<Expr>, Box<Expr>),
    Ne(Box<Expr>, Box<Expr>),
    In(Box<Expr>, Box<Expr>),
    SubsetEq(Box<Expr>, Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Forall {
        var: String,
        set: Box<Expr>,
        body: Box<Expr>,
    },
    Card(Box<Expr>),
    Set(Vec<Expr>),
    Tuple(Vec<Expr>),
    Comp {
        elem: Box<Expr>,
        var: String,
        set: Box<Expr>,
        pred: Box<Expr>,
    },
    BoxAction {
        action: String,
        subscript: String,
    },
    Paren(Box<Expr>),
}

struct Parser<'a> {
    file: &'a str,
    toks: &'a [Located],
    i: usize,
}

struct Located {
    tok: Tok,
    line: u32,
}

impl<'a> Parser<'a> {
    fn parse_module(&mut self) -> Result<Policy, ToolError> {
        self.expect_kind(|t| matches!(t, Tok::Dash), "module header")?;
        self.expect_ident("MODULE")?;
        let _name = self.expect_any_ident()?;
        self.expect_kind(|t| matches!(t, Tok::Dash), "module header")?;
        self.expect_ident("EXTENDS")?;
        let ext = self.expect_any_ident()?;
        if !EXTENDS_OK.contains(&ext.as_str()) {
            return self.err(format!(
                "unsupported TLA+ construct: EXTENDS {ext} is outside the profile"
            ));
        }
        while self.eat_kind(|t| matches!(t, Tok::Comma)) {
            let ext = self.expect_any_ident()?;
            if !EXTENDS_OK.contains(&ext.as_str()) {
                return self.err(format!(
                    "unsupported TLA+ construct: EXTENDS {ext} is outside the profile"
                ));
            }
        }

        let mut defs: Vec<(String, u32, Expr)> = Vec::new();
        let mut saw_clock = false;
        while !self.eat_kind(|t| matches!(t, Tok::Footer)) {
            if self.eat_ident("VARIABLE") || self.eat_ident("VARIABLES") {
                let name = self.expect_any_ident()?;
                if name != "clock" || saw_clock {
                    return self
                        .err("unsupported TLA+ construct: the scaffold has one VARIABLE clock");
                }
                saw_clock = true;
                continue;
            }
            let (name, line) = self.expect_any_ident_at()?;
            self.expect_kind(|t| matches!(t, Tok::EqEq), "==")?;
            let expr = self.parse_expr()?;
            if defs.iter().any(|(n, _, _)| n == &name) {
                return self.err(format!("duplicate operator {name}"));
            }
            defs.push((name, line, expr));
        }
        if self.peek_tok().is_some() {
            return self.err("unsupported TLA+ construct after the module footer");
        }
        if !saw_clock {
            return Err(ToolError::parse(
                self.file,
                1,
                "policy module is missing VARIABLE clock",
            ));
        }
        self.finish(defs)
    }

    fn finish(&self, defs: Vec<(String, u32, Expr)>) -> Result<Policy, ToolError> {
        let mut policy = Policy {
            file: self.file.to_owned(),
            modules: BTreeMap::new(),
            owners: BTreeMap::new(),
            effects: BTreeMap::new(),
            capabilities: BTreeMap::new(),
            float_allowed: BTreeMap::new(),
            dependencies: Vec::new(),
            writers: Vec::new(),
            capability_callers: Vec::new(),
            allowed_effects: Vec::new(),
            path_owners: Vec::new(),
            operator_line: BTreeMap::new(),
        };
        let mut well: Option<&Expr> = None;
        for (name, line, expr) in &defs {
            policy.operator_line.insert(name.clone(), *line);
            match name.as_str() {
                "Init" => check_init(self.file, expr)?,
                "Next" => check_next(self.file, expr)?,
                "Spec" => check_spec(self.file, expr)?,
                "WellFormed" => well = Some(expr),
                other if DATA_OPS.contains(&other) => {
                    self.store_data(&mut policy, other, expr)?;
                }
                other => {
                    return self.err(format!(
                        "unsupported TLA+ construct: operator {other} is outside the profile"
                    ));
                }
            }
        }
        for op in DATA_OPS {
            if !policy.operator_line.contains_key(*op) {
                return Err(ToolError::parse(
                    self.file,
                    1,
                    format!("policy module is missing {op}"),
                ));
            }
        }
        for op in ["Init", "Next", "Spec", "WellFormed"] {
            if !policy.operator_line.contains_key(op) {
                return Err(ToolError::parse(
                    self.file,
                    1,
                    format!("policy module is missing {op}"),
                ));
            }
        }
        let well = well.ok_or_else(|| ToolError::parse(self.file, 1, "missing WellFormed"))?;
        eval_well(self.file, well, &policy)?;
        reject_unknown_effects(&policy)?;
        reject_bad_capability_names(&policy)?;
        Ok(policy)
    }

    fn store_data(&self, policy: &mut Policy, name: &str, expr: &Expr) -> Result<(), ToolError> {
        if STRING_OPS.contains(&name) {
            let set = as_string_set(self.file, expr).map_err(|err| {
                ToolError::parse(
                    self.file,
                    policy.line_of(name),
                    format!("{name} must be a finite set of strings: {err}"),
                )
            })?;
            let map = match name {
                "Modules" => &mut policy.modules,
                "Owners" => &mut policy.owners,
                "Effects" => &mut policy.effects,
                "Capabilities" => &mut policy.capabilities,
                "FloatAllowed" => &mut policy.float_allowed,
                _ => unreachable!(),
            };
            for (value, line) in set {
                map.entry(value).or_insert(line);
            }
            return Ok(());
        }
        if PAIR_OPS.contains(&name) {
            let set = as_pair_set(self.file, expr).map_err(|err| {
                ToolError::parse(
                    self.file,
                    policy.line_of(name),
                    format!("{name} must be a finite set of string pairs: {err}"),
                )
            })?;
            let dst = match name {
                "AllowedDependencies" => &mut policy.dependencies,
                "AllowedStateWriters" => &mut policy.writers,
                "AllowedCapabilityCallers" => &mut policy.capability_callers,
                "AllowedEffects" => &mut policy.allowed_effects,
                "PathOwners" => &mut policy.path_owners,
                _ => unreachable!(),
            };
            *dst = set;
            dst.sort();
            dst.dedup();
            return Ok(());
        }
        self.err(format!("unsupported operator {name}"))
    }

    fn parse_expr(&mut self) -> Result<Expr, ToolError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ToolError> {
        let leading = self.eat_kind(|t| matches!(t, Tok::Or));
        let mut items = vec![self.parse_and()?];
        while self.eat_kind(|t| matches!(t, Tok::Or)) {
            items.push(self.parse_and()?);
        }
        if items.len() == 1 && !leading {
            Ok(items.pop().unwrap())
        } else {
            Ok(Expr::Or(items))
        }
    }

    fn parse_and(&mut self) -> Result<Expr, ToolError> {
        let leading = self.eat_kind(|t| matches!(t, Tok::And));
        let mut items = vec![self.parse_cmp()?];
        while self.eat_kind(|t| matches!(t, Tok::And)) {
            items.push(self.parse_cmp()?);
        }
        if items.len() == 1 && !leading {
            Ok(items.pop().unwrap())
        } else {
            Ok(Expr::And(items))
        }
    }

    fn parse_cmp(&mut self) -> Result<Expr, ToolError> {
        let left = self.parse_unary()?;
        if self.eat_kind(|t| matches!(t, Tok::Eq)) {
            return Ok(Expr::Eq(Box::new(left), Box::new(self.parse_unary()?)));
        }
        if self.eat_kind(|t| matches!(t, Tok::Ne)) {
            return Ok(Expr::Ne(Box::new(left), Box::new(self.parse_unary()?)));
        }
        if self.eat_kind(|t| matches!(t, Tok::In)) {
            return Ok(Expr::In(Box::new(left), Box::new(self.parse_unary()?)));
        }
        if self.eat_kind(|t| matches!(t, Tok::SubsetEq)) {
            return Ok(Expr::SubsetEq(
                Box::new(left),
                Box::new(self.parse_unary()?),
            ));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ToolError> {
        let mut expr = self.parse_primary()?;
        if self.eat_kind(|t| matches!(t, Tok::Prime)) {
            let Expr::Ident(name) = expr else {
                return self.err("unsupported TLA+ construct: prime on a non-name");
            };
            expr = Expr::Prime(name);
        }
        while self.eat_kind(|t| matches!(t, Tok::LBrack)) {
            let n = self.expect_num()?;
            self.expect_kind(|t| matches!(t, Tok::RBrack), "]")?;
            expr = Expr::Index(Box::new(expr), n);
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ToolError> {
        if self.eat_kind(|t| matches!(t, Tok::Forall)) {
            return self.parse_forall();
        }
        if self.eat_kind(|t| matches!(t, Tok::LParen)) {
            let inner = self.parse_expr()?;
            self.expect_kind(|t| matches!(t, Tok::RParen), ")")?;
            return Ok(Expr::Paren(Box::new(inner)));
        }
        if self.eat_kind(|t| matches!(t, Tok::LBrace)) {
            return self.parse_set();
        }
        if self.eat_kind(|t| matches!(t, Tok::LTuple)) {
            return self.parse_tuple();
        }
        if self.eat_kind(|t| matches!(t, Tok::Box)) {
            self.expect_kind(|t| matches!(t, Tok::LBrack), "[")?;
            let action = self.expect_any_ident()?;
            self.expect_kind(|t| matches!(t, Tok::RBrack), "]")?;
            self.expect_kind(|t| matches!(t, Tok::Under), "_")?;
            let subscript = self.expect_any_ident()?;
            return Ok(Expr::BoxAction { action, subscript });
        }
        match self.peek_tok() {
            Some(Tok::Ident(name)) => {
                let name = name.clone();
                self.bump();
                if name == "Cardinality" && self.eat_kind(|t| matches!(t, Tok::LParen)) {
                    let inner = self.parse_expr()?;
                    self.expect_kind(|t| matches!(t, Tok::RParen), ")")?;
                    return Ok(Expr::Card(Box::new(inner)));
                }
                if matches!(
                    name.as_str(),
                    "CHOOSE"
                        | "IF"
                        | "THEN"
                        | "ELSE"
                        | "CASE"
                        | "LET"
                        | "IN"
                        | "INSTANCE"
                        | "WITH"
                        | "EXCEPT"
                        | "UNION"
                        | "SUBSET"
                        | "DOMAIN"
                        | "ENABLED"
                        | "UNCHANGED"
                        | "ASSUME"
                        | "AXIOM"
                        | "THEOREM"
                        | "LEMMA"
                        | "LOCAL"
                        | "CONSTANT"
                        | "CONSTANTS"
                        | "RECURSIVE"
                        | "WF_"
                        | "SF_"
                ) {
                    return self.err(format!(
                        "unsupported TLA+ construct: {name} is outside the profile"
                    ));
                }
                Ok(Expr::Ident(name))
            }
            Some(Tok::Str(s)) => {
                let s = s.clone();
                self.bump();
                Ok(Expr::Str(s))
            }
            Some(Tok::Num(n)) => {
                let n = *n;
                self.bump();
                Ok(Expr::Num(n))
            }
            _ => self.err("unsupported TLA+ construct"),
        }
    }

    fn parse_forall(&mut self) -> Result<Expr, ToolError> {
        let var = self.expect_any_ident()?;
        self.expect_kind(|t| matches!(t, Tok::In), "\\in")?;
        let set = self.parse_unary()?;
        self.expect_kind(|t| matches!(t, Tok::Colon), ":")?;
        if !self.eat_kind(|t| matches!(t, Tok::LParen)) {
            return self.err(
                "unsupported TLA+ construct: a \\A body must be parenthesized so a later /\\ is not swallowed",
            );
        }
        let body = self.parse_expr()?;
        self.expect_kind(|t| matches!(t, Tok::RParen), ")")?;
        Ok(Expr::Forall {
            var,
            set: Box::new(set),
            body: Box::new(body),
        })
    }

    fn parse_set(&mut self) -> Result<Expr, ToolError> {
        if self.eat_kind(|t| matches!(t, Tok::RBrace)) {
            return Ok(Expr::Set(Vec::new()));
        }
        // `{ x \in S : P }` is the only comprehension SANY accepts that this
        // profile evaluates. `{ e : x \in S : P }` is not TLA+ and is rejected.
        if self.is_filter_comprehension() {
            let var = self.expect_any_ident()?;
            self.expect_kind(|t| matches!(t, Tok::In), "\\in")?;
            let set = self.parse_unary()?;
            self.expect_kind(|t| matches!(t, Tok::Colon), ":")?;
            let pred = self.parse_expr()?;
            self.expect_kind(|t| matches!(t, Tok::RBrace), "}")?;
            return Ok(Expr::Comp {
                elem: Box::new(Expr::Ident(var.clone())),
                var,
                set: Box::new(set),
                pred: Box::new(pred),
            });
        }
        let mut items = vec![self.parse_expr()?];
        while self.eat_kind(|t| matches!(t, Tok::Comma)) {
            items.push(self.parse_expr()?);
        }
        self.expect_kind(|t| matches!(t, Tok::RBrace), "}")?;
        Ok(Expr::Set(items))
    }

    fn is_filter_comprehension(&self) -> bool {
        let ident = matches!(self.toks.get(self.i).map(|t| &t.tok), Some(Tok::Ident(_)));
        let inn = matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::In));
        ident && inn
    }

    fn parse_tuple(&mut self) -> Result<Expr, ToolError> {
        let mut items = Vec::new();
        if !self.check(|t| matches!(t, Tok::RTuple)) {
            items.push(self.parse_expr()?);
            while self.eat_kind(|t| matches!(t, Tok::Comma)) {
                items.push(self.parse_expr()?);
            }
        }
        self.expect_kind(|t| matches!(t, Tok::RTuple), ">>")?;
        Ok(Expr::Tuple(items))
    }

    fn expect_ident(&mut self, want: &str) -> Result<(), ToolError> {
        match self.peek_tok() {
            Some(Tok::Ident(got)) if got == want => {
                self.bump();
                Ok(())
            }
            _ => self.err(format!("expected {want}")),
        }
    }

    fn expect_any_ident(&mut self) -> Result<String, ToolError> {
        Ok(self.expect_any_ident_at()?.0)
    }

    fn expect_any_ident_at(&mut self) -> Result<(String, u32), ToolError> {
        match self.peek_tok() {
            Some(Tok::Ident(name)) => {
                let name = name.clone();
                let line = self.line();
                self.bump();
                Ok((name, line))
            }
            _ => self.err("expected a name"),
        }
    }

    fn expect_num(&mut self) -> Result<i64, ToolError> {
        match self.peek_tok() {
            Some(Tok::Num(n)) => {
                let n = *n;
                self.bump();
                Ok(n)
            }
            _ => self.err("expected a number"),
        }
    }

    fn expect_kind(&mut self, pred: impl Fn(&Tok) -> bool, what: &str) -> Result<(), ToolError> {
        if self.check(&pred) {
            self.bump();
            Ok(())
        } else {
            self.err(format!("unsupported TLA+ construct: expected {what}"))
        }
    }

    fn eat_ident(&mut self, want: &str) -> bool {
        match self.peek_tok() {
            Some(Tok::Ident(got)) if got == want => {
                self.bump();
                true
            }
            _ => false,
        }
    }

    fn eat_kind(&mut self, pred: impl Fn(&Tok) -> bool) -> bool {
        if self.check(&pred) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn check(&self, pred: impl Fn(&Tok) -> bool) -> bool {
        self.peek_tok().is_some_and(pred)
    }

    fn peek_tok(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.tok)
    }

    fn line(&self) -> u32 {
        self.toks.get(self.i).map(|t| t.line).unwrap_or(1)
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ToolError> {
        Err(ToolError::parse(self.file, self.line(), message))
    }
}

fn strip(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(inner) => strip(inner),
        other => other,
    }
}

fn check_init(file: &str, expr: &Expr) -> Result<(), ToolError> {
    match strip(expr) {
        Expr::Eq(left, right) => match (strip(left), strip(right)) {
            (Expr::Ident(name), Expr::Num(0)) if name == "clock" => Ok(()),
            _ => Err(ToolError::parse(
                file,
                1,
                "unsupported TLA+ construct: Init must be clock = 0",
            )),
        },
        _ => Err(ToolError::parse(
            file,
            1,
            "unsupported TLA+ construct: Init must be clock = 0",
        )),
    }
}

fn check_next(file: &str, expr: &Expr) -> Result<(), ToolError> {
    let ok = matches!(
        strip(expr),
        Expr::Or(items)
            if items.len() == 2
                && is_clock_step(&items[0], 0, 1)
                && is_clock_step(&items[1], 1, 0)
    );
    if ok {
        Ok(())
    } else {
        Err(ToolError::parse(
            file,
            1,
            "unsupported TLA+ construct: Next must toggle clock between 0 and 1",
        ))
    }
}

fn is_clock_step(expr: &Expr, from: i64, to: i64) -> bool {
    let Expr::And(items) = strip(expr) else {
        return false;
    };
    items.len() == 2 && is_clock_eq(&items[0], from) && is_clock_prime(&items[1], to)
}

fn is_clock_eq(expr: &Expr, n: i64) -> bool {
    match strip(expr) {
        Expr::Eq(left, right) => {
            matches!(strip(left), Expr::Ident(name) if name == "clock")
                && matches!(strip(right), Expr::Num(v) if *v == n)
        }
        _ => false,
    }
}

fn is_clock_prime(expr: &Expr, n: i64) -> bool {
    match strip(expr) {
        Expr::Eq(left, right) => {
            matches!(strip(left), Expr::Prime(name) if name == "clock")
                && matches!(strip(right), Expr::Num(v) if *v == n)
        }
        _ => false,
    }
}

fn check_spec(file: &str, expr: &Expr) -> Result<(), ToolError> {
    let ok = matches!(
        strip(expr),
        Expr::And(items)
            if items.len() == 2
                && matches!(strip(&items[0]), Expr::Ident(n) if n == "Init")
                && matches!(
                    strip(&items[1]),
                    Expr::BoxAction { action, subscript }
                        if action == "Next" && subscript == "clock"
                )
    );
    if ok {
        Ok(())
    } else {
        Err(ToolError::parse(
            file,
            1,
            "unsupported TLA+ construct: Spec must be Init /\\ [][Next]_clock",
        ))
    }
}

fn as_string_set(file: &str, expr: &Expr) -> Result<Vec<(String, u32)>, ToolError> {
    let Expr::Set(items) = strip(expr) else {
        return Err(ToolError::parse(
            file,
            1,
            "unsupported TLA+ construct: expected a set literal",
        ));
    };
    let mut out = Vec::new();
    for item in items {
        match strip(item) {
            Expr::Str(s) => out.push((s.clone(), 1)),
            _ => {
                return Err(ToolError::parse(
                    file,
                    1,
                    "unsupported TLA+ construct: set element is not a string",
                ))
            }
        }
    }
    Ok(out)
}

fn as_pair_set(file: &str, expr: &Expr) -> Result<Vec<Pair>, ToolError> {
    let Expr::Set(items) = strip(expr) else {
        return Err(ToolError::parse(
            file,
            1,
            "unsupported TLA+ construct: expected a set literal",
        ));
    };
    let mut out = Vec::new();
    for item in items {
        let Expr::Tuple(parts) = strip(item) else {
            return Err(ToolError::parse(
                file,
                1,
                "unsupported TLA+ construct: set element is not a string pair",
            ));
        };
        if parts.len() != 2 {
            return Err(ToolError::parse(
                file,
                1,
                "unsupported TLA+ construct: tuples in this profile have two strings",
            ));
        }
        let Expr::Str(left) = strip(&parts[0]) else {
            return Err(ToolError::parse(
                file,
                1,
                "unsupported TLA+ construct: tuple component is not a string",
            ));
        };
        let Expr::Str(right) = strip(&parts[1]) else {
            return Err(ToolError::parse(
                file,
                1,
                "unsupported TLA+ construct: tuple component is not a string",
            ));
        };
        out.push(Pair {
            left: left.clone(),
            right: right.clone(),
            line: 1,
        });
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum El {
    Str(String),
    Tup(String, String),
}

fn eval_well(file: &str, expr: &Expr, policy: &Policy) -> Result<(), ToolError> {
    let parts: Vec<&Expr> = match strip(expr) {
        Expr::And(items) => items.iter().collect(),
        other => vec![other],
    };
    if parts.is_empty() {
        return Err(ToolError::parse(
            file,
            policy.line_of("WellFormed"),
            "unsupported TLA+ construct: empty WellFormed",
        ));
    }
    for part in parts {
        eval_conjunct(file, part, policy)?;
    }
    Ok(())
}

fn eval_conjunct(file: &str, expr: &Expr, policy: &Policy) -> Result<(), ToolError> {
    match strip(expr) {
        Expr::SubsetEq(left, right) => {
            let lname = ident_name(left)?;
            let rname = ident_name(right)?;
            let left_set = strings_of(policy, &lname).ok_or_else(|| {
                ToolError::parse(
                    file,
                    policy.line_of("WellFormed"),
                    format!("unsupported TLA+ construct: {lname} is not a string set"),
                )
            })?;
            let right_set = strings_of(policy, &rname).ok_or_else(|| {
                ToolError::parse(
                    file,
                    policy.line_of("WellFormed"),
                    format!("unsupported TLA+ construct: {rname} is not a string set"),
                )
            })?;
            if !left_set.is_subset(&right_set) {
                return Err(ToolError::wf(
                    file,
                    policy.line_of(&lname),
                    format!("{lname} is not a subset of {rname}"),
                ));
            }
            Ok(())
        }
        Expr::Eq(left, right) => match (strip(left), strip(right)) {
            (Expr::Ident(name), Expr::Set(_)) => {
                let lit = as_string_set(file, right)?;
                let lit_set: BTreeSet<String> = lit.into_iter().map(|(s, _)| s).collect();
                let actual = strings_of(policy, name).ok_or_else(|| {
                    ToolError::parse(
                        file,
                        policy.line_of("WellFormed"),
                        format!("unsupported TLA+ construct: {name} is not a string set"),
                    )
                })?;
                if actual != lit_set {
                    return Err(ToolError::wf(
                        file,
                        policy.line_of(name),
                        format!("{name} does not equal the set written in WellFormed"),
                    ));
                }
                Ok(())
            }
            _ => Err(ToolError::parse(
                file,
                policy.line_of("WellFormed"),
                "unsupported TLA+ construct: WellFormed equality is not a string-set check",
            )),
        },
        Expr::Forall { var, set, body } => {
            let set_name = ident_name(set)?;
            let elements = elements_of(policy, &set_name).ok_or_else(|| {
                ToolError::parse(
                    file,
                    policy.line_of("WellFormed"),
                    format!("unsupported TLA+ construct: {set_name} is not a policy set"),
                )
            })?;
            for el in elements {
                let mut env = BTreeMap::new();
                env.insert(var.clone(), el.clone());
                if !truth(file, body, policy, &env)? {
                    return Err(ToolError::wf(
                        file,
                        policy.line_of(&set_name),
                        format!("WellFormed failed for {set_name} element {el:?}"),
                    ));
                }
            }
            Ok(())
        }
        _ => Err(ToolError::parse(
            file,
            policy.line_of("WellFormed"),
            "unsupported TLA+ construct: WellFormed conjunct is not a check the engine evaluates",
        )),
    }
}

fn truth(
    file: &str,
    expr: &Expr,
    policy: &Policy,
    env: &BTreeMap<String, El>,
) -> Result<bool, ToolError> {
    match strip(expr) {
        Expr::And(items) => {
            for item in items {
                if !truth(file, item, policy, env)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Expr::In(left, right) => {
            let value = value_of(file, left, env)?;
            let El::Str(s) = value else {
                return Err(ToolError::parse(
                    file,
                    1,
                    "unsupported TLA+ construct: \\in expects a string",
                ));
            };
            let set_name = ident_name(right)?;
            let set = strings_of(policy, &set_name).ok_or_else(|| {
                ToolError::parse(
                    file,
                    1,
                    format!("unsupported TLA+ construct: {set_name} is not a string set"),
                )
            })?;
            Ok(set.contains(&s))
        }
        Expr::Ne(left, right) => Ok(value_of(file, left, env)? != value_of(file, right, env)?),
        Expr::Eq(left, right) => match strip(left) {
            Expr::Card(inner) => {
                let n = match strip(right) {
                    Expr::Num(n) => *n,
                    _ => {
                        return Err(ToolError::parse(
                            file,
                            1,
                            "unsupported TLA+ construct: Cardinality compared to a non-number",
                        ))
                    }
                };
                let set = eval_set(file, inner, policy, env)?;
                Ok(set.len() as i64 == n)
            }
            _ => Ok(value_of(file, left, env)? == value_of(file, right, env)?),
        },
        _ => Err(ToolError::parse(
            file,
            1,
            "unsupported TLA+ construct: predicate is not a check the engine evaluates",
        )),
    }
}

fn eval_set(
    file: &str,
    expr: &Expr,
    policy: &Policy,
    env: &BTreeMap<String, El>,
) -> Result<BTreeSet<El>, ToolError> {
    match strip(expr) {
        Expr::Comp {
            elem,
            var,
            set,
            pred,
        } => {
            let set_name = ident_name(set)?;
            let elements = elements_of(policy, &set_name).ok_or_else(|| {
                ToolError::parse(
                    file,
                    1,
                    format!("unsupported TLA+ construct: {set_name} is not a policy set"),
                )
            })?;
            let mut out = BTreeSet::new();
            for el in elements {
                let mut nested = env.clone();
                nested.insert(var.clone(), el);
                if truth(file, pred, policy, &nested)? {
                    out.insert(value_of(file, elem, &nested)?);
                }
            }
            Ok(out)
        }
        _ => Err(ToolError::parse(
            file,
            1,
            "unsupported TLA+ construct: set expression is not a comprehension the engine evaluates",
        )),
    }
}

fn value_of(file: &str, expr: &Expr, env: &BTreeMap<String, El>) -> Result<El, ToolError> {
    match strip(expr) {
        Expr::Str(s) => Ok(El::Str(s.clone())),
        Expr::Ident(name) => env.get(name).cloned().ok_or_else(|| {
            ToolError::parse(
                file,
                1,
                format!("unsupported TLA+ construct: {name} is not bound"),
            )
        }),
        Expr::Index(base, idx) => {
            let El::Tup(a, b) = value_of(file, base, env)? else {
                return Err(ToolError::parse(
                    file,
                    1,
                    "unsupported TLA+ construct: index of a non-tuple",
                ));
            };
            match idx {
                1 => Ok(El::Str(a)),
                2 => Ok(El::Str(b)),
                _ => Err(ToolError::parse(
                    file,
                    1,
                    "unsupported TLA+ construct: tuple index is not 1 or 2",
                )),
            }
        }
        _ => Err(ToolError::parse(
            file,
            1,
            "unsupported TLA+ construct: value is not a string or a tuple component",
        )),
    }
}

fn ident_name(expr: &Expr) -> Result<String, ToolError> {
    match strip(expr) {
        Expr::Ident(name) => Ok(name.clone()),
        _ => Err(ToolError::parse(
            "",
            1,
            "unsupported TLA+ construct: expected a name",
        )),
    }
}

fn strings_of(policy: &Policy, name: &str) -> Option<BTreeSet<String>> {
    policy.string_set(name).map(|m| m.keys().cloned().collect())
}

fn elements_of(policy: &Policy, name: &str) -> Option<Vec<El>> {
    if let Some(map) = policy.string_set(name) {
        return Some(map.keys().cloned().map(El::Str).collect());
    }
    policy.pair_set(name).map(|pairs| {
        pairs
            .iter()
            .map(|p| El::Tup(p.left.clone(), p.right.clone()))
            .collect()
    })
}

fn reject_unknown_effects(policy: &Policy) -> Result<(), ToolError> {
    const KNOWN: &[&str] = &["env", "fs_write", "net", "process"];
    for name in policy.effects.keys() {
        if !KNOWN.contains(&name.as_str()) {
            return Err(ToolError::parse(
                &policy.file,
                policy.line_of("Effects"),
                format!(
                    "unsupported effect `{name}`: the engine implements env, fs_write, net, process"
                ),
            ));
        }
    }
    for pair in &policy.allowed_effects {
        if !policy.effects.contains_key(&pair.right) {
            return Err(ToolError::wf(
                &policy.file,
                pair.line,
                format!("allowed effect {} is not in Effects", pair.right),
            ));
        }
    }
    Ok(())
}

fn reject_bad_capability_names(policy: &Policy) -> Result<(), ToolError> {
    for name in policy.capabilities.keys() {
        if !is_rust_ident(name) {
            return Err(ToolError::parse(
                &policy.file,
                policy.line_of("Capabilities"),
                format!("capability `{name}` is not a Rust function name"),
            ));
        }
    }
    Ok(())
}

fn is_rust_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric()) && name != "_"
}

fn tokenize(file: &str, text: &str) -> Result<Vec<Located>, ToolError> {
    let mut lx = Lexer {
        file,
        src: text,
        i: 0,
        line: 1,
    };
    let mut out = Vec::new();
    while let Some(tok) = lx.next_tok()? {
        out.push(tok);
    }
    Ok(out)
}

struct Lexer<'a> {
    file: &'a str,
    src: &'a str,
    i: usize,
    line: u32,
}

impl<'a> Lexer<'a> {
    fn next_tok(&mut self) -> Result<Option<Located>, ToolError> {
        self.skip_ws_and_comments()?;
        let Some(c) = self.peek() else {
            return Ok(None);
        };
        let line = self.line;
        let tok = match c {
            '-' => {
                let start = self.i;
                while self.peek() == Some('-') {
                    self.bump();
                }
                if self.i - start < 4 {
                    return self.err("unsupported TLA+ construct: '-'");
                }
                Tok::Dash
            }
            '=' => {
                let mut n = 0;
                while self.peek() == Some('=') {
                    self.bump();
                    n += 1;
                }
                match n {
                    1 => Tok::Eq,
                    2 => Tok::EqEq,
                    n if n >= 4 => Tok::Footer,
                    _ => return self.err("unsupported TLA+ construct: '='"),
                }
            }
            '#' => {
                self.bump();
                Tok::Ne
            }
            '\'' => {
                self.bump();
                Tok::Prime
            }
            '{' => {
                self.bump();
                Tok::LBrace
            }
            '}' => {
                self.bump();
                Tok::RBrace
            }
            '(' => {
                self.bump();
                if self.peek() == Some('*') {
                    return self.err("unterminated comment");
                }
                Tok::LParen
            }
            ')' => {
                self.bump();
                Tok::RParen
            }
            '[' => {
                self.bump();
                if self.peek() == Some(']') {
                    self.bump();
                    Tok::Box
                } else {
                    Tok::LBrack
                }
            }
            ']' => {
                self.bump();
                Tok::RBrack
            }
            '<' => {
                self.bump();
                if self.peek() == Some('<') {
                    self.bump();
                    Tok::LTuple
                } else {
                    return self.err("unsupported TLA+ construct: '<'");
                }
            }
            '>' => {
                self.bump();
                if self.peek() == Some('>') {
                    self.bump();
                    Tok::RTuple
                } else {
                    return self.err("unsupported TLA+ construct: '>'");
                }
            }
            ',' => {
                self.bump();
                Tok::Comma
            }
            ':' => {
                self.bump();
                Tok::Colon
            }
            '_' => {
                self.bump();
                Tok::Under
            }
            '/' => {
                self.bump();
                if self.peek() == Some('\\') {
                    self.bump();
                    Tok::And
                } else {
                    return self.err("unsupported TLA+ construct: '/'");
                }
            }
            '\\' => self.backslash()?,
            '"' => self.string()?,
            c if c.is_ascii_digit() => self.number()?,
            c if c.is_ascii_alphabetic() => self.ident()?,
            _ => return self.err(format!("unsupported TLA+ construct: {c:?}")),
        };
        Ok(Some(Located { tok, line }))
    }

    fn backslash(&mut self) -> Result<Tok, ToolError> {
        self.bump();
        match self.peek() {
            Some('/') => {
                self.bump();
                Ok(Tok::Or)
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                    self.bump();
                }
                match &self.src[start..self.i] {
                    "in" => Ok(Tok::In),
                    "subseteq" => Ok(Tok::SubsetEq),
                    "A" => Ok(Tok::Forall),
                    other => self.err(format!(
                        "unsupported TLA+ construct: \\{other} is outside the profile"
                    )),
                }
            }
            _ => self.err("unsupported TLA+ construct: '\\'"),
        }
    }

    fn string(&mut self) -> Result<Tok, ToolError> {
        self.bump();
        let mut s = String::new();
        loop {
            match self.peek() {
                None => return self.err("unterminated string"),
                Some('"') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    match self.bump() {
                        Some('"') => s.push('"'),
                        Some('\\') => s.push('\\'),
                        _ => return self.err("unsupported TLA+ construct: string escape"),
                    }
                }
                Some('\n') => return self.err("unterminated string"),
                Some(c) => {
                    s.push(c);
                    self.bump();
                }
            }
        }
        Ok(Tok::Str(s))
    }

    fn number(&mut self) -> Result<Tok, ToolError> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        let n: i64 = self.src[start..self.i].parse().map_err(|_| {
            ToolError::parse(self.file, self.line, "unsupported TLA+ construct: number")
        })?;
        Ok(Tok::Num(n))
    }

    fn ident(&mut self) -> Result<Tok, ToolError> {
        let start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.bump();
        }
        Ok(Tok::Ident(self.src[start..self.i].to_owned()))
    }

    fn skip_ws_and_comments(&mut self) -> Result<(), ToolError> {
        loop {
            while self.peek().is_some_and(|c| c.is_whitespace()) {
                self.bump();
            }
            if self.peek() == Some('\\') && self.peek_at(1) == Some('*') {
                self.bump();
                self.bump();
                while self.peek().is_some_and(|c| c != '\n') {
                    self.bump();
                }
                continue;
            }
            if self.peek() == Some('(') && self.peek_at(1) == Some('*') {
                self.bump();
                self.bump();
                let mut depth = 1;
                while depth > 0 {
                    match self.bump() {
                        None => return self.err("unterminated comment"),
                        Some('(') if self.peek() == Some('*') => {
                            self.bump();
                            depth += 1;
                        }
                        Some('*') if self.peek() == Some(')') => {
                            self.bump();
                            depth -= 1;
                        }
                        Some(_) => {}
                    }
                }
                continue;
            }
            break;
        }
        Ok(())
    }

    fn peek(&self) -> Option<char> {
        self.src[self.i..].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.src[self.i..].chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ToolError> {
        Err(ToolError::parse(self.file, self.line, message))
    }
}

/// Line numbers on set elements come from a second walk of the source.
/// The parser above records operator lines; element lines are filled by
/// scanning the operator body so diagnostics can cite the tuple.
pub fn fill_element_lines(text: &str, policy: &mut Policy) {
    fill_string_lines(text, "Modules", &mut policy.modules);
    fill_string_lines(text, "Owners", &mut policy.owners);
    fill_string_lines(text, "Effects", &mut policy.effects);
    fill_string_lines(text, "Capabilities", &mut policy.capabilities);
    fill_string_lines(text, "FloatAllowed", &mut policy.float_allowed);
    fill_pair_lines(text, &mut policy.dependencies);
    fill_pair_lines(text, &mut policy.writers);
    fill_pair_lines(text, &mut policy.capability_callers);
    fill_pair_lines(text, &mut policy.allowed_effects);
    fill_pair_lines(text, &mut policy.path_owners);
}

fn fill_string_lines(text: &str, _op: &str, map: &mut BTreeMap<String, u32>) {
    for (value, line) in map.iter_mut() {
        if let Some(found) = find_string_line(text, value) {
            *line = found;
        }
    }
}

fn fill_pair_lines(text: &str, pairs: &mut [Pair]) {
    for pair in pairs.iter_mut() {
        let needle = format!("<<\"{}\", \"{}\">>", pair.left, pair.right);
        let needle_sp = format!("<<\"{}\",\"{}\">>", pair.left, pair.right);
        if let Some(line) = find_line(text, &needle).or_else(|| find_line(text, &needle_sp)) {
            pair.line = line;
        }
    }
}

fn find_string_line(text: &str, value: &str) -> Option<u32> {
    let needle = format!("\"{value}\"");
    find_line(text, &needle)
}

fn find_line(text: &str, needle: &str) -> Option<u32> {
    text.find(needle)
        .map(|idx| text[..idx].bytes().filter(|b| *b == b'\n').count() as u32 + 1)
}

pub fn read_policy(path: &Path) -> Result<(Policy, String), ToolError> {
    let text = fs::read_to_string(path)
        .map_err(|err| ToolError::tool(format!("read {}: {err}", path.display())))?;
    let policy = parse_policy(&path.display().to_string(), &text)?;
    Ok((policy, text))
}
