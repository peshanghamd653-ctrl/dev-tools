//! Command detection and summary parsing behind the AI tool surface's
//! `run_tests` and `run_lint` tools.
//!
//! The same two-part shape serves both: a `detect_*` function decides *what*
//! to run from project-root markers, pure and testable without spawning
//! anything, and a `summarize_*` function turns raw stdout/stderr into a
//! count the model (and the tool-event log in the UI) can read at a glance
//! instead of re-deriving it from a wall of text on every call.
//!
//! Deliberately scoped to what this codebase itself needed to dogfood: Rust
//! (cargo) and JavaScript (npm/pnpm/yarn), plus Python and Go test detection
//! because they were cheap to add correctly. `dotnet test`, `mvn test` and
//! `gradle test` are not here — there was no real project available in this
//! session to verify a guessed invocation against, and a wrong guess
//! presented with the same confidence as a verified one is worse than no
//! detector at all. Python has no lint detector for the same reason, one
//! level further: pytest is close to a universal default for Python testing,
//! but linting is not — ruff, flake8 and pylint are all common and none is
//! the obvious single choice the way pytest, cargo and eslint are for their
//! ecosystems. Adding one needs the same thing every detector here got: real
//! output, actually read, not a remembered shape.

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

    if let Some(cmd) = detect_js_script(dir, "test") {
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

/// A `package.json` counts as a detected setup only if it actually declares
/// `script` — a JS project with none (a pure library with no suite, or one
/// that never wired up a linter) should not offer a command that would just
/// run npm's "Error: missing script" and look like a tool failure.
///
/// Shared by test and lint detection, parameterized on which script name to
/// look for — the only thing that differs between them.
fn detect_js_script(dir: &Path, script: &str) -> Option<DetectedCommand> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value["scripts"][script].as_str()?;

    let manager = if dir.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if dir.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    };
    // All three treat a declared script as runnable bare, no `run` needed,
    // for any script name — true of npm, yarn and pnpm alike.
    Some(DetectedCommand {
        ecosystem: manager,
        command: format!("{manager} {script}"),
    })
}

/// Find the one lint command for `dir`, on the same terms as
/// [`detect_test_command`] — including the same refusal, for the same
/// reason, when more than one setup is found.
///
/// The Rust command is deliberately `-D warnings`, not plain `cargo clippy`.
/// Plain clippy prints one "generated N warnings" line *per compiled
/// target* — this repository's own tree produces a `lib` line and a
/// separate `lib test` line whose count re-includes the first target's
/// warnings and annotates some as "(K duplicate)" — and summing those lines
/// naively double-counts. `-D warnings` turns every warning into a compile
/// error instead, which collapses all of that into one unambiguous final
/// line: `error: could not compile ... due to N previous errors`. This is
/// also the exact command this project's own CI and CONTRIBUTING.md already
/// run, so `run_lint` on DevOS itself checks the same thing the merge gate
/// does.
pub fn detect_lint_command(dir: &Path) -> Result<DetectedCommand, String> {
    let mut found = Vec::new();

    if dir.join("Cargo.toml").is_file() {
        let is_workspace = std::fs::read_to_string(dir.join("Cargo.toml"))
            .map(|s| s.contains("[workspace]"))
            .unwrap_or(false);
        found.push(DetectedCommand {
            ecosystem: "cargo",
            command: if is_workspace {
                "cargo clippy --workspace --all-targets -- -D warnings".into()
            } else {
                "cargo clippy --all-targets -- -D warnings".into()
            },
        });
    }

    if let Some(cmd) = detect_js_script(dir, "lint") {
        found.push(cmd);
    }

    if dir.join("go.mod").is_file() {
        found.push(DetectedCommand {
            ecosystem: "go",
            command: "go vet ./...".into(),
        });
    }

    match found.len() {
        0 => Err(format!(
            "no recognized lint setup at {} — looked for Cargo.toml, a package.json with a \
             \"lint\" script, and go.mod. Use run_command with an explicit lint command instead.",
            dir.display()
        )),
        1 => Ok(found.into_iter().next().expect("len checked above")),
        _ => {
            let listed: Vec<String> = found
                .iter()
                .map(|c| format!("{} (`{}`)", c.ecosystem, c.command))
                .collect();
            Err(format!(
                "more than one lint setup found at {}: {}. Pick one with run_command instead of \
                 guessing which this project means by \"lint\".",
                dir.display(),
                listed.join(", ")
            ))
        }
    }
}

/// Clean, or not — with a best-effort problem count when the tool's own
/// summary line is recognized.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LintSummary {
    pub clean: bool,
    pub problem_count: Option<u64>,
}

impl LintSummary {
    pub fn render(&self) -> String {
        if self.clean {
            return "clean — no problems found".to_string();
        }
        match self.problem_count {
            Some(1) => "1 problem found".to_string(),
            Some(n) => format!("{n} problems found"),
            None => "problems found (count not recognized — see output below)".to_string(),
        }
    }
}

/// Parse a lint summary from raw output plus whether the process exited
/// clean.
///
/// The exit code decides `clean` outright — both eslint and `cargo clippy --
/// -D warnings` exit nonzero on any problem, so this never depends on
/// spotting the right word in the text to know pass from fail, only to find
/// a *count* once fail is already established from the exit code.
///
/// Two shapes recognized with real confidence, both captured directly from
/// this repository while writing this: eslint's `✖ N problem(s) (M errors,
/// K warnings)` (searched as `" problem"`, a substring of both the singular
/// and plural form so one marker covers both) and clippy's `-D warnings`
/// failure, `error: could not compile \`x\` (target) due to N previous
/// error(s)` (searched the same way via `" previous error"`, and read from
/// the *last* matching line since a workspace build can print one such line
/// per crate that failed and the final line's count is the one to report).
pub fn summarize_lint_output(raw: &str, exit_clean: bool) -> LintSummary {
    if exit_clean {
        return LintSummary {
            clean: true,
            problem_count: Some(0),
        };
    }

    if let Some(n) = raw
        .lines()
        .rev()
        .find_map(|l| uint_immediately_before(l, " problem"))
    {
        return LintSummary {
            clean: false,
            problem_count: Some(n),
        };
    }

    if let Some(n) = raw
        .lines()
        .rev()
        .find_map(|l| uint_immediately_before(l, " previous error"))
    {
        return LintSummary {
            clean: false,
            problem_count: Some(n),
        };
    }

    LintSummary {
        clean: false,
        problem_count: None,
    }
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

    // --- detect_lint_command -----------------------------------------------

    #[test]
    fn detects_cargo_clippy_with_deny_warnings() {
        let dir = dir_with(&[("Cargo.toml", "[workspace]\n")]);
        let detected = detect_lint_command(dir.path()).expect("detected");
        assert_eq!(detected.ecosystem, "cargo");
        assert_eq!(
            detected.command,
            "cargo clippy --workspace --all-targets -- -D warnings"
        );
    }

    #[test]
    fn detects_a_js_lint_script() {
        let dir = dir_with(&[(
            "package.json",
            r#"{"scripts": {"test": "vitest run", "lint": "eslint ."}}"#,
        )]);
        let detected = detect_lint_command(dir.path()).expect("detected");
        assert_eq!(detected.command, "npm lint");
    }

    /// A `test` script alone must not be mistaken for a `lint` one — the two
    /// are detected independently even though they read the same file.
    #[test]
    fn a_test_script_alone_is_not_a_lint_setup() {
        let dir = dir_with(&[("package.json", r#"{"scripts": {"test": "vitest run"}}"#)]);
        assert!(detect_lint_command(dir.path()).is_err());
    }

    #[test]
    fn detects_go_vet() {
        let dir = dir_with(&[("go.mod", "module x\n")]);
        assert_eq!(
            detect_lint_command(dir.path()).unwrap().command,
            "go vet ./..."
        );
    }

    /// The same real, load-bearing ambiguity as test detection: this
    /// repository's own root would offer both.
    #[test]
    fn a_root_with_both_cargo_and_js_lint_refuses_to_guess() {
        let dir = dir_with(&[
            ("Cargo.toml", "[workspace]\n"),
            ("package.json", r#"{"scripts": {"lint": "eslint ."}}"#),
        ]);
        let err = detect_lint_command(dir.path()).unwrap_err();
        assert!(err.contains("cargo"), "{err}");
        assert!(err.contains("npm"), "{err}");
    }

    // --- summarize_lint_output ----------------------------------------------

    /// A clean exit is clean regardless of what the (empty, for eslint) text
    /// says — the exit code decides pass/fail, never the text.
    #[test]
    fn a_clean_exit_is_clean_even_with_empty_output() {
        let summary = summarize_lint_output("", true);
        assert!(summary.clean);
        assert_eq!(summary.problem_count, Some(0));
        assert_eq!(summary.render(), "clean — no problems found");
    }

    /// This is the exact text this session captured from a real `eslint`
    /// run against a deliberately broken scratch file, not a fabricated
    /// example.
    #[test]
    fn recognizes_a_real_eslint_summary_line() {
        let raw = "\
C:\\project\\src\\__lint_scratch__.ts
  2:9  error  'unused' is assigned a value but never used. Allowed unused vars must match /^_/u  @typescript-eslint/no-unused-vars

\u{2716} 1 problem (1 error, 0 warnings)
";
        let summary = summarize_lint_output(raw, false);
        assert_eq!(summary.problem_count, Some(1));
        assert_eq!(summary.render(), "1 problem found");
    }

    /// And this is the exact text this session captured from a real
    /// `cargo clippy -- -D warnings` run against a deliberately broken
    /// scratch function.
    #[test]
    fn recognizes_a_real_clippy_deny_warnings_failure() {
        let raw = "error: could not compile `devos-desktop` (lib test) due to 2 previous errors";
        let summary = summarize_lint_output(raw, false);
        assert_eq!(summary.problem_count, Some(2));
        assert_eq!(summary.render(), "2 problems found");
    }

    #[test]
    fn a_nonzero_exit_with_unrecognized_text_reports_dirty_with_no_count() {
        let summary = summarize_lint_output("something went sideways\n", false);
        assert!(!summary.clean);
        assert_eq!(summary.problem_count, None);
        assert_eq!(
            summary.render(),
            "problems found (count not recognized — see output below)"
        );
    }
}
