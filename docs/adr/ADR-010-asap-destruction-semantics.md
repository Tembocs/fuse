# ADR-010: ASAP destruction is last-use scoped at function level

**Decision:** A value's `__del__` fires immediately after the statement
containing its last reference within the enclosing function body. Deferred
callbacks (`defer`) fire after all ASAP destruction, at function exit, in
reverse registration order.

**Rationale:** ASAP destruction must be deterministic and predictable from
reading the source code. Scoping destruction to the function body (rather
than individual blocks or the entire program) gives the right granularity:
lock guards release as soon as they are no longer used, memory is reclaimed
promptly, and destruction order is visible in the code. Firing defers after
ASAP destruction ensures that cleanup callbacks see a consistent state where
all owned values have already been finalized.

**Execution order within a function:**

1. Statements execute sequentially.
2. After each statement, if any variable's last reference was in that
   statement and the value has a `__del__`, it fires immediately.
3. After the last statement, any remaining ASAP destruction fires.
4. Deferred callbacks fire in reverse registration order (LIFO).

**Edge cases:**

- Variables referenced only inside a `defer` expression are kept alive
  until after all non-deferred statements complete (their last-use index
  is set to beyond the last statement).
- Variables transferred via `move` are not destroyed in the caller's scope;
  destruction responsibility transfers to the callee.
- `__del__` bodies execute without ASAP destruction to prevent infinite
  recursion.

**Rejected:** Scope-end destruction (wastes memory, less predictable),
reference counting (runtime overhead inappropriate for a systems language),
tracing GC (violates the "no GC" property).
