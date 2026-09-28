//! Shape choice (Track G §6.1) and the route each shape compiles to.
//!
//! A selected reflex is read structurally, never by kind. Its items (context, then continuation)
//! may hold several deliverable parts, each one row of the shape table in [`super`]:
//!
//! - a prompt item followed by an act its value **flows** into → the lookup (`run`, typed slot);
//! - an act followed by a read of a path its output **listed** (`same_art`) → `read-batch`;
//! - a context ending in a **failed** verify act, continued by a working verify act → `avoid`;
//! - a context ending in an **edit**: one regenerate act → `obligation`; one act whose argument
//!   is a segment of the edited path (a flow edge) → `run` with an `edit_segment` slot; one
//!   environment act → `run`, a nascent until reviewed; 2–3 acts starting with a regenerate or
//!   environment step, the rest regenerate or verify steps → `sequence`;
//! - otherwise, a context of two or three compilable events (acts, failures, edits) → a context
//!   trigger (`run` or `sequence`), inert until G4; a single successful act → `after_command` →
//!   `run`.
//!
//! A continuation item compiles to one concrete command only: the dominant variant of its item
//! (≥ `dominant` of its matches), or, for an item bound by a flow edge, the one template all its
//! variants share. A target with a variable argument and no bound source is not directable and is
//! dropped. A check or an observation after an edit is the agent's own habit, not a convention,
//! and is not compiled in G3. Every run or sequence that directs a runner a compiled avoid names
//! is dropped before emission (interlocking, Track G §6.3).
//!
//! **Strength.** Fire needs the posterior's lower bound (`conf_lb`) ≥ `fire_lower_bound` and a
//! fire ceiling; prime needs the posterior mean (`conf`, with an instinct's prior mixed in when
//! one directs the same command at the same point) ≥ `prime_rate` — the reflex passed its gate,
//! so `V > 0` (a guard: a lower bound ≥ 0.5) already holds. An avoid is inhibit. No reflex compiles
//! above prime from prior mass alone: fire reads the organic lower bound only.

use super::stable::{self, LINKED_FILES_ID, LOOKUP_ID};
use super::{glob, ids, max_effect, Compiled, Dropped, Input, Output, MAX_ROUTES};
use crate::events::NodeType;
use crate::select::{ItemView, Reflex};
use adage_core::pattern;
use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_tracks::why::{Members, Why};
use adage_tracks::*;
use std::collections::{BTreeMap, BTreeSet};

/// The contract of a path segment bound by an `edit_segment` slot: one plain path component.
pub const SEGMENT_RE: &str = "^[A-Za-z0-9_][A-Za-z0-9_.-]{0,63}$";

fn is_act(v: &ItemView) -> bool {
    v.ty == NodeType::Act
}

/// The exact members of an item, most common first (the label itself when none were recorded).
fn member_keys(v: &ItemView) -> Vec<String> {
    if v.members.is_empty() {
        vec![v.label.clone()]
    } else {
        v.members.iter().map(|(k, _)| k.clone()).collect()
    }
}

/// The dominant exact member of an item: at least `share` of its matches.
fn dominant(v: &ItemView, share: f64) -> Option<String> {
    let total: u32 = v.members.iter().map(|(_, n)| n).sum();
    match v.members.first() {
        Some((k, n)) if total > 0 && *n as f64 / total as f64 >= share => Some(k.clone()),
        None if v.level == crate::events::Level::Exact => Some(v.label.clone()),
        _ => None,
    }
}

fn why(r: &Reflex, shape: &str, prior: Option<String>) -> Why {
    let members = r
        .context
        .iter()
        .chain(r.continuation.iter())
        .filter(|v| v.members.len() > 1 || v.level != crate::events::Level::Exact)
        .map(|v| Members { item: format!("{}/{:?}:{}", v.ty.name(), v.level, v.label), members: v.members.clone() })
        .collect();
    Why {
        shape: shape.into(),
        pattern: r.pattern.clone(),
        backoff: r.context.len(),
        gain: (r.gain * 10.0).round() / 10.0,
        lift: (r.lift * 100.0).round() / 100.0,
        conf: (r.conf * 1e4).round() / 1e4,
        conf_lb: (r.conf_lb * 1e4).round() / 1e4,
        support: r.support,
        days: r.days,
        members,
        examples: r.examples.iter().take(3).cloned().collect(),
        prior,
    }
}

fn evidence(r: &Reflex, provenance: &str) -> Evidence {
    let opp = r.opportunities.max(r.support);
    Evidence {
        opportunities: opp,
        support: r.support,
        days: r.days,
        rate: if opp > 0 { r.support as f64 / opp as f64 } else { 0.0 },
        lower_bound: wilson_lower(r.support, opp),
        provenance: provenance.into(),
        exposed: r.exposed,
        origin: Some(Origin::Learned),
        ..Default::default()
    }
}

/// The strength of a reflex's route (see the module documentation); `None` when it is not
/// delivered at all.
fn strength(r: &Reflex, effect: Effect, by_edit: bool, prior: Option<(f64, f64)>, input: &Input) -> Option<Strength> {
    let max = ceiling(effect, by_edit)?;
    let p = input.params;
    let mean = match prior {
        Some((kappa, m)) => {
            let n = r.opportunities.max(r.support) as f64;
            (r.support as f64 + kappa * m) / (n + kappa)
        }
        None => r.conf,
    };
    if r.conf_lb >= p.fire_lower_bound && max == Strength::Fire {
        Some(Strength::Fire)
    } else if mean.max(r.conf) >= p.prime_rate {
        Some(Strength::Prime)
    } else {
        None
    }
}

/// The strength of a route whose evidence is counted again on its own trigger and step (the
/// caller's judgement, `adage_learn::engine`): the ceiling, at most prime, until then. A route
/// decomposed from a longer reflex (the lookup inside a linked-files reflex) must not inherit that
/// reflex's confidence.
fn provisional(effect: Effect, by_edit: bool) -> Option<Strength> {
    ceiling(effect, by_edit).map(|_| Strength::Prime)
}

/// The most common exact member of a context item (a trigger may be broad; only a directed
/// command must be one dominant variant).
fn most_common(v: &ItemView) -> String {
    v.members.first().map(|(k, _)| k.clone()).unwrap_or_else(|| v.label.clone())
}

/// An instinct that directs `command` at the same decision point as `trigger`.
fn prior_for<'a>(trigger: &Trigger, command: Option<&str>, input: &'a Input) -> Option<&'a Route> {
    let c = command?;
    input.priors.iter().map(|p| &p.route).find(|ir| {
        interlock::same_point(trigger, &ir.trigger)
            && super::commands(&ir.step).first().is_some_and(|x| pattern::same_command(x, c))
    })
}

#[allow(clippy::too_many_arguments)]
fn route(
    id: String,
    summary: &str,
    trigger: Trigger,
    step: Step,
    slots: Vec<Slot>,
    effect: Effect,
    strength: Strength,
    exit: Exit,
    ev: Evidence,
) -> Route {
    Route {
        id,
        summary: summary.into(),
        trigger,
        step,
        slots,
        effect,
        strength,
        state: initial_state(effect),
        exit: vec![exit],
        evidence: ev,
        authored: false,
    }
}

/// Programs an agent runs to look around or tidy up (count lines, list a directory, print, wait,
/// make a directory): how agents work, never a step of the repository's to direct.
const HOUSEKEEPING: &[&str] = &[
    "cat", "clear", "date", "df", "du", "echo", "env", "false", "head", "ls", "mkdir", "printenv", "printf", "pwd",
    "sleep", "stat", "tail", "touch", "tree", "true", "wc", "which", "whoami",
];

/// Whether `command` runs a housekeeping program ([`HOUSEKEEPING`]).
pub fn housekeeping(command: &str) -> bool {
    let head = command.split_whitespace().next().unwrap_or("");
    HOUSEKEEPING.contains(&head.rsplit('/').next().unwrap_or(head))
}

/// A command a continuation item directs: its dominant variant's literal, when that has no
/// placeholder (a variable argument with no bound source is not directable).
fn fixed_command(v: &ItemView, target: Option<&str>, input: &Input) -> Result<(String, String, Effect), String> {
    let key = match target {
        Some(t) => t.to_string(),
        None => dominant(v, input.params.dominant).ok_or_else(|| format!("{} is ambiguous", v.label))?,
    };
    let (lit, _) = input.corpus.literal(&key).ok_or_else(|| format!("no trace ran {key}"))?;
    if lit.contains('{') || lit.contains('\'') || !io::safe_command(lit) {
        return Err(format!("{lit:?} is not a fixed, plain command"));
    }
    if housekeeping(lit) {
        return Err(format!("{lit:?} is housekeeping, not a step to direct"));
    }
    // A key with a placeholder stands for a variable argument: directable only as one dominant
    // literal (else it needs a bound source, which a fixed command does not have).
    if key.contains('{') {
        let lits = &input.corpus.acts[&key].literals;
        let total: u32 = lits.values().map(|(_, n)| n).sum();
        let top = lits.get(lit).map_or(0, |(_, n)| *n);
        if total == 0 || (top as f64) < input.params.dominant * total as f64 {
            return Err(format!("{key} has a variable argument ({lit:?} is {top} of {total} runs)"));
        }
    }
    let effect = input.corpus.effect(&key).unwrap_or(Effect::Environment);
    Ok((key, lit.to_string(), effect))
}

/// The one template of a flow-bound command: every literal variant of the item's members with the
/// component that is segment `depth` of an edited member path replaced by `{part}`.
fn segment_template(v: &ItemView, edits: &[String], input: &Input) -> Option<(String, u8, Effect)> {
    let keys = member_keys(v);
    let mut lits: Vec<(&Vec<String>, u32)> = Vec::new();
    for k in &keys {
        if let Some(a) = input.corpus.acts.get(k) {
            lits.extend(a.literals.values().map(|(argv, n)| (argv, *n)));
        }
    }
    let max_depth = edits.iter().map(|p| p.split('/').count()).max().unwrap_or(0).min(8);
    for depth in 0..max_depth.saturating_sub(1) {
        let segs: BTreeSet<&str> = edits.iter().filter_map(|p| p.split('/').nth(depth)).collect();
        if segs.len() < 2 {
            continue;
        }
        let mut templates: BTreeMap<String, u32> = BTreeMap::new();
        let mut total = 0;
        for (argv, n) in &lits {
            total += n;
            let mut done = false;
            let t: Vec<String> = argv
                .iter()
                .map(|tok| {
                    if done {
                        return tok.clone();
                    }
                    let parts: Vec<&str> = tok.split('/').collect();
                    match parts.iter().position(|p| segs.contains(p)) {
                        Some(i) => {
                            done = true;
                            let mut q: Vec<String> = parts.iter().map(|p| p.to_string()).collect();
                            q[i] = "{part}".into();
                            q.join("/")
                        }
                        None => tok.clone(),
                    }
                })
                .collect();
            if done {
                *templates.entry(stable::shell_join(&t)).or_default() += n;
            }
        }
        if let Some((t, n)) = templates.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))) {
            if total > 0 && *n as f64 / total as f64 >= input.params.dominant && io::safe_command(t) {
                let effect = keys.iter().filter_map(|k| input.corpus.effect(k)).fold(Effect::Observe, max_effect);
                let filled = t.replace("{part}", "x");
                let effect = max_effect(effect, io::classify(&filled, &adage_core::CommandTable::load()));
                return Some((t.clone(), depth as u8, effect));
            }
        }
    }
    None
}

/// The globs an edit item stands for (the lattice over its members).
fn edit_globs(v: &ItemView, input: &Input) -> Vec<String> {
    let members: Vec<&str> = v.members.iter().map(|(k, _)| k.as_str()).collect();
    let members = if members.is_empty() { vec![v.label.as_str()] } else { members };
    glob::lattice(&members, &input.corpus.tree, &input.corpus.touched, input.params.glob_precision)
        .into_iter()
        .filter(|g| !g.starts_with("**"))
        .collect()
}

/// A candidate before grouping: avoids group by working command and signature.
struct AvoidCore<'a> {
    r: &'a Reflex,
    runner_keys: Vec<String>,
    wkey: String,
    instead: String,
}

struct Builder<'a> {
    input: &'a Input<'a>,
    out: Vec<Compiled>,
    dropped: Vec<Dropped>,
    avoids: Vec<AvoidCore<'a>>,
}

impl<'a> Builder<'a> {
    fn drop(&mut self, r: &Reflex, reason: impl Into<String>) {
        self.dropped.push(Dropped { pattern: r.pattern.clone(), reason: reason.into() });
    }

    fn push(&mut self, shape: &'static str, fixed_id: bool, r: &Reflex, route: Route, prior: Option<&Route>) {
        let why = why(r, shape, prior.map(|p| p.id.clone()));
        self.out.push(Compiled { shape, route, why, fixed_id });
    }

    /// A prompt value flowing into an act: the lookup, with G1's learned type for the sink.
    fn lookup(&mut self, r: &'a Reflex, act: &ItemView) -> bool {
        let Some(identity) = act.identity.as_deref() else { return false };
        let Some(values) = self.input.values else {
            self.drop(r, "a prompt value flows into a command, but no value type was learned");
            return false;
        };
        let slot = values.slots.iter().find(|s| {
            let prompt = s.slot.sources.get("prompt").copied().unwrap_or(0);
            let all: u32 = s.slot.sources.values().sum();
            s.slot.sink.identity == identity && prompt * 2 > all && s.vtype().is_some()
        });
        let Some(st) = slot else {
            self.drop(r, format!("no safe value type for {identity}"));
            return false;
        };
        let t = st.vtype().expect("typed");
        let sink = &st.slot.sink;
        let placeholder = format!("{}{{item}}{}", sink.prefix, sink.suffix);
        let mut variants: BTreeMap<String, u32> = BTreeMap::new();
        for a in self.input.corpus.acts.values() {
            for (argv, n) in a.literals.values() {
                for (id, at, token) in crate::flow::sinks(argv) {
                    if id != sink.identity || at != sink.at {
                        continue;
                    }
                    let v = token.strip_prefix(sink.prefix.as_str()).and_then(|x| x.strip_suffix(sink.suffix.as_str()));
                    if !v.is_some_and(|v| adage_core::vtype::matches(&t.runs, v)) {
                        continue;
                    }
                    let mut done = false;
                    let tpl: Vec<String> = argv
                        .iter()
                        .map(|w| {
                            if !done && *w == token {
                                done = true;
                                placeholder.clone()
                            } else if !done
                                && w.strip_suffix(token.as_str())
                                    .is_some_and(|o| o.starts_with('-') && o.ends_with('='))
                            {
                                done = true;
                                format!("{}{placeholder}", w.strip_suffix(token.as_str()).unwrap_or_default())
                            } else {
                                w.clone()
                            }
                        })
                        .collect();
                    *variants.entry(stable::shell_join(&tpl)).or_default() += n;
                }
            }
        }
        let Some(command) =
            variants.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(k, _)| k.clone())
        else {
            self.drop(r, format!("no trace ran {identity} with a value of its type"));
            return false;
        };
        let effect = io::classify(&command, &adage_core::CommandTable::load());
        let trigger = Trigger::Prompt { span: SpanKind::Ticket };
        let Some(asp) = provisional(effect, false) else {
            self.drop(r, format!("{command:?} may never be directed"));
            return false;
        };
        let vt = ValueType {
            id: t.id.clone(),
            runs: t.runs.clone(),
            regex: t.regex.clone(),
            samples_hash: t.samples_hash.clone(),
        };
        let slots = vec![Slot {
            name: "item".into(),
            source: SlotSource::PromptValue { vtype: vt, mentions: vec![] },
            contract: Contract::Pattern { regex: t.regex.clone() },
        }];
        let ev = evidence(r, &self.input.params.provenance);
        let rt = route(
            LOOKUP_ID.into(),
            "Look up the work item named in the prompt",
            trigger,
            Step::Run { command },
            slots,
            effect,
            asp,
            Exit::IntentChanges,
            ev,
        );
        self.push("run", true, r, rt, None);
        true
    }

    /// An act whose output listed the paths then read: read them together.
    fn read_batch(&mut self, r: &'a Reflex, act: &ItemView) -> bool {
        let key = most_common(act);
        let field = self.input.corpus.acts.get(&key).and_then(|a| {
            a.fields.iter().max_by(|x, y| x.1.cmp(y.1).then_with(|| y.0.cmp(x.0))).map(|(f, _)| f.clone())
        });
        let Some(field) = field else {
            self.drop(r, format!("{key}'s output lists no files"));
            return false;
        };
        let mut ev = evidence(r, &self.input.params.provenance);
        ev.turns_saved = 1.0;
        let rt = route(
            LINKED_FILES_ID.into(),
            "Read the files the work item links, together",
            Trigger::AfterCommand { command: key },
            Step::ReadBatch { slot: "files".into(), max: 5 },
            vec![Slot {
                name: "files".into(),
                source: SlotSource::ResultField { field },
                contract: Contract::RepoPath,
            }],
            Effect::Observe,
            Strength::Prime,
            Exit::IntentChanges,
            ev,
        );
        self.push("read-batch", true, r, rt, None);
        true
    }

    /// A context ending in an edit (see the module documentation).
    fn after_edit(&mut self, r: &Reflex, edit: &ItemView) -> bool {
        let cont = &r.continuation;
        if cont.is_empty() || !cont.iter().all(|v| is_act(v) && v.ok != Some(false)) {
            self.drop(r, "the continuation is not a run of successful commands");
            return false;
        }
        let globs = edit_globs(edit, self.input);
        if globs.is_empty() {
            self.drop(r, "no precise glob for the edited files");
            return false;
        }
        let edits: Vec<String> = edit.members.iter().map(|(k, _)| k.clone()).collect();
        // A value from the edited path: the scope of a narrow check or build. The flow edge may
        // come from an earlier read of the same path (the first source of a token wins), so a first
        // command whose variable argument is, in every variant, a segment of the edited paths is
        // bound the same way; its evidence is then counted per trace on the segment of the trace's
        // own last edit (the caller's judgement).
        let bound = segment_template(&cont[0], &edits, self.input)
            .filter(|_| cont[0].flow || fixed_command(&cont[0], r.target.as_deref(), self.input).is_err());
        if let Some((tpl, depth, effect)) = bound {
            if effect == Effect::Observe {
                self.drop(r, "an observation after an edit is not compiled");
                return false;
            }
            let mut any = false;
            for g in globs {
                let trigger = Trigger::AfterEdit { paths: g.clone() };
                let prior = prior_for(&trigger, Some(&tpl), self.input);
                let Some(asp) = provisional(effect, true) else {
                    self.drop(r, format!("{tpl:?} may never be directed"));
                    continue;
                };
                let step = Step::Run { command: tpl.clone() };
                let slots = vec![Slot {
                    name: "part".into(),
                    source: SlotSource::EditSegment { glob: g.clone(), depth },
                    contract: Contract::Pattern { regex: SEGMENT_RE.into() },
                }];
                let id = ids::route_id("run", &trigger, &step, &slots);
                let ev = evidence(r, &self.input.params.provenance);
                let rt = route(
                    id,
                    "A step scoped to the part of the repository that was edited",
                    trigger,
                    step,
                    slots,
                    effect,
                    asp,
                    Exit::ExploratoryChange,
                    ev,
                );
                self.push("run", false, r, rt, prior);
                any = true;
            }
            return any;
        }
        if cont[0].flow {
            self.drop(r, "a flow-bound command has no single template");
            return false;
        }
        let mut fixed: Vec<(String, String, Effect)> = Vec::new();
        let mut complete = true;
        for (i, v) in cont.iter().enumerate() {
            match fixed_command(v, if i == 0 { r.target.as_deref() } else { None }, self.input) {
                // A step repeated at once adds nothing to a sequence (`make check` twice).
                Ok(c) if fixed.last().is_some_and(|l| l.1 == c.1) => {}
                Ok(c) => fixed.push(c),
                Err(e) => {
                    if i == 0 {
                        self.drop(r, e);
                        return false;
                    }
                    complete = false;
                    break;
                }
            }
        }
        let first = fixed[0].2;
        let seq_ok = fixed.len() >= 2
            && complete
            && matches!(first, Effect::Regenerate | Effect::Environment)
            && fixed[1..].iter().all(|c| matches!(c.2, Effect::Regenerate | Effect::Verify))
            && fixed.iter().map(|c| &c.1).collect::<BTreeSet<_>>().len() == fixed.len();
        // A sequence that starts with a regeneration owes that step as well: its obligation (with
        // the Stop request a sequence never makes) is compiled beside it.
        if seq_ok && first == Effect::Regenerate {
            let single = Reflex { continuation: vec![cont[0].clone()], ..r.clone() };
            self.after_edit(&single, edit);
        }
        let (shape, step, effect, legacy_ok) = if seq_ok {
            let effect = fixed.iter().map(|c| c.2).fold(Effect::Observe, max_effect);
            ("sequence", Step::Sequence { commands: fixed.iter().map(|c| c.1.clone()).collect() }, effect, false)
        } else {
            match first {
                Effect::Regenerate => ("obligation", Step::Run { command: fixed[0].1.clone() }, first, true),
                Effect::Environment if cont.len() == 1 => {
                    ("run", Step::Run { command: fixed[0].1.clone() }, first, false)
                }
                _ => {
                    self.drop(
                        r,
                        format!(
                            "{:?} after an edit is not an obligation, a sequence or an environment step",
                            fixed[0].1
                        ),
                    );
                    return false;
                }
            }
        };
        let mut any = false;
        for g in globs {
            let trigger = Trigger::AfterEdit { paths: g.clone() };
            let cmd = super::commands(&step).first().map(|c| c.to_string());
            let prior = prior_for(&trigger, cmd.as_deref(), self.input);
            // The strength is the lowest ceiling of the commands (a sequence), after an edit.
            let low = match &step {
                Step::Sequence { .. } => fixed.iter().map(|c| c.2).fold(Effect::Observe, |a, e| {
                    if ceiling(e, true) == Some(Strength::Fire) {
                        a
                    } else {
                        max_effect(a, e)
                    }
                }),
                _ => effect,
            };
            let Some(asp) = provisional(low, true) else {
                self.drop(r, "may never be directed");
                continue;
            };
            let id = if legacy_ok {
                stable::finish_line_id(&g, cmd.as_deref().unwrap_or_default())
            } else {
                ids::route_id(shape, &trigger, &step, &[])
            };
            let summary = match shape {
                "obligation" => "A regeneration step this repository requires after editing these files",
                "sequence" => "Steps this repository runs, in order, after editing these files",
                _ => "An environment step this repository needs after editing these files",
            };
            let ev = evidence(r, &self.input.params.provenance);
            let rt = route(id, summary, trigger, step.clone(), vec![], effect, asp, Exit::ExploratoryChange, ev);
            self.push(shape, legacy_ok, r, rt, prior);
            any = true;
        }
        any
    }

    /// A context of two or three compilable events, or one successful act.
    fn context(&mut self, r: &'a Reflex) -> bool {
        let informative: Vec<&ItemView> =
            r.context.iter().filter(|v| !(v.ty == NodeType::Prompt && v.level == crate::events::Level::Kind)).collect();
        let cont = &r.continuation;
        if cont.is_empty() || !cont.iter().all(|v| is_act(v) && v.ok != Some(false)) {
            self.drop(r, "the continuation is not a run of successful commands");
            return false;
        }
        let mut fixed = Vec::new();
        for (i, v) in cont.iter().enumerate() {
            match fixed_command(v, if i == 0 { r.target.as_deref() } else { None }, self.input) {
                Ok(c) if !matches!(c.2, Effect::Author | Effect::Publish) => fixed.push(c),
                Ok(c) => {
                    self.drop(r, format!("{:?} may never be directed", c.1));
                    return false;
                }
                Err(e) => {
                    self.drop(r, e);
                    return false;
                }
            }
        }
        let step = if fixed.len() == 1 {
            Step::Run { command: fixed[0].1.clone() }
        } else if fixed.iter().all(|c| matches!(c.2, Effect::Regenerate | Effect::Verify | Effect::Environment)) {
            Step::Sequence { commands: fixed.iter().map(|c| c.1.clone()).collect() }
        } else {
            self.drop(r, "a multi-step continuation with an observation is not a sequence");
            return false;
        };
        let effect = fixed.iter().map(|c| c.2).fold(Effect::Observe, max_effect);
        let trigger = if informative.len() == 1 {
            let v = informative[0];
            if !(self.input.params.after_command_runs && is_act(v) && v.ok != Some(false)) || fixed.len() != 1 {
                self.drop(r, "a one-event context of this type has no trigger");
                return false;
            }
            if !matches!(fixed[0].2, Effect::Verify | Effect::Regenerate) {
                self.drop(r, "after a command, only a check or a regeneration is directed");
                return false;
            }
            if member_keys(v).contains(&fixed[0].0) {
                self.drop(r, format!("{} is directed after itself", fixed[0].1));
                return false;
            }
            Trigger::AfterCommand { command: most_common(v) }
        } else if (2..=CONTEXT_MAX_ITEMS).contains(&informative.len()) {
            let mut items = Vec::new();
            for v in &informative {
                let item = match v.ty {
                    NodeType::Act if v.ok == Some(false) => {
                        let keys = member_keys(v);
                        let sig = keys
                            .iter()
                            .filter_map(|k| self.input.corpus.acts.get(k))
                            .flat_map(|a| a.signatures.iter())
                            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                            .map(|(s, _)| s.clone());
                        sig.map(|signature| CtxItem::Fail { keys, signature })
                    }
                    NodeType::Act => Some(CtxItem::Act { keys: member_keys(v) }),
                    NodeType::Edit => match edit_globs(v, self.input).as_slice() {
                        [g] => Some(CtxItem::Edit { glob: g.clone() }),
                        _ => None,
                    },
                    _ => None,
                };
                match item {
                    Some(i) => items.push(i),
                    None => {
                        self.drop(r, format!("{} cannot be a context item", v.label));
                        return false;
                    }
                }
            }
            Trigger::Context { items, within: self.input.params.within }
        } else {
            self.drop(r, "no compilable context");
            return false;
        };
        // A context route keeps the reflex's own posterior (nothing counts it again in G3); an
        // after-command route is counted again by the caller.
        let asp = if matches!(trigger, Trigger::Context { .. }) {
            strength(r, effect, false, None, self.input)
        } else {
            provisional(effect, false)
        };
        let Some(asp) = asp else {
            self.drop(r, "below the bar, or may never be directed");
            return false;
        };
        let shape = if matches!(step, Step::Sequence { .. }) { "sequence" } else { "run" };
        let id = ids::route_id(shape, &trigger, &step, &[]);
        let summary = if matches!(trigger, Trigger::Context { .. }) {
            "What follows this context in this repository"
        } else {
            "A step that follows this command in this repository"
        };
        let ev = evidence(r, &self.input.params.provenance);
        let rt = route(id, summary, trigger, step, vec![], effect, asp, Exit::IntentChanges, ev);
        self.push(shape, false, r, rt, None);
        true
    }

    /// Group avoid cores by working command, then signature, and
    /// announce each at session start or after its failure.
    fn finish_avoids(&mut self) {
        let p = self.input.params;
        // (working key) → (signature → (runner names, runner keys, support, reflexes))
        type Sig<'b> = (BTreeSet<String>, BTreeSet<String>, u32, Vec<&'b Reflex>);
        let mut by_w: BTreeMap<String, (String, BTreeMap<String, Sig>)> = BTreeMap::new();
        let cores = std::mem::take(&mut self.avoids);
        for c in &cores {
            let sig = c
                .runner_keys
                .iter()
                .filter_map(|k| self.input.corpus.acts.get(k))
                .flat_map(|a| a.signatures.iter())
                .fold(BTreeMap::<&String, u32>::new(), |mut m, (s, n)| {
                    *m.entry(s).or_default() += n;
                    m
                })
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)))
                .map(|(s, _)| s.clone());
            let Some(sig) = sig else {
                self.dropped.push(Dropped {
                    pattern: c.r.pattern.clone(),
                    reason: "the failure has no renderable signature".into(),
                });
                continue;
            };
            // Runners that fail where they run (a failure family names several).
            let corpus = self.input.corpus;
            let consistent = |r: &String| {
                let ran = corpus.runners.get(r).copied().unwrap_or(0);
                let ok = corpus.runners_passed.get(r).copied().unwrap_or(0);
                ran > 0 && (ran - ok) as f64 >= p.consistency * ran as f64 - 1e-9
            };
            let runners: BTreeSet<String> = c
                .runner_keys
                .iter()
                .filter_map(|k| corpus.literal(k))
                .map(|(_, argv)| stable::runner(argv))
                .filter(consistent)
                .collect();
            if runners.is_empty() {
                self.dropped.push(Dropped {
                    pattern: c.r.pattern.clone(),
                    reason: "every runner it fails with also passes where it runs".into(),
                });
                continue;
            }
            let e = by_w.entry(c.wkey.clone()).or_insert_with(|| (c.instead.clone(), BTreeMap::new()));
            let s = e.1.entry(sig).or_default();
            s.0.extend(runners);
            s.1.extend(c.runner_keys.iter().cloned());
            s.2 = s.2.max(c.r.support);
            s.3.push(c.r);
        }
        for (wkey, (instead, sigs)) in by_w {
            let mut ranked: Vec<(String, Sig)> = sigs.into_iter().collect();
            ranked.sort_by(|a, b| b.1 .2.cmp(&a.1 .2).then_with(|| b.0.cmp(&a.0)));
            ranked.truncate(p.max_signatures.max(1));
            let mut named: BTreeSet<String> = BTreeSet::new();
            for (rank, (sig, (runners, keys, _, rs))) in ranked.into_iter().enumerate() {
                // Most traces first, then by name; each runner named once, under its most common
                // signature.
                let mut runners: Vec<String> = runners.into_iter().filter(|x| !named.contains(x)).collect();
                let ran = |x: &String| self.input.corpus.runners.get(x).copied().unwrap_or(0);
                runners.sort_by(|a, b| ran(b).cmp(&ran(a)).then_with(|| a.cmp(b)));
                runners.truncate(3);
                if runners.is_empty() {
                    continue;
                }
                named.extend(runners.iter().cloned());
                let best = rs
                    .iter()
                    .max_by(|a, b| a.support.cmp(&b.support).then_with(|| b.pattern.cmp(&a.pattern)))
                    .expect("one");
                let ran =
                    runners.iter().map(|x| self.input.corpus.runners.get(x).copied().unwrap_or(0)).max().unwrap_or(0);
                let at_start = self.input.corpus.sessions > 0
                    && ran as f64 / self.input.corpus.sessions as f64 >= p.announce_at_start;
                let id = stable::dead_end_id(&wkey, &sig, rank);
                let mut ev = evidence(best, &p.provenance);
                ev.opportunities = ran.max(best.support);
                ev.rate = best.support as f64 / ev.opportunities.max(1) as f64;
                ev.lower_bound = wilson_lower(best.support, ev.opportunities);
                ev.turns_saved = 1.0;
                let rt = if at_start {
                    route(
                        id,
                        "A verification command that fails in this repository, and what works instead",
                        Trigger::SessionStart,
                        Step::Avoid { avoid: runners.join(" / "), instead: instead.clone(), signature: sig.clone() },
                        vec![],
                        Effect::Verify,
                        Strength::Inhibit,
                        Exit::IntentChanges,
                        ev,
                    )
                } else {
                    let key = keys.iter().next().cloned().unwrap_or_default();
                    let Some(asp) = strength(best, Effect::Verify, false, None, self.input) else { continue };
                    route(
                        id,
                        "After this failure, what works instead in this repository",
                        Trigger::AfterFailure { command: key, signature: sig.clone() },
                        Step::Run { command: instead.clone() },
                        vec![],
                        Effect::Verify,
                        asp,
                        Exit::IntentChanges,
                        ev,
                    )
                };
                self.push("avoid", true, best, rt, None);
            }
        }
    }
}

/// Compile (see [`super::compile`]).
pub fn compile(input: &Input) -> Output {
    let mut b = Builder { input, out: Vec::new(), dropped: Vec::new(), avoids: Vec::new() };
    for r in input.reflexes {
        let items: Vec<&ItemView> = r.context.iter().chain(r.continuation.iter()).collect();
        let cut = r.context.len();
        let mut any = false;
        // Lookups and linked files, wherever they sit in the pattern.
        for w in 0..items.len().saturating_sub(1) {
            let (a, x) = (items[w], items[w + 1]);
            if a.ty == NodeType::Prompt && is_act(x) && x.flow && x.ok != Some(false) {
                any |= b.lookup(r, x);
            }
            if is_act(a) && a.ok != Some(false) && x.ty == NodeType::Read && x.same_art {
                any |= b.read_batch(r, a);
            }
        }
        let last = r.context.last();
        match last {
            // A failed check, and the check that works.
            Some(x) if is_act(x) && x.ok == Some(false) => {
                let y = r.continuation.first().filter(|y| is_act(y) && y.ok != Some(false));
                let wkey = y.and_then(|y| r.target.clone().or_else(|| dominant(y, input.params.dominant)));
                match (y, wkey) {
                    (Some(_), Some(wkey)) => {
                        let ok_verify = |k: &str| input.corpus.effect(k) == Some(Effect::Verify);
                        let runner_keys: Vec<String> = member_keys(x).into_iter().filter(|k| ok_verify(k)).collect();
                        let instead = input.corpus.literal(&wkey).map(|(l, _)| l.to_string());
                        match instead {
                            Some(instead)
                                if ok_verify(&wkey) && !runner_keys.is_empty() && io::safe_command(&instead) =>
                            {
                                b.avoids.push(AvoidCore { r, runner_keys, wkey, instead });
                                any = true;
                            }
                            _ => b.drop(r, "an avoid names a check that fails and a check that works"),
                        }
                    }
                    _ => b.drop(r, "no directable working command after the failure"),
                }
            }
            Some(e) if e.ty == NodeType::Edit => {
                any |= b.after_edit(r, e);
            }
            _ => {}
        }
        // A context route, only for a reflex the MDL cover selected (a candidate that merely
        // passed its gates delivers only the shapes above) and only when nothing else was compiled
        // from it; `context` says why it compiles nothing.
        let selected = r.gain > 0.0;
        if !any
            && selected
            && cut > 0
            && !matches!(last, Some(x) if (is_act(x) && x.ok == Some(false)) || x.ty == NodeType::Edit)
        {
            b.context(r);
        }
    }
    b.finish_avoids();
    // Interlocking before emission: a run or sequence directing an avoided runner is dropped.
    let avoided: Vec<String> = b
        .out
        .iter()
        .filter_map(|c| match &c.route.step {
            Step::Avoid { avoid, .. } => Some(avoid.clone()),
            _ => None,
        })
        .collect();
    let mut kept: BTreeMap<String, Compiled> = BTreeMap::new();
    for c in b.out {
        let conflicts = matches!(c.route.step, Step::Run { .. } | Step::Sequence { .. })
            && avoided
                .iter()
                .any(|a| super::commands(&c.route.step).iter().any(|cmd| pattern::directs_avoided(a, cmd)));
        if conflicts {
            b.dropped.push(Dropped {
                pattern: c.why.pattern.clone(),
                reason: format!("{} directs an avoided runner", c.route.id),
            });
            continue;
        }
        // One route per id: the one with the most support (ties: the earlier pattern).
        let better = |k: &Compiled| {
            (c.route.evidence.support, std::cmp::Reverse(&c.why.pattern))
                > (k.route.evidence.support, std::cmp::Reverse(&k.why.pattern))
        };
        if kept.get(&c.route.id).is_none_or(better) {
            kept.insert(c.route.id.clone(), c);
        }
    }
    let mut routes: Vec<Compiled> = kept.into_values().collect();
    routes.truncate(MAX_ROUTES);
    Output { routes, dropped: b.dropped }
}
