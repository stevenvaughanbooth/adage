//! The world model: what the person was doing outside the session, as Adage's situational
//! awareness layer records it (docs/architecture.md, layer 1; the design is the "Adage Situational
//! Awareness Layer" document and its implementation plan).
//!
//! The agent trace is one time series: what the agent did. The world is the second: what the
//! person looked at, when their calendar was busy, which pull requests and tickets were open.
//! Every source (a calendar connector read through the Claude Code mod, a transcript directory
//! watched on disk, the agent's own spool records) is translated once, at its edge, into the
//! types here; the resolver (`adage-world`), the hook and the learner read these types only.
//!
//! Nothing here holds content: an [`Artifact`] carries a title, a locator and at most a short
//! summary made locally; an [`ActivityEvent`] names the artifact and what happened to it. The
//! agent reads the artifact itself, under the harness's own permission model.

use serde::{Deserialize, Serialize};

/// What kind of thing an artifact is. Never renamed: every reader of the world store and every
/// source descriptor (`packs/world/*.toml`, `kind`) names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// A meeting transcript or notes file that landed on disk.
    Transcript,
    /// A document (a shared doc, a wiki page, a design doc).
    Doc,
    /// A pull request.
    Pr,
    /// A ticket or work item.
    Ticket,
    /// A calendar event, with its attendees and attached documents.
    CalendarEvent,
    /// A file in a configured root.
    File,
    /// A URL the person had open.
    Url,
}

impl ArtifactKind {
    pub const ALL: [ArtifactKind; 7] = [
        ArtifactKind::Transcript,
        ArtifactKind::Doc,
        ArtifactKind::Pr,
        ArtifactKind::Ticket,
        ArtifactKind::CalendarEvent,
        ArtifactKind::File,
        ArtifactKind::Url,
    ];

    /// The kind as a descriptor or a record spells it.
    pub const fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Transcript => "transcript",
            ArtifactKind::Doc => "doc",
            ArtifactKind::Pr => "pr",
            ArtifactKind::Ticket => "ticket",
            ArtifactKind::CalendarEvent => "calendar_event",
            ArtifactKind::File => "file",
            ArtifactKind::Url => "url",
        }
    }

    pub fn parse(s: &str) -> Option<ArtifactKind> {
        ArtifactKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// How a person would name one in a hint ("transcript", "pull request").
    pub const fn label(self) -> &'static str {
        match self {
            ArtifactKind::Transcript => "transcript",
            ArtifactKind::Doc => "document",
            ArtifactKind::Pr => "pull request",
            ArtifactKind::Ticket => "ticket",
            ArtifactKind::CalendarEvent => "meeting",
            ArtifactKind::File => "file",
            ArtifactKind::Url => "page",
        }
    }
}

/// What happened to an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// The source listed it (a calendar event in the lookahead window, an open pull request).
    Seen,
    /// The person opened it (a window title, a browser tab, a file read).
    Opened,
    /// The person edited it.
    Edited,
    /// A calendar event started.
    Started,
    /// A calendar event ended.
    Ended,
    /// A file landed on disk (a transcript written by a meeting tool).
    Landed,
    /// Its state changed (CI went red, a ticket moved); `detail` says how.
    Changed,
}

/// One thing that happened, in the world or in the agent's own trace: the append-only activity
/// timeline's record (`$ADAGE_HOME/world/activity.spool`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// Milliseconds since the Unix epoch.
    pub ts: u64,
    /// The source descriptor's name (`calendar`, `github`, `meetings`, `files`, `desktop`), or
    /// `agent` for the agent's own trace.
    pub source: String,
    pub kind: ArtifactKind,
    /// [`Artifact::id`].
    pub artifact: String,
    pub action: Action,
    /// The agent session this came from, for `agent` events; absent for the world's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// What changed, for [`Action::Changed`] (`ci:red`, `state:in_review`); never content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One thing the person may mean: a transcript, a document, a pull request, a ticket, a meeting,
/// a file or a page. Kept in `$ADAGE_HOME/world/artifacts.state`, bounded, most recently touched
/// first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    /// Stable within a source: `calendar:<event id>`, `github:pr:412`, `file:<path>`.
    pub id: String,
    pub kind: ArtifactKind,
    pub title: String,
    /// Where the agent finds it: a path, a URL, a pull request number, a ticket key, an event id.
    pub locator: String,
    /// At most [`MAX_SUMMARY`] characters, made locally (the first lines of a transcript, a pull
    /// request's description head); never the whole artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// `adage_intent::quantize` of the embedding of the title and summary, in units of 1e-4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<i16>>,
    pub first_seen: u64,
    pub last_touched: u64,
    /// Ids of artifacts this one points at (a meeting's attached documents, a ticket's pull
    /// request).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<String>,
    /// The artifact's state as its source reports it (`open`, `ci:green`, `in_review`), for
    /// the snapshot and the delta classifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Who is on it (a meeting's attendees, a pull request's reviewers), as names the resolver
    /// may match a prompt against; never addresses beyond what a title would show.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<String>,
}

/// The most characters a summary keeps.
pub const MAX_SUMMARY: usize = 300;

impl Artifact {
    /// Text the resolver embeds and matches: the title and the summary's head.
    pub fn text(&self) -> String {
        match &self.summary {
            Some(s) if !s.is_empty() => format!("{} {}", self.title, truncate(s, MAX_SUMMARY)),
            _ => self.title.clone(),
        }
    }
}

/// `s` cut to at most `max` characters, on a character boundary.
pub fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// One block on the person's calendar, in the window a source looked at (`$ADAGE_HOME/world/calendar.state`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalendarBlock {
    /// [`Artifact::id`] of the matching calendar event.
    pub id: String,
    pub start: u64,
    pub end: u64,
    pub title: String,
    /// The person is unavailable during it (not a free or transparent event).
    pub busy: bool,
    /// The person accepted it (or owns it); a declined or tentative block is not a gap.
    #[serde(default = "yes")]
    pub accepted: bool,
    /// The person marked it as not to be interrupted, even by an interrupt.
    #[serde(default)]
    pub focus: bool,
    #[serde(default)]
    pub attendees: u32,
}

const fn yes() -> bool {
    true
}

impl CalendarBlock {
    /// Whether the block makes the person unavailable at `ts`.
    pub fn covers(&self, ts: u64) -> bool {
        self.busy && self.accepted && self.start <= ts && ts < self.end
    }
}

/// Where an injection landed in the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionAt {
    /// `UserPromptSubmit` context (the command-hook floor), or the mod's `prompt.submit` context.
    Prompt,
    /// The first message's context block (the mod's `prompt.context`), sent once and after a
    /// compaction.
    FirstMessage,
    /// A row appended to the running conversation (`$.session.append`).
    Append,
    /// A prompt the mod submitted when the session was idle (`$.prompt.submit`).
    Submit,
}

/// What an injection was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionClass {
    /// One likely reference, attached.
    Hint,
    /// Two or three contenders, for the agent to confirm in one line.
    Disambiguation,
    /// The world as it stood: today's calendar, open pull requests, assigned tickets.
    Snapshot,
    /// What changed since the last injection, coalesced.
    Delta,
    /// A change that blocks or invalidates the session's current work, delivered at once.
    Interrupt,
}

/// One delivery of world context to a session: the `injection` record of the session's spool,
/// and the learner's label (`used`, `overridden`) once the next turns are seen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Injection {
    /// `w<ts>-<n>`: unique within the session.
    pub id: String,
    pub session: String,
    pub at: InjectionAt,
    pub cls: InjectionClass,
    /// The artifacts named, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    /// An estimate of the text's tokens (characters / 4).
    pub tokens: u32,
    /// The resolver's top score, for a hint or a disambiguation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// The agent opened a named artifact within the next three turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<bool>,
    /// The person supplied a different artifact in the next prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overridden: Option<bool>,
}

/// The world as one session saw it last: what the snapshot and the delta are computed from
/// (`world` on the session's state file).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WorldView {
    /// When it was taken.
    pub ts: u64,
    /// A hash of the rendered snapshot, so the same text is served after a compaction unless the
    /// world changed.
    pub snapshot_hash: String,
    /// `(artifact id, state)` for every open pull request and ticket at the time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<(String, String)>,
    /// The calendar blocks it listed, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    /// The artifacts this session is working on (its branch's pull request, files it read or
    /// edited in the last ten turns): what makes a change an interrupt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus: Vec<String>,
    /// Interrupts delivered so far (at most three per session).
    #[serde(default)]
    pub interrupts: u32,
    /// When the last delta was delivered.
    #[serde(default)]
    pub last_delta_ts: u64,
    /// The hint delivered at the latest prompt, waiting to be labelled: `used` once the agent
    /// opens one of its artifacts within the next three tool batches, else not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingHint>,
    /// Hints delivered this session, for the record's ids.
    #[serde(default)]
    pub hints: u32,
}

impl WorldView {
    pub fn is_empty(&self) -> bool {
        *self == WorldView::default()
    }
}

/// A hint waiting for its label ([`WorldView::pending`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingHint {
    /// [`Injection::id`].
    pub id: String,
    /// `(artifact id, locator)` of each artifact the hint named.
    pub artifacts: Vec<(String, String)>,
    /// Tool batches left before the hint counts as unused.
    pub batches_left: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_by_name() {
        for k in ArtifactKind::ALL {
            assert_eq!(ArtifactKind::parse(k.as_str()), Some(k));
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(json, format!("\"{}\"", k.as_str()), "serde and as_str agree");
        }
        assert_eq!(ArtifactKind::parse("meeting"), None);
    }

    #[test]
    fn an_event_serialises_with_sorted_keys_and_no_empty_fields() {
        let e = ActivityEvent {
            ts: 5,
            source: "calendar".into(),
            kind: ArtifactKind::CalendarEvent,
            artifact: "calendar:1".into(),
            action: Action::Ended,
            session: None,
            detail: None,
        };
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["action", "artifact", "kind", "source", "ts"]);
        let back: ActivityEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn a_block_covers_only_busy_accepted_time() {
        let b = CalendarBlock {
            id: "c".into(),
            start: 100,
            end: 200,
            title: "sync".into(),
            busy: true,
            accepted: true,
            focus: false,
            attendees: 3,
        };
        assert!(b.covers(100) && b.covers(199) && !b.covers(200) && !b.covers(99));
        assert!(!CalendarBlock { busy: false, ..b.clone() }.covers(150));
        assert!(!CalendarBlock { accepted: false, ..b }.covers(150));
    }

    #[test]
    fn artifact_text_keeps_the_title_and_a_bounded_summary() {
        let a = Artifact {
            id: "t".into(),
            kind: ArtifactKind::Transcript,
            title: "Marketing sync".into(),
            locator: "/m.md".into(),
            summary: Some("x".repeat(400)),
            embedding: None,
            first_seen: 0,
            last_touched: 0,
            related: vec![],
            state: None,
            people: vec![],
        };
        assert_eq!(a.text().chars().count(), "Marketing sync ".len() + MAX_SUMMARY);
        assert_eq!(truncate("héllo", 2), "hé");
    }
}
