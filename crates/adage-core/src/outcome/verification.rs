//! Runner-specific evidence. A successful shell pipeline is not a successful test run.
//! These summaries are transcript evidence, not an execution certificate or a task-quality grade.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// What the identified verification invocation established. Unknown and no-tests are neither
/// successful checks nor failed-test examples for a recovery miner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Passed,
    Failed,
    SetupFailure,
    NoTests,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    pub outcome: VerificationOutcome,
    /// Actual cases reported as run, where the format supplies them. Never files collected,
    /// skipped cases, or a build's successful exit. A missing count is not zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_run: Option<u64>,
}

impl VerificationReport {
    /// A known command outcome, for consumers that only have a success/failure channel.
    pub fn status(self) -> Option<bool> {
        match self.outcome {
            VerificationOutcome::Passed => Some(true),
            VerificationOutcome::Failed | VerificationOutcome::SetupFailure => Some(false),
            VerificationOutcome::NoTests | VerificationOutcome::Unknown => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Runner {
    Pytest,
    Unittest,
    CargoTest,
    CargoBuild,
    Javascript,
    Typecheck,
    VerifyScript,
    Go,
}

fn base(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

// This is deliberately narrower than action classification. Inline Python, custom shell scripts,
// and make targets have no known report format merely because their arguments mention a runner.
fn runner(argv: &[String]) -> Option<Runner> {
    let (p, args) = argv.split_first()?;
    let first = args.first().map(String::as_str).unwrap_or("");
    match base(p) {
        "pytest" | "py.test" => Some(Runner::Pytest),
        "cargo" => match first {
            "test" => Some(Runner::CargoTest),
            "build" | "check" | "clippy" => Some(Runner::CargoBuild),
            _ => None,
        },
        "go" if first == "test" => Some(Runner::Go),
        "tsc" => Some(Runner::Typecheck),
        "jest" | "vitest" => Some(Runner::Javascript),
        "npm" | "pnpm" | "yarn" | "bun"
            if matches!(first, "test" | "t" | "jest" | "vitest")
                || (matches!(first, "run" | "run-script") && args.get(1).is_some_and(|s| s == "test")) =>
        {
            Some(Runner::Javascript)
        }
        "npm" | "pnpm" | "yarn" | "bun"
            if matches!(first, "exec" | "dlx" | "x")
                && args.get(1).is_some_and(|s| matches!(s.as_str(), "jest" | "vitest")) =>
        {
            Some(Runner::Javascript)
        }
        "npx" | "bunx" if matches!(first, "jest" | "vitest") => Some(Runner::Javascript),
        "npm" | "pnpm" | "yarn" | "bun" => {
            let script = if matches!(first, "run" | "run-script") {
                args.get(1).map(String::as_str).unwrap_or("")
            } else {
                first
            };
            match script {
                "typecheck" | "type-check" => Some(Runner::Typecheck),
                "verify" => Some(Runner::VerifyScript),
                _ => None,
            }
        }
        "uv" | "poetry" | "pdm" if first == "run" => runner(&args[1..]),
        _ if crate::action::is_python(p) => {
            // Only interpreter switches before -m, never a -c program that contains these words.
            let at = args.iter().position(|a| a == "-m")?;
            if !args[..at].iter().all(|s| matches!(s.as_str(), "-B" | "-I" | "-E" | "-s" | "-u")) {
                return None;
            }
            match args.get(at + 1)?.as_str() {
                "pytest" | "py.test" => Some(Runner::Pytest),
                "unittest" => Some(Runner::Unittest),
                _ => None,
            }
        }
        _ => None,
    }
}

fn report(outcome: VerificationOutcome, tests_run: Option<u64>) -> VerificationReport {
    VerificationReport { outcome, tests_run }
}

/// Deliberate discovery/help modes do not demonstrate a failed attempt to execute tests.
pub fn verification_is_discovery(argv: &[String]) -> bool {
    argv.iter().any(|a| {
        matches!(
            a.as_str(),
            "--help"
                | "-h"
                | "--version"
                | "--collect-only"
                | "--co"
                | "--list"
                | "--listTests"
                | "--showConfig"
                | "--no-run"
                | "--list-tests"
                | "--listTests=true"
        ) || a.starts_with("--collect-only=")
            || a == "-list"
            || a.starts_with("-list=")
    }) || argv.first().is_some_and(|p| base(p) == "vitest") && argv.get(1).is_some_and(|s| s == "list")
}

const MAX_REPORT: usize = 64 * 1024;
static ANSI: Lazy<Regex> = Lazy::new(|| Regex::new(r"\x1b\[[0-9;]*m").unwrap());
static TRUNCATED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?im)^(?:warning: truncated output\b|output (?:was )?truncated\b|\[output truncated\]|\.\.\.\s*[0-9]+ (?:tokens|characters) truncated\s*\.\.\.)").unwrap()
});

fn complete_text(output: &str) -> Option<std::borrow::Cow<'_, str>> {
    if output.len() > MAX_REPORT {
        return None;
    }
    let clean = ANSI.replace_all(output, "");
    if clean.contains('\x1b') || clean.contains('\0') || TRUNCATED.is_match(&clean) {
        return None;
    }
    Some(clean)
}
static PYTEST: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:=+\s*)?(.+?)(?:\s+in\s+[0-9]+(?:\.[0-9]+)?s(?:\s*\([^)]+\))?)?(?:\s*=+)?$").unwrap());
static COUNT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^([0-9]+) (passed|failed|errors?|skipped|xfailed|xpassed|deselected|warnings?|todo|pending)$").unwrap()
});
static CARGO: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed;(?: .*)?$").unwrap());
static UNIT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^Ran ([0-9]+) tests? in [0-9.]+s$").unwrap());
static UNIT_OK: Lazy<Regex> = Lazy::new(|| Regex::new(r"^OK(?: \(skipped=([0-9]+)\))?$").unwrap());
static UNIT_FAILED: Lazy<Regex> = Lazy::new(|| Regex::new(r"^FAILED \((.+)\)$").unwrap());
static JS: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(Tests?:|Test Suites:|Tests|Test Files)\s+(.+)$").unwrap());
static JS_COUNT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^([0-9]+) (passed|failed|skipped|todo|pending|total)(?: \([0-9]+\))?$").unwrap());
static PYTEST_EMPTY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:=+\s*)?no tests (?:ran|collected) in [0-9]+(?:\.[0-9]+)?s(?:\s*=+)?$").unwrap());
static JS_EMPTY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^No test(?:s| files) found, exiting with code [01]$").unwrap());
static GO: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(ok|FAIL)\s+\S+\s+(?:[0-9.]+s|\(cached\))(?:\s.*)?$").unwrap());
static CARGO_ERROR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^error(?:\[E[0-9]+\])?: (?:could not compile|cannot compile|aborting due to|failed to compile|missing field|no field|mismatched types|unresolved import|cannot find|expected |failed to run custom build command)").unwrap()
});
static TYPESCRIPT_ERROR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:[^\s].*?(?:\([0-9]+,[0-9]+\)|:[0-9]+:[0-9]+):?\s*(?:-\s*)?)?error TS[0-9]{4,}: ").unwrap()
});

#[derive(Default)]
struct Counts {
    passed: u64,
    failed: u64,
    errors: u64,
    skipped: u64,
    seen: bool,
}

impl Counts {
    fn add(&mut self, n: u64, kind: &str) {
        self.seen = true;
        match kind {
            "passed" | "xpassed" => self.passed = self.passed.saturating_add(n),
            "failed" => self.failed = self.failed.saturating_add(n),
            "error" | "errors" => self.errors = self.errors.saturating_add(n),
            _ => self.skipped = self.skipped.saturating_add(n),
        }
    }

    fn finish(&self) -> VerificationReport {
        let n = self.passed.saturating_add(self.failed);
        let outcome = if self.failed > 0 {
            VerificationOutcome::Failed
        } else if self.errors > 0 {
            VerificationOutcome::SetupFailure
        } else if n > 0 {
            VerificationOutcome::Passed
        } else if self.seen {
            VerificationOutcome::NoTests
        } else {
            VerificationOutcome::Unknown
        };
        report(outcome, self.seen.then_some(n))
    }
}

/// Read a recognized runner's own report. None means unsupported argv; Unknown means the runner
/// was recognized but its output did not establish an outcome. No transport status is inferred.
/// The caller must establish that this output belongs to this invocation, not a read or a sibling
/// in a compound shell call. `verification_for_command` does that for canonical Bash calls.
pub fn verification_report(argv: &[String], output: &str) -> Option<VerificationReport> {
    report_details(argv, output).map(|(report, _)| report)
}

fn report_details(argv: &[String], output: &str) -> Option<(VerificationReport, bool)> {
    let runner = runner(argv)?;
    if verification_is_discovery(argv) {
        return Some((report(VerificationOutcome::NoTests, Some(0)), false));
    }
    let Some(clean) = complete_text(output) else {
        return Some((report(VerificationOutcome::Unknown, None), false));
    };
    let mut counts = Counts::default();
    let mut failed = false;
    let mut setup = false;
    let mut built = false;
    let mut unit_n: Option<u64> = None;
    let mut unit_end = None;
    let mut unit_skipped = 0;
    let mut unit_errors = 0;
    let mut unit_failures = 0;
    let mut go_ok = false;
    for raw in clean.lines() {
        let line = raw.trim();
        match runner {
            Runner::Pytest => {
                if PYTEST_EMPTY.is_match(line) {
                    counts.seen = true;
                }
                if line.contains("Interrupted:") && line.contains("during collection") {
                    setup = true;
                }
                if let Some(c) = PYTEST.captures(line) {
                    let parts: Vec<_> = c[1].split(", ").map(|s| COUNT.captures(s)).collect();
                    if !parts.is_empty() && parts.iter().all(Option::is_some) {
                        for c in parts.into_iter().flatten() {
                            counts.add(c[1].parse().unwrap_or(u64::MAX), &c[2]);
                        }
                    }
                }
            }
            Runner::CargoTest | Runner::CargoBuild => {
                if let Some(c) = CARGO.captures(line) {
                    counts.add(c[2].parse().unwrap_or(u64::MAX), "passed");
                    counts.add(c[3].parse().unwrap_or(u64::MAX), "failed");
                    failed |= &c[1] == "FAILED";
                }
                failed |= CARGO_ERROR.is_match(line);
                built |= line.starts_with("Finished `") && line.contains(" profile ");
            }
            Runner::Javascript | Runner::VerifyScript => {
                failed |= runner == Runner::VerifyScript && TYPESCRIPT_ERROR.is_match(line);
                if JS_EMPTY.is_match(line) {
                    counts.seen = true;
                }
                if let Some(c) = JS.captures(line) {
                    let case_line = matches!(&c[1], "Tests:" | "Test:" | "Tests");
                    // Vitest uses bars and a total suffix; Jest uses commas and a total clause.
                    let body = c[2].trim();
                    let parts: Vec<_> = body.split([',', '|']).map(str::trim).collect();
                    let parsed: Vec<_> = parts.iter().map(|s| JS_COUNT.captures(s)).collect();
                    if parsed.iter().all(Option::is_some) {
                        for c in parsed.into_iter().flatten() {
                            let n = c[1].parse().unwrap_or(u64::MAX);
                            failed |= &c[2] == "failed" && n > 0;
                            if case_line && &c[2] != "total" {
                                counts.add(n, &c[2]);
                            }
                        }
                    }
                }
            }
            Runner::Unittest => {
                if let Some(c) = UNIT.captures(line) {
                    unit_n = c[1].parse().ok();
                }
                if let Some(c) = UNIT_OK.captures(line) {
                    unit_skipped = c.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                    unit_end = Some(true);
                }
                if let Some(c) = UNIT_FAILED.captures(line) {
                    let mut errors: u64 = 0;
                    let mut failures: u64 = 0;
                    let mut skipped: u64 = 0;
                    let valid = c[1].split(", ").all(|part| {
                        let Some((kind, count)) = part.split_once('=') else { return false };
                        let Ok(n) = count.parse::<u64>() else { return false };
                        match kind {
                            "failures" | "unexpected successes" => failures = failures.saturating_add(n),
                            "errors" => errors = errors.saturating_add(n),
                            "skipped" | "expected failures" => skipped = skipped.saturating_add(n),
                            _ => return false,
                        }
                        true
                    });
                    if valid && (errors > 0 || failures > 0) {
                        unit_end = Some(false);
                        unit_errors = errors;
                        unit_failures = failures;
                        unit_skipped = skipped;
                    }
                }
            }
            Runner::Go => {
                if let Some(c) = GO.captures(line) {
                    failed |= &c[1] == "FAIL";
                    go_ok |= &c[1] == "ok" && !line.contains("[no tests to run]");
                    counts.seen |= line.contains("[no tests to run]");
                }
                if line.starts_with("--- PASS: ") || line.starts_with("--- FAIL: ") {
                    counts.add(1, if line.starts_with("--- PASS") { "passed" } else { "failed" });
                }
                failed |= line.starts_with("FAIL\t") || line.ends_with("[build failed]");
                counts.seen |= line.starts_with('?') && line.ends_with("[no test files]");
            }
            Runner::Typecheck => failed |= TYPESCRIPT_ERROR.is_match(line),
        }
    }
    let mut out = counts.finish();
    if runner == Runner::Unittest {
        if let (Some(n), Some(ok)) = (unit_n, unit_end) {
            let n = n.saturating_sub(unit_skipped);
            out = report(
                if !ok && unit_errors > 0 && unit_failures == 0 {
                    // unittest counts an import-error `_FailedTest` as a test. The plain
                    // summary cannot distinguish that from an error during an actual case.
                    VerificationOutcome::SetupFailure
                } else if !ok {
                    VerificationOutcome::Failed
                } else if n == 0 {
                    VerificationOutcome::NoTests
                } else {
                    VerificationOutcome::Passed
                },
                (unit_errors == 0).then_some(n),
            );
        }
    }
    if failed {
        out.outcome = VerificationOutcome::Failed;
    } else if setup {
        out.outcome = VerificationOutcome::SetupFailure;
    } else if out.outcome == VerificationOutcome::Unknown && (built && runner == Runner::CargoBuild || go_ok) {
        out.outcome = VerificationOutcome::Passed;
    }
    // Only when no actual test result explains the failure. A test's own traceback may contain
    // these strings; it does not turn an ordinary failed assertion into an environment misfire.
    if out.outcome == VerificationOutcome::Unknown
        && (super::environment_failure(&clean) || super::USAGE_FAILURES.iter().any(|s| clean.contains(s)))
    {
        out.outcome = VerificationOutcome::SetupFailure;
    }
    let failed_cases = (counts.failed > 0 || unit_failures > 0)
        && counts.errors == 0
        && unit_errors == 0
        && !setup
        && !clean.lines().any(|l| {
            CARGO_ERROR.is_match(l.trim()) || TYPESCRIPT_ERROR.is_match(l.trim()) || l.ends_with("[build failed]")
        });
    Some((out, failed_cases))
}

/// A direct test attempt suitable for comparing recovery observations. This is deliberately
/// stricter than ordinary outcome attribution: no setup commands, pipelines, shell syntax,
/// wrappers with shared results, discovery modes, or successful exits without executed cases.
/// A failed case is a runner-reported test failure, not proof of a particular assertion type.
pub fn verification_test_attempt(
    argv: &[String],
    command: &str,
    output: &str,
    reported_failed: bool,
) -> Option<VerificationReport> {
    if command != argv.join(" ")
        || command.is_empty()
        || !command.chars().all(|c| c.is_ascii_alphanumeric() || " _./:=@,+-".contains(c))
        || !matches!(
            runner(argv)?,
            Runner::Pytest | Runner::Unittest | Runner::CargoTest | Runner::Javascript | Runner::Go
        )
        || (runner(argv)? == Runner::Javascript && !matches!(base(argv.first()?), "jest" | "vitest"))
        || attributable(command, runner(argv)?)?
    // Filtered output is never a full recheck.
    {
        return None;
    }
    let (r, failed_cases) = report_details(argv, output)?;
    if r.tests_run.unwrap_or(0) == 0 {
        return None;
    }
    match r.outcome {
        VerificationOutcome::Failed if failed_cases => Some(r),
        VerificationOutcome::Passed if !reported_failed => Some(r),
        _ => None,
    }
}

/// Outcome for a direct, attributable invocation. Tests require a report; an exit alone does not
/// prove cases ran. A direct failed process does establish command failure without any case
/// count. For a recognized build/typecheck, a direct exit also establishes check completion
/// with ordinary nonempty output (including older Cargo formats). Neither rule applies to
/// unreadable/truncated output. A failed transport never certifies an earlier passing summary.
pub fn verification(argv: &[String], output: &str, reported_failed: bool) -> Option<VerificationReport> {
    let mut r = verification_report(argv, output)?;
    if reported_failed && r.outcome == VerificationOutcome::Passed {
        r.outcome = VerificationOutcome::Unknown;
    } else if r.outcome == VerificationOutcome::Unknown && complete_text(output).is_some() {
        if reported_failed {
            r = report(VerificationOutcome::Failed, None);
        } else if matches!(runner(argv), Some(Runner::CargoBuild | Runner::Typecheck)) {
            r.outcome = VerificationOutcome::Passed;
        }
    }
    Some(r)
}

/// Outcome for a canonical shell call. More than one meaningful command, branches, loops and
/// inline wrappers have shared/unproven output, even if the normalizer flattened a runner out.
/// A sole runner after `cd`/environment setup and through byte-preserving filters remains useful.
pub fn verification_for_command(
    argv: &[String],
    command: &str,
    output: &str,
    reported_failed: bool,
) -> Option<VerificationReport> {
    let r = verification_report(argv, output)?;
    let Some(filtered) = attributable(command, runner(argv)?) else {
        return Some(report(VerificationOutcome::Unknown, None));
    };
    if filtered {
        // The shell's status belongs to the filter. In particular, never infer a build passed
        // from no retained diagnostics, and do not turn grep's status into a runner failure.
        Some(if r.outcome == VerificationOutcome::Passed {
            report(VerificationOutcome::Unknown, r.tests_run)
        } else {
            r
        })
    } else {
        verification(argv, output, reported_failed)
    }
}

fn attributable(command: &str, expected: Runner) -> Option<bool> {
    use crate::types::CanonicalAction;
    if command.len() > crate::shell::MAX_COMMAND {
        return None;
    }
    let norms = crate::shell::normalize_shell(command, "/", crate::CommandTable::load());
    let [n] = norms.as_slice() else { return None };
    let CanonicalAction::Exec { argv, .. } = &n.action else { return None };
    if runner(argv)? != expected {
        return None;
    }
    if n.mutating && !n.note.starts_with("redirected to ") {
        return None;
    }
    // Normalization intentionally drops shell control and pipeline tails. Verify their syntax
    // too, so an echoed fixture, an alternate branch, or a transforming filter is not a report.
    let mut p = tree_sitter::Parser::new();
    p.set_language(&tree_sitter_bash::LANGUAGE.into()).ok()?;
    let tree = p.parse(command, None)?;
    if tree.root_node().has_error() {
        return None;
    }
    let mut stack = vec![tree.root_node()];
    let mut meaningful = 0;
    let mut filtered = false;
    while let Some(node) = stack.pop() {
        match node.kind() {
            "program" | "list" => {
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    if child.kind() == "||" || child.kind() == "&" {
                        return None;
                    }
                    if child.is_named() {
                        stack.push(child);
                    }
                }
            }
            "pipeline" => {
                let mut c = node.walk();
                let stages: Vec<_> = node.named_children(&mut c).collect();
                let (head, tail) = stages.split_first()?;
                for stage in tail {
                    if stage.kind() != "command" || !filter(*stage, command) {
                        return None;
                    }
                }
                filtered = true;
                stack.push(*head);
            }
            "redirected_statement" => {
                let mut c = node.walk();
                if node.named_children(&mut c).any(|x| matches!(x.kind(), "heredoc_redirect" | "herestring_redirect")) {
                    return None;
                }
                stack.push(node.child_by_field_name("body")?);
            }
            "command" => {
                let av = crate::shell::command_argv(node, command);
                let head = av.first().map(|s| base(s)).unwrap_or("");
                if matches!(head, "cd" | "export") {
                    // A substitution inside setup could print somebody else's report.
                    let text = node.utf8_text(command.as_bytes()).ok()?;
                    if text.contains("$(") || text.contains('`') {
                        return None;
                    }
                } else {
                    runner(&av)?;
                    meaningful += 1;
                }
            }
            "declaration_command" => {
                // `export NAME=value`: no command substitutions or arbitrary shell code.
                let text = node.utf8_text(command.as_bytes()).ok()?;
                if !text.starts_with("export ") || text.contains("$(") || text.contains('`') {
                    return None;
                }
            }
            _ => return None,
        }
    }
    (meaningful == 1).then_some(filtered)
}

fn filter(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let av = crate::shell::command_argv(node, source);
    let Some((head, args)) = av.split_first() else { return false };
    // Only filters that forward input bytes, with no additional file input or replacement.
    match base(head) {
        "head" | "tail" => {
            let mut value = false;
            for arg in args {
                if value {
                    if arg.parse::<u64>().is_err() {
                        return false;
                    }
                    value = false;
                } else if matches!(arg.as_str(), "-n" | "-c") {
                    value = true;
                } else if !arg.strip_prefix('-').is_some_and(|s| s.parse::<u64>().is_ok()) {
                    return false;
                }
            }
            !value
        }
        "grep" | "rg" => {
            let mut operands = 0;
            let mut value = false;
            for arg in args {
                if value {
                    if arg.parse::<u64>().is_err() {
                        return false;
                    }
                    value = false;
                } else if matches!(arg.as_str(), "-A" | "-B" | "-C" | "-m") {
                    value = true;
                } else if matches!(arg.as_str(), "-E" | "-F" | "-i" | "-v" | "--no-heading" | "--color=never")
                    || ["-A", "-B", "-C", "-m"]
                        .iter()
                        .any(|p| arg.strip_prefix(p).is_some_and(|s| !s.is_empty() && s.parse::<u64>().is_ok()))
                {
                } else if arg.starts_with('-') {
                    return false;
                } else {
                    operands += 1;
                }
            }
            !value && operands == 1
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn av(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn recognized_test_reports_distinguish_cases_from_collection_and_setup() {
        let cases = [
            ("pytest -q", "1 passed", VerificationOutcome::Passed, Some(1)),
            ("pytest", "1 failed, 2 passed", VerificationOutcome::Failed, Some(3)),
            ("pytest -q", "2 passed in 0.10s", VerificationOutcome::Passed, Some(2)),
            ("python3 -B -m pytest", "=== 1 failed, 2 passed in 0.10s ===", VerificationOutcome::Failed, Some(3)),
            ("pytest", "=== 1 error in 0.10s ===", VerificationOutcome::SetupFailure, Some(0)),
            ("pytest", "no tests ran in 0.10s", VerificationOutcome::NoTests, Some(0)),
            (
                "pytest",
                "============================ no tests ran in 0.00s =============================",
                VerificationOutcome::NoTests,
                Some(0),
            ),
            ("pytest", "2 skipped in 0.10s", VerificationOutcome::NoTests, Some(0)),
            ("pytest", "/usr/bin/python: No module named pytest", VerificationOutcome::SetupFailure, None),
            ("pytest", "collected 2 items", VerificationOutcome::Unknown, None),
            ("pytest --collect-only", "2 tests collected in 0.1s", VerificationOutcome::NoTests, Some(0)),
            ("python -m unittest", "Ran 2 tests in 0.1s\n\nOK", VerificationOutcome::Passed, Some(2)),
            ("python -m unittest", "OK", VerificationOutcome::Unknown, None),
            ("python -m unittest", "Ran 2 tests in 0.1s\n\nOK (skipped=2)", VerificationOutcome::NoTests, Some(0)),
            ("python -m unittest", "Ran 2 tests in 0.1s\nFAILED (failures=1)", VerificationOutcome::Failed, Some(2)),
            ("python -m unittest", "Ran 1 test in 0.1s\nFAILED (errors=1)", VerificationOutcome::SetupFailure, None),
            (
                "python -m unittest",
                "Ran 2 tests in 0.1s\nFAILED (failures=1, errors=1)",
                VerificationOutcome::Failed,
                None,
            ),
            ("cargo test", "test result: ok. 0 passed; 0 failed; 3 ignored", VerificationOutcome::NoTests, Some(0)),
            ("cargo test -- --list", "case: test\n1 test, 0 benchmarks", VerificationOutcome::NoTests, Some(0)),
            ("go test ./...", "? pkg [no test files]", VerificationOutcome::NoTests, Some(0)),
        ];
        for (cmd, output, outcome, tests_run) in cases {
            assert_eq!(verification_report(&av(cmd), output), Some(report(outcome, tests_run)), "{cmd}: {output}");
        }
        for output in ["the fixture expects 1 passed", "1 passed is the expected message", "collected 2 items"] {
            assert_eq!(verification_report(&av("pytest"), output).unwrap().outcome, VerificationOutcome::Unknown);
        }
    }

    #[test]
    fn javascript_failures_behind_successful_pipes_are_not_passes() {
        let cases = [
            ("pnpm test", "Test Files  1 failed | 2 passed (3)\n     Tests  1 failed | 14 passed (15)\n", 15),
            ("npx jest", "Test Suites: 1 failed, 2 passed, 3 total\nTests:       2 failed, 12 passed, 14 total\n", 14),
            ("vitest run", "\x1b[31mTest Files  1 failed (1)\x1b[0m\nTests  1 failed (1)\n", 1),
        ];
        for (cmd, output, n) in cases {
            let piped = format!("cd /project && {cmd} 2>&1 | tail -30");
            let r = verification_for_command(&av(cmd), &piped, output, false).unwrap();
            assert_eq!(r, report(VerificationOutcome::Failed, Some(n)), "{cmd}");
            assert!(super::super::tests_failed(&ANSI.replace_all(output, "")));
        }
        assert_eq!(
            verification_report(&av("pnpm test"), "No test files found, exiting with code 1").unwrap().outcome,
            VerificationOutcome::NoTests
        );
        assert_eq!(
            verification_report(&av("npm test"), "No tests found, exiting with code 1").unwrap().outcome,
            VerificationOutcome::NoTests
        );
        for (command, text) in [
            ("pnpm test", "No test files found is the fixture string under test"),
            ("npm test", "No tests found is the expected error message"),
            ("pytest", "no tests collected is a fixture string"),
            ("pytest", "no tests ran in this example"),
        ] {
            assert_eq!(
                verification_report(&av(command), text).unwrap().outcome,
                VerificationOutcome::Unknown,
                "{text}"
            );
        }
        assert!(!super::super::tests_failed("Tests: 0 failed, 4 passed, 4 total"));
    }

    #[test]
    fn cargo_failure_dominates_other_passing_targets_and_silent_build_is_not_a_test() {
        let output = "test result: ok. 4 passed; 0 failed; 0 ignored\n\
                      test result: FAILED. 2 passed; 1 failed; 0 ignored\n";
        let cmd = "export PATH=\"$HOME/.cargo/bin:$PATH\"; cargo test --workspace 2>&1 | grep -E 'test result|FAILED' -A3 | head -40";
        assert_eq!(
            verification_for_command(&av("cargo test --workspace"), cmd, output, false).unwrap(),
            report(VerificationOutcome::Failed, Some(7))
        );
        let compiler = "error[E0063]: missing field `record` in initializer of `Item`\nerror: could not compile `example` (bin test) due to 1 previous error\n";
        assert_eq!(
            verification_for_command(
                &av("cargo build --all-targets"),
                "cargo build --all-targets 2>&1 | tail -20",
                compiler,
                false
            )
            .unwrap()
            .outcome,
            VerificationOutcome::Failed
        );
        assert_eq!(verification(&av("cargo check"), "", false), Some(report(VerificationOutcome::Passed, None)));
        assert_eq!(
            verification(&av("cargo check"), "Finished dev [unoptimized + debuginfo] target(s) in 0.12s", false),
            Some(report(VerificationOutcome::Passed, None))
        );
        assert_eq!(verification(&av("cargo test"), "", false), Some(report(VerificationOutcome::Unknown, None)));
        assert_eq!(
            verification_for_command(&av("cargo check"), "cargo check | tail", "", false).unwrap().outcome,
            VerificationOutcome::Unknown
        );
        assert_eq!(
            verification_for_command(
                &av("cargo test"),
                "cargo test | head",
                "test result: ok. 2 passed; 0 failed; 0 ignored",
                false
            )
            .unwrap()
            .outcome,
            VerificationOutcome::Unknown
        );
    }

    #[test]
    fn direct_transport_failure_is_not_evidence_that_test_cases_executed() {
        let argv = av("python3 -m unittest discover -s tests");
        assert_eq!(
            verification_for_command(
                &argv,
                "PROJECT_ENV=test python3 -m unittest discover -s tests",
                "Ran 1 test in 0.001s\n\nOK\n",
                false
            ),
            Some(report(VerificationOutcome::Passed, Some(1)))
        );
        let output = "Traceback (most recent call last):\nRuntimeError: PROJECT_ENV is not set\n";
        assert_eq!(verification_report(&argv, output), Some(report(VerificationOutcome::Unknown, None)));
        assert_eq!(
            verification_for_command(&argv, "python3 -m unittest discover -s tests", output, true),
            Some(report(VerificationOutcome::Failed, None))
        );
        for (command, failed) in [
            ("python3 -m unittest discover -s tests", false),
            ("python3 -m unittest discover -s tests | tail", true),
            ("python3 -m unittest discover -s tests && python3 -m unittest", true),
        ] {
            assert_eq!(
                verification_for_command(&argv, command, output, failed).unwrap(),
                report(VerificationOutcome::Unknown, None),
                "{command}"
            );
        }
        for output in [
            "Warning: truncated output (original token count: 100000)\nFinished checking",
            "Output was truncated\ncompiler diagnostics",
            "... 400 tokens truncated ...\n1 passed in 0.1s",
            "\x1b[2Jcompiler diagnostics",
        ] {
            for failed in [false, true] {
                assert_eq!(
                    verification(&av("cargo check"), output, failed).unwrap().outcome,
                    VerificationOutcome::Unknown,
                    "{output}"
                );
            }
        }
    }

    #[test]
    fn arbitrary_text_and_shared_compound_reports_have_no_runner_attribution() {
        let output = "Tests: 1 failed, 2 passed, 3 total";
        for cmd in ["cat fixture.txt", "echo Tests: 1 failed", "python3 -c print -m pytest", "node show-log.js"] {
            assert_eq!(verification_report(&av(cmd), output), None, "{cmd}");
        }
        for raw in [
            "cat fixture.txt; pnpm test",
            "echo 'Tests: 1 failed'; pnpm test",
            "pnpm test && pnpm test",
            "pnpm test || pnpm test",
            "if true; then pnpm test; fi",
            "for x in a b; do pnpm test; done",
            "pnpm test | sed 's/passed/failed/g'",
            "pnpm test | grep failed fixture.txt",
            "pnpm test | rg --replace failed passed",
            "bash -lc 'pnpm test'",
        ] {
            assert_eq!(
                verification_for_command(&av("pnpm test"), raw, output, false).unwrap().outcome,
                VerificationOutcome::Unknown,
                "{raw}"
            );
        }
        assert_eq!(
            verification_for_command(&av("pytest"), "pnpm test", "no tests ran in 0.1s", false).unwrap().outcome,
            VerificationOutcome::Unknown
        );
    }

    #[test]
    fn known_typecheck_and_verify_scripts_keep_diagnostic_failures() {
        let output = "src/main.ts(3,8): error TS2307: Cannot find module './other'.\nELIFECYCLE Command failed with exit code 2.\n";
        for cmd in ["pnpm typecheck", "pnpm run typecheck", "pnpm verify", "pnpm run verify", "tsc --noEmit"] {
            assert_eq!(
                verification_for_command(&av(cmd), cmd, output, false).unwrap().outcome,
                VerificationOutcome::Failed,
                "{cmd}"
            );
        }
        assert!(verification_report(&av("pnpm custom-script"), output).is_none());
        assert_eq!(
            verification_report(&av("pnpm typecheck"), "src/main.ts:3:8 - error TS2307: Cannot find module './other'.")
                .unwrap()
                .outcome,
            VerificationOutcome::Failed
        );
    }

    #[test]
    fn limits_and_contradictions_never_upgrade_to_success() {
        assert_eq!(
            verification(&av("pytest"), "2 passed in 0.1s", true).unwrap().outcome,
            VerificationOutcome::Unknown
        );
        assert_eq!(
            verification_report(&av("pytest"), "E FileNotFoundError: No such file or directory\n1 failed in 0.1s")
                .unwrap()
                .outcome,
            VerificationOutcome::Failed
        );
        assert_eq!(
            verification_report(&av("pnpm test"), "No tests found\nTests: 1 failed, 1 total").unwrap().outcome,
            VerificationOutcome::Failed
        );
        assert_eq!(
            verification_report(&av("pytest"), "no tests ran in 0.1s\n1 passed in 0.2s").unwrap().outcome,
            VerificationOutcome::Passed
        );
        assert_eq!(
            verification(&av("cargo check"), &"x".repeat(MAX_REPORT + 1), false).unwrap().outcome,
            VerificationOutcome::Unknown
        );
        assert_eq!(
            verification_report(&av("pytest"), "\x1b[2J2 passed in 0.1s").unwrap().outcome,
            VerificationOutcome::Unknown
        );
    }

    #[test]
    fn recovery_attempts_require_direct_executed_test_cases_and_complete_reports() {
        for (command, red, green, count) in [
            ("pytest tests/test_a.py", "1 failed, 1 passed in 0.1s", "2 passed in 0.1s", 2),
            ("python3 -m unittest", "Ran 2 tests in 0.1s\nFAILED (failures=1)", "Ran 2 tests in 0.1s\nOK", 2),
            ("vitest run", "Tests  1 failed | 1 passed (2)", "Tests  2 passed (2)", 2),
            (
                "cargo test",
                "test result: FAILED. 1 passed; 1 failed; 0 ignored",
                "test result: ok. 2 passed; 0 failed; 0 ignored",
                2,
            ),
        ] {
            assert_eq!(
                verification_test_attempt(&av(command), command, red, true),
                Some(report(VerificationOutcome::Failed, Some(count))),
                "{command}"
            );
            assert_eq!(
                verification_test_attempt(&av(command), command, green, false),
                Some(report(VerificationOutcome::Passed, Some(count))),
                "{command}"
            );
        }
        for (command, output, failed) in [
            ("pytest", "1 error, 1 passed in 0.1s", true),
            ("pytest", "no tests ran in 0.1s", true),
            ("pytest --collect-only", "2 passed in 0.1s", false),
            ("pytest", "2 passed in 0.1s\n[output truncated]", false),
            ("pytest", "2 passed in 0.1s", true),
            ("pytest | tail -5", "2 passed in 0.1s", false),
            ("pnpm test", "Tests  1 failed (1)", true),
            ("echo fixture", "1 failed in 0.1s", true),
            (
                "cargo test",
                "test result: FAILED. 1 passed; 1 failed; 0 ignored\nerror: could not compile other crate",
                true,
            ),
            ("python3 -m unittest", "Ran 2 tests in 0.1s\nFAILED (errors=1)", true),
        ] {
            assert_eq!(verification_test_attempt(&av(command), command, output, failed), None, "{command}: {output}");
        }
    }
}
