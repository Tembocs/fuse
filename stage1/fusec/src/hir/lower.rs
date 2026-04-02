//! AST → HIR lowering.
//!
//! Walks the AST and produces HIR with resolved types, ownership conventions,
//! and ASAP last-use information. The initial implementation uses HirType::Unknown
//! for most expressions — the codegen passes all values as boxed FuseValue pointers
//! through the runtime, so full type inference is an optimization, not a requirement.

use std::collections::HashMap;
use crate::ast::nodes::*;
use super::nodes::*;

// ═════════════════════════════════════════════════════════════════════
// Lowerer
// ═════════════════════════════════════════════════════════════════════

pub struct Lowerer {
    /// Enum name → variant list (for pattern matching, type resolution).
    enums: HashMap<String, Vec<String>>,
    /// Struct name → field names.
    structs: HashMap<String, Vec<String>>,
    /// Data class name → field names.
    data_classes: HashMap<String, Vec<String>>,
    /// Function name → return type hint.
    fn_ret_types: HashMap<String, HirType>,
    /// Struct/DC name → has @value annotation.
    has_value: HashMap<String, bool>,
    /// Struct/DC name → has __del__ method.
    has_del: HashMap<String, bool>,
}

impl Lowerer {
    pub fn new() -> Self {
        Self {
            enums: HashMap::new(),
            structs: HashMap::new(),
            data_classes: HashMap::new(),
            fn_ret_types: HashMap::new(),
            has_value: HashMap::new(),
            has_del: HashMap::new(),
        }
    }

    // ── Entry point ─────────────────────────────────────────────────

    pub fn lower(&mut self, program: &Program) -> HirProgram {
        // Pass 1: collect all type declarations.
        for decl in &program.decls {
            self.collect_decl(decl);
        }

        // Pass 2: lower all declarations.
        let mut hir_decls = Vec::new();
        let mut hir_enums = Vec::new();
        let mut hir_structs = Vec::new();
        let mut hir_data_classes = Vec::new();
        let mut asap_info = Vec::new();

        for decl in &program.decls {
            match decl {
                Decl::Fn(f) => {
                    let hf = self.lower_fn_decl(f);
                    // Compute ASAP info for function body.
                    if let HirFnBody::Block(ref stmts) = hf.body {
                        let info = self.compute_asap(stmts, &hf.name);
                        asap_info.push((hf.name.clone(), info));
                    }
                    hir_decls.push(HirDecl::Fn(hf));
                }
                Decl::Enum(e) => {
                    let he = self.lower_enum_decl(e);
                    hir_enums.push(he.clone());
                    hir_decls.push(HirDecl::Enum(he));
                }
                Decl::Struct(s) => {
                    let hs = self.lower_struct_decl(s);
                    hir_structs.push(hs.clone());
                    hir_decls.push(HirDecl::Struct(hs));
                }
                Decl::DataClass(d) => {
                    let hd = self.lower_data_class_decl(d);
                    hir_data_classes.push(hd.clone());
                    hir_decls.push(HirDecl::DataClass(hd));
                }
                Decl::ExternFn(ef) => {
                    hir_decls.push(HirDecl::ExternFn(self.lower_extern_fn(ef)));
                }
                Decl::ExternBlock { fns, .. } => {
                    for ef in fns {
                        hir_decls.push(HirDecl::ExternFn(self.lower_extern_fn(ef)));
                    }
                }
                Decl::TopVal { name, ty, value, span, .. } => {
                    let hv = self.lower_expr(value);
                    let ht = ty.as_ref().map(|t| self.lower_type_expr(t)).unwrap_or_else(|| hv.ty.clone());
                    hir_decls.push(HirDecl::TopVal {
                        name: name.clone(), ty: ht, value: hv, span: span.clone(),
                    });
                }
                Decl::TopVar { name, ty, value, span, .. } => {
                    let hv = self.lower_expr(value);
                    let ht = ty.as_ref().map(|t| self.lower_type_expr(t)).unwrap_or_else(|| hv.ty.clone());
                    hir_decls.push(HirDecl::TopVar {
                        name: name.clone(), ty: ht, value: hv, span: span.clone(),
                    });
                }
            }
        }

        HirProgram {
            decls: hir_decls,
            enums: hir_enums,
            structs: hir_structs,
            data_classes: hir_data_classes,
            asap: asap_info,
            span: program.span.clone(),
        }
    }

    // ── Pass 1: collect declarations ────────────────────────────────

    fn collect_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Enum(e) => {
                let variants: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
                self.enums.insert(e.name.clone(), variants);
            }
            Decl::Struct(s) => {
                let fields: Vec<String> = s.fields.iter().map(|f| f.name.clone()).collect();
                self.structs.insert(s.name.clone(), fields);
                let is_value = s.annotations.iter().any(|a| a.name == "value");
                self.has_value.insert(s.name.clone(), is_value);
                let has_del = s.methods.iter().any(|m| m.name == "__del__");
                self.has_del.insert(s.name.clone(), has_del || is_value);
            }
            Decl::DataClass(d) => {
                let fields: Vec<String> = d.params.iter().map(|f| f.name.clone()).collect();
                self.data_classes.insert(d.name.clone(), fields);
                let is_value = d.annotations.iter().any(|a| a.name == "value");
                self.has_value.insert(d.name.clone(), is_value);
                let has_del = d.methods.iter().any(|m| m.name == "__del__");
                self.has_del.insert(d.name.clone(), has_del || is_value);
            }
            Decl::Fn(f) => {
                let ret = f.ret_ty.as_ref()
                    .map(|t| self.lower_type_expr(t))
                    .unwrap_or(HirType::Unit);
                self.fn_ret_types.insert(f.name.clone(), ret);
            }
            _ => {}
        }
    }

    // ── Lower type expressions ──────────────────────────────────────

    fn lower_type_expr(&self, ty: &TypeExpr) -> HirType {
        match ty {
            TypeExpr::Simple(name, _) => match name.as_str() {
                "Int" => HirType::Int,
                "Float" => HirType::Float,
                "Bool" => HirType::Bool,
                "String" => HirType::Str,
                "()" => HirType::Unit,
                "Ptr" => HirType::Ptr,
                _ => {
                    if self.enums.contains_key(name) {
                        HirType::Enum(name.clone())
                    } else if self.structs.contains_key(name) {
                        HirType::Struct(name.clone())
                    } else if self.data_classes.contains_key(name) {
                        HirType::DataClass(name.clone())
                    } else {
                        HirType::Unknown
                    }
                }
            },
            TypeExpr::Generic(name, args, _) => match name.as_str() {
                "List" => {
                    let inner = args.first()
                        .map(|a| self.lower_type_expr(a))
                        .unwrap_or(HirType::Unknown);
                    HirType::List(Box::new(inner))
                }
                "Option" => {
                    let inner = args.first()
                        .map(|a| self.lower_type_expr(a))
                        .unwrap_or(HirType::Unknown);
                    HirType::Option(Box::new(inner))
                }
                "Result" => {
                    let ok = args.first()
                        .map(|a| self.lower_type_expr(a))
                        .unwrap_or(HirType::Unknown);
                    let err = args.get(1)
                        .map(|a| self.lower_type_expr(a))
                        .unwrap_or(HirType::Unknown);
                    HirType::Result(Box::new(ok), Box::new(err))
                }
                _ => HirType::Unknown,
            },
            TypeExpr::Union(_, _) => HirType::Unknown,
        }
    }

    // ── Lower declarations ──────────────────────────────────────────

    fn lower_fn_decl(&self, f: &FnDecl) -> HirFnDecl {
        let params: Vec<HirParam> = f.params.iter().map(|p| HirParam {
            name: p.name.clone(),
            convention: match p.convention.as_deref() {
                Some("ref") => Convention::Ref,
                Some("mutref") => Convention::Mutref,
                Some("owned") => Convention::Owned,
                _ => Convention::Default,
            },
            ty: p.ty.as_ref()
                .map(|t| self.lower_type_expr(t))
                .unwrap_or(HirType::Unknown),
            span: p.span.clone(),
        }).collect();

        let ret_ty = f.ret_ty.as_ref()
            .map(|t| self.lower_type_expr(t))
            .unwrap_or(HirType::Unit);

        let body = match &f.body {
            FnBody::Block(stmts) => HirFnBody::Block(self.lower_stmts(stmts)),
            FnBody::Expr(e) => HirFnBody::Expr(self.lower_expr(e)),
        };

        let is_entrypoint = f.annotations.iter().any(|a| a.name == "entrypoint");
        let has_del = f.name == "__del__";

        HirFnDecl {
            name: f.name.clone(),
            ext_type: f.ext_type.clone(),
            params,
            ret_ty,
            body,
            is_entrypoint,
            is_async: f.is_async,
            has_del,
            span: f.span.clone(),
        }
    }

    fn lower_enum_decl(&self, e: &EnumDecl) -> HirEnumDecl {
        HirEnumDecl {
            name: e.name.clone(),
            variants: e.variants.iter().map(|v| HirEnumVariant {
                name: v.name.clone(),
                fields: v.fields.iter().map(|t| self.lower_type_expr(t)).collect(),
                span: v.span.clone(),
            }).collect(),
            span: e.span.clone(),
        }
    }

    fn lower_struct_decl(&self, s: &StructDecl) -> HirStructDecl {
        let del_method = if s.methods.iter().any(|m| m.name == "__del__") {
            Some("__del__".into())
        } else {
            None
        };
        HirStructDecl {
            name: s.name.clone(),
            fields: s.fields.iter().map(|f| self.lower_field(f)).collect(),
            methods: s.methods.iter().map(|m| self.lower_fn_decl(m)).collect(),
            has_value_annotation: s.annotations.iter().any(|a| a.name == "value"),
            del_method,
            span: s.span.clone(),
        }
    }

    fn lower_data_class_decl(&self, d: &DataClassDecl) -> HirDataClassDecl {
        let del_method = if d.methods.iter().any(|m| m.name == "__del__") {
            Some("__del__".into())
        } else {
            None
        };
        HirDataClassDecl {
            name: d.name.clone(),
            fields: d.params.iter().map(|f| self.lower_field(f)).collect(),
            methods: d.methods.iter().map(|m| self.lower_fn_decl(m)).collect(),
            has_value_annotation: d.annotations.iter().any(|a| a.name == "value"),
            del_method,
            span: d.span.clone(),
        }
    }

    fn lower_extern_fn(&self, ef: &ExternFnDecl) -> HirExternFnDecl {
        HirExternFnDecl {
            name: ef.name.clone(),
            params: ef.params.iter().map(|p| HirParam {
                name: p.name.clone(),
                convention: Convention::Default,
                ty: p.ty.as_ref().map(|t| self.lower_type_expr(t)).unwrap_or(HirType::Unknown),
                span: p.span.clone(),
            }).collect(),
            ret_ty: ef.ret_ty.as_ref().map(|t| self.lower_type_expr(t)).unwrap_or(HirType::Unit),
            span: ef.span.clone(),
        }
    }

    fn lower_field(&self, f: &Field) -> HirField {
        HirField {
            mutable: f.mutable,
            name: f.name.clone(),
            ty: self.lower_type_expr(&f.ty),
            span: f.span.clone(),
        }
    }

    // ── Lower statements ────────────────────────────────────────────

    fn lower_stmts(&self, stmts: &[Stmt]) -> Vec<HirStmt> {
        stmts.iter().map(|s| self.lower_stmt(s)).collect()
    }

    fn lower_stmt(&self, stmt: &Stmt) -> HirStmt {
        match stmt {
            Stmt::Val { name, convention, ty, value, span } => {
                let hv = self.lower_expr(value);
                let ht = ty.as_ref()
                    .map(|t| self.lower_type_expr(t))
                    .unwrap_or_else(|| hv.ty.clone());
                let conv = match convention.as_deref() {
                    Some("ref") => Convention::Ref,
                    Some("mutref") => Convention::Mutref,
                    _ => Convention::Default,
                };
                HirStmt::Val { name: name.clone(), convention: conv, ty: ht, value: hv, span: span.clone() }
            }
            Stmt::ValTuple { names, value, span } => {
                let hv = self.lower_expr(value);
                HirStmt::ValTuple { names: names.clone(), value: hv, span: span.clone() }
            }
            Stmt::Var { name, ty, value, span } => {
                let hv = self.lower_expr(value);
                let ht = ty.as_ref()
                    .map(|t| self.lower_type_expr(t))
                    .unwrap_or_else(|| hv.ty.clone());
                HirStmt::Var { name: name.clone(), ty: ht, value: hv, span: span.clone() }
            }
            Stmt::Assign { target, value, span } => {
                HirStmt::Assign {
                    target: self.lower_expr(target),
                    value: self.lower_expr(value),
                    span: span.clone(),
                }
            }
            Stmt::Expr(e) => HirStmt::Expr(self.lower_expr(e)),
            Stmt::Return(opt_e, span) => {
                HirStmt::Return(opt_e.as_ref().map(|e| self.lower_expr(e)), span.clone())
            }
            Stmt::Defer(e, span) => {
                HirStmt::Defer(self.lower_expr(e), span.clone())
            }
            Stmt::If { cond, then_b, else_b, span } => {
                HirStmt::If {
                    cond: self.lower_expr(cond),
                    then_body: self.lower_stmts(then_b),
                    else_body: else_b.as_ref().map(|eb| self.lower_else_body(eb)),
                    span: span.clone(),
                }
            }
            Stmt::For { var, iter, body, span } => {
                HirStmt::For {
                    var: var.clone(),
                    iter: self.lower_expr(iter),
                    body: self.lower_stmts(body),
                    span: span.clone(),
                }
            }
            Stmt::Loop(body, span) => {
                HirStmt::Loop(self.lower_stmts(body), span.clone())
            }
        }
    }

    fn lower_else_body(&self, eb: &ElseBody) -> HirElseBody {
        match eb {
            ElseBody::ElseIf(s) => HirElseBody::ElseIf(Box::new(self.lower_stmt(s))),
            ElseBody::Block(stmts) => HirElseBody::Block(self.lower_stmts(stmts)),
        }
    }

    // ── Lower expressions ───────────────────────────────────────────

    fn lower_expr(&self, expr: &Expr) -> HirExpr {
        match expr {
            Expr::IntLit(v, span) => HirExpr {
                kind: HirExprKind::IntLit(*v),
                ty: HirType::Int,
                span: span.clone(),
            },
            Expr::FloatLit(v, span) => HirExpr {
                kind: HirExprKind::FloatLit(*v),
                ty: HirType::Float,
                span: span.clone(),
            },
            Expr::StrLit(s, span) => HirExpr {
                kind: HirExprKind::StrLit(s.clone()),
                ty: HirType::Str,
                span: span.clone(),
            },
            Expr::BoolLit(b, span) => HirExpr {
                kind: HirExprKind::BoolLit(*b),
                ty: HirType::Bool,
                span: span.clone(),
            },
            Expr::Unit(span) => HirExpr {
                kind: HirExprKind::Unit,
                ty: HirType::Unit,
                span: span.clone(),
            },
            Expr::Ident(name, span) => HirExpr {
                kind: HirExprKind::Ident(name.clone()),
                ty: HirType::Unknown, // resolved during codegen via runtime
                span: span.clone(),
            },
            Expr::SelfExpr(span) => HirExpr {
                kind: HirExprKind::SelfExpr,
                ty: HirType::Unknown,
                span: span.clone(),
            },
            Expr::FStr(parts, span) => {
                let hparts: Vec<HirExpr> = parts.iter().map(|p| self.lower_expr(p)).collect();
                HirExpr {
                    kind: HirExprKind::FStr(hparts),
                    ty: HirType::Str,
                    span: span.clone(),
                }
            }
            Expr::List(elems, span) => {
                let helems: Vec<HirExpr> = elems.iter().map(|e| self.lower_expr(e)).collect();
                let inner_ty = helems.first()
                    .map(|e| e.ty.clone())
                    .unwrap_or(HirType::Unknown);
                HirExpr {
                    kind: HirExprKind::List(helems),
                    ty: HirType::List(Box::new(inner_ty)),
                    span: span.clone(),
                }
            }
            Expr::Tuple(elems, span) => {
                let helems: Vec<HirExpr> = elems.iter().map(|e| self.lower_expr(e)).collect();
                let types: Vec<HirType> = helems.iter().map(|e| e.ty.clone()).collect();
                HirExpr {
                    kind: HirExprKind::Tuple(helems),
                    ty: HirType::Tuple(types),
                    span: span.clone(),
                }
            }
            Expr::Binary(lhs, op, rhs, span) => {
                let hl = self.lower_expr(lhs);
                let hr = self.lower_expr(rhs);
                let ty = match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt |
                    BinOp::Le | BinOp::Ge | BinOp::And | BinOp::Or => HirType::Bool,
                    BinOp::Add => {
                        // String + String → String, otherwise inherit left type.
                        if hl.ty == HirType::Str { HirType::Str } else { hl.ty.clone() }
                    }
                    _ => hl.ty.clone(),
                };
                HirExpr {
                    kind: HirExprKind::Binary(Box::new(hl), *op, Box::new(hr)),
                    ty,
                    span: span.clone(),
                }
            }
            Expr::Unary(op, inner, span) => {
                let hi = self.lower_expr(inner);
                let ty = match op {
                    UnaryOp::Not => HirType::Bool,
                    UnaryOp::Neg => hi.ty.clone(),
                };
                HirExpr {
                    kind: HirExprKind::Unary(*op, Box::new(hi)),
                    ty,
                    span: span.clone(),
                }
            }
            Expr::Call(callee, args, span) => {
                self.lower_call(callee, args, span)
            }
            Expr::Field(obj, name, span) => {
                // Check for EnumName.Variant pattern (no args — bare variant).
                if let Expr::Ident(obj_name, _) = obj.as_ref() {
                    if self.enums.contains_key(obj_name) {
                        return HirExpr {
                            kind: HirExprKind::EnumConstruct {
                                enum_name: obj_name.clone(),
                                variant: name.clone(),
                                value: None,
                            },
                            ty: HirType::Enum(obj_name.clone()),
                            span: span.clone(),
                        };
                    }
                }
                let ho = self.lower_expr(obj);
                HirExpr {
                    kind: HirExprKind::Field(Box::new(ho), name.clone()),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            Expr::OptChain(obj, name, span) => {
                let ho = self.lower_expr(obj);
                HirExpr {
                    kind: HirExprKind::OptChain(Box::new(ho), name.clone()),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            Expr::Question(inner, span) => {
                let hi = self.lower_expr(inner);
                HirExpr {
                    kind: HirExprKind::Question(Box::new(hi)),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            Expr::Elvis(lhs, rhs, span) => {
                let hl = self.lower_expr(lhs);
                let hr = self.lower_expr(rhs);
                HirExpr {
                    kind: HirExprKind::Elvis(Box::new(hl), Box::new(hr)),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            Expr::Match(subj, arms, span) => {
                let hs = self.lower_expr(subj);
                let harms: Vec<HirMatchArm> = arms.iter().map(|a| HirMatchArm {
                    pattern: self.lower_pattern(&a.pattern),
                    body: self.lower_expr(&a.body),
                    span: a.span.clone(),
                }).collect();
                let ty = harms.first()
                    .map(|a| a.body.ty.clone())
                    .unwrap_or(HirType::Unknown);
                HirExpr {
                    kind: HirExprKind::Match(Box::new(hs), harms),
                    ty,
                    span: span.clone(),
                }
            }
            Expr::When(arms, span) => {
                let harms: Vec<HirWhenArm> = arms.iter().map(|a| HirWhenArm {
                    cond: a.cond.as_ref().map(|c| self.lower_expr(c)),
                    body: self.lower_expr(&a.body),
                    span: a.span.clone(),
                }).collect();
                let ty = harms.first()
                    .map(|a| a.body.ty.clone())
                    .unwrap_or(HirType::Unknown);
                HirExpr {
                    kind: HirExprKind::When(harms),
                    ty,
                    span: span.clone(),
                }
            }
            Expr::Lambda(params, body, span) => {
                let hbody = self.lower_stmts(body);
                HirExpr {
                    kind: HirExprKind::Lambda {
                        params: params.clone(),
                        body: hbody,
                        captures: Vec::new(), // populated during codegen
                    },
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            Expr::Move(inner, span) => {
                let hi = self.lower_expr(inner);
                let ty = hi.ty.clone();
                HirExpr {
                    kind: HirExprKind::Move(Box::new(hi)),
                    ty,
                    span: span.clone(),
                }
            }
            Expr::MutrefE(inner, span) => {
                let hi = self.lower_expr(inner);
                let ty = hi.ty.clone();
                HirExpr {
                    kind: HirExprKind::MutrefE(Box::new(hi)),
                    ty,
                    span: span.clone(),
                }
            }
            Expr::RefE(inner, span) => {
                let hi = self.lower_expr(inner);
                let ty = hi.ty.clone();
                HirExpr {
                    kind: HirExprKind::RefE(Box::new(hi)),
                    ty,
                    span: span.clone(),
                }
            }
            Expr::Block(stmts, span) => {
                let hstmts = self.lower_stmts(stmts);
                HirExpr {
                    kind: HirExprKind::Block(hstmts),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            Expr::Spawn(inner, is_async, span) => {
                let hi = self.lower_expr(inner);
                HirExpr {
                    kind: HirExprKind::Spawn(Box::new(hi), *is_async),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            Expr::Await(inner, span) => {
                let hi = self.lower_expr(inner);
                HirExpr {
                    kind: HirExprKind::Await(Box::new(hi)),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            Expr::Path(obj, name, span) => {
                // Namespace::method — no args yet, just the path.
                let ho = self.lower_expr(obj);
                HirExpr {
                    kind: HirExprKind::Field(Box::new(ho), name.clone()),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
        }
    }

    /// Lower a call expression. Distinguishes between:
    /// - Regular function calls: `foo(args)`
    /// - Method calls: `obj.method(args)`
    /// - Struct/DC construction: `MyStruct(args)`
    /// - Enum variant construction: `EnumName.Variant(args)` or `Ok(v)`
    /// - Namespace calls: `Shared::new(v)`, `Chan::bounded(4)`
    fn lower_call(&self, callee: &Expr, args: &[Expr], span: &Span) -> HirExpr {
        let hargs: Vec<HirExpr> = args.iter().map(|a| self.lower_expr(a)).collect();

        match callee {
            // obj.method(args) → MethodCall or enum variant construction
            Expr::Field(obj, method, _) => {
                // Check for Enum.Variant(value) pattern
                if let Expr::Ident(name, _) = obj.as_ref() {
                    if self.enums.contains_key(name) {
                        return HirExpr {
                            kind: HirExprKind::EnumConstruct {
                                enum_name: name.clone(),
                                variant: method.clone(),
                                value: hargs.into_iter().next().map(Box::new),
                            },
                            ty: HirType::Enum(name.clone()),
                            span: span.clone(),
                        };
                    }
                }
                let ho = self.lower_expr(obj);
                let receiver_type = self.expr_type_name(&ho);
                HirExpr {
                    kind: HirExprKind::MethodCall {
                        receiver: Box::new(ho),
                        method: method.clone(),
                        args: hargs,
                        receiver_type,
                    },
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            // Namespace::method(args) — Path expression
            Expr::Path(ns_expr, method, _) => {
                if let Expr::Ident(ns, _) = ns_expr.as_ref() {
                    return HirExpr {
                        kind: HirExprKind::PathCall {
                            namespace: ns.clone(),
                            method: method.clone(),
                            args: hargs,
                        },
                        ty: HirType::Unknown,
                        span: span.clone(),
                    };
                }
                // Fallback: treat as regular call
                let hc = self.lower_expr(callee);
                HirExpr {
                    kind: HirExprKind::Call(Box::new(hc), hargs),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
            // Foo(args) — could be struct construction, DC construction, enum variant, or function call
            Expr::Ident(name, _) => {
                // Struct construction
                if let Some(fields) = self.structs.get(name) {
                    return HirExpr {
                        kind: HirExprKind::StructConstruct {
                            type_name: name.clone(),
                            args: hargs,
                            field_names: fields.clone(),
                        },
                        ty: HirType::Struct(name.clone()),
                        span: span.clone(),
                    };
                }
                // Data class construction
                if let Some(fields) = self.data_classes.get(name) {
                    return HirExpr {
                        kind: HirExprKind::StructConstruct {
                            type_name: name.clone(),
                            args: hargs,
                            field_names: fields.clone(),
                        },
                        ty: HirType::DataClass(name.clone()),
                        span: span.clone(),
                    };
                }
                // Built-in enum constructors: Ok, Err, Some
                match name.as_str() {
                    "Ok" => return HirExpr {
                        kind: HirExprKind::EnumConstruct {
                            enum_name: "Result".into(),
                            variant: "Ok".into(),
                            value: hargs.into_iter().next().map(Box::new),
                        },
                        ty: HirType::Result(Box::new(HirType::Unknown), Box::new(HirType::Unknown)),
                        span: span.clone(),
                    },
                    "Err" => return HirExpr {
                        kind: HirExprKind::EnumConstruct {
                            enum_name: "Result".into(),
                            variant: "Err".into(),
                            value: hargs.into_iter().next().map(Box::new),
                        },
                        ty: HirType::Result(Box::new(HirType::Unknown), Box::new(HirType::Unknown)),
                        span: span.clone(),
                    },
                    "Some" => return HirExpr {
                        kind: HirExprKind::EnumConstruct {
                            enum_name: "Option".into(),
                            variant: "Some".into(),
                            value: hargs.into_iter().next().map(Box::new),
                        },
                        ty: HirType::Option(Box::new(HirType::Unknown)),
                        span: span.clone(),
                    },
                    _ => {}
                }
                // Regular function call
                let ret_ty = self.fn_ret_types.get(name).cloned().unwrap_or(HirType::Unknown);
                HirExpr {
                    kind: HirExprKind::Call(
                        Box::new(HirExpr {
                            kind: HirExprKind::Ident(name.clone()),
                            ty: HirType::Unknown,
                            span: span.clone(),
                        }),
                        hargs,
                    ),
                    ty: ret_ty,
                    span: span.clone(),
                }
            }
            // Lambda call or other dynamic call
            _ => {
                let hc = self.lower_expr(callee);
                HirExpr {
                    kind: HirExprKind::Call(Box::new(hc), hargs),
                    ty: HirType::Unknown,
                    span: span.clone(),
                }
            }
        }
    }

    /// Best-effort type name for method dispatch.
    fn expr_type_name(&self, expr: &HirExpr) -> String {
        match &expr.ty {
            HirType::Int => "Int".into(),
            HirType::Float => "Float".into(),
            HirType::Bool => "Bool".into(),
            HirType::Str => "String".into(),
            HirType::List(_) => "List".into(),
            HirType::Struct(n) | HirType::DataClass(n) | HirType::Enum(n) => n.clone(),
            _ => String::new(),
        }
    }

    // ── Lower patterns ──────────────────────────────────────────────

    fn lower_pattern(&self, pat: &Pattern) -> HirPattern {
        match pat {
            Pattern::Wildcard(span) => HirPattern::Wildcard(span.clone()),
            Pattern::Ident(name, span) => HirPattern::Ident(name.clone(), HirType::Unknown, span.clone()),
            Pattern::Literal(lit, span) => {
                let hlit = match lit {
                    Lit::Int(v) => HirLit::Int(*v),
                    Lit::Float(v) => HirLit::Float(*v),
                    Lit::Str(s) => HirLit::Str(s.clone()),
                    Lit::Bool(b) => HirLit::Bool(*b),
                };
                HirPattern::Literal(hlit, span.clone())
            }
            Pattern::Constructor(name, pats, span) => {
                let hpats: Vec<HirPattern> = pats.iter().map(|p| self.lower_pattern(p)).collect();
                HirPattern::Constructor(name.clone(), hpats, HirType::Unknown, span.clone())
            }
            Pattern::Tuple(pats, span) => {
                let hpats: Vec<HirPattern> = pats.iter().map(|p| self.lower_pattern(p)).collect();
                HirPattern::Tuple(hpats, span.clone())
            }
        }
    }

    // ── ASAP last-use analysis ──────────────────────────────────────

    /// Compute ASAP destruction metadata for a sequence of statements.
    fn compute_asap(&self, stmts: &[HirStmt], fn_name: &str) -> AsapInfo {
        let mut last_use: HashMap<String, usize> = HashMap::new();
        let mut destructible = Vec::new();

        for (i, stmt) in stmts.iter().enumerate() {
            let names = self.collect_names_in_stmt(stmt);
            for name in names {
                last_use.insert(name, i);
            }
        }

        // Check which variables have destructors.
        for stmt in stmts {
            if let HirStmt::Val { name, ty, .. } | HirStmt::Var { name, ty, .. } = stmt {
                let type_name = match ty {
                    HirType::Struct(n) | HirType::DataClass(n) => Some(n.clone()),
                    _ => None,
                };
                if let Some(tn) = type_name {
                    if self.has_del.get(&tn).copied().unwrap_or(false) {
                        destructible.push(name.clone());
                    }
                }
            }
        }

        AsapInfo {
            last_use: last_use.into_iter().collect(),
            destructible,
        }
    }

    /// Collect all variable names referenced in a statement.
    fn collect_names_in_stmt(&self, stmt: &HirStmt) -> Vec<String> {
        let mut names = Vec::new();
        match stmt {
            HirStmt::Val { value, .. } | HirStmt::Var { value, .. } => {
                self.collect_names_in_expr(value, &mut names);
            }
            HirStmt::ValTuple { value, .. } => {
                self.collect_names_in_expr(value, &mut names);
            }
            HirStmt::Assign { target, value, .. } => {
                self.collect_names_in_expr(target, &mut names);
                self.collect_names_in_expr(value, &mut names);
            }
            HirStmt::Expr(e) => self.collect_names_in_expr(e, &mut names),
            HirStmt::Return(Some(e), _) => self.collect_names_in_expr(e, &mut names),
            HirStmt::Defer(e, _) => self.collect_names_in_expr(e, &mut names),
            HirStmt::If { cond, then_body, else_body, .. } => {
                self.collect_names_in_expr(cond, &mut names);
                for s in then_body { names.extend(self.collect_names_in_stmt(s)); }
                if let Some(eb) = else_body {
                    match eb {
                        HirElseBody::ElseIf(s) => names.extend(self.collect_names_in_stmt(s)),
                        HirElseBody::Block(ss) => {
                            for s in ss { names.extend(self.collect_names_in_stmt(s)); }
                        }
                    }
                }
            }
            HirStmt::For { iter, body, .. } => {
                self.collect_names_in_expr(iter, &mut names);
                for s in body { names.extend(self.collect_names_in_stmt(s)); }
            }
            HirStmt::Loop(body, _) => {
                for s in body { names.extend(self.collect_names_in_stmt(s)); }
            }
            _ => {}
        }
        names
    }

    /// Collect all identifier references in an expression.
    fn collect_names_in_expr(&self, expr: &HirExpr, names: &mut Vec<String>) {
        match &expr.kind {
            HirExprKind::Ident(n) => names.push(n.clone()),
            HirExprKind::SelfExpr => names.push("self".into()),
            HirExprKind::Binary(l, _, r) => {
                self.collect_names_in_expr(l, names);
                self.collect_names_in_expr(r, names);
            }
            HirExprKind::Unary(_, inner) | HirExprKind::Move(inner) |
            HirExprKind::MutrefE(inner) | HirExprKind::RefE(inner) |
            HirExprKind::Question(inner) | HirExprKind::Await(inner) |
            HirExprKind::Spawn(inner, _) => {
                self.collect_names_in_expr(inner, names);
            }
            HirExprKind::Call(callee, args) => {
                self.collect_names_in_expr(callee, names);
                for a in args { self.collect_names_in_expr(a, names); }
            }
            HirExprKind::MethodCall { receiver, args, .. } => {
                self.collect_names_in_expr(receiver, names);
                for a in args { self.collect_names_in_expr(a, names); }
            }
            HirExprKind::Field(obj, _) | HirExprKind::OptChain(obj, _) => {
                self.collect_names_in_expr(obj, names);
            }
            HirExprKind::Elvis(l, r) => {
                self.collect_names_in_expr(l, names);
                self.collect_names_in_expr(r, names);
            }
            HirExprKind::FStr(parts) | HirExprKind::List(parts) | HirExprKind::Tuple(parts) => {
                for p in parts { self.collect_names_in_expr(p, names); }
            }
            HirExprKind::Match(subj, arms) => {
                self.collect_names_in_expr(subj, names);
                for arm in arms { self.collect_names_in_expr(&arm.body, names); }
            }
            HirExprKind::When(arms) => {
                for arm in arms {
                    if let Some(c) = &arm.cond { self.collect_names_in_expr(c, names); }
                    self.collect_names_in_expr(&arm.body, names);
                }
            }
            HirExprKind::StructConstruct { args, .. } | HirExprKind::PathCall { args, .. } => {
                for a in args { self.collect_names_in_expr(a, names); }
            }
            HirExprKind::EnumConstruct { value, .. } => {
                if let Some(v) = value { self.collect_names_in_expr(v, names); }
            }
            HirExprKind::Lambda { body, .. } => {
                for s in body { names.extend(self.collect_names_in_stmt(s)); }
            }
            HirExprKind::Block(stmts) => {
                for s in stmts { names.extend(self.collect_names_in_stmt(s)); }
            }
            _ => {}
        }
    }
}
