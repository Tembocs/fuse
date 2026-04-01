# Fuse Language Guide

> **For AI agents reading this document:** This is a language specification guide for
> Fuse, a conceptual programming language. Each section is self-contained. Key decisions
> are recorded as ADRs at the end. Code blocks are tagged `fuse`. Sections follow the
> pattern: concept → rationale → code example → summary rule.

---

## Table of Contents

1. [What Is Fuse](#1-what-is-fuse)
2. [Language DNA](#2-language-dna)
3. [Fuse Core and Fuse Full](#3-fuse-core-and-fuse-full)
4. [Syntax Foundations](#4-syntax-foundations)
5. [Ownership: The Convention Family](#5-ownership-the-convention-family)
6. [Memory Model](#6-memory-model)
7. [Error Handling](#7-error-handling)
8. [Concurrency: Three Tiers](#8-concurrency-three-tiers)
9. [Race Condition Prevention](#9-race-condition-prevention)
10. [Deadlock Prevention](#10-deadlock-prevention)
11. [Putting It Together](#11-putting-it-together)
12. [Development Stages](#12-development-stages)
13. [Design Decisions](#13-design-decisions)

---

## 1. What Is Fuse

Fuse is a general-purpose programming language designed around three non-negotiable
properties:

- **Memory safety without a garbage collector.** Memory is reclaimed deterministically
  at the last use of a value, not at an arbitrary collection cycle.
- **Concurrency safety without a borrow checker.** Ownership is declared through
  four readable keywords. The compiler enforces the rules; the developer does not
  negotiate with a lifetime system.
- **Developer experience as a first-class concern.** Every keyword, annotation, and
  error message is chosen so that reading code aloud produces a correct description
  of what it does.

Fuse is not a research language. It is designed to be implemented, self-hosted, and
used to build production systems.

### The one-sentence version

> Fuse is a statically typed, compiled language that combines the best ownership model
> of Mojo, the error handling of Rust, the concurrency model of Go, the null safety of
> Kotlin, the async story of C#, the readability of Python, and the type expressiveness
> of TypeScript — with a developer experience that ties them together into a single
> coherent whole.

---

## 2. Language DNA

Fuse does not invent new concepts. It selects the best-proven idea from each source
language and integrates them so they feel like one language, not six bolted together.

| Source | What Fuse takes |
|---|---|
| **Mojo** | `owned`/`mutref`/`ref` argument conventions, ASAP destruction, `@value` auto-lifecycle, `SIMD<T,N>` primitives |
| **Rust** | `Result<T,E>`, `Option<T>`, `?` error propagation, exhaustive `match` |
| **Kotlin** | `val`/`var` type inference, Elvis operator `?:`, optional chaining `?.`, `data class`, `suspend`, scope functions (`let`, `also`, `takeIf`) |
| **C#** | `async`/`await`, LINQ-style method chains (`.map`, `.filter`, `.sorted`) |
| **Python** | `f"..."` string interpolation, `[x for x in y if ...]` list comprehensions, `@decorator` syntax |
| **Go** | `spawn` (goroutines), `defer`, typed channels (`Chan<T>`) |
| **TypeScript** | Union types (`A \| B \| C`), optional chaining `?.`, `interface` constraints |

Every feature in this table has been proven in production at scale. Fuse does not
experiment — it integrates.

---

## 3. Fuse Core and Fuse Full

Fuse is defined in two layers. This distinction matters for implementation order
and for understanding which features are foundational.

### Fuse Core

The minimal subset of the language sufficient to write a compiler. A single-threaded
data transformation pipeline needs no concurrency primitives. Core is complete for
building real programs — it just does not yet include parallel execution.

**Included in Core:**

- `fn`, `struct`, `data class`, `enum`, `@value`, `@entrypoint`
- `val`, `var`, type inference
- `ref`, `mutref`, `owned`, `move` — the full ownership model
- `Result<T,E>`, `Option<T>`, `match`, `when`, `?`
- `if`/`else`, `for`/`in`, `loop`, `return`
- `List<T>`, `String`, `Int`, `Float`, `Bool`
- `f"..."` interpolation, `?.` chaining, `?:` Elvis
- `defer`
- Extension functions
- Expression-body functions (`fn foo() => expr`)
- Block expressions (`val x = { ... }`)
- Integer division (truncating), float division

**Not in Core (added in Fuse Full):**

- `spawn`, `Chan<T>`, `Shared<T>`, `@rank`
- `async`/`await`, `suspend`
- `SIMD<T,N>`
- `interface` / `trait` polymorphism

### Fuse Full

Everything in Core plus the concurrency and performance primitives. The language
as described in this guide in its entirety.

> **Rule:** Implement Core first. A working Core interpreter validates the language
> design before concurrency complexity is introduced.

### Fuse Core Stability

**Fuse Core is stable.** The Stage 0 Python interpreter implements the complete
Core feature set. The 26-test Core suite plus the `four_functions.fuse` milestone
program pass with correct output. The semantics described in this guide are the
contract that Stage 1 implements.

The following clarifications were established during Stage 0 implementation:

- **`data` is a contextual keyword.** It is only special when followed by `class`.
  It may be used as a variable name, parameter name, or field name. See ADR-009.
- **Integer division truncates toward negative infinity** (Python `//` semantics).
  `10 / 3` evaluates to `3`. Float division (`10.0 / 3.0`) produces a float.
- **ASAP destruction is last-use scoped within the enclosing function body.**
  `__del__` fires immediately after the statement containing the last reference.
  Deferred callbacks fire after all ASAP destruction, at function exit, in
  reverse registration order. See ADR-010.
- **`self` parameter type annotations are optional.** When omitted, the type
  is inferred as `Self`. This applies to methods and `__del__`.
- **Block expressions** return the value of their last expression statement.
  `val x = { val a = 1; val b = 2; a + b }` binds `x` to `3`.
- **`enum` is part of Fuse Core.** It was omitted from the original Core list
  but is required by `Result<T,E>`, `Option<T>`, and the milestone program.

---

## 4. Syntax Foundations

### Variables

```fuse
val name   = "Amara"           // immutable — cannot be reassigned
var count  = 0                 // mutable — can be reassigned
val score: Float = 98.6        // explicit type annotation when needed
```

`val` is the default. Reach for `var` only when the value changes after assignment.
The compiler infers types; annotations are optional and used for documentation clarity.

### Functions

```fuse
fn add(ref a: Int, ref b: Int) -> Int {
  a + b    // last expression is the return value — no `return` keyword needed
}

// expression body for single-line functions
fn double(ref n: Int) -> Int => n * 2
```

### Structs and Data Classes

```fuse
// @value auto-generates: copy constructor, move constructor, destructor
// No manual implementation needed
@value
struct User {
  val id       : Int
  val username : String
  val profile  : Option<Profile>
  val metrics  : List<Metric>
}

// data class: @value + structural equality + readable toString
@value
data class Summary(
  val avg    : Float,
  val peak   : Float,
  val status : Status
)
```

`@value` is the standard annotation for any struct that should behave as a plain
value — copyable, movable, and automatically destroyed at last use.

### Pattern Matching

```fuse
// match is exhaustive — the compiler rejects missing cases
match response.status {
  200        => Ok(response.json<User>()?)
  404        => Err(NetworkError.NotFound(id))
  _          => Err(NetworkError.Http(response.status))
}

// match on tuples
match (language, title) {
  ("sw", Some(t)) => f"Karibu, {t} {name}!"
  ("sw", None)    => f"Karibu, {name}!"
  (_,    Some(t)) => f"Welcome, {t} {name}."
  _               => f"Hello, {name}."
}

// when — expression form of match for conditions
val status = when {
  avg <  50.0  => Status.Ok
  avg < 200.0  => Status.Warn("elevated")
  else         => Status.Err("critical threshold")
}
```

---

## 5. Ownership: The Convention Family

Ownership in Fuse is declared through four keywords that form a single coherent
family. Reading them left to right describes a spectrum of increasing commitment
to a value.

```
ref  →  mutref  →  owned  →  move
read it  change it  own it  transfer it
```

The `ref` prefix is the shared root. `ref` reads a value. `mutref` is `ref` plus
mutation. The relationship between them is visible in the names — no concept needs
to be learned that the names do not already teach.

### `ref` — read it

```fuse
// ref: read-only view, zero cost, no ownership change
// Multiple concurrent refs to the same value are always safe
fn greetUser(ref self: User) -> String {
  val name = self.profile?.displayName ?: self.username
  f"Hello, {name}."
}

// ref is the default — can be omitted when obvious
fn fetchUser(ref id: Int) -> Result<User, NetworkError> { ... }
```

### `mutref` — change it, not own it

```fuse
// mutref: mutable in-place reference
// Caller's value is modified directly — no copy, no ownership transfer
// Replaces Mojo's `inout` with a self-documenting name
fn processMetrics(mutref data: List<Metric>) -> Summary {
  data.retainWhere { m => m.value > 0.0 and m.value < 1000.0 }
  // `data` in the caller is now filtered
  Summary(data.average(), data.max(), Status.Ok)
}

// at the call site, mutref is explicit
val summary = processMetrics(mutref user.metrics)
//                           ^^^^^^ signals to reader: this will be modified
```

### `owned` — own it

```fuse
// owned: function declares it takes full ownership of this value
// The value will be destroyed inside the function unless moved further
fn shutdown(owned conn: Connection) {
  conn.takeIf { it.isHealthy }
      ?.also  { it.flushPending() }
  // conn is destroyed here — Connection.__del__ fires at last use
}
```

### `move` — transfer it, now

```fuse
// move: call-site keyword that transfers ownership
// After `move conn`, the compiler forbids any use of conn in this scope
shutdown(move conn)
//       ^^^^ ownership transferred — conn is gone from this point
```

### Convention summary

| Convention | Where written | Mutates caller | Transfers ownership | Cost |
|---|---|---|---|---|
| `ref` | parameter | no | no | zero |
| `mutref` | parameter + call site | yes | no | zero |
| `owned` | parameter | — | callee decides | move or copy |
| `move` | call site only | — | yes, enforced by compiler | zero |

> **Rule:** `ref` is the default. Use `mutref` when the function must modify the
> caller's value in place. Use `owned` + `move` when ownership must transfer.

---

## 6. Memory Model

### ASAP Destruction

Every value in Fuse is destroyed at its **last use**, not at the end of its
enclosing scope and not by a garbage collector.

```fuse
fn fetchUser(ref id: Int) -> Result<User, NetworkError> {
  val resp = await Http.get(f"/users/{id}")?

  val body = match resp.status {
    200 => resp.json<User>()?
    404 => return Err(NetworkError.NotFound(id))
    _   => return Err(NetworkError.Http(resp.status))
  }

  Ok(body)
  // resp is destroyed HERE — at its last use on the match line above
  // body is moved into Ok(...) and ownership transfers to the caller
  // No GC pause. No scope-end cleanup. No defer needed for resp.
}
```

ASAP destruction means:
- Lock guards release immediately when you stop using them — no forgotten unlocks
- Memory is reclaimed at the earliest possible point — no bloat
- Destruction order is predictable and readable from the code

### `@value` — automatic lifecycle

```fuse
@value
struct Connection {
  val dsn     : String
  var pending : Int

  // __del__ is called at the last use of any Connection value
  // deterministic — never called by a GC, always called exactly once
  fn __del__(owned self) {
    logShutdown(self.dsn)
    self.pending  // pending is 0 at clean shutdown
  }
}
```

`@value` generates `__copyinit__`, `__moveinit__`, and `__del__` automatically.
Define only `__del__` when you need a custom cleanup action. The rest is handled.

### No borrow checker

There is no lifetime annotation system in Fuse. The ownership conventions
(`ref`, `mutref`, `owned`, `move`) carry all the information the compiler
needs to enforce safety. You declare intent; the compiler verifies it.

> **Rule:** No GC. No borrow checker. Values are destroyed at last use.
> `@value` handles lifecycle automatically. Write `__del__` only for
> custom teardown logic.

---

## 7. Error Handling

### `Result<T, E>` — fallible operations

```fuse
// Every function that can fail returns Result<T, E>
// The compiler will not let you ignore an unhandled Result
fn divide(ref a: Float, ref b: Float) -> Result<Float, MathError> {
  if b == 0.0 {
    return Err(MathError.DivisionByZero)
  }
  Ok(a / b)
}
```

### `?` — propagate errors upward

```fuse
// ? unwraps Ok(value) or immediately returns Err(e) from the current function
// Chains of fallible operations become a single readable pipeline
suspend async fn loadDashboard(ref userId: Int) -> Result<Dashboard, AppError> {
  val user    = await fetchUser(userId)?       // returns early on Err
  val prefs   = await loadPrefs(ref user)?     // returns early on Err
  val metrics = await fetchMetrics(ref user)?  // returns early on Err
  Ok(Dashboard(user, prefs, metrics))
}
```

### `Option<T>` — nullable values

```fuse
// Option<T> = Some(value) | None
// There is no null in Fuse — absence is explicit in the type
val displayName: Option<String> = user.profile?.displayName

// Elvis operator: unwrap or use fallback
val name = user.profile?.displayName ?: user.username

// Optional chaining: short-circuits to None if any link is None
val language = user.profile?.locale?.language() ?: "en"
```

### Exhaustive `match`

```fuse
// The compiler rejects match expressions with missing arms
// You cannot accidentally ignore an error case
match result {
  Ok(value)               => process(value)
  Err(AppError.NotFound)  => showEmpty()
  Err(AppError.Auth(msg)) => redirect(f"/login?reason={msg}")
  Err(e)                  => logAndAbort(e)
}
```

> **Rule:** Every fallible function returns `Result<T,E>`. Every nullable value
> is `Option<T>`. `match` is exhaustive. There is no null, no unchecked exception,
> and no silent failure in Fuse.

---

## 8. Concurrency: Three Tiers

Fuse's concurrency model is a mandatory hierarchy. The tiers are not alternatives —
they form a decision path. Start at Tier 1. Move to Tier 2 only when Tier 1 is
not sufficient. Tier 3 is a last resort.

```
Does data need to flow between tasks?
  └─ Yes → Tier 1: Chan<T>           (preferred, zero locks)

Must multiple tasks share a live mutable value?
  └─ Yes → Tier 2: Shared<T> + @rank  (compile-time safe)

Is the lock order truly dynamic?
  └─ Yes → Tier 3: try_write(timeout) (explicit, handled via Result)
```

### Tier 1 — Channels (preferred)

```fuse
// Chan<T>: typed, directional, no shared state, no locks
// Deadlock is geometrically impossible when there are no locks to cycle over
val (tx, rx) = Chan::<Summary>.bounded(1)

// worker owns its sender — fully isolated
spawn async {
  val batch = await metricRx.recvBatch(100)
  var data  = move batch
  tx.send(move processMetrics(mutref data))
}

// main receives the result
val summary = await rx.recv()?
```

### Tier 2 — `Shared<T>` with `@rank` (when sharing is necessary)

```fuse
// @rank is MANDATORY on every Shared<T> — compile error without it
// Rank must be acquired in strictly ascending order — cycles are impossible
@rank(1) val config  = Shared::new(Config.load())
@rank(2) val db      = Shared::new(Db.open("db://localhost"))
@rank(3) val metrics = Shared::new(Vec<Metric>.new())

fn update() {
  val ref    cfg  = config.read()    // rank 1 acquired
  val mutref conn = db.write()       // rank 2 > 1 ✅
  val mutref m    = metrics.write()  // rank 3 > 2 ✅
  m.push(fetchRow(ref conn, ref cfg))
  // guards released in reverse — ASAP at each last use
  // no explicit unlock — destruction handles it
}

fn broken() {
  val mutref m    = metrics.write()  // rank 3 acquired
  val mutref conn = db.write()       // rank 2 < 3
  //               ^ compile error: cannot acquire @rank(2) while holding @rank(3)
  //                 acquire `db` before `metrics`, or release `metrics` first
}
```

### Tier 3 — `try_write(timeout)` (dynamic lock order)

```fuse
// Use when lock order is not known at compile time
// Return type Result<Guard, LockError> makes the risk explicit
// The caller must handle timeout — it cannot be silently ignored
fn dynamicUpdate(ref resources: List<Shared<Resource>>) -> Result<(), LockError> {
  val guards = [
    r.try_write(Timeout.ms(50))?   // ? propagates LockError::Timeout
    for r in resources
  ]
  for mutref g in guards { g.flush() }
  // all guards released here — ASAP
  Ok(())
}

// at the call site, the timeout case is handled explicitly
match dynamicUpdate(ref pool) {
  Ok(())                        => println("flushed")
  Err(LockError.Timeout(rank))  => retry(Backoff.exp())
  Err(e)                        => eprintln(f"lock failed: {e}")
}
```

> **Rule:** Channels first. `Shared<T>` when sharing is unavoidable. `@rank` is
> mandatory on every `Shared<T>` — not optional, not a warning. `try_write` when
> static ordering is impossible.

---

## 9. Race Condition Prevention

A race condition occurs when two tasks access the same memory concurrently and at
least one access is a write. Fuse prevents this through two compile-time rules.

### Spawn capture rule

A spawned task may only capture values through `move` or `ref`.
Capturing a `mutref` across a `spawn` boundary is a compile error.

```fuse
val count = Shared::new(0)

// ✅ move: task takes sole ownership of its copy
spawn move || { process(count.clone()) }

// ✅ ref: read-only, any number of concurrent readers is safe
spawn ref  || { println(count.read()) }

// ❌ compile error: mutref cannot cross a thread boundary
spawn || { mutate(mutref count) }
//    ^ error: `mutref` capture is not permitted across spawn boundary
//      use `Shared<T>` for shared mutable state across tasks
```

### `Shared<T>` — safe shared mutation

When genuine cross-task mutation is needed, the value must be wrapped in
`Shared<T>`. Acquiring write access returns a guarded `mutref`. ASAP
destruction releases the lock at the last use — no manual unlock.

```fuse
@rank(3) val metrics = Shared::new(Vec<Metric>.new())

suspend async fn heartbeat(owned metrics: Shared<Vec<Metric>>) {
  loop {
    await Timer.sleep(1000)

    // write() blocks until no other task holds a write guard
    val mutref guard = metrics.write()    // lock acquired
    guard.push(Metric.new(sampleCpu()))
    // guard destroyed here — ASAP, lock released immediately
    // any tasks waiting on write() or read() can now proceed
  }
}
```

### Race condition coverage

| Scenario | Prevention | Enforcement |
|---|---|---|
| Two tasks, same owned value | `move` — second task cannot compile | compile error |
| Task reads + task writes concurrently | `Shared<T>` RwLock — readers block during write | runtime |
| `mutref` captured across `spawn` | Spawn capture rule | compile error |
| Concurrent reads, no writes | `ref` is inherently shareable | zero cost |
| Forgotten unlock | ASAP destruction — guard releases at last use | impossible |

---

## 10. Deadlock Prevention

A deadlock is a cycle: task A waits for task B, task B waits for task A.
A strict ordering makes cycles geometrically impossible.

### `@rank` is mandatory — not optional

```fuse
// Without @rank: compile error
val metrics = Shared::new(Vec<Metric>.new())
//           ^ error: Shared<T> requires @rank(N) annotation
//             deadlock safety cannot be guaranteed without it
//             hint: add @rank(1) if this is your only shared resource

// With @rank: the compiler tracks acquisition order forever
@rank(1) val config  = Shared::new(Config.load())
@rank(2) val db      = Shared::new(Db.open())
@rank(3) val metrics = Shared::new(Vec<Metric>.new())

// Same rank means independent — safe to acquire in any order
@rank(10) val cache   = Shared::new(LruCache.new(512))
@rank(10) val session = Shared::new(SessionMap.new())
```

### Why mandatory, not optional

Optional safety annotations get skipped under deadline pressure and added only
after a production incident. If `@rank` is a compile error when absent, the
language is never in an unguarded state. The cost is one integer, written once,
at the declaration site where the developer already understands the dependency
order.

### The async lock lint

Holding a write guard across an `await` point is a common source of subtle
deadlocks in async code: if the task suspends, any other task waiting on that
guard will never make progress until the suspended task resumes. Fuse emits a
compile warning when a write guard is live at an `await` point.

```fuse
async fn broken() {
  val mutref guard = db.write()    // write guard acquired
  val result = await http.get(url) // ⚠ warning: write guard held across await
  guard.update(result)             //   another task waiting on db.write() will
}                                  //   be blocked for the entire await duration

// fix: narrow the write scope
async fn correct() {
  val result = await http.get(url)  // fetch first
  val mutref guard = db.write()     // then write
  guard.update(result)
  // guard released here — ASAP
}
```

### Deadlock prevention coverage

| Scenario | How prevented | When |
|---|---|---|
| Multiple `Shared<T>`, static order known | `@rank` enforced ordering | compile error |
| No shared state | Channels — no locks to cycle | not possible |
| Dynamic lock order | `try_write(timeout)` + `Result` | runtime detection |
| Write guard held across `await` | Compiler lint | compile warning |
| Forgotten unlock | ASAP destruction — always active | impossible |
| Circular deadlock (A waits B, B waits A) | `@rank` — lower rank cannot be acquired while holding higher | compile error |

---

## 11. Putting It Together

The following four functions demonstrate the full language as designed. They are
the canonical Fuse example and serve as the reference implementation target for
the Stage 0 interpreter.

```fuse
// Type definitions
// ─────────────────────────────────────────────────────────────────────────────

@value
struct User {
  val id       : Int
  val username : String
  val profile  : Option<Profile>
  val metrics  : List<Metric>
}

@value
struct Connection {
  val dsn     : String
  var pending : Int

  fn __del__(owned self) { logShutdown(self.dsn) }
}

@value
data class Summary(val avg: Float, val peak: Float, val status: Status)


// Entry point
// ─────────────────────────────────────────────────────────────────────────────

@entrypoint
async fn main() {

  // background task — move its channel sender in, fully isolated
  val (metricTx, metricRx)   = Chan::<Metric>.unbounded()
  val (summaryTx, summaryRx) = Chan::<Summary>.bounded(1)

  spawn heartbeat(move metricTx)

  spawn async {
    val batch = await metricRx.recvBatch(100)
    var data  = move batch
    summaryTx.send(move processMetrics(mutref data))
  }

  val user     = await fetchUser(42)?
  val summary  = await summaryRx.recv()?
  val greeting = greetUser(user)

  match summary.status {
    Status.Ok      => println(greeting)
    Status.Warn(w) => println(f"Warning: {w}")
    Status.Err(e)  => eprintln(f"Fatal: {e}")
  }

  shutdown(move conn)
}


// Function 1: fetch a user over the network
// ─────────────────────────────────────────────────────────────────────────────
// Demonstrates: suspend async, ref, Result, match, ?, ASAP destruction

suspend async fn fetchUser(ref id: Int) -> Result<User, NetworkError> {
  val resp = await Http.get(f"/users/{id}")?

  val body = match resp.status {
    200 => resp.json<User>()?
    404 => return Err(NetworkError.NotFound(id))
    _   => return Err(NetworkError.Http(resp.status))
  }

  Ok(body)
  // resp destroyed here — ASAP at last use, no defer needed
}


// Function 2: process a list of metrics
// ─────────────────────────────────────────────────────────────────────────────
// Demonstrates: mutref, LINQ chains, SIMD, when expression, data class

fn processMetrics(mutref data: List<Metric>) -> Summary {
  // mutref: filter the caller's list in place — no copy
  data.retainWhere { m => m.value > 0.0 and m.value < 1000.0 }

  val values = data.map { m => m.value }

  // SIMD vectorised sum — Mojo hardware primitive
  val avg  = SIMD<Float32, 8>.sum(values) / values.len().toFloat()
  val peak = values.sorted().last()

  val status: Status.Ok | Status.Warn | Status.Err = when {
    avg <  50.0  => Status.Ok
    avg < 200.0  => Status.Warn("elevated")
    else         => Status.Err("critical threshold")
  }

  Summary(avg, peak, status)
}


// Function 3: produce a localised greeting
// ─────────────────────────────────────────────────────────────────────────────
// Demonstrates: extension fn, ref self, optional chaining, Elvis, f-strings, match

fn User.greetUser(ref self) -> String {
  val name  = self.profile?.displayName ?: self.username
  val lang  = self.profile?.locale?.language() ?: "en"
  val title = self.role?.toTitle()

  match (lang, title) {
    ("sw", Some(t)) => f"Karibu, {t} {name}!"
    ("sw", None)    => f"Karibu, {name}!"
    (_,    Some(t)) => f"Welcome, {t} {name}."
    _               => f"Hello, {name}."
  }
}


// Function 4: shut down a database connection
// ─────────────────────────────────────────────────────────────────────────────
// Demonstrates: owned, move (call site), defer, scope functions, unwrap_or
// Note: Connection.__del__ fires at conn's last use — no explicit close() needed

fn shutdown(owned conn: Connection) {
  defer println("Shutdown sequence complete")

  conn.takeIf { it.isHealthy }
      ?.also  { it.flushPending() }
      ?.let   { println(f"Flushed {it.pending} ops") }

  val msg = conn.statusMsg().unwrap_or("clean exit")
  println(f"Shutdown: {msg}")

  // conn destroyed here — __del__ fires, logShutdown runs
  // defer fires last: "Shutdown sequence complete"
}
```

---

## 12. Development Stages

Fuse is implemented in three stages. Each stage has a clear entry condition,
a clear output, and a clear milestone that determines readiness to advance.

No stage has a deadline. Each stage is complete when its milestone is met.

### Fuse Core — the implementation target for Stage 0

Stage 0 implements only Fuse Core. Concurrency is not required to build a
compiler. Establishing correct semantics is the only goal of Stage 0.

**Fuse Core includes:** `fn`, `struct`, `@value`, `val`/`var`, `ref`/`mutref`/`owned`/`move`,
`Result<T,E>`, `Option<T>`, `match`, `when`, `?`, `List<T>`, `String`, `Int`, `Float`,
`Bool`, `f"..."`, `?.`, `?:`, `defer`, extension functions.

**Fuse Core excludes:** `spawn`, `Chan<T>`, `Shared<T>`, `@rank`, `async`/`await`,
`suspend`, `SIMD<T,N>`, `interface`/`trait`.

---

### Stage 0 — Python tree-walking interpreter

**Purpose:** Validate language semantics. Does `ref`/`mutref` enforce correctly?
Does `match` exhaust correctly? Does `@rank` catch ordering violations? None of
these questions require a native compiler. A Python interpreter answers them in
days, not months.

**What to build:**

```
Lexer → Parser → AST → Tree-walking Evaluator
```

**Why Python:**

- Fastest time to a working interpreter
- Allows complete focus on language semantics, not code generation
- Python's dynamic nature makes AST evaluation natural to write
- The Python interpreter stays permanently as a reference implementation
  for testing against the Rust compiler

**Stage 0 milestone:** The four functions in Section 11 execute correctly in the
Python interpreter. `ref`/`mutref`/`owned`/`move` are enforced. `@rank` violations
are rejected. `match` exhaustiveness is checked.

---

### Stage 1 — Rust compiler with Cranelift backend

**Purpose:** Produce real native binaries. Semantics are proven in Stage 0.
Stage 1 is about performance and completeness.

**Why Rust:**

- Ideal for compiler infrastructure: pattern matching, algebraic types, strong
  tooling, excellent performance
- Matches Fuse's own systems-level ambitions
- The Fuse community will likely be comfortable reading and contributing to a
  Rust-based compiler

**Why Cranelift over LLVM:**

- Cranelift is significantly simpler than LLVM
- Designed as a code generation backend, not a full compiler infrastructure
- Faster compilation, simpler integration, sufficient for Stage 1
- LLVM can be added as an optional backend later for maximum optimisation

**What to add incrementally:**

1. Compile Fuse Core to native code via Cranelift
2. Add async runtime (tokio-compatible or custom)
3. Add `spawn`, `Chan<T>`, `Shared<T>`, `@rank` enforcement
4. Add `SIMD<T,N>` mapped to platform intrinsics

**Stage 1 milestone:** The Stage 0 Python interpreter's test suite passes
against the Rust-compiled binaries. The Rust compiler compiles a Fuse Core
program that implements a subset of the Fuse compiler itself.

---

### Stage 2 — Self-hosting: the Fuse compiler written in Fuse

**Purpose:** Write the Fuse compiler in Fuse Core. This is the milestone that
proves the language is complete and expressive enough to build production software.

**Why self-hosting matters:**

- Every improvement to the language becomes immediately available to the compiler
- The compiler becomes the largest, most real-world test of the language
- Self-hosting is the standard proof-of-completeness for a production language

**Precedents:** Rust's first compiler was written in OCaml. Go's first compiler
was in C. Kotlin compiled to JVM bytecode for years before a native backend.
None started by writing a self-hosting compiler. They started by getting the
language right.

**Stage 2 milestone:** The Fuse compiler compiles itself. The Rust compiler is
no longer required to build Fuse.

---

## 13. Design Decisions

Each decision is recorded in three lines: the decision, the rationale, and the
alternatives that were considered and rejected. No entry is longer than this.

---

**ADR-001** · `borrowed` renamed to `ref`

**Decision:** The read-only argument convention is named `ref`, not `borrowed`.

**Rationale:** `ref` and `mutref` share a visible prefix. A reader seeing both
for the first time understands the relationship immediately: `ref` reads,
`mutref` reads and modifies. `borrowed` has no such relationship to `mutref`
and requires learning two unrelated names for related concepts.

**Rejected:** `borrowed` (Mojo's original), `ro` (too abbreviated), `read` (too verbose for a convention keyword).

---

**ADR-002** · `inout` renamed to `mutref`

**Decision:** The mutable reference convention is named `mutref`, not `inout`.

**Rationale:** `mutref` is self-documenting: mutable reference. A developer
reads `mutref data` and knows two things — the value will not be copied and it
will be modified. `inout` is an audio-engineering term with no semantic precision
in a programming context.

**Rejected:** `inout` (Mojo's original), `mut` (implies ownership, not reference), `rw` (too abbreviated).

---

**ADR-003** · `^` transfer operator replaced by `move` keyword

**Decision:** Ownership transfer at the call site is written `move value`,
not `value^`.

**Rationale:** A keyword reads as intent. `shutdown(move conn)` says "move
ownership of conn into shutdown" — it can be read aloud correctly. `shutdown(conn^)`
requires a legend to understand. A prefix keyword also aligns with how all
other conventions are written — before the argument.

**Rejected:** `conn^` (Mojo's sigil), `give conn` (informal, non-standard), `transfer conn` (verbose), `own conn` (ambiguous direction).

---

**ADR-004** · `@rank` on `Shared<T>` is a compile error when absent, not a warning

**Decision:** Declaring a `Shared<T>` without an `@rank(N)` annotation is a
hard compile error.

**Rationale:** Optional safety annotations get skipped under deadline pressure
and are added only after a production incident. A compile error means the language
is never in an unguarded state. The cost is one integer, written once, at the
declaration site where the developer already has the most context about dependency
order.

**Rejected:** Lint warning (ignored under pressure), runtime detection (too late,
wrong layer), no enforcement (defeats the purpose of the tier system).

---

**ADR-005** · Deadlock prevention is a three-tier mandatory hierarchy

**Decision:** The concurrency model prescribes three tiers — channels, `@rank`,
`try_write` — in a mandatory decision path, not as interchangeable alternatives.

**Rationale:** Offering equivalent alternatives puts the cognitive burden of
deadlock safety on the developer at every decision point. A hierarchy with a
clear default (channels) and clear escalation criteria (shared mutation → `@rank`,
dynamic order → `try_write`) means the lowest-friction path is also the safest
path.

**Rejected:** Single approach (too restrictive), purely advisory guidance (no enforcement), runtime cycle detection (high cost, wrong layer for a systems language).

---

**ADR-006** · Stage 0 implemented in Python, not Rust

**Decision:** The first interpreter is a Python tree-walking evaluator.

**Rationale:** Stage 0 must answer one question: are the language semantics
correct? Python allows complete focus on that question without the overhead of
managing a code generation backend. The Python interpreter becomes the permanent
reference implementation against which the Rust compiler is tested.

**Rejected:** Rust from the start (correct long-term, wrong for Stage 0 — conflates semantic validation with code generation), TypeScript (good for parsing, less natural for evaluation), Haskell (precedent from Rust, but team familiarity with Python and Rust makes Python the better choice).

---

**ADR-007** · Cranelift chosen over LLVM for Stage 1

**Decision:** The Stage 1 Rust compiler uses Cranelift as its code generation backend.

**Rationale:** Cranelift is designed as a code generation backend — simpler API,
faster compile times, sufficient output quality for a Stage 1 compiler. LLVM's
full optimisation pipeline is not needed until Stage 2 or beyond. Cranelift
reduces integration complexity and lets Stage 1 focus on correctness over
maximum performance.

**Rejected:** LLVM (correct long-term, too much integration complexity for Stage 1), QBE (less mature, smaller community), custom backend (unnecessary work given Cranelift's quality).

---

**ADR-008** · No timelines in this document or in the project

**Decision:** No milestone has a date, sprint, or estimated duration attached to it.

**Rationale:** A language is complete when it is correct, not when a calendar
says it should be. Premature timelines on foundational work produce shortcuts
that become permanent. Each stage is complete when its stated milestone is met.

**Rejected:** Milestone dates (pressure without information), sprint planning
(wrong granularity for language design), versioned release schedule (premature
for a pre-Stage-1 language).

---

*End of Fuse Language Guide*

---

> **For AI agents:** The canonical test target is the four functions in Section 11.
> The ownership model is fully described in Section 5. The concurrency decision
> tree is in Section 8. All design rationale is in Section 13 as numbered ADRs.
> Fuse Core (the Stage 0 implementation target) is defined in Section 3.
