# Fuse Self-Hosting Plan

> **For AI agents reading this document:**
> This is the comprehensive plan for making Fuse self-hosting (Phase 9 of the implementation plan). Every task is explicit and ordered. Entry conditions, deliverables, and done-when criteria follow the same structure as the main implementation plan. No timelines. Each step is complete when it is correct.

---

## Current state

Before laying out the plan, here is an honest assessment of where things stand.

**What exists:**

| Component | Status |
|---|---|
| Stage 0 (Python interpreter) | Complete — 26/26 Core tests, milestone passes |
| Stage 1 lexer, parser, AST | Complete — tokenizes and parses all Fuse Core and Full |
| Stage 1 checker | Complete — ownership, exhaustiveness, @rank, spawn, async lint |
| Stage 1 evaluator (eval.rs) | Complete — 1,163 lines, tree-walking interpreter in Rust |
| Stage 1 HIR | Stub — 1-line placeholder files |
| Stage 1 Cranelift codegen | Stub — 1-line placeholder files |
| Stage 1 runtime (fuse-runtime) | Partial — value.rs, builtins, list/string ops done; asap, async, chan, shared are stubs |
| Stage 2 interpreter (main.fuse) | 1,789-line proof-of-concept; cannot run (stack overflow) |

**What is missing for self-hosting:**

1. **No native code generation.** The Stage 1 compiler runs programs through a tree-walking evaluator. The `codegen/cranelift.rs`, `codegen/layout.rs`, `hir/nodes.rs`, and `hir/lower.rs` files are single-line comments. Phase 7 milestone was met via the evaluator path, not actual compilation to binaries.

2. **No FFI mechanism.** Fuse has no `extern`, `foreign`, or FFI keyword. The self-hosting compiler needs to call Cranelift (a Rust library) from Fuse code. This requires a language-level FFI design.

3. **No Stage 2 compiler.** What exists in `stage2/src/main.fuse` is a tree-walking interpreter, not a compiler. The bootstrap sequence requires a compiler that produces native binaries.

4. **Stack overflow in nested interpretation.** The Stage 2 interpreter uses recursion for all loops (Fuse has no `while`). When run inside the Stage 1 evaluator (also recursive), even `println("hi")` overflows the stack.

**Implication:** Self-hosting cannot begin until Stage 1 produces actual native binaries. The plan below addresses this prerequisite before tackling the self-hosting compiler itself.

---

## Plan structure

The self-hosting effort breaks into five stages, each with explicit entry conditions:

```
Stage A — Native codegen in Stage 1          (prerequisite)
Stage B — FFI and platform layer             (prerequisite)
Stage C — Write the Stage 2 compiler in Fuse (the work)
Stage D — Bootstrap and verify               (the proof)
Stage E — Toolchain foundation               (the future)
```

---

## Stage A — Native code generation in Stage 1

**One job:** Make `fusec file.fuse` produce a native binary, not evaluate via tree-walking.

### Why this comes first

The bootstrap sequence is: write fusec2 in Fuse → compile fusec2 with Stage 1 → get a native binary → use that binary to compile fusec2 again. If Stage 1 cannot produce binaries, the chain never starts. Every subsequent stage depends on this.

### Entry condition

Stage 1 evaluator passes all 36 integration tests (this is already true).

---

### A.1 — HIR node definitions

**File:** `stage1/fusec/src/hir/nodes.rs`

Define the high-level intermediate representation. HIR is the AST with all type information and ownership annotations made explicit. The checker operates on AST; codegen operates on HIR.

**Tasks:**

- A.1.1 — Define `HirType` enum: `Int`, `Float`, `Bool`, `Str`, `Unit`, `List(Box<HirType>)`, `Struct(String)`, `Enum(String)`, `Fn(Vec<HirType>, Box<HirType>)`, `Lambda`, `Void`
- A.1.2 — Define `HirExpr` enum mirroring AST `Expr` but with resolved types on every node: each variant carries a `ty: HirType` field
- A.1.3 — Define `HirStmt` enum mirroring AST `Stmt` with type annotations
- A.1.4 — Define `HirDecl` enum for top-level declarations with fully resolved signatures
- A.1.5 — Define `HirFnDecl` struct with resolved parameter types, return type, convention annotations, and body as `Vec<HirStmt>`
- A.1.6 — Define `HirPattern` enum with type information for match arm destructuring
- A.1.7 — Define `HirProgram` as the root: all declarations, type registry, enum registry

**Done when:** All HIR node types compile. No logic yet — just data definitions.

---

### A.2 — AST to HIR lowering

**File:** `stage1/fusec/src/hir/lower.rs`

Walk the AST and produce HIR. This is where type inference results are attached to every expression and statement.

**Tasks:**

- A.2.1 — Implement `Lowerer` struct holding type environment, enum registry, struct registry, function signatures
- A.2.2 — Implement `lower_program()`: collect all type declarations first (two-pass: signatures then bodies)
- A.2.3 — Implement `lower_fn_decl()`: resolve parameter types, return type, lower body statements
- A.2.4 — Implement `lower_expr()`: resolve type for every expression variant
  - Literals: type is obvious (IntLit → Int, etc.)
  - Identifiers: look up in type environment
  - Binary ops: infer from operand types (Int+Int → Int, String+String → String, etc.)
  - Calls: look up function signature, match argument types
  - Field access: look up struct field type
  - Match/When: infer from arm body types (all arms must agree)
  - Lambda: infer from usage context
  - FString parts: each interpolated expression produces String
- A.2.5 — Implement `lower_stmt()`: Val, Var, Assign, Return, If, For, Loop, Defer, ExprStmt
- A.2.6 — Implement `lower_pattern()`: attach type to each pattern node for codegen destructuring
- A.2.7 — Implement ownership convention propagation: `ref`, `mutref`, `owned` carried through to HIR so codegen knows calling convention
- A.2.8 — Implement ASAP last-use analysis at HIR level: mark each binding's last-use statement index so codegen can insert destructor calls
- A.2.9 — Implement defer collection: gather defer expressions per function for codegen cleanup blocks
- A.2.10 — Write unit tests: lower every Core test file's AST to HIR without panic

**Done when:** `lower_program()` successfully converts every Core test's AST to well-typed HIR.

---

### A.3 — Value layout and ABI

**File:** `stage1/fusec/src/codegen/layout.rs`

Define how Fuse values are represented in memory and how functions are called at the machine level.

**Tasks:**

- A.3.1 — Define `FuseABI` struct: maps each `HirType` to a Cranelift `Type` or aggregate
  - `Int` → `i64`
  - `Float` → `f64`
  - `Bool` → `i8` (0 or 1)
  - `Str` → pointer to `{ len: i64, data: *u8 }` (or use runtime's String representation)
  - `Unit` → zero-sized (no register)
  - `List` → pointer to runtime `FuseList` struct
  - `Struct`, `Enum` → pointer to heap-allocated runtime object
  - `Fn` → function pointer
  - `Lambda` → fat pointer: `{ fn_ptr, env_ptr }`
- A.3.2 — Define calling convention: all arguments passed as `i64` (pointer or value), return value as `i64`
- A.3.3 — Define `FuseValue` boxing strategy for runtime: when values must be boxed (heterogeneous lists, enum payloads) vs unboxed (local Int/Float/Bool)
- A.3.4 — Define struct field layout: sequential fields, each as boxed `FuseValue` pointer (start simple, optimize later)
- A.3.5 — Define enum layout: `{ tag: i64, payload: FuseValue }` — tag identifies variant, payload holds associated value
- A.3.6 — Define string layout: use runtime's existing `FuseValue::Str` representation, pass as pointer
- A.3.7 — Document the ABI in a comment block at top of file for Stage 2 to replicate

**Done when:** Every Fuse type has a defined memory layout. Layout decisions are documented.

---

### A.4 — Runtime FFI surface

**Files:** `stage1/fuse-runtime/src/lib.rs` and new file `stage1/fuse-runtime/src/ffi.rs`

Expose runtime functions with C-compatible signatures so compiled Fuse code can call them.

**Tasks:**

- A.4.1 — Create `ffi.rs` with `#[no_mangle] pub extern "C"` wrappers for all runtime operations:
  - Value construction: `fuse_rt_int(i64) -> *mut FuseValue`, `fuse_rt_float(f64) -> *mut FuseValue`, `fuse_rt_bool(i8) -> *mut FuseValue`, `fuse_rt_str(ptr, len) -> *mut FuseValue`, `fuse_rt_unit() -> *mut FuseValue`, `fuse_rt_list_new() -> *mut FuseValue`, `fuse_rt_list_push(*mut FuseValue, *mut FuseValue)`, `fuse_rt_struct_new(name_ptr, name_len) -> *mut FuseValue`, `fuse_rt_struct_set_field(*mut FuseValue, name_ptr, name_len, *mut FuseValue)`, `fuse_rt_enum_variant(enum_ptr, enum_len, var_ptr, var_len, *mut FuseValue) -> *mut FuseValue`
  - Value access: `fuse_rt_as_int(*mut FuseValue) -> i64`, `fuse_rt_as_float(*mut FuseValue) -> f64`, `fuse_rt_as_bool(*mut FuseValue) -> i8`, `fuse_rt_as_str_ptr(*mut FuseValue) -> *const u8`, `fuse_rt_as_str_len(*mut FuseValue) -> i64`, `fuse_rt_field(*mut FuseValue, name_ptr, name_len) -> *mut FuseValue`
  - Arithmetic: `fuse_rt_add`, `fuse_rt_sub`, `fuse_rt_mul`, `fuse_rt_div`, `fuse_rt_mod`, `fuse_rt_neg` — each takes two `*mut FuseValue`, returns `*mut FuseValue`
  - Comparison: `fuse_rt_eq`, `fuse_rt_ne`, `fuse_rt_lt`, `fuse_rt_gt`, `fuse_rt_le`, `fuse_rt_ge` — same signature
  - I/O: `fuse_rt_println(*mut FuseValue)`, `fuse_rt_eprintln(*mut FuseValue)`
  - String methods: `fuse_rt_str_len`, `fuse_rt_str_char_at`, `fuse_rt_str_substring`, `fuse_rt_str_starts_with`, `fuse_rt_str_contains`, `fuse_rt_str_char_code_at`, `fuse_rt_str_split`, `fuse_rt_str_trim`, `fuse_rt_str_replace`, `fuse_rt_str_to_upper`, `fuse_rt_str_to_lower`
  - List methods: `fuse_rt_list_len`, `fuse_rt_list_get`, `fuse_rt_list_set`, `fuse_rt_list_push`, `fuse_rt_list_contains`, `fuse_rt_list_first`, `fuse_rt_list_last`, `fuse_rt_list_sum`, `fuse_rt_list_sorted`, `fuse_rt_list_is_empty`
  - Type methods: `fuse_rt_int_to_float`, `fuse_rt_int_to_string`, `fuse_rt_int_is_even`, `fuse_rt_float_to_string`, `fuse_rt_type_name`, `fuse_rt_is_truthy`
  - Enum helpers: `fuse_rt_is_ok`, `fuse_rt_is_err`, `fuse_rt_is_some`, `fuse_rt_is_none`, `fuse_rt_unwrap_enum`, `fuse_rt_ok`, `fuse_rt_err`, `fuse_rt_some`, `fuse_rt_none`
  - System: `fuse_rt_read_file(path_ptr, path_len) -> *mut FuseValue` (returns Result), `fuse_rt_args() -> *mut FuseValue` (returns List), `fuse_rt_exit(code: i64)`, `fuse_rt_from_char_code(code: i64) -> *mut FuseValue`, `fuse_rt_parse_int(ptr, len) -> *mut FuseValue`, `fuse_rt_parse_float(ptr, len) -> *mut FuseValue`, `fuse_rt_panic(ptr, len)`
  - Display: `fuse_rt_to_string(*mut FuseValue) -> *mut FuseValue` (uses Display impl)
- A.4.2 — Add memory management: `fuse_rt_clone(*mut FuseValue) -> *mut FuseValue`, `fuse_rt_drop(*mut FuseValue)` — clone and free
- A.4.3 — Add lambda support: `fuse_rt_lambda_new(fn_id: i64, captures: *mut FuseValue) -> *mut FuseValue`, `fuse_rt_lambda_id(*mut FuseValue) -> i64`, `fuse_rt_lambda_captures(*mut FuseValue) -> *mut FuseValue`
- A.4.4 — Export all FFI symbols in `lib.rs`
- A.4.5 — Build runtime as `staticlib` (already configured in Cargo.toml) and verify symbols are exported: `nm libfuse_runtime.a | grep fuse_rt_`
- A.4.6 — Write a C test program that links against `libfuse_runtime.a` and calls `fuse_rt_println(fuse_rt_int(42))` — verify it prints `42`

**Done when:** All runtime operations accessible via C-compatible function calls. The C test program links and runs correctly.

---

### A.5 — Cranelift code generation

**File:** `stage1/fusec/src/codegen/cranelift.rs`

Translate HIR to Cranelift IR, produce an object file, link it with the runtime into a native binary.

**Tasks:**

- A.5.1 — Implement `Codegen` struct: holds Cranelift `Module`, `FunctionBuilderContext`, the HIR program, and a symbol table mapping Fuse function names to Cranelift `FuncId`
- A.5.2 — Implement `codegen_program()`: iterate over all HIR function declarations, create Cranelift function stubs first (forward declarations), then fill in bodies
- A.5.3 — Implement runtime function import: declare all `fuse_rt_*` functions as imported symbols in the Cranelift module with their C signatures
- A.5.4 — Implement `codegen_fn()`: create Cranelift function, translate parameters (using ABI from A.3), translate body statements, handle return
- A.5.5 — Implement `codegen_expr()` for literal expressions: call `fuse_rt_int`, `fuse_rt_float`, etc. to construct boxed values
- A.5.6 — Implement `codegen_expr()` for identifiers: load from variable slot (Cranelift `Variable`)
- A.5.7 — Implement `codegen_expr()` for binary operations: evaluate both sides, call `fuse_rt_add` / `fuse_rt_sub` / etc.
- A.5.8 — Implement `codegen_expr()` for comparison operations: call `fuse_rt_eq` / `fuse_rt_lt` / etc., extract i8 result for branching
- A.5.9 — Implement `codegen_expr()` for function calls: look up callee `FuncId`, emit Cranelift `call` instruction with arguments
- A.5.10 — Implement `codegen_expr()` for method calls: translate to runtime method calls (e.g., `list.len()` → `fuse_rt_list_len(list)`)
- A.5.11 — Implement `codegen_expr()` for field access: call `fuse_rt_field(obj, name)`
- A.5.12 — Implement `codegen_expr()` for string interpolation (FString): evaluate each part, concatenate via `fuse_rt_add`
- A.5.13 — Implement `codegen_expr()` for `?` operator: call `fuse_rt_is_ok`/`fuse_rt_is_some`, branch to early-return block on failure
- A.5.14 — Implement `codegen_expr()` for optional chaining (`?.`): check `fuse_rt_is_none`, short-circuit to None
- A.5.15 — Implement `codegen_expr()` for Elvis (`?:`): check `fuse_rt_is_none`, use fallback on None
- A.5.16 — Implement `codegen_expr()` for `move`, `ref`, `mutref`: handle ownership convention at call site
- A.5.17 — Implement `codegen_expr()` for lambdas: create a trampoline function, capture environment as a struct, return `fuse_rt_lambda_new`
- A.5.18 — Implement `codegen_expr()` for list/tuple literals: call `fuse_rt_list_new`, `fuse_rt_list_push` for each element
- A.5.19 — Implement `codegen_expr()` for struct/data class construction: call `fuse_rt_struct_new`, `fuse_rt_struct_set_field` for each field
- A.5.20 — Implement `codegen_expr()` for enum variant construction: call `fuse_rt_enum_variant`
- A.5.21 — Implement `codegen_expr()` for match expressions: lower to Cranelift switch/branch tree. For each arm: test pattern, bind variables, evaluate body
- A.5.22 — Implement `codegen_expr()` for when expressions: lower to chained if-else branches
- A.5.23 — Implement `codegen_expr()` for block expressions: emit statements, return value of last expression
- A.5.24 — Implement `codegen_stmt()` for val/var declarations: allocate Cranelift `Variable`, store value
- A.5.25 — Implement `codegen_stmt()` for assignment: look up target variable, store new value
- A.5.26 — Implement `codegen_stmt()` for if/else: Cranelift conditional branching with basic blocks for then/else
- A.5.27 — Implement `codegen_stmt()` for for loops: evaluate iterator (must be List), emit loop header, body, increment, back-edge
- A.5.28 — Implement `codegen_stmt()` for loop (infinite): Cranelift loop block with back-edge, break via return
- A.5.29 — Implement `codegen_stmt()` for return: emit return instruction with value
- A.5.30 — Implement `codegen_stmt()` for defer: register cleanup in a deferred-block list, emit cleanup sequence before function return
- A.5.31 — Implement ASAP destruction: at each statement boundary, check last-use map from HIR. If a variable's last use was this statement and it has `__del__`, emit call to `__del__` function
- A.5.32 — Implement mutref writeback: after call returns, if argument was `mutref`, store the returned/modified value back to the caller's variable
- A.5.33 — Implement extension function dispatch: translate `obj.method(args)` to `Type_method(obj, args)` call
- A.5.34 — Implement entry point: generate a `main()` function that calls the `@entrypoint`-annotated function, handles Result return (print error and exit 1 on Err)
- A.5.35 — Emit object file: use `cranelift-object` to produce a `.o` file
- A.5.36 — Link the binary: invoke system linker (`cc` or `link.exe`) to combine the `.o` file with `libfuse_runtime.a` into a native executable
- A.5.37 — Update `main.rs` to add `--emit=obj` flag (produce object file only) and make default mode produce a binary
- A.5.38 — Retain the evaluator path: `fusec --interpret file.fuse` uses the existing tree-walking evaluator. The default mode now compiles

**Done when:** `fusec tests/fuse/milestone/four_functions.fuse -o four_functions && ./four_functions` produces correct output. All 26 Core tests compile to native binaries and produce output identical to Stage 0.

---

### A.6 — Codegen for Fuse Full features

Extend the codegen to handle concurrency, async, and SIMD.

**Tasks:**

- A.6.1 — Implement `codegen_expr()` for spawn: call `fuse_rt_spawn(fn_ptr, captures)` — runtime creates a thread
- A.6.2 — Implement `codegen_expr()` for await: call `fuse_rt_await(future_ptr)` — runtime blocks until result ready
- A.6.3 — Implement Chan methods: translate `.send()`, `.recv()`, `.bounded()`, `.unbounded()` to runtime calls
- A.6.4 — Implement Shared methods: translate `.read()`, `.write()`, `.try_write()` to runtime calls
- A.6.5 — Implement SIMD: translate `SIMD<T,N>` operations to Cranelift vector instructions
- A.6.6 — Implement runtime stubs for chan.rs, shared.rs, async_rt.rs (currently 1-line placeholders)

**Done when:** All 36 integration tests (Core + Full) compile to native binaries and pass.

---

## Stage B — FFI and platform layer

**One job:** Give Fuse code the ability to call C-compatible functions, so the self-hosting compiler can call Cranelift.

### Why FFI is required

The self-hosting compiler must emit machine code. Machine code is generated by Cranelift, which is a Rust library. Fuse code cannot call Rust directly. The bridge is C-compatible FFI: Cranelift exposes a C API (or we write C wrappers), and Fuse calls those functions through an `extern` mechanism.

### Entry condition

Stage A is complete. Stage 1 produces native binaries for all test programs.

---

### B.1 — Language design: extern functions

**File:** `docs/guide/fuse-language-guide.md` (update)

Add FFI to the language specification. This is a Fuse Full feature, not Core.

**Tasks:**

- B.1.1 — Design `extern` function declaration syntax:
  ```fuse
  extern fn fuse_rt_println(val: Ptr) -> ()
  extern fn fuse_rt_int(v: Int) -> Ptr
  extern fn fuse_rt_add(a: Ptr, b: Ptr) -> Ptr
  ```
- B.1.2 — Design FFI types: `Ptr` (raw pointer, i64-sized), `CStr` (null-terminated string pointer), `Byte` (u8). These are the bridge types — they exist only at FFI boundaries
- B.1.3 — Design `extern` block for grouping:
  ```fuse
  extern "fuse-runtime" {
    fn fuse_rt_println(val: Ptr) -> ()
    fn fuse_rt_int(v: Int) -> Ptr
    // ...
  }
  ```
- B.1.4 — Document FFI safety rules: extern functions are inherently unsafe. Fuse does not add safety annotations — the developer accepts responsibility at the FFI boundary
- B.1.5 — Write ADR-011: FFI design — why `extern fn`, not `@foreign`, not annotations. Key rationale: `extern` is universally understood, matches C/Rust/Go convention
- B.1.6 — Add FFI section to language guide: syntax, types, examples, safety contract

**Done when:** FFI is specified in the language guide. ADR written. Syntax is unambiguous.

---

### B.2 — Implement FFI in Stage 1 compiler

**Files:** Lexer, parser, AST, checker, codegen

**Tasks:**

- B.2.1 — Add `extern` keyword to lexer token set
- B.2.2 — Add `Ptr`, `CStr`, `Byte` as recognized type names (or as built-in types in the type system)
- B.2.3 — Add `ExternFn` AST node: name, parameters (with FFI types), return type, library hint
- B.2.4 — Add `ExternBlock` AST node: library name, list of `ExternFn`
- B.2.5 — Parse `extern fn` declarations and `extern "lib" { ... }` blocks
- B.2.6 — Add `ExternFn` to HIR: resolved FFI types mapped to Cranelift types (Ptr → i64, etc.)
- B.2.7 — Check extern functions: validate FFI types are used correctly, no ownership conventions on FFI params (they are raw)
- B.2.8 — Codegen for extern function calls: emit Cranelift `call` to imported symbol. No boxing — arguments are raw values at FFI boundary
- B.2.9 — Linker integration: ensure extern symbols resolve against linked libraries

**Done when:** A Fuse program can declare `extern fn`, call it, and the compiled binary correctly invokes the native function.

---

### B.3 — Cranelift C API wrappers

**File:** New crate `stage1/cranelift-ffi/`

Wrap the Cranelift Rust API in C-compatible functions that the Stage 2 compiler (written in Fuse) can call through FFI.

**Tasks:**

- B.3.1 — Create `cranelift-ffi` crate with `crate-type = ["staticlib"]`
- B.3.2 — Wrap module creation: `cl_module_new() -> *mut Module`, `cl_module_finish(*mut Module, path_ptr, path_len)` (writes object file)
- B.3.3 — Wrap function creation: `cl_func_new(*mut Module, name_ptr, name_len, param_count, ret) -> FuncId`, `cl_func_begin(*mut Module, FuncId) -> *mut FuncBuilder`
- B.3.4 — Wrap parameter/return types: `cl_type_i64() -> TypeId`, `cl_type_f64() -> TypeId`, `cl_type_i8() -> TypeId`
- B.3.5 — Wrap variable management: `cl_var_declare(*mut FuncBuilder, TypeId) -> VarId`, `cl_var_def(*mut FuncBuilder, VarId, Value)`, `cl_var_use(*mut FuncBuilder, VarId) -> Value`
- B.3.6 — Wrap instruction emission: `cl_iconst(*mut FuncBuilder, i64) -> Value`, `cl_fconst(*mut FuncBuilder, f64) -> Value`, `cl_iadd`, `cl_isub`, `cl_imul`, `cl_idiv`, `cl_icmp`, `cl_call`, `cl_return`, `cl_brz`, `cl_brnz`, `cl_jump`
- B.3.7 — Wrap basic block management: `cl_block_new(*mut FuncBuilder) -> BlockId`, `cl_block_switch(*mut FuncBuilder, BlockId)`, `cl_block_param(*mut FuncBuilder, BlockId, TypeId) -> Value`
- B.3.8 — Wrap function finalization: `cl_func_finish(*mut FuncBuilder)`, `cl_func_define(*mut Module, FuncId)`
- B.3.9 — Wrap symbol import: `cl_import_fn(*mut Module, name_ptr, name_len, param_count, ret) -> FuncRef`
- B.3.10 — Wrap linker invocation: `cl_link(obj_path_ptr, obj_path_len, lib_paths, lib_count, out_path_ptr, out_path_len)` — calls system linker
- B.3.11 — Build and verify: compile `cranelift-ffi` as static library, verify symbols with `nm`
- B.3.12 — Write a test: C program that uses `cranelift-ffi` to generate a trivial native binary (add two numbers, print result) — verify the generated binary works

**Done when:** The Cranelift API is fully accessible through C-compatible function calls. A non-Rust program can generate native binaries using these wrappers.

---

### B.4 — Platform abstraction: file I/O and process management

The self-hosting compiler needs to read source files, write object files, and invoke the linker. These are provided by the runtime's FFI surface (from A.4) plus a few additions.

**Tasks:**

- B.4.1 — Add `fuse_rt_write_file(path_ptr, path_len, data_ptr, data_len) -> *mut FuseValue` (returns Result)
- B.4.2 — Add `fuse_rt_run_process(cmd_ptr, cmd_len, args_ptr, args_count) -> *mut FuseValue` (returns Result with exit code) — for invoking the linker
- B.4.3 — Add `fuse_rt_getcwd() -> *mut FuseValue` (returns String) — for resolving relative paths
- B.4.4 — Add `fuse_rt_path_join(a_ptr, a_len, b_ptr, b_len) -> *mut FuseValue` — path manipulation
- B.4.5 — Add `fuse_rt_path_exists(path_ptr, path_len) -> i8` — file existence check
- B.4.6 — Add `fuse_rt_env_var(name_ptr, name_len) -> *mut FuseValue` — read environment variables (returns Option)
- B.4.7 — Add `fuse_rt_time_ms() -> i64` — monotonic clock for compilation timing

**Done when:** A Fuse program can read files, write files, invoke subprocesses, and manipulate paths — everything a compiler needs to do.

---

## Stage C — Write the Stage 2 compiler in Fuse

**One job:** Implement the full Fuse compiler pipeline in Fuse Core, targeting Cranelift via FFI.

### Entry condition

Stage A is complete (native codegen works). Stage B is complete (FFI works, Cranelift wrappers exist). A Fuse program compiled by Stage 1 can call Cranelift and produce a native binary.

### Architecture

The Stage 2 compiler follows the same pipeline as Stage 1:

```
Source text
  ↓ Lexer (tokenize)
Token stream
  ↓ Parser (parse)
AST
  ↓ Checker (check)
Diagnostics + validated AST
  ↓ HIR Lowerer (lower)
HIR (typed)
  ↓ Codegen (codegen)
Cranelift IR → Object file → Linked binary
```

The directory structure:

```
stage2/src/
  main.fuse           ← CLI entry point
  lexer/
    token.fuse         ← Token enum and keyword table
    lexer.fuse         ← Tokenizer
  ast/
    nodes.fuse         ← AST node types
  parser/
    parser.fuse        ← Recursive descent parser
  checker/
    checker.fuse       ← Main checker orchestration
    ownership.fuse     ← ref/mutref/owned/move enforcement
    types.fuse         ← Type inference and consistency
    exhaustiveness.fuse ← Match arm coverage
  hir/
    nodes.fuse         ← HIR node types
    lower.fuse         ← AST → HIR lowering
  codegen/
    codegen.fuse       ← HIR → Cranelift IR translation
    layout.fuse        ← Value layout and ABI
    ffi.fuse           ← Cranelift FFI declarations
  error.fuse           ← Error types and formatting
```

### Design constraints

1. **Fuse Core only.** The self-hosting compiler must be writable in Fuse Core (no concurrency, no async). This is a deliberate constraint from the implementation plan — Fuse Core is the stable, frozen subset.

2. **No recursion for iteration.** The existing Stage 2 interpreter used recursion for loops and caused stack overflow. The self-hosting compiler must use `for` and `loop` for iteration. Recursive descent in the parser is acceptable (bounded by nesting depth), but token-level loops must be iterative.

3. **Identical output.** The Stage 2 compiler must produce binaries whose output matches Stage 0 and Stage 1 exactly, for every test program.

4. **Same error messages.** Checker diagnostics must match Stage 1's format and content.

---

### C.1 — Error types

**File:** `stage2/src/error.fuse`

**Tasks:**

- C.1.1 — Define `FuseError` data class: `message: String`, `hint: Option<String>`, `file: String`, `line: Int`, `col: Int`, `kind: ErrorKind`
- C.1.2 — Define `ErrorKind` enum: `Error`, `Warning`
- C.1.3 — Implement `FuseError.display(ref self) -> String`: format error message matching Stage 1's output format exactly
- C.1.4 — Define `Result` type alias usage pattern for error propagation

**Done when:** Error formatting matches Stage 1 output character-for-character.

---

### C.2 — Token definitions

**File:** `stage2/src/lexer/token.fuse`

**Tasks:**

- C.2.1 — Define `Tok` enum with all token variants:
  - Keywords: `Fn`, `Val`, `Var`, `Ref`, `Mutref`, `Owned`, `Move`, `Struct`, `Class`, `Enum`, `Match`, `When`, `If`, `Else`, `For`, `In`, `Loop`, `Return`, `Defer`, `And`, `Or`, `Not`, `True`, `False`, `SelfKw`, `Spawn`, `Async`, `Await`, `Suspend`, `Data`, `Extern`
  - Operators: `Arrow`, `FatArrow`, `QuestionDot`, `Elvis`, `Question`, `At`, `Dot`, `DotDot`, `Colon`, `ColonColon`, `Eq`, `EqEq`, `BangEq`, `Lt`, `Gt`, `LtEq`, `GtEq`, `Plus`, `Minus`, `Star`, `Slash`, `Percent`, `Pipe`
  - Delimiters: `LParen`, `RParen`, `LBrace`, `RBrace`, `LBracket`, `RBracket`, `Comma`, `Semicolon`
  - Literals: `IntLit(Int)`, `FloatLit(Float)`, `StrLit(String)`, `FStringLit(List<FStringPart>)`, `BoolLit(Bool)`
  - Special: `Ident(String)`, `Eof`
- C.2.2 — Define `FStringPart` enum: `Literal(String)`, `Expr(String)`
- C.2.3 — Define `Token` data class: `ty: Tok`, `line: Int`, `col: Int`
- C.2.4 — Implement `keyword(s: String) -> Option<Tok>`: keyword lookup table

**Done when:** Token types cover every terminal in Fuse Core and Full.

---

### C.3 — Lexer

**File:** `stage2/src/lexer/lexer.fuse`

**Tasks:**

- C.3.1 — Define `Lexer` struct: `src: String`, `pos: Int`, `line: Int`, `col: Int`, `file: String`
- C.3.2 — Implement `tokenize(mutref self) -> Result<List<Token>, FuseError>`: main loop using `loop` or `for`, not recursion
- C.3.3 — Implement `skip_whitespace(mutref self)`: skip spaces, tabs, newlines, `//` comments — use `loop` with break
- C.3.4 — Implement `read_number(mutref self) -> Token`: integer and float parsing with `.` lookahead
- C.3.5 — Implement `read_string(mutref self) -> Token`: quoted strings with escape sequences `\n \t \\ \" \{ \}`
- C.3.6 — Implement `read_fstring(mutref self) -> Token`: `f"text {expr} text"` with nested brace tracking — use loop with depth counter
- C.3.7 — Implement `read_identifier(mutref self) -> Token`: alphanumeric + underscore, keyword recognition
- C.3.8 — Implement `read_operator(mutref self) -> Token`: two-character operators first (`=>`, `->`, `?.`, `?:`, `==`, `!=`, `<=`, `>=`, `::`), then single-character
- C.3.9 — Test: tokenize all Core test files, compare token counts with Stage 1

**Done when:** Lexer produces identical token streams to Stage 1 for all test files.

---

### C.4 — AST node definitions

**File:** `stage2/src/ast/nodes.fuse`

**Tasks:**

- C.4.1 — Define `Span` data class: `line: Int`, `col: Int`
- C.4.2 — Define `TypeExpr` enum: `Simple(String, Span)`, `Generic(String, List<TypeExpr>, Span)`, `Union(List<TypeExpr>, Span)`
- C.4.3 — Define `BinOp` enum: `Add`, `Sub`, `Mul`, `Div`, `Mod`, `Eq`, `Ne`, `Lt`, `Gt`, `Le`, `Ge`, `And`, `Or`
- C.4.4 — Define `UnaryOp` enum: `Neg`, `Not`
- C.4.5 — Define `Lit` enum for literal values in patterns: `IntLit(Int)`, `FloatLit(Float)`, `StrLit(String)`, `BoolLit(Bool)`
- C.4.6 — Define `Pattern` enum: `Wildcard(Span)`, `Ident(String, Span)`, `Literal(Lit, Span)`, `Constructor(String, List<Pattern>, Span)`, `Tuple(List<Pattern>, Span)`
- C.4.7 — Define `Expr` enum with all 28+ variants (matching Stage 1 exactly)
- C.4.8 — Define `Stmt` enum: `Val`, `ValTuple`, `Var`, `Assign`, `ExprStmt`, `Return`, `Defer`, `If`, `For`, `Loop`
- C.4.9 — Define `MatchArm`, `WhenArm` data classes
- C.4.10 — Define `FnBody` enum: `Block(List<Stmt>)`, `Expr(Expr)`
- C.4.11 — Define `Param`, `Field`, `Annotation` data classes
- C.4.12 — Define `FnDecl`, `EnumDecl`, `EnumVariant`, `StructDecl`, `DataClassDecl` data classes
- C.4.13 — Define `Decl` enum: `Fn(FnDecl)`, `Enum(EnumDecl)`, `Struct(StructDecl)`, `DataClass(DataClassDecl)`, `TopVal`, `TopVar`, `ExternFn`, `ExternBlock`
- C.4.14 — Define `Program` data class: `decls: List<Decl>`, `span: Span`

**Done when:** Every AST node from Stage 1 has an equivalent Fuse definition.

---

### C.5 — Parser

**File:** `stage2/src/parser/parser.fuse`

Port the Stage 1 recursive descent parser to Fuse.

**Tasks:**

- C.5.1 — Define `Parser` struct: `tokens: List<Token>`, `pos: Int`, `file: String`, `allow_brace: Bool`
- C.5.2 — Implement utility methods: `peek() -> Tok`, `at(ty) -> Bool`, `advance() -> Token`, `eat(ty) -> Bool`, `expect(ty, ctx) -> Result<Token, FuseError>`
- C.5.3 — Implement `parse(mutref self) -> Result<Program, FuseError>`: top-level loop collecting declarations
- C.5.4 — Implement declaration parsing: `parse_annotation`, `parse_decl`, `parse_fn_decl`, `parse_enum_decl`, `parse_struct_decl`, `parse_data_class_decl`, `parse_top_val`, `parse_top_var`, `parse_extern_fn`, `parse_extern_block`
- C.5.5 — Implement statement parsing: `parse_block`, `parse_stmt`, `parse_val_decl` (including tuple destructuring), `parse_var_decl`, `parse_if_stmt`, `parse_for_stmt`, `parse_loop_stmt`, `parse_return_stmt`, `parse_defer_stmt`
- C.5.6 — Implement expression parsing with precedence climbing: `parse_expr` → `parse_elvis` → `parse_or` → `parse_and` → `parse_not` → `parse_comparison` → `parse_addition` → `parse_multiplication` → `parse_unary` → `parse_postfix` → `parse_primary`
  - Use `loop` for left-associative operators (not recursion)
- C.5.7 — Implement postfix parsing: field access `.`, optional chain `?.`, question `?`, function call `()`, lambda trailing `{}`
  - Use `loop` for postfix chain (not recursion)
- C.5.8 — Implement primary parsing: literals, identifiers, parenthesized expressions, tuples, list literals, match, when, block expressions
- C.5.9 — Implement pattern parsing: wildcard, identifier, literal, constructor (with qualified names), tuple
- C.5.10 — Implement type expression parsing: simple, generic `<...>` (with nested generics), union `|`
- C.5.11 — Implement lambda detection: lookahead to distinguish `{x => body}` from block
- C.5.12 — Implement f-string expression parsing: re-lex and re-parse expressions within `{...}` interpolation
- C.5.13 — Test: parse all Core test files, verify no parse errors

**Done when:** Parser produces structurally identical ASTs to Stage 1 for all test files.

---

### C.6 — Checker

**Files:** `stage2/src/checker/checker.fuse`, `ownership.fuse`, `types.fuse`, `exhaustiveness.fuse`

Port the Stage 1 semantic checker to Fuse.

**Tasks:**

- C.6.1 — Define `Checker` struct: `file: String`, `errors: List<FuseError>`, `enums: Map<String, EnumDecl>`, `scopes: List<Map<String, Binding>>`
- C.6.2 — Define `Binding` data class: `is_mutable: Bool`, `convention: Option<String>`, `moved: Bool`, `moved_line: Int`
- C.6.3 — Implement scope management: `push_scope`, `pop_scope`, `define`, `lookup`
- C.6.4 — Implement `check(ref self, ref program: Program) -> List<FuseError>`: two-pass (collect types, then check declarations)
- C.6.5 — Implement `check_fn`: validate function body, track parameter conventions, check return paths
- C.6.6 — Implement `check_stmt`: validate each statement type
  - Val: define as immutable in scope
  - Var: define as mutable in scope
  - Assign: verify target is mutable (`var` or `mutref`), not `val`
  - Return: validate return value matches function signature
  - If/For/Loop: push scope, check body, pop scope
- C.6.7 — Implement `check_expr`: validate each expression type
  - Identifier: verify defined, not moved
  - Move: mark variable as moved, error on subsequent use
  - Mutref: verify at call site matches parameter convention
  - Call: verify argument count and conventions
  - Field access: verify field exists on struct type
- C.6.8 — Implement ownership enforcement (ownership.fuse):
  - `ref` parameters: reject assignment through, reject move from
  - `mutref` parameters: allow modification, reject move
  - `owned` parameters: allow all operations
  - `move` at call site: mark as consumed, error on reuse
- C.6.9 — Implement type checking (types.fuse):
  - Track `Result<T,E>` and `Option<T>` as known types
  - Validate `?` operator is used on Result or Option
  - Validate match subject type for exhaustiveness
- C.6.10 — Implement match exhaustiveness (exhaustiveness.fuse):
  - Collect all enum variants for subject type
  - Verify every variant appears in arms (or wildcard `_` covers remainder)
  - Produce specific error: "missing case: VariantName"
- C.6.11 — Test: check all Core test files. Valid programs produce no errors. Error test files produce matching error messages

**Done when:** Checker accepts all valid Core programs and rejects all error programs with messages matching Stage 1.

---

### C.7 — HIR nodes and lowering

**Files:** `stage2/src/hir/nodes.fuse`, `stage2/src/hir/lower.fuse`

**Tasks:**

- C.7.1 — Define HIR node types in Fuse (mirror A.1 definitions)
- C.7.2 — Implement `Lowerer` struct with type environment and registries
- C.7.3 — Implement `lower_program`: collect type declarations, lower all functions
- C.7.4 — Implement `lower_fn_decl`: resolve types, lower body
- C.7.5 — Implement `lower_expr`: attach resolved type to every expression
- C.7.6 — Implement `lower_stmt`: lower each statement variant
- C.7.7 — Implement `lower_pattern`: attach type for destructuring
- C.7.8 — Implement last-use analysis: compute ASAP destruction points
- C.7.9 — Implement defer collection: gather deferred expressions per function

**Done when:** HIR lowering succeeds for all Core test programs.

---

### C.8 — Cranelift FFI declarations

**File:** `stage2/src/codegen/ffi.fuse`

Declare all Cranelift wrapper functions and runtime functions as `extern`.

**Tasks:**

- C.8.1 — Declare all `cl_*` functions from Stage B.3 (module, function, instruction, block management)
- C.8.2 — Declare all `fuse_rt_*` functions from Stage A.4 (value construction, methods, I/O, system)
- C.8.3 — Define helper functions that wrap raw FFI calls with Fuse-ergonomic signatures:
  ```fuse
  fn emitInt(mutref builder: Ptr, value: Int) -> Ptr {
    cl_iconst(builder, value)
  }
  ```
- C.8.4 — Test: a minimal Fuse program that uses FFI to call `cl_module_new()` and `cl_module_finish()` — produces an empty but valid object file

**Done when:** All FFI declarations compile and link successfully.

---

### C.9 — Code generation (codegen)

**Files:** `stage2/src/codegen/codegen.fuse`, `stage2/src/codegen/layout.fuse`

The core of the self-hosting compiler. Translate HIR to Cranelift IR via FFI.

**Tasks:**

- C.9.1 — Define `Codegen` struct: `module: Ptr` (Cranelift module handle), `symbols: Map<String, Int>` (function name → FuncId), `hir: HirProgram`, `file: String`
- C.9.2 — Implement layout (layout.fuse): replicate the ABI decisions from A.3. Map each Fuse type to its Cranelift representation
- C.9.3 — Implement `codegen_program`:
  - Create Cranelift module via `cl_module_new()`
  - Import all `fuse_rt_*` symbols via `cl_import_fn()`
  - Forward-declare all Fuse functions
  - Generate body for each function
  - Emit entry point `main` wrapper
  - Finalize module, write object file via `cl_module_finish()`
  - Invoke linker via `fuse_rt_run_process()` to produce final binary
- C.9.4 — Implement `codegen_fn`: create function, declare parameters, generate body, finalize
- C.9.5 — Implement `codegen_expr` for each expression variant (same logic as A.5.5 through A.5.23, but in Fuse calling Cranelift via FFI)
- C.9.6 — Implement `codegen_stmt` for each statement variant (same logic as A.5.24 through A.5.30)
- C.9.7 — Implement ASAP destruction emission: insert destructor calls at last-use points
- C.9.8 — Implement defer cleanup: emit cleanup blocks before function return
- C.9.9 — Implement mutref writeback after function calls
- C.9.10 — Implement lambda codegen: trampoline functions, environment capture
- C.9.11 — Implement match codegen: branch tree for pattern matching
- C.9.12 — Implement entry point generation: main() calls @entrypoint, handles Result return

**Done when:** The Stage 2 compiler, compiled by Stage 1, produces working native binaries for all Core test programs.

---

### C.10 — CLI entry point

**File:** `stage2/src/main.fuse`

Replace the existing interpreter with the compiler CLI.

**Tasks:**

- C.10.1 — Implement argument parsing: `fusec2 <file.fuse>`, `fusec2 --check <file>`, `fusec2 --version`, `fusec2 --help`, `fusec2 -o <output> <file>`
- C.10.2 — Implement pipeline orchestration: read source → lex → parse → check → lower → codegen → link
- C.10.3 — Implement error reporting: print diagnostics to stderr, exit 1 on errors
- C.10.4 — Implement `--check` mode: lex → parse → check only, no codegen
- C.10.5 — Implement `--emit=obj` mode: produce object file without linking
- C.10.6 — Implement output path: default to input filename without `.fuse` extension, override with `-o`

**Done when:** `fusec2 file.fuse -o output && ./output` works for all Core test programs.

---

### C.11 — Integration testing

**Tasks:**

- C.11.1 — Compile Stage 2 compiler with Stage 1: `fusec stage2/src/main.fuse -o fusec2`
- C.11.2 — Run all Core tests through fusec2: for each `.fuse` file in `tests/fuse/core/`, compile with fusec2 and verify output matches expected
- C.11.3 — Run milestone program: `fusec2 tests/fuse/milestone/four_functions.fuse -o four_functions && ./four_functions` — output matches expected
- C.11.4 — Run error tests: `fusec2 --check` on error test files, verify diagnostics match
- C.11.5 — Verify output parity: for every test, Stage 0 (Python), Stage 1 (Rust), and Stage 2 (Fuse) produce identical output
- C.11.6 — Run Full tests through fusec2 (if Full features are implemented in Stage 2)

**Done when:** fusec2 (compiled by Stage 1) passes all Core tests with output identical to Stage 0 and Stage 1.

---

## Stage D — Bootstrap and verify

**One job:** Prove that Fuse can compile itself reproducibly.

### Entry condition

Stage C is complete. fusec2 (compiled by Stage 1) produces correct binaries for all test programs.

---

### D.1 — Bootstrap step 1: fusec2-bootstrap

**Task:** Compile the Stage 2 compiler using the Stage 1 Rust compiler.

```bash
fusec stage2/src/main.fuse -o fusec2-bootstrap
```

**Done when:** `fusec2-bootstrap` is a native binary that runs.

---

### D.2 — Bootstrap step 2: fusec2-stage2

**Task:** Use fusec2-bootstrap to compile the Stage 2 compiler.

```bash
./fusec2-bootstrap stage2/src/main.fuse -o fusec2-stage2
```

**Done when:** `fusec2-stage2` is a native binary that runs and passes all tests.

---

### D.3 — Bootstrap step 3: fusec2-verified

**Task:** Use fusec2-stage2 to compile the Stage 2 compiler again.

```bash
./fusec2-stage2 stage2/src/main.fuse -o fusec2-verified
```

**Done when:** `fusec2-verified` is a native binary.

---

### D.4 — Reproducibility check

**Task:** Verify that fusec2-stage2 and fusec2-verified are byte-for-byte identical.

```bash
sha256sum fusec2-stage2 fusec2-verified
# Both hashes must match
```

If they differ, the compiler is non-deterministic — likely due to pointer addresses in output, hash map iteration order, or timestamps. Each source of non-determinism must be found and eliminated.

**Common sources of non-determinism to check:**
- D.4.1 — HashMap iteration order: use sorted keys or ordered maps
- D.4.2 — Pointer addresses in generated code: use stable indices, not addresses
- D.4.3 — Timestamps or system-dependent values: strip or make reproducible
- D.4.4 — Floating-point representation: ensure deterministic formatting

**Done when:** `sha256sum` produces identical hashes for fusec2-stage2 and fusec2-verified.

---

### D.5 — Full test suite on self-compiled compiler

**Task:** Run the complete test suite using fusec2-verified.

- D.5.1 — All 26 Core tests pass
- D.5.2 — Milestone program runs correctly
- D.5.3 — All error test files produce correct diagnostics
- D.5.4 — Output is byte-for-byte identical to Stage 0 and Stage 1

**Done when:** fusec2-verified passes every test. The Rust compiler is no longer required to build Fuse.

---

### D.6 — Archive Stage 1

**Tasks:**

- D.6.1 — Document the bootstrap process in `stage2/README.md`: how to rebuild from scratch on a new platform (requires Rust toolchain once, then Fuse is self-sufficient)
- D.6.2 — Tag the repository: `v0.1.0-self-hosting`
- D.6.3 — Update main `README.md`: Fuse is self-hosting. Building requires only `fusec2` and a C linker

**Done when:** Documentation is complete. The project is self-sufficient.

---

## Stage E — Toolchain foundation

**One job:** Extend the self-hosting compiler into a complete development toolchain.

### Why this section exists

The self-hosting compiler is the foundation, not the finish. A language used for production work needs a package manager, a build system, a test runner, a formatter, and a language server. All of these should be written in Fuse — they are the first real-world programs the language builds, and they exercise every feature in the standard library.

### Entry condition

Stage D is complete. Fuse compiles itself. The bootstrap is verified.

---

### E.1 — Build system: `fuse build`

**Tasks:**

- E.1.1 — Design project manifest: `fuse.toml` — project name, version, dependencies, entry point, build options
  ```toml
  [project]
  name = "myapp"
  version = "0.1.0"
  entry = "src/main.fuse"

  [dependencies]
  http = "0.2.0"
  json = "1.0.0"

  [build]
  target = "native"
  optimize = true
  ```
- E.1.2 — Implement multi-file compilation: resolve `import` statements, build dependency graph, compile in topological order
- E.1.3 — Implement incremental compilation: track file modification times, only recompile changed files and their dependents
- E.1.4 — Implement `fuse build` command: read `fuse.toml`, compile all sources, link into final binary
- E.1.5 — Implement `fuse run` command: build then execute
- E.1.6 — Implement `fuse check` command: type-check without codegen
- E.1.7 — Implement `fuse clean` command: remove build artifacts

**Done when:** A multi-file Fuse project builds from `fuse.toml` manifest.

---

### E.2 — Module and import system

**Tasks:**

- E.2.1 — Design import syntax:
  ```fuse
  import std.collections.HashMap
  import mylib.{Parser, Lexer}
  import mylib as ml
  ```
- E.2.2 — Design module resolution: file path maps to module path (`src/lexer/token.fuse` → `lexer.token`)
- E.2.3 — Design visibility: `pub` keyword for exported declarations, everything else is module-private
- E.2.4 — Update language guide with module system specification
- E.2.5 — Implement in lexer: `import`, `pub`, `as` keywords
- E.2.6 — Implement in parser: import declarations, pub modifier on decl
- E.2.7 — Implement in checker: module-level name resolution, visibility enforcement
- E.2.8 — Implement in codegen: cross-module function references, separate compilation units

**Done when:** A project with multiple `.fuse` files can import symbols between them.

---

### E.3 — Package manager: `fuse pkg`

**Tasks:**

- E.3.1 — Design package registry protocol: HTTP API for publishing, discovering, and downloading packages
- E.3.2 — Design lock file format: `fuse.lock` — exact versions and checksums for reproducible builds
- E.3.3 — Implement `fuse pkg init` — create new project with `fuse.toml`
- E.3.4 — Implement `fuse pkg add <name>` — add dependency to `fuse.toml`, fetch and install
- E.3.5 — Implement `fuse pkg remove <name>` — remove dependency
- E.3.6 — Implement `fuse pkg install` — install all dependencies from `fuse.toml`
- E.3.7 — Implement `fuse pkg publish` — publish package to registry
- E.3.8 — Implement dependency resolution: semver constraint solving, conflict detection
- E.3.9 — Implement `fuse pkg update` — update dependencies within semver constraints
- E.3.10 — Implement vendoring: `fuse pkg vendor` — copy all dependencies into project for offline builds

**Done when:** A Fuse project can declare dependencies, install them, and build against them.

---

### E.4 — Test runner: `fuse test`

**Tasks:**

- E.4.1 — Design test syntax:
  ```fuse
  @test
  fn test_addition() {
    assert(1 + 1 == 2)
  }

  @test
  fn test_result_propagation() -> Result<(), String> {
    val v = parse("42")?
    assert(v == 42)
    Ok(())
  }
  ```
- E.4.2 — Implement `@test` annotation recognition in parser
- E.4.3 — Implement `assert(condition)` and `assert_eq(a, b)` builtins with failure messages
- E.4.4 — Implement test discovery: find all `@test` functions across all source files
- E.4.5 — Implement `fuse test` command: compile tests, run each in isolation, report results
- E.4.6 — Implement test filtering: `fuse test --filter "pattern"` — run matching tests only
- E.4.7 — Implement test output: pass/fail counts, failure details with file:line

**Done when:** `fuse test` discovers and runs all tests in a project, reports pass/fail.

---

### E.5 — Formatter: `fuse fmt`

**Tasks:**

- E.5.1 — Define canonical formatting rules (indentation, brace placement, line length, spacing)
- E.5.2 — Implement AST pretty-printer: take parsed AST, emit canonically formatted source
- E.5.3 — Implement `fuse fmt` command: format files in-place
- E.5.4 — Implement `fuse fmt --check` command: verify files are formatted, exit 1 if not (for CI)
- E.5.5 — Ensure idempotence: formatting already-formatted code produces identical output

**Done when:** `fuse fmt` consistently formats Fuse source code. Formatting the Stage 2 compiler's source produces stable output.

---

### E.6 — Language server protocol (LSP)

**Tasks:**

- E.6.1 — Implement basic LSP server in Fuse: handle `textDocument/didOpen`, `textDocument/didChange`
- E.6.2 — Implement diagnostics: run checker on save, push errors/warnings to editor
- E.6.3 — Implement go-to-definition: resolve identifier to declaration location
- E.6.4 — Implement hover information: show type and documentation for symbol under cursor
- E.6.5 — Implement completion: suggest identifiers, keywords, struct fields, enum variants
- E.6.6 — Implement find-references: find all uses of a symbol
- E.6.7 — Package as VS Code extension

**Done when:** VS Code provides real-time error checking, go-to-definition, and completion for Fuse files.

---

## Appendix A — Builtins required by the self-hosting compiler

The Stage 2 compiler must be able to call these builtins. The runtime FFI surface (Stage A.4) must expose them all.

### I/O

| Function | Signature | Purpose |
|---|---|---|
| `println` | `(val: Any) -> ()` | Print to stdout |
| `eprintln` | `(val: Any) -> ()` | Print to stderr |
| `readFile` | `(path: String) -> Result<String, String>` | Read source file |
| `writeFile` | `(path: String, data: String) -> Result<(), String>` | Write object file |
| `args` | `() -> List<String>` | CLI arguments |
| `exit` | `(code: Int) -> !` | Terminate process |

### Conversion

| Function | Signature | Purpose |
|---|---|---|
| `parseInt` | `(s: String) -> Result<Int, String>` | Parse integer literals |
| `parseFloat` | `(s: String) -> Result<Float, String>` | Parse float literals |
| `fromCharCode` | `(code: Int) -> String` | Character construction |

### String methods

| Method | Purpose |
|---|---|
| `.len()` | String length |
| `.charAt(i)` | Character at index |
| `.charCodeAt(i)` | Character code at index |
| `.substring(start, end)` | Extract substring |
| `.startsWith(prefix)` | Prefix test |
| `.contains(needle)` | Substring test |
| `.split(delimiter)` | Split into list |
| `.trim()` | Remove whitespace |
| `.replace(find, rep)` | String substitution |
| `.toUpper()` | Uppercase |
| `.toLower()` | Lowercase |

### List methods

| Method | Purpose |
|---|---|
| `.len()` | List length |
| `.get(i)` | Element at index |
| `.set(i, v)` | Replace element |
| `.push(v)` | Append element |
| `.contains(v)` | Membership test |
| `.first()` | First element as Option |
| `.last()` | Last element as Option |
| `.isEmpty()` | Emptiness test |
| `.map(fn)` | Functional map |
| `.filter(fn)` | Functional filter |
| `.sorted()` | Sorted copy |

### Int/Float methods

| Method | Purpose |
|---|---|
| `Int.toFloat()` | Convert to float |
| `Int.toString()` | Convert to string |
| `Float.toString()` | Convert to string |

### Process management (needed for linker invocation)

| Function | Signature | Purpose |
|---|---|---|
| `runProcess` | `(cmd: String, args: List<String>) -> Result<Int, String>` | Invoke subprocess |
| `getCwd` | `() -> String` | Current directory |
| `pathJoin` | `(a: String, b: String) -> String` | Path manipulation |
| `pathExists` | `(path: String) -> Bool` | File existence |
| `envVar` | `(name: String) -> Option<String>` | Environment variable |

---

## Appendix B — Critical design decisions

### B.1 — Why not keep the interpreter approach

The existing Stage 2 `main.fuse` is a tree-walking interpreter. Three reasons it cannot be the self-hosting path:

1. **The bootstrap requires binaries.** Steps 3-5 of the bootstrap sequence require that the compiler produces native executables. An interpreter can only run programs — it cannot produce a binary that runs independently of the host.

2. **Stack overflow is fundamental.** Fuse has no `while` loop — iteration is via `for`/`loop`. The interpreter used recursive functions for lexer loops. When interpreted by the Stage 1 evaluator (also recursive), each Fuse recursion level creates many Rust stack frames. This is recursion-squared and overflows even with 128MB stack. The fix is not more stack — it is native compilation, where Fuse's `loop` compiles to a machine-level jump.

3. **Performance.** A compiler compiled to native code runs at machine speed. An interpreter interpreted by an interpreter runs at ~1000x slowdown. Compiling the compiler's own source (1,800+ lines) would be impractical.

### B.2 — Why Cranelift FFI wrappers, not direct LLVM/QBE

ADR-007 chose Cranelift for Stage 1. The self-hosting compiler should use the same backend for consistency and because the team already understands it. The C-compatible wrapper layer is the minimal bridge between Fuse and Cranelift — it adds a thin translation layer without reimplementing code generation logic.

### B.3 — Why Fuse Core, not Fuse Full

The implementation plan specifies "Write the Fuse compiler in Fuse Core." Core is the frozen, stable subset. Using Full features (concurrency, async) in the compiler would create a circular dependency — the compiler would need features it hasn't yet compiled. Core is sufficient for a single-threaded, synchronous compiler.

### B.4 — Why iterative loops, not recursive

The proof-of-concept interpreter used recursion for all iteration (skipWsLoop, tokenizeLoop, etc.) because Fuse Core has no `while`. However, Fuse Core has `loop` (infinite loop with break via return) and `for` (iteration over collections). Both compile to machine-level jumps and do not consume stack frames. All iteration in the self-hosting compiler must use `loop` or `for`, reserving recursion for genuinely recursive structures (AST walking, pattern matching descent).

---

## Appendix C — Iteration order

```
Stage A  ──  Native codegen in Stage 1         (make fusec produce binaries)
Stage B  ──  FFI + Cranelift wrappers           (let Fuse call native code)
Stage C  ──  Write Stage 2 compiler in Fuse     (the actual self-hosting work)
Stage D  ──  Bootstrap and verify               (prove it works)
Stage E  ──  Toolchain: build, test, fmt, pkg   (make it usable)
```

Each stage has an explicit entry condition. No stage begins until the previous stage's done-when condition is met. No stage has a deadline. Each stage is complete when it is correct.

The guide precedes the implementation. If a behaviour is not in the language guide, it does not exist yet.

---

## Appendix D — Recommended implementation sequence

The stages are ordered by dependency, but not every section within a stage must complete before the next stage begins. The self-hosting compiler is written in Fuse Core — Full features (concurrency, async, SIMD) are not required and can be deferred.

### Phase 1: Core native codegen

```
A.1  HIR node definitions
A.2  AST to HIR lowering
A.3  Value layout and ABI
A.4  Runtime FFI surface
A.5  Cranelift code generation
```

A.5 is the largest section (38 subtasks). Build it in layers, testing at each checkpoint:

**Layer 1 — Minimal output** (A.5.1–A.5.6, A.5.34–A.5.38):
Literals, identifiers, `println`, entry point generation, object emission, linking.
Checkpoint: `println(42)` compiles to a native binary and prints `42`.

**Layer 2 — Arithmetic and control flow** (A.5.7–A.5.8, A.5.24–A.5.28):
Binary ops, comparisons, val/var declarations, assignment, if/else, for loops, loop.
Checkpoint: programs with variables, arithmetic, and branching compile and run.

**Layer 3 — Functions and types** (A.5.9–A.5.12, A.5.19–A.5.20, A.5.33):
Function calls, method calls, field access, f-strings, struct/data class construction, enum construction, extension function dispatch.
Checkpoint: programs with structs, enums, and function calls compile and run.

**Layer 4 — Pattern matching and error handling** (A.5.13–A.5.16, A.5.21–A.5.23):
`?` operator, optional chaining, Elvis, move/ref/mutref, match expressions, when expressions, block expressions.
Checkpoint: all 26 Core tests compile to native binaries with correct output.

**Layer 5 — Advanced features** (A.5.17–A.5.18, A.5.29–A.5.32):
Lambdas, list/tuple literals, loop (infinite), defer, ASAP destruction, mutref writeback.
Checkpoint: `four_functions.fuse` compiles to a native binary with output identical to Stage 0.

### Phase 2: FFI layer

```
B.1  Language design: extern functions
B.2  Implement FFI in Stage 1 compiler
B.3  Cranelift C API wrappers
B.4  Platform abstraction: file I/O and process management
```

### Phase 3: Stage 2 compiler

```
C.1  Error types
C.2  Token definitions
C.3  Lexer
C.4  AST node definitions
C.5  Parser
C.6  Checker
C.7  HIR nodes and lowering
C.8  Cranelift FFI declarations
C.9  Code generation
C.10 CLI entry point
C.11 Integration testing
```

### Phase 4: Bootstrap

```
D.1  Compile fusec2 with Stage 1          → fusec2-bootstrap
D.2  Compile fusec2 with fusec2-bootstrap → fusec2-stage2
D.3  Compile fusec2 with fusec2-stage2    → fusec2-verified
D.4  Reproducibility check (sha256 match)
D.5  Full test suite on self-compiled compiler
D.6  Archive Stage 1, tag release
```

### Deferred

```
A.6  Codegen for Fuse Full features       (after self-hosting is achieved)
E.*  Toolchain: build, test, fmt, pkg     (after self-hosting is achieved)
```

---

*End of Fuse Self-Hosting Plan*

---

> **For AI agents:**
> This plan supersedes the Phase 9 section of the main implementation plan. It provides the granular task breakdown that the main plan intentionally omitted. Stage A must be completed before any Stage C work begins — there is no shortcut. The canonical test program remains `tests/fuse/milestone/four_functions.fuse`. Verify output parity across all three stages (Python, Rust, Fuse) at every milestone. A.6 and Stage E are deferred until after the bootstrap succeeds.
