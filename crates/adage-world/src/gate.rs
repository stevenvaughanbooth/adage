//! The trigger gate: does this prompt refer to something outside the session? Cheap heuristics,
//! before anything is read or embedded (the design's "Trigger gate"): demonstratives and anaphora,
//! temporal anchors, a kind named with an article ("the PR"), a key or a number, or a bare entity
//! that matches an artifact's title or a person on it. A prompt that already carries a path, a
//! URL or an `@file` mention names its own artifact and is never resolved.

use crate::text::{contains_phrase, words};
use adage_trace::{Artifact, ArtifactKind};

/// What the gate found in the prompt.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Signal {
    /// The prompt as given, for the scorer.
    pub prompt: String,
    /// "that", "those", "the notes", "what we discussed".
    pub demonstrative: bool,
    /// "just", "earlier", "this morning", "from the meeting", "yesterday".
    pub temporal: bool,
    /// The kinds the prompt names with an article ("the PR", "the transcript", "the meeting").
    pub kinds: Vec<ArtifactKind>,
    /// Ids of artifacts whose title, key, number or person the prompt names.
    pub entities: Vec<String>,
    /// Ticket keys and pull request numbers spelled in the prompt (`mkt-1182`, `#412`).
    pub keys: Vec<String>,
}

impl Signal {
    pub fn is_empty(&self) -> bool {
        !self.demonstrative
            && !self.temporal
            && self.kinds.is_empty()
            && self.entities.is_empty()
            && self.keys.is_empty()
    }
}

const DEMONSTRATIVE: &[&[&str]] = &[
    &["that"],
    &["those"],
    &["these"],
    &["the", "notes"],
    &["the", "follow-ups"],
    &["the", "follow", "ups"],
    &["the", "action", "items"],
    &["what", "we", "discussed"],
    &["what", "we", "talked"],
    &["we", "discussed"],
    &["we", "talked", "about"],
    &["the", "thing"],
    &["the", "one"],
    &["the", "same"],
    &["the", "last"],
];

const TEMPORAL: &[&[&str]] = &[
    &["just"],
    &["earlier"],
    &["this", "morning"],
    &["this", "afternoon"],
    &["today"],
    &["yesterday"],
    &["last", "night"],
    &["from", "the", "meeting"],
    &["from", "the", "call"],
    &["from", "the", "sync"],
    &["from", "the", "standup"],
    &["after", "the", "meeting"],
    &["in", "the", "meeting"],
    &["an", "hour", "ago"],
    &["minutes", "ago"],
    &["recent"],
    &["recently"],
    &["latest"],
];

/// A kind named with an article: `("the pr", Pr)`.
const KINDS: &[(&[&str], ArtifactKind)] = &[
    (&["the", "pr"], ArtifactKind::Pr),
    (&["the", "pull", "request"], ArtifactKind::Pr),
    (&["that", "pr"], ArtifactKind::Pr),
    (&["my", "pr"], ArtifactKind::Pr),
    (&["the", "ticket"], ArtifactKind::Ticket),
    (&["the", "issue"], ArtifactKind::Ticket),
    (&["that", "ticket"], ArtifactKind::Ticket),
    (&["the", "transcript"], ArtifactKind::Transcript),
    (&["the", "notes"], ArtifactKind::Transcript),
    (&["those", "notes"], ArtifactKind::Transcript),
    (&["the", "follow-ups"], ArtifactKind::Transcript),
    (&["those", "follow-ups"], ArtifactKind::Transcript),
    (&["the", "action", "items"], ArtifactKind::Transcript),
    (&["those", "action", "items"], ArtifactKind::Transcript),
    (&["the", "meeting"], ArtifactKind::CalendarEvent),
    (&["the", "call"], ArtifactKind::CalendarEvent),
    (&["the", "sync"], ArtifactKind::CalendarEvent),
    (&["the", "standup"], ArtifactKind::CalendarEvent),
    (&["the", "doc"], ArtifactKind::Doc),
    (&["the", "document"], ArtifactKind::Doc),
    (&["the", "spec"], ArtifactKind::Doc),
    (&["the", "design", "doc"], ArtifactKind::Doc),
    (&["the", "page"], ArtifactKind::Url),
    (&["that", "page"], ArtifactKind::Url),
    (&["the", "file"], ArtifactKind::File),
    (&["that", "file"], ArtifactKind::File),
];

/// Whether the prompt names its own artifact: a path, a URL or an `@` mention.
pub fn names_its_own(prompt: &str) -> bool {
    prompt.split_whitespace().any(|t| {
        let t =
            t.trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == ',' || c == '.' || c == ')' || c == '(');
        t.starts_with("http://")
            || t.starts_with("https://")
            || t.starts_with('@') && t.len() > 1
            || t.starts_with("~/")
            || t.starts_with("./")
            || t.starts_with('/') && t.len() > 1
            || t.contains('/')
                && t.rsplit('.').next().is_some_and(|ext| ext.len() <= 5 && ext.chars().all(char::is_alphanumeric))
    })
}

/// A ticket key (`MKT-1182`) or a pull request number (`#412`), as [`crate::text::words`] keeps them.
pub fn is_key(w: &str) -> bool {
    if let Some(n) = w.strip_prefix('#') {
        return !n.is_empty() && n.chars().all(|c| c.is_ascii_digit());
    }
    match w.split_once('-') {
        Some((a, b)) => {
            (2..=10).contains(&a.len())
                && a.chars().all(|c| c.is_ascii_alphabetic())
                && !b.is_empty()
                && b.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// The gate: `None` when the prompt names its own artifact or carries no signal.
pub fn gate(prompt: &str, artifacts: &[Artifact]) -> Option<Signal> {
    if names_its_own(prompt) {
        return None;
    }
    let ws = words(prompt);
    let has = |table: &[&[&str]]| {
        table.iter().any(|phrase| {
            let p: Vec<String> = phrase.iter().map(|s| s.to_string()).collect();
            contains_phrase(&ws, &p)
        })
    };
    let mut s = Signal {
        prompt: prompt.to_string(),
        demonstrative: has(DEMONSTRATIVE),
        temporal: has(TEMPORAL),
        ..Signal::default()
    };
    for (phrase, kind) in KINDS {
        let p: Vec<String> = phrase.iter().map(|x| x.to_string()).collect();
        if contains_phrase(&ws, &p) && !s.kinds.contains(kind) {
            s.kinds.push(*kind);
        }
    }
    s.keys = ws.iter().filter(|w| is_key(w)).cloned().collect();
    s.entities = artifacts.iter().filter(|a| names(a, &ws)).map(|a| a.id.clone()).collect();
    s.entities.sort();
    s.entities.dedup();
    (!s.is_empty()).then_some(s)
}

/// Whether the prompt's words name this artifact: its title's content phrase (two or more
/// consecutive words, or its one word of four or more characters), its key or number, or a person
/// on it by a name of three or more characters.
pub fn names(a: &Artifact, prompt_words: &[String]) -> bool {
    let title: Vec<String> = words(&a.title).into_iter().filter(|w| !crate::text::STOP.contains(&w.as_str())).collect();
    let by_title = match title.len() {
        0 => false,
        1 => title[0].len() >= 4 && prompt_words.contains(&title[0]),
        _ => title.windows(2).any(|pair| contains_phrase(prompt_words, pair)),
    };
    let by_person = a.people.iter().any(|p| {
        let first = words(p).into_iter().next().unwrap_or_default();
        first.len() >= 3 && prompt_words.contains(&first)
    });
    by_title || names_by_key(a, prompt_words) || by_person
}

/// Whether a key or a pull request number among `prompt_words` is this artifact's (its locator
/// or id carries the key; a pull request's number matches `#<n>`).
pub fn names_by_key(a: &Artifact, prompt_words: &[String]) -> bool {
    let loc = words(&a.locator);
    loc.iter().chain(words(&a.id).iter()).any(|w| is_key(w) && prompt_words.contains(w))
        || a.kind == ArtifactKind::Pr
            && loc.iter().chain(words(&a.id).iter()).any(|w| {
                w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty() && prompt_words.contains(&format!("#{w}"))
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(id: &str, kind: ArtifactKind, title: &str, locator: &str, people: &[&str]) -> Artifact {
        Artifact {
            id: id.into(),
            kind,
            title: title.into(),
            locator: locator.into(),
            summary: None,
            embedding: None,
            first_seen: 0,
            last_touched: 0,
            related: vec![],
            state: None,
            people: people.iter().map(|p| p.to_string()).collect(),
        }
    }

    fn world() -> Vec<Artifact> {
        vec![
            art(
                "calendar:1",
                ArtifactKind::CalendarEvent,
                "Marketing sync",
                "evt1",
                &["Priya Natarajan", "Dana Ortiz"],
            ),
            art("github:pr:412", ArtifactKind::Pr, "Schema evolution for invoices", "#412", &[]),
            art("jira:MKT-1182", ArtifactKind::Ticket, "Tracking spec for Q4 campaign", "MKT-1182", &[]),
        ]
    }

    #[test]
    fn a_prompt_naming_its_own_artifact_is_never_resolved() {
        for p in [
            "read ~/Meetings/2026-10-27.md and file the follow-ups",
            "open https://example.com/doc and summarise it",
            "look at @README.md",
            "fix src/lib.rs",
            "fix ./Makefile",
        ] {
            assert_eq!(gate(p, &world()), None, "{p}");
        }
    }

    #[test]
    fn a_prompt_with_no_signal_is_nothing() {
        for p in ["add a currency field to the invoice model", "run the tests", "hello", ""] {
            assert_eq!(gate(p, &world()), None, "{p}");
        }
    }

    #[test]
    fn signals_are_found_by_kind() {
        let s = gate("file those follow-ups from the marketing sync", &world()).unwrap();
        assert!(s.demonstrative && !s.temporal);
        assert_eq!(s.kinds, [ArtifactKind::Transcript], "\"those follow-ups\" names the transcript");
        assert_eq!(s.entities, ["calendar:1"]);
        let s = gate("file the follow-ups from the meeting", &world()).unwrap();
        assert!(s.temporal);
        assert_eq!(s.kinds, [ArtifactKind::Transcript, ArtifactKind::CalendarEvent]);
        let s = gate("review the PR", &world()).unwrap();
        assert_eq!(s.kinds, [ArtifactKind::Pr]);
        assert!(s.entities.is_empty() && !s.temporal);
        let s = gate("close MKT-1182", &world()).unwrap();
        assert_eq!(s.keys, ["mkt-1182"]);
        assert_eq!(s.entities, ["jira:MKT-1182"]);
        let s = gate("what did Priya ask for?", &world()).unwrap();
        assert_eq!(s.entities, ["calendar:1"], "a person on the meeting names it");
        let s = gate("merge #412 when green", &world()).unwrap();
        assert_eq!(s.entities, ["github:pr:412"]);
    }

    #[test]
    fn keys_are_recognised_and_plain_words_are_not() {
        assert!(is_key("mkt-1182") && is_key("#412") && is_key("plat-7"));
        assert!(!is_key("follow-ups") && !is_key("#") && !is_key("q4-plan") || !is_key("abc"));
        assert!(!is_key("the"));
    }
}
