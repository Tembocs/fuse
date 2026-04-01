# Fuse Standard Library

Standard library definitions written in Fuse.

## Availability by stage

| Directory | Available from | Description |
|---|---|---|
| `core/` | Stage 0 | Core types: Result, Option, List, String, Int, Float, Bool |
| `full/` | Stage 1 | Concurrency and performance: Chan, Shared, Timer, SIMD, Http |

## Stage 0 note

The Stage 0 Python interpreter implements Core stdlib natively in Python for
practicality. The API surface defined in these `.fuse` files is the contract —
the Python implementation must match it exactly.

The Stage 1 Rust compiler compiles these files as ordinary Fuse source.
