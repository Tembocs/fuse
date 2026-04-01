"""Fuse Stage 0 — Runtime value representations.

Fuse primitives map to Python primitives (int, float, str, bool).
Complex types use dedicated classes below.
"""

from __future__ import annotations
from dataclasses import dataclass, field


# =====================================================================
# Sentinels
# =====================================================================

class _FuseUnit:
    """The () value."""
    __slots__ = ()
    def __repr__(self): return "()"
    def __str__(self): return "()"
    def __bool__(self): return False

FUSE_UNIT = _FuseUnit()


# =====================================================================
# Enum variants (Option, Result, user-defined)
# =====================================================================

class FuseEnumVariant:
    __slots__ = ("enum_name", "variant_name", "value")

    def __init__(self, enum_name: str, variant_name: str, value=None):
        self.enum_name = enum_name
        self.variant_name = variant_name
        self.value = value

    def __repr__(self):
        if self.value is not None:
            return f"{self.variant_name}({format_value(self.value)})"
        return self.variant_name

    def __eq__(self, other):
        return (isinstance(other, FuseEnumVariant)
                and self.enum_name == other.enum_name
                and self.variant_name == other.variant_name
                and self.value == other.value)

    def __hash__(self):
        return hash((self.enum_name, self.variant_name))


class FuseEnumType:
    """Namespace for an enum — field access returns variants."""
    __slots__ = ("name", "variants_info")

    def __init__(self, name: str, variants_info: dict[str, int]):
        self.name = name
        self.variants_info = variants_info      # {variant_name: num_fields}

    def __repr__(self):
        return f"<enum {self.name}>"


# =====================================================================
# Structs
# =====================================================================

class FuseStruct:
    __slots__ = ("type_name", "fields", "del_fn")

    def __init__(self, type_name: str, fields: dict, del_fn=None):
        self.type_name = type_name
        self.fields = dict(fields)
        self.del_fn = del_fn

    def __repr__(self):
        vals = ", ".join(format_value(v) for v in self.fields.values())
        return f"{self.type_name}({vals})"

    def __eq__(self, other):
        return (isinstance(other, FuseStruct)
                and self.type_name == other.type_name
                and self.fields == other.fields)

    def __hash__(self):
        return id(self)

    def copy(self):
        return FuseStruct(self.type_name, dict(self.fields), self.del_fn)


# =====================================================================
# Lists
# =====================================================================

class FuseList:
    __slots__ = ("elements",)

    def __init__(self, elements=None):
        self.elements = list(elements) if elements else []

    def __repr__(self):
        return "[" + ", ".join(format_value(e) for e in self.elements) + "]"

    def __eq__(self, other):
        return isinstance(other, FuseList) and self.elements == other.elements


# =====================================================================
# Callables
# =====================================================================

class FuseFunction:
    """Wraps a user-defined FnDecl."""
    __slots__ = ("decl",)
    def __init__(self, decl):
        self.decl = decl

class LambdaValue:
    """A captured lambda with closure environment."""
    __slots__ = ("params", "body", "closure_env")
    def __init__(self, params, body, closure_env):
        self.params = params
        self.body = body
        self.closure_env = closure_env

class BuiltinFunction:
    __slots__ = ("name",)
    def __init__(self, name: str):
        self.name = name

class BuiltinConstructor:
    """Constructor for an enum variant (Some, Ok, Err, Status.Warn, ...)."""
    __slots__ = ("enum_name", "variant_name")
    def __init__(self, enum_name: str, variant_name: str):
        self.enum_name = enum_name
        self.variant_name = variant_name

class StructConstructor:
    __slots__ = ("decl",)
    def __init__(self, decl):
        self.decl = decl

class DataClassConstructor:
    __slots__ = ("decl",)
    def __init__(self, decl):
        self.decl = decl


# =====================================================================
# Control flow
# =====================================================================

class EarlyReturn(Exception):
    """Raised by `return` and `?` to unwind to the enclosing function."""
    __slots__ = ("value",)
    def __init__(self, value):
        self.value = value


# =====================================================================
# Value formatting
# =====================================================================

def format_value(value) -> str:
    """Format a Fuse value for display (f-strings, println)."""
    if value is FUSE_UNIT:
        return "()"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return str(value)
    if isinstance(value, str):
        return value
    if isinstance(value, FuseStruct):
        vals = ", ".join(format_value(v) for v in value.fields.values())
        return f"{value.type_name}({vals})"
    if isinstance(value, FuseList):
        return "[" + ", ".join(format_value(e) for e in value.elements) + "]"
    if isinstance(value, FuseEnumVariant):
        if value.value is not None:
            return f"{value.variant_name}({format_value(value.value)})"
        return value.variant_name
    return str(value)
