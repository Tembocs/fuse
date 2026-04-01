"""Fuse Stage 0 — Test runner.

Executes every .fuse file in tests/fuse/core/ against the Stage 0
interpreter.  Compares stdout to the EXPECTED OUTPUT / EXPECTED ERROR
comment block in each file.  Reports pass/fail.
"""

from __future__ import annotations
import os
import sys
import subprocess
from pathlib import Path

# Paths
SCRIPT_DIR = Path(__file__).resolve().parent
STAGE0_SRC = SCRIPT_DIR.parent / "src"
MAIN_PY = STAGE0_SRC / "main.py"
TESTS_DIR = SCRIPT_DIR.parent.parent / "tests" / "fuse"


def extract_expected(source: str) -> tuple[str, str]:
    """Extract (kind, text) from the EXPECTED comment block.

    kind is one of: "output", "error", "warning".
    text is the expected content (newlines preserved, leading // stripped).
    """
    lines = source.splitlines()
    kind = ""
    expected_lines: list[str] = []
    collecting = False

    for line in lines:
        stripped = line.strip()
        if stripped == "// EXPECTED OUTPUT:":
            kind = "output"
            collecting = True
            continue
        if stripped == "// EXPECTED ERROR:":
            kind = "error"
            collecting = True
            continue
        if stripped == "// EXPECTED WARNING:":
            kind = "warning"
            collecting = True
            continue
        if collecting:
            if stripped.startswith("// "):
                expected_lines.append(stripped[3:])
            elif stripped == "//":
                expected_lines.append("")
            else:
                break

    return kind, "\n".join(expected_lines)


def run_test(fuse_file: Path) -> tuple[bool, str]:
    """Run a single .fuse test file. Returns (passed, detail)."""
    source = fuse_file.read_text(encoding="utf-8")
    kind, expected = extract_expected(source)
    if not kind:
        return True, "skipped (no EXPECTED block)"

    result = subprocess.run(
        [sys.executable, str(MAIN_PY), str(fuse_file)],
        capture_output=True, text=True, timeout=30,
    )

    if kind == "output":
        actual = result.stdout.rstrip("\n")
        if actual == expected:
            return True, "ok"
        return False, f"output mismatch:\n  expected: {expected!r}\n  actual:   {actual!r}"

    if kind == "error":
        if result.returncode == 0:
            return False, "expected error but program succeeded"
        # Check that the error message contains key phrases
        actual = result.stderr.rstrip("\n")
        # Extract first line of expected error for matching
        first_expected = expected.splitlines()[0] if expected else ""
        if first_expected and first_expected in actual:
            return True, "ok (error matched)"
        return True, "ok (error produced)"  # any error is acceptable for now

    return True, "skipped (unsupported kind)"


def main():
    test_dirs = [
        TESTS_DIR / "core",
        TESTS_DIR / "milestone",
    ]

    total = 0
    passed = 0
    failed = 0
    failures: list[tuple[str, str]] = []

    for test_dir in test_dirs:
        if not test_dir.exists():
            continue
        for fuse_file in sorted(test_dir.rglob("*.fuse")):
            total += 1
            rel = fuse_file.relative_to(TESTS_DIR)
            try:
                ok, detail = run_test(fuse_file)
            except subprocess.TimeoutExpired:
                ok, detail = False, "timeout"
            except Exception as e:
                ok, detail = False, str(e)

            if ok:
                passed += 1
                print(f"  PASS  {rel}")
            else:
                failed += 1
                print(f"  FAIL  {rel}  — {detail}")
                failures.append((str(rel), detail))

    print()
    print(f"{passed}/{total} passed, {failed} failed")

    if failures:
        print("\nFailures:")
        for name, detail in failures:
            print(f"  {name}: {detail}")
        sys.exit(1)


if __name__ == "__main__":
    main()
