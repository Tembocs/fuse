"""Fuse Stage 0 — CLI entry point.

Usage:
    python main.py <file.fuse>        Parse and display AST
    python main.py --tokens <file>    Tokenize only
    python main.py --check <file>     Check without running (Phase 3+)
    python main.py --repl             Interactive REPL (Phase 4+)
"""

from __future__ import annotations
import sys
import os

# Ensure the script's directory is on the path for local imports.
sys.path.insert(0, os.path.dirname(__file__))

from lexer import Lexer
from parser import Parser
from errors import FuseError


def main():
    if len(sys.argv) < 2:
        print(__doc__.strip())
        sys.exit(1)

    mode = "parse"
    filepath = sys.argv[1]

    if sys.argv[1] == "--tokens" and len(sys.argv) > 2:
        mode = "tokens"
        filepath = sys.argv[2]
    elif sys.argv[1] == "--check" and len(sys.argv) > 2:
        mode = "check"
        filepath = sys.argv[2]
    elif sys.argv[1] == "--repl":
        print("REPL not yet implemented (Phase 4).")
        sys.exit(0)

    try:
        with open(filepath) as f:
            source = f.read()
    except FileNotFoundError:
        print(f"error: file not found: {filepath}", file=sys.stderr)
        sys.exit(1)

    try:
        lexer = Lexer(source, filepath)
        tokens = lexer.tokenize()

        if mode == "tokens":
            for tok in tokens:
                print(tok)
            return

        parser = Parser(tokens, filepath)
        program = parser.parse()

        if mode == "check":
            print("Check not yet implemented (Phase 3).")
            return

        # Default: print AST summary
        for decl in program.declarations:
            print(decl)
            print()

    except FuseError as e:
        print(e, file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
