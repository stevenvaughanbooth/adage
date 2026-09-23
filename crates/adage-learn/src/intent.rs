//! Intent routes (Design §8.2.1): prompts that mean the same thing *and* lead agents to do the same
//! thing, and the command those sessions ran first.
//!
//! Per repository: normalise and embed each session's prompt; order the prompts canonically
//! (normalised text, then session id) so the result does not depend on input order; cluster them
//! with kNN + Louvain (spike S7); label each session with its *route* — the first directable
//! command it ran before editing that is not run by nearly every session; split and merge
//! families by behaviour; keep families whose dominant route clears the purity bar. Each kept
//! family becomes a route whose trigger is the family's centroid and a similarity threshold
//! calibrated on the repository's own prompts, and whose step is the command. Commands that take
//! a value from the prompt belong to the work-item miner, not here: intent routes bind nothing.

use crate::import::Session;
use crate::mine::{argv, aspect_for, days, median, models, shell_join, stable, LearnConfig, MaxRank};
use adage_core::{spans, CommandTable, Effect};
use adage_intent::cluster::{self, Params};
use adage_intent::model::StaticModel;
use adage_intent::normalize::normalize;
use adage_intent::purity::{self, Purity};
use adage_tracks::*;
use std::collections::{BTreeMap, BTreeSet};

/// Commands run before the first edit by more than this share of a repository's sessions (of at
/// least five) are not specific to any intent.
const UBIQUITOUS: f64 = 0.8;
/// A family's command must be this much more common among matching prompts than among the rest.
const CONTRAST: f64 = 0.3;
/// No intent route ever matches below this similarity, whatever calibration says.
const FLOOR: f32 = INTENT_MIN_THRESHOLD;

struct Cand {
    group: String,
    literal: String,
    position: usize,
    turn: u32,
}

/// Directable commands (observe or verify) a session ran before its first edit, one per command
/// group, in order.
fn candidates(s: &Session) -> Vec<Cand> {
    let from_prompt: BTreeSet<String> = spans::extract(&s.prompt).into_iter().map(|x| x.value).collect();
    let disc = s.discovery();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (i, st) in disc.iter().enumerate() {
        let Some(av) = argv(st) else { continue };
        if !matches!(st.norm.effect, Effect::Observe | Effect::Verify) || av.iter().any(|a| from_prompt.contains(a)) {
            continue;
        }
        if st.call.failed {
            // A failing verify run is still the first step of a fix (a red test run is how it
            // starts) — unless the session then switched to a different verify command that
            // worked, which makes it a dead end, not a route.
            let switched = disc
                .iter()
                .skip(i + 1)
                .any(|b| argv(b).is_some() && !b.call.failed && b.norm.effect == Effect::Verify && b.key != st.key);
            if st.norm.effect != Effect::Verify || switched {
                continue;
            }
        }
        let group = format!("{}|{}", adage_core::keys::command_head(av), st.norm.effect.as_str());
        if seen.insert(group.clone()) {
            out.push(Cand { group, literal: shell_join(av), position: i, turn: st.turn });
        }
    }
    out
}

#[derive(Default)]
pub struct Mined {
    pub routes: Vec<Route>,
    /// Families that cleared the purity bar (before evidence and contrast checks).
    pub families: usize,
}

fn floor4(x: f32) -> f32 {
    ((x as f64 * 1e4).floor() / 1e4) as f32
}

pub fn intent_routes(sessions: &[&Session], model: &StaticModel, cfg: &LearnConfig) -> Mined {
    struct Item<'a> {
        text: String,
        session: &'a Session,
        vec: Vec<f32>,
        cands: Vec<Cand>,
    }
    let mut items: Vec<Item> = sessions
        .iter()
        .filter_map(|s| {
            let text = normalize(&s.prompt);
            let vec = model.embed(&text)?;
            Some(Item { text, session: s, vec, cands: candidates(s) })
        })
        .collect();
    items.sort_by(|a, b| (&a.text, &a.session.id).cmp(&(&b.text, &b.session.id)));
    let n = items.len();
    let min = cfg.min_support.max(2) as usize;
    if n < min {
        return Mined::default();
    }

    // The route each session took: its first directable command that is not ubiquitous.
    let mut df: BTreeMap<String, usize> = BTreeMap::new();
    for it in &items {
        for c in &it.cands {
            *df.entry(c.group.clone()).or_default() += 1;
        }
    }
    let common = |g: &str| n >= 5 && df[g] as f64 > UBIQUITOUS * n as f64;
    let routes: Vec<Option<String>> =
        items.iter().map(|it| it.cands.iter().find(|c| !common(&c.group)).map(|c| c.group.clone())).collect();

    // Embeddings propose; behaviour decides.
    let vecs: Vec<Vec<f32>> = items.iter().map(|it| it.vec.clone()).collect();
    let pur = Purity { min_group: min, ..Default::default() };
    let labels = cluster::families(&vecs, &Params { min_size: min, ..Default::default() });
    let labels = purity::split(&vecs, &labels, &routes, &pur);
    let labels = purity::merge(&vecs, &labels, &routes, &pur);
    let members = purity::members(&labels);

    let table = CommandTable::load();
    let mut mined = Mined::default();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for (c, (route, share, size)) in purity::dominant(&labels, &routes) {
        let Some(group) = route else { continue };
        if share < pur.bar || size < min {
            continue;
        }
        mined.families += 1;
        // The centroid exactly as the runtime will see it.
        let centroid = adage_intent::quantize(&purity::centroid(&vecs, &members[&c]));
        let cent = adage_intent::dequantize(&centroid);
        let sims: Vec<f32> = vecs.iter().map(|v| adage_intent::cosine(v, &cent)).collect();
        let ran: Vec<bool> = items.iter().map(|it| it.cands.iter().any(|x| x.group == group)).collect();
        // Calibrate on the repository's own prompts: every prompt at least `t` similar is an
        // opportunity; choose the `t` whose share that took the route has the best Wilson lower
        // bound (ties: the higher `t`).
        let mut ts: Vec<f32> = members[&c].iter().map(|&i| floor4(sims[i])).filter(|s| *s >= FLOOR).collect();
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
        if support < cfg.min_support {
            continue;
        }
        let rate = support as f64 / opp as f64;
        let outside = n as u32 - opp;
        let ran_outside = (0..n).filter(|&i| sims[i] < threshold && ran[i]).count() as f64;
        let rate_out = if outside == 0 { 0.0 } else { ran_outside / outside as f64 };
        if rate - rate_out < CONTRAST {
            continue;
        }
        let mut variants: BTreeMap<String, u32> = BTreeMap::new();
        let (mut positions, mut turns, mut who) = (Vec::new(), Vec::new(), BTreeSet::new());
        for i in (0..n).filter(|&i| sims[i] >= threshold && ran[i]) {
            let x = items[i].cands.iter().find(|x| x.group == group).expect("ran");
            *variants.entry(x.literal.clone()).or_default() += 1;
            positions.push(x.position as f64);
            turns.push(x.turn as f64);
            who.insert(items[i].session.id.clone());
        }
        let command = variants
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        let observed = group.rsplit('|').next().and_then(Effect::parse).unwrap_or(Effect::Environment);
        // Re-derive the effect from the command that will be directed; never inherit it.
        let effect = observed.max_by_rank(io::classify(&command, &table));
        let base = format!("intent-{}", stable(&[&group]));
        let id = (1..)
            .map(|k| if k == 1 { base.clone() } else { format!("{base}-{k}") })
            .find(|x| !ids.contains(x))
            .unwrap();
        let Some(aspect) = aspect_for(effect, false, lb, rate, cfg, &id) else { continue };
        let route = Route {
            id: id.clone(),
            summary: "Start requests like these with the command this repository's sessions ran first".into(),
            trigger: Trigger::Intent { family: id.clone(), model: model.id.clone(), centroid, threshold },
            step: Step::Run { command },
            slots: vec![],
            effect,
            aspect,
            state: RouteState::Active,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence {
                opportunities: opp,
                support,
                days: days(sessions, &who),
                rate,
                lower_bound: lb,
                median_position: median(&mut positions),
                turns_saved: median(&mut turns),
                provenance: cfg.provenance.clone(),
                models: models(sessions),
                ..Default::default()
            },
            authored: false,
        };
        // Anything a track could not carry (an unsafe command, say) is dropped here, not later.
        let probe = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "probe/intents".into(),
                    version: "0.0.0".into(),
                    description: String::new(),
                    provenance: String::new(),
                },
                scope: Scope { repo: String::new() },
                gauge: Gauge::default(),
            },
            routes: vec![route.clone()],
        };
        if io::validate(&probe).is_ok() {
            ids.insert(id);
            mined.routes.push(route);
        }
    }
    mined
}
