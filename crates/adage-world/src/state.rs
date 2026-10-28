//! The shapes of the world's state files (`adage_store::Paths::world_*`): plain data, read and
//! written by the hook, the follow child and the ingest verb through `adage_store::state`. No
//! I/O here.

use adage_trace::{Action, ActivityEvent, Artifact, ArtifactKind, CalendarBlock};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The most artifacts kept; the least recently touched go first.
pub const MAX_ARTIFACTS: usize = 2_000;

/// `world/artifacts.state`: every artifact the timeline names, by id.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ArtifactsState {
    #[serde(default)]
    pub artifacts: BTreeMap<String, Artifact>,
    /// The last action written per artifact, `(action, ts)`, so a source that lists the same
    /// thing every poll writes it once.
    #[serde(default)]
    pub last: BTreeMap<String, (Action, u64)>,
}

/// An event this close to the artifact's last identical one is the same event.
pub const DEDUPE_MS: u64 = 60_000;

impl ArtifactsState {
    /// Add or refresh an artifact: a known id keeps its `first_seen`, its embedding when the new
    /// one has none, and the later `last_touched`.
    pub fn upsert(&mut self, mut a: Artifact) {
        if let Some(old) = self.artifacts.get(&a.id) {
            a.first_seen = old.first_seen.min(a.first_seen);
            a.last_touched = old.last_touched.max(a.last_touched);
            if a.embedding.is_none() {
                a.embedding = old.embedding.clone();
            }
            if a.summary.is_none() {
                a.summary = old.summary.clone();
            }
            if a.related.is_empty() {
                a.related = old.related.clone();
            }
            if a.people.is_empty() {
                a.people = old.people.clone();
            }
        }
        self.artifacts.insert(a.id.clone(), a);
    }

    /// Whether `e` repeats the artifact's last event within [`DEDUPE_MS`].
    pub fn is_duplicate(&self, e: &ActivityEvent) -> bool {
        self.last.get(&e.artifact).is_some_and(|(a, ts)| *a == e.action && e.ts.abs_diff(*ts) < DEDUPE_MS)
    }

    /// Note an event written: the artifact's `last_touched` moves to it.
    pub fn touched(&mut self, e: &ActivityEvent) {
        self.last.insert(e.artifact.clone(), (e.action, e.ts));
        if let Some(a) = self.artifacts.get_mut(&e.artifact) {
            a.last_touched = a.last_touched.max(e.ts);
        }
    }

    /// Drop artifacts not touched for `keep_ms`, and the least recently touched beyond
    /// [`MAX_ARTIFACTS`]; an open pull request or ticket is kept while open.
    pub fn prune(&mut self, now: u64, keep_ms: u64) {
        let since = now.saturating_sub(keep_ms);
        self.artifacts.retain(|_, a| {
            a.last_touched >= since
                || (matches!(a.kind, ArtifactKind::Pr | ArtifactKind::Ticket) && crate::window::is_open(a))
        });
        if self.artifacts.len() > MAX_ARTIFACTS {
            let mut by_age: Vec<(u64, String)> =
                self.artifacts.values().map(|a| (a.last_touched, a.id.clone())).collect();
            by_age.sort();
            for (_, id) in by_age.into_iter().take(self.artifacts.len() - MAX_ARTIFACTS) {
                self.artifacts.remove(&id);
            }
        }
        let ids: Vec<String> = self.last.keys().filter(|k| !self.artifacts.contains_key(*k)).cloned().collect();
        for id in ids {
            self.last.remove(&id);
        }
    }

    /// Every artifact, any order.
    pub fn all(&self) -> Vec<Artifact> {
        self.artifacts.values().cloned().collect()
    }
}

/// `world/calendar.state`: the blocks in the window a source last looked at.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CalendarState {
    /// When the source last looked.
    pub ts: u64,
    #[serde(default)]
    pub blocks: Vec<CalendarBlock>,
}

/// One source's health, as `adage world status` and the mod's Health tab show it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SourceHealth {
    pub name: String,
    /// Where it ran: `mod` (a connected server's name follows), `follow`, or `agent`.
    #[serde(default)]
    pub via: String,
    #[serde(default)]
    pub server: String,
    pub last_poll: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// Events written by this source so far.
    #[serde(default)]
    pub events: u64,
}

/// `world/health.state`: every source's health, by name.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HealthState {
    #[serde(default)]
    pub sources: BTreeMap<String, SourceHealth>,
}

/// One line of `world/eval.jsonl`: a prompt the person labelled with what they meant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalCase {
    pub ts: u64,
    pub prompt: String,
    /// The artifact id the person meant; empty when they meant none (a no-attach case).
    pub meant: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(id: &str, kind: ArtifactKind, touched: u64) -> Artifact {
        Artifact {
            id: id.into(),
            kind,
            title: id.into(),
            locator: id.into(),
            summary: None,
            embedding: None,
            first_seen: touched,
            last_touched: touched,
            related: vec![],
            state: Some("open".into()),
            people: vec![],
        }
    }

    #[test]
    fn upsert_keeps_what_the_new_copy_lacks_and_prune_keeps_open_items() {
        let mut s = ArtifactsState::default();
        let mut a = art("file:a", ArtifactKind::File, 100);
        a.embedding = Some(vec![1, 2]);
        a.summary = Some("s".into());
        s.upsert(a);
        s.upsert(art("file:a", ArtifactKind::File, 200));
        let got = &s.artifacts["file:a"];
        assert_eq!((got.first_seen, got.last_touched), (100, 200));
        assert_eq!(got.embedding, Some(vec![1, 2]));
        assert_eq!(got.summary.as_deref(), Some("s"));
        s.upsert(art("github:pr:1", ArtifactKind::Pr, 50));
        s.upsert(art("file:b", ArtifactKind::File, 50));
        s.prune(1_000, 500);
        assert!(s.artifacts.contains_key("github:pr:1"), "open items stay");
        assert!(!s.artifacts.contains_key("file:b") && !s.artifacts.contains_key("file:a"));
    }

    #[test]
    fn a_repeated_event_is_a_duplicate_within_a_minute() {
        let mut s = ArtifactsState::default();
        s.upsert(art("github:pr:1", ArtifactKind::Pr, 50));
        let e = ActivityEvent {
            ts: 1_000,
            source: "github".into(),
            kind: ArtifactKind::Pr,
            artifact: "github:pr:1".into(),
            action: Action::Seen,
            session: None,
            detail: None,
        };
        assert!(!s.is_duplicate(&e));
        s.touched(&e);
        assert!(s.is_duplicate(&ActivityEvent { ts: 30_000, ..e.clone() }));
        assert!(!s.is_duplicate(&ActivityEvent { ts: 90_000, ..e.clone() }));
        assert!(!s.is_duplicate(&ActivityEvent { action: Action::Changed, ..e.clone() }));
        assert_eq!(s.artifacts["github:pr:1"].last_touched, 1_000);
    }
}
