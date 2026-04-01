# ADR-009: `data` is a contextual keyword, not reserved

**Decision:** The word `data` is only treated as a keyword when immediately
followed by `class`. In all other positions it is a regular identifier.

**Rationale:** The canonical Fuse example uses `data` as a parameter name
in `processMetrics(mutref data: List<Metric>)`. Reserving `data` as a
keyword would force renaming a natural parameter name. Since `data` only
has special meaning in the two-word sequence `data class`, making it
contextual costs nothing in parser complexity (one token of lookahead)
and preserves developer ergonomics.

**Rejected:** Reserving `data` as a keyword (breaks natural naming), using
a different keyword for data classes like `record` (adds an unfamiliar term).
