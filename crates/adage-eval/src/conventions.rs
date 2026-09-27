//! Ground truth for route learning: a sandbox's hidden conventions (`conventions.json`), and
//! `adage eval routes`, which scores the routes learned from a train split against them.
//!
//! ```json
//! {
//!   "schema": "adage-conventions/1",
//!   "sandbox": "acme-billing",
//!   "conventions": [
//!     {"id": "work-item-lookup", "kind": "lookup", "command": "python3 scripts/wi-lookup {id}",
//!      "id_pattern": "^W-\\d{4}$", "samples": ["W-1041"], "counter_samples": ["W-31001"]},
//!     {"id": "tests-through-make", "kind": "dead_end", "command": "make test",
//!      "avoided": ["python3 -m pytest"]},
//!     {"id": "regenerate-models", "kind": "finish_line", "command": "make gen",
//!      "glob": "schema/*.json"}
//!   ]
//! }
//! ```
//!
//! A learned route **matches** a convention when:
//!
//! - it is of the same kind (`lookup`, `linked_files`, `dead_end`, `finish_line`, `intent`);
//! - its command is the convention's `command`: the same canonical key as the scorer computes
//!   (`adage_runtime::command_key`), or the same command head where that head names a script
//!   (so `python3 scripts/wi-lookup` and `./scripts/wi-lookup` are one command, but
//!   `npm run check` and `npm run codegen` are not). Every `{slot}` is bound to the
//!   convention's first sample first. The command compared is the step's for a lookup, finish
//!   line or intent, the triggering command for linked files, and the recommended (`instead`)
//!   command for a dead end;
//! - a lookup's slot contracts accept every sample id and reject every counter-sample;
//! - every runner a dead end avoids starts with one of the convention's `avoided` commands;
//! - a finish line's glob covers the convention's: it matches every sandbox file the
//!   convention's glob matches (or, with no such file, the convention's glob text itself).
//!
//! Route precision is the share of learned routes that match some convention; route recall is
//! the share of conventions some learned route matches. Intent conventions are listed but not
//! counted in recall: offline learning has no embedding model, so it cannot learn them.

use crate::dataset::SandboxEntry;
use crate::score::{self, argv_runs, Expect};
use crate::{Options, Prepared};
use adage_core::CommandTable;
use adage_tracks::{Contract, Route, Step, Trigger};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

pub const SCHEMA: &str = "adage-conventions/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Lookup,
    LinkedFiles,
    DeadEnd,
    FinishLine,
    Intent,
}

impl Kind {
    /// The kind of a learned route, as [`score::kind`] names it.
    pub fn of_route(r: &Route) -> Option<Kind> {
        match score::kind(r) {
            "lookup" => Some(Kind::Lookup),
            "linked-files" => Some(Kind::LinkedFiles),
            "dead-end" => Some(Kind::DeadEnd),
            "finish-line" => Some(Kind::FinishLine),
            "intent" => Some(Kind::Intent),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Lookup => "lookup",
            Kind::LinkedFiles => "linked_files",
            Kind::DeadEnd => "dead_end",
            Kind::FinishLine => "finish_line",
            Kind::Intent => "intent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conventions {
    /// `adage-conventions/1`.
    pub schema: String,
    /// The sandbox (repository key) these conventions belong to.
    pub sandbox: String,
    #[serde(default)]
    pub description: String,
    pub conventions: Vec<Convention>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Convention {
    pub id: String,
    pub kind: Kind,
    /// The canonical command, as argv words separated by spaces. `{id}` stands for a sample id.
    /// Lookup: the lookup. Linked files: the command whose output lists them. Dead end: the
    /// command that works. Finish line: the step owed after the edit. Intent: the step.
    pub command: String,
    /// Dead end: the commands that fail (prefixes of argv, e.g. `python3 -m pytest`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avoided: Vec<String>,
    /// Finish line: the repository-relative glob whose edits create the obligation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    /// Lookup: the anchored regex every real id matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_pattern: Option<String>,
    /// Ids a learned contract must accept (they must match `id_pattern`). Also the value bound
    /// to `{id}` when commands are compared.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<String>,
    /// Ids a learned contract must reject (they must not match `id_pattern`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counter_samples: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

fn anchored(re: &str) -> Result<regex::Regex> {
    if !re.starts_with('^') || !re.ends_with('$') {
        bail!("pattern {re:?} must be anchored (^…$)");
    }
    regex::Regex::new(re).with_context(|| format!("pattern {re:?}"))
}

impl Conventions {
    pub fn load(path: &Path) -> Result<Conventions> {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        let c: Conventions = serde_json::from_str(&text).with_context(|| path.display().to_string())?;
        c.validate().with_context(|| path.display().to_string())?;
        Ok(c)
    }

    /// Check the file is well formed: every field its kind needs, and samples consistent with
    /// the id pattern.
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA {
            bail!("schema {:?}, expected {SCHEMA:?}", self.schema);
        }
        if self.conventions.is_empty() {
            bail!("no conventions");
        }
        let mut ids = BTreeSet::new();
        for c in &self.conventions {
            let ctx = || format!("convention {:?}", c.id);
            if c.id.is_empty() || !ids.insert(c.id.as_str()) {
                bail!("convention id {:?} is empty or repeated", c.id);
            }
            if c.command.split_whitespace().next().is_none() {
                bail!("{}: empty command", ctx());
            }
            if c.command.contains('{') && c.samples.is_empty() {
                bail!("{}: a command with a placeholder needs samples to bind it", ctx());
            }
            match c.kind {
                Kind::DeadEnd if c.avoided.iter().all(|a| a.trim().is_empty()) => {
                    bail!("{}: a dead end needs `avoided`", ctx())
                }
                Kind::FinishLine => {
                    let g = c.glob.as_deref().with_context(|| format!("{}: a finish line needs `glob`", ctx()))?;
                    glob::Pattern::new(g).with_context(|| format!("{}: glob {g:?}", ctx()))?;
                }
                Kind::Lookup => {
                    let p =
                        c.id_pattern.as_deref().with_context(|| format!("{}: a lookup needs `id_pattern`", ctx()))?;
                    let re = anchored(p).with_context(ctx)?;
                    if c.samples.is_empty() {
                        bail!("{}: a lookup needs samples", ctx());
                    }
                    if let Some(s) = c.samples.iter().find(|s| !re.is_match(s)) {
                        bail!("{}: sample {s:?} does not match its id_pattern", ctx());
                    }
                    if let Some(s) = c.counter_samples.iter().find(|s| re.is_match(s)) {
                        bail!("{}: counter-sample {s:?} matches its id_pattern", ctx());
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Bind every `{slot}` in a command to `value`.
fn bind(cmd: &str, value: Option<&str>) -> String {
    let Some(v) = value else { return cmd.to_string() };
    cmd.split(' ')
        .map(|w| if w.len() > 2 && w.starts_with('{') && w.ends_with('}') { v } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A trigger's command template (already a canonical key, e.g. `exec python3 scripts/x {id}`).
fn template_expect(template: &str) -> Expect {
    Expect { key: template.to_string() }
}

struct Ctx<'a> {
    table: &'a CommandTable,
    files: &'a [String],
}

/// Does `route` match convention `c`? `Err` says why not (for routes of the same kind).
fn matches(route: &Route, c: &Convention, cx: &Ctx) -> std::result::Result<(), String> {
    // Advice not to hand-edit a generated file, naming the command that regenerates it, is the
    // regeneration convention seen from the output's side.
    if let Step::Advise { template, slots } = &route.step {
        let cmd = slots.get("command").filter(|_| template == "generated-file" && c.kind == Kind::FinishLine);
        return match cmd {
            Some(cmd) if Expect::of(cmd, cx.table).same(&Expect::of(&c.command, cx.table)) => Ok(()),
            _ => Err("advice matches no other convention".into()),
        };
    }
    if Kind::of_route(route) != Some(c.kind) {
        return Err("another kind".into());
    }
    let sample = c.samples.first().map(String::as_str);
    let want = Expect::of(&bind(&c.command, sample), cx.table);
    let same = |got: &Expect, what: &str| {
        if got.same(&want) {
            Ok(())
        } else {
            Err(format!("{what} differs ({} vs {})", got.key, want.key))
        }
    };
    match (&route.trigger, &route.step) {
        (_, Step::Avoid { avoid, instead, .. }) => {
            same(&Expect::of(instead, cx.table), "working command")?;
            for runner in avoid.split(" / ") {
                let argv: Vec<String> = runner.split_whitespace().map(str::to_string).collect();
                if !c.avoided.iter().any(|a| argv_runs(&argv, a)) {
                    return Err(format!("avoids {runner:?}, which the convention does not"));
                }
            }
            Ok(())
        }
        // An instinct's dead end: after the avoided runner fails, run the working command.
        (Trigger::AfterFailure { command: key, .. }, Step::Run { command }) => {
            same(&Expect::of(command, cx.table), "working command")?;
            let argv: Vec<String> =
                key.strip_prefix("exec ").unwrap_or(key).split_whitespace().map(str::to_string).collect();
            if !c.avoided.iter().any(|a| argv_runs(&argv, a)) {
                return Err(format!("follows a failure of {key:?}, which the convention does not avoid"));
            }
            Ok(())
        }
        (_, Step::Advise { .. }) => Err("advice matches no convention".into()),
        (trigger, Step::ReadBatch { slot, .. }) => {
            let Trigger::AfterCommand { command } = trigger else {
                return Err("linked files not triggered by a command".into());
            };
            same(&template_expect(command), "triggering command")?;
            match route.slots.iter().find(|s| &s.name == slot).map(|s| &s.contract) {
                Some(Contract::RepoPath) => Ok(()),
                _ => Err(format!("slot {slot:?} is not a repository-path contract")),
            }
        }
        (Trigger::AfterEdit { paths }, Step::Run { command }) => {
            same(&Expect::of(command, cx.table), "command")?;
            let conv = c.glob.as_deref().unwrap_or_default();
            let (Ok(route_glob), Ok(conv_glob)) = (glob::Pattern::new(paths), glob::Pattern::new(conv)) else {
                return Err(format!("glob {paths:?} does not parse"));
            };
            let owed: Vec<&String> = cx.files.iter().filter(|f| conv_glob.matches(f)).collect();
            let covered =
                if owed.is_empty() { route_glob.matches(conv) } else { owed.iter().all(|f| route_glob.matches(f)) };
            if covered {
                Ok(())
            } else {
                Err(format!("glob {paths:?} does not cover {conv:?}"))
            }
        }
        (_, Step::Run { command }) => {
            same(&Expect::of(&bind(command, sample), cx.table), "command")?;
            if c.kind != Kind::Lookup {
                return Ok(());
            }
            let slots: Vec<&str> = command
                .split_whitespace()
                .filter_map(|w| w.strip_prefix('{').and_then(|w| w.strip_suffix('}')))
                .collect();
            for name in slots {
                let Some(Contract::Pattern { regex }) =
                    route.slots.iter().find(|s| s.name == name).map(|s| &s.contract)
                else {
                    return Err(format!("slot {name:?} has no pattern contract"));
                };
                let Ok(re) = regex::Regex::new(regex) else { return Err(format!("contract {regex:?} does not parse")) };
                if let Some(s) = c.samples.iter().find(|s| !re.is_match(s)) {
                    return Err(format!("contract {regex:?} rejects sample {s:?}"));
                }
                if let Some(s) = c.counter_samples.iter().find(|s| re.is_match(s)) {
                    return Err(format!("contract {regex:?} accepts counter-sample {s:?}"));
                }
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RouteMatch {
    pub id: String,
    pub kind: String,
    pub step: String,
    /// Conventions this route matches.
    pub matches: Vec<String>,
    /// Why it matches none of the conventions of its kind (`<convention>: <reason>`).
    pub misses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConventionMatch {
    pub id: String,
    pub kind: String,
    /// Learned routes that match it.
    pub matched_by: Vec<String>,
    /// Counted in recall (intent conventions are not: offline learning has no model).
    pub evaluable: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RoutesReport {
    pub dataset: String,
    pub split: String,
    pub thresholds: String,
    pub train_sessions: usize,
    pub test_sessions: usize,
    pub task_overlap: usize,
    pub task_overlap_ids: Vec<String>,
    pub leaky: bool,
    pub split_note: Option<String>,
    /// The conventions file, relative to the dataset.
    pub conventions_file: String,
    pub sandbox: String,
    pub index_generation: String,
    pub learned_routes: usize,
    /// Learned routes that match at least one convention.
    pub true_routes: usize,
    pub false_routes: Vec<String>,
    pub conventions_evaluable: usize,
    pub conventions_found: usize,
    pub missed_conventions: Vec<String>,
    pub precision: f64,
    pub precision_ci: [f64; 2],
    pub recall: f64,
    pub recall_ci: [f64; 2],
    pub f1: f64,
    pub routes: Vec<RouteMatch>,
    pub conventions: Vec<ConventionMatch>,
}

/// Score an index's routes for `repo` against `conv`. `files` are the sandbox's files.
pub fn score_routes(
    routes: &[&Route],
    conv: &Conventions,
    files: &[String],
    table: &CommandTable,
) -> (Vec<RouteMatch>, Vec<ConventionMatch>) {
    let cx = Ctx { table, files };
    let mut cms: Vec<ConventionMatch> = conv
        .conventions
        .iter()
        .map(|c| ConventionMatch {
            id: c.id.clone(),
            kind: c.kind.name().into(),
            matched_by: vec![],
            evaluable: c.kind != Kind::Intent,
        })
        .collect();
    let mut rms = Vec::new();
    for r in routes {
        let mut rm = RouteMatch {
            id: r.id.clone(),
            kind: score::kind(r).into(),
            step: crate::step_text(&r.step),
            matches: vec![],
            misses: vec![],
        };
        for (c, cm) in conv.conventions.iter().zip(cms.iter_mut()) {
            match matches(r, c, &cx) {
                Ok(()) => {
                    rm.matches.push(c.id.clone());
                    cm.matched_by.push(r.id.clone());
                }
                Err(why) if Kind::of_route(r) == Some(c.kind) => rm.misses.push(format!("{}: {why}", c.id)),
                Err(_) => {}
            }
        }
        if !rm.matches.is_empty() {
            rm.misses.clear();
        }
        rms.push(rm);
    }
    (rms, cms)
}

/// `adage eval routes <dataset>`: learn from the train split as `adage eval` does, then score
/// the learned routes against the dataset's conventions.
pub fn run(dir: &Path, opts: &Options) -> Result<RoutesReport> {
    score_prepared(&crate::prepare(dir, opts)?)
}

/// Score the routes of a prepared dataset's index against its conventions (the second half of
/// [`run`]).
pub fn score_prepared(p: &Prepared) -> Result<RoutesReport> {
    let Prepared { ds, split_name, split, thresholds, index, overlap, .. } = p;
    let file = ds.conventions_file()?.with_context(|| {
        format!("dataset {} has no conventions file (manifest `conventions`, or conventions.json)", ds.manifest.name)
    })?;
    let conv = Conventions::load(&file)?;
    if conv.sandbox != ds.manifest.repo {
        bail!("conventions are for {:?}, the dataset is {:?}", conv.sandbox, ds.manifest.repo);
    }
    let files: Vec<String> = ds
        .sandbox()?
        .into_iter()
        .map(|e| match e {
            SandboxEntry::File(p) => p,
            SandboxEntry::Symlink { path, .. } => path,
        })
        .filter(|p| crate::safe_rel(p))
        .collect();
    let table = CommandTable::load();
    let routes: Vec<&Route> = index.for_repo(&ds.manifest.repo).map(|ir| &ir.route).collect();
    let (rms, cms) = score_routes(&routes, &conv, &files, &table);
    let true_routes = rms.iter().filter(|r| !r.matches.is_empty()).count();
    let false_routes: Vec<String> = rms.iter().filter(|r| r.matches.is_empty()).map(|r| r.id.clone()).collect();
    let evaluable: Vec<&ConventionMatch> = cms.iter().filter(|c| c.evaluable).collect();
    let found = evaluable.iter().filter(|c| !c.matched_by.is_empty()).count();
    let missed: Vec<String> = evaluable.iter().filter(|c| c.matched_by.is_empty()).map(|c| c.id.clone()).collect();
    let (n_routes, n_conv) = (rms.len() as u32, evaluable.len() as u32);
    let precision = score::ratio(true_routes as u32, n_routes);
    let recall = score::ratio(found as u32, n_conv);
    let f1 = if precision + recall > 0.0 { score::round(2.0 * precision * recall / (precision + recall)) } else { 0.0 };
    let conventions_file = file
        .strip_prefix(&ds.dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "conventions.json".into());
    Ok(RoutesReport {
        dataset: ds.manifest.name.clone(),
        split: split_name.clone(),
        thresholds: thresholds.clone(),
        train_sessions: split.train.len(),
        test_sessions: split.test.len(),
        task_overlap: overlap.len(),
        leaky: !overlap.is_empty(),
        task_overlap_ids: overlap.clone(),
        split_note: split.note.clone(),
        conventions_file,
        sandbox: conv.sandbox.clone(),
        index_generation: index.generation.clone(),
        learned_routes: rms.len(),
        true_routes,
        false_routes,
        conventions_evaluable: evaluable.len(),
        conventions_found: found,
        missed_conventions: missed,
        precision,
        precision_ci: score::wilson(true_routes as u32, n_routes),
        recall,
        recall_ci: score::wilson(found as u32, n_conv),
        f1,
        routes: rms,
        conventions: cms,
    })
}

/// Human-readable routes report. Deterministic, like the JSON.
pub fn render_text(r: &RoutesReport) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "adage eval routes: {} (split {}, thresholds {}; conventions {}, {})",
        r.dataset, r.split, r.thresholds, r.conventions_file, r.sandbox
    );
    let _ = writeln!(s, "learned from {} train sessions ({} test sessions unused)", r.train_sessions, r.test_sessions);
    s.push_str(&crate::leakage_text(r.split_note.as_deref(), &r.task_overlap_ids, r.leaky));
    let _ = writeln!(s, "index {} with {} routes", r.index_generation, r.learned_routes);
    for x in &r.routes {
        let verdict = if x.matches.is_empty() { "FALSE".to_string() } else { format!("= {}", x.matches.join(", ")) };
        let _ = writeln!(s, "  {:<22} {:<12} {:<44} {}", x.id, x.kind, x.step, verdict);
        for m in &x.misses {
            let _ = writeln!(s, "      not {m}");
        }
    }
    let _ = writeln!(s, "\nconventions");
    for c in &r.conventions {
        let found = match (c.evaluable, c.matched_by.is_empty()) {
            (false, _) => "not learnable offline (no model)".to_string(),
            (true, true) => "MISSED".to_string(),
            (true, false) => format!("found by {}", c.matched_by.join(", ")),
        };
        let _ = writeln!(s, "  {:<22} {:<12} {}", c.id, c.kind, found);
    }
    let _ = writeln!(
        s,
        "\nroute precision {}/{} {:.1}% (95% CI {}); route recall {}/{} {:.1}% (95% CI {}); F1 {:.3}",
        r.true_routes,
        r.learned_routes,
        r.precision * 100.0,
        score::ci_text(r.precision_ci),
        r.conventions_found,
        r.conventions_evaluable,
        r.recall * 100.0,
        score::ci_text(r.recall_ci),
        r.f1
    );
    let none = || "none".to_string();
    let list = |v: &[String]| if v.is_empty() { none() } else { v.join(", ") };
    let _ = writeln!(s, "false routes: {}", list(&r.false_routes));
    let _ = writeln!(s, "missed conventions: {}", list(&r.missed_conventions));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_replaces_whole_placeholder_words_only() {
        assert_eq!(bind("python3 scripts/wi-lookup {item}", Some("W-1")), "python3 scripts/wi-lookup W-1");
        assert_eq!(bind("echo {}", Some("W-1")), "echo {}");
        assert_eq!(bind("make gen", None), "make gen");
    }

    #[test]
    fn validation_rejects_inconsistent_samples() {
        let mut c: Conventions = serde_json::from_value(serde_json::json!({
            "schema": SCHEMA, "sandbox": "x",
            "conventions": [{"id": "l", "kind": "lookup", "command": "tool {id}",
                             "id_pattern": "^W-\\d{4}$", "samples": ["W-1234"], "counter_samples": ["W-12"]}]
        }))
        .unwrap();
        c.validate().unwrap();
        c.conventions[0].samples.push("W-12345".into());
        assert!(c.validate().is_err());
        c.conventions[0].samples.pop();
        c.conventions[0].id_pattern = Some("W-\\d{4}".into());
        assert!(c.validate().is_err(), "unanchored");
    }

    fn route(id: &str, trigger: serde_json::Value, step: serde_json::Value, slots: serde_json::Value) -> Route {
        serde_json::from_value(serde_json::json!({
            "id": id, "summary": "s", "trigger": trigger, "step": step, "slots": slots,
            "effect": "observe", "aspect": "green", "state": "active",
            "evidence": {"days": 1, "deviations_failed": 0, "lower_bound": 0.5, "median_position": 0.0,
                         "models": [], "opportunities": 1, "provenance": "desire-path", "rate": 1.0,
                         "successes": 0, "support": 1, "turns_saved": 0.0}
        }))
        .unwrap()
    }

    fn acme() -> Conventions {
        serde_json::from_str(crate::generate::ACME_CONVENTIONS).unwrap()
    }

    fn verdicts(routes: &[Route]) -> Vec<(String, Vec<String>)> {
        let files: Vec<String> = crate::generate::ACME_FILES.iter().map(|s| s.to_string()).collect();
        let refs: Vec<&Route> = routes.iter().collect();
        let (rms, _) = score_routes(&refs, &acme(), &files, &CommandTable::load());
        rms.into_iter().map(|r| (r.id, r.matches)).collect()
    }

    #[test]
    fn near_misses_are_false_routes() {
        use serde_json::json;
        let contract = |re: &str| {
            json!([{"name": "item", "source": {"from": "prompt", "span": "ticket"},
                    "contract": {"kind": "pattern", "regex": re}}])
        };
        let lookup = |id: &str, cmd: &str, re: &str| {
            route(id, json!({"on": "prompt", "span": "ticket"}), json!({"do": "run", "command": cmd}), contract(re))
        };
        let routes = vec![
            lookup("ok", "./scripts/wi-lookup {item}", r"^W-\d{4}$"),
            lookup("narrow-contract", "python3 scripts/wi-lookup {item}", r"^W-10\d{2}$"),
            lookup("loose-contract", "python3 scripts/wi-lookup {item}", r"^W-\d+$"),
            lookup("other-command", "python3 scripts/other {item}", r"^W-\d{4}$"),
            route(
                "narrow-glob",
                json!({"on": "after_edit", "paths": "schema/invoice.json"}),
                json!({"do": "run", "command": "make gen"}),
                json!([]),
            ),
            route(
                "wrong-step",
                json!({"on": "after_edit", "paths": "schema/*.json"}),
                json!({"do": "run", "command": "make lint"}),
                json!([]),
            ),
            route(
                "broad-glob",
                json!({"on": "after_edit", "paths": "**/*.json"}),
                json!({"do": "run", "command": "make gen"}),
                json!([]),
            ),
            route(
                "avoids-working",
                json!({"on": "session_start"}),
                json!({"do": "avoid", "avoid": "python3 -m pytest / make ci", "instead": "make test", "signature": "x"}),
                json!([]),
            ),
            route(
                "session-hint",
                json!({"on": "session_start"}),
                json!({"do": "run", "command": "make test"}),
                json!([]),
            ),
        ];
        let got = verdicts(&routes);
        let matched: Vec<&str> = got.iter().filter(|(_, m)| !m.is_empty()).map(|(id, _)| id.as_str()).collect();
        // A glob broader than the convention's still covers it: coverage, not equality.
        assert_eq!(matched, ["ok", "broad-glob"], "{got:?}");
    }

    #[test]
    fn a_shared_npm_run_head_is_not_the_same_command() {
        let t = CommandTable::load();
        assert!(!Expect::of("npm run check", &t).same(&Expect::of("npm run codegen", &t)));
        assert!(Expect::of("python3 scripts/wi-lookup W-1", &t).same(&Expect::of("./scripts/wi-lookup W-2", &t)));
    }

    #[test]
    fn the_committed_conventions_files_are_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for f in ["demo/sandbox/conventions.json", "evals/live/sandboxes/harbor-dispatch/conventions.json"] {
            Conventions::load(&root.join(f)).unwrap_or_else(|e| panic!("{f}: {e:#}"));
        }
        // Every acme dataset carries an exact copy of the sandbox's file.
        for ds in ["demo-haiku45", "synthetic", "adversarial"] {
            let copy = std::fs::read_to_string(root.join("evals/datasets").join(ds).join("conventions.json")).unwrap();
            assert_eq!(copy, crate::generate::ACME_CONVENTIONS, "{ds}/conventions.json differs from demo/sandbox's");
        }
    }

    #[test]
    fn unknown_fields_are_refused() {
        let r: Result<Conventions, _> = serde_json::from_value(serde_json::json!({
            "schema": SCHEMA, "sandbox": "x",
            "conventions": [{"id": "l", "kind": "dead_end", "command": "make test", "avoid": ["pytest"]}]
        }));
        assert!(r.is_err());
    }
}
