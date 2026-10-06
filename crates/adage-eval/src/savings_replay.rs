//! The offline replay savings metric Ŝ (`evals/launch/architecture-review.md` §5.2), the number
//! the engine is optimised against. Definitions, calibration and baselines: `evals/replay/README.md`.
//!
//! A held-out trace τ = (a₁…aₙ) is replayed through the real runtime (`adage_runtime::decide`, the
//! control arm, as [`crate::replay`] does), and each directive d_t it would have given at hook
//! event t is scored against what the agent did afterwards:
//!
//! * **Sat(d)**: the equivalence class of actions that satisfy d ([`Class::sat`]). For a
//!   test-runner directive: a test run whose interpreter and runner equal d's and that executed
//!   tests (its output says so: `collected N items`, `N passed`, `Tests:`, …).
//! * **W(d)**: the waste class d targets ([`Class::waste`]). For a test-runner directive, the
//!   environment friction: runner invocations that failed without running tests, any call whose
//!   output carries an environment signature (`No module named …`, `command not found`, …), and
//!   environment probes and repairs (`pip install`, `which python`, `ls .venv`, …). For a
//!   file→test directive, test-discovery searches.
//! * **t\*** = the first t′ > t with a_t′ ∈ Sat(d). d is **useful** if t\* exists, **harmful**
//!   if the session passed with an action that contradicts d (a different runner, or a different
//!   test file, executed tests while d's never did after t).
//! * **S(d_t)** = |{a_k ∈ W(d) : t < k < t\*}| + 𝟙[d pre-emptive ∧ a_t ∉ Sat(d) ∧ a_t failed]
//!   (0 when d is not useful).
//! * **C(d)** = directive tokens × cache-read price × the API calls left in the session, divided
//!   by the session's mean $ per tool call: its cost in calls.
//! * **Ŝ** = the mean over sessions of Σ_t [S(d_t) − C(d_t)], with a bootstrap CI that resamples
//!   tasks; also the useful rate (Wilson 90% lower bound), harmful rate, directives per 10 calls,
//!   and Ŝ in dollars per session.
//!
//! Two delivery modes: **post-hoc** (the directive arrives after the event that triggered it, as
//! the hooks deliver today) and **pre-emptive** (at `PreToolUse` of a call that would fail, the
//! failure-triggered routes are asked "had we intercepted this call": what a `PreToolUse` rewrite
//! would do, scored before it is built). A directive given inside the recovery window of an
//! earlier useful one is **superseded** (in the counterfactual the agent never got there) and is
//! neither scored nor costed.
//!
//! Read-only: it replays transcripts and runs nothing.

use crate::transcript::{self, Item, ToolCall, Transcript};
use adage_core::CommandTable;
use adage_runtime::{decide, Arm, Call, Ctx, Event, RuntimeParams, SessionState};
use adage_tracks::{Index, Origin, Step, Trigger};
use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------
// Actions: one per tool call, with the features Sat and W are defined on.
// ---------------------------------------------------------------------------------------------

/// A test invocation: the environment it ran in, and the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Invocation {
    /// `system` (a bare `python`/`pytest` outside any project environment), `venv` (the project's
    /// `.venv`, activated, by path, or via `uv run`), `tox`, `nox`, `node` (the project's
    /// `node_modules`, through `npx`/`pnpm`/`yarn`/`npm`), `go`, `cargo`.
    pub interp: &'static str,
    /// `pytest`, `unittest`, `tox`, `nox`, `vitest`, `jest`, `mocha`, `pkg-test` (a package
    /// manager's `test` script), `node-test`, `go-test`, `cargo-test`.
    pub runner: &'static str,
}

/// One tool call a_k and its features.
#[derive(Debug, Clone, Default)]
pub struct Act {
    pub tool: String,
    /// The Bash command, with the session's working directory made relative.
    pub cmd: String,
    pub output: String,
    /// The harness saw the call fail (`is_error`).
    pub is_error: bool,
    /// The test invocation this call made, if it ran a test runner.
    pub run: Option<Invocation>,
    /// Test files (repository-relative) a test run named.
    pub targets: Vec<String>,
    /// A test run whose output shows tests executed.
    pub executed: bool,
    /// A Bash call whose output carries an environment signature.
    pub env_sig: bool,
    /// An environment probe or repair (`pip install`, `which python`, `ls .venv`, a manifest read…).
    pub env_probe: bool,
    /// A search for tests (Grep/Glob/find/grep/ls mentioning tests).
    pub discovery: bool,
    /// The repository-relative file an edit wrote.
    pub edited: Option<String>,
}

impl Act {
    /// The call failed: the harness said so, or its output carries an environment signature (a
    /// piped `… 2>&1 | head` exits 0).
    pub fn failed(&self) -> bool {
        self.is_error || self.env_sig
    }

    /// Environment/test-invocation friction: the waste class of a test-runner directive.
    pub fn env_friction(&self) -> bool {
        self.env_sig || self.env_probe || (self.run.is_some() && !self.executed && self.is_error)
    }
}

static ENV_SIG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"No module named|ModuleNotFoundError|command not found|: not found\b|Cannot find module|ERR_MODULE_NOT_FOUND|executable file not found|is not recognized as|ImportError while loading conftest|(python3?|pytest|/bin/\S+|tox|nox|vitest|jest|npx): No such file or directory",
    )
    .unwrap()
});

static EXECUTED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?m)collected [1-9]\d* items?|\b\d+ passed\b|\b\d+ failed\b|^\s*Tests:\s+\d|Test Files\s+\d|\bRan [1-9]\d* tests?\b|^ok\s+\S+|^--- (PASS|FAIL)|^(PASS|FAIL)\s+\S|test result: (ok|FAILED)|^# tests [1-9]",
    )
    .unwrap()
});

static ENV_PROBE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?x)
        \bpip3?\s+(install|list|show|freeze)\b | -m\s+pip\b | \buv\s+(pip|sync|venv|add|lock)\b | -m\s+venv\b
        | \bvirtualenv\b | (^|[;&|]\s*)which\s | \bcommand\s+-v\s | \btype\s+(-\w+\s+)?(python|pytest|pip)
        | \b(python3?|pytest|pip3?|node|npm|pnpm|tox|uv)\s+(--version|-V)\b
        | \bls\b[^\n|;&]*(\.venv|venv/|site-packages|/bin\b)
        | \bfind\b[^\n]*(venv|site-packages|bin/python|-name\s+["']?(pytest|pyproject|setup\.py|setup\.cfg|tox\.ini))
        | ^\s*(source|\.)\s+\S*activate\s*$
        | \b(npm|pnpm|yarn)\s+(install|ci|i)\b
        | \b(cat|head|less|grep\s.*)\s+[^\n|;&]*\b(pyproject\.toml|setup\.cfg|tox\.ini|pytest\.ini|noxfile\.py|requirements[\w.-]*\.txt)\b
        "#,
    )
    .unwrap()
});

static MANIFEST_READ: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(^|/)(pyproject\.toml|setup\.cfg|tox\.ini|pytest\.ini|noxfile\.py|requirements[\w.-]*\.txt)$").unwrap()
});

static TESTISH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)test|spec\b|__tests__").unwrap());

/// Split a shell command into its simple commands (on `&&`, `||`, `;`, `|` and newlines).
fn segments(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut sq, mut dq) = (false, false);
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' if !dq => sq = !sq,
            '"' if !sq => dq = !dq,
            _ => {}
        }
        if !sq && !dq && (c == ';' || c == '|' || c == '&' || c == '\n') {
            if !cur.trim().is_empty() {
                out.push(cur.trim().to_string());
            }
            cur.clear();
            // `&&`, `||`, `|&`: one separator.
            while i + 1 < chars.len() && matches!(chars[i + 1], '&' | '|') {
                i += 1;
            }
        } else {
            cur.push(c);
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn words(seg: &str) -> Vec<String> {
    seg.split_whitespace().map(|w| w.trim_matches(|c| c == '"' || c == '\'').to_string()).collect()
}

fn base(w: &str) -> &str {
    w.rsplit('/').next().unwrap_or(w)
}

fn is_python(w: &str) -> bool {
    let b = base(w);
    b == "python" || b == "python3" || (b.starts_with("python3.") && b[8..].chars().all(|c| c.is_ascii_digit()))
}

fn in_venv(w: &str) -> bool {
    w.contains(".venv/bin/") || w.starts_with("venv/bin/") || w.contains("/venv/bin/")
}

/// The test invocation a command makes, and the arguments after the runner. `None` for a
/// command that runs no test runner.
pub fn invocation(cmd: &str) -> Option<(Invocation, Vec<String>)> {
    let mut active = false;
    for seg in segments(cmd) {
        let mut w = words(&seg);
        // Leading assignments and wrappers.
        while let Some(f) = w.first() {
            let wrapper = matches!(f.as_str(), "env" | "time" | "exec" | "nice" | "command")
                || (f.contains('=') && !f.starts_with('-') && !f.contains('/'))
                || f == "timeout";
            if !wrapper {
                break;
            }
            let timeout = f == "timeout";
            w.remove(0);
            if timeout && w.first().is_some_and(|x| x.chars().next().is_some_and(|c| c.is_ascii_digit())) {
                w.remove(0);
            }
        }
        let Some(first) = w.first().cloned() else { continue };
        if (first == "source" || first == ".") && w.get(1).is_some_and(|p| p.ends_with("bin/activate")) {
            active = true;
            continue;
        }
        let py_env = |p: &str| if in_venv(p) || active { "venv" } else { "system" };
        let rest = |i: usize| w.get(i..).map(|s| s.to_vec()).unwrap_or_default();
        let inv = |interp: &'static str, runner: &'static str| Invocation { interp, runner };
        // `uv run [--flags] <cmd>`: the project's environment.
        if first == "uv" && w.get(1).map(String::as_str) == Some("run") {
            let mut i = 2;
            while w.get(i).is_some_and(|x| x.starts_with('-')) {
                i += 1;
            }
            let inner = w.get(i..).map(|s| s.join(" ")).unwrap_or_default();
            if let Some((r, args)) = invocation(&inner) {
                return Some((Invocation { interp: "venv", runner: r.runner }, args));
            }
            continue;
        }
        if is_python(&first) {
            if let Some(m) = w.iter().position(|x| x == "-m") {
                let r = match w.get(m + 1).map(String::as_str) {
                    Some("pytest") | Some("py.test") => Some("pytest"),
                    Some("unittest") => Some("unittest"),
                    Some("tox") => return Some((inv("tox", "tox"), rest(m + 2))),
                    Some("nox") => return Some((inv("nox", "nox"), rest(m + 2))),
                    _ => None,
                };
                if let Some(r) = r {
                    return Some((inv(py_env(&first), r), rest(m + 2)));
                }
            }
            continue;
        }
        match base(&first) {
            "pytest" | "py.test" => return Some((inv(py_env(&first), "pytest"), rest(1))),
            "tox" => return Some((inv("tox", "tox"), rest(1))),
            "nox" => return Some((inv("nox", "nox"), rest(1))),
            b @ ("vitest" | "jest" | "mocha") => {
                let r = match b {
                    "vitest" => "vitest",
                    "jest" => "jest",
                    _ => "mocha",
                };
                return Some((inv("node", r), rest(1)));
            }
            "npx" | "pnpm" | "yarn" | "npm" | "bunx" => {
                let mut i = 1;
                while w.get(i).is_some_and(|x| x.starts_with('-') || x == "exec" || x == "run" || x == "--") {
                    // `pnpm --filter x exec vitest`, `npm run test`
                    if w.get(i).is_some_and(|x| x == "--filter" || x == "-F" || x == "-C" || x == "--dir") {
                        i += 1;
                    }
                    i += 1;
                }
                match w.get(i).map(|x| base(x)) {
                    Some("vitest") => return Some((inv("node", "vitest"), rest(i + 1))),
                    Some("jest") => return Some((inv("node", "jest"), rest(i + 1))),
                    Some("mocha") => return Some((inv("node", "mocha"), rest(i + 1))),
                    Some(t) if t == "test" || t.starts_with("test:") => {
                        return Some((inv("node", "pkg-test"), rest(i + 1)))
                    }
                    _ => {}
                }
            }
            "node" if w.get(1).map(String::as_str) == Some("--test") => {
                return Some((inv("node", "node-test"), rest(2)))
            }
            "go" if w.get(1).map(String::as_str) == Some("test") => return Some((inv("go", "go-test"), rest(2))),
            "cargo" if w.get(1).map(String::as_str) == Some("test") => {
                return Some((inv("cargo", "cargo-test"), rest(2)))
            }
            _ => {}
        }
    }
    None
}

/// Repository-relative path of `p` given the session's working directory.
fn rel(p: &str, cwd: &str) -> String {
    let c = cwd.trim_end_matches('/');
    let r = p.strip_prefix(&format!("{c}/")).unwrap_or(p);
    r.trim_start_matches("./").to_string()
}

/// Test files a test run names: path-like arguments with a test-ish name, `::node` stripped.
fn targets(args: &[String], cwd: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for a in args {
        if a.starts_with('-') {
            continue;
        }
        let p = a.split("::").next().unwrap_or(a);
        if (p.contains('/') || p.contains('.')) && TESTISH.is_match(p) && !p.contains('=') {
            out.insert(rel(p, cwd));
        }
    }
    out.into_iter().collect()
}

/// The packages a `go test` names, as `pkg:<dir>` (`pkg:` the root, `pkg:x/...` recursive):
/// Go targets packages, not files.
fn go_packages(args: &[String], cwd: &str) -> Vec<String> {
    const VALUED: [&str; 10] =
        ["-run", "-count", "-timeout", "-tags", "-p", "-parallel", "-coverprofile", "-bench", "-skip", "-cpu"];
    let mut out = BTreeSet::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a.starts_with('-') {
            skip = VALUED.contains(&a.as_str());
            continue;
        }
        let p = rel(a, cwd);
        let p = p.trim_start_matches("./").trim_end_matches('/');
        let p = if p == "." { "" } else { p };
        out.insert(format!("pkg:{p}"));
    }
    if out.is_empty() {
        out.insert("pkg:".to_string());
    }
    out.into_iter().collect()
}

fn str_field<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

/// The action a tool call is.
pub fn act(c: &ToolCall, cwd: &str) -> Act {
    let mut a = Act { tool: c.tool.clone(), output: c.output.clone(), is_error: c.failed, ..Default::default() };
    let c_trim = cwd.trim_end_matches('/');
    match c.tool.as_str() {
        "Bash" => {
            let raw = str_field(&c.input, "command");
            a.cmd = raw.replace(&format!("{c_trim}/"), "").replace(c_trim, ".");
            a.env_sig = ENV_SIG.is_match(&c.output);
            if let Some((inv, args)) = invocation(&a.cmd) {
                a.run = Some(inv);
                a.targets = if inv.runner == "go-test" { go_packages(&args, cwd) } else { targets(&args, cwd) };
                a.executed = EXECUTED.is_match(&c.output) && !a.env_sig;
            } else {
                // A listing of the repository's root (`ls -la | head`): looking for the environment.
                let root_ls = segments(&a.cmd).iter().any(|s| {
                    let w = words(s);
                    w.first().is_some_and(|f| f == "ls")
                        && w[1..].iter().all(|x| x.starts_with('-') || x == "." || x == "./")
                });
                a.env_probe = root_ls || ENV_PROBE.is_match(&a.cmd);
                let first = segments(&a.cmd)
                    .into_iter()
                    .map(|s| words(&s))
                    .find(|w| w.first().is_some_and(|f| f != "cd"))
                    .and_then(|w| w.first().cloned())
                    .unwrap_or_default();
                let search = matches!(base(&first), "find" | "grep" | "rg" | "ls" | "fd" | "tree" | "ag")
                    || a.cmd.contains("git grep")
                    || a.cmd.contains("git ls-files");
                a.discovery = search && TESTISH.is_match(&a.cmd);
            }
        }
        "Grep" | "Glob" => {
            let q = format!(
                "{} {} {}",
                str_field(&c.input, "pattern"),
                str_field(&c.input, "path"),
                str_field(&c.input, "glob")
            );
            a.discovery = TESTISH.is_match(&q);
        }
        "Read" => {
            let p = rel(str_field(&c.input, "file_path"), cwd);
            a.env_probe = MANIFEST_READ.is_match(&p);
        }
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            let p = str_field(&c.input, "file_path");
            let p = if p.is_empty() { str_field(&c.input, "notebook_path") } else { p };
            if !p.is_empty() {
                a.edited = Some(rel(p, cwd));
            }
        }
        _ => {}
    }
    a
}

pub fn is_test_path(p: &str) -> bool {
    TESTISH.is_match(p)
}

// ---------------------------------------------------------------------------------------------
// Directive classes: Sat(d) and W(d).
// ---------------------------------------------------------------------------------------------

/// How Sat(d) treats the interpreter of a test-runner directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SatRule {
    /// The goal class (the default, calibrated against the live U11 measurement; see
    /// `evals/replay/README.md`): the runner equals d's and tests executed, in whatever
    /// environment the agent got working (a `pip install` into the system Python reaches the same
    /// goal, later and by a worse route).
    #[default]
    Goal,
    /// The first definition: interpreter and runner equal d's, and tests executed. It scores the
    /// recoveries that went through `pip install` as not useful and under-predicts the live saving
    /// eightfold.
    Strict,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "class", rename_all = "kebab-case")]
pub enum Class {
    /// Run the tests with this invocation.
    Runner { interp: &'static str, runner: &'static str },
    /// Run this command (not a test runner): Sat is a call starting with it that did not fail.
    Command { head: String },
    /// Run these test files (a file→test directive).
    TestFiles { files: Vec<String> },
    /// Run the tests of this area (a monorepo package, `packages/runtime-core`) with this runner:
    /// the case store's prompt → area directive.
    Area { runner: &'static str, dir: String },
    /// Advice or a read batch: nothing in a trace can show it satisfied.
    Unscored,
}

fn command_class(cmd: &str) -> Class {
    if let Some((inv, _)) = invocation(cmd) {
        return Class::Runner { interp: inv.interp, runner: inv.runner };
    }
    let head: Vec<String> = words(cmd).into_iter().filter(|w| !w.starts_with('{')).take(2).collect();
    if head.is_empty() {
        Class::Unscored
    } else {
        Class::Command { head: head.join(" ") }
    }
}

impl Class {
    pub fn of_step(step: &Step) -> Class {
        match step {
            Step::Run { command } => command_class(command),
            Step::Sequence { commands } => commands.first().map(|c| command_class(c)).unwrap_or(Class::Unscored),
            Step::Avoid { instead, .. } => command_class(instead),
            Step::Advise { .. } | Step::ReadBatch { .. } | Step::Card { .. } => Class::Unscored,
        }
    }

    pub fn sat(&self, a: &Act, rule: SatRule) -> bool {
        match self {
            Class::Runner { interp, runner } => a
                .run
                .is_some_and(|r| r.runner == *runner && a.executed && (rule == SatRule::Goal || r.interp == *interp)),
            Class::Command { head } => {
                !a.failed() && segments(&a.cmd).iter().any(|s| s == head || s.starts_with(&format!("{head} ")))
            }
            Class::TestFiles { files } => {
                a.run.is_some() && a.executed && a.targets.iter().any(|t| files.iter().any(|f| same_file(t, f)))
            }
            Class::Area { runner, dir } => {
                a.run.is_some_and(|r| r.runner == *runner) && a.executed && a.targets.iter().any(|t| under(t, dir))
            }
            Class::Unscored => false,
        }
    }

    pub fn waste(&self, a: &Act) -> bool {
        match self {
            Class::Runner { .. } => a.env_friction(),
            Class::Command { .. } => a.failed(),
            Class::TestFiles { .. } | Class::Area { .. } => a.discovery,
            Class::Unscored => false,
        }
    }

    /// An action that contradicts d, if the session reached its goal with it: another runner (or
    /// another test file) that executed tests.
    fn contradicts(&self, a: &Act) -> bool {
        match self {
            Class::Runner { runner, .. } => a.run.is_some_and(|r| r.runner != *runner && a.executed),
            Class::TestFiles { files } => {
                a.run.is_some()
                    && a.executed
                    && !a.targets.is_empty()
                    && !a.targets.iter().any(|t| files.iter().any(|f| same_file(t, f)))
            }
            Class::Area { runner, dir } => {
                a.run.is_some_and(|r| r.runner == *runner)
                    && a.executed
                    && !a.targets.is_empty()
                    && !a.targets.iter().any(|t| under(t, dir))
            }
            _ => false,
        }
    }

    /// d's own action was taken (whatever the interpreter): what a contradiction must lack.
    fn own(&self, a: &Act) -> bool {
        match self {
            Class::Runner { runner, .. } => a.run.is_some_and(|r| r.runner == *runner && a.executed),
            Class::TestFiles { .. } | Class::Area { .. } => self.sat(a, SatRule::Strict),
            _ => false,
        }
    }
}

/// A test-run target inside an area directory.
fn under(target: &str, dir: &str) -> bool {
    let t = target.strip_prefix("pkg:").unwrap_or(target).trim_start_matches("./");
    t == dir || t.starts_with(&format!("{dir}/"))
}

/// A test-run target names a directed file: equal, or one a path suffix of the other (a run
/// from a package directory), or a bare name filter (`vitest run foo`) inside the file name.
fn same_file(target: &str, file: &str) -> bool {
    if let Some(pkg) = target.strip_prefix("pkg:") {
        let dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        return match pkg.strip_suffix("...") {
            Some(pre) => {
                let pre = pre.trim_end_matches('/');
                pre.is_empty() || dir == pre || dir.starts_with(&format!("{pre}/"))
            }
            None => dir == pkg,
        };
    }
    target == file
        || file.ends_with(&format!("/{target}"))
        || target.ends_with(&format!("/{file}"))
        || (!target.contains('/') && target.len() >= 4 && base(file).contains(target))
}

// ---------------------------------------------------------------------------------------------
// The policy: the runtime over an index, or the co-change oracle.
// ---------------------------------------------------------------------------------------------

/// A hypothetical file→test policy: after an edit to a source file, direct the test files that
/// most often changed with it in the repository's history before the task's base commit.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CoChange {
    /// Task → source file → test files, best first.
    pub tasks: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// How many test files to direct (3).
    #[serde(default = "three")]
    pub k: usize,
}

fn three() -> usize {
    3
}

pub enum Policy<'a> {
    /// `adage_runtime::decide` over this index.
    Runtime {
        index: &'a Index,
        repo: &'a str,
        root: &'a Path,
        table: &'a CommandTable,
        params: RuntimeParams,
        /// The prompt embedding intent routes match against (`None`: no model, as today's replay).
        embed: Option<adage_runtime::Embed<'a>>,
        /// The case store, asked where the runtime gives nothing.
        cases: Option<CaseCtx<'a>>,
    },
    CoChange(&'a CoChange),
}

/// The case store in a replay (`adage_learn::cases`).
#[derive(Clone, Copy)]
pub struct CaseCtx<'a> {
    pub store: &'a adage_learn::cases::CaseStore,
    pub embedder: &'a adage_intent::model::Embedder,
    /// Give proposals the value model did not admit too (a diagnostic: what the gate prevents).
    pub ungated: bool,
}

/// What the case store would say at one decision point of a replayed session, once per proposal.
struct CaseState {
    steps: Vec<adage_learn::import::StepObs>,
    prompt: String,
    prompts: u16,
    turn: u32,
    given: BTreeSet<usize>,
}

impl CaseState {
    fn ask(&mut self, c: &CaseCtx, kind: adage_learn::cases::Kind, upto: usize) -> Option<(String, Class, u32)> {
        let (p, s) = adage_learn::cases::vectors(c.embedder, &self.prompt, &self.steps[..upto.min(self.steps.len())]);
        let g = c.store.propose(kind, &p, &s, None, c.ungated)?;
        if !self.given.insert(g.index) {
            return None;
        }
        let pr = g.proposal;
        let class = match (&pr.area, invocation(&pr.command)) {
            (Some(dir), Some((inv, _))) => Class::Area { runner: inv.runner, dir: dir.clone() },
            _ => command_class(&pr.command),
        };
        let text = format!(
            "<adage-reflex route=\"case\" kind=\"prime\">Similar past sessions in this repository reached passing tests with: {}</adage-reflex>",
            pr.command
        );
        Some((format!("case:{}", pr.key), class, tokens_of(&text)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Delivered after the event that triggered it (today's hooks).
    #[default]
    PostHoc,
    /// Failure-triggered routes also asked at `PreToolUse` of a call that would fail.
    PreEmptive,
}

/// A directive the policy gave at call index t.
#[derive(Debug, Clone, Serialize)]
pub struct Given {
    pub route: String,
    /// The index of the call it follows (post-hoc) or intercepts (pre-emptive); −1 before any call.
    pub t: i64,
    pub pre: bool,
    pub event: &'static str,
    pub tokens: u32,
    #[serde(flatten)]
    pub class: Class,
}

/// A directive, scored.
#[derive(Debug, Clone, Serialize)]
pub struct Scored {
    #[serde(flatten)]
    pub given: Given,
    pub t_star: Option<usize>,
    pub useful: bool,
    pub harmful: bool,
    /// Given inside an earlier useful directive's recovery window: not in the counterfactual.
    pub superseded: bool,
    /// Calls in W(d) between t and t* (and the intercepted call, pre-emptively).
    pub saved: f64,
    /// Its cost in calls.
    pub cost: f64,
}

/// Tokens of a directive as delivered (its wrapper included): 4 characters a token.
fn tokens_of(text: &str) -> u32 {
    (text.chars().count() as u32).div_ceil(4)
}

fn call_of(c: &ToolCall, cwd: &str, root: &str) -> Call {
    Call {
        tool: c.tool.clone(),
        input: transcript::rebase_value(&c.input, cwd, root),
        output: c.output.replace(cwd, root),
        failed: true,
    }
}

fn instinct_of(ix: &Index, repo: &str, route: &str) -> Option<Class> {
    ix.for_repo(repo).find(|r| r.route.id == route).map(|r| Class::of_step(&r.route.step))
}

/// The directives a policy gives over one session. `acts` is [`act`] of every call in order.
pub fn directives(policy: &Policy, t: &Transcript, acts: &[Act], task: &str, stop: bool, mode: Mode) -> Vec<Given> {
    match policy {
        Policy::CoChange(oracle) => cochange_directives(oracle, t, acts, task),
        Policy::Runtime { index, repo, root, table, params, embed, cases } => {
            let root_s = root.to_string_lossy().to_string();
            let ctx = Ctx {
                checkout: &adage_runtime::checkout::Disk,
                index,
                repo: repo.to_string(),
                root,
                table,
                embed: *embed,
                params: *params,
                policy: Default::default(),
            };
            let mut cs =
                CaseState { steps: Vec::new(), prompt: String::new(), prompts: 0, turn: 0, given: BTreeSet::new() };
            let cwd = PathBuf::from(&t.cwd);
            let mut st = SessionState { arm: Some(Arm::Control), repo: Some(repo.to_string()), ..Default::default() };
            let failure_routes: BTreeSet<String> = index
                .for_repo(repo)
                .filter(|r| matches!(r.route.trigger, Trigger::AfterFailure { .. }))
                .map(|r| r.route.id.clone())
                .collect();
            let steps = transcript::events(t, &root_s, stop);
            let mut steps = steps.into_iter();
            let mut out = Vec::new();
            // At most one directive per hook event: the first the runtime would have given.
            let push =
                |d: adage_runtime::Decision, at: i64, pre: bool, name: &'static str, out: &mut Vec<Given>| -> bool {
                    if let Some(w) = d.would_have.into_iter().find(|w| !pre || failure_routes.contains(&w.route)) {
                        let class = instinct_of(index, repo, &w.route).unwrap_or(Class::Unscored);
                        out.push(Given {
                            tokens: tokens_of(&adage_runtime::render::wrap_directive(&w)),
                            route: w.route,
                            t: at,
                            pre,
                            event: name,
                            class,
                        });
                        return true;
                    }
                    false
                };
            // The case store, where the runtime gave nothing at this event.
            use adage_learn::cases::Kind;
            let ask = |cs: &mut CaseState,
                       kind: Kind,
                       upto: usize,
                       at: i64,
                       pre: bool,
                       name: &'static str,
                       out: &mut Vec<Given>| {
                if let Some(c) = cases {
                    if let Some((route, class, tokens)) = cs.ask(c, kind, upto) {
                        out.push(Given { route, t: at, pre, event: name, tokens, class });
                    }
                }
            };
            let next = |steps: &mut std::vec::IntoIter<transcript::Step>| steps.next().map(|s| s.event);
            if let Some(ev) = next(&mut steps) {
                let d = decide(&ctx, &mut st, &ev);
                push(d, -1, false, transcript::hook_name(&ev), &mut out);
            }
            let mut k = 0usize;
            for item in &t.items {
                match item {
                    Item::Prompt(p) => {
                        cs.prompt = p.clone();
                        cs.prompts += 1;
                        if let Some(ev) = next(&mut steps) {
                            let d = decide(&ctx, &mut st, &ev);
                            if !push(d, k as i64 - 1, false, "UserPromptSubmit", &mut out) {
                                let n = cs.steps.len();
                                ask(&mut cs, Kind::Prompt, n, k as i64 - 1, false, "UserPromptSubmit", &mut out);
                            }
                        }
                    }
                    Item::Turn(calls) => {
                        // Each call's steps, as the importer makes them; `ends[j]` is the step count
                        // once call j is in.
                        let mut ends = Vec::with_capacity(calls.len());
                        if cases.is_some() {
                            for c in calls {
                                let call = adage_runtime::Call {
                                    tool: c.tool.clone(),
                                    input: c.input.clone(),
                                    output: c.output.clone(),
                                    failed: c.failed,
                                };
                                let prompt = cs.prompts.saturating_sub(1);
                                cs.steps.extend(adage_learn::cases::steps_of_call(cs.turn, prompt, &call, &cwd, table));
                                ends.push(cs.steps.len());
                            }
                            cs.turn += 1;
                        }
                        let end = |j: usize| ends.get(j).copied().unwrap_or(0);
                        if mode == Mode::PreEmptive {
                            for (j, c) in calls.iter().enumerate() {
                                let rejected = c.output.trim_start().starts_with("<tool_use_error>");
                                if acts[k + j].failed() && !rejected {
                                    let ev = Event::Failure { call: call_of(c, &t.cwd, &root_s) };
                                    let d = decide(&ctx, &mut st, &ev);
                                    if !push(d, (k + j) as i64, true, "PreToolUse", &mut out) {
                                        ask(
                                            &mut cs,
                                            Kind::Failure,
                                            end(j),
                                            (k + j) as i64,
                                            true,
                                            "PreToolUse",
                                            &mut out,
                                        );
                                    }
                                }
                            }
                        }
                        for (j, c) in calls.iter().enumerate() {
                            if c.failed && !c.output.trim_start().starts_with("<tool_use_error>") {
                                if let Some(ev) = next(&mut steps) {
                                    let d = decide(&ctx, &mut st, &ev);
                                    if !push(d, (k + j) as i64, false, "PostToolUseFailure", &mut out) {
                                        ask(
                                            &mut cs,
                                            Kind::Failure,
                                            end(j),
                                            (k + j) as i64,
                                            false,
                                            "PostToolUseFailure",
                                            &mut out,
                                        );
                                    }
                                }
                            }
                        }
                        k += calls.len();
                        if let Some(ev) = next(&mut steps) {
                            let d = decide(&ctx, &mut st, &ev);
                            if !push(d, k as i64 - 1, false, "PostToolBatch", &mut out) {
                                let n = cs.steps.len();
                                ask(&mut cs, Kind::Batch, n, k as i64 - 1, false, "PostToolBatch", &mut out);
                            }
                        }
                    }
                }
            }
            for ev in steps.map(|s| s.event) {
                let d = decide(&ctx, &mut st, &ev);
                let name = transcript::hook_name(&ev);
                push(d, k as i64 - 1, false, name, &mut out);
            }
            out
        }
    }
}

fn cochange_directives(oracle: &CoChange, t: &Transcript, acts: &[Act], task: &str) -> Vec<Given> {
    let Some(map) = oracle.tasks.get(task) else { return Vec::new() };
    let mut out = Vec::new();
    let mut done = BTreeSet::new();
    let mut k = 0usize;
    for calls in t.turns() {
        let turn = k..k + calls.len();
        k += calls.len();
        // At most one directive per event (the batch): the first edited source file with a map.
        let hit = turn.clone().find_map(|i| {
            let p = acts[i].edited.as_deref()?;
            let tests = map.get(p)?;
            (!is_test_path(p) && !done.contains(p) && !tests.is_empty()).then(|| (p.to_string(), tests.clone()))
        });
        if let Some((src, tests)) = hit {
            let files: Vec<String> = tests.into_iter().take(oracle.k.max(1)).collect();
            let text = format!(
                "<adage-reflex route=\"cochange\" kind=\"prime\">Tests that usually change with {src}: {}</adage-reflex>",
                files.join(", ")
            );
            out.push(Given {
                route: "cochange-oracle".into(),
                t: k as i64 - 1,
                pre: false,
                event: "PostToolBatch",
                tokens: tokens_of(&text),
                class: Class::TestFiles { files },
            });
            done.insert(src);
        }
    }
    out
}

/// Pricing context for C(d).
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Price {
    /// $ per million cache-read tokens for the session's model.
    pub cache_read: f64,
    /// The session's mean $ per tool call.
    pub usd_per_call: f64,
    /// API calls (model responses) per tool call.
    pub api_per_call: f64,
}

/// Score a session's directives.
pub fn score(given: Vec<Given>, acts: &[Act], passed: Option<bool>, price: &Price, rule: SatRule) -> Vec<Scored> {
    let n = acts.len();
    let mut out: Vec<Scored> = Vec::new();
    // Recovery windows of useful directives: (t, t*).
    let mut windows: Vec<(i64, usize)> = Vec::new();
    for g in given {
        let from = (g.t + 1).max(0) as usize;
        let t_star = (from..n).find(|&k| g.class.sat(&acts[k], rule));
        let superseded = windows.iter().any(|&(a, b)| a <= g.t && g.t < b as i64);
        let useful = t_star.is_some();
        let harmful = !useful
            && passed == Some(true)
            && acts[from.min(n)..].iter().any(|a| g.class.contradicts(a))
            && !acts[from.min(n)..].iter().any(|a| g.class.own(a));
        let mut saved = 0.0;
        if let (Some(ts), false) = (t_star, superseded) {
            saved = acts[from..ts].iter().filter(|a| g.class.waste(a)).count() as f64;
            if g.pre && g.t >= 0 {
                let at = &acts[g.t as usize];
                if !g.class.sat(at, rule) && at.failed() {
                    saved += 1.0;
                }
            }
            windows.push((g.t, ts));
        }
        let remaining_api = (n.saturating_sub(from)) as f64 * price.api_per_call;
        let cost = if superseded || price.usd_per_call <= 0.0 {
            0.0
        } else {
            g.tokens as f64 * price.cache_read / 1e6 * remaining_api / price.usd_per_call
        };
        out.push(Scored { given: g, t_star, useful, harmful, superseded, saved, cost });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Sessions, the report, and the statistics.
// ---------------------------------------------------------------------------------------------

/// What `evals/replay/manifest.py` writes: one repository's held-out sessions (and its training
/// sessions, for `--learn`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub name: String,
    pub repo_key: String,
    /// A checkout of the repository with its environment (`.venv`, `node_modules`): the replay's
    /// root, and what instincts bind against.
    pub root: PathBuf,
    /// The index delivered live, if any (`$ADAGE_HOME/index.json`).
    #[serde(default)]
    pub index: Option<PathBuf>,
    #[serde(default)]
    pub train: Vec<PathBuf>,
    pub sessions: Vec<SessionMeta>,
    #[serde(default)]
    pub cochange: Option<CoChange>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionMeta {
    pub id: String,
    pub task: String,
    #[serde(default)]
    pub arm: String,
    pub transcript: PathBuf,
    #[serde(default)]
    pub passed: Option<bool>,
    /// The harness's cost (`total_cost_usd`); the ledger's priced tokens when absent.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// The session ended with a Stop hook.
    #[serde(default)]
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionRow {
    pub id: String,
    pub task: String,
    pub arm: String,
    pub passed: Option<bool>,
    pub calls: usize,
    pub usd_per_call: f64,
    /// A test run in the session hit U11's dead end (a runner failing with an environment
    /// signature), as `evals/launch/deadends.py` counts it.
    pub dead_end: bool,
    pub directives: usize,
    pub useful: usize,
    pub harmful: usize,
    pub superseded: usize,
    pub saved: f64,
    pub cost: f64,
    /// Σ_t [S(d_t) − C(d_t)] in calls.
    pub net: f64,
    pub net_usd: f64,
    pub scored: Vec<Scored>,
}

static U11_SIG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"No module named pytest|pytest: command not found|command not found: pytest|No module named '(click|attr|attrs|marshmallow)'|python: command not found").unwrap()
});
static U11_TEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(pytest|py\.test|tox|nox)\b").unwrap());

pub fn session_price(path: &Path, cost_usd: Option<f64>, calls: usize) -> Price {
    let s = adage_learn::ledger::claude(path);
    let table = adage_core::pricing::table();
    let model = s.as_ref().and_then(|s| s.models.first().cloned()).unwrap_or_default();
    let rates = table.rates(&model);
    let usd = cost_usd.or_else(|| s.as_ref().and_then(|s| s.cost.usd)).unwrap_or(0.0);
    let turns = s.as_ref().map(|s| s.model_turns).unwrap_or(0) as f64;
    let calls_f = calls.max(1) as f64;
    Price {
        cache_read: rates.map(|r| r.cache_read).unwrap_or(0.0),
        usd_per_call: usd / calls_f,
        api_per_call: if turns > 0.0 { turns / calls_f } else { 1.0 },
    }
}

/// Replay and score one session.
pub fn session(policy: &Policy, meta: &SessionMeta, mode: Mode, rule: SatRule) -> Result<SessionRow> {
    let t = transcript::read(&meta.transcript)?;
    let acts: Vec<Act> = t.turns().flatten().map(|c| act(c, &t.cwd)).collect();
    let price = session_price(&meta.transcript, meta.cost_usd, acts.len());
    let given = directives(policy, &t, &acts, &meta.task, meta.stopped, mode);
    let scored = score(given, &acts, meta.passed, &price, rule);
    if std::env::var_os("ADAGE_REPLAY_EXPLAIN").is_some() {
        eprint!("{}", explain(&meta.id, &acts, &scored, rule));
    }
    Ok(row(meta, &acts, scored, &price))
}

/// Each directive's window, call by call (`ADAGE_REPLAY_EXPLAIN=1`): `S` Sat, `W` waste, `F` failed.
pub fn explain(id: &str, acts: &[Act], scored: &[Scored], rule: SatRule) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    for s in scored {
        let g = &s.given;
        let _ = writeln!(
            o,
            "{id} {} at t={} ({}{}) t*={:?} useful={} harmful={} superseded={} S={} C={:.4}",
            g.route,
            g.t,
            g.event,
            if g.pre { ", pre-emptive" } else { "" },
            s.t_star,
            s.useful,
            s.harmful,
            s.superseded,
            s.saved,
            s.cost
        );
        let lo = g.t.max(0) as usize;
        let hi = s.t_star.map(|x| x + 1).unwrap_or(lo + 12).min(acts.len()).min(lo + 25);
        for (k, a) in acts.iter().enumerate().take(hi).skip(lo) {
            let what = if a.cmd.is_empty() { a.edited.clone().unwrap_or_default() } else { a.cmd.replace('\n', " ") };
            let _ = writeln!(
                o,
                "   {k:>3} {}{}{} {:<5} {:?} {}",
                if g.class.sat(a, rule) { 'S' } else { '.' },
                if g.class.waste(a) { 'W' } else { '.' },
                if a.failed() { 'F' } else { '.' },
                a.tool,
                a.run.map(|r| format!("{}/{}{}", r.interp, r.runner, if a.executed { "+ran" } else { "" })),
                what.chars().take(110).collect::<String>()
            );
        }
    }
    o
}

pub fn row(meta: &SessionMeta, acts: &[Act], scored: Vec<Scored>, price: &Price) -> SessionRow {
    let live: Vec<&Scored> = scored.iter().filter(|s| !s.superseded).collect();
    let saved: f64 = live.iter().map(|s| s.saved).sum();
    let cost: f64 = live.iter().map(|s| s.cost).sum();
    SessionRow {
        id: meta.id.clone(),
        task: meta.task.clone(),
        arm: meta.arm.clone(),
        passed: meta.passed,
        calls: acts.len(),
        usd_per_call: price.usd_per_call,
        dead_end: acts.iter().any(|a| a.tool == "Bash" && U11_TEST.is_match(&a.cmd) && U11_SIG.is_match(&a.output)),
        directives: live.len(),
        useful: live.iter().filter(|s| s.useful).count(),
        harmful: live.iter().filter(|s| s.harmful).count(),
        superseded: scored.len() - live.len(),
        saved,
        cost,
        net: saved - cost,
        net_usd: (saved - cost) * price.usd_per_call,
        scored,
    }
}

/// A deterministic generator for the bootstrap (SplitMix64).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn r4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

/// The mean of `f` over sessions and its 95% percentile bootstrap CI, resampling tasks (every
/// session of a drawn task comes with it). 2000 draws, seeded.
pub fn clustered_mean(rows: &[SessionRow], f: impl Fn(&SessionRow) -> f64) -> (f64, [f64; 2]) {
    let (mean, boots) = clustered_boots(rows, f);
    if boots.is_empty() {
        return (mean, [0.0, 0.0]);
    }
    (mean, [r4(boots[49]), r4(boots[1949])])
}

/// [`clustered_mean`] with the 90% interval (the same draws).
pub fn clustered_mean90(rows: &[SessionRow], f: impl Fn(&SessionRow) -> f64) -> (f64, [f64; 2]) {
    let (mean, boots) = clustered_boots(rows, f);
    if boots.is_empty() {
        return (mean, [0.0, 0.0]);
    }
    (mean, [r4(boots[99]), r4(boots[1899])])
}

/// The mean and the sorted bootstrap means (2000 draws, seeded; empty for no rows).
fn clustered_boots(rows: &[SessionRow], f: impl Fn(&SessionRow) -> f64) -> (f64, Vec<f64>) {
    if rows.is_empty() {
        return (0.0, Vec::new());
    }
    let mut by: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for r in rows {
        by.entry(r.task.as_str()).or_default().push(f(r));
    }
    let tasks: Vec<&Vec<f64>> = by.values().collect();
    let mean = rows.iter().map(&f).sum::<f64>() / rows.len() as f64;
    let mut rng = Rng(0x5EED_0ADA_9E00_0001);
    let mut boots = Vec::with_capacity(2000);
    for _ in 0..2000 {
        let (mut s, mut n) = (0.0, 0usize);
        for _ in 0..tasks.len() {
            let v = tasks[rng.below(tasks.len())];
            s += v.iter().sum::<f64>();
            n += v.len();
        }
        boots.push(s / n.max(1) as f64);
    }
    boots.sort_by(|a, b| a.total_cmp(b));
    (r4(mean), boots)
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub sessions: usize,
    pub tasks: usize,
    pub calls: usize,
    pub directives: usize,
    pub useful: usize,
    pub harmful: usize,
    pub superseded: usize,
    /// Ŝ: mean over sessions of Σ_t [S − C], in calls, with its task-clustered 95% CI.
    pub s_hat: f64,
    pub s_hat_ci: [f64; 2],
    /// The same draws' 90% interval.
    pub s_hat_ci90: [f64; 2],
    /// Ŝ in dollars per session.
    pub s_hat_usd: f64,
    pub s_hat_usd_ci: [f64; 2],
    /// Mean Σ S and Σ C per session.
    pub saved: f64,
    pub cost: f64,
    pub useful_rate: f64,
    /// Wilson 90% lower bound of useful / emitted (the §5.2 gate: ≥ 0.8).
    pub useful_lb90: f64,
    /// The §5.2 gate: ≤ 1%.
    pub harmful_rate: f64,
    /// The §5.2 gate: ≤ 1.
    pub per_10_calls: f64,
    /// The same restricted to sessions that hit U11's dead end (the calibration population).
    pub dead_end_sessions: usize,
    pub s_hat_dead_end: f64,
    pub s_hat_dead_end_ci: [f64; 2],
}

pub fn summarise(rows: &[SessionRow]) -> Summary {
    let directives: usize = rows.iter().map(|r| r.directives).sum();
    let useful: usize = rows.iter().map(|r| r.useful).sum();
    let harmful: usize = rows.iter().map(|r| r.harmful).sum();
    let calls: usize = rows.iter().map(|r| r.calls).sum();
    let (s_hat, s_hat_ci) = clustered_mean(rows, |r| r.net);
    let (_, s_hat_ci90) = clustered_mean90(rows, |r| r.net);
    let (s_hat_usd, s_hat_usd_ci) = clustered_mean(rows, |r| r.net_usd);
    let de: Vec<SessionRow> = rows.iter().filter(|r| r.dead_end).cloned().collect();
    let (s_de, s_de_ci) = clustered_mean(&de, |r| r.net);
    let n = rows.len().max(1) as f64;
    let rate = |k: usize| if directives == 0 { 0.0 } else { r4(k as f64 / directives as f64) };
    Summary {
        sessions: rows.len(),
        tasks: rows.iter().map(|r| r.task.as_str()).collect::<BTreeSet<_>>().len(),
        calls,
        directives,
        useful,
        harmful,
        superseded: rows.iter().map(|r| r.superseded).sum(),
        s_hat,
        s_hat_ci,
        s_hat_ci90,
        s_hat_usd: r4(s_hat_usd * 100.0) / 100.0,
        s_hat_usd_ci: [r4(s_hat_usd_ci[0] * 100.0) / 100.0, r4(s_hat_usd_ci[1] * 100.0) / 100.0],
        saved: r4(rows.iter().map(|r| r.saved).sum::<f64>() / n),
        cost: r4(rows.iter().map(|r| r.cost).sum::<f64>() / n),
        useful_rate: rate(useful),
        useful_lb90: r4(adage_graph::stats::wilson_lower(useful as f64, directives as f64, 1.644_853_626_951_472_2)),
        harmful_rate: rate(harmful),
        per_10_calls: if calls == 0 { 0.0 } else { r4(10.0 * directives as f64 / calls as f64) },
        dead_end_sessions: de.len(),
        s_hat_dead_end: s_de,
        s_hat_dead_end_ci: s_de_ci,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub name: String,
    pub policy: String,
    pub mode: Mode,
    pub sat: SatRule,
    /// What embedded (`--models`), if anything.
    pub embedder: Option<String>,
    pub cases: CasesMode,
    /// The case store's proposals and the value model's verdicts.
    pub proposals: Vec<adage_learn::cases::Proposal>,
    pub routes: Vec<String>,
    pub summary: Summary,
    pub by_route: BTreeMap<String, RouteStats>,
    pub sessions: Vec<SessionRow>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RouteStats {
    pub directives: usize,
    pub useful: usize,
    pub harmful: usize,
    pub saved: f64,
    pub cost: f64,
}

fn by_route(rows: &[SessionRow]) -> BTreeMap<String, RouteStats> {
    let mut m: BTreeMap<String, RouteStats> = BTreeMap::new();
    for s in rows.iter().flat_map(|r| &r.scored).filter(|s| !s.superseded) {
        let e = m.entry(s.given.route.clone()).or_default();
        e.directives += 1;
        e.useful += s.useful as usize;
        e.harmful += s.harmful as usize;
        e.saved = r4(e.saved + s.saved);
        e.cost = r4(e.cost + s.cost);
    }
    m
}

// ---------------------------------------------------------------------------------------------
// Indexes: bound, instincts only, learned + instincts; filtered to some instincts.
// ---------------------------------------------------------------------------------------------

fn instincts_cfg(repo: &str, root: &Path) -> adage_learn::instincts::Config {
    let mut c = adage_learn::instincts::Config::default();
    c.roots.insert(repo.to_string(), root.to_path_buf());
    c
}

/// Learn from the training transcripts at `thresholds`, then bind instincts against `root`.
pub fn learned_index(repo: &str, root: &Path, train: &[PathBuf], thresholds: &str) -> Result<Index> {
    learned_index_with(repo, root, train, thresholds, None)
}

/// [`learned_index`], learning intent families with the model `models` resolves to (installed,
/// bundled, or none: `adage_intent::model::resolve`).
pub fn learned_index_with(
    repo: &str,
    root: &Path,
    train: &[PathBuf],
    thresholds: &str,
    models: Option<&Path>,
) -> Result<Index> {
    let home = tempfile::tempdir()?;
    let mut cfg = crate::config(thresholds)?;
    cfg.instincts = Some(instincts_cfg(repo, root));
    let inputs = adage_learn::Inputs {
        identities: Default::default(),
        transcripts: train.to_vec(),
        outcomes: BTreeMap::new(),
        repo_override: Some(repo.to_string()),
        models: models.map(Path::to_path_buf),
        cache: None,
        only_repo: None,
    };
    let index_path = home.path().join("index.json");
    adage_learn::learn(&inputs, &home.path().join("tracks"), &index_path, &cfg)?;
    Ok(Index::load(&index_path))
}

/// Keep only the routes of these instincts (`U11`, …); the derived trigger tables are dropped so
/// the index derives them from what is left.
pub fn only_instincts(mut ix: Index, ids: &BTreeSet<String>) -> Index {
    ix.routes.retain(|r| matches!(r.route.origin(), Origin::Instinct { instinct, .. } if ids.contains(&instinct)));
    ix.triggers.clear();
    ix.contexts.clear();
    ix
}

pub struct Options {
    pub policy: String,
    pub mode: Mode,
    pub sat: SatRule,
    pub index: Option<PathBuf>,
    pub only: Option<BTreeSet<String>>,
    pub arm: Option<String>,
    pub thresholds: String,
    /// A models directory: intent families are learned and matched with the model it resolves to
    /// (`ADAGE_EMBED_MODEL` selects), and the case store embeds with it. `None`: no embedding, as
    /// the replay always did.
    pub models: Option<PathBuf>,
    pub cases: CasesMode,
}

/// Whether the case store takes part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CasesMode {
    #[default]
    Off,
    /// Proposals the value model admitted.
    On,
    /// Every proposal (a diagnostic).
    Ungated,
}

/// A repository's case store, built from the manifest's training transcripts.
pub fn case_store(
    m: &Manifest,
    e: &adage_intent::model::Embedder,
    thresholds: &str,
) -> Result<adage_learn::cases::CaseStore> {
    let table = CommandTable::load();
    let sessions: Vec<adage_learn::import::Session> =
        m.train.iter().filter_map(|p| adage_learn::import::parse_transcript(p, table, Some(&m.repo_key))).collect();
    let refs: Vec<&adage_learn::import::Session> = sessions.iter().collect();
    let cfg = crate::config(thresholds)?;
    let env_kind = adage_learn::value::Statics::of(Some(&m.root)).env_kind;
    Ok(adage_learn::cases::build(&m.repo_key, &refs, e, &cfg.priors, &env_kind))
}

pub fn run(m: &Manifest, o: &Options) -> Result<Report> {
    let table = CommandTable::load();
    let index = match o.policy.as_str() {
        "cochange" => Index::default(),
        "index" => {
            let p = o
                .index
                .clone()
                .or_else(|| m.index.clone())
                .context("--policy index needs --index or a manifest index")?;
            serde_json::from_slice::<Index>(&std::fs::read(&p).with_context(|| p.display().to_string())?)?
        }
        "instincts" => crate::instincts_index(&m.repo_key, &m.root)?,
        "learned" => learned_index_with(&m.repo_key, &m.root, &m.train, &o.thresholds, o.models.as_deref())?,
        other => bail!("unknown policy {other:?} (index, instincts, learned, cochange)"),
    };
    let index = match &o.only {
        Some(ids) => only_instincts(index, ids),
        None => index,
    };
    let empty = CoChange::default();
    // The embedder (with a models directory): the model intent routes were learned with, and
    // what the case store embeds with.
    let resolved = o.models.as_deref().map(|d| adage_intent::model::resolve(d, true));
    let embed_fn = |text: &str| -> Option<(String, Vec<f32>)> {
        let m = resolved.as_ref()?.embedder.model()?;
        Some((m.id.clone(), m.embed(&adage_intent::normalize::normalize(text))?))
    };
    let store = match (&resolved, o.cases) {
        (Some(r), CasesMode::On | CasesMode::Ungated) => Some(case_store(m, &r.embedder, &o.thresholds)?),
        (None, CasesMode::On | CasesMode::Ungated) => bail!("--cases needs --models <dir>"),
        _ => None,
    };
    let policy = if o.policy == "cochange" {
        Policy::CoChange(m.cochange.as_ref().unwrap_or(&empty))
    } else {
        Policy::Runtime {
            index: &index,
            repo: &m.repo_key,
            root: &m.root,
            table,
            params: adage_tracks::params::EngineParams::preset(&o.thresholds)?.runtime,
            embed: resolved.as_ref().map(|_| &embed_fn as adage_runtime::Embed),
            cases: match (&store, &resolved) {
                (Some(st), Some(r)) => {
                    Some(CaseCtx { store: st, embedder: &r.embedder, ungated: o.cases == CasesMode::Ungated })
                }
                _ => None,
            },
        }
    };
    let routes = index
        .for_repo(&m.repo_key)
        .map(|r| {
            format!(
                "{} ({})",
                r.route.id,
                match r.route.origin() {
                    Origin::Instinct { instinct, .. } => instinct,
                    _ => "learned".into(),
                }
            )
        })
        .collect();
    let mut rows = Vec::new();
    let mut sessions: Vec<&SessionMeta> =
        m.sessions.iter().filter(|s| o.arm.as_ref().is_none_or(|a| &s.arm == a)).collect();
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    for s in sessions {
        rows.push(session(&policy, s, o.mode, o.sat).with_context(|| s.id.clone())?);
    }
    Ok(Report {
        name: m.name.clone(),
        policy: o.policy.clone(),
        mode: o.mode,
        sat: o.sat,
        embedder: resolved.as_ref().map(|r| r.embedder.id()),
        cases: o.cases,
        proposals: store.map(|s| s.proposals).unwrap_or_default(),
        routes,
        summary: summarise(&rows),
        by_route: by_route(&rows),
        sessions: rows,
    })
}

pub fn render_text(r: &Report) -> String {
    use std::fmt::Write;
    let s = &r.summary;
    let mut o = String::new();
    let _ = writeln!(
        o,
        "replay-savings: {} · policy {} · {} · Sat {}",
        r.name,
        r.policy,
        match r.mode {
            Mode::PostHoc => "post-hoc",
            Mode::PreEmptive => "pre-emptive",
        },
        match r.sat {
            SatRule::Strict => "strict",
            SatRule::Goal => "goal",
        }
    );
    let _ = writeln!(o, "  {} sessions, {} tasks, {} calls; {} routes", s.sessions, s.tasks, s.calls, r.routes.len());
    if let Some(e) = &r.embedder {
        let admitted = r.proposals.iter().filter(|p| p.admitted).count();
        let _ = writeln!(
            o,
            "  embedder {e}; case store {:?}: {} proposals, {admitted} admitted",
            r.cases,
            r.proposals.len()
        );
    }
    let _ = writeln!(
        o,
        "  Ŝ = {:+.3} calls/session [95% CI {:+.3}, {:+.3}; 90% {:+.3}, {:+.3}] = ${:+.4}/session  (S {:.3}, C {:.4})",
        s.s_hat, s.s_hat_ci[0], s.s_hat_ci[1], s.s_hat_ci90[0], s.s_hat_ci90[1], s.s_hat_usd, s.saved, s.cost
    );
    let _ = writeln!(
        o,
        "  directives {} ({} superseded), useful {} ({:.1}%, Wilson90 LB {:.3}), harmful {} ({:.1}%), {:.3} per 10 calls",
        s.directives,
        s.superseded,
        s.useful,
        s.useful_rate * 100.0,
        s.useful_lb90,
        s.harmful,
        s.harmful_rate * 100.0,
        s.per_10_calls
    );
    let _ = writeln!(
        o,
        "  sessions that hit U11's dead end: {}, Ŝ there {:+.3} [95% CI {:+.3}, {:+.3}]",
        s.dead_end_sessions, s.s_hat_dead_end, s.s_hat_dead_end_ci[0], s.s_hat_dead_end_ci[1]
    );
    for (k, v) in &r.by_route {
        let _ = writeln!(
            o,
            "    {k:<20} {:>3} given, {:>3} useful, {:>2} harmful, saved {:.1}, cost {:.3}",
            v.directives, v.useful, v.harmful, v.saved, v.cost
        );
    }
    o
}

const USAGE: &str = "usage: adage eval replay-savings <manifest.json> [--policy index|instincts|learned|cochange] [--index <index.json>] [--only U11,…] [--mode post-hoc|pre-emptive] [--sat goal|strict] [--arm <arm>] [--thresholds default|demo] [--models <dir>] [--cases off|on|ungated] [--json]";

/// `adage eval replay-savings …`
pub fn cli(args: &[String]) -> Result<()> {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1).cloned());
    let Some(path) = args.first().filter(|a| !a.starts_with("--")) else {
        println!("{USAGE}");
        return Ok(());
    };
    let m: Manifest = serde_json::from_slice(&std::fs::read(path).with_context(|| path.clone())?)
        .with_context(|| format!("{path}: not a replay manifest"))?;
    let o = Options {
        policy: flag("--policy").unwrap_or_else(|| "index".into()),
        mode: match flag("--mode").as_deref() {
            None | Some("post-hoc") => Mode::PostHoc,
            Some("pre-emptive") => Mode::PreEmptive,
            Some(x) => bail!("--mode {x:?}: post-hoc or pre-emptive"),
        },
        sat: match flag("--sat").as_deref() {
            None | Some("goal") => SatRule::Goal,
            Some("strict") => SatRule::Strict,
            Some(x) => bail!("--sat {x:?}: strict or goal"),
        },
        index: flag("--index").map(PathBuf::from),
        only: flag("--only").map(|s| s.split(',').map(|x| x.trim().to_uppercase()).collect()),
        arm: flag("--arm"),
        thresholds: flag("--thresholds").unwrap_or_else(|| "default".into()),
        models: flag("--models").map(PathBuf::from),
        cases: match flag("--cases").as_deref() {
            None | Some("off") => CasesMode::Off,
            Some("on") => CasesMode::On,
            Some("ungated") => CasesMode::Ungated,
            Some(x) => bail!("--cases {x:?}: off, on or ungated"),
        },
    };
    let r = run(&m, &o)?;
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        print!("{}", render_text(&r));
    }
    Ok(())
}

#[cfg(test)]
#[path = "savings_replay_tests.rs"]
mod tests;
