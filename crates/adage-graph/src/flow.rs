//! Value-flow detection (Track G §4.1): which argv positions of which commands carry *data* that
//! came from earlier in the session (a prompt, an earlier output, an earlier path), and which are
//! part of the command's identity.
//!
//! - **Sources**, in session order: prompt tokens (the shared tokeniser, `adage_core::tokens`,
//!   sub-tokens included); earlier outputs (JSON string and number leaves by field path, else text
//!   tokens); earlier read or edited paths (the whole path and each segment, with its depth).
//! - **Sinks**: every later Exec's positional operands and option values, keyed by
//!   [`SinkKey`]: the command's HeadSub identity (`adage_core::pattern::Shape`), the positional
//!   index or option name, and the literal prefix and suffix around the value inside the token
//!   (`#` in `#482`, when only `482` flowed).
//! - **Flow**: exact string equality between a source seen earlier and the sink token, or else one
//!   of its sub-tokens (the first, left to right).
//! - **Value slot test** ([`FlowParams`]): at least 3 distinct values; at least 60% of the sink's
//!   occurrences flow-bound; no single value holding more than half of them (a task repeated twice in four traces is not a literal). A value the sink took
//!   from no source in two or more traces and that never flowed there is a **literal use** (a
//!   default id a script or a habit repeats): it is left out of the test, and is no negative.
//! - **Thin slots** (G5): exactly 2 distinct flowed values, every flow-bound occurrence from a
//!   prompt, and at least 60% flow-bound. Such a slot is kept, marked [`Slot::thin`]; its type must
//!   start with a literal prefix (`BIL-`), and it may only compile on outcome lift, at prime.
//!
//! Determinism: sessions are observed independently (in parallel when asked) and folded in
//! session-id order; every map is a `BTreeMap`.

use crate::input::{Session, Step, MAX_OUTPUT, MAX_SOURCE_TOKENS, MAX_STEPS};
use adage_core::pattern::Shape;
use adage_core::tokens;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Where in a command a value sits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum At {
    /// The n-th positional operand after the identity.
    Pos(u8),
    /// The value of an option (`--issue=123`, `-C dir`), by the option's name.
    Opt(String),
}

/// A sink: (HeadSub identity, position or option, affix inside the token).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SinkKey {
    pub identity: String,
    pub at: At,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
}

impl SinkKey {
    /// A readable, canonical name: `scripts/wi-lookup #0`, `gh issue view --repo`, `x #0 [#…]`.
    pub fn name(&self) -> String {
        let at = match &self.at {
            At::Pos(i) => format!("#{i}"),
            At::Opt(o) => o.clone(),
        };
        let affix = if self.prefix.is_empty() && self.suffix.is_empty() {
            String::new()
        } else {
            format!(" [{}…{}]", self.prefix, self.suffix)
        };
        format!("{} {at}{affix}", self.identity)
    }
}

/// Where a flowing value came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    Prompt,
    Output {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<String>,
    },
    Path {
        /// Segment index (0 = first directory); `None` for the whole path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<u8>,
    },
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Prompt => "prompt".into(),
            Source::Output { field: None } => "output".into(),
            Source::Output { field: Some(f) } => format!("output:{f}"),
            Source::Path { depth: None } => "path".into(),
            Source::Path { depth: Some(d) } => format!("path:{d}"),
        }
    }

    pub fn is_path(&self) -> bool {
        matches!(self, Source::Path { .. })
    }
}

/// The value slot test's thresholds (Track G §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FlowParams {
    pub min_distinct: u32,
    /// Distinct values of a thin slot (below `min_distinct`); 0 turns thin slots off.
    #[serde(default)]
    pub thin_distinct: u32,
    pub min_flow_rate: f64,
    pub max_top_share: f64,
}

impl Default for FlowParams {
    fn default() -> Self {
        FlowParams { min_distinct: 3, thin_distinct: 2, min_flow_rate: 0.6, max_top_share: 0.5 }
    }
}

/// One sink occurrence in one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occ {
    pub identity: String,
    pub at: At,
    pub token: String,
    /// The flowing part of the token (byte offset and length) and its source.
    pub flow: Option<(usize, usize, Source)>,
}

/// What one session contributes: its sink occurrences and its prompt words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionObs {
    pub id: String,
    pub occs: Vec<Occ>,
    pub prompt_words: BTreeSet<String>,
}

/// The sinks of one argv: (identity, position or option, value).
pub fn sinks(argv: &[String]) -> Vec<(String, At, String)> {
    let Some(shape) = Shape::of_tokens(argv) else { return Vec::new() };
    let identity = shape.identity.join(" ");
    let mut out = Vec::new();
    for (i, p) in shape.positionals.iter().enumerate().take(16) {
        out.push((identity.clone(), At::Pos(i as u8), p.clone()));
    }
    for o in &shape.options {
        let split = o.split_once(' ').or_else(|| if o.starts_with('-') { o.split_once('=') } else { None });
        if let Some((name, value)) = split {
            if !name.is_empty() && !value.is_empty() {
                out.push((identity.clone(), At::Opt(name.to_string()), value.to_string()));
            }
        }
    }
    out
}

fn add(sources: &mut BTreeMap<String, Source>, value: &str, src: &Source) {
    if value.is_empty() || value.len() > tokens::MAX_TOKEN_LEN {
        return;
    }
    // The earliest source of a value wins.
    sources.entry(value.to_string()).or_insert_with(|| src.clone());
}

fn add_text(sources: &mut BTreeMap<String, Source>, text: &str, src: &Source) {
    for t in tokens::tokens(text, MAX_SOURCE_TOKENS) {
        add(sources, &t.text, src);
    }
}

fn add_json(sources: &mut BTreeMap<String, Source>, v: &serde_json::Value, path: &str, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    match v {
        serde_json::Value::String(s) => {
            *budget -= 1;
            let src = Source::Output { field: Some(path.to_string()) };
            add(sources, s, &src);
            add_text(sources, s, &src);
        }
        serde_json::Value::Number(n) => {
            *budget -= 1;
            add(sources, &n.to_string(), &Source::Output { field: Some(path.to_string()) });
        }
        serde_json::Value::Array(xs) => {
            for x in xs {
                add_json(sources, x, &format!("{path}[]"), budget);
            }
        }
        serde_json::Value::Object(m) => {
            for (k, x) in m {
                let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                add_json(sources, x, &p, budget);
            }
        }
        _ => {}
    }
}

fn add_output(sources: &mut BTreeMap<String, Source>, output: &str) {
    let out = tokens::bounded(output, MAX_OUTPUT);
    let trimmed = out.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(out) {
            let mut budget = MAX_SOURCE_TOKENS;
            add_json(sources, &v, "", &mut budget);
            return;
        }
    }
    add_text(sources, out, &Source::Output { field: None });
}

fn add_path(sources: &mut BTreeMap<String, Source>, path: &str) {
    add(sources, path, &Source::Path { depth: None });
    for (d, seg) in path.split('/').filter(|s| !s.is_empty()).enumerate().take(32) {
        add(sources, seg, &Source::Path { depth: Some(d as u8) });
    }
}

/// Observe one session: a pure function of the session.
pub fn observe(s: &Session) -> SessionObs {
    let mut sources: BTreeMap<String, Source> = BTreeMap::new();
    let mut prompt_words = BTreeSet::new();
    let mut prompts: Vec<&crate::input::Prompt> = s.prompts.iter().collect();
    prompts.sort_by_key(|p| p.at_step);
    let mut next_prompt = 0;
    let mut occs = Vec::new();
    for (i, step) in s.steps.iter().enumerate().take(MAX_STEPS) {
        while next_prompt < prompts.len() && prompts[next_prompt].at_step as usize <= i {
            let text = &prompts[next_prompt].text;
            add_text(&mut sources, text, &Source::Prompt);
            prompt_words.extend(tokens::words(text, MAX_SOURCE_TOKENS));
            next_prompt += 1;
        }
        match step {
            Step::Exec { argv, output, .. } => {
                for (identity, at, token) in sinks(argv) {
                    let flow = if let Some(src) = sources.get(&token) {
                        Some((0, token.len(), src.clone()))
                    } else {
                        tokens::subs(&token)
                            .into_iter()
                            .find_map(|(off, sub)| sources.get(sub).map(|src| (off, sub.len(), src.clone())))
                    };
                    occs.push(Occ { identity, at, token, flow });
                }
                add_output(&mut sources, output);
            }
            Step::Read { path } | Step::Edit { path } => add_path(&mut sources, path),
            // A skill's name is no value a command takes.
            Step::Skill { .. } => {}
        }
    }
    SessionObs { id: s.id.clone(), occs, prompt_words }
}

/// Observe every session, `threads` at a time, and return the observations in session-id order
/// (ties by content), so the result does not depend on the input order or the thread count.
pub fn observe_all(sessions: &[Session], threads: usize) -> Vec<SessionObs> {
    let mut order: Vec<&Session> = sessions.iter().collect();
    order.sort_by(|a, b| {
        a.id.cmp(&b.id).then_with(|| {
            serde_json::to_string(a).unwrap_or_default().cmp(&serde_json::to_string(b).unwrap_or_default())
        })
    });
    let threads = threads.max(1).min(order.len().max(1));
    if threads == 1 {
        return order.into_iter().map(observe).collect();
    }
    let chunk = order.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let handles: Vec<_> = order
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().map(|s| observe(s)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("observe thread")).collect()
    })
}

/// A sink with its evidence, and whether it passed the value slot test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub sink: SinkKey,
    pub occurrences: u32,
    pub sessions: u32,
    pub distinct: u32,
    /// Share of occurrences carrying a value seen earlier in a source.
    pub flow_rate: f64,
    /// Share of occurrences held by the most common value.
    pub top_share: f64,
    /// Flow-bound occurrences per source label.
    pub sources: BTreeMap<String, u32>,
    /// Flow-bound values and their occurrence counts: the type's positive samples.
    pub positives: BTreeMap<String, u32>,
    /// Values the type must reject: tokens of the same prompts that did not flow here, and values
    /// seen at this sink from no source.
    pub negatives: BTreeSet<String>,
    pub is_slot: bool,
    /// Passed as a thin slot only (two values from prompts): its route needs outcome lift.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub thin: bool,
    /// Literal uses left out of the test (values from no source, repeated in 2+ traces).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub literals: BTreeSet<String>,
}

impl Slot {
    /// True when most flow-bound occurrences came from a path: such a slot takes a repository
    /// path contract, never a learned regex (Track G §4.2).
    pub fn path_sourced(&self) -> bool {
        let path: u32 = self.sources.iter().filter(|(k, _)| k.starts_with("path")).map(|(_, v)| v).sum();
        let all: u32 = self.sources.values().sum();
        all > 0 && path * 2 > all
    }
}

#[derive(Default)]
struct Acc {
    occurrences: u32,
    sessions: BTreeSet<String>,
    values: BTreeMap<String, u32>,
    flowed: u32,
    sources: BTreeMap<String, u32>,
    positives: BTreeMap<String, u32>,
    unflowed: BTreeSet<String>,
    prompt_sessions: BTreeSet<String>,
    /// Sessions per unflowed value.
    unflowed_in: BTreeMap<String, BTreeSet<String>>,
}

/// Fold observations (in the order given, which [`observe_all`] makes canonical) into sinks, and
/// apply the value slot test. Every sink with at least one flow is returned, in key order.
pub fn fold(obs: &[SessionObs], p: &FlowParams) -> Vec<Slot> {
    // Affixes that carried a flow, per (identity, at): unflowed tokens that share one are counted
    // at that affixed sink.
    let mut affixes: BTreeMap<(String, At), BTreeSet<(String, String)>> = BTreeMap::new();
    for o in obs {
        for occ in &o.occs {
            if let Some((off, len, _)) = &occ.flow {
                let pre = occ.token[..*off].to_string();
                let suf = occ.token[off + len..].to_string();
                affixes.entry((occ.identity.clone(), occ.at.clone())).or_default().insert((pre, suf));
            }
        }
    }
    let mut acc: BTreeMap<SinkKey, Acc> = BTreeMap::new();
    let mut words: BTreeMap<String, &BTreeSet<String>> = BTreeMap::new();
    for o in obs {
        words.insert(o.id.clone(), &o.prompt_words);
        for occ in &o.occs {
            let base = (occ.identity.clone(), occ.at.clone());
            let Some(known) = affixes.get(&base) else { continue };
            let (pre, suf, value, src) = match &occ.flow {
                Some((off, len, src)) => (
                    occ.token[..*off].to_string(),
                    occ.token[off + len..].to_string(),
                    occ.token[*off..off + len].to_string(),
                    Some(src),
                ),
                None => {
                    let hit = known.iter().find(|(pre, suf)| {
                        (!pre.is_empty() || !suf.is_empty())
                            && occ.token.len() > pre.len() + suf.len()
                            && occ.token.starts_with(pre.as_str())
                            && occ.token.ends_with(suf.as_str())
                    });
                    match hit {
                        Some((pre, suf)) => (
                            pre.clone(),
                            suf.clone(),
                            occ.token[pre.len()..occ.token.len() - suf.len()].to_string(),
                            None,
                        ),
                        None => (String::new(), String::new(), occ.token.clone(), None),
                    }
                }
            };
            let key = SinkKey { identity: occ.identity.clone(), at: occ.at.clone(), prefix: pre, suffix: suf };
            let a = acc.entry(key).or_default();
            a.occurrences += 1;
            a.sessions.insert(o.id.clone());
            *a.values.entry(value.clone()).or_default() += 1;
            match src {
                Some(src) => {
                    a.flowed += 1;
                    *a.sources.entry(src.label()).or_default() += 1;
                    *a.positives.entry(value).or_default() += 1;
                    if matches!(src, Source::Prompt) {
                        a.prompt_sessions.insert(o.id.clone());
                    }
                }
                None => {
                    a.unflowed_in.entry(value.clone()).or_default().insert(o.id.clone());
                    a.unflowed.insert(value);
                }
            }
        }
    }
    acc.into_iter()
        .filter(|(_, a)| a.flowed > 0)
        .map(|(sink, a)| {
            // Literal uses: a value from no source, in two or more traces, that never flowed here.
            let literals: BTreeSet<String> = a
                .unflowed_in
                .iter()
                .filter(|(v, ss)| ss.len() >= 2 && !a.positives.contains_key(*v))
                .map(|(v, _)| v.clone())
                .collect();
            let lit_occ: u32 = literals.iter().map(|v| a.values.get(v).copied().unwrap_or(0)).sum();
            let n = a.occurrences.saturating_sub(lit_occ).max(1) as f64;
            let top =
                a.values.iter().filter(|(v, _)| !literals.contains(*v)).map(|(_, c)| *c).max().unwrap_or(0) as f64;
            let flow_rate = a.flowed as f64 / n;
            let top_share = top / n;
            let distinct = a.values.keys().filter(|v| !literals.contains(*v)).count() as u32;
            let mut negatives: BTreeSet<String> = a.unflowed.clone();
            for sid in &a.prompt_sessions {
                if let Some(w) = words.get(sid) {
                    negatives.extend(w.iter().cloned());
                }
            }
            negatives.retain(|v| !a.positives.contains_key(v) && !literals.contains(v));
            let is_slot = distinct >= p.min_distinct && flow_rate >= p.min_flow_rate && top_share <= p.max_top_share;
            let prompt = a.sources.get("prompt").copied().unwrap_or(0);
            let thin = !is_slot
                && p.thin_distinct > 0
                && a.positives.len() as u32 == p.thin_distinct
                && distinct == p.thin_distinct
                && prompt == a.flowed
                && flow_rate >= p.min_flow_rate;
            Slot {
                sink,
                occurrences: a.occurrences,
                sessions: a.sessions.len() as u32,
                distinct,
                flow_rate,
                top_share,
                sources: a.sources,
                positives: a.positives,
                negatives,
                is_slot: is_slot || thin,
                thin,
                literals,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Prompt, Step};

    fn exec(argv: &str) -> Step {
        Step::Exec { argv: argv.split(' ').map(str::to_string).collect(), output: String::new(), failed: false }
    }

    fn session(id: &str, prompt: &str, steps: Vec<Step>) -> Session {
        Session { id: id.into(), prompts: vec![Prompt { text: prompt.into(), at_step: 0 }], steps }
    }

    #[test]
    fn prompt_flow_to_positional() {
        let ss: Vec<Session> = (0..5)
            .map(|i| {
                let id = format!("BIL-10{i}{i}");
                session(
                    &format!("s{i}"),
                    &format!("Please work on {id}."),
                    vec![exec("ls"), exec(&format!("./scripts/wi-lookup {id} --json")), exec("make check")],
                )
            })
            .collect();
        let slots = fold(&observe_all(&ss, 1), &FlowParams::default());
        let s = slots.iter().find(|s| s.is_slot).expect("a slot");
        assert_eq!(s.sink.name(), "scripts/wi-lookup #0");
        assert_eq!(s.distinct, 5);
        assert_eq!(s.flow_rate, 1.0);
        assert!(s.negatives.contains("Please") && !s.negatives.contains("BIL-1000"));
        assert_eq!(slots.iter().filter(|s| s.is_slot).count(), 1);
    }

    #[test]
    fn sub_token_flow_records_the_affix() {
        // The prompt says `ops/41`; the command takes `ops-41`: only the number flows.
        let ss: Vec<Session> = (0..4)
            .map(|i| {
                session(&format!("s{i}"), &format!("fix ops/4{i}"), vec![exec(&format!("tool show --id=T{i}x 4{i}"))])
            })
            .collect();
        let slots = fold(&observe_all(&ss, 1), &FlowParams::default());
        let pos = slots.iter().find(|s| s.sink.at == At::Pos(1)).unwrap();
        assert!(pos.is_slot);
        assert!(slots.iter().all(|s| s.sink.at != At::Opt("--id".into())));
        // And the sub-token path: `#482` in the prompt, `#482` as a sub-token source of `x#482`.
        let ss: Vec<Session> = (0..4)
            .map(|i| {
                session(&format!("s{i}"), &format!("look at #48{i}"), vec![exec(&format!("gh issue view gh#48{i}"))])
            })
            .collect();
        let slots = fold(&observe_all(&ss, 1), &FlowParams::default());
        let s = slots.iter().find(|s| s.is_slot).unwrap();
        assert_eq!(s.sink.prefix, "gh#");
        assert_eq!(s.positives.keys().cloned().collect::<Vec<_>>(), ["480", "481", "482", "483"]);
    }

    #[test]
    fn literal_and_rare_flows_are_not_slots() {
        // `check` flows from the prompt every time but is one value: a literal.
        let ss: Vec<Session> =
            (0..6).map(|i| session(&format!("s{i}"), "run check", vec![exec("make check")])).collect();
        let slots = fold(&observe_all(&ss, 1), &FlowParams::default());
        assert!(slots.iter().all(|s| !s.is_slot));
    }

    #[test]
    fn outputs_and_paths_are_sources() {
        let ss: Vec<Session> = (0..3)
            .map(|i| Session {
                id: format!("s{i}"),
                prompts: vec![],
                steps: vec![
                    Step::Exec { argv: vec!["ls".into()], output: format!("{{\"id\": \"W-{i}{i}\"}}"), failed: false },
                    Step::Edit { path: format!("crates/pkg{i}/src/lib.rs") },
                    exec(&format!("cargo test -p pkg{i}")),
                    exec(&format!("tool W-{i}{i}")),
                ],
            })
            .collect();
        let slots = fold(&observe_all(&ss, 1), &FlowParams::default());
        let p = slots.iter().find(|s| s.sink.identity == "cargo test").unwrap();
        assert!(p.is_slot && p.path_sourced());
        let o = slots.iter().find(|s| s.sink.identity == "tool").unwrap();
        assert_eq!(o.sources.keys().cloned().collect::<Vec<_>>(), ["output:id"]);
    }
}
