"""Fuse Stage 0 — Ownership checker and type verifier.

Walks the AST and rejects invalid programs before evaluation.

Enforces:
  - val immutability (no reassignment to val bindings)
  - use-after-move detection (move transfers ownership)
  - ref parameters cannot be assigned through
  - match exhaustiveness (all enum variants must be covered)
"""

from __future__ import annotations
from dataclasses import dataclass, field

from ast_nodes import (
    Span, Program,
    SimpleType, GenericType,
    WildcardPattern, IdentPattern, LiteralPattern, ConstructorPattern,
    TuplePattern,
    IntLiteral, FloatLiteral, StringLiteral, BoolLiteral, UnitLiteral,
    NoneLiteral, Identifier, SelfExpr, FStringExpr, ListLiteral,
    TupleLiteral, BinaryExpr, UnaryExpr, CallExpr, FieldAccessExpr,
    OptionalChainExpr, QuestionExpr, ElvisExpr, MatchExpr, WhenExpr,
    LambdaExpr, MoveExpr, MutrefExpr, RefExpr, Block,
    ValDecl, VarDecl, AssignStmt, ExprStmt, ReturnStmt, DeferStmt,
    IfStmt, ForStmt, LoopStmt,
    Annotation, Param, Field, FnDecl, EnumVariant, EnumDecl,
    StructDecl, DataClassDecl, MatchArm, WhenArm,
)
from errors import CheckError


# =====================================================================
# Binding tracker
# =====================================================================

@dataclass
class BindingInfo:
    is_mutable: bool            # var=True, val/param=False
    convention: str | None = None   # "ref", "mutref", "owned", or None
    moved: bool = False
    moved_line: int = 0


# =====================================================================
# Checker
# =====================================================================

class Checker:
    def __init__(self, program: Program, filename: str):
        self.program = program
        self.filename = filename
        self.errors: list[CheckError] = []

        # Type registry — populated before checking
        self.enums: dict[str, EnumDecl] = {}

        # Scope stack: each scope is {name: BindingInfo}
        self.scopes: list[dict[str, BindingInfo]] = []

        self._register_builtins()
        self._collect_types()

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def check(self) -> list[CheckError]:
        for decl in self.program.declarations:
            self._check_decl(decl)
        return self.errors

    # ------------------------------------------------------------------
    # Setup
    # ------------------------------------------------------------------

    def _register_builtins(self):
        """Register built-in enum types (Option, Result)."""
        s = Span(0, 0)
        self.enums["Option"] = EnumDecl("Option", [
            EnumVariant("Some", [SimpleType("T", s)], s),
            EnumVariant("None", [], s),
        ], [], s)
        self.enums["Result"] = EnumDecl("Result", [
            EnumVariant("Ok", [SimpleType("T", s)], s),
            EnumVariant("Err", [SimpleType("E", s)], s),
        ], [], s)

    def _collect_types(self):
        """First pass — collect all user-defined enum declarations."""
        for decl in self.program.declarations:
            if isinstance(decl, EnumDecl):
                self.enums[decl.name] = decl

    # ------------------------------------------------------------------
    # Scope helpers
    # ------------------------------------------------------------------

    def _push_scope(self):
        self.scopes.append({})

    def _pop_scope(self):
        self.scopes.pop()

    def _define(self, name: str, info: BindingInfo):
        self.scopes[-1][name] = info

    def _lookup(self, name: str) -> BindingInfo | None:
        for scope in reversed(self.scopes):
            if name in scope:
                return scope[name]
        return None

    # ------------------------------------------------------------------
    # Error reporting
    # ------------------------------------------------------------------

    def _report(self, message: str, span: Span, hint: str | None = None):
        self.errors.append(CheckError(
            message=message,
            filename=self.filename,
            line=span.line,
            col=span.col,
            hint=hint,
        ))

    # ------------------------------------------------------------------
    # Declaration checking
    # ------------------------------------------------------------------

    def _check_decl(self, decl):
        if isinstance(decl, FnDecl):
            self._check_fn(decl)
        elif isinstance(decl, StructDecl):
            for method in decl.methods:
                self._check_fn(method)
        elif isinstance(decl, DataClassDecl):
            for method in decl.methods:
                self._check_fn(method)
        # EnumDecl, top-level ValDecl/VarDecl — nothing to check yet

    def _check_fn(self, fn: FnDecl):
        self._push_scope()
        for param in fn.params:
            self._define(param.name, BindingInfo(
                is_mutable=(param.convention == "mutref"),
                convention=param.convention,
            ))
        if isinstance(fn.body, Block):
            self._check_block(fn.body)
        else:
            self._check_expr(fn.body)
        self._pop_scope()

    # ------------------------------------------------------------------
    # Block & statement checking
    # ------------------------------------------------------------------

    def _check_block(self, block: Block):
        for stmt in block.stmts:
            self._check_stmt(stmt)

    def _check_stmt(self, stmt):
        if isinstance(stmt, ValDecl):
            self._check_expr(stmt.value)
            self._define(stmt.name, BindingInfo(is_mutable=False))

        elif isinstance(stmt, VarDecl):
            self._check_expr(stmt.value)
            self._define(stmt.name, BindingInfo(is_mutable=True))

        elif isinstance(stmt, AssignStmt):
            self._check_assignment(stmt)

        elif isinstance(stmt, ExprStmt):
            self._check_expr(stmt.expr)

        elif isinstance(stmt, ReturnStmt):
            if stmt.value:
                self._check_expr(stmt.value)

        elif isinstance(stmt, DeferStmt):
            self._check_expr(stmt.expr)

        elif isinstance(stmt, IfStmt):
            self._check_if(stmt)

        elif isinstance(stmt, ForStmt):
            self._check_expr(stmt.iterable)
            self._push_scope()
            self._define(stmt.var_name, BindingInfo(is_mutable=False))
            self._check_block(stmt.body)
            self._pop_scope()

        elif isinstance(stmt, LoopStmt):
            self._push_scope()
            self._check_block(stmt.body)
            self._pop_scope()

    def _check_assignment(self, stmt: AssignStmt):
        self._check_expr(stmt.value)

        if isinstance(stmt.target, Identifier):
            name = stmt.target.name
            info = self._lookup(name)
            if info is None:
                return
            if not info.is_mutable:
                if info.convention == "ref":
                    self._report(
                        f"cannot assign through `ref` parameter `{name}`",
                        stmt.target.span,
                    )
                else:
                    self._report(
                        f"cannot reassign to `{name}` \u2014 declared as `val`",
                        stmt.target.span,
                        hint="use `var` if reassignment is intended",
                    )
        else:
            # Field assignment (e.g., obj.field = val) — check the object
            self._check_expr(stmt.target)

    def _check_if(self, stmt: IfStmt):
        self._check_expr(stmt.condition)
        self._push_scope()
        self._check_block(stmt.then_body)
        self._pop_scope()
        if stmt.else_body is not None:
            if isinstance(stmt.else_body, IfStmt):
                self._check_if(stmt.else_body)
            else:
                self._push_scope()
                self._check_block(stmt.else_body)
                self._pop_scope()

    # ------------------------------------------------------------------
    # Expression checking
    # ------------------------------------------------------------------

    def _check_expr(self, expr):
        if expr is None:
            return

        if isinstance(expr, Identifier):
            self._check_use(expr)

        elif isinstance(expr, MoveExpr):
            self._check_move(expr)

        elif isinstance(expr, RefExpr):
            self._check_expr(expr.expr)
        elif isinstance(expr, MutrefExpr):
            self._check_expr(expr.expr)

        elif isinstance(expr, BinaryExpr):
            self._check_expr(expr.left)
            self._check_expr(expr.right)
        elif isinstance(expr, UnaryExpr):
            self._check_expr(expr.operand)

        elif isinstance(expr, CallExpr):
            self._check_expr(expr.callee)
            for arg in expr.args:
                self._check_expr(arg)

        elif isinstance(expr, FieldAccessExpr):
            self._check_expr(expr.object)
        elif isinstance(expr, OptionalChainExpr):
            self._check_expr(expr.object)
        elif isinstance(expr, QuestionExpr):
            self._check_expr(expr.expr)
        elif isinstance(expr, ElvisExpr):
            self._check_expr(expr.left)
            self._check_expr(expr.right)

        elif isinstance(expr, MatchExpr):
            self._check_match(expr)
        elif isinstance(expr, WhenExpr):
            self._check_when(expr)

        elif isinstance(expr, FStringExpr):
            for part in expr.parts:
                self._check_expr(part)
        elif isinstance(expr, ListLiteral):
            for elem in expr.elements:
                self._check_expr(elem)
        elif isinstance(expr, TupleLiteral):
            for elem in expr.elements:
                self._check_expr(elem)

        elif isinstance(expr, LambdaExpr):
            self._push_scope()
            for p in expr.params:
                self._define(p, BindingInfo(is_mutable=False))
            for stmt in expr.body:
                self._check_stmt(stmt)
            self._pop_scope()

        elif isinstance(expr, Block):
            self._push_scope()
            self._check_block(expr)
            self._pop_scope()

        # Literals (Int, Float, String, Bool, Unit, None, Self) — nothing to check

    # ------------------------------------------------------------------
    # Ownership: move & use-after-move
    # ------------------------------------------------------------------

    def _check_move(self, expr: MoveExpr):
        self._check_expr(expr.expr)
        if isinstance(expr.expr, Identifier):
            info = self._lookup(expr.expr.name)
            if info is not None:
                info.moved = True
                info.moved_line = expr.span.line

    def _check_use(self, expr: Identifier):
        info = self._lookup(expr.name)
        if info is not None and info.moved:
            self._report(
                f"cannot use `{expr.name}` after `move`",
                expr.span,
                hint=f"ownership was transferred on line {info.moved_line}",
            )

    # ------------------------------------------------------------------
    # Match exhaustiveness
    # ------------------------------------------------------------------

    def _check_match(self, expr: MatchExpr):
        self._check_expr(expr.subject)
        for arm in expr.arms:
            self._check_expr(arm.body)
        self._check_exhaustiveness(expr)

    def _check_when(self, expr: WhenExpr):
        for arm in expr.arms:
            if arm.condition is not None:
                self._check_expr(arm.condition)
            self._check_expr(arm.body)

    def _check_exhaustiveness(self, match_expr: MatchExpr):
        # Determine which enum (if any) is being matched
        enum_name = self._infer_enum_from_arms(match_expr.arms)
        if enum_name is None:
            return  # cannot determine — skip

        enum_decl = self.enums.get(enum_name)
        if enum_decl is None:
            return  # unknown enum — skip

        all_variants = {v.name for v in enum_decl.variants}
        covered: set[str] = set()
        has_wildcard = False

        for arm in match_expr.arms:
            if isinstance(arm.pattern, WildcardPattern):
                has_wildcard = True
            else:
                covered |= self._covered_variants(arm.pattern, enum_name)

        if has_wildcard:
            return

        missing = all_variants - covered
        if missing:
            formatted = ", ".join(
                f"`{self._format_variant(enum_decl, v)}`"
                for v in sorted(missing)
            )
            self._report(
                "match is not exhaustive",
                match_expr.span,
                hint=f"missing case: {formatted}",
            )

    def _infer_enum_from_arms(self, arms: list[MatchArm]) -> str | None:
        """Try to determine the enum type from the patterns in the arms."""
        for arm in arms:
            name = self._enum_from_pattern(arm.pattern)
            if name is not None:
                return name
        return None

    def _enum_from_pattern(self, pat) -> str | None:
        """Extract the enum name from a single pattern."""
        if isinstance(pat, ConstructorPattern):
            if "." in pat.name:
                return pat.name.split(".")[0]
            if pat.name in ("Some",):
                return "Option"
            if pat.name in ("Ok", "Err"):
                return "Result"
        elif isinstance(pat, IdentPattern):
            if "." in pat.name:
                return pat.name.split(".")[0]
            if pat.name == "None":
                return "Option"
        return None

    def _covered_variants(self, pat, enum_name: str) -> set[str]:
        """Return the set of variant names covered by this pattern."""
        vs: set[str] = set()
        if isinstance(pat, ConstructorPattern):
            if "." in pat.name:
                prefix, variant = pat.name.rsplit(".", 1)
                if prefix == enum_name:
                    vs.add(variant)
            elif enum_name in ("Option", "Result"):
                vs.add(pat.name)        # Some, Ok, Err
        elif isinstance(pat, IdentPattern):
            if "." in pat.name:
                prefix, variant = pat.name.rsplit(".", 1)
                if prefix == enum_name:
                    vs.add(variant)
            elif pat.name == "None" and enum_name == "Option":
                vs.add("None")
            elif pat.name in ("Ok", "Err") and enum_name == "Result":
                vs.add(pat.name)
        elif isinstance(pat, WildcardPattern):
            pass    # handled by caller
        elif isinstance(pat, LiteralPattern):
            pass    # literals don't name enum variants
        return vs

    def _format_variant(self, enum_decl: EnumDecl, variant_name: str) -> str:
        for v in enum_decl.variants:
            if v.name == variant_name:
                if v.fields:
                    types = ", ".join(
                        t.name if isinstance(t, SimpleType) else str(t)
                        for t in v.fields
                    )
                    return f"{variant_name}({types})"
                return variant_name
        return variant_name
