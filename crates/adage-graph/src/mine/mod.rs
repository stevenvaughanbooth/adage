//! Frequent, closed, target-rooted episode mining over temporal DAGs (Track G §3).
//!
//! A **pattern** is 2 to 6 items in time order. Each item is a label at some taxonomy level (a
//! label id of the corpus: its level is the label's), an optional polarity (an act that
//! succeeded or failed), and its relation to the previous item:
//!
//! - `same`: in the same batch (an itemset; same-batch items are sorted by label id, so the
//!   canonical form is a string), otherwise in a later batch;
//! - `flow`: a value-flow edge from the previous item's event into this one;
//! - `same_art`: this read or edit is of a path the previous item's output listed;
//! - `last` (on a context edit): the event is the last one with that label in its prompt
//!   segment ("after the last edit", which finish lines need).
//!
//! Consecutive items lie within `gap` (G = 8) **alphabet events** (events carrying at least one
//! label frequent enough to be an item; the others are dropped before mining, so they can never
//! change a match) and inside one prompt segment. Constraints join adjacent items only (at most
//! 3 per pattern), which makes every constraint local: matching is an exact dynamic program over
//! (item, position) and growth by projection agrees with it.
//!
//! `items[..cut]` is the context, `items[cut..]` the continuation (the target first). Support is
//! the number of distinct sessions with an occurrence whose continuation events are not exposed
//! (a session that received a directive naming that command, from the directive on).
//!
//! - [`seq`]: target-rooted growth (backward to depth 3, forward ≤ 2, constraint extensions, the
//!   taxonomy climb), with a deterministic node budget.
//! - [`closed`]: BIDE-style closure over the frequent set.
//! - [`border`]: incremental mining with a negative border; equal to a full re-mine.

pub mod border;
pub mod closed;
pub mod seq;

use crate::events::{LEvent, LSession, LabelTable, Level, NodeType, LEVELS, NONE};
use adage_core::types::Effect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Mining parameters (Track G §3, §9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MineParams {
    /// Minimum distinct sessions (`learn.min_support`).
    pub min_support: u32,
    /// `s_min = max(min_support, ⌈share · N⌉)`.
    pub min_support_share: f64,
    /// Maximum gap between consecutive items, in alphabet events.
    pub gap: u32,
    /// Context items (backward growth depth).
    pub max_back: usize,
    /// Continuation items after the target (forward growth).
    pub max_fwd: usize,
    pub max_items: usize,
    pub max_constraints: usize,
    /// Pattern nodes expanded per repository, at most.
    pub budget: u64,
    /// The negative border keeps candidates with support in `[s_min − border, s_min)`.
    pub border: u32,
    /// Children expanded per pattern node, at most (a deterministic beam).
    pub max_children: usize,
    pub threads: usize,
}

impl Default for MineParams {
    fn default() -> Self {
        MineParams {
            min_support: 5,
            min_support_share: 0.01,
            gap: 8,
            max_back: 3,
            max_fwd: 2,
            max_items: 6,
            max_constraints: 3,
            budget: 2_000_000,
            border: 2,
            max_children: 8,
            threads: 1,
        }
    }
}

impl MineParams {
    pub fn s_min(&self, n: usize) -> u32 {
        self.min_support.max((self.min_support_share * n as f64).ceil() as u32).max(1)
    }
}

/// An act's polarity in an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pol {
    Any,
    Ok,
    Fail,
}

/// One pattern item (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Item {
    pub label: u32,
    pub pol: Pol,
    #[serde(default)]
    pub same: bool,
    #[serde(default)]
    pub flow: bool,
    #[serde(default)]
    pub same_art: bool,
    #[serde(default)]
    pub last: bool,
}

impl Item {
    pub fn new(label: u32, pol: Pol) -> Item {
        Item { label, pol, same: false, flow: false, same_art: false, last: false }
    }
    /// Constraints this item carries.
    pub fn constraints(&self) -> usize {
        usize::from(self.flow) + usize::from(self.same_art) + usize::from(self.last)
    }
    /// The item as the first of a pattern: no incoming edge.
    pub fn first(mut self) -> Item {
        self.same = false;
        self.flow = false;
        self.same_art = false;
        self
    }
}

/// A pattern: items in time order, `items[..cut]` the context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Pattern {
    pub items: Vec<Item>,
    pub cut: usize,
}

impl Pattern {
    pub fn constraints(&self) -> usize {
        self.items.iter().map(Item::constraints).sum()
    }
    pub fn context(&self) -> &[Item] {
        &self.items[..self.cut]
    }
    pub fn continuation(&self) -> &[Item] {
        &self.items[self.cut..]
    }

    /// The canonical string: items joined by their relation (`→` a later batch, `&` the same
    /// batch, with `=flow=>` / `=same_art=>` edges), `last(…)` around an item, `✓`/`✗` for
    /// polarity, and `||` at the cut.
    pub fn render(&self, t: &LabelTable) -> String {
        let mut s = String::new();
        for (i, it) in self.items.iter().enumerate() {
            if i > 0 {
                if i == self.cut {
                    s.push_str(" ||");
                }
                let mut rel = String::new();
                if it.flow {
                    rel.push_str("flow");
                }
                if it.same_art {
                    if !rel.is_empty() {
                        rel.push('+');
                    }
                    rel.push_str("same_art");
                }
                match (it.same, rel.is_empty()) {
                    (true, true) => s.push_str(" & "),
                    (true, false) => s.push_str(&format!(" &{rel}& ")),
                    (false, true) => s.push_str(" → "),
                    (false, false) => s.push_str(&format!(" ={rel}=> ")),
                }
            }
            let pol = match it.pol {
                Pol::Any => "",
                Pol::Ok => "✓",
                Pol::Fail => "✗",
            };
            let body = format!("[{}{pol}]", t.name(it.label));
            if it.last {
                s.push_str(&format!("last{body}"));
            } else {
                s.push_str(&body);
            }
        }
        s
    }
}

/// A prepared session: alphabet events only, with edges remapped and per-event flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbSession {
    pub id: String,
    pub day: u32,
    /// The session's task, when known (held-out folds and inner folds group by it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub ev: Vec<LEvent>,
    /// Original event index of each alphabet event.
    pub orig: Vec<u32>,
    pub flows: Vec<(u32, u32)>,
    pub same_art: Vec<(u32, u32)>,
    /// Bit `l`: the event is the last with its level-`l` label in its segment.
    pub last: Vec<u8>,
    /// A continuation item cannot use this event (a directive named its command earlier).
    pub exposed: Vec<bool>,
}

impl DbSession {
    pub fn has_flow(&self, p: usize, q: usize) -> bool {
        self.flows.binary_search(&(p as u32, q as u32)).is_ok()
    }
    pub fn has_same_art(&self, p: usize, q: usize) -> bool {
        self.same_art.binary_search(&(p as u32, q as u32)).is_ok()
    }
    pub fn len(&self) -> usize {
        self.ev.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ev.is_empty()
    }
}

/// The mining database: the label table, per-label supports over the sessions, the alphabet
/// and the prepared sessions (sorted by id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Db {
    pub labels: LabelTable,
    pub s_min: u32,
    pub gap: u32,
    /// Distinct sessions carrying each label (unexposed ok occurrences count for targets
    /// separately, in [`seq`]).
    pub support: Vec<u32>,
    /// Labels that may be items: support ≥ s_min.
    pub alphabet: Vec<bool>,
    pub sessions: Vec<DbSession>,
}

/// Per-label session support over `sessions`.
pub fn label_support(n_labels: usize, sessions: &[&LSession]) -> Vec<u32> {
    let mut sup = vec![0u32; n_labels];
    let mut seen: Vec<u32> = vec![u32::MAX; n_labels];
    for (si, s) in sessions.iter().enumerate() {
        for e in &s.events {
            for &l in e.lab.iter() {
                if l != NONE && seen[l as usize] != si as u32 {
                    seen[l as usize] = si as u32;
                    sup[l as usize] += 1;
                }
            }
        }
    }
    sup
}

impl Db {
    /// Build from labelled sessions (any order: they are sorted by id).
    pub fn build(labels: &LabelTable, sessions: &[&LSession], p: &MineParams) -> Db {
        let mut order: Vec<&LSession> = sessions.to_vec();
        order.sort_by(|a, b| a.id.cmp(&b.id));
        let s_min = p.s_min(order.len());
        let support = label_support(labels.len(), &order);
        let alphabet: Vec<bool> =
            support.iter().enumerate().map(|(i, &c)| c >= s_min && item_label(labels, i as u32)).collect();
        let mut db = Db { labels: labels.clone(), s_min, gap: p.gap, support, alphabet, sessions: vec![] };
        db.sessions = order.iter().map(|s| db.prepare(s)).collect();
        db
    }

    /// Prepare a session with this database's alphabet.
    pub fn prepare(&self, s: &LSession) -> DbSession {
        let keep: Vec<usize> = (0..s.events.len())
            .filter(|&i| s.events[i].lab.iter().any(|&l| l != NONE && self.alphabet[l as usize]))
            .collect();
        let mut map = vec![u32::MAX; s.events.len()];
        for (j, &i) in keep.iter().enumerate() {
            map[i] = j as u32;
        }
        let remap = |edges: &[(u32, u32)]| -> Vec<(u32, u32)> {
            let mut v: Vec<(u32, u32)> = edges
                .iter()
                .filter_map(|&(a, b)| {
                    let (x, y) = (*map.get(a as usize)?, *map.get(b as usize)?);
                    (x != u32::MAX && y != u32::MAX).then_some((x, y))
                })
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let ev: Vec<LEvent> = keep.iter().map(|&i| s.events[i]).collect();
        // `last` flags: scan backwards per segment.
        let mut last = vec![0u8; ev.len()];
        let mut seen: BTreeSet<(u16, u32)> = BTreeSet::new();
        for j in (0..ev.len()).rev() {
            for l in 0..LEVELS {
                let id = ev[j].lab[l];
                if id != NONE && seen.insert((ev[j].seg, id)) {
                    last[j] |= 1 << l;
                }
            }
        }
        let exposed: Vec<bool> = keep
            .iter()
            .map(|&i| s.influence.iter().any(|&(from, lab)| from as usize <= i && s.events[i].lab[0] == lab))
            .collect();
        DbSession {
            id: s.id.clone(),
            day: s.day,
            task: s.task.clone(),
            orig: keep.iter().map(|&i| i as u32).collect(),
            flows: remap(&s.flows),
            same_art: remap(&s.same_art),
            ev,
            last,
            exposed,
        }
    }

    pub fn level(&self, label: u32) -> Level {
        self.labels.level[label as usize]
    }

    pub fn ty(&self, label: u32) -> NodeType {
        self.labels.ty[label as usize]
    }

    /// Whether event `q` of `s` matches item `it` (label at its level, polarity, `last`, and,
    /// for a continuation item, not exposed).
    #[inline]
    pub fn node_ok(&self, s: &DbSession, q: usize, it: &Item, cont: bool) -> bool {
        let e = &s.ev[q];
        let l = self.labels.level[it.label as usize] as usize;
        e.lab[l] == it.label
            && match it.pol {
                Pol::Any => true,
                Pol::Ok => e.ok,
                Pol::Fail => !e.ok,
            }
            && (!it.last || s.last[q] & (1 << l) != 0)
            && (!cont || !s.exposed[q])
    }

    /// Whether `it` at `q` may follow `prev` at `p`: same segment, the relation (same batch in
    /// canonical order, or a later batch), within the gap, and the edge constraints.
    #[inline]
    pub fn edge_ok(&self, s: &DbSession, p: usize, prev: &Item, q: usize, it: &Item) -> bool {
        let (a, b) = (&s.ev[p], &s.ev[q]);
        if a.seg != b.seg || p.abs_diff(q) > self.gap as usize {
            return false;
        }
        let rel =
            if it.same { a.batch == b.batch && (prev.label, p) < (it.label, q) } else { q > p && b.batch > a.batch };
        rel && (!it.flow || s.has_flow(p, q)) && (!it.same_art || s.has_same_art(p, q))
    }

    /// Reachable end positions of each prefix of `items` (an exact dynamic program: every
    /// constraint is unary or joins adjacent items). Items from `cont_from` on are continuation
    /// items. `avail`, when given, masks the positions continuation items may use.
    #[allow(clippy::needless_range_loop)]
    pub fn reach(&self, s: &DbSession, items: &[Item], cont_from: usize, avail: Option<&[bool]>) -> Vec<Vec<u32>> {
        let n = s.len();
        let mut out: Vec<Vec<u32>> = Vec::with_capacity(items.len());
        if items.is_empty() {
            return out;
        }
        let ok_pos = |q: usize, i: usize| i < cont_from || avail.is_none_or(|a| a[q]);
        let first = items[0];
        out.push(
            (0..n).filter(|&q| ok_pos(q, 0) && self.node_ok(s, q, &first, cont_from == 0)).map(|q| q as u32).collect(),
        );
        let g = self.gap as usize;
        let mut mark = vec![false; n];
        for i in 1..items.len() {
            let (prev, it) = (&items[i - 1], &items[i]);
            let mut next = Vec::new();
            for &p in &out[i - 1] {
                let p = p as usize;
                let lo = p.saturating_sub(g);
                let hi = (p + g).min(n.saturating_sub(1));
                for q in lo..=hi {
                    if !mark[q]
                        && ok_pos(q, i)
                        && self.node_ok(s, q, it, i >= cont_from)
                        && self.edge_ok(s, p, prev, q, it)
                    {
                        mark[q] = true;
                        next.push(q as u32);
                    }
                }
            }
            for &q in &next {
                mark[q as usize] = false;
            }
            next.sort_unstable();
            if next.is_empty() {
                out.push(next);
                for _ in i + 1..items.len() {
                    out.push(Vec::new());
                }
                return out;
            }
            out.push(next);
        }
        out
    }

    /// Whether `items` occur in `s`.
    pub fn occurs(&self, s: &DbSession, items: &[Item], cont_from: usize) -> bool {
        self.reach(s, items, cont_from, None).last().is_some_and(|v| !v.is_empty())
    }

    /// Whether the continuation `ys` can follow an anchor at `p` (as item `prev`); returns the
    /// batch gap to the earliest first continuation event that completes the chain.
    pub fn follows(&self, s: &DbSession, p: usize, prev: &Item, ys: &[Item], cont: bool) -> Option<u32> {
        let g = self.gap as usize;
        let n = s.len();
        let y0 = ys.first()?;
        let lo = p.saturating_sub(g);
        let hi = (p + g).min(n.saturating_sub(1));
        let mut best: Option<u32> = None;
        for q in lo..=hi {
            if !(self.node_ok(s, q, y0, cont) && self.edge_ok(s, p, prev, q, y0)) {
                continue;
            }
            let bg = s.ev[q].batch.saturating_sub(s.ev[p].batch);
            if best.is_some_and(|b| b <= bg) {
                continue;
            }
            if ys.len() == 1 || self.chain_from(s, q, ys, cont) {
                best = Some(bg);
            }
        }
        best
    }

    /// Whether `ys[1..]` completes from `ys[0]` at `q`.
    fn chain_from(&self, s: &DbSession, q: usize, ys: &[Item], cont: bool) -> bool {
        let g = self.gap as usize;
        let n = s.len();
        let mut cur = vec![q];
        for i in 1..ys.len() {
            let mut next: Vec<usize> = Vec::new();
            for &p in &cur {
                for r in p.saturating_sub(g)..=(p + g).min(n.saturating_sub(1)) {
                    if !next.contains(&r)
                        && self.node_ok(s, r, &ys[i], cont)
                        && self.edge_ok(s, p, &ys[i - 1], r, &ys[i])
                    {
                        next.push(r);
                    }
                }
            }
            if next.is_empty() {
                return false;
            }
            cur = next;
        }
        true
    }
}

/// Whether a label may be an item at all (Act labels at the kind level may not).
pub fn item_label(t: &LabelTable, l: u32) -> bool {
    let (ty, lv) = (t.ty[l as usize], t.level[l as usize]);
    lv.allowed(ty) && !matches!(ty, NodeType::Obs | NodeType::Fail)
}

/// Whether a label can be a target (something a reflex could direct): an act whose effect is at
/// most `Environment`, or a read.
pub fn target_label(t: &LabelTable, l: u32) -> bool {
    match t.ty[l as usize] {
        NodeType::Act => {
            t.level[l as usize] != Level::Kind && t.effect[l as usize].is_none_or(|e| e <= Effect::Environment)
        }
        NodeType::Read => true,
        _ => false,
    }
}

/// A frequent pattern and its supporting sessions (indices into `Db::sessions`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinedPattern {
    pub pattern: Pattern,
    pub sessions: Vec<u32>,
    pub closed: bool,
}

/// The result of a mining run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mined {
    pub s_min: u32,
    pub sessions: usize,
    pub targets: usize,
    /// Frequent patterns (closed and not), sorted by pattern.
    pub patterns: Vec<MinedPattern>,
    pub expansions: u64,
    /// The node budget ran out: the result is a deterministic prefix of the full search.
    pub truncated: bool,
}

impl Mined {
    pub fn closed(&self) -> impl Iterator<Item = &MinedPattern> {
        self.patterns.iter().filter(|p| p.closed)
    }
}

/// Mine a database: [`seq::mine`], then [`closed::mark`].
pub fn mine(db: &Db, p: &MineParams) -> Mined {
    let run = seq::mine(db, p, false);
    let mut patterns: Vec<MinedPattern> = run
        .found
        .into_iter()
        .filter(|f| f.frequent && f.kept)
        .map(|f| MinedPattern { pattern: f.pattern, sessions: f.sessions, closed: false })
        .collect();
    patterns.sort_by(|a, b| a.pattern.cmp(&b.pattern));
    patterns.dedup_by(|a, b| a.pattern == b.pattern);
    closed::mark(db, &mut patterns);
    Mined {
        s_min: db.s_min,
        sessions: db.sessions.len(),
        targets: run.targets,
        patterns,
        expansions: run.expansions,
        truncated: run.truncated,
    }
}
