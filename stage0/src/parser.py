"""Fuse Stage 0 — Recursive descent parser.

Produces an AST from the token stream.
Covers all Fuse Core constructs.
"""

from __future__ import annotations
from fuse_token import Token, TokenType
from lexer import Lexer
from errors import ParseError
from ast_nodes import (
    Span, Program,
    # Types
    SimpleType, GenericType, UnionType, TypeExpr,
    # Patterns
    WildcardPattern, IdentPattern, LiteralPattern, ConstructorPattern,
    TuplePattern, Pattern,
    # Expressions
    IntLiteral, FloatLiteral, StringLiteral, BoolLiteral, UnitLiteral,
    NoneLiteral, Identifier, SelfExpr, FStringExpr, ListLiteral,
    TupleLiteral, BinaryExpr, UnaryExpr, CallExpr, FieldAccessExpr,
    OptionalChainExpr, QuestionExpr, ElvisExpr, MatchExpr, WhenExpr,
    LambdaExpr, MoveExpr, MutrefExpr, RefExpr, Block, Expr,
    # Statements
    ValDecl, VarDecl, AssignStmt, ExprStmt, ReturnStmt, DeferStmt,
    IfStmt, ForStmt, LoopStmt, Stmt,
    # Declarations
    Annotation, Param, Field, FnDecl, EnumVariant, EnumDecl,
    StructDecl, DataClassDecl, MatchArm, WhenArm,
)


class Parser:
    def __init__(self, tokens: list[Token], filename: str = "<stdin>"):
        self.tokens = tokens
        self.pos = 0
        self.filename = filename
        self._allow_trailing_brace = True

    # ==================================================================
    # Token helpers
    # ==================================================================

    def _span(self) -> Span:
        t = self.tokens[self.pos]
        return Span(t.line, t.col)

    def _peek(self) -> Token:
        return self.tokens[self.pos]

    def _at(self, *types: TokenType) -> bool:
        return self._peek().type in types

    def _advance(self) -> Token:
        tok = self.tokens[self.pos]
        self.pos += 1
        return tok

    def _expect(self, tt: TokenType, context: str = "") -> Token:
        tok = self._peek()
        if tok.type != tt:
            where = f" in {context}" if context else ""
            raise self._error(
                f"expected {tt.name}, got {tok.type.name} ({tok.value!r}){where}"
            )
        return self._advance()

    def _match(self, *types: TokenType) -> Token | None:
        if self._peek().type in types:
            return self._advance()
        return None

    def _error(self, msg: str, hint: str | None = None) -> ParseError:
        t = self._peek()
        return ParseError(msg, self.filename, t.line, t.col, hint)

    # ==================================================================
    # Program
    # ==================================================================

    def parse(self) -> Program:
        span = self._span()
        decls: list = []
        while not self._at(TokenType.EOF):
            decls.append(self._parse_declaration())
        return Program(decls, span)

    # ==================================================================
    # Annotations
    # ==================================================================

    def _parse_annotation(self) -> Annotation:
        span = self._span()
        self._expect(TokenType.AT)
        name = self._expect(TokenType.IDENT, "@annotation").value
        args: list[Expr] = []
        if self._match(TokenType.LPAREN):
            if not self._at(TokenType.RPAREN):
                args.append(self._parse_expr())
                while self._match(TokenType.COMMA):
                    args.append(self._parse_expr())
            self._expect(TokenType.RPAREN, "annotation args")
        return Annotation(name, args, span)

    # ==================================================================
    # Declarations
    # ==================================================================

    def _parse_declaration(self):
        annotations: list[Annotation] = []
        while self._at(TokenType.AT):
            annotations.append(self._parse_annotation())

        if self._at(TokenType.FN):
            return self._parse_fn_decl(annotations)
        if self._at(TokenType.ENUM):
            return self._parse_enum_decl(annotations)
        if self._at(TokenType.STRUCT):
            return self._parse_struct_decl(annotations)
        if self._at(TokenType.IDENT) and self._peek().value == "data":
            return self._parse_data_class_decl(annotations)
        if self._at(TokenType.VAL):
            return self._parse_top_val_decl(annotations)
        if self._at(TokenType.VAR):
            return self._parse_top_var_decl(annotations)
        raise self._error(
            f"expected declaration, got {self._peek().type.name}",
            hint="top level allows: fn, enum, struct, data class, val, var",
        )

    # --- fn ---------------------------------------------------------

    def _parse_fn_decl(self, annotations: list[Annotation]) -> FnDecl:
        span = self._span()
        self._expect(TokenType.FN)

        # Extension function: fn Type.method(...)
        name = self._expect(TokenType.IDENT, "function name").value
        extension_type: str | None = None
        if self._match(TokenType.DOT):
            extension_type = name
            name = self._expect(TokenType.IDENT, "method name").value

        # Parameters
        self._expect(TokenType.LPAREN, "function params")
        params: list[Param] = []
        if not self._at(TokenType.RPAREN):
            params.append(self._parse_param())
            while self._match(TokenType.COMMA):
                params.append(self._parse_param())
        self._expect(TokenType.RPAREN, "function params")

        # Return type
        return_type: TypeExpr | None = None
        if self._match(TokenType.ARROW):
            return_type = self._parse_type_expr()

        # Body: expression (=>) or block ({})
        is_expr_body = False
        if self._match(TokenType.FAT_ARROW):
            body = self._parse_expr()
            is_expr_body = True
        else:
            body = self._parse_block()

        return FnDecl(name, extension_type, params, return_type, body,
                       annotations, is_expr_body, span)

    def _parse_param(self) -> Param:
        span = self._span()
        convention: str | None = None
        if self._at(TokenType.REF, TokenType.MUTREF, TokenType.OWNED):
            convention = self._advance().value

        # Allow 'self' as a parameter name (extension methods, __del__)
        if self._at(TokenType.SELF):
            name = self._advance().value
        else:
            name = self._expect(TokenType.IDENT, "parameter name").value

        # Type annotation — optional for 'self' parameters
        type_expr: TypeExpr | None = None
        if self._match(TokenType.COLON):
            type_expr = self._parse_type_expr()
        elif name != "self":
            raise self._error("expected ':' after parameter name")
        if type_expr is None:
            type_expr = SimpleType("Self", span)

        return Param(convention, name, type_expr, span)

    # --- enum -------------------------------------------------------

    def _parse_enum_decl(self, annotations: list[Annotation]) -> EnumDecl:
        span = self._span()
        self._expect(TokenType.ENUM)
        name = self._expect(TokenType.IDENT, "enum name").value
        self._expect(TokenType.LBRACE, "enum body")
        variants: list[EnumVariant] = []
        while not self._at(TokenType.RBRACE):
            variants.append(self._parse_enum_variant())
            self._match(TokenType.COMMA)  # optional trailing comma
        self._expect(TokenType.RBRACE, "enum body")
        return EnumDecl(name, variants, annotations, span)

    def _parse_enum_variant(self) -> EnumVariant:
        span = self._span()
        name = self._expect(TokenType.IDENT, "enum variant").value
        fields: list[TypeExpr] = []
        if self._match(TokenType.LPAREN):
            if not self._at(TokenType.RPAREN):
                fields.append(self._parse_type_expr())
                while self._match(TokenType.COMMA):
                    fields.append(self._parse_type_expr())
            self._expect(TokenType.RPAREN, "variant fields")
        return EnumVariant(name, fields, span)

    # --- struct -----------------------------------------------------

    def _parse_struct_decl(self, annotations: list[Annotation]) -> StructDecl:
        span = self._span()
        self._expect(TokenType.STRUCT)
        name = self._expect(TokenType.IDENT, "struct name").value
        self._expect(TokenType.LBRACE, "struct body")
        fields: list[Field] = []
        methods: list[FnDecl] = []
        while not self._at(TokenType.RBRACE):
            if self._at(TokenType.FN):
                methods.append(self._parse_fn_decl([]))
            elif self._at(TokenType.VAL, TokenType.VAR):
                fields.append(self._parse_field())
            else:
                raise self._error(
                    f"expected field or method in struct, got {self._peek().type.name}"
                )
        self._expect(TokenType.RBRACE, "struct body")
        return StructDecl(name, fields, methods, annotations, span)

    def _parse_field(self) -> Field:
        span = self._span()
        mutable = self._advance().type == TokenType.VAR  # val or var
        name = self._expect(TokenType.IDENT, "field name").value
        self._expect(TokenType.COLON, "field type")
        type_expr = self._parse_type_expr()
        return Field(mutable, name, type_expr, span)

    # --- data class -------------------------------------------------

    def _parse_data_class_decl(self, annotations: list[Annotation]) -> DataClassDecl:
        span = self._span()
        self._advance()  # consume 'data' (IDENT)
        self._expect(TokenType.CLASS, "data class")
        name = self._expect(TokenType.IDENT, "data class name").value

        # Constructor parameters
        self._expect(TokenType.LPAREN, "data class params")
        params: list[Field] = []
        if not self._at(TokenType.RPAREN):
            params.append(self._parse_field())
            while self._match(TokenType.COMMA):
                params.append(self._parse_field())
        self._expect(TokenType.RPAREN, "data class params")

        # Optional body with methods
        methods: list[FnDecl] = []
        if self._at(TokenType.LBRACE):
            self._advance()
            while not self._at(TokenType.RBRACE):
                if self._at(TokenType.FN):
                    methods.append(self._parse_fn_decl([]))
                else:
                    raise self._error("expected method in data class body")
            self._expect(TokenType.RBRACE, "data class body")

        return DataClassDecl(name, params, methods, annotations, span)

    # --- top-level val / var ----------------------------------------

    def _parse_top_val_decl(self, annotations: list[Annotation]) -> ValDecl:
        decl = self._parse_val_decl()
        # Attach annotations for later use (e.g. @rank on Shared<T>)
        return decl

    def _parse_top_var_decl(self, annotations: list[Annotation]) -> VarDecl:
        return self._parse_var_decl()

    # ==================================================================
    # Blocks & statements
    # ==================================================================

    def _parse_block(self) -> Block:
        span = self._span()
        self._expect(TokenType.LBRACE, "block")
        stmts: list[Stmt] = []
        while not self._at(TokenType.RBRACE):
            stmts.append(self._parse_stmt())
        self._expect(TokenType.RBRACE, "block")
        return Block(stmts, span)

    def _parse_stmt(self) -> Stmt:
        if self._at(TokenType.VAL):
            return self._parse_val_decl()
        if self._at(TokenType.VAR):
            return self._parse_var_decl()
        if self._at(TokenType.RETURN):
            return self._parse_return_stmt()
        if self._at(TokenType.DEFER):
            return self._parse_defer_stmt()
        if self._at(TokenType.IF):
            return self._parse_if_stmt()
        if self._at(TokenType.FOR):
            return self._parse_for_stmt()
        if self._at(TokenType.LOOP):
            return self._parse_loop_stmt()

        # Expression statement or assignment
        span = self._span()
        expr = self._parse_expr()
        if self._match(TokenType.EQUALS):
            value = self._parse_expr()
            return AssignStmt(expr, value, span)
        return ExprStmt(expr, span)

    def _parse_val_decl(self) -> ValDecl:
        span = self._span()
        self._expect(TokenType.VAL)
        name = self._expect(TokenType.IDENT, "val name").value
        type_ann: TypeExpr | None = None
        if self._match(TokenType.COLON):
            type_ann = self._parse_type_expr()
        self._expect(TokenType.EQUALS, "val initializer")
        value = self._parse_expr()
        return ValDecl(name, type_ann, value, span)

    def _parse_var_decl(self) -> VarDecl:
        span = self._span()
        self._expect(TokenType.VAR)
        name = self._expect(TokenType.IDENT, "var name").value
        type_ann: TypeExpr | None = None
        if self._match(TokenType.COLON):
            type_ann = self._parse_type_expr()
        self._expect(TokenType.EQUALS, "var initializer")
        value = self._parse_expr()
        return VarDecl(name, type_ann, value, span)

    def _parse_return_stmt(self) -> ReturnStmt:
        span = self._span()
        self._expect(TokenType.RETURN)
        value: Expr | None = None
        # return has a value unless followed by } or EOF
        if not self._at(TokenType.RBRACE, TokenType.EOF):
            value = self._parse_expr()
        return ReturnStmt(value, span)

    def _parse_defer_stmt(self) -> DeferStmt:
        span = self._span()
        self._expect(TokenType.DEFER)
        expr = self._parse_expr()
        return DeferStmt(expr, span)

    def _parse_if_stmt(self) -> IfStmt:
        span = self._span()
        self._expect(TokenType.IF)

        saved = self._allow_trailing_brace
        self._allow_trailing_brace = False
        condition = self._parse_expr()
        self._allow_trailing_brace = saved

        then_body = self._parse_block()

        else_body: Block | IfStmt | None = None
        if self._match(TokenType.ELSE):
            if self._at(TokenType.IF):
                else_body = self._parse_if_stmt()
            else:
                else_body = self._parse_block()

        return IfStmt(condition, then_body, else_body, span)

    def _parse_for_stmt(self) -> ForStmt:
        span = self._span()
        self._expect(TokenType.FOR)
        var_name = self._expect(TokenType.IDENT, "for variable").value
        self._expect(TokenType.IN, "for loop")

        saved = self._allow_trailing_brace
        self._allow_trailing_brace = False
        iterable = self._parse_expr()
        self._allow_trailing_brace = saved

        body = self._parse_block()
        return ForStmt(var_name, iterable, body, span)

    def _parse_loop_stmt(self) -> LoopStmt:
        span = self._span()
        self._expect(TokenType.LOOP)
        body = self._parse_block()
        return LoopStmt(body, span)

    # ==================================================================
    # Expressions — precedence climbing
    # ==================================================================
    #
    # Precedence (low → high):
    #   elvis  ?:
    #   or
    #   and
    #   not          (prefix)
    #   comparison   == != < > <= >=
    #   addition     + -
    #   multiply     * / %
    #   unary        - not move ref mutref
    #   postfix      . ?. ? () {} (trailing lambda)
    #   primary      literals, identifiers, match, when, ...
    #

    def _parse_expr(self) -> Expr:
        return self._parse_elvis()

    def _parse_elvis(self) -> Expr:
        left = self._parse_or()
        if self._match(TokenType.ELVIS):
            right = self._parse_or()
            return ElvisExpr(left, right, left.span)
        return left

    def _parse_or(self) -> Expr:
        left = self._parse_and()
        while self._match(TokenType.OR):
            right = self._parse_and()
            left = BinaryExpr(left, "or", right, left.span)
        return left

    def _parse_and(self) -> Expr:
        left = self._parse_not()
        while self._match(TokenType.AND):
            right = self._parse_not()
            left = BinaryExpr(left, "and", right, left.span)
        return left

    def _parse_not(self) -> Expr:
        if self._at(TokenType.NOT):
            span = self._span()
            self._advance()
            operand = self._parse_not()
            return UnaryExpr("not", operand, span)
        return self._parse_comparison()

    _CMP_OPS = {
        TokenType.EQEQ: "==", TokenType.BANGEQ: "!=",
        TokenType.LT: "<", TokenType.GT: ">",
        TokenType.LTEQ: "<=", TokenType.GTEQ: ">=",
    }

    def _parse_comparison(self) -> Expr:
        left = self._parse_addition()
        if self._peek().type in self._CMP_OPS:
            op = self._CMP_OPS[self._advance().type]
            right = self._parse_addition()
            return BinaryExpr(left, op, right, left.span)
        return left

    def _parse_addition(self) -> Expr:
        left = self._parse_multiplication()
        while self._at(TokenType.PLUS, TokenType.MINUS):
            op = self._advance().value
            right = self._parse_multiplication()
            left = BinaryExpr(left, op, right, left.span)
        return left

    def _parse_multiplication(self) -> Expr:
        left = self._parse_unary()
        while self._at(TokenType.STAR, TokenType.SLASH, TokenType.PERCENT):
            op = self._advance().value
            right = self._parse_unary()
            left = BinaryExpr(left, op, right, left.span)
        return left

    def _parse_unary(self) -> Expr:
        if self._at(TokenType.MINUS):
            span = self._span()
            self._advance()
            operand = self._parse_unary()
            return UnaryExpr("-", operand, span)
        if self._at(TokenType.MOVE):
            span = self._span()
            self._advance()
            operand = self._parse_unary()
            return MoveExpr(operand, span)
        if self._at(TokenType.REF):
            span = self._span()
            self._advance()
            operand = self._parse_unary()
            return RefExpr(operand, span)
        if self._at(TokenType.MUTREF):
            span = self._span()
            self._advance()
            operand = self._parse_unary()
            return MutrefExpr(operand, span)
        return self._parse_postfix()

    def _parse_postfix(self) -> Expr:
        expr = self._parse_primary()
        while True:
            if self._match(TokenType.DOT):
                name = self._expect(TokenType.IDENT, "field name").value
                expr = FieldAccessExpr(expr, name, expr.span)

            elif self._match(TokenType.QUESTION_DOT):
                name = self._expect(TokenType.IDENT, "optional chain field").value
                expr = OptionalChainExpr(expr, name, expr.span)

            elif self._at(TokenType.QUESTION):
                # Distinguish ? (postfix) from ?. and ?: which are already
                # handled by the lexer as separate token types.
                self._advance()
                expr = QuestionExpr(expr, expr.span)

            elif self._at(TokenType.LPAREN):
                expr = self._parse_call(expr)

            elif (self._at(TokenType.LBRACE)
                  and self._allow_trailing_brace
                  and self._is_lambda()):
                # Trailing lambda: expr { params => body }
                lam = self._parse_lambda()
                expr = CallExpr(expr, [lam], expr.span)

            else:
                break
        return expr

    def _parse_call(self, callee: Expr) -> CallExpr:
        self._expect(TokenType.LPAREN)
        args: list[Expr] = []
        if not self._at(TokenType.RPAREN):
            args.append(self._parse_expr())
            while self._match(TokenType.COMMA):
                args.append(self._parse_expr())
        self._expect(TokenType.RPAREN, "call args")

        # Optional trailing lambda: foo(args) { ... }
        if (self._at(TokenType.LBRACE)
                and self._allow_trailing_brace
                and self._is_lambda()):
            args.append(self._parse_lambda())

        return CallExpr(callee, args, callee.span)

    # ------------------------------------------------------------------
    # Primary expressions
    # ------------------------------------------------------------------

    def _parse_primary(self) -> Expr:
        span = self._span()

        # Literals
        if self._at(TokenType.INT):
            return IntLiteral(self._advance().value, span)
        if self._at(TokenType.FLOAT):
            return FloatLiteral(self._advance().value, span)
        if self._at(TokenType.STRING):
            return StringLiteral(self._advance().value, span)
        if self._at(TokenType.TRUE):
            self._advance()
            return BoolLiteral(True, span)
        if self._at(TokenType.FALSE):
            self._advance()
            return BoolLiteral(False, span)

        # F-string
        if self._at(TokenType.FSTRING):
            return self._parse_fstring_expr(self._advance())

        # self
        if self._at(TokenType.SELF):
            self._advance()
            return SelfExpr(span)

        # Identifier
        if self._at(TokenType.IDENT):
            return Identifier(self._advance().value, span)

        # Parenthesised expression, unit, or tuple
        if self._at(TokenType.LPAREN):
            self._advance()
            if self._at(TokenType.RPAREN):
                self._advance()
                return UnitLiteral(span)
            first = self._parse_expr()
            if self._match(TokenType.COMMA):
                elements = [first]
                if not self._at(TokenType.RPAREN):
                    elements.append(self._parse_expr())
                    while self._match(TokenType.COMMA):
                        elements.append(self._parse_expr())
                self._expect(TokenType.RPAREN, "tuple")
                return TupleLiteral(elements, span)
            self._expect(TokenType.RPAREN, "parenthesised expression")
            return first

        # List literal
        if self._at(TokenType.LBRACKET):
            return self._parse_list_literal()

        # match
        if self._at(TokenType.MATCH):
            return self._parse_match_expr()

        # when
        if self._at(TokenType.WHEN):
            return self._parse_when_expr()

        # Block as expression
        if self._at(TokenType.LBRACE):
            return self._parse_block()

        raise self._error(
            f"expected expression, got {self._peek().type.name} ({self._peek().value!r})"
        )

    # ------------------------------------------------------------------
    # Compound expression forms
    # ------------------------------------------------------------------

    def _parse_list_literal(self) -> ListLiteral:
        span = self._span()
        self._expect(TokenType.LBRACKET)
        elements: list[Expr] = []
        if not self._at(TokenType.RBRACKET):
            elements.append(self._parse_expr())
            while self._match(TokenType.COMMA):
                if self._at(TokenType.RBRACKET):
                    break  # trailing comma
                elements.append(self._parse_expr())
        self._expect(TokenType.RBRACKET, "list literal")
        return ListLiteral(elements, span)

    def _parse_match_expr(self) -> MatchExpr:
        span = self._span()
        self._expect(TokenType.MATCH)

        saved = self._allow_trailing_brace
        self._allow_trailing_brace = False
        subject = self._parse_expr()
        self._allow_trailing_brace = saved

        self._expect(TokenType.LBRACE, "match body")
        arms: list[MatchArm] = []
        while not self._at(TokenType.RBRACE):
            arms.append(self._parse_match_arm())
        self._expect(TokenType.RBRACE, "match body")
        return MatchExpr(subject, arms, span)

    def _parse_match_arm(self) -> MatchArm:
        span = self._span()
        pattern = self._parse_pattern()
        self._expect(TokenType.FAT_ARROW, "match arm")
        body = self._parse_expr()
        return MatchArm(pattern, body, span)

    def _parse_when_expr(self) -> WhenExpr:
        span = self._span()
        self._expect(TokenType.WHEN)
        self._expect(TokenType.LBRACE, "when body")
        arms: list[WhenArm] = []
        while not self._at(TokenType.RBRACE):
            arms.append(self._parse_when_arm())
        self._expect(TokenType.RBRACE, "when body")
        return WhenExpr(arms, span)

    def _parse_when_arm(self) -> WhenArm:
        span = self._span()
        if self._match(TokenType.ELSE):
            self._expect(TokenType.FAT_ARROW, "when else arm")
            body = self._parse_expr()
            return WhenArm(None, body, span)

        saved = self._allow_trailing_brace
        self._allow_trailing_brace = False
        condition = self._parse_expr()
        self._allow_trailing_brace = saved

        self._expect(TokenType.FAT_ARROW, "when arm")
        body = self._parse_expr()
        return WhenArm(condition, body, span)

    def _parse_fstring_expr(self, tok: Token) -> FStringExpr:
        span = Span(tok.line, tok.col)
        parts: list[Expr] = []
        for kind, text in tok.value:
            if kind == "str":
                parts.append(StringLiteral(text, span))
            elif kind == "expr":
                sub_lexer = Lexer(text, self.filename)
                sub_tokens = sub_lexer.tokenize()
                sub_parser = Parser(sub_tokens, self.filename)
                parts.append(sub_parser._parse_expr())
        return FStringExpr(parts, span)

    # ------------------------------------------------------------------
    # Lambda
    # ------------------------------------------------------------------

    def _is_lambda(self) -> bool:
        """Lookahead: does { start a lambda (ident, ... => body)?"""
        saved = self.pos
        try:
            if self.tokens[self.pos].type != TokenType.LBRACE:
                return False
            self.pos += 1
            if self.pos >= len(self.tokens):
                return False
            if self.tokens[self.pos].type != TokenType.IDENT:
                return False
            self.pos += 1
            while (self.pos < len(self.tokens)
                   and self.tokens[self.pos].type == TokenType.COMMA):
                self.pos += 1
                if (self.pos >= len(self.tokens)
                        or self.tokens[self.pos].type != TokenType.IDENT):
                    return False
                self.pos += 1
            return (self.pos < len(self.tokens)
                    and self.tokens[self.pos].type == TokenType.FAT_ARROW)
        finally:
            self.pos = saved

    def _parse_lambda(self) -> LambdaExpr:
        span = self._span()
        self._expect(TokenType.LBRACE)
        params: list[str] = []
        params.append(self._expect(TokenType.IDENT, "lambda param").value)
        while self._match(TokenType.COMMA):
            params.append(self._expect(TokenType.IDENT, "lambda param").value)
        self._expect(TokenType.FAT_ARROW, "lambda =>")

        body: list[Stmt] = []
        while not self._at(TokenType.RBRACE):
            body.append(self._parse_stmt())
        self._expect(TokenType.RBRACE, "lambda body")
        return LambdaExpr(params, body, span)

    # ==================================================================
    # Patterns
    # ==================================================================

    def _parse_pattern(self) -> Pattern:
        span = self._span()

        # Wildcard
        if self._at(TokenType.IDENT) and self._peek().value == "_":
            self._advance()
            return WildcardPattern(span)

        # Tuple pattern
        if self._at(TokenType.LPAREN):
            self._advance()
            elements: list[Pattern] = []
            if not self._at(TokenType.RPAREN):
                elements.append(self._parse_pattern())
                while self._match(TokenType.COMMA):
                    elements.append(self._parse_pattern())
            self._expect(TokenType.RPAREN, "tuple pattern")
            return TuplePattern(elements, span)

        # Int literal
        if self._at(TokenType.INT):
            return LiteralPattern(self._advance().value, span)
        # Float literal
        if self._at(TokenType.FLOAT):
            return LiteralPattern(self._advance().value, span)
        # String literal
        if self._at(TokenType.STRING):
            return LiteralPattern(self._advance().value, span)
        # Bool literal
        if self._at(TokenType.TRUE):
            self._advance()
            return LiteralPattern(True, span)
        if self._at(TokenType.FALSE):
            self._advance()
            return LiteralPattern(False, span)

        # Identifier or constructor pattern (possibly dotted)
        if self._at(TokenType.IDENT):
            name = self._advance().value
            while self._match(TokenType.DOT):
                name += "." + self._expect(TokenType.IDENT, "pattern name").value
            if self._match(TokenType.LPAREN):
                args: list[Pattern] = []
                if not self._at(TokenType.RPAREN):
                    args.append(self._parse_pattern())
                    while self._match(TokenType.COMMA):
                        args.append(self._parse_pattern())
                self._expect(TokenType.RPAREN, "constructor pattern")
                return ConstructorPattern(name, args, span)
            return IdentPattern(name, span)

        raise self._error(f"expected pattern, got {self._peek().type.name}")

    # ==================================================================
    # Type expressions
    # ==================================================================

    def _parse_type_expr(self) -> TypeExpr:
        left = self._parse_single_type()
        if self._at(TokenType.PIPE):
            types = [left]
            while self._match(TokenType.PIPE):
                types.append(self._parse_single_type())
            return UnionType(types, left.span)
        return left

    def _parse_single_type(self) -> TypeExpr:
        span = self._span()

        # Unit type ()
        if self._at(TokenType.LPAREN):
            self._advance()
            self._expect(TokenType.RPAREN, "unit type")
            return SimpleType("Unit", span)

        name = self._expect(TokenType.IDENT, "type name").value

        # Generic type: Name<T, U, ...>
        if self._match(TokenType.LT):
            args = [self._parse_type_expr()]
            while self._match(TokenType.COMMA):
                args.append(self._parse_type_expr())
            self._expect(TokenType.GT, "generic type >")
            return GenericType(name, args, span)

        return SimpleType(name, span)


# ======================================================================
# Standalone entry point
# ======================================================================

if __name__ == "__main__":
    import sys

    if len(sys.argv) < 2:
        print("Usage: python parser.py <file.fuse>")
        sys.exit(1)

    with open(sys.argv[1]) as f:
        source = f.read()

    lexer = Lexer(source, sys.argv[1])
    tokens = lexer.tokenize()
    parser = Parser(tokens, sys.argv[1])
    program = parser.parse()

    for decl in program.declarations:
        print(decl)
        print()
