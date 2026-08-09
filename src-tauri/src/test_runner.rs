//! Test-command detection and summary parsing behind the AI tool surface's
//! `run_tests` tool.
//!
//! Two things live here, both pure and both testable without spawning a
//! process: [`detect_test_command`] decides *what* to run, and
//! [`summarize_test_output`] turns raw stdout/stderr into a pass/fail count
//! the model (and the tool-event log in the UI) can read at a glance instead
//! of re-deriving it from a wall of text on every call.
//!
//! Deliberately scoped to what this codebase itself needed to dogfood: Rust
//! (cargo) and JavaScript (npm/pnpm/yarn), plus Python and Go detection
//! because they were cheap to add correctly. `dotnet test`, `mvn test` and
//! `gradle test` are not here — there was no real project available in this
//! session to verify a guessed invocation against, and a wrong guess
//! presented with the same confidence as a verified one is worse than no
//! detector at all. Adding one needs the same thing cargo's and vitest's
//! parsing got: real output, actually read.

use std::path::Path;

/// What [`detect_test_command`] decided to run, and why — `ecosystem` is
/// surfaced to the model and the UI so "ran `cargo test --workspace`" reads
/// as a decision rather than a mystery command appearing from nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedCommand {
    pub ecosystem: &'static str,
    pub command: String,
}

/// Find the one test command for `dir`, or explain why none was chosen.
///
/// "Explain why none was chosen" covers two different failures on purpose.
/// Zero markers found is the obvious one. More than one is the other, and
/// it is not a hypothetical: this repository's own root has both a
/// workspace `Cargo.toml` *and* a `package.json` with a `test` script, so
/// silently preferring one language over the other would mean `run_tests`
/// on DevOS itself either never runs the frontend suite or never runs the
/// Rust one, depending on which arbitrary tie-break was picked — and
/// whichever way it lost, that half of an actual regression could pass this
/// tool's `run_tests` call in a bug-fixing loop while being completely
/// unexercised. An error naming both and asking the model to point
/// `run_command` at the specific one is a worse first message and a much
/// better second one.
pub fn detect_test_command(dir: &Path) -> Result<DetectedCommand, String> {
    let mut found = Vec::new();

    if dir.join("Cargo.toml").is_file() {
        let is_workspace = std::fs::read_to_string(dir.join("Cargo.toml"))
            .map(|s| s.contains("[workspace]"))
            .unwrap_or(false);
        found.push(DetectedCommand {
            ecosystem: "cargo",
            command: if is_workspace {
                "cargo test --workspace".into()
            } else {
                "cargo test".into()
            },
        });
    }

    if let Some(cmd) = detect_js(dir) {
        found.push(cmd);
    }

    if dir.join("pyproject.toml").is_file()
        || dir.join("pytest.ini").is_file()
        || dir.join("setup.cfg").is_file()
    {
        found.push(DetectedCommand {
            ecosystem: "pytest",
            command: "pytest".into(),
        });
    }

    if dir.join("go.mod").is_file() {
        found.push(DetectedCommand {
            ecosystem: "go",
            command: "go test ./...".into(),
        });
    }

    match found.len() {
        0 => Err(format!(
            "no recognized test setup at {} — looked for Cargo.toml, a package.json with a \
             \"test\" script, pyproject.toml/pytest.ini/setup.cfg, and go.mod. Use run_command \
             with an explicit test command instead.",
            dir.display()
        )),
        1 => Ok(found.into_iter().next().expect("len checked above")),
        _ => {
            let listed: Vec<String> = found
                .iter()
                .map(|c| format!("{} (`{}`)", c.ecosystem, c.command))
                .collect();
            Err(format!(
                "more than one test setup found at {}: {}. Pick one with run_command instead of \
                 guessing which this project means by \"the tests\".",
                dir.display(),
                listed.join(", ")
            ))
        }
    }
}

/// A `package.json` counts as a detected test setup only if it actually
/// declares a `test` script — a JS project with none (a pure library with no
/// suite, or one that only lints) should not offer a command that would just
/// run npm's "Error: missing script: test" and look like a tool failure.
fn detect_js(dir: &Path) -> Option<DetectedCommand> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value["scripts"]["test"].as_str()?;

    let manager = if dir.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if dir.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    };
    // All three treat `test` as a reserved script name runnable bare, no
    // `run` needed — true of npm, yarn and pnpm alike.
    Some(DetectedCommand {
        ecosystem: manager,
        command: format!("{manager} test"),
    })
}

/// A pass/fail count pulled out of raw test-runner output, or the honest
/// absence of one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TestSummary {
    pub passed: Option<u64>,
    pub failed: Option<u64>,
}

impl TestSummary {
    /// One line for the top of the tool result — the part a model or a
    /// glance at the tool-event log should be able to act on without reading
    /// the raw dump beneath it.
    pub fn render(&self) -> String {
        match (self.passed, self.failed) {
            (None, None) => {
                "could not recognize a pass/fail summary in the output below".to_string()
            }
            (passed, failed) => {
                let passed = passed.unwrap_or(0);
                match failed {
                    Some(0) => format!("{passed} passed"),
                    Some(failed) => format!("{passed} passed, {failed} failed"),
                    None => format!("{passed} passed (failure count not recognized)"),
                }
            }
        }
    }
}

/// Parse a pass/fail summary out of raw test-runner output.
///
/// Two shapes are recognized with actual confidence, both taken from output
/// this codebase's own suites produce — read directly, repeatedly, over the
/// course of the session this was written in, including real failing runs:
///
///   cargo test, one line per compiled test binary, summed across every one
///   (a `--workspace` run emits many):
///     "test result: ok. 63 passed; 0 failed; 0 ignored; ..."
///     "test result: FAILED. 62 passed; 1 failed; 0 ignored; ..."
///
///   vitest, one summary line:
///     " Tests  401 passed (401)"
///
/// The vitest *failing* shape was not directly observed this session — every
/// frontend run in it passed. Rather than guess at its exact punctuation,
/// this treats a "Tests" line with a recognized passed count and no "failed"
/// substring as zero failures, on the general convention (common across
/// several test runners, vitest included so far as documented rather than
/// witnessed here) of a summary line omitting a category once it hits zero.
/// That is one inferred detail in an otherwise-verified parser, and it is
/// flagged here rather than presented with the same confidence as the rest.
///
/// Anything else falls back to `passed: None, failed: None` — reported as
/// unrecognized, not guessed at.
pub fn summarize_test_output(raw: &str) -> TestSummary {
    if let Some(summary) = sum_cargo_lines(raw) {
        return summary;
    }
    if let Some(summary) = vitest_tests_line(raw) {
        return summary;
    }
    TestSummary::default()
}

fn sum_cargo_lines(raw: &str) -> Option<TestSummary> {
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut matched = false;
    for line in raw.lines() {
        if !line.trim_start().starts_with("test result:") {
            continue;
        }
        let (Some(p), Some(f)) = (
            uint_immediately_before(line, " passed"),
            uint_immediately_before(line, " failed"),
        ) else {
            continue;
        };
        matched = true;
        passed += p;
        failed += f;
    }
    matched.then_some(TestSummary {
        passed: Some(passed),
        failed: Some(failed),
    })
}

fn vitest_tests_line(raw: &str) -> Option<TestSummary> {
    let line = raw
        .lines()
        .find(|l| l.trim_start().starts_with("Tests") && l.contains(" passed"))?;
    let passed = uint_immediately_before(line, " passed")?;
    // See the doc comment on `summarize_test_output`: this is the one
    // inferred (not directly observed) branch.
    let failed = uint_immediately_before(line, " failed").unwrap_or(0);
    Some(TestSummary {
        passed: Some(passed),
        failed: Some(failed),
    })
}

/// The unsigned integer made of the digits immediately preceding `marker` in
/// `s` — `"...ok. 63 passed;"` with marker `" passed"` yields `63`. `None` if
/// `marker` is absent or nothing digit-shaped sits directly before it, which
/// is the correct outcome for a line whose format does not match rather than
/// an error worth propagating: the caller treats it as "not this shape".
fn uint_immediately_before(s: &str, marker: &str) -> Option<u64> {
    let idx = s.find(marker)?;
    let digits: String = s[..idx]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.chars().rev().collect::<String>().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            fs::write(dir.path().join(name), content).unwrap();
        }
        dir
    }

    #[test]
    fn detects_a_cargo_workspace() {
        let dir = dir_with(&[("Cargo.toml", "[workspace]\nmembers = [\"a\"]\n")]);
        let detected = detect_test_command(dir.path()).expect("detected");
        assert_eq!(detected.ecosystem, "cargo");
        assert_eq!(detected.command, "cargo test --workspace");
    }

    #[test]
    fn detects_a_single_cargo_crate_without_the_workspace_flag() {
        let dir = dir_with(&[("Cargo.toml", "[package]\nname = \"x\"\n")]);
        let detected = detect_test_command(dir.path()).expect("detected");
        assert_eq!(detected.command, "cargo test");
    }

    #[test]
    fn detects_pnpm_over_npm_when_a_pnpm_lockfile_is_present() {
        let dir = dir_with(&[
            ("package.json", r#"{"scripts": {"test": "vitest run"}}"#),
            ("pnpm-lock.yaml", ""),
        ]);
        let detected = detect_test_command(dir.path()).expect("detected");
        assert_eq!(detected.ecosystem, "pnpm");
        assert_eq!(detected.command, "pnpm test");
    }

    #[test]
    fn falls_back_to_npm_with_no_recognized_lockfile() {
        let dir = dir_with(&[("package.json", r#"{"scripts": {"test": "jest"}}"#)]);
        let detected = detect_test_command(dir.path()).expect("detected");
        assert_eq!(detected.ecosystem, "npm");
    }

    /// A package.json that declares no test script is not a test setup —
    /// running it would just surface npm's own "missing script" error
    /// dressed up as a tool result.
    #[test]
    fn a_package_json_with_no_test_script_is_not_detected() {
        let dir = dir_with(&[("package.json", r#"{"scripts": {"build": "tsc"}}"#)]);
        assert!(detect_test_command(dir.path()).is_err());
    }

    #[test]
    fn detects_pytest_markers() {
        for marker in ["pyproject.toml", "pytest.ini", "setup.cfg"] {
            let dir = dir_with(&[(marker, "")]);
            let detected = detect_test_command(dir.path()).expect("detected");
            assert_eq!(detected.command, "pytest", "marker file: {marker}");
        }
    }

    #[test]
    fn detects_go() {
        let dir = dir_with(&[("go.mod", "module x\n")]);
        assert_eq!(
            detect_test_command(dir.path()).unwrap().command,
            "go test ./..."
        );
    }

    #[test]
    fn nothing_recognized_is_an_error_naming_what_was_checked() {
        let dir = tempfile::tempdir().unwrap();
        let err = detect_test_command(dir.path()).unwrap_err();
        assert!(err.contains("Cargo.toml"));
        assert!(err.contains("go.mod"));
    }

    /// The real, load-bearing case: this repository's own root has both a
    /// workspace Cargo.toml and a package.json with a test script. Silently
    /// picking one would leave the other language's regressions unexercised
    /// by `run_tests` on this exact project.
    #[test]
    fn a_root_with_both_cargo_and_js_refuses_to_guess() {
        let dir = dir_with(&[
            ("Cargo.toml", "[workspace]\n"),
            ("package.json", r#"{"scripts": {"test": "vitest run"}}"#),
        ]);
        let err = detect_test_command(dir.path()).unwrap_err();
        assert!(err.contains("cargo"), "{err}");
        assert!(
            err.contains("npm") || err.contains("pnpm") || err.contains("yarn"),
            "{err}"
        );
    }

    // --- summarize_test_output -------------------------------------------

    #[test]
    fn sums_cargo_test_across_every_binary_in_a_workspace_run() {
        let raw = "\
Compiling devos-kernel v0.1.0
test result: ok. 63 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.12s

Running unittests src\\lib.rs
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
";
        let summary = summarize_test_output(raw);
        assert_eq!(summary.passed, Some(87));
        assert_eq!(summary.failed, Some(0));
        assert_eq!(summary.render(), "87 passed");
    }

    /// This is the exact text this session read from a real failing run
    /// earlier today (`a_restore_that_cannot_preserve_the_current_database_
    /// is_refused`), not a fabricated example.
    #[test]
    fn recognizes_a_real_cargo_failure_line() {
        let raw = "test result: FAILED. 62 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.15s";
        let summary = summarize_test_output(raw);
        assert_eq!(summary.passed, Some(62));
        assert_eq!(summary.failed, Some(1));
        assert_eq!(summary.render(), "62 passed, 1 failed");
    }

    #[test]
    fn recognizes_the_vitest_tests_line() {
        let raw = " Test Files  35 passed (35)\n      Tests  401 passed (401)\n";
        let summary = summarize_test_output(raw);
        assert_eq!(summary.passed, Some(401));
        // Inferred, not observed — see the doc comment. Pinned here so a
        // future change to that inference is a deliberate edit, not a
        // silent drift.
        assert_eq!(summary.failed, Some(0));
    }

    #[test]
    fn unrecognized_output_says_so_rather_than_guessing() {
        let summary = summarize_test_output("Ran 12 examples, 0 failures\n");
        assert_eq!(summary, TestSummary::default());
        assert_eq!(
            summary.render(),
            "could not recognize a pass/fail summary in the output below"
        );
    }
}
