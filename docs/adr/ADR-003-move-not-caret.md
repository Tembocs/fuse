# ADR-003: `^` transfer operator replaced by `move` keyword

**Decision:** Ownership transfer at the call site is written `move value`,
not `value^`.

**Rationale:** A keyword reads as intent. `shutdown(move conn)` says "move
ownership of conn into shutdown" — it can be read aloud correctly. `shutdown(conn^)`
requires a legend to understand. A prefix keyword also aligns with how all
other conventions are written — before the argument.

**Rejected:** `conn^` (Mojo's sigil), `give conn` (informal, non-standard), `transfer conn` (verbose), `own conn` (ambiguous direction).
