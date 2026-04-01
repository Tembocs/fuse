use crate::error::FuseError;
use crate::lexer::token::*;
use crate::ast::nodes::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    file: String,
    allow_brace: bool,
}

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════
impl Parser {
    pub fn new(tokens: Vec<Token>, file: &str) -> Self {
        Self { tokens, pos: 0, file: file.into(), allow_brace: true }
    }

    fn span(&self) -> Span { let t = &self.tokens[self.pos]; Span { line: t.line, col: t.col } }
    fn peek(&self) -> &Tok { &self.tokens[self.pos].ty }
    fn at(&self, t: &Tok) -> bool { std::mem::discriminant(self.peek()) == std::mem::discriminant(t) }
    fn at_eof(&self) -> bool { matches!(self.peek(), Tok::Eof) }

    fn advance(&mut self) -> &Token { let t = &self.tokens[self.pos]; self.pos += 1; t }
    fn advance_clone(&mut self) -> Token { let t = self.tokens[self.pos].clone(); self.pos += 1; t }

    fn expect(&mut self, expected: &Tok, ctx: &str) -> Result<Token, FuseError> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            Ok(self.advance_clone())
        } else {
            Err(self.err(&format!("expected {expected:?}, got {:?} in {ctx}", self.peek())))
        }
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) { self.pos += 1; true } else { false }
    }

    fn expect_ident(&mut self, ctx: &str) -> Result<String, FuseError> {
        match self.peek().clone() {
            Tok::Ident(s) => { self.pos += 1; Ok(s) }
            Tok::SelfKw => { self.pos += 1; Ok("self".into()) }
            other => Err(self.err(&format!("expected identifier, got {other:?} in {ctx}"))),
        }
    }

    fn ident_value(&self) -> Option<String> {
        match self.peek() { Tok::Ident(s) => Some(s.clone()), _ => None }
    }

    fn err(&self, msg: &str) -> FuseError {
        let t = &self.tokens[self.pos];
        FuseError::new(msg, &self.file, t.line, t.col)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Program / declarations
// ═══════════════════════════════════════════════════════════════════════
impl Parser {
    pub fn parse(&mut self) -> Result<Program, FuseError> {
        let span = self.span();
        let mut decls = Vec::new();
        while !self.at_eof() {
            decls.push(self.declaration()?);
        }
        Ok(Program { decls, span })
    }

    fn annotation(&mut self) -> Result<Annotation, FuseError> {
        let span = self.span();
        self.expect(&Tok::At, "annotation")?;
        let name = self.expect_ident("annotation name")?;
        let mut args = Vec::new();
        if self.eat(&Tok::LParen) {
            if !self.at(&Tok::RParen) {
                args.push(self.expr()?);
                while self.eat(&Tok::Comma) { args.push(self.expr()?); }
            }
            self.expect(&Tok::RParen, "annotation")?;
        }
        Ok(Annotation { name, args, span })
    }

    fn declaration(&mut self) -> Result<Decl, FuseError> {
        let mut anns = Vec::new();
        while self.at(&Tok::At) { anns.push(self.annotation()?); }

        match self.peek() {
            Tok::Fn => Ok(Decl::Fn(self.fn_decl(anns, false, false)?)),
            Tok::Async => {
                self.pos += 1;
                Ok(Decl::Fn(self.fn_decl(anns, true, false)?))
            }
            Tok::Suspend => {
                self.pos += 1;
                let is_async = self.eat(&Tok::Async);
                Ok(Decl::Fn(self.fn_decl(anns, is_async, true)?))
            }
            Tok::Enum => Ok(Decl::Enum(self.enum_decl(anns)?)),
            Tok::Struct => Ok(Decl::Struct(self.struct_decl(anns)?)),
            Tok::Ident(s) if s == "data" => Ok(Decl::DataClass(self.data_class_decl(anns)?)),
            Tok::Val => { let d = self.val_decl()?; Ok(Decl::TopVal { name: d.0, ty: d.2, value: d.3, annotations: anns, span: d.4 }) }
            Tok::Var => { let d = self.var_decl()?; Ok(Decl::TopVar { name: d.0, ty: d.1, value: d.2, annotations: anns, span: d.3 }) }
            _ => Err(self.err("expected declaration")),
        }
    }

    // ── fn ───────────────────────────────────────────────────────────
    fn fn_decl(&mut self, annotations: Vec<Annotation>, is_async: bool, is_suspend: bool) -> Result<FnDecl, FuseError> {
        let span = self.span();
        self.expect(&Tok::Fn, "fn")?;
        let mut name = self.expect_ident("function name")?;
        let mut ext_type = None;
        if self.eat(&Tok::Dot) {
            ext_type = Some(name);
            name = self.expect_ident("method name")?;
        }
        self.expect(&Tok::LParen, "fn params")?;
        let mut params = Vec::new();
        if !self.at(&Tok::RParen) {
            params.push(self.param()?);
            while self.eat(&Tok::Comma) { params.push(self.param()?); }
        }
        self.expect(&Tok::RParen, "fn params")?;
        let ret_ty = if self.eat(&Tok::Arrow) { Some(self.type_expr()?) } else { None };
        let body = if self.eat(&Tok::FatArrow) {
            FnBody::Expr(self.expr()?)
        } else {
            FnBody::Block(self.block()?)
        };
        Ok(FnDecl { name, ext_type, params, ret_ty, body, annotations, is_async, is_suspend, span })
    }

    fn param(&mut self) -> Result<Param, FuseError> {
        let span = self.span();
        let conv = match self.peek() {
            Tok::Ref | Tok::Mutref | Tok::Owned => {
                let s = format!("{:?}", self.peek()).to_lowercase();
                let c = match self.peek() { Tok::Ref => "ref", Tok::Mutref => "mutref", _ => "owned" };
                let _ = s; // suppress warning
                self.pos += 1;
                Some(c.to_string())
            }
            _ => None,
        };
        let name = self.expect_ident("parameter")?;
        let ty = if self.eat(&Tok::Colon) { Some(self.type_expr()?) }
                 else if name == "self" { Some(TypeExpr::Simple("Self".into(), span.clone())) }
                 else { return Err(self.err("expected ':' after parameter name")); };
        Ok(Param { convention: conv, name, ty, span })
    }

    // ── enum ─────────────────────────────────────────────────────────
    fn enum_decl(&mut self, annotations: Vec<Annotation>) -> Result<EnumDecl, FuseError> {
        let span = self.span();
        self.expect(&Tok::Enum, "enum")?;
        let name = self.expect_ident("enum name")?;
        self.expect(&Tok::LBrace, "enum")?;
        let mut variants = Vec::new();
        while !self.at(&Tok::RBrace) {
            let vspan = self.span();
            let vname = self.expect_ident("variant")?;
            let mut fields = Vec::new();
            if self.eat(&Tok::LParen) {
                if !self.at(&Tok::RParen) {
                    fields.push(self.type_expr()?);
                    while self.eat(&Tok::Comma) { fields.push(self.type_expr()?); }
                }
                self.expect(&Tok::RParen, "variant")?;
            }
            self.eat(&Tok::Comma);
            variants.push(EnumVariant { name: vname, fields, span: vspan });
        }
        self.expect(&Tok::RBrace, "enum")?;
        Ok(EnumDecl { name, variants, annotations, span })
    }

    // ── struct ───────────────────────────────────────────────────────
    fn struct_decl(&mut self, annotations: Vec<Annotation>) -> Result<StructDecl, FuseError> {
        let span = self.span();
        self.expect(&Tok::Struct, "struct")?;
        let name = self.expect_ident("struct name")?;
        self.expect(&Tok::LBrace, "struct")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.at(&Tok::RBrace) {
            if self.at(&Tok::Fn) { methods.push(self.fn_decl(vec![], false, false)?); }
            else if self.at(&Tok::Val) || self.at(&Tok::Var) { fields.push(self.field()?); }
            else { return Err(self.err("expected field or method in struct")); }
        }
        self.expect(&Tok::RBrace, "struct")?;
        Ok(StructDecl { name, fields, methods, annotations, span })
    }

    fn field(&mut self) -> Result<Field, FuseError> {
        let span = self.span();
        let mutable = self.at(&Tok::Var);
        self.pos += 1; // consume val/var
        let name = self.expect_ident("field name")?;
        self.expect(&Tok::Colon, "field type")?;
        let ty = self.type_expr()?;
        Ok(Field { mutable, name, ty, span })
    }

    // ── data class ───────────────────────────────────────────────────
    fn data_class_decl(&mut self, annotations: Vec<Annotation>) -> Result<DataClassDecl, FuseError> {
        let span = self.span();
        self.pos += 1; // 'data'
        self.expect(&Tok::Class, "data class")?;
        let name = self.expect_ident("data class name")?;
        self.expect(&Tok::LParen, "data class")?;
        let mut params = Vec::new();
        if !self.at(&Tok::RParen) {
            params.push(self.field()?);
            while self.eat(&Tok::Comma) { params.push(self.field()?); }
        }
        self.expect(&Tok::RParen, "data class")?;
        let mut methods = Vec::new();
        if self.at(&Tok::LBrace) {
            self.pos += 1;
            while !self.at(&Tok::RBrace) {
                if self.at(&Tok::Fn) { methods.push(self.fn_decl(vec![], false, false)?); }
                else { return Err(self.err("expected method in data class body")); }
            }
            self.expect(&Tok::RBrace, "data class body")?;
        }
        Ok(DataClassDecl { name, params, methods, annotations, span })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Blocks / statements
// ═══════════════════════════════════════════════════════════════════════
impl Parser {
    fn block(&mut self) -> Result<Vec<Stmt>, FuseError> {
        self.expect(&Tok::LBrace, "block")?;
        let mut stmts = Vec::new();
        while !self.at(&Tok::RBrace) { stmts.push(self.stmt()?); }
        self.expect(&Tok::RBrace, "block")?;
        Ok(stmts)
    }

    fn stmt(&mut self) -> Result<Stmt, FuseError> {
        match self.peek() {
            Tok::Val => {
                // Check for tuple destructuring: val (a, b) = expr
                let span = self.span();
                self.pos += 1; // consume val
                if self.at(&Tok::LParen) {
                    self.pos += 1;
                    let mut names = Vec::new();
                    names.push(self.expect_ident("val tuple")?);
                    while self.eat(&Tok::Comma) { names.push(self.expect_ident("val tuple")?); }
                    self.expect(&Tok::RParen, "val tuple")?;
                    self.expect(&Tok::Eq, "val tuple")?;
                    let value = self.expr()?;
                    Ok(Stmt::ValTuple { names, value, span })
                } else {
                    // optional convention: val ref x = ..., val mutref x = ...
                    let conv = match self.peek() {
                        Tok::Ref => { self.pos += 1; Some("ref".to_string()) }
                        Tok::Mutref => { self.pos += 1; Some("mutref".to_string()) }
                        _ => None,
                    };
                    let name = self.expect_ident("val name")?;
                    let ty = if self.eat(&Tok::Colon) { Some(self.type_expr()?) } else { None };
                    self.expect(&Tok::Eq, "val")?;
                    let value = self.expr()?;
                    Ok(Stmt::Val { name, convention: conv, ty, value, span })
                }
            }
            Tok::Var => { let d = self.var_decl()?; Ok(Stmt::Var { name: d.0, ty: d.1, value: d.2, span: d.3 }) }
            Tok::Return => self.return_stmt(),
            Tok::Defer => self.defer_stmt(),
            Tok::If => self.if_stmt(),
            Tok::For => self.for_stmt(),
            Tok::Loop => { let span = self.span(); self.pos += 1; Ok(Stmt::Loop(self.block()?, span)) }
            _ => {
                let span = self.span();
                let e = self.expr()?;
                if self.eat(&Tok::Eq) {
                    let v = self.expr()?;
                    Ok(Stmt::Assign { target: e, value: v, span })
                } else {
                    Ok(Stmt::Expr(e))
                }
            }
        }
    }

    fn val_decl(&mut self) -> Result<(String, Option<String>, Option<TypeExpr>, Expr, Span), FuseError> {
        let span = self.span();
        self.expect(&Tok::Val, "val")?;
        // optional convention: val ref x = ..., val mutref x = ...
        let conv = match self.peek() {
            Tok::Ref => { self.pos += 1; Some("ref".to_string()) }
            Tok::Mutref => { self.pos += 1; Some("mutref".to_string()) }
            _ => None,
        };
        let name = self.expect_ident("val name")?;
        let ty = if self.eat(&Tok::Colon) { Some(self.type_expr()?) } else { None };
        self.expect(&Tok::Eq, "val")?;
        let value = self.expr()?;
        Ok((name, conv, ty, value, span))
    }

    fn var_decl(&mut self) -> Result<(String, Option<TypeExpr>, Expr, Span), FuseError> {
        let span = self.span();
        self.expect(&Tok::Var, "var")?;
        let name = self.expect_ident("var name")?;
        let ty = if self.eat(&Tok::Colon) { Some(self.type_expr()?) } else { None };
        self.expect(&Tok::Eq, "var")?;
        let value = self.expr()?;
        Ok((name, ty, value, span))
    }

    fn return_stmt(&mut self) -> Result<Stmt, FuseError> {
        let span = self.span(); self.pos += 1;
        let val = if self.at(&Tok::RBrace) || self.at_eof() { None } else { Some(self.expr()?) };
        Ok(Stmt::Return(val, span))
    }

    fn defer_stmt(&mut self) -> Result<Stmt, FuseError> {
        let span = self.span(); self.pos += 1;
        Ok(Stmt::Defer(self.expr()?, span))
    }

    fn if_stmt(&mut self) -> Result<Stmt, FuseError> {
        let span = self.span(); self.pos += 1;
        let saved = self.allow_brace; self.allow_brace = false;
        let cond = self.expr()?;
        self.allow_brace = saved;
        let then_b = self.block()?;
        let else_b = if self.eat(&Tok::Else) {
            if self.at(&Tok::If) {
                Some(ElseBody::ElseIf(Box::new(self.if_stmt()?)))
            } else {
                Some(ElseBody::Block(self.block()?))
            }
        } else { None };
        Ok(Stmt::If { cond, then_b, else_b, span })
    }

    fn for_stmt(&mut self) -> Result<Stmt, FuseError> {
        let span = self.span(); self.pos += 1;
        let var = self.expect_ident("for var")?;
        self.expect(&Tok::In, "for")?;
        let saved = self.allow_brace; self.allow_brace = false;
        let iter = self.expr()?;
        self.allow_brace = saved;
        let body = self.block()?;
        Ok(Stmt::For { var, iter, body, span })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Expressions — precedence climbing
// ═══════════════════════════════════════════════════════════════════════
impl Parser {
    fn expr(&mut self) -> Result<Expr, FuseError> { self.elvis() }

    fn elvis(&mut self) -> Result<Expr, FuseError> {
        let left = self.or()?;
        if self.eat(&Tok::Elvis) {
            let right = self.or()?;
            let span = expr_span(&left);
            Ok(Expr::Elvis(Box::new(left), Box::new(right), span))
        } else { Ok(left) }
    }

    fn or(&mut self) -> Result<Expr, FuseError> {
        let mut left = self.and()?;
        while self.eat(&Tok::Or) {
            let right = self.and()?;
            let span = expr_span(&left);
            left = Expr::Binary(Box::new(left), BinOp::Or, Box::new(right), span);
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, FuseError> {
        let mut left = self.not()?;
        while self.eat(&Tok::And) {
            let right = self.not()?;
            let span = expr_span(&left);
            left = Expr::Binary(Box::new(left), BinOp::And, Box::new(right), span);
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr, FuseError> {
        if self.at(&Tok::Not) {
            let span = self.span(); self.pos += 1;
            let operand = self.not()?;
            Ok(Expr::Unary(UnaryOp::Not, Box::new(operand), span))
        } else { self.comparison() }
    }

    fn comparison(&mut self) -> Result<Expr, FuseError> {
        let left = self.addition()?;
        let op = match self.peek() {
            Tok::EqEq => Some(BinOp::Eq), Tok::BangEq => Some(BinOp::Ne),
            Tok::Lt => Some(BinOp::Lt), Tok::Gt => Some(BinOp::Gt),
            Tok::LtEq => Some(BinOp::Le), Tok::GtEq => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let right = self.addition()?;
            let span = expr_span(&left);
            Ok(Expr::Binary(Box::new(left), op, Box::new(right), span))
        } else { Ok(left) }
    }

    fn addition(&mut self) -> Result<Expr, FuseError> {
        let mut left = self.multiplication()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => Some(BinOp::Add), Tok::Minus => Some(BinOp::Sub), _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let right = self.multiplication()?;
                let span = expr_span(&left);
                left = Expr::Binary(Box::new(left), op, Box::new(right), span);
            } else { break; }
        }
        Ok(left)
    }

    fn multiplication(&mut self) -> Result<Expr, FuseError> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => Some(BinOp::Mul), Tok::Slash => Some(BinOp::Div),
                Tok::Percent => Some(BinOp::Mod), _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let right = self.unary()?;
                let span = expr_span(&left);
                left = Expr::Binary(Box::new(left), op, Box::new(right), span);
            } else { break; }
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, FuseError> {
        match self.peek() {
            Tok::Minus => { let sp = self.span(); self.pos += 1; let o = self.unary()?; Ok(Expr::Unary(UnaryOp::Neg, Box::new(o), sp)) }
            Tok::Move  => { let sp = self.span(); self.pos += 1; let o = self.unary()?; Ok(Expr::Move(Box::new(o), sp)) }
            Tok::Ref   => { let sp = self.span(); self.pos += 1; let o = self.unary()?; Ok(Expr::RefE(Box::new(o), sp)) }
            Tok::Mutref => { let sp = self.span(); self.pos += 1; let o = self.unary()?; Ok(Expr::MutrefE(Box::new(o), sp)) }
            Tok::Await => { let sp = self.span(); self.pos += 1; let o = self.unary()?; Ok(Expr::Await(Box::new(o), sp)) }
            Tok::Spawn => {
                let sp = self.span(); self.pos += 1;
                let is_async = self.eat(&Tok::Async);
                let o = self.unary()?;
                Ok(Expr::Spawn(Box::new(o), is_async, sp))
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<Expr, FuseError> {
        let mut e = self.primary()?;
        loop {
            if self.eat(&Tok::Dot) {
                let name = self.expect_ident("field")?;
                let span = expr_span(&e);
                e = Expr::Field(Box::new(e), name, span);
            } else if self.eat(&Tok::ColonColon) {
                // Handle :: path expressions: Shared::new, Chan::<T>
                // Skip optional turbofish: ::<Type>
                if self.eat(&Tok::Lt) {
                    // turbofish ::<T> — parse type args then skip >
                    let _ty = self.type_expr()?;
                    while self.eat(&Tok::Comma) { let _ = self.type_expr()?; }
                    self.expect(&Tok::Gt, "turbofish")?;
                }
                if matches!(self.peek(), Tok::Ident(_)) {
                    let name = self.expect_ident("::")?;
                    let span = expr_span(&e);
                    e = Expr::Path(Box::new(e), name, span);
                }
            } else if self.at(&Tok::Lt) && self.is_generic_namespace() {
                // Handle Type<Args>.method() — e.g. SIMD<Float32, 4>.sum(values)
                self.pos += 1; // <
                let _ty = self.type_expr()?;
                while self.eat(&Tok::Comma) { let _ = self.type_expr()?; }
                self.expect(&Tok::Gt, "generic namespace")?;
                // continue postfix loop — next will likely be .field or .method
            } else if self.eat(&Tok::QuestionDot) {
                let name = self.expect_ident("?. field")?;
                let span = expr_span(&e);
                e = Expr::OptChain(Box::new(e), name, span);
            } else if self.at(&Tok::Question) {
                let span = expr_span(&e);
                self.pos += 1;
                e = Expr::Question(Box::new(e), span);
            } else if self.at(&Tok::LParen) {
                e = self.call(e)?;
            } else if self.at(&Tok::LBrace) && self.allow_brace && self.is_lambda() {
                let lam = self.lambda()?;
                let span = expr_span(&e);
                e = Expr::Call(Box::new(e), vec![lam], span);
            } else { break; }
        }
        Ok(e)
    }

    fn call(&mut self, callee: Expr) -> Result<Expr, FuseError> {
        self.expect(&Tok::LParen, "call")?;
        let mut args = Vec::new();
        if !self.at(&Tok::RParen) {
            args.push(self.expr()?);
            while self.eat(&Tok::Comma) { args.push(self.expr()?); }
        }
        self.expect(&Tok::RParen, "call")?;
        if self.at(&Tok::LBrace) && self.allow_brace && self.is_lambda() {
            args.push(self.lambda()?);
        }
        let span = expr_span(&callee);
        Ok(Expr::Call(Box::new(callee), args, span))
    }

    fn primary(&mut self) -> Result<Expr, FuseError> {
        let span = self.span();
        match self.peek().clone() {
            Tok::Int(v) => { self.pos += 1; Ok(Expr::IntLit(v, span)) }
            Tok::Float(v) => { self.pos += 1; Ok(Expr::FloatLit(v, span)) }
            Tok::Str(v) => { self.pos += 1; Ok(Expr::StrLit(v, span)) }
            Tok::True => { self.pos += 1; Ok(Expr::BoolLit(true, span)) }
            Tok::False => { self.pos += 1; Ok(Expr::BoolLit(false, span)) }
            Tok::SelfKw => { self.pos += 1; Ok(Expr::SelfExpr(span)) }
            Tok::Ident(name) => { self.pos += 1; Ok(Expr::Ident(name, span)) }
            Tok::FString(parts) => { self.pos += 1; self.parse_fstring(parts, span) }
            Tok::LParen => {
                self.pos += 1;
                if self.at(&Tok::RParen) { self.pos += 1; return Ok(Expr::Unit(span)); }
                let first = self.expr()?;
                if self.eat(&Tok::Comma) {
                    let mut elems = vec![first];
                    if !self.at(&Tok::RParen) {
                        elems.push(self.expr()?);
                        while self.eat(&Tok::Comma) { elems.push(self.expr()?); }
                    }
                    self.expect(&Tok::RParen, "tuple")?;
                    Ok(Expr::Tuple(elems, span))
                } else {
                    self.expect(&Tok::RParen, "paren expr")?;
                    Ok(first)
                }
            }
            Tok::LBracket => self.list_literal(),
            Tok::Match => self.match_expr(),
            Tok::When => self.when_expr(),
            Tok::LBrace => {
                let stmts = self.block()?;
                Ok(Expr::Block(stmts, span))
            }
            _ => Err(self.err(&format!("expected expression, got {:?}", self.peek()))),
        }
    }

    fn list_literal(&mut self) -> Result<Expr, FuseError> {
        let span = self.span();
        self.expect(&Tok::LBracket, "list")?;
        let mut elems = Vec::new();
        if !self.at(&Tok::RBracket) {
            elems.push(self.expr()?);
            while self.eat(&Tok::Comma) {
                if self.at(&Tok::RBracket) { break; }
                elems.push(self.expr()?);
            }
        }
        self.expect(&Tok::RBracket, "list")?;
        Ok(Expr::List(elems, span))
    }

    fn match_expr(&mut self) -> Result<Expr, FuseError> {
        let span = self.span(); self.pos += 1;
        let saved = self.allow_brace; self.allow_brace = false;
        let subject = self.expr()?;
        self.allow_brace = saved;
        self.expect(&Tok::LBrace, "match")?;
        let mut arms = Vec::new();
        while !self.at(&Tok::RBrace) {
            let aspan = self.span();
            let pattern = self.pattern()?;
            self.expect(&Tok::FatArrow, "match arm")?;
            let body = self.expr()?;
            arms.push(MatchArm { pattern, body, span: aspan });
        }
        self.expect(&Tok::RBrace, "match")?;
        Ok(Expr::Match(Box::new(subject), arms, span))
    }

    fn when_expr(&mut self) -> Result<Expr, FuseError> {
        let span = self.span(); self.pos += 1;
        self.expect(&Tok::LBrace, "when")?;
        let mut arms = Vec::new();
        while !self.at(&Tok::RBrace) {
            let aspan = self.span();
            if self.eat(&Tok::Else) {
                self.expect(&Tok::FatArrow, "when else")?;
                let body = self.expr()?;
                arms.push(WhenArm { cond: None, body, span: aspan });
            } else {
                let saved = self.allow_brace; self.allow_brace = false;
                let cond = self.expr()?;
                self.allow_brace = saved;
                self.expect(&Tok::FatArrow, "when arm")?;
                let body = self.expr()?;
                arms.push(WhenArm { cond: Some(cond), body, span: aspan });
            }
        }
        self.expect(&Tok::RBrace, "when")?;
        Ok(Expr::When(arms, span))
    }

    fn parse_fstring(&mut self, parts: Vec<FStringPart>, span: Span) -> Result<Expr, FuseError> {
        let mut exprs = Vec::new();
        for part in parts {
            match part {
                FStringPart::Str(s) => exprs.push(Expr::StrLit(s, span.clone())),
                FStringPart::Expr(src) => {
                    let mut lexer = crate::lexer::Lexer::new(&src, &self.file);
                    let toks = lexer.tokenize()?;
                    let mut sub = Parser::new(toks, &self.file);
                    exprs.push(sub.expr()?);
                }
            }
        }
        Ok(Expr::FStr(exprs, span))
    }

    // ── lambda ───────────────────────────────────────────────────────
    fn is_lambda(&self) -> bool {
        let mut i = self.pos;
        if self.tokens.get(i).map(|t| &t.ty) != Some(&Tok::LBrace) { return false; }
        i += 1;
        if !matches!(self.tokens.get(i).map(|t| &t.ty), Some(Tok::Ident(_))) { return false; }
        i += 1;
        while matches!(self.tokens.get(i).map(|t| &t.ty), Some(Tok::Comma)) {
            i += 1;
            if !matches!(self.tokens.get(i).map(|t| &t.ty), Some(Tok::Ident(_))) { return false; }
            i += 1;
        }
        matches!(self.tokens.get(i).map(|t| &t.ty), Some(Tok::FatArrow))
    }

    /// Lookahead: is the current `<` part of a generic namespace like `SIMD<Float32, 4>.sum()`?
    /// Returns true if we see `< ... >` followed by `.`
    fn is_generic_namespace(&self) -> bool {
        let mut i = self.pos;
        if self.tokens.get(i).map(|t| &t.ty) != Some(&Tok::Lt) { return false; }
        i += 1;
        let mut depth = 1u32;
        while let Some(t) = self.tokens.get(i) {
            match &t.ty {
                Tok::Lt => depth += 1,
                Tok::Gt => { depth -= 1; if depth == 0 { break; } }
                Tok::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        if depth != 0 { return false; }
        i += 1; // skip >
        matches!(self.tokens.get(i).map(|t| &t.ty), Some(Tok::Dot))
    }

    fn lambda(&mut self) -> Result<Expr, FuseError> {
        let span = self.span();
        self.expect(&Tok::LBrace, "lambda")?;
        let mut params = Vec::new();
        params.push(self.expect_ident("lambda param")?);
        while self.eat(&Tok::Comma) { params.push(self.expect_ident("lambda param")?); }
        self.expect(&Tok::FatArrow, "lambda")?;
        let mut body = Vec::new();
        while !self.at(&Tok::RBrace) { body.push(self.stmt()?); }
        self.expect(&Tok::RBrace, "lambda")?;
        Ok(Expr::Lambda(params, body, span))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Patterns
// ═══════════════════════════════════════════════════════════════════════
impl Parser {
    fn pattern(&mut self) -> Result<Pattern, FuseError> {
        let span = self.span();
        match self.peek().clone() {
            Tok::Ident(s) if s == "_" => { self.pos += 1; Ok(Pattern::Wildcard(span)) }
            Tok::Ident(_) => {
                let mut name = self.expect_ident("pattern")?;
                while self.eat(&Tok::Dot) { name.push('.'); name.push_str(&self.expect_ident("pattern")?); }
                if self.eat(&Tok::LParen) {
                    let mut args = Vec::new();
                    if !self.at(&Tok::RParen) {
                        args.push(self.pattern()?);
                        while self.eat(&Tok::Comma) { args.push(self.pattern()?); }
                    }
                    self.expect(&Tok::RParen, "constructor pattern")?;
                    Ok(Pattern::Constructor(name, args, span))
                } else { Ok(Pattern::Ident(name, span)) }
            }
            Tok::Int(v) => { self.pos += 1; Ok(Pattern::Literal(Lit::Int(v), span)) }
            Tok::Float(v) => { self.pos += 1; Ok(Pattern::Literal(Lit::Float(v), span)) }
            Tok::Str(v) => { self.pos += 1; Ok(Pattern::Literal(Lit::Str(v), span)) }
            Tok::True => { self.pos += 1; Ok(Pattern::Literal(Lit::Bool(true), span)) }
            Tok::False => { self.pos += 1; Ok(Pattern::Literal(Lit::Bool(false), span)) }
            Tok::LParen => {
                self.pos += 1;
                let mut elems = Vec::new();
                if !self.at(&Tok::RParen) {
                    elems.push(self.pattern()?);
                    while self.eat(&Tok::Comma) { elems.push(self.pattern()?); }
                }
                self.expect(&Tok::RParen, "tuple pattern")?;
                Ok(Pattern::Tuple(elems, span))
            }
            _ => Err(self.err(&format!("expected pattern, got {:?}", self.peek()))),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Type expressions
// ═══════════════════════════════════════════════════════════════════════
impl Parser {
    fn type_expr(&mut self) -> Result<TypeExpr, FuseError> {
        let left = self.single_type()?;
        if self.at(&Tok::Pipe) {
            let mut types = vec![left];
            while self.eat(&Tok::Pipe) { types.push(self.single_type()?); }
            let span = types[0].span().clone();
            Ok(TypeExpr::Union(types, span))
        } else { Ok(left) }
    }

    fn single_type(&mut self) -> Result<TypeExpr, FuseError> {
        let span = self.span();
        if self.at(&Tok::LParen) {
            self.pos += 1;
            self.expect(&Tok::RParen, "unit type")?;
            return Ok(TypeExpr::Simple("Unit".into(), span));
        }
        // Allow integer literals as generic args (e.g. SIMD<Float32, 4>)
        if let Tok::Int(v) = self.peek().clone() {
            self.pos += 1;
            return Ok(TypeExpr::Simple(v.to_string(), span));
        }
        let name = self.expect_ident("type name")?;
        if self.eat(&Tok::Lt) {
            let mut args = vec![self.type_expr()?];
            while self.eat(&Tok::Comma) { args.push(self.type_expr()?); }
            self.expect(&Tok::Gt, "generic type")?;
            Ok(TypeExpr::Generic(name, args, span))
        } else {
            Ok(TypeExpr::Simple(name, span))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Utilities
// ═══════════════════════════════════════════════════════════════════════

impl TypeExpr {
    pub fn span(&self) -> &Span {
        match self {
            TypeExpr::Simple(_, s) | TypeExpr::Generic(_, _, s) | TypeExpr::Union(_, s) => s,
        }
    }
}

pub fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::IntLit(_, s) | Expr::FloatLit(_, s) | Expr::StrLit(_, s)
        | Expr::BoolLit(_, s) | Expr::Unit(s) | Expr::Ident(_, s)
        | Expr::SelfExpr(s) | Expr::FStr(_, s) | Expr::List(_, s)
        | Expr::Tuple(_, s) | Expr::Binary(_, _, _, s) | Expr::Unary(_, _, s)
        | Expr::Call(_, _, s) | Expr::Field(_, _, s) | Expr::OptChain(_, _, s)
        | Expr::Question(_, s) | Expr::Elvis(_, _, s) | Expr::Match(_, _, s)
        | Expr::When(_, s) | Expr::Lambda(_, _, s) | Expr::Move(_, s)
        | Expr::MutrefE(_, s) | Expr::RefE(_, s) | Expr::Block(_, s)
        | Expr::Spawn(_, _, s) | Expr::Await(_, s) | Expr::Path(_, _, s) => s.clone(),
    }
}
