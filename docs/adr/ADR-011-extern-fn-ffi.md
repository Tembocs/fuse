# ADR-011: FFI uses `extern fn`

## Status

Accepted

## Context

The self-hosting compiler (Stage 2) must call Cranelift — a Rust library — from Fuse code. This requires a foreign function interface. The FFI must be simple enough to use without ceremony but explicit enough that the developer knows they're leaving Fuse's safety guarantees.

## Decision

Foreign functions are declared with `extern fn`:

```fuse
extern fn fuse_rt_println(val: Ptr) -> ()
extern fn fuse_rt_int(v: Int) -> Ptr
```

Related declarations can be grouped in `extern` blocks:

```fuse
extern "fuse-runtime" {
  fn fuse_rt_println(val: Ptr) -> ()
  fn fuse_rt_int(v: Int) -> Ptr
}
```

FFI types: `Ptr` (raw 64-bit pointer), `Byte` (u8), plus `Int`, `Float`, `Bool` passed as raw values.

## Rationale

`extern` is universally understood. C, Rust, Go, and C# all use it. Reading `extern fn` immediately communicates "this function is defined outside Fuse." The keyword requires no learning — it means what it has always meant across languages.

Annotations (`@foreign`) imply the function has a Fuse body that is modified by the annotation. That is the opposite of what FFI does — there is no body.

## Alternatives rejected

- `@foreign fn` — annotation implies body exists
- `native fn` — ambiguous with "native code generation" (which is what the compiler itself does)
- `#[link]` — Rust-specific notation, unfamiliar outside Rust
- No explicit FFI — using string-based dynamic dispatch is too slow and loses all compile-time checking
