# ADR-007: Cranelift chosen over LLVM for Stage 1

**Decision:** The Stage 1 Rust compiler uses Cranelift as its code generation backend.

**Rationale:** Cranelift is designed as a code generation backend — simpler API,
faster compile times, sufficient output quality for a Stage 1 compiler. LLVM's
full optimisation pipeline is not needed until Stage 2 or beyond. Cranelift
reduces integration complexity and lets Stage 1 focus on correctness over
maximum performance.

**Rejected:** LLVM (correct long-term, too much integration complexity for Stage 1), QBE (less mature, smaller community), custom backend (unnecessary work given Cranelift's quality).
