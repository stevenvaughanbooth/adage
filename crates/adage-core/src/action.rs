//! Actions: what a command *does*, as a typed label (evals/lab/abstraction/README.md §4.1).
//!
//! Agents rarely repeat a command string (74% of test runs are distinct), but they repeat what the
//! command does: run the project's tests of what they edited, reproduce a bug with a scratch
//! script, install a package. [`classify`] reads the argv segments of one command line (the parts
//! of a compound, in order: `source .venv/bin/activate && python -m pytest tests/x.py -q | tail`)
//! into an [`Action`]: its **role** (test run, repro, environment setup, …), its **environment**
//! (the project's `.venv`, `uv run`, an npm script, or whatever the shell finds first), the
//! **runner** (pytest, vitest, go test, …), its **targets** (files, test ids, directories) and
//! **selectors** (`-k`, `-t`, `-run`), and whether its output was filtered.
//!
//! - The environment's **class** ([`EnvClass`]) is what a dead end turns on: a system-environment
//!   test run hit `No module named …` 34% of the time in the RepoBench traces, a project-environment
//!   one never. It is an attribute of the action, not part of what the agent meant to do.
//! - The **intent** ([`Action::intent`]) is role · scope class, environment-free: `test-run|narrow`.
//! - [`satisfies`] is the follow judgement for a directed action: the same role and runner, a
//!   project environment where one was directed, and targets that overlap or cover the directed
//!   ones (arguments free otherwise). `.venv/bin/python -m pytest tests/test_options.py::test_x
//!   -xvs | head` satisfies `.venv/bin/python -m pytest tests/test_options.py`.
//!
//! Everything is table lookups over tokens: no regex is compiled, so the hook can call it.

use serde::{Deserialize, Serialize};

/// What a command is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    TestRun,
    /// Running code to see what it does: an inline program, a scratch script.
    Repro,
    Build,
    Lint,
    Codegen,
    /// Installing or syncing packages.
    EnvSetup,
    VcsRead,
    VcsWrite,
    Read,
    Search,
    Trivial,
    Other,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::TestRun => "test-run",
            Role::Repro => "repro",
            Role::Build => "build",
            Role::Lint => "lint",
            Role::Codegen => "codegen",
            Role::EnvSetup => "env-setup",
            Role::VcsRead => "vcs-read",
            Role::VcsWrite => "vcs-write",
            Role::Read => "read",
            Role::Search => "search",
            Role::Trivial => "trivial",
            Role::Other => "other",
        }
    }
}

/// Where a command runs its tools.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "dir", rename_all = "kebab-case")]
pub enum Env {
    /// The project's virtual environment (its directory: `.venv`), by path or activated.
    Venv(String),
    /// `uv run`, `poetry run`, `pdm run`, `hatch run`, `tox`, `nox`: the project's managed
    /// environment (the tool's name).
    Managed(String),
    /// `npx`, `pnpm exec`, `yarn`, `bunx`, `node_modules/.bin/…`: the project's node tools.
    NodeLocal,
    /// A package script (`npm test`, `npm run build`).
    Script,
    /// A toolchain that brings its own environment (`go`, `cargo`).
    Toolchain,
    /// A tool installed into the user site (`~/.local/bin/pytest`).
    UserSite,
    /// Whatever the shell finds first.
    System,
    /// Not a program that needs an environment (`git`, `ls`).
    Shell,
}

/// The class of an environment: what a dead end turns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnvClass {
    Project,
    System,
    NotApplicable,
}

impl Env {
    pub fn class(&self) -> EnvClass {
        match self {
            Env::Venv(_) | Env::Managed(_) | Env::NodeLocal | Env::Script | Env::Toolchain => EnvClass::Project,
            Env::UserSite | Env::System => EnvClass::System,
            Env::Shell => EnvClass::NotApplicable,
        }
    }

    /// A short name (`venv`, `managed`, `system`, …).
    pub fn kind(&self) -> &'static str {
        match self {
            Env::Venv(_) => "venv",
            Env::Managed(_) => "managed",
            Env::NodeLocal => "node-local",
            Env::Script => "script",
            Env::Toolchain => "toolchain",
            Env::UserSite => "user-site",
            Env::System => "system",
            Env::Shell => "shell",
        }
    }
}

/// What a test run or a repro selects.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum Target {
    /// A file (`tests/test_core.py`, `packages/a/__tests__/x.spec.ts`).
    File(String),
    /// A directory or package (`tests`, `./...`, `packages/runtime-core`).
    Dir(String),
    /// One test of a file (`tests/test_core.py::test_x`): the file and the test.
    TestId(String, String),
}

impl Target {
    /// The path it names.
    pub fn path(&self) -> &str {
        match self {
            Target::File(p) | Target::Dir(p) | Target::TestId(p, _) => p,
        }
    }
}

/// How much of the repository an action covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Everything (`pytest`, `go test ./...`, `npm test`).
    Repo,
    /// A directory or package.
    Package,
    File,
    /// One test, or tests selected by name.
    TestId,
    /// Code given inline (`python -c`, a heredoc).
    Inline,
    /// A script outside the repository's own tree, or at its root.
    ScratchFile,
    /// A script of the repository's.
    RepoFile,
    None,
}

/// A classified command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub role: Role,
    pub env: Env,
    /// The tool (`pytest`, `unittest`, `vitest`, `jest`, `go test`, `cargo test`, `npm test`,
    /// `python`, `node`, `pip`, …); empty when none applies.
    pub runner: String,
    pub scope: Scope,
    pub targets: Vec<Target>,
    pub selectors: Vec<String>,
    /// The output went through a filter (`| head`, `| tail`, `| grep`).
    pub piped: bool,
    /// The argv of the significant segment (the one the role was read from).
    pub argv: Vec<String>,
}

impl Action {
    /// Role · scope class, environment-free (`test-run|narrow`, `test-run|all`, `repro|scratch`).
    pub fn intent(&self) -> String {
        let scope = match (self.role, self.scope) {
            (Role::TestRun, Scope::Repo) => "all",
            (Role::TestRun, _) => "narrow",
            (Role::Repro, Scope::RepoFile) => "repo-file",
            (Role::Repro, _) => "scratch",
            _ => "",
        };
        if scope.is_empty() {
            self.role.as_str().to_string()
        } else {
            format!("{}|{scope}", self.role.as_str())
        }
    }
}

fn base(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// `python`, `python3`, `python3.12`.
pub fn is_python(p: &str) -> bool {
    let b = base(p);
    b == "python"
        || b.strip_prefix("python").is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// The virtual environment a program path lies in (`.venv/bin/python` → `.venv`): a relative
/// directory inside the repository, never the user site.
pub fn venv_of(p: &str) -> Option<String> {
    let (dir, _) = p.rsplit_once("/bin/")?;
    let dir = dir.strip_prefix("./").unwrap_or(dir);
    (!dir.is_empty()
        && !dir.starts_with('/')
        && !dir.starts_with('~')
        && !dir.contains("..")
        && !dir.contains(".local"))
    .then(|| dir.to_string())
}

/// Commands that only wrap another (`timeout 60 pytest`, `env A=1 pytest`, `time pytest`).
const WRAPPERS: &[&str] = &["timeout", "env", "time", "nice", "nohup", "stdbuf", "xargs"];
/// Programs that filter another's output in a pipeline.
const FILTERS: &[&str] = &[
    "head", "tail", "grep", "egrep", "rg", "sort", "uniq", "wc", "cut", "less", "more", "tee", "awk", "sed", "tr",
    "column", "cat",
];
const READERS: &[&str] = &[
    "cat", "head", "tail", "less", "more", "nl", "bat", "sed", "awk", "wc", "diff", "cmp", "file", "stat", "od", "xxd",
    "jq",
];
const SEARCHERS: &[&str] = &["grep", "egrep", "fgrep", "rg", "ag", "ack", "find", "fd", "ls", "tree", "locate", "du"];
const TRIVIAL: &[&str] = &[
    "echo", "printf", "pwd", "true", "false", "sleep", "date", "whoami", "clear", "which", "type", "command", "cd",
    "export", "set", "unset", "mkdir", "touch", "rm", "cp", "mv", "chmod",
];
const LINTERS: &[&str] = &[
    "ruff",
    "flake8",
    "pylint",
    "mypy",
    "pyright",
    "black",
    "isort",
    "eslint",
    "prettier",
    "tsc",
    "biome",
    "golangci-lint",
    "gofmt",
    "shellcheck",
];
const PY_TOOLS: &[&str] = &[
    "pytest", "py.test", "tox", "nox", "pip", "pip3", "ruff", "mypy", "black", "flake8", "isort", "pyright", "coverage",
];

/// Leading environment assignments (`PYTHONPATH=src`) and wrappers skipped: the index of the
/// program.
fn program_at(av: &[String]) -> usize {
    let mut i = 0;
    loop {
        while i < av.len() && av[i].contains('=') && !av[i].starts_with('-') && !av[i].starts_with('/') {
            i += 1;
        }
        match av.get(i).map(|p| base(p)) {
            Some(w) if WRAPPERS.contains(&w) => {
                i += 1;
                // `timeout 60`, `nice -n 5`: the wrapper's own options and a numeric argument.
                while i < av.len()
                    && (av[i].starts_with('-')
                        || av[i].chars().all(|c| c.is_ascii_digit() || c == '.' || c == 's' || c == 'm'))
                {
                    i += 1;
                }
            }
            _ => return i,
        }
    }
}

/// The test runner and where its operands start, for a segment that runs tests.
fn test_runner(av: &[String], at: usize) -> Option<(String, usize)> {
    let prog = av.get(at)?;
    let b = base(prog);
    let word = |k: usize| av.get(k).map(String::as_str).unwrap_or("");
    let module = |m: &str| match m {
        "pytest" | "py.test" => Some("pytest"),
        "unittest" => Some("unittest"),
        "tox" => Some("tox"),
        "nox" => Some("nox"),
        _ => None,
    };
    if is_python(prog) {
        let k = av.iter().skip(at + 1).position(|t| t == "-m")?;
        let m = at + 2 + k;
        return module(word(m)).map(|r| (r.to_string(), m + 1));
    }
    let r = match b {
        "pytest" | "py.test" => "pytest",
        "tox" => "tox",
        "nox" => "nox",
        "vitest" => "vitest",
        "jest" => "jest",
        "mocha" => "mocha",
        "go" if word(at + 1) == "test" => "go test",
        "cargo" if word(at + 1) == "test" => "cargo test",
        "cargo" if word(at + 1) == "nextest" => "cargo nextest",
        "node" if av.iter().any(|t| t == "--test") => "node --test",
        "npm" | "pnpm" | "yarn" | "bun" => {
            let (w1, w2) = (word(at + 1), word(at + 2));
            return match (w1, w2) {
                ("test" | "t", _) => Some(("npm test".into(), at + 2)),
                ("run" | "run-script", "test") => Some(("npm test".into(), at + 3)),
                ("exec" | "dlx" | "x", "vitest" | "jest") => Some((w2.to_string(), at + 3)),
                ("vitest" | "jest", _) => Some((w1.to_string(), at + 2)),
                _ => None,
            };
        }
        "npx" | "bunx" => {
            return matches!(word(at + 1), "vitest" | "jest" | "mocha").then(|| (word(at + 1).to_string(), at + 2));
        }
        "uv" | "poetry" | "pdm" | "hatch" if word(at + 1) == "run" => {
            let inner = program_at(&av[at + 2..]) + at + 2;
            return test_runner(av, inner);
        }
        _ => return None,
    };
    let from = match r {
        "go test" | "cargo test" | "cargo nextest" => at + 2,
        _ => at + 1,
    };
    Some((r.to_string(), from))
}

/// Flags whose value is a selector, per runner family.
const SELECTOR_FLAGS: &[&str] = &["-k", "-t", "--testNamePattern", "-run", "--grep", "-g", "--filter", "--exact"];
/// Flags that take a value that is not a target.
const VALUE_FLAGS: &[&str] = &[
    "--tb",
    "-p",
    "-m",
    "-n",
    "--maxfail",
    "-W",
    "-c",
    "--rootdir",
    "--timeout",
    "--reporter",
    "--config",
    "-C",
    "--durations",
    "-o",
    "--junitxml",
    "--cov",
    "-count",
    "-timeout",
    "-tags",
    "--project",
    "--dir",
    "--cwd",
];

/// Targets and selectors of a test run's operands.
fn targets(av: &[String], from: usize) -> (Vec<Target>, Vec<String>) {
    let (mut t, mut s) = (Vec::new(), Vec::new());
    let mut i = from;
    while i < av.len() {
        let a = av[i].as_str();
        if a == "--" {
            i += 1;
            continue;
        }
        if a.starts_with('-') {
            let (flag, inline) = a.split_once('=').map_or((a, None), |(f, v)| (f, Some(v)));
            if SELECTOR_FLAGS.contains(&flag) {
                match inline {
                    Some(v) => s.push(v.to_string()),
                    None => {
                        if let Some(v) = av.get(i + 1) {
                            s.push(v.clone());
                        }
                        i += 1;
                    }
                }
            } else if inline.is_none() && VALUE_FLAGS.contains(&flag) {
                i += 1;
            }
            i += 1;
            continue;
        }
        let a = a.trim_end_matches('/');
        if let Some((file, test)) = a.split_once("::") {
            t.push(Target::TestId(file.to_string(), test.to_string()));
        } else if a == "." || a == "./..." || a.ends_with("/...") || !base(a).contains('.') {
            t.push(Target::Dir(a.to_string()));
        } else {
            t.push(Target::File(a.to_string()));
        }
        i += 1;
    }
    (t, s)
}

/// The environment a program runs in, with an activation earlier in the line.
fn env_of(prog: &str, activated: Option<&String>) -> Env {
    let b = base(prog);
    if let Some(d) = venv_of(prog) {
        return if prog.contains("node_modules/") { Env::NodeLocal } else { Env::Venv(d) };
    }
    if prog.contains(".local/bin/") {
        return Env::UserSite;
    }
    match b {
        "uv" | "poetry" | "pdm" | "hatch" | "tox" | "nox" => Env::Managed(b.to_string()),
        "npx" | "bunx" | "pnpm" | "yarn" => Env::NodeLocal,
        "npm" | "bun" => Env::Script,
        "go" | "cargo" | "rustc" => Env::Toolchain,
        _ if is_python(prog) || PY_TOOLS.contains(&b) => match activated {
            Some(d) => Env::Venv(d.clone()),
            None => Env::System,
        },
        "node" | "vitest" | "jest" | "mocha" | "tsc" | "eslint" => Env::System,
        _ => Env::Shell,
    }
}

fn install(av: &[String], at: usize) -> bool {
    let b = base(&av[at]);
    let word = |k: usize| av.get(k).map(String::as_str).unwrap_or("");
    let pip = matches!(b, "pip" | "pip3") || b.starts_with("pip3.");
    let py_pip =
        is_python(&av[at]) && av.iter().skip(at + 1).position(|t| t == "-m").is_some_and(|k| word(at + 2 + k) == "pip");
    (pip || py_pip) && av.iter().any(|t| t == "install" || t == "uninstall")
        || (matches!(b, "npm" | "pnpm" | "yarn" | "bun") && matches!(word(at + 1), "install" | "i" | "ci" | "add"))
        || (b == "uv" && matches!(word(at + 1), "add" | "sync" | "pip"))
        || (b == "poetry" && matches!(word(at + 1), "install" | "add"))
        || (b == "go" && matches!(word(at + 1), "get" | "install")
            || b == "go" && word(at + 1) == "mod" && word(at + 2) == "download")
        || (b == "cargo" && matches!(word(at + 1), "install" | "add" | "fetch"))
        || matches!(b, "apt" | "apt-get" | "brew" | "conda")
}

/// Rank of a role when a line has several (`pip install pytest && python -m pytest` is a test run).
fn significance(r: Role) -> u8 {
    match r {
        Role::TestRun => 12,
        Role::Repro => 11,
        Role::Codegen => 10,
        Role::Build => 9,
        Role::Lint => 8,
        Role::EnvSetup => 7,
        Role::VcsWrite => 6,
        Role::VcsRead => 5,
        Role::Search => 4,
        Role::Read => 3,
        Role::Other => 2,
        Role::Trivial => 1,
    }
}

/// Classify one segment (an argv), given the virtual environment an earlier `source …/activate`
/// in the line activated.
fn segment(av: &[String], activated: Option<&String>) -> Option<Action> {
    let at = program_at(av);
    let prog = av.get(at)?;
    let b = base(prog);
    let word = |k: usize| av.get(k).map(String::as_str).unwrap_or("");
    let env = env_of(prog, activated);
    let action = |role: Role, runner: &str, scope: Scope, t: Vec<Target>, s: Vec<String>| Action {
        role,
        env: env.clone(),
        runner: runner.to_string(),
        scope,
        targets: t,
        selectors: s,
        piped: false,
        argv: av.to_vec(),
    };
    if let Some((runner, mut from)) = test_runner(av, at) {
        // vitest's own subcommands (`vitest run x`) are no target.
        if runner == "vitest" && matches!(word(from), "run" | "watch" | "related" | "bench") {
            from += 1;
        }
        let (t, s) = targets(av, from);
        let scope = if t.iter().any(|x| matches!(x, Target::TestId(..))) || !s.is_empty() {
            Scope::TestId
        } else if t.iter().any(|x| matches!(x, Target::File(_))) {
            Scope::File
        } else if t.iter().any(|x| matches!(x, Target::Dir(d) if d != "." && d != "./...")) {
            Scope::Package
        } else {
            Scope::Repo
        };
        return Some(action(Role::TestRun, &runner, scope, t, s));
    }
    if install(av, at) {
        return Some(action(Role::EnvSetup, b, Scope::None, vec![], vec![]));
    }
    if is_python(prog) || matches!(b, "node" | "deno" | "ruby" | "perl" | "tsx" | "ts-node") {
        let rest = &av[at + 1..];
        let code = rest.iter().any(|t| t == "-c" || t == "-e" || t == "--eval") || rest.is_empty();
        if rest.iter().any(|t| t == "-m") {
            let m = rest.iter().skip_while(|t| *t != "-m").nth(1).map(String::as_str).unwrap_or("");
            let role = match m {
                "pip" => Role::EnvSetup,
                "mypy" | "ruff" | "flake8" | "pylint" | "black" | "isort" => Role::Lint,
                "build" | "compileall" => Role::Build,
                "venv" | "virtualenv" => Role::EnvSetup,
                _ => Role::Repro,
            };
            return Some(action(role, m, Scope::RepoFile, vec![], vec![]));
        }
        let script = rest.iter().find(|t| !t.starts_with('-'));
        let scope = match (code, script) {
            (true, _) | (false, None) => Scope::Inline,
            (false, Some(s)) if s.starts_with('/') || !s.contains('/') => Scope::ScratchFile,
            _ => Scope::RepoFile,
        };
        let runner = if is_python(prog) { "python" } else { b };
        let t = script.filter(|_| !code).map(|s| vec![Target::File(s.clone())]).unwrap_or_default();
        return Some(action(Role::Repro, runner, scope, t, vec![]));
    }
    let role = match b {
        "git" | "gh" => match word(at + 1) {
            "status" | "diff" | "log" | "show" | "blame" | "branch" | "rev-parse" | "ls-files" | "grep" | "remote"
            | "describe" | "shortlog" | "reflog" => Role::VcsRead,
            _ => Role::VcsWrite,
        },
        "make" | "gmake" | "just" | "task" => match word(at + 1) {
            w if w.contains("test") || w == "check" => Role::TestRun,
            w if w.contains("gen") => Role::Codegen,
            w if w.contains("lint") || w.contains("fmt") || w.contains("format") => Role::Lint,
            _ => Role::Build,
        },
        "cargo" => match word(at + 1) {
            "clippy" | "fmt" => Role::Lint,
            "build" | "check" | "doc" => Role::Build,
            _ => Role::Other,
        },
        "go" => match word(at + 1) {
            "build" | "install" => Role::Build,
            "vet" | "fmt" => Role::Lint,
            "generate" => Role::Codegen,
            _ => Role::Other,
        },
        "npm" | "pnpm" | "yarn" | "bun" if matches!(word(at + 1), "run" | "run-script") => match word(at + 2) {
            w if w.contains("lint") || w.contains("format") || w.contains("check") || w.contains("type") => Role::Lint,
            w if w.contains("gen") => Role::Codegen,
            _ => Role::Build,
        },
        _ if LINTERS.contains(&b) => Role::Lint,
        _ if READERS.contains(&b) => Role::Read,
        _ if SEARCHERS.contains(&b) => Role::Search,
        _ if TRIVIAL.contains(&b) => Role::Trivial,
        _ => Role::Other,
    };
    Some(action(role, b, Scope::None, vec![], vec![]))
}

/// Classify a command line from its argv segments in order (the parts of a compound: `&&`, `;`,
/// `|`). An activation (`source .venv/bin/activate`, `. venv/bin/activate`) puts the segments after
/// it in that environment; the most significant segment gives the action; a filter after it
/// (`| tail -20`) marks it piped.
pub fn classify(segments: &[&[String]]) -> Action {
    let mut activated: Option<String> = None;
    let mut best: Option<(u8, usize, Action)> = None;
    for (i, av) in segments.iter().enumerate() {
        let at = program_at(av);
        let Some(prog) = av.get(at) else { continue };
        let b = base(prog);
        if (b == "source" || b == ".") && av.get(at + 1).is_some_and(|p| p.ends_with("/bin/activate")) {
            activated = av.get(at + 1).and_then(|p| venv_of(p));
            continue;
        }
        let Some(a) = segment(av, activated.as_ref()) else { continue };
        let rank = significance(a.role);
        if best.as_ref().is_none_or(|(r, _, _)| rank > *r) {
            best = Some((rank, i, a));
        }
    }
    let Some((_, at, mut a)) = best else {
        return Action {
            role: Role::Trivial,
            env: Env::Shell,
            runner: String::new(),
            scope: Scope::None,
            targets: vec![],
            selectors: vec![],
            piped: false,
            argv: vec![],
        };
    };
    a.piped = segments.iter().skip(at + 1).any(|av| {
        let p = program_at(av);
        av.get(p).is_some_and(|x| FILTERS.contains(&base(x)))
    });
    a
}

/// Classify one argv (a command with no compound parts).
pub fn classify_argv(argv: &[String]) -> Action {
    classify(&[argv])
}

/// Whether target `w` (directed) is covered by, or overlaps, an observed target set.
fn target_met(w: &Target, observed: &[Target]) -> bool {
    let wp = w.path().trim_end_matches('/');
    observed.iter().any(|o| {
        let op = o.path().trim_end_matches('/');
        // The same file (any test of it), a directory holding it, or a file under the directed one.
        op == wp
            || wp.strip_prefix(op).is_some_and(|r| r.starts_with('/'))
            || op.strip_prefix(wp).is_some_and(|r| r.starts_with('/'))
            || op == "."
    })
}

/// Whether an observed action satisfies a directed one (the follow judgement, `Sat(d)`): the same
/// role and runner; where the directed environment is a project one, the observed one is the
/// project's too; and, where the directed action names targets, the observed run covers or
/// overlaps them (a run of the whole suite covers everything). Arguments are free otherwise.
pub fn satisfies(directed: &Action, observed: &Action) -> bool {
    if directed.role != observed.role || directed.runner != observed.runner {
        return false;
    }
    if directed.env.class() == EnvClass::Project && observed.env.class() != EnvClass::Project {
        return false;
    }
    if directed.targets.is_empty() || observed.scope == Scope::Repo {
        return true;
    }
    directed.targets.iter().any(|t| target_met(t, &observed.targets))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn line(parts: &[&str]) -> Action {
        let v: Vec<Vec<String>> = parts.iter().map(|p| seg(p)).collect();
        let r: Vec<&[String]> = v.iter().map(|x| x.as_slice()).collect();
        classify(&r)
    }

    #[test]
    fn test_runs_are_read_with_their_environment_and_targets() {
        let a = line(&[".venv/bin/python -m pytest tests/test_options.py::test_x -xvs", "head -50"]);
        assert_eq!(
            (a.role, a.runner.as_str(), a.env.class(), a.scope),
            (Role::TestRun, "pytest", EnvClass::Project, Scope::TestId)
        );
        assert_eq!(a.targets, vec![Target::TestId("tests/test_options.py".into(), "test_x".into())]);
        assert!(a.piped);
        assert_eq!(a.intent(), "test-run|narrow");

        let a = line(&["source .venv/bin/activate", "python -m pytest tests/test_options.py -q"]);
        assert_eq!((a.env.clone(), a.scope), (Env::Venv(".venv".into()), Scope::File));

        let a = line(&["python3 -m pytest tests/test_basic.py::test_group -xvs"]);
        assert_eq!(a.env, Env::System);
        let a = line(&["python3 -W error -m pytest"]);
        assert_eq!((a.role, a.scope), (Role::TestRun, Scope::Repo));
        let a = line(&["uv run pytest -k parser"]);
        assert_eq!(
            (a.env.class(), a.scope, a.selectors.clone()),
            (EnvClass::Project, Scope::TestId, vec!["parser".to_string()])
        );
        let a = line(&["npm test -- packages/runtime-core/__tests__/renderer.spec.ts --run"]);
        assert_eq!((a.runner.as_str(), a.env.class(), a.scope), ("npm test", EnvClass::Project, Scope::File));
        let a = line(&["npx vitest run packages/runtime-core"]);
        assert_eq!((a.runner.as_str(), a.scope), ("vitest", Scope::Package));
        let a = line(&["go test ./... -run TestFlag -v"]);
        assert_eq!(
            (a.runner.as_str(), a.env.class(), a.selectors.clone()),
            ("go test", EnvClass::Project, vec!["TestFlag".to_string()])
        );
        let a = line(&["PYTHONPATH=src timeout 60 python -m pytest tests"]);
        assert_eq!((a.role, a.scope), (Role::TestRun, Scope::Package));
        // An install then a test run is a test run.
        let a = line(&["pip install pytest -q", "python -m pytest tests/test_x.py"]);
        assert_eq!(a.role, Role::TestRun);
        assert_eq!(a.env.class(), EnvClass::System);
        let a = line(&["/root/.local/bin/pytest tests/test_x.py"]);
        assert_eq!(a.env, Env::UserSite);
    }

    #[test]
    fn other_roles() {
        assert_eq!(line(&["python3 -c import click"]).role, Role::Repro);
        assert_eq!(line(&["python3 -c import click"]).scope, Scope::Inline);
        assert_eq!(line(&["python /tmp/repro.py"]).scope, Scope::ScratchFile);
        assert_eq!(line(&["pip install -e ."]).role, Role::EnvSetup);
        assert_eq!(line(&["python -m pip install pytest"]).role, Role::EnvSetup);
        assert_eq!(line(&["git status"]).role, Role::VcsRead);
        assert_eq!(line(&["git commit -m x"]).role, Role::VcsWrite);
        assert_eq!(line(&["grep -n def src/x.py"]).role, Role::Search);
        assert_eq!(line(&["cargo clippy --workspace"]).role, Role::Lint);
        assert_eq!(line(&["echo hi"]).role, Role::Trivial);
    }

    #[test]
    fn the_follow_judgement_is_an_equivalence_class() {
        let d = line(&[".venv/bin/python -m pytest tests/test_options.py"]);
        assert!(satisfies(&d, &line(&[".venv/bin/python -m pytest tests/test_options.py::test_x -xvs", "head"])));
        assert!(satisfies(&d, &line(&["source .venv/bin/activate", "python -m pytest tests/test_options.py -q"])));
        assert!(satisfies(&d, &line(&[".venv/bin/python -m pytest tests"])));
        assert!(satisfies(&d, &line(&[".venv/bin/python -m pytest"])));
        assert!(!satisfies(&d, &line(&["python -m pytest tests/test_options.py"])), "the system environment");
        assert!(!satisfies(&d, &line(&[".venv/bin/python -m pytest tests/test_basic.py"])), "another file");
        let any = line(&[".venv/bin/python -m pytest"]);
        assert!(satisfies(&any, &line(&[".venv/bin/python -m pytest tests/test_basic.py -x"])));
    }
}
