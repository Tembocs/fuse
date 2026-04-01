# ADR-001: `borrowed` renamed to `ref`

**Decision:** The read-only argument convention is named `ref`, not `borrowed`.

**Rationale:** `ref` and `mutref` share a visible prefix. A reader seeing both
for the first time understands the relationship immediately: `ref` reads,
`mutref` reads and modifies. `borrowed` has no such relationship to `mutref`
and requires learning two unrelated names for related concepts.

**Rejected:** `borrowed` (Mojo's original), `ro` (too abbreviated), `read` (too verbose for a convention keyword).
