//! Exposure and stale routes (E1: evidence integrity).
//!
//! Each miner ignores the sessions steered by its own kind of route (Design §11), so a route that
//! is delivered and followed starves itself of evidence: nearly every session where it applies is
//! steered, and once the transcripts from before it expire only the holdout is left to confirm
//! it. Without this module such a route would fall below the support bar and be dropped, then be
//! re-learned from the holdout, then dropped again.
//!
//! **Exposure.** For a route of kind K, the sessions where its trigger occurred ([`occurs`]) are
//! split into the *exposed* ones (steered on K, so excluded from its miner) and the *unsteered*
//! ones. `Evidence::exposed` records the first count on every learned route.
//!
//! **Stale.** A route that was in service but is not found again this run is kept as
//! `RouteState::Stale` ([`adage_tracks::review::keep_stale`]) when [`verdict`] finds:
//!
//! - it is exposed (at least one exposed session), and either at least `cfg.stale_exposure`
//!   ([`STALE_EXPOSURE`] by default) of the
//!   sessions where it applies were exposed, or its unsteered support is below `min_support`; and
//! - there is no contrary evidence: among unsteered sessions ([`probe`]), at least `min_support`
//!   opportunities with a support rate below `retain_rate` (the bar that keeps an existing route)
//!   drops it as before.
//!
//! Anything else is dropped as before. A person's rejection always wins (it is never kept stale).
//! Derail flags live in the spools, which learning does not read: they surface in `adage review`,
//! where retiring the route is the person's call.

use crate::import::Session;
use crate::intent;
use crate::mine::{argv, glob_for, runner, shell_join, LearnConfig};
use adage_core::spans;
use adage_core::Effect;
use adage_intent::model::StaticModel;
use adage_intent::normalize::normalize;
use adage_tracks::{Route, Step, Trigger};
use std::collections::BTreeMap;

/// The share of a route's occurrences that must be exposed for its absence to be put down to
/// exposure alone, whatever its unsteered support. The default of `LearnConfig::stale_exposure`.
pub const STALE_EXPOSURE: f64 = 0.5;

/// Where a route's trigger occurred, split by whether the session was steered on its kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Exposure {
    /// Sessions steered on the route's kind (excluded from its miner).
    pub exposed: u32,
    /// Sessions not steered on the route's kind.
    pub unsteered: u32,
}

/// Unsteered evidence for a route the miner did not find: sessions where it applied, and those
/// that did what it directs, counted as its miner counts them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Probe {
    pub opportunities: u32,
    pub support: u32,
}

/// Prompt embeddings, computed at most once per session and only when an intent route asks.
pub struct Embeddings<'a> {
    model: Option<&'a StaticModel>,
    cache: BTreeMap<String, Option<Vec<f32>>>,
}

impl<'a> Embeddings<'a> {
    pub fn new(model: Option<&'a StaticModel>) -> Self {
        Embeddings { model, cache: BTreeMap::new() }
    }

    /// Whether `s`'s prompt means what the intent trigger means (the runtime's test). A route
    /// learned with another model, or no model at all, never matches.
    fn matches(&mut self, s: &Session, model_id: &str, centroid: &[i16], threshold: f32) -> bool {
        let Some(m) = self.model.filter(|m| m.id == model_id) else { return false };
        let v = self.cache.entry(s.id.clone()).or_insert_with(|| m.embed(&normalize(&s.prompt)));
        match v {
            Some(v) if v.len() == centroid.len() => {
                adage_intent::cosine(v, &adage_intent::dequantize(centroid)) >= threshold
            }
            _ => false,
        }
    }
}

/// Whether `r`'s trigger occurred in `s`: the situations its miner counts as opportunities, judged
/// the same way whether or not the session was steered.
///
/// | Trigger | Occurs when the session |
/// | --- | --- |
/// | `prompt` | has a span of that kind in its first prompt |
/// | `after_command` | ran the command successfully |
/// | `after_failure` | ran the command and it failed with the signature |
/// | `after_edit` | edited a file of the glob |
/// | `session_start` (a dead end) | ran a runner it avoids, or the command it suggests instead: the verify step a steered agent runs directly |
/// | `intent` | has a first prompt that clears the family's threshold (with the same model) |
/// | `context` | has the items' events in order, each within the gap of the one before, the gap counted over the events of any of its items (the runtime counts the repository's whole alphabet); prompt items are not read from traces |
pub fn occurs(r: &Route, s: &Session, emb: &mut Embeddings) -> bool {
    match &r.trigger {
        Trigger::Prompt { span } => spans::extract(&s.prompt).iter().any(|x| x.kind == *span),
        Trigger::AfterCommand { command } => s.steps.iter().any(|x| x.key == *command && !x.call.failed),
        Trigger::AfterFailure { command, signature } => {
            s.steps.iter().any(|x| x.key == *command && x.call.failed && x.call.output.contains(signature.as_str()))
        }
        Trigger::AfterEdit { paths } => s.steps.iter().any(|x| {
            adage_core::keys::is_author(&x.norm.action)
                && x.rel_path.as_deref().and_then(glob_for).is_some_and(|g| g == *paths)
        }),
        Trigger::SessionStart => match &r.step {
            Step::Avoid { avoid, instead, .. } => {
                let avoided: Vec<&str> = avoid.split(" / ").collect();
                s.steps.iter().any(|x| {
                    x.norm.effect == Effect::Verify
                        && argv(x)
                            .is_some_and(|av| avoided.contains(&runner(av).as_str()) || shell_join(av) == *instead)
                })
            }
            // A session-start route of another shape applies to every session.
            _ => true,
        },
        Trigger::Intent { model, centroid, threshold, .. } => emb.matches(s, model, centroid, *threshold),
        Trigger::Context { items, within } => context_occurs(items, *within, s),
    }
}

/// Whether a trace has a context's items as a gapped subsequence (see [`occurs`]).
fn context_occurs(items: &[adage_tracks::CtxItem], within: u8, s: &Session) -> bool {
    use adage_tracks::CtxItem;
    let sat = |it: &CtxItem, x: &crate::import::StepObs| match it {
        CtxItem::Act { keys } => !x.call.failed && keys.iter().any(|k| adage_core::pattern::meets(k, &x.key)),
        CtxItem::Fail { keys, signature } => {
            x.call.failed
                && x.call.output.contains(signature.as_str())
                && keys.iter().any(|k| adage_core::pattern::meets(k, &x.key))
        }
        CtxItem::Edit { glob } => {
            adage_core::keys::is_author(&x.norm.action)
                && x.rel_path.as_deref().is_some_and(|p| glob::Pattern::new(glob).is_ok_and(|g| g.matches(p)))
        }
        CtxItem::PromptValue { .. } | CtxItem::Intent { .. } => false,
    };
    // The events of any item, in order, and which items each satisfies.
    let alpha: Vec<Vec<bool>> = s
        .steps
        .iter()
        .map(|x| items.iter().map(|it| sat(it, x)).collect::<Vec<bool>>())
        .filter(|v| v.iter().any(|b| *b))
        .collect();
    // Reachable end positions of each prefix of the items.
    let mut ends: Vec<usize> = (0..alpha.len()).filter(|&q| alpha[q][0]).collect();
    for i in 1..items.len() {
        ends = (0..alpha.len())
            .filter(|&q| alpha[q][i] && ends.iter().any(|&p| p < q && q - p <= within as usize))
            .collect();
    }
    !ends.is_empty()
}

/// [`Exposure`] of `r` (of miner kind `kind`) over a repository's sessions.
pub fn exposure(r: &Route, kind: &str, sessions: &[&Session], emb: &mut Embeddings) -> Exposure {
    let mut e = Exposure::default();
    for s in sessions {
        if occurs(r, s, emb) {
            if s.steered_by(kind) {
                e.exposed += 1;
            } else {
                e.unsteered += 1;
            }
        }
    }
    e
}

/// Unsteered evidence for `r` among `unsteered` (the sessions its miner saw), counted as its miner
/// counts it; `None` where it cannot be judged.
///
/// | Route | Opportunities | Support |
/// | --- | --- | --- |
/// | Lookup (`prompt` → `run`) | a span of the slot's kind in the prompt | ran a command with the same head, with the prompt's value, successfully, before the first edit |
/// | Linked files | as the miner (`read_batch_routes`) | as the miner |
/// | Finish line | edited a file of the glob | ran the command successfully after the last such edit |
/// | Dead end | ran a runner it avoids | every such run failed |
/// | Intent | the prompt clears the threshold | ran a directable command with the same head before the first edit |
pub fn probe(r: &Route, unsteered: &[&Session], cfg: &LearnConfig, emb: &mut Embeddings) -> Option<Probe> {
    let mut p = Probe::default();
    match (&r.trigger, &r.step) {
        (Trigger::Prompt { span }, Step::Run { command }) => {
            let head = head_of(command);
            for s in unsteered {
                let values: Vec<String> =
                    spans::extract(&s.prompt).into_iter().filter(|x| x.kind == *span).map(|x| x.value).collect();
                if values.is_empty() {
                    continue;
                }
                p.opportunities += 1;
                let did = s.discovery().iter().any(|x| {
                    !x.call.failed
                        && argv(x).is_some_and(|av| {
                            av.iter().any(|a| values.contains(a)) && adage_core::keys::command_head(av) == head
                        })
                });
                p.support += u32::from(did);
            }
        }
        (Trigger::AfterCommand { command }, Step::ReadBatch { .. }) => {
            let relaxed = relaxed(cfg);
            let found = crate::mine::read_batch_routes(unsteered, command, &relaxed).into_iter().find(|x| x.id == r.id);
            return Some(match found {
                Some(x) => Probe { opportunities: x.evidence.opportunities, support: x.evidence.support },
                None => Probe::default(),
            });
        }
        (Trigger::AfterEdit { paths }, Step::Run { command }) => {
            for s in unsteered {
                let last = s.steps.iter().rposition(|x| {
                    adage_core::keys::is_author(&x.norm.action)
                        && x.rel_path.as_deref().and_then(glob_for).is_some_and(|g| g == *paths)
                });
                let Some(last) = last else { continue };
                p.opportunities += 1;
                let ran = s.steps.iter().skip(last + 1).any(|x| {
                    x.norm.effect == Effect::Regenerate
                        && !x.call.failed
                        && argv(x).is_some_and(|av| shell_join(av) == *command)
                });
                p.support += u32::from(ran);
            }
        }
        (Trigger::SessionStart, Step::Avoid { avoid, .. }) => {
            let avoided: Vec<&str> = avoid.split(" / ").collect();
            for s in unsteered {
                let runs: Vec<bool> = s
                    .steps
                    .iter()
                    .filter(|x| x.norm.effect == Effect::Verify)
                    .filter(|x| argv(x).is_some_and(|av| avoided.contains(&runner(av).as_str())))
                    .map(|x| x.call.failed)
                    .collect();
                if runs.is_empty() {
                    continue;
                }
                p.opportunities += 1;
                p.support += u32::from(runs.iter().all(|f| *f));
            }
        }
        (Trigger::Intent { model, centroid, threshold, .. }, Step::Run { command }) => {
            let head = head_of(command);
            for s in unsteered {
                if !emb.matches(s, model, centroid, *threshold) {
                    continue;
                }
                p.opportunities += 1;
                let took = intent::candidates(s).iter().any(|c| c.group.split('|').next() == Some(head.as_str()));
                p.support += u32::from(took);
            }
        }
        _ => return None,
    }
    Some(p)
}

/// The head of a command template, as the miners group commands (`keys::command_head`).
fn head_of(command: &str) -> String {
    let av: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    adage_core::keys::command_head(&av)
}

/// Thresholds under which a miner reports whatever it finds, for [`probe`].
fn relaxed(cfg: &LearnConfig) -> LearnConfig {
    LearnConfig { min_support: 1, yellow_rate: 0.0, retain_rate: 0.0, green_lower_bound: f64::INFINITY, ..cfg.clone() }
}

/// For a route that was in service and was not found again: `Some(exposed)` to keep it stale,
/// `None` to drop it (see the module documentation).
pub fn verdict(
    r: &Route,
    kind: &str,
    sessions: &[&Session],
    unsteered: &[&Session],
    cfg: &LearnConfig,
    emb: &mut Embeddings,
) -> Option<u32> {
    let e = exposure(r, kind, sessions, emb);
    if e.exposed == 0 {
        return None;
    }
    let p = probe(r, unsteered, cfg, emb);
    let contrary = p.is_some_and(|p| {
        p.opportunities >= cfg.min_support && (p.support as f64) < cfg.retain_rate * p.opportunities as f64
    });
    if contrary {
        return None;
    }
    let share = e.exposed as f64 / (e.exposed + e.unsteered) as f64;
    let thin = p.map_or(e.unsteered, |p| p.support) < cfg.min_support;
    (share >= cfg.stale_exposure || thin).then_some(e.exposed)
}

/// [`verdict`] from counts alone, with no dispatch on the route's shape: the graph engine's
/// scorer counts, for a reflex's own pattern, the traces where it occurs only through exposed
/// continuation events (`exposed`), the unsteered traces where its context occurs
/// (`opportunities`) and those where it occurs (`support`) (Track G §6.3).
pub fn verdict_counts(exposed: u32, opportunities: u32, support: u32, cfg: &LearnConfig) -> Option<u32> {
    if exposed == 0 {
        return None;
    }
    let contrary = opportunities >= cfg.min_support && (support as f64) < cfg.retain_rate * opportunities as f64;
    if contrary {
        return None;
    }
    let share = exposed as f64 / (exposed + opportunities) as f64;
    (share >= cfg.stale_exposure || support < cfg.min_support).then_some(exposed)
}
