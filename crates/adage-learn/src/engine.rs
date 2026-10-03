//! Learning inside `adage condition` (Track G): reflexes mined from the traces, compiled to delivery
//! shapes, judged on the traces. This is the only learner; it knows no route kinds.
//!
//! 1. **Mine** (G2): every trace becomes an event sequence ([`crate::bridge::events`]), with the
//!    events its directives named marked exposed (steering by influence, Track G §3.2);
//!    `adage_graph::select::learn` mines, scores and selects reflexes.
//! 2. **Compile** (G3, `adage_graph::compile`): each reflex becomes zero or more routes by its
//!    structure, with a stable, content-derived id (so `review.yaml` decisions survive) and a
//!    `why` block for people. Value slots (G1) give the lookup's typed slot, and prompt-intent
//!    prototypes (`adage_graph::compile::intent`) the intent routes.
//! 3. **Judge.** A compiled route's evidence is counted again on the traces exactly as its own
//!    trigger and step read them: the lookup through G1's value types
//!    ([`crate::graph::lookup_routes`]), a read batch over the traces that ran its command, an
//!    after-edit route over the traces that edited a file of its glob (run after the last such
//!    edit, contrast against the traces that did not), an after-command route over the traces
//!    that ran the command, an avoid over the traces that ran an avoided runner. Its strength is
//!    the Wilson rule over those counts (a lower bound for fire, a rate for prime, capped by the
//!    ceiling), with the outcome-lift path as a second gate. Context routes keep the reflex's own
//!    evidence. A trace that received a directive naming what a route directs is left out of its
//!    count ([`exposed`]). Routine execution candidates are separately recounted at every
//!    compiled trigger, including multi-event contexts: a compatible successful continuation
//!    must occur in a later model turn within eight events, before another mutation or task,
//!    with ≥90% observed support and the existing session/task support gates.
//!    Their certificate preserves executable knowledge without low-value advisory delivery.
//!
//! **Instincts as priors.** The repository's installed instinct routes are pseudo-evidence on the
//! reflex at the same decision point directing the same command: 3 traces at 0.7 in the prime
//! test only; fire reads the organic lower bound alone, so no reflex is delivered fire from
//! prior mass, and a reflex that says otherwise always wins (interlocking puts instincts last).

use crate::bridge;
use crate::evidence::{argv, days, median, models, outcome_lift, shell_join, strength_for, LearnConfig, PATHS_FIELD};
use crate::import::Exposure as _;
use crate::import::Session;
use adage_core::json::extract_json;
use adage_core::spans::{self, SpanKind};
use adage_core::{CanonicalAction, Effect};
use adage_graph::compile::{self, CompileParams, Corpus, Dropped, Prior};
use adage_graph::select::{self, Counts};
use adage_intent::model::StaticModel;
use adage_tracks::why::Why;
use adage_tracks::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// What learning did for one repository.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EngineReport {
    /// Traces mined.
    pub sessions: usize,
    pub reflexes: usize,
    pub compiled: usize,
    /// Routes that passed judgement.
    pub routes: usize,
    /// `routes` by where each came from (`mined pattern`, `value estimate`, `lookup`, `prompt
    /// intent`, `procedure card`): what `adage condition` sums its count from.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kept_by: BTreeMap<String, usize>,
    /// Routes compiled with a context trigger (matched at hook time).
    pub contexts: usize,
    /// Prompt-intent prototypes that cleared the purity bar (none without an embedding model).
    pub intent_families: usize,
    /// Families of similar tasks found, and procedure cards learned for them ([`crate::cards`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub card_families: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cards: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<Dropped>,
    /// Every safe candidate the value model weighed ([`crate::value`]), delivered or not.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub valued: Vec<crate::value::Valued>,
    /// The tests-of-the-edit route as planned ([`crate::testmap`]), delivered or not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testmap: Option<crate::testmap::Planned>,
    /// The session-start line as planned ([`crate::testmap::card`]), delivered or not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<crate::testmap::Card>,
    /// Candidates compiled (plan D1): sequences counted below the support a reflex needs, before
    /// the learner leaves out those that are reflexes.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub candidates: usize,
}

/// One judged route, and why it exists.
#[derive(Debug, Clone)]
pub struct Judged {
    pub route: Route,
    pub why: Why,
}

/// The result of learning for one repository.
#[derive(Debug, Clone, Default)]
pub struct EngineRun {
    pub routes: Vec<Judged>,
    pub report: EngineReport,
    /// Every scored candidate's counts by pattern (for dormant routes).
    pub counts: BTreeMap<String, Counts>,
    /// The candidates (plan D1): border patterns compiled as the reflex they would be, never
    /// judged (`compile::Compiled::candidate`), one per id. What `adage_tracks::candidates`
    /// records, once the learner has left out those that are reflexes.
    pub candidates: Vec<compile::Compiled>,
    /// Each judged route's supporting session ids, as mined (what tells which traces tipped a
    /// candidate into it).
    pub sessions: BTreeMap<String, Vec<String>>,
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8)
}

pub(crate) fn glob_hit(glob: &str, rel: &str) -> bool {
    adage_graph::select::glob_match(glob, rel) || glob::Pattern::new(glob).is_ok_and(|g| g.matches(rel))
}

/// The repository-relative path an edit step touched.
fn edited(st: &crate::import::StepObs) -> Option<&str> {
    adage_core::keys::is_author(&st.norm.action).then_some(st.rel_path.as_deref()).flatten()
}

fn ok_literal(st: &crate::import::StepObs) -> Option<String> {
    if st.call.failed {
        return None;
    }
    argv(st).map(|av| shell_join(av))
}

/// The commands a route directs or names: its step's commands, an avoid's runners, and the
/// runner whose failure an after-failure route follows. Reviewed for `Step::Skill` (D3): a skill
/// step names no command (`compile::commands` gives none), so only a directive that named
/// everything exposes a session to it.
pub fn named_commands(r: &Route) -> Vec<String> {
    let mut v: Vec<String> = compile::commands(&r.step).into_iter().map(str::to_string).collect();
    if let Step::Avoid { avoid, .. } = &r.step {
        v.extend(avoid.split(" / ").map(str::to_string));
    }
    if let Trigger::AfterFailure { command, .. } = &r.trigger {
        v.push(command.trim_start_matches("exec ").to_string());
    }
    v
}

/// Whether trace `s` received a directive naming what `r` directs (Track G §3.2): it is then no
/// evidence for `r`.
pub fn exposed(s: &Session, r: &Route) -> bool {
    let cmds = named_commands(r);
    let refs: Vec<&str> = cmds.iter().map(String::as_str).collect();
    // A skill route: a directive that said to use that skill (and only that one) steered it.
    let skill = match &r.step {
        Step::Skill { skill, .. } => s.influence.iter().any(|d| d.skills.iter().any(|x| x == skill)),
        _ => false,
    };
    skill || s.exposed_to(&refs, matches!(r.step, Step::ReadBatch { .. }))
}

/// The commands a step ran successfully, each with its own effect class: the step's command, or
/// every part of a compound one (`cargo fmt --all && cargo test --workspace`). A failed compound
/// is failed in every part (which part failed is not recorded).
fn ok_parts(st: &crate::import::StepObs) -> Vec<(String, Effect)> {
    fn walk(n: &adage_core::Normalized, out: &mut Vec<(String, Effect)>) {
        match &n.action {
            CanonicalAction::Exec { argv, .. } if !argv.is_empty() => {
                out.push((shell_join(argv), bridge::effect_of(n)));
            }
            CanonicalAction::Compound { actions, .. } => actions.iter().for_each(|a| walk(a, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    if !st.call.failed {
        walk(&st.norm, &mut out);
    }
    out
}

/// Whether a step runs a command at all (a compound one included).
fn runs_command(st: &crate::import::StepObs) -> bool {
    match &st.norm.action {
        CanonicalAction::Exec { .. } => true,
        CanonicalAction::Compound { actions, .. } => {
            actions.iter().any(|a| matches!(a.action, CanonicalAction::Exec { .. }))
        }
        _ => false,
    }
}

/// A directed command (or skill), read once for matching observed runs of its family.
struct Want {
    command: String,
    shape: Option<adage_core::pattern::Shape>,
    effect: Effect,
    /// A skill to use, not a command: its Skill tool use is the hit.
    skill: Option<String>,
}

impl Want {
    fn new(command: String) -> Want {
        let shape = compile::variants::shape(&command);
        let effect = io::classify(&command, &adage_core::CommandTable::load());
        Want { command, shape, effect, skill: None }
    }

    /// Using the skill `name` (a `Step::Skill`, judged like a run: plan §2.1.2).
    fn skill(name: &str) -> Want {
        Want { command: adage_tracks::skill_key(name), shape: None, effect: Effect::Observe, skill: Some(name.into()) }
    }

    /// What `step` directs, in order, with `{part}` bound by `bind`.
    fn of(step: &Step, bind: impl Fn(&str) -> String) -> Vec<Want> {
        match step {
            Step::Skill { skill, .. } => vec![Want::skill(skill)],
            _ => compile::commands(step).into_iter().map(|c| Want::new(bind(c))).collect(),
        }
    }

    /// Whether step `st` ran this command or a variant it covers (`compile::variants`: an
    /// extension with options, or, for a repo-wide command, a narrowing of the same effect):
    /// traces that ran `cargo test -p adage-cli` after an edit are evidence for
    /// `cargo test --workspace --locked`, the family's covering form.
    fn hit(&self, st: &crate::import::StepObs) -> bool {
        if let Some(name) = &self.skill {
            return !st.call.failed
                && matches!(&st.norm.action, CanonicalAction::Control { tool } if tool.strip_prefix("Skill ") == Some(name.as_str()));
        }
        ok_parts(st).iter().any(|(lit, e)| {
            *lit == self.command
                || (st.project_pytest_root_cwd
                    && adage_core::execution_equivalence::project_pytest_equivalent(&self.command, lit))
                || self.shape.as_ref().is_some_and(|w| {
                    compile::variants::shape(lit).is_some_and(|v| compile::variants::covers(w, &v, *e == self.effect))
                })
        })
    }
}

/// Whether `steps` ran what `step` directs after position `from` (a sequence in order), with
/// `{part}` bound to `part`.
fn ran_after(steps: &[crate::import::StepObs], from: usize, step: &Step, part: Option<&str>) -> bool {
    let bind = |c: &str| part.map_or(c.to_string(), |p| c.replace("{part}", p));
    let want: Vec<Want> = Want::of(step, bind);
    let mut i = 0;
    for st in steps.iter().skip(from + 1) {
        if i < want.len() && want[i].hit(st) {
            i += 1;
        }
    }
    !want.is_empty() && i == want.len()
}

/// Whether the first run of `want` after position `from` recovered from a failed check: a
/// different check failed after the edit and before it (`python -m pytest` fails, then
/// `make test`). That run answers the failure, a dead end's evidence, not the edit.
fn recovered(steps: &[crate::import::StepObs], from: usize, want: &Want) -> bool {
    let Some(hit) = steps.iter().enumerate().skip(from + 1).find(|(_, st)| want.hit(st)).map(|(i, _)| i) else {
        return false;
    };
    let runner = |st: &crate::import::StepObs| argv(st).map(|a| crate::evidence::runner(a));
    let own = runner(&steps[hit]);
    steps[from + 1..hit]
        .iter()
        .any(|st| st.call.failed && st.norm.effect == Effect::Verify && runner(st).is_some() && runner(st) != own)
}

/// The traces in which `command` (or a variant it covers) ran successfully within five steps of a
/// different check's failure: the recovery `count_avoid` counts for a dead end.
fn recovery_traces(command: &str, ss: &[&Session]) -> u32 {
    let want = Want::new(command.to_string());
    let runner = |st: &crate::import::StepObs| argv(st).map(|a| crate::evidence::runner(a));
    let own = compile::variants::shape(command).map(|s| s.identity.join(" "));
    ss.iter()
        .filter(|s| {
            s.steps.iter().enumerate().any(|(i, st)| {
                st.call.failed
                    && st.norm.effect == Effect::Verify
                    && runner(st).is_some_and(|r| Some(&r) != own.as_ref() && !command.starts_with(&r))
                    && s.steps.iter().skip(i + 1).take(5).any(|b| want.hit(b))
            })
        })
        .count() as u32
}

struct Count {
    opportunities: u32,
    /// The traces where the trigger occurred (for the outcome lift).
    open: BTreeSet<String>,
    support: BTreeSet<String>,
    rate_without: f64,
    /// (ran the step, traces) among the traces the trigger did not occur in.
    without: (u32, u32),
    deviations_failed: u32,
}

/// An after-edit route over the traces (see the module documentation).
fn count_after_edit(r: &Route, sessions: &[&Session]) -> Option<Count> {
    let Trigger::AfterEdit { paths } = &r.trigger else { return None };
    let depth = r.slots.iter().find_map(|s| match &s.source {
        SlotSource::EditSegment { depth, .. } => Some(*depth as usize),
        _ => None,
    });
    let want_first = Want::of(&r.step, str::to_string).into_iter().next()?;
    let first = want_first.command.clone();
    // A plain check after an edit: its run is evidence only when the edit, not a failed check,
    // called for it.
    let check = r.effect == Effect::Verify && r.slots.is_empty() && matches!(r.step, Step::Run { .. });
    let (mut opp, mut support, mut ran_without, mut not_editing, mut failed) = (0, BTreeSet::new(), 0u32, 0u32, 0u32);
    let mut open = BTreeSet::new();
    for s in sessions {
        let last = s
            .steps
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(i, st)| edited(st).map(|p| (i, p)))
            .find(|(_, p)| glob_hit(paths, p));
        match last {
            Some((i, p)) => {
                opp += 1;
                open.insert(s.id.clone());
                let part = depth.and_then(|d| p.split('/').filter(|x| !x.is_empty()).nth(d));
                if depth.is_some() && part.is_none() {
                    continue;
                }
                if ran_after(&s.steps, i, &r.step, part) && !(check && recovered(&s.steps, i, &want_first)) {
                    support.insert(s.id.clone());
                } else if s.outcome == Some(false) {
                    failed += 1;
                }
            }
            None => {
                not_editing += 1;
                let head = first.split('{').next().unwrap_or_default().to_string();
                let ran = if depth.is_some() {
                    s.steps.iter().flat_map(ok_parts).any(|(l, _)| l.starts_with(&head))
                } else {
                    s.steps.iter().any(|st| want_first.hit(st))
                };
                ran_without += u32::from(ran);
            }
        }
    }
    let rate_without = if not_editing == 0 { 0.0 } else { ran_without as f64 / not_editing as f64 };
    Some(Count {
        opportunities: opp,
        open,
        support,
        rate_without,
        without: (ran_without, not_editing),
        deviations_failed: failed,
    })
}

/// The share of an after-edit sequence's traces that must run its steps one right after another
/// for it to be delivered beside an obligation owing its first step.
const SEQUENCE_TOGETHER: f64 = 0.75;

/// Of the traces that ran an after-edit sequence's steps in order after their last matching edit,
/// how many ran each later step as the very next command after the one before it: (together, of).
fn together(r: &Route, sessions: &[&Session]) -> (u32, u32) {
    let (Trigger::AfterEdit { paths }, Step::Sequence { commands }) = (&r.trigger, &r.step) else { return (0, 0) };
    let (mut together, mut of) = (0, 0);
    for s in sessions {
        let Some(last) = s.steps.iter().rposition(|st| edited(st).is_some_and(|p| glob_hit(paths, p))) else {
            continue;
        };
        // Each step's position, found in order.
        let mut at = Vec::new();
        let mut from = last + 1;
        for c in commands {
            let want = Want::new(c.clone());
            match s.steps.iter().enumerate().skip(from).find(|(_, st)| want.hit(st)) {
                Some((i, _)) => {
                    at.push(i);
                    from = i + 1;
                }
                None => break,
            }
        }
        if at.len() != commands.len() {
            continue;
        }
        of += 1;
        let next_command =
            |i: usize| s.steps.iter().enumerate().skip(i + 1).find(|(_, st)| runs_command(st)).map(|(j, _)| j);
        together += u32::from(at.windows(2).all(|w| next_command(w[0]) == Some(w[1])));
    }
    (together, of)
}

/// An after-command run over the traces: those that ran the command, and those that then ran the
/// directed one within the next eight steps.
fn count_after_command(r: &Route, sessions: &[&Session]) -> Option<Count> {
    let Trigger::AfterCommand { command: key } = &r.trigger else { return None };
    let (mut opp, mut support, mut open) = (0, BTreeSet::new(), BTreeSet::new());
    for s in sessions {
        let Some(i) = s.steps.iter().position(|st| &st.key == key && !st.call.failed) else { continue };
        opp += 1;
        open.insert(s.id.clone());
        if ran_after(&s.steps[..(i + 9).min(s.steps.len())], i, &r.step, None) {
            support.insert(s.id.clone());
        }
    }
    Some(Count { opportunities: opp, open, support, rate_without: 0.0, without: (0, 0), deviations_failed: 0 })
}

/// An avoid over the traces: those that ran an avoided runner (the opportunities), and those
/// where a run of it failed and the working command then succeeded within five steps (support).
fn count_avoid(runners: &[&str], instead: &str, sessions: &[&Session]) -> Count {
    let (mut opp, mut support) = (0, BTreeSet::new());
    for s in sessions {
        let runs: Vec<usize> = s
            .steps
            .iter()
            .enumerate()
            .filter(|(_, st)| argv(st).is_some_and(|av| runners.contains(&crate::evidence::runner(av).as_str())))
            .map(|(i, _)| i)
            .collect();
        if runs.is_empty() {
            continue;
        }
        opp += 1;
        // The recovery is the first check that then passed (within five steps): a later check
        // that also passed is not what replaced the runner.
        let recovered = runs.iter().any(|&i| {
            let failed = &s.steps[i];
            failed.call.failed
                && s.steps
                    .iter()
                    .skip(i + 1)
                    .take(5)
                    .find(|b| {
                        argv(b).is_some() && !b.call.failed && b.norm.effect == Effect::Verify && b.key != failed.key
                    })
                    .and_then(ok_literal)
                    .as_deref()
                    == Some(instead)
        });
        if recovered {
            support.insert(s.id.clone());
        }
    }
    Count {
        opportunities: opp,
        open: BTreeSet::new(),
        support,
        rate_without: 0.0,
        without: (0, 0),
        deviations_failed: 0,
    }
}

/// A read batch over the traces: those whose run of `key` listed files (in JSON, or repository
/// paths in its text) are the opportunities, those that then read at least half of them within
/// sixteen steps the support. The field most supporting runs listed them in is the slot's.
pub fn judge_read_batch(r: &Route, sessions: &[&Session], cfg: &LearnConfig) -> Option<Route> {
    let Trigger::AfterCommand { command: key } = &r.trigger else { return None };
    let mut opportunities = 0u32;
    let mut open: BTreeSet<String> = BTreeSet::new();
    let mut support: BTreeSet<String> = BTreeSet::new();
    let mut fields: BTreeMap<String, u32> = BTreeMap::new();
    let mut saved: Vec<f64> = Vec::new();
    for s in sessions {
        let Some(pos) = s.steps.iter().position(|x| &x.key == key && !x.call.failed) else { continue };
        let output = &s.steps[pos].call.output;
        let mut listed: Vec<(String, Vec<String>)> = extract_json(output)
            .and_then(|j| j.as_object().cloned())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| {
                        let arr: Vec<String> =
                            v.as_array()?.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
                        (!arr.is_empty() && arr.iter().all(|p| p.contains('/') || p.contains('.')))
                            .then_some((k.clone(), arr))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if listed.is_empty() {
            // Plain-text output: the repository paths it mentions.
            let paths: Vec<String> =
                spans::extract(output).into_iter().filter(|x| x.kind == SpanKind::Path).map(|x| x.value).collect();
            if !paths.is_empty() {
                listed.push((PATHS_FIELD.to_string(), paths));
            }
        }
        let Some((field, files)) = listed.into_iter().next() else { continue };
        opportunities += 1;
        open.insert(s.id.clone());
        let mut read_turns: BTreeSet<u32> = BTreeSet::new();
        let mut covered: BTreeSet<String> = BTreeSet::new();
        for st in s.steps.iter().skip(pos + 1).take(16) {
            if let (CanonicalAction::Read { .. }, Some(rel)) = (&st.norm.action, &st.rel_path) {
                if files.contains(rel) {
                    covered.insert(rel.clone());
                    read_turns.insert(st.turn);
                }
            }
        }
        if covered.len() * 2 >= files.len() {
            support.insert(s.id.clone());
            *fields.entry(field).or_default() += 1;
            saved.push(read_turns.len().saturating_sub(1) as f64);
        }
    }
    let n = support.len() as u32;
    let rate = n as f64 / opportunities.max(1) as f64;
    let lift = outcome_lift(sessions, &open, &support);
    // The rate gate, else the outcome-lift path (prime either way: a read is only suggested).
    if (n < cfg.min_support || rate < cfg.bar(&r.id))
        && crate::evidence::lift_strength(Effect::Observe, false, cfg, lift.as_ref()).is_none()
    {
        return None;
    }
    let field = fields.into_iter().max_by_key(|(_, c)| *c).map(|(f, _)| f).unwrap_or_default();
    Some(Route {
        slots: vec![Slot {
            name: "files".into(),
            source: SlotSource::ResultField { field },
            contract: Contract::RepoPath,
        }],
        strength: Strength::Prime,
        state: RouteState::Firm,
        evidence: Evidence {
            opportunities,
            support: n,
            days: days(sessions, &support),
            rate,
            lower_bound: wilson_lower(n, opportunities),
            turns_saved: median(&mut saved),
            provenance: cfg.provenance.clone(),
            models: models(sessions),
            outcome_lift: lift,
            origin: r.evidence.origin.clone(),
            ..Default::default()
        },
        ..r.clone()
    })
}

/// The z of the one-sided 90% bound a check after an edit is contrasted with.
const CHECK_Z: f64 = 1.2816;

/// A check after an edit is suggested only when at least this many traces, and this share of
/// them, missed it after such an edit: otherwise agents already run it unaided.
const CHECK_MISSED: u32 = 2;
const CHECK_MISSED_SHARE: f64 = 0.1;

/// The Wilson score interval's upper bound at `z` (1 with no trials).
fn wilson_upper(k: u32, n: u32, z: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    let (n, p) = (n as f64, k as f64 / n as f64);
    let z2 = z * z;
    ((p + z2 / (2.0 * n) + z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt()) / (1.0 + z2 / n)).min(1.0)
}

/// At least this many distinct tasks must support a directing route.
const MIN_TASKS: usize = 2;

/// The task a trace worked on, as far as the trace shows it: its first prompt, normalised (case
/// and spacing). Repeats of one task (arms, retries, reruns) share it; a trace with no prompt is a
/// task of its own.
fn task_of(s: &Session) -> String {
    let p: String =
        s.prompt.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase().chars().take(240).collect();
    if p.is_empty() {
        format!("\u{0}{}", s.id)
    } else {
        p
    }
}

/// Support must span distinct tasks (`MIN_TASKS`): steps repeated by reruns of one task (its
/// scratch script, run after each edit in every attempt at it) are that task's, not the
/// repository's. It applies to the steps a trigger directs (after an edit or a command); an
/// avoid is a failure of the environment (`No module named pytest`), the same whatever the task.
fn one_task(ss: &[&Session], support: &BTreeSet<String>) -> Result<(), String> {
    let tasks: BTreeSet<String> = ss.iter().filter(|s| support.contains(&s.id)).map(|s| task_of(s)).collect();
    if tasks.len() < MIN_TASKS {
        return Err(format!(
            "its {} supporting traces worked on {} task(s), fewer than {MIN_TASKS}",
            support.len(),
            tasks.len()
        ));
    }
    Ok(())
}

/// Recount the executable prediction using the same generalized context matcher as the runtime.
/// Compatible successful continuations may follow observations or failed checks, but not a new
/// source mutation. Every eligible trigger counts, including abstentions and contrary outcomes.
/// This is observational prediction evidence, not a grant or a claim of eliminated inference.
fn execution_evidence(r: &Route, ss: &[&Session], cfg: &LearnConfig) -> Result<ExecutionEvidence, String> {
    let Step::Run { command } = &r.step else {
        return Err("execution candidate is not one command".into());
    };
    if !matches!(r.effect, Effect::Verify | Effect::Regenerate) {
        return Err("execution candidate is not verification or regeneration".into());
    }
    let depth = match r.slots.as_slice() {
        [] => None,
        [Slot { name, source: SlotSource::EditSegment { depth, .. }, .. }] if name == "part" => Some(*depth as usize),
        _ => return Err("execution candidate has unsupported input bindings".into()),
    };
    let net = adage_tracks::context::CtxNet::of([r]);
    // New verbosity pooling cannot borrow a success from another/unknown cwd. Exact historical
    // command behavior stays unchanged when the evidence contains no different equivalent spelling.
    let pooled_pytest =
        ss.iter().flat_map(|s| &s.steps).flat_map(ok_parts).any(|(lit, _)| {
            lit != *command && adage_core::execution_equivalence::project_pytest_equivalent(command, &lit)
        });
    let symbols = adage_tracks::matching::Symbols::new(&net);
    let (mut opportunities, mut successful) = (0, 0);
    let mut support = BTreeSet::new();
    for s in ss {
        let mut context = adage_tracks::matching::CtxState::default();
        for (i, st) in s.steps.iter().enumerate() {
            let bound = match &r.trigger {
                Trigger::AfterEdit { paths } => {
                    let Some(path) = edited(st).filter(|p| glob_hit(paths, p)) else { continue };
                    match depth {
                        Some(d) => match path.split('/').filter(|x| !x.is_empty()).nth(d) {
                            Some(part) => command.replace("{part}", part),
                            None => continue,
                        },
                        None => command.clone(),
                    }
                }
                Trigger::Context { .. } if r.slots.is_empty() => {
                    let syms = if let Some(path) = edited(st) {
                        symbols.edit(path)
                    } else if st.call.failed {
                        symbols.fail(&st.key, &st.call)
                    } else {
                        symbols.act(&st.key)
                    };
                    if !context.push(&net, &syms) || !net.walk(&context.recent()).contains(r.id.as_str()) {
                        continue;
                    }
                    command.clone()
                }
                _ => return Err("execution candidate has unsupported predictive trigger".into()),
            };
            opportunities += 1;
            let want = Want::new(bound);
            // An eight-event horizon admits intervening inspection and environment-failure
            // observations. Never cross another edit, setup, generation or publishing action.
            let mut hit = false;
            for next in s.steps.iter().skip(i + 1).take(8) {
                if next.prompt != st.prompt || edited(next).is_some() {
                    break;
                }
                if want.hit(next) {
                    // A shell group's final status does not establish each member's outcome.
                    // Keep the sequence context broad while requiring reliable action evidence.
                    let raw = next
                        .call
                        .input
                        .get("command")
                        .or_else(|| next.call.input.get("cmd"))
                        .and_then(serde_json::Value::as_str);
                    hit = next.turn > st.turn
                        && (!pooled_pytest || next.project_pytest_root_cwd)
                        && raw.is_some_and(adage_tracks::io::safe_command)
                        && !adage_core::outcome::misfired(&next.call.output)
                        && !adage_core::outcome::tests_failed(&next.call.output);
                    break;
                }
                if matches!(
                    next.norm.effect,
                    Effect::Author | Effect::Environment | Effect::Regenerate | Effect::Publish
                ) || (next.norm.effect == Effect::Verify && !next.call.failed)
                {
                    break;
                }
            }
            if hit {
                successful += 1;
                support.insert(s.id.clone());
            }
        }
    }
    if support.len() < cfg.min_support as usize
        || opportunities == 0
        || successful as f64 / (opportunities as f64) < 0.9
    {
        return Err(format!("predictive execution needs {} unexposed sessions and 90% supported continuations: {} sessions, {successful}/{opportunities} opportunities", cfg.min_support, support.len()));
    }
    one_task(ss, &support)?;
    Ok(ExecutionEvidence { opportunities, successful, sessions: support.len() as u32 })
}

/// Judge one compiled route (see the module documentation); `Err` drops it.
fn judge(
    c: &compile::Compiled,
    ss: &[&Session],
    cfg: &LearnConfig,
    lookup: &[Route],
    prior: bool,
) -> Result<Route, String> {
    let mut r = c.route.clone();
    let seen: Vec<&Session> = ss.iter().copied().filter(|s| !exposed(s, &r)).collect();
    let execution = if c.execution_candidate {
        match execution_evidence(&r, &seen, cfg) {
            Ok(e) => Some(e),
            Err(_) if c.advisory_eligible => None,
            Err(reason) => return Err(reason),
        }
    } else {
        None
    };
    // Predicting an approved action and estimating the value of advice are different questions.
    // A reliable continuation beyond an avoidable failed attempt must not be discarded by the
    // advisory counter's deliberate exclusion of recovery runs.
    if let Some(evidence) = execution {
        r.evidence.execution = Some(evidence);
        r.evidence.models = models(&seen);
        r.strength = Strength::Prime;
        return Ok(r);
    }
    match (&r.trigger, &r.step) {
        (Trigger::Prompt { .. }, Step::Run { command }) => {
            let id = |c: &str| adage_graph::select::command_identity(c);
            return lookup
                .iter()
                .find(|l| matches!(&l.step, Step::Run { command: c } if id(c) == id(command)))
                .cloned()
                .ok_or_else(|| "the value-type lookup does not judge it".to_string());
        }
        (Trigger::AfterCommand { .. }, Step::ReadBatch { .. }) => {
            return judge_read_batch(&r, &seen, cfg)
                .ok_or_else(|| "too few traces read the files it listed together".into());
        }
        (Trigger::SessionStart, Step::Avoid { avoid, instead, .. }) => {
            let runners: Vec<&str> = avoid.split(" / ").collect();
            let c = count_avoid(&runners, instead, &seen);
            let n = c.support.len() as u32;
            if n < cfg.min_support {
                return Err(format!("{n} unexposed traces recovered, fewer than {}", cfg.min_support));
            }
            let opp = c.opportunities.max(n);
            r.evidence = Evidence {
                opportunities: opp,
                support: n,
                days: days(&seen, &c.support),
                rate: n as f64 / opp as f64,
                lower_bound: wilson_lower(n, opp),
                turns_saved: 1.0,
                provenance: cfg.provenance.clone(),
                models: models(&seen),
                origin: r.evidence.origin.clone(),
                ..Default::default()
            };
            return Ok(r);
        }
        (Trigger::AfterFailure { command: key, .. }, Step::Run { command }) => {
            let runner = crate::evidence::runner(
                &key.trim_start_matches("exec ").split(' ').map(str::to_string).collect::<Vec<_>>(),
            );
            let c = count_avoid(&[runner.as_str()], command, &seen);
            let n = c.support.len() as u32;
            if n < cfg.min_support {
                return Err(format!("{n} unexposed traces recovered, fewer than {}", cfg.min_support));
            }
            let opp = c.opportunities.max(n);
            let (rate, lb) = (n as f64 / opp as f64, wilson_lower(n, opp));
            r.strength = strength_for(Effect::Verify, false, lb, rate, cfg, &r.id)
                .ok_or_else(|| format!("rate {rate:.2} below the bar"))?;
            r.evidence = Evidence {
                opportunities: opp,
                support: n,
                days: days(&seen, &c.support),
                rate,
                lower_bound: lb,
                turns_saved: 1.0,
                provenance: cfg.provenance.clone(),
                models: models(&seen),
                origin: r.evidence.origin.clone(),
                ..Default::default()
            };
            return Ok(r);
        }
        (Trigger::Context { .. }, _) => {
            // Advisory scoring keeps the graph posterior; execution is separately recounted
            // through the actual compiled generalized context above.
            r.evidence.execution = execution;
            r.evidence.models = models(&seen);
            return Ok(r);
        }
        _ => {}
    }
    let by_edit = matches!(r.trigger, Trigger::AfterEdit { .. });
    let c = match &r.trigger {
        Trigger::AfterEdit { .. } => count_after_edit(&r, &seen),
        Trigger::AfterCommand { .. } => count_after_command(&r, &seen),
        _ => None,
    }
    .ok_or_else(|| "no counter for this shape".to_string())?;
    let n = c.support.len() as u32;
    let rate = n as f64 / c.opportunities.max(1) as f64;
    // The outcome lift over the traces where the trigger occurred: the compiler's second gate
    // (`adage_graph::compile::outcome_gate`), for a reflex too few traces show for the rate bar.
    let lift = outcome_lift(&seen, &c.open, &c.support);
    if n < cfg.min_support && outcome_gate(r.effect, by_edit, lift.as_ref(), cfg).is_none() {
        return Err(format!("{n} supporting traces, fewer than {}", cfg.min_support));
    }
    one_task(&seen, &c.support)?;
    if by_edit && rate - c.rate_without < cfg.finish_line_contrast {
        return Err(format!("contrast {:.2} below {}", rate - c.rate_without, cfg.finish_line_contrast));
    }
    // A check after an edit must not be what agents run anyway: the contrast must hold between
    // the bounds, the one-sided 90% lower bound of its rate after the edit against the upper bound
    // of its rate in the traces that made no such edit (a handful of traces on either side proves
    // little). A regeneration owed by an edit is not run without one, and a check scoped to the
    // edited part names what was edited: both keep the plain contrast.
    if by_edit && r.effect == Effect::Verify && r.slots.is_empty() && execution.is_none() {
        let (k, m) = c.without;
        let upper = wilson_upper(k, m, CHECK_Z);
        let lower = 1.0 - wilson_upper(c.opportunities - n, c.opportunities, CHECK_Z);
        if lower - upper < cfg.finish_line_contrast {
            return Err(format!(
                "a check agents may run anyway: {n} of {} traces after the edit (lower bound {lower:.2}), {k} of {m} without it (upper bound {upper:.2})",
                c.opportunities
            ));
        }
        // A suggestion's worth is in the traces that would miss it (Track G §5: V has the factor
        // 1 − p_soon): a check agents already ran after (nearly) every such edit adds nothing.
        let missed = c.opportunities - n;
        if missed < CHECK_MISSED || (missed as f64) < CHECK_MISSED_SHARE * c.opportunities as f64 {
            return Err(format!(
                "a check agents already run after the edit: {n} of {} traces ran it, {missed} missed it",
                c.opportunities
            ));
        }
    }
    let lb = wilson_lower(n, c.opportunities);
    // An instinct's prior enters the prime test only (3 traces at 0.7).
    let prime_rate = if prior { (n as f64 + 3.0 * 0.7) / (c.opportunities as f64 + 3.0) } else { rate };
    // A sequence is capped by its lowest ceiling.
    let effect = match &r.step {
        Step::Sequence { commands } => commands
            .iter()
            .map(|x| io::classify(x, &adage_core::CommandTable::load()))
            .fold(Effect::Observe, |a, e| if ceiling(e, by_edit) == Some(Strength::Fire) { a } else { e }),
        _ => r.effect,
    };
    let strength =
        (if n >= cfg.min_support { strength_for(effect, by_edit, lb, prime_rate.max(rate), cfg, &r.id) } else { None })
            .or_else(|| outcome_gate(effect, by_edit, lift.as_ref(), cfg))
            .ok_or_else(|| format!("rate {rate:.2} below the bar and no outcome lift, or may never be directed"))?;
    // A check after an edit is suggested, never fired at every edit (`compile::shapes`); a skill
    // is never above its ceiling (`adage_tracks::SKILL_CEILING`).
    let strength = if (by_edit && effect == Effect::Verify && matches!(r.step, Step::Run { .. }))
        || matches!(r.step, Step::Skill { .. })
    {
        Strength::Prime
    } else {
        strength
    };
    r.strength = strength;
    r.evidence = Evidence {
        execution,
        opportunities: c.opportunities,
        support: n,
        days: days(&seen, &c.support),
        deviations_failed: c.deviations_failed,
        rate,
        lower_bound: lb,
        provenance: cfg.provenance.clone(),
        models: models(&seen),
        origin: r.evidence.origin.clone(),
        outcome_lift: lift,
        ..Default::default()
    };
    Ok(r)
}

/// The compiler's outcome-lift gate with the learn thresholds.
fn outcome_gate(
    effect: Effect,
    by_edit: bool,
    lift: Option<&adage_tracks::lift::OutcomeLift>,
    cfg: &LearnConfig,
) -> Option<Strength> {
    let p = CompileParams {
        outcome_lift_lower_bound: cfg.outcome_lift_lower_bound,
        outcome_min_support: cfg.outcome_min_support,
        ..Default::default()
    };
    compile::outcome_gate(effect, by_edit, lift, &p)
}

/// After-edit routes that direct the same step on different globs compete for one explanation:
/// agents who edit `crates/**/*.rs` and `docs/*.md` in one trace, then run
/// `cargo clippy --workspace …`, show the check after both kinds of edit, but only one kind calls
/// for it. Strongest first (support, then id), a route is kept only when the traces that edited
/// its files and **none** of a kept route's still ran its step after their last matching edit at
/// the prime bar: its files predict the step on their own. Without such traces nothing tells the
/// two apart, and the stronger explanation stands alone.
fn explain_away(routes: &mut Vec<Judged>, ss: &[&Session], cfg: &LearnConfig, report: &mut EngineReport) {
    let glob_of = |r: &Route| match &r.trigger {
        Trigger::AfterEdit { paths } if r.slots.is_empty() => Some(paths.clone()),
        _ => None,
    };
    let mut order: Vec<usize> = (0..routes.len()).filter(|&i| glob_of(&routes[i].route).is_some()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&routes[a].route, &routes[b].route);
        y.evidence.support.cmp(&x.evidence.support).then_with(|| x.id.cmp(&y.id))
    });
    let mut kept: Vec<usize> = Vec::new();
    let mut gone: BTreeSet<usize> = BTreeSet::new();
    for i in order {
        let r = &routes[i].route;
        let g = glob_of(r).expect("after-edit");
        let rival = kept.iter().map(|&k| &routes[k].route).find(|k| {
            k.step == r.step && glob_of(k).is_some_and(|kg| kg != g) && {
                let kg = glob_of(k).expect("after-edit");
                let alone: Vec<&Session> = ss
                    .iter()
                    .copied()
                    .filter(|s| {
                        !exposed(s, r) && !s.steps.iter().any(|st| edited(st).is_some_and(|p| glob_hit(&kg, p)))
                    })
                    .collect();
                let c = count_after_edit(r, &alone);
                let (n, opp) = c.map_or((0, 0), |c| (c.support.len() as u32, c.opportunities));
                opp == 0 || (n as f64) < cfg.bar(&r.id) * opp as f64
            }
        });
        match rival {
            Some(k) => {
                report.dropped.push(Dropped {
                    pattern: routes[i].why.pattern.clone(),
                    reason: format!(
                        "{}: explained by {} (the traces that edited {g} but no file of {} did not run it)",
                        r.id,
                        k.id,
                        glob_of(k).unwrap_or_default()
                    ),
                });
                gone.insert(i);
            }
            None => kept.push(i),
        }
    }
    let mut i = 0;
    routes.retain(|_| {
        i += 1;
        !gone.contains(&(i - 1))
    });
}

/// Learn, compile and judge the routes for one repository's traces. `model` is the pinned
/// embedding model (prompt-intent prototypes need it; without one there are none).
pub fn run(ss: &[&Session], cfg: &LearnConfig, priors: &[Route], model: Option<&StaticModel>) -> EngineRun {
    // Every trace is mined; what its directives named is exposed, not evidence. Evidence for
    // what is delivered is counted again below on the traces not exposed to the route.
    let annotated: Vec<adage_graph::input::Annotated> = ss.iter().map(|s| bridge::annotate(s)).collect();
    let events: Vec<adage_graph::events::EventSession> =
        ss.iter().zip(&annotated).map(|(s, a)| bridge::events(a, s)).collect();
    let mut p = select::Params::default().with_threads(threads());
    p.mine.min_support = cfg.min_support;
    p.score.min_support = cfg.min_support;
    let (num, den) = cfg.dead_end_consistency;
    p.score.consistency = num as f64 / den.max(1) as f64;
    p.candidates = cfg.candidates;
    let learned = select::learn(&events, &p);
    // G1's value slots, learned in the discovery window as the lookup is judged there.
    let prefix: Vec<adage_graph::input::Session> =
        ss.iter().map(|s| bridge::annotate_prefix(s, s.discovery().len()).session).collect();
    let values = adage_graph::learn(&prefix, &adage_graph::Params { threads: threads(), ..Default::default() });
    // The glob lattice's tree is what the traces touched: a pure function of the traces, whatever
    // is on this machine (see `adage_graph::compile::glob`).
    let corpus = Corpus::of(&annotated, &[]);
    let params = CompileParams {
        fire_lower_bound: cfg.fire_lower_bound,
        prime_rate: cfg.prime_rate,
        outcome_lift_lower_bound: cfg.outcome_lift_lower_bound,
        outcome_min_support: cfg.outcome_min_support,
        max_signatures: cfg.dead_end_signatures,
        consistency: num as f64 / den.max(1) as f64,
        provenance: cfg.provenance.clone(),
        ..Default::default()
    };
    let priors: Vec<Prior> = priors.iter().map(|r| Prior { route: r.clone() }).collect();
    // The selected reflexes first, then every candidate that passed its gates (their cores are
    // deduplicated by route id, so a selected reflex explains a route whenever one does).
    let pool: Vec<select::Reflex> = learned.reflexes.iter().chain(learned.unselected.iter()).cloned().collect();
    let out = compile::compile(&compile::Input {
        reflexes: &pool,
        corpus: &corpus,
        values: Some(&values),
        priors: &priors,
        params: &params,
    });
    // The candidates, compiled apart so nothing above changes (plan D1); their drops are not the
    // routes' and are not reported.
    let candidates = compile::compile_candidates(&compile::Input {
        reflexes: &learned.candidates,
        corpus: &corpus,
        values: Some(&values),
        priors: &[],
        params: &params,
    });
    // The lookup is a value-flow reflex (Track G §8, `[prompt] =flow=> [act …]`): G1's value slot
    // test over every trace finds it, and judges it.
    let lookup = crate::graph::lookup_routes(ss, cfg);
    let mut report = EngineReport {
        sessions: ss.len(),
        reflexes: learned.reflexes.len(),
        compiled: out.routes.len(),
        dropped: out.dropped.clone(),
        ..Default::default()
    };
    let mut routes = Vec::new();
    let mut sessions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut kept_candidates = Vec::new();
    for c in out.routes.into_iter().chain(candidates.routes) {
        // A candidate is counted, never judged into a route.
        if c.candidate {
            kept_candidates.push(c);
            continue;
        }
        sessions.insert(c.route.id.clone(), c.sessions.clone());
        let prior = c.why.prior.is_some();
        match judge(&c, ss, cfg, &lookup, prior) {
            Ok(r) => {
                report.contexts += usize::from(matches!(r.trigger, Trigger::Context { .. }));
                routes.push(Judged { route: r, why: c.why });
            }
            Err(reason) => report
                .dropped
                .push(Dropped { pattern: c.why.pattern.clone(), reason: format!("{}: {reason}", c.route.id) }),
        }
    }
    // A sequence whose first step an obligation on the same trigger already owes adds only its
    // later steps: kept when the traces did them together, one right after another, and dropped
    // when other commands came between (they are then separate habits, such as the repository's
    // usual check before finishing, and the obligation alone is delivered).
    let owed: Vec<(Trigger, String)> = routes
        .iter()
        .filter(|j| shape(&j.route) == "obligation")
        .filter_map(|j| match &j.route.step {
            Step::Run { command } => Some((j.route.trigger.clone(), command.clone())),
            _ => None,
        })
        .collect();
    routes.retain(|j| {
        let Step::Sequence { commands } = &j.route.step else { return true };
        if !owed.iter().any(|(t, c)| *t == j.route.trigger && commands.first() == Some(c)) {
            return true;
        }
        let seen: Vec<&Session> = ss.iter().copied().filter(|s| !exposed(s, &j.route)).collect();
        let (together, of) = together(&j.route, &seen);
        let keep = of > 0 && together as f64 >= SEQUENCE_TOGETHER * of as f64;
        if !keep {
            report.dropped.push(Dropped {
                pattern: j.why.pattern.clone(),
                reason: format!(
                    "{}: its first step is owed by an obligation, and {together} of {of} traces ran the rest right after it",
                    j.route.id
                ),
            });
        }
        keep
    });
    explain_away(&mut routes, ss, cfg, &mut report);
    let instead: Vec<String> = routes
        .iter()
        .filter_map(|j| match &j.route.step {
            Step::Avoid { instead, .. } => Some(instead.clone()),
            Step::Run { command } if matches!(j.route.trigger, Trigger::AfterFailure { .. }) => Some(command.clone()),
            _ => None,
        })
        .collect();
    // A check after an edit that an obligation on the same trigger already makes (the check
    // meets the obligation's command: `cargo fmt --all --check` after `cargo fmt --all`) adds
    // nothing: the obligation is delivered alone.
    routes.retain(|j| {
        let (Trigger::AfterEdit { .. }, Step::Run { command }) = (&j.route.trigger, &j.route.step) else {
            return true;
        };
        // (A check scoped to the edited part names that part: not the dead end's repo-wide command.)
        if j.route.effect != Effect::Verify || !j.route.slots.is_empty() {
            return true;
        }
        // A check an avoid already names as what to run instead is delivered at session start; so
        // is one the traces show as a dead end's working command (the check agents turned to after
        // another check failed, in at least `min_support` traces): it belongs to that dead end,
        // learned or not, not to the edit.
        let recoveries = recovery_traces(command, ss);
        if let Some(a) = instead.iter().find(|i| adage_core::pattern::same_command(i, command)) {
            report.dropped.push(Dropped {
                pattern: j.why.pattern.clone(),
                reason: format!("{}: a dead end already names {a:?} as the check to run", j.route.id),
            });
            return false;
        }
        if recoveries >= cfg.min_support {
            report.dropped.push(Dropped {
                pattern: j.why.pattern.clone(),
                reason: format!(
                    "{}: {recoveries} traces ran {command:?} after another check failed: a dead end's working command",
                    j.route.id
                ),
            });
            return false;
        }
        let checked = owed.iter().find(|(t, c)| *t == j.route.trigger && adage_core::pattern::same_command(c, command));
        if let Some((_, c)) = checked {
            report.dropped.push(Dropped {
                pattern: j.why.pattern.clone(),
                reason: format!("{}: it checks what the obligation to run {c:?} already owes", j.route.id),
            });
        }
        checked.is_none()
    });
    // Directive value (outcome-optimisation §2, `crate::value`): after a test runner fails for
    // the environment, the safe candidate that gets agents to a test run that ran tests soonest,
    // admitted when verified or when its value is positive at the one-sided 90% bound. It
    // replaces a mined route on the same trigger (a count is no evidence of value).
    let repo = ss.first().map(|s| s.repo.clone()).unwrap_or_default();
    let root = cfg
        .instincts
        .as_ref()
        .and_then(|c| c.roots.get(&repo))
        .or_else(|| cfg.roots.get(&repo))
        .cloned()
        .or_else(|| crate::instincts::root_of(ss.iter().map(|s| s.root.as_path())));
    let statics = crate::value::Statics::of(root.as_deref());
    let valued = crate::value::value_failures(ss, &statics, &cfg.priors, cfg.uplift_bar());
    let valued_routes = crate::value::failure_routes(&valued, ss, &cfg.provenance);
    // A learned step that runs a runner the value model shows failing here for the environment is
    // spelled with the environment that works (`python -m pytest tests/test_{part}` becomes
    // `.venv/bin/python -m pytest tests/test_{part}`): the same command family (its evidence is
    // counted on the family), run where it runs.
    let respell: Vec<(String, String)> = valued
        .iter()
        .filter(|v| v.strength.is_some())
        .filter_map(|v| Some((v.failed.clone()?, v.command.clone())))
        .collect();
    for j in routes.iter_mut() {
        let Step::Run { command } = &j.route.step else { continue };
        let Some((from, to)) = respell.iter().find(|(f, _)| command == f || command.starts_with(&format!("{f} ")))
        else {
            continue;
        };
        let new = format!("{to}{}", &command[from.len()..]);
        if !io::safe_command(&new) {
            continue;
        }
        let shape = adage_tracks::shape(&j.route).to_string();
        j.route.step = Step::Run { command: new };
        if !j.route.id.starts_with("dead-end-") && !j.route.id.starts_with("finish-line-") {
            j.route.id = compile::ids::route_id(&shape, &j.route.trigger, &j.route.step, &j.route.slots);
        }
    }
    for (route, why) in valued_routes {
        routes.retain(|j| {
            let same = j.route.id == route.id || j.route.trigger == route.trigger;
            if same {
                report.dropped.push(Dropped {
                    pattern: j.why.pattern.clone(),
                    reason: format!("{}: {} is valued on the same trigger", j.route.id, route.id),
                });
            }
            !same
        });
        routes.push(Judged { route, why });
    }
    report.valued = valued;
    // The tests of what was edited (`crate::testmap`): the project's runner, the edited file's tests
    // bound from a map. It explains a mined check after an edit that runs the same test tool
    // (`python -m pytest tests/test_{part}`, `npm test -- packages/{part}`): one reflex, built.
    let plan = crate::testmap::plan(ss, &statics, root.as_deref(), &cfg.priors, cfg.uplift_bar(), &cfg.provenance);
    if let Some((Some((route, why)), planned)) = plan.clone() {
        let family = planned.runner.clone();
        routes.retain(|j| {
            let (Trigger::AfterEdit { .. }, Step::Run { command }) = (&j.route.trigger, &j.route.step) else {
                return true;
            };
            let a =
                adage_core::action::classify_argv(&command.split_whitespace().map(str::to_string).collect::<Vec<_>>());
            let same = a.role == adage_core::action::Role::TestRun && a.runner == family;
            if same {
                report.dropped.push(Dropped {
                    pattern: j.why.pattern.clone(),
                    reason: format!("{}: explained by {}, the tests of what was edited", j.route.id, route.id),
                });
            }
            !same
        });
        routes.push(Judged { route, why });
        report.testmap = Some(planned);
    } else if let Some((None, planned)) = plan {
        report.testmap = Some(planned);
    }
    // How tests run here, once per session, when that was a useful directive in the training
    // sessions (`crate::testmap::card`).
    if let Some((found, plan)) = crate::testmap::card(ss, &statics, &cfg.provenance) {
        if let Some((route, why)) = found.filter(|(r, _)| !routes.iter().any(|j| j.route.id == r.id)) {
            routes.push(Judged { route, why });
        }
        report.card = Some(plan);
    }
    // A value-flow reflex MDL did not select on its own (a longer pattern holding it was chosen,
    // or none): its route is the value slot's, with the slot's own evidence.
    if !routes.iter().any(|j| j.route.id == compile::stable::LOOKUP_ID) {
        if let Some(l) = lookup.first() {
            let Step::Run { command } = &l.step else { unreachable!("a lookup runs a command") };
            let why = Why {
                shape: "run".into(),
                pattern: format!("[prompt] =flow=> [act {command}]"),
                backoff: 1,
                gain: 0.0,
                lift: 0.0,
                conf: (l.evidence.rate * 1e4).round() / 1e4,
                conf_lb: (l.evidence.lower_bound * 1e4).round() / 1e4,
                support: l.evidence.support,
                days: l.evidence.days,
                members: vec![],
                examples: vec![],
                prior: None,
            };
            routes.push(Judged { route: l.clone(), why });
        }
    }
    // Prompt-intent prototypes (Track G §2, §6.2), with the pinned model.
    if let Some(m) = model {
        let influence: Vec<Vec<adage_graph::events::Influence>> = ss.iter().map(|s| bridge::influence(s)).collect();
        let ip = compile::intent::IntentParams {
            min_support: cfg.min_support,
            ubiquitous: cfg.intent_ubiquitous,
            contrast: cfg.intent_contrast,
        };
        let strength = |effect: Effect, lb: f64, rate: f64, id: &str| strength_for(effect, false, lb, rate, cfg, id);
        let found = compile::intent::compile(&annotated, &influence, m, &ip, &cfg.provenance, &strength);
        report.intent_families = found.families;
        let all_models = models(ss);
        for mut x in found.routes {
            x.route.evidence.models = all_models.clone();
            routes.push(Judged { route: x.route, why: x.why });
        }
    }
    // Procedure cards for families of similar tasks (recurring work), with the pinned model.
    if let (Some(m), Some(cp)) = (model, cfg.cards.as_ref()) {
        let (found, families) = crate::cards::compile(ss, m, cp, &cfg.provenance);
        report.card_families = families;
        report.cards = found.len();
        for (route, why) in found {
            routes.push(Judged { route, why });
        }
    }
    report.routes = routes.len();
    for j in &routes {
        *report.kept_by.entry(kept_kind(j).to_string()).or_default() += 1;
    }
    report.candidates = kept_candidates.len();
    EngineRun { routes, report, counts: learned.counts, candidates: kept_candidates, sessions }
}

/// Where a kept route came from, for [`EngineReport::kept_by`].
fn kept_kind(j: &Judged) -> &'static str {
    if matches!(j.route.step, Step::Card { .. }) {
        "procedure card"
    } else if matches!(j.route.trigger, Trigger::Intent { .. }) {
        "prompt intent"
    } else if j.why.pattern.contains("=tests_of=>") || j.why.pattern.contains("=value=>") {
        "value estimate"
    } else if j.why.pattern.contains("=flow=>") {
        "lookup"
    } else {
        "mined pattern"
    }
}

/// For a route that was in service and was not learned again: `Some(verdict)` from the scorer's
/// counts for its own pattern (its `why`; the verdict is `Some(exposed)` to keep it dormant),
/// `None` when the scorer has none for it (the caller then judges it on the traces,
/// [`crate::dormant::verdict`]).
pub fn verdict(why: Option<&Why>, counts: &BTreeMap<String, Counts>, cfg: &LearnConfig) -> Option<Option<u32>> {
    let c = counts.get(&why?.pattern)?;
    Some(crate::dormant::verdict_counts(c.exposed, c.opportunities, c.support, cfg))
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[cfg(test)]
mod execution_equivalence_tests {
    use super::*;
    use serde_json::json;

    /// Synthetic contracts with production thresholds; never pilot evidence.
    fn routine(commands: &[&str]) -> Index {
        routine_cwd(commands, "root")
    }

    fn routine_cwd(commands: &[&str], mode: &str) -> Index {
        let temp = tempfile::tempdir().unwrap();
        let mut transcripts = Vec::new();
        for (i, command) in commands.iter().enumerate() {
            let sid = format!("equivalent-{i}");
            let cwd = "/fixture/equivalent";
            let mut lines = vec![json!({"type":"user", "sessionId":sid, "cwd":cwd,
                "message":{"content":format!("Implement independent feature {i}")}})];
            let mut calls = Vec::new();
            if mode == "prior-shell" {
                calls.push(("Bash", json!({"command":"cd sub"})));
            }
            if mode == "lookup" {
                calls.push(("Bash", json!({"command":"git log --oneline -3"})));
            }
            calls.push(("Write", json!({"file_path":format!("{cwd}/tests/test_a.py"), "content":"pass"})));
            if mode == "changelog" || mode == "most-edits" {
                calls.push(("Write", json!({"file_path":format!("{cwd}/CHANGELOG.rst"), "content":"- Add a."})));
            }
            if mode == "rewrite" {
                calls.push(("Write", json!({"file_path":format!("{cwd}/tests/test_a.py"), "content":"assert True"})));
            }
            calls.push(("Bash", json!({"command":command})));
            let closing = ["changelog", "rewrite", "lookup", "every-edit", "most-edits"].contains(&mode);
            if mode.starts_with("trailing-action") || closing {
                calls.push(("Bash", json!({"command":".venv/bin/python -c \"print(1)\""})));
            }
            if closing {
                // Not every edit is followed by the check: the context is what predicts it.
                calls.push(("Write", json!({"file_path":format!("{cwd}/docs/notes.md"), "content":"done"})));
            }
            if mode == "every-edit" || (mode == "most-edits" && i > 0) {
                // Unless every edit, or nearly every one, is.
                calls.push(("Bash", json!({"command":command})));
            }
            if mode == "then-ci" {
                calls.push(("Read", json!({"file_path":format!("{cwd}/README.md")})));
                calls.push(("Bash", json!({"command":"make ci"})));
            }
            if mode == "same-turn-shell" {
                calls.push(("Bash", json!({"command":"cd sub"})));
            }
            for (turn, (tool, input)) in calls.into_iter().enumerate() {
                let id = format!("tool-{turn}");
                let message_turn = if mode == "same-turn-shell" && turn == 2 { 1 } else { turn };
                let recorded_cwd = match mode {
                    "subdir" => json!(format!("{cwd}/sub")),
                    "unknown" => serde_json::Value::Null,
                    _ => json!(cwd),
                };
                lines.push(json!({"type":"assistant", "sessionId":sid, "cwd":recorded_cwd,
                    "message":{"id":format!("turn-{message_turn}"), "model":"fixture", "content":[
                        {"type":"tool_use", "id":id, "name":tool, "input":input}]}}));
                let failed = mode == "trailing-action-failure" && i == 0 && turn == 1;
                lines.push(json!({"type":"user", "sessionId":sid, "cwd":cwd,
                    "message":{"content":[{"type":"tool_result", "tool_use_id":id, "is_error":failed,
                        "content":if failed {"1 failed"} else {"1 passed"}}]}}));
            }
            let path = temp.path().join(format!("{sid}.jsonl"));
            std::fs::write(&path, lines.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
            transcripts.push(path);
        }
        let inputs = crate::Inputs {
            transcripts,
            repo_override: Some("equivalent".into()),
            outcomes: BTreeMap::new(),
            models: None,
            cache: None,
            only_repo: None,
        };
        let index = temp.path().join("index.json");
        crate::learn(&inputs, &temp.path().join("tracks"), &index, &LearnConfig::default()).unwrap();
        Index::load(&index)
    }

    #[test]
    fn independently_executable_check_survives_a_common_later_action() {
        let command = ".venv/bin/python -m pytest tests/test_a.py -q";
        let commands = [command; 5];
        let index = routine_cwd(&commands, "trailing-action");
        let certified: Vec<_> = index.routes.iter().filter(|r| r.route.evidence.execution.is_some()).collect();
        assert!(!certified.is_empty(), "closure must not absorb the independently executable check");
        for r in certified {
            assert_eq!(r.route.step, Step::Run { command: command.into() });
            let ev = r.route.evidence.execution.as_ref().unwrap();
            assert_eq!((ev.sessions, ev.successful, ev.opportunities), (5, 5, 5));
        }
        for (commands, mode) in [(&commands[..4], "trailing-action"), (&commands[..], "trailing-action-failure")] {
            let index = routine_cwd(commands, mode);
            assert!(
                !index.routes.iter().any(|r| r.route.evidence.execution.is_some()),
                "prefix admission must preserve independent support and success recount: {mode}"
            );
        }
    }

    #[test]
    fn a_check_after_tests_then_changelog_edits_certifies_as_a_context() {
        // The guided Marshmallow routine (run-175d85): edit a test, then the changelog, then the
        // test command, then a common later action that closure absorbs the check into.
        let command = ".venv/bin/python -m pytest tests/test_a.py -q";
        let index = routine_cwd(&[command; 5], "changelog");
        let want = Trigger::Context {
            items: vec![CtxItem::Edit { glob: "tests/*.py".into() }, CtxItem::Edit { glob: "CHANGELOG.rst".into() }],
            within: 8,
        };
        let route = index
            .routes
            .iter()
            .find(|r| r.route.trigger == want && r.route.evidence.execution.is_some())
            .expect("the tests-then-changelog context is certified");
        assert_eq!(route.route.step, Step::Run { command: command.into() });
        let ev = route.route.evidence.execution.as_ref().unwrap();
        assert_eq!((ev.sessions, ev.successful, ev.opportunities), (5, 5, 5));
    }

    #[test]
    fn a_context_that_edits_one_file_twice_is_not_admitted_for_execution_alone() {
        // The second write of the same test is a later mutation the first one's trigger would
        // cross; as a context it is the same crossing, so nothing is certified.
        let command = ".venv/bin/python -m pytest tests/test_a.py -q";
        let index = routine_cwd(&[command; 5], "rewrite");
        assert!(
            !index.routes.iter().any(|r| r.route.evidence.execution.is_some()),
            "{:?}",
            index.routes.iter().map(|r| (&r.route.trigger, &r.route.evidence.execution)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_context_holding_a_command_is_not_admitted_for_execution_alone() {
        // A lookup before the edit: the check is certified after the edit, never after the lookup
        // and the edit (after another command, the check is a step of a sequence).
        let command = ".venv/bin/python -m pytest tests/test_a.py -q";
        let index = routine_cwd(&[command; 5], "lookup");
        let certified: Vec<_> = index.routes.iter().filter(|r| r.route.evidence.execution.is_some()).collect();
        assert!(!certified.is_empty(), "the check after the edit is still certified");
        for r in certified {
            let command = |i: &CtxItem| matches!(i, CtxItem::Act { .. });
            let holds = matches!(&r.route.trigger, Trigger::Context { items, .. } if items.iter().any(command));
            assert!(!holds, "{:?}", r.route.trigger);
        }
    }

    #[test]
    fn a_check_after_every_edit_is_not_admitted_for_execution_alone() {
        // With every edit followed by the check (one that names no file), no edit predicts it
        // better than another: the trigger would be the anchor type relabelled, which advisory
        // contrast refuses too.
        let index = routine_cwd(&["make test"; 5], "every-edit");
        assert!(
            !index.routes.iter().any(|r| r.route.evidence.execution.is_some()),
            "{:?}",
            index.routes.iter().map(|r| (&r.route.trigger, &r.route.evidence.execution)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_prefix_closure_absorbed_still_needs_the_advisory_contrast() {
        // The tests-then-changelog routine, but four of five later edits are followed by the
        // check too (contrast 0.1), and closure absorbs the check into the later action: one
        // bypass at a time, so nothing is certified.
        let index = routine_cwd(&[".venv/bin/python -m pytest tests/test_a.py -q"; 5], "most-edits");
        assert!(
            !index.routes.iter().any(|r| r.route.evidence.execution.is_some()),
            "{:?}",
            index.routes.iter().map(|r| (&r.route.trigger, &r.route.evidence.execution)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_step_after_a_passed_check_is_not_an_execution_candidate() {
        // Edit, the check passes, then `make ci`: the check is certified after the edit, but once
        // it has passed the change is checked, and `make ci` is the next step of a sequence.
        let command = ".venv/bin/python -m pytest tests/test_a.py -q";
        let index = routine_cwd(&[command; 5], "then-ci");
        let certified: Vec<_> = index.routes.iter().filter(|r| r.route.evidence.execution.is_some()).collect();
        assert!(certified.iter().any(|r| r.route.step == Step::Run { command: command.into() }), "{certified:?}");
        assert!(
            certified.iter().all(|r| r.route.step != Step::Run { command: "make ci".into() }),
            "{:?}",
            certified.iter().map(|r| (&r.route.trigger, &r.route.step)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn verbosity_variants_certify_only_with_five_genuine_sessions() {
        let commands = [
            ".venv/bin/python -m pytest tests/test_a.py -q",
            ".venv/bin/python -m pytest tests/test_a.py -qq",
            ".venv/bin/python -m pytest tests/test_a.py -v",
            ".venv/bin/python -m pytest tests/test_a.py -vv",
            ".venv/bin/python -m pytest tests/test_a.py --quiet",
        ];
        let index = routine(&commands);
        let route =
            index.routes.iter().find(|r| r.route.evidence.execution.is_some()).expect("equivalent direct successes");
        let ev = route.route.evidence.execution.as_ref().unwrap();
        assert_eq!((ev.sessions, ev.successful, ev.opportunities), (5, 5, 5));
        assert!(
            matches!(&route.route.step, Step::Run { command } if commands.contains(&command.as_str())),
            "the selected command was actually observed"
        );
        assert!(!routine(&commands[..4]).routes.iter().any(|r| r.route.evidence.execution.is_some()));
        for mode in ["subdir", "unknown", "prior-shell", "same-turn-shell"] {
            assert!(
                !routine_cwd(&commands, mode).routes.iter().any(|r| r.route.evidence.execution.is_some()),
                "native cwd missing, non-root or possibly changed: {mode}"
            );
        }
        let mut compound = commands;
        compound[4] = "source .venv/bin/activate && python -m pytest tests/test_a.py -q";
        assert!(
            !routine(&compound).routes.iter().any(|r| r.route.evidence.execution.is_some()),
            "activation is not newly admitted"
        );
        for different in [
            ".venv/bin/python -m pytest tests/test_a.py -x",
            ".venv/bin/python -m pytest tests/test_a.py -s",
            ".venv/bin/python -m pytest tests/test_b.py -q",
            ".venv/bin/python -m pytest tests/test_a.py -k one",
            "other/bin/python -m pytest tests/test_a.py -q",
            ".venv/bin/python -m pytest tests/test_a.py -q && true",
            ".venv/bin/python -m pytest tests/test_a.py -q | tail",
        ] {
            let mut mixed = commands;
            mixed[4] = different;
            assert!(!routine(&mixed).routes.iter().any(|r| r.route.evidence.execution.is_some()), "{different}");
        }
    }
}
