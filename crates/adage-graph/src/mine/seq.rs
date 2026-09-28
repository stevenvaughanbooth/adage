//! Target-rooted, gap-bounded, PrefixSpan-style growth (Track G §3.2).
//!
//! Targets are labels a reflex could direct ([`super::target_label`]) with support ≥ s_min,
//! after the taxonomy climb among themselves; they are searched in (support desc, label id)
//! order. For each target *y*:
//!
//! 1. its occurrences are pseudo-projections `(session, left, right)`;
//! 2. **backward growth** prepends a context item found within the gap before (or in the same
//!    batch as) the leftmost item, to depth 3 (PrefixSpan on reversed sequences);
//! 3. **forward growth** appends up to 2 continuation items (acts that succeeded, reads) after
//!    the rightmost item; prepending stops once a pattern has grown forward, so each pattern has
//!    one derivation;
//! 4. **constraint extension**: each candidate carries the subset of {flow, same_art, last} it
//!    requires (at most 3 per pattern);
//! 5. **taxonomy climb**: a candidate at a coarser level (or with polarity `Any`, or with fewer
//!    constraints) is kept only if its support is strictly above every one of its direct
//!    refinements (a child label, a specific polarity, one more constraint);
//! 6. support counts distinct sessions; candidates below s_min are pruned (exactly:
//!    support is anti-monotone), those within `border` of it are recorded as the negative border
//!    (when mining incrementally);
//! 7. a **beam**: at each node only the `max_children` best frequent, kept children are
//!    expanded ([`beam_cmp`]: support over the added label's support, so a specific failed run
//!    is not crowded out by reads every session makes); the others are not patterns of the
//!    result.
//!
//! A read or an edit at the kind level (`read`, `edit`) is an item only with a constraint
//! (`same_art`, `flow`, `last`): alone it would join every pattern and say nothing. A read in
//! the continuation must be **bound** (`same_art` or `flow` from the item before it; a read-batch
//! compiles only so, Track G §6.1), so a read target's first context item is joined to it by one
//! of those edges, and kind-level labels are never targets.
//!
//! **Budget.** Each expanded pattern node costs 1. Each target gets an equal share of the budget
//! (`budget / targets`) and is searched independently (in parallel when `threads > 1`), best
//! child first, so truncation is deterministic whatever the thread count and never starves the
//! later targets.

use super::{target_label, Db, DbSession, Item, MineParams, Pattern, Pol};
use crate::events::{Level, NodeType, LEVELS, NONE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A candidate recorded during growth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub pattern: Pattern,
    /// The pattern it extends, and how.
    pub parent: Pattern,
    pub ext: Ext,
    /// Supporting sessions (indices into `Db::sessions`), sorted.
    pub sessions: Vec<u32>,
    /// Support ≥ s_min.
    pub frequent: bool,
    /// Survived the taxonomy climb (so it was expanded, if frequent).
    pub kept: bool,
    /// The target this was grown from.
    pub target: u32,
}

/// All targets' growth.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub found: Vec<Found>,
    pub targets: usize,
    pub expansions: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Occ {
    s: u32,
    l: u32,
    r: u32,
}

/// A candidate extension: direction, label, polarity, relation and constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Ext {
    pub fwd: bool,
    pub label: u32,
    pub pol: Pol,
    pub same: bool,
    pub flow: bool,
    pub same_art: bool,
    pub last: bool,
}

impl Ext {
    fn flags(&self) -> usize {
        usize::from(self.flow) + usize::from(self.same_art) + usize::from(self.last)
    }
}

/// Target labels and their supports (sessions with an ok occurrence), after the climb.
#[allow(clippy::needless_range_loop)]
pub fn targets(db: &Db) -> Vec<(u32, u32)> {
    let n = db.labels.len();
    let mut sup = vec![0u32; n];
    let mut seen = vec![u32::MAX; n];
    for (si, s) in db.sessions.iter().enumerate() {
        for e in &s.ev {
            if e.ty == NodeType::Act && !e.ok {
                continue;
            }
            for &l in e.lab.iter() {
                if l != NONE && seen[l as usize] != si as u32 {
                    seen[l as usize] = si as u32;
                    sup[l as usize] += 1;
                }
            }
        }
    }
    // Kind-level labels (`read`) are never targets: alone they direct nothing.
    let ok = |l: usize| {
        db.alphabet[l] && sup[l] >= db.s_min && target_label(&db.labels, l as u32) && db.labels.level[l] != Level::Kind
    };
    let mut maxchild = vec![0u32; n];
    for l in 0..n {
        let p = db.labels.parent[l];
        if p != NONE && sup[l] > maxchild[p as usize] {
            maxchild[p as usize] = sup[l];
        }
    }
    let mut out: Vec<(u32, u32)> =
        (0..n).filter(|&l| ok(l) && sup[l] > maxchild[l]).map(|l| (l as u32, sup[l])).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// The taxonomy climb: for each extension, the highest support among its direct refinements (a
/// child label, a specific polarity, one more constraint). An extension is kept only when its
/// own support is strictly higher.
pub fn refinements(db: &Db, counts: impl Iterator<Item = (Ext, u32)>) -> HashMap<Ext, u32> {
    let mut maxref: HashMap<Ext, u32> = HashMap::new();
    for (k, c) in counts {
        let mut bump = |g: Ext| {
            let e = maxref.entry(g).or_insert(0);
            if c > *e {
                *e = c;
            }
        };
        let parent = db.labels.parent[k.label as usize];
        if parent != NONE {
            bump(Ext { label: parent, ..k });
        }
        if k.pol != Pol::Any {
            bump(Ext { pol: Pol::Any, ..k });
        }
        if k.flow {
            bump(Ext { flow: false, ..k });
        }
        if k.same_art {
            bump(Ext { same_art: false, ..k });
        }
        if k.last {
            bump(Ext { last: false, ..k });
        }
    }
    maxref
}

/// The beam's order of sibling extensions: extensions carrying evidence of a relation first (an
/// edge constraint, `last`, a failed run), then by informativeness, the support of the child
/// over the support of the label it adds (P(pattern | label)), then support, then the extension.
pub fn beam_cmp(db: &Db, a: (u32, &Ext), b: (u32, &Ext)) -> std::cmp::Ordering {
    let r = |c: u32, e: &Ext| c as f64 / db.support[e.label as usize].max(1) as f64;
    let evidence = |e: &Ext| usize::from(e.flow || e.same_art || e.last) + usize::from(e.pol == Pol::Fail);
    evidence(b.1).cmp(&evidence(a.1)).then(r(b.0, b.1).total_cmp(&r(a.0, a.1))).then(b.0.cmp(&a.0)).then(a.1.cmp(b.1))
}

struct Ctx<'a> {
    db: &'a Db,
    p: &'a MineParams,
    /// Record every counted candidate (the negative border and the climb's losers), for
    /// incremental mining; otherwise only the expanded patterns.
    record_all: bool,
    budget: u64,
    used: u64,
    truncated: bool,
    found: Vec<Found>,
    target: u32,
}

/// The candidate keys at one position `q` relative to an occurrence. `bound` requires the new
/// edge to carry `flow` or `same_art` (a read is directable only as a bound read-batch).
#[allow(clippy::too_many_arguments)]
fn keys_at(
    db: &Db,
    s: &DbSession,
    anchor: usize,
    anchor_item: &Item,
    q: usize,
    fwd: bool,
    room: usize,
    bound: bool,
    out: &mut Vec<(Ext, u32)>,
) {
    let e = &s.ev[q];
    let (a, b) = if fwd { (anchor, q) } else { (q, anchor) };
    let (ea, eb) = (&s.ev[a], &s.ev[b]);
    if ea.seg != eb.seg || a == b || a.abs_diff(b) > db.gap as usize {
        return;
    }
    let same = ea.batch == eb.batch;
    if !same && (b < a || eb.batch < ea.batch) {
        return;
    }
    if fwd {
        // Continuation: acts that succeeded, and reads, never exposed.
        if s.exposed[q] || !(e.ty == NodeType::Read || (e.ty == NodeType::Act && e.ok)) {
            return;
        }
    }
    let flow = s.has_flow(a, b);
    let same_art = s.has_same_art(a, b);
    // A continuation read, or the first context item of a read target, must be bound.
    let bound = bound || (fwd && e.ty == NodeType::Read);
    if bound && !flow && !same_art {
        return;
    }
    for l in 0..LEVELS {
        let label = e.lab[l];
        if label == NONE || !db.alphabet[label as usize] {
            continue;
        }
        if fwd && !super::target_label(&db.labels, label) {
            continue;
        }
        // Same batch: canonical order (label, position) must increase along the pattern.
        if same {
            let ord = if fwd { (anchor_item.label, a) < (label, b) } else { (label, a) < (anchor_item.label, b) };
            if !ord {
                continue;
            }
        }
        // `last` ("after the last edit") qualifies context edits only.
        let last = !fwd && e.ty == NodeType::Edit && s.last[q] & (1 << l) != 0;
        let pols: &[Pol] = match (e.ty, fwd) {
            (NodeType::Act, true) => &[Pol::Ok],
            (NodeType::Act, false) => {
                if e.ok {
                    &[Pol::Any, Pol::Ok]
                } else {
                    &[Pol::Any, Pol::Fail]
                }
            }
            _ => &[Pol::Any],
        };
        // Only the subsets of the constraints that hold here.
        let avail = u8::from(flow) | (u8::from(same_art) << 1) | (u8::from(last) << 2);
        for &pol in pols {
            let mut mask = avail;
            loop {
                let m = mask;
                mask = if mask == 0 { u8::MAX } else { (mask - 1) & avail };
                let (f, sa, la) = (m & 1 != 0, m & 2 != 0, m & 4 != 0);
                if bound && !f && !sa {
                    if m == 0 {
                        break;
                    }
                    continue;
                }
                // A read or an edit at the kind level says nothing on its own: it is an item
                // only with a constraint (a read of a listed path, the last edit). A failed act's
                // kind level is its failure family, an item as a failure.
                let k = Ext { fwd, label, pol, same, flow: f, same_art: sa, last: la };
                let family = e.ty == NodeType::Act && pol == Pol::Fail;
                let skip = (l == Level::Kind as usize && e.ty != NodeType::Prompt && m == 0 && !family)
                    || (l == Level::Kind as usize && e.ty == NodeType::Act && pol != Pol::Fail)
                    || k.flags() > room;
                if !skip {
                    out.push((k, q as u32));
                }
                if m == 0 {
                    break;
                }
            }
        }
    }
}

impl Ctx<'_> {
    /// The child pattern of `pat` under key `k`.
    fn child(pat: &Pattern, n_back: usize, k: &Ext) -> Pattern {
        let mut items = pat.items.clone();
        let mut new = Item::new(k.label, k.pol);
        if k.fwd {
            new.same = k.same;
            new.flow = k.flow;
            new.same_art = k.same_art;
            items.push(new);
            Pattern { items, cut: n_back.max(1) }
        } else {
            new.last = k.last;
            let head = &mut items[0];
            head.same = k.same;
            head.flow = k.flow;
            head.same_art = k.same_art;
            items.insert(0, new);
            Pattern { items, cut: n_back + 1 }
        }
    }

    fn expand(&mut self, pat: &Pattern, occs: &[Occ], n_back: usize, n_fwd: usize) {
        if self.used >= self.budget {
            self.truncated = true;
            return;
        }
        self.used += 1;
        let (db, p) = (self.db, self.p);
        let can_back = n_fwd == 0 && n_back < p.max_back && pat.items.len() < p.max_items;
        let can_fwd = n_fwd < p.max_fwd && pat.items.len() < p.max_items;
        if !can_back && !can_fwd {
            return;
        }
        let room = p.max_constraints.saturating_sub(pat.constraints());
        let g = db.gap as usize;
        // Pass 1: distinct-session counts per key.
        let mut counts: HashMap<Ext, (u32, u32)> = HashMap::new();
        let mut buf: Vec<(Ext, u32)> = Vec::new();
        let mut i = 0;
        while i < occs.len() {
            let s_idx = occs[i].s;
            let s = &db.sessions[s_idx as usize];
            buf.clear();
            while i < occs.len() && occs[i].s == s_idx {
                let o = occs[i];
                self.cands(s, o, pat, n_back, can_back, can_fwd, room, g, &mut buf);
                i += 1;
            }
            let mut ks: Vec<Ext> = buf.iter().map(|x| x.0).collect();
            ks.sort_unstable();
            ks.dedup();
            for k in ks {
                let e = counts.entry(k).or_insert((0, u32::MAX));
                if e.1 != s_idx {
                    e.0 += 1;
                    e.1 = s_idx;
                }
            }
        }
        let floor = db.s_min.saturating_sub(p.border).max(1);
        let maxref = refinements(db, counts.iter().map(|(k, &(c, _))| (*k, c)));
        let mut interest: Vec<(Ext, u32, bool)> = counts
            .iter()
            .filter(|(_, &(c, _))| c >= floor)
            .map(|(k, &(c, _))| (*k, c, c > maxref.get(k).copied().unwrap_or(0)))
            .collect();
        // At the root, a failed run just before the target is the `recovers_with` relation (Track
        // G §1.1: the next successful act after a failure), and the last edit before it is the
        // obligation's relation: the evidence of the guards, expanded before any other context,
        // so a busy trace cannot push them out of the beam.
        let recovers = |k: &Ext| {
            let root = n_back == 0 && n_fwd == 0 && !k.fwd;
            u8::from(root && k.last) + 2 * u8::from(root && k.pol == Pol::Fail)
        };
        interest.sort_by(|a, b| recovers(&b.0).cmp(&recovers(&a.0)).then(beam_cmp(db, (a.1, &a.0), (b.1, &b.0))));
        // The beam: only the best `max_children` frequent, kept children are expanded.
        let mut rank = 0usize;
        for x in interest.iter_mut() {
            if x.2 && x.1 >= db.s_min {
                x.2 = rank < p.max_children;
                rank += 1;
            }
        }
        if !self.record_all {
            interest.retain(|x| x.2 && x.1 >= db.s_min);
        }
        if interest.is_empty() {
            return;
        }
        // Pass 2: projections for the keys of interest.
        let index: HashMap<Ext, usize> = interest.iter().enumerate().map(|(j, x)| (x.0, j)).collect();
        let mut proj: Vec<Vec<Occ>> = vec![Vec::new(); interest.len()];
        for &o in occs {
            let s = &db.sessions[o.s as usize];
            buf.clear();
            self.cands(s, o, pat, n_back, can_back, can_fwd, room, g, &mut buf);
            for &(k, q) in &buf {
                if let Some(&j) = index.get(&k) {
                    proj[j].push(if k.fwd { Occ { s: o.s, l: o.l, r: q } } else { Occ { s: o.s, l: q, r: o.r } });
                }
            }
        }
        for (j, (k, c, kept)) in interest.into_iter().enumerate() {
            let mut po = std::mem::take(&mut proj[j]);
            po.sort_unstable();
            po.dedup();
            let mut sessions: Vec<u32> = po.iter().map(|o| o.s).collect();
            sessions.dedup();
            debug_assert_eq!(sessions.len() as u32, c);
            let child = Self::child(pat, n_back, &k);
            let frequent = c >= db.s_min;
            self.found.push(Found {
                pattern: child.clone(),
                parent: pat.clone(),
                ext: k,
                sessions,
                frequent,
                kept,
                target: self.target,
            });
            if frequent && kept {
                let (nb, nf) = if k.fwd { (n_back, n_fwd + 1) } else { (n_back + 1, n_fwd) };
                self.expand(&child, &po, nb, nf);
                if self.truncated {
                    return;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cands(
        &self,
        s: &DbSession,
        o: Occ,
        pat: &Pattern,
        n_back: usize,
        can_back: bool,
        can_fwd: bool,
        room: usize,
        g: usize,
        buf: &mut Vec<(Ext, u32)>,
    ) {
        let n = s.len();
        let (l, r) = (o.l as usize, o.r as usize);
        // The first prepend makes the target a continuation item: it must not be exposed.
        if can_back && !(n_back == 0 && s.exposed[l]) {
            let first = pat.items[0];
            let bound = n_back == 0 && self.db.ty(first.label) == NodeType::Read;
            for q in l.saturating_sub(g)..=(l + g).min(n - 1) {
                keys_at(self.db, s, l, &first, q, false, room, bound, buf);
            }
        }
        if can_fwd {
            let last = pat.items[pat.items.len() - 1];
            for q in r.saturating_sub(g)..=(r + g).min(n - 1) {
                keys_at(self.db, s, r, &last, q, true, room, false, buf);
            }
        }
    }
}

/// Grow one target with a budget.
fn grow(db: &Db, p: &MineParams, target: u32, budget: u64, record_all: bool) -> (Vec<Found>, u64, bool) {
    let item = Item::new(target, if db.ty(target) == NodeType::Act { Pol::Ok } else { Pol::Any });
    let lvl = db.level(target) as usize;
    let mut occs = Vec::new();
    for (si, s) in db.sessions.iter().enumerate() {
        for q in 0..s.len() {
            if s.ev[q].lab[lvl] == target && db.node_ok(s, q, &item, false) {
                occs.push(Occ { s: si as u32, l: q as u32, r: q as u32 });
            }
        }
    }
    let mut cx = Ctx { db, p, record_all, budget, used: 0, truncated: false, found: Vec::new(), target };
    let root = Pattern { items: vec![item], cut: 0 };
    cx.expand(&root, &occs, 0, 0);
    (cx.found, cx.used, cx.truncated)
}

/// Grow every target, merge in target order under the budget. `record_all` also records the
/// negative border and the candidates the climb or the beam dropped (for [`super::border`]).
pub fn mine(db: &Db, p: &MineParams, record_all: bool) -> Run {
    let ts = targets(db);
    // Every target gets an equal share of the budget, so the most frequent targets (often the
    // least informative) cannot starve the rest; a share a target leaves unused is not passed on,
    // which keeps targets independent (parallel, and the same for any thread count).
    let share = if ts.is_empty() { 0 } else { p.budget / ts.len() as u64 };
    let threads = p.threads.max(1).min(ts.len().max(1));
    let results: Vec<(Vec<Found>, u64, bool)> = if threads <= 1 {
        ts.iter().map(|&(t, _)| grow(db, p, t, share, record_all)).collect()
    } else {
        let mut slots: Vec<Option<(Vec<Found>, u64, bool)>> = vec![None; ts.len()];
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads)
                .map(|w| {
                    let ts = &ts;
                    scope.spawn(move || {
                        (w..ts.len())
                            .step_by(threads)
                            .map(|i| (i, grow(db, p, ts[i].0, share, record_all)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for h in handles {
                for (i, r) in h.join().expect("mining thread") {
                    slots[i] = Some(r);
                }
            }
        });
        slots.into_iter().map(|x| x.expect("every target mined")).collect()
    };
    let mut run = Run { targets: ts.len(), ..Default::default() };
    for (found, used, truncated) in results {
        run.found.extend(found);
        run.expansions += used;
        run.truncated |= truncated;
    }
    run
}
