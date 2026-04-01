# ADR-006: Stage 0 implemented in Python, not Rust

**Decision:** The first interpreter is a Python tree-walking evaluator.

**Rationale:** Stage 0 must answer one question: are the language semantics
correct? Python allows complete focus on that question without the overhead of
managing a code generation backend. The Python interpreter becomes the permanent
reference implementation against which the Rust compiler is tested.

**Rejected:** Rust from the start (correct long-term, wrong for Stage 0 — conflates semantic validation with code generation), TypeScript (good for parsing, less natural for evaluation), Haskell (precedent from Rust, but team familiarity with Python and Rust makes Python the better choice).
