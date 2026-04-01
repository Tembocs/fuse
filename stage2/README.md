# Stage 2 — Self-Hosting Fuse Compiler

The Fuse compiler written in Fuse Core.

## Entry condition

Stage 1 milestone is met. The language is stable enough that writing a compiler
in it is practical — every feature the compiler needs exists in the language.

## Milestone

```bash
./fusec2-stage2 stage2/src/main.fuse -o fusec2-verified
diff <(sha256sum fusec2-stage2) <(sha256sum fusec2-verified)
# no output — binaries are identical
```

Stage 2 is complete when the Fuse compiler compiles itself and the
reproducibility check passes.
