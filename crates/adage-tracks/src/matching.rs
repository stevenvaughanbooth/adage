//! Context state and symbolisation (Track G §7.1–§7.3).
//!
//! A session keeps its recent **alphabet symbols** ([`CtxState`]): the ids, in the repository's
//! context alphabet ([`adage_tracks::context::CtxNet`]), of the context items its recent events
//! satisfied. An event is **symbolised** ([`Symbols`]) into those ids:
//!
//! | Event | Items it can satisfy |
//! | --- | --- |
//! | a successful command (a batch call) | `act`: its canonical key meets one of the item's keys ([`crate::meets`]); looked up by the key's identity (program and subcommand, `pattern::Shape`), exact keys first |
//! | a failed command (a failure event) | `fail`: the same, and the output holds the signature |
//! | an edit or write | `edit`: the repository-relative path matches the glob |
//! | a prompt | `prompt_value`: a prompt token the learned value type accepts; `intent`: the family's test on the prompt's embedding |
//!
//! An event that satisfies no item is not an alphabet event: nothing is pushed, and nothing that
//! a later match reads changes. So the hook's no-match fast path stays exact when `Triggers` holds
//! the words of every item (`adage_tracks::triggers`).
//!
//! **Bounds.** At most [`RING`] symbols are kept, and none older than [`SPAN`] alphabet events
//! (two gaps of [`CONTEXT_MAX_WITHIN`]: the most a three-item context can span); an event
//! carries at most [`PER_EVENT`] symbols.
//!
//! **A failed call reported again.** Claude Code raises a failure hook for a failed call and then
//! lists the same call in the tool batch, not marked failed. The failure records its key
//! (`last_fail`), and the batch that follows does not count that call as a successful command.
//! The record lapses as soon as another alphabet event arrives.

use crate::Call;
use adage_core::pattern::Shape;
use adage_tracks::context::CtxNet;
use adage_tracks::model::{CtxItem, CONTEXT_MAX_WITHIN};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The most symbols a session keeps.
pub const RING: usize = 16;
/// The oldest symbol kept, in alphabet events.
pub const SPAN: u8 = 2 * CONTEXT_MAX_WITHIN;
/// The most symbols one event carries.
pub const PER_EVENT: usize = 8;

/// A session's recent context symbols (Track G §7.1). Bounded: at most [`RING`] entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CtxState {
    /// The alphabet the symbols belong to (`CtxNet::digest`); another alphabet starts over.
    #[serde(default)]
    pub alphabet: u32,
    /// Symbol ids, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ring: Vec<u32>,
    /// For each ring entry, the alphabet events since the one that carried it (0: the newest).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub since: Vec<u8>,
    /// Alphabet events seen in this session (saturating).
    #[serde(default)]
    pub seen: u32,
    /// The latest failed call that is also a command item: `(key hash, seen at the failure)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fail: Option<(u32, u32)>,
}

impl CtxState {
    pub fn is_empty(&self) -> bool {
        *self == CtxState::default()
    }

    /// `(symbol, since)` pairs, for [`CtxNet::walk`].
    pub fn recent(&self) -> Vec<(u32, u8)> {
        self.ring.iter().copied().zip(self.since.iter().copied()).collect()
    }

    /// Start over when the symbols belong to another alphabet.
    fn align(&mut self, net: &CtxNet) {
        if self.alphabet != net.digest {
            *self = CtxState { alphabet: net.digest, ..Default::default() };
        }
    }

    /// Push one alphabet event's symbols (nothing, for an event outside the alphabet). Returns
    /// whether it was an alphabet event.
    pub fn push(&mut self, net: &CtxNet, syms: &[u32]) -> bool {
        if syms.is_empty() {
            return false;
        }
        self.align(net);
        for s in &mut self.since {
            *s = s.saturating_add(1);
        }
        (self.ring, self.since) =
            self.ring.iter().copied().zip(self.since.iter().copied()).filter(|(_, s)| *s <= SPAN).unzip();
        for &s in syms.iter().take(PER_EVENT) {
            self.ring.push(s);
            self.since.push(0);
        }
        if self.ring.len() > RING {
            let extra = self.ring.len() - RING;
            self.ring.drain(..extra);
            self.since.drain(..extra);
        }
        self.seen = self.seen.saturating_add(1);
        true
    }

    /// Record a failed call whose key is a command item (see the module documentation).
    pub fn failed(&mut self, net: &CtxNet, key: &str) {
        self.align(net);
        self.last_fail = Some((key_hash(key), self.seen));
    }

    /// The failure record still in force at the start of a batch, if any.
    pub fn open_failure(&self, net: &CtxNet) -> Option<u32> {
        self.last_fail.filter(|(_, at)| self.alphabet == net.digest && *at == self.seen).map(|(h, _)| h)
    }
}

/// A stable 32-bit hash of a key (FNV-1a).
pub fn key_hash(key: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in key.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// A key's lookup bucket: the identity of the command it runs, or the key itself.
fn bucket(key: &str) -> String {
    match Shape::of_key(key) {
        Some(s) => s.identity.join(" "),
        None => key.to_string(),
    }
}

/// The symboliser of one repository's alphabet (built once per decision, only when the
/// repository has context triggers).
pub struct Symbols<'a> {
    pub net: &'a CtxNet,
    /// Exact key → the command items naming it.
    exact: BTreeMap<&'a str, Vec<u32>>,
    /// Identity → (key, symbol) of the command items with a key of that identity.
    acts: BTreeMap<String, Vec<(&'a str, u32)>>,
    /// Identity → (key, signature, symbol) of the failure items.
    fails: BTreeMap<String, Vec<(&'a str, &'a str, u32)>>,
    edits: Vec<(glob::Pattern, u32)>,
    values: Vec<(&'a adage_core::vtype::Runs, u32)>,
    intents: Vec<(&'a str, &'a str, &'a [i16], f32, u32)>,
}

impl<'a> Symbols<'a> {
    pub fn new(net: &'a CtxNet) -> Symbols<'a> {
        let mut s = Symbols {
            net,
            exact: BTreeMap::new(),
            acts: BTreeMap::new(),
            fails: BTreeMap::new(),
            edits: Vec::new(),
            values: Vec::new(),
            intents: Vec::new(),
        };
        for (i, it) in net.alphabet.iter().enumerate() {
            let id = i as u32;
            match it {
                CtxItem::Act { keys } => {
                    for k in keys {
                        s.exact.entry(k.as_str()).or_default().push(id);
                        s.acts.entry(bucket(k)).or_default().push((k.as_str(), id));
                    }
                }
                CtxItem::Fail { keys, signature } => {
                    for k in keys {
                        s.fails.entry(bucket(k)).or_default().push((k.as_str(), signature.as_str(), id));
                    }
                }
                CtxItem::Edit { glob } => {
                    if let Ok(g) = glob::Pattern::new(glob) {
                        s.edits.push((g, id));
                    }
                }
                CtxItem::PromptValue { vtype } => s.values.push((&vtype.runs, id)),
                CtxItem::Intent { family, model, centroid, threshold } => {
                    s.intents.push((family.as_str(), model.as_str(), centroid.as_slice(), *threshold, id))
                }
            }
        }
        s
    }

    /// The command items a successful call with this canonical key satisfies: the exact key,
    /// then every item key of the same identity it meets.
    pub fn act(&self, key: &str) -> Vec<u32> {
        let mut out: Vec<u32> = self.exact.get(key).cloned().unwrap_or_default();
        if let Some(cands) = self.acts.get(&bucket(key)) {
            for (k, id) in cands {
                if !out.contains(id) && crate::meets(k, key) {
                    out.push(*id);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The failure items a failed call satisfies.
    pub fn fail(&self, key: &str, call: &Call) -> Vec<u32> {
        let mut out = Vec::new();
        if let Some(cands) = self.fails.get(&bucket(key)) {
            for (k, sig, id) in cands {
                if crate::meets(k, key) && call.output.contains(sig) {
                    out.push(*id);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The edit items an edit of this repository-relative path satisfies.
    pub fn edit(&self, rel: &str) -> Vec<u32> {
        self.edits.iter().filter(|(g, _)| g.matches(rel)).map(|(_, id)| *id).collect()
    }

    /// Whether any prompt item exists (a prompt is then symbolised).
    pub fn has_prompt_items(&self) -> bool {
        !self.values.is_empty() || !self.intents.is_empty()
    }

    /// The prompt items a prompt satisfies. `embed` is called at most once, and only when there
    /// is an intent item.
    pub fn prompt(&self, text: &str, embed: impl FnOnce() -> Option<(String, Vec<f32>)>) -> Vec<u32> {
        let mut out = Vec::new();
        if !self.values.is_empty() {
            let toks = adage_core::tokens::tokens(text, adage_core::tokens::PROMPT_TOKENS);
            for (runs, id) in &self.values {
                if toks.iter().any(|t| adage_core::vtype::matches(runs, &t.text)) {
                    out.push(*id);
                }
            }
        }
        if !self.intents.is_empty() {
            if let Some((model, v)) = embed() {
                for (_, m, centroid, threshold, id) in &self.intents {
                    if *m == model
                        && centroid.len() == v.len()
                        && adage_intent::cosine(&v, &adage_intent::dequantize(centroid)) >= *threshold
                    {
                        out.push(*id);
                    }
                }
            }
        }
        out.sort_unstable();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(items: Vec<CtxItem>) -> CtxNet {
        CtxNet { digest: 7, alphabet: items, nodes: vec![Default::default()] }
    }

    #[test]
    fn events_are_symbolised_by_identity_then_meets() {
        let n = net(vec![
            CtxItem::Act { keys: vec!["exec make gen".into()] },
            CtxItem::Act { keys: vec!["exec make check".into(), "exec make gen".into()] },
            CtxItem::Fail { keys: vec!["exec make gen".into()], signature: "out of date".into() },
            CtxItem::Edit { glob: "registry/*.csv".into() },
        ]);
        let s = Symbols::new(&n);
        assert_eq!(s.act("exec make gen"), [0, 1]);
        assert_eq!(s.act("exec make gen -j4"), [0, 1], "added options still meet");
        assert_eq!(s.act("exec make check"), [1]);
        assert!(s.act("exec make test").is_empty());
        assert!(s.act("exec cargo test").is_empty());
        let call =
            |out: &str| Call { tool: "Bash".into(), input: Default::default(), output: out.into(), failed: true };
        assert_eq!(s.fail("exec make gen", &call("error: tables out of date")), [2]);
        assert!(s.fail("exec make gen", &call("boom")).is_empty());
        assert_eq!(s.edit("registry/rates.csv"), [3]);
        assert!(s.edit("docs/rates.csv").is_empty());
    }

    #[test]
    fn the_ring_is_bounded_and_ages_by_alphabet_events() {
        let n = net(vec![CtxItem::Edit { glob: "*".into() }]);
        let mut st = CtxState::default();
        assert!(!st.push(&n, &[]), "not an alphabet event");
        assert!(st.is_empty());
        assert!(st.push(&n, &[0]));
        assert_eq!((st.alphabet, st.recent()), (7, vec![(0, 0)]));
        for _ in 0..SPAN {
            st.push(&n, &[1, 2]);
        }
        assert_eq!(st.ring.len(), RING);
        assert!(st.since.iter().all(|s| *s < SPAN));
        let many: Vec<u32> = (0..20).collect();
        st.push(&n, &many);
        assert_eq!(st.recent().iter().filter(|(_, s)| *s == 0).count(), PER_EVENT);
        // Another alphabet: start over.
        let other = CtxNet { digest: 9, ..n.clone() };
        st.push(&other, &[0]);
        assert_eq!((st.alphabet, st.ring.len(), st.seen), (9, 1, 1));
        // A failure record lapses with the next alphabet event.
        st.failed(&other, "exec make gen");
        assert_eq!(st.open_failure(&other), Some(key_hash("exec make gen")));
        st.push(&other, &[0]);
        assert_eq!(st.open_failure(&other), None);
    }
}
