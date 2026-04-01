"""Fuse Stage 0 — Tree-walking evaluator.

Executes AST nodes. Handles:
  - All expression forms
  - Pattern matching with destructuring
  - ? operator (unwrap Ok/Some or return early)
  - defer callbacks
  - ASAP destruction simulation
  - Extension function dispatch
  - f"..." interpolation
  - ?. and ?: short-circuit evaluation
"""
