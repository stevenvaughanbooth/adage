//! Detectors: one per catalogue entry. Each reads only [`DeclaredFacts`] and proposes candidate
//! routes and advisories; the binder ([`crate::bind`]) assigns ids, applies the override rules and
//! validates every candidate.

use crate::catalogue::Catalogue;
use crate::declared::{DeclaredFacts, Entry, Source};
use crate::safety::{command_key, rank, Segment};
use adage_core::{CommandTable, Effect};
use adage_tracks::{Exit, Trigger};
use std::collections::{BTreeMap, BTreeSet};

/// The order detectors run in. Where two instincts propose the same trigger, the earlier wins, so
/// specific instincts (a wrapper that explains a failure) come before generic ones (the one test
/// command). Every catalogue id appears exactly once.
pub const BIND_ORDER: [&str; 21] = [
    "U1", "U11", "P1", "N1", "U3", "U5", "U4", "U6", "U12", "U13", "U2", "U7", "U8", "U9", "U10", "N2", "N3", "R1",
    "R2", "G1", "X1",
];

/// How a candidate route's id is formed.
#[derive(Debug, Clone, PartialEq)]
pub enum IdHint {
    /// The learner's dead-end id for the working command `wkey`: `dead-end-<stable(wkey)>` for the
    /// first route directing it, `dead-end-<stable(wkey, trigger key)>` for further ones.
    DeadEnd { wkey: String },
    /// The learner's finish-line id: `finish-line-<stable(glob, command)>`.
    FinishLine { glob: String },
    /// The learner learns no this shape: `instinct-<tag>-<stable(parts)>`.
    Own { tag: String, parts: Vec<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteCandidate {
    pub instinct: String,
    pub id: IdHint,
    pub trigger: Trigger,
    pub command: String,
    pub effect: Effect,
    pub exit: Exit,
    pub summary: String,
    /// The literal command whose failure triggers it, for people and override checks.
    pub runner: Option<String>,
    /// For the directive's wrapper: the ecosystem (one of `adage_tracks::ECOSYSTEMS`) and the
    /// file that declares what it directs.
    pub ecosystem: Option<&'static str>,
    pub declared: Option<String>,
}

impl RouteCandidate {
    fn from(mut self, ecosystem: Option<&'static str>, declared: Option<String>) -> Self {
        self.ecosystem = ecosystem;
        self.declared = declared;
        self
    }
}

/// The file a declared entry lives in.
pub fn source_file(s: Source) -> &'static str {
    match s {
        Source::Makefile => "Makefile",
        Source::PackageJson => "package.json",
        Source::Justfile => "justfile",
        Source::Taskfile => "Taskfile.yml",
        Source::Tox => "tox.ini",
        Source::Nox => "noxfile.py",
    }
}

/// The ecosystem of a runner family (`py:pytest` → Python).
pub fn ecosystem_of_family(fam: &str) -> Option<&'static str> {
    if fam.starts_with("py:") {
        Some("Python")
    } else if fam.starts_with("node:") || fam.starts_with("js:") {
        Some("Node.js")
    } else {
        None
    }
}

/// The ecosystem of a lockfile's manager.
pub fn ecosystem_of_manager(m: &str) -> Option<&'static str> {
    match m {
        "npm" | "pnpm" | "yarn" | "bun" => Some("Node.js"),
        "cargo" => Some("Rust"),
        "go" => Some("Go"),
        "poetry" | "uv" | "pipenv" | "pdm" => Some("Python"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdvisoryCandidate {
    pub instinct: String,
    pub template_id: String,
    pub trigger: Trigger,
    pub slots: BTreeMap<String, String>,
    pub id_parts: Vec<String>,
    pub ecosystem: Option<&'static str>,
    pub declared: Option<String>,
}

#[derive(Debug, Default)]
pub struct Detected {
    pub routes: Vec<RouteCandidate>,
    pub advisories: Vec<AdvisoryCandidate>,
    pub skipped: Vec<(String, String)>,
}

pub struct Ctx<'a> {
    pub facts: &'a DeclaredFacts,
    pub table: &'a CommandTable,
    pub cat: &'a Catalogue,
}

pub fn detect(id: &str, ctx: &Ctx) -> Detected {
    match id {
        "U1" => u1(ctx),
        "P1" => p1(ctx),
        "N1" => n1(ctx),
        "U3" => u3(ctx),
        "U5" => u5(ctx),
        "U4" => u4(ctx),
        "U6" => u6(ctx),
        "U11" => u11(ctx),
        "U12" => u12(ctx),
        "U13" => u13(ctx),
        // Catalogued, not yet bindable: the model cannot express them (see docs/instincts.md).
        "U2" | "U7" | "U8" | "U9" | "U10" | "N2" | "N3" | "R1" | "R2" | "G1" | "X1" => Detected::default(),
        other => Detected {
            skipped: vec![(other.to_string(), "no detector for this instinct".into())],
            ..Default::default()
        },
    }
}

/// U12: a declared, recognizable test runner reported no matching tests. The runtime also
/// requires the core runner-report parser to establish NoTests for the actual shell invocation;
/// a substring in an assertion or quoted log is insufficient. No command is run by this advice.
fn u12(ctx: &Ctx) -> Detected {
    use adage_core::outcome::{verification_report, VerificationOutcome};
    let mut d = Detected::default();
    let mut seen = BTreeSet::new();
    for e in &ctx.facts.entries {
        if e.effect() != Some(Effect::Verify) || e.alias_of.is_some() {
            continue;
        }
        let segments: Vec<_> = e.lines.iter().flat_map(|l| &l.segments).collect();
        let [segment] = segments.as_slice() else { continue };
        if !segment.env.is_empty() {
            continue;
        }
        // A test-named wrapper must not conceal a declared help/list/collect-only body.
        if verification_report(&segment.argv, "").is_none_or(|r| r.outcome != VerificationOutcome::Unknown) {
            continue;
        }
        let Some((fam, _)) = family(segment) else { continue };
        let (signature, sample) = match fam.as_str() {
            "py:pytest" => ("no tests ran", "no tests ran in 0.01s"),
            "js:vitest" => ("No test files found", "No test files found, exiting with code 1"),
            "js:jest" => ("No tests found", "No tests found, exiting with code 1"),
            _ => continue,
        };
        let body = segment.argv.join(" ");
        // The wrapper is eligible only when its invocation has the same recognized report
        // format as its one body command. Opaque make/just/custom wrappers abstain; the directly
        // declared runner may still bind. Inline interpreter programs and passive flags abstain.
        for command in e.command.iter().chain(std::iter::once(&body)) {
            let argv: Vec<String> = command.split(' ').map(str::to_string).collect();
            if !adage_tracks::advice::plain_command(command)
                || adage_tracks::io::classify(command, ctx.table) != Effect::Verify
                || verification_report(&argv, "").is_none_or(|r| r.outcome != VerificationOutcome::Unknown)
                || verification_report(&argv, sample).is_none_or(|r| r.outcome != VerificationOutcome::NoTests)
            {
                continue;
            }
            let Some(key) = command_key(command, ctx.table) else { continue };
            if !seen.insert((key.clone(), signature)) {
                continue;
            }
            d.advisories.push(AdvisoryCandidate {
                instinct: "U12".into(),
                template_id: "verification-discovery".into(),
                trigger: Trigger::AfterFailure { command: key.clone(), signature: signature.into() },
                slots: [("command".into(), command.clone())].into(),
                id_parts: vec![key, signature.into(), command.clone()],
                ecosystem: ecosystem_of_family(&fam),
                declared: Some(source_file(e.source).into()),
            });
        }
    }
    d
}

/// Optional recovery advice uses the same bounded declaration discovery, but only direct test
/// runners. Native runtime evidence, not a declaration or substring, gates actual delivery.
fn u13(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    let mut seen = BTreeSet::new();
    for mut a in u12(ctx).advisories {
        let Some(command) = a.slots.get("command") else { continue };
        let argv: Vec<String> = command.split(' ').map(str::to_string).collect();
        let recognized = ["1 failed in 0.01s", "Tests  1 failed (1)"]
            .iter()
            .any(|sample| adage_core::outcome::verification_test_attempt(&argv, command, sample, true).is_some());
        if !recognized || !seen.insert(command.clone()) {
            continue;
        }
        let Some(key) = command_key(command, ctx.table) else { continue };
        a.instinct = "U13".into();
        a.template_id = adage_tracks::recovery::TEMPLATE.into();
        a.trigger = Trigger::AfterCommand { command: key.clone() };
        a.id_parts = vec![key, command.clone()];
        d.advisories.push(a);
    }
    d
}

fn basename(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

/// The program a command runs (`npm ci` → `npm`).
fn basename_of(command: &str) -> &str {
    command.split(' ').next().map(basename).unwrap_or(command)
}

/// A verify runner's family and its bare spelling: `ACME_ENV=test python3 -m unittest discover`
/// → (`py:unittest`, `python3 -m unittest`).
pub fn family(seg: &Segment) -> Option<(String, String)> {
    let a = &seg.argv;
    let head = basename(a.first()?);
    match head {
        "python" | "python3" => {
            let i = a.iter().position(|t| t == "-m")?;
            let m = a.get(i + 1)?.as_str();
            ["pytest", "unittest"].contains(&m).then(|| (format!("py:{m}"), format!("{head} -m {m}")))
        }
        "pytest" => Some(("py:pytest".into(), "pytest".into())),
        "node" if a.iter().any(|t| t == "--test") => Some(("node:test".into(), "node --test".into())),
        "jest" | "vitest" | "mocha" => Some((format!("js:{head}"), head.to_string())),
        "npx" => {
            let t = a.get(1)?.as_str();
            ["jest", "vitest", "mocha"].contains(&t).then(|| (format!("js:{t}"), format!("npx {t}")))
        }
        _ => None,
    }
}

/// A segment's runner: its [`family`], or, for `python3 tools/testkit.py`, the family of a
/// repository script that drives a test runner itself ([`DeclaredFacts::script_runners`]). The
/// flag says the runner is reached through such a script (which is setup the bare runner lacks).
pub fn runner(seg: &Segment, facts: &DeclaredFacts) -> Option<(String, String, bool)> {
    if let Some((f, b)) = family(seg) {
        return Some((f, b, false));
    }
    let head = basename(seg.argv.first()?);
    if !["python", "python3"].contains(&head) {
        return None;
    }
    let script = seg.argv.get(1)?;
    let fam = facts.script_runners.get(script.strip_prefix("./").unwrap_or(script))?;
    let module = fam.strip_prefix("py:")?;
    Some((fam.clone(), format!("{head} -m {module}"), true))
}

const LOADER_FLAGS: [&str; 3] = ["--import", "--loader", "--experimental-loader"];
const REQUIRE_FLAGS: [&str; 2] = ["--require", "-r"];
const SETUP_FLAGS: [&str; 12] = [
    "--env-file",
    "--conditions",
    "-c",
    "--config",
    "--config-file",
    "-p",
    "--ds",
    "--rootdir",
    "-o",
    "--setupFiles",
    "--settings",
    "--confcutdir",
];

fn flag_name(t: &str) -> &str {
    t.split_once('=').map(|(a, _)| a).unwrap_or(t)
}

/// A declared entry that runs a verify runner with setup the bare runner lacks.
#[derive(Debug, Clone)]
pub struct Wrapper<'a> {
    pub entry: &'a Entry,
    pub family: String,
    pub bare: String,
    pub env: Vec<String>,
    pub flags: Vec<String>,
}

impl Wrapper<'_> {
    /// What the bare runner's failure prints when it failed for want of what the wrapper sets, one
    /// signature per route: each env variable the wrapper sets (a guard names it); else the
    /// module-resolution error of a missing `--import`/`--require` loader, and the wrapper's own
    /// command (the repository's failure message tells the agent to run it). Never empty: an
    /// assertion failure names none of these.
    pub fn signatures(&self) -> Vec<String> {
        let mut out: Vec<String> = self.env.iter().take(MAX_SIGNATURES).cloned().collect();
        if self.flags.iter().any(|f| LOADER_FLAGS.contains(&f.as_str())) {
            out.push("ERR_MODULE_NOT_FOUND".into());
        }
        if self.flags.iter().any(|f| REQUIRE_FLAGS.contains(&f.as_str())) {
            out.push("Cannot find module".into());
        }
        // A missing loader may fail in the repository's own words instead (`the test fixtures are
        // not loaded, this suite runs through npm run check`). An env guard names its variable.
        if self.env.is_empty() {
            out.extend(self.entry.command.iter().filter_map(|c| mention(c)));
        }
        out
    }
}

/// At most this many env-variable signatures per wrapper.
pub const MAX_SIGNATURES: usize = 3;

/// A directed command as a failure signature: the failure's own output tells the agent to run it
/// (`run the suite with 'make check'`). Only a command of two words or more, so the signature is
/// never a bare program name that output mentions in passing.
pub fn mention(command: &str) -> Option<String> {
    (command.contains(' ') && command.len() >= 6 && adage_tracks::io::safe_text(command)).then(|| command.to_string())
}

/// What a shell prints when `program` is not installed: bash (`pytest: command not found`), then
/// zsh (`command not found: pytest`).
pub fn not_found(program: &str) -> [String; 2] {
    [format!("{program}: command not found"), format!("command not found: {program}")]
}

/// What a Python runner's failure prints when it ran outside the project's environment: the
/// runner itself missing (`pytest: command not found`, `No module named pytest`, `python: command
/// not found`), or the project's own package not importable (`No module named 'click'`). None of
/// these is printed by a test that fails on its assertions, or by a mistake in the code under
/// test: the project's package is never missing from its own environment.
pub fn environment_signatures(probe: &str, facts: &DeclaredFacts) -> Vec<String> {
    let mut out = runner_missing_signatures(probe);
    if ["pytest", "python", "python3"].contains(&basename_of(probe)) {
        out.extend(facts.python.packages.iter().map(|p| format!("No module named '{p}'")));
    }
    out
}

/// What a runner's failure prints when the runner itself is not installed (the first half of
/// [`environment_signatures`]): any command that does not run that runner answers it.
pub fn runner_missing_signatures(probe: &str) -> Vec<String> {
    let seg = crate::safety::segments(probe).into_iter().next();
    let argv = seg.map(|s| s.argv).unwrap_or_default();
    let head = argv.first().map(|h| basename(h)).unwrap_or("");
    let module = argv.iter().position(|t| t == "-m").and_then(|i| argv.get(i + 1)).map(String::as_str);
    let mut out: Vec<String> = Vec::new();
    match (head, module) {
        ("pytest", _) | ("tox", _) | ("nox", _) => out.extend(not_found(head)),
        ("python" | "python3", Some("pytest")) => {
            out.push("No module named pytest".into());
            if head == "python" {
                out.extend(not_found("python"));
            }
        }
        ("python" | "python3", Some(_)) if head == "python" => out.extend(not_found("python")),
        _ => {}
    }
    out
}

/// Whether any of `entries` runs the runner family `fam` (bare or not).
fn runs_family(entries: &[&Entry], fam: &str) -> bool {
    entries.iter().flat_map(|e| e.lines.iter().flat_map(|l| &l.segments)).filter_map(family).any(|(f, _)| f == fam)
}

/// Whether an entry runs its tests inside a managed environment (`uv run`, `poetry run`, tox, a
/// virtual environment's own interpreter), so it works where the bare runner lacks the
/// environment.
fn managed_environment(e: &Entry) -> bool {
    const MANAGERS: [&str; 8] = ["uv", "uvx", "poetry", "pdm", "hatch", "pipenv", "tox", "nox"];
    e.source == Source::Tox
        || e.lines.iter().flat_map(|l| &l.segments).any(|s| {
            s.argv
                .first()
                .is_some_and(|h| MANAGERS.contains(&basename(h)) || h.starts_with(".venv/") || h.starts_with("venv/"))
        })
}

/// Distinct, in order.
fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    v.into_iter().filter(|x| !x.is_empty() && seen.insert(x.clone())).collect()
}

/// Names a repository gives its main verification entry point. A wrapper with any other name
/// (`test-legacy`, `bench`, `e2e`) is special-purpose, and never explains a failure (decoys).
pub const CANONICAL_CHECKS: [&str; 10] =
    ["test", "tests", "check", "ci", "verify", "unit", "unittest", "test:unit", "test-unit", "unit-test"];

fn name_rank(n: &str) -> usize {
    CANONICAL_CHECKS.iter().position(|c| *c == n).unwrap_or(CANONICAL_CHECKS.len())
}

/// The best directable wrapper per runner family: a Makefile target or package.json script with a
/// canonical check name ([`CANONICAL_CHECKS`]) whose body is plain, classifies at verify or below,
/// and runs the runner with env vars or setup flags.
pub fn wrappers<'a>(facts: &'a DeclaredFacts) -> BTreeMap<String, Wrapper<'a>> {
    let mut all: Vec<Wrapper> = Vec::new();
    for e in &facts.entries {
        // An alias (`"test": "npm run check"`) defers to what it runs, when that is directable.
        let defers = e.alias_of.as_ref().is_some_and(|a| {
            facts.entry(e.source, a).is_some_and(|t| t.command.is_some() && CANONICAL_CHECKS.contains(&a.as_str()))
        });
        if e.command.is_none()
            || e.effect().is_none_or(|x| rank(x) > rank(Effect::Verify))
            || !CANONICAL_CHECKS.contains(&e.name.as_str())
            || defers
        {
            continue;
        }
        for l in &e.lines {
            for s in &l.segments {
                let Some((fam, bare, via_script)) = runner(s, facts) else { continue };
                let flags: Vec<String> = s
                    .argv
                    .iter()
                    .map(|t| flag_name(t).to_string())
                    .filter(|f| {
                        LOADER_FLAGS.contains(&f.as_str())
                            || REQUIRE_FLAGS.contains(&f.as_str())
                            || SETUP_FLAGS.contains(&f.as_str())
                    })
                    .collect();
                // A runner script of the repository's own is setup in itself.
                if s.env.is_empty() && flags.is_empty() && !via_script {
                    continue;
                }
                all.push(Wrapper { entry: e, family: fam, bare, env: s.env.clone(), flags });
            }
        }
    }
    all.sort_by(|a, b| {
        (&a.family, name_rank(&a.entry.name), a.entry.body.len(), a.entry.source, &a.entry.name).cmp(&(
            &b.family,
            name_rank(&b.entry.name),
            b.entry.body.len(),
            b.entry.source,
            &b.entry.name,
        ))
    });
    let mut best = BTreeMap::new();
    for w in all {
        best.entry(w.family.clone()).or_insert(w);
    }
    best
}

/// The runner families an entry's body runs without any setup (bare).
fn families_run_bare(e: &Entry) -> Vec<String> {
    e.lines
        .iter()
        .flat_map(|l| &l.segments)
        .filter(|s| {
            s.env.is_empty()
                && !s.argv.iter().any(|t| {
                    let f = flag_name(t);
                    LOADER_FLAGS.contains(&f) || REQUIRE_FLAGS.contains(&f) || SETUP_FLAGS.contains(&f)
                })
        })
        .filter_map(family)
        .map(|(f, _)| f)
        .collect()
}

/// Whether an entry's body runs `family` without any setup (the bare runner, declared).
fn runs_bare(e: &Entry, fam: &str) -> bool {
    families_run_bare(e).iter().any(|f| f == fam)
}

fn dead_end(
    instinct: &str,
    runner: &str,
    signature: String,
    command: &str,
    effect: Effect,
    summary: &str,
    ctx: &Ctx,
) -> Option<RouteCandidate> {
    let rkey = command_key(runner, ctx.table)?;
    let wkey = command_key(command, ctx.table)?;
    (rkey != wkey).then(|| RouteCandidate {
        instinct: instinct.into(),
        id: IdHint::DeadEnd { wkey },
        trigger: Trigger::AfterFailure { command: rkey, signature },
        command: command.into(),
        effect,
        exit: Exit::IntentChanges,
        summary: summary.into(),
        runner: Some(runner.into()),
        ecosystem: None,
        declared: None,
    })
}

/// U1: a declared wrapper explains a failure.
fn u1(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    for (fam, w) in wrappers(ctx.facts) {
        let (Some(cmd), Some(effect)) = (&w.entry.command, w.entry.effect()) else { continue };
        let summary = "Instinct U1: a declared script runs this repository's test runner with the setup it needs";
        // Declared entries that run the same runner bare (`npm test` → `node --test`) first: they
        // are what an agent reaches for, so the first takes the learner's id. Then the bare runner.
        let mut runners: Vec<String> = Vec::new();
        for e in &ctx.facts.entries {
            if let Some(c) = &e.command {
                if c != cmd && e.effect().is_some() && runs_bare(e, &fam) {
                    runners.push(c.clone());
                }
            }
        }
        runners.push(w.bare.clone());
        let sigs = w.signatures();
        if sigs.is_empty() {
            d.skipped.push(("U1".into(), format!("{cmd}: no failure signature says the bare runner lacks its setup")));
        }
        for r in runners {
            for sig in &sigs {
                // A package manager echoes the script it runs: its output names a mentioned command
                // on every failure (see U3).
                if Some(sig) == mention(cmd).as_ref() && ["npm", "pnpm", "yarn", "bun"].contains(&basename_of(&r)) {
                    continue;
                }
                if let Some(c) = dead_end("U1", &r, sig.clone(), cmd, effect, summary, ctx) {
                    d.routes.push(c.from(ecosystem_of_family(&fam), Some(source_file(w.entry.source).into())));
                }
            }
        }
    }
    d
}

/// P1: pytest is configured, and a declared wrapper runs it with what it needs: every spelling of
/// pytest leads to the wrapper. A Makefile or package.json wrapper binds on its own signatures
/// (U1's); tox (`tox -e py`, when its base test environment runs plain commands) binds on the
/// signatures of a missing environment. nox is reported, never bound: its sessions are code.
fn p1(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    let f = ctx.facts;
    let py = &f.python;
    if !py.pytest_configured {
        return d;
    }
    let summary = "Instinct P1: pytest runs through this repository's declared wrapper";
    let ws = wrappers(f);
    if let Some(w) = ws.get("py:pytest") {
        let (Some(cmd), Some(effect)) = (&w.entry.command, w.entry.effect()) else { return d };
        for p in &ctx.cat.probes.pytest {
            for sig in w.signatures() {
                if let Some(c) = dead_end("P1", p, sig, cmd, effect, summary, ctx) {
                    d.routes.push(c.from(Some("Python"), Some(source_file(w.entry.source).into())));
                }
            }
        }
        return d;
    }
    if let Some(e) = f.entries.iter().find(|e| e.source == Source::Tox) {
        match (&e.command, e.effect()) {
            (Some(cmd), Some(effect)) if rank(effect) <= rank(Effect::Verify) => {
                for p in &ctx.cat.probes.pytest {
                    for sig in environment_signatures(p, f) {
                        if let Some(c) = dead_end("P1", p, sig, cmd, effect, summary, ctx) {
                            d.routes.push(c.from(Some("Python"), Some(tox_file(f))));
                        }
                    }
                }
            }
            _ => d.skipped.push((
                "P1".into(),
                format!("tox wraps pytest here, but {}", refusal(e).unwrap_or_else(|| "cannot be directed".into())),
            )),
        }
    }
    if let Some(e) = f.entries.iter().find(|e| e.source == Source::Nox) {
        d.skipped.push(("P1".into(), format!("nox wraps pytest here, but {}", refusal(e).unwrap_or_default())));
    }
    d
}

/// The file that declares tox's environments.
fn tox_file(f: &DeclaredFacts) -> String {
    if f.python.tox_ini {
        "tox.ini".into()
    } else {
        "pyproject.toml".into()
    }
}

/// U11: use the project's environment. A Python project with its own virtual environment (made
/// but not activated), or a uv project: after pytest (any spelling), tox or nox fails because it
/// ran outside that environment ([`environment_signatures`]), run pytest inside it:
/// `.venv/bin/python -m pytest`, else `uv run pytest` (`uv run --group tests pytest` when pytest is
/// only in a non-default group).
fn u11(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    let f = ctx.facts;
    let py = &f.python;
    if !(py.pyproject || py.pytest_configured) {
        return d;
    }
    let (command, declared) = match (&py.venv, &py.uv_pytest_group) {
        (Some(v), _) if py.pytest_configured || py.uv_pytest_group.is_some() => {
            (format!("{v}/bin/python -m pytest"), format!("{v}/pyvenv.cfg"))
        }
        (_, Some(g)) if py.uv => (
            if g.is_empty() { "uv run pytest".to_string() } else { format!("uv run --group {g} pytest") },
            if f.lockfiles.iter().any(|(p, _)| p == "uv.lock") { "uv.lock".into() } else { "pyproject.toml".into() },
        ),
        (None, None) if py.uv => {
            d.skipped.push(("U11".into(), "a uv project, but no dependency group has pytest".into()));
            return d;
        }
        _ => return d,
    };
    let effect = adage_tracks::io::classify(&command, ctx.table);
    if rank(effect) > rank(Effect::Verify) {
        d.skipped.push(("U11".into(), format!("{command} classifies as {}", effect.as_str())));
        return d;
    }
    let mut probes: Vec<String> = ctx.cat.probes.pytest.clone();
    if py.tox {
        probes.push("tox".into());
    }
    if py.nox {
        probes.push("nox".into());
    }
    let summary = "Instinct U11: the project's environment has what this runner needs";
    for p in &probes {
        for sig in environment_signatures(p, f) {
            if let Some(c) = dead_end("U11", p, sig, &command, effect, summary, ctx) {
                d.routes.push(c.from(Some("Python"), Some(declared.clone())));
            }
        }
    }
    // Any other Python run (`python -c "import click"`, `python repro.py`) that cannot import the
    // project's own package or a declared test dependency ran outside the project's environment
    // too: say which interpreter has them. Advice, not a run step: running an interpreter is not
    // a step Adage may direct (it classifies as environment). The pytest routes above answer a
    // pytest run's failure first (`adage_runtime`: advice after a failure only when no run step
    // answered it).
    let interpreter = match command.strip_suffix(" -m pytest") {
        Some(venv_python) => venv_python.to_string(),
        None => format!("{}python", command.strip_suffix("pytest").unwrap_or("uv run ")),
    };
    if adage_tracks::advice::project_interpreter(&interpreter) {
        let modules = dedup(py.packages.iter().chain(&py.test_deps).cloned().collect());
        for probe in PYTHON_PROBES {
            let Some(rkey) = command_key(probe, ctx.table) else { continue };
            for m in &modules {
                d.advisories.push(AdvisoryCandidate {
                    instinct: "U11".into(),
                    template_id: "project-python".into(),
                    trigger: Trigger::AfterFailure {
                        command: rkey.clone(),
                        signature: format!("No module named '{m}'"),
                    },
                    slots: [("interpreter".to_string(), interpreter.clone())].into_iter().collect(),
                    id_parts: vec![probe.to_string(), m.to_string()],
                    ecosystem: Some("Python"),
                    declared: Some(declared.clone()),
                });
            }
        }
    }
    d
}

/// Bare interpreters whose any run (`python repro.py`, `python3 -c ...`) U11 answers when it
/// cannot import the project or its test dependencies.
pub const PYTHON_PROBES: [&str; 2] = ["python", "python3"];

/// N1: package-manager discipline in a pnpm, yarn or bun repository.
fn n1(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    let f = ctx.facts;
    let Some(pm) = &f.package_manager else { return d };
    if pm.name == "npm" || pm.declared_by == "default" {
        return d;
    }
    if f.package_manager_conflict {
        d.skipped.push(("N1".into(), "the lockfiles and packageManager field name different package managers".into()));
        return d;
    }
    let install = format!("{} install", pm.name);
    let install_effect = adage_tracks::io::classify(&install, ctx.table);
    let declared_pm =
        if pm.declared_by == "package-manager-field" { "package.json".to_string() } else { pm.declared_by.clone() };
    // What each other tool prints when it is the wrong package manager here. Never a bare
    // non-zero exit: an install that fails on the network, or a test that fails on its
    // assertions, fails the same way under the right package manager.
    let mut npm_wrong: Vec<String> = Vec::new();
    if f.dev_engines_manager.as_deref() == Some(pm.name.as_str()) {
        npm_wrong.push("EBADDEVENGINES".into());
    }
    let mut npm_install_wrong = npm_wrong.clone();
    if f.non_npm_protocol {
        npm_install_wrong.push("Unsupported URL Type".into());
    }
    if f.only_allow.as_deref() == Some(pm.name.as_str()) {
        // only-allow prints: Use "pnpm install" for installation in this project.
        npm_install_wrong.push("for installation in this project".into());
    }
    // Corepack refuses a package manager the packageManager field does not name; or it is not
    // installed at all.
    let other_wrong = |tool: &str| -> Vec<String> {
        let mut v: Vec<String> = not_found(tool).into();
        if pm.declared_by == "package-manager-field" {
            v.push(format!("configured to use {}", pm.name));
        }
        v
    };
    let others: Vec<&str> = ["pnpm", "yarn"].into_iter().filter(|t| *t != pm.name).collect();
    let mut npm_ci_wrong = npm_install_wrong.clone();
    // `npm ci` needs a package-lock.json, which a repository of another package manager lacks.
    npm_ci_wrong.push("an existing package-lock.json".into());
    let mut installs: Vec<(String, Vec<String>)> =
        vec![("npm install".into(), npm_install_wrong), ("npm ci".into(), npm_ci_wrong)];
    installs.extend(others.iter().map(|t| (format!("{t} install"), other_wrong(t))));
    for (trig, sigs) in installs {
        let Some(tkey) = command_key(&trig, ctx.table) else { continue };
        if sigs.is_empty() {
            d.skipped.push(("N1".into(), format!("{trig}: nothing here makes it refuse to install")));
        }
        for sig in sigs {
            d.routes.push(RouteCandidate {
                instinct: "N1".into(),
                id: IdHint::Own { tag: "n1".into(), parts: vec![tkey.clone(), install.clone()] },
                trigger: Trigger::AfterFailure { command: tkey.clone(), signature: sig },
                command: install.clone(),
                effect: install_effect,
                exit: Exit::IntentChanges,
                summary: format!("Instinct N1: this repository installs with {}, not {}", pm.name, basename_of(&trig)),
                runner: Some(trig.clone()),
                ecosystem: Some("Node.js"),
                declared: Some(declared_pm.clone()),
            });
        }
    }
    if let Some(e) = f.entry(Source::PackageJson, "test") {
        match (&e.command, e.effect()) {
            (Some(cmd), Some(eff)) if rank(eff) <= rank(Effect::Verify) => {
                let summary = format!("Instinct N1: this repository runs its tests with {}", pm.name);
                // `npm test` runs the same script as `pnpm test` and fails the same way, unless npm
                // itself refuses to run here.
                let mut tests: Vec<(String, Vec<String>)> = vec![("npm test".into(), npm_wrong)];
                tests.extend(others.iter().map(|t| (format!("{t} test"), other_wrong(t))));
                for (trig, sigs) in tests {
                    if sigs.is_empty() {
                        d.skipped.push((
                            "N1".into(),
                            format!("{trig} runs the same script as {cmd}, and nothing here makes npm refuse to"),
                        ));
                    }
                    for sig in sigs {
                        if let Some(c) = dead_end("N1", &trig, sig, cmd, eff, &summary, ctx) {
                            d.routes.push(c.from(Some("Node.js"), Some("package.json".into())));
                        }
                    }
                }
            }
            (Some(cmd), _) => d.skipped.push((
                "N1".into(),
                format!("{cmd}: {}", refusal(e).unwrap_or_else(|| "classifies above verify".into())),
            )),
            _ => {}
        }
    }
    let lockfile = if pm.declared_by == "package-manager-field" {
        f.lockfiles.iter().find(|(_, m)| *m == pm.name).map(|(p, _)| p.clone())
    } else {
        Some(pm.declared_by.clone())
    };
    if let (Some(lockfile), Some(tkey)) = (lockfile, command_key("npm install", ctx.table)) {
        d.advisories.push(AdvisoryCandidate {
            instinct: "N1".into(),
            template_id: "package-manager".into(),
            trigger: Trigger::AfterCommand { command: tkey },
            slots: [("manager".to_string(), pm.name.clone()), ("lockfile".to_string(), lockfile.clone())]
                .into_iter()
                .collect(),
            id_parts: vec!["npm install".into(), pm.name.clone()],
            ecosystem: Some("Node.js"),
            declared: Some(lockfile),
        });
    }
    d
}

fn refusal(e: &Entry) -> Option<String> {
    match &e.verdict {
        crate::declared::Verdict::Refused { reason } => Some(format!("body refused: {reason}")),
        _ => None,
    }
}

fn is_test_entry(e: &Entry) -> bool {
    match e.source {
        Source::Makefile | Source::Justfile => e.name == "test" || e.name == "tests",
        Source::PackageJson | Source::Taskfile => e.name == "test",
        Source::Tox | Source::Nox => true,
    }
}

/// U3: the repository's one declared test command, after an undeclared runner fails.
fn u3(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    let f = ctx.facts;
    let points: Vec<&Entry> = f.entries.iter().filter(|e| is_test_entry(e)).collect();
    let chosen = match points.len() {
        0 => return d,
        1 => points[0],
        n => {
            // CI may say which one the repository really uses.
            let in_ci: Vec<&&Entry> = points
                .iter()
                .filter(|e| {
                    e.command
                        .as_ref()
                        .is_some_and(|c| f.ci_commands.iter().any(|x| x == c || x.starts_with(&format!("{c} "))))
                })
                .collect();
            if in_ci.len() == 1 {
                in_ci[0]
            } else {
                let names: Vec<String> = points.iter().map(|e| format!("{:?} {}", e.source, e.name)).collect();
                d.skipped.push(("U3".into(), format!("{n} test entry points ({})", names.join(", "))));
                return d;
            }
        }
    };
    let (Some(cmd), Some(eff)) = (&chosen.command, chosen.effect()) else {
        let why = refusal(chosen).unwrap_or_else(|| format!("{:?} {} cannot be directed", chosen.source, chosen.name));
        d.skipped.push(("U3".into(), why));
        return d;
    };
    if rank(eff) > rank(Effect::Verify) {
        d.skipped.push(("U3".into(), format!("{cmd} classifies as {}", eff.as_str())));
        return d;
    }
    // A declared test command that runs its runner bare, where a wrapper supplies the setup, is
    // the dead end U1 explains; the wrapper is the working command.
    let families: BTreeSet<String> =
        chosen.lines.iter().flat_map(|l| &l.segments).filter_map(|s| runner(s, f)).map(|(x, ..)| x).collect();
    let ws = wrappers(f);
    // An alias (`"test": "npm run check"`) is directed as what it runs.
    let target = chosen.alias_of.as_ref().and_then(|a| f.entry(chosen.source, a));
    let wrapper = families.iter().find_map(|fam| ws.get(fam).filter(|_| runs_bare(chosen, fam)));
    let (command, effect, via) = match wrapper {
        Some(w) => match (&w.entry.command, w.entry.effect()) {
            (Some(c), Some(e)) => (c.clone(), e, Some(w)),
            _ => (cmd.clone(), eff, None),
        },
        None => match target.and_then(|t| Some((t.command.clone()?, t.effect()?))) {
            Some((c, e)) if rank(e) <= rank(Effect::Verify) => (c, e, None),
            _ => (cmd.clone(), eff, None),
        },
    };
    // The declared test command may itself be the wrapper (`test: ACME_ENV=test python3 -m …`).
    let via = via.or_else(|| ws.values().find(|w| w.entry.command.as_deref() == Some(command.as_str())));
    let python = families.iter().any(|x| x.starts_with("py:"))
        || (chosen.source != Source::PackageJson && (f.python.pyproject || f.python.pytest_configured));
    // In a pnpm, yarn or bun repository another package manager's test is N1's to answer.
    let node = (families.iter().any(|x| x.starts_with("node:") || x.starts_with("js:"))
        || chosen.source == Source::PackageJson)
        && f.package_manager.as_ref().is_none_or(|pm| pm.name == "npm");
    let mut probes: Vec<&String> = Vec::new();
    if python {
        // In a pytest project an agent does not reach for unittest; a unittest failure there says
        // nothing the declared command answers.
        let pytest_project = f.python.pytest_configured && !families.iter().any(|x| x == "py:unittest");
        probes.extend(ctx.cat.probes.python.iter().filter(|p| !(pytest_project && p.contains("unittest"))));
    }
    if node {
        probes.extend(&ctx.cat.probes.node);
    }
    if probes.is_empty() {
        d.skipped
            .push(("U3".into(), format!("{command}: no undeclared runners are known for this repository's stack")));
    }
    let summary = "Instinct U3: this repository declares one test command";
    // A runner the test command itself runs bare is no undeclared attempt: the command would fail
    // the same way. (Not so inside a managed environment: tox's `pytest` runs in tox's.)
    let bare: BTreeSet<String> = f
        .entries
        .iter()
        .filter(|e| e.command.as_deref() == Some(command.as_str()) && !managed_environment(e))
        .flat_map(families_run_bare)
        .collect();
    for p in probes {
        let pf =
            family(&crate::safety::segments(p).into_iter().next().unwrap_or(Segment { env: vec![], argv: vec![] }));
        if pf.as_ref().is_some_and(|(fam, _)| bare.contains(fam)) {
            continue;
        }
        // The declared entry itself, when it only runs what is directed, works already.
        if chosen.alias_of.is_some() && p == cmd {
            continue;
        }
        // Only a failure that says the runner was the wrong way in: the setup a wrapper supplies,
        // a missing environment the declared command provides, or the repository's own message
        // naming the declared command. Never a test failing on its assertions.
        let mut sigs: Vec<String> = via.map(|w| w.signatures()).unwrap_or_default();
        let directed: Vec<&Entry> =
            f.entries.iter().filter(|e| e.command.as_deref() == Some(command.as_str())).collect();
        if directed.iter().any(|e| managed_environment(e)) {
            sigs.extend(environment_signatures(p, f));
        } else if !directed.is_empty() && pf.as_ref().is_some_and(|(fam, _)| !runs_family(&directed, fam)) {
            // The runner is missing and the declared command does not use it (the demo: pytest is
            // not installed, `make test` runs unittest).
            sigs.extend(runner_missing_signatures(p));
        }
        // A package manager echoes the script it runs (`> npm run check`), so a script's output
        // would name the directed command on every failure.
        sigs.extend(mention(&command));
        if ["npm", "pnpm", "yarn", "bun"].contains(&basename_of(p)) {
            sigs.retain(|s| Some(s) != mention(&command).as_ref());
        }
        for sig in dedup(sigs) {
            if let Some(c) = dead_end("U3", p, sig, &command, effect, summary, ctx) {
                let eco = if ctx.cat.probes.python.contains(p) { Some("Python") } else { Some("Node.js") };
                d.routes.push(c.from(eco, Some(source_file(chosen.source).into())));
            }
        }
    }
    d
}

/// U5: declared regeneration after an edit to the generator's inputs.
fn u5(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    for g in &ctx.facts.generators {
        let Some(e) = ctx.facts.entry(g.source, &g.entry) else { continue };
        let Some(effect) = e.effect() else { continue };
        for glob in &g.inputs {
            d.routes.push(RouteCandidate {
                instinct: "U5".into(),
                id: IdHint::FinishLine { glob: glob.clone() },
                trigger: Trigger::AfterEdit { paths: glob.clone() },
                command: g.command.clone(),
                effect,
                exit: Exit::ExploratoryChange,
                summary: format!(
                    "Instinct U5: a declared generator reads these files ({})",
                    if g.inputs_from == "declared" { "Makefile prerequisites" } else { "named in its script" }
                ),
                runner: None,
                ecosystem: None,
                declared: Some(source_file(g.source).into()),
            });
        }
    }
    d
}

/// At most this many generated-file advisories per repository.
pub const MAX_GENERATED_ADVISORIES: usize = 16;

/// U4: don't hand-edit generated files.
fn u4(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    let f = ctx.facts;
    // The command that regenerates Go code, if its directives are plain.
    let go_cmd = f.go.as_ref().filter(|g| !g.generate.is_empty() && g.generate_refused.is_none()).and_then(|_| {
        let cmd = "go generate ./...";
        (adage_tracks::io::classify(cmd, ctx.table) == Effect::Regenerate).then(|| cmd.to_string())
    });
    // Group by directory and extension where every such file is generated.
    let mut by_group: BTreeMap<(String, String), Vec<&crate::declared::Generated>> = BTreeMap::new();
    for g in &f.generated {
        let (dir, file) = g.path.rsplit_once('/').unwrap_or(("", &g.path));
        let ext = file.rsplit_once('.').map(|(_, e)| e).unwrap_or("").to_string();
        by_group.entry((dir.to_string(), ext)).or_default().push(g);
    }
    let mut out = Vec::new();
    for ((dir, ext), gs) in by_group {
        // A declared generator that writes the file, else `go generate` for Go code.
        let command = |g: &crate::declared::Generated| -> Option<String> {
            f.generators
                .iter()
                .find(|l| l.outputs.contains(&g.path))
                .map(|l| l.command.clone())
                .or_else(|| g.path.ends_with(".go").then(|| go_cmd.clone()).flatten())
        };
        // What declares the file generated: its generator's entry, else its own marker.
        let declared = |g: &crate::declared::Generated| -> String {
            f.generators
                .iter()
                .find(|l| l.outputs.contains(&g.path))
                .map(|l| source_file(l.source).to_string())
                .unwrap_or_else(|| g.path.clone())
        };
        let cmds: BTreeSet<Option<String>> = gs.iter().map(|g| command(g)).collect();
        if gs.len() >= 3 && cmds.len() == 1 && !ext.is_empty() {
            let glob = if dir.is_empty() { format!("*.{ext}") } else { format!("{dir}/*.{ext}") };
            let decl: BTreeSet<String> = gs.iter().map(|g| declared(g)).filter(|d| !d.contains('/')).collect();
            let decl = (decl.len() == 1).then(|| decl.into_iter().next()).flatten();
            out.push((glob, cmds.into_iter().next().flatten(), decl));
        } else {
            out.extend(gs.iter().map(|g| (g.path.clone(), command(g), Some(declared(g)))));
        }
    }
    if out.len() > MAX_GENERATED_ADVISORIES {
        d.skipped.push((
            "U4".into(),
            format!("{} generated paths; only the first {MAX_GENERATED_ADVISORIES} are advised", out.len()),
        ));
        out.truncate(MAX_GENERATED_ADVISORIES);
    }
    for (path, command, declared) in out {
        let mut slots: BTreeMap<String, String> = [("path".to_string(), path.clone())].into_iter().collect();
        if let Some(c) = command {
            slots.insert("command".into(), c);
        }
        d.advisories.push(AdvisoryCandidate {
            instinct: "U4".into(),
            template_id: "generated-file".into(),
            trigger: Trigger::AfterEdit { paths: path.clone() },
            slots,
            id_parts: vec![path],
            ecosystem: None,
            declared,
        });
    }
    d
}

/// U6: don't hand-edit lockfiles.
fn u6(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    for (path, manager) in &ctx.facts.lockfiles {
        d.advisories.push(AdvisoryCandidate {
            instinct: "U6".into(),
            template_id: "lockfile".into(),
            trigger: Trigger::AfterEdit { paths: path.clone() },
            slots: [("path".to_string(), path.clone()), ("manager".to_string(), manager.clone())].into_iter().collect(),
            id_parts: vec![path.clone()],
            ecosystem: ecosystem_of_manager(manager),
            declared: Some(path.clone()),
        });
    }
    d
}
