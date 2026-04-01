"""Fuse Stage 0 — Token definitions.

Token type enum covering every terminal in Fuse Core,
plus reserved keywords for Fuse Full.
"""

from __future__ import annotations
from enum import Enum, auto
from dataclasses import dataclass
from typing import Any


class TokenType(Enum):
    # --- Keywords ---
    FN = auto()
    VAL = auto()
    VAR = auto()
    REF = auto()
    MUTREF = auto()
    OWNED = auto()
    MOVE = auto()
    STRUCT = auto()
    DATA = auto()
    CLASS = auto()
    ENUM = auto()
    MATCH = auto()
    WHEN = auto()
    IF = auto()
    ELSE = auto()
    FOR = auto()
    IN = auto()
    LOOP = auto()
    RETURN = auto()
    DEFER = auto()
    AND = auto()
    OR = auto()
    NOT = auto()
    TRUE = auto()
    FALSE = auto()
    SELF = auto()

    # Fuse Full keywords (reserved — parsed but not evaluated in Stage 0)
    SPAWN = auto()
    ASYNC = auto()
    AWAIT = auto()
    SUSPEND = auto()

    # --- Operators ---
    ARROW = auto()          # ->
    FAT_ARROW = auto()      # =>
    QUESTION_DOT = auto()   # ?.
    ELVIS = auto()          # ?:
    QUESTION = auto()       # ?
    AT = auto()             # @
    DOT = auto()            # .
    DOTDOT = auto()         # ..
    COLON = auto()          # :
    COLONCOLON = auto()     # ::
    EQUALS = auto()         # =
    EQEQ = auto()          # ==
    BANGEQ = auto()         # !=
    LT = auto()             # <
    GT = auto()             # >
    LTEQ = auto()           # <=
    GTEQ = auto()           # >=
    PLUS = auto()           # +
    MINUS = auto()          # -
    STAR = auto()           # *
    SLASH = auto()          # /
    PERCENT = auto()        # %
    PIPE = auto()           # |

    # --- Delimiters ---
    LPAREN = auto()         # (
    RPAREN = auto()         # )
    LBRACE = auto()         # {
    RBRACE = auto()         # }
    LBRACKET = auto()       # [
    RBRACKET = auto()       # ]
    COMMA = auto()          # ,
    SEMICOLON = auto()      # ;

    # --- Literals ---
    INT = auto()
    FLOAT = auto()
    STRING = auto()
    FSTRING = auto()        # f"..." — value is list of ("str"|"expr", text) pairs

    # --- Identifiers ---
    IDENT = auto()

    # --- Special ---
    EOF = auto()


@dataclass
class Token:
    type: TokenType
    value: Any
    line: int
    col: int

    def __repr__(self):
        if self.value is not None:
            return f"Token({self.type.name}, {self.value!r}, {self.line}:{self.col})"
        return f"Token({self.type.name}, {self.line}:{self.col})"


KEYWORDS: dict[str, TokenType] = {
    "fn":       TokenType.FN,
    "val":      TokenType.VAL,
    "var":      TokenType.VAR,
    "ref":      TokenType.REF,
    "mutref":   TokenType.MUTREF,
    "owned":    TokenType.OWNED,
    "move":     TokenType.MOVE,
    "struct":   TokenType.STRUCT,
    "class":    TokenType.CLASS,
    "enum":     TokenType.ENUM,
    "match":    TokenType.MATCH,
    "when":     TokenType.WHEN,
    "if":       TokenType.IF,
    "else":     TokenType.ELSE,
    "for":      TokenType.FOR,
    "in":       TokenType.IN,
    "loop":     TokenType.LOOP,
    "return":   TokenType.RETURN,
    "defer":    TokenType.DEFER,
    "and":      TokenType.AND,
    "or":       TokenType.OR,
    "not":      TokenType.NOT,
    "true":     TokenType.TRUE,
    "false":    TokenType.FALSE,
    "self":     TokenType.SELF,
    "spawn":    TokenType.SPAWN,
    "async":    TokenType.ASYNC,
    "await":    TokenType.AWAIT,
    "suspend":  TokenType.SUSPEND,
}
