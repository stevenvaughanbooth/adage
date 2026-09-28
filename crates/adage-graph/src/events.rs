//! The miner's own event-sequence input (Track G §1.1, G2).
//!
//! A session is an ordered list of typed events. Each event carries its node type, a content key
//! at the exact level (a canonical command key, a repository path, normalised prompt text), a
//! head key and a blocking key for prototypes, an optional embedding, its batch (a model turn: the
//! calls of one turn form an itemset), ok/fail, and its prompt segment. Two kinds of edges join
//! events: **value flow** (a token of an earlier prompt, output or path reappears in a command's
//! argv) and **same artifact** (a path listed in an earlier command's output is read or edited).
//!
//! [`from_input`] converts the G1 input ([`crate::input::Session`]) so mining runs today; the
//! integration moves the source to the event-graph substrate (`SessionFacts`) later.
//!
//! [`Corpus`] is the labelled form the miner reads: every event gets a label id at each taxonomy
//! level ([`Level`]), assigned by [`crate::proto`]. Label ids are ranks of the sorted label
//! strings (content-derived, never discovery order).

use crate::input::{self, Step};
use adage_core::pattern::Shape;
use adage_core::types::Effect;
use adage_core::{keys, tokens, CommandTable};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A node type (Track G §1.1). `Read` and `Edit` are the two accesses of an `Art` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Prompt,
    Act,
    Read,
    Edit,
    Obs,
    Fail,
}

impl NodeType {
    pub fn name(self) -> &'static str {
        match self {
            NodeType::Prompt => "prompt",
            NodeType::Act => "act",
            NodeType::Read => "read",
            NodeType::Edit => "edit",
            NodeType::Obs => "obs",
            NodeType::Fail => "fail",
        }
    }

    pub fn parse(s: &str) -> Option<NodeType> {
        Some(match s {
            "prompt" => NodeType::Prompt,
            "act" => NodeType::Act,
            "read" => NodeType::Read,
            "edit" => NodeType::Edit,
            "obs" => NodeType::Obs,
            "fail" => NodeType::Fail,
            _ => return None,
        })
    }

    /// The prototype family: reads and edits of a path share artifact prototypes.
    pub fn family(self) -> &'static str {
        match self {
            NodeType::Read | NodeType::Edit => "art",
            t => t.name(),
        }
    }
}

/// Taxonomy levels, fine to coarse: exact ⊂ prototype ⊂ coarse prototype ⊂ head ⊂ kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Exact = 0,
    Proto = 1,
    Coarse = 2,
    /// HeadSub identity (Act), `top/**/*.ext` (Art).
    Head = 3,
    /// The node type alone (`read`, `edit`, `prompt`); never used for an Act.
    Kind = 4,
}

pub const LEVELS: usize = 5;
pub const ALL_LEVELS: [Level; LEVELS] = [Level::Exact, Level::Proto, Level::Coarse, Level::Head, Level::Kind];

impl Level {
    pub fn from_index(i: usize) -> Level {
        ALL_LEVELS[i.min(LEVELS - 1)]
    }
    pub fn tag(self) -> char {
        match self {
            Level::Exact => 'x',
            Level::Proto => 'p',
            Level::Coarse => 'c',
            Level::Head => 'h',
            Level::Kind => 'k',
        }
    }
    /// Whether items of type `ty` may use this level.
    pub fn allowed(self, ty: NodeType) -> bool {
        match ty {
            NodeType::Act => self != Level::Kind,
            NodeType::Prompt => self != Level::Head,
            _ => true,
        }
    }
}

/// A node embedding: unit f32, or quantised with `adage_intent::quantize`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Embedding {
    F32(Vec<f32>),
    I16(Vec<i16>),
}

impl Embedding {
    pub fn to_unit(&self) -> Vec<f32> {
        match self {
            Embedding::F32(v) => adage_intent::unit(v.clone()),
            Embedding::I16(q) => adage_intent::dequantize(q),
        }
    }
}

/// One event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub ty: NodeType,
    /// The exact-level content: `exec <template>` (Act), the path (Read/Edit), normalised text
    /// (Prompt), the signature (Fail), the shape hash (Obs).
    pub key: String,
    /// The head-level content: HeadSub identity (Act), `top/**/*.<ext family>` (Read/Edit).
    #[serde(default)]
    pub head: String,
    /// Prototypes form only inside one block: effect class and program (Act), top directory and
    /// extension family (Read/Edit).
    #[serde(default)]
    pub block: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Embedding>,
    /// The model turn (an itemset: calls in one batch are unordered).
    pub batch: u32,
    /// For an Act: it succeeded.
    pub ok: bool,
    /// The prompt segment (0 before the first prompt's segment starts).
    pub segment: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
    /// For a failed Act: its failure signature (explanations only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// A directive the session received (Track G §3.2, steering without kinds): from event `from`
/// on, a continuation item whose exact key is `key` is exposed, not evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Influence {
    pub from: u32,
    pub key: String,
}

/// One session as events and edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSession {
    pub id: String,
    /// Days since 1970-01-01 (UTC) of the session's start; 0 when unknown.
    pub day: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<bool>,
    pub events: Vec<Event>,
    /// Value-flow edges (source event → sink Act), sorted.
    pub flows: Vec<(u32, u32)>,
    /// Same-artifact edges (Act whose output listed the path → Read/Edit of it), sorted.
    pub same_art: Vec<(u32, u32)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub influence: Vec<Influence>,
}

/// What [`from_input`] cannot read from an [`input::Session`].
#[derive(Debug, Clone, Default)]
pub struct Meta {
    /// `YYYY-MM-DD…` of the session start.
    pub day: Option<String>,
    pub task: Option<String>,
    pub outcome: Option<bool>,
    /// Model turn of each input step (same length as `steps`); each step is its own batch
    /// without it.
    pub turns: Option<Vec<u32>>,
    /// Effect of each input step, when the importer knows it (else classified here).
    pub effects: Option<Vec<Option<Effect>>>,
    pub influence: Vec<Influence>,
}

/// Days since 1970-01-01 of a `YYYY-MM-DD…` string (0 when it does not parse).
pub fn day_number(s: &str) -> u32 {
    let p: Vec<i64> = s.get(..10).unwrap_or("").split('-').filter_map(|x| x.parse().ok()).collect();
    if p.len() != 3 || !(1..=12).contains(&p[1]) || !(1..=31).contains(&p[2]) {
        return 0;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let (y, m, d) = (if p[1] <= 2 { p[0] - 1 } else { p[0] }, p[1], p[2]);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468).max(0) as u32
}

/// Extension family of a path (blocking and the head level of an artifact).
pub fn ext_family(path: &str) -> &'static str {
    let base = path.rsplit('/').next().unwrap_or(path);
    let ext = match base.rsplit_once('.') {
        Some((stem, e)) if !stem.is_empty() => e.to_ascii_lowercase(),
        _ => return "noext",
    };
    match ext.as_str() {
        "py" | "pyi" => "py",
        "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "mts" | "cts" => "js",
        "rs" => "rs",
        "go" => "go",
        "json" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" => "config",
        "csv" | "tsv" => "table",
        "md" | "rst" | "txt" | "adoc" => "doc",
        "sh" | "bash" | "zsh" => "sh",
        "html" | "css" | "scss" => "web",
        "c" | "h" | "cc" | "cpp" | "hpp" => "c",
        "java" | "kt" => "jvm",
        "rb" => "rb",
        "lock" => "lock",
        _ => "other",
    }
}

fn top_dir(path: &str) -> &str {
    match path.split_once('/') {
        Some((t, rest)) if !t.is_empty() && !rest.is_empty() => t,
        _ => ".",
    }
}

/// The head key of an artifact path: `top/**/*.<family>`.
pub fn art_head(path: &str) -> String {
    format!("{}/**/*.{}", top_dir(path), ext_family(path))
}

/// The canonical key of a command (`exec <template>`), as adage keys it.
pub fn act_key(argv: &[String]) -> String {
    format!("exec {}", keys::template_command(argv).join(" "))
}

/// The HeadSub identity of a command.
pub fn act_identity(argv: &[String]) -> String {
    Shape::of_argv(argv).map(|s| s.identity.join(" ")).unwrap_or_else(|| argv.first().cloned().unwrap_or_default())
}

fn program(argv: &[String]) -> String {
    Shape::of_argv(argv).map(|s| s.program).unwrap_or_else(|| argv.first().cloned().unwrap_or_default())
}

/// The effect class of an argv (Design §8.6, the command table).
pub fn classify(argv: &[String], table: &CommandTable) -> Effect {
    let cmd = argv.join(" ");
    let n = adage_core::shell::normalize_shell(&cmd, "/", table);
    n.iter().map(|a| a.effect).max().unwrap_or(Effect::Environment)
}

/// The failure signature of an output: its last error-like line, else its last line; paths and
/// numbers become placeholders, at most 120 characters.
pub fn signature(output: &str) -> String {
    let out = tokens::bounded(output, input::MAX_OUTPUT);
    let lines: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let errorish = |l: &&&str| {
        let x = l.to_ascii_lowercase();
        ["error", "fail", "exception", "not found", "denied", "cannot", "traceback"].iter().any(|w| x.contains(w))
    };
    let line = lines.iter().rev().find(errorish).or(lines.last()).copied().unwrap_or("");
    line.split_whitespace()
        .map(|w| {
            if w.contains('/') {
                "{path}"
            } else if w.chars().any(|c| c.is_ascii_digit()) && w.chars().all(|c| c.is_ascii_digit() || ".:,-".contains(c))
            {
                "{n}"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

/// Convert a G1 input session. Prompts open prompt segments; each prompt is a batch of its own.
pub fn from_input(s: &input::Session, meta: &Meta, table: &CommandTable) -> EventSession {
    let mut prompts: Vec<&input::Prompt> = s.prompts.iter().collect();
    prompts.sort_by_key(|p| p.at_step);
    let mut next_prompt = 0usize;
    let mut events: Vec<Event> = Vec::new();
    let mut flows: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut same_art: BTreeSet<(u32, u32)> = BTreeSet::new();
    // token → earliest source event (value flow); path token → latest command that printed it.
    let mut sources: BTreeMap<String, u32> = BTreeMap::new();
    let mut printed: BTreeMap<String, u32> = BTreeMap::new();
    let mut batch: u32 = 0;
    let mut have_batch = false;
    let mut last_turn: Option<u32> = None;
    let mut segment: u16 = 0;
    let steps = &s.steps[..s.steps.len().min(input::MAX_STEPS)];
    let new_batch = |have: &mut bool, b: &mut u32| {
        if *have {
            *b += 1;
        }
        *have = true;
    };
    for (i, step) in steps.iter().enumerate() {
        while next_prompt < prompts.len() && prompts[next_prompt].at_step as usize <= i {
            let text = &prompts[next_prompt].text;
            if !events.is_empty() || next_prompt > 0 {
                segment = segment.saturating_add(1);
            }
            new_batch(&mut have_batch, &mut batch);
            last_turn = None;
            let at = events.len() as u32;
            let norm: String = adage_intent::normalize::normalize(text).chars().take(256).collect();
            events.push(Event {
                ty: NodeType::Prompt,
                key: norm,
                head: "prompt".into(),
                block: "prompt".into(),
                embedding: None,
                batch,
                ok: true,
                segment,
                effect: None,
                signature: None,
            });
            for t in tokens::tokens(text, input::MAX_SOURCE_TOKENS) {
                sources.entry(t.text).or_insert(at);
            }
            next_prompt += 1;
        }
        let turn = meta.turns.as_ref().and_then(|t| t.get(i)).copied();
        match (turn, last_turn) {
            (Some(t), Some(l)) if t == l => {}
            _ => new_batch(&mut have_batch, &mut batch),
        }
        last_turn = turn;
        let at = events.len() as u32;
        match step {
            Step::Exec { argv, output, failed } => {
                if argv.is_empty() {
                    continue;
                }
                let effect = meta
                    .effects
                    .as_ref()
                    .and_then(|e| e.get(i).copied().flatten())
                    .unwrap_or_else(|| classify(argv, table));
                for (_, _, value) in crate::flow::sinks(argv) {
                    let src = sources.get(&value).copied().or_else(|| {
                        tokens::subs(&value).into_iter().find_map(|(_, sub)| sources.get(sub).copied())
                    });
                    if let Some(src) = src {
                        flows.insert((src, at));
                    }
                }
                events.push(Event {
                    ty: NodeType::Act,
                    key: act_key(argv),
                    head: act_identity(argv),
                    block: format!("{} {}", effect.as_str(), program(argv)),
                    embedding: None,
                    batch,
                    ok: !failed,
                    segment,
                    effect: Some(effect),
                    signature: failed.then(|| signature(output)),
                });
                for t in tokens::tokens(tokens::bounded(output, input::MAX_OUTPUT), input::MAX_SOURCE_TOKENS) {
                    if t.text.contains('/') || t.text.contains('.') {
                        printed.insert(t.text.clone(), at);
                    }
                    sources.entry(t.text).or_insert(at);
                }
            }
            Step::Read { path } | Step::Edit { path } => {
                if path.is_empty() || path == "." || path == "./" {
                    continue;
                }
                let ty = if matches!(step, Step::Read { .. }) { NodeType::Read } else { NodeType::Edit };
                if let Some(&src) = printed.get(path.as_str()) {
                    same_art.insert((src, at));
                }
                events.push(Event {
                    ty,
                    key: path.clone(),
                    head: art_head(path),
                    block: format!("{}:{}", top_dir(path), ext_family(path)),
                    embedding: None,
                    batch,
                    ok: true,
                    segment,
                    effect: None,
                    signature: None,
                });
                sources.entry(path.clone()).or_insert(at);
                for seg in path.split('/').filter(|x| !x.is_empty()).take(32) {
                    sources.entry(seg.to_string()).or_insert(at);
                }
            }
        }
    }
    EventSession {
        id: s.id.clone(),
        day: meta.day.as_deref().map(day_number).unwrap_or(0),
        task: meta.task.clone(),
        outcome: meta.outcome,
        events,
        flows: flows.into_iter().collect(),
        same_art: same_art.into_iter().collect(),
        influence: meta.influence.clone(),
    }
}

/// No label at this level.
pub const NONE: u32 = u32::MAX;

/// The label table: content-ranked label strings (`<type>/<level tag>:<content>`), each with its
/// type, level, parent (the label at the next defined level) and the highest effect among its
/// events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LabelTable {
    pub names: Vec<String>,
    pub ty: Vec<NodeType>,
    pub level: Vec<Level>,
    pub parent: Vec<u32>,
    pub effect: Vec<Option<Effect>>,
}

impl LabelTable {
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
    pub fn id(&self, name: &str) -> Option<u32> {
        self.names.binary_search_by(|n| n.as_str().cmp(name)).ok().map(|i| i as u32)
    }
    pub fn name(&self, id: u32) -> &str {
        self.names.get(id as usize).map(String::as_str).unwrap_or("?")
    }
    /// The content part of a label (after `<type>/<tag>:`).
    pub fn content(&self, id: u32) -> &str {
        let n = self.name(id);
        n.split_once(':').map(|(_, c)| c).unwrap_or(n)
    }
    /// Whether `anc` is `id` or one of its ancestors.
    pub fn is_ancestor_or_self(&self, anc: u32, mut id: u32) -> bool {
        loop {
            if id == anc {
                return true;
            }
            match self.parent.get(id as usize) {
                Some(&p) if p != NONE => id = p,
                _ => return false,
            }
        }
    }
}

/// The name of a label: `<type>/<level tag>:<content>`.
pub fn label_name(ty: NodeType, level: Level, content: &str) -> String {
    format!("{}/{}:{}", ty.name(), level.tag(), content)
}

/// A labelled event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LEvent {
    pub ty: NodeType,
    pub lab: [u32; LEVELS],
    pub batch: u32,
    pub ok: bool,
    pub seg: u16,
}

/// A labelled session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LSession {
    pub id: String,
    pub day: u32,
    pub task: Option<String>,
    pub events: Vec<LEvent>,
    pub flows: Vec<(u32, u32)>,
    pub same_art: Vec<(u32, u32)>,
    /// (from event, exact label id) pairs: continuation matches there are exposed.
    pub influence: Vec<(u32, u32)>,
}

/// Labelled sessions (sorted by id) and their label table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Corpus {
    pub labels: LabelTable,
    pub sessions: Vec<LSession>,
}

/// Build a corpus from sessions and, per event, its label strings at each level (`None` where
/// the level is not defined). Label ids are ranks of the sorted strings; parents follow each
/// event's chain of defined levels.
pub fn build_corpus(sessions: &[EventSession], names: &dyn Fn(&Event) -> [Option<String>; LEVELS]) -> Corpus {
    let mut order: Vec<&EventSession> = sessions.iter().collect();
    order.sort_by(|a, b| a.id.cmp(&b.id));
    let per: Vec<Vec<[Option<String>; LEVELS]>> =
        order.iter().map(|s| s.events.iter().map(names).collect()).collect();
    let mut set: BTreeMap<String, (NodeType, Level)> = BTreeMap::new();
    for (s, evs) in order.iter().zip(&per) {
        for (e, ns) in s.events.iter().zip(evs) {
            for (l, n) in ns.iter().enumerate() {
                if let Some(n) = n {
                    set.entry(n.clone()).or_insert((e.ty, Level::from_index(l)));
                }
            }
        }
    }
    let names_v: Vec<String> = set.keys().cloned().collect();
    let mut table = LabelTable {
        ty: set.values().map(|v| v.0).collect(),
        level: set.values().map(|v| v.1).collect(),
        parent: vec![NONE; names_v.len()],
        effect: vec![None; names_v.len()],
        names: names_v,
    };
    let mut out = Vec::with_capacity(order.len());
    for (s, evs) in order.iter().zip(&per) {
        let mut events = Vec::with_capacity(s.events.len());
        for (e, ns) in s.events.iter().zip(evs) {
            let mut lab = [NONE; LEVELS];
            for (l, n) in ns.iter().enumerate() {
                if let Some(n) = n {
                    lab[l] = table.id(n).unwrap_or(NONE);
                }
            }
            let mut prev = NONE;
            for &id in lab.iter() {
                if id == NONE {
                    continue;
                }
                if prev != NONE && table.parent[prev as usize] == NONE && prev != id {
                    table.parent[prev as usize] = id;
                }
                if e.effect > table.effect[id as usize] {
                    table.effect[id as usize] = e.effect;
                }
                prev = id;
            }
            events.push(LEvent { ty: e.ty, lab, batch: e.batch, ok: e.ok, seg: e.segment });
        }
        let influence = s
            .influence
            .iter()
            .filter_map(|inf| table.id(&label_name(NodeType::Act, Level::Exact, &inf.key)).map(|id| (inf.from, id)))
            .collect();
        out.push(LSession {
            id: s.id.clone(),
            day: s.day,
            task: s.task.clone(),
            events,
            flows: s.flows.clone(),
            same_art: s.same_art.clone(),
            influence,
        });
    }
    Corpus { labels: table, sessions: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn converts_a_session_with_flows_and_same_artifact_edges() {
        let s = input::Session {
            id: "s1".into(),
            prompts: vec![input::Prompt { text: "Pick up BIL-1872 please".into(), at_step: 0 }],
            steps: vec![
                Step::Exec { argv: argv("ls"), output: "Makefile\nREADME.md".into(), failed: false },
                Step::Exec {
                    argv: argv("./scripts/wi-lookup BIL-1872"),
                    output: "BIL-1872\nFiles: tally/accounts.py".into(),
                    failed: false,
                },
                Step::Read { path: "tally/accounts.py".into() },
                Step::Edit { path: "tally/accounts.py".into() },
                Step::Exec { argv: argv("python3 -m unittest"), output: "RuntimeError: env".into(), failed: true },
            ],
        };
        let meta = Meta { day: Some("2026-08-29T10:40:00Z".into()), turns: Some(vec![0, 1, 2, 2, 3]), ..Default::default() };
        let e = from_input(&s, &meta, &CommandTable::load());
        assert_eq!(e.events.len(), 6);
        assert_eq!(e.events[0].ty, NodeType::Prompt);
        assert_eq!(e.events[2].key, "exec ./scripts/wi-lookup {id}");
        assert_eq!(e.events[2].head, "scripts/wi-lookup");
        assert_eq!(e.flows, vec![(0, 2)]);
        assert_eq!(e.same_art, vec![(2, 3), (2, 4)]);
        assert_eq!(e.events[3].batch, e.events[4].batch);
        assert!(!e.events[5].ok);
        assert_eq!(e.events[3].head, "tally/**/*.py");
        assert_eq!(e.day, day_number("2026-08-29"));
        assert_eq!(day_number("1970-01-02"), 1);
    }
}
