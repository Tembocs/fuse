# ADR-005: Deadlock prevention is a three-tier mandatory hierarchy

**Decision:** The concurrency model prescribes three tiers — channels, `@rank`,
`try_write` — in a mandatory decision path, not as interchangeable alternatives.

**Rationale:** Offering equivalent alternatives puts the cognitive burden of
deadlock safety on the developer at every decision point. A hierarchy with a
clear default (channels) and clear escalation criteria (shared mutation -> `@rank`,
dynamic order -> `try_write`) means the lowest-friction path is also the safest
path.

**Rejected:** Single approach (too restrictive), purely advisory guidance (no enforcement), runtime cycle detection (high cost, wrong layer for a systems language).
