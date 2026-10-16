//! Local outcomes (evals/launch/outcome-optimisation.md §1, §2.1): what a trace shows happened
//! *after* a decision point, measured close to it, so that a directive can be valued by what it
//! changes rather than by how often agents did something.
//!
//! - **Goal events** ([`Goal`]): typed, deterministic detectors. [`Goal::Test`] is a test run that
//!   actually ran tests, read from an attributable runner report whatever the exit code (a failing
//!   assertion is still productive, while collection/setup is not); [`Goal::Import`] a Python
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
use adage_core::outcome::VerificationOutcome;
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

/// Whether an attributable runner report establishes positive executed test cases. Collection,
/// skipped-only reports, setup errors and shared compound output are not execution evidence.
/// A safe filtered report may establish a positive count without establishing the final status:
/// the local goal is tests *executed*, not tests passed. Unsupported wrappers remain unknown.
pub fn executed_tests(st: &StepObs) -> bool {
    st.verification().is_some_and(|r| {
        r.tests_run.is_some_and(|n| n > 0)
            && matches!(
                r.outcome,
                VerificationOutcome::Passed | VerificationOutcome::Failed | VerificationOutcome::Unknown
            )
    })
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
    static Z: OnceLock<Regex> = OnceLock::new();
    let out = adage_core::tokens::bounded(output, 64 * 1024);
    if let Some(c) = re(&M, r"No module named '?([A-Za-z_][A-Za-z0-9_]*)'?").captures(out) {
        return Some((format!("no-module:{}", &c[1]), c[0].to_string()));
    }
    // zsh puts the missing program last. Parse this before the legacy form so neither `zsh`
    // nor `(eval)`'s line number becomes the program identity. Keep a literal diagnostic
    // substring for the trigger; a corrected family name alone would never match this output.
    if let Some(c) =
        re(&Z, r"(?m)^(?:zsh(?::\d+)?|\(eval\):\d+): command not found: ([A-Za-z0-9_.+-]+)[\t ]*\r?$").captures(out)
    {
        let p = &c[1];
        return Some((format!("cmd-not-found:{p}"), format!("command not found: {p}")));
    }
    // A shell's own words: `bash: pytest: command not found`, `sh: 1: pytest: not found`.
    // Require the diagnostic to end here: malformed reverse-form output must not fall back
    // to treating a shell name or line number as the missing executable.
    if let Some(c) = re(&C, r"(?m)(?:^|[\s:])([A-Za-z0-9_.+-]+): command not found[\t ]*\r?$|^(?:ba|z)?sh: (?:line )?\d+: ([A-Za-z0-9_.+-]+): not found[\t ]*\r?$")
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
    is_test_run(st) && executed_tests(st)
}

/// A successful build/check, distinct from the local test-execution goal. Known runners use
/// typed evidence, so a pipeline's exit cannot conceal their failure. For unsupported custom
/// Verify commands, preserve the prior exit-based contract only for one literal direct command;
/// no success is inferred for a pipeline, compound, refusal, or a normalized argv without a call.
pub fn build_goal(st: &StepObs) -> bool {
    if st.norm.effect != Effect::Verify || is_test_run(st) {
        return false;
    }
    if let Some(r) = st.verification() {
        return r.outcome == VerificationOutcome::Passed;
    }
    if st.call.failed || st.call.refused() || st.call.tool != "Bash" {
        return false;
    }
    let CanonicalAction::Exec { argv, .. } = &st.norm.action else { return false };
    st.call
        .input
        .get("command")
        .and_then(|c| c.as_str())
        .and_then(adage_core::shell::plain_pipeline)
        .is_some_and(|stages| matches!(stages.as_slice(), [one] if one.argv == *argv))
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
    /// Recorded native calls when their identity is retained; otherwise the legacy normalized-
    /// step fallback. This is not proof of a complete transcript. The units must not be pooled.
    #[serde(default)]
    pub unit: RewardUnit,
    /// The goal came within the horizon.
    pub reached: bool,
    /// A harm-class action ran before the goal (or within the horizon).
    pub harm: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewardUnit {
    NativeCalls,
    #[default]
    NormalizedSteps,
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
        Goal::Build => build_goal(st),
    }
}

/// The local reward of the trigger at step `t` of `s` for goal `g`: calls to the first goal event
/// after it, censored at `horizon`. One function for learning and for replay.
pub fn local(s: &Session, t: usize, g: Goal, horizon: u32) -> LocalReward {
    window(s, t, g, horizon, false).reward
}

struct Window {
    /// Only steps in the bounded, same-task window, with their cost from the trigger.
    steps: Vec<(usize, u32)>,
    reward: LocalReward,
}

fn native_id(st: &StepObs) -> Option<&str> {
    st.call.tool_use_id.as_deref().filter(|id| !id.is_empty() && id.len() <= 256)
}

fn in_cwd(n: &Normalized, cwd: &str) -> bool {
    match &n.action {
        CanonicalAction::Exec { cwd: actual, .. } => actual == cwd,
        CanonicalAction::Compound { actions, .. } => actions.iter().all(|a| in_cwd(a, cwd)),
        _ => true,
    }
}

// Count complete original calls, not the multiple normalized actions a shell call can produce.
// A changed prompt, actor, cwd, or Adage-made call ends the observation. Environment-failure
// recovery additionally stops at mutations/opaque work: a later passing test after an install
// is not evidence for an interpreter-only repair. Refused attempts cost a call but repair nothing.
fn window(s: &Session, t: usize, g: Goal, horizon: u32, recovery: bool) -> Window {
    let empty = || Window {
        steps: Vec::new(),
        reward: LocalReward { calls: horizon, unit: RewardUnit::NormalizedSteps, reached: false, harm: false },
    };
    let Some(trigger) = s.steps.get(t).filter(|st| st.call.by.is_agent()) else { return empty() };
    let cwd = match &trigger.norm.action {
        CanonicalAction::Exec { cwd, .. } => cwd.as_str(),
        _ => s.root.to_str().unwrap_or(""),
    };
    if cwd.is_empty() {
        return empty();
    }
    let next_prompt = s.prompts.iter().map(|p| p.at_step as usize).filter(|p| *p > t).min();
    let mut exact = native_id(trigger).is_some();
    let mut ids = std::collections::BTreeSet::new();
    if let Some(id) = native_id(trigger) {
        ids.insert(id);
    }
    let mut groups = Vec::new();
    let mut harmed = false;
    let mut previous_turn = trigger.turn;
    let mut i = t + 1;
    while i < s.steps.len() && trigger.same_native_call(&s.steps[i]) {
        i += 1;
    }
    while i < s.steps.len() && groups.len() < horizon as usize {
        let first = &s.steps[i];
        let mut end = i + 1;
        while end < s.steps.len() && first.same_native_call(&s.steps[end]) {
            end += 1;
        }
        if s.steps[i..end].iter().enumerate().any(|(offset, st)| {
            st.prompt != trigger.prompt
                || st.turn <= trigger.turn
                || st.turn < previous_turn
                || st.turn > previous_turn.saturating_add(1)
                || next_prompt.is_some_and(|p| i + offset >= p)
                || st.call.agent != trigger.call.agent
                || !st.call.by.is_agent()
                || !in_cwd(&st.norm, cwd)
                || ["cwd", "workdir"]
                    .iter()
                    .any(|key| st.call.input.get(key).and_then(|v| v.as_str()).is_some_and(|actual| actual != cwd))
        }) {
            break;
        }
        // A turn's tool calls can run concurrently. Their transcript order does not prove that
        // a passing test preceded an install/edit in the same batch; do not credit that repair.
        if recovery
            && s.steps[i..end].iter().any(|st| is_goal(st, g))
            && s.steps.iter().enumerate().any(|(k, st)| {
                !(i..end).contains(&k)
                    && st.turn == first.turn
                    && st.call.agent == first.call.agent
                    && !first.same_native_call(st)
            })
        {
            break;
        }
        let refused = s.steps[i..end].iter().any(|st| st.call.refused());
        harmed |= !refused && s.steps[i..end].iter().any(harm);
        if recovery
            && !refused
            && (!s.steps[i..end].iter().all(|st| {
                !st.norm.mutating
                    && !st.norm.contains_unknown()
                    && !st.norm.low_confidence()
                    && matches!(st.norm.effect, Effect::Observe | Effect::Verify)
            }) || !match first.call.tool.as_str() {
                "Read" | "Grep" | "Glob" | "LS" => true,
                "Bash" => {
                    first
                        .call
                        .input
                        .get("command")
                        .and_then(|v| v.as_str())
                        .is_some_and(adage_core::shell::observational_commands)
                        || (end == i + 1
                            && first.verification().is_some_and(|r| {
                                matches!(
                                    r.outcome,
                                    VerificationOutcome::Passed
                                        | VerificationOutcome::Failed
                                        | VerificationOutcome::SetupFailure
                                        | VerificationOutcome::NoTests
                                ) || executed_tests(first)
                            }))
                }
                _ => false,
            })
        {
            break;
        }
        exact &= native_id(first).is_some_and(|id| ids.insert(id));
        groups.push(i..end);
        if s.steps[i..end].iter().any(|st| is_goal(st, g)) || (!exact && end - t > horizon as usize) {
            break;
        }
        previous_turn = first.turn;
        i = end;
    }
    let unit = if exact { RewardUnit::NativeCalls } else { RewardUnit::NormalizedSteps };
    let mut steps = Vec::new();
    for (call, group) in groups.into_iter().enumerate() {
        for k in group {
            let calls = if exact { call + 1 } else { k - t } as u32;
            if calls <= horizon {
                steps.push((k, calls));
            }
        }
    }
    let goal = steps.iter().find(|(k, _)| is_goal(&s.steps[*k], g));
    let reward =
        LocalReward { calls: goal.map_or(horizon, |(_, calls)| *calls), unit, reached: goal.is_some(), harm: harmed };
    Window { steps, reward }
}

/// A delayed environment repair witnessed inside the same bounded native-call window as local
/// outcome accounting. This is recurrence evidence, never an immediate choice, a task grade, or
/// proof that unrecorded filesystem/environment state was unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecoveryWitness {
    pub at: usize,
    pub goal_at: usize,
    pub calls: u32,
}

/// A direct literal argv belonging to one complete native call at the recorded repository root.
/// A sibling tool in the same turn or a compound sharing its result is not an ordered witness.
fn direct_root_argv(s: &Session, at: usize) -> Option<&[String]> {
    let st = s.steps.get(at)?;
    let id = native_id(st)?;
    if st.call.tool != "Bash" || !st.call.by.is_agent() || st.call.refused() {
        return None;
    }
    if s.steps.iter().enumerate().any(|(i, other)| {
        i != at && (native_id(other) == Some(id) || (other.turn == st.turn && other.call.agent == st.call.agent))
    }) {
        return None;
    }
    let CanonicalAction::Exec { argv, cwd } = &st.norm.action else { return None };
    if argv.is_empty() || cwd.is_empty() || s.root.to_str() != Some(cwd) || st.norm.effect != Effect::Verify {
        return None;
    }
    let raw = st.call.input.get("command")?.as_str()?;
    let stages = adage_core::shell::plain_pipeline(raw)?;
    matches!(stages.as_slice(), [one] if one.argv == *argv).then_some(argv.as_slice())
}

/// The first productive check after a typed environment failure, if it passed with actual test
/// cases and changed only the executable environment. All test arguments remain byte-for-byte
/// equal: no selector, flag, or scope is inferred from a different successful command. Managed
/// wrappers and other shapes that cannot establish this exact relation abstain.
pub(crate) fn recovery_witness(s: &Session, at: usize) -> Option<RecoveryWitness> {
    let failed = s.steps.get(at)?;
    if !failed.call.failed
        || failed.verification()?.outcome != VerificationOutcome::SetupFailure
        || env_signature(&failed.call.output).is_none()
    {
        return None;
    }
    let before = direct_root_argv(s, at)?;
    let from = runner(failed)?;
    if from.env.class() != action::EnvClass::System {
        return None;
    }
    let observed = window(s, at, Goal::Test, HORIZON, true);
    if observed.reward.unit != RewardUnit::NativeCalls || !observed.reward.reached || observed.reward.harm {
        return None;
    }
    let &(goal_at, calls) = observed.steps.iter().find(|(i, _)| test_goal(&s.steps[*i]))?;
    let goal = &s.steps[goal_at];
    let after = direct_root_argv(s, goal_at)?;
    let to = runner(goal)?;
    let report = goal.verification()?;
    let same_program =
        base(&before[0]) == base(&after[0]) || (action::is_python(&before[0]) && action::is_python(&after[0]));
    (!goal.call.failed
        && report.outcome == VerificationOutcome::Passed
        && report.tests_run.is_some_and(|n| n > 0)
        && from.family == to.family
        && to.env.class() == action::EnvClass::Project
        && from.env != to.env
        && same_program
        && before[1..] == after[1..])
        .then_some(RecoveryWitness { at, goal_at, calls })
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
    /// The next test attempt was the very next native call, decided after this failure. A
    /// delayed discovery is useful candidate evidence, but not an immediate unaided choice.
    #[serde(default)]
    pub immediate_next: bool,
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
        let (trigger, failed, signature) = if let Some(r) =
            runner(st).filter(|_| st.verification().is_some_and(|r| r.outcome == VerificationOutcome::SetupFailure))
        {
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
        let observed = window(s, i, Goal::Test, HORIZON, failed.is_some());
        let next = observed.steps.iter().find_map(|(k, _)| {
            let x = &s.steps[*k];
            (x.turn > st.turn && x.verification().is_some()).then(|| runner(x).map(|r| (*k, r))).flatten()
        });
        let immediate_next = observed.reward.unit == RewardUnit::NativeCalls
            && next.as_ref().is_some_and(|(k, _)| {
                observed.steps.first() == Some(&(*k, 1))
                    && s.steps[*k].turn == st.turn.saturating_add(1)
                    && s.steps[*k].verification().is_some_and(|r| {
                        matches!(
                            r.outcome,
                            VerificationOutcome::Passed
                                | VerificationOutcome::Failed
                                | VerificationOutcome::SetupFailure
                        ) || executed_tests(&s.steps[*k])
                    })
            });
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
                s.steps[*k].verification().is_some_and(|r| r.outcome == VerificationOutcome::SetupFailure)
                    && env_failure(&s.steps[*k].call.output).is_some()
            }),
            next_at: next.as_ref().map(|(k, _)| *k),
            immediate_next,
            next: next.map(|(_, r)| r),
            reward: observed.reward,
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
    fn env_failures_are_typed() {
        assert_eq!(env_failure("/usr/bin/python: No module named pytest").as_deref(), Some("no-module:pytest"));
        assert_eq!(env_failure("ModuleNotFoundError: No module named 'click'").as_deref(), Some("no-module:click"));
        assert_eq!(env_failure("bash: pytest: command not found").as_deref(), Some("cmd-not-found:pytest"));
        assert_eq!(env_failure("AssertionError: 1 != 2"), None);
    }

    #[test]
    fn missing_commands_keep_program_identity_across_shell_dialects() {
        for (output, signature) in [
            ("(eval):1: command not found: cargo", "command not found: cargo"),
            ("(eval):19: command not found: cargo", "command not found: cargo"),
            ("zsh: command not found: cargo", "command not found: cargo"),
            ("zsh:4: command not found: cargo", "command not found: cargo"),
            ("bash: cargo: command not found", "cargo: command not found"),
            ("sh: 1: cargo: not found", "cargo: not found"),
        ] {
            let expected = Some(("cmd-not-found:cargo".into(), signature.into()));
            assert_eq!(env_signature(output), expected, "{output}");
            assert!(output.contains(signature), "the trigger must match the actual diagnostic");
        }
        assert_eq!(env_failure("(eval):1: command not found: timeout").as_deref(), Some("cmd-not-found:timeout"));
        assert_eq!(env_failure("(eval):1: command not found: python3.11").as_deref(), Some("cmd-not-found:python3.11"));
    }

    #[test]
    fn malformed_reverse_diagnostics_are_not_shell_or_line_number_failures() {
        for output in [
            "zsh: command not found:",
            "(eval):1: command not found: cargo --version",
            "(eval):1: command not found: cargo; echo unrelated",
            "(eval):1: command not found: /custom/cargo",
            "assert message == '(eval):1: command not found: cargo'",
            "command not found: cargo",
        ] {
            assert_eq!(env_signature(output), None, "{output}");
        }
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
