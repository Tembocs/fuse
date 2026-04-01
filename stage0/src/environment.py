"""Fuse Stage 0 — Scope and binding management.

Tracks variable bindings with a parent-chain for lexical scoping.
"""

from __future__ import annotations


class Environment:
    __slots__ = ("parent", "bindings", "_moved")

    def __init__(self, parent: Environment | None = None):
        self.parent = parent
        self.bindings: dict[str, object] = {}
        self._moved: set[str] = set()

    def define(self, name: str, value: object):
        self.bindings[name] = value

    def get(self, name: str) -> object | None:
        if name in self.bindings:
            return self.bindings[name]
        if self.parent is not None:
            return self.parent.get(name)
        return None

    def get_local(self, name: str) -> object | None:
        """Look up only in the current scope (not parent)."""
        return self.bindings.get(name)

    def set(self, name: str, value: object) -> bool:
        """Reassign an existing binding. Returns True if found."""
        if name in self.bindings:
            self.bindings[name] = value
            return True
        if self.parent is not None:
            return self.parent.set(name, value)
        return False

    def mark_moved(self, name: str):
        self._moved.add(name)

    def is_moved(self, name: str) -> bool:
        return name in self._moved
