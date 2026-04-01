pub mod types;
pub mod ownership;
pub mod exhaustiveness;
pub mod rank;
pub mod spawn;
pub mod async_lint;

use std::collections::{HashMap, HashSet};
use crate::error::FuseError;
use crate::ast::nodes::*;
use crate::parser::expr_span;

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
    in_spawn: bool,
    in_async: bool,
    // Track @rank annotations: name -> rank value
    ranks: HashMap<String, i64>,
    // Track currently held ranks (acquired Shared locks in scope)
    held_ranks: Vec<(String, i64)>,
    // Track write guards held: name -> true
    write_guards: HashSet<String>,
}

impl Checker {
    pub fn new(program: &Program, file: &str) -> Self {
        let mut c = Self {
            file: file.into(),
            errors: Vec::new(),
            enums: HashMap::new(),
            scopes: Vec::new(),
            in_spawn: false,
            in_async: false,
            ranks: HashMap::new(),
            held_ranks: Vec::new(),
            write_guards: HashSet::new(),
        };
        c.register_builtins();
        c.collect_enums(program);
        c
    }

    pub fn check(mut self, program: &Program) -> Vec<FuseError> {
        // First pass: collect @rank annotations on TopVal/TopVar
        for decl in &program.decls {
            self.collect_ranks(decl);
        }
        // Second pass: check everything
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

    // ── @rank collection ─────────────────────────────────────────────
    fn collect_ranks(&mut self, decl: &Decl) {
        match decl {
            Decl::TopVal { name, annotations, .. } | Decl::TopVar { name, annotations, .. } => {
                for ann in annotations {
                    if ann.name == "rank" {
                        if let Some(Expr::IntLit(v, _)) = ann.args.first() {
                            self.ranks.insert(name.clone(), *v);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ── declaration checking ─────────────────────────────────────────
    fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Fn(f) => self.check_fn(f),
            Decl::Struct(s) => { for m in &s.methods { self.check_fn(m); } }
            Decl::DataClass(d) => { for m in &d.methods { self.check_fn(m); } }
            Decl::TopVal { name, value, annotations, span, .. } => {
                self.check_shared_requires_rank(name, value, annotations, span);
            }
            Decl::TopVar { name, value, annotations, span, .. } => {
                self.check_shared_requires_rank(name, value, annotations, span);
            }
            _ => {}
        }
    }

    /// Check that Shared::new() calls have @rank annotations
    fn check_shared_requires_rank(&mut self, _name: &str, value: &Expr, annotations: &[Annotation], _span: &Span) {
        if self.is_shared_new(value) {
            let has_rank = annotations.iter().any(|a| a.name == "rank");
            if !has_rank {
                let val_span = expr_span(value);
                self.report(
                    "Shared<T> requires @rank(N) annotation",
                    &val_span,
                    Some("deadlock safety cannot be guaranteed without it\n       hint: add @rank(1) if this is your only shared resource".into()),
                );
            }
        }
    }

    fn is_shared_new(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Call(callee, _, _) => {
                if let Expr::Path(obj, method, _) = callee.as_ref() {
                    if let Expr::Ident(name, _) = obj.as_ref() {
                        return name == "Shared" && method == "new";
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn check_fn(&mut self, f: &FnDecl) {
        let prev_async = self.in_async;
        self.in_async = f.is_async;
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
        self.in_async = prev_async;
    }

    // ── statement checking ───────────────────────────────────────────
    fn check_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts { self.check_stmt(s); }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Val { name, convention, value, span, .. } => {
                self.check_expr(value);
                let is_mut = convention.as_deref() == Some("mutref");
                // Check for local Shared::new without @rank
                if self.is_shared_new(value) && !self.ranks.contains_key(name) {
                    let val_span = expr_span(value);
                    self.report(
                        "Shared<T> requires @rank(N) annotation",
                        &val_span,
                        Some("deadlock safety cannot be guaranteed without it\n       hint: add @rank(1) if this is your only shared resource".into()),
                    );
                }
                // Track write guards and check rank ordering
                if let Some(conv) = convention {
                    if let Some(shared_name) = self.extract_shared_call(value) {
                        if let Some(&rank) = self.ranks.get(&shared_name) {
                            // Check rank ordering
                            if let Some(&(ref held_name, held_rank)) = self.held_ranks.last() {
                                if rank < held_rank {
                                    self.report(
                                        format!("cannot acquire @rank({rank}) while holding @rank({held_rank})"),
                                        span,
                                        Some(format!("acquire `{shared_name}` before `{held_name}`, or release `{held_name}` first")),
                                    );
                                }
                            }
                            self.held_ranks.push((shared_name.clone(), rank));
                        }
                        if conv == "mutref" {
                            self.write_guards.insert(format!("{shared_name}"));
                        }
                    }
                }
                self.define(name, Binding { is_mutable: is_mut, convention: convention.clone(), moved: false, moved_line: 0 });
            }
            Stmt::ValTuple { names, value, .. } => {
                self.check_expr(value);
                for name in names {
                    self.define(name, Binding { is_mutable: false, convention: None, moved: false, moved_line: 0 });
                }
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
            Expr::Await(e, span) => {
                // Check for write guard held across await
                if !self.write_guards.is_empty() {
                    let guards: Vec<_> = self.write_guards.iter().cloned().collect();
                    for guard_name in &guards {
                        self.report_warning(
                            "write guard held across await point",
                            span,
                            Some(format!("another task waiting on `{guard_name}.write()` will be blocked\n       for the entire await duration")),
                        );
                    }
                }
                self.check_expr(e);
            }
            Expr::Spawn(e, _, span) => {
                let prev = self.in_spawn;
                self.in_spawn = true;
                // Collect outer var names before entering spawn scope
                let outer_vars: Vec<String> = self.scopes.iter().flat_map(|s| {
                    s.iter().filter(|(_, b)| b.is_mutable).map(|(n, _)| n.clone())
                }).collect();
                self.check_spawn_body(e, &outer_vars, span);
                self.in_spawn = prev;
            }
            Expr::Path(e, _, _) => self.check_expr(e),
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

    // ── spawn capture checking ───────────────────────────────────────
    fn check_spawn_body(&mut self, expr: &Expr, outer_vars: &[String], spawn_span: &Span) {
        match expr {
            Expr::Block(stmts, _) => {
                // Check all statements in the spawn block for mutref captures
                for stmt in stmts {
                    self.check_spawn_stmt(stmt, outer_vars, spawn_span);
                }
            }
            _ => self.check_expr(expr),
        }
    }

    fn check_spawn_stmt(&mut self, stmt: &Stmt, outer_vars: &[String], spawn_span: &Span) {
        match stmt {
            Stmt::Assign { target, value, .. } => {
                if let Expr::Ident(name, _) = target {
                    if outer_vars.contains(name) {
                        self.report(
                            "`mutref` capture is not permitted across spawn boundary",
                            spawn_span,
                            Some("use `Shared<T>` for shared mutable state across tasks".into()),
                        );
                        return;
                    }
                }
                self.check_expr(value);
            }
            Stmt::Expr(e) => {
                self.check_spawn_expr(e, outer_vars, spawn_span);
            }
            _ => {}
        }
    }

    fn check_spawn_expr(&mut self, expr: &Expr, outer_vars: &[String], spawn_span: &Span) {
        match expr {
            Expr::Ident(name, _) => {
                if outer_vars.contains(name) {
                    // Reading a mutable var from outer scope is also a capture
                }
            }
            Expr::Call(callee, args, _) => {
                self.check_spawn_expr(callee, outer_vars, spawn_span);
                for a in args { self.check_spawn_expr(a, outer_vars, spawn_span); }
            }
            _ => self.check_expr(expr),
        }
    }

    /// Extract the shared variable name from expr.read() or expr.write()
    fn extract_shared_call(&self, value: &Expr) -> Option<String> {
        if let Expr::Call(callee, _, _) = value {
            if let Expr::Field(obj, method, _) = callee.as_ref() {
                if method == "read" || method == "write" {
                    if let Expr::Ident(name, _) = obj.as_ref() {
                        return Some(name.clone());
                    }
                }
            }
        }
        None
    }

    fn report_warning(&mut self, msg: impl Into<String>, span: &Span, hint: Option<String>) {
        let mut e = FuseError::warning(msg, &self.file, span.line, span.col);
        e.hint = hint;
        self.errors.push(e);
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
