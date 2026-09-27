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
pub const BIND_ORDER: [&str; 19] = [
    "U1", "P1", "N1", "U3", "U5", "U4", "U6", "U2", "U7", "U8", "U9", "U10", "U11", "N2", "N3", "R1", "R2", "G1", "X1",
];

/// How a candidate route's id is formed.
#[derive(Debug, Clone, PartialEq)]
pub enum IdHint {
    /// The dead-end miner's id for the working command `wkey`: `dead-end-<stable(wkey)>` for the
    /// first route directing it, `dead-end-<stable(wkey, trigger key)>` for further ones.
    DeadEnd { wkey: String },
    /// The finish-line miner's id: `finish-line-<stable(glob, command)>`.
    FinishLine { glob: String },
    /// No miner learns this shape: `instinct-<tag>-<stable(parts)>`.
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdvisoryCandidate {
    pub instinct: String,
    pub template_id: String,
    pub trigger: Trigger,
    pub slots: BTreeMap<String, String>,
    pub id_parts: Vec<String>,
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
        // Catalogued, not yet bindable: the model cannot express them (see docs/instincts.md).
        "U2" | "U7" | "U8" | "U9" | "U10" | "U11" | "N2" | "N3" | "R1" | "R2" | "G1" | "X1" => Detected::default(),
        other => Detected {
            skipped: vec![(other.to_string(), "no detector for this instinct".into())],
            ..Default::default()
        },
    }
}

fn basename(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
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
    /// What the failure of the bare runner names, if it names what the wrapper sets.
    pub fn signature(&self) -> String {
        if self.env.len() == 1 {
            return self.env[0].clone();
        }
        if self.flags.iter().any(|f| LOADER_FLAGS.contains(&f.as_str())) {
            return "ERR_MODULE_NOT_FOUND".into();
        }
        if self.flags.iter().any(|f| REQUIRE_FLAGS.contains(&f.as_str())) {
            return "Cannot find module".into();
        }
        String::new()
    }
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
    })
}

/// U1: a declared wrapper explains a failure.
fn u1(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    for (fam, w) in wrappers(ctx.facts) {
        let (Some(cmd), Some(effect)) = (&w.entry.command, w.entry.effect()) else { continue };
        let summary = "Instinct U1: a declared script runs this repository's test runner with the setup it needs";
        // Declared entries that run the same runner bare (`npm test` → `node --test`) first: they
        // are what an agent reaches for, so the first takes the miner's id. Then the bare runner.
        let mut runners: Vec<String> = Vec::new();
        for e in &ctx.facts.entries {
            if let Some(c) = &e.command {
                if c != cmd && e.effect().is_some() && runs_bare(e, &fam) {
                    runners.push(c.clone());
                }
            }
        }
        runners.push(w.bare.clone());
        for r in runners {
            if let Some(c) = dead_end("U1", &r, w.signature(), cmd, effect, summary, ctx) {
                d.routes.push(c);
            }
        }
    }
    d
}

/// P1: pytest is configured, and a wrapper sets what it needs: every spelling of pytest leads to
/// the wrapper. Wrappers Adage cannot direct (tox, nox, uv run) are reported, not bound.
fn p1(ctx: &Ctx) -> Detected {
    let mut d = Detected::default();
    let py = &ctx.facts.python;
    if !py.pytest_configured {
        return d;
    }
    let ws = wrappers(ctx.facts);
    match ws.get("py:pytest") {
        Some(w) => {
            let (Some(cmd), Some(effect)) = (&w.entry.command, w.entry.effect()) else { return d };
            for p in &ctx.cat.probes.pytest {
                let summary = "Instinct P1: pytest runs through this repository's declared wrapper";
                if let Some(c) = dead_end("P1", p, w.signature(), cmd, effect, summary, ctx) {
                    d.routes.push(c);
                }
            }
        }
        None => {
            if py.tox {
                d.skipped
                    .push(("P1".into(), "tox wraps pytest here, but tox is not in adage-core's command table".into()));
            }
            if py.nox {
                d.skipped
                    .push(("P1".into(), "nox wraps pytest here, but nox is not in adage-core's command table".into()));
            }
            if py.uv {
                d.skipped.push((
                    "P1".into(),
                    "uv run would wrap pytest, but uv is not in adage-core's command table".into(),
                ));
            }
        }
    }
    d
}

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
    for trig in ["npm install", "npm ci"] {
        let Some(tkey) = command_key(trig, ctx.table) else { continue };
        d.routes.push(RouteCandidate {
            instinct: "N1".into(),
            id: IdHint::Own { tag: "n1".into(), parts: vec![tkey.clone(), install.clone()] },
            trigger: Trigger::AfterFailure { command: tkey, signature: String::new() },
            command: install.clone(),
            effect: install_effect,
            exit: Exit::IntentChanges,
            summary: format!("Instinct N1: this repository installs with {}, not npm", pm.name),
            runner: Some(trig.into()),
        });
    }
    if let Some(e) = f.entry(Source::PackageJson, "test") {
        match (&e.command, e.effect()) {
            (Some(cmd), Some(eff)) if rank(eff) <= rank(Effect::Verify) => {
                let summary = format!("Instinct N1: this repository runs its tests with {}, not npm", pm.name);
                if let Some(c) = dead_end("N1", "npm test", String::new(), cmd, eff, &summary, ctx) {
                    d.routes.push(c);
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
            slots: [("manager".to_string(), pm.name.clone()), ("lockfile".to_string(), lockfile)].into_iter().collect(),
            id_parts: vec!["npm install".into(), pm.name.clone()],
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
    let (command, effect) = match families.iter().find_map(|fam| ws.get(fam).filter(|_| runs_bare(chosen, fam))) {
        Some(w) => match (&w.entry.command, w.entry.effect()) {
            (Some(c), Some(e)) => (c.clone(), e),
            _ => (cmd.clone(), eff),
        },
        None => match target.and_then(|t| Some((t.command.clone()?, t.effect()?))) {
            Some((c, e)) if rank(e) <= rank(Effect::Verify) => (c, e),
            _ => (cmd.clone(), eff),
        },
    };
    let python = families.iter().any(|x| x.starts_with("py:"))
        || (chosen.source != Source::PackageJson && (f.python.pyproject || f.python.pytest_configured));
    let node =
        families.iter().any(|x| x.starts_with("node:") || x.starts_with("js:")) || chosen.source == Source::PackageJson;
    let mut probes: Vec<&String> = Vec::new();
    if python {
        probes.extend(&ctx.cat.probes.python);
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
    // the same way.
    let bare: BTreeSet<String> = f
        .entries
        .iter()
        .filter(|e| e.command.as_deref() == Some(command.as_str()))
        .flat_map(families_run_bare)
        .collect();
    for p in probes {
        let pf =
            family(&crate::safety::segments(p).into_iter().next().unwrap_or(Segment { env: vec![], argv: vec![] }));
        if pf.is_some_and(|(fam, _)| bare.contains(&fam)) {
            continue;
        }
        // The declared entry itself, when it only runs what is directed, works already.
        if chosen.alias_of.is_some() && p == cmd {
            continue;
        }
        if let Some(c) = dead_end("U3", p, String::new(), &command, effect, summary, ctx) {
            d.routes.push(c);
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
        let cmds: BTreeSet<Option<String>> = gs.iter().map(|g| command(g)).collect();
        if gs.len() >= 3 && cmds.len() == 1 && !ext.is_empty() {
            let glob = if dir.is_empty() { format!("*.{ext}") } else { format!("{dir}/*.{ext}") };
            out.push((glob, cmds.into_iter().next().flatten()));
        } else {
            out.extend(gs.iter().map(|g| (g.path.clone(), command(g))));
        }
    }
    if out.len() > MAX_GENERATED_ADVISORIES {
        d.skipped.push((
            "U4".into(),
            format!("{} generated paths; only the first {MAX_GENERATED_ADVISORIES} are advised", out.len()),
        ));
        out.truncate(MAX_GENERATED_ADVISORIES);
    }
    for (path, command) in out {
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
        });
    }
    d
}
