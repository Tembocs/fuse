# Plan: fuse-language-guide-2.md

> This document captures what we agreed to write, why, and how it is organized.
> It exists so that the actual guide does not drift from intent during writing.

---

## Why a new guide

The original `fuse-language-guide.md` was a language design document. It described
what Fuse should be, but left the implementation to figure itself out. Gaps that
emerged during implementation:

1. **No `break` or `while`.** The language had `loop` but no way to exit it except
   `return`, which exits the entire function. This forced every loop into its own
   helper function and created patterns that broke the Cranelift codegen when
   functions grew large. The Stage 2 compiler (2000+ lines, one file) crashed the
   OS repeatedly because of this.

2. **No module system.** Stage 2 was a 2000-line monolith because there is no
   `import` statement. No way to split code across files.

3. **No code in the spec.** The guide described ownership and error handling in
   prose but did not show complete, runnable programs for every feature. AI agents
   implementing the compiler had to guess at edge cases.

4. **Implementation plan was separate.** `self_hosting.md` was written ad-hoc
   after the guide. It overlapped in places, contradicted in others, and the
   actual implementation diverged from both.

5. **Runtime internals undocumented.** How FuseValue works, how mutref ref-cells
   work, how the linker is invoked, how FFI passes arguments — all of this was
   reverse-engineered from code, never written down.

6. **Codegen bugs discovered too late.** Three bugs in Stage 1's Cranelift codegen
   were only found during Stage D (bootstrap). They should have been caught by
   tests much earlier. The guide needs to specify what to test and when.

---

## What changes in the language

### Added to Fuse Core

- **`break`** — exits the innermost `loop` or `while`. No value.
- **`continue`** — skips to the next iteration of the innermost `loop`, `while`, or `for`.
- **`while condition { body }`** — loop that runs while condition is true.
- **Module system** — `import` statements, file-based modules, visibility (`pub`).

### Unchanged

- Everything else in Fuse Core and Fuse Full stays as designed.
- `loop` remains (infinite loop, exited with `break` or `return`).
- `for x in list { ... }` remains.
- Ownership model (`ref`, `mutref`, `owned`, `move`) unchanged.
- Error handling (`Result`, `Option`, `?`, `match`) unchanged.
- Concurrency model unchanged.

---

## Stages (unchanged strategy)

| Stage | Language | Purpose |
|---|---|---|
| Stage 0 | Python | Tree-walking interpreter. Validates language design. |
| Stage 1 | Rust + Cranelift | Native compiler. Produces binaries. |
| Stage 2 | Fuse | Self-hosting. Fuse compiles itself. |

Stage 2 is written in Fuse Core (no concurrency/async). It uses FFI to call
Cranelift wrappers. The bootstrap chain: Stage 1 compiles Stage 2 into fusec2,
then fusec2 compiles itself.

---

## Document structure

The guide is organized so you can read it top to bottom and build the entire
project. Every section has code. No section refers to another document — this
is the single source of truth.

### Part 1: Language Specification

What Fuse IS. Every construct, with runnable code examples.

```
1.1  What is Fuse (philosophy, goals, non-goals)
1.2  Language DNA (what comes from which language)
1.3  Fuse Core vs Fuse Full (the two layers)
1.4  Variables and types (val, var, type inference, Int/Float/Bool/String)
1.5  Functions (fn, parameters, return types, expression body)
1.6  Control flow (if/else, for/in, while, loop, break, continue, return)
1.7  Data types (struct, data class, enum, List<T>, tuples)
1.8  Pattern matching (match, when, destructuring)
1.9  Ownership (ref, mutref, owned, move — the full model with code)
1.10 Memory model (ASAP destruction, __del__, defer)
1.11 Error handling (Result<T,E>, Option<T>, ?, match, propagation)
1.12 Modules and imports (file-based modules, import, pub, visibility)
1.13 Extension functions (fn Type.method syntax)
1.14 Generics (List<T>, Result<T,E>, type parameters)
1.15 String operations (f-strings, charAt, substring, split, etc.)
1.16 FFI (extern fn, Ptr type, calling C code)
1.17 Concurrency — Fuse Full (spawn, Chan<T>, Shared<T>, @rank)
1.18 Async — Fuse Full (async/await, suspend)
1.19 SIMD — Fuse Full (SIMD<T,N>)
```

Each subsection follows the pattern:
- What it is (one paragraph)
- Code example (complete, runnable)
- Rules (bullet list of compiler-enforced constraints)
- Edge cases (things that are easy to get wrong)

**Section 1.6 is new and critical.** It specifies:

```fuse
// while — runs while condition is true
var i = 0
while i < 10 {
  println(f"i = {i}")
  i = i + 1
}

// loop — infinite, exit with break or return
loop {
  val line = readLine()
  if line == "quit" { break }
  println(f"you said: {line}")
}

// break — exits innermost loop/while
for item in items {
  if item == target { break }
}

// continue — skips to next iteration
for item in items {
  if item.isEmpty() { continue }
  process(item)
}
```

**Section 1.12 is new.** It specifies the module system:

```fuse
// file: src/lexer/token.fuse
pub enum Tok {
  Fn, Val, Var, Ident(String), IntLit(Int), Eof
}

pub data class Token(val ty: Tok, val line: Int, val col: Int)

// file: src/parser/parser.fuse
import lexer.token.{Token, Tok}

fn parse(tokens: List<Token>) -> Program {
  // ...
}
```

Module resolution: `import a.b.c` maps to `src/a/b/c.fuse` relative to the
project root. `pub` makes items visible outside the module. Items without `pub`
are module-private.

### Part 2: Architecture

How the project is structured and how each stage works.

```
2.1  Repository layout (directory tree with annotations)
2.2  Stage 0 — Python interpreter (pipeline, what it covers, how to run)
2.3  Stage 1 — Rust compiler (pipeline, Cranelift, runtime, how to build)
2.4  Stage 2 — Self-hosting compiler (pipeline, FFI, bootstrap chain)
2.5  Compilation pipeline (Source → Lexer → Parser → AST → HIR → Codegen → Binary)
2.6  Runtime internals (FuseValue, ref cells, mutref writeback, ASAP destruction)
2.7  Linker integration (how object files become executables, platform differences)
```

Section 2.6 is critical. It documents:
- FuseValue enum (Int, Float, Bool, Str, List, Struct, Enum, Fn, Unit)
- How mutref works: ref cells (fuse_rt_ref_new, fuse_rt_ref_get, fuse_rt_ref_set)
- How field access works: fuse_rt_field, fuse_rt_set_field
- How ASAP destruction is implemented
- The C FFI calling convention (all values are i64 pointers)

### Part 3: Implementation Plan

Phase-by-phase build order with entry conditions, deliverables, and done-when.

```
3.1  Phase 1 — Test suite (write tests before code)
3.2  Phase 2 — Stage 0 lexer and parser
3.3  Phase 3 — Stage 0 ownership checker
3.4  Phase 4 — Stage 0 evaluator (milestone: four_functions.fuse)
3.5  Phase 5 — Language stabilization (freeze Core)
3.6  Phase 6 — Stage 1 frontend (Rust lexer, parser, checker)
3.7  Phase 7 — Stage 1 backend (HIR, Cranelift codegen, native binaries)
3.8  Phase 8 — Fuse Full (concurrency, async, SIMD)
3.9  Phase 9 — Stage 2 self-hosting (write compiler in Fuse, bootstrap, verify)
```

Each phase includes:
- Entry condition (what must be true before starting)
- Deliverables (specific files and functions)
- Done-when (testable criterion)
- Known pitfalls (bugs we discovered, patterns to avoid)

**Phase 7 known pitfalls (from this session):**

1. **mutref_cells must be set before compiling function body.** If set after,
   explicit `return` inside loops won't write back mutref parameters. The
   caller sees stale values. The loop appears to make no progress and runs
   forever.

2. **`and`/`or` short-circuit in large programs.** The Cranelift SSA variable
   tracking breaks when short-circuit boolean expressions (`and`/`or`) are used
   inside functions with loops and mutref parameters in programs with many
   functions. Workaround: extract `and`/`or` conditions into helper functions
   or use nested `if` chains. Root cause: likely a block-sealing issue in
   Cranelift variable resolution when many functions share one ObjectModule.
   Must be fixed before Stage D.

3. **UTF-8 byte vs character indexing.** `String.len()` returns byte length.
   `charAt(i)` uses byte indexing. Multi-byte characters (em-dash, emoji) cause
   panics if charAt hits a non-boundary byte. Fix: make charAt/substring
   boundary-safe.

4. **Stack size.** Compiled Fuse programs default to 1MB stack on Windows.
   Compilers need 8MB. Pass `/STACK:8388608` to the MSVC linker.

### Part 4: Test Suite

```
4.1  Test organization (core/, full/, milestone/, errors/)
4.2  Test contract (Stage 0 = Stage 1 = Stage 2 output)
4.3  How to run tests (commands for each stage)
4.4  How to add tests (naming, expected output format)
4.5  Error tests (EXPECTED ERROR marker, format matching)
```

### Part 5: Design Decisions (ADRs)

Preserved from the original guide. Each ADR with rationale.

---

## What this document replaces

Once `fuse-language-guide-2.md` is complete:

- `fuse-language-guide.md` — superseded (keep for history)
- `self_hosting.md` — absorbed into Part 3, Phase 9
- `fuse-implementation-plan.md` — absorbed into Part 3
- `fuse-repository-layout.md` — absorbed into Part 2

One document. No drift. No gaps.

---

## Constraints on writing

1. **Every language feature has a complete code example.** Not a snippet — a
   program you can save to a `.fuse` file and run.

2. **No forward references.** Section N does not depend on Section N+1. A reader
   (human or AI) can stop at any section and have complete knowledge up to that
   point.

3. **Implementation details are in Part 2-3, not Part 1.** The language spec
   describes what the programmer sees. The architecture and plan describe what
   the compiler builder does. They do not mix.

4. **Known bugs are documented where they matter.** Not in an appendix — in the
   phase where you will encounter them, with the fix.

5. **No timelines.** (ADR-008, unchanged.)
