//! Route miners (Design §11). Each miner counts evidence in **distinct sessions** and returns
//! routes whose aspect is set by the evidence and capped by the step's effect class.

use crate::import::{Session, StepObs};
use adage_core::spans::{self, SpanKind};
use adage_core::{CanonicalAction, Effect};
use adage_runtime::extract_json;
use adage_tracks::params::LearnParams;
use adage_tracks::*;
use std::collections::{BTreeMap, BTreeSet};

/// Learning thresholds, as the miners read them. Build one from [`LearnParams`] with
/// [`LearnConfig::from_params`]; [`LearnConfig::default`] and [`LearnConfig::demo`] are the two
/// presets (`adage_tracks::params`).
#[derive(Debug, Clone)]
pub struct LearnConfig {
    /// Minimum distinct supporting sessions for any route.
    pub min_support: u32,
    /// Wilson lower bound required for green.
    pub green_lower_bound: f64,
    /// Minimum support rate for yellow.
    pub yellow_rate: f64,
    /// Lower bar that keeps an *existing* route (hysteresis): routes do not flap when their rate
    /// dips just below the bar that created them. Demotion on harm is separate.
    pub retain_rate: f64,
    /// Ids of routes that already exist; they are judged against `retain_rate`.
    pub existing: BTreeSet<String>,
    pub provenance: String,
    /// A dead end names a runner only if it failed in at least `num/den` of the sessions that
    /// ran it ([`DEAD_END_CONSISTENCY`] by default).
    pub dead_end_consistency: (usize, usize),
    /// At most this many dead ends per working command ([`DEAD_END_SIGNATURES`] by default).
    pub dead_end_signatures: usize,
    /// A finish-line command's rate after the edit must exceed its rate without it by this much.
    pub finish_line_contrast: f64,
    /// Intent routes: the share of sessions above which a command is ubiquitous.
    pub intent_ubiquitous: f64,
    /// Intent routes: the in-family minus out-of-family rate a command needs.
    pub intent_contrast: f64,
    /// Review: the lower-bound movement that counts as a material change.
    pub material_lower_bound: f64,
    /// Stale routes: the exposed share that explains a route's absence ([`crate::stale`]).
    pub stale_exposure: f64,
    /// Bind instincts per repository after mining ([`crate::instincts`]); `None` binds none (the
    /// offline eval's default, so its numbers are those of reflexes alone).
    pub instincts: Option<crate::instincts::Config>,
    /// The learning engine (`LearnParams::engine`): `miners`, `shadow` or `graph`. Anything but
    /// `miners` takes the lookup's slot and contract from learned value types ([`crate::graph`]).
    pub engine: String,
    /// Where session facts are cached (`$ADAGE_HOME/cache/facts`) when the engine computes them;
    /// `None` computes them without a cache.
    pub facts_cache: Option<std::path::PathBuf>,
}

impl Default for LearnConfig {
    fn default() -> Self {
        LearnConfig::from_params(&LearnParams::default())
    }
}

impl LearnConfig {
    /// The rate bar for a route id: `retain_rate` if it already exists, else `yellow_rate`.
    pub fn bar(&self, id: &str) -> f64 {
        if self.existing.contains(id) {
            self.retain_rate
        } else {
            self.yellow_rate
        }
    }

    /// Whether the graph engine takes part in learning (`engine` is `shadow` or `graph`).
    pub fn graph(&self) -> bool {
        self.engine != "miners"
    }

    /// Thresholds for small demonstrations; the report always states which were used.
    pub fn demo() -> Self {
        LearnConfig::from_params(&LearnParams::demo())
    }

    /// The miners' configuration for a set of learn parameters.
    pub fn from_params(p: &LearnParams) -> Self {
        LearnConfig {
            min_support: p.min_support,
            green_lower_bound: p.green_lower_bound,
            yellow_rate: p.yellow_rate,
            retain_rate: p.retain_rate,
            existing: BTreeSet::new(),
            provenance: "desire-path".into(),
            dead_end_consistency: (p.dead_end_consistency[0] as usize, p.dead_end_consistency[1] as usize),
            dead_end_signatures: p.dead_end_signatures,
            finish_line_contrast: p.finish_line_contrast,
            intent_ubiquitous: p.intent_ubiquitous,
            intent_contrast: p.intent_contrast,
            material_lower_bound: p.material_lower_bound,
            stale_exposure: p.stale_exposure,
            instincts: None,
            engine: p.engine.clone(),
            facts_cache: None,
        }
    }
}

pub(crate) fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

pub(crate) fn aspect_for(
    effect: Effect,
    by_edit: bool,
    lb: f64,
    rate: f64,
    cfg: &LearnConfig,
    id: &str,
) -> Option<Aspect> {
    let max = ceiling(effect, by_edit)?;
    if lb >= cfg.green_lower_bound && max == Aspect::Green {
        Some(Aspect::Green)
    } else if rate >= cfg.bar(id) {
        Some(Aspect::Yellow)
    } else {
        None
    }
}

/// A short, stable id suffix derived from content, so a route keeps its identity across versions.
pub(crate) fn stable(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    format!("{:06x}", h & 0xff_ffff)
}

pub(crate) fn models(sessions: &[&Session]) -> Vec<String> {
    let s: BTreeSet<String> = sessions.iter().filter_map(|x| x.model.clone()).collect();
    s.into_iter().collect()
}

pub(crate) fn days(sessions: &[&Session], ids: &BTreeSet<String>) -> u32 {
    sessions.iter().filter(|s| ids.contains(&s.id)).map(|s| s.day.clone()).collect::<BTreeSet<_>>().len() as u32
}

pub(crate) fn argv(step: &StepObs) -> Option<&Vec<String>> {
    match &step.norm.action {
        CanonicalAction::Exec { argv, .. } => Some(argv),
        _ => None,
    }
}

pub(crate) fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.chars().all(|c| c.is_ascii_alphanumeric() || "-_./={}:,@+".contains(c)) {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', ""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pseudo-field meaning "repository paths mentioned in the command's text output".
pub const PATHS_FIELD: &str = "*paths";

pub(crate) trait MaxRank {
    fn max_by_rank(self, other: Effect) -> Effect;
}

impl MaxRank for Effect {
    fn max_by_rank(self, other: Effect) -> Effect {
        let r = |e: Effect| match e {
            Effect::Observe => 0,
            Effect::Verify => 1,
            Effect::Regenerate => 2,
            Effect::Environment => 3,
            Effect::Author => 4,
            Effect::Publish => 5,
        };
        if r(other) > r(self) {
            other
        } else {
            self
        }
    }
}

/// A command's runner, without its targets: its program and the subcommand it multiplexes to, or
/// the script an interpreter runs (`adage_core::pattern`). `python3 -m pytest tests/a.py -v` →
/// `python3 -m pytest`, `node --test src/` → `node --test` (not `node`, which would also name
/// `node tools/codegen.mjs`), `python3 scripts/check.py` → `python3 scripts/check.py`,
/// `cargo test --workspace` → `cargo test`.
pub(crate) fn runner(argv: &[String]) -> String {
    use adage_core::pattern::{is_slot, Shape};
    match Shape::of_argv(argv) {
        Some(s) if !s.identity.iter().any(|t| is_slot(t)) => {
            if s.identity.first() == Some(&s.program) {
                s.identity.join(" ")
            } else {
                format!("{} {}", s.program, s.identity.join(" "))
            }
        }
        _ => legacy_runner(argv),
    }
}

/// The first words of a command, up to its first path, dotted word or flag.
fn legacy_runner(argv: &[String]) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let t = argv[i].as_str();
        if i > 0 && (t.contains('/') || t.contains('.') || (t.starts_with('-') && t != "-m")) {
            break;
        }
        out.push(t);
        if t == "-m" {
            if let Some(m) = argv.get(i + 1) {
                out.push(m);
            }
            break;
        }
        i += 1;
        if out.len() >= 3 {
            break;
        }
    }
    out.join(" ")
}

/// A failure signature safe to store and share: no absolute paths, no machine details.
fn signature_of(line: &str) -> String {
    let parts: Vec<&str> = line.split(": ").collect();
    let kept: Vec<&str> =
        parts.iter().copied().skip_while(|p| p.trim_start().starts_with('/') || p.contains("/bin/")).collect();
    let s = if kept.is_empty() { line } else { &kept.join(": ") };
    s.split_whitespace().filter(|w| !w.starts_with('/')).collect::<Vec<_>>().join(" ").chars().take(120).collect()
}

pub struct Mined {
    pub routes: Vec<(String, Route)>, // (track suffix, route)
}

/// Prompt → command routes, with the prompt's ticket bound into the command.
pub fn prompt_routes(sessions: &[&Session], cfg: &LearnConfig) -> Vec<Route> {
    #[derive(Default)]
    struct C {
        sessions: BTreeSet<String>,
        positions: Vec<f64>,
        turns: Vec<f64>,
        values: Vec<String>,
        effect: Option<Effect>,
        key: String,
        variants: BTreeMap<String, u32>,
    }
    let mut opportunities = 0u32;
    let mut cands: BTreeMap<String, C> = BTreeMap::new();
    for s in sessions {
        let tickets: Vec<String> =
            spans::extract(&s.prompt).into_iter().filter(|x| x.kind == SpanKind::Ticket).map(|x| x.value).collect();
        if tickets.is_empty() {
            continue;
        }
        opportunities += 1;
        let mut seen = BTreeSet::new();
        for (i, step) in s.discovery().iter().enumerate() {
            let Some(av) = argv(step) else { continue };
            let Some(t) = tickets.iter().find(|t| av.contains(t)) else { continue };
            let templ: Vec<String> = av.iter().map(|a| if a == t { "{item}".to_string() } else { a.clone() }).collect();
            let command = shell_join(&templ);
            // Variants of one command (interpreter prefix, flags) are one behaviour: group by head.
            // Only variants with the same effect class are one behaviour: `gh issue view` and
            // `gh issue close` share a head but must never be merged.
            let head = format!("{}|{}", adage_core::keys::command_head(av), step.norm.effect.as_str());
            if !seen.insert(head.clone()) || step.call.failed {
                continue;
            }
            let c = cands.entry(head).or_default();
            *c.variants.entry(command).or_default() += 1;
            c.sessions.insert(s.id.clone());
            c.positions.push(i as f64);
            c.turns.push(step.turn as f64);
            c.values.push(t.clone());
            c.effect.get_or_insert(step.norm.effect);
            if c.key.is_empty() {
                c.key = step.key.clone();
            }
        }
    }
    let Some((_head, mut best)) =
        cands.into_iter().max_by(|a, b| a.1.sessions.len().cmp(&b.1.sessions.len()).then_with(|| b.0.cmp(&a.0)))
    else {
        return vec![];
    };
    // The representative command: the most common variant.
    let command = best
        .variants
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(k, _)| k.clone())
        .unwrap_or_default();
    let support = best.sessions.len() as u32;
    if support < cfg.min_support {
        return vec![];
    }
    let rate = support as f64 / opportunities.max(1) as f64;
    let lb = wilson_lower(support, opportunities);
    // Re-derive the effect from the command that will actually be directed; never inherit it.
    let effect = best
        .effect
        .unwrap_or(Effect::Environment)
        .max_by_rank(adage_tracks::io::classify(&command, &adage_core::CommandTable::load()));
    let Some(aspect) = aspect_for(effect, false, lb, rate, cfg, "lookup-work-item") else { return vec![] };
    let Some(contract) = spans::ticket_contract(&best.values) else { return vec![] };
    let mut turns = best.turns.clone();
    vec![Route {
        id: "lookup-work-item".into(),
        summary: "Look up the work item named in the prompt".into(),
        trigger: Trigger::Prompt { span: SpanKind::Ticket },
        step: Step::Run { command },
        slots: vec![Slot {
            name: "item".into(),
            source: SlotSource::Prompt { span: SpanKind::Ticket },
            contract: Contract::Pattern { regex: contract },
        }],
        effect,
        aspect,
        state: initial_state(effect),
        exit: vec![Exit::IntentChanges],
        evidence: Evidence {
            opportunities,
            support,
            days: days(sessions, &best.sessions),
            rate,
            lower_bound: lb,
            median_position: median(&mut best.positions),
            turns_saved: median(&mut turns),
            provenance: cfg.provenance.clone(),
            models: models(sessions),
            ..Default::default()
        },
        authored: false,
    }]
}

/// After a lookup whose JSON output lists files, agents read those files — often one per turn.
pub fn read_batch_routes(sessions: &[&Session], lookup_key: &str, cfg: &LearnConfig) -> Vec<Route> {
    let mut opportunities = 0u32;
    let mut support: BTreeSet<String> = BTreeSet::new();
    let mut fields: BTreeMap<String, u32> = BTreeMap::new();
    let mut saved: Vec<f64> = Vec::new();
    for s in sessions {
        let Some(pos) = s.steps.iter().position(|x| x.key == lookup_key && !x.call.failed) else { continue };
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
    if n < cfg.min_support {
        return vec![];
    }
    let rate = n as f64 / opportunities.max(1) as f64;
    if rate < cfg.bar("read-linked-files") {
        return vec![];
    }
    let field = fields.into_iter().max_by_key(|(_, c)| *c).map(|(f, _)| f).unwrap_or_default();
    vec![Route {
        id: "read-linked-files".into(),
        summary: "Read the files the work item links, together".into(),
        trigger: Trigger::AfterCommand { command: lookup_key.to_string() },
        step: Step::ReadBatch { slot: "files".into(), max: 5 },
        slots: vec![Slot {
            name: "files".into(),
            source: SlotSource::ResultField { field },
            contract: Contract::RepoPath,
        }],
        effect: Effect::Observe,
        aspect: Aspect::Yellow,
        state: RouteState::Active,
        // Design W2: every directive states when the agent may leave it.
        exit: vec![Exit::IntentChanges],
        evidence: Evidence {
            opportunities,
            support: n,
            days: days(sessions, &support),
            rate,
            lower_bound: wilson_lower(n, opportunities),
            turns_saved: median(&mut saved),
            provenance: cfg.provenance.clone(),
            models: models(sessions),
            ..Default::default()
        },
        authored: false,
    }]
}

/// A runner is named in a dead end only if it failed in at least this share of the sessions that
/// ran it (numerator, denominator; integer arithmetic keeps the rule exact). The default of
/// `LearnParams::dead_end_consistency`.
pub const DEAD_END_CONSISTENCY: (usize, usize) = (9, 10);

/// At most this many dead ends (failure signatures) per working command. The default of
/// `LearnParams::dead_end_signatures`.
pub const DEAD_END_SIGNATURES: usize = 3;

/// The failure signature of a failed run: its last error line that can be rendered safely. Tool
/// output is untrusted, and one unrenderable line must not cost the route (or the learn).
fn failure_signature(step: &StepObs) -> Option<String> {
    step.call.output.lines().rev().map(|l| signature_of(l.trim())).find(|l| {
        let lc = l.to_ascii_lowercase();
        (lc.contains("error") || lc.contains("no module named") || lc.contains("not found") || lc.contains("not set"))
            && adage_tracks::io::safe_text(l)
    })
}

/// The key with the most sessions. Ties go to the greatest key, as the miner has always broken
/// them, so the choice is deterministic.
fn most_sessions(m: &BTreeMap<String, BTreeSet<&str>>) -> Option<String> {
    m.iter().max_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| a.0.cmp(b.0))).map(|(k, _)| k.clone())
}

/// A verify command that fails, followed by a different verify command that succeeds.
///
/// Dead ends are keyed by (working command, failure signature), and a runner is named only where
/// the evidence says it is a dead end, not merely that it failed once (docs/evals.md: `python3 -m
/// unittest` fails without `ACME_ENV` and passes with it). For each working command W:
///
/// 1. A *failure pair* is a failed verify run of runner R with signature S (its last renderable
///    error line), followed within five steps by W succeeding.
/// 2. R is *consistent* if it failed in at least `cfg.dead_end_consistency` (9 in 10) of the
///    sessions that ran it at all. A session in which any run of R passed counts against R: that
///    is usually an environment fix, such as `ACME_ENV=test python3 -m unittest`, which normalises
///    to the same argv, so the warning would be wrong for an agent that knows the fix.
/// 3. Each signature S among consistent runners' pairs is a candidate route, most sessions first
///    (ties: the greatest S), at most `cfg.dead_end_signatures` (3).
/// 4. R is avoided under S if it is consistent, is not already named under a more common
///    signature, and S is the signature in at least half of the sessions with a signed failure
///    pair of R. At most three runners are named, most sessions first, then by name.
/// 5. Support is the sessions with a pair of an avoided runner and S; opportunities are the
///    sessions that ran an avoided runner. A signature without enough support is no route.
///
/// So there is one dead end per (working command, signature). The most common signature's route
/// has an id derived from W alone, as when there was one dead end per working command (the demo's
/// `dead-end-d41b1e`), so it keeps its id when its runners or evidence change; each further
/// signature's id is derived from (W, S).
pub fn dead_end_routes(sessions: &[&Session], cfg: &LearnConfig) -> Vec<Route> {
    /// (session, runner, signature, working-command literal)
    type Pair<'a> = (&'a str, String, Option<String>, String);
    let mut pairs: BTreeMap<String, BTreeSet<Pair>> = BTreeMap::new();
    // Per runner: the sessions that ran it (as a verify step), and those in which it passed.
    let mut ran: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    let mut passed: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for s in sessions {
        for (i, a) in s.steps.iter().enumerate() {
            let Some(av) = argv(a) else { continue };
            if a.norm.effect != Effect::Verify {
                continue;
            }
            let r = runner(av);
            ran.entry(r.clone()).or_default().insert(&s.id);
            if !a.call.failed {
                passed.entry(r).or_default().insert(&s.id);
                continue;
            }
            let Some(b) = s
                .steps
                .iter()
                .skip(i + 1)
                .take(5)
                .find(|b| argv(b).is_some() && !b.call.failed && b.norm.effect == Effect::Verify && b.key != a.key)
            else {
                continue;
            };
            pairs.entry(b.key.clone()).or_default().insert((
                &s.id,
                r,
                failure_signature(a),
                shell_join(argv(b).unwrap()),
            ));
        }
    }
    let consistent = |r: &str| {
        let n = ran.get(r).map_or(0, |x| x.len());
        let ok = passed.get(r).map_or(0, |x| x.len());
        let (num, den) = cfg.dead_end_consistency;
        n > 0 && (n - ok) * den >= n * num
    };
    let mut out = Vec::new();
    for (wkey, ps) in pairs.into_iter() {
        // 3. The candidate signatures, from consistent runners only, most sessions first (ties:
        //    the greatest signature, as the miner has always broken them).
        let mut by_sig: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
        for (sid, r, sig, _) in &ps {
            if let Some(sig) = sig.as_ref().filter(|_| consistent(r)) {
                by_sig.entry(sig.clone()).or_default().insert(sid);
            }
        }
        let mut signatures: Vec<(&String, usize)> = by_sig.iter().map(|(s, ids)| (s, ids.len())).collect();
        signatures.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(a.0)));
        signatures.truncate(cfg.dead_end_signatures);
        // Per runner: the sessions with a signed failure pair, and those per signature.
        type Signed<'a> = (BTreeSet<&'a str>, BTreeMap<&'a str, BTreeSet<&'a str>>);
        let mut signed: BTreeMap<&str, Signed> = BTreeMap::new();
        for (sid, r, sig, _) in &ps {
            if let Some(sig) = sig {
                let e = signed.entry(r.as_str()).or_default();
                e.0.insert(sid);
                e.1.entry(sig.as_str()).or_default().insert(sid);
            }
        }
        // A runner is named by one route only: the first (most common) signature that claims it.
        let mut named: BTreeSet<&str> = BTreeSet::new();
        for (rank, (signature, _)) in signatures.iter().enumerate() {
            let signature: &str = signature.as_str();
            // 4. The runners that share it.
            let mut runners: Vec<(&str, usize)> = signed
                .iter()
                .filter(|(r, _)| consistent(r) && !named.contains(*r))
                .filter_map(|(r, (all, per))| {
                    let same = per.get(signature).map_or(0, |x| x.len());
                    (same > 0 && same * 2 >= all.len()).then_some((*r, same))
                })
                .collect();
            runners.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            runners.truncate(3);
            let avoid: BTreeSet<&str> = runners.iter().map(|(r, _)| *r).collect();
            // 5. Evidence.
            let mut support: BTreeSet<String> = BTreeSet::new();
            let mut instead: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
            for (sid, r, sig, lit) in &ps {
                if avoid.contains(r.as_str()) && sig.as_deref() == Some(signature) {
                    support.insert(sid.to_string());
                    instead.entry(lit.clone()).or_default().insert(sid);
                }
            }
            let n = support.len() as u32;
            if n < cfg.min_support {
                continue;
            }
            named.extend(avoid.iter().copied());
            let opp = avoid.iter().filter_map(|r| ran.get(*r)).flatten().collect::<BTreeSet<_>>().len().max(n as usize)
                as u32;
            // The most common signature keeps the id derived from W alone, as when there was one
            // dead end per working command; each further signature gets its own.
            let id = if rank == 0 {
                format!("dead-end-{}", stable(&[&wkey]))
            } else {
                format!("dead-end-{}", stable(&[&wkey, signature]))
            };
            out.push(Route {
                id,
                summary: "A verification command that fails in this repository, and what works instead".into(),
                trigger: Trigger::SessionStart,
                step: Step::Avoid {
                    avoid: runners.iter().map(|(r, _)| *r).collect::<Vec<_>>().join(" / "),
                    instead: most_sessions(&instead).unwrap_or_default(),
                    signature: signature.to_string(),
                },
                slots: vec![],
                effect: Effect::Verify,
                aspect: Aspect::Red,
                state: RouteState::Active,
                // Design W2: every directive states when the agent may leave it.
                exit: vec![Exit::IntentChanges],
                evidence: Evidence {
                    opportunities: opp,
                    support: n,
                    days: days(sessions, &support),
                    rate: n as f64 / opp.max(1) as f64,
                    lower_bound: wilson_lower(n, opp),
                    turns_saved: 1.0,
                    provenance: cfg.provenance.clone(),
                    models: models(sessions),
                    ..Default::default()
                },
                authored: false,
            });
        }
    }
    out
}

/// The path pattern an edit to `rel` stands for: `schema/invoice.json` → `schema/*.json`.
pub fn glob_for(rel: &str) -> Option<String> {
    let (dir, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    let ext = file.rsplit_once('.').map(|(_, e)| e)?;
    Some(if dir.is_empty() { format!("*.{ext}") } else { format!("{dir}/*.{ext}") })
}

/// A generalised finish-line glob (`a/**/*.ext`) and the sibling globs it replaces.
struct GeneralGlob {
    glob: String,
    command: String,
    parts: Vec<String>,
    editors: BTreeSet<String>,
    support: BTreeSet<String>,
}

/// The glob lattice, kept conservative: `a/b/*.ext` and `a/c/*.ext`, each followed by the same
/// regenerate command, become `a/**/*.ext` only when the evidence is unanimous. Every session that
/// edited a `.ext` file anywhere under `a/` ran the command after its last such edit, no session
/// that ran it after editing `.ext` files edited one outside `a/` (the step belongs to `a/`, not
/// to the extension), and the general glob has enough support and more than any sibling alone.
/// The common prefix is never empty: nothing generalises to `**/*.ext`.
fn generalise_globs(
    sessions: &[&Session],
    after: &BTreeMap<(String, String), BTreeSet<String>>,
    runs: &BTreeMap<String, BTreeSet<String>>,
    cfg: &LearnConfig,
) -> Vec<GeneralGlob> {
    // (command, extension) -> directories of the specific globs followed by it
    let mut groups: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for (g, c) in after.keys() {
        let Some((dir, ext)) = g.split_once("/*.") else { continue };
        if !dir.contains('*') && !ext.contains(['*', '/']) {
            groups.entry((c.clone(), ext.to_string())).or_default().insert(dir.to_string());
        }
    }
    let total = sessions.len();
    let mut out = Vec::new();
    for ((c, ext), dirs) in groups {
        if dirs.len() < 2 {
            continue;
        }
        let split: Vec<Vec<&str>> = dirs.iter().map(|d| d.split('/').collect()).collect();
        let mut prefix: Vec<&str> = split[0].clone();
        for d in &split[1..] {
            let n = prefix.iter().zip(d).take_while(|(a, b)| a == b).count();
            prefix.truncate(n);
        }
        if prefix.is_empty() {
            continue;
        }
        let p = prefix.join("/");
        let (under_p, suffix) = (format!("{p}/"), format!(".{ext}"));
        let under = |rel: &str| rel.starts_with(&under_p) && rel.ends_with(&suffix);
        let mut editors = BTreeSet::new();
        let mut support = BTreeSet::new();
        let mut outside = false;
        for s in sessions {
            let edits: Vec<(usize, &str)> = s
                .steps
                .iter()
                .enumerate()
                .filter(|(_, st)| adage_core::keys::is_author(&st.norm.action))
                .filter_map(|(i, st)| st.rel_path.as_deref().map(|r| (i, r)))
                .collect();
            let Some(last) = edits.iter().filter(|(_, r)| under(r)).map(|(i, _)| *i).max() else { continue };
            editors.insert(s.id.clone());
            let ran = s.steps.iter().skip(last + 1).any(|st| {
                st.norm.effect == Effect::Regenerate
                    && !st.call.failed
                    && argv(st).is_some_and(|av| shell_join(av) == c)
            });
            if ran {
                support.insert(s.id.clone());
                outside |= edits.iter().any(|(_, r)| r.ends_with(&suffix) && !under(r));
            }
        }
        let best_part = dirs
            .iter()
            .filter_map(|d| after.get(&(format!("{d}/*.{ext}"), c.clone())))
            .map(|ids| ids.len())
            .max()
            .unwrap_or(0);
        let n = support.len();
        let ran_without = runs.get(&c).map_or(0, |r| r.len()).saturating_sub(n);
        let not_editing = total.saturating_sub(editors.len());
        let rate_without = if not_editing == 0 { 0.0 } else { ran_without as f64 / not_editing as f64 };
        if outside
            || n != editors.len()
            || (n as u32) < cfg.min_support
            || n <= best_part
            || 1.0 - rate_without < cfg.finish_line_contrast
        {
            continue;
        }
        out.push(GeneralGlob {
            glob: format!("{p}/**/*.{ext}"),
            command: c,
            parts: dirs.iter().map(|d| format!("{d}/*.{ext}")).collect(),
            editors,
            support,
        });
    }
    out
}

/// Edits to a path pattern followed, in successful sessions, by a regenerate step — and sessions
/// that skipped it failing. Sibling directories may generalise ([`generalise_globs`]).
pub fn obligation_routes(sessions: &[&Session], cfg: &LearnConfig) -> Vec<Route> {
    // glob -> sessions editing it
    let mut editors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // (glob, command literal, key) -> sessions running it after the last edit
    let mut after: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut cmd_key: BTreeMap<String, String> = BTreeMap::new();
    // command -> sessions running it at all
    let mut runs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut failed_without: BTreeMap<(String, String), u32> = BTreeMap::new();

    for s in sessions {
        let mut last_edit: BTreeMap<String, usize> = BTreeMap::new();
        for (i, st) in s.steps.iter().enumerate() {
            if adage_core::keys::is_author(&st.norm.action) {
                if let Some(g) = st.rel_path.as_deref().and_then(glob_for) {
                    last_edit.insert(g.clone(), i);
                    editors.entry(g).or_default().insert(s.id.clone());
                }
            }
            if st.norm.effect == Effect::Regenerate && !st.call.failed {
                if let Some(av) = argv(st) {
                    let lit = shell_join(av);
                    runs.entry(lit.clone()).or_default().insert(s.id.clone());
                    cmd_key.entry(lit).or_insert_with(|| st.key.clone());
                }
            }
        }
        for (g, &li) in &last_edit {
            let mut ran_after = BTreeSet::new();
            for st in s.steps.iter().skip(li + 1) {
                if st.norm.effect == Effect::Regenerate && !st.call.failed {
                    if let Some(av) = argv(st) {
                        ran_after.insert(shell_join(av));
                    }
                }
            }
            for c in &ran_after {
                after.entry((g.clone(), c.clone())).or_default().insert(s.id.clone());
            }
            for c in cmd_key.keys() {
                if !ran_after.contains(c) && s.outcome == Some(false) {
                    *failed_without.entry((g.clone(), c.clone())).or_default() += 1;
                }
            }
        }
    }

    let total = sessions.len() as u32;
    let mut out = Vec::new();
    // The glob lattice: sibling directories with one regenerate step may generalise.
    let general = generalise_globs(sessions, &after, &runs, cfg);
    let generalised = |g: &str, c: &str| general.iter().any(|x| x.command == c && x.parts.iter().any(|p| p == g));
    let after: Vec<((String, String), BTreeSet<String>)> = after
        .into_iter()
        .filter(|((g, c), _)| !generalised(g, c))
        .chain(general.iter().map(|x| ((x.glob.clone(), x.command.clone()), x.support.clone())))
        .collect();
    for x in &general {
        editors.insert(x.glob.clone(), x.editors.clone());
    }
    for ((g, c), ids) in after {
        let support = ids.len() as u32;
        let opp = editors.get(&g).map(|s| s.len() as u32).unwrap_or(0);
        if support < cfg.min_support || opp == 0 {
            continue;
        }
        let rate_with = support as f64 / opp as f64;
        let ran_total = runs.get(&c).map(|s| s.len() as u32).unwrap_or(0);
        let not_editing = total.saturating_sub(opp);
        let ran_without = ran_total.saturating_sub(support);
        let rate_without = if not_editing == 0 { 0.0 } else { ran_without as f64 / not_editing as f64 };
        let id = format!("finish-line-{}", stable(&[&g, &c]));
        if rate_with < cfg.bar(&id) || rate_with - rate_without < cfg.finish_line_contrast {
            continue;
        }
        let lb = wilson_lower(support, opp);
        let Some(aspect) = aspect_for(Effect::Regenerate, true, lb, rate_with, cfg, &id) else { continue };
        out.push(Route {
            id,
            summary: "A regeneration step this repository requires after editing these files".into(),
            trigger: Trigger::AfterEdit { paths: g.clone() },
            step: Step::Run { command: c.clone() },
            slots: vec![],
            effect: Effect::Regenerate,
            aspect,
            state: RouteState::Active,
            exit: vec![Exit::ExploratoryChange],
            evidence: Evidence {
                opportunities: opp,
                support,
                days: days(sessions, &ids),
                deviations_failed: failed_without.get(&(g.clone(), c.clone())).copied().unwrap_or(0),
                rate: rate_with,
                lower_bound: lb,
                turns_saved: 0.0,
                provenance: cfg.provenance.clone(),
                models: models(sessions),
                ..Default::default()
            },
            authored: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runners_and_signatures_are_generalised() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(runner(&v(&["python3", "-m", "pytest", "tests/a.py", "-v"])), "python3 -m pytest");
        assert_eq!(runner(&v(&["pytest", "tests/", "-v"])), "pytest");
        assert_eq!(runner(&v(&["node", "--test", "src/"])), "node --test");
        assert_eq!(runner(&v(&["node", "tools/codegen.mjs"])), "node tools/codegen.mjs");
        assert_eq!(runner(&v(&["python3", "scripts/check.py", "-q"])), "python3 scripts/check.py");
        assert_eq!(runner(&v(&["cargo", "test", "--workspace"])), "cargo test");
        assert_eq!(runner(&v(&["npm", "run", "check", "--", "--fix"])), "npm run check");
        assert_eq!(
            signature_of("/Applications/Xcode.app/x/bin/python3: No module named pytest"),
            "No module named pytest"
        );
        assert_eq!(signature_of("RuntimeError: ACME_ENV is not set"), "RuntimeError: ACME_ENV is not set");
    }

    fn step(i: u32, action: CanonicalAction, effect: Effect) -> StepObs {
        let rel_path = adage_core::keys::action_path(&action).map(str::to_string);
        StepObs {
            turn: i,
            key: adage_core::keys::template_key(&action),
            norm: adage_core::Normalized { action, effect, mutating: false, confidence: 1.0, note: String::new() },
            call: adage_runtime::Call {
                tool: "Bash".into(),
                input: serde_json::Value::Null,
                output: String::new(),
                failed: false,
            },
            rel_path,
            prompt: 0,
        }
    }

    /// A session editing these files, then (if `regen`) running `make gen`.
    fn editing(id: &str, files: &[&str], regen: bool) -> Session {
        let mut steps: Vec<StepObs> = files
            .iter()
            .enumerate()
            .map(|(i, f)| step(i as u32, CanonicalAction::Edit { path: f.to_string() }, Effect::Author))
            .collect();
        if regen {
            let argv = vec!["make".to_string(), "gen".to_string()];
            steps.push(step(9, CanonicalAction::Exec { argv, cwd: "/r".into() }, Effect::Regenerate));
        }
        Session {
            id: id.into(),
            root: "/r".into(),
            repo: "acme".into(),
            model: None,
            day: "2026-01-01".into(),
            prompt: "p".into(),
            steps,
            steered: false,
            directed: BTreeSet::new(),
            steered_kinds: BTreeSet::new(),
            outcome: None,
            harness: "claude-code".into(),
            prompts: vec![crate::import::PromptObs { text: "p".into(), at_step: 0 }],
        }
    }

    fn finish_globs(sessions: &[Session]) -> Vec<String> {
        let refs: Vec<&Session> = sessions.iter().collect();
        obligation_routes(&refs, &LearnConfig::default())
            .into_iter()
            .map(|r| match r.trigger {
                Trigger::AfterEdit { paths } => paths,
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn sibling_globs_generalise_only_on_unanimous_evidence() {
        let mut ss: Vec<Session> = Vec::new();
        for i in 0..5 {
            ss.push(editing(&format!("a{i}"), &["proto/billing/x.proto"], true));
            ss.push(editing(&format!("b{i}"), &["proto/users/v1/y.proto"], true));
            ss.push(editing(&format!("n{i}"), &["README.md"], false));
        }
        assert_eq!(finish_globs(&ss), ["proto/**/*.proto"]);
        let g = glob::Pattern::new("proto/**/*.proto").unwrap();
        assert!(
            g.matches("proto/billing/x.proto") && g.matches("proto/users/v1/y.proto") && g.matches("proto/z.proto")
        );
        assert!(!g.matches("other/proto/z.proto"));

        // One session under the prefix that skipped the step: the siblings stay as they are.
        let mut skipped = ss.clone();
        skipped.push(editing("c0", &["proto/admin/z.proto"], false));
        assert_eq!(finish_globs(&skipped), ["proto/billing/*.proto", "proto/users/v1/*.proto"]);

        // The step also followed edits outside the prefix: it is not the prefix's step.
        let mut outside = ss.clone();
        outside.push(editing("o0", &["proto/billing/x.proto", "vendor/w.proto"], true));
        assert!(!finish_globs(&outside).contains(&"proto/**/*.proto".to_string()));

        // Siblings with no common directory never become `**/*.proto`.
        let mut apart: Vec<Session> = Vec::new();
        for i in 0..5 {
            apart.push(editing(&format!("a{i}"), &["api/x.proto"], true));
            apart.push(editing(&format!("b{i}"), &["rpc/y.proto"], true));
            apart.push(editing(&format!("n{i}"), &["README.md"], false));
        }
        assert_eq!(finish_globs(&apart), ["api/*.proto", "rpc/*.proto"]);
    }

    #[test]
    fn globs() {
        assert_eq!(glob_for("schema/invoice.json").unwrap(), "schema/*.json");
        assert_eq!(glob_for("setup.py").unwrap(), "*.py");
        assert!(glob_for("Makefile").is_none());
    }
}
