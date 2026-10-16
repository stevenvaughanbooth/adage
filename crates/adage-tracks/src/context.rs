//! Context triggers at hook time (Track G §7.2): per repository, the **alphabet** of context
//! items and a **discrimination net** over them, compiled into the index with the routes.
//!
//! - **Alphabet.** Every distinct [`CtxItem`] of the repository's context triggers, interned: a
//!   symbol id is the item's rank in the sorted canonical (JSON) forms, so the same routes always
//!   give the same ids. An event is *symbolised* by the runtime into the ids of the items it
//!   satisfies (`adage_runtime::context`); an event that satisfies none is not an alphabet event,
//!   and nothing about it is kept.
//! - **Net.** A trie over reversed item sequences: the root's children are the items a context
//!   *ends* with, their children the item before, and so on, at most [`CONTEXT_MAX_ITEMS`] deep.
//!   Each node lists the routes whose context is exactly the path to it, with their leaf guard
//!   (`within`: the most alphabet events allowed between consecutive items). The runtime walks it
//!   from the newest event's symbols over its recent symbols ([`CtxNet::walk`]).
//!
//! Matching is a **subsequence within a gap over alphabet events only** (§7.3): item *i* is an
//! earlier alphabet event than item *i + 1*, at most `within` alphabet events before it. So an
//! event outside the alphabet can never change a match, which is what keeps the hook's no-match
//! fast path exact (`Triggers` holds the words of every item).
//!
//! Derived from the routes, like `Triggers`, so not part of an index's generation.

use crate::model::{CtxItem, Route, Trigger, CONTEXT_MAX_ITEMS, CONTEXT_MAX_WITHIN};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// One node of the net.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DNode {
    /// `(symbol, child node)`, sorted by symbol.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<(u32, u32)>,
    /// `(route id, within)`: the routes whose context is the path to this node, read backwards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<(String, u8)>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CtxNet {
    /// Which alphabet the symbol ids belong to: a digest of `alphabet`. A session state that
    /// recorded symbols of another alphabet (the index was relearned mid-session) starts over.
    pub digest: u32,
    /// The alphabet: symbol id → item.
    pub alphabet: Vec<CtxItem>,
    /// The net; node 0 is the root.
    pub nodes: Vec<DNode>,
}

impl CtxNet {
    /// The alphabet and net of these routes' context triggers (other triggers are ignored).
    pub fn of<'a>(routes: impl IntoIterator<Item = &'a Route>) -> CtxNet {
        CtxNet::build(routes.into_iter().map(|r| (r.id.as_str(), &r.trigger)), b"")
    }

    /// The alphabet and net of a repository's candidates (`crate::candidates`), which the runtime
    /// walks apart from the routes' net: candidates change at nearly every learn, and a changed
    /// digest resets a session's context state, so they must never share the routes' alphabet.
    /// Its digest is salted, so it differs from a routes' net over the same items.
    pub fn of_candidates<'a>(candidates: impl IntoIterator<Item = &'a crate::candidates::Candidate>) -> CtxNet {
        CtxNet::build(candidates.into_iter().map(|c| (c.id.as_str(), &c.trigger)), b"candidates\0")
    }

    fn build<'a>(triggers: impl Iterator<Item = (&'a str, &'a Trigger)>, salt: &[u8]) -> CtxNet {
        let contexts: Vec<(&str, &[CtxItem], u8)> = triggers
            .filter_map(|(id, t)| match t {
                Trigger::Context { items, within } if !items.is_empty() => {
                    Some((id, items.as_slice(), (*within).min(CONTEXT_MAX_WITHIN)))
                }
                _ => None,
            })
            .collect();
        if contexts.is_empty() {
            return CtxNet::default();
        }
        let canon = |it: &CtxItem| serde_json::to_string(it).unwrap_or_default();
        let forms: BTreeMap<String, &CtxItem> =
            contexts.iter().flat_map(|(_, items, _)| items.iter()).map(|it| (canon(it), it)).collect();
        let ids: BTreeMap<&str, u32> = forms.keys().enumerate().map(|(i, k)| (k.as_str(), i as u32)).collect();
        let alphabet: Vec<CtxItem> = forms.values().map(|it| (*it).clone()).collect();
        let mut h = Sha256::new();
        h.update(salt);
        for k in forms.keys() {
            h.update(k.as_bytes());
            h.update([0u8]);
        }
        let d = h.finalize();
        let digest = u32::from_le_bytes([d[0], d[1], d[2], d[3]]).max(1);
        let mut net = CtxNet { digest, alphabet, nodes: vec![DNode::default()] };
        for (id, items, within) in contexts {
            let mut node = 0u32;
            for it in items.iter().rev().take(CONTEXT_MAX_ITEMS) {
                let sym = ids[canon(it).as_str()];
                node = match net.child(node, sym) {
                    Some(n) => n,
                    None => {
                        let n = net.nodes.len() as u32;
                        net.nodes.push(DNode::default());
                        let next = &mut net.nodes[node as usize].next;
                        let at = next.partition_point(|(s, _)| *s < sym);
                        next.insert(at, (sym, n));
                        n
                    }
                };
            }
            net.nodes[node as usize].routes.push((id.to_string(), within));
        }
        for n in &mut net.nodes {
            n.routes.sort();
            n.routes.dedup();
        }
        net
    }

    pub fn is_empty(&self) -> bool {
        self.alphabet.is_empty()
    }

    /// The child of `node` on `sym`.
    pub fn child(&self, node: u32, sym: u32) -> Option<u32> {
        let next = &self.nodes.get(node as usize)?.next;
        next.binary_search_by_key(&sym, |(s, _)| *s).ok().map(|i| next[i].1)
    }

    /// The routes whose context ends with the newest alphabet event. `recent` holds the session's
    /// recent symbols as `(symbol, since)`, where `since` counts the alphabet events after the
    /// one that carried the symbol (0: the newest event; one event may carry several symbols).
    /// The walk starts at each symbol of the newest event and descends through strictly older
    /// events, each at most [`CONTEXT_MAX_WITHIN`] events before the last, at most
    /// [`CONTEXT_MAX_ITEMS`] deep. A route fires when some path to its node kept every gap within
    /// its own `within`. Ids come back sorted, once each.
    pub fn walk(&self, recent: &[(u32, u8)]) -> BTreeSet<&str> {
        let mut out = BTreeSet::new();
        if self.is_empty() {
            return out;
        }
        for &(sym, since) in recent {
            if since == 0 {
                if let Some(n) = self.child(0, sym) {
                    self.descend(n, 0, 0, 1, recent, &mut out);
                }
            }
        }
        out
    }

    fn descend<'a>(
        &'a self,
        node: u32,
        at: u8,
        gap: u8,
        depth: usize,
        recent: &[(u32, u8)],
        out: &mut BTreeSet<&'a str>,
    ) {
        if depth >= 2 {
            for (id, within) in &self.nodes[node as usize].routes {
                if gap <= *within {
                    out.insert(id.as_str());
                }
            }
        }
        if depth >= CONTEXT_MAX_ITEMS {
            return;
        }
        for &(sym, since) in recent {
            if since <= at || since - at > CONTEXT_MAX_WITHIN {
                continue;
            }
            if let Some(n) = self.child(node, sym) {
                self.descend(n, since, gap.max(since - at), depth + 1, recent, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use adage_core::Effect;

    fn route(id: &str, items: Vec<CtxItem>, within: u8) -> Route {
        Route {
            id: id.into(),
            summary: String::new(),
            trigger: Trigger::Context { items, within },
            step: Step::Run { command: "make check".into() },
            slots: vec![],
            effect: Effect::Verify,
            strength: Strength::Prime,
            state: RouteState::Firm,
            exit: vec![],
            evidence: Evidence::default(),
            authored: false,
        }
    }

    fn act(k: &str) -> CtxItem {
        CtxItem::Act { keys: vec![k.into()] }
    }

    fn edit(g: &str) -> CtxItem {
        CtxItem::Edit { glob: g.into() }
    }

    #[test]
    fn interning_is_by_content_and_the_walk_is_a_gapped_subsequence() {
        let rs = [
            route("seq", vec![edit("registry/*.csv"), act("exec make gen")], 4),
            route("three", vec![act("exec make deps"), edit("registry/*.csv"), act("exec make gen")], 8),
        ];
        let net = CtxNet::of(rs.iter());
        assert_eq!(net.alphabet.len(), 3, "a shared item is one symbol");
        assert_eq!(net, CtxNet::of(rs.iter().rev()), "the order of the routes does not matter");
        let sym = |it: &CtxItem| net.alphabet.iter().position(|x| x == it).unwrap() as u32;
        let (e, g, d) = (sym(&edit("registry/*.csv")), sym(&act("exec make gen")), sym(&act("exec make deps")));
        // The edit 3 events back, make gen now.
        assert_eq!(net.walk(&[(e, 3), (g, 0)]).into_iter().collect::<Vec<_>>(), ["seq"]);
        // Too far back for `seq` (within 4), not for `three`'s own gap.
        assert!(net.walk(&[(e, 5), (g, 0)]).is_empty());
        assert_eq!(net.walk(&[(d, 7), (e, 5), (g, 0)]).into_iter().collect::<Vec<_>>(), ["three"]);
        // The items must be in order, on different events, the last one the newest.
        assert!(net.walk(&[(g, 3), (e, 0)]).is_empty());
        assert!(net.walk(&[(e, 0), (g, 0)]).is_empty());
        assert!(net.walk(&[(e, 2), (g, 1)]).is_empty(), "nothing fires on an older event");
        assert!(CtxNet::of(std::iter::empty()).walk(&[(0, 0)]).is_empty());
    }
}
