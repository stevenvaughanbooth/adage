//! MDL selection (Track G §5, SQS-style: Tatti & Vreeken 2012), the [`Reflex`] output, and the
//! G2 pipeline ([`learn`], [`learn_split`]).
//!
//! **Selection.** Candidates that pass the gates are added to a code table best compression gain
//! first (SQS's greedy order, evaluated lazily; ties by value desc, support desc, canonical string
//! asc), and each is kept only if it lowers the total description length `L(CT) + L(D | CT)`, where the data D is every training session's
//! alphabet-event sequence (prompt segments bound every window). A reflex codes data the way it
//! is used: its context is the trigger, its continuation what it predicts.
//!
//! - Code-table patterns, in insertion order, code the data: at every end event of a pattern's
//!   context (an anchor) the pattern emits a hit or a miss; a hit codes the earliest continuation
//!   events after the anchor that no earlier pattern coded (gaps allowed). Every other event is a
//!   singleton (its exact label);
//! - `L(D | CT) = Σ_e u_e · −log2(u_e / U)` over singleton usages, plus for each pattern its
//!   hit/miss stream (`h · −log2(h/A) + m · −log2(m/A)` over its A anchors), the offset of each
//!   coded event from the one before it under a usage-based code per continuation item (SQS's
//!   gap codes, as offsets), and `−log2(f(exact) / f(item label))` bits to refine an event coded
//!   by a coarser, unbound item to its exact label;
//! - `L(CT)` = for each used singleton its standard code `−log2(f(e) / N)`, for each pattern the
//!   standard codes of its item labels plus 4 bits per item (level, polarity, relation,
//!   constraints) plus `log2 6` for its length.
//!
//! Overlapping patterns compete: the longer survives only if it compresses further. The result is
//! deterministic and every kept reflex carries its compression gain in bits.
//!
//! One code table per **delivery shape** (Track G §6.1, decided structurally from the pattern:
//! `avoid` when the context ends with a failed act, `read-batch` when the continuation starts with a
//! read, `obligation` when the context ends with `last(edit)`, `sequence` for two or more acts, `run` otherwise):
//! reflexes that would deliver different directives do not compete for the same events (a
//! dead end's recovery command is also every session's final check; both are kept). Avoids get
//! one table per failing runner.

use crate::events::{EventSession, Level, NodeType, NONE};
use crate::mine::{self, Db, Item, MineParams, Pattern, Pol};
use crate::proto::{self, ProtoParams};
use crate::score::{self, Calibration, HeldOut, Rule, ScoreParams, Scored};
use adage_core::pattern::Shape;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Instant;

/// All G2 parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    pub proto: ProtoParams,
    pub mine: MineParams,
    pub score: ScoreParams,
    /// At most this many reflexes (the compiled-route bound, Track G §1.3).
    pub max_reflexes: usize,
    /// Total scoring cap: closed advisory patterns first, then single-action execution prefixes.
    pub max_closed: usize,
    /// Worker threads everywhere. Never changes the result.
    pub threads: usize,
    /// Also mine the candidates ([`Learned::candidates`], plan D1): a second pass after the
    /// reflexes', which it never changes. Off unless the caller keeps candidates (the learner).
    pub candidates: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            proto: ProtoParams::default(),
            mine: MineParams::default(),
            score: ScoreParams::default(),
            max_reflexes: 256,
            max_closed: 20_000,
            threads: 1,
            candidates: false,
        }
    }
}

impl Params {
    /// The `--demo` preset: `min_support` 3.
    pub fn demo() -> Params {
        let mut p = Params::default();
        p.mine.min_support = 3;
        p.score.min_support = 3;
        p
    }

    pub fn with_threads(mut self, threads: usize) -> Params {
        self.threads = threads;
        self
    }

    fn resolved(&self) -> Params {
        let mut p = self.clone();
        p.proto.threads = self.threads;
        p.mine.threads = self.threads;
        p.score.threads = self.threads;
        p
    }
}

/// One pattern item, readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemView {
    #[serde(rename = "type")]
    pub ty: NodeType,
    pub level: Level,
    /// The label's content (a command key, a path, a glob-like head, a prototype's name).
    pub label: String,
    /// For an act: it succeeded (`true`) or failed (`false`); `None` for either.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub last: bool,
    /// Relation to the previous item: same batch, value flow, same artifact.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub same_batch: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flow: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub same_art: bool,
    /// For an act: its HeadSub identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Exact members seen at this item in supporting sessions, with counts (at most 20).
    pub members: Vec<(String, u32)>,
}

/// A selected reflex (Track G §0): context → continuation, with its evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reflex {
    /// The canonical pattern string.
    pub pattern: String,
    /// The delivery shape it would compile to ([`shape`]).
    pub shape: String,
    pub context: Vec<ItemView>,
    pub continuation: Vec<ItemView>,
    /// `flow(i→j)`, `same_art(i→j)`, `last(i)` over item indices (context first).
    pub constraints: Vec<String>,
    pub support: u32,
    /// Sessions where the context occurs (the opportunities, undiscounted).
    #[serde(default)]
    pub opportunities: u32,
    pub days: u32,
    pub conf: f64,
    pub conf_lb: f64,
    /// `conf` and `conf_lb` after calibration against inner held-out outcomes
    /// ([`Calibration`]).
    #[serde(default)]
    pub conf_cal: f64,
    #[serde(default)]
    pub conf_lb_cal: f64,
    pub lift: f64,
    pub contrast: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consistency: Option<f64>,
    pub value: f64,
    #[serde(default)]
    pub execution_candidate: bool,
    /// Retained for execution judgement only; advisory admission did not pass.
    #[serde(default)]
    pub execution_only: bool,
    /// Compression gain, bits.
    pub gain: f64,
    pub exposed: u32,
    /// The directed command (dominant exact variant of the target), when not ambiguous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// At most 3 supporting session ids.
    pub examples: Vec<String>,
    pub why: String,
    #[serde(skip)]
    pub raw: Option<Pattern>,
    /// Every supporting session id, sorted: which traces a candidate was seen in, and which tipped
    /// it into a reflex (plan D1).
    #[serde(skip)]
    pub sessions: Vec<String>,
    /// For a candidate: the median number of turns from the end of its context to its
    /// continuation in the supporting sessions (how near the trigger is to the step).
    #[serde(skip)]
    pub lag: u32,
}

/// Wall-clock times of a run, milliseconds (reported, never part of the result).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timings {
    pub proto_ms: u64,
    pub mine_ms: u64,
    pub score_ms: u64,
    pub select_ms: u64,
    /// The candidate pass ([`candidate_reflexes`]), when it ran.
    #[serde(default)]
    pub candidates_ms: u64,
}

/// The result of a G2 learn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    pub sessions: usize,
    pub labels: usize,
    pub alphabet: usize,
    pub s_min: u32,
    pub targets: usize,
    pub expansions: u64,
    pub truncated: bool,
    pub frequent: usize,
    pub closed: usize,
    pub gated: usize,
    pub singleton_share: f64,
    /// Calibrated τ per family.
    pub tau: BTreeMap<String, f32>,
    pub reflexes: Vec<Reflex>,
    /// The posterior calibration map fitted on this learn's sessions.
    #[serde(default)]
    pub calibration: Calibration,
    /// Candidates that passed every gate but that the MDL cover did not select, best first
    /// (value, support, canonical string), at most [`MAX_UNSELECTED`], with no compression gain.
    /// The compiler reads them after the selected reflexes (Track G G3): a deliverable seen in
    /// few traces can cost more bits than it saves and still be worth delivering.
    #[serde(skip)]
    pub unselected: Vec<Reflex>,
    /// Every scored candidate's counts by canonical pattern string, selected or not: what
    /// `dormant::verdict` reads for a reflex that was not selected again (Track G §6.3).
    #[serde(skip)]
    pub counts: BTreeMap<String, Counts>,
    /// The candidates ([`candidate_reflexes`]), with [`Params::candidates`]: border patterns with a
    /// context of two or more items, never selected, most support first. The compiler reads them
    /// apart from the reflexes (`compile::compile_candidates`).
    #[serde(skip)]
    pub candidates: Vec<Reflex>,
    #[serde(skip)]
    pub timings: Timings,
}

/// At most this many unselected candidates are kept for the compiler.
pub const MAX_UNSELECTED: usize = 2_000;

/// A scored candidate's session counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// Sessions where it occurs only through exposed (steered) continuation events.
    pub exposed: u32,
    /// Unsteered sessions where its context occurs, and where it occurs.
    pub opportunities: u32,
    pub support: u32,
}

fn lg(x: f64) -> f64 {
    if x > 0.0 {
        x.log2()
    } else {
        0.0
    }
}

fn ulogu(u: f64) -> f64 {
    if u > 0.0 {
        u * u.log2()
    } else {
        0.0
    }
}

/// The first continuation chain after anchor `p` over available events (earliest positions).
fn chain_from(db: &Db, s: &mine::DbSession, p: usize, prev: &Item, ys: &[Item], avail: &[bool]) -> Option<Vec<usize>> {
    let Some(y) = ys.first() else { return Some(Vec::new()) };
    let g = db.gap as usize;
    for q in p.saturating_sub(g)..=(p + g).min(s.len().saturating_sub(1)) {
        if q != p && avail[q] && db.node_ok(s, q, y, true) && db.edge_ok(s, p, prev, q, y) {
            if let Some(mut rest) = chain_from(db, s, q, y, &ys[1..], avail) {
                if !rest.contains(&q) {
                    rest.insert(0, q);
                    return Some(rest);
                }
            }
        }
    }
    None
}

/// Code one session with a reflex: its context's end events (anchors), and for each anchor in
/// order the continuation events it codes (a hit), over events not yet coded.
#[allow(clippy::type_complexity)]
fn code_session(
    db: &Db,
    s: &mine::DbSession,
    items: &[Item],
    cut: usize,
    avail: &mut [bool],
) -> (u32, Vec<(usize, Vec<usize>)>) {
    let mut xs: Vec<Item> = items[..cut].to_vec();
    xs[0] = xs[0].first();
    let ends = db.reach(s, &xs, cut, None).pop().unwrap_or_default();
    let mut hits = Vec::new();
    for &p in &ends {
        if let Some(chain) = chain_from(db, s, p as usize, &items[cut - 1], &items[cut..], avail) {
            for &q in &chain {
                avail[q] = false;
            }
            hits.push((p as usize, chain));
        }
    }
    (ends.len() as u32, hits)
}

struct Mdl<'a> {
    db: &'a Db,
    n_events: f64,
    freq_lab: Vec<f64>,
    usage: BTreeMap<u32, f64>,
    u_total: f64,
    s_sum: f64,
    /// Hit/miss streams, gap codes and refinements of the patterns in the table.
    streams: f64,
    model: f64,
    avail: Vec<Vec<bool>>,
}

/// Bits per pattern item beyond its label's standard code: level, polarity, relation and
/// constraints.
const ITEM_BITS: f64 = 4.0;

fn entropy_bits(h: f64, m: f64) -> f64 {
    let n = h + m;
    let part = |x: f64| if x > 0.0 { -x * lg(x / n) } else { 0.0 };
    part(h) + part(m)
}

impl<'a> Mdl<'a> {
    fn new(db: &'a Db) -> Mdl<'a> {
        let mut freq_lab = vec![0.0; db.labels.len()];
        let mut usage: BTreeMap<u32, f64> = BTreeMap::new();
        let mut n = 0.0;
        for s in &db.sessions {
            for e in &s.ev {
                n += 1.0;
                for &l in e.lab.iter() {
                    if l != NONE {
                        freq_lab[l as usize] += 1.0;
                    }
                }
                *usage.entry(e.lab[0]).or_default() += 1.0;
            }
        }
        let u_total = n;
        let s_sum = usage.values().map(|&u| ulogu(u)).sum();
        let model = usage.keys().map(|&e| -lg(freq_lab[e as usize] / n)).sum();
        let avail = db.sessions.iter().map(|s| vec![true; s.len()]).collect();
        Mdl { db, n_events: n, freq_lab, usage, u_total, s_sum, streams: 0.0, model, avail }
    }

    fn total(&self) -> f64 {
        ulogu(self.u_total) - self.s_sum + self.streams + self.model
    }

    fn pattern_bits(&self, items: &[Item]) -> f64 {
        items.iter().map(|i| -lg(self.freq_lab[i.label as usize] / self.n_events) + ITEM_BITS).sum::<f64>() + lg(6.0)
    }

    /// The gain of adding `sc` (bits); commits when positive and `commit`.
    fn try_add(&mut self, sc: &Scored, commit: bool) -> f64 {
        let items = &sc.pattern.items;
        let cut = sc.pattern.cut;
        let mut offsets: BTreeMap<(usize, usize), f64> = BTreeMap::new();
        let mut local: Vec<(u32, Vec<bool>)> = Vec::new();
        let mut dec: BTreeMap<u32, f64> = BTreeMap::new();
        let (mut anchors, mut hits, mut refine) = (0.0, 0.0, 0.0);
        for &si in &sc.opp_list {
            let s = &self.db.sessions[si as usize];
            let mut av = self.avail[si as usize].clone();
            let (a, chains) = code_session(self.db, s, items, cut, &mut av);
            anchors += a as f64;
            for (anchor, chain) in &chains {
                hits += 1.0;
                // The offset of each coded event from the event before it (alphabet events).
                let mut prev = *anchor;
                for (j, &q) in chain.iter().enumerate() {
                    *offsets.entry((j, q.abs_diff(prev))).or_default() += 1.0;
                    prev = q;
                    let exact = s.ev[q].lab[0];
                    *dec.entry(exact).or_default() += 1.0;
                    let it = &items[cut + j];
                    if self.db.level(it.label) != Level::Exact && !it.flow && !it.same_art {
                        refine += -lg(self.freq_lab[exact as usize] / self.freq_lab[it.label as usize]);
                    }
                }
            }
            if !chains.is_empty() {
                local.push((si, av));
            }
        }
        if hits == 0.0 {
            return 0.0;
        }
        let before = self.total();
        let mut s_sum = self.s_sum;
        let mut model = self.model + self.pattern_bits(items);
        let covered: f64 = dec.values().sum();
        for (&e, &c) in &dec {
            let u = self.usage[&e];
            s_sum += ulogu(u - c) - ulogu(u);
            if u - c <= 0.0 {
                model -= -lg(self.freq_lab[e as usize] / self.n_events);
            }
        }
        let u_total = self.u_total - covered;
        let offset_bits: f64 = offsets.values().map(|&c| -c * lg(c / hits)).sum();
        let stream = entropy_bits(hits, anchors - hits) + offset_bits + refine;
        let after = ulogu(u_total) - s_sum + self.streams + stream + model;
        let gain = before - after;
        if gain > 0.0 && commit {
            for (e, c) in dec {
                *self.usage.get_mut(&e).expect("usage") -= c;
            }
            self.s_sum = s_sum;
            self.model = model;
            self.u_total = u_total;
            self.streams += stream;
            for (si, av) in local {
                self.avail[si as usize] = av;
            }
        }
        gain
    }
}

fn content_identity(ty: NodeType, level: Level, content: &str) -> Option<String> {
    if ty != NodeType::Act {
        return None;
    }
    if level == Level::Head {
        return Some(content.to_string());
    }
    Shape::of_key(content).map(|s| s.identity.join(" "))
}

fn views(db: &Db, pattern: &Pattern, sessions: &[u32]) -> Vec<ItemView> {
    let items = &pattern.items;
    let mut members: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new(); items.len()];
    for &si in sessions.iter().take(64) {
        let s = &db.sessions[si as usize];
        let r = db.reach(s, items, pattern.cut, None);
        for (i, ps) in r.iter().enumerate() {
            for &q in ps {
                *members[i].entry(db.labels.content(s.ev[q as usize].lab[0]).to_string()).or_default() += 1;
            }
        }
    }
    items
        .iter()
        .zip(members)
        .map(|(it, m)| {
            let ty = db.ty(it.label);
            let level = db.level(it.label);
            let label = db.labels.content(it.label).to_string();
            let mut mem: Vec<(String, u32)> = m.into_iter().collect();
            mem.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            mem.truncate(20);
            ItemView {
                ty,
                level,
                identity: content_identity(ty, level, &label),
                label,
                ok: match it.pol {
                    Pol::Any => None,
                    Pol::Ok => Some(true),
                    Pol::Fail => Some(false),
                },
                last: it.last,
                same_batch: it.same,
                flow: it.flow,
                same_art: it.same_art,
                members: mem,
            }
        })
        .collect()
}

/// The delivery shape of a pattern (structural, Track G §6.1).
pub fn shape(db: &Db, p: &Pattern) -> &'static str {
    let ctx = p.context();
    let cont = p.continuation();
    if ctx.last().is_some_and(|i| i.pol == Pol::Fail) {
        "avoid"
    } else if cont.first().is_some_and(|i| db.ty(i.label) == NodeType::Skill) {
        "skill"
    } else if cont.first().is_some_and(|i| db.ty(i.label) == NodeType::Read) {
        "read-batch"
    } else if ctx.last().is_some_and(|i| i.last && db.ty(i.label) == NodeType::Edit) {
        "obligation"
    } else if cont.iter().filter(|i| db.ty(i.label) == NodeType::Act).count() >= 2 {
        "sequence"
    } else {
        "run"
    }
}

fn reflex(db: &Db, sc: &Scored, gain: f64) -> Reflex {
    let mut all = views(db, &sc.pattern, &sc.sessions);
    let continuation = all.split_off(sc.pattern.cut);
    let mut constraints = Vec::new();
    for (i, it) in sc.pattern.items.iter().enumerate() {
        if it.flow {
            constraints.push(format!("flow({}→{i})", i - 1));
        }
        if it.same_art {
            constraints.push(format!("same_art({}→{i})", i - 1));
        }
        if it.last {
            constraints.push(format!("last({i})"));
        }
    }
    let examples: Vec<String> = sc.sessions.iter().take(3).map(|&i| db.sessions[i as usize].id.clone()).collect();
    let target = sc.variant.map(|v| db.labels.content(v).to_string());
    let pattern = sc.pattern.render(&db.labels);
    let why = format!(
        "{pattern}: {} sessions over {} days; conf {:.2} (lower bound {:.2}; backoff depth {}, prior {:.2}); lift {:.2}; contrast {:.2}{}; value {:.2}; gain {:.1} bits; target {}; e.g. {}",
        sc.support,
        sc.days,
        sc.conf,
        sc.conf_lb,
        sc.pattern.cut,
        sc.prior,
        sc.lift,
        sc.contrast,
        sc.consistency.map(|c| format!("; consistency {c:.2}")).unwrap_or_default(),
        sc.value,
        gain,
        target.clone().unwrap_or_else(|| format!("ambiguous ({:.0}% dominant)", 100.0 * sc.variant_share)),
        examples.join(", ")
    );
    Reflex {
        shape: shape(db, &sc.pattern).to_string(),
        pattern,
        context: all,
        continuation,
        constraints,
        support: sc.support,
        opportunities: sc.opp_sessions,
        days: sc.days,
        conf: sc.conf,
        conf_lb: sc.conf_lb,
        conf_cal: sc.conf_cal,
        conf_lb_cal: sc.conf_lb_cal,
        lift: sc.lift,
        contrast: sc.contrast,
        consistency: sc.consistency,
        value: sc.value,
        execution_candidate: sc.execution_candidate,
        execution_only: sc.execution_only,
        gain,
        exposed: sc.exposed,
        target,
        examples,
        why,
        raw: Some(sc.pattern.clone()),
        sessions: session_ids(db, &sc.sessions),
        lag: 0,
    }
}

/// The ids of sessions (indices into `db.sessions`), sorted.
fn session_ids(db: &Db, sessions: &[u32]) -> Vec<String> {
    let mut ids: Vec<String> = sessions.iter().map(|&i| db.sessions[i as usize].id.clone()).collect();
    ids.sort();
    ids.dedup();
    ids
}

/// At most this many candidates leave the candidate pass, most support first.
pub const MAX_CANDIDATES: usize = 512;

/// The candidate pass (plan D1): [`mine::mine_candidates`] over the training sessions, each closed
/// border pattern as a [`Reflex`] the compiler can read: its items with their members, its
/// support (distinct unsteered traces), days, the traces where its context occurs, and its
/// posterior as counted (no prior, no calibration, no gate, no compression gain: a candidate is
/// counted, never selected or delivered). Most support first, then the canonical string; at most
/// [`MAX_CANDIDATES`].
pub fn candidate_reflexes(
    labels: &crate::events::LabelTable,
    train: &[&crate::events::LSession],
    p: &MineParams,
) -> Vec<Reflex> {
    let (db, mined) = mine::mine_candidates(labels, train, p);
    let mut out: Vec<Reflex> = mined.closed().map(|m| candidate_reflex(&db, m)).collect();
    out.sort_by(|a, b| b.support.cmp(&a.support).then_with(|| a.pattern.cmp(&b.pattern)));
    out.truncate(MAX_CANDIDATES);
    out
}

fn candidate_reflex(db: &Db, m: &mine::MinedPattern) -> Reflex {
    let pat = &m.pattern;
    let mut all = views(db, pat, &m.sessions);
    let continuation = all.split_off(pat.cut);
    let mut constraints = Vec::new();
    for (i, it) in pat.items.iter().enumerate() {
        if it.flow {
            constraints.push(format!("flow({}→{i})", i - 1));
        }
        if it.same_art {
            constraints.push(format!("same_art({}→{i})", i - 1));
        }
        if it.last {
            constraints.push(format!("last({i})"));
        }
    }
    // The traces where its context occurs: the rate it would be judged on.
    let mut ctx: Vec<Item> = pat.items[..pat.cut].to_vec();
    ctx[0] = ctx[0].first();
    let opportunities =
        (db.sessions.iter().filter(|s| db.occurs(s, &ctx, pat.cut)).count() as u32).max(m.sessions.len() as u32);
    let support = m.sessions.len() as u32;
    let days =
        m.sessions.iter().map(|&i| db.sessions[i as usize].day).collect::<std::collections::BTreeSet<_>>().len() as u32;
    let conf = support as f64 / opportunities.max(1) as f64;
    let conf_lb = adage_tracks::wilson_lower(support, opportunities);
    // How near the context's end is to the continuation, in turns: per supporting session the
    // nearest anchor's, then the median.
    let mut lags: Vec<f64> = m
        .sessions
        .iter()
        .filter_map(|&si| {
            let s = &db.sessions[si as usize];
            let ends = db.reach(s, &ctx, pat.cut, None).pop().unwrap_or_default();
            ends.iter()
                .filter_map(|&p| db.follows(s, p as usize, &pat.items[pat.cut - 1], &pat.items[pat.cut..], true))
                .min()
        })
        .map(f64::from)
        .collect();
    let lag = crate::stats::median(&mut lags).round() as u32;
    let sessions = session_ids(db, &m.sessions);
    let pattern = pat.render(&db.labels);
    let why = format!(
        "{pattern}: a candidate, {support} sessions over {days} days of {opportunities} where its context occurs"
    );
    Reflex {
        shape: shape(db, pat).to_string(),
        pattern,
        context: all,
        continuation,
        constraints,
        support,
        opportunities,
        days,
        conf,
        conf_lb,
        conf_cal: conf,
        conf_lb_cal: conf_lb,
        lift: 0.0,
        contrast: 0.0,
        consistency: None,
        value: 0.0,
        execution_candidate: false,
        execution_only: false,
        gain: 0.0,
        exposed: 0,
        target: None,
        examples: sessions.iter().take(3).cloned().collect(),
        why,
        raw: Some(pat.clone()),
        sessions,
        lag,
    }
}

/// Select among scored candidates: gates, the canonical order, then one MDL code table per
/// delivery shape. Returns (index, gain) by gain desc (ties by canonical string), at most `max`.
pub fn select(db: &Db, scored: &[Scored], max: usize) -> Vec<(usize, f64)> {
    let mut order: Vec<(usize, String)> =
        scored.iter().enumerate().filter(|(_, s)| s.passes).map(|(i, s)| (i, s.pattern.render(&db.labels))).collect();
    order.sort_by(|a, b| {
        let (x, y) = (&scored[a.0], &scored[b.0]);
        y.value.total_cmp(&x.value).then(y.support.cmp(&x.support)).then(a.1.cmp(&b.1))
    });
    let mut tables: BTreeMap<(&str, u32), Vec<(usize, String)>> = BTreeMap::new();
    for (i, text) in order {
        let pat = &scored[i].pattern;
        let sh = shape(db, pat);
        // An avoid names its failing runner: avoids of different runners are different directives.
        let runner = if sh == "avoid" {
            pat.context().iter().rev().find(|x| x.pol == Pol::Fail).map_or(NONE, |x| x.label)
        } else {
            NONE
        };
        tables.entry((sh, runner)).or_default().push((i, text));
    }
    let mut kept: Vec<(usize, f64, String)> = Vec::new();
    for cands in tables.into_values() {
        // Best gain first, lazily: a gain only falls as the table codes more events, so a
        // re-evaluated candidate still ahead of every other bound is the best. Ties go to the
        // (value, support, string) order.
        let mut mdl = Mdl::new(db);
        let mut bound: Vec<(f64, usize, usize, String)> = cands
            .into_iter()
            .enumerate()
            .map(|(pos, (i, t))| (mdl.try_add(&scored[i], false), pos, i, t))
            .filter(|x| x.0 > 0.0)
            .collect();
        while !bound.is_empty() {
            let best = (0..bound.len())
                .max_by(|&a, &b| bound[a].0.total_cmp(&bound[b].0).then(bound[b].1.cmp(&bound[a].1)))
                .expect("non-empty");
            let (_, pos, i, t) = bound.swap_remove(best);
            let g = mdl.try_add(&scored[i], false);
            if g <= 0.0 {
                continue;
            }
            let ahead = bound.iter().all(|x| g > x.0 || (g == x.0 && pos < x.1));
            if ahead {
                mdl.try_add(&scored[i], true);
                kept.push((i, g, t));
            } else {
                bound.push((g, pos, i, t));
            }
        }
    }
    kept.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
    kept.truncate(max);
    kept.into_iter().map(|(i, g, _)| (i, g)).collect()
}

/// Keep the existing advisory closed pool first, then independently executable prefixes.
/// Closure can absorb a useful check into a later unrelated action with identical support.
/// Both use the same mined patterns, score cache, deterministic ordering and total cap.
fn scoring_patterns<'a>(db: &Db, mined: &'a mine::Mined, max: usize) -> Vec<&'a mine::MinedPattern> {
    let mut patterns: Vec<_> =
        mined.patterns.iter().filter(|m| m.closed || score::execution_shape(db, &m.pattern)).collect();
    patterns.sort_by(|a, b| {
        b.closed.cmp(&a.closed).then(b.sessions.len().cmp(&a.sessions.len())).then(a.pattern.cmp(&b.pattern))
    });
    patterns.truncate(max);
    patterns
}

/// Learn reflexes from `train`; evaluate on `test` (held out) when it is not empty.
pub fn learn_split(train: &[EventSession], test: &[EventSession], p: &Params) -> (Learned, Option<HeldOut>) {
    let p = p.resolved();
    let t0 = Instant::now();
    let epoch = proto::build(train, &p.proto);
    let all: Vec<EventSession> = train.iter().chain(test.iter()).cloned().collect();
    let corpus = epoch.label(&all);
    let train_ids: std::collections::BTreeSet<&str> = train.iter().map(|s| s.id.as_str()).collect();
    let (tr, te): (Vec<_>, Vec<_>) = corpus.sessions.iter().partition(|s| train_ids.contains(s.id.as_str()));
    let db = Db::build(&corpus.labels, &tr, &p.mine);
    let t1 = Instant::now();
    let mined = mine::mine(&db, &p.mine);
    let t2 = Instant::now();
    // Score the closed advisory pool and execution prefixes within the shared cap.
    let closed = scoring_patterns(&db, &mined, p.max_closed);
    let mut scored = score::score_all(&db, &closed, &mined.patterns, &p.score);
    // Execution prefixes must not change the calibration population used by existing advice.
    let advisory_scored: Vec<_> =
        closed.iter().zip(&scored).filter(|(m, _)| m.closed).map(|(_, sc)| sc.clone()).collect();
    let calibration = fit_calibration(&corpus.labels, &tr, &db, &advisory_scored, &p);
    for sc in scored.iter_mut() {
        calibration.calibrate(sc, p.score.quantile);
    }
    let t3 = Instant::now();
    let kept = select(&db, &scored, p.max_reflexes);
    let reflexes: Vec<Reflex> = kept.iter().map(|&(i, g)| reflex(&db, &scored[i], g)).collect();
    let chosen: std::collections::BTreeSet<usize> = kept.iter().map(|&(i, _)| i).collect();
    let mut rest: Vec<(usize, String)> = scored
        .iter()
        .enumerate()
        .filter(|(i, s)| s.passes && !chosen.contains(i))
        .map(|(i, s)| (i, s.pattern.render(&db.labels)))
        .collect();
    rest.sort_by(|a, b| {
        let (x, y) = (&scored[a.0], &scored[b.0]);
        y.value.total_cmp(&x.value).then(y.support.cmp(&x.support)).then(a.1.cmp(&b.1))
    });
    rest.truncate(MAX_UNSELECTED);
    let unselected: Vec<Reflex> = rest.iter().map(|&(i, _)| reflex(&db, &scored[i], 0.0)).collect();
    let t4 = Instant::now();
    // The candidates: a pass of its own, after everything above, which it never reads or changes.
    let candidates = if p.candidates { candidate_reflexes(&corpus.labels, &tr, &p.mine) } else { Vec::new() };
    let t5 = Instant::now();
    let ms = |a: Instant, b: Instant| (b - a).as_millis() as u64;
    let learned = Learned {
        sessions: db.sessions.len(),
        labels: db.labels.len(),
        alphabet: db.alphabet.iter().filter(|&&a| a).count(),
        s_min: db.s_min,
        targets: mined.targets,
        expansions: mined.expansions,
        truncated: mined.truncated,
        frequent: mined.patterns.len(),
        closed: closed.iter().filter(|m| m.closed).count(),
        gated: scored.iter().filter(|s| s.passes).count(),
        singleton_share: epoch.singleton_share(),
        tau: epoch.families.iter().map(|(k, f)| (k.clone(), f.tau)).collect(),
        reflexes,
        calibration,
        unselected,
        counts: scored
            .iter()
            .map(|sc| {
                let c = Counts { exposed: sc.exposed, opportunities: sc.opp_sessions, support: sc.support };
                (sc.pattern.render(&db.labels), c)
            })
            .collect(),
        candidates,
        timings: Timings {
            proto_ms: ms(t0, t1),
            mine_ms: ms(t1, t2),
            score_ms: ms(t2, t3),
            select_ms: ms(t3, t4),
            candidates_ms: ms(t4, t5),
        },
    };
    let held_out = (!te.is_empty()).then(|| {
        // Every candidate is a calibration pair source; the selected ones are also reported apart.
        let chosen: std::collections::BTreeSet<usize> = kept.iter().map(|k| k.0).collect();
        let rules: Vec<Rule> = scored
            .iter()
            .enumerate()
            .map(|(i, sc)| Rule {
                pattern: sc.pattern.clone(),
                conf: sc.conf,
                conf_cal: sc.conf_cal,
                conf_lb: sc.conf_lb,
                p_next: sc.p_next,
                variant: sc.variant,
                selected: chosen.contains(&i),
            })
            .collect();
        let test_db: Vec<mine::DbSession> = te.iter().map(|s| db.prepare(s)).collect();
        score::held_out(&db, &rules, &test_db)
    });
    (learned, held_out)
}

/// Inner folds of the calibration fit.
const CAL_FOLDS: usize = 3;

/// At most this many training sessions get the exact calibration fit (re-mining per inner
/// fold); larger corpora use [`Calibration::fit`].
pub const CAL_REMINE_MAX: usize = 400;

/// Fit the posterior calibration on the training sessions (Track G §5): on task-grouped inner
/// folds, mine and score candidates on the other folds and compare each candidate's posterior
/// with whether its continuation follows its context in the fold's sessions.
fn fit_calibration(
    labels: &crate::events::LabelTable,
    train: &[&crate::events::LSession],
    db: &Db,
    scored: &[Scored],
    p: &Params,
) -> Calibration {
    if db.sessions.len() > CAL_REMINE_MAX {
        return Calibration::fit(db, scored, p.score.kappa, CAL_FOLDS);
    }
    let fold = score::inner_folds(&db.sessions, CAL_FOLDS);
    let fold_of: BTreeMap<&str, usize> = db.sessions.iter().zip(&fold).map(|(s, &f)| (s.id.as_str(), f)).collect();
    let mut bins = Calibration::bins();
    for f in 0..CAL_FOLDS {
        let (itr, ite): (Vec<&crate::events::LSession>, Vec<&crate::events::LSession>) =
            train.iter().partition(|s| fold_of.get(s.id.as_str()) != Some(&f));
        if itr.is_empty() || ite.is_empty() {
            continue;
        }
        let idb = Db::build(labels, &itr, &p.mine);
        let mined = mine::mine(&idb, &p.mine);
        let mut closed: Vec<_> = mined.closed().collect();
        closed.sort_by(|a, b| b.sessions.len().cmp(&a.sessions.len()).then(a.pattern.cmp(&b.pattern)));
        closed.truncate(p.max_closed);
        let iscored = score::score_all(&idb, &closed, &mined.patterns, &p.score);
        let test: Vec<mine::DbSession> = ite.iter().map(|s| idb.prepare(s)).collect();
        for sc in &iscored {
            let cut = sc.pattern.cut;
            let mut xs: Vec<Item> = sc.pattern.items[..cut].to_vec();
            xs[0] = xs[0].first();
            for s in &test {
                if idb.occurs(s, &xs, cut) {
                    Calibration::add(&mut bins, sc.conf, idb.occurs(s, &sc.pattern.items, cut));
                }
            }
        }
    }
    Calibration::from_bins(&bins)
}

/// Every scored candidate of a learn, with its canonical string, for inspection (`why` a pattern
/// was or was not selected: `Scored::gate`).
pub fn candidates(sessions: &[EventSession], p: &Params) -> Vec<(String, Scored)> {
    candidates_shaped(sessions, p).into_iter().map(|(t, _, s)| (t, s)).collect()
}

/// [`candidates`], each with the delivery shape it would compile to ([`shape`]).
pub fn candidates_shaped(sessions: &[EventSession], p: &Params) -> Vec<(String, &'static str, Scored)> {
    let p = p.resolved();
    let epoch = proto::build(sessions, &p.proto);
    let corpus = epoch.label(sessions);
    let all: Vec<&crate::events::LSession> = corpus.sessions.iter().collect();
    let db = Db::build(&corpus.labels, &all, &p.mine);
    let mined = mine::mine(&db, &p.mine);
    let closed = scoring_patterns(&db, &mined, p.max_closed);
    score::score_all(&db, &closed, &mined.patterns, &p.score)
        .into_iter()
        .map(|s| (s.pattern.render(&db.labels), shape(&db, &s.pattern), s))
        .collect()
}

/// Learn reflexes from sessions.
pub fn learn(sessions: &[EventSession], p: &Params) -> Learned {
    learn_split(sessions, &[], p).0
}

/// A repository glob (`*` within a segment, `**` across segments).
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn seg(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => seg(&p[1..], s) || (!s.is_empty() && seg(p, &s[1..])),
            (Some(a), Some(b)) if a == b => seg(&p[1..], &s[1..]),
            _ => false,
        }
    }
    fn go(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => go(&p[1..], s) || (!s.is_empty() && go(p, &s[1..])),
            (Some(a), Some(b)) => seg(a.as_bytes(), b.as_bytes()) && go(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    go(&p, &s)
}

/// The HeadSub identity of a command line (placeholders allowed).
pub fn command_identity(command: &str) -> Option<String> {
    Shape::of_command(command).map(|s| s.identity.join(" "))
}

fn runs(identity: &str, runner: &str) -> bool {
    let Some(r) = command_identity(runner) else { return false };
    identity == r || identity.starts_with(&format!("{r} "))
}

/// A convention kind of the evals (`conventions.json`) as a mined pattern (Track G §8), for scoring:
///
/// - `lookup`: `[prompt] =flow=> [act command]`;
/// - `linked_files`: `[act command] =same_art=> [read]`;
/// - `dead_end` / `instinct_violation`: `[act avoided ✗] → … → [act command ✓]` with the working
///   command in the continuation;
/// - `finish_line`: `last[edit]` whose members all match `glob`, then `[act command ✓]` in the
///   continuation;
/// - `decoy`: the command anywhere in the continuation (a false reflex).
pub fn matches_convention(r: &Reflex, kind: &str, command: &str, avoided: &[String], glob: Option<&str>) -> bool {
    let Some(id) = command_identity(command) else { return false };
    let items: Vec<&ItemView> = r.context.iter().chain(r.continuation.iter()).collect();
    let cut = r.context.len();
    let is_cmd = |v: &ItemView| v.ty == NodeType::Act && v.identity.as_deref() == Some(id.as_str());
    match kind {
        "lookup" => items.windows(2).any(|w| w[0].ty == NodeType::Prompt && w[1].flow && is_cmd(w[1])),
        "linked_files" => items.windows(2).any(|w| is_cmd(w[0]) && w[1].ty == NodeType::Read && w[1].same_art),
        "dead_end" | "instinct_violation" => items.iter().enumerate().any(|(i, v)| {
            v.ty == NodeType::Act
                && v.ok == Some(false)
                && v.identity.as_deref().is_some_and(|x| avoided.iter().any(|a| runs(x, a)))
                && items.iter().enumerate().skip((i + 1).max(cut)).any(|(_, w)| is_cmd(w) && w.ok == Some(true))
        }),
        "finish_line" => {
            let Some(g) = glob else { return false };
            items.iter().enumerate().any(|(i, v)| {
                v.ty == NodeType::Edit
                    && v.last
                    && !v.members.is_empty()
                    && v.members.iter().all(|(m, _)| glob_match(g, m))
                    && items.iter().enumerate().skip((i + 1).max(cut)).any(|(_, w)| is_cmd(w))
            })
        }
        "decoy" => r.continuation.iter().any(is_cmd),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("registry/*.csv", "registry/ports.csv"));
        assert!(!glob_match("registry/*.csv", "registry/sub/ports.csv"));
        assert!(glob_match("schema/**/*.json", "schema/a/b.json"));
        assert!(glob_match("schema/**/*.json", "schema/b.json"));
        assert!(!glob_match("schema/*.json", "src/a.json"));
    }

    #[test]
    fn identities() {
        assert_eq!(command_identity("./scripts/wi-lookup {item}").as_deref(), Some("scripts/wi-lookup"));
        assert!(runs("python3 -m unittest", "python3 -m unittest"));
        assert!(runs("node --test", "node --test"));
        assert!(!runs("make check", "make test"));
    }
}
