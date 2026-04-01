pub mod types;
pub mod ownership;
pub mod exhaustiveness;
pub mod rank;
pub mod spawn;
pub mod async_lint;

use std::collections::{HashMap, HashSet};
use crate::error::FuseError;
use crate::ast::nodes::*;

// ═══════════════════════════════════════════════════════════════════════
// Binding info for scope tracking
// ═══════════════════════════════════════════════════════════════════════
#[derive(Clone)]
struct Binding {
    is_mutable: bool,
    convention: Option<String>,
    moved: bool,
    moved_line: usize,
}

// ═══════════════════════════════════════════════════════════════════════
// Checker
// ═══════════════════════════════════════════════════════════════════════
pub struct Checker {
    file: String,
    errors: Vec<FuseError>,
    enums: HashMap<String, EnumDecl>,
    scopes: Vec<HashMap<String, Binding>>,
}

impl Checker {
    pub fn new(program: &Program, file: &str) -> Self {
        let mut c = Self {
            file: file.into(),
            errors: Vec::new(),
            enums: HashMap::new(),
            scopes: Vec::new(),
        };
        c.register_builtins();
        c.collect_enums(program);
        c
    }

    pub fn check(mut self, program: &Program) -> Vec<FuseError> {
        for decl in &program.decls {
            self.check_decl(decl);
        }
        self.errors
    }

    // ── setup ────────────────────────────────────────────────────────
    fn register_builtins(&mut self) {
        let s = Span { line: 0, col: 0 };
        self.enums.insert("Option".into(), EnumDecl {
            name: "Option".into(),
            variants: vec![
                EnumVariant { name: "Some".into(), fields: vec![TypeExpr::Simple("T".into(), s.clone())], span: s.clone() },
                EnumVariant { name: "None".into(), fields: vec![], span: s.clone() },
            ],
            annotations: vec![], span: s.clone(),
        });
        self.enums.insert("Result".into(), EnumDecl {
            name: "Result".into(),
            variants: vec![
                EnumVariant { name: "Ok".into(), fields: vec![TypeExpr::Simple("T".into(), s.clone())], span: s.clone() },
                EnumVariant { name: "Err".into(), fields: vec![TypeExpr::Simple("E".into(), s.clone())], span: s.clone() },
            ],
            annotations: vec![], span: s,
        });
    }

    fn collect_enums(&mut self, program: &Program) {
        for decl in &program.decls {
            if let Decl::Enum(e) = decl { self.enums.insert(e.name.clone(), e.clone()); }
        }
    }

    // ── scope helpers ────────────────────────────────────────────────
    fn push_scope(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop_scope(&mut self) { self.scopes.pop(); }

    fn define(&mut self, name: &str, info: Binding) {
        if let Some(scope) = self.scopes.last_mut() { scope.insert(name.into(), info); }
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) { return Some(b); }
        }
        None
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Binding> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.get_mut(name) { return Some(b); }
        }
        None
    }

    fn report(&mut self, msg: impl Into<String>, span: &Span, hint: Option<String>) {
        let mut e = FuseError::new(msg, &self.file, span.line, span.col);
        e.hint = hint;
        self.errors.push(e);
    }

    // ── declaration checking ─────────────────────────────────────────
    fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Fn(f) => self.check_fn(f),
            Decl::Struct(s) => { for m in &s.methods { self.check_fn(m); } }
            Decl::DataClass(d) => { for m in &d.methods { self.check_fn(m); } }
            _ => {}
        }
    }

    fn check_fn(&mut self, f: &FnDecl) {
        self.push_scope();
        for p in &f.params {
            self.define(&p.name, Binding {
                is_mutable: p.convention.as_deref() == Some("mutref"),
                convention: p.convention.clone(),
                moved: false,
                moved_line: 0,
            });
        }
        match &f.body {
            FnBody::Block(stmts) => self.check_stmts(stmts),
            FnBody::Expr(e) => self.check_expr(e),
        }
        self.pop_scope();
    }

    // ── statement checking ───────────────────────────────────────────
    fn check_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts { self.check_stmt(s); }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Val { name, value, .. } => {
                self.check_expr(value);
                self.define(name, Binding { is_mutable: false, convention: None, moved: false, moved_line: 0 });
            }
            Stmt::Var { name, value, .. } => {
                self.check_expr(value);
                self.define(name, Binding { is_mutable: true, convention: None, moved: false, moved_line: 0 });
            }
            Stmt::Assign { target, value, .. } => {
                self.check_expr(value);
                if let Expr::Ident(name, span) = target {
                    if let Some(b) = self.lookup(name) {
                        if !b.is_mutable {
                            if b.convention.as_deref() == Some("ref") {
                                self.report(
                                    format!("cannot assign through `ref` parameter `{name}`"),
                                    span, None,
                                );
                            } else {
                                self.report(
                                    format!("cannot reassign to `{name}` \u{2014} declared as `val`"),
                                    span,
                                    Some("use `var` if reassignment is intended".into()),
                                );
                            }
                        }
                    }
                } else { self.check_expr(target); }
            }
            Stmt::Expr(e) => self.check_expr(e),
            Stmt::Return(val, _) => { if let Some(e) = val { self.check_expr(e); } }
            Stmt::Defer(e, _) => self.check_expr(e),
            Stmt::If { cond, then_b, else_b, .. } => {
                self.check_expr(cond);
                self.push_scope(); self.check_stmts(then_b); self.pop_scope();
                if let Some(eb) = else_b {
                    match eb {
                        ElseBody::ElseIf(s) => self.check_stmt(s),
                        ElseBody::Block(stmts) => { self.push_scope(); self.check_stmts(stmts); self.pop_scope(); }
                    }
                }
            }
            Stmt::For { var, iter, body, .. } => {
                self.check_expr(iter);
                self.push_scope();
                self.define(var, Binding { is_mutable: false, convention: None, moved: false, moved_line: 0 });
                self.check_stmts(body);
                self.pop_scope();
            }
            Stmt::Loop(stmts, _) => { self.push_scope(); self.check_stmts(stmts); self.pop_scope(); }
        }
    }

    // ── expression checking ──────────────────────────────────────────
    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name, span) => self.check_use(name, span),
            Expr::Move(inner, span) => {
                self.check_expr(inner);
                if let Expr::Ident(name, _) = inner.as_ref() {
                    if let Some(b) = self.lookup_mut(name) {
                        b.moved = true;
                        b.moved_line = span.line;
                    }
                }
            }
            Expr::Binary(l, _, r, _) => { self.check_expr(l); self.check_expr(r); }
            Expr::Unary(_, o, _) => self.check_expr(o),
            Expr::Call(callee, args, _) => { self.check_expr(callee); for a in args { self.check_expr(a); } }
            Expr::Field(o, _, _) | Expr::OptChain(o, _, _) => self.check_expr(o),
            Expr::Question(e, _) | Expr::RefE(e, _) | Expr::MutrefE(e, _) => self.check_expr(e),
            Expr::Elvis(l, r, _) => { self.check_expr(l); self.check_expr(r); }
            Expr::Match(subject, arms, span) => {
                self.check_expr(subject);
                for arm in arms { self.check_expr(&arm.body); }
                self.check_exhaustiveness(arms, span);
            }
            Expr::When(arms, _) => {
                for arm in arms {
                    if let Some(c) = &arm.cond { self.check_expr(c); }
                    self.check_expr(&arm.body);
                }
            }
            Expr::FStr(parts, _) => { for p in parts { self.check_expr(p); } }
            Expr::List(elems, _) | Expr::Tuple(elems, _) => { for e in elems { self.check_expr(e); } }
            Expr::Lambda(params, body, _) => {
                self.push_scope();
                for p in params {
                    self.define(p, Binding { is_mutable: false, convention: None, moved: false, moved_line: 0 });
                }
                self.check_stmts(body);
                self.pop_scope();
            }
            Expr::Block(stmts, _) => { self.push_scope(); self.check_stmts(stmts); self.pop_scope(); }
            _ => {} // literals, self, unit
        }
    }

    fn check_use(&mut self, name: &str, span: &Span) {
        if let Some(b) = self.lookup(name) {
            if b.moved {
                self.report(
                    format!("cannot use `{name}` after `move`"),
                    span,
                    Some(format!("ownership was transferred on line {}", b.moved_line)),
                );
            }
        }
    }

    // ── match exhaustiveness ─────────────────────────────────────────
    fn check_exhaustiveness(&mut self, arms: &[MatchArm], span: &Span) {
        let enum_name = arms.iter().find_map(|a| self.enum_from_pattern(&a.pattern));
        let enum_name = match enum_name { Some(n) => n, None => return };
        let decl = match self.enums.get(&enum_name) { Some(d) => d.clone(), None => return };

        let all: HashSet<String> = decl.variants.iter().map(|v| v.name.clone()).collect();
        let mut covered = HashSet::new();
        let mut has_wildcard = false;

        for arm in arms {
            if matches!(&arm.pattern, Pattern::Wildcard(_)) { has_wildcard = true; }
            covered.extend(self.covered_variants(&arm.pattern, &enum_name));
        }
        if has_wildcard { return; }

        let missing: Vec<_> = all.difference(&covered).cloned().collect();
        if !missing.is_empty() {
            let formatted: Vec<_> = missing.iter().map(|v| {
                let vd = decl.variants.iter().find(|vv| vv.name == *v);
                if let Some(vd) = vd {
                    if vd.fields.is_empty() { format!("`{v}`") }
                    else {
                        let ts: Vec<_> = vd.fields.iter().map(|f| match f {
                            TypeExpr::Simple(n, _) => n.clone(),
                            _ => "?".into(),
                        }).collect();
                        format!("`{v}({})`", ts.join(", "))
                    }
                } else { format!("`{v}`") }
            }).collect();
            self.report("match is not exhaustive", span,
                Some(format!("missing case: {}", formatted.join(", "))));
        }
    }

    fn enum_from_pattern(&self, pat: &Pattern) -> Option<String> {
        match pat {
            Pattern::Constructor(name, _, _) => {
                if name.contains('.') { Some(name.split('.').next().unwrap().into()) }
                else if name == "Some" { Some("Option".into()) }
                else if name == "Ok" || name == "Err" { Some("Result".into()) }
                else { None }
            }
            Pattern::Ident(name, _) => {
                if name.contains('.') { Some(name.split('.').next().unwrap().into()) }
                else if name == "None" { Some("Option".into()) }
                else { None }
            }
            _ => None,
        }
    }

    fn covered_variants(&self, pat: &Pattern, enum_name: &str) -> HashSet<String> {
        let mut s = HashSet::new();
        match pat {
            Pattern::Constructor(name, _, _) => {
                if let Some(v) = name.rsplit('.').next() {
                    if name.starts_with(enum_name) { s.insert(v.into()); }
                }
                if enum_name == "Option" || enum_name == "Result" {
                    if !name.contains('.') { s.insert(name.clone()); }
                }
            }
            Pattern::Ident(name, _) => {
                if let Some((prefix, variant)) = name.rsplit_once('.') {
                    if prefix == enum_name { s.insert(variant.into()); }
                }
                if name == "None" && enum_name == "Option" { s.insert("None".into()); }
                if (name == "Ok" || name == "Err") && enum_name == "Result" {
                    s.insert(name.clone());
                }
            }
            _ => {}
        }
        s
    }
}
