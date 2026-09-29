//! What a command's output says about how it ended, where its exit status cannot be trusted or
//! does not say enough.
//!
//! - **Piped runs.** `pytest … 2>&1 | head -50` exits with `head`'s status (0), even when pytest
//!   printed `No module named pytest`. [`piped`] tells such a command apart, and
//!   [`piped_failure`] reads its output: an environment failure ([`ENVIRONMENT_FAILURES`]) or a
//!   test runner's own summary of failures (`3 failed, 2 passed in 0.4s`, `1 error in 0.1s`).
//!   Only failure-triggered dead ends and instincts read it (`adage_runtime::decide`); an
//!   obligation never does.
//! - **Follow judgements.** A followed test run that fails because tests fail is still followed:
//!   the runner ran and reported ([`tests_ran`]). Only a failure that is the directive's fault
//!   (the interpreter or module missing, a target that does not exist) is a misfire
//!   ([`misfired`]).

use once_cell::sync::Lazy;
use regex::Regex;

/// What a command prints when its environment, not the code under test, is wrong: the runner or
/// a module missing, a program not installed. Test output rarely contains these on its own, and
/// they are only read beside a runner that did not report tests ([`tests_ran`]).
pub const ENVIRONMENT_FAILURES: &[&str] = &[
    "No module named",
    "ModuleNotFoundError",
    "command not found",
    "No such file or directory",
    "Cannot find module",
    "ERR_MODULE_NOT_FOUND",
    "is not recognized as an internal or external command",
];

/// What a command prints when it was called wrongly: the target, script or subcommand it names
/// does not exist, or it refuses its arguments. With [`ENVIRONMENT_FAILURES`], what makes a
/// followed directive a misfire ([`misfired`]).
pub const USAGE_FAILURES: &[&str] = &[
    "No rule to make target",
    "Missing script:",
    "no such command",
    "unrecognized arguments",
    "Unknown command",
    "error: unexpected argument",
];

/// Whether the output holds an environment failure ([`ENVIRONMENT_FAILURES`]).
pub fn environment_failure(output: &str) -> bool {
    ENVIRONMENT_FAILURES.iter().any(|s| output.contains(s))
}

/// A test runner's summary line: pytest (`=== 1 failed, 3 passed in 0.12s ===`, `-q`'s bare
/// form, `no tests ran`), unittest (`Ran 3 tests in`), cargo (`test result:`), go (`ok  \t`,
/// `FAIL\t`, `--- FAIL:`), jest and vitest (`Tests:`, `Test Files`).
static SUMMARY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?m)\b\d+ (?:passed|failed|errors?|skipped|xfailed|xpassed|deselected|warnings?)\b[^\n]*\bin [0-9.]+ ?s\b|no tests ran in [0-9.]+ ?s|^Ran \d+ tests? in |^test result: |^(?:ok|FAIL)\s+\S+\s+[0-9.]+s|^--- FAIL: |^\s*Tests?:\s+\d|^\s*Test Files\s+\d|Interrupted: \d+ errors? during collection",
    )
    .unwrap()
});

/// A pytest summary that reports failures or errors (`2 failed`, `1 error`), or a collection
/// interrupted by errors, or unittest's `FAILED (failures=1)`.
static FAILED_SUMMARY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?m)\b\d+ (?:failed|errors?)\b[^\n]*\bin [0-9.]+ ?s\b|Interrupted: \d+ errors? during collection|^FAILED \((?:failures|errors)=\d+|^--- FAIL: |^FAIL\s+\S+\s+[0-9.]+s|^test result: FAILED",
    )
    .unwrap()
});

/// Whether a test runner ran and reported: its output holds a summary line ([`SUMMARY`]). A run
/// whose tests failed ran; one that could not import its runner did not.
pub fn tests_ran(output: &str) -> bool {
    SUMMARY.is_match(output)
}

/// Whether a runner's own summary reports failed or erroring tests.
pub fn tests_failed(output: &str) -> bool {
    FAILED_SUMMARY.is_match(output)
}

/// Whether a command line pipes one command into another (`a | b`, `a |& b`), outside quotes. `||`
/// is not a pipe.
pub fn piped(command: &str) -> bool {
    let (mut quote, mut escaped) = (None::<char>, false);
    let b: Vec<char> = command.chars().collect();
    for (i, &c) in b.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, c) {
            (_, '\\') if quote != Some('\'') => escaped = true,
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '|') => {
                let prev = i.checked_sub(1).map(|j| b[j]);
                let next = b.get(i + 1).copied();
                if prev != Some('|') && next != Some('|') {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Whether a piped command that exited 0 failed all the same, by its output: an environment
/// failure where no runner reported tests, or a runner's summary of failures. Conservative: only
/// failure-triggered dead ends and instincts act on it, and they still need their own signature
/// in the output.
pub fn piped_failure(output: &str) -> bool {
    (environment_failure(output) && !tests_ran(output)) || tests_failed(output)
}

/// Whether a followed directive that failed (by its exit status, or, piped, by its output)
/// misfired: it failed in a way that is the directive's fault, an environment or usage failure
/// ([`ENVIRONMENT_FAILURES`], [`USAGE_FAILURES`]), without a runner reporting tests. A test run
/// that reported failing tests, or a failure in the code under test (a syntax error in the
/// agent's own edit), is followed, not misfired: that is the task, not the directive.
pub fn misfired(output: &str) -> bool {
    let wrong = environment_failure(output) || USAGE_FAILURES.iter().any(|s| output.contains(s));
    wrong && !tests_ran(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipes_are_told_apart_from_or_chains_and_quotes() {
        assert!(piped("python -m pytest tests/x.py -xvs 2>&1 | head -50"));
        assert!(piped("pytest |& tail"));
        assert!(!piped("make test || true"));
        assert!(!piped("grep 'a|b' file"));
        assert!(!piped("python -m pytest -k \"a or b|c\""));
        assert!(!piped(".venv/bin/python -m pytest tests"));
    }

    #[test]
    fn summaries_say_whether_tests_ran_and_failed() {
        let pytest_fail = "tests/test_a.py F.\n=================== 1 failed, 1 passed in 0.12s ===================\n";
        assert!(tests_ran(pytest_fail) && tests_failed(pytest_fail));
        let quiet = "..\n2 passed in 0.03s\n";
        assert!(tests_ran(quiet) && !tests_failed(quiet));
        let collect =
            "ERROR tests/test_a.py\n!!! Interrupted: 1 error during collection !!!\n=== 1 error in 0.2s ===\n";
        assert!(tests_ran(collect) && tests_failed(collect));
        let missing = "/usr/bin/python: No module named pytest\n";
        assert!(!tests_ran(missing) && environment_failure(missing) && piped_failure(missing));
        assert!(tests_ran("Ran 3 tests in 0.001s\n\nOK\n"));
        assert!(tests_failed("Ran 3 tests in 0.001s\n\nFAILED (failures=1)\n"));
        assert!(tests_ran("test result: ok. 3 passed; 0 failed"));
        assert!(!piped_failure("all good\n"));
        // A test's own traceback naming a missing file, in a run that reported: not the environment.
        let ran = "E   FileNotFoundError: No such file or directory\n=== 1 failed in 0.1s ===";
        assert!(!misfired(ran));
        assert!(misfired("bash: .venv/bin/python: No such file or directory"));
        assert!(misfired("/usr/bin/python3: No module named pytest"));
        assert!(misfired("make: *** No rule to make target 'gen'.  Stop."));
        assert!(!misfired("=== 3 passed in 0.2s ==="));
        // RepoBench v2, attrs: the agent's own edit broke the import; pytest exited 4 with no
        // summary. Followed: the failure is the code under test, not the directive.
        let conftest = "ImportError while loading conftest 'conftest.py'.\nE     File \"src/attr/validators.py\", line 405\nE   SyntaxError: invalid syntax";
        assert!(!misfired(conftest));
    }
}
