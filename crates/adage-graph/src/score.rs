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
    pub positions: u64,
    /// Alphabet events per node type.
    pub by_type: BTreeMap<NodeType, u64>,
    pub days: u32,
    pub runs: Vec<u32>,
    pub fails: Vec<u32>,
    /// Sessions with any exposed event.
    pub exposed: Vec<u32>,
}

/// Per-thread memo of the background counts, keyed by (anchor item, continuation).
pub type Cache = HashMap<(Item, Vec<Item>), (u64, u64)>;

impl Stats {
    pub fn new(db: &Db, p: &ScoreParams) -> Stats {
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
        let (mut runs, mut fails) = (vec![0u32; n], vec![0u32; n]);
        let mut positions = 0u64;
        let mut by_type: BTreeMap<NodeType, u64> = BTreeMap::new();
        for (si, s) in db.sessions.iter().enumerate() {
            positions += s.len() as u64;
            for e in &s.ev {
                *by_type.entry(e.ty).or_default() += 1;
                for &l in e.lab.iter() {
                    if l == NONE {
                        continue;
                    }
                    let pl = &mut postings[l as usize];
                    if pl.last() != Some(&(si as u32)) {
                        pl.push(si as u32);
                    }
                    runs[l as usize] += 1;
                    if e.ty == NodeType::Act && !e.ok {
                        fails[l as usize] += 1;
                    }
                }
            }
        }
        let days: BTreeSet<u32> = db.sessions.iter().map(|s| s.day).collect();
        let exposed = (0..db.sessions.len() as u32).filter(|&i| db.sessions[i as usize].exposed.iter().any(|&x| x)).collect();
        Stats {
            exposed,
            total_weight: weight.iter().sum(),
            weight,
            postings,
            positions,
            by_type,
            days: days.len() as u32,
            runs,
            fails,
        }
    }

    pub fn positions_of(&self, ty: NodeType) -> u64 {
        self.by_type.get(&ty).copied().unwrap_or(0)
    }

    /// Candidate sessions for `items`: the shortest posting list among them.
    fn candidates(&self, items: &[Item]) -> &[u32] {
        items.iter().map(|i| self.postings[i.label as usize].as_slice()).min_by_key(|v| v.len()).unwrap_or(&[])
    }
}

/// Sessions (weighted) where `items` occur, and where `items ++ ys` occur.
fn opp_succ(db: &Db, st: &Stats, xs: &[Item], ys: &[Item]) -> (f64, f64, u32) {
    let full: Vec<Item> = xs.iter().chain(ys.iter()).copied().collect();
    let (mut n, mut s, mut c) = (0.0, 0.0, 0u32);
    for &si in st.candidates(xs) {
        let sess = &db.sessions[si as usize];
        if db.occurs(sess, xs, xs.len()) {
            n += st.weight[si as usize];
            c += 1;
            if db.occurs(sess, &full, xs.len()) {
                s += st.weight[si as usize];
            }
        }
    }
    (n, s, c)
}

/// Score one pattern.
pub fn score(db: &Db, st: &Stats, mp: &MinedPattern, p: &ScoreParams, cache: &mut Cache) -> Scored {
    let pat = &mp.pattern;
    let cut = pat.cut.max(1).min(pat.items.len() - 1);
    let xs = &pat.items[..cut];
    let ys = &pat.items[cut..];
    let y_alone: Vec<Item> = std::iter::once(ys[0].first()).chain(ys[1..].iter().copied()).collect();
    // Base rate of Y, then the backoff chain over context suffixes.
    let mut prior = {
        let mut w = 0.0;
        for &si in st.candidates(&y_alone) {
            if db.occurs(&db.sessions[si as usize], &y_alone, 0) {
                w += st.weight[si as usize];
            }
        }
        if st.total_weight > 0.0 {
            w / st.total_weight
        } else {
            0.0
        }
    };
    let mut last = (0.0, 0.0, 0u32);
    for k in 1..=cut {
        let mut xk: Vec<Item> = xs[cut - k..].to_vec();
        xk[0] = xk[0].first();
        let (n, s, c) = opp_succ(db, st, &xk, ys);
        last = (n, s, c);
        if k < cut {
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
    let mut opp_list: Vec<u32> = Vec::new();
    let mut variants: BTreeMap<u32, u32> = BTreeMap::new();
    let first_x: Vec<Item> = {
        let mut v = xs.to_vec();
        v[0] = v[0].first();
        v
    };
    for &si in st.candidates(&first_x) {
        let sess = &db.sessions[si as usize];
        let r = db.reach(sess, &first_x, cut, None);
        let ends = r.last().cloned().unwrap_or_default();
        if !ends.is_empty() {
            opp_list.push(si);
        }
        for &q in &ends {
            x_ends += 1;
            if let Some((gap, y0)) = follows_at(db, sess, q as usize, &x_last, ys) {
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
    let (bg_any, bg_same) = *cache.entry((x_last, ys.to_vec())).or_insert_with(|| background(db, st, &x_last, ys));
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
    let supp_set: BTreeSet<u32> = mp.sessions.iter().copied().collect();
    for &si in &st.exposed {
        if !supp_set.contains(&si) && db.occurs(&db.sessions[si as usize], &pat.items, pat.items.len()) {
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

/// Alphabet events followed by `ys` as if they were `x_last`: (any type, `x_last`'s type).
fn background(db: &Db, st: &Stats, x_last: &Item, ys: &[Item]) -> (u64, u64) {
    let y0 = ys[0];
    let ty = db.ty(x_last.label);
    let (mut bg_follow, mut bg_same) = (0u64, 0u64);
    let g = db.gap as usize;
    let mut cand: Vec<usize> = Vec::new();
    for &si in &st.postings[y0.label as usize] {
        let sess = &db.sessions[si as usize];
        cand.clear();
        for q in 0..sess.len() {
            if db.node_ok(sess, q, &y0, true) {
                cand.extend(q.saturating_sub(g)..=(q + g).min(sess.len() - 1));
            }
        }
        cand.sort_unstable();
        cand.dedup();
        for &a in &cand {
            if follows_at(db, sess, a, x_last, ys).is_some() {
                bg_follow += 1;
                bg_same += u64::from(sess.ev[a].ty == ty);
            }
        }
    }
    (bg_follow, bg_same)
}

/// `Db::follows`, also returning the first continuation event.
fn follows_at(db: &Db, s: &DbSession, p: usize, prev: &Item, ys: &[Item]) -> Option<(u32, usize)> {
    let g = db.gap as usize;
    let n = s.len();
    let y0 = ys.first()?;
    let mut best: Option<(u32, usize)> = None;
    for q in p.saturating_sub(g)..=(p + g).min(n.saturating_sub(1)) {
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
pub fn score_all(db: &Db, closed: &[&MinedPattern], p: &ScoreParams) -> Vec<Scored> {
    let st = Stats::new(db, p);
    let threads = p.threads.max(1).min(closed.len().max(1));
    if threads <= 1 {
        let mut cache = Cache::new();
        return closed.iter().map(|m| score(db, &st, m, p, &mut cache)).collect();
    }
    let mut out: Vec<Option<Scored>> = vec![None; closed.len()];
    std::thread::scope(|scope| {
        let hs: Vec<_> = (0..threads)
            .map(|w| {
                let st = &st;
                scope.spawn(move || {
                    let mut cache = Cache::new();
                    (w..closed.len())
                        .step_by(threads)
                        .map(|i| (i, score(db, st, closed[i], p, &mut cache)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in hs {
            for (i, s) in h.join().expect("scoring thread") {
                out[i] = Some(s);
            }
        }
    });
    out.into_iter().map(|x| x.expect("scored")).collect()
}

/// A reflex as the predictor sees it.
#[derive(Debug, Clone)]
pub struct Rule {
    pub pattern: Pattern,
    pub conf: f64,
    pub conf_lb: f64,
    /// P(the next alphabet event is the target | the context just ended).
    pub p_next: f64,
    /// The directed act (dominant exact label of the target), when there is one.
    pub variant: Option<u32>,
}

/// Held-out evaluation (Track G §5 conformance, Design §8.5).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Holdout {
    pub sessions: usize,
    /// Positions whose next alphabet event is an act.
    pub positions: u64,
    pub top1_engine: f64,
    pub top1_baseline: f64,
    /// Positions where a reflex fired, and the two accuracies there.
    pub fired: u64,
    pub top1_engine_fired: f64,
    pub top1_baseline_fired: f64,
    /// Mean over sessions of the mean −ln p(next act).
    pub conformance_engine: f64,
    pub conformance_baseline: f64,
    /// Calibration of `conf` against held-out outcomes (session level, 10 bins).
    pub ece: f64,
    pub ece_pairs: u64,
    /// Raw counts, so held-out folds pool exactly ([`Holdout::merge`]).
    pub hits_engine: u64,
    pub hits_baseline: u64,
    pub hits_engine_fired: u64,
    pub hits_baseline_fired: u64,
    pub nats_engine: f64,
    pub nats_baseline: f64,
    pub scored_sessions: u32,
    /// (sum of predicted, sum of observed, count) per bin of width 0.1.
    pub bins: Vec<(f64, f64, u64)>,
}

impl Holdout {
    fn finish(&mut self) {
        let r = |a: u64, b: u64| if b == 0 { 0.0 } else { a as f64 / b as f64 };
        self.top1_engine = r(self.hits_engine, self.positions);
        self.top1_baseline = r(self.hits_baseline, self.positions);
        self.top1_engine_fired = r(self.hits_engine_fired, self.fired);
        self.top1_baseline_fired = r(self.hits_baseline_fired, self.fired);
        if self.scored_sessions > 0 {
            self.conformance_engine = self.nats_engine / self.scored_sessions as f64;
            self.conformance_baseline = self.nats_baseline / self.scored_sessions as f64;
        }
        self.ece_pairs = self.bins.iter().map(|b| b.2).sum();
        self.ece = 0.0;
        for b in &self.bins {
            if b.2 > 0 {
                let (mp, mo) = (b.0 / b.2 as f64, b.1 / b.2 as f64);
                self.ece += (b.2 as f64 / self.ece_pairs as f64) * (mp - mo).abs();
            }
        }
    }

    /// Pool held-out folds.
    pub fn merge(parts: &[Holdout]) -> Holdout {
        let mut h = Holdout { bins: vec![(0.0, 0.0, 0); 10], ..Default::default() };
        for p in parts {
            h.sessions += p.sessions;
            h.positions += p.positions;
            h.fired += p.fired;
            h.hits_engine += p.hits_engine;
            h.hits_baseline += p.hits_baseline;
            h.hits_engine_fired += p.hits_engine_fired;
            h.hits_baseline_fired += p.hits_baseline_fired;
            h.nats_engine += p.nats_engine;
            h.nats_baseline += p.nats_baseline;
            h.scored_sessions += p.scored_sessions;
            for (a, b) in h.bins.iter_mut().zip(&p.bins) {
                a.0 += b.0;
                a.1 += b.1;
                a.2 += b.2;
            }
        }
        h.finish();
        h
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

/// Evaluate rules on held-out sessions prepared with the training alphabet.
pub fn holdout(train: &Db, rules: &[Rule], test: &[DbSession]) -> Holdout {
    let base = Order1::train(train);
    // Predictive rules: an act continuation with a variant. `last` is dropped from contexts
    // (it looks ahead in the session).
    let g = train.gap as usize;
    let pred: Vec<(Vec<Item>, u32, f64)> = rules
        .iter()
        .filter_map(|r| {
            let v = r.variant?;
            let y0 = r.pattern.items[r.pattern.cut];
            (train.ty(y0.label) == NodeType::Act).then(|| {
                let mut xs: Vec<Item> = r.pattern.items[..r.pattern.cut].to_vec();
                for x in xs.iter_mut() {
                    x.last = false;
                }
                xs[0] = xs[0].first();
                (xs, v, r.conf_lb)
            })
        })
        .collect();
    // Each rule's hazard on the training sessions: P(the next event is its act | its context
    // ended k events ago and the act has not happened since), k = 1..=G.
    let hazard: Vec<Vec<f64>> = pred
        .iter()
        .map(|(xs, v, _)| {
            let (mut hit, mut risk) = (vec![0f64; g + 1], vec![0f64; g + 1]);
            for s in &train.sessions {
                for &p in &train.reach(s, xs, xs.len(), None).pop().unwrap_or_default() {
                    let p = p as usize;
                    for k in 1..=g {
                        let q = p + k;
                        if q >= s.len() || s.ev[q].seg != s.ev[p].seg {
                            break;
                        }
                        risk[k] += 1.0;
                        if s.ev[q].lab[0] == *v {
                            hit[k] += 1.0;
                            break;
                        }
                    }
                }
            }
            // Shrunk towards 0 by 5 pseudo-trials: a rule speaks only with evidence.
            (0..=g).map(|k| hit[k] / (risk[k] + 5.0)).collect()
        })
        .collect();
    let mut h = Holdout { sessions: test.len(), bins: vec![(0.0, 0.0, 0); 10], ..Default::default() };
    for s in test {
        // X end positions per predictive rule.
        let ends: Vec<Vec<u32>> =
            pred.iter().map(|(xs, _, _)| train.reach(s, xs, xs.len(), None).pop().unwrap_or_default()).collect();
        let (mut le, mut lb, mut n) = (0.0, 0.0, 0u32);
        for t in 0..s.len().saturating_sub(1) {
            let next = &s.ev[t + 1];
            if next.ty != NodeType::Act {
                continue;
            }
            let truth = next.lab[0];
            let prev = s.ev[t].lab[0];
            let bpred = base.predict(prev);
            // A rule is active while its context ended at most G events ago (the latest end) and
            // its act has not happened since; it fires when its hazard beats the backoff's
            // confidence in its own guess. The most confident rule speaks.
            let floor = base.best_share(prev);
            let mut fire: Option<(f64, f64, usize)> = None;
            for (ri, (_, v, lb_)) in pred.iter().enumerate() {
                let Some(&p) = ends[ri].iter().rev().find(|&&p| p as usize <= t) else { continue };
                let p = p as usize;
                let k = t + 1 - p;
                if k > g || s.ev[p].seg != s.ev[t + 1].seg || (p + 1..=t).any(|q| s.ev[q].lab[0] == *v) {
                    continue;
                }
                let hz = hazard[ri][k];
                if hz <= floor {
                    continue;
                }
                if fire.is_none_or(|(b, bl, _)| hz > b || (hz == b && *lb_ > bl)) {
                    fire = Some((hz, *lb_, ri));
                }
            }
            let pb = base.prob(prev, truth);
            let (epred, pe) = match fire {
                Some((hz, _, ri)) => {
                    let v = pred[ri].1;
                    let q = hz.clamp(0.01, 0.99);
                    (v, q * f64::from(u8::from(v == truth)) + (1.0 - q) * pb)
                }
                None => (bpred, pb),
            };
            h.positions += 1;
            h.hits_engine += u64::from(epred == truth);
            h.hits_baseline += u64::from(bpred == truth);
            if fire.is_some() {
                h.fired += 1;
                h.hits_engine_fired += u64::from(epred == truth);
                h.hits_baseline_fired += u64::from(bpred == truth);
            }
            le += -pe.ln();
            lb += -pb.ln();
            n += 1;
        }
        if n > 0 {
            h.nats_engine += le / n as f64;
            h.nats_baseline += lb / n as f64;
            h.scored_sessions += 1;
        }
    }
    // Calibration: each (session, rule) where the context occurs.
    let bins = &mut h.bins;
    for s in test {
        for r in rules {
            let cut = r.pattern.cut;
            let mut xs: Vec<Item> = r.pattern.items[..cut].to_vec();
            xs[0] = xs[0].first();
            if !train.occurs(s, &xs, cut) {
                continue;
            }
            let outcome = train.occurs(s, &r.pattern.items, cut);
            let b = ((r.conf * 10.0) as usize).min(9);
            bins[b].0 += r.conf;
            bins[b].1 += f64::from(u8::from(outcome));
            bins[b].2 += 1;
        }
    }
    h.finish();
    h
}

/// Whether an item's label is at the exact level.
pub fn is_exact(db: &Db, it: &Item) -> bool {
    db.level(it.label) == Level::Exact
}
