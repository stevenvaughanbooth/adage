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
use super::{glob, ids, max_effect, variants, Compiled, Dropped, Input, Output, MAX_ROUTES};
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
    HOUSEKEEPING.contains(&head.rsplit('/').next().unwrap_or(head)) || adage_core::pattern::is_housekeeping(command)
}

/// Whether a literal may be directed as it is: no placeholder, nothing quoted, safe, and not
/// housekeeping.
fn directable(lit: &str) -> bool {
    !lit.contains('{') && !lit.contains('\'') && io::safe_command(lit) && !housekeeping(lit)
}

/// The covering variant of an item's family ([`variants`]), when its members disagree: every
/// command line agents ran under the item's member keys, weighted by the member's matches at this
/// item and split over its literals by how often each ran. Returns (key, command line).
fn covering(v: &ItemView, input: &Input) -> Option<(String, String)> {
    let table = adage_core::CommandTable::load();
    let mut family: Vec<variants::Variant> = Vec::new();
    for (k, n) in &v.members {
        let (Some(a), Some(effect)) = (input.corpus.acts.get(k), input.corpus.effect(k)) else { continue };
        let total: u32 = a.literals.values().map(|(_, c)| c).sum();
        for (lit, (_, c)) in &a.literals {
            // The member's matches here, shared by its literals (at least one run each).
            let runs = ((*n as u64 * *c as u64) / total.max(1) as u64).max(1) as u32;
            family.push(variants::Variant {
                key: k.clone(),
                command: lit.clone(),
                runs,
                ok_sessions: a.literal_ok.get(lit).copied().unwrap_or(0),
                ok_session_ids: a.literal_ok_ids.get(lit).cloned().unwrap_or_default(),
                effect: max_effect(effect, io::classify(lit, table)),
                directable: directable(lit),
            });
        }
    }
    variants::covering(&family, input.params.min_sessions, input.params.dominant).map(|(k, c, _)| (k, c))
}

/// A command a continuation item directs: its dominant variant's literal, when that has no
/// placeholder (a variable argument with no bound source is not directable).
fn fixed_command(v: &ItemView, target: Option<&str>, input: &Input) -> Result<(String, String, Effect), String> {
    let key = match target {
        Some(t) => Some(t.to_string()),
        None => dominant(v, input.params.dominant),
    };
    let first = key.map(|k| dominant_literal(k, input));
    if let Some(Ok(x)) = first {
        return Ok(x);
    }
    // No one spelling dominates: the family's covering variant, a command line agents ran that
    // covers the others ([`variants`]).
    if let Some((key, lit)) = covering(v, input) {
        let table = adage_core::CommandTable::load();
        let effect = max_effect(input.corpus.effect(&key).unwrap_or(Effect::Environment), io::classify(&lit, table));
        return Ok((key, lit, effect));
    }
    match first {
        Some(Err(e)) => Err(e),
        _ => Err(format!("{} is ambiguous", v.label)),
    }
}

/// The literal a dominant key directs: its most common one, which must be a fixed, plain command
/// and, for a key with a placeholder, itself dominant among the key's literals.
fn dominant_literal(key: String, input: &Input) -> Result<(String, String, Effect), String> {
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

/// Where a path component names one of `segs`: the segment itself, or, for a file segment, a test
/// file named for it with a short word before it (`test_core.py` for `core.py`, `test-cli.js` for
/// `cli.js`). A pytest node id after it (`::test_x`) is not part of the name. Returns the
/// component with `{part}` in the segment's place (the node id dropped) and whether a node id
/// followed.
fn templated(component: &str, segs: &BTreeSet<&str>) -> Option<(String, bool)> {
    let (base, node) = match component.split_once("::") {
        Some((b, _)) => (b, true),
        None => (component, false),
    };
    if segs.contains(base) {
        return Some(("{part}".into(), node));
    }
    segs.iter().filter(|s| s.contains('.') && !s.starts_with('.')).find_map(|s| {
        let prefix = base.strip_suffix(*s)?;
        let word = prefix.strip_suffix(['_', '-'])?;
        (!word.is_empty() && word.len() <= 12 && word.chars().all(|c| c.is_ascii_alphanumeric()))
            .then(|| (format!("{prefix}{{part}}"), node))
    })
}

/// The one template of a flow-bound command: the literal variants of the item's members with the
/// component that is segment `depth` of an edited member path replaced by `{part}`
/// (`make -C packages/{part} check`, `cargo test -p {part}`), or, for the edited file's own name,
/// the test file named for it (`python -m pytest tests/test_{part}` after `src/pkg/core.py`:
/// [`templated`]). A path argument that goes on past the segment (one test file of the part:
/// `npm test -- packages/runtime-core/__tests__/x.spec.ts`, or one test of the file:
/// `tests/test_core.py::test_x`) also stands for the part's directory or file
/// (`npm test -- packages/{part}`), with no options after it (`-xvs`, `-k name`): the part's
/// tests, the family's stable prefix with the variable part bound to what was edited. The template
/// covering at least the prime share of the runs ([`variants::covers`]; the most runs of its own
/// first) wins. The judge then counts it trace by trace, bound to each trace's own last edit
/// (`adage_learn::engine`); the share here only rules out a template most runs do not follow.
fn segment_template(v: &ItemView, edits: &[String], input: &Input) -> Option<(String, u8, Effect)> {
    let keys = member_keys(v);
    let mut lits: Vec<(&Vec<String>, u32)> = Vec::new();
    for k in &keys {
        if let Some(a) = input.corpus.acts.get(k) {
            lits.extend(a.literals.values().map(|(argv, n)| (argv, *n)));
        }
    }
    let max_depth = edits.iter().map(|p| p.split('/').count()).max().unwrap_or(0).min(8);
    for depth in 0..max_depth {
        // The parts: the segment at `depth` of the item's edited paths, or, below a directory
        // they share (`packages/` for `packages/runtime-core/src/x.ts`), of every path the traces
        // edited under it, so one item's single part still shows the part varying across the
        // repository's traces. Top-level directories are never parts of one another.
        let own: BTreeSet<&str> = edits.iter().filter_map(|p| p.split('/').nth(depth)).collect();
        let parent: BTreeSet<Vec<&str>> = edits.iter().map(|p| p.split('/').take(depth).collect::<Vec<_>>()).collect();
        let segs: BTreeSet<&str> = if own.len() >= 2 || depth == 0 {
            own
        } else {
            if parent.len() != 1 || edits.iter().any(|p| p.split('/').count() <= depth) {
                continue;
            }
            let parent = parent.into_iter().next().expect("one parent");
            input
                .corpus
                .edited
                .iter()
                .chain(edits.iter())
                .filter(|p| p.split('/').count() > depth)
                .filter(|p| p.split('/').take(depth).eq(parent.iter().copied()))
                .filter_map(|p| p.split('/').nth(depth))
                .collect()
        };
        if segs.len() < 2 {
            continue;
        }
        // Each run's full template, and the templates standing for it (itself, and its part's
        // directory or file with nothing after it from the first option on), with the runs each
        // stands for itself.
        let mut full: Vec<(String, u32)> = Vec::new();
        let mut own: BTreeMap<String, u32> = BTreeMap::new();
        let mut total = 0;
        for (argv, n) in &lits {
            total += n;
            let mut at: Option<usize> = None;
            let mut cut: Option<String> = None;
            let t: Vec<String> = argv
                .iter()
                .enumerate()
                .map(|(k, tok)| {
                    if at.is_some() {
                        return tok.clone();
                    }
                    let parts: Vec<&str> = tok.split('/').collect();
                    match parts.iter().enumerate().find_map(|(i, p)| templated(p, &segs).map(|x| (i, x))) {
                        Some((i, (part, node))) => {
                            at = Some(k);
                            let mut q: Vec<String> = parts.iter().map(|p| p.to_string()).collect();
                            let whole = std::mem::replace(&mut q[i], part);
                            if (i + 1 < q.len() || node) && !tok.starts_with('-') {
                                cut = Some(q[..=i].join("/"));
                            }
                            if let Some((_, rest)) = whole.split_once("::") {
                                q[i] = format!("{}::{rest}", q[i]);
                            }
                            q.join("/")
                        }
                        None => tok.clone(),
                    }
                })
                .collect();
            let Some(at) = at else { continue };
            let f = stable::shell_join(&t);
            *own.entry(f.clone()).or_default() += n;
            let mut short: Vec<String> = t[..at].to_vec();
            short.push(cut.unwrap_or_else(|| t[at].clone()));
            short.extend(t[at + 1..].iter().take_while(|x| !x.starts_with('-')).cloned());
            let s = stable::shell_join(&short);
            if s != f {
                *own.entry(s).or_default() += n;
            }
            full.push((f, *n));
        }
        let mut best: Option<(u32, u32, &String)> = None;
        for (t, n) in &own {
            if !io::safe_command(t) {
                continue;
            }
            let Some(ts) = variants::shape(t) else { continue };
            let covered: u32 = full
                .iter()
                .filter(|(f, _)| variants::shape(f).is_some_and(|fs| variants::covers(&ts, &fs, true)))
                .map(|(_, c)| c)
                .sum();
            if total == 0 || (covered as f64) < input.params.prime_rate * total as f64 - 1e-9 {
                continue;
            }
            if best.is_none_or(|(o, c, b)| (*n, covered, std::cmp::Reverse(t)) > (o, c, std::cmp::Reverse(b))) {
                best = Some((*n, covered, t));
            }
        }
        if let Some((_, _, t)) = best {
            let effect = keys.iter().filter_map(|k| input.corpus.effect(k)).fold(Effect::Observe, max_effect);
            let filled = t.replace("{part}", "x");
            let effect = max_effect(effect, io::classify(&filled, adage_core::CommandTable::load()));
            return Some((t.clone(), depth as u8, effect));
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
        let advisory_eligible = !r.execution_only
            && (r.value > 0.0
                || r.context.last().is_some_and(|v| v.ok == Some(false) || (v.ty == NodeType::Edit && v.last)));
        self.out.push(Compiled {
            shape,
            route,
            why,
            fixed_id,
            execution_candidate: r.execution_candidate,
            advisory_eligible,
            execution_only: r.execution_only,
        });
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
        let effect = io::classify(&command, adage_core::CommandTable::load());
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
        // A path flow need not vary: a repeated edit to the same test file followed by the
        // same check is executable without a slot. A singleton, or only reviewed pytest
        // verbosity variants with identical inputs, is constant; dominance alone is not.
        let literals: BTreeSet<_> = member_keys(&cont[0])
            .iter()
            .filter_map(|k| self.input.corpus.acts.get(k))
            .flat_map(|a| a.literals.keys())
            .collect();
        let root_support: BTreeSet<_> = member_keys(&cont[0])
            .iter()
            .filter_map(|k| self.input.corpus.acts.get(k))
            .flat_map(|a| a.literal_ok_ids.values().flatten())
            .collect();
        let constant_flow = r.execution_candidate
            && (literals.len() == 1
                || (root_support.len() >= self.input.params.min_sessions as usize
                    && literals.first().is_some_and(|first| {
                        literals
                            .iter()
                            .all(|lit| adage_core::execution_equivalence::project_pytest_equivalent(first, lit))
                    })));
        if cont[0].flow && !constant_flow {
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
                // The repository's check after editing these files: a run, prime at most (the
                // judge caps it), never an obligation with a Stop request.
                Effect::Verify => ("run", Step::Run { command: fixed[0].1.clone() }, first, false),
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
                _ if effect == Effect::Verify => "A check this repository runs after editing these files",
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

/// Whether every command a step directs runs over the whole repository
/// ([`variants::repo_wide`]).
fn repo_wide_step(step: &Step) -> bool {
    let cmds = super::commands(step);
    !cmds.is_empty() && cmds.iter().all(|c| pattern::Shape::of_command(c).is_some_and(|s| variants::repo_wide(&s)))
}

/// After-edit routes that direct the same repo-wide step on sibling directories' globs are one
/// reflex: their globs are generalised together ([`glob::widen`]), one route per resulting glob,
/// built from the route with the most support (its evidence is counted again on the new glob by
/// the judge). Routes whose glob does not widen are kept as they are.
fn widen_repo_wide(out: Vec<Compiled>, input: &Input) -> Vec<Compiled> {
    // A step scoped by the edited part (`npm test -- packages/{part}`) answers an edit in any
    // sibling part the same way: it widens too, its slot's glob with it.
    let segment_depth = |c: &Compiled| -> Option<Option<u8>> {
        let depths: BTreeSet<u8> = c
            .route
            .slots
            .iter()
            .map(|s| match &s.source {
                SlotSource::EditSegment { depth, .. } => Some(*depth),
                _ => None,
            })
            .collect::<Option<_>>()?;
        match depths.len() {
            0 => repo_wide_step(&c.route.step).then_some(None),
            1 => Some(depths.into_iter().next()),
            _ => None,
        }
    };
    let mut groups: BTreeMap<(&'static str, String, Option<u8>), Vec<usize>> = BTreeMap::new();
    for (i, c) in out.iter().enumerate() {
        if !matches!(c.route.trigger, Trigger::AfterEdit { .. }) {
            continue;
        }
        if let Some(depth) = segment_depth(c) {
            let step = serde_json::to_string(&c.route.step).unwrap_or_default();
            groups.entry((c.shape, step, depth)).or_default().push(i);
        }
    }
    let glob_of = |c: &Compiled| match &c.route.trigger {
        Trigger::AfterEdit { paths } => paths.clone(),
        _ => String::new(),
    };
    let mut replaced: BTreeSet<usize> = BTreeSet::new();
    let mut added: Vec<Compiled> = Vec::new();
    for ((shape, _, depth), idx) in groups {
        let globs: Vec<String> = idx.iter().map(|&i| glob_of(&out[i])).collect::<BTreeSet<_>>().into_iter().collect();
        let p = input.params;
        let wide = glob::widen(&globs, &input.corpus.tree, &input.corpus.touched, p.glob_precision);
        // A part is a segment under a fixed parent: a glob that widens past it binds nothing.
        let fixed_parent = |g: &String| {
            depth.is_none_or(|d| {
                g.split('/').take(d as usize + 1).count() == d as usize + 1
                    && !g.split('/').take(d as usize).any(|x| x.contains('*'))
            })
        };
        if wide == globs || !wide.iter().all(fixed_parent) {
            continue;
        }
        let best = *idx
            .iter()
            .max_by(|&&a, &&b| {
                let (x, y) = (&out[a], &out[b]);
                x.route.evidence.support.cmp(&y.route.evidence.support).then_with(|| y.why.pattern.cmp(&x.why.pattern))
            })
            .expect("a group has a route");
        for &i in &idx {
            if !wide.contains(&glob_of(&out[i])) {
                replaced.insert(i);
            }
        }
        for g in wide.iter().filter(|g| !globs.contains(g)) {
            let mut c = out[best].clone();
            c.route.trigger = Trigger::AfterEdit { paths: g.clone() };
            for s in c.route.slots.iter_mut() {
                if let SlotSource::EditSegment { glob, .. } = &mut s.source {
                    *glob = g.clone();
                }
            }
            let cmd = super::commands(&c.route.step).first().map(|x| x.to_string()).unwrap_or_default();
            c.route.id = if c.fixed_id && shape == "obligation" {
                stable::finish_line_id(g, &cmd)
            } else {
                ids::route_id(shape, &c.route.trigger, &c.route.step, &c.route.slots)
            };
            added.push(c);
        }
    }
    out.into_iter().enumerate().filter(|(i, _)| !replaced.contains(i)).map(|(_, c)| c).chain(added).collect()
}

/// A scoped run whose glob fixes the bound segment (`tests/**/*.py` at depth 0) binds one value
/// only: the segment is written into the command (`pytest tests -xvs`) and the slot dropped.
fn fix_parts(mut c: Compiled) -> Compiled {
    let (Trigger::AfterEdit { paths }, Step::Run { command }) = (&c.route.trigger, &c.route.step) else { return c };
    let [Slot { source: SlotSource::EditSegment { depth, .. }, name, .. }] = c.route.slots.as_slice() else { return c };
    let segs: Vec<&str> = paths.split('/').collect();
    let d = *depth as usize;
    let fixed = segs.len() > d + 1 && segs[..=d].iter().all(|s| !s.contains('*'));
    if !fixed {
        return c;
    }
    let command = command.replace(&format!("{{{name}}}"), segs[d]);
    if command.contains('{') {
        return c;
    }
    c.route.step = Step::Run { command };
    c.route.slots.clear();
    c.route.id = ids::route_id(c.shape, &c.route.trigger, &c.route.step, &[]);
    c
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
        // Preserve a predictive multi-event boundary instead of flattening every sequence
        // ending in an edit or failure into one event. Recounting uses this same context at
        // learning and runtime; fallback shapes remain useful when context cannot compile.
        if r.execution_candidate && (2..=CONTEXT_MAX_ITEMS).contains(&r.context.len()) {
            let start = b.out.len();
            if b.context(r) {
                // This shape was retained for executable prediction. If its compiled trigger
                // fails the independent recount, it must not turn into new advisory traffic.
                for c in &mut b.out[start..] {
                    c.advisory_eligible = false;
                }
                continue;
            }
        }
        match last {
            // A failed check, and the check that works.
            Some(x) if is_act(x) && x.ok == Some(false) => {
                let y = r.continuation.first().filter(|y| is_act(y) && y.ok != Some(false));
                let wkey = y.and_then(|y| r.target.clone().or_else(|| dominant(y, input.params.dominant)));
                match (y, wkey) {
                    (Some(_), Some(wkey)) => {
                        let ok_verify = |k: &str| input.corpus.effect(k) == Some(Effect::Verify);
                        let runner_keys: Vec<String> = member_keys(x).into_iter().filter(|k| ok_verify(k)).collect();
                        // A working command with a variable argument (one test file per trace) is
                        // named by its family's covering form, never by one trace's file.
                        let instead = if wkey.contains('{') {
                            y.and_then(|y| covering(y, input)).map(|(_, l)| l)
                        } else {
                            input.corpus.literal(&wkey).map(|(l, _)| l.to_string())
                        };
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
    let out = std::mem::take(&mut b.out);
    b.out = widen_repo_wide(out, input).into_iter().map(fix_parts).collect();
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
        // One route per id: an advisory reflex's before an execution-only one's (a prefix kept
        // for execution judgement often has more support than the closed pattern that absorbs
        // it, and must not shadow its advice), then the one with the most support (ties: the
        // earlier pattern). The advisory route keeps its own judgement: a certificate would turn
        // it into an execution-only route, which is never delivered as advice.
        if kept.get(&c.route.id).is_none_or(|k| rank(&c) > rank(k)) {
            kept.insert(c.route.id.clone(), c);
        }
    }
    let mut routes: Vec<Compiled> = kept.into_values().collect();
    routes.truncate(MAX_ROUTES);
    Output { routes, dropped: b.dropped }
}

/// The order among compiled routes of one id (greater wins).
fn rank(c: &Compiled) -> (bool, u32, std::cmp::Reverse<&str>) {
    (!c.execution_only, c.route.evidence.support, std::cmp::Reverse(c.why.pattern.as_str()))
}
