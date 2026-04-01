"""Fuse Stage 0 — Interpreter error types and formatting."""

from __future__ import annotations
from dataclasses import dataclass


@dataclass
class FuseError(Exception):
    """Base error for all Fuse interpreter errors."""
    message: str
    filename: str = "<unknown>"
    line: int = 0
    col: int = 0
    hint: str | None = None

    def __post_init__(self):
        super().__init__(self.message)

    def __str__(self):
        result = f"error: {self.message}"
        if self.hint:
            result += f"\n       {self.hint}"
        result += f"\n  --> {self.filename}:{self.line}:{self.col}"
        return result


class LexerError(FuseError):
    """Error during tokenization."""
    pass


class ParseError(FuseError):
    """Error during parsing."""
    pass


class CheckError(FuseError):
    """Error during ownership/type checking."""
    pass


class EvalError(FuseError):
    """Error during evaluation."""
    pass
