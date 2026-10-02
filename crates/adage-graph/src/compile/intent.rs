//! Prompt-intent prototypes (Track G §2, §6.2): prompts that mean the same thing *and* lead agents
//! to do the same thing, compiled to `intent` → `run`.
//!
//! A prompt's prototype is its family: each trace's first prompt is normalised and embedded with
//! the pinned static model (the one the hook embeds with), the prompts are ordered canonically
//! (normalised text, then trace id), and clustered with exact kNN and deterministic Louvain
//! (`adage_intent::cluster`). Embeddings propose, behaviour decides: each trace is labelled with
//! its *continuation*, the first directable command it ran before its first edit that is not run
//! by nearly every trace, and families are split and merged by it (`adage_intent::purity`).
//! A family whose dominant continuation clears the purity bar becomes a route: its trigger is the
//! family's quantised centroid with a similarity threshold calibrated on the repository's own
//! prompts (the best Wilson lower bound), its step the continuation's most common literal, and
//! its id `intent-<stable(group)>`. Commands that take a value from the prompt are the value-flow
//! lookup's, never an intent's: an intent route binds nothing.
//!
//! Steering by influence (§3.2): a command a directive named before the trace ran it is not that
//! trace's own continuation, so it is skipped when the trace is labelled.

use super::stable::{shell_join, stable};
use crate::events::{self, Influence};
use crate::input::{Annotated, Step as InStep};
use adage_core::{spans, tokens, CommandTable, Effect};
use adage_intent::cluster::{self, Params as ClusterParams};
use adage_intent::model::StaticModel;
use adage_intent::normalize::normalize;
use adage_intent::purity::{self, Purity};
use adage_tracks::why::Why;
use adage_tracks::*;
use std::collections::{BTreeMap, BTreeSet};

/// The prompt-intent path's thresholds (`learn.min_support`, `learn.intent_ubiquitous`,
/// `learn.intent_contrast`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntentParams {
    pub min_support: u32,
    /// Commands run before the first edit by more than this share of traces (of at least five)
    /// are not specific to any intent.
    pub ubiquitous: f64,
    /// A family's command must be this much more common among matching prompts than the rest.
    pub contrast: f64,
}

/// One candidate continuation of a trace: a directable command it ran before its first edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cand {
    /// The command's head and effect (`scripts/check|verify`): what groups its variants.
    pub group: String,
    pub literal: String,
    pub position: usize,
    pub turn: u32,
}

fn effect_of(meta: &str, argv: &[String], table: &CommandTable) -> Effect {
    match meta {
        "observe" => Effect::Observe,
        "verify" => Effect::Verify,
        "regenerate" => Effect::Regenerate,
        "environment" => Effect::Environment,
        "author" => Effect::Author,
        "publish" => Effect::Publish,
        _ => events::classify(argv, table),
    }
}

/// The trace's first prompt.
pub fn first_prompt(a: &Annotated) -> &str {
    a.session.prompts.iter().min_by_key(|p| p.at_step).map(|p| p.text.as_str()).unwrap_or("")
}

/// Directable commands (observe or verify) a trace ran before its first edit, one per group, in
/// order; commands that took a value from the prompt, and commands a directive named first, are
/// left out.
pub fn candidates(a: &Annotated, influence: &[Influence], table: &CommandTable) -> Vec<Cand> {
    let prompt = first_prompt(a);
    let from_prompt: BTreeSet<String> = spans::extract(prompt).into_iter().map(|x| x.value).collect();
    // The value flow's view of the prompt (the shared tokeniser, sub-tokens included: `#4280`
    // gives `4280`, `ops/412` gives `412`), its identifier-like tokens only (with a digit), so a
    // command that takes an item's id from a prompt that marks it (`Please work on #4280`) is the
    // lookup's too, while a word the prompt happens to share with a command (`tests`) is not a value.
    let ids: BTreeSet<String> = tokens::tokens(prompt, tokens::PROMPT_TOKENS)
        .into_iter()
        .map(|t| t.text)
        .filter(|t| t.chars().any(|c| c.is_ascii_digit()))
        .collect();
    let takes_value = |av: &[String]| {
        av.iter().any(|x| from_prompt.contains(x))
            || crate::flow::sinks(av)
                .iter()
                .any(|(_, _, v)| ids.contains(v) || tokens::subs(v).iter().any(|(_, s)| ids.contains(*s)))
    };
    let steps = &a.session.steps;
    let end = steps.iter().position(|s| matches!(s, InStep::Edit { .. })).unwrap_or(steps.len());
    let disc = &steps[..end];
    let exec = |i: usize| match &disc[i] {
        InStep::Exec { argv, failed, .. } if !argv.is_empty() => {
            let meta = a.meta.get(i);
            let effect = effect_of(meta.map_or("", |m| m.effect.as_str()), argv, table);
            let key = meta.map(|m| m.key.clone()).filter(|k| !k.is_empty()).unwrap_or_else(|| events::act_key(argv));
            Some((argv, *failed, effect, key))
        }
        _ => None,
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for i in 0..disc.len() {
        let Some((av, failed, effect, key)) = exec(i) else { continue };
        if !matches!(effect, Effect::Observe | Effect::Verify) || takes_value(av) {
            continue;
        }
        // A command that gives an agent nothing to go on (`echo ---` between outputs, `pwd`,
        // `sleep`) is never how a task starts, however often traces ran it.
        if adage_core::action::classify_argv(av).role == adage_core::action::Role::Trivial {
            continue;
        }
        let identity = events::act_identity(av);
        if influence.iter().any(|d| d.at_step as usize <= i && d.names_act(&identity)) {
            continue;
        }
        if failed {
            // A failing check is still the first step of a fix (an inhibit test run is how it
            // starts), unless the trace then switched to another check that worked, which makes
            // it an avoid, not a continuation.
            let switched =
                (i + 1..disc.len()).filter_map(exec).any(|(_, f, e, k)| !f && e == Effect::Verify && k != key);
            if effect != Effect::Verify || switched {
                continue;
            }
        }
        let group = format!("{}|{}", adage_core::keys::command_head(av), effect.as_str());
        if seen.insert(group.clone()) {
            let turn = a.meta.get(i).map_or(i as u32, |m| m.turn);
            out.push(Cand { group, literal: shell_join(av), position: i, turn });
        }
    }
    out
}

fn floor4(x: f32) -> f32 {
    ((x as f64 * 1e4).floor() / 1e4) as f32
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// One compiled intent route: the route (its evidence counted on the traces), why it exists, and
/// the traces that took it.
#[derive(Debug, Clone, PartialEq)]
pub struct IntentRoute {
    pub route: Route,
    pub why: Why,
    pub sessions: BTreeSet<String>,
}

/// What the prompt-intent path found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Intents {
    pub routes: Vec<IntentRoute>,
    /// Families that cleared the purity bar (before evidence and contrast checks).
    pub families: usize,
}

/// Compile prompt-intent prototypes into routes (see the module documentation). `influence[i]`
/// are the directives trace `i` received. `strength(effect, lower bound, rate, id)` is the learner's
/// strength rule (with its hysteresis); `None` drops the family.
pub fn compile(
    sessions: &[Annotated],
    influence: &[Vec<Influence>],
    model: &StaticModel,
    p: &IntentParams,
    provenance: &str,
    strength: &dyn Fn(Effect, f64, f64, &str) -> Option<Strength>,
) -> Intents {
    struct Item<'a> {
        text: String,
        a: &'a Annotated,
        vec: Vec<f32>,
        cands: Vec<Cand>,
    }
    let table = CommandTable::load();
    let none: Vec<Influence> = Vec::new();
    let mut items: Vec<Item> = sessions
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let text = normalize(first_prompt(a));
            let vec = model.embed(&text)?;
            let cands = candidates(a, influence.get(i).unwrap_or(&none), &table);
            Some(Item { text, a, vec, cands })
        })
        .collect();
    items.sort_by(|a, b| (&a.text, &a.a.session.id).cmp(&(&b.text, &b.a.session.id)));
    let n = items.len();
    let min = p.min_support.max(2) as usize;
    if n < min {
        return Intents::default();
    }

    // The continuation each trace took: its first directable command that is not ubiquitous.
    let mut df: BTreeMap<String, usize> = BTreeMap::new();
    for it in &items {
        for c in &it.cands {
            *df.entry(c.group.clone()).or_default() += 1;
        }
    }
    let common = |g: &str| n >= 5 && df[g] as f64 > p.ubiquitous * n as f64;
    let conts: Vec<Option<String>> =
        items.iter().map(|it| it.cands.iter().find(|c| !common(&c.group)).map(|c| c.group.clone())).collect();

    // Embeddings propose; behaviour decides.
    let vecs: Vec<Vec<f32>> = items.iter().map(|it| it.vec.clone()).collect();
    let pur = Purity { min_group: min, ..Default::default() };
    let labels = cluster::families(&vecs, &ClusterParams { min_size: min, ..Default::default() });
    let labels = purity::split(&vecs, &labels, &conts, &pur);
    let labels = purity::merge(&vecs, &labels, &conts, &pur);
    let members = purity::members(&labels);

    let mut out = Intents::default();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for (c, (cont, share, size)) in purity::dominant(&labels, &conts) {
        let Some(group) = cont else { continue };
        if share < pur.bar || size < min {
            continue;
        }
        out.families += 1;
        // The centroid exactly as the runtime will see it.
        let centroid = adage_intent::quantize(&purity::centroid(&vecs, &members[&c]));
        let cent = adage_intent::dequantize(&centroid);
        let sims: Vec<f32> = vecs.iter().map(|v| adage_intent::cosine(v, &cent)).collect();
        let ran: Vec<bool> = items.iter().map(|it| it.cands.iter().any(|x| x.group == group)).collect();
        // Calibrate on the repository's own prompts: every prompt at least `t` similar is an
        // opportunity; the `t` whose share that took the continuation has the best Wilson lower
        // bound wins (ties: the higher `t`).
        let mut ts: Vec<f32> =
            members[&c].iter().map(|&i| floor4(sims[i])).filter(|s| *s >= INTENT_MIN_THRESHOLD).collect();
        ts.sort_by(|a, b| b.total_cmp(a));
        ts.dedup();
        let mut best: Option<(f64, f32, u32, u32)> = None;
        for &t in &ts {
            let t = t.min(INTENT_MAX_THRESHOLD);
            let opp = sims.iter().filter(|s| **s >= t).count() as u32;
            let sup = (0..n).filter(|&i| sims[i] >= t && ran[i]).count() as u32;
            let lb = wilson_lower(sup, opp);
            if best.is_none_or(|b| lb > b.0) {
                best = Some((lb, t, sup, opp));
            }
        }
        let Some((lb, threshold, support, opp)) = best else { continue };
        if support < p.min_support {
            continue;
        }
        let rate = support as f64 / opp as f64;
        let outside = n as u32 - opp;
        let ran_outside = (0..n).filter(|&i| sims[i] < threshold && ran[i]).count() as f64;
        let rate_out = if outside == 0 { 0.0 } else { ran_outside / outside as f64 };
        if rate - rate_out < p.contrast {
            continue;
        }
        let mut variants: BTreeMap<String, u32> = BTreeMap::new();
        let (mut positions, mut turns, mut who) = (Vec::new(), Vec::new(), BTreeSet::new());
        let mut days: BTreeSet<&str> = BTreeSet::new();
        for i in (0..n).filter(|&i| sims[i] >= threshold && ran[i]) {
            let x = items[i].cands.iter().find(|x| x.group == group).expect("ran");
            *variants.entry(x.literal.clone()).or_default() += 1;
            positions.push(x.position as f64);
            turns.push(x.turn as f64);
            who.insert(items[i].a.session.id.clone());
            days.insert(items[i].a.day.as_str());
        }
        let command = variants
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        let observed = group.rsplit('|').next().and_then(Effect::parse).unwrap_or(Effect::Environment);
        // Re-derive the effect from the command that will be directed; never inherit it.
        let effect = super::max_effect(observed, io::classify(&command, &table));
        let base = format!("intent-{}", stable(&[&group]));
        let id = (1..).map(|k| if k == 1 { base.clone() } else { format!("{base}-{k}") }).find(|x| !ids.contains(x));
        let id = id.expect("an unused id");
        let Some(asp) = strength(effect, lb, rate, &id) else { continue };
        let route = Route {
            id: id.clone(),
            summary: "Start requests like these with the command this repository's sessions ran first".into(),
            trigger: Trigger::Intent { family: id.clone(), model: model.id.clone(), centroid, threshold },
            step: Step::Run { command: command.clone() },
            slots: vec![],
            effect,
            strength: asp,
            state: initial_state(effect),
            exit: vec![Exit::IntentChanges],
            evidence: Evidence {
                opportunities: opp,
                support,
                days: days.len() as u32,
                rate,
                lower_bound: lb,
                median_position: median(&mut positions),
                turns_saved: median(&mut turns),
                provenance: provenance.to_string(),
                ..Default::default()
            },
            authored: false,
        };
        // Anything a track could not carry (an unsafe command, say) is dropped here, not later.
        let probe = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "probe/learned".into(),
                    version: "0.0.0".into(),
                    description: String::new(),
                    provenance: String::new(),
                },
                scope: Scope { repo: String::new() },
                gauge: Gauge::default(),
            },
            routes: vec![route.clone()],
        };
        if io::validate(&probe).is_err() {
            continue;
        }
        let why = Why {
            shape: "run".into(),
            pattern: format!("[prompt ~ {id} (τ {threshold:.4})] → [act {command}]"),
            backoff: 1,
            gain: 0.0,
            lift: if rate_out > 0.0 { (rate / rate_out * 1e4).round() / 1e4 } else { 0.0 },
            conf: (rate * 1e4).round() / 1e4,
            conf_lb: (lb * 1e4).round() / 1e4,
            support,
            days: days.len() as u32,
            members: vec![adage_tracks::why::Members {
                item: format!("act {group}"),
                members: variants.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            }],
            examples: who.iter().take(3).cloned().collect(),
            prior: None,
        };
        ids.insert(id);
        out.routes.push(IntentRoute { route, why, sessions: who });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Prompt, Session, StepMeta};

    fn trace(prompt: &str, cmds: &[&str]) -> Annotated {
        let steps: Vec<InStep> = cmds
            .iter()
            .map(|c| InStep::Exec {
                argv: c.split_whitespace().map(str::to_string).collect(),
                output: "ok".into(),
                failed: false,
            })
            .collect();
        let meta = (0..steps.len())
            .map(|i| StepMeta {
                project_pytest_root_cwd: false,
                turn: i as u32,
                prompt: 0,
                effect: "observe".into(),
                key: String::new(),
            })
            .collect();
        Annotated {
            session: Session { id: "s".into(), prompts: vec![Prompt { text: prompt.into(), at_step: 0 }], steps },
            meta,
            day: "2026-09-01".into(),
        }
    }

    #[test]
    fn a_command_given_the_prompts_item_id_is_the_lookups_not_an_intents() {
        // Regression (evals/gen, the `mixed` id shape): the prompt marks the id (`#4280`), the
        // lookup takes it bare (`4280`). That command is the value-flow lookup's.
        let table = CommandTable::load();
        for prompt in ["Please work on #4280.", "Please work on 4280.", "Pick up ops/4280 and get it done."] {
            let c = candidates(&trace(prompt, &["scripts/wi-lookup 4280", "scripts/check tests"]), &[], &table);
            let lits: Vec<&str> = c.iter().map(|x| x.literal.as_str()).collect();
            assert_eq!(lits, ["scripts/check tests"], "{prompt}");
        }
        // A word the prompt shares with a command is not a value.
        let c = candidates(&trace("Fix the failing tests", &["scripts/check tests"]), &[], &table);
        assert_eq!(c.len(), 1);
    }

    /// Regression (2026-10-01, a real repository): agents print `echo ---` between outputs, and
    /// "requests like these usually start by running `echo ---`" was learned. A command that gives
    /// an agent nothing to go on is never a task's first step.
    #[test]
    fn a_trivial_command_is_never_how_a_task_starts() {
        let table = CommandTable::load();
        let c = candidates(&trace("Fix the failing tests", &["echo ---", "pwd", "scripts/check tests"]), &[], &table);
        let lits: Vec<&str> = c.iter().map(|x| x.literal.as_str()).collect();
        assert_eq!(lits, ["scripts/check tests"]);
    }
}
