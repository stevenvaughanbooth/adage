//! Value flow inside `adage condition` (Track G G0–G1).
//!
//! - [`compute_facts`]: every session's facts (stage S2, `adage_graph::facts`), through the facts
//!   cache (`$ADAGE_HOME/cache/facts`), in parallel and in session-id order, for
//!   `adage graph --learned --dump`.
//! - [`lookup_routes`]: the lookup, a value-flow reflex (`[prompt] =flow=> [act …]`), with its
//!   slot and contract from `adage_graph::learn`: the value slots whose flow comes mostly from the
//!   prompt and that have a safe type, most traces first. Each is judged on the traces (those
//!   whose prompt holds a value of the type are the opportunities; running the command with that
//!   value before the first edit is support; a trace a directive steered to the command is left
//!   out), and **the effect ceiling comes before anything else**: a slot whose command may never
//!   be directed (`gh issue close {item}`, Publish) is dropped, however common. A thin slot (two
//!   values, `adage_graph::flow`) forms only on outcome lift, at prime. The route's id is
//!   `lookup-work-item`; its slot is `SlotSource::PromptValue` with the learned type, and its
//!   contract the type's regex.

use crate::bridge;
use crate::evidence::{days, median, models, outcome_lift, shell_join, strength_with_lift, LearnConfig, MaxRank};
use crate::import::Exposure as _;
use crate::import::Session;
use adage_core::spans::SpanKind;
use adage_core::{tokens, vtype, Effect};
use adage_graph::embed::Embedder;
use adage_graph::facts::SessionFacts;
use adage_graph::flow;
use adage_intent::model::StaticModel;
use adage_tracks::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

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
    thin: bool,
    cfg: &LearnConfig,
) -> Option<Route> {
    let slot = format!("{}{{item}}{}", sink.prefix, sink.suffix);
    let mut opportunities = 0u32;
    let mut open: BTreeSet<String> = BTreeSet::new();
    let mut support: BTreeSet<String> = BTreeSet::new();
    let (mut positions, mut turns) = (Vec::new(), Vec::new());
    let mut variants: BTreeMap<String, u32> = BTreeMap::new();
    let mut effect: Option<Effect> = None;
    // How each supporting session's prompt wrote the value (`#` in `#482`).
    let mut mentions: BTreeMap<tokens::Mention, u32> = BTreeMap::new();
    for s in sessions {
        // A trace a directive steered to this command is no evidence for it (Track G §3.2).
        if s.exposed_identity(&sink.identity) {
            continue;
        }
        let values = prompt_values(s, &t.runs);
        if values.is_empty() {
            continue;
        }
        opportunities += 1;
        open.insert(s.id.clone());
        'steps: for (i, step) in s.discovery().iter().enumerate() {
            let Some(av) = crate::evidence::argv(step) else { continue };
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
        .max_by_rank(adage_tracks::io::classify(&command, adage_core::CommandTable::load()));
    // A repository script the table does not know: classified by what the traces show it doing.
    let (effect, effect_evidence) = crate::effects::effect_of(effect, &command, sessions);
    ceiling(effect, false)?;
    let n = support.len() as u32;
    let rate = n as f64 / opportunities.max(1) as f64;
    let lb = wilson_lower(n, opportunities);
    // The rate gate, else the outcome-lift path (prime at most).
    let lift = outcome_lift(sessions, &open, &support);
    // A thin slot (two values) forms on outcome lift alone, at prime.
    let strength = if thin {
        crate::evidence::lift_strength(effect, false, cfg, lift.as_ref())?
    } else {
        strength_with_lift(effect, false, n, lb, rate, cfg, "lookup-work-item", lift.as_ref())?
    };
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
        strength,
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
            outcome_lift: lift,
            effect_evidence,
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
            if let Some(av) = crate::evidence::argv(st) {
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
    cands.into_iter().find_map(|(slot, t)| route_for(sessions, &slot.sink, t, slot.thin, cfg)).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{PromptObs, StepObs};
    use adage_core::types::CanonicalAction;

    fn exec(i: u32, cmd: &str, effect: Effect) -> StepObs {
        let action = CanonicalAction::Exec { argv: cmd.split(' ').map(str::to_string).collect(), cwd: "/r".into() };
        StepObs {
            project_pytest_root_cwd: false,
            turn: i,
            key: adage_core::keys::template_key(&action),
            norm: adage_core::Normalized { action, effect, mutating: false, confidence: 1.0, note: String::new() },
            call: adage_trace::Call {
                tool: "Bash".into(),
                input: serde_json::Value::Null,
                output: "ok".into(),
                failed: false,
                by: adage_trace::By::Agent,
                tool_use_id: None,
                agent: None,
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
            influence: vec![],
            outcome: None,
            harness: "claude-code".into(),
            prompts: vec![PromptObs { text: prompt.into(), at_step: 0 }],
            ending: Default::default(),
        }
    }

    fn cfg() -> LearnConfig {
        LearnConfig::demo()
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
        assert!(adage_core::spans::extract("Please fix #482 today").iter().all(|x| x.kind != SpanKind::Ticket));
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
        // The view slot is judged on its own evidence: 5 of 10 sessions, prime at the demo bar.
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].step, Step::Run { command: "gh issue view {item}".into() });
    }

    /// A trace whose prompt names `W-10{i}`, which (if `lookup`) looks it up, with `passed`.
    fn ticket_session(i: usize, lookup: bool, passed: Option<bool>) -> Session {
        let ticket = format!("W-10{i:02}");
        let mut cmds: Vec<(String, Effect)> = vec![("ls".into(), Effect::Observe)];
        if lookup {
            cmds.push((format!("python3 scripts/wi-lookup {ticket}"), Effect::Observe));
        }
        let cmds: Vec<(&str, Effect)> = cmds.iter().map(|(c, e)| (c.as_str(), *e)).collect();
        let mut s = session(i, &format!("Pick up {ticket}"), &cmds);
        s.outcome = passed;
        s
    }

    #[test]
    fn a_rare_step_that_successful_traces_took_forms_a_prime_reflex() {
        // 5 of 12 traces used the lookup (below the 0.5 rate bar); all 5 passed, 1 of 7 others did.
        let ss: Vec<Session> =
            (0..12).map(|i| ticket_session(i, i < 5, Some(if i < 5 { true } else { i == 11 }))).collect();
        let refs: Vec<&Session> = ss.iter().collect();
        let r = lookup_routes(&refs, &LearnConfig::default());
        assert_eq!(r.len(), 1, "the lift path forms the lookup");
        assert_eq!(r[0].strength, Strength::Prime, "prime at most on lift alone");
        let l = r[0].evidence.outcome_lift.as_ref().expect("evidence records the lift");
        assert_eq!((l.with, l.with_passed, l.without, l.without_passed), (5, 5, 7, 1));
        assert!(r[0].evidence.rate < 0.5);
        // Without outcomes (and no transcript proxy: no check ran), the rate bar alone decides.
        let blind: Vec<Session> = (0..12).map(|i| ticket_session(i, i < 5, None)).collect();
        assert!(lookup_routes(&blind.iter().collect::<Vec<_>>(), &LearnConfig::default()).is_empty());
        // No lift: the step's takers did no better.
        let flat: Vec<Session> = (0..12).map(|i| ticket_session(i, i < 5, Some(i % 2 == 0))).collect();
        assert!(lookup_routes(&flat.iter().collect::<Vec<_>>(), &LearnConfig::default()).is_empty());
        // Common enough for the rate bar: fire stays possible.
        let common: Vec<Session> = (0..30).map(|i| ticket_session(i % 12, i < 29, Some(i < 29))).collect();
        let common: Vec<Session> = common
            .into_iter()
            .enumerate()
            .map(|(k, mut s)| {
                s.id = format!("c{k:02}");
                s
            })
            .collect();
        let r = lookup_routes(&common.iter().collect::<Vec<_>>(), &LearnConfig::default());
        assert_eq!(r[0].strength, Strength::Fire);
    }

    #[test]
    fn two_values_form_a_lookup_only_on_outcome_lift_and_with_a_literal_prefix() {
        // Two distinct ids in the four traces that took the lookup: a thin slot. Its type keeps the
        // literal prefix and the observed lengths only (`BIL-` and 3 to 4 digits).
        let mk = |i: usize, id: &str, lookup: bool, passed: bool| {
            let mut cmds: Vec<(String, Effect)> = vec![("ls".into(), Effect::Observe)];
            if lookup {
                cmds.push((format!("./scripts/wi-lookup {id}"), Effect::Observe));
            }
            let cmds: Vec<(&str, Effect)> = cmds.iter().map(|(c, e)| (c.as_str(), *e)).collect();
            let mut s = session(i, &format!("Work on {id}"), &cmds);
            s.outcome = Some(passed);
            s
        };
        let mut ss = vec![
            mk(0, "BIL-495", true, true),
            mk(1, "BIL-495", true, true),
            mk(2, "BIL-4812", true, true),
            mk(3, "BIL-4812", true, true),
        ];
        for i in 4..12 {
            ss.push(mk(i, &format!("BIL-{}", 100 + i), false, i == 11));
        }
        let refs: Vec<&Session> = ss.iter().collect();
        let r = lookup_routes(&refs, &cfg());
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].strength, Strength::Prime);
        let Contract::Pattern { regex } = &r[0].slots[0].contract else { panic!() };
        assert_eq!(regex, "^(?:BIL\\-[0-9]{3,4})$");
        // Without the lift (every trace passed), two values are not enough.
        for s in ss.iter_mut() {
            s.outcome = Some(true);
        }
        assert!(lookup_routes(&ss.iter().collect::<Vec<_>>(), &cfg()).is_empty());
        // Two bare numbers never make a type, lift or not.
        let mut bare = vec![
            mk(0, "495", true, true),
            mk(1, "495", true, true),
            mk(2, "4812", true, true),
            mk(3, "4812", true, true),
        ];
        for i in 4..12 {
            bare.push(mk(i, &format!("{}", 100 + i), false, i == 11));
        }
        assert!(lookup_routes(&bare.iter().collect::<Vec<_>>(), &cfg()).is_empty());
    }

    #[test]
    fn a_trace_steered_to_the_lookup_is_no_evidence_for_it() {
        let mut ss: Vec<Session> = (0..8)
            .map(|i| {
                let n = 400 + 17 * i;
                session(i, &format!("Please fix #{n}"), &[(&format!("./scripts/wi-lookup {n}"), Effect::Observe)])
            })
            .collect();
        for s in ss.iter_mut().take(6) {
            s.influence = vec![crate::import::Directed {
                at_step: 0,
                route: "lookup-work-item".into(),
                commands: vec![format!("./scripts/wi-lookup {}", 1)],
                ..Default::default()
            }];
        }
        let r = lookup_routes(&ss.iter().collect::<Vec<_>>(), &cfg());
        assert!(r.iter().all(|r| r.evidence.support <= 2), "{r:?}");
    }
}
