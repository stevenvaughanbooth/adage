//! The graph engine inside `adage learn` (Track G G0–G1), when `LearnParams::engine` is `shadow`
//! or `graph`.
//!
//! - [`facts`]: every session's facts (stage S2, `adage_graph::facts`), through the facts cache
//!   (`$ADAGE_HOME/cache/facts`), in parallel and in session-id order. Nothing decides on them
//!   yet (G2 mines them); `adage learn --json` reports their counts, and
//!   `adage graph --learned --dump` prints them.
//! - [`lookup_routes`]: the lookup miner on learned value types. Instead of TICKET spans and
//!   `spans::ticket_contract`, it takes its slot and contract from `adage_graph::learn`: the value
//!   slots whose flow comes mostly from the prompt and that have a safe type, most sessions
//!   first. Each is judged exactly as the miner judges its candidate (sessions whose prompt holds
//!   a value of the type are the opportunities; running the command with that value before the
//!   first edit is support), and **the effect ceiling comes before anything else**: a slot whose
//!   command may never be directed (`gh issue close {item}`, Publish) is dropped, however
//!   common. The route keeps the id `lookup-work-item`, so review decisions survive a change
//!   of engine; its slot is `SlotSource::PromptValue` with the learned type, and its contract the
//!   type's regex.

use crate::bridge;
use crate::import::Session;
use crate::mine::{aspect_for, days, median, models, shell_join, LearnConfig, MaxRank};
use adage_core::spans::SpanKind;
use adage_core::{tokens, vtype, Effect};
use adage_graph::embed::Embedder;
use adage_graph::facts::{SessionFacts, Stats};
use adage_graph::flow;
use adage_intent::model::StaticModel;
use adage_tracks::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

/// What computing session facts did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FactsReport {
    pub stats: Stats,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8)
}

/// Session facts for these sessions (see the module documentation), with the cache when `cache`
/// names a directory. The same bytes for any thread count.
pub fn compute_facts(
    ss: &[&Session],
    model: Option<&StaticModel>,
    cache: Option<&std::path::Path>,
    threads: usize,
) -> (Vec<SessionFacts>, usize, usize) {
    let annotated: Vec<_> = ss.iter().map(|s| bridge::annotate(s)).collect();
    let e = Embedder::new(model);
    match cache {
        Some(dir) => {
            let c = adage_graph::cache::FactsCache::open(dir);
            let f = adage_graph::facts::facts_all_with(&annotated, threads, |a| c.get(a, &e));
            (f, c.hits.load(Ordering::Relaxed), c.misses.load(Ordering::Relaxed))
        }
        None => (adage_graph::facts::facts_all(&annotated, &e, threads), 0, 0),
    }
}

/// [`compute_facts`] for one repository's sessions in a learn run, summarised.
pub fn facts(ss: &[&Session], model: Option<&StaticModel>, cfg: &LearnConfig) -> FactsReport {
    let (all, hits, misses) = compute_facts(ss, model, cfg.facts_cache.as_deref(), threads());
    FactsReport { stats: Stats::of(&all), cache_hits: hits, cache_misses: misses }
}

/// A learned type as a track stores it.
pub fn stored(t: &adage_graph::types::ValueType) -> ValueType {
    ValueType { id: t.id.clone(), runs: t.runs.clone(), regex: t.regex.clone(), samples_hash: t.samples_hash.clone() }
}

/// The values of a type in a session's prompts before its first edit, in order of appearance,
/// as the runtime would find them (`adage_core::tokens`, then `vtype::matches`).
pub fn prompt_values(s: &Session, t: &vtype::Runs) -> Vec<String> {
    let window = s.discovery().len() as u32;
    let texts: Vec<&str> = if s.prompts.is_empty() {
        vec![s.prompt.as_str()]
    } else {
        s.prompts.iter().filter(|p| p.at_step <= window).map(|p| p.text.as_str()).collect()
    };
    let mut out: Vec<String> = Vec::new();
    for text in texts {
        for tok in tokens::tokens(text, tokens::PROMPT_TOKENS) {
            if vtype::matches(t, &tok.text) && !out.contains(&tok.text) {
                out.push(tok.text);
            }
        }
    }
    out
}

/// How a session's prompt first wrote `value`: its [`tokens::Mention`].
fn mention_of(s: &Session, value: &str) -> Option<tokens::Mention> {
    let texts: Vec<&str> = if s.prompts.is_empty() {
        vec![s.prompt.as_str()]
    } else {
        s.prompts.iter().map(|p| p.text.as_str()).collect()
    };
    texts.into_iter().find_map(|text| {
        tokens::with_mentions(text, tokens::PROMPT_TOKENS).into_iter().find(|(t, _)| t.text == value).map(|(_, m)| m)
    })
}

/// The mentions a route requires: every supporting session wrote the value one of at most two
/// ways, none of them the bare token. Otherwise none (any token of the type binds), which is
/// what a bare id shape (`BIL-1041`, `ops/412`) or a mix with one gives.
fn learned_mentions(seen: &BTreeMap<tokens::Mention, u32>, support: u32) -> Vec<tokens::Mention> {
    let total: u32 = seen.values().sum();
    let bare = seen.keys().any(|m| m.prefix.is_empty() && m.suffix.is_empty());
    if total < support || bare || seen.is_empty() || seen.len() > 2 {
        return Vec::new();
    }
    seen.keys().cloned().collect()
}

/// `argv` with the sink's token replaced by the slot (`{item}`, with the sink's affix), as a
/// positional or as an option's `=` value.
fn template(av: &[String], token: &str, slot: &str) -> Vec<String> {
    let mut done = false;
    av.iter()
        .map(|a| {
            if done {
                return a.clone();
            }
            if a == token {
                done = true;
                return slot.to_string();
            }
            if let Some(opt) = a.strip_suffix(token).filter(|o| o.starts_with('-') && o.ends_with('=')) {
                done = true;
                return format!("{opt}{slot}");
            }
            a.clone()
        })
        .collect()
}

/// Judge one learned slot as the lookup's (see the module documentation).
fn route_for(
    sessions: &[&Session],
    sink: &flow::SinkKey,
    t: &adage_graph::types::ValueType,
    cfg: &LearnConfig,
) -> Option<Route> {
    let slot = format!("{}{{item}}{}", sink.prefix, sink.suffix);
    let mut opportunities = 0u32;
    let mut support: BTreeSet<String> = BTreeSet::new();
    let (mut positions, mut turns) = (Vec::new(), Vec::new());
    let mut variants: BTreeMap<String, u32> = BTreeMap::new();
    let mut effect: Option<Effect> = None;
    // How each supporting session's prompt wrote the value (`#` in `#482`).
    let mut mentions: BTreeMap<tokens::Mention, u32> = BTreeMap::new();
    for s in sessions {
        let values = prompt_values(s, &t.runs);
        if values.is_empty() {
            continue;
        }
        opportunities += 1;
        'steps: for (i, step) in s.discovery().iter().enumerate() {
            let Some(av) = crate::mine::argv(step) else { continue };
            if step.call.failed {
                continue;
            }
            for (identity, at, token) in flow::sinks(av) {
                if identity != sink.identity || at != sink.at {
                    continue;
                }
                let v = token.strip_prefix(sink.prefix.as_str()).and_then(|x| x.strip_suffix(sink.suffix.as_str()));
                if !v.is_some_and(|v| values.iter().any(|x| x == v)) {
                    continue;
                }
                *variants.entry(shell_join(&template(av, &token, &slot))).or_default() += 1;
                if let Some(m) = mention_of(s, v.unwrap_or_default()) {
                    *mentions.entry(m).or_default() += 1;
                }
                support.insert(s.id.clone());
                positions.push(i as f64);
                turns.push(step.turn as f64);
                effect = Some(effect.map_or(step.norm.effect, |e| e.max_by_rank(step.norm.effect)));
                break 'steps;
            }
        }
    }
    let command = variants.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(k, _)| k.clone())?;
    // The effect of what would be directed, never inherited; the ceiling comes first.
    let effect = effect
        .unwrap_or(Effect::Environment)
        .max_by_rank(adage_tracks::io::classify(&command, &adage_core::CommandTable::load()));
    ceiling(effect, false)?;
    let n = support.len() as u32;
    if n < cfg.min_support {
        return None;
    }
    let rate = n as f64 / opportunities.max(1) as f64;
    let lb = wilson_lower(n, opportunities);
    let aspect = aspect_for(effect, false, lb, rate, cfg, "lookup-work-item")?;
    Some(Route {
        id: "lookup-work-item".into(),
        summary: "Look up the work item named in the prompt".into(),
        trigger: Trigger::Prompt { span: SpanKind::Ticket },
        step: Step::Run { command },
        slots: vec![Slot {
            name: "item".into(),
            source: SlotSource::PromptValue { vtype: stored(t), mentions: learned_mentions(&mentions, n) },
            contract: Contract::Pattern { regex: t.regex.clone() },
        }],
        effect,
        aspect,
        state: initial_state(effect),
        exit: vec![Exit::IntentChanges],
        evidence: Evidence {
            opportunities,
            support: n,
            days: days(sessions, &support),
            rate,
            lower_bound: lb,
            median_position: median(&mut positions),
            turns_saved: median(&mut turns),
            provenance: cfg.provenance.clone(),
            models: models(sessions),
            ..Default::default()
        },
        authored: false,
    })
}

/// The canonical key of the learned lookup's calls, for the linked-files route to trigger on: the
/// most common key of the calls that instantiate the directed command (its words, with the slot's
/// word filled by a value). A key is taken from real calls because a value's shape decides its
/// placeholder (`482`, `ops/412` and a UUID template differently), so no stand-in value would do.
pub fn lookup_key(sessions: &[&Session], r: &Route) -> Option<String> {
    let Step::Run { command } = &r.step else { return None };
    let tw: Vec<&str> = command.split_whitespace().collect();
    let fits = |argv: &[String]| {
        let mut words: Vec<String> = argv.to_vec();
        if let Some(first) = words.first_mut() {
            if let Some(rest) = first.strip_prefix("./") {
                *first = rest.to_string();
            }
        }
        words.len() >= tw.len()
            && tw.iter().zip(&words).all(|(t, w)| match t.split_once("{item}") {
                Some((pre, suf)) => w.len() > pre.len() + suf.len() && w.starts_with(pre) && w.ends_with(suf),
                None => t.trim_start_matches("./") == w.as_str(),
            })
    };
    let mut keys: BTreeMap<&str, u32> = BTreeMap::new();
    for s in sessions {
        for st in s.discovery() {
            if let Some(av) = crate::mine::argv(st) {
                if !st.call.failed && fits(av) {
                    *keys.entry(st.key.as_str()).or_default() += 1;
                }
            }
        }
    }
    keys.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0))).map(|(k, _)| k.to_string())
}

/// The lookup route from learned value types (see the module documentation): at most one.
pub fn lookup_routes(sessions: &[&Session], cfg: &LearnConfig) -> Vec<Route> {
    // The lookup is judged in the discovery window (before the first edit), so value flow is
    // learned there too: the same slots for the lookup, at a fraction of the work.
    let inputs: Vec<adage_graph::input::Session> =
        sessions.iter().map(|s| bridge::annotate_prefix(s, s.discovery().len()).session).collect();
    let learned = adage_graph::learn(&inputs, &adage_graph::Params { threads: threads(), ..Default::default() });
    let mut cands: Vec<(&flow::Slot, &adage_graph::types::ValueType)> = learned
        .slots
        .iter()
        .filter_map(|st| {
            let t = st.vtype()?;
            let prompt = st.slot.sources.get("prompt").copied().unwrap_or(0);
            let all: u32 = st.slot.sources.values().sum();
            (prompt * 2 > all).then_some((&st.slot, t))
        })
        .collect();
    cands.sort_by(|a, b| b.0.sessions.cmp(&a.0.sessions).then_with(|| a.0.sink.cmp(&b.0.sink)));
    cands.into_iter().find_map(|(slot, t)| route_for(sessions, &slot.sink, t, cfg)).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{PromptObs, StepObs};
    use adage_core::types::CanonicalAction;

    fn exec(i: u32, cmd: &str, effect: Effect) -> StepObs {
        let action = CanonicalAction::Exec { argv: cmd.split(' ').map(str::to_string).collect(), cwd: "/r".into() };
        StepObs {
            turn: i,
            key: adage_core::keys::template_key(&action),
            norm: adage_core::Normalized { action, effect, mutating: false, confidence: 1.0, note: String::new() },
            call: adage_runtime::Call {
                tool: "Bash".into(),
                input: serde_json::Value::Null,
                output: "ok".into(),
                failed: false,
            },
            rel_path: None,
            prompt: 0,
        }
    }

    fn session(i: usize, prompt: &str, cmds: &[(&str, Effect)]) -> Session {
        Session {
            id: format!("s{i:02}"),
            root: "/r".into(),
            repo: "acme".into(),
            model: None,
            day: format!("2026-01-{:02}", 1 + i % 20),
            prompt: prompt.into(),
            steps: cmds.iter().enumerate().map(|(k, (c, e))| exec(k as u32, c, *e)).collect(),
            steered: false,
            directed: BTreeSet::new(),
            steered_kinds: BTreeSet::new(),
            outcome: None,
            harness: "claude-code".into(),
            prompts: vec![PromptObs { text: prompt.into(), at_step: 0 }],
        }
    }

    fn cfg() -> LearnConfig {
        LearnConfig { engine: "graph".into(), ..LearnConfig::demo() }
    }

    #[test]
    fn learns_a_lookup_for_an_id_shape_the_ticket_detector_misses() {
        let ss: Vec<Session> = (0..8)
            .map(|i| {
                let n = 400 + 17 * i;
                session(
                    i,
                    &format!("Please fix #{n} today"),
                    &[
                        ("ls", Effect::Observe),
                        (&format!("./scripts/wi-lookup {n}"), Effect::Observe),
                        ("make check", Effect::Verify),
                    ],
                )
            })
            .collect();
        let refs: Vec<&Session> = ss.iter().collect();
        assert!(crate::mine::prompt_routes(&refs, &cfg()).is_empty(), "no TICKET span in `#482`");
        let r = lookup_routes(&refs, &cfg());
        assert_eq!(r.len(), 1);
        let r = &r[0];
        assert_eq!(r.id, "lookup-work-item");
        assert_eq!(r.step, Step::Run { command: "./scripts/wi-lookup {item}".into() });
        let SlotSource::PromptValue { vtype, mentions } = &r.slots[0].source else { panic!("{:?}", r.slots) };
        assert_eq!(r.slots[0].contract, Contract::Pattern { regex: vtype.regex.clone() });
        assert_eq!(mentions, &[tokens::Mention { prefix: "#".into(), suffix: String::new() }]);
        assert_eq!(vtype.regex, "^(?:[0-9]{3})$");
        assert_eq!((r.evidence.support, r.evidence.opportunities), (8, 8));
        let t = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "acme/work-items".into(),
                    version: "0.1.0".into(),
                    description: String::new(),
                    provenance: String::new(),
                },
                scope: Scope { repo: "acme".into() },
                gauge: Gauge::default(),
            },
            routes: vec![r.clone()],
        };
        adage_tracks::io::validate(&t).unwrap();
    }

    #[test]
    fn the_effect_ceiling_drops_a_slot_that_may_never_be_directed() {
        // Every session closes the issue the prompt names; a few also view it first.
        let ss: Vec<Session> = (0..10)
            .map(|i| {
                let n = 500 + 13 * i;
                let mut cmds = vec![(format!("gh issue close {n}"), Effect::Publish)];
                if i % 2 == 0 {
                    cmds.insert(0, (format!("gh issue view {n}"), Effect::Observe));
                }
                let cmds: Vec<(&str, Effect)> = cmds.iter().map(|(c, e)| (c.as_str(), *e)).collect();
                session(i, &format!("Close out #{n}"), &cmds)
            })
            .collect();
        let refs: Vec<&Session> = ss.iter().collect();
        let r = lookup_routes(&refs, &cfg());
        assert!(r.iter().all(|r| !matches!(&r.step, Step::Run { command } if command.contains("close"))), "{r:?}");
        // The view slot is judged on its own evidence: 5 of 10 sessions, yellow at the demo bar.
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].step, Step::Run { command: "gh issue view {item}".into() });
    }
}
