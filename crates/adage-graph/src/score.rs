//! Scoring: frequent is not useful (Track G §5).
//!
//! Each closed pattern P with its cut gives a candidate reflex: context X = P[..cut],
//! continuation Y = P[cut..]. For each:
//!
//! - **Support gates:** `supp ≥ min_support` distinct unexposed sessions and `days ≥ min_days`
//!   (capped at the number of distinct days in the data, so a one-day corpus can pass).
//! - **Confidence:** `conf = supp(X ⇒ Y) / opp(X)` over sessions, with discounted weights
//!   `0.5^(age / half_life)`; the posterior is `Beta(s + κm, n − s + κ(1 − m))` whose prior mean
//!   m is the parent context's posterior (X without its earliest item), recursively down to the
//!   base rate of Y: the variable-order backoff. `conf_lb` is its 5% quantile ([`crate::stats`]).
//! - **Lift and contrast** at the anchor level: `conf_a` is the share of X's end events followed
//!   by Y within the gap (edge constraints checked from the anchor); the background is the same
//!   share over every alphabet event (lift = conf_a / background) and over every event that does
//!   not end X (contrast = conf_a − that).
//! - **Consistency**, for patterns with a failed-act item: the share of that label's runs that
//!   failed (the minimum over such items).
//! - **Value:** `V = p_need · (1 − p_soon) · Δturns − λ_fp · (1 − p_need)`, with p_need the
//!   posterior mean, p_soon the share of X's end events whose Y starts in the same or the next
//!   batch, Δturns the median batch gap from X to Y. For a pattern whose context holds a failed
//!   act, p_soon is 0: its directive is delivered before the failing run, and what it saves is
//!   the turns from the failure to the recovery (Δturns), however soon the agent recovers.
//! - **p_next:** the share of X's end events whose very next alphabet event is Y's first item
//!   (the next-action predictor's confidence).
//! - **Exposed:** sessions where P occurs only through exposed continuation events.
//! - **Target variant:** the dominant exact label of the target over its matches; a target with
//!   no variant holding ≥ 80% is ambiguous (never compiled to a directed command).
//!
//! The gate is support and days, then consistency ≥ 0.9 for a pattern with a failed act
//! (a dead end is a property of the runner, not of a contrast) or contrast ≥ `contrast`
//! otherwise, and V > 0 for a directive (not for a guard: an obligation after `last(edit)` or an
//! avoid after a failure, whose worth is in the sessions that would miss them; a guard needs a
//! Beta lower bound ≥ 0.5 instead).
//!
//! [`holdout`] evaluates selected reflexes on held-out sessions: next-action top-1 accuracy
//! against an order-1 baseline, conformance (mean −ln p of the next event) and calibration ECE.

use crate::events::{Level, NodeType, NONE};
use crate::mine::{Db, DbSession, Item, MinedPattern, Pattern, Pol};
use crate::stats;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoreParams {
    pub min_support: u32,
    pub min_days: u32,
    pub kappa: f64,
    pub lambda_fp: f64,
    /// The lower bound's quantile.
    pub quantile: f64,
    pub half_life_days: f64,
    /// `learn.contrast`.
    pub contrast: f64,
    /// `dead_end_consistency` as a share.
    pub consistency: f64,
    /// A target variant must hold this share of the target's matches.
    pub dominant: f64,
    /// A guard (obligation, avoid) needs this Beta lower bound: it may stop the agent or name a
    /// runner as broken, so a coin flip is not enough.
    pub guard_lower_bound: f64,
    /// Anchor-level measures (lift, contrast, p_soon, p_next, Δturns, the target variant) are
    /// computed on at most this many sessions: every k-th session by id, k = ⌈N / this⌉.
    /// Support, days and the Beta posterior always use every session.
    pub anchor_sessions: usize,
    pub threads: usize,
}

impl Default for ScoreParams {
    fn default() -> Self {
        ScoreParams {
            min_support: 5,
            min_days: 2,
            kappa: 2.0,
            lambda_fp: 2.0,
            quantile: 0.05,
            half_life_days: 60.0,
            contrast: 0.3,
            consistency: 0.9,
            dominant: 0.8,
            guard_lower_bound: 0.5,
            anchor_sessions: 300,
            threads: 1,
        }
    }
}

/// A scored candidate reflex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scored {
    pub pattern: Pattern,
    pub sessions: Vec<u32>,
    /// Sessions where the context occurs (the opportunities).
    pub opp_list: Vec<u32>,
    pub support: u32,
    pub days: u32,
    /// Discounted opportunity and success weights.
    pub opp: f64,
    pub succ: f64,
    /// Opportunity sessions (undiscounted).
    pub opp_sessions: u32,
    pub prior: f64,
    pub conf: f64,
    pub conf_lb: f64,
    /// `conf` and `conf_lb` after calibration ([`Calibration`]; equal to them until then).
    #[serde(default)]
    pub conf_cal: f64,
    #[serde(default)]
    pub conf_lb_cal: f64,
    pub alpha: f64,
    pub beta: f64,
    pub lift: f64,
    pub contrast: f64,
    pub conf_anchor: f64,
    pub consistency: Option<f64>,
    pub p_soon: f64,
    pub p_next: f64,
    pub dturns: f64,
    pub value: f64,
    pub exposed: u32,
    /// The dominant exact label of the target and its share of the target's matches.
    pub variant: Option<u32>,
    pub variant_share: f64,
    pub passes: bool,
    /// Why it fails the gate, when it does.
    pub gate: String,
}

/// Corpus-wide quantities every score needs.
pub struct Stats {
    pub weight: Vec<f64>,
    pub total_weight: f64,
    /// Sessions containing each label (at its level), sorted.
    pub postings: Vec<Vec<u32>>,
    /// Positions of each label, per posting: for label `l`, the positions in session
    /// `postings[l][k]` are `pos[l][start[l][k]..start[l][k + 1]]`, ascending. Joins walk these
    /// lists instead of rescanning sessions.
    start: Vec<Vec<u32>>,
    pos: Vec<Vec<u32>>,
    /// Alphabet events in the anchor sample.
    pub positions: u64,
    /// Sessions in the anchor sample.
    pub sample: Vec<bool>,
    /// Alphabet events per node type.
    pub by_type: BTreeMap<NodeType, u64>,
    pub days: u32,
    pub runs: Vec<u32>,
    pub fails: Vec<u32>,
    /// Sessions with any exposed event.
    pub exposed: Vec<u32>,
    /// Supporting sessions of every mined pattern, by (items, cut).
    pub known: HashMap<(Vec<Item>, usize), Vec<u32>>,
}

/// The sessions (sorted) where an item list occurs, and their summed discounted weight.
#[derive(Debug, Clone, Default)]
pub struct Occ {
    pub sessions: Vec<u32>,
    pub weight: f64,
}

type OccKey = (Vec<Item>, usize);
type BgKey = (u32, Vec<Item>);

/// The joins and background counts a pattern's score reads ([`plan`]).
struct Plan {
    /// Y alone (the base rate of the backoff chain).
    y_alone: OccKey,
    /// Per context suffix length k = 1..=cut: (X_k, X_k ++ Y), both with the continuation from |X_k|.
    chain: Vec<(OccKey, OccKey)>,
    /// (the anchor's label when the continuation starts in its batch, else nothing; Y).
    background: BgKey,
}

fn plan(pat: &Pattern) -> Plan {
    let cut = pat.cut.max(1).min(pat.items.len() - 1);
    let xs = &pat.items[..cut];
    let ys = &pat.items[cut..];
    let y_alone: Vec<Item> = std::iter::once(ys[0].first()).chain(ys[1..].iter().copied()).collect();
    let chain = (1..=cut)
        .map(|k| {
            let mut xk: Vec<Item> = xs[cut - k..].to_vec();
            xk[0] = xk[0].first();
            let full: Vec<Item> = xk.iter().chain(ys.iter()).copied().collect();
            ((xk, k), (full, k))
        })
        .collect();
    let x_last = xs[cut - 1];
    let key_label = if ys[0].same { x_last.label } else { NONE };
    Plan { y_alone: (y_alone, 0), chain, background: (key_label, ys.to_vec()) }
}

/// Every join and background count the scores of a pattern set read, computed once (in
/// parallel: each value is a pure function of its key, so the thread count changes nothing)
/// and shared read-only by the scoring threads.
#[derive(Default)]
pub struct Cache {
    occ: HashMap<OccKey, Occ>,
    background: HashMap<BgKey, BTreeMap<NodeType, u64>>,
}

impl Cache {
    pub fn new() -> Cache {
        Cache::default()
    }

    /// Precompute what scoring `patterns` needs. A mined pattern's sessions are known (mining
    /// and the matcher agree exactly) and are not joined again. Each `X_k ++ Y` join extends the
    /// end positions of its context `X_k` (computed once per context), and a join that only
    /// needs to know whether a session matches stops at the first match.
    pub fn build(db: &Db, st: &Stats, patterns: &[&MinedPattern], threads: usize) -> Cache {
        let mut occ: HashMap<OccKey, Occ> = HashMap::new();
        // Work units, in first-use order: a Y-alone join, or a context with its extensions.
        let mut alone: Vec<OccKey> = Vec::new();
        let mut group_of: HashMap<OccKey, usize> = HashMap::new();
        let mut groups: Vec<(OccKey, Vec<OccKey>)> = Vec::new();
        let mut seen: HashMap<OccKey, ()> = HashMap::new();
        let mut bg_seen: HashMap<BgKey, ()> = HashMap::new();
        let mut bg_keys: Vec<BgKey> = Vec::new();
        for m in patterns {
            let pl = plan(&m.pattern);
            if !seen.contains_key(&pl.y_alone) {
                seen.insert(pl.y_alone.clone(), ());
                if let Some(v) = st.known.get(&pl.y_alone) {
                    occ.insert(pl.y_alone.clone(), known_occ(st, v));
                } else {
                    alone.push(pl.y_alone);
                }
            }
            for (x, full) in pl.chain {
                let g = match group_of.get(&x) {
                    Some(&g) => g,
                    None => {
                        group_of.insert(x.clone(), groups.len());
                        groups.push((x, Vec::new()));
                        groups.len() - 1
                    }
                };
                if !seen.contains_key(&full) {
                    seen.insert(full.clone(), ());
                    if let Some(v) = st.known.get(&full) {
                        occ.insert(full, known_occ(st, v));
                    } else {
                        groups[g].1.push(full);
                    }
                }
            }
            if !bg_seen.contains_key(&pl.background) {
                bg_seen.insert(pl.background.clone(), ());
                bg_keys.push(pl.background);
            }
        }
        for (k, o) in alone.iter().zip(par_map(&alone, threads, |k| occ_of(db, st, &k.0, k.1))) {
            occ.insert(k.clone(), o);
        }
        for (g, outs) in groups.iter().zip(par_map(&groups, threads, |(x, fulls)| extend_group(db, st, x, fulls))) {
            let mut outs = outs.into_iter();
            occ.insert(g.0.clone(), outs.next().expect("context"));
            for (k, o) in g.1.iter().zip(outs) {
                occ.insert(k.clone(), o);
            }
        }
        let bgs = par_map(&bg_keys, threads, |k| background(db, st, k.0, &k.1));
        Cache { occ, background: bg_keys.into_iter().zip(bgs).collect() }
    }

    fn occ(&self, db: &Db, st: &Stats, key: &OccKey) -> std::borrow::Cow<'_, Occ> {
        match self.occ.get(key) {
            Some(o) => std::borrow::Cow::Borrowed(o),
            None => std::borrow::Cow::Owned(occ_of(db, st, &key.0, key.1)),
        }
    }

    fn background(&self, db: &Db, st: &Stats, key: &BgKey) -> std::borrow::Cow<'_, BTreeMap<NodeType, u64>> {
        match self.background.get(key) {
            Some(b) => std::borrow::Cow::Borrowed(b),
            None => std::borrow::Cow::Owned(background(db, st, key.0, &key.1)),
        }
    }
}

/// Map `f` over `items` on `threads` workers (work shared by an atomic cursor); the result is
/// in input order whatever the thread count.
pub fn par_map<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = threads.max(1).min(items.len().max(1));
    if threads <= 1 {
        return items.iter().map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let hs: Vec<_> = (0..threads)
            .map(|_| {
                let (next, f) = (&next, &f);
                scope.spawn(move || {
                    let mut got = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if i >= items.len() {
                            break;
                        }
                        got.push((i, f(&items[i])));
                    }
                    got
                })
            })
            .collect();
        for h in hs {
            for (i, r) in h.join().expect("worker thread") {
                out[i] = Some(r);
            }
        }
    });
    out.into_iter().map(|x| x.expect("mapped")).collect()
}

fn known_occ(st: &Stats, v: &[u32]) -> Occ {
    Occ { weight: v.iter().map(|&i| st.weight[i as usize]).sum(), sessions: v.to_vec() }
}

/// Sessions holding every one of `items`' labels (a posting-list intersection), ascending.
fn candidates(st: &Stats, items: &[Item]) -> Vec<u32> {
    let mut lists: Vec<&[u32]> = items.iter().map(|i| st.postings[i.label as usize].as_slice()).collect();
    lists.sort_by_key(|v| v.len());
    let Some((first, rest)) = lists.split_first() else { return Vec::new() };
    let mut cursor: Vec<usize> = vec![0; rest.len()];
    let mut out = Vec::new();
    'next: for &si in first.iter() {
        for (c, l) in cursor.iter_mut().zip(rest) {
            while *c < l.len() && l[*c] < si {
                *c += 1;
            }
            if *c >= l.len() || l[*c] != si {
                continue 'next;
            }
        }
        out.push(si);
    }
    out
}

fn occ_from(st: &Stats, sessions: Vec<u32>) -> Occ {
    Occ { weight: sessions.iter().map(|&i| st.weight[i as usize]).sum(), sessions }
}

/// The sessions where `items` occur (a posting-list join: only sessions holding every item's
/// label are matched, each by the dynamic program of [`Stats::ends`], stopping at the first
/// full match), with their summed weight.
fn occ_of(db: &Db, st: &Stats, items: &[Item], cont_from: usize) -> Occ {
    if let Some(v) = st.known.get(&(items.to_vec(), cont_from)) {
        return known_occ(st, v);
    }
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let out = candidates(st, items)
        .into_iter()
        .filter(|&si| {
            st.first_row(db, si, items, cont_from, true, &mut a);
            st.extend(db, si, items, 1, cont_from, true, &mut a, &mut b);
            !a.is_empty()
        })
        .collect();
    occ_from(st, out)
}

/// A context `x` (its items all context) and joins extending it (`x ++ Y`, the continuation
/// from |x|): the context's end positions are computed once per session and extended. Returns
/// the context's sessions, then each extension's, in order.
fn extend_group(db: &Db, st: &Stats, x: &OccKey, fulls: &[OccKey]) -> Vec<Occ> {
    let (items, c) = (&x.0, x.1);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let mut ends: Vec<(u32, Vec<u32>)> = Vec::new();
    for si in candidates(st, items) {
        st.first_row(db, si, items, c, false, &mut a);
        st.extend(db, si, items, 1, c, false, &mut a, &mut b);
        if !a.is_empty() {
            ends.push((si, a.clone()));
        }
    }
    let mut out = Vec::with_capacity(fulls.len() + 1);
    out.push(occ_from(st, ends.iter().map(|e| e.0).collect()));
    for (full, cf) in fulls {
        let rest = &full[c..];
        let mut sess = Vec::new();
        for (si, e) in &ends {
            if rest.iter().any(|it| st.positions(it.label, *si).is_empty()) {
                continue;
            }
            a.clear();
            a.extend_from_slice(e);
            st.extend(db, *si, full, c, *cf, true, &mut a, &mut b);
            if !a.is_empty() {
                sess.push(*si);
            }
        }
        out.push(occ_from(st, sess));
    }
    out
}

impl Stats {
    pub fn new(db: &Db, p: &ScoreParams, known: &[MinedPattern]) -> Stats {
        let known = known.iter().map(|m| ((m.pattern.items.clone(), m.pattern.cut), m.sessions.clone())).collect();
        let maxday = db.sessions.iter().map(|s| s.day).max().unwrap_or(0);
        let weight: Vec<f64> = db
            .sessions
            .iter()
            .map(|s| {
                let age = if s.day == 0 { 0.0 } else { (maxday - s.day) as f64 };
                0.5f64.powf(age / p.half_life_days.max(1e-9))
            })
            .collect();
        let n = db.labels.len();
        let mut postings: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut start: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut pos: Vec<Vec<u32>> = vec![Vec::new(); n];
        let (mut runs, mut fails) = (vec![0u32; n], vec![0u32; n]);
        let mut positions = 0u64;
        let mut by_type: BTreeMap<NodeType, u64> = BTreeMap::new();
        let stride = db.sessions.len().div_ceil(p.anchor_sessions.max(1)).max(1);
        let sample: Vec<bool> = (0..db.sessions.len()).map(|i| i % stride == 0).collect();
        for (si, s) in db.sessions.iter().enumerate() {
            if sample[si] {
                positions += s.len() as u64;
            }
            for (q, e) in s.ev.iter().enumerate() {
                if sample[si] {
                    *by_type.entry(e.ty).or_default() += 1;
                }
                for &l in e.lab.iter() {
                    if l == NONE {
                        continue;
                    }
                    let l = l as usize;
                    if postings[l].last() != Some(&(si as u32)) {
                        postings[l].push(si as u32);
                        start[l].push(pos[l].len() as u32);
                    }
                    pos[l].push(q as u32);
                    runs[l] += 1;
                    if e.ty == NodeType::Act && !e.ok {
                        fails[l] += 1;
                    }
                }
            }
        }
        for (s, p) in start.iter_mut().zip(&pos) {
            s.push(p.len() as u32);
        }
        let days: BTreeSet<u32> = db.sessions.iter().map(|s| s.day).collect();
        let exposed =
            (0..db.sessions.len() as u32).filter(|&i| db.sessions[i as usize].exposed.iter().any(|&x| x)).collect();
        Stats {
            known,
            exposed,
            total_weight: weight.iter().sum(),
            weight,
            postings,
            start,
            pos,
            positions,
            sample,
            by_type,
            days: days.len() as u32,
            runs,
            fails,
        }
    }

    pub fn positions_of(&self, ty: NodeType) -> u64 {
        self.by_type.get(&ty).copied().unwrap_or(0)
    }

    /// Positions of label `l` in session `si`, ascending.
    pub fn positions(&self, l: u32, si: u32) -> &[u32] {
        let l = l as usize;
        match self.postings[l].binary_search(&si) {
            Ok(k) => &self.pos[l][self.start[l][k] as usize..self.start[l][k + 1] as usize],
            Err(_) => &[],
        }
    }

    /// The reachable end positions of `items` in session `si`: the last row of
    /// [`Db::reach`] (the same dynamic program, over each item's position list instead of every
    /// event). `cur` and `next` are scratch; the result borrows `cur`.
    pub fn ends<'a>(
        &self,
        db: &Db,
        si: u32,
        items: &[Item],
        cont_from: usize,
        cur: &'a mut Vec<u32>,
        next: &mut Vec<u32>,
    ) -> &'a [u32] {
        self.first_row(db, si, items, cont_from, false, cur);
        self.extend(db, si, items, 1, cont_from, false, cur, next);
        cur
    }

    /// Row 0 of the dynamic program: the positions matching the first item (only the first
    /// one with `exists` when it is the last row).
    fn first_row(&self, db: &Db, si: u32, items: &[Item], cont_from: usize, exists: bool, cur: &mut Vec<u32>) {
        cur.clear();
        let Some(first) = items.first() else { return };
        let s = &db.sessions[si as usize];
        let stop = exists && items.len() == 1;
        for &q in self.positions(first.label, si) {
            if db.node_ok(s, q as usize, first, cont_from == 0) {
                cur.push(q);
                if stop {
                    return;
                }
            }
        }
    }

    /// Rows `from..` of the dynamic program, starting from row `from − 1` in `cur` (ascending);
    /// the last row is left in `cur`. With `exists`, the last row stops at its first position.
    /// Each row either merges the previous row with the item's position list or, when the
    /// previous row is much shorter, probes the list around each previous position.
    #[allow(clippy::too_many_arguments)]
    fn extend(
        &self,
        db: &Db,
        si: u32,
        items: &[Item],
        from: usize,
        cont_from: usize,
        exists: bool,
        cur: &mut Vec<u32>,
        next: &mut Vec<u32>,
    ) {
        let s = &db.sessions[si as usize];
        let g = db.gap as usize;
        for i in from..items.len() {
            if cur.is_empty() {
                return;
            }
            let last_row = exists && i + 1 == items.len();
            next.clear();
            let (prev, it) = (&items[i - 1], &items[i]);
            let cont = i >= cont_from;
            let ps = self.positions(it.label, si);
            if cur.len() * 8 < ps.len() {
                'probe: for &p in cur.iter() {
                    let pu = p as usize;
                    let lo = ps.partition_point(|&q| (q as usize) + g < pu);
                    for &q in &ps[lo..] {
                        let qu = q as usize;
                        if qu > pu + g {
                            break;
                        }
                        if db.node_ok(s, qu, it, cont) && db.edge_ok(s, pu, prev, qu, it) {
                            next.push(q);
                            if last_row {
                                break 'probe;
                            }
                        }
                    }
                }
                next.sort_unstable();
                next.dedup();
            } else {
                let mut lo = 0usize;
                for &q in ps {
                    let qu = q as usize;
                    if !db.node_ok(s, qu, it, cont) {
                        continue;
                    }
                    while lo < cur.len() && (cur[lo] as usize) + g < qu {
                        lo += 1;
                    }
                    let mut j = lo;
                    while j < cur.len() && (cur[j] as usize) <= qu + g {
                        if db.edge_ok(s, cur[j] as usize, prev, qu, it) {
                            next.push(q);
                            break;
                        }
                        j += 1;
                    }
                    if last_row && !next.is_empty() {
                        break;
                    }
                }
            }
            std::mem::swap(cur, next);
        }
    }
}

/// Score one pattern (every join it reads comes from `cache`, or is computed when missing).
pub fn score(db: &Db, st: &Stats, mp: &MinedPattern, p: &ScoreParams, cache: &Cache) -> Scored {
    let pat = &mp.pattern;
    let cut = pat.cut.max(1).min(pat.items.len() - 1);
    let xs = &pat.items[..cut];
    let ys = &pat.items[cut..];
    let pl = plan(pat);
    // Base rate of Y, then the backoff chain over context suffixes.
    let mut prior = {
        let w = cache.occ(db, st, &pl.y_alone).weight;
        if st.total_weight > 0.0 {
            w / st.total_weight
        } else {
            0.0
        }
    };
    let mut last = (0.0, 0.0, 0u32);
    for (k, (xk, full)) in pl.chain.iter().enumerate() {
        let opp = cache.occ(db, st, xk);
        let succ = cache.occ(db, st, full);
        let (n, s) = (opp.weight, succ.weight);
        last = (n, s, opp.sessions.len() as u32);
        if k + 1 < cut {
            prior = (s + p.kappa * prior) / (n + p.kappa);
        }
    }
    let (opp, succ, opp_sessions) = last;
    let (alpha, beta) = stats::posterior(succ, opp, prior, p.kappa);
    let conf = alpha / (alpha + beta);
    let conf_lb = stats::beta_quantile(p.quantile, alpha, beta);

    // Anchor-level statistics.
    let x_last = xs[cut - 1];
    let (mut x_ends, mut x_follow, mut soon, mut next) = (0u64, 0u64, 0u64, 0u64);
    let mut gaps: Vec<f64> = Vec::new();
    let mut variants: BTreeMap<u32, u32> = BTreeMap::new();
    let first_x = &pl.chain[cut - 1].0;
    let x_sessions = cache.occ(db, st, first_x);
    let opp_list: Vec<u32> = x_sessions.sessions.clone();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for &si in x_sessions.sessions.iter().filter(|&&si| st.sample[si as usize]) {
        let sess = &db.sessions[si as usize];
        for &q in st.ends(db, si, &first_x.0, cut, &mut a, &mut b) {
            x_ends += 1;
            if let Some((gap, y0)) = follows_at(db, st, si, q as usize, &x_last, ys) {
                x_follow += 1;
                gaps.push(gap as f64);
                if gap <= 1 {
                    soon += 1;
                }
                if y0 == q as usize + 1 {
                    next += 1;
                }
                *variants.entry(sess.ev[y0].lab[0]).or_default() += 1;
            }
        }
    }
    // Background: anchors of X's last item's type (every alphabet event when X ends at every
    // one of them); only anchors near a match of Y's first item can be followed.
    // The anchor matters to the background only through same-batch ordering (its label) and
    // its type (counted separately), so continuations share one count across contexts.
    let by_type = cache.background(db, st, &pl.background);
    let bg_any: u64 = by_type.values().sum();
    let bg_same = by_type.get(&db.ty(x_last.label)).copied().unwrap_or(0);
    let conf_anchor = if x_ends > 0 { x_follow as f64 / x_ends as f64 } else { 0.0 };
    let same_n = st.positions_of(db.ty(x_last.label));
    let (bg, bg_n) = if same_n > x_ends { (bg_same, same_n) } else { (bg_any, st.positions) };
    let bg_rate = if bg_n > 0 { bg as f64 / bg_n as f64 } else { 0.0 };
    let others = bg_n.saturating_sub(x_ends);
    let bg_other = if others > 0 { bg.saturating_sub(x_follow) as f64 / others as f64 } else { 0.0 };
    let lift = if bg_rate > 0.0 { conf_anchor / bg_rate } else { 0.0 };
    let contrast = conf_anchor - bg_other;
    // A context that ends with a failed act: a dead end, delivered before the failing run.
    let has_fail = x_last.pol == Pol::Fail;
    let p_soon = if x_ends > 0 && !has_fail { soon as f64 / x_ends as f64 } else { 0.0 };
    let p_next = if x_ends > 0 { next as f64 / x_ends as f64 } else { 0.0 };
    let dturns = stats::median(&mut gaps);
    let consistency = pat
        .items
        .iter()
        .filter(|i| i.pol == Pol::Fail)
        .map(|i| {
            let r = st.runs[i.label as usize];
            if r == 0 {
                0.0
            } else {
                st.fails[i.label as usize] as f64 / r as f64
            }
        })
        .reduce(f64::min);
    let value = conf * (1.0 - p_soon) * dturns - p.lambda_fp * (1.0 - conf);
    let tot: u32 = variants.values().sum();
    let (variant, variant_share) = match variants.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))) {
        Some((&v, &c)) if tot > 0 => {
            let share = c as f64 / tot as f64;
            (Some(v).filter(|_| share >= p.dominant), share)
        }
        _ => (None, 0.0),
    };
    // Exposed: occurrences only through exposed continuation events.
    let mut exposed = 0u32;
    for &si in &st.exposed {
        if mp.sessions.binary_search(&si).is_err()
            && !st.ends(db, si, &pat.items, pat.items.len(), &mut a, &mut b).is_empty()
        {
            exposed += 1;
        }
    }
    let days: BTreeSet<u32> = mp.sessions.iter().map(|&i| db.sessions[i as usize].day).collect();
    let support = mp.sessions.len() as u32;
    let min_days = p.min_days.min(st.days.max(1));
    let gate = if support < p.min_support.max(db.s_min) {
        format!("support {support} < {}", p.min_support.max(db.s_min))
    } else if (days.len() as u32) < min_days {
        format!("days {} < {min_days}", days.len())
    } else if let Some(c) = consistency {
        if c < p.consistency {
            format!("consistency {c:.2} < {}", p.consistency)
        } else {
            String::new()
        }
    } else if contrast < p.contrast {
        format!("contrast {contrast:.2} < {}", p.contrast)
    } else {
        String::new()
    };
    // A directive with no value is never delivered (yellow needs V > 0, Track G §6.1). An
    // obligation (after the last edit) or an avoid (after a failure) is a guard: its worth is in
    // the sessions that would miss it, which p_soon does not see, so its gate is contrast or
    // consistency alone.
    let guard = has_fail || (x_last.last && db.ty(x_last.label) == NodeType::Edit);
    let gate = if !gate.is_empty() {
        gate
    } else if !guard && value <= 0.0 {
        format!("value {value:.2} <= 0")
    } else if guard && conf_lb < p.guard_lower_bound {
        format!("guard lower bound {conf_lb:.2} < {}", p.guard_lower_bound)
    } else {
        gate
    };
    Scored {
        pattern: Pattern { items: pat.items.clone(), cut },
        sessions: mp.sessions.clone(),
        opp_list,
        support,
        days: days.len() as u32,
        opp,
        succ,
        opp_sessions,
        prior,
        conf,
        conf_lb,
        conf_cal: conf,
        conf_lb_cal: conf_lb,
        alpha,
        beta,
        lift,
        contrast,
        conf_anchor,
        consistency,
        p_soon,
        p_next,
        dturns,
        value,
        exposed,
        variant,
        variant_share,
        passes: gate.is_empty(),
        gate,
    }
}

/// Alphabet events of the anchor sample followed by `ys` as if they were an anchor labelled
/// `key_label` (it matters only when Y starts in the anchor's batch), per event type: first the
/// Y matches whose continuation completes, then every anchor they can follow.
#[allow(clippy::needless_range_loop)]
fn background(db: &Db, st: &Stats, key_label: u32, ys: &[Item]) -> BTreeMap<NodeType, u64> {
    let y0 = ys[0];
    let prev = Item::new(key_label, Pol::Any);
    let mut out: BTreeMap<NodeType, u64> = BTreeMap::new();
    let g = db.gap as usize;
    let mut qs: Vec<usize> = Vec::new();
    let mut mark: Vec<bool> = Vec::new();
    for &si in st.postings[y0.label as usize].iter().filter(|&&si| st.sample[si as usize]) {
        let sess = &db.sessions[si as usize];
        qs.clear();
        for &q in st.positions(y0.label, si) {
            let q = q as usize;
            if db.node_ok(sess, q, &y0, true) && (ys.len() == 1 || db.follows(sess, q, &y0, &ys[1..], true).is_some()) {
                qs.push(q);
            }
        }
        if qs.is_empty() {
            continue;
        }
        mark.clear();
        mark.resize(sess.len(), false);
        for &q in &qs {
            for a in q.saturating_sub(g)..=(q + g).min(sess.len() - 1) {
                if !mark[a] && db.edge_ok(sess, a, &prev, q, &y0) {
                    mark[a] = true;
                    *out.entry(sess.ev[a].ty).or_default() += 1;
                }
            }
        }
    }
    out
}

/// `Db::follows` from the anchor at `p` of session `si`, also returning the first continuation
/// event (the earliest with the smallest batch gap).
fn follows_at(db: &Db, st: &Stats, si: u32, p: usize, prev: &Item, ys: &[Item]) -> Option<(u32, usize)> {
    let g = db.gap as usize;
    let s = &db.sessions[si as usize];
    let y0 = ys.first()?;
    let ps = st.positions(y0.label, si);
    let lo = ps.partition_point(|&q| (q as usize) + g < p);
    let mut best: Option<(u32, usize)> = None;
    for &q in &ps[lo..] {
        let q = q as usize;
        if q > p + g {
            break;
        }
        if !(db.node_ok(s, q, y0, true) && db.edge_ok(s, p, prev, q, y0)) {
            continue;
        }
        let gap = s.ev[q].batch.saturating_sub(s.ev[p].batch);
        if best.is_some_and(|b| b.0 <= gap) {
            continue;
        }
        if ys.len() == 1 || db.follows(s, q, y0, &ys[1..], true).is_some() {
            best = Some((gap, q));
        }
    }
    best
}

/// Score every closed pattern (in parallel when `threads > 1`; the result is in input order).
/// `known` is every mined pattern, whose supporting sessions need no matching.
pub fn score_all(db: &Db, closed: &[&MinedPattern], known: &[MinedPattern], p: &ScoreParams) -> Vec<Scored> {
    let st = Stats::new(db, p, known);
    let cache = Cache::build(db, &st, closed, p.threads);
    par_map(closed, p.threads, |m| score(db, &st, m, p, &cache))
}

/// One context feature of the next-action model ([`NextModel`]), read at an anchor position t
/// (the model predicts the act at t + 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "feat")]
pub enum Feat {
    /// The label of event t − `back` at taxonomy `level` (or the nearest finer defined level,
    /// so a singleton keeps its exact label); a fixed value before the session start.
    Ev { back: u8, level: u8 },
    /// The polarity of event t: an act that succeeded, one that failed, or not an act.
    Ok,
    /// The label of the prompt that opened event t's segment, at `level` (the intent prototype
    /// at level 1).
    Intent { level: u8 },
    /// The label of the `back`-th most recent act at or before t, at `level`.
    Act { back: u8, level: u8 },
    /// Whether an edit happened since the most recent act (or the segment start).
    Edited,
    /// Whether any edit happened in the segment up to t (the session's phase: exploring or
    /// changing).
    EverEdited,
    /// t's position in its segment, bucketed (0, 1, 2, 3–5, 6–10, 11+).
    Pos,
    /// The label of event t − `back`: exact for an act or a prompt, `level` for an artifact
    /// access (paths are task-specific; commands are not).
    Mixed { back: u8, level: u8 },
}

impl Feat {
    /// `x0` exact label of t, `p1` prototype of t − 1, `c`, `h`, `k` coarse, head, kind; `o` the
    /// polarity; `i1` the segment's intent prototype.
    pub fn parse(s: &str) -> Option<Feat> {
        let (c, rest) = s.split_at(1.min(s.len()));
        if c == "o" {
            return Some(Feat::Ok);
        }
        if c == "e" {
            return Some(Feat::Edited);
        }
        if c == "E" {
            return Some(Feat::EverEdited);
        }
        if c == "n" {
            return Some(Feat::Pos);
        }
        if c == "m" || c == "M" {
            let n: u8 = rest.parse().ok()?;
            return Some(Feat::Mixed { back: n, level: if c == "m" { 3 } else { 1 } });
        }
        if c == "a" || c == "b" {
            let n: u8 = rest.parse().ok()?;
            return Some(Feat::Act { back: n, level: if c == "a" { 0 } else { 3 } });
        }
        let n: u8 = rest.parse().ok()?;
        let level = |c: &str| ["x", "p", "c", "h", "k"].iter().position(|&x| x == c).map(|l| l as u8);
        match c {
            "i" => Some(Feat::Intent { level: n.min(4) }),
            _ => Some(Feat::Ev { back: n, level: level(c)? }),
        }
    }
}

/// How a context's counts are shrunk towards its parent's distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Escape {
    /// A Beta/Dirichlet prior of `kappa` pseudo-counts at the parent's distribution
    /// (Track G §5: the longest context with evidence speaks, shorter contexts shrink it).
    Kappa,
    /// PPM-C (Witten-Bell): the escape mass is the number of distinct successors seen.
    Ppmc,
}

/// Next-action model parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NextParams {
    /// The backoff chain, coarse to fine: context k is the first k features.
    pub chain: Vec<Feat>,
    pub escape: Escape,
    pub kappa: f64,
    /// Candidate chains; when there are several, [`NextModel::fit`] picks one per repository by
    /// cross-validation on the training sessions (task-grouped inner folds).
    pub candidates: Vec<Vec<Feat>>,
    pub folds: usize,
    /// Weight of the session's own earlier transitions (0: none).
    pub session_weight: f64,
    /// Taxonomy level of the predicted act's label (0: exact).
    pub target: u8,
}

impl Default for NextParams {
    fn default() -> Self {
        NextParams {
            chain: ["h0", "o", "x0", "h1", "x1", "x2"].iter().filter_map(|f| Feat::parse(f)).collect(),
            escape: Escape::Kappa,
            kappa: 2.0,
            candidates: Vec::new(),
            folds: 3,
            session_weight: 0.0,
            target: 0,
        }
    }
}

/// Parse a comma-separated chain (`h0,x0,h1,x1,x2`).
pub fn parse_chain(s: &str) -> Vec<Feat> {
    s.split(',').filter_map(Feat::parse).collect()
}

/// Inner fold of each session: distinct tasks (the id when there is none) in sorted order,
/// dealt round-robin.
pub fn inner_folds(sessions: &[DbSession], k: usize) -> Vec<usize> {
    let key = |s: &DbSession| s.task.clone().unwrap_or_else(|| s.id.clone());
    let tasks: BTreeSet<String> = sessions.iter().map(key).collect();
    let rank: BTreeMap<&String, usize> = tasks.iter().enumerate().map(|(i, t)| (t, i)).collect();
    sessions.iter().map(|s| rank[&key(s)] % k.max(1)).collect()
}

#[derive(Debug, Clone, Default)]
struct Row {
    n: u32,
    c: BTreeMap<u32, u32>,
}

impl Row {
    fn add(&mut self, y: u32) {
        self.n += 1;
        *self.c.entry(y).or_default() += 1;
    }
}

/// Value before the session start.
const BOS: u32 = u32::MAX - 1;

/// The variable-order backoff model of the next act (Track G §5, Design §8.5): counts of the
/// next act's exact label per context of the chain; a prediction starts from the base rate and
/// walks the chain from the shortest context to the longest, each context with evidence
/// shrinking its counts towards the previous distribution. Deterministic: counts are exact and
/// ties go to the smaller label id.
pub struct NextModel {
    p: NextParams,
    /// The candidate chosen by [`NextModel::fit`] and each candidate's inner-fold (hits,
    /// positions, nats).
    pub chosen: usize,
    pub inner: Vec<(u64, u64, f64)>,
    vocab: Vec<u32>,
    uni: Row,
    rows: Vec<HashMap<Vec<u32>, Row>>,
}

/// Every position `t` of `s` whose next event is an act.
fn act_positions(s: &DbSession) -> impl Iterator<Item = usize> + '_ {
    (0..s.len().saturating_sub(1)).filter(|&t| s.ev[t + 1].ty == NodeType::Act)
}

/// The label of the act at `q` at the model's target level (the nearest finer defined one).
fn target_of(s: &DbSession, q: usize, level: u8) -> u32 {
    let lab = &s.ev[q].lab;
    (0..=level as usize).rev().map(|l| lab[l]).find(|&x| x != NONE).unwrap_or(NONE)
}

fn feat_value(s: &DbSession, t: usize, f: Feat) -> u32 {
    let at = |q: usize, level: u8| {
        let lab = &s.ev[q].lab;
        (0..=level as usize).rev().map(|l| lab[l]).find(|&x| x != NONE).unwrap_or(NONE)
    };
    match f {
        Feat::Ev { back, level } => {
            if t < back as usize {
                BOS
            } else {
                at(t - back as usize, level)
            }
        }
        Feat::Ok => match (s.ev[t].ty, s.ev[t].ok) {
            (NodeType::Act, true) => 1,
            (NodeType::Act, false) => 0,
            _ => 2,
        },
        Feat::Intent { level } => {
            let seg = s.ev[t].seg;
            (0..=t)
                .rev()
                .take_while(|&q| s.ev[q].seg == seg)
                .find(|&q| s.ev[q].ty == NodeType::Prompt)
                .map_or(BOS, |q| at(q, level))
        }
        Feat::Act { back, level } => {
            (0..=t).rev().filter(|&q| s.ev[q].ty == NodeType::Act).nth(back as usize).map_or(BOS, |q| at(q, level))
        }
        Feat::Edited => {
            let seg = s.ev[t].seg;
            let mut e = 0;
            for q in (0..=t).rev() {
                if s.ev[q].seg != seg || s.ev[q].ty == NodeType::Act {
                    break;
                }
                if s.ev[q].ty == NodeType::Edit {
                    e = 1;
                    break;
                }
            }
            e
        }
        Feat::Mixed { back, level } => {
            if t < back as usize {
                BOS
            } else {
                let q = t - back as usize;
                at(q, if matches!(s.ev[q].ty, NodeType::Read | NodeType::Edit) { level } else { 0 })
            }
        }
        Feat::EverEdited => {
            let seg = s.ev[t].seg;
            u32::from((0..=t).rev().take_while(|&q| s.ev[q].seg == seg).any(|q| s.ev[q].ty == NodeType::Edit))
        }
        Feat::Pos => {
            let seg = s.ev[t].seg;
            let k = (0..=t).rev().take_while(|&q| s.ev[q].seg == seg).count() - 1;
            match k {
                0..=2 => k as u32,
                3..=5 => 3,
                6..=10 => 4,
                _ => 5,
            }
        }
    }
}

impl NextModel {
    pub fn train(sessions: &[DbSession], p: &NextParams) -> NextModel {
        let mut m = NextModel {
            p: p.clone(),
            chosen: 0,
            inner: Vec::new(),
            vocab: Vec::new(),
            uni: Row::default(),
            rows: vec![HashMap::new(); p.chain.len()],
        };
        let mut vocab: BTreeSet<u32> = BTreeSet::new();
        for s in sessions {
            for t in act_positions(s) {
                let y = target_of(s, t + 1, p.target);
                vocab.insert(y);
                m.uni.add(y);
                let mut key = Vec::with_capacity(p.chain.len());
                for (k, &f) in p.chain.iter().enumerate() {
                    key.push(feat_value(s, t, f));
                    m.rows[k].entry(key.clone()).or_default().add(y);
                }
            }
        }
        m.vocab = vocab.into_iter().collect();
        m
    }

    /// Train on `sessions`, first choosing the chain among `p.candidates` (when there are
    /// several) by task-grouped inner cross-validation: most held-out top-1 hits, then fewest
    /// nats, then the earlier candidate.
    pub fn fit(sessions: &[DbSession], p: &NextParams) -> NextModel {
        if p.candidates.is_empty() {
            return NextModel::train(sessions, p);
        }
        let fold = inner_folds(sessions, p.folds);
        let mut inner = Vec::new();
        for chain in &p.candidates {
            let q = NextParams { chain: chain.clone(), candidates: Vec::new(), ..p.clone() };
            let (mut hits, mut n, mut nats) = (0u64, 0u64, 0.0);
            for f in 0..p.folds.max(1) {
                let tr: Vec<DbSession> =
                    sessions.iter().zip(&fold).filter(|x| *x.1 != f).map(|x| x.0.clone()).collect();
                if tr.is_empty() {
                    continue;
                }
                let m = NextModel::train(&tr, &q);
                for (s, _) in sessions.iter().zip(&fold).filter(|x| *x.1 == f) {
                    for t in act_positions(s) {
                        let y = s.ev[t + 1].lab[0];
                        n += 1;
                        hits += u64::from(m.predict(s, t).0 == y);
                        nats -= m.prob(s, t, y).max(1e-12).ln();
                    }
                }
            }
            inner.push((hits, n, nats));
        }
        let chosen = (0..inner.len())
            .min_by(|&a, &b| inner[b].0.cmp(&inner[a].0).then(inner[a].2.total_cmp(&inner[b].2)).then(a.cmp(&b)))
            .unwrap_or(0);
        let q = NextParams { chain: p.candidates[chosen].clone(), candidates: Vec::new(), ..p.clone() };
        let mut m = NextModel::train(sessions, &q);
        m.chosen = chosen;
        m.inner = inner;
        m
    }

    /// The predictive distribution after position `t`: the candidate acts (the training
    /// vocabulary, plus acts seen earlier in this session when the session's own history counts),
    /// their probabilities, and the mass left for any other act.
    ///
    /// With `session_weight` w > 0, the session's own earlier transitions (positions before t,
    /// whose next act is already observed) are added to every context's counts with weight w: a
    /// cache model, since agents repeat themselves within a session.
    pub fn dist(&self, s: &DbSession, t: usize) -> (Vec<u32>, Vec<f64>, f64) {
        let w = self.p.session_weight;
        let k_n = self.p.chain.len();
        let mut ov_uni = Row::default();
        let mut ov: Vec<HashMap<Vec<u32>, Row>> = vec![HashMap::new(); if w > 0.0 { k_n } else { 0 }];
        if w > 0.0 {
            for u in act_positions(s).take_while(|&u| u < t) {
                let y = target_of(s, u + 1, self.p.target);
                ov_uni.add(y);
                let mut key = Vec::with_capacity(k_n);
                for (k, &f) in self.p.chain.iter().enumerate() {
                    key.push(feat_value(s, u, f));
                    ov[k].entry(key.clone()).or_default().add(y);
                }
            }
        }
        let mut cands: Vec<u32> = self.vocab.clone();
        for &y in ov_uni.c.keys() {
            if let Err(i) = cands.binary_search(&y) {
                cands.insert(i, y);
            }
        }
        let v = cands.len() as f64 + 1.0;
        let empty = Row::default();
        let shrink = |row: &Row, o: &Row, prior: &[f64], prior_unseen: f64, ppmc: bool| -> (Vec<f64>, f64) {
            let n = row.n as f64 + w * o.n as f64;
            let e = match (self.p.escape, ppmc) {
                (Escape::Kappa, false) => self.p.kappa,
                _ => (row.c.keys().chain(o.c.keys()).collect::<BTreeSet<_>>().len() as f64).max(1.0),
            };
            let z = n + e;
            let out = cands
                .iter()
                .zip(prior)
                .map(|(y, &pr)| {
                    let c = row.c.get(y).copied().unwrap_or(0) as f64 + w * o.c.get(y).copied().unwrap_or(0) as f64;
                    (c + e * pr) / z
                })
                .collect();
            (out, e * prior_unseen / z)
        };
        // The base rate escapes to a novel act as PPM-C does (the number of distinct acts seen),
        // half of that mass spread over the known acts, half left for a new one.
        let flat = vec![0.5 / (v - 1.0).max(1.0); cands.len()];
        let (base, base_unseen) = shrink(&self.uni, &ov_uni, &flat, 0.5, true);
        let (mut p, mut unseen) = (base.clone(), base_unseen);
        let mut key = Vec::with_capacity(k_n);
        for (k, &f) in self.p.chain.iter().enumerate() {
            key.push(feat_value(s, t, f));
            let row = self.rows[k].get(&key);
            let o = ov.get(k).and_then(|m| m.get(&key));
            if row.is_none_or(|r| r.n == 0) && o.is_none() {
                // An unseen context: every longer one is unseen too.
                break;
            }
            let (q, u) = shrink(row.unwrap_or(&empty), o.unwrap_or(&empty), &p, unseen, false);
            p = q;
            unseen = u;
        }
        // A tenth of the mass stays on the base rate: a long context is never certain.
        let p = p.iter().zip(&base).map(|(a, b)| 0.9 * a + 0.1 * b).collect();
        (cands, p, 0.9 * unseen + 0.1 * base_unseen)
    }

    /// The most probable next act and its probability (ties to the smaller label id).
    pub fn predict(&self, s: &DbSession, t: usize) -> (u32, f64) {
        let (cands, p, _) = self.dist(s, t);
        let mut best = (NONE, -1.0);
        for (y, &q) in cands.iter().zip(&p) {
            if q > best.1 {
                best = (*y, q);
            }
        }
        best
    }

    /// P(the act after `t` is `y`).
    pub fn prob(&self, s: &DbSession, t: usize, y: u32) -> f64 {
        let (cands, p, unseen) = self.dist(s, t);
        match cands.binary_search(&y) {
            Ok(i) => p[i],
            Err(_) => unseen,
        }
    }
}

/// Order-k (k = 2) exact-label baseline: the most frequent next act after the last two events,
/// else after the last one, else the most frequent act.
pub struct Order2 {
    rows: HashMap<(u32, u32), BTreeMap<u32, u32>>,
    one: Order1,
}

impl Order2 {
    pub fn train(db: &Db) -> Order2 {
        let mut rows: HashMap<(u32, u32), BTreeMap<u32, u32>> = HashMap::new();
        for s in &db.sessions {
            for t in act_positions(s) {
                let prev2 = if t == 0 { BOS } else { s.ev[t - 1].lab[0] };
                *rows.entry((prev2, s.ev[t].lab[0])).or_default().entry(s.ev[t + 1].lab[0]).or_default() += 1;
            }
        }
        Order2 { rows, one: Order1::train(db) }
    }

    pub fn predict(&self, s: &DbSession, t: usize) -> u32 {
        let prev2 = if t == 0 { BOS } else { s.ev[t - 1].lab[0] };
        match self.rows.get(&(prev2, s.ev[t].lab[0])) {
            Some(r) => argmax(r),
            None => self.one.predict(s.ev[t].lab[0]),
        }
    }
}

fn argmax(r: &BTreeMap<u32, u32>) -> u32 {
    let mut best = (NONE, 0u32);
    for (&l, &c) in r {
        if c > best.1 {
            best = (l, c);
        }
    }
    best.0
}

/// The oracle ceilings of held-out top-1 accuracy for a context: the share of held-out
/// positions a predictor would get right if it knew, for each context value, the held-out
/// data's own most frequent next act (no predictor using only that context can do better),
/// and the held-out conditional entropy H(next act | context) in bits.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Oracle {
    pub hits: u64,
    pub positions: u64,
    /// Σ over contexts of n·H(next | context), bits (divide by `positions`).
    pub nbits: f64,
    /// Fraction; set by [`Holdout::merge`].
    pub ceiling: f64,
    pub entropy_bits: f64,
}

fn oracle(test: &[DbSession], key: impl Fn(&DbSession, usize) -> Vec<u32>) -> Oracle {
    let mut rows: BTreeMap<Vec<u32>, BTreeMap<u32, u32>> = BTreeMap::new();
    for s in test {
        for t in act_positions(s) {
            *rows.entry(key(s, t)).or_default().entry(s.ev[t + 1].lab[0]).or_default() += 1;
        }
    }
    let mut o = Oracle::default();
    for r in rows.values() {
        let n: u32 = r.values().sum();
        o.positions += n as u64;
        o.hits += r.values().max().copied().unwrap_or(0) as u64;
        for &c in r.values() {
            let p = c as f64 / n as f64;
            o.nbits -= c as f64 * p.log2();
        }
    }
    o
}

impl Oracle {
    fn add(&mut self, o: &Oracle) {
        self.hits += o.hits;
        self.positions += o.positions;
        self.nbits += o.nbits;
    }
    fn finish(&mut self) {
        if self.positions > 0 {
            self.ceiling = self.hits as f64 / self.positions as f64;
            self.entropy_bits = self.nbits / self.positions as f64;
        }
    }
}

/// Order-1 act model: counts of the next act's exact label given the previous event's exact
/// label, from the training sessions.
pub struct Order1 {
    rows: HashMap<u32, HashMap<u32, u32>>,
    totals: HashMap<u32, u32>,
    best: HashMap<u32, u32>,
    global: u32,
    vocab: f64,
}

impl Order1 {
    pub fn train(db: &Db) -> Order1 {
        let mut rows: HashMap<u32, HashMap<u32, u32>> = HashMap::new();
        let mut uni: BTreeMap<u32, u32> = BTreeMap::new();
        for s in &db.sessions {
            for w in s.ev.windows(2) {
                if w[1].ty != NodeType::Act {
                    continue;
                }
                *rows.entry(w[0].lab[0]).or_default().entry(w[1].lab[0]).or_default() += 1;
            }
            for e in s.ev.iter().filter(|e| e.ty == NodeType::Act) {
                *uni.entry(e.lab[0]).or_default() += 1;
            }
        }
        let pick = |m: &mut dyn Iterator<Item = (u32, u32)>| -> u32 {
            let mut best = (NONE, 0u32);
            for (l, c) in m {
                if c > best.1 || (c == best.1 && l < best.0) {
                    best = (l, c);
                }
            }
            best.0
        };
        let global = pick(&mut uni.iter().map(|(&l, &c)| (l, c)));
        let best = rows.iter().map(|(&k, r)| (k, pick(&mut r.iter().map(|(&l, &c)| (l, c))))).collect();
        let totals = rows.iter().map(|(&k, r)| (k, r.values().sum())).collect();
        Order1 { rows, totals, best, global, vocab: uni.len() as f64 + 1.0 }
    }

    pub fn predict(&self, prev: u32) -> u32 {
        self.best.get(&prev).copied().unwrap_or(self.global)
    }

    /// How often the model's own top-1 prediction after `prev` was right in training (the
    /// unsmoothed share; 0 for an unseen `prev`).
    pub fn best_share(&self, prev: u32) -> f64 {
        let t = self.totals.get(&prev).copied().unwrap_or(0);
        if t == 0 {
            return 0.0;
        }
        let c = self.rows.get(&prev).and_then(|r| r.get(&self.predict(prev))).copied().unwrap_or(0);
        c as f64 / t as f64
    }

    pub fn prob(&self, prev: u32, next: u32) -> f64 {
        let a = 0.5;
        let c = self.rows.get(&prev).and_then(|r| r.get(&next)).copied().unwrap_or(0) as f64;
        let t = self.totals.get(&prev).copied().unwrap_or(0) as f64;
        (c + a) / (t + a * self.vocab)
    }
}

/// A scored candidate as the held-out evaluation sees it.
#[derive(Debug, Clone)]
pub struct Rule {
    pub pattern: Pattern,
    /// The posterior mean from training, before and after calibration ([`Calibration`]).
    pub conf: f64,
    pub conf_cal: f64,
    pub conf_lb: f64,
    /// P(the next alphabet event is the target | the context just ended).
    pub p_next: f64,
    /// The directed act (dominant exact label of the target), when there is one.
    pub variant: Option<u32>,
    /// Selected into the code table (a reflex), not only a candidate.
    pub selected: bool,
}

/// Calibration bins of width 0.1: (sum of predicted, sum of observed, count).
pub type Bins = Vec<(f64, f64, u64)>;

fn bins10() -> Bins {
    vec![(0.0, 0.0, 0); 10]
}

fn bin_add(b: &mut Bins, pred: f64, outcome: bool) {
    let i = ((pred * 10.0) as usize).min(9);
    b[i].0 += pred;
    b[i].1 += f64::from(u8::from(outcome));
    b[i].2 += 1;
}

/// Expected calibration error over 10 equal-width bins, and the pair count.
pub fn ece_of(b: &Bins) -> (f64, u64) {
    let n: u64 = b.iter().map(|x| x.2).sum();
    let mut e = 0.0;
    for x in b.iter().filter(|x| x.2 > 0) {
        e += (x.2 as f64 / n as f64) * (x.0 / x.2 as f64 - x.1 / x.2 as f64).abs();
    }
    (e, n)
}

/// A 90% percentile bootstrap interval of the ECE of `pairs` (1,000 resamples from a fixed
/// generator: deterministic).
pub fn ece_ci(pairs: &[(f64, bool)]) -> (f64, f64) {
    if pairs.is_empty() {
        return (0.0, 0.0);
    }
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        state >> 33
    };
    let mut es: Vec<f64> = (0..1000)
        .map(|_| {
            let mut b = bins10();
            for _ in 0..pairs.len() {
                let (p, o) = pairs[(next() % pairs.len() as u64) as usize];
                bin_add(&mut b, p, o);
            }
            ece_of(&b).0
        })
        .collect();
    es.sort_by(|a, b| a.total_cmp(b));
    (es[49], es[949])
}

/// Held-out evaluation (Track G §5 conformance, Design §8.5).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Holdout {
    pub sessions: usize,
    /// Positions whose next alphabet event is an act.
    pub positions: u64,
    /// The backoff model ([`NextModel`]), the order-1 and the order-2 exact-label baselines.
    pub top1_engine: f64,
    pub top1_baseline: f64,
    pub top1_order2: f64,
    /// Oracle ceilings with the order-1, order-2 and order-3 exact contexts ([`Oracle`]).
    pub oracle: Vec<Oracle>,
    /// The chain each fold's model chose (index into [`NextParams::candidates`]).
    pub chosen: Vec<usize>,
    /// Mean over sessions of the mean −ln p(next act).
    pub conformance_engine: f64,
    pub conformance_baseline: f64,
    /// Calibration of the calibrated posterior mean over every held-out (session, candidate)
    /// pair where the candidate's context occurs; `ece_raw` is the same pairs before
    /// calibration; `ece_selected` only the selected reflexes' pairs, with a 90% bootstrap
    /// interval.
    pub ece: f64,
    pub ece_pairs: u64,
    pub ece_raw: f64,
    pub ece_selected: f64,
    pub ece_selected_pairs: u64,
    pub ece_selected_ci: (f64, f64),
    /// Raw counts, so held-out folds pool exactly ([`Holdout::merge`]).
    pub hits_engine: u64,
    pub hits_baseline: u64,
    pub hits_order2: u64,
    pub nats_engine: f64,
    pub nats_baseline: f64,
    pub scored_sessions: u32,
    pub bins: Bins,
    pub bins_raw: Bins,
    pub bins_selected: Bins,
    /// (calibrated prediction, outcome) of the selected reflexes' pairs (for the interval).
    pub selected_pairs: Vec<(f64, bool)>,
}

impl Holdout {
    fn finish(&mut self) {
        let r = |a: u64, b: u64| if b == 0 { 0.0 } else { a as f64 / b as f64 };
        self.top1_engine = r(self.hits_engine, self.positions);
        self.top1_baseline = r(self.hits_baseline, self.positions);
        self.top1_order2 = r(self.hits_order2, self.positions);
        for o in self.oracle.iter_mut() {
            o.finish();
        }
        if self.scored_sessions > 0 {
            self.conformance_engine = self.nats_engine / self.scored_sessions as f64;
            self.conformance_baseline = self.nats_baseline / self.scored_sessions as f64;
        }
        (self.ece, self.ece_pairs) = ece_of(&self.bins);
        self.ece_raw = ece_of(&self.bins_raw).0;
        (self.ece_selected, self.ece_selected_pairs) = ece_of(&self.bins_selected);
        self.ece_selected_ci = ece_ci(&self.selected_pairs);
    }

    /// Pool held-out folds.
    pub fn merge(parts: &[Holdout]) -> Holdout {
        let mut h = Holdout { bins: bins10(), bins_raw: bins10(), bins_selected: bins10(), ..Default::default() };
        let add = |a: &mut Bins, b: &Bins| {
            for (x, y) in a.iter_mut().zip(b) {
                x.0 += y.0;
                x.1 += y.1;
                x.2 += y.2;
            }
        };
        for p in parts {
            h.sessions += p.sessions;
            h.positions += p.positions;
            h.hits_engine += p.hits_engine;
            h.hits_baseline += p.hits_baseline;
            h.hits_order2 += p.hits_order2;
            if h.oracle.len() < p.oracle.len() {
                h.oracle.resize(p.oracle.len(), Oracle::default());
            }
            for (a, b) in h.oracle.iter_mut().zip(&p.oracle) {
                a.add(b);
            }
            h.chosen.extend(&p.chosen);
            h.nats_engine += p.nats_engine;
            h.nats_baseline += p.nats_baseline;
            h.scored_sessions += p.scored_sessions;
            add(&mut h.bins, &p.bins);
            add(&mut h.bins_raw, &p.bins_raw);
            add(&mut h.bins_selected, &p.bins_selected);
            h.selected_pairs.extend(&p.selected_pairs);
        }
        h.finish();
        h
    }
}

/// Calibration bins over [0, 1].
pub const CAL_BINS: usize = 200;

/// A monotone map from a candidate's posterior mean to the held-out rate at which its
/// continuation follows its context (Track G §5 calibration). It is fitted on the training
/// sessions alone: each candidate's posterior is recomputed on task-grouped inner folds (its
/// opportunity and support lists restricted to the other folds, the same prior and κ) and
/// compared with the outcomes in the held-out inner fold; the pairs are binned (width 1/200)
/// and fitted by isotonic regression (pool adjacent violators). Deterministic. On small corpora
/// the candidates are re-mined and re-scored on each inner training side
/// (`select::fit_calibration`), exactly as the outer held-out evaluation does, so the fold's
/// outcomes do not flatter patterns mined from them; [`Calibration::fit`] is the cheap form.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// (mean predicted, mean observed) of each isotonic block, ascending; linear in between,
    /// flat outside.
    pub points: Vec<(f64, f64)>,
    /// Inner (session, candidate) pairs the map was fitted on.
    pub pairs: u64,
}

impl Calibration {
    /// Bins of width 1/[`CAL_BINS`] for (predicted, outcome) pairs.
    pub fn bins() -> Vec<(f64, f64, f64)> {
        vec![(0.0, 0.0, 0.0); CAL_BINS]
    }

    pub fn add(bins: &mut [(f64, f64, f64)], pred: f64, outcome: bool) {
        let i = ((pred * CAL_BINS as f64) as usize).min(CAL_BINS - 1);
        bins[i].0 += pred;
        bins[i].1 += f64::from(u8::from(outcome));
        bins[i].2 += 1.0;
    }

    /// The cheap fit, for large corpora: each candidate's posterior recomputed on inner folds
    /// from its own opportunity and support lists (candidates mined on every training session,
    /// so outcomes in the inner held-out fold are slightly optimistic; with many sessions the
    /// bias is small).
    pub fn fit(db: &Db, scored: &[Scored], kappa: f64, folds: usize) -> Calibration {
        let k = folds.max(2);
        let fold = inner_folds(&db.sessions, k);
        let mut bins = Calibration::bins();
        let (mut opp, mut hit) = (vec![0u32; k], vec![0u32; k]);
        for sc in scored {
            // One merge walk: per fold, opportunities and those followed by the continuation.
            opp.iter_mut().chain(hit.iter_mut()).for_each(|x| *x = 0);
            let mut j = 0usize;
            for &si in &sc.opp_list {
                while j < sc.sessions.len() && sc.sessions[j] < si {
                    j += 1;
                }
                let f = fold[si as usize];
                opp[f] += 1;
                hit[f] += u32::from(j < sc.sessions.len() && sc.sessions[j] == si);
            }
            let (n_all, s_all): (u32, u32) = (opp.iter().sum(), hit.iter().sum());
            for f in 0..k {
                let n = f64::from(n_all - opp[f]);
                if n == 0.0 || opp[f] == 0 {
                    continue;
                }
                let (a, b) = stats::posterior(f64::from(s_all - hit[f]), n, sc.prior, kappa);
                let pred = a / (a + b);
                let i = ((pred * CAL_BINS as f64) as usize).min(CAL_BINS - 1);
                bins[i].0 += pred * f64::from(opp[f]);
                bins[i].1 += f64::from(hit[f]);
                bins[i].2 += f64::from(opp[f]);
            }
        }
        Calibration::from_bins(&bins)
    }

    /// Isotonic regression (pool adjacent violators) over the non-empty bins.
    pub fn from_bins(bins: &[(f64, f64, f64)]) -> Calibration {
        let mut blocks: Vec<(f64, f64, f64)> = Vec::new();
        for &b in bins.iter().filter(|b| b.2 > 0.0) {
            blocks.push(b);
            while blocks.len() >= 2 {
                let (x, y) = (blocks[blocks.len() - 2], blocks[blocks.len() - 1]);
                if x.1 / x.2 <= y.1 / y.2 {
                    break;
                }
                blocks.pop();
                let last = blocks.len() - 1;
                blocks[last] = (x.0 + y.0, x.1 + y.1, x.2 + y.2);
            }
        }
        Calibration {
            points: blocks.iter().map(|b| (b.0 / b.2, b.1 / b.2)).collect(),
            pairs: bins.iter().map(|b| b.2 as u64).sum(),
        }
    }

    /// The calibrated value of a posterior mean (the identity when nothing was fitted).
    pub fn apply(&self, x: f64) -> f64 {
        let p = &self.points;
        match p.len() {
            0 => x,
            1 => p[0].1,
            _ => {
                if x <= p[0].0 {
                    return p[0].1;
                }
                for w in p.windows(2) {
                    if x <= w[1].0 {
                        let t = if w[1].0 > w[0].0 { (x - w[0].0) / (w[1].0 - w[0].0) } else { 1.0 };
                        return w[0].1 + t * (w[1].1 - w[0].1);
                    }
                }
                p[p.len() - 1].1
            }
        }
    }

    /// Calibrate a scored candidate: the mean through the map, and the lower bound as the same
    /// quantile of a Beta with the calibrated mean and the posterior's concentration.
    pub fn calibrate(&self, sc: &mut Scored, quantile: f64) {
        let m = self.apply(sc.conf).clamp(1e-6, 1.0 - 1e-6);
        let c = sc.alpha + sc.beta;
        sc.conf_cal = m;
        sc.conf_lb_cal = stats::beta_quantile(quantile, m * c, (1.0 - m) * c);
    }
}

/// The next-action model the held-out evaluation uses: the chain chosen per training set among
/// the taxonomy chain over mixed labels (exact acts, head-level paths), the taxonomy chain over
/// exact labels and the exact chain, orders 1–3, κ = 2, the session's own transitions counted
/// twice.
pub fn next_params() -> NextParams {
    NextParams {
        candidates: ["h0,m0,h1,m1,m2", "h0,x0,h1,x1,x2", "x0,x1,x2"].iter().map(|c| parse_chain(c)).collect(),
        session_weight: 2.0,
        ..NextParams::default()
    }
}

/// Evaluate on held-out sessions prepared with the training alphabet: next-action top-1 of the
/// backoff model against the order-1 and order-2 baselines (every predictor sees the same
/// history, alphabet events only, and predicts the exact label of the next alphabet event when
/// it is an act), conformance, oracle ceilings, and the calibration of every candidate.
pub fn holdout(train: &Db, rules: &[Rule], test: &[DbSession]) -> Holdout {
    let base = Order1::train(train);
    let order2 = Order2::train(train);
    let model = NextModel::fit(&train.sessions, &next_params());
    let mut h = Holdout {
        sessions: test.len(),
        chosen: vec![model.chosen],
        bins: bins10(),
        bins_raw: bins10(),
        bins_selected: bins10(),
        ..Default::default()
    };
    for s in test {
        let (mut le, mut lb, mut n) = (0.0, 0.0, 0u32);
        for t in act_positions(s) {
            let truth = s.ev[t + 1].lab[0];
            let prev = s.ev[t].lab[0];
            h.positions += 1;
            h.hits_engine += u64::from(model.predict(s, t).0 == truth);
            h.hits_baseline += u64::from(base.predict(prev) == truth);
            h.hits_order2 += u64::from(order2.predict(s, t) == truth);
            le -= model.prob(s, t, truth).max(1e-12).ln();
            lb -= base.prob(prev, truth).ln();
            n += 1;
        }
        if n > 0 {
            h.nats_engine += le / n as f64;
            h.nats_baseline += lb / n as f64;
            h.scored_sessions += 1;
        }
    }
    let exact = |s: &DbSession, t: usize, k: usize| -> Vec<u32> {
        (0..k).map(|b| if t < b { BOS } else { s.ev[t - b].lab[0] }).collect()
    };
    h.oracle = (1..=3).map(|k| oracle(test, |s, t| exact(s, t, k))).collect();
    // Calibration: each (session, candidate) where the candidate's context occurs.
    for r in rules {
        let cut = r.pattern.cut;
        let mut xs: Vec<Item> = r.pattern.items[..cut].to_vec();
        xs[0] = xs[0].first();
        for s in test {
            if !train.occurs(s, &xs, cut) {
                continue;
            }
            let outcome = train.occurs(s, &r.pattern.items, cut);
            bin_add(&mut h.bins, r.conf_cal, outcome);
            bin_add(&mut h.bins_raw, r.conf, outcome);
            if r.selected {
                bin_add(&mut h.bins_selected, r.conf_cal, outcome);
                h.selected_pairs.push((r.conf_cal, outcome));
            }
        }
    }
    h.finish();
    h
}

/// Whether an item's label is at the exact level.
pub fn is_exact(db: &Db, it: &Item) -> bool {
    db.level(it.label) == Level::Exact
}
