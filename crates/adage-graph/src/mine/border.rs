//! Incremental mining with a negative border (Track G §9, IncSpan style).
//!
//! A full mine stores every candidate it counted with support ≥ `s_min − border`: the frequent
//! patterns (kept by the taxonomy climb or not) and the **negative border** below them, each
//! with its supporting session ids, its parent pattern and the extension that grew it.
//!
//! A new session is matched against every stored pattern (the exact matcher,
//! [`super::Db::occurs`]) and the counts are updated; a removed session is subtracted. Then the
//! stored decisions are re-derived: frequent (≥ s_min) and kept (the climb among siblings). The
//! result equals a full re-mine as long as:
//!
//! - s_min, the alphabet and the target set are unchanged (checked);
//! - no stored node changed between expanded and not (a border pattern crossing s_min, or a
//!   climb decision flipping, checked);
//! - fewer than `border + 1` sessions were **added** since the last full mine: a candidate that
//!   was not stored had support below `s_min − border`, and each added session raises a support
//!   by at most one, so none can have become frequent (removals only lower supports);
//! - the full mine was not truncated by the node budget.
//!
//! Otherwise it re-mines in full. `tests/border.rs` checks the equality over random arrival
//! orders. Sessions must be labelled with one fixed label table (a frozen epoch).

use super::closed;
use super::seq::{self, Ext};
use super::{Db, MineParams, MinedPattern, Pattern};
use crate::events::{LSession, LabelTable};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A mining result in terms of session ids: comparable across runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub s_min: u32,
    /// (pattern, supporting session ids, closed), sorted by pattern.
    pub patterns: Vec<(Pattern, Vec<String>, bool)>,
}

/// A full mine of `sessions`, as an [`Output`].
pub fn full(labels: &LabelTable, sessions: &[&LSession], p: &MineParams) -> Output {
    let db = Db::build(labels, sessions, p);
    let m = super::mine(&db, p);
    Output {
        s_min: m.s_min,
        patterns: m
            .patterns
            .into_iter()
            .map(|mp| {
                let ids = mp.sessions.iter().map(|&i| db.sessions[i as usize].id.clone()).collect();
                (mp.pattern, ids, mp.closed)
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Stored {
    pattern: Pattern,
    parent: Pattern,
    ext: Ext,
    target: u32,
    sessions: BTreeSet<String>,
    /// Frequent and kept at the last evaluation.
    expanded: bool,
}

/// Incremental mining state.
#[derive(Debug, Clone)]
pub struct Incremental {
    p: MineParams,
    labels: LabelTable,
    sessions: BTreeMap<String, LSession>,
    db: Db,
    targets: Vec<(u32, u32)>,
    stored: Vec<Stored>,
    headroom: u32,
    truncated: bool,
    /// Full mines run (the first included).
    pub full_mines: u32,
    /// Updates applied without a full mine.
    pub incremental: u32,
    /// Why each full mine after the first ran.
    pub reasons: BTreeMap<String, u32>,
}

impl Incremental {
    pub fn new(labels: &LabelTable, sessions: Vec<LSession>, p: &MineParams) -> Incremental {
        let db = Db::build(labels, &[], p);
        let mut inc = Incremental {
            p: p.clone(),
            labels: labels.clone(),
            sessions: sessions.into_iter().map(|s| (s.id.clone(), s)).collect(),
            db,
            targets: vec![],
            stored: vec![],
            headroom: 0,
            truncated: false,
            full_mines: 0,
            incremental: 0,
            reasons: BTreeMap::new(),
        };
        inc.remine();
        inc
    }

    fn build_db(&self) -> Db {
        let all: Vec<&LSession> = self.sessions.values().collect();
        Db::build(&self.labels, &all, &self.p)
    }

    fn why(&self, db: &Db, adding: bool) -> &'static str {
        if self.truncated {
            "truncated"
        } else if adding && self.headroom == 0 {
            "border exhausted"
        } else if db.s_min != self.db.s_min {
            "s_min"
        } else if db.alphabet != self.db.alphabet {
            "alphabet"
        } else if !self.same_frame(db) {
            "targets"
        } else {
            ""
        }
    }

    fn remine_for(&mut self, reason: &str) {
        *self.reasons.entry(reason.to_string()).or_default() += 1;
        self.remine();
    }

    fn remine(&mut self) {
        self.db = self.build_db();
        self.targets = seq::targets(&self.db);
        let run = seq::mine(&self.db, &self.p, true);
        self.truncated = run.truncated;
        let ids: Vec<String> = self.db.sessions.iter().map(|s| s.id.clone()).collect();
        self.stored = run
            .found
            .into_iter()
            .map(|f| Stored {
                expanded: f.frequent && f.kept,
                pattern: f.pattern,
                parent: f.parent,
                ext: f.ext,
                target: f.target,
                sessions: f.sessions.iter().map(|&i| ids[i as usize].clone()).collect(),
            })
            .collect();
        self.headroom = self.p.border;
        self.full_mines += 1;
    }

    /// Whether the structure a full mine depends on is unchanged.
    fn same_frame(&self, db: &Db) -> bool {
        // The target set (not the supports, which move with every session; their order matters
        // only under a truncated budget, which always re-mines).
        let set = |t: &[(u32, u32)]| t.iter().map(|x| x.0).collect::<std::collections::BTreeSet<u32>>();
        db.s_min == self.db.s_min && db.alphabet == self.db.alphabet && set(&seq::targets(db)) == set(&self.targets)
    }

    /// Re-derive frequent/kept; `false` when any expansion decision changed.
    fn decisions_hold(&self, s_min: u32) -> bool {
        let mut groups: BTreeMap<(u32, &Pattern), Vec<usize>> = BTreeMap::new();
        for (i, s) in self.stored.iter().enumerate() {
            groups.entry((s.target, &s.parent)).or_default().push(i);
        }
        for idx in groups.values() {
            let maxref = seq::refinements(
                &self.db,
                idx.iter().map(|&i| (self.stored[i].ext, self.stored[i].sessions.len() as u32)),
            );
            // Frequent and kept, then the beam in (support desc, extension) order.
            let mut ok: Vec<(u32, Ext, usize)> = idx
                .iter()
                .filter_map(|&i| {
                    let s = &self.stored[i];
                    let c = s.sessions.len() as u32;
                    (c >= s_min && c > maxref.get(&s.ext).copied().unwrap_or(0)).then_some((c, s.ext, i))
                })
                .collect();
            ok.sort_by(|a, b| seq::beam_cmp(&self.db, (a.0, &a.1), (b.0, &b.1)));
            let expanded: std::collections::BTreeSet<usize> =
                ok.iter().take(self.p.max_children).map(|x| x.2).collect();
            for &i in idx {
                if expanded.contains(&i) != self.stored[i].expanded {
                    return false;
                }
            }
        }
        true
    }

    /// Add (or replace) a session. Returns true when it ran a full mine.
    pub fn add(&mut self, s: LSession) -> bool {
        if self.sessions.contains_key(&s.id) {
            self.remove(&s.id.clone());
        }
        let id = s.id.clone();
        self.sessions.insert(id.clone(), s);
        let db = self.build_db();
        let why = self.why(&db, true);
        if !why.is_empty() {
            self.remine_for(why);
            return true;
        }
        let ds = db.prepare(&self.sessions[&id]);
        self.db = db;
        for st in self.stored.iter_mut() {
            if self.db.occurs(&ds, &st.pattern.items, st.pattern.cut) {
                st.sessions.insert(id.clone());
            }
        }
        self.headroom -= 1;
        if !self.decisions_hold(self.db.s_min) {
            self.remine_for("decision flipped");
            return true;
        }
        self.incremental += 1;
        false
    }

    /// Remove a session. Returns true when it ran a full mine.
    pub fn remove(&mut self, id: &str) -> bool {
        if self.sessions.remove(id).is_none() {
            return false;
        }
        let db = self.build_db();
        let why = self.why(&db, false);
        if !why.is_empty() {
            self.remine_for(why);
            return true;
        }
        self.db = db;
        for st in self.stored.iter_mut() {
            st.sessions.remove(id);
        }
        if !self.decisions_hold(self.db.s_min) {
            self.remine_for("decision flipped");
            return true;
        }
        self.incremental += 1;
        false
    }

    /// The current result.
    pub fn output(&self) -> Output {
        let index: BTreeMap<&str, u32> =
            self.sessions.keys().enumerate().map(|(i, k)| (k.as_str(), i as u32)).collect();
        let mut pats: Vec<MinedPattern> = self
            .stored
            .iter()
            .filter(|s| s.expanded)
            .map(|s| MinedPattern {
                pattern: s.pattern.clone(),
                sessions: s.sessions.iter().map(|k| index[k.as_str()]).collect(),
                closed: false,
            })
            .collect();
        pats.sort_by(|a, b| a.pattern.cmp(&b.pattern));
        pats.dedup_by(|a, b| a.pattern == b.pattern);
        closed::mark(&self.db, &mut pats);
        let ids: Vec<&String> = self.sessions.keys().collect();
        Output {
            s_min: self.db.s_min,
            patterns: pats
                .into_iter()
                .map(|m| (m.pattern, m.sessions.iter().map(|&i| ids[i as usize].clone()).collect(), m.closed))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}
