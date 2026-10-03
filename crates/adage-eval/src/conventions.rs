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
//!   convention's glob matches (or, with no such file, the convention's glob text itself);
//! - (Track G G3) a **scope** route runs the convention's command after an edit of its glob, with
//!   the command's `{slot}` bound from a segment of the edited path (an `edit_segment` slot whose
//!   contract accepts every sample and rejects every counter-sample); an **env_sync** route runs
//!   the convention's environment step after an edit of its glob (it is nascent until a person
//!   approves it, as its ceiling requires); a **sequence** route runs the convention's `steps`, in
//!   order, after an edit of its glob.
//!
//! Two matches cross kinds: a sequence after an edit whose first step is a finish line's (or an
//! env_sync's) command delivers that convention (it directs the owed step, and more); and a finish line whose
//! command is a sequence convention's first step delivers part of it, so it is a true route (not
//! false) without recovering the sequence. A context route (Track G §6.2) fires on the event of
//! its last item, so it is scored as the one-event trigger on that item (an edit, a command, a
//! failure); one ending at a prompt has no such trigger and is listed apart, neither true nor
//! false.
//!
//! Route precision is the share of learned routes that match some convention; route recall is
//! the share of conventions some learned route matches. Intent conventions are listed but not
//! counted in recall: offline learning has no embedding model, so it cannot learn them.

use crate::dataset::SandboxEntry;
use crate::score::{self, argv_runs, Expect};
use crate::{Options, Prepared};
use adage_core::CommandTable;
use adage_tracks::{Contract, Route, SlotSource, Step, Trigger};
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
    /// A narrow step scoped to the part of the repository an edit touched (Track G §8).
    Scope,
    /// An environment step owed after a manifest edit (Track G §8).
    EnvSync,
    /// Steps run in order after an edit (Track G §8).
    Sequence,
}

impl Kind {
    /// The kind of convention a learned route's structure (its trigger and step) can deliver.
    /// Conventions are ground truth, written by kind; the learner knows none.
    pub fn of_route(r: &Route) -> Option<Kind> {
        let segment = r.slots.iter().any(|s| matches!(s.source, SlotSource::EditSegment { .. }));
        Some(match (&r.trigger, &r.step) {
            (_, Step::Sequence { .. }) => Kind::Sequence,
            (Trigger::AfterEdit { .. }, Step::Run { .. }) if segment => Kind::Scope,
            (Trigger::AfterEdit { .. }, Step::Run { .. }) if r.effect == adage_core::Effect::Environment => {
                Kind::EnvSync
            }
            (_, Step::Avoid { .. }) | (Trigger::AfterFailure { .. }, Step::Run { .. }) => Kind::DeadEnd,
            (_, Step::ReadBatch { .. }) => Kind::LinkedFiles,
            (_, Step::Advise { .. }) => return None,
            (Trigger::AfterEdit { .. }, _) => Kind::FinishLine,
            (Trigger::Prompt { .. }, _) => Kind::Lookup,
            (Trigger::Intent { .. }, _) => Kind::Intent,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Lookup => "lookup",
            Kind::LinkedFiles => "linked_files",
            Kind::DeadEnd => "dead_end",
            Kind::FinishLine => "finish_line",
            Kind::Intent => "intent",
            Kind::Scope => "scope",
            Kind::EnvSync => "env_sync",
            Kind::Sequence => "sequence",
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
    /// Commands the sandbox plants that are no convention (`evals/gen` decoys): a learned route
    /// directing one is a decoy hit, and false.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decoys: Vec<Decoy>,
}

/// A planted command that is no convention.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decoy {
    pub id: String,
    /// Always `decoy`.
    pub kind: String,
    /// How the generator planted it (`unused_target`, `lookalike_regen`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub command: String,
    #[serde(default)]
    pub notes: String,
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
    /// Sequence: the commands, in order (`command` is the first).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
    /// How a generator planted it (`evals/gen`: a dead end's `mode`, `instinct_violation` for the
    /// superseded universal default). Informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The failure (or staleness) signature the sandbox prints. Informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
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
                Kind::FinishLine | Kind::Scope | Kind::EnvSync | Kind::Sequence => {
                    let g =
                        c.glob.as_deref().with_context(|| format!("{}: a {} needs `glob`", ctx(), c.kind.name()))?;
                    glob::Pattern::new(g).with_context(|| format!("{}: glob {g:?}", ctx()))?;
                    if c.kind == Kind::Sequence && !(2..=3).contains(&c.steps.len()) {
                        bail!("{}: a sequence needs 2 to 3 `steps`", ctx());
                    }
                    if c.kind == Kind::Sequence && c.steps.first() != Some(&c.command) {
                        bail!("{}: a sequence's `command` is its first step", ctx());
                    }
                    if c.kind == Kind::Scope && !c.command.contains('{') {
                        bail!("{}: a scope's command has a `{{slot}}` for the edited part", ctx());
                    }
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
        for d in &self.decoys {
            if d.kind != "decoy" || d.id.is_empty() || !ids.insert(d.id.as_str()) {
                bail!("decoy {:?}: kind must be `decoy` and its id unique", d.id);
            }
            if d.command.split_whitespace().next().is_none() {
                bail!("decoy {:?}: empty command", d.id);
            }
        }
        Ok(())
    }
}

/// Learned routes that direct a decoy's command (`route: decoy`): each is a false route.
pub fn decoy_hits(routes: &[&Route], conv: &Conventions, table: &CommandTable) -> Vec<String> {
    let mut out = Vec::new();
    for r in routes {
        let mut cmds: Vec<&str> = match &r.step {
            Step::Run { command } => vec![command],
            Step::Sequence { commands } => commands.iter().map(String::as_str).collect(),
            Step::Avoid { instead, .. } => vec![instead],
            _ => vec![],
        };
        if let Trigger::AfterCommand { command } = &r.trigger {
            cmds.push(command.trim_start_matches("exec "));
        }
        for d in &conv.decoys {
            if cmds.iter().any(|c| Expect::of(c, table).same(&Expect::of(&d.command, table))) {
                out.push(format!("{}: {}", r.id, d.id));
            }
        }
    }
    out
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

/// How a route matches a convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Match {
    /// It delivers the convention: counted in recall.
    Full,
    /// It delivers part of it (a finish line that is a sequence's first step): a true route, not a
    /// recovery.
    Partial,
    /// It is the convention, but narrower than it: its contract accepts some of the convention's
    /// ids and none of its counter-samples, or its glob covers some of the files the convention
    /// owes. Fail-safe (it never fires where it should not), so not a false route; not a recovery
    /// either.
    Narrow,
}

/// Whether a route's glob covers a convention's: every sandbox file the convention's glob matches
/// (or, with none, the convention's glob text). Some but not all of them: [`Match::Narrow`].
fn covers(paths: &str, conv: &str, cx: &Ctx) -> std::result::Result<Match, String> {
    let (Ok(route_glob), Ok(conv_glob)) = (glob::Pattern::new(paths), glob::Pattern::new(conv)) else {
        return Err(format!("glob {paths:?} does not parse"));
    };
    let owed: Vec<&String> = cx.files.iter().filter(|f| conv_glob.matches(f)).collect();
    let hit = owed.iter().filter(|f| route_glob.matches(f)).count();
    if (owed.is_empty() && route_glob.matches(conv)) || (!owed.is_empty() && hit == owed.len()) {
        Ok(Match::Full)
    } else if hit > 0 {
        Ok(Match::Narrow)
    } else {
        Err(format!("glob {paths:?} does not cover {conv:?}"))
    }
}

/// Does `route` match convention `c`? `Err` says why not (for routes of the same kind).
fn matches(route: &Route, c: &Convention, cx: &Ctx) -> std::result::Result<Match, String> {
    // Advice not to hand-edit a generated file, naming the command that regenerates it, is the
    // regeneration convention seen from the output's side.
    if let Step::Advise { template, slots } = &route.step {
        let cmd = slots.get("command").filter(|_| template == "generated-file" && c.kind == Kind::FinishLine);
        return match cmd {
            Some(cmd) if Expect::of(cmd, cx.table).same(&Expect::of(&c.command, cx.table)) => Ok(Match::Full),
            _ => Err("advice matches no other convention".into()),
        };
    }
    let same_cmd = |a: &str, b: &str| Expect::of(a, cx.table).same(&Expect::of(b, cx.table));
    let conv_glob = c.glob.as_deref().unwrap_or_default();
    // Across kinds: a sequence whose first step is the owed finish-line step delivers it; a finish
    // line that is a sequence's first step delivers part of the sequence.
    match (&route.trigger, &route.step, c.kind, Kind::of_route(route)) {
        (Trigger::AfterEdit { paths }, Step::Sequence { commands }, Kind::FinishLine | Kind::EnvSync, _) => {
            let first = commands.first().map(String::as_str).unwrap_or_default();
            if !same_cmd(first, &c.command) {
                return Err(format!("first step {first:?} is not {:?}", c.command));
            }
            return covers(paths, conv_glob, cx);
        }
        (Trigger::AfterEdit { paths }, Step::Run { command }, Kind::Sequence, Some(Kind::FinishLine)) => {
            let first = c.steps.first().map(String::as_str).unwrap_or(&c.command);
            if !same_cmd(command, first) {
                return Err(format!("{command:?} is not the sequence's first step"));
            }
            return covers(paths, conv_glob, cx).map(|m| if m == Match::Full { Match::Partial } else { m });
        }
        _ => {}
    }
    if Kind::of_route(route) != Some(c.kind) {
        return Err("another kind".into());
    }
    match (c.kind, &route.trigger, &route.step) {
        (Kind::Sequence, Trigger::AfterEdit { paths }, Step::Sequence { commands }) => {
            if commands.len() != c.steps.len() || !commands.iter().zip(&c.steps).all(|(a, b)| same_cmd(a, b)) {
                return Err(format!("steps {commands:?} are not {:?}", c.steps));
            }
            return covers(paths, conv_glob, cx);
        }
        (Kind::Sequence, _, _) => return Err("a sequence convention follows an edit".into()),
        (Kind::EnvSync, Trigger::AfterEdit { paths }, Step::Run { command }) => {
            if !same_cmd(command, &c.command) {
                return Err(format!("command differs ({command} vs {})", c.command));
            }
            return covers(paths, conv_glob, cx);
        }
        (Kind::Scope, Trigger::AfterEdit { paths }, Step::Run { command }) => {
            let sample = c.samples.first().map(String::as_str);
            if !same_cmd(&bind(command, sample), &bind(&c.command, sample)) {
                return Err(format!("command differs ({command} vs {})", c.command));
            }
            let glob = covers(paths, conv_glob, cx)?;
            for s in route.slots.iter().filter(|s| matches!(s.source, SlotSource::EditSegment { .. })) {
                let Contract::Pattern { regex } = &s.contract else { return Err("not a pattern contract".into()) };
                let Ok(re) = regex::Regex::new(regex) else { return Err(format!("contract {regex:?} does not parse")) };
                if let Some(x) = c.samples.iter().find(|x| !re.is_match(x)) {
                    return Err(format!("contract {regex:?} rejects sample {x:?}"));
                }
                if let Some(x) = c.counter_samples.iter().find(|x| re.is_match(x)) {
                    return Err(format!("contract {regex:?} accepts counter-sample {x:?}"));
                }
            }
            return Ok(glob);
        }
        _ => {}
    }
    let sample = c.samples.first().map(String::as_str);
    let want = Expect::of(&bind(&c.command, sample), cx.table);
    // Set when the route is right but accepts less than the convention (fail-safe).
    let narrow = std::cell::Cell::new(false);
    let same = |got: &Expect, what: &str| {
        if got.same(&want) {
            Ok(())
        } else {
            Err(format!("{what} differs ({} vs {})", got.key, want.key))
        }
    };
    #[allow(clippy::redundant_closure_call)]
    let res = (|| -> std::result::Result<(), String> {
        match (&route.trigger, &route.step) {
            (_, Step::Card { .. }) => Err("a procedure card matches no convention".into()),
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
            (_, Step::Sequence { .. }) => Err("a sequence matches only sequence and finish-line conventions".into()),
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
                narrow.set(covers(paths, conv_glob, cx)? == Match::Narrow);
                Ok(())
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
                    let Ok(re) = regex::Regex::new(regex) else {
                        return Err(format!("contract {regex:?} does not parse"));
                    };
                    if let Some(s) = c.counter_samples.iter().find(|s| re.is_match(s)) {
                        return Err(format!("contract {regex:?} accepts counter-sample {s:?}"));
                    }
                    // Rejecting some of the convention's ids (but not all) is narrow, not false.
                    if c.samples.iter().all(|s| !re.is_match(s)) {
                        return Err(format!("contract {regex:?} rejects every sample"));
                    }
                    if c.samples.iter().any(|s| !re.is_match(s)) {
                        narrow.set(true);
                    }
                    // A learned value type binds with its matcher, not the regex (Track G §4.3): the
                    // matcher must say the same on the convention's ids.
                    let vtype = route.slots.iter().find(|s| s.name == name).and_then(|s| s.source.vtype());
                    if let Some(t) = vtype {
                        let ok = |v: &str| adage_core::vtype::matches(&t.runs, v);
                        if let Some(s) = c.counter_samples.iter().find(|s| ok(s)) {
                            return Err(format!("value type {} accepts counter-sample {s:?}", t.id));
                        }
                        if c.samples.iter().all(|s| !ok(s)) {
                            return Err(format!("value type {} rejects every sample", t.id));
                        }
                        if c.samples.iter().any(|s| !ok(s)) {
                            narrow.set(true);
                        }
                    }
                }
                Ok(())
            }
        }
    })();
    res.map(|_| if narrow.get() { Match::Narrow } else { Match::Full })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RouteMatch {
    pub id: String,
    pub kind: String,
    pub step: String,
    /// The route's state (a learned environment step is nascent until reviewed).
    pub state: String,
    /// Conventions this route matches.
    pub matches: Vec<String>,
    /// Conventions it delivers part of (a true route that recovers nothing).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partial: Vec<String>,
    /// Conventions it is too narrow for ([`Match::Narrow`]): fail-safe, so not false, but no
    /// recovery.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrow: Vec<String>,
    /// The route's strength (`fire`, `prime`, `inhibit`).
    pub strength: String,
    /// Why it matches none of the conventions of its kind (`<convention>: <reason>`).
    pub misses: Vec<String>,
}

impl RouteMatch {
    /// Not a false route: it matches a convention, delivers part of one, or is one too narrowly.
    pub fn is_true(&self) -> bool {
        !self.matches.is_empty() || !self.partial.is_empty() || !self.narrow.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConventionMatch {
    pub id: String,
    pub kind: String,
    /// Learned routes that match it.
    pub matched_by: Vec<String>,
    /// Learned routes that deliver part of it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partly_by: Vec<String>,
    /// Learned routes that are this convention, but too narrow ([`Match::Narrow`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrowly_by: Vec<String>,
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
    /// The index scored, when one was given (`--index`): nothing was learned from the dataset,
    /// so there are no train sessions and no leakage to report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_file: Option<String>,
    /// The conventions file, relative to the dataset.
    pub conventions_file: String,
    pub sandbox: String,
    pub index_generation: String,
    pub learned_routes: usize,
    /// Context routes ending at a prompt, which no convention's trigger describes: neither true
    /// nor false, and not in `learned_routes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inert_routes: Vec<String>,
    /// Learned routes that match at least one convention (or deliver part of one, or are one too
    /// narrowly).
    pub true_routes: usize,
    pub false_routes: Vec<String>,
    /// Routes that are a convention but narrower than it: counted true (fail-safe), not recalled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrow_routes: Vec<String>,
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
    /// Learned routes that direct a planted decoy's command ([`decoy_hits`]).
    #[serde(default)]
    pub decoy_hits: Vec<String>,
    /// Decoys the conventions file lists.
    #[serde(default)]
    pub decoys: usize,
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
            partly_by: vec![],
            narrowly_by: vec![],
            evaluable: c.kind != Kind::Intent,
        })
        .collect();
    let mut rms = Vec::new();
    for r in routes {
        let kind = Kind::of_route(r).map(|k| k.name().to_string()).unwrap_or_else(|| score::point(r));
        let mut rm = RouteMatch {
            id: r.id.clone(),
            kind,
            step: crate::step_text(&r.step),
            state: format!("{:?}", r.state).to_lowercase(),
            matches: vec![],
            partial: vec![],
            narrow: vec![],
            strength: format!("{:?}", r.strength).to_lowercase(),
            misses: vec![],
        };
        for (c, cm) in conv.conventions.iter().zip(cms.iter_mut()) {
            match matches(r, c, &cx) {
                Ok(Match::Full) => {
                    rm.matches.push(c.id.clone());
                    cm.matched_by.push(r.id.clone());
                }
                Ok(Match::Partial) => {
                    rm.partial.push(c.id.clone());
                    cm.partly_by.push(r.id.clone());
                }
                Ok(Match::Narrow) => {
                    rm.narrow.push(c.id.clone());
                    cm.narrowly_by.push(r.id.clone());
                }
                Err(why) if Kind::of_route(r) == Some(c.kind) => rm.misses.push(format!("{}: {why}", c.id)),
                Err(_) => {}
            }
        }
        if rm.is_true() {
            rm.misses.clear();
        }
        rms.push(rm);
    }
    (rms, cms)
}

/// The routes to score, and those listed apart: a context route is scored as the one-event
/// trigger on its last item (it fires on that item's event); one ending at a prompt is listed
/// apart.
fn live_and_inert(routes: &[&Route]) -> (Vec<Route>, Vec<String>) {
    let (mut live, mut inert) = (Vec::new(), Vec::new());
    for r in routes {
        let Trigger::Context { items, .. } = &r.trigger else {
            live.push((*r).clone());
            continue;
        };
        let last = match items.last() {
            Some(adage_tracks::CtxItem::Edit { glob }) => Some(Trigger::AfterEdit { paths: glob.clone() }),
            Some(adage_tracks::CtxItem::Act { keys }) => {
                keys.first().map(|k| Trigger::AfterCommand { command: k.clone() })
            }
            Some(adage_tracks::CtxItem::Fail { keys, signature }) => {
                keys.first().map(|k| Trigger::AfterFailure { command: k.clone(), signature: signature.clone() })
            }
            _ => None,
        };
        match last {
            Some(t) => live.push(Route { trigger: t, ..(*r).clone() }),
            None => inert.push(r.id.clone()),
        }
    }
    (live, inert)
}

/// `adage eval routes <dataset>`: learn from the train split as `adage eval` does, then score
/// the learned routes against the dataset's conventions.
pub fn run(dir: &Path, opts: &Options) -> Result<RoutesReport> {
    score_prepared(&crate::prepare(dir, opts)?)
}

impl Conventions {
    /// A generated bundle's ground truth: `evals/gen` writes `conventions.json` in this schema,
    /// decoys included, so it is read like any other.
    pub fn from_bundle(dir: &Path) -> Result<Conventions> {
        Conventions::load(&dir.join("conventions.json"))
    }
}

/// Repository files under `root` (relative, sorted; `.git` skipped).
fn repo_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if e.file_name() == ".git" {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => {
                    if let Ok(rel) = p.strip_prefix(root) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// `adage eval routes <gen-bundle>`: learn from every rehearsal transcript of a generated bundle
/// (`python3 evals/gen/rehearse.py <bundle> --no-learn` writes them) at the demo thresholds and no
/// instincts, as the rehearsal does, then score
/// every learned route **in any state but extinguished** against the bundle's conventions: an
/// environment step is nascent until a person approves it, and it is still what was learned.
pub fn run_bundle(dir: &Path) -> Result<RoutesReport> {
    let conv = Conventions::from_bundle(dir)?;
    let reh = dir.join("rehearsal");
    let mut transcripts: Vec<std::path::PathBuf> = Vec::new();
    for proj in std::fs::read_dir(reh.join("transcripts")).with_context(|| {
        format!("{}: no rehearsal transcripts (python3 evals/gen/rehearse.py <bundle> --no-learn)", dir.display())
    })? {
        for f in std::fs::read_dir(proj?.path())? {
            let f = f?.path();
            if f.extension().is_some_and(|x| x == "jsonl") {
                transcripts.push(f);
            }
        }
    }
    transcripts.sort();
    let outcomes: std::collections::BTreeMap<String, bool> = std::fs::read_to_string(reh.join("outcomes.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let cfg = adage_learn::LearnConfig::demo();
    let home = tempfile::tempdir()?;
    let inputs = adage_learn::Inputs {
        transcripts,
        outcomes,
        repo_override: Some(conv.sandbox.clone()),
        models: None,
        cache: None,
        only_repo: None,
    };
    let (tracks, index_path) = (home.path().join("tracks"), home.path().join("index.json"));
    let report = adage_learn::learn(&inputs, &tracks, &index_path, &cfg)?;
    let learned: Vec<Route> = adage_tracks::io::read_all(&tracks)?
        .into_iter()
        .flat_map(|t| t.routes)
        .filter(|r| r.state != adage_tracks::RouteState::Extinguished)
        .collect();
    let refs: Vec<&Route> = learned.iter().collect();
    let (live, inert_routes) = live_and_inert(&refs);
    let live: Vec<&Route> = live.iter().collect();
    let files = repo_files(&dir.join("repo"));
    let (rms, cms) = score_routes(&live, &conv, &files, CommandTable::load());
    let decoy_hits = decoy_hits(&live, &conv, CommandTable::load());
    let true_routes = rms.iter().filter(|r| r.is_true()).count();
    let false_routes: Vec<String> = rms.iter().filter(|r| !r.is_true()).map(|r| r.id.clone()).collect();
    let narrow_routes: Vec<String> =
        rms.iter().filter(|r| r.matches.is_empty() && !r.narrow.is_empty()).map(|r| r.id.clone()).collect();
    let evaluable: Vec<&ConventionMatch> = cms.iter().filter(|c| c.evaluable).collect();
    let found = evaluable.iter().filter(|c| !c.matched_by.is_empty()).count();
    let missed: Vec<String> = evaluable.iter().filter(|c| c.matched_by.is_empty()).map(|c| c.id.clone()).collect();
    let (n_routes, n_conv) = (rms.len() as u32, evaluable.len() as u32);
    let precision = score::ratio(true_routes as u32, n_routes);
    let recall = score::ratio(found as u32, n_conv);
    let f1 = if precision + recall > 0.0 { score::round(2.0 * precision * recall / (precision + recall)) } else { 0.0 };
    Ok(RoutesReport {
        dataset: dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        split: "all".into(),
        thresholds: "demo".into(),
        train_sessions: inputs.transcripts.len(),
        test_sessions: 0,
        task_overlap: 0,
        task_overlap_ids: vec![],
        leaky: false,
        split_note: Some("every rehearsal transcript; routes in any state but extinguished".into()),
        index_file: None,
        conventions_file: "conventions.json".into(),
        sandbox: conv.sandbox.clone(),
        index_generation: report.index_generation,
        learned_routes: rms.len(),
        inert_routes,
        true_routes,
        false_routes,
        narrow_routes,
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
        decoy_hits,
        decoys: conv.decoys.len(),
    })
}

/// Score the routes of a prepared dataset's index against its conventions (the second half of
/// [`run`]).
pub fn score_prepared(p: &Prepared) -> Result<RoutesReport> {
    let Prepared { ds, split_name, split, thresholds, index, overlap, index_file, .. } = p;
    // A given index learned nothing from this dataset: no train side, no leakage.
    let given = index_file.as_ref().map(|f| f.to_string_lossy().into_owned());
    let (train_sessions, overlap) =
        if given.is_some() { (0, Vec::new()) } else { (split.train.len(), overlap.clone()) };
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
    let (routes, inert_routes) = live_and_inert(&routes);
    let routes: Vec<&Route> = routes.iter().collect();
    let (rms, cms) = score_routes(&routes, &conv, &files, table);
    let decoy_hits = decoy_hits(&routes, &conv, table);
    let true_routes = rms.iter().filter(|r| r.is_true()).count();
    let false_routes: Vec<String> = rms.iter().filter(|r| !r.is_true()).map(|r| r.id.clone()).collect();
    let narrow_routes: Vec<String> =
        rms.iter().filter(|r| r.matches.is_empty() && !r.narrow.is_empty()).map(|r| r.id.clone()).collect();
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
        train_sessions,
        test_sessions: split.test.len(),
        task_overlap: overlap.len(),
        leaky: !overlap.is_empty(),
        task_overlap_ids: overlap,
        split_note: if given.is_some() { None } else { split.note.clone() },
        index_file: given,
        conventions_file,
        sandbox: conv.sandbox.clone(),
        index_generation: index.generation.clone(),
        learned_routes: rms.len(),
        inert_routes,
        true_routes,
        false_routes,
        narrow_routes,
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
        decoy_hits,
        decoys: conv.decoys.len(),
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
    match &r.index_file {
        Some(f) => {
            let _ = writeln!(s, "scored the given index {f}: nothing was conditioned on this dataset's traces");
        }
        None => {
            let _ = writeln!(
                s,
                "conditioned on {} train traces ({} test traces unused)",
                r.train_sessions, r.test_sessions
            );
            s.push_str(&crate::leakage_text(r.split_note.as_deref(), &r.task_overlap_ids, r.leaky));
        }
    }
    let _ = writeln!(s, "index {} with {}", r.index_generation, crate::count(r.learned_routes, "reflex"));
    for x in &r.routes {
        let verdict = match (x.matches.is_empty(), x.partial.is_empty(), x.narrow.is_empty()) {
            (false, _, _) => format!("= {}", x.matches.join(", ")),
            (true, false, _) => format!("part of {}", x.partial.join(", ")),
            (true, true, false) => format!("too narrow for {} (fail-safe)", x.narrow.join(", ")),
            (true, true, true) => "FALSE".to_string(),
        };
        let mut tags: Vec<&str> = Vec::new();
        if x.state != "firm" {
            tags.push(&x.state);
        }
        if x.strength != "fire" {
            tags.push(&x.strength);
        }
        let state = if tags.is_empty() { String::new() } else { format!(" [{}]", tags.join(", ")) };
        let _ = writeln!(s, "  {:<22} {:<12} {:<44} {}{state}", x.id, x.kind, x.step, verdict);
        for m in &x.misses {
            let _ = writeln!(s, "      not {m}");
        }
    }
    let _ = writeln!(s, "\nconventions");
    for c in &r.conventions {
        let found = match (c.evaluable, c.matched_by.is_empty()) {
            (false, _) => "cannot be conditioned offline (no intent model)".to_string(),
            (true, true) if !c.narrowly_by.is_empty() => {
                format!("MISSED (too narrow: {})", c.narrowly_by.join(", "))
            }
            (true, true) => "MISSED".to_string(),
            (true, false) => format!("found by {}", c.matched_by.join(", ")),
        };
        let _ = writeln!(s, "  {:<22} {:<12} {}", c.id, c.kind, found);
    }
    let _ = writeln!(
        s,
        "\nreflex precision {}/{} {:.1}% (95% CI {}); reflex recall {}/{} {:.1}% (95% CI {}); F1 {:.3}",
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
    let _ = writeln!(s, "false reflexes: {}", list(&r.false_routes));
    if !r.narrow_routes.is_empty() {
        let _ = writeln!(s, "too narrow (fail-safe, counted true, not recalled): {}", r.narrow_routes.join(", "));
    }
    if r.decoys > 0 {
        let _ = writeln!(s, "decoy hits ({} decoys): {}", r.decoys, list(&r.decoy_hits));
    }
    if !r.inert_routes.is_empty() {
        let _ = writeln!(s, "not scored (a context ending at a prompt): {}", r.inert_routes.join(", "));
    }
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
            "effect": "observe", "strength": "fire", "state": "firm",
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
        let (rms, _) = score_routes(&refs, &acme(), &files, CommandTable::load());
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

    /// A route that is the convention but accepts less (a contract that rejects some of its ids,
    /// a glob that covers some of its files) is fail-safe: "too narrow", never "false", and no
    /// recovery. One that also accepts a counter-sample, or rejects every id, is false.
    #[test]
    fn narrow_routes_are_reported_apart_from_false_ones() {
        use serde_json::json;
        let contract = |re: &str| {
            json!([{"name": "item", "source": {"from": "prompt", "span": "ticket"},
                    "contract": {"kind": "pattern", "regex": re}}])
        };
        let lookup = |id: &str, re: &str| {
            route(
                id,
                json!({"on": "prompt", "span": "ticket"}),
                json!({"do": "run", "command": "python3 scripts/wi-lookup {item}"}),
                contract(re),
            )
        };
        let routes = [
            lookup("narrow-contract", r"^W-10\d{2}$"),
            lookup("loose-contract", r"^W-\d+$"),
            lookup("disjoint-contract", r"^X-\d{4}$"),
            route(
                "narrow-glob",
                json!({"on": "after_edit", "paths": "schema/invoice.json"}),
                json!({"do": "run", "command": "make gen"}),
                json!([]),
            ),
        ];
        let files: Vec<String> = crate::generate::ACME_FILES.iter().map(|s| s.to_string()).collect();
        let refs: Vec<&Route> = routes.iter().collect();
        let (rms, cms) = score_routes(&refs, &acme(), &files, CommandTable::load());
        let narrow: Vec<&str> = rms.iter().filter(|r| !r.narrow.is_empty()).map(|r| r.id.as_str()).collect();
        assert_eq!(narrow, ["narrow-contract", "narrow-glob"], "{rms:?}");
        let false_ids: Vec<&str> = rms.iter().filter(|r| !r.is_true()).map(|r| r.id.as_str()).collect();
        assert_eq!(false_ids, ["loose-contract", "disjoint-contract"]);
        assert!(rms.iter().all(|r| r.matches.is_empty()), "narrow is no recovery");
        assert!(cms.iter().any(|c| c.narrowly_by == ["narrow-contract"]));
    }

    #[test]
    fn a_shared_npm_run_head_is_not_the_same_command() {
        let t = CommandTable::load();
        assert!(!Expect::of("npm run check", t).same(&Expect::of("npm run codegen", t)));
        assert!(Expect::of("python3 scripts/wi-lookup W-1", t).same(&Expect::of("./scripts/wi-lookup W-2", t)));
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
    fn a_given_index_is_reported_as_given_not_as_learned() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/datasets/live-20260928-acme-billing");
        let opts = crate::Options {
            split: Some("round1".into()),
            index: Some(dir.join("recorded/index-round1.json")),
            ..Default::default()
        };
        let r = run(&dir, &opts).unwrap();
        let t = render_text(&r);
        assert!(t.contains("scored the given index"), "{t}");
        assert!(!t.contains("train traces") && !t.contains("LEAKY") && !t.contains("\nengine "), "{t}");
        assert_eq!((r.train_sessions, r.leaky), (0, false));
        // Conditioning on the split still says so.
        let r = run(&dir, &crate::Options { split: Some("round1".into()), ..Default::default() }).unwrap();
        assert!(render_text(&r).contains("conditioned on 12 train traces"));
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
