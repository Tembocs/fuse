#[derive(Debug, Clone)]
pub struct Span { pub line: usize, pub col: usize }

// ── Type expressions ─────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum TypeExpr {
    Simple(String, Span),
    Generic(String, Vec<TypeExpr>, Span),
    Union(Vec<TypeExpr>, Span),
}

// ── Patterns ─────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard(Span),
    Ident(String, Span),
    Literal(Lit, Span),
    Constructor(String, Vec<Pattern>, Span),
    Tuple(Vec<Pattern>, Span),
}

#[derive(Debug, Clone)]
pub enum Lit { Int(i64), Float(f64), Str(String), Bool(bool) }

// ── Expressions ──────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum Expr {
    IntLit(i64, Span),
    FloatLit(f64, Span),
    StrLit(String, Span),
    BoolLit(bool, Span),
    Unit(Span),
    Ident(String, Span),
    SelfExpr(Span),
    FStr(Vec<Expr>, Span),
    List(Vec<Expr>, Span),
    Tuple(Vec<Expr>, Span),
    Binary(Box<Expr>, BinOp, Box<Expr>, Span),
    Unary(UnaryOp, Box<Expr>, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
    Field(Box<Expr>, String, Span),
    OptChain(Box<Expr>, String, Span),
    Question(Box<Expr>, Span),
    Elvis(Box<Expr>, Box<Expr>, Span),
    Match(Box<Expr>, Vec<MatchArm>, Span),
    When(Vec<WhenArm>, Span),
    Lambda(Vec<String>, Vec<Stmt>, Span),
    Move(Box<Expr>, Span),
    MutrefE(Box<Expr>, Span),
    RefE(Box<Expr>, Span),
    Block(Vec<Stmt>, Span),
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Gt, Le, Ge,
    And, Or,
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp { Neg, Not }

#[derive(Debug, Clone)]
pub struct MatchArm { pub pattern: Pattern, pub body: Expr, pub span: Span }

#[derive(Debug, Clone)]
pub struct WhenArm { pub cond: Option<Expr>, pub body: Expr, pub span: Span }

// ── Statements ───────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum Stmt {
    Val { name: String, ty: Option<TypeExpr>, value: Expr, span: Span },
    Var { name: String, ty: Option<TypeExpr>, value: Expr, span: Span },
    Assign { target: Expr, value: Expr, span: Span },
    Expr(Expr),
    Return(Option<Expr>, Span),
    Defer(Expr, Span),
    If { cond: Expr, then_b: Vec<Stmt>, else_b: Option<ElseBody>, span: Span },
    For { var: String, iter: Expr, body: Vec<Stmt>, span: Span },
    Loop(Vec<Stmt>, Span),
}

#[derive(Debug, Clone)]
pub enum ElseBody { ElseIf(Box<Stmt>), Block(Vec<Stmt>) }

// ── Declarations ─────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct Annotation { pub name: String, pub args: Vec<Expr>, pub span: Span }

#[derive(Debug, Clone)]
pub struct Param {
    pub convention: Option<String>,
    pub name: String,
    pub ty: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub mutable: bool,
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: String,
    pub ext_type: Option<String>,
    pub params: Vec<Param>,
    pub ret_ty: Option<TypeExpr>,
    pub body: FnBody,
    pub annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum FnBody { Block(Vec<Stmt>), Expr(Expr) }

#[derive(Debug, Clone)]
pub struct EnumVariant { pub name: String, pub fields: Vec<TypeExpr>, pub span: Span }

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<Field>,
    pub methods: Vec<FnDecl>,
    pub annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DataClassDecl {
    pub name: String,
    pub params: Vec<Field>,
    pub methods: Vec<FnDecl>,
    pub annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Decl {
    Fn(FnDecl),
    Enum(EnumDecl),
    Struct(StructDecl),
    DataClass(DataClassDecl),
    TopVal { name: String, ty: Option<TypeExpr>, value: Expr, span: Span },
    TopVar { name: String, ty: Option<TypeExpr>, value: Expr, span: Span },
}

#[derive(Debug, Clone)]
pub struct Program { pub decls: Vec<Decl>, pub span: Span }
