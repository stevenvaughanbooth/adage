//! The tests of what was edited (evals/lab/abstraction/README.md §4.4; architecture review §3.3):
//! after an edit of a source file, run the tests that go with it, with the runner that works in
//! this repository.
//!
//! The reflex is built, not copied: agents rarely run the same test command twice (a different
//! file and flags each time), but what they do is the same, the project's tests of the edited area
//! (`test-run|narrow`, [`adage_core::action`]). So:
//!
//! - **The runner** is the project's own (`.venv/bin/python -m pytest` from the repository's
//!   declared environment, else the project-environment spelling that ran tests in the traces:
//!   `npm test --`, `npx vitest run`), never the system interpreter a dead end names.
//! - **The targets** are bound at hook time from the edited file through a map (edited source path
//!   → its test files, best first, [`TestsOf`]). Names and co-change supply candidate relations;
//!   delivery additionally requires observed execution of each target by the selected command.
//!   A test argument, collection report or green zero-test run alone cannot establish this.
//!   Dynamic runner configuration is not interpreted or executed. The slot is `edit_tests`: a
//!   file the qualified map does not list gets nothing.
//! - **Admission and strength** come from the value of the directive at the `after-edit:source`
//!   situation ([`crate::value`]): calls from the first source edit to a test run that ran tests,
//!   without a directive and when the agent's next run was in the project's environment. It is
//!   also admitted when verified (the runner declared and seen running tests, and a trace that ran a
//!   mapped test after editing its source). A check after an edit is suggested, never fired: prime
//!   at most.

use crate::import::Exposure as _;
use crate::import::Session;
use crate::outcome::{self, Runner};
use crate::value::{self, Priors, Statics};
use adage_core::action::{self, EnvClass, Role, Target};
use adage_core::gitobj::{parse_hex, Oid, Repo};
use adage_core::Effect;
use adage_graph::stats::{self, Advantage};
use adage_tracks::why::Why;
use adage_tracks::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// At most this many tests are bound for one edited file.
pub const K: u8 = 3;
/// Commits (merges aside) of the history read for co-change, at most, and the largest commit that counts (a
/// sweeping change says nothing about which tests go with which file).
const HISTORY: usize = 2_000;
const MAX_COMMIT_FILES: usize = 40;
/// Files of a repository read, at most.
const MAX_FILES: usize = 60_000;

/// Directories never read for the map.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".tox",
    ".nox",
    "dist",
    "build",
    "target",
    ".next",
    "coverage",
    ".cache",
    "site-packages",
    "vendor",
];

/// The repository's files (relative paths), skipping environments and build output.
pub fn files(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(e.path());
                }
            } else if ft.is_file() {
                if let Ok(rel) = e.path().strip_prefix(root) {
                    out.insert(rel.to_string_lossy().replace('\\', "/"));
                }
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out
}

/// Pairs of files the history changed together, with the commits that did: the newest commits
/// reachable from `HEAD` (all parents, newest committer time first, merges skipped: what
/// `git log --no-merges` lists), read from the object store with [`adage_core::gitobj`] (the
/// learner spawns nothing). None when the root is not a checkout the reader can see (no history,
/// shallow, alternates).
pub fn cochange(root: &Path) -> BTreeMap<(String, String), u32> {
    let mut out = BTreeMap::new();
    let Some(mut repo) = Repo::open(root) else { return out };
    let Some(head) = repo.head() else { return out };
    let Some((COMMIT, body)) = repo.object(&head, MAX_OBJECT) else { return out };
    let mut queue = std::collections::BinaryHeap::from([(commit_links(&body).2, head, body)]);
    let mut seen = BTreeSet::from([head]);
    let mut read = 0;
    for _ in 0..adage_core::gitobj::MAX_WALK {
        if read >= HISTORY {
            break;
        }
        let Some((_, _, body)) = queue.pop() else { break };
        let (Some(tree), parents, _) = commit_links(&body) else { continue };
        let mut parent_tree = None;
        for p in &parents {
            let Some((COMMIT, pb)) = repo.object(p, MAX_OBJECT) else { continue };
            let links = commit_links(&pb);
            parent_tree = links.0;
            if seen.insert(*p) {
                queue.push((links.2, *p, pb));
            }
        }
        // Merges are skipped, as `git log --no-merges` skips them.
        if parents.len() >= 2 {
            continue;
        }
        read += 1;
        let mut files = BTreeSet::new();
        let complete = changed(&mut repo, Some(tree), parent_tree, "", &mut files, 0);
        if complete && files.len() >= 2 && files.len() <= MAX_COMMIT_FILES {
            let tests: Vec<&String> = files.iter().filter(|f| outcome::is_test_path(f)).collect();
            for s in files.iter().filter(|f| source_path(f)) {
                for t in &tests {
                    *out.entry((s.clone(), (*t).clone())).or_default() += 1;
                }
            }
        }
    }
    out
}

const COMMIT: u8 = 1;
const TREE: u8 = 2;
/// The largest commit or tree object the walk reads.
const MAX_OBJECT: usize = 4 << 20;
/// How deep a tree comparison descends.
const MAX_DEPTH: usize = 32;

/// A tree entry: whether it is a directory, and its object.
type Entry = (bool, Oid);

/// A commit's tree, parents and committer time (headers end at the first blank line).
fn commit_links(body: &[u8]) -> (Option<Oid>, Vec<Oid>, i64) {
    let (mut tree, mut parents, mut time) = (None, Vec::new(), 0);
    let hex = |l: &[u8]| std::str::from_utf8(l).ok().and_then(parse_hex);
    for line in body.split(|&b| b == b'\n') {
        if line.is_empty() {
            break;
        }
        if let Some(t) = line.strip_prefix(b"tree ") {
            tree = hex(t);
        } else if let Some(p) = line.strip_prefix(b"parent ") {
            parents.extend(hex(p));
        } else if let Some(c) = line.strip_prefix(b"committer ") {
            // `Name <email> <seconds> <zone>`
            time = String::from_utf8_lossy(c).rsplit(' ').nth(1).and_then(|t| t.parse().ok()).unwrap_or(0);
        }
    }
    (tree, parents, time)
}

/// A tree's entries by name (submodules left out); empty for no tree.
fn entries(repo: &mut Repo, id: Option<Oid>) -> Option<BTreeMap<String, Entry>> {
    let mut out = BTreeMap::new();
    let Some(id) = id else { return Some(out) };
    let (TREE, body) = repo.object(&id, MAX_OBJECT)? else { return None };
    let mut rest = body.as_slice();
    while !rest.is_empty() {
        let sp = rest.iter().position(|&b| b == b' ')?;
        let nul = rest.iter().position(|&b| b == 0)?;
        if nul < sp || rest.len() < nul + 21 {
            return None;
        }
        let mode = &rest[..sp];
        let name = String::from_utf8_lossy(&rest[sp + 1..nul]).into_owned();
        let oid: Oid = rest[nul + 1..nul + 21].try_into().ok()?;
        rest = &rest[nul + 21..];
        if mode != b"160000" {
            out.insert(name, (mode == b"40000", oid));
        }
    }
    Some(out)
}

/// The files that differ between two trees, into `files`; false when a tree could not be read or
/// the commit changed more files than the map uses.
fn changed(
    repo: &mut Repo,
    new: Option<Oid>,
    old: Option<Oid>,
    prefix: &str,
    files: &mut BTreeSet<String>,
    depth: usize,
) -> bool {
    if new == old {
        return true;
    }
    if depth > MAX_DEPTH {
        return false;
    }
    let (Some(n), Some(o)) = (entries(repo, new), entries(repo, old)) else { return false };
    let names: BTreeSet<&String> = n.keys().chain(o.keys()).collect();
    let dir = |e: Option<&Entry>| e.filter(|e| e.0).map(|e| e.1);
    let blob = |e: Option<&Entry>| e.is_some_and(|e| !e.0);
    for name in names {
        let (a, b) = (n.get(name), o.get(name));
        if a == b {
            continue;
        }
        let path = format!("{prefix}{name}");
        if blob(a) || blob(b) {
            files.insert(path.clone());
        }
        let (da, db) = (dir(a), dir(b));
        if (da.is_some() || db.is_some()) && !changed(repo, da, db, &format!("{path}/"), files, depth + 1) {
            return false;
        }
        if files.len() > MAX_COMMIT_FILES {
            return false;
        }
    }
    true
}

/// A source file the map is for: code, inside a directory, not a test.
pub fn source_path(p: &str) -> bool {
    let name = p.rsplit('/').next().unwrap_or(p);
    let ext = name.rsplit_once('.').map(|x| x.1).unwrap_or("");
    p.contains('/')
        && !outcome::is_test_path(p)
        && matches!(ext, "py" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "mts" | "vue" | "go" | "rs" | "rb")
        && !name.starts_with("__init__")
        && !name.ends_with(".d.ts")
}

/// A source file's name as a test names it: no directory, no extension, no leading underscore
/// (`src/attr/_make.py` → `make`).
fn source_stem(p: &str) -> &str {
    let name = p.rsplit('/').next().unwrap_or(p);
    let stem = name.split('.').next().unwrap_or(name);
    stem.trim_start_matches('_')
}

/// The name a test file gives the source it tests (`tests/test_make.py` → `make`,
/// `__tests__/effectScope.spec.ts` → `effectScope`, `flag_test.go` → `flag`).
fn test_stem(p: &str) -> &str {
    let name = p.rsplit('/').next().unwrap_or(p);
    let stem = name.split('.').next().unwrap_or(name);
    let stem = stem.strip_prefix("test_").unwrap_or(stem);
    let stem = stem.strip_suffix("_test").unwrap_or(stem);
    stem.trim_start_matches('_')
}

/// The package a path is in: its first two segments (`packages/runtime-core`, `src/click`).
fn package(p: &str) -> String {
    p.split('/').take(2).collect::<Vec<_>>().join("/")
}

fn language(p: &str) -> Option<&'static str> {
    match p.rsplit_once('.')?.1 {
        "py" => Some("python"),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "mts" | "cjs" | "cts" | "vue" => Some("javascript"),
        "rs" => Some("rust"),
        "go" => Some("go"),
        "rb" => Some("ruby"),
        _ => None,
    }
}

/// A nearest manifest is a package boundary, not evidence that its tests cover a source file.
fn owner<'a>(p: &'a str, files: &BTreeSet<String>) -> &'a str {
    let mut dir = p.rsplit_once('/').map_or("", |(d, _)| d);
    loop {
        if ["package.json", "pyproject.toml", "setup.cfg", "Cargo.toml", "go.mod", "Gemfile"]
            .iter()
            .any(|name| files.contains(&if dir.is_empty() { (*name).into() } else { format!("{dir}/{name}") }))
        {
            return dir;
        }
        if dir.is_empty() {
            return "";
        }
        dir = dir.rsplit_once('/').map_or("", |(d, _)| d);
    }
}

/// The map from an edited source path to its tests, best first.
pub type TestsOf = BTreeMap<String, Vec<String>>;

/// Evidence for one (source, test) pair.
#[derive(Debug, Clone, Default)]
struct Pair {
    named: bool,
    same_package: bool,
    cochange: u32,
    traces: u32,
}

/// Candidate file → tests relations, not a runner-qualified delivery map. For every source file, its
/// best `K` test files, each scored by a trace that ran it after editing the source (2 each, at
/// most 4), a name that matches (1, and 0.5 more in the same package), and history: from two
/// commits that changed both, 0.5 plus half its share of the source's co-changes with tests.
/// Ties by path. Names cannot cross languages or known package boundaries; co-change alone cannot
/// cross languages. Use [`qualified_tests_of`] before binding a runner to these candidates.
pub fn tests_of(
    files: &BTreeSet<String>,
    cochange: &BTreeMap<(String, String), u32>,
    confirmed: &BTreeMap<(String, String), u32>,
) -> TestsOf {
    let tests: Vec<&String> = files.iter().filter(|f| outcome::is_test_path(f)).collect();
    let mut by_stem: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    for t in &tests {
        by_stem.entry(test_stem(t)).or_default().push(t);
    }
    let mut pairs: BTreeMap<String, BTreeMap<String, Pair>> = BTreeMap::new();
    for s in files.iter().filter(|f| source_path(f)) {
        let stem = source_stem(s);
        if stem.len() < 2 {
            continue;
        }
        for t in by_stem.get(stem).into_iter().flatten() {
            if language(s).is_none() || language(s) != language(t) || owner(s, files) != owner(t, files) {
                continue;
            }
            let p = pairs.entry(s.clone()).or_default().entry((*t).clone()).or_default();
            p.named = true;
            p.same_package = package(s) == package(t);
        }
    }
    for ((s, t), n) in cochange {
        if files.contains(s)
            && files.contains(t)
            && source_path(s)
            && outcome::is_test_path(t)
            && language(s).is_some()
            && language(s) == language(t)
            && *n >= 2
        {
            pairs.entry(s.clone()).or_default().entry(t.clone()).or_default().cochange = *n;
        }
    }
    for ((s, t), n) in confirmed {
        if files.contains(s) && files.contains(t) && source_path(s) && outcome::is_test_path(t) && *n > 0 {
            pairs.entry(s.clone()).or_default().entry(t.clone()).or_default().traces = *n;
        }
    }
    let mut out = TestsOf::new();
    for (s, ts) in pairs {
        let most = ts.values().map(|p| p.cochange).max().unwrap_or(0).max(1) as f64;
        let mut scored: Vec<(f64, String)> = ts
            .into_iter()
            .map(|(t, p)| {
                let score = (2.0 * p.traces as f64).min(4.0)
                    + if p.named { 1.0 + if p.same_package { 0.5 } else { 0.0 } } else { 0.0 }
                    + if p.cochange >= 2 { 0.5 + 0.5 * p.cochange as f64 / most } else { 0.0 };
                (score, t)
            })
            .filter(|(score, _)| *score >= 0.5)
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let best: Vec<String> = scored.into_iter().take(K as usize).map(|(_, t)| t).collect();
        if !best.is_empty() {
            out.insert(s, best);
        }
    }
    out
}

/// The test files a test run selected (a test id's file; directories are not files).
fn run_files(a: &action::Action) -> Vec<String> {
    a.targets
        .iter()
        .filter_map(|t| match t {
            Target::File(p) | Target::TestId(p, _) => Some(p.trim_start_matches("./").to_string()),
            Target::Dir(_) => None,
        })
        .collect()
}

/// (source, test) pairs with attributable execution evidence, counted once per trace. This
/// aggregate is for inspection; a delivery map must use [`qualified_tests_of`] to keep runners
/// and command spellings separate.
pub fn confirmed(ss: &[&Session]) -> BTreeMap<(String, String), u32> {
    observations(ss, None).0
}

/// Candidate relations qualified by actual target execution under this exact command prefix.
/// Neither executable configuration nor a filename/argument alone is discovery evidence. A
/// configuration that has not produced attributable test output therefore fails closed here.
/// This records historical selection, not passing assertions, current coverage or causal value.
pub fn qualified_tests_of(
    files: &BTreeSet<String>,
    cochange: &BTreeMap<(String, String), u32>,
    ss: &[&Session],
    command: &str,
) -> TestsOf {
    let (confirmed, targets) = observations(ss, Some(command));
    // Filter before ranking: unsupported targets must not displace an evidenced one from K.
    let eligible = files.iter().filter(|p| !outcome::is_test_path(p) || targets.contains(*p)).cloned().collect();
    tests_of(&eligible, cochange, &confirmed)
}

fn observations(ss: &[&Session], command: Option<&str>) -> (BTreeMap<(String, String), u32>, BTreeSet<String>) {
    let mut out: BTreeMap<(String, String), u32> = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for s in ss {
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        let mut last: Option<String> = None;
        for st in &s.steps {
            if let Some(p) = outcome::source_edit(st) {
                last = Some(p.to_string());
                continue;
            }
            let Some(a) = attributed_run(st, s) else { continue };
            let Some(prefix) = map_prefix(&a) else { continue };
            if command.is_some_and(|c| c != prefix) || s.exposed_to(&[prefix.as_str()], false) {
                continue;
            }
            for t in executed_targets(&a, &st.call.output) {
                targets.insert(t.clone());
                if let Some(src) = &last {
                    seen.insert((src.clone(), t));
                }
            }
        }
        for k in seen {
            *out.entry(k).or_default() += 1;
        }
    }
    (out, targets)
}

/// Flattened steps can share output; ordinary normalization also erases pure pipe filters and
/// assignment prefixes. Require one literal stage in the original canonical shell call, so no
/// other command, filter, cwd change or environment assignment can lend it evidence.
fn attributed_run(st: &crate::import::StepObs, session: &Session) -> Option<action::Action> {
    if st.call.tool != "Bash"
        || !st.call.by.is_agent()
        || st.call.refused()
        || st.call.input.get("cwd").and_then(|v| v.as_str()).is_some_and(|cwd| Path::new(cwd) != session.root)
    {
        return None;
    }
    let command = st.call.input.get("command")?.as_str()?;
    let stages = adage_core::shell::plain_pipeline(command)?;
    let [stage] = stages.as_slice() else { return None };
    // Preserve the canonical repository-relative spelling of a project interpreter/target,
    // after the original syntax has passed the attribution gate.
    let mut n = st.norm.clone();
    n.action = adage_core::CanonicalAction::Exec {
        argv: stage.argv.clone(),
        cwd: session.root.to_string_lossy().into_owned(),
    };
    adage_core::keys::canonicalise(&mut n, &session.root);
    let adage_core::CanonicalAction::Exec { argv, .. } = &n.action else { return None };
    let a = action::classify_argv(argv);
    (a.role == Role::TestRun && a.env.class() == EnvClass::Project).then_some(a)
}

/// Retain only default-scope spellings. Stripping --config/--project/root/environment options
/// would transfer evidence to a different command. Selectors and unknown flags also abstain.
fn map_prefix(a: &action::Action) -> Option<String> {
    if !matches!(a.runner.as_str(), "pytest" | "vitest" | "jest" | "npm test") || !a.selectors.is_empty() {
        return None;
    }
    for (i, token) in a.argv.iter().enumerate() {
        if token.contains('=') || matches!(token.as_str(), "watch" | "related" | "bench" | "list") {
            return None;
        }
        if !token.starts_with('-') || token == "--" {
            continue;
        }
        if token == "-m" && a.runner == "pytest" && a.argv.get(i + 1).is_some_and(|m| m == "pytest") {
            continue;
        }
        let verbosity = token.strip_prefix('-').is_some_and(|s| {
            !s.is_empty() && s.chars().all(|c| if a.runner == "pytest" { "qvxs".contains(c) } else { c == 'v' })
        });
        if !verbosity {
            return None;
        }
    }
    let p = prefix(a)?;
    let parsed = action::classify_argv(&p.split_whitespace().map(str::to_string).collect::<Vec<_>>());
    (parsed.role == Role::TestRun
        && parsed.runner == a.runner
        && parsed.env == a.env
        && parsed.targets.is_empty()
        && (a.runner != "vitest" || p.ends_with("vitest run")))
    .then_some(p)
}

fn runner_file(family: &str, path: &str) -> bool {
    adage_core::repo::is_safe_relative(path)
        && path.len() <= 200
        && path.bytes().all(|c| c.is_ascii_alphanumeric() || b"_./-@+".contains(&c))
        && match family {
            "pytest" => path.ends_with(".py"),
            "vitest" | "jest" | "npm test" => matches!(language(path), Some("javascript")) && !path.ends_with(".vue"),
            _ => false,
        }
}

fn executed_targets(a: &action::Action, output: &str) -> Vec<String> {
    use adage_core::outcome::{verification_report, VerificationOutcome};
    let Some(report) = verification_report(&a.argv, output) else { return Vec::new() };
    if !matches!(report.outcome, VerificationOutcome::Passed | VerificationOutcome::Failed)
        || !report.tests_run.is_some_and(|n| n > 0)
    {
        return Vec::new();
    }
    let selected = run_files(a);
    // Pytest selects an exact path. JS positional arguments are filename filters: even one
    // filter can match a different file, so those runners always need per-file output.
    let single = a.runner == "pytest" && a.targets.len() == 1 && selected.len() == 1;
    selected
        .into_iter()
        .filter(|p| runner_file(&a.runner, p))
        .filter(|p| single || file_reported(&a.runner, p, output))
        .collect()
}

fn file_reported(family: &str, path: &str, output: &str) -> bool {
    if family == "npm test" {
        // A script's name is not its implementation. Require its own output to identify the
        // file using a supported runner report; never infer this from package.json text.
        return file_reported("vitest", path, output) || file_reported("jest", path, output);
    }
    // Only runner-shaped lines qualify, never a command echo or a traceback mentioning a path.
    // Unknown reporter formats abstain; this deliberately does not parse arbitrary JS config.
    adage_core::tokens::bounded(output, 64 * 1024).lines().any(|line| {
        let line = line.trim();
        match family {
            "pytest" => line.strip_prefix(path).is_some_and(|rest| {
                let rest = rest.trim_start();
                rest.starts_with("::") && rest.split_whitespace().any(|t| matches!(t, "PASSED" | "FAILED"))
                    || (!rest.is_empty()
                        && rest.split_whitespace().next().is_some_and(|t| {
                            t.chars().all(|c| ".FsxX".contains(c)) && t.chars().any(|c| c == '.' || c == 'F')
                        }))
            }),
            "vitest" => line
                .strip_prefix('✓')
                .or_else(|| line.strip_prefix('❯'))
                .map(str::trim_start)
                .and_then(|s| s.strip_prefix(path))
                .and_then(|s| s.strip_prefix(" ("))
                .is_some_and(|s| {
                    let Some((counts, _)) = s.split_once(')') else { return false };
                    if counts.contains("skipped") || counts.contains("todo") {
                        return false;
                    }
                    let mut words = s.split_whitespace();
                    words.next().and_then(|n| n.parse::<u64>().ok()).is_some_and(|n| n > 0)
                        && words.next().is_some_and(|s| s == "tests)" || s == "test)" || s == "tests" || s == "test")
                }),
            "jest" => line.strip_prefix("PASS ").map(str::trim_start).is_some_and(|s| {
                s.strip_prefix(path).is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
            }),
            _ => false,
        }
    })
}

/// The command prefix of a test run, up to its first target or selector: `.venv/bin/python -m
/// pytest`, `npm test --`, `npx vitest run`. Options are dropped (`-xvs` is one run's choice).
fn prefix(a: &action::Action) -> Option<String> {
    let first_target = a.targets.first().map(|t| t.path().to_string());
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    let av = &a.argv;
    while i < av.len() {
        let t = av[i].as_str();
        if Some(t.trim_end_matches('/')) == first_target.as_deref()
            || t.split("::").next() == first_target.as_deref()
            || a.selectors.iter().any(|s| s == t)
        {
            break;
        }
        if t == "-m" {
            out.push(t);
            if let Some(m) = av.get(i + 1) {
                out.push(m);
            }
            i += 2;
            continue;
        }
        if t == "--" || !t.starts_with('-') {
            if t.contains('=') && out.is_empty() {
                // A leading environment assignment is one run's choice.
                i += 1;
                continue;
            }
            out.push(t);
        }
        i += 1;
    }
    let p = out.join(" ");
    (!p.is_empty() && io::safe_command(&p)).then_some(p)
}

/// The test-map route, valued (see the module documentation).
#[derive(Debug, Clone, Serialize)]
pub struct Planned {
    pub glob: String,
    pub command: String,
    pub runner: String,
    pub map_files: usize,
    pub advantage: Advantage,
    pub n_null: u32,
    pub n_sat: u32,
    pub verified: bool,
    /// What delivering it changes (`value::uplift`): gated, it is not delivered.
    pub uplift: Uplift,
    pub strength: Option<Strength>,
}

/// Plan the tests-of-the-edit route for the repository's traces: `None` when the traces run no
/// tests after source edits, or no file has mapped tests; the route only when it is admitted and
/// has a strength, and the plan either way.
pub fn plan(
    ss: &[&Session],
    statics: &Statics,
    root: Option<&Path>,
    priors: &Priors,
    uplift_min: f64,
    provenance: &str,
) -> Option<(Option<(Route, Why)>, Planned)> {
    // The situations: the first source edit before a test run that ran tests.
    let mut at: Vec<(&Session, outcome::Situation)> = Vec::new();
    for s in ss {
        for x in outcome::situations(s, &statics.env_kind) {
            if x.key.trigger == "after-edit:source" {
                at.push((s, x));
            }
        }
    }
    // The family agents test with after an edit, and its project-environment spelling.
    let mut families: BTreeMap<String, u32> = BTreeMap::new();
    for (_, x) in &at {
        if let Some(r) = &x.next {
            *families.entry(r.family.clone()).or_default() += 1;
        }
    }
    let family = families.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(f, _)| f.clone())?;
    // The reflex binds the edited file's tests as file paths: only for runners that take them
    // (`cargo test crate tests/x.rs` is not a command that can run).
    if !outcome::takes_test_files(&family) {
        return None;
    }
    let mut spellings: BTreeMap<String, (u32, action::Env)> = BTreeMap::new();
    let mut ran_tests = 0u32;
    for s in ss {
        let mut ran = false;
        for st in &s.steps {
            let Some(a) = outcome::action_of(st).filter(|a| a.role == Role::TestRun && a.runner == family) else {
                continue;
            };
            if attributed_run(st, s).is_none() || executed_targets(&a, &st.call.output).is_empty() {
                continue;
            }
            if let Some(p) = map_prefix(&a) {
                ran = true;
                spellings.entry(p).or_insert((0, a.env.clone())).0 += 1;
            }
        }
        ran_tests += u32::from(ran);
    }
    let seen = spellings
        .iter()
        .max_by(|a, b| a.1 .0.cmp(&b.1 .0).then_with(|| b.0.cmp(a.0)))
        .map(|(p, (_, e))| (p.clone(), e.clone()));
    let declared = statics.declared.get(&family).map(|(e, c)| (c.clone(), e.clone()));
    let (command, env) = declared.clone().or(seen)?;
    let runner = Runner { env: env.clone(), family: family.clone() };
    // The map, and the area it covers.
    let files: BTreeSet<String> = match root.filter(|r| r.is_dir()) {
        Some(r) => files(r),
        None => BTreeSet::new(),
    };
    let (confirmed, _) = observations(ss, Some(&command));
    let files: BTreeSet<String> = if files.is_empty() {
        // No repository at hand: what the traces edited and ran.
        confirmed.keys().flat_map(|(s, t)| [s.clone(), t.clone()]).collect()
    } else {
        files
    };
    let history = root.map(cochange).unwrap_or_default();
    let map = qualified_tests_of(&files, &history, ss, &command);
    // The area: the lattice over the source files the traces edited that have mapped tests.
    let edited: BTreeSet<String> = ss
        .iter()
        .flat_map(|s| s.steps.iter().filter_map(|st| outcome::source_edit(st).map(str::to_string)))
        .filter(|p| map.contains_key(p))
        .collect();
    if edited.is_empty() {
        return None;
    }
    let keys: BTreeSet<String> = map.keys().cloned().collect();
    let members: Vec<&str> = edited.iter().map(String::as_str).collect();
    let globs = adage_graph::compile::glob::lattice(&members, &keys, &keys, 0.9);
    // One glob: the least general one covering most of the edited files.
    let glob = globs
        .iter()
        .filter(|g| !g.starts_with("**") && !g.starts_with('*'))
        .max_by(|a, b| {
            let n = |g: &str| edited.iter().filter(|p| adage_graph::select::glob_match(g, p)).count();
            n(a).cmp(&n(b)).then_with(|| b.cmp(a))
        })?
        .clone();
    let map: TestsOf = map.into_iter().filter(|(s, _)| adage_graph::select::glob_match(&glob, s)).collect();
    if map.is_empty() || !glob.contains('*') {
        return None;
    }
    // Value at the after-edit situation.
    let key = at.first().map(|(_, x)| x.key.clone())?;
    let role = env.kind().to_string();
    let unexposed: Vec<&(&Session, outcome::Situation)> =
        at.iter().filter(|(s, _)| !s.exposed_to(&[command.as_str()], false)).collect();
    let null: Vec<f64> = unexposed.iter().map(|(_, x)| x.reward.calls as f64).collect();
    let sat: Vec<&&(&Session, outcome::Situation)> = unexposed
        .iter()
        .filter(|(_, x)| {
            x.next.as_ref().is_some_and(|r| r.family == runner.family && r.env.class() == EnvClass::Project)
        })
        .collect();
    let in_sat = |x: &outcome::Situation| {
        x.next.as_ref().is_some_and(|r| r.family == runner.family && r.env.class() == EnvClass::Project)
    };
    let other: Vec<f64> = unexposed.iter().filter(|(_, x)| !in_sat(x)).map(|(_, x)| x.reward.calls as f64).collect();
    let (m0, k0, _, _) = priors.prior(&key, "null");
    let (m1, k1, ma, mb) = priors.prior(&key, &role);
    let q_other = stats::gamma_poisson(other.iter().sum(), other.len() as f64, m0, k0);
    let q_sat = stats::gamma_poisson(sat.iter().map(|(_, x)| x.reward.calls as f64).sum(), sat.len() as f64, m1, k1);
    let mis = sat.iter().filter(|(_, x)| x.next_misfired).count() as f64;
    // What delivering changes: the edits after which agents already ran the tests in the project's
    // environment are `p_self`'s evidence (the research's prior for an after-edit run: 0.32).
    let unaided =
        value::Unaided { traces: unexposed.len() as u32, traces_took: sat.len() as u32, ..Default::default() };
    let up = value::uplift("run-after-edit", &unaided, priors, uplift_min);
    let adv = value::advantage_uplift(
        q_other,
        q_sat,
        &up,
        (ma + mis, mb + sat.len() as f64 - mis),
        value::DIRECTIVE_COST,
        value::MISFIRE_COST,
    );
    let map_confirmed = confirmed.iter().any(|((s, t), _)| map.get(s).is_some_and(|ts| ts.contains(t)));
    // The runner ran tests in a trace (declared or not), and a trace ran a mapped test after its source.
    let verified = ran_tests >= 1 && map_confirmed;
    let effective = sat.len() as f64 + k1;
    // Verified or valued, and worth delivering: agents who already run the tests the way that works
    // after such an edit gain nothing from being told.
    let admitted = (verified || (adv.lower > 0.0 && effective >= value::MIN_EFFECTIVE)) && !up.summary.gated;
    // A check after an edit is suggested, never fired.
    let strength = admitted.then(|| value::strength_of(&adv, Effect::Verify, true)).flatten().map(|s| {
        if s == Strength::Fire {
            Strength::Prime
        } else {
            s
        }
    });
    let planned = Planned {
        glob: glob.clone(),
        command: format!("{command} {{tests}}"),
        runner: family.clone(),
        map_files: map.len(),
        advantage: adv,
        n_null: null.len() as u32,
        n_sat: sat.len() as u32,
        verified,
        uplift: up.summary.clone(),
        strength,
    };
    let Some(strength) = strength.filter(|_| io::safe_command(&command)) else { return Some((None, planned)) };
    let step = Step::Run { command: planned.command.clone() };
    let trigger = Trigger::AfterEdit { paths: glob.clone() };
    let slots = vec![Slot {
        name: "tests".into(),
        source: SlotSource::EditTests { glob: glob.clone(), k: K, map },
        contract: Contract::RepoPath,
    }];
    let id = adage_graph::compile::ids::route_id("run", &trigger, &step, &slots);
    let (support, opp) = (planned.n_sat, planned.n_null.max(planned.n_sat));
    let route = Route {
        id,
        summary: "The tests of what was edited, run the way that works in this repository".into(),
        trigger,
        step,
        slots,
        effect: Effect::Verify,
        strength,
        state: initial_state(Effect::Verify),
        exit: vec![Exit::ExploratoryChange],
        evidence: Evidence {
            opportunities: opp,
            support,
            rate: if opp > 0 { support as f64 / opp as f64 } else { 0.0 },
            lower_bound: wilson_lower(support, opp),
            turns_saved: (adv.mean * 100.0).round() / 100.0,
            provenance: provenance.into(),
            models: crate::evidence::models(ss),
            origin: Some(Origin::Learned),
            uplift: Some(up.summary),
            ..Default::default()
        },
        authored: false,
    };
    let why = Why {
        shape: "run".into(),
        pattern: format!(
            "[edit {glob}] =tests_of=> [{} {{tests}}]: A {:.2} calls (sd {:.2}, P(A>0) {:.2}); Q(∅) {:.2}, Q(Sat) {:.2}; {} triggers, {} ran it unaided; uplift {:+.2}; {} files mapped; {}",
            command,
            adv.mean,
            adv.sd,
            adv.p_positive,
            adv.q_null,
            adv.q_sat,
            planned.n_null,
            planned.n_sat,
            planned.uplift.mean,
            planned.map_files,
            if verified { "verified" } else { "valued" }
        ),
        backoff: 1,
        gain: (adv.mean * 100.0).round() / 100.0,
        lift: 0.0,
        conf: (adv.p_positive * 1e4).round() / 1e4,
        conf_lb: (adv.lower * 1e4).round() / 1e4,
        support,
        days: 0,
        members: vec![],
        examples: vec![],
        prior: None,
    };
    Some((Some((route, why)), planned))
}

/// Files that say how to work in a repository: reading one before the first test run is a step
/// spent finding out how to run things.
const HOW_TO: &[&str] = &[
    "pyproject.toml",
    "setup.cfg",
    "setup.py",
    "tox.ini",
    "noxfile.py",
    "pytest.ini",
    "conftest.py",
    "package.json",
    "go.mod",
    "Cargo.toml",
    "Makefile",
    "justfile",
    "requirements.txt",
    "requirements-dev.txt",
    "vitest.config.ts",
    "vitest.config.js",
    "jest.config.js",
    ".python-version",
    "CONTRIBUTING.md",
    "CONTRIBUTING.rst",
];

/// Whether a step explores how to work in the repository (outcome-optimisation §1.2's discovery;
/// the embedding lab's "exploratory" steps): a manifest or tool configuration read, an install, a
/// look at the environment (`which`, `--version`, `pip list`, a `.venv` listing), or a test run
/// that failed for the environment.
fn exploratory(st: &crate::import::StepObs) -> bool {
    if let Some(p) = st.rel_path.as_deref() {
        if !adage_core::keys::is_author(&st.norm.action) && HOW_TO.contains(&p.rsplit('/').next().unwrap_or(p)) {
            return true;
        }
    }
    let Some(a) = outcome::action_of(st) else { return false };
    if a.role == Role::EnvSetup {
        return true;
    }
    if a.role == Role::TestRun {
        return st.verification().is_some_and(|r| r.outcome == adage_core::outcome::VerificationOutcome::SetupFailure)
            && outcome::env_failure(&st.call.output).is_some();
    }
    let text = a.argv.join(" ");
    matches!(a.role, Role::Search | Role::Read | Role::Trivial | Role::Other)
        && (a.runner == "which"
            || text.contains("--version")
            || text.contains("venv")
            || text.contains("site-packages")
            || text.contains("node_modules/.bin")
            || (text.starts_with("pip") && (text.contains(" list") || text.contains(" show"))))
}

/// The session-start line (the embedding lab's usefulness-labelled baseline, and the review's
/// affordance card): "tests run with <the project's runner>", delivered once per session when it
/// was a *useful* directive in at least [`CARD_RATE`] of the training sessions and at least two of
/// them. Useful means the session later ran tests that way (the same tool, in a project
/// environment, and tests ran), and before that spent a failed command or at least two steps
/// finding out how to run things; a session that went straight to it did not need telling.
pub fn card(ss: &[&Session], statics: &Statics, provenance: &str) -> Option<(Option<(Route, Why)>, Card)> {
    // The tool this repository's tests run with: the family most sessions ran tests with.
    let mut families: BTreeMap<String, u32> = BTreeMap::new();
    for s in ss {
        let fams: BTreeSet<String> = s
            .steps
            .iter()
            .filter(|st| outcome::test_goal(st))
            .filter_map(|st| outcome::runner(st).map(|r| r.family))
            .collect();
        for f in fams {
            *families.entry(f).or_default() += 1;
        }
    }
    let family = families.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(f, _)| f.clone())?;
    let mut spellings: BTreeMap<String, u32> = BTreeMap::new();
    for s in ss {
        for st in &s.steps {
            let Some(a) = outcome::action_of(st).filter(|a| a.role == Role::TestRun && a.runner == family) else {
                continue;
            };
            if a.env.class() == EnvClass::Project && outcome::executed_tests(st) {
                if let Some(p) = prefix(&a) {
                    *spellings.entry(p).or_default() += 1;
                }
            }
        }
    }
    let seen = spellings.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(p, _)| p.clone());
    let command = statics.declared.get(&family).map(|(_, c)| c.clone()).or(seen)?;
    let (mut useful, n) = (0u32, ss.len() as u32);
    for s in ss {
        let Some(at) = s.steps.iter().position(|st| {
            outcome::test_goal(st)
                && outcome::runner(st).is_some_and(|r| r.family == family && r.env.class() == EnvClass::Project)
        }) else {
            continue;
        };
        let before = &s.steps[..at];
        let failed = before.iter().any(|st| st.call.failed && outcome::action_of(st).is_some());
        let explored = before.iter().filter(|st| exploratory(st)).count();
        useful += u32::from(failed || explored >= 2);
    }
    let rate = if n > 0 { useful as f64 / n as f64 } else { 0.0 };
    let plan = Card { command: command.clone(), useful, sessions: n };
    if useful < 2 || rate < CARD_RATE {
        return Some((None, plan));
    }
    let mut slots_ = BTreeMap::new();
    slots_.insert("command".to_string(), command.clone());
    let step = Step::Advise { template: "test-command".into(), slots: slots_ };
    let trigger = Trigger::SessionStart;
    let id = adage_graph::compile::ids::route_id("advise", &trigger, &step, &[]);
    let route = Route {
        id,
        summary: "How tests run in this repository".into(),
        trigger,
        step,
        slots: vec![],
        effect: Effect::Observe,
        strength: Strength::Prime,
        state: initial_state(Effect::Observe),
        exit: vec![Exit::IntentChanges],
        evidence: Evidence {
            opportunities: n,
            support: useful,
            rate,
            lower_bound: wilson_lower(useful, n),
            provenance: provenance.into(),
            models: crate::evidence::models(ss),
            origin: Some(Origin::Learned),
            ..Default::default()
        },
        authored: false,
    };
    let why = Why {
        shape: "advise".into(),
        pattern: format!(
            "[session start] =useful=> [{command}]: a useful directive in {useful} of {n} sessions (they later ran tests that way, after a failed command or steps spent finding out how)"
        ),
        backoff: 0,
        gain: 0.0,
        lift: 0.0,
        conf: (rate * 1e4).round() / 1e4,
        conf_lb: (wilson_lower(useful, n) * 1e4).round() / 1e4,
        support: useful,
        days: 0,
        members: vec![],
        examples: vec![],
        prior: None,
    };
    Some((Some((route, why)), plan))
}

/// The session-start line as planned: its command, and the training sessions it was useful in.
#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub command: String,
    pub useful: u32,
    pub sessions: u32,
}

/// The share of training sessions in which the session-start line must have been useful.
pub const CARD_RATE: f64 = 0.7;

#[cfg(test)]
#[path = "testmap_qualification_tests.rs"]
mod qualification_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_names_tests_by_name_history_and_traces() {
        let files: BTreeSet<String> = [
            "src/click/core.py",
            "src/click/shell_completion.py",
            "src/attr/_make.py",
            "tests/test_shell_completion.py",
            "tests/test_options.py",
            "tests/test_basic.py",
            "tests/test_make.py",
            "packages/rt/src/scheduler.ts",
            "packages/rt/__tests__/scheduler.spec.ts",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut cc = BTreeMap::new();
        cc.insert(("src/click/core.py".to_string(), "tests/test_options.py".to_string()), 5);
        cc.insert(("src/click/core.py".to_string(), "tests/test_basic.py".to_string()), 3);
        cc.insert(("src/click/core.py".to_string(), "tests/test_make.py".to_string()), 1);
        let m = tests_of(&files, &cc, &BTreeMap::new());
        assert_eq!(m["src/click/shell_completion.py"], vec!["tests/test_shell_completion.py".to_string()]);
        assert_eq!(m["src/attr/_make.py"], vec!["tests/test_make.py".to_string()]);
        assert_eq!(m["packages/rt/src/scheduler.ts"], vec!["packages/rt/__tests__/scheduler.spec.ts".to_string()]);
        // History alone: the most co-changed first; a single co-change is no evidence.
        assert_eq!(
            m["src/click/core.py"],
            vec!["tests/test_options.py".to_string(), "tests/test_basic.py".to_string()]
        );
        // A trace confirms a pair over the name.
        let mut conf = BTreeMap::new();
        conf.insert(("src/click/shell_completion.py".to_string(), "tests/test_basic.py".to_string()), 2);
        let m = tests_of(&files, &cc, &conf);
        assert_eq!(m["src/click/shell_completion.py"][0], "tests/test_basic.py");
    }

    #[test]
    fn prefixes_drop_targets_and_options() {
        let p = |s: &str| prefix(&action::classify_argv(&s.split_whitespace().map(str::to_string).collect::<Vec<_>>()));
        assert_eq!(
            p(".venv/bin/python -m pytest tests/test_x.py::t -xvs").as_deref(),
            Some(".venv/bin/python -m pytest")
        );
        assert_eq!(p("npm test -- packages/a/__tests__/x.spec.ts").as_deref(), Some("npm test --"));
        assert_eq!(p("npx vitest run packages/a").as_deref(), Some("npx vitest run"));
        assert_eq!(p("python -m pytest -xvs tests/x.py").as_deref(), Some("python -m pytest"));
    }
}
