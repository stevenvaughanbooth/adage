//! Session facts (Track G §1.1, §1.3 stage S2): one session as a small typed event graph.
//!
//! [`facts`] is a pure function of (session, embedder). It reads an [`Annotated`] session (the
//! bridge's output: steps, prompts, and per-step turn, prompt, effect and key) and returns:
//!
//! - **events**, in time order: `Prompt` nodes (normalised text, token count, prompt index) and
//!   `Act` nodes (one per step: an Exec's argv, or a read or edit; its template key, effect class,
//!   batch, prompt segment and ok/fail);
//! - **artifacts** (`Art`: the distinct repository-relative paths read or written, sorted),
//!   **observations** (`Obs`: an Exec's output shape) and **failures** (`Fail`: a failed Exec's
//!   signature);
//! - **edges**: `follows` (consecutive events, with same-batch and same-segment flags; gaps are
//!   derived from positions when mining), `binds` (value flow from an earlier token occurrence to
//!   an Exec's argv position or option, with the literal affix around it, found exactly as
//!   `flow` finds it), `reads` / `writes` (Act → Art), `yields` (Act → Obs), `fails_with`
//!   (Act → Fail) and `recovers_with` (Fail → the next successful Exec with the same effect class
//!   within [`RECOVERY_WINDOW`] events and a different key);
//! - **token occurrences**: every prompt token, and every earlier output or path token that a
//!   `binds` edge uses;
//! - **embeddings** of the session's distinct nodes ([`crate::embed`]), quantised.
//!
//! Bounds (§1.3): at most [`MAX_EVENTS`] events, [`MAX_TOKEN_OCCS`] token occurrences and
//! [`MAX_OUTPUT`] bytes of output per event; whatever is cut is recorded in `truncated`.
//!
//! Determinism: no hash maps, every list in a content-derived or time order; [`facts_all`]
//! returns facts in session-id order whatever the input order or thread count.

use crate::embed::Embedder;
use crate::flow::{sinks, At, Source};
use crate::input::{Annotated, Step, MAX_OUTPUT, MAX_STEPS};
use adage_core::tokens;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The facts format. Part of every facts cache key: bump it whenever what [`facts`] computes
/// changes.
pub const FACTS_FORMAT: u32 = 1;
/// At most this many events (prompts and acts) per session.
pub const MAX_EVENTS: usize = 2_000;
/// At most this many token occurrences per session.
pub const MAX_TOKEN_OCCS: usize = 512;
/// A failure is recovered by a successful act at most this many events later.
pub const RECOVERY_WINDOW: usize = 5;
/// Bytes of an output's head kept, normalised, in its `Obs` node.
pub const OBS_HEAD: usize = 256;

/// What an act did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActKind {
    Exec,
    Read,
    Edit,
}

/// A time-ordered node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "lowercase")]
pub enum Event {
    /// A prompt the person typed.
    Prompt {
        index: u16,
        /// `adage_intent::normalize` of the text: ids, paths and numbers are placeholders.
        text: String,
        /// Its token occurrences (the first `tokens` entries after `first_token`).
        first_token: u32,
        tokens: u32,
    },
    /// A tool step.
    Act {
        kind: ActKind,
        /// The step's index in the session.
        step: u32,
        /// Model turn: acts of one turn form one batch.
        turn: u32,
        /// The prompt segment it belongs to (index of the latest prompt before it).
        segment: u16,
        /// Canonical template key, or a fallback (`exec <argv>`, `read`, `edit`) without one.
        key: String,
        /// An Exec's literal argv (empty for a read or an edit).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        effect: String,
        ok: bool,
    },
}

/// An Exec's output shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obs {
    /// The event that produced it.
    pub act: u32,
    /// SHA-256 (16 hex digits) of the shape: JSON key set, lists-paths flag, line classes.
    pub shape: String,
    /// Top-level JSON keys, sorted (empty for text).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
    pub lists_paths: bool,
    /// Line classes: empty, JSON-like, path-like, error-like, other.
    pub lines: [u32; 5],
    /// The first [`OBS_HEAD`] bytes, digits folded to `0`.
    pub head: String,
}

/// A failed Exec's signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fail {
    pub act: u32,
    pub signature: String,
}

/// An edge. Event, artifact, observation, failure and token indices refer to the lists of the
/// same [`SessionFacts`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "edge", rename_all = "snake_case")]
pub enum Edge {
    Follows { from: u32, to: u32, same_batch: bool, same_segment: bool },
    Binds { token: u32, act: u32, at: At, prefix: String, suffix: String },
    Reads { act: u32, art: u32 },
    Writes { act: u32, art: u32 },
    Yields { act: u32, obs: u32 },
    FailsWith { act: u32, fail: u32 },
    RecoversWith { fail: u32, act: u32 },
}

impl Edge {
    pub fn kind(&self) -> &'static str {
        match self {
            Edge::Follows { .. } => "follows",
            Edge::Binds { .. } => "binds",
            Edge::Reads { .. } => "reads",
            Edge::Writes { .. } => "writes",
            Edge::Yields { .. } => "yields",
            Edge::FailsWith { .. } => "fails_with",
            Edge::RecoversWith { .. } => "recovers_with",
        }
    }
}

/// One occurrence of a token in a source: a prompt, an output (by JSON field), or a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenOcc {
    pub text: String,
    /// The event it occurred in (the prompt, or the act whose output or path held it).
    pub event: u32,
    pub source: Source,
}

/// A distinct node's embedding: `prompt:<index>`, `act:<key>`, `art:<path>`, `fail:<signature>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeVec {
    pub node: String,
    /// Stored as hex of the little-endian `i16`s: exact, and a fraction of the size and parse
    /// time of a JSON number list (most of a facts file is embeddings).
    #[serde(with = "hex_i16")]
    pub vec: Vec<i16>,
}

mod hex_i16 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[i16], s: S) -> Result<S::Ok, S::Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(v.len() * 4);
        for x in v {
            for b in x.to_le_bytes() {
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 15) as usize] as char);
            }
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<i16>, D::Error> {
        let s = String::deserialize(d)?;
        let nib = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        let b = s.as_bytes();
        if b.len() % 4 != 0 {
            return Err(serde::de::Error::custom("embedding hex length"));
        }
        b.chunks(4)
            .map(|c| {
                let byte = |i: usize| Some(nib(c[i])? << 4 | nib(c[i + 1])?);
                match (byte(0), byte(2)) {
                    (Some(lo), Some(hi)) => Ok(i16::from_le_bytes([lo, hi])),
                    _ => Err(serde::de::Error::custom("embedding hex digit")),
                }
            })
            .collect()
    }
}

/// What a bound cut.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Truncated {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub events: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tokens: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub output: bool,
}

/// One session's facts (stage S2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFacts {
    pub format: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub day: String,
    /// The embedding model, when one was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub events: Vec<Event>,
    pub arts: Vec<String>,
    pub obs: Vec<Obs>,
    pub fails: Vec<Fail>,
    pub edges: Vec<Edge>,
    pub tokens: Vec<TokenOcc>,
    pub embeddings: Vec<NodeVec>,
    #[serde(default)]
    pub truncated: Truncated,
}

/// Counts, for `adage graph --learned --dump`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub sessions: usize,
    pub events: usize,
    pub prompts: usize,
    pub acts: usize,
    pub failed_acts: usize,
    pub arts: usize,
    pub obs: usize,
    pub fails: usize,
    pub edges: BTreeMap<String, usize>,
    pub tokens: usize,
    pub embeddings: usize,
    pub distinct_act_keys: usize,
    pub distinct_arts: usize,
    pub truncated_sessions: usize,
    pub max_events: usize,
}

impl Stats {
    pub fn of<'a>(all: impl IntoIterator<Item = &'a SessionFacts>) -> Stats {
        let mut s = Stats::default();
        let mut keys = BTreeSet::new();
        let mut arts = BTreeSet::new();
        for f in all {
            s.sessions += 1;
            s.events += f.events.len();
            s.max_events = s.max_events.max(f.events.len());
            for e in &f.events {
                match e {
                    Event::Prompt { .. } => s.prompts += 1,
                    Event::Act { key, ok, .. } => {
                        s.acts += 1;
                        s.failed_acts += usize::from(!ok);
                        keys.insert(key.clone());
                    }
                }
            }
            s.arts += f.arts.len();
            arts.extend(f.arts.iter().cloned());
            s.obs += f.obs.len();
            s.fails += f.fails.len();
            for e in &f.edges {
                *s.edges.entry(e.kind().to_string()).or_default() += 1;
            }
            s.tokens += f.tokens.len();
            s.embeddings += f.embeddings.len();
            s.truncated_sessions += usize::from(f.truncated != Truncated::default());
        }
        s.distinct_act_keys = keys.len();
        s.distinct_arts = arts.len();
        s
    }
}

fn hex16(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A failure's signature: the last line that reads as an error (else the last non-empty line),
/// with absolute paths dropped, at most 120 characters.
pub fn signature(output: &str) -> String {
    let lines: Vec<&str> = output.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let errorish = |l: &str| {
        let lc = l.to_ascii_lowercase();
        lc.contains("error") || lc.contains("not found") || lc.contains("no module named") || lc.contains("not set")
    };
    let line = lines.iter().rev().find(|l| errorish(l)).or(lines.last()).copied().unwrap_or("");
    line.split_whitespace().filter(|w| !w.starts_with('/')).collect::<Vec<_>>().join(" ").chars().take(120).collect()
}

fn line_class(l: &str) -> usize {
    let t = l.trim();
    if t.is_empty() {
        0
    } else if t.starts_with('{') || t.starts_with('[') || t.starts_with('"') {
        1
    } else if !t.contains(' ') && t.contains('/') {
        2
    } else if t.to_ascii_lowercase().contains("error") {
        3
    } else {
        4
    }
}

fn obs_of(act: u32, output: &str) -> Obs {
    let out = tokens::bounded(output, MAX_OUTPUT);
    let mut keys: Vec<String> = Vec::new();
    let mut lists_paths = false;
    let trimmed = out.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(out) {
            keys = m.keys().cloned().collect();
            keys.sort();
            lists_paths = m.values().any(|v| {
                v.as_array().is_some_and(|a| {
                    !a.is_empty() && a.iter().all(|x| x.as_str().is_some_and(|s| s.contains('/') || s.contains('.')))
                })
            });
        }
    }
    let mut lines = [0u32; 5];
    for l in out.lines().take(10_000) {
        lines[line_class(l)] += 1;
        lists_paths |= {
            let t = l.trim();
            !t.contains(' ') && t.contains('/') && t.rsplit_once('.').is_some_and(|(_, e)| (1..=5).contains(&e.len()))
        };
    }
    let head: String =
        tokens::bounded(out, OBS_HEAD).chars().map(|c| if c.is_ascii_digit() { '0' } else { c }).collect();
    let classes = lines.map(|n| n.min(1));
    let shape = hex16(format!("{keys:?}|{lists_paths}|{classes:?}").as_bytes());
    Obs { act, shape, keys, lists_paths, lines, head }
}

/// The sources seen so far: value → (event, source kind), the earliest winning.
#[derive(Default)]
struct Sources {
    map: BTreeMap<String, (u32, Source)>,
}

impl Sources {
    fn add(&mut self, value: &str, event: u32, src: &Source) {
        if value.is_empty() || value.len() > tokens::MAX_TOKEN_LEN {
            return;
        }
        self.map.entry(value.to_string()).or_insert_with(|| (event, src.clone()));
    }

    fn text(&mut self, text: &str, event: u32, src: &Source) {
        for t in tokens::tokens(text, crate::input::MAX_SOURCE_TOKENS) {
            self.add(&t.text, event, src);
        }
    }

    fn json(&mut self, v: &serde_json::Value, path: &str, event: u32, budget: &mut usize) {
        if *budget == 0 {
            return;
        }
        match v {
            serde_json::Value::String(s) => {
                *budget -= 1;
                let src = Source::Output { field: Some(path.to_string()) };
                self.add(s, event, &src);
                self.text(s, event, &src);
            }
            serde_json::Value::Number(n) => {
                *budget -= 1;
                self.add(&n.to_string(), event, &Source::Output { field: Some(path.to_string()) });
            }
            serde_json::Value::Array(xs) => {
                for x in xs {
                    self.json(x, &format!("{path}[]"), event, budget);
                }
            }
            serde_json::Value::Object(m) => {
                for (k, x) in m {
                    let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    self.json(x, &p, event, budget);
                }
            }
            _ => {}
        }
    }

    fn output(&mut self, output: &str, event: u32) {
        let out = tokens::bounded(output, MAX_OUTPUT);
        let t = out.trim_start();
        if t.starts_with('{') || t.starts_with('[') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(out) {
                let mut budget = crate::input::MAX_SOURCE_TOKENS;
                self.json(&v, "", event, &mut budget);
                return;
            }
        }
        self.text(out, event, &Source::Output { field: None });
    }

    fn path(&mut self, path: &str, event: u32) {
        self.add(path, event, &Source::Path { depth: None });
        for (d, seg) in path.split('/').filter(|s| !s.is_empty()).enumerate().take(32) {
            self.add(seg, event, &Source::Path { depth: Some(d as u8) });
        }
    }
}

/// Token occurrences, bounded, with each distinct (text, event, source) stored once.
#[derive(Default)]
struct Occs {
    list: Vec<TokenOcc>,
    index: BTreeMap<(String, u32, Source), u32>,
    full: bool,
}

impl Occs {
    fn push(&mut self, text: &str, event: u32, source: &Source) -> Option<u32> {
        let k = (text.to_string(), event, source.clone());
        if let Some(i) = self.index.get(&k) {
            return Some(*i);
        }
        if self.list.len() >= MAX_TOKEN_OCCS {
            self.full = true;
            return None;
        }
        let i = self.list.len() as u32;
        self.list.push(TokenOcc { text: text.to_string(), event, source: source.clone() });
        self.index.insert(k, i);
        Some(i)
    }
}

/// Compute one session's facts (see the module documentation).
pub fn facts(a: &Annotated, embedder: &Embedder) -> SessionFacts {
    let s = &a.session;
    let mut trunc = Truncated::default();
    let mut prompts: Vec<(usize, &crate::input::Prompt)> = s.prompts.iter().enumerate().collect();
    prompts.sort_by_key(|(i, p)| (p.at_step, *i));
    let mut events: Vec<Event> = Vec::new();
    let mut occs = Occs::default();
    let mut sources = Sources::default();
    let mut arts: BTreeSet<String> = BTreeSet::new();
    // (act event, art path, write?)
    let mut access: Vec<(u32, String, bool)> = Vec::new();
    let mut obs: Vec<Obs> = Vec::new();
    let mut fails: Vec<Fail> = Vec::new();
    let mut binds: Vec<Edge> = Vec::new();
    let mut next_prompt = 0;
    let mut segment: u16 = 0;
    let mut seen_prompt = false;
    let push_prompt =
        |events: &mut Vec<Event>, occs: &mut Occs, sources: &mut Sources, idx: usize, text: &str| -> bool {
            if events.len() >= MAX_EVENTS {
                return false;
            }
            let ev = events.len() as u32;
            let first = occs.list.len() as u32;
            for t in tokens::tokens(text, crate::input::MAX_SOURCE_TOKENS) {
                if occs.push(&t.text, ev, &Source::Prompt).is_none() {
                    break;
                }
            }
            let n = occs.list.len() as u32 - first;
            sources.text(text, ev, &Source::Prompt);
            events.push(Event::Prompt {
                index: idx.min(u16::MAX as usize) as u16,
                text: adage_intent::normalize::normalize(text),
                first_token: first,
                tokens: n,
            });
            true
        };
    let steps = s.steps.len();
    if steps > MAX_STEPS {
        trunc.events = true;
    }
    for (i, step) in s.steps.iter().enumerate().take(MAX_STEPS) {
        while next_prompt < prompts.len() && prompts[next_prompt].1.at_step as usize <= i {
            let (idx, p) = prompts[next_prompt];
            if !push_prompt(&mut events, &mut occs, &mut sources, idx, &p.text) {
                trunc.events = true;
            }
            segment = idx.min(u16::MAX as usize) as u16;
            seen_prompt = true;
            next_prompt += 1;
        }
        if events.len() >= MAX_EVENTS {
            trunc.events = true;
            break;
        }
        let meta = a.meta.get(i).cloned().unwrap_or_default();
        let ev = events.len() as u32;
        let seg = if seen_prompt { segment } else { 0 };
        match step {
            Step::Exec { argv, output, failed } => {
                let Some(ok) = meta.execution_status(*failed) else { continue };
                if output.len() > MAX_OUTPUT {
                    trunc.output = true;
                }
                for (_, at, token) in sinks(argv) {
                    let hit = match sources.map.get(&token) {
                        Some(src) => Some((0usize, token.len(), token.clone(), src.clone())),
                        None => tokens::subs(&token).into_iter().find_map(|(off, sub)| {
                            sources.map.get(sub).map(|src| (off, sub.len(), sub.to_string(), src.clone()))
                        }),
                    };
                    if let Some((off, len, value, (from, src))) = hit {
                        match occs.push(&value, from, &src) {
                            Some(tok) => binds.push(Edge::Binds {
                                token: tok,
                                act: ev,
                                at,
                                prefix: token[..off].to_string(),
                                suffix: token[off + len..].to_string(),
                            }),
                            None => trunc.tokens = true,
                        }
                    }
                }
                let key = if meta.key.is_empty() { format!("exec {}", argv.join(" ")) } else { meta.key.clone() };
                events.push(Event::Act {
                    kind: ActKind::Exec,
                    step: i as u32,
                    turn: meta.turn,
                    segment: seg,
                    key,
                    argv: argv.clone(),
                    effect: meta.effect.clone(),
                    ok,
                });
                if !output.trim().is_empty() {
                    obs.push(obs_of(ev, output));
                }
                if !ok {
                    fails.push(Fail { act: ev, signature: signature(tokens::bounded(output, MAX_OUTPUT)) });
                }
                sources.output(output, ev);
            }
            Step::Read { path } | Step::Edit { path } => {
                let kind = if matches!(step, Step::Read { .. }) { ActKind::Read } else { ActKind::Edit };
                let key = if meta.key.is_empty() {
                    if kind == ActKind::Read {
                        "read".to_string()
                    } else {
                        "edit".to_string()
                    }
                } else {
                    meta.key.clone()
                };
                events.push(Event::Act {
                    kind,
                    step: i as u32,
                    turn: meta.turn,
                    segment: seg,
                    key,
                    argv: Vec::new(),
                    effect: meta.effect.clone(),
                    ok: true,
                });
                arts.insert(path.clone());
                access.push((ev, path.clone(), kind == ActKind::Edit));
                sources.path(path, ev);
            }
            // Session facts are commands, files and values; a skill used is none of them.
            Step::Skill { .. } => {}
        }
    }
    // Prompts typed after the last step.
    while next_prompt < prompts.len() {
        let (idx, p) = prompts[next_prompt];
        if !push_prompt(&mut events, &mut occs, &mut sources, idx, &p.text) {
            trunc.events = true;
        }
        next_prompt += 1;
    }
    trunc.tokens |= occs.full;

    let arts: Vec<String> = arts.into_iter().collect();
    let art_id: BTreeMap<&str, u32> = arts.iter().enumerate().map(|(i, p)| (p.as_str(), i as u32)).collect();
    let mut edges: Vec<Edge> = Vec::new();
    // follows: consecutive events.
    let turn_seg = |e: &Event| match e {
        Event::Act { turn, segment, .. } => (Some(*turn), *segment),
        Event::Prompt { index, .. } => (None, *index),
    };
    for w in 0..events.len().saturating_sub(1) {
        let (ta, sa) = turn_seg(&events[w]);
        let (tb, sb) = turn_seg(&events[w + 1]);
        let same_batch = ta.is_some() && ta == tb;
        // A prompt opens its own segment, so a prompt and the act after it share one.
        let same_segment = sa == sb;
        edges.push(Edge::Follows { from: w as u32, to: w as u32 + 1, same_batch, same_segment });
    }
    edges.extend(binds);
    for (act, path, write) in &access {
        let art = art_id[path.as_str()];
        edges.push(if *write { Edge::Writes { act: *act, art } } else { Edge::Reads { act: *act, art } });
    }
    for (i, o) in obs.iter().enumerate() {
        edges.push(Edge::Yields { act: o.act, obs: i as u32 });
    }
    for (i, f) in fails.iter().enumerate() {
        edges.push(Edge::FailsWith { act: f.act, fail: i as u32 });
        let Event::Act { key: fkey, effect: feffect, .. } = &events[f.act as usize] else { continue };
        let mut seen = 0;
        for (j, e) in events.iter().enumerate().skip(f.act as usize + 1) {
            let Event::Act { kind, key, effect, ok, .. } = e else { continue };
            seen += 1;
            if seen > RECOVERY_WINDOW {
                break;
            }
            if *kind == ActKind::Exec && *ok && effect == feffect && key != fkey {
                edges.push(Edge::RecoversWith { fail: i as u32, act: j as u32 });
                break;
            }
        }
    }

    // Embeddings of distinct nodes, by label.
    let mut vecs: BTreeMap<String, Vec<i16>> = BTreeMap::new();
    for e in &events {
        match e {
            Event::Prompt { index, .. } => {
                let text = s.prompts.get(*index as usize).map(|p| p.text.as_str()).unwrap_or("");
                if let Some(v) = embedder.prompt(text) {
                    vecs.insert(format!("prompt:{index}"), v);
                }
            }
            Event::Act { key, argv, effect, .. } => {
                vecs.entry(format!("act:{key}")).or_insert_with(|| embedder.act(argv, key, effect));
            }
        }
    }
    for p in &arts {
        vecs.insert(format!("art:{p}"), embedder.art(p, &[]));
    }
    for f in &fails {
        if let std::collections::btree_map::Entry::Vacant(e) = vecs.entry(format!("fail:{}", f.signature)) {
            if let Some(v) = embedder.text(&f.signature) {
                e.insert(v);
            }
        }
    }
    SessionFacts {
        format: FACTS_FORMAT,
        id: s.id.clone(),
        day: a.day.clone(),
        model: embedder.model_id().map(str::to_string),
        events,
        arts,
        obs,
        fails,
        edges,
        tokens: occs.list,
        embeddings: vecs.into_iter().map(|(node, vec)| NodeVec { node, vec }).collect(),
        truncated: trunc,
    }
}

/// The canonical bytes of an annotated session: what its facts are a function of (with the
/// embedder's model and the formats).
pub fn input_bytes(a: &Annotated) -> Vec<u8> {
    serde_json::to_vec(a).unwrap_or_default()
}

/// Order sessions canonically: by id, ties by content.
pub fn canonical_order(sessions: &[Annotated]) -> Vec<&Annotated> {
    let mut order: Vec<(&Annotated, Vec<u8>)> = sessions.iter().map(|a| (a, input_bytes(a))).collect();
    order.sort_by(|x, y| x.0.session.id.cmp(&y.0.session.id).then_with(|| x.1.cmp(&y.1)));
    order.into_iter().map(|(a, _)| a).collect()
}

/// Facts of every session, `threads` at a time, in canonical order: the same bytes whatever the
/// input order or the thread count. `compute` is [`facts`] or a cached version of it.
pub fn facts_all_with<F>(sessions: &[Annotated], threads: usize, compute: F) -> Vec<SessionFacts>
where
    F: Fn(&Annotated) -> SessionFacts + Sync,
{
    let order = canonical_order(sessions);
    let threads = threads.max(1).min(order.len().max(1));
    if threads == 1 {
        return order.into_iter().map(&compute).collect();
    }
    let chunk = order.len().div_ceil(threads);
    let compute = &compute;
    std::thread::scope(|scope| {
        let handles: Vec<_> = order
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().map(|a| compute(a)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("facts thread")).collect()
    })
}

/// [`facts_all_with`] computing [`facts`] directly.
pub fn facts_all(sessions: &[Annotated], embedder: &Embedder, threads: usize) -> Vec<SessionFacts> {
    facts_all_with(sessions, threads, |a| facts(a, embedder))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Prompt, Session, StepMeta};

    fn exec(argv: &str, output: &str, failed: bool) -> Step {
        Step::Exec { argv: argv.split(' ').map(str::to_string).collect(), output: output.into(), failed }
    }

    fn meta(turn: u32, prompt: u16, effect: &str, key: &str) -> StepMeta {
        StepMeta {
            project_pytest_root_cwd: false,
            turn,
            prompt,
            effect: effect.into(),
            key: key.into(),
            verification: None,
        }
    }

    fn sample() -> Annotated {
        Annotated {
            session: Session {
                id: "s1".into(),
                prompts: vec![
                    Prompt { text: "Please fix BIL-1041.".into(), at_step: 0 },
                    Prompt { text: "Now run the checks".into(), at_step: 4 },
                ],
                steps: vec![
                    exec("./scripts/wi-lookup BIL-1041", r#"{"id":"BIL-1041","files":["tally/invoices.py"]}"#, false),
                    Step::Read { path: "tally/invoices.py".into() },
                    Step::Edit { path: "tally/invoices.py".into() },
                    exec("python3 -m unittest", "Traceback\nRuntimeError: ACME_ENV is not set", true),
                    exec("make test", "ok", false),
                ],
            },
            meta: vec![
                meta(0, 0, "observe", "exec scripts/wi-lookup {arg}"),
                meta(1, 0, "observe", "read {path}"),
                meta(1, 0, "author", "edit {path}"),
                meta(2, 0, "verify", "exec python3 -m unittest"),
                meta(3, 1, "verify", "exec make test"),
            ],
            day: "2026-09-01".into(),
        }
    }

    #[test]
    fn a_session_becomes_an_event_graph() {
        let f = facts(&sample(), &Embedder::new(None));
        let kinds: Vec<&str> = f
            .events
            .iter()
            .map(|e| match e {
                Event::Prompt { .. } => "P",
                Event::Act { kind: ActKind::Exec, .. } => "X",
                Event::Act { kind: ActKind::Read, .. } => "R",
                Event::Act { kind: ActKind::Edit, .. } => "E",
            })
            .collect();
        assert_eq!(kinds, ["P", "X", "R", "E", "X", "P", "X"]);
        assert_eq!(f.arts, ["tally/invoices.py"]);
        let count = |k: &str| f.edges.iter().filter(|e| e.kind() == k).count();
        assert_eq!(count("follows"), 6);
        assert_eq!((count("reads"), count("writes"), count("fails_with")), (1, 1, 1));
        assert_eq!(count("recovers_with"), 1, "{:?}", f.edges);
        // The lookup's argument flowed from the prompt.
        let b = f.edges.iter().find_map(|e| match e {
            Edge::Binds { token, act, .. } => Some((*token, *act)),
            _ => None,
        });
        let (tok, act) = b.expect("a binds edge");
        assert_eq!((f.tokens[tok as usize].text.as_str(), act), ("BIL-1041", 1));
        assert_eq!(f.tokens[tok as usize].source, Source::Prompt);
        assert_eq!(f.fails[0].signature, "RuntimeError: ACME_ENV is not set");
        // The second prompt starts segment 1; the act after it is in it.
        match &f.events[6] {
            Event::Act { segment, turn, .. } => assert_eq!((*segment, *turn), (1, 3)),
            e => panic!("{e:?}"),
        }
        assert!(f.obs[0].lists_paths && f.obs[0].keys == ["files", "id"]);
        // Embeddings: every distinct act key and artifact, no prompt without a model.
        assert_eq!(f.embeddings.len(), 5 + 1);
        assert!(f.embeddings.iter().all(|n| !n.node.starts_with("prompt:")));
    }

    #[test]
    fn bounds_hold() {
        let mut a = sample();
        let big: String = (0..5_000).map(|i| format!("tok{i} ")).collect();
        a.session.prompts[0].text = big.clone();
        a.session.steps = (0..MAX_STEPS + 10).map(|i| exec(&format!("tool tok{}", i % 3000), &big, false)).collect();
        let f = facts(&a, &Embedder::new(None));
        assert!(f.events.len() <= MAX_EVENTS);
        assert!(f.tokens.len() <= MAX_TOKEN_OCCS);
        assert!(f.truncated.events && f.truncated.tokens);
    }

    #[test]
    fn same_bytes_for_any_order_and_thread_count() {
        let mut all = Vec::new();
        for i in 0..23 {
            let mut a = sample();
            a.session.id = format!("s{:02}", (i * 7) % 23);
            if i % 2 == 0 {
                a.session.steps.pop();
            }
            all.push(a);
        }
        let e = Embedder::new(None);
        let base = serde_json::to_string(&facts_all(&all, &e, 1)).unwrap();
        for threads in [2, 4, 7] {
            let mut rev = all.clone();
            rev.reverse();
            rev.rotate_left(threads);
            assert_eq!(serde_json::to_string(&facts_all(&rev, &e, threads)).unwrap(), base, "threads {threads}");
        }
    }
}
