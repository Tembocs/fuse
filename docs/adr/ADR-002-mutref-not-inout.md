# ADR-002: `inout` renamed to `mutref`

**Decision:** The mutable reference convention is named `mutref`, not `inout`.

**Rationale:** `mutref` is self-documenting: mutable reference. A developer
reads `mutref data` and knows two things — the value will not be copied and it
will be modified. `inout` is an audio-engineering term with no semantic precision
in a programming context.

**Rejected:** `inout` (Mojo's original), `mut` (implies ownership, not reference), `rw` (too abbreviated).
