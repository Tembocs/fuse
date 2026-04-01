// Fuse Stage 1 — Test runner.
// Executes tests/fuse/ against the Stage 1 compiler.
//
// For each .fuse test file:
//   - Parse `// EXPECTED OUTPUT:`, `// EXPECTED ERROR:`, or `// EXPECTED WARNING:` blocks
//   - Run `fusec --check <file>` and capture stderr
//   - Valid tests (EXPECTED OUTPUT): must exit 0, no errors
//   - Error tests (EXPECTED ERROR): must exit 1, stderr matches expected error lines
//   - Warning tests (EXPECTED WARNING): must exit 0, stderr matches expected warning lines

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug)]
enum TestKind {
    Output,   // EXPECTED OUTPUT — valid file, should pass
    Error,    // EXPECTED ERROR — should fail with specific errors
    Warning,  // EXPECTED WARNING — should pass with specific warnings
}

struct TestCase {
    path: PathBuf,
    kind: TestKind,
    expected_lines: Vec<String>,
}

fn discover_tests(root: &str) -> Vec<TestCase> {
    let mut tests = Vec::new();
    let root = PathBuf::from(root);
    walk_dir(&root, &mut tests);
    tests.sort_by(|a, b| a.path.cmp(&b.path));
    tests
}

fn walk_dir(dir: &PathBuf, tests: &mut Vec<TestCase>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_dir(&path, tests);
            } else if path.extension().map(|e| e == "fuse").unwrap_or(false) {
                if let Some(tc) = parse_test_file(&path) {
                    tests.push(tc);
                }
            }
        }
    }
}

fn parse_test_file(path: &PathBuf) -> Option<TestCase> {
    let content = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();

    let kind = if lines.iter().any(|l| l.starts_with("// EXPECTED ERROR:")) {
        TestKind::Error
    } else if lines.iter().any(|l| l.starts_with("// EXPECTED WARNING:")) {
        TestKind::Warning
    } else if lines.iter().any(|l| l.starts_with("// EXPECTED OUTPUT:")) {
        TestKind::Output
    } else {
        return None; // No expected block — skip
    };

    let expected_lines: Vec<String> = match &kind {
        TestKind::Error => extract_expected(&lines, "// EXPECTED ERROR:"),
        TestKind::Warning => extract_expected(&lines, "// EXPECTED WARNING:"),
        TestKind::Output => extract_expected(&lines, "// EXPECTED OUTPUT:"),
    };

    Some(TestCase { path: path.clone(), kind, expected_lines })
}

fn extract_expected(lines: &[&str], marker: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_block = false;

    for line in lines {
        if line.starts_with(marker) {
            in_block = true;
            continue;
        }
        if in_block {
            if line.starts_with("//") {
                // Strip "// " prefix
                let content = line.strip_prefix("// ").unwrap_or(
                    line.strip_prefix("//").unwrap_or("")
                );
                result.push(content.to_string());
            } else {
                break; // End of expected block
            }
        }
    }
    result
}

fn fusec_path() -> PathBuf {
    // Look for the fusec binary relative to the test executable
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // remove test binary name
    path.pop(); // remove deps/
    path.push("fusec");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    if path.exists() {
        return path;
    }
    // Fallback: try current directory
    PathBuf::from("target/debug/fusec")
}

fn tests_root() -> PathBuf {
    // Navigate from stage1/ up to repo root, then into tests/fuse/
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().join("tests").join("fuse")
}

#[test]
fn run_fuse_tests() {
    let root = tests_root();
    let fusec = fusec_path();

    assert!(root.exists(), "tests/fuse directory not found at {:?}", root);
    assert!(fusec.exists(), "fusec binary not found at {:?}", fusec);

    let tests = discover_tests(root.to_str().unwrap());
    assert!(!tests.is_empty(), "no test files found");

    let mut passed = 0;
    let mut failed = 0;
    let mut failures = Vec::new();

    for tc in &tests {
        let name = tc.path.file_name().unwrap().to_str().unwrap();

        let ok = match tc.kind {
            TestKind::Output => {
                // Run the program and compare stdout against expected output
                let result = Command::new(&fusec)
                    .arg(&tc.path)
                    .output()
                    .expect("failed to run fusec");
                let exit_code = result.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&result.stdout).to_string();
                let stderr = String::from_utf8_lossy(&result.stderr).to_string();

                if exit_code != 0 {
                    failures.push(format!("FAIL {name}: expected exit 0, got {exit_code}\n  stderr: {stderr}"));
                    false
                } else {
                    let expected = tc.expected_lines.join("\n");
                    let actual = stdout.replace('\r', "").trim_end().to_string();
                    if actual == expected {
                        true
                    } else {
                        failures.push(format!(
                            "FAIL {name}: output mismatch\n  expected: {:?}\n  actual:   {:?}",
                            expected, actual
                        ));
                        false
                    }
                }
            }
            TestKind::Error => {
                // Check mode: expect exit 1
                let result = Command::new(&fusec)
                    .arg("--check")
                    .arg(&tc.path)
                    .output()
                    .expect("failed to run fusec");
                let exit_code = result.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&result.stderr).to_string();

                if exit_code == 0 {
                    failures.push(format!("FAIL {name}: expected exit 1 (error), got 0"));
                    false
                } else {
                    check_expected_output(&stderr, &tc.expected_lines, name, &mut failures)
                }
            }
            TestKind::Warning => {
                // Check mode: expect exit 0 with warnings
                let result = Command::new(&fusec)
                    .arg("--check")
                    .arg(&tc.path)
                    .output()
                    .expect("failed to run fusec");
                let exit_code = result.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&result.stderr).to_string();

                if exit_code != 0 {
                    failures.push(format!("FAIL {name}: expected exit 0 (warning only), got {exit_code}\n  stderr: {stderr}"));
                    false
                } else {
                    check_expected_output(&stderr, &tc.expected_lines, name, &mut failures)
                }
            }
        };

        if ok {
            passed += 1;
            eprintln!("  PASS {name}");
        } else {
            failed += 1;
            eprintln!("  FAIL {name}");
        }
    }

    eprintln!("\n{passed} passed, {failed} failed, {} total", passed + failed);

    if !failures.is_empty() {
        eprintln!("\nFailure details:");
        for f in &failures {
            eprintln!("  {f}");
        }
        panic!("{failed} test(s) failed");
    }
}

fn check_expected_output(
    stderr: &str,
    expected_lines: &[String],
    name: &str,
    failures: &mut Vec<String>,
) -> bool {
    // Each expected line should be a substring of stderr
    let stderr_normalized = stderr.replace('\r', "");
    for expected in expected_lines {
        if !stderr_normalized.contains(expected.as_str()) {
            failures.push(format!(
                "FAIL {name}: expected stderr to contain:\n    \"{expected}\"\n  actual stderr:\n    \"{stderr_normalized}\""
            ));
            return false;
        }
    }
    true
}
