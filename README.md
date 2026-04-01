# Fuse

> Memory safety without a garbage collector.  
> Concurrency safety without a borrow checker.  
> Developer experience that makes both feel obvious.

Fuse is a statically typed, compiled, general-purpose programming language. It
draws the best-proven idea from seven existing languages and integrates them so
they read as one. The result is a language where the safest code is also the
most natural code to write.

**Current stage:** Language design complete. Stage 0 (Python interpreter) in active development.

---

## A taste of Fuse

```fuse
// Ownership is declared through four readable keywords.
// ref = read it. mutref = change it. owned = own it. move = transfer it.

@value
data class User(val id: Int, val name: String, val score: Float)

@value
data class Summary(val top: User, val count: Int, val status: Status)


// mutref: modify the caller's list in place — no copy, no clone
fn processUsers(mutref users: List<User>) -> Result<Summary, AppError> {
  users.retainWhere { u => u.score > 0.0 }

  val top = users
    .sorted  { a, b => b.score - a.score }
    .first() ?: return Err(AppError.Empty)

  val status = when {
    top.score > 90.0 => Status.Ok
    top.score > 50.0 => Status.Warn("scores are low")
    else             => Status.Err("critical — review required")
  }

  Ok(Summary(top, users.len(), status))
}


// greetUser reads user — borrowed, zero cost, no ownership change
fn User.greetUser(ref self) -> String {
  val title = self.role?.toTitle()
  match title {
    Some(t) => f"Welcome, {t} {self.name}."
    None    => f"Welcome, {self.name}."
  }
}


@entrypoint
async fn main() {
  // Channels: no shared state, no locks, no deadlock possible
  val (tx, rx) = Chan::<User>.unbounded()
  spawn loadUsers(move tx)             // move: ownership transferred to the task

  var users   = await rx.recvAll()?
  val summary = processUsers(mutref users)?

  match summary.status {
    Status.Ok      => println(summary.top.greetUser())
    Status.Warn(w) => println(f"Warning: {w}")
    Status.Err(e)  => eprintln(f"Fatal: {e}")
  }
}
```

No null pointer. No garbage collector pause. No lifetime annotations. No
unchecked error. The compiler rejects any missing `match` arm, any `mutref`
capture across a thread boundary, and any `Shared<T>` declared without an
ordering rank. Safety violations are compile errors, not runtime surprises.

---

## Three non-negotiable properties

**Memory safety without a garbage collector.**  
Values are destroyed at their last use — not at scope end, not by a GC cycle.
Deterministic, predictable, zero pause. `@value` generates the full lifecycle
automatically. `__del__` fires exactly once, exactly when the value is last
touched.

**Concurrency safety without a borrow checker.**  
Ownership is declared through four keywords: `ref`, `mutref`, `owned`, `move`.
The compiler enforces the rules. There are no lifetime parameters to annotate,
no borrow checker to negotiate with. The concurrency model is a three-tier
hierarchy — channels first, ranked shared state second, timeout-guarded dynamic
locking third.

**Developer experience as a first-class concern.**  
Every keyword is chosen so that reading code aloud produces a correct
description of what it does. `shutdown(move conn)` means "move conn into
shutdown." `processMetrics(mutref data)` means "process metrics, modifying
data in place." The language teaches its own semantics through its names.

---

## Language DNA

Fuse does not invent new concepts. It selects the best-proven idea from each
source and integrates them so they feel like one language.

| Source | What Fuse takes |
|---|---|
| **Mojo** | `ref`/`mutref`/`owned`/`move` argument conventions, ASAP destruction, `@value`, `SIMD<T,N>` |
| **Rust** | `Result<T,E>`, `Option<T>`, `?` propagation, exhaustive `match` |
| **Kotlin** | `val`/`var` inference, `?.` chaining, `?:` Elvis, `data class`, `suspend`, scope functions |
| **C#** | `async`/`await`, LINQ-style method chains |
| **Python** | `f"..."` interpolation, list comprehensions, `@decorator` syntax |
| **Go** | `spawn`, `defer`, typed channels `Chan<T>` |
| **TypeScript** | Union types `A \| B \| C`, optional chaining, `interface` constraints |

---

## Ownership in sixty seconds

The full ownership model is four keywords that form a spectrum:

```
ref  ──►  mutref  ──►  owned  ──►  move
read it   change it    own it      transfer it
```

`ref` and `mutref` share a prefix deliberately. A reader new to Fuse
understands the relationship immediately — no documentation required.

```fuse
fn readData(ref conn: Connection) -> String { ... }
//          ^^^
//          read-only view — zero cost, no ownership change

fn appendLog(mutref log: List<Entry>, ref entry: Entry) { ... }
//           ^^^^^^
//           modifies the caller's list in place — no copy

fn shutdown(owned conn: Connection) { ... }
//          ^^^^^
//          this function takes full ownership — conn will be destroyed here

shutdown(move conn)
//       ^^^^
//       transfers ownership — compiler forbids any use of conn after this line
```

---

## Concurrency in sixty seconds

Three tiers. Use the lowest tier that solves the problem.

**Tier 1 — Channels** (no locks, no deadlock possible):

```fuse
val (tx, rx) = Chan::<Result>.bounded(1)
spawn worker(move tx)
val result = await rx.recv()?
```

**Tier 2 — `Shared<T>` with mandatory `@rank`** (compile-time deadlock prevention):

```fuse
@rank(1) val config  = Shared::new(Config.load())
@rank(2) val metrics = Shared::new(Vec<Metric>.new())

// ranks must be acquired in ascending order — the compiler enforces this
val ref    cfg = config.read()     // rank 1
val mutref m   = metrics.write()   // rank 2 > 1 ✅
// reverse order → compile error, not a runtime surprise
```

**Tier 3 — `try_write(timeout)`** (for dynamic lock order — rare):

```fuse
match resource.try_write(Timeout.ms(50)) {
  Ok(mutref guard)             => guard.flush()
  Err(LockError.Timeout(_))   => retry(Backoff.exp())
}
```

`@rank` is mandatory — declaring `Shared<T>` without it is a compile error.
Optional safety annotations get skipped under deadline pressure; mandatory
ones do not.

---

## Error handling in sixty seconds

There is no null in Fuse. There are no unchecked exceptions.

```fuse
// Every fallible function returns Result<T, E>
fn divide(ref a: Float, ref b: Float) -> Result<Float, MathError> {
  if b == 0.0 { return Err(MathError.DivisionByZero) }
  Ok(a / b)
}

// ? propagates errors up — chains of fallible calls stay readable
async fn loadDashboard(ref userId: Int) -> Result<Dashboard, AppError> {
  val user    = await fetchUser(userId)?
  val metrics = await fetchMetrics(ref user)?
  Ok(Dashboard(user, metrics))
}

// match is exhaustive — the compiler rejects missing arms
match result {
  Ok(value)              => render(value)
  Err(AppError.NotFound) => show404()
  Err(e)                 => logAndAbort(e)
}
```

---

## Getting started

### Prerequisites

- Python 3.11 or later
- No third-party dependencies for Stage 0

### Run the interpreter

```bash
git clone https://github.com/your-org/fuse
cd fuse/stage0
python src/main.py ../examples/hello.fuse
```

### Run the REPL

```bash
python src/main.py --repl
```

### Run the test suite

```bash
python tests/run_tests.py
```

The Stage 0 interpreter implements **Fuse Core** — the full ownership model,
error handling, pattern matching, and type system. Concurrency primitives
(`spawn`, `Chan<T>`, `Shared<T>`) are part of Fuse Full and are implemented
in Stage 1.

### Run the canonical milestone program

```bash
python src/main.py ../tests/fuse/milestone/four_functions.fuse
```

This is the single program that defines Stage 0 completeness. When it runs
correctly, Stage 0 is done.

---

## Repository structure

```
fuse/
├── docs/                   language guide and design decisions
├── tests/fuse/             shared test suite — all stages run these
├── stdlib/                 standard library written in Fuse
├── examples/               annotated example programs
├── stage0/                 Python tree-walking interpreter (Fuse Core)
├── stage1/                 Rust compiler + Cranelift backend (Fuse Full)
└── stage2/                 self-hosting Fuse compiler (future)
```

Full layout with file-level annotations: [`docs/fuse-repository-layout.md`](docs/fuse-repository-layout.md)

---

## Documentation

| Document | What it covers |
|---|---|
| [`docs/guide/fuse-language-guide.md`](docs/guide/fuse-language-guide.md) | The complete language — ownership, memory, errors, concurrency, development stages |
| [`docs/fuse-repository-layout.md`](docs/fuse-repository-layout.md) | Every directory and file explained |
| [`docs/adr/`](docs/adr/) | Design decisions — what was chosen, why, and what was rejected |
| [`examples/`](examples/) | Annotated Fuse programs for learning |
| [`tests/fuse/milestone/four_functions.fuse`](tests/fuse/milestone/four_functions.fuse) | The canonical Fuse program — all key features in one file |

---

## Development stages

Fuse is built in three stages. Each stage is complete when its milestone is met.
There are no timelines — a stage is done when it is correct.

| Stage | Language | Target | Milestone |
|---|---|---|---|
| **0** | Python | Fuse Core | `four_functions.fuse` runs correctly in the interpreter |
| **1** | Rust + Cranelift | Fuse Full | `four_functions.fuse` compiles to a native binary; full test suite passes |
| **2** | Fuse | Self-hosting | Fuse compiler compiles itself; Rust compiler no longer required |

The Python interpreter (Stage 0) is permanently retained as the reference
implementation — it is the ground truth against which the Rust compiler (Stage 1)
is tested.

---

## Contributing

Fuse is in active design and early implementation. Contributions are welcome at
every level.

**Before writing code:**  
Read [`docs/guide/fuse-language-guide.md`](docs/guide/fuse-language-guide.md).
Every feature in Fuse has a documented rationale. Understanding the why prevents
implementing something that contradicts a deliberate decision.

**Adding a language feature:**  
1. Update the language guide with the concept, rationale, and a code example  
2. Add an ADR to `docs/adr/` if the design choice is non-obvious  
3. Add test cases to `tests/fuse/core/` or `tests/fuse/full/`  
4. Implement in Stage 0 first — verify tests pass  
5. Implement in Stage 1 — verify the same tests pass  

The guide is updated before the implementation. A feature without a guide
entry does not exist yet.

**Reporting issues:**  
If observed behaviour does not match the language guide, that is a bug in the
implementation. If the language guide itself is wrong or incomplete, open a
discussion — guide changes may require an ADR.

---

## Design principles

These principles are not aspirational — they are constraints. Every decision in
the language has been made against them.

**The safe path is the easy path.**  
`@rank` is mandatory. The spawn capture rule is a compile error. `match` is
exhaustive. Safety is enforced by the compiler before the developer can forget it.

**Names teach semantics.**  
`ref` and `mutref` share a prefix because they are related concepts. `move` is
a word, not a symbol, because code is read more than written. Every keyword was
chosen to make reading code aloud produce a correct description of its behaviour.

**The guide precedes the implementation.**  
No feature exists until it is documented. Documentation-first prevents
implementation drift and gives AI agents and human contributors a stable contract
to work against.

**No timelines.**  
A stage is complete when its milestone is met. Premature timelines on
foundational work produce shortcuts that become permanent.

---

## Status

| Component | Status |
|---|---|
| Language design | Complete |
| Language guide | Complete |
| Repository layout | Complete |
| Stage 0 — lexer | In progress |
| Stage 0 — parser | Not started |
| Stage 0 — evaluator | Not started |
| Stage 1 | Not started |
| Stage 2 | Not started |

---

*Fuse — fusing the best of seven languages into one.*
