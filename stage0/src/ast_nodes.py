"""Fuse Stage 0 — AST node definitions.

Dataclass definitions for every Fuse Core construct.
All nodes carry a Span for error reporting in later phases.
"""

from __future__ import annotations
from dataclasses import dataclass, field
from typing import Any


# =====================================================================
# Location
# =====================================================================

@dataclass
class Span:
    line: int
    col: int


# =====================================================================
# Type expressions
# =====================================================================

@dataclass
class SimpleType:
    name: str
    span: Span

@dataclass
class GenericType:
    name: str
    args: list[TypeExpr]
    span: Span

@dataclass
class UnionType:
    types: list[TypeExpr]
    span: Span

TypeExpr = SimpleType | GenericType | UnionType


# =====================================================================
# Patterns  (used in match arms)
# =====================================================================

@dataclass
class WildcardPattern:
    span: Span

@dataclass
class IdentPattern:
    """Variable binding or qualified enum name (e.g. 'v', 'None', 'Status.Ok')."""
    name: str
    span: Span

@dataclass
class LiteralPattern:
    value: Any          # int, float, str, bool
    span: Span

@dataclass
class ConstructorPattern:
    """E.g. Some(t), Err(AppError.NotFound(id)), Status.Warn(w)."""
    name: str           # possibly dotted: "AppError.NotFound"
    args: list[Pattern]
    span: Span

@dataclass
class TuplePattern:
    elements: list[Pattern]
    span: Span

Pattern = WildcardPattern | IdentPattern | LiteralPattern | ConstructorPattern | TuplePattern


# =====================================================================
# Expressions
# =====================================================================

@dataclass
class IntLiteral:
    value: int
    span: Span

@dataclass
class FloatLiteral:
    value: float
    span: Span

@dataclass
class StringLiteral:
    value: str
    span: Span

@dataclass
class BoolLiteral:
    value: bool
    span: Span

@dataclass
class UnitLiteral:
    """The () value."""
    span: Span

@dataclass
class NoneLiteral:
    span: Span

@dataclass
class Identifier:
    name: str
    span: Span

@dataclass
class SelfExpr:
    span: Span

@dataclass
class FStringExpr:
    """f"..." — parts is a list of StringLiteral | other Expr."""
    parts: list[Expr]
    span: Span

@dataclass
class ListLiteral:
    elements: list[Expr]
    span: Span

@dataclass
class TupleLiteral:
    elements: list[Expr]
    span: Span

@dataclass
class BinaryExpr:
    left: Expr
    op: str
    right: Expr
    span: Span

@dataclass
class UnaryExpr:
    op: str
    operand: Expr
    span: Span

@dataclass
class CallExpr:
    callee: Expr
    args: list[Expr]
    span: Span

@dataclass
class FieldAccessExpr:
    object: Expr
    field_name: str
    span: Span

@dataclass
class OptionalChainExpr:
    """expr?.field"""
    object: Expr
    field_name: str
    span: Span

@dataclass
class QuestionExpr:
    """expr? — unwrap Result/Option or return early."""
    expr: Expr
    span: Span

@dataclass
class ElvisExpr:
    """expr ?: fallback"""
    left: Expr
    right: Expr
    span: Span

@dataclass
class MatchArm:
    pattern: Pattern
    body: Expr
    span: Span

@dataclass
class MatchExpr:
    subject: Expr
    arms: list[MatchArm]
    span: Span

@dataclass
class WhenArm:
    condition: Expr | None      # None for the 'else' arm
    body: Expr
    span: Span

@dataclass
class WhenExpr:
    arms: list[WhenArm]
    span: Span

@dataclass
class LambdaExpr:
    """{ params => body }"""
    params: list[str]
    body: list[Stmt]
    span: Span

@dataclass
class MoveExpr:
    expr: Expr
    span: Span

@dataclass
class MutrefExpr:
    expr: Expr
    span: Span

@dataclass
class RefExpr:
    expr: Expr
    span: Span

@dataclass
class Block:
    """{ stmts } — the last ExprStmt (if any) is the block's value."""
    stmts: list[Stmt]
    span: Span

Expr = (
    IntLiteral | FloatLiteral | StringLiteral | BoolLiteral | UnitLiteral |
    NoneLiteral | Identifier | SelfExpr | FStringExpr | ListLiteral |
    TupleLiteral | BinaryExpr | UnaryExpr | CallExpr | FieldAccessExpr |
    OptionalChainExpr | QuestionExpr | ElvisExpr | MatchExpr | WhenExpr |
    LambdaExpr | MoveExpr | MutrefExpr | RefExpr | Block
)


# =====================================================================
# Statements
# =====================================================================

@dataclass
class ValDecl:
    name: str
    type_annotation: TypeExpr | None
    value: Expr
    span: Span

@dataclass
class VarDecl:
    name: str
    type_annotation: TypeExpr | None
    value: Expr
    span: Span

@dataclass
class AssignStmt:
    target: Expr
    value: Expr
    span: Span

@dataclass
class ExprStmt:
    expr: Expr
    span: Span

@dataclass
class ReturnStmt:
    value: Expr | None
    span: Span

@dataclass
class DeferStmt:
    expr: Expr
    span: Span

@dataclass
class IfStmt:
    condition: Expr
    then_body: Block
    else_body: Block | IfStmt | None
    span: Span

@dataclass
class ForStmt:
    var_name: str
    iterable: Expr
    body: Block
    span: Span

@dataclass
class LoopStmt:
    body: Block
    span: Span

Stmt = (
    ValDecl | VarDecl | AssignStmt | ExprStmt | ReturnStmt |
    DeferStmt | IfStmt | ForStmt | LoopStmt
)


# =====================================================================
# Declarations
# =====================================================================

@dataclass
class Annotation:
    name: str
    args: list[Expr]
    span: Span

@dataclass
class Param:
    convention: str | None      # "ref", "mutref", "owned", or None
    name: str
    type_expr: TypeExpr
    span: Span

@dataclass
class Field:
    mutable: bool               # val = False, var = True
    name: str
    type_expr: TypeExpr
    span: Span

@dataclass
class FnDecl:
    name: str
    extension_type: str | None  # "User" for fn User.greetUser(...)
    params: list[Param]
    return_type: TypeExpr | None
    body: Block | Expr          # Block for { ... }, Expr for => expr
    annotations: list[Annotation]
    is_expression_body: bool
    span: Span

@dataclass
class EnumVariant:
    name: str
    fields: list[TypeExpr]
    span: Span

@dataclass
class EnumDecl:
    name: str
    variants: list[EnumVariant]
    annotations: list[Annotation]
    span: Span

@dataclass
class StructDecl:
    name: str
    fields: list[Field]
    methods: list[FnDecl]
    annotations: list[Annotation]
    span: Span

@dataclass
class DataClassDecl:
    name: str
    params: list[Field]         # constructor parameters
    methods: list[FnDecl]
    annotations: list[Annotation]
    span: Span

Decl = FnDecl | EnumDecl | StructDecl | DataClassDecl


# =====================================================================
# Program
# =====================================================================

@dataclass
class Program:
    declarations: list[Decl | ValDecl | VarDecl]
    span: Span
