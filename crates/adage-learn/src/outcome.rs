//! Local outcomes (evals/launch/outcome-optimisation.md §1, §2.1): what a trace shows happened
//! *after* a decision point, measured close to it, so that a directive can be valued by what it
//! changes rather than by how often agents did something.
//!
//! - **Goal events** ([`Goal`]): typed, deterministic detectors. [`Goal::Test`] is a test run that
//!   actually ran tests, read from its output whatever the exit code (a failing test run is still
//!   a productive one, and a piped run exits 0 whatever pytest said); [`Goal::Import`] a Python
//!   run that imported what it needed; [`Goal::Locate`] the first edit of a source file;
//!   [`Goal::Build`] a build or check that passed after one that failed.
//! - **The local reward** ([`local`]): tool calls from a trigger to the first goal event, censored
//!   at a horizon (the censored calls count as the horizon), and whether a harm-class action ran in
//!   between ([`harm`]: an install into the system or user site, a push). Harm is a veto, never a
//!   term of the reward.
//! - **Runners** ([`Runner`]): a test command read as its environment and its tool family
//!   (`.venv/bin/python -m pytest tests/x.py -q`, and `source .venv/bin/activate && python -m
//!   pytest`, are the project environment's pytest). Two test runs satisfy the same directive when
//!   their runners are equal, whatever their arguments (the `Sat(d)` equivalence class).
//! - **Situations** ([`situations`]): the decision points of a trace, keyed by a typed, coarse
//!   [`SituationKey`] (ecosystem, environment kind, trigger family, phase), each with the class of
//!   the agent's next test run and the local reward from there.
//!
//! One function computes the reward for learning and for replay, so the two agree. Everything
//! here is a pure function of the trace (and of the repository facts passed in).

use crate::import::{Session, StepObs};
pub use adage_core::action::Env;
use adage_core::action::{self, Action, Role};
use adage_core::{CanonicalAction, Effect, Normalized};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// The calls after a trigger within which a goal must come (else the reward is censored).
pub const HORIZON: u32 = 15;

/// A goal event (outcome-optimisation §1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Goal {
    /// A test run that executed tests.
    Test,
    /// A Python run that imported what it needed (no `No module named`).
    Import,
    /// The first edit of a source file.
    Locate,
    /// A build or check that passed after one that failed.
    Build,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a valid pattern"))
}

/// Whether a test run's output shows that tests ran: pytest's `collected N items` (N > 0) or
/// `N passed`/`N failed`, unittest's `Ran N tests`, vitest's `Test Files`, jest's `Tests:`, Go's
/// `ok  <pkg>`/`--- PASS`, cargo's `test result:`. Never `no tests ran`, a missing module or a
/// missing command. The exit code is not read.
/// Its word boundaries are ASCII (`(?-u:\b)`): a Unicode `\b` stops the regex engine's lazy DFA
/// at the first non-ASCII byte (a reporter's `✓` or `━`), and the slow fallback made this most of
/// a learn run's matching time.
pub fn executed_tests(output: &str) -> bool {
    static YES: OnceLock<Regex> = OnceLock::new();
    static NO: OnceLock<Regex> = OnceLock::new();
    let yes = re(
        &YES,
        r"(?m)collected [1-9]\d* items?|(?-u:\b)\d+ (passed|failed|errors?)(?-u:\b)|(?-u:\b)Ran [1-9]\d* tests?(?-u:\b)|Test Files\s+\d|^\s*Tests:\s+\d|^(ok|FAIL)\s+\S+\s+[\d.]+s|^--- (PASS|FAIL)|test result: (ok|FAILED)",
    );
    let no = re(&NO, r"no tests ran|No module named|ModuleNotFoundError|command not found|Cannot find module");
    let out = adage_core::tokens::bounded(output, 64 * 1024);
    yes.is_match(out) && (!no.is_match(out) || re_count_passed(out))
}

/// Whether an output reports tests that passed or failed (a run whose output also names a missing
/// module inside a test's own failure still ran tests).
fn re_count_passed(out: &str) -> bool {
    static P: OnceLock<Regex> = OnceLock::new();
    re(&P, r"(?-u:\b)\d+ (passed|failed)(?-u:\b)|Test Files\s+\d|test result: ").is_match(out)
}

/// The environment family of a failure, read from its output: `no-module:<name>` (Python's
/// `No module named 'x'`), `cmd-not-found:<program>`, `cannot-find-module` (Node). `None` for
/// any other failure (a test's own assertion, a syntax error).
pub fn env_failure(output: &str) -> Option<String> {
    env_signature(output).map(|(f, _)| f)
}

/// [`env_failure`], with the text of the output it was read from (`No module named pytest`,
/// `No module named 'click'`, `pytest: command not found`): what an after-failure trigger matches.
pub fn env_signature(output: &str) -> Option<(String, String)> {
    static M: OnceLock<Regex> = OnceLock::new();
    static C: OnceLock<Regex> = OnceLock::new();
    let out = adage_core::tokens::bounded(output, 64 * 1024);
    if let Some(c) = re(&M, r"No module named '?([A-Za-z_][A-Za-z0-9_]*)'?").captures(out) {
        return Some((format!("no-module:{}", &c[1]), c[0].to_string()));
    }
    // A shell's own words: `bash: pytest: command not found`, `sh: 1: pytest: not found`.
    if let Some(c) = re(&C, r"(?m)(?:^|[\s:])([A-Za-z0-9_.+-]+): command not found|^(?:ba|z)?sh: (?:line )?\d+: ([A-Za-z0-9_.+-]+): not found")
        .captures(out)
    {
        let (p, text) = match (c.get(1), c.get(2)) {
            (Some(p), _) => (p.as_str(), format!("{}: command not found", p.as_str())),
            (_, Some(p)) => (p.as_str(), format!("{}: not found", p.as_str())),
            _ => return None,
        };
        return Some((format!("cmd-not-found:{p}"), text));
    }
    out.contains("Cannot find module").then(|| ("cannot-find-module".to_string(), "Cannot find module".to_string()))
}

/// A test command as its environment and its runner family (`pytest`, `vitest`, `npm test`,
/// `go test`, `cargo test`, `unittest`, `tox`, `nox`, `jest`, `node --test`): the test-run class
/// of an [`adage_core::action::Action`]. Two runs satisfy the same directive when their runners
/// are equal, whatever their arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Runner {
    pub env: Env,
    pub family: String,
}

impl Runner {
    /// The ecosystem a family belongs to.
    pub fn ecosystem(&self) -> &'static str {
        ecosystem(&self.family)
    }
}

/// Whether a test family is given the tests to run as file paths (`pytest tests/test_core.py`,
/// `npx vitest run src/a.test.ts`). Cargo selects tests by name, `go test` by package, and tox and
/// nox by environment, so a list of files would break their command line.
pub fn takes_test_files(family: &str) -> bool {
    !matches!(family, "cargo test" | "cargo nextest" | "go test" | "tox" | "nox")
}

/// The ecosystem of a test family.
pub fn ecosystem(family: &str) -> &'static str {
    match family {
        "pytest" | "unittest" | "tox" | "nox" => "py",
        "go test" => "go",
        "cargo test" | "cargo nextest" => "rust",
        _ => "node",
    }
}

/// The argvs of an action in order (every part of a compound).
pub fn argvs(n: &Normalized) -> Vec<&Vec<String>> {
    fn walk<'a>(n: &'a Normalized, out: &mut Vec<&'a Vec<String>>) {
        match &n.action {
            CanonicalAction::Exec { argv, .. } if !argv.is_empty() => out.push(argv),
            CanonicalAction::Compound { actions, .. } => actions.iter().for_each(|a| walk(a, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(n, &mut out);
    out
}

fn base(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// What a step's command does ([`adage_core::action::classify`] over its parts); `None` for a
/// step that runs no command.
pub fn action_of(st: &StepObs) -> Option<Action> {
    let parts = argvs(&st.norm);
    if parts.is_empty() {
        return None;
    }
    let segs: Vec<&[String]> = parts.iter().map(|v| v.as_slice()).collect();
    Some(action::classify(&segs))
}

/// The test family of one argv, if it runs tests.
pub fn test_family(argv: &[String]) -> Option<String> {
    let a = action::classify_argv(argv);
    (a.role == Role::TestRun).then_some(a.runner)
}

/// The runner of a step's test run, in the environment the step ran it in (an activation earlier
/// in the same command, `source .venv/bin/activate && …`, counts).
pub fn runner(st: &StepObs) -> Option<Runner> {
    let a = action_of(st)?;
    (a.role == Role::TestRun).then_some(Runner { env: a.env, family: a.runner })
}

/// Whether a step runs tests, whatever came of it.
pub fn is_test_run(st: &StepObs) -> bool {
    runner(st).is_some()
}

/// Whether a step is a test run that executed tests ([`Goal::Test`]).
pub fn test_goal(st: &StepObs) -> bool {
    is_test_run(st) && executed_tests(&st.call.output)
}

/// Whether a step is a harm-class action (outcome-optimisation §1.1, L0): a package install outside
/// the project's environment (`pip install` into the system or user site, `--user`,
/// `--break-system-packages`), anything run with `sudo`, or a push. A harm is a veto, never a
/// cost.
pub fn harm(st: &StepObs) -> bool {
    let mut activated = false;
    for av in argvs(&st.norm) {
        let first = av.first().map(String::as_str).unwrap_or("");
        if (first == "source" || first == ".") && av.last().is_some_and(|p| p.ends_with("/bin/activate")) {
            activated = true;
            continue;
        }
        if first == "sudo" {
            return true;
        }
        let a = action::classify_argv(av);
        if a.role == Role::VcsWrite && av.iter().any(|t| t == "push") {
            return true;
        }
        if a.role != Role::EnvSetup {
            continue;
        }
        let prog = av.iter().find(|t| !t.contains('=') || t.starts_with('-')).map(String::as_str).unwrap_or("");
        let b = prog.rsplit('/').next().unwrap_or(prog);
        let pip = matches!(b, "pip" | "pip3")
            || b.starts_with("pip3.")
            || (action::is_python(prog) && av.iter().any(|t| t == "pip"));
        if !pip || !av.iter().any(|t| t == "install") {
            continue;
        }
        let in_venv = action::venv_of(prog).is_some() || activated;
        let user = av.iter().any(|t| t == "--user" || t == "--break-system-packages");
        let target = av.iter().any(|t| t == "--target" || t == "-t" || t.starts_with("--target=") || t == "--prefix");
        if user || (!in_venv && !target) {
            return true;
        }
    }
    false
}

/// The local reward from one trigger (outcome-optimisation §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LocalReward {
    /// Tool calls after the trigger up to and including the goal event (the horizon when
    /// censored).
    pub calls: u32,
    /// The goal came within the horizon.
    pub reached: bool,
    /// A harm-class action ran before the goal (or within the horizon).
    pub harm: bool,
}

/// Whether step `st` is goal `g`.
pub fn is_goal(st: &StepObs, g: Goal) -> bool {
    match g {
        Goal::Test => test_goal(st),
        Goal::Import => {
            !st.call.failed
                && action_of(st).is_some_and(|a| a.role == Role::Repro && a.runner == "python")
                && env_failure(&st.call.output).is_none()
        }
        Goal::Locate => source_edit(st).is_some(),
        Goal::Build => !st.call.failed && st.norm.effect == Effect::Verify && !is_test_run(st),
    }
}

/// The local reward of the trigger at step `t` of `s` for goal `g`: calls to the first goal event
/// after it, censored at `horizon`. One function for learning and for replay.
pub fn local(s: &Session, t: usize, g: Goal, horizon: u32) -> LocalReward {
    let mut harmed = false;
    for (k, st) in s.steps.iter().enumerate().skip(t + 1) {
        let calls = (k - t) as u32;
        if calls > horizon {
            break;
        }
        harmed |= harm(st);
        if is_goal(st, g) {
            return LocalReward { calls, reached: true, harm: harmed };
        }
    }
    LocalReward { calls: horizon, reached: false, harm: harmed }
}

const CODE: &[&str] = &[
    "py", "pyi", "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "vue", "go", "rs", "rb", "java", "kt", "c", "h", "cc",
    "cpp", "hpp", "cs", "swift", "php",
];

/// Whether a repository path names a test file: under a `tests`/`test`/`__tests__`/`spec`
/// directory, or named like one (`test_x.py`, `x_test.py`, `x_test.go`, `x.spec.ts`,
/// `x.test.js`), with a code extension.
pub fn is_test_path(p: &str) -> bool {
    let name = base(p);
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    if !CODE.contains(&ext) {
        return false;
    }
    stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with(".spec")
        || stem.ends_with(".test")
        || p.split('/').any(|d| matches!(d, "tests" | "test" | "__tests__" | "spec" | "testing")) && p.contains('/')
}

/// The source file a step edited: a repository path inside a directory, with a code extension,
/// not a test file.
pub fn source_edit(st: &StepObs) -> Option<&str> {
    if !adage_core::keys::is_author(&st.norm.action) {
        return None;
    }
    let p = st.rel_path.as_deref()?;
    let ext = base(p).rsplit_once('.').map(|x| x.1)?;
    (p.contains('/') && !p.starts_with('/') && !p.starts_with("..") && CODE.contains(&ext) && !is_test_path(p))
        .then_some(p)
}

/// A decision point's typed key (outcome-optimisation §2.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SituationKey {
    /// `py`, `node`, `go`, `rust`.
    pub ecosystem: String,
    /// The repository's declared environment: `venv`, `managed`, `node_modules`, `none`.
    pub env_kind: String,
    /// `no-module:pytest`, `cmd-not-found:pytest`, `after-edit:source`.
    pub trigger: String,
    /// `pre-goal` (no goal event yet in the session) or `post-goal`.
    pub phase: String,
}

/// One decision point in a trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Situation {
    pub key: SituationKey,
    /// The trigger's step.
    pub at: usize,
    /// For a failure trigger: the runner that failed, and the text its output said it with.
    pub failed: Option<Runner>,
    pub signature: Option<String>,
    /// The runner of the agent's next test run after the trigger (its action class), if any
    /// before the horizon.
    pub next: Option<Runner>,
    /// The next test run failed for the environment again.
    pub next_misfired: bool,
    /// The step of that next test run, if any.
    pub next_at: Option<usize>,
    pub reward: LocalReward,
}

/// The decision points of a trace for the test goal: every test run that failed for the
/// environment (`no-module:<m>`, `cmd-not-found:<p>`: the first of each family per trace), and the
/// first edit of a source file before any test goal (`after-edit:source`). `env_kind` is the
/// repository's declared environment (`venv`, `managed`, `node_modules`, `none`).
pub fn situations(s: &Session, env_kind: &str) -> Vec<Situation> {
    let mut out = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = Default::default();
    let first_goal = s.steps.iter().position(test_goal);
    let mut edited = false;
    for (i, st) in s.steps.iter().enumerate() {
        let phase = if first_goal.is_some_and(|g| g < i) { "post-goal" } else { "pre-goal" };
        let (trigger, failed, signature) = if let Some(r) = runner(st).filter(|_| !executed_tests(&st.call.output)) {
            match env_signature(&st.call.output) {
                Some((f, text)) if seen.insert(f.clone()) => (f, Some(r), Some(text)),
                _ => continue,
            }
        } else if !edited && source_edit(st).is_some() && phase == "pre-goal" {
            edited = true;
            ("after-edit:source".to_string(), None, None)
        } else {
            continue;
        };
        let reward = local(s, i, Goal::Test, HORIZON);
        let horizon_end = (i + 1 + HORIZON as usize).min(s.steps.len());
        let next = s.steps[i + 1..horizon_end].iter().enumerate().find_map(|(k, x)| runner(x).map(|r| (i + 1 + k, r)));
        let eco = failed
            .as_ref()
            .map(|r| r.ecosystem())
            .or_else(|| next.as_ref().map(|(_, r)| r.ecosystem()))
            .unwrap_or("any");
        out.push(Situation {
            key: SituationKey {
                ecosystem: eco.to_string(),
                env_kind: env_kind.to_string(),
                trigger,
                phase: phase.to_string(),
            },
            at: i,
            failed,
            signature,
            next_misfired: next.as_ref().is_some_and(|(k, _)| {
                !executed_tests(&s.steps[*k].call.output) && env_failure(&s.steps[*k].call.output).is_some()
            }),
            next_at: next.as_ref().map(|(k, _)| *k),
            next: next.map(|(_, r)| r),
            reward,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (2026-10-01, a real repository): "the tests of what was edited" bound test file
    /// paths into `cargo test adage-cli {tests}`, which cargo rejects. Only runners that take test
    /// files as arguments get that reflex.
    #[test]
    fn only_runners_that_take_test_files_are_given_them() {
        for f in ["pytest", "unittest", "vitest", "jest", "mocha"] {
            assert!(takes_test_files(f), "{f}");
        }
        for f in ["cargo test", "cargo nextest", "go test", "tox", "nox"] {
            assert!(!takes_test_files(f), "{f}");
        }
    }

    fn av(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn executed_tests_reads_the_report_not_the_exit_code() {
        assert!(executed_tests("============ 47 passed in 0.32s ============"));
        assert!(executed_tests("collected 12 items\n\ntests/test_x.py ....F"));
        assert!(executed_tests(" Test Files  1 passed (1)\n      Tests  3 passed (3)"));
        assert!(executed_tests("ok  \tgithub.com/urfave/cli/v3\t0.412s"));
        assert!(executed_tests("test result: ok. 5 passed; 0 failed"));
        assert!(executed_tests("Ran 4 tests in 0.01s\n\nOK"));
        assert!(!executed_tests("/usr/bin/python: No module named pytest"));
        assert!(!executed_tests("collected 0 items\n\nno tests ran in 0.01s"));
        assert!(!executed_tests("bash: pytest: command not found"));
        // A test that fails on a missing module still ran.
        assert!(executed_tests("E   ModuleNotFoundError: No module named 'yaml'\n1 failed, 3 passed in 0.2s"));
        // Reporters that print non-ASCII (vitest's ✓, pytest's ━) read the same.
        assert!(executed_tests(" ✓ src/a.test.ts (3)\n\n Test Files  1 passed (1)"));
        assert!(executed_tests("━━━━━━━━━━ 12 passed in 0.40s ━━━━━━━━━━"));
        assert!(!executed_tests("━━━━━━━━━━ no tests ran in 0.01s ━━━━━━━━━━"));
    }

    #[test]
    fn env_failures_are_typed() {
        assert_eq!(env_failure("/usr/bin/python: No module named pytest").as_deref(), Some("no-module:pytest"));
        assert_eq!(env_failure("ModuleNotFoundError: No module named 'click'").as_deref(), Some("no-module:click"));
        assert_eq!(env_failure("bash: pytest: command not found").as_deref(), Some("cmd-not-found:pytest"));
        assert_eq!(env_failure("AssertionError: 1 != 2"), None);
    }

    #[test]
    fn test_families_and_environments() {
        assert_eq!(test_family(&av("python -m pytest tests/test_x.py -xvs")).as_deref(), Some("pytest"));
        assert_eq!(test_family(&av(".venv/bin/python -m pytest")).as_deref(), Some("pytest"));
        assert_eq!(test_family(&av("python3 -W error -m pytest")).as_deref(), Some("pytest"));
        assert_eq!(test_family(&av("PYTHONPATH=src python3 -m pytest")).as_deref(), Some("pytest"));
        assert_eq!(test_family(&av("npm test -- packages/x")).as_deref(), Some("npm test"));
        assert_eq!(test_family(&av("npm run test")).as_deref(), Some("npm test"));
        assert_eq!(test_family(&av("go test ./... -run X")).as_deref(), Some("go test"));
        assert_eq!(test_family(&av("uv run pytest -q")).as_deref(), Some("pytest"));
        assert_eq!(test_family(&av("python -c import x")), None);
        assert_eq!(test_family(&av("pip install pytest")), None);
    }

    #[test]
    fn test_paths() {
        assert!(is_test_path("tests/test_core.py"));
        assert!(is_test_path("packages/runtime-core/__tests__/hydration.spec.ts"));
        assert!(is_test_path("flag_test.go"));
        assert!(!is_test_path("src/click/core.py"));
        assert!(!is_test_path("tests/conftest.txt"));
    }
}
