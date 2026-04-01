# Contributing to Fuse

Fuse is in active design and early implementation. Contributions are welcome at
every level.

## Before writing code

Read [`docs/guide/fuse-language-guide.md`](docs/guide/fuse-language-guide.md).
Every feature in Fuse has a documented rationale. Understanding the why prevents
implementing something that contradicts a deliberate decision.

## Adding a language feature

1. Update the language guide with the concept, rationale, and a code example
2. Add an ADR to `docs/adr/` if the design choice is non-obvious
3. Add test cases to `tests/fuse/core/` or `tests/fuse/full/`
4. Implement in Stage 0 first — verify tests pass
5. Implement in Stage 1 — verify the same tests pass

The guide is updated before the implementation. A feature without a guide
entry does not exist yet.

## Reporting issues

If observed behaviour does not match the language guide, that is a bug in the
implementation. If the language guide itself is wrong or incomplete, open a
discussion — guide changes may require an ADR.
