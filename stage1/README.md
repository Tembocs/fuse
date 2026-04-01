# Stage 1 — Rust Compiler with Cranelift Backend

Implements **Fuse Full** — everything in Fuse Core plus concurrency, async, and SIMD.

## Prerequisites

- Rust (stable toolchain)
- Cranelift (via cargo dependency)

## Build

```bash
cargo build --release
```

## Compile a Fuse program

```bash
cargo run --bin fusec -- <file.fuse>
```

## Run the test suite

```bash
cargo test
```

## Milestone

```bash
cargo run --bin fusec -- ../../tests/fuse/milestone/four_functions.fuse
./four_functions
```

Stage 1 is complete when this compiles and produces the expected output.
