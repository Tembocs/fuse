# Fuse Repository Layout

> **For AI agents reading this document:** This describes the source repository
> structure for the Fuse programming language. Each top-level directory maps to
> one concern. Stage boundaries are hard directory boundaries — `stage0/`,
> `stage1/`, `stage2/`. Shared test cases live in `tests/fuse/` and are
> executed by all stages. The canonical milestone program is
> `tests/fuse/milestone/four_functions.fuse`.

---

## Philosophy

One repository, three stages, one test suite. The stages do not share source
code — each is an independent implementation. They do share test cases, the
language guide, and the standard library definitions. A test that passes in
Stage 0 must produce identical output in Stage 1 and Stage 2.

The repository is structured so that each stage can be understood, built, and
tested in isolation. Nothing in `stage1/` depends on `stage0/`. The shared
test suite is the contract between them.

---

## Full Tree

```
fuse/
│
├── README.md                          # Project overview and quick start
├── CONTRIBUTING.md                    # Contribution guidelines
├── LICENSE
│
├── docs/                              # All human and AI-readable documentation
│   ├── guide/
│   │   └── fuse-language-guide.md    # The canonical language guide
│   ├── adr/                           # Architecture Decision Records (standalone copies)
│   │   ├── ADR-001-ref-not-borrowed.md
│   │   ├── ADR-002-mutref-not-inout.md
│   │   ├── ADR-003-move-not-caret.md
│   │   ├── ADR-004-rank-mandatory.md
│   │   ├── ADR-005-deadlock-three-tiers.md
│   │   ├── ADR-006-stage0-python.md
│   │   ├── ADR-007-cranelift-not-llvm.md
│   │   └── ADR-008-no-timelines.md
│   └── spec/                          # Future: formal grammar and type rules
│       └── .gitkeep                   # Reserved — populated in Stage 1
│
├── tests/                             # Shared test suite — all stages must pass these
│   └── fuse/
│       ├── milestone/
│       │   └── four_functions.fuse    # Stage 0 milestone: the canonical example
│       ├── core/                      # Tests for Fuse Core features only
│       │   ├── ownership/
│       │   │   ├── ref_read_only.fuse
│       │   │   ├── mutref_modifies_caller.fuse
│       │   │   ├── move_transfers_ownership.fuse
│       │   │   └── move_prevents_reuse.fuse
│       │   ├── memory/
│       │   │   ├── asap_destruction.fuse
│       │   │   ├── value_auto_lifecycle.fuse
│       │   │   └── del_fires_at_last_use.fuse
│       │   ├── errors/
│       │   │   ├── result_propagation.fuse
│       │   │   ├── option_chaining.fuse
│       │   │   ├── match_exhaustive.fuse
│       │   │   └── question_mark_shortcircuit.fuse
│       │   └── types/
│       │       ├── val_immutable.fuse
│       │       ├── var_mutable.fuse
│       │       ├── data_class_equality.fuse
│       │       └── extension_functions.fuse
│       └── full/                      # Tests for Fuse Full — Stage 1 and beyond
│           ├── concurrency/
│           │   ├── chan_basic.fuse
│           │   ├── chan_bounded_backpressure.fuse
│           │   ├── shared_rank_ascending.fuse
│           │   ├── shared_rank_violation.fuse    # must produce compile error
│           │   ├── shared_no_rank.fuse           # must produce compile error
│           │   └── spawn_mutref_rejected.fuse    # must produce compile error
│           ├── async/
│           │   ├── await_basic.fuse
│           │   ├── suspend_fn.fuse
│           │   └── write_guard_across_await.fuse # must produce compile warning
│           └── simd/
│               └── simd_sum.fuse
│
├── stdlib/                            # Standard library — written in Fuse
│   ├── README.md                      # Which stdlib files are available at each stage
│   ├── core/                          # Available in Fuse Core (Stage 0+)
│   │   ├── result.fuse                # Result<T,E>, Ok, Err
│   │   ├── option.fuse                # Option<T>, Some, None
│   │   ├── list.fuse                  # List<T> — map, filter, sorted, retainWhere, etc.
│   │   ├── string.fuse                # String — interpolation, slicing, parsing
│   │   ├── int.fuse                   # Int — arithmetic, conversions
│   │   ├── float.fuse                 # Float — arithmetic, SIMD-ready layout
│   │   └── bool.fuse                  # Bool — and, or, not
│   └── full/                          # Available in Fuse Full (Stage 1+)
│       ├── chan.fuse                   # Chan<T> — bounded, unbounded, send, recv
│       ├── shared.fuse                # Shared<T> — read, write, try_write
│       ├── timer.fuse                 # Timer — sleep, timeout
│       ├── simd.fuse                  # SIMD<T,N> — sum, dot, broadcast
│       └── http.fuse                  # Http — get, post (used in canonical example)
│
├── examples/                          # Standalone Fuse programs for learning and testing
│   ├── README.md
│   ├── hello.fuse                     # Simplest possible Fuse program
│   ├── ownership_tour.fuse            # ref, mutref, owned, move demonstrated
│   ├── error_handling.fuse            # Result, Option, match, ?
│   ├── channels.fuse                  # Tier 1 concurrency — Chan<T>
│   ├── shared_state.fuse              # Tier 2 concurrency — Shared<T> + @rank
│   └── four_functions.fuse            # The full canonical example from the guide
│
├── stage0/                            # Python tree-walking interpreter — Fuse Core only
│   ├── README.md                      # How to install, run, and test Stage 0
│   ├── requirements.txt               # Python dependencies (stdlib only — no third-party)
│   │
│   ├── src/
│   │   ├── main.py                    # CLI entry point: fusec0 <file.fuse>
│   │   ├── repl.py                    # Interactive REPL: fusec0 --repl
│   │   ├── lexer.py                   # Tokeniser — produces token stream from source text
│   │   ├── token.py                   # Token type definitions and literals
│   │   ├── parser.py                  # Recursive descent parser — produces AST
│   │   ├── ast.py                     # AST node dataclasses for every construct
│   │   ├── checker.py                 # Ownership checker and basic type verifier
│   │   │                              #   - ref/mutref/owned/move enforcement
│   │   │                              #   - match exhaustiveness
│   │   │                              #   - @rank ordering (preparatory — no runtime yet)
│   │   ├── evaluator.py               # Tree-walking evaluator — executes AST nodes
│   │   ├── environment.py             # Scope and binding management
│   │   ├── values.py                  # Runtime value representations
│   │   │                              #   - FuseInt, FuseFloat, FuseString
│   │   │                              #   - FuseStruct, FuseList
│   │   │                              #   - FuseResult (Ok/Err), FuseOption (Some/None)
│   │   └── errors.py                  # Interpreter error types and formatting
│   │
│   └── tests/
│       ├── run_tests.py               # Test runner — executes tests/fuse/core/ against Stage 0
│       └── snapshots/                 # Expected output for each test case
│           └── ...
│
├── stage1/                            # Rust compiler with Cranelift backend — Fuse Full
│   ├── README.md                      # How to build, run, and test Stage 1
│   ├── Cargo.toml                     # Workspace manifest
│   │
│   ├── fusec/                         # The compiler binary crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs                # CLI entry point: fusec <file.fuse>
│   │       ├── lexer/
│   │       │   ├── mod.rs
│   │       │   ├── token.rs           # Token types — mirrors stage0/token.py
│   │       │   └── lexer.rs           # Tokeniser
│   │       ├── parser/
│   │       │   ├── mod.rs
│   │       │   └── parser.rs          # Recursive descent parser — produces AST
│   │       ├── ast/
│   │       │   ├── mod.rs
│   │       │   └── nodes.rs           # AST node definitions
│   │       ├── hir/                   # High-level intermediate representation
│   │       │   ├── mod.rs
│   │       │   ├── lower.rs           # AST → HIR lowering
│   │       │   └── nodes.rs           # HIR node definitions
│   │       ├── checker/               # Semantic analysis
│   │       │   ├── mod.rs
│   │       │   ├── types.rs           # Type inference and checking
│   │       │   ├── ownership.rs       # ref/mutref/owned/move enforcement
│   │       │   ├── exhaustiveness.rs  # match exhaustiveness checking
│   │       │   ├── rank.rs            # @rank ordering enforcement
│   │       │   ├── spawn.rs           # spawn capture rule enforcement
│   │       │   └── async_lint.rs      # write-guard-across-await warning
│   │       └── codegen/
│   │           ├── mod.rs
│   │           ├── cranelift.rs       # HIR → Cranelift IR translation
│   │           └── layout.rs          # Value layout and ABI
│   │
│   ├── fuse-runtime/                  # Runtime support library crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── asap.rs                # ASAP destruction bookkeeping
│   │       ├── chan.rs                # Chan<T> implementation
│   │       ├── shared.rs              # Shared<T> + RwLock guard implementation
│   │       └── async_rt.rs            # Async executor (lightweight, no tokio dependency)
│   │
│   └── tests/
│       └── run_tests.rs               # Test runner — executes tests/fuse/ against Stage 1
│
└── stage2/                            # Self-hosting Fuse compiler — written in Fuse
    ├── README.md                      # Entry condition and milestone definition
    └── src/
        └── .gitkeep                   # Reserved — populated when Stage 1 milestone is met
```

---

## Directory Reference

### `docs/`

All documentation lives here. The language guide is the authoritative source for
language behaviour. ADRs in `docs/adr/` are standalone files — the same decisions
recorded at the end of the guide, each given a dedicated file so they can be
linked, cited, or updated independently.

The `spec/` directory is intentionally empty until Stage 1. A formal grammar is
worth writing once the language is stable enough that writing it is descriptive
rather than speculative.

---

### `tests/fuse/`

The shared test suite. Every `.fuse` file here is a valid (or intentionally
invalid) Fuse program. Test runners in each stage execute these files and compare
output against snapshots.

**Three categories of test:**

- **Core tests** (`tests/fuse/core/`) — Fuse Core features only. Stage 0 must
  pass all of these. Stage 1 and Stage 2 must also pass all of these.
- **Full tests** (`tests/fuse/full/`) — Fuse Full features. Stage 1 and Stage 2
  only.
- **Milestone** (`tests/fuse/milestone/`) — the canonical four-function program
  from Section 11 of the language guide. This is the single most important test
  in the repository.

**Error tests:** Files whose names end in `_rejected.fuse` or `_error.fuse`
are expected to produce a compiler error, not execute successfully. The test
runner verifies that the error is produced and that its message matches the
expected snapshot.

---

### `stdlib/`

Standard library definitions written in Fuse. Core stdlib (`stdlib/core/`) is
available to the Stage 0 interpreter. Full stdlib (`stdlib/full/`) is available
from Stage 1 onward.

These files are source of truth for the standard library API. The Stage 0
interpreter may implement them natively in Python for performance; the Stage 1
compiler compiles them as ordinary Fuse source. The API must be identical.

---

### `examples/`

Standalone Fuse programs intended for learning and manual testing. Not part of
the automated test suite. These are the programs a new developer or AI agent
would read first to understand how Fuse feels to write.

`four_functions.fuse` here is the narrative version of the canonical example —
annotated with comments explaining each feature. The copy in `tests/fuse/milestone/`
is the clean version used for automated testing.

---

### `stage0/`

The Python tree-walking interpreter. Implements Fuse Core only.

**Entry point:** `python src/main.py <file.fuse>`

**Key files:**

| File | Responsibility |
|---|---|
| `lexer.py` | Converts source text to a flat token stream |
| `parser.py` | Converts token stream to an AST using recursive descent |
| `ast.py` | Dataclass definitions for every AST node |
| `checker.py` | Enforces ownership rules and match exhaustiveness before evaluation |
| `evaluator.py` | Walks the AST and produces a result value |
| `values.py` | Python representations of every Fuse runtime value |

**Stage 0 milestone:** `python src/main.py ../../tests/fuse/milestone/four_functions.fuse`
executes without error and produces the expected output.

---

### `stage1/`

The Rust compiler. A Cargo workspace with two crates.

**`fusec/`** — the compiler. Takes a `.fuse` source file, runs it through
lexer → parser → AST → HIR → checker → codegen, and emits a native binary
via Cranelift.

**`fuse-runtime/`** — the runtime library linked into every compiled Fuse
program. Provides ASAP destruction bookkeeping, `Chan<T>`, `Shared<T>`, and
the async executor.

**Entry point:** `cargo run --bin fusec -- <file.fuse>`

**Stage 1 milestone:** `fusec tests/fuse/milestone/four_functions.fuse` compiles
and runs correctly. The full `tests/fuse/` suite passes. The Stage 0 snapshot
suite passes against Stage 1 output.

---

### `stage2/`

Placeholder. The self-hosting Fuse compiler written in Fuse Core.

**Entry condition:** The Stage 1 milestone is met and the language is stable
enough that writing a compiler in it is practical — meaning the compiler would
not need features not yet in the language.

**Stage 2 milestone:** `fusec stage2/src/main.fuse` compiles a Fuse program
using only the Stage 2 compiler, without invoking the Stage 1 Rust binary.

---

## Conventions

### File naming

| Pattern | Meaning |
|---|---|
| `*_rejected.fuse` | Program that must fail with a compile error |
| `*_warning.fuse` | Program that must produce a specific compile warning |
| `*_test.fuse` | Executable test — checked against a snapshot |
| `*_tour.fuse` | Annotated example for learning — not in automated suite |

### Snapshot format

Each test file in `tests/fuse/` has a corresponding snapshot in the stage's
`tests/snapshots/` directory. Snapshots are plain text files containing the
exact expected stdout output, or for error tests, the expected error message.

```
tests/fuse/core/ownership/ref_read_only.fuse
stage0/tests/snapshots/core/ownership/ref_read_only.txt
stage1/tests/snapshots/core/ownership/ref_read_only.txt
```

Stage 0 and Stage 1 snapshots must be identical for all Core tests. If they
differ, Stage 1 has a bug.

### Adding a new language feature

1. Update `docs/guide/fuse-language-guide.md` with the concept, rationale,
   and code example.
2. Add an ADR to `docs/adr/` if it records a non-obvious design choice.
3. Add test cases to `tests/fuse/core/` or `tests/fuse/full/`.
4. Add snapshots to `stage0/tests/snapshots/` (or `stage1/` if Full-only).
5. Implement in Stage 0 first. Verify tests pass.
6. Implement in Stage 1. Verify the same tests pass.

The guide is updated before the implementation. A feature without a guide
entry does not exist yet.

---

## Relationship Between Repositories and Stages

```
fuse/                          ← this repository
│
├── docs/        ←─────────────── read by humans and AI agents at all stages
├── tests/fuse/  ←─────────────── executed by stage0, stage1, stage2
├── stdlib/      ←─────────────── interpreted by stage0, compiled by stage1+
├── examples/    ←─────────────── read by developers and AI agents
│
├── stage0/      ←─────────────── Python; runs tests/fuse/core/
├── stage1/      ←─────────────── Rust + Cranelift; runs tests/fuse/ (all)
└── stage2/      ←─────────────── Fuse; runs tests/fuse/ (all)

The stages share no source code with each other.
They share: tests, stdlib definitions, examples, documentation.
```

---

## Quick Reference: Where Things Live

| I want to... | Go to... |
|---|---|
| Understand the language | `docs/guide/fuse-language-guide.md` |
| Find the canonical example | `tests/fuse/milestone/four_functions.fuse` |
| Understand a design decision | `docs/adr/ADR-NNN-*.md` |
| Run the interpreter | `stage0/` |
| Build the compiler | `stage1/` |
| See stdlib API | `stdlib/core/` or `stdlib/full/` |
| Add a test | `tests/fuse/core/` or `tests/fuse/full/` |
| Read an annotated example | `examples/` |

---

*End of Fuse Repository Layout*

---

> **For AI agents:** The single most important file in this repository is
> `tests/fuse/milestone/four_functions.fuse`. Stage 0 is complete when
> that file runs. Stage 1 is complete when that file compiles to a native
> binary. All ownership and concurrency semantics are documented in
> `docs/guide/fuse-language-guide.md`. New features always start with
> a guide update before any implementation.
