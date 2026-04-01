"""Fuse Stage 0 — Lexer.

Converts source text to a flat token stream.
Tracks line and column for every token.
"""

from __future__ import annotations
from fuse_token import Token, TokenType, KEYWORDS
from errors import LexerError


class Lexer:
    def __init__(self, source: str, filename: str = "<stdin>"):
        self.source = source
        self.filename = filename
        self.pos = 0
        self.line = 1
        self.col = 1
        self.tokens: list[Token] = []

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def tokenize(self) -> list[Token]:
        while self.pos < len(self.source):
            self._skip_whitespace_and_comments()
            if self.pos >= len(self.source):
                break

            ch = self._current()

            if ch.isdigit():
                self._read_number()
            elif ch == '"':
                self._read_string()
            elif ch == "f" and self._peek(1) == '"':
                self._read_fstring()
            elif ch.isalpha() or ch == "_":
                self._read_identifier()
            else:
                self._read_operator()

        self.tokens.append(Token(TokenType.EOF, None, self.line, self.col))
        return self.tokens

    # ------------------------------------------------------------------
    # Character helpers
    # ------------------------------------------------------------------

    def _current(self) -> str:
        return self.source[self.pos] if self.pos < len(self.source) else "\0"

    def _peek(self, offset: int = 1) -> str:
        p = self.pos + offset
        return self.source[p] if p < len(self.source) else "\0"

    def _advance(self) -> str:
        ch = self.source[self.pos]
        self.pos += 1
        if ch == "\n":
            self.line += 1
            self.col = 1
        else:
            self.col += 1
        return ch

    def _error(self, msg: str) -> LexerError:
        return LexerError(msg, self.filename, self.line, self.col)

    # ------------------------------------------------------------------
    # Whitespace & comments
    # ------------------------------------------------------------------

    def _skip_whitespace_and_comments(self):
        while self.pos < len(self.source):
            ch = self._current()
            if ch in " \t\r\n":
                self._advance()
            elif ch == "/" and self._peek() == "/":
                while self.pos < len(self.source) and self._current() != "\n":
                    self._advance()
            else:
                break

    # ------------------------------------------------------------------
    # Numbers
    # ------------------------------------------------------------------

    def _read_number(self):
        start_line, start_col = self.line, self.col
        num = ""
        is_float = False

        while self.pos < len(self.source):
            ch = self._current()
            if ch.isdigit():
                num += self._advance()
            elif ch == ".":
                nxt = self._peek()
                if nxt == ".":          # .. range operator
                    break
                if nxt.isalpha() or nxt == "_":  # method call on int
                    break
                is_float = True
                num += self._advance()
            else:
                break

        if is_float:
            self.tokens.append(Token(TokenType.FLOAT, float(num), start_line, start_col))
        else:
            self.tokens.append(Token(TokenType.INT, int(num), start_line, start_col))

    # ------------------------------------------------------------------
    # Strings
    # ------------------------------------------------------------------

    def _read_string(self):
        start_line, start_col = self.line, self.col
        self._advance()  # skip opening "
        result = self._read_string_body()
        self.tokens.append(Token(TokenType.STRING, result, start_line, start_col))

    def _read_string_body(self) -> str:
        start_line, start_col = self.line, self.col
        result = ""
        while self.pos < len(self.source) and self._current() != '"':
            if self._current() == "\\":
                self._advance()
                result += self._read_escape()
            else:
                result += self._advance()
        if self.pos >= len(self.source):
            raise self._error("Unterminated string")
        self._advance()  # skip closing "
        return result

    def _read_escape(self) -> str:
        if self.pos >= len(self.source):
            raise self._error("Unterminated escape sequence")
        ch = self._advance()
        return {"n": "\n", "t": "\t", "\\": "\\", '"': '"',
                "{": "{", "}": "}"}.get(ch, "\\" + ch)

    # ------------------------------------------------------------------
    # F-strings
    # ------------------------------------------------------------------

    def _read_fstring(self):
        start_line, start_col = self.line, self.col
        self._advance()  # skip 'f'
        self._advance()  # skip '"'

        parts: list[tuple[str, str]] = []
        current = ""

        while self.pos < len(self.source) and self._current() != '"':
            if self._current() == "{":
                if current:
                    parts.append(("str", current))
                    current = ""
                self._advance()  # skip '{'
                expr = self._read_fstring_expr()
                parts.append(("expr", expr))
            elif self._current() == "\\":
                self._advance()
                current += self._read_escape()
            else:
                current += self._advance()

        if current:
            parts.append(("str", current))

        if self.pos >= len(self.source):
            raise self._error("Unterminated f-string")
        self._advance()  # skip closing "

        self.tokens.append(Token(TokenType.FSTRING, parts, start_line, start_col))

    def _read_fstring_expr(self) -> str:
        """Read the expression inside {...} of an f-string."""
        expr = ""
        depth = 1
        while self.pos < len(self.source) and depth > 0:
            ch = self._current()
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
            if depth > 0:
                expr += self._advance()
            else:
                self._advance()  # skip closing '}'
        if depth > 0:
            raise self._error("Unterminated f-string interpolation")
        return expr

    # ------------------------------------------------------------------
    # Identifiers & keywords
    # ------------------------------------------------------------------

    def _read_identifier(self):
        start_line, start_col = self.line, self.col
        ident = ""
        while self.pos < len(self.source) and (self._current().isalnum() or self._current() == "_"):
            ident += self._advance()
        token_type = KEYWORDS.get(ident, TokenType.IDENT)
        self.tokens.append(Token(token_type, ident, start_line, start_col))

    # ------------------------------------------------------------------
    # Operators & delimiters
    # ------------------------------------------------------------------

    _TWO_CHAR: dict[str, TokenType] = {
        "=>": TokenType.FAT_ARROW,
        "->": TokenType.ARROW,
        "?.": TokenType.QUESTION_DOT,
        "?:": TokenType.ELVIS,
        "==": TokenType.EQEQ,
        "!=": TokenType.BANGEQ,
        "<=": TokenType.LTEQ,
        ">=": TokenType.GTEQ,
        "::": TokenType.COLONCOLON,
        "..": TokenType.DOTDOT,
    }

    _ONE_CHAR: dict[str, TokenType] = {
        "?": TokenType.QUESTION,
        "@": TokenType.AT,
        ".": TokenType.DOT,
        ":": TokenType.COLON,
        "=": TokenType.EQUALS,
        "<": TokenType.LT,
        ">": TokenType.GT,
        "+": TokenType.PLUS,
        "-": TokenType.MINUS,
        "*": TokenType.STAR,
        "/": TokenType.SLASH,
        "%": TokenType.PERCENT,
        "|": TokenType.PIPE,
        "(": TokenType.LPAREN,
        ")": TokenType.RPAREN,
        "{": TokenType.LBRACE,
        "}": TokenType.RBRACE,
        "[": TokenType.LBRACKET,
        "]": TokenType.RBRACKET,
        ",": TokenType.COMMA,
        ";": TokenType.SEMICOLON,
    }

    def _read_operator(self):
        start_line, start_col = self.line, self.col
        two = self._current() + self._peek()

        if two in self._TWO_CHAR:
            self._advance()
            self._advance()
            self.tokens.append(Token(self._TWO_CHAR[two], two, start_line, start_col))
            return

        ch = self._current()
        if ch in self._ONE_CHAR:
            self._advance()
            self.tokens.append(Token(self._ONE_CHAR[ch], ch, start_line, start_col))
            return

        raise self._error(f"Unexpected character: {ch!r}")


# ------------------------------------------------------------------
# Standalone entry point
# ------------------------------------------------------------------

if __name__ == "__main__":
    import sys
    if len(sys.argv) < 2:
        print("Usage: python lexer.py <file.fuse>")
        sys.exit(1)
    with open(sys.argv[1]) as f:
        source = f.read()
    lexer = Lexer(source, sys.argv[1])
    for tok in lexer.tokenize():
        print(tok)
