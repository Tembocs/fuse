# ADR-004: `@rank` on `Shared<T>` is a compile error when absent

**Decision:** Declaring a `Shared<T>` without an `@rank(N)` annotation is a
hard compile error.

**Rationale:** Optional safety annotations get skipped under deadline pressure
and are added only after a production incident. A compile error means the language
is never in an unguarded state. The cost is one integer, written once, at the
declaration site where the developer already has the most context about dependency
order.

**Rejected:** Lint warning (ignored under pressure), runtime detection (too late,
wrong layer), no enforcement (defeats the purpose of the tier system).
