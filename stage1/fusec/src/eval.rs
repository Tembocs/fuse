//! Fuse Stage 1 — Native tree-walking evaluator.
//!
//! A Rust port of the Stage 0 Python evaluator. Uses `fuse-runtime` FuseValue
//! for all runtime values. Produces identical output to Stage 0 for all Core tests.

use std::collections::HashMap;
use std::process;

use fuse_runtime::*;
use crate::ast::nodes::*;

/// Deferred expression + env snapshot for `defer`.
type DeferEntry = (Expr, Vec<(String, FuseValue)>);

pub struct Evaluator {
    program: Program,
    file: String,
    globals: HashMap<String, FuseValue>,
    structs: HashMap<String, StructDecl>,
    data_classes: HashMap<String, DataClassDecl>,
    enums: HashMap<String, EnumDecl>,
    ext_fns: HashMap<(String, String), FnDecl>,
    all_fns: HashMap<String, FnDecl>,
    defers: Vec<DeferEntry>,
}

/// Scope: a stack of name→value maps.
struct Env {
    scopes: Vec<HashMap<String, FuseValue>>,
    moved: std::collections::HashSet<String>,
}

impl Env {
    fn new() -> Self { Self { scopes: vec![HashMap::new()], moved: std::collections::HashSet::new() } }
    fn push(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop(&mut self) { self.scopes.pop(); }
    fn mark_moved(&mut self, name: &str) { self.moved.insert(name.into()); }
    fn is_moved(&self, name: &str) -> bool { self.moved.contains(name) }
    fn define(&mut self, name: &str, val: FuseValue) {
        self.scopes.last_mut().unwrap().insert(name.into(), val);
    }
    fn get(&self, name: &str) -> Option<FuseValue> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) { return Some(v.clone()); }
        }
        None
    }
    fn set(&mut self, name: &str, val: FuseValue) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) { scope.insert(name.into(), val); return true; }
        }
        false
    }
    fn snapshot(&self) -> Vec<(String, FuseValue)> {
        let mut out = Vec::new();
        for scope in &self.scopes {
            for (k, v) in scope { out.push((k.clone(), v.clone())); }
        }
        out
    }
}

/// Control flow: early return from a function.
enum ControlFlow {
    Return(FuseValue),
}

impl Evaluator {
    pub fn new(program: Program, file: &str) -> Self {
        let mut e = Self {
            program, file: file.into(),
            globals: HashMap::new(),
            structs: HashMap::new(),
            data_classes: HashMap::new(),
            enums: HashMap::new(),
            ext_fns: HashMap::new(),
            all_fns: HashMap::new(),
            defers: Vec::new(),
        };
        e.register_decls();
        e
    }

    fn register_decls(&mut self) {
        for decl in &self.program.decls.clone() {
            match decl {
                Decl::Struct(s) => { self.structs.insert(s.name.clone(), s.clone()); }
                Decl::DataClass(d) => { self.data_classes.insert(d.name.clone(), d.clone()); }
                Decl::Enum(e) => { self.enums.insert(e.name.clone(), e.clone()); }
                Decl::Fn(f) => {
                    if let Some(ref ext) = f.ext_type {
                        self.ext_fns.insert((ext.clone(), f.name.clone()), f.clone());
                    } else {
                        self.all_fns.insert(f.name.clone(), f.clone());
                    }
                }
                _ => {}
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // Public entry point
    // ══════════════════════════════════════════════════════════════════

    pub fn run(&mut self) {
        // Evaluate top-level val/var declarations
        let decls = self.program.decls.clone();
        for decl in &decls {
            match decl {
                Decl::TopVal { name, value, .. } | Decl::TopVar { name, value, .. } => {
                    let mut env = Env::new();
                    // Make already-evaluated globals available
                    for (k, v) in &self.globals { env.define(k, v.clone()); }
                    let val = match self.eval_expr(value, &mut env) {
                        Ok(v) => v,
                        Err(ControlFlow::Return(v)) => v,
                    };
                    self.globals.insert(name.clone(), val);
                }
                _ => {}
            }
        }

        // Find @entrypoint fn
        let entry = self.program.decls.clone().into_iter().find_map(|d| {
            if let Decl::Fn(f) = d {
                if f.annotations.iter().any(|a| a.name == "entrypoint") && f.ext_type.is_none() {
                    return Some(f);
                }
            }
            None
        });
        let entry = entry.expect("no @entrypoint function found");
        let result = self.call_fn(&entry, vec![], None);
        // If main returns Err, print it
        if result.is_err() {
            eprintln!("Error: {}", result.unwrap_enum_value());
            process::exit(1);
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // Function calling
    // ══════════════════════════════════════════════════════════════════

    /// Call a function. Returns (result, mutref_writeback) where mutref_writeback
    /// contains the final values of mutref parameters to write back to the caller.
    fn call_fn_inner(&mut self, decl: &FnDecl, args: Vec<FuseValue>, self_val: Option<FuseValue>) -> (FuseValue, HashMap<String, FuseValue>) {
        let mut env = Env::new();
        // Make globals available
        for (k, v) in &self.globals { env.define(k, v.clone()); }
        let mut arg_idx = 0;
        let mut mutref_params: Vec<String> = Vec::new();
        for p in &decl.params {
            if p.name == "self" {
                if let Some(ref sv) = self_val { env.define("self", sv.clone()); }
            } else {
                let v = args.get(arg_idx).cloned().unwrap_or(FuseValue::Unit);
                env.define(&p.name, v);
                if p.convention.as_deref() == Some("mutref") {
                    mutref_params.push(p.name.clone());
                }
                arg_idx += 1;
            }
        }
        let saved_defers = std::mem::take(&mut self.defers);
        let result = match &decl.body {
            FnBody::Block(stmts) => self.eval_block(stmts, &mut env, true),
            FnBody::Expr(e) => self.eval_expr(e, &mut env),
        };
        let result = match result {
            Ok(v) => v,
            Err(ControlFlow::Return(v)) => v,
        };
        // Collect mutref parameter values for writeback
        let mut writeback = HashMap::new();
        for name in &mutref_params {
            if let Some(v) = env.get(name) { writeback.insert(name.clone(), v); }
        }
        // Fire defers in reverse
        let my_defers = std::mem::replace(&mut self.defers, saved_defers);
        for (expr, snapshot) in my_defers.into_iter().rev() {
            let mut defer_env = Env::new();
            for (k, v) in snapshot { defer_env.define(&k, v); }
            let _ = self.eval_expr(&expr, &mut defer_env);
        }
        (result, writeback)
    }

    fn call_fn(&mut self, decl: &FnDecl, args: Vec<FuseValue>, self_val: Option<FuseValue>) -> FuseValue {
        self.call_fn_inner(decl, args, self_val).0
    }

    fn call_method(&mut self, obj: &FuseValue, method: &str, args: Vec<FuseValue>, env: &mut Env) -> FuseValue {
        let type_name = obj.type_name().to_string();

        // Extension functions
        let key = (type_name.clone(), method.to_string());
        if let Some(f) = self.ext_fns.get(&key).cloned() {
            return self.call_fn(&f, args, Some(obj.clone()));
        }

        // Struct methods
        if let FuseValue::Struct(s) = obj {
            let tn = s.type_name.clone();
            if let Some(sd) = self.structs.get(&tn).cloned() {
                for m in &sd.methods {
                    if m.name == method {
                        return self.call_fn(&m.clone(), args, Some(obj.clone()));
                    }
                }
            }
            if let Some(dc) = self.data_classes.get(&tn).cloned() {
                for m in &dc.methods {
                    if m.name == method {
                        return self.call_fn(&m.clone(), args, Some(obj.clone()));
                    }
                }
            }
        }

        // Chan methods: .send(val), .recv()
        if let FuseValue::Struct(ref s) = obj {
            if s.type_name == "Chan" {
                match method {
                    "send" => {
                        // Push value into the channel buffer
                        // Need to mutate via the global CHAN_STORE
                        if let Some(val) = args.into_iter().next() {
                            CHAN_BUFFER.with(|buf| buf.borrow_mut().push(val));
                        }
                        return FuseValue::Unit;
                    }
                    "recv" => {
                        // Pop value from channel buffer
                        let val = CHAN_BUFFER.with(|buf| {
                            let mut b = buf.borrow_mut();
                            if b.is_empty() { FuseValue::none() } else { FuseValue::ok(b.remove(0)) }
                        });
                        return val;
                    }
                    _ => {}
                }
            }
        }

        // Shared<T> methods: .read() and .write()
        if let FuseValue::Struct(ref s) = obj {
            if s.type_name == "Shared" {
                match method {
                    "read" | "write" => {
                        return s.fields.iter()
                            .find(|(k, _)| k == "value")
                            .map(|(_, v)| v.clone())
                            .unwrap_or(FuseValue::Unit);
                    }
                    _ => {}
                }
            }
        }

        // Chan namespace methods: .unbounded(), .bounded(n)
        if let FuseValue::Fn(ref f) = obj {
            if f.name == "Chan" {
                match method {
                    "unbounded" => {
                        // Return a (tx, rx) pair as a List of two channel endpoints
                        let chan = FuseValue::new_struct("Chan", vec![
                            ("buffer", FuseValue::List(vec![])),
                        ], None);
                        return FuseValue::List(vec![chan.clone(), chan]);
                    }
                    "bounded" => {
                        let _capacity = args.first().map(|a| a.as_int()).unwrap_or(1);
                        let chan = FuseValue::new_struct("Chan", vec![
                            ("buffer", FuseValue::List(vec![])),
                        ], None);
                        return FuseValue::List(vec![chan.clone(), chan]);
                    }
                    _ => {}
                }
            }
        }

        // SIMD<T,N> namespace: .sum()
        if let FuseValue::Fn(ref f) = obj {
            if f.name.starts_with("SIMD") || type_name == "Fn" {
                match method {
                    "sum" => {
                        // SIMD<Float32, N>.sum(values) — just sum the list
                        if let Some(list) = args.first() {
                            return fuse_list_sum(list);
                        }
                        return FuseValue::Float(0.0);
                    }
                    _ => {}
                }
            }
        }

        // Built-in methods
        match obj {
            FuseValue::List(_) => return self.list_method(obj.clone(), method, args, env),
            FuseValue::Str(_) => return self.string_method(obj, method),
            FuseValue::Int(_) => return self.int_method(obj, method),
            FuseValue::Float(_) => return self.float_method(obj, method),
            _ => {}
        }
        panic!("no method '{method}' on {type_name}");
    }

    fn list_method(&mut self, mut list: FuseValue, method: &str, args: Vec<FuseValue>, env: &mut Env) -> FuseValue {
        match method {
            "retainWhere" => {
                if let Some(FuseValue::Lambda(lam)) = args.first() {
                    let lam = lam.clone();
                    let elems = list.as_list_mut();
                    elems.retain(|e| {
                        self.call_lambda(&lam, vec![e.clone()]).is_truthy()
                    });
                }
                FuseValue::Unit
            }
            "map" => {
                if let Some(FuseValue::Lambda(lam)) = args.first() {
                    let lam = lam.clone();
                    let mapped: Vec<_> = list.as_list().iter()
                        .map(|e| self.call_lambda(&lam, vec![e.clone()]))
                        .collect();
                    FuseValue::List(mapped)
                } else { FuseValue::List(vec![]) }
            }
            "filter" => {
                if let Some(FuseValue::Lambda(lam)) = args.first() {
                    let lam = lam.clone();
                    let filtered: Vec<_> = list.as_list().iter()
                        .filter(|e| self.call_lambda(&lam, vec![(*e).clone()]).is_truthy())
                        .cloned().collect();
                    FuseValue::List(filtered)
                } else { FuseValue::List(vec![]) }
            }
            "sorted" => fuse_list_sorted(&list),
            "first" => fuse_list_first(&list),
            "last" => fuse_list_last(&list),
            "len" => fuse_list_len(&list),
            "isEmpty" => fuse_list_is_empty(&list),
            "sum" => fuse_list_sum(&list),
            _ => panic!("unknown list method: {method}"),
        }
    }

    fn string_method(&self, s: &FuseValue, method: &str) -> FuseValue {
        match method {
            "toUpper" => fuse_string_to_upper(s),
            "toLower" => fuse_string_to_lower(s),
            "len" => fuse_string_len(s),
            _ => panic!("unknown string method: {method}"),
        }
    }

    fn int_method(&self, v: &FuseValue, method: &str) -> FuseValue {
        match method {
            "toFloat" => fuse_int_to_float(v),
            "isEven" => fuse_int_is_even(v),
            "toString" => FuseValue::Str(format!("{}", v.as_int())),
            _ => panic!("unknown int method: {method}"),
        }
    }

    fn float_method(&self, v: &FuseValue, method: &str) -> FuseValue {
        match method {
            "toString" => FuseValue::Str(format!("{}", v)),
            "toInt" => FuseValue::Int(v.as_float() as i64),
            _ => panic!("unknown float method: {method}"),
        }
    }

    fn call_lambda(&mut self, lam: &FuseLambda, args: Vec<FuseValue>) -> FuseValue {
        // Look up the lambda declaration
        let lambda_decl = self.find_lambda(lam.id);
        let mut lam_env = Env::new();
        // Restore captures
        for (k, v) in &lam.captures { lam_env.define(k, v.clone()); }
        // Bind params
        if let Some(decl) = lambda_decl {
            for (p, a) in decl.0.iter().zip(args) { lam_env.define(p, a); }
            let result = self.eval_stmts(&decl.1, &mut lam_env);
            match result { Ok(v) => v, Err(ControlFlow::Return(v)) => v }
        } else {
            FuseValue::Unit
        }
    }

    fn find_lambda(&self, id: usize) -> Option<(Vec<String>, Vec<Stmt>)> {
        // Lambdas are stored with their id
        LAMBDA_STORE.with(|store| store.borrow().get(&id).cloned())
    }

    // ══════════════════════════════════════════════════════════════════
    // Block evaluation with ASAP destruction
    // ══════════════════════════════════════════════════════════════════

    fn eval_block(&mut self, stmts: &[Stmt], env: &mut Env, asap: bool) -> Result<FuseValue, ControlFlow> {
        let last_use = if asap { self.compute_last_use(stmts) } else { HashMap::new() };
        let mut result = FuseValue::Unit;
        for (i, stmt) in stmts.iter().enumerate() {
            result = self.eval_stmt(stmt, env)?;

            if asap {
                for (name, &last_idx) in &last_use {
                    if last_idx == i && !env.is_moved(name) {
                        if let Some(val) = env.get(name) {
                            if let FuseValue::Struct(s) = &val {
                                if s.del_fn.is_some() {
                                    self.call_del(&val);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    fn call_del(&mut self, val: &FuseValue) {
        if let FuseValue::Struct(s) = val {
            if let Some(ref _del_name) = s.del_fn {
                let f = self.structs.get(&s.type_name).and_then(|sd| {
                    sd.methods.iter().find(|m| m.name == "__del__").cloned()
                }).or_else(|| {
                    self.data_classes.get(&s.type_name).and_then(|dc| {
                        dc.methods.iter().find(|m| m.name == "__del__").cloned()
                    })
                });
                if let Some(f) = f {
                    // Pass self with del_fn cleared to prevent infinite recursion
                    let mut clean = s.clone();
                    clean.del_fn = None;
                    self.call_fn(&f, vec![], Some(FuseValue::Struct(clean)));
                }
            }
        }
    }

    fn compute_last_use(&self, stmts: &[Stmt]) -> HashMap<String, usize> {
        let mut last: HashMap<String, usize> = HashMap::new();
        let n = stmts.len();
        for (i, stmt) in stmts.iter().enumerate() {
            let idx = if matches!(stmt, Stmt::Defer(..)) { n } else { i };
            for name in self.collect_idents(stmt) {
                let e = last.entry(name).or_insert(0);
                if idx > *e { *e = idx; }
            }
        }
        last
    }

    fn collect_idents(&self, node: &Stmt) -> Vec<String> {
        let mut names = Vec::new();
        self.walk_stmt_idents(node, &mut names);
        names
    }

    fn walk_stmt_idents(&self, stmt: &Stmt, names: &mut Vec<String>) {
        match stmt {
            Stmt::Val { value, .. } | Stmt::Var { value, .. } => self.walk_expr_idents(value, names),
            Stmt::Assign { target, value, .. } => { self.walk_expr_idents(target, names); self.walk_expr_idents(value, names); }
            Stmt::Expr(e) => self.walk_expr_idents(e, names),
            Stmt::Return(Some(e), _) => self.walk_expr_idents(e, names),
            Stmt::Defer(e, _) => self.walk_expr_idents(e, names),
            Stmt::If { cond, then_b, else_b, .. } => {
                self.walk_expr_idents(cond, names);
                for s in then_b { self.walk_stmt_idents(s, names); }
                if let Some(eb) = else_b {
                    match eb { ElseBody::ElseIf(s) => self.walk_stmt_idents(s, names),
                               ElseBody::Block(ss) => { for s in ss { self.walk_stmt_idents(s, names); } } }
                }
            }
            Stmt::For { iter, body, .. } => { self.walk_expr_idents(iter, names); for s in body { self.walk_stmt_idents(s, names); } }
            _ => {}
        }
    }

    fn walk_expr_idents(&self, expr: &Expr, names: &mut Vec<String>) {
        match expr {
            Expr::Ident(n, _) => names.push(n.clone()),
            Expr::SelfExpr(_) => names.push("self".into()),
            Expr::Binary(l, _, r, _) => { self.walk_expr_idents(l, names); self.walk_expr_idents(r, names); }
            Expr::Unary(_, o, _) => self.walk_expr_idents(o, names),
            Expr::Call(c, args, _) => { self.walk_expr_idents(c, names); for a in args { self.walk_expr_idents(a, names); } }
            Expr::Field(o, _, _) | Expr::OptChain(o, _, _) | Expr::Question(o, _) => self.walk_expr_idents(o, names),
            Expr::Elvis(l, r, _) => { self.walk_expr_idents(l, names); self.walk_expr_idents(r, names); }
            Expr::Move(e, _) | Expr::RefE(e, _) | Expr::MutrefE(e, _) => self.walk_expr_idents(e, names),
            Expr::FStr(parts, _) | Expr::List(parts, _) | Expr::Tuple(parts, _) => { for p in parts { self.walk_expr_idents(p, names); } }
            Expr::Match(subj, arms, _) => { self.walk_expr_idents(subj, names); for a in arms { self.walk_expr_idents(&a.body, names); } }
            Expr::When(arms, _) => { for a in arms { if let Some(c) = &a.cond { self.walk_expr_idents(c, names); } self.walk_expr_idents(&a.body, names); } }
            Expr::Block(stmts, _) => { for s in stmts { self.walk_stmt_idents(s, names); } }
            _ => {}
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // Statement evaluation
    // ══════════════════════════════════════════════════════════════════

    fn eval_stmts(&mut self, stmts: &[Stmt], env: &mut Env) -> Result<FuseValue, ControlFlow> {
        let mut result = FuseValue::Unit;
        for stmt in stmts { result = self.eval_stmt(stmt, env)?; }
        Ok(result)
    }

    fn eval_stmt(&mut self, stmt: &Stmt, env: &mut Env) -> Result<FuseValue, ControlFlow> {
        match stmt {
            Stmt::Val { name, value, .. } => {
                let v = self.eval_expr(value, env)?;
                env.define(name, v);
                Ok(FuseValue::Unit)
            }
            Stmt::Var { name, value, .. } => {
                let v = self.eval_expr(value, env)?;
                env.define(name, v);
                Ok(FuseValue::Unit)
            }
            Stmt::ValTuple { names, value, .. } => {
                let v = self.eval_expr(value, env)?;
                // Destructure tuple
                if let FuseValue::List(elems) = &v {
                    for (i, name) in names.iter().enumerate() {
                        env.define(name, elems.get(i).cloned().unwrap_or(FuseValue::Unit));
                    }
                }
                Ok(FuseValue::Unit)
            }
            Stmt::Assign { target, value, .. } => {
                let v = self.eval_expr(value, env)?;
                match target {
                    Expr::Ident(name, _) => { env.set(name, v); }
                    Expr::Field(obj_expr, field, _) => {
                        // Need to mutate the object in the env
                        if let Expr::Ident(obj_name, _) = obj_expr.as_ref() {
                            if let Some(mut obj) = env.get(obj_name) {
                                obj.set_field(field, v);
                                env.set(obj_name, obj);
                            }
                        }
                    }
                    _ => {}
                }
                Ok(FuseValue::Unit)
            }
            Stmt::Expr(e) => self.eval_expr(e, env),
            Stmt::Return(val, _) => {
                let v = match val {
                    Some(e) => self.eval_expr(e, env)?,
                    None => FuseValue::Unit,
                };
                Err(ControlFlow::Return(v))
            }
            Stmt::Defer(e, _) => {
                self.defers.push((e.clone(), env.snapshot()));
                Ok(FuseValue::Unit)
            }
            Stmt::If { cond, then_b, else_b, .. } => {
                let c = self.eval_expr(cond, env)?;
                if c.is_truthy() {
                    env.push();
                    let r = self.eval_stmts(then_b, env)?;
                    env.pop();
                    Ok(r)
                } else if let Some(eb) = else_b {
                    match eb {
                        ElseBody::ElseIf(s) => self.eval_stmt(s, env),
                        ElseBody::Block(stmts) => {
                            env.push();
                            let r = self.eval_stmts(stmts, env)?;
                            env.pop();
                            Ok(r)
                        }
                    }
                } else { Ok(FuseValue::Unit) }
            }
            Stmt::For { var, iter, body, .. } => {
                let iterable = self.eval_expr(iter, env)?;
                if let FuseValue::List(elems) = iterable {
                    for item in elems {
                        env.push();
                        env.define(var, item);
                        self.eval_stmts(body, env)?;
                        env.pop();
                    }
                }
                Ok(FuseValue::Unit)
            }
            Stmt::Loop(body, _) => {
                loop {
                    env.push();
                    self.eval_stmts(body, env)?;
                    env.pop();
                }
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // Expression evaluation
    // ══════════════════════════════════════════════════════════════════

    fn eval_expr(&mut self, expr: &Expr, env: &mut Env) -> Result<FuseValue, ControlFlow> {
        match expr {
            Expr::IntLit(v, _) => Ok(FuseValue::Int(*v)),
            Expr::FloatLit(v, _) => Ok(FuseValue::Float(*v)),
            Expr::StrLit(v, _) => Ok(FuseValue::Str(v.clone())),
            Expr::BoolLit(v, _) => Ok(FuseValue::Bool(*v)),
            Expr::Unit(_) => Ok(FuseValue::Unit),

            Expr::Ident(name, _) => {
                if let Some(v) = env.get(name) { return Ok(v); }
                // Check enum types
                if self.enums.contains_key(name) {
                    return Ok(FuseValue::Str(name.clone())); // enum namespace
                }
                if name == "None" { return Ok(FuseValue::none()); }
                // Type namespaces (SIMD, Chan, Shared, etc.)
                match name.as_str() {
                    "SIMD" | "Chan" | "Shared" => {
                        return Ok(FuseValue::Fn(FuseFn { name: name.clone() }));
                    }
                    _ => {}
                }
                Ok(FuseValue::Unit)
            }
            Expr::SelfExpr(_) => Ok(env.get("self").unwrap_or(FuseValue::Unit)),

            Expr::Move(inner, _) => {
                let val = self.eval_expr(inner, env)?;
                if let Expr::Ident(name, _) = inner.as_ref() {
                    env.mark_moved(name);
                }
                Ok(val)
            }
            Expr::RefE(inner, _) | Expr::MutrefE(inner, _) => {
                self.eval_expr(inner, env)
            }

            Expr::Binary(l, op, r, _) => {
                let lv = self.eval_expr(l, env)?;
                let rv = self.eval_expr(r, env)?;
                Ok(match op {
                    BinOp::Add => lv.add(&rv),
                    BinOp::Sub => lv.sub(&rv),
                    BinOp::Mul => lv.mul(&rv),
                    BinOp::Div => lv.div(&rv),
                    BinOp::Mod => lv.modulo(&rv),
                    BinOp::Eq => lv.eq(&rv),
                    BinOp::Ne => lv.ne(&rv),
                    BinOp::Lt => lv.lt(&rv),
                    BinOp::Gt => lv.gt(&rv),
                    BinOp::Le => lv.le(&rv),
                    BinOp::Ge => lv.ge(&rv),
                    BinOp::And => FuseValue::Bool(lv.is_truthy() && rv.is_truthy()),
                    BinOp::Or => FuseValue::Bool(lv.is_truthy() || rv.is_truthy()),
                })
            }
            Expr::Unary(op, operand, _) => {
                let v = self.eval_expr(operand, env)?;
                Ok(match op {
                    UnaryOp::Neg => v.neg(),
                    UnaryOp::Not => FuseValue::Bool(!v.is_truthy()),
                })
            }

            Expr::Field(obj, field, _) => self.eval_field(obj, field, env),
            Expr::OptChain(obj, field, _) => self.eval_opt_chain(obj, field, env),
            Expr::Question(inner, _) => self.eval_question(inner, env),
            Expr::Elvis(l, r, _) => self.eval_elvis(l, r, env),
            Expr::Call(callee, args, _) => self.eval_call(callee, args, env),
            Expr::Match(subject, arms, _) => self.eval_match(subject, arms, env),
            Expr::When(arms, _) => self.eval_when(arms, env),

            Expr::FStr(parts, _) => {
                let mut s = String::new();
                for p in parts {
                    let v = self.eval_expr(p, env)?;
                    s.push_str(&format!("{v}"));
                }
                Ok(FuseValue::Str(s))
            }
            Expr::List(elems, _) => {
                let mut v = Vec::new();
                for e in elems { v.push(self.eval_expr(e, env)?); }
                Ok(FuseValue::List(v))
            }
            Expr::Tuple(elems, _) => {
                let mut v = Vec::new();
                for e in elems { v.push(self.eval_expr(e, env)?); }
                Ok(FuseValue::List(v)) // tuples as lists at runtime
            }
            Expr::Lambda(params, body, _) => {
                let id = register_lambda(params.clone(), body.clone());
                Ok(FuseValue::Lambda(FuseLambda { id, captures: env.snapshot() }))
            }
            Expr::Block(stmts, _) => {
                env.push();
                let r = self.eval_stmts(stmts, env)?;
                env.pop();
                Ok(r)
            }
            Expr::Path(obj_expr, method, _) => {
                // Path expressions: Shared::new, Chan::<T>.unbounded, SIMD<T,N>.sum
                // Return a callable marker so the Call handler can dispatch
                if let Expr::Ident(name, _) = obj_expr.as_ref() {
                    return Ok(FuseValue::Fn(FuseFn { name: format!("{name}::{method}") }));
                }
                Ok(FuseValue::Unit)
            }
            Expr::Spawn(inner, _is_async, _) => {
                // Execute spawned block/call synchronously
                // (Phase 8: single-threaded; correct for test semantics)
                self.eval_expr(inner, env)?;
                Ok(FuseValue::Unit)
            }
            Expr::Await(inner, _) => {
                // Evaluate the awaited expression and return its value
                self.eval_expr(inner, env)
            }
        }
    }

    // ── Field access ─────────────────────────────────────────────────

    fn eval_field(&mut self, obj_expr: &Expr, field: &str, env: &mut Env) -> Result<FuseValue, ControlFlow> {
        let obj = self.eval_expr(obj_expr, env)?;

        // Struct field
        if let FuseValue::Struct(ref s) = obj {
            for (k, v) in &s.fields {
                if k == field { return Ok(v.clone()); }
            }
        }

        // Enum namespace: Status.Ok, Status.Warn("text")
        if let Expr::Ident(name, _) = obj_expr {
            if let Some(e) = self.enums.get(name).cloned() {
                for v in &e.variants {
                    if v.name == field {
                        if v.fields.is_empty() {
                            return Ok(FuseValue::enum_variant(name, field, None));
                        }
                        // Return a constructor marker — will be called
                        return Ok(FuseValue::Fn(FuseFn { name: format!("{name}.{field}") }));
                    }
                }
            }
        }

        // FuseFn namespace: methods on namespace objects (SIMD, Chan, etc.)
        if let FuseValue::Fn(ref f) = obj {
            return Ok(FuseValue::Fn(FuseFn { name: format!("{}.{field}", f.name) }));
        }

        // Unknown identifier that might be a type namespace (SIMD, etc.)
        if matches!(obj, FuseValue::Unit) {
            if let Expr::Ident(name, _) = obj_expr {
                return Ok(FuseValue::Fn(FuseFn { name: format!("{name}.{field}") }));
            }
            // Could be a Path expr that resolved to Unit
            return Ok(FuseValue::Fn(FuseFn { name: format!("?.{field}") }));
        }

        panic!("no field '{field}' on {}", obj.type_name());
    }

    // ── Optional chaining ?. ─────────────────────────────────────────

    fn eval_opt_chain(&mut self, obj_expr: &Expr, field: &str, env: &mut Env) -> Result<FuseValue, ControlFlow> {
        let obj = self.eval_expr(obj_expr, env)?;
        if obj.is_none() { return Ok(FuseValue::none()); }
        let inner = if obj.is_some() { obj.unwrap_enum_value() } else { obj };
        if let FuseValue::Struct(ref s) = inner {
            for (k, v) in &s.fields {
                if k == field { return Ok(v.clone()); }
            }
        }
        panic!("cannot access '{field}' via ?.");
    }

    // ── ? operator ───────────────────────────────────────────────────

    fn eval_question(&mut self, inner_expr: &Expr, env: &mut Env) -> Result<FuseValue, ControlFlow> {
        let val = self.eval_expr(inner_expr, env)?;
        if val.is_ok() { return Ok(val.unwrap_enum_value()); }
        if val.is_err() { return Err(ControlFlow::Return(val)); }
        if val.is_some() { return Ok(val.unwrap_enum_value()); }
        if val.is_none() { return Err(ControlFlow::Return(val)); }
        panic!("? used on non-Result/Option");
    }

    // ── ?: Elvis ─────────────────────────────────────────────────────

    fn eval_elvis(&mut self, l: &Expr, r: &Expr, env: &mut Env) -> Result<FuseValue, ControlFlow> {
        let lv = self.eval_expr(l, env)?;
        if lv.is_none() { return self.eval_expr(r, env); }
        Ok(lv)
    }

    // ── Call ──────────────────────────────────────────────────────────

    fn eval_call(&mut self, callee: &Expr, arg_exprs: &[Expr], env: &mut Env) -> Result<FuseValue, ControlFlow> {
        // Method call: obj.method(args)
        if let Expr::Field(obj_expr, method, _) = callee {
            let obj = self.eval_expr(obj_expr, env)?;

            // Enum variant constructor: Status.Warn("text")
            if let Expr::Ident(name, _) = obj_expr.as_ref() {
                if let Some(e) = self.enums.get(name).cloned() {
                    if e.variants.iter().any(|v| v.name == *method && !v.fields.is_empty()) {
                        let mut args = Vec::new();
                        for a in arg_exprs { args.push(self.eval_expr(a, env)?); }
                        let val = if args.len() == 1 { Some(args.remove(0)) } else { None };
                        return Ok(FuseValue::enum_variant(name, method, val));
                    }
                }
            }

            let mut args = Vec::new();
            for a in arg_exprs { args.push(self.eval_expr(a, env)?); }

            // Mutating list methods (mutref semantics): modify variable in-place
            if matches!(&obj, FuseValue::List(_)) && method == "retainWhere" {
                if let Expr::Ident(var_name, _) = obj_expr.as_ref() {
                    if let Some(mut list_val) = env.get(var_name) {
                        if let Some(FuseValue::Lambda(ref lam)) = args.first() {
                            let lam = lam.clone();
                            let elems = list_val.as_list_mut();
                            elems.retain(|e| {
                                self.call_lambda(&lam, vec![e.clone()]).is_truthy()
                            });
                            env.set(var_name, list_val);
                        }
                        return Ok(FuseValue::Unit);
                    }
                }
            }

            return Ok(self.call_method(&obj, method, args, env));
        }

        // Regular call
        let callee_val = self.eval_expr(callee, env)?;
        let mut args = Vec::new();
        for a in arg_exprs { args.push(self.eval_expr(a, env)?); }

        // Built-in functions
        if let Expr::Ident(name, _) = callee {
            match name.as_str() {
                "println" => { fuse_println(args.first().unwrap_or(&FuseValue::Unit)); return Ok(FuseValue::Unit); }
                "eprintln" => { fuse_eprintln(args.first().unwrap_or(&FuseValue::Unit)); return Ok(FuseValue::Unit); }
                "Some" => { return Ok(FuseValue::some(args.into_iter().next().unwrap_or(FuseValue::Unit))); }
                "Ok" => { return Ok(FuseValue::ok(args.into_iter().next().unwrap_or(FuseValue::Unit))); }
                "Err" => { return Ok(FuseValue::err(args.into_iter().next().unwrap_or(FuseValue::Unit))); }
                _ => {}
            }

            // User function (with mutref writeback)
            if let Some(f) = self.all_fns.get(name).cloned() {
                let (result, writeback) = self.call_fn_inner(&f, args, None);
                // Write back mutref parameters to the caller's env
                if !writeback.is_empty() {
                    for (param, arg_expr) in f.params.iter().zip(arg_exprs) {
                        if param.convention.as_deref() == Some("mutref") {
                            if let Expr::MutrefE(inner, _) = arg_expr {
                                if let Expr::Ident(var_name, _) = inner.as_ref() {
                                    if let Some(val) = writeback.get(&param.name) {
                                        env.set(var_name, val.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                return Ok(result);
            }

            // Struct constructor
            if let Some(sd) = self.structs.get(name).cloned() {
                return Ok(self.construct_struct(&sd, args));
            }
            // Data class constructor
            if let Some(dc) = self.data_classes.get(name).cloned() {
                return Ok(self.construct_data_class(&dc, args));
            }
        }

        // Lambda call
        if let FuseValue::Lambda(lam) = callee_val {
            return Ok(self.call_lambda(&lam, args));
        }

        // Path-based calls: Shared::new, Chan::unbounded/bounded, SIMD::sum
        if let FuseValue::Fn(func) = &callee_val {
            if func.name.contains("::") {
                return Ok(self.call_path_fn(&func.name, args));
            }
            // Enum variant constructor from field access (e.g., Status.Warn)
            if func.name.contains('.') {
                let parts: Vec<&str> = func.name.split('.').collect();
                let val = if args.len() == 1 { Some(args.into_iter().next().unwrap()) } else { None };
                return Ok(FuseValue::enum_variant(parts[0], parts[1], val));
            }
        }

        panic!("not callable: {}", callee_val.type_name());
    }

    fn construct_struct(&self, decl: &StructDecl, args: Vec<FuseValue>) -> FuseValue {
        let mut fields = Vec::new();
        for (f, v) in decl.fields.iter().zip(args) {
            fields.push((f.name.as_str(), v));
        }
        let del_fn = decl.methods.iter().find(|m| m.name == "__del__").map(|_| "__del__");
        FuseValue::new_struct(&decl.name, fields, del_fn)
    }

    fn construct_data_class(&self, decl: &DataClassDecl, args: Vec<FuseValue>) -> FuseValue {
        let mut fields = Vec::new();
        for (p, v) in decl.params.iter().zip(args) {
            fields.push((p.name.as_str(), v));
        }
        let del_fn = decl.methods.iter().find(|m| m.name == "__del__").map(|_| "__del__");
        FuseValue::new_struct(&decl.name, fields, del_fn)
    }

    // ── Path-based function calls (Shared::new, SIMD::sum, etc.) ──

    fn call_path_fn(&mut self, name: &str, args: Vec<FuseValue>) -> FuseValue {
        match name {
            "Shared::new" => {
                // Wrap value in a Shared container (struct with inner value)
                let inner = args.into_iter().next().unwrap_or(FuseValue::Unit);
                FuseValue::new_struct("Shared", vec![("value", inner)], None)
            }
            _ => {
                // Chan::String, Chan::Int, etc. — return a Chan type namespace
                if name.starts_with("Chan::") {
                    return FuseValue::Fn(FuseFn { name: "Chan".into() });
                }
                panic!("unknown path call: {name}");
            }
        }
    }

    // ── Match ────────────────────────────────────────────────────────

    fn eval_match(&mut self, subject: &Expr, arms: &[MatchArm], env: &mut Env) -> Result<FuseValue, ControlFlow> {
        let val = self.eval_expr(subject, env)?;
        for arm in arms {
            if let Some(bindings) = self.match_pattern(&arm.pattern, &val) {
                env.push();
                for (k, v) in bindings { env.define(&k, v); }
                let result = self.eval_expr(&arm.body, env)?;
                env.pop();
                return Ok(result);
            }
        }
        panic!("no matching arm");
    }

    fn match_pattern(&self, pat: &Pattern, val: &FuseValue) -> Option<Vec<(String, FuseValue)>> {
        match pat {
            Pattern::Wildcard(_) => Some(vec![]),
            Pattern::Literal(lit, _) => {
                let matches = match (lit, val) {
                    (Lit::Int(a), FuseValue::Int(b)) => a == b,
                    (Lit::Float(a), FuseValue::Float(b)) => a == b,
                    (Lit::Str(a), FuseValue::Str(b)) => a == b,
                    (Lit::Bool(a), FuseValue::Bool(b)) => a == b,
                    _ => false,
                };
                if matches { Some(vec![]) } else { None }
            }
            Pattern::Ident(name, _) => {
                if name.contains('.') {
                    let parts: Vec<&str> = name.split('.').collect();
                    if let FuseValue::Enum(e) = val {
                        if e.enum_name == parts[0] && e.variant == parts[1] && e.value.is_none() {
                            return Some(vec![]);
                        }
                    }
                    return None;
                }
                if name == "None" {
                    if val.is_none() { return Some(vec![]); } else { return None; }
                }
                Some(vec![(name.clone(), val.clone())])
            }
            Pattern::Constructor(name, args, _) => {
                if let FuseValue::Enum(e) = val {
                    let (enum_n, variant_n) = if name.contains('.') {
                        let parts: Vec<&str> = name.split('.').collect();
                        (parts[0], parts[1])
                    } else {
                        let en = match name.as_str() {
                            "Some" | "None" => "Option",
                            "Ok" | "Err" => "Result",
                            _ => &e.enum_name,
                        };
                        (en, name.as_str())
                    };
                    if e.enum_name != enum_n || e.variant != variant_n { return None; }
                    if args.is_empty() { return Some(vec![]); }
                    if args.len() == 1 {
                        let inner = e.value.as_ref().map(|v| *v.clone()).unwrap_or(FuseValue::Unit);
                        return self.match_pattern(&args[0], &inner);
                    }
                }
                None
            }
            Pattern::Tuple(elems, _) => {
                if let FuseValue::List(vals) = val {
                    if elems.len() != vals.len() { return None; }
                    let mut bindings = Vec::new();
                    for (p, v) in elems.iter().zip(vals) {
                        match self.match_pattern(p, v) {
                            Some(b) => bindings.extend(b),
                            None => return None,
                        }
                    }
                    Some(bindings)
                } else { None }
            }
        }
    }

    // ── When ─────────────────────────────────────────────────────────

    fn eval_when(&mut self, arms: &[WhenArm], env: &mut Env) -> Result<FuseValue, ControlFlow> {
        for arm in arms {
            if let Some(ref cond) = arm.cond {
                let c = self.eval_expr(cond, env)?;
                if c.is_truthy() { return self.eval_expr(&arm.body, env); }
            } else {
                return self.eval_expr(&arm.body, env);
            }
        }
        panic!("when: no matching arm");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Lambda store (global registry for lambda bodies)
// ═══════════════════════════════════════════════════════════════════════

use std::cell::RefCell;
thread_local! {
    static LAMBDA_STORE: RefCell<HashMap<usize, (Vec<String>, Vec<Stmt>)>> = RefCell::new(HashMap::new());
    static LAMBDA_COUNTER: RefCell<usize> = RefCell::new(0);
    static CHAN_BUFFER: RefCell<Vec<FuseValue>> = RefCell::new(Vec::new());
}

fn register_lambda(params: Vec<String>, body: Vec<Stmt>) -> usize {
    LAMBDA_COUNTER.with(|c| {
        let id = *c.borrow();
        *c.borrow_mut() = id + 1;
        LAMBDA_STORE.with(|s| s.borrow_mut().insert(id, (params, body)));
        id
    })
}
