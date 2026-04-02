//! HIR (High-level Intermediate Representation) node definitions.
//!
//! HIR is the AST with all type information made explicit. Every expression
//! carries a resolved `HirType`. The checker operates on AST; codegen
//! operates on HIR. AST → HIR lowering resolves types, attaches ownership
//! conventions, and computes ASAP last-use information.

use crate::ast::nodes::{BinOp, UnaryOp, Span};

// ── Resolved types ──────────────────────────────────────────────────

/// A fully resolved Fuse type. Every HIR expression node carries one.
#[derive(Debug, Clone, PartialEq)]
pub enum HirType {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    List(Box<HirType>),
    Tuple(Vec<HirType>),
    Struct(String),
    DataClass(String),
    Enum(String),
    /// Function type: parameter types → return type.
    Fn(Vec<HirType>, Box<HirType>),
    Lambda(Vec<HirType>, Box<HirType>),
    /// Option<T> and Result<T,E> as first-class types.
    Option(Box<HirType>),
    Result(Box<HirType>, Box<HirType>),
    /// Raw pointer for FFI boundaries.
    Ptr,
    /// Type not yet resolved (used during lowering as a placeholder).
    Unknown,
}

// ── Ownership conventions ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Convention {
    Ref,
    Mutref,
    Owned,
    Default, // no explicit convention — treated as ref
}

// ── Patterns ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum HirPattern {
    Wildcard(Span),
    Ident(String, HirType, Span),
    Literal(HirLit, Span),
    Constructor(String, Vec<HirPattern>, HirType, Span),
    Tuple(Vec<HirPattern>, Span),
}

#[derive(Debug, Clone)]
pub enum HirLit {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

// ── Expressions ─────────────────────────────────────────────────────

/// Every expression carries its resolved type and source span.
#[derive(Debug, Clone)]
pub struct HirExpr {
    pub kind: HirExprKind,
    pub ty: HirType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirExprKind {
    IntLit(i64),
    FloatLit(f64),
    StrLit(String),
    BoolLit(bool),
    Unit,

    /// Variable reference.
    Ident(String),
    /// `self` inside a method.
    SelfExpr,

    /// f"text {expr} text" — parts alternate between literal strings and exprs.
    FStr(Vec<HirExpr>),

    /// [a, b, c]
    List(Vec<HirExpr>),
    /// (a, b, c)
    Tuple(Vec<HirExpr>),

    /// a + b, a == b, etc.
    Binary(Box<HirExpr>, BinOp, Box<HirExpr>),
    /// -x, not x
    Unary(UnaryOp, Box<HirExpr>),

    /// function_name(arg1, arg2)
    Call(Box<HirExpr>, Vec<HirExpr>),
    /// Method call: obj.method(args) — resolved to (receiver, method_name, args).
    MethodCall {
        receiver: Box<HirExpr>,
        method: String,
        args: Vec<HirExpr>,
        /// The type name of the receiver (for dispatch).
        receiver_type: String,
    },

    /// obj.field
    Field(Box<HirExpr>, String),
    /// obj?.field — short-circuits to None if obj is None.
    OptChain(Box<HirExpr>, String),
    /// expr? — unwrap Ok/Some or early-return Err/None.
    Question(Box<HirExpr>),
    /// a ?: b — unwrap or use fallback.
    Elvis(Box<HirExpr>, Box<HirExpr>),

    /// match expr { arms }
    Match(Box<HirExpr>, Vec<HirMatchArm>),
    /// when { cond => body, else => body }
    When(Vec<HirWhenArm>),

    /// Lambda: captured env + params + body.
    Lambda {
        params: Vec<String>,
        body: Vec<HirStmt>,
        captures: Vec<String>,
    },

    /// move expr — transfer ownership.
    Move(Box<HirExpr>),
    /// mutref expr — pass as mutable reference.
    MutrefE(Box<HirExpr>),
    /// ref expr — pass as read-only reference.
    RefE(Box<HirExpr>),

    /// { stmts } as expression — value is last expression.
    Block(Vec<HirStmt>),

    /// Struct/data class construction: TypeName(field1, field2, ...).
    StructConstruct {
        type_name: String,
        args: Vec<HirExpr>,
        field_names: Vec<String>,
    },

    /// Enum variant construction: EnumName.Variant or EnumName.Variant(value).
    EnumConstruct {
        enum_name: String,
        variant: String,
        value: Option<Box<HirExpr>>,
    },

    /// Namespace path call: Shared::new(v), Chan::bounded(4).
    PathCall {
        namespace: String,
        method: String,
        args: Vec<HirExpr>,
    },

    /// spawn expr (Fuse Full).
    Spawn(Box<HirExpr>, bool),
    /// await expr (Fuse Full).
    Await(Box<HirExpr>),
}

#[derive(Debug, Clone)]
pub struct HirMatchArm {
    pub pattern: HirPattern,
    pub body: HirExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirWhenArm {
    /// None means `else` arm.
    pub cond: Option<HirExpr>,
    pub body: HirExpr,
    pub span: Span,
}

// ── Statements ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum HirStmt {
    /// val [ref|mutref] name = expr
    Val {
        name: String,
        convention: Convention,
        ty: HirType,
        value: HirExpr,
        span: Span,
    },
    /// val (a, b) = expr
    ValTuple {
        names: Vec<String>,
        value: HirExpr,
        span: Span,
    },
    /// var name = expr
    Var {
        name: String,
        ty: HirType,
        value: HirExpr,
        span: Span,
    },
    /// target = expr
    Assign {
        target: HirExpr,
        value: HirExpr,
        span: Span,
    },
    /// expression as statement
    Expr(HirExpr),
    /// return [expr]
    Return(Option<HirExpr>, Span),
    /// defer expr
    Defer(HirExpr, Span),
    /// if cond { then } [else { else }]
    If {
        cond: HirExpr,
        then_body: Vec<HirStmt>,
        else_body: Option<HirElseBody>,
        span: Span,
    },
    /// for var in iter { body }
    For {
        var: String,
        iter: HirExpr,
        body: Vec<HirStmt>,
        span: Span,
    },
    /// loop { body }
    Loop(Vec<HirStmt>, Span),
}

#[derive(Debug, Clone)]
pub enum HirElseBody {
    ElseIf(Box<HirStmt>),
    Block(Vec<HirStmt>),
}

// ── Declarations ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: String,
    pub convention: Convention,
    pub ty: HirType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirField {
    pub mutable: bool,
    pub name: String,
    pub ty: HirType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirFnBody {
    Block(Vec<HirStmt>),
    Expr(HirExpr),
}

#[derive(Debug, Clone)]
pub struct HirFnDecl {
    pub name: String,
    /// Some("TypeName") for extension functions.
    pub ext_type: Option<String>,
    pub params: Vec<HirParam>,
    pub ret_ty: HirType,
    pub body: HirFnBody,
    pub is_entrypoint: bool,
    pub is_async: bool,
    pub has_del: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirEnumVariant {
    pub name: String,
    pub fields: Vec<HirType>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirEnumDecl {
    pub name: String,
    pub variants: Vec<HirEnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirStructDecl {
    pub name: String,
    pub fields: Vec<HirField>,
    pub methods: Vec<HirFnDecl>,
    pub has_value_annotation: bool,
    pub del_method: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirDataClassDecl {
    pub name: String,
    pub fields: Vec<HirField>,
    pub methods: Vec<HirFnDecl>,
    pub has_value_annotation: bool,
    pub del_method: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirDecl {
    Fn(HirFnDecl),
    Enum(HirEnumDecl),
    Struct(HirStructDecl),
    DataClass(HirDataClassDecl),
    TopVal {
        name: String,
        ty: HirType,
        value: HirExpr,
        span: Span,
    },
    TopVar {
        name: String,
        ty: HirType,
        value: HirExpr,
        span: Span,
    },
}

// ── ASAP destruction info ───────────────────────────────────────────

/// Per-function metadata computed during lowering for codegen.
#[derive(Debug, Clone)]
pub struct AsapInfo {
    /// Maps variable name → index of the last statement that references it.
    pub last_use: Vec<(String, usize)>,
    /// Variables that have a `__del__` method.
    pub destructible: Vec<String>,
}

// ── Program ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HirProgram {
    pub decls: Vec<HirDecl>,
    /// Registry of enum types for codegen dispatch.
    pub enums: Vec<HirEnumDecl>,
    /// Registry of struct types.
    pub structs: Vec<HirStructDecl>,
    /// Registry of data class types.
    pub data_classes: Vec<HirDataClassDecl>,
    /// Per-function ASAP destruction metadata, keyed by function name.
    pub asap: Vec<(String, AsapInfo)>,
    pub span: Span,
}
