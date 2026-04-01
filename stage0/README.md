# Stage 0 — Python Tree-Walking Interpreter

Implements **Fuse Core** only. No concurrency primitives.

## Prerequisites

- Python 3.11 or later
- No third-party dependencies

## Run a file

```bash
python src/main.py <file.fuse>
```

## Run the REPL

```bash
python src/main.py --repl
```

## Check without running

```bash
python src/main.py --check <file.fuse>
```

## Run the test suite

```bash
python tests/run_tests.py
```

## Milestone

```bash
python src/main.py ../../tests/fuse/milestone/four_functions.fuse
```

Stage 0 is complete when this produces the expected output.
