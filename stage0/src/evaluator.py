"""Fuse Stage 0 — Tree-walking evaluator.

Executes AST nodes produced by the parser.  Handles all Fuse Core
constructs including pattern matching, ?, ?., ?:, defer, ASAP
destruction, and extension function dispatch.
"""

from __future__ import annotations
import sys
from typing import Any

from ast_nodes import (
    Span, Program, Block,
    SimpleType, GenericType,
    WildcardPattern, IdentPattern, LiteralPattern, ConstructorPattern,
    TuplePattern,
    IntLiteral, FloatLiteral, StringLiteral, BoolLiteral, UnitLiteral,
    NoneLiteral, Identifier, SelfExpr, FStringExpr, ListLiteral,
    TupleLiteral, BinaryExpr, UnaryExpr, CallExpr, FieldAccessExpr,
    OptionalChainExpr, QuestionExpr, ElvisExpr, MatchExpr, WhenExpr,
    LambdaExpr, MoveExpr, MutrefExpr, RefExpr,
    ValDecl, VarDecl, AssignStmt, ExprStmt, ReturnStmt, DeferStmt,
    IfStmt, ForStmt, LoopStmt,
    Annotation, Param, FnDecl, EnumDecl, StructDecl, DataClassDecl,
    MatchArm, WhenArm,
)
from values import (
    FUSE_UNIT, FuseEnumVariant, FuseEnumType, FuseStruct, FuseList,
    FuseFunction, LambdaValue, BuiltinFunction, BuiltinConstructor,
    StructConstructor, DataClassConstructor, EarlyReturn, format_value,
)
from environment import Environment
from errors import EvalError


class Evaluator:
    def __init__(self, program: Program, filename: str):
        self.program = program
        self.filename = filename
        self.global_env = Environment()

        # Type registry
        self.structs: dict[str, StructDecl] = {}
        self.data_classes: dict[str, DataClassDecl] = {}
        self.enums: dict[str, EnumDecl] = {}
        self.extension_fns: dict[tuple[str, str], FnDecl] = {}

        # Defer stack (active defers for the current function)
        self._defers: list[tuple] = []

        self._register_builtins()
        self._register_declarations()

    # ==================================================================
    # Setup
    # ==================================================================

    def _register_builtins(self):
        e = self.global_env
        e.define("println", BuiltinFunction("println"))
        e.define("eprintln", BuiltinFunction("eprintln"))
        e.define("Some", BuiltinConstructor("Option", "Some"))
        e.define("None", FuseEnumVariant("Option", "None"))
        e.define("Ok", BuiltinConstructor("Result", "Ok"))
        e.define("Err", BuiltinConstructor("Result", "Err"))

    def _register_declarations(self):
        for decl in self.program.declarations:
            if isinstance(decl, StructDecl):
                self.structs[decl.name] = decl
                self.global_env.define(decl.name, StructConstructor(decl))
            elif isinstance(decl, DataClassDecl):
                self.data_classes[decl.name] = decl
                self.global_env.define(decl.name, DataClassConstructor(decl))
            elif isinstance(decl, EnumDecl):
                self.enums[decl.name] = decl
                info = {v.name: len(v.fields) for v in decl.variants}
                self.global_env.define(decl.name, FuseEnumType(decl.name, info))
            elif isinstance(decl, FnDecl):
                if decl.extension_type:
                    self.extension_fns[(decl.extension_type, decl.name)] = decl
                else:
                    self.global_env.define(decl.name, FuseFunction(decl))

    # ==================================================================
    # Public API
    # ==================================================================

    def run(self):
        """Find @entrypoint and execute it."""
        for decl in self.program.declarations:
            if isinstance(decl, FnDecl) and not decl.extension_type:
                if any(a.name == "entrypoint" for a in decl.annotations):
                    result = self._call_user_fn(decl, [])
                    # If main returns Err, print it
                    if (isinstance(result, FuseEnumVariant)
                            and result.enum_name == "Result"
                            and result.variant_name == "Err"):
                        print(f"Error: {format_value(result.value)}", file=sys.stderr)
                        sys.exit(1)
                    return
        raise EvalError("no @entrypoint function found", self.filename, 0, 0)

    # ==================================================================
    # Function calls
    # ==================================================================

    def _call_user_fn(self, decl: FnDecl, args: list,
                      self_value=None) -> Any:
        fn_env = Environment(parent=self.global_env)
        params = decl.params
        arg_idx = 0
        for param in params:
            if param.name == "self" and self_value is not None:
                fn_env.define("self", self_value)
            else:
                fn_env.define(param.name, args[arg_idx] if arg_idx < len(args) else FUSE_UNIT)
                arg_idx += 1

        saved_defers = self._defers
        self._defers = []
        try:
            if isinstance(decl.body, Block):
                result = self._eval_block(decl.body, fn_env, asap=True)
            else:
                result = self._eval_expr(decl.body, fn_env)
        except EarlyReturn as r:
            result = r.value
        finally:
            for defer_expr, defer_env in reversed(self._defers):
                try:
                    self._eval_expr(defer_expr, defer_env)
                except EarlyReturn:
                    pass
            self._defers = saved_defers
        return result

    def _call_builtin(self, name: str, args: list) -> Any:
        if name == "println":
            print(format_value(args[0]) if args else "")
            return FUSE_UNIT
        if name == "eprintln":
            print(format_value(args[0]) if args else "", file=sys.stderr)
            return FUSE_UNIT
        raise EvalError(f"unknown builtin: {name}", self.filename, 0, 0)

    def _call_method(self, obj: Any, method: str, args: list,
                     env: Environment) -> Any:
        type_name = self._type_name(obj)

        # Extension functions
        key = (type_name, method)
        if key in self.extension_fns:
            return self._call_user_fn(self.extension_fns[key], args,
                                      self_value=obj)

        # Struct __del__ and user methods
        if isinstance(obj, FuseStruct):
            decl = self.structs.get(obj.type_name) or self.data_classes.get(obj.type_name)
            if decl:
                for m in (decl.methods if hasattr(decl, 'methods') else []):
                    if m.name == method:
                        return self._call_user_fn(m, args, self_value=obj)

        # Built-in methods by type
        if isinstance(obj, FuseList):
            return self._list_method(obj, method, args, env)
        if isinstance(obj, str):
            return self._string_method(obj, method, args)
        if isinstance(obj, bool):
            pass  # bool before int
        if isinstance(obj, int):
            return self._int_method(obj, method, args)
        if isinstance(obj, float):
            return self._float_method(obj, method, args)

        raise EvalError(f"no method '{method}' on {type_name}",
                        self.filename, 0, 0)

    # ------------------------------------------------------------------
    # Built-in methods
    # ------------------------------------------------------------------

    def _list_method(self, lst: FuseList, method: str, args: list,
                     env: Environment) -> Any:
        if method == "retainWhere":
            pred = args[0]
            lst.elements = [e for e in lst.elements
                            if self._call_lambda(pred, [e])]
            return FUSE_UNIT
        if method == "map":
            fn = args[0]
            return FuseList([self._call_lambda(fn, [e]) for e in lst.elements])
        if method == "filter":
            fn = args[0]
            return FuseList([e for e in lst.elements
                             if self._call_lambda(fn, [e])])
        if method == "sorted":
            return FuseList(sorted(lst.elements))
        if method == "first":
            if lst.elements:
                return FuseEnumVariant("Option", "Some", lst.elements[0])
            return FuseEnumVariant("Option", "None")
        if method == "last":
            if lst.elements:
                return lst.elements[-1]
            return FuseEnumVariant("Option", "None")
        if method == "len":
            return len(lst.elements)
        if method == "isEmpty":
            return len(lst.elements) == 0
        if method == "sum":
            return sum(lst.elements)
        if method == "push":
            lst.elements.append(args[0])
            return FUSE_UNIT
        raise EvalError(f"unknown list method: {method}", self.filename, 0, 0)

    def _string_method(self, s: str, method: str, args: list) -> Any:
        if method == "toUpper":
            return s.upper()
        if method == "toLower":
            return s.lower()
        if method == "len":
            return len(s)
        raise EvalError(f"unknown string method: {method}", self.filename, 0, 0)

    def _int_method(self, n: int, method: str, args: list) -> Any:
        if method == "toFloat":
            return float(n)
        if method == "isEven":
            return n % 2 == 0
        if method == "toString":
            return str(n)
        raise EvalError(f"unknown int method: {method}", self.filename, 0, 0)

    def _float_method(self, f: float, method: str, args: list) -> Any:
        if method == "toString":
            return str(f)
        if method == "toInt":
            return int(f)
        raise EvalError(f"unknown float method: {method}", self.filename, 0, 0)

    def _call_lambda(self, lam: LambdaValue, args: list) -> Any:
        lam_env = Environment(parent=lam.closure_env)
        for param, arg in zip(lam.params, args):
            lam_env.define(param, arg)
        result = FUSE_UNIT
        for stmt in lam.body:
            result = self._eval_stmt(stmt, lam_env)
        return result

    # ==================================================================
    # Block evaluation with ASAP destruction
    # ==================================================================

    def _eval_block(self, block: Block, env: Environment,
                    asap: bool = False) -> Any:
        stmts = block.stmts
        last_use = self._compute_last_use(stmts) if asap else {}

        result: Any = FUSE_UNIT
        for i, stmt in enumerate(stmts):
            result = self._eval_stmt(stmt, env)

            if asap:
                for name, last_idx in last_use.items():
                    if last_idx == i and not env.is_moved(name):
                        val = env.get_local(name)
                        if (isinstance(val, FuseStruct)
                                and val.del_fn is not None):
                            self._call_del(val)
        return result

    def _call_del(self, struct_val: FuseStruct):
        fn_decl = struct_val.del_fn.decl
        del_env = Environment(parent=self.global_env)
        if fn_decl.params:
            del_env.define(fn_decl.params[0].name, struct_val)
        try:
            if isinstance(fn_decl.body, Block):
                self._eval_block(fn_decl.body, del_env, asap=False)
            else:
                self._eval_expr(fn_decl.body, del_env)
        except EarlyReturn:
            pass

    # ------------------------------------------------------------------
    # Last-use analysis for ASAP destruction
    # ------------------------------------------------------------------

    def _compute_last_use(self, stmts) -> dict[str, int]:
        last_use: dict[str, int] = {}
        n = len(stmts)
        for i, stmt in enumerate(stmts):
            from ast_nodes import DeferStmt as _DS
            idx = n if isinstance(stmt, _DS) else i
            for name in self._collect_idents(stmt):
                prev = last_use.get(name, -1)
                if idx > prev:
                    last_use[name] = idx
        return last_use

    def _collect_idents(self, node) -> set[str]:
        names: set[str] = set()
        self._walk_idents(node, names)
        return names

    def _walk_idents(self, node, names: set[str]):
        if node is None:
            return
        if isinstance(node, Identifier):
            names.add(node.name)
            return
        if isinstance(node, SelfExpr):
            names.add("self")
            return
        if isinstance(node, (int, float, str, bool, type(None))):
            return
        if isinstance(node, Span):
            return
        if isinstance(node, list):
            for item in node:
                self._walk_idents(item, names)
            return
        if isinstance(node, tuple):
            for item in node:
                self._walk_idents(item, names)
            return
        if hasattr(node, "__dataclass_fields__"):
            for field_name in node.__dataclass_fields__:
                self._walk_idents(getattr(node, field_name), names)

    # ==================================================================
    # Statement evaluation
    # ==================================================================

    def _eval_stmt(self, stmt, env: Environment) -> Any:
        if isinstance(stmt, ValDecl):
            value = self._eval_expr(stmt.value, env)
            if isinstance(value, FuseStruct) and isinstance(stmt.value, Identifier):
                value = value.copy()
            env.define(stmt.name, value)
            return FUSE_UNIT

        if isinstance(stmt, VarDecl):
            value = self._eval_expr(stmt.value, env)
            if isinstance(value, FuseStruct) and isinstance(stmt.value, Identifier):
                value = value.copy()
            env.define(stmt.name, value)
            return FUSE_UNIT

        if isinstance(stmt, AssignStmt):
            value = self._eval_expr(stmt.value, env)
            if isinstance(stmt.target, Identifier):
                env.set(stmt.target.name, value)
            elif isinstance(stmt.target, FieldAccessExpr):
                obj = self._eval_expr(stmt.target.object, env)
                if isinstance(obj, FuseStruct):
                    obj.fields[stmt.target.field_name] = value
            return FUSE_UNIT

        if isinstance(stmt, ExprStmt):
            return self._eval_expr(stmt.expr, env)

        if isinstance(stmt, ReturnStmt):
            value = self._eval_expr(stmt.value, env) if stmt.value else FUSE_UNIT
            raise EarlyReturn(value)

        if isinstance(stmt, DeferStmt):
            self._defers.append((stmt.expr, env))
            return FUSE_UNIT

        if isinstance(stmt, IfStmt):
            return self._eval_if(stmt, env)

        if isinstance(stmt, ForStmt):
            return self._eval_for(stmt, env)

        if isinstance(stmt, LoopStmt):
            while True:
                self._eval_block(stmt.body, env)
            return FUSE_UNIT

        raise EvalError(f"unknown statement: {type(stmt).__name__}",
                        self.filename, 0, 0)

    def _eval_if(self, stmt: IfStmt, env: Environment) -> Any:
        cond = self._eval_expr(stmt.condition, env)
        if cond:
            return self._eval_block(stmt.then_body, env)
        if stmt.else_body is not None:
            if isinstance(stmt.else_body, IfStmt):
                return self._eval_if(stmt.else_body, env)
            return self._eval_block(stmt.else_body, env)
        return FUSE_UNIT

    def _eval_for(self, stmt: ForStmt, env: Environment) -> Any:
        iterable = self._eval_expr(stmt.iterable, env)
        items = iterable.elements if isinstance(iterable, FuseList) else iterable
        for item in items:
            env.define(stmt.var_name, item)
            self._eval_block(stmt.body, env)
        return FUSE_UNIT

    # ==================================================================
    # Expression evaluation
    # ==================================================================

    def _eval_expr(self, expr, env: Environment) -> Any:
        # -- Literals --
        if isinstance(expr, IntLiteral):
            return expr.value
        if isinstance(expr, FloatLiteral):
            return expr.value
        if isinstance(expr, StringLiteral):
            return expr.value
        if isinstance(expr, BoolLiteral):
            return expr.value
        if isinstance(expr, UnitLiteral):
            return FUSE_UNIT

        # -- Identifier --
        if isinstance(expr, Identifier):
            val = env.get(expr.name)
            if val is None and expr.name in ("true", "false"):
                return expr.name == "true"
            return val

        if isinstance(expr, SelfExpr):
            return env.get("self")

        # -- Ownership wrappers --
        if isinstance(expr, MoveExpr):
            value = self._eval_expr(expr.expr, env)
            if isinstance(expr.expr, Identifier):
                env.mark_moved(expr.expr.name)
            return value
        if isinstance(expr, RefExpr):
            return self._eval_expr(expr.expr, env)
        if isinstance(expr, MutrefExpr):
            return self._eval_expr(expr.expr, env)

        # -- Binary --
        if isinstance(expr, BinaryExpr):
            return self._eval_binary(expr, env)

        # -- Unary --
        if isinstance(expr, UnaryExpr):
            operand = self._eval_expr(expr.operand, env)
            if expr.op == "-":
                return -operand
            if expr.op == "not":
                return not operand
            raise EvalError(f"unknown unary op: {expr.op}",
                            self.filename, expr.span.line, expr.span.col)

        # -- Field access --
        if isinstance(expr, FieldAccessExpr):
            return self._eval_field_access(expr, env)

        # -- Optional chaining ?. --
        if isinstance(expr, OptionalChainExpr):
            return self._eval_optional_chain(expr, env)

        # -- Question mark ? --
        if isinstance(expr, QuestionExpr):
            return self._eval_question(expr, env)

        # -- Elvis ?: --
        if isinstance(expr, ElvisExpr):
            return self._eval_elvis(expr, env)

        # -- Call --
        if isinstance(expr, CallExpr):
            return self._eval_call(expr, env)

        # -- Match --
        if isinstance(expr, MatchExpr):
            return self._eval_match(expr, env)

        # -- When --
        if isinstance(expr, WhenExpr):
            return self._eval_when(expr, env)

        # -- F-string --
        if isinstance(expr, FStringExpr):
            parts = [format_value(self._eval_expr(p, env)) for p in expr.parts]
            return "".join(parts)

        # -- List literal --
        if isinstance(expr, ListLiteral):
            return FuseList([self._eval_expr(e, env) for e in expr.elements])

        # -- Tuple literal --
        if isinstance(expr, TupleLiteral):
            return tuple(self._eval_expr(e, env) for e in expr.elements)

        # -- Lambda --
        if isinstance(expr, LambdaExpr):
            return LambdaValue(expr.params, expr.body, env)

        # -- Block --
        if isinstance(expr, Block):
            return self._eval_block(expr, env)

        raise EvalError(f"unknown expr: {type(expr).__name__}",
                        self.filename, 0, 0)

    # ------------------------------------------------------------------
    # Binary operators
    # ------------------------------------------------------------------

    def _eval_binary(self, expr: BinaryExpr, env: Environment) -> Any:
        left = self._eval_expr(expr.left, env)
        right = self._eval_expr(expr.right, env)
        op = expr.op
        if op == "+":   return left + right
        if op == "-":   return left - right
        if op == "*":   return left * right
        if op == "/":
            if isinstance(left, int) and isinstance(right, int):
                return left // right
            return left / right
        if op == "%":   return left % right
        if op == "==":  return left == right
        if op == "!=":  return left != right
        if op == "<":   return left < right
        if op == ">":   return left > right
        if op == "<=":  return left <= right
        if op == ">=":  return left >= right
        if op == "and": return left and right
        if op == "or":  return left or right
        raise EvalError(f"unknown binary op: {op}",
                        self.filename, expr.span.line, expr.span.col)

    # ------------------------------------------------------------------
    # Field access
    # ------------------------------------------------------------------

    def _eval_field_access(self, expr: FieldAccessExpr,
                           env: Environment) -> Any:
        obj = self._eval_expr(expr.object, env)
        field = expr.field_name

        if isinstance(obj, FuseStruct):
            if field in obj.fields:
                return obj.fields[field]

        if isinstance(obj, FuseEnumType):
            nfields = obj.variants_info.get(field)
            if nfields is not None:
                if nfields == 0:
                    return FuseEnumVariant(obj.name, field)
                return BuiltinConstructor(obj.name, field)

        if isinstance(obj, FuseEnumVariant):
            # Accessing inner value's fields (e.g., result.value)
            if obj.value is not None and isinstance(obj.value, FuseStruct):
                if field in obj.value.fields:
                    return obj.value.fields[field]

        raise EvalError(
            f"no field '{field}' on {self._type_name(obj)}",
            self.filename, expr.span.line, expr.span.col,
        )

    # ------------------------------------------------------------------
    # Optional chaining ?.
    # ------------------------------------------------------------------

    def _eval_optional_chain(self, expr: OptionalChainExpr,
                             env: Environment) -> Any:
        obj = self._eval_expr(expr.object, env)
        field = expr.field_name

        if (isinstance(obj, FuseEnumVariant) and obj.enum_name == "Option"
                and obj.variant_name == "None"):
            return obj  # propagate None

        if (isinstance(obj, FuseEnumVariant) and obj.enum_name == "Option"
                and obj.variant_name == "Some"):
            obj = obj.value  # unwrap

        if isinstance(obj, FuseStruct) and field in obj.fields:
            return obj.fields[field]

        raise EvalError(
            f"cannot access '{field}' via ?.",
            self.filename, expr.span.line, expr.span.col,
        )

    # ------------------------------------------------------------------
    # ? operator
    # ------------------------------------------------------------------

    def _eval_question(self, expr: QuestionExpr, env: Environment) -> Any:
        value = self._eval_expr(expr.expr, env)

        if isinstance(value, FuseEnumVariant):
            if value.enum_name == "Result":
                if value.variant_name == "Ok":
                    return value.value
                raise EarlyReturn(value)  # propagate Err
            if value.enum_name == "Option":
                if value.variant_name == "Some":
                    return value.value
                raise EarlyReturn(value)  # propagate None

        raise EvalError("? used on non-Result/Option value",
                        self.filename, expr.span.line, expr.span.col)

    # ------------------------------------------------------------------
    # ?: Elvis operator
    # ------------------------------------------------------------------

    def _eval_elvis(self, expr: ElvisExpr, env: Environment) -> Any:
        left = self._eval_expr(expr.left, env)
        if (isinstance(left, FuseEnumVariant) and left.enum_name == "Option"
                and left.variant_name == "None"):
            return self._eval_expr(expr.right, env)
        return left

    # ------------------------------------------------------------------
    # Call expressions
    # ------------------------------------------------------------------

    def _eval_call(self, expr: CallExpr, env: Environment) -> Any:
        # Method call: obj.method(args)
        if isinstance(expr.callee, FieldAccessExpr):
            obj = self._eval_expr(expr.callee.object, env)
            method = expr.callee.field_name

            # Enum variant constructor: Status.Warn("text")
            if isinstance(obj, FuseEnumType):
                args = [self._eval_expr(a, env) for a in expr.args]
                nf = obj.variants_info.get(method, 0)
                val = args[0] if nf == 1 and args else (tuple(args) if args else None)
                return FuseEnumVariant(obj.name, method, val)

            args = [self._eval_expr(a, env) for a in expr.args]
            return self._call_method(obj, method, args, env)

        # Regular call
        callee = self._eval_expr(expr.callee, env)
        args = [self._eval_expr(a, env) for a in expr.args]

        if isinstance(callee, BuiltinFunction):
            return self._call_builtin(callee.name, args)

        if isinstance(callee, BuiltinConstructor):
            val = args[0] if args else None
            return FuseEnumVariant(callee.enum_name, callee.variant_name, val)

        if isinstance(callee, FuseFunction):
            return self._call_user_fn(callee.decl, args)

        if isinstance(callee, StructConstructor):
            return self._construct_struct(callee.decl, args)

        if isinstance(callee, DataClassConstructor):
            return self._construct_data_class(callee.decl, args)

        if isinstance(callee, LambdaValue):
            return self._call_lambda(callee, args)

        raise EvalError(f"not callable: {type(callee).__name__}",
                        self.filename, expr.span.line, expr.span.col)

    # ------------------------------------------------------------------
    # Struct / data class construction
    # ------------------------------------------------------------------

    def _construct_struct(self, decl: StructDecl, args: list) -> FuseStruct:
        fields = {}
        for field_decl, value in zip(decl.fields, args):
            fields[field_decl.name] = value
        del_fn = None
        for m in decl.methods:
            if m.name == "__del__":
                del_fn = FuseFunction(m)
        return FuseStruct(decl.name, fields, del_fn)

    def _construct_data_class(self, decl: DataClassDecl,
                              args: list) -> FuseStruct:
        fields = {}
        for param, value in zip(decl.params, args):
            fields[param.name] = value
        del_fn = None
        for m in decl.methods:
            if m.name == "__del__":
                del_fn = FuseFunction(m)
        return FuseStruct(decl.name, fields, del_fn)

    # ------------------------------------------------------------------
    # Match
    # ------------------------------------------------------------------

    def _eval_match(self, expr: MatchExpr, env: Environment) -> Any:
        subject = self._eval_expr(expr.subject, env)
        for arm in expr.arms:
            bindings = self._match_pattern(arm.pattern, subject)
            if bindings is not None:
                match_env = Environment(parent=env)
                for name, value in bindings.items():
                    match_env.define(name, value)
                return self._eval_expr(arm.body, match_env)
        raise EvalError("no matching arm in match",
                        self.filename, expr.span.line, expr.span.col)

    def _match_pattern(self, pat, value) -> dict[str, Any] | None:
        if isinstance(pat, WildcardPattern):
            return {}

        if isinstance(pat, LiteralPattern):
            return {} if value == pat.value else None

        if isinstance(pat, IdentPattern):
            name = pat.name
            # Qualified enum: Status.Ok, etc.
            if "." in name:
                enum_name, variant = name.rsplit(".", 1)
                if (isinstance(value, FuseEnumVariant)
                        and value.enum_name == enum_name
                        and value.variant_name == variant
                        and value.value is None):
                    return {}
                return None
            # Known singleton variants
            if name == "None":
                if (isinstance(value, FuseEnumVariant)
                        and value.variant_name == "None"):
                    return {}
                return None
            # Variable binding
            return {name: value}

        if isinstance(pat, ConstructorPattern):
            if not isinstance(value, FuseEnumVariant):
                return None
            # Parse the pattern name
            if "." in pat.name:
                enum_name, variant = pat.name.rsplit(".", 1)
                if value.enum_name != enum_name or value.variant_name != variant:
                    return None
            else:
                if value.variant_name != pat.name:
                    return None
            # Match inner args
            if not pat.args:
                return {}
            if len(pat.args) == 1:
                return self._match_pattern(pat.args[0], value.value)
            # Multiple args — treat value.value as tuple
            if not isinstance(value.value, tuple):
                return None
            bindings: dict[str, Any] = {}
            for sub_pat, sub_val in zip(pat.args, value.value):
                sub = self._match_pattern(sub_pat, sub_val)
                if sub is None:
                    return None
                bindings.update(sub)
            return bindings

        if isinstance(pat, TuplePattern):
            if not isinstance(value, tuple):
                return None
            if len(pat.elements) != len(value):
                return None
            bindings = {}
            for sub_pat, sub_val in zip(pat.elements, value):
                sub = self._match_pattern(sub_pat, sub_val)
                if sub is None:
                    return None
                bindings.update(sub)
            return bindings

        return None

    # ------------------------------------------------------------------
    # When
    # ------------------------------------------------------------------

    def _eval_when(self, expr: WhenExpr, env: Environment) -> Any:
        for arm in expr.arms:
            if arm.condition is None:
                return self._eval_expr(arm.body, env)
            if self._eval_expr(arm.condition, env):
                return self._eval_expr(arm.body, env)
        raise EvalError("when expression: no matching arm",
                        self.filename, expr.span.line, expr.span.col)

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _type_name(value) -> str:
        if isinstance(value, bool):
            return "Bool"
        if isinstance(value, int):
            return "Int"
        if isinstance(value, float):
            return "Float"
        if isinstance(value, str):
            return "String"
        if isinstance(value, FuseStruct):
            return value.type_name
        if isinstance(value, FuseList):
            return "List"
        if isinstance(value, FuseEnumVariant):
            return value.enum_name
        if isinstance(value, FuseEnumType):
            return value.name
        return type(value).__name__
