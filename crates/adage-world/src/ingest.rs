//! What `adage world ingest` checks before anything is written: an event's descriptor, its
//! shape, the deny list. Pure; the verb does the I/O.

use crate::descriptor::Descriptor;
use adage_trace::{ActivityEvent, Artifact, ArtifactKind};

/// The longest title, locator or id written.
pub const MAX_FIELD: usize = 512;
/// The most events one ingest takes.
pub const MAX_BATCH: usize = 1_000;

/// Whether `text` matches a deny pattern (`world.deny`): a glob over the whole text, or a
/// substring when the pattern has no glob character.
pub fn denied(patterns: &[String], text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    patterns.iter().any(|p| {
        let p = p.to_ascii_lowercase();
        if p.contains(['*', '?', '[']) {
            glob::Pattern::new(&p).is_ok_and(|g| g.matches(&lower))
        } else {
            !p.is_empty() && lower.contains(&p)
        }
    })
}

/// Whether an artifact is denied: its title, locator or id matches.
pub fn artifact_denied(patterns: &[String], a: &Artifact) -> bool {
    denied(patterns, &a.title) || denied(patterns, &a.locator) || denied(patterns, &a.id)
}

/// Everything wrong with an event for `d`, or nothing.
pub fn check_event(d: Option<&Descriptor>, e: &ActivityEvent) -> Result<(), String> {
    if e.source.is_empty() || e.source.len() > 32 {
        return Err("source: empty or too long".into());
    }
    if e.artifact.is_empty() || e.artifact.len() > MAX_FIELD {
        return Err("artifact: empty or too long".into());
    }
    if e.ts == 0 {
        return Err("ts: missing".into());
    }
    match d {
        Some(d) if d.kind() != e.kind => {
            Err(format!("kind {} is not the source's {}", e.kind.as_str(), d.kind().as_str()))
        }
        Some(_) => Ok(()),
        // The agent's own trace names no descriptor; anything else must.
        None if e.source == "agent" => Ok(()),
        None => Err(format!("no descriptor named {}", e.source)),
    }
}

/// Everything wrong with an artifact for `d`, or nothing; `d` is `None` for the agent's own.
pub fn check_artifact(d: Option<&Descriptor>, a: &Artifact) -> Result<(), String> {
    if a.id.is_empty() || a.id.len() > MAX_FIELD {
        return Err("id: empty or too long".into());
    }
    if a.title.len() > MAX_FIELD || a.locator.len() > MAX_FIELD {
        return Err(format!("{}: title or locator too long", a.id));
    }
    if a.summary.as_ref().is_some_and(|s| s.chars().count() > adage_trace::world::MAX_SUMMARY) {
        return Err(format!("{}: summary over {} characters", a.id, adage_trace::world::MAX_SUMMARY));
    }
    if a.people.len() > 32 || a.related.len() > 32 {
        return Err(format!("{}: too many people or related ids", a.id));
    }
    match d {
        Some(d) if d.kind() != a.kind && !(d.kind() == ArtifactKind::CalendarEvent && a.kind == ArtifactKind::Doc) => {
            Err(format!("{}: kind {} is not the source's {}", a.id, a.kind.as_str(), d.kind().as_str()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_trace::Action;

    #[test]
    fn deny_patterns_match_as_globs_or_substrings() {
        let pats: Vec<String> = ["*bank*", "Keychain Access", "*/.ssh/*"].iter().map(|s| s.to_string()).collect();
        assert!(denied(&pats, "Chase Bank - Login"));
        assert!(denied(&pats, "keychain access"));
        assert!(denied(&pats, "/home/u/.ssh/id_ed25519"));
        assert!(!denied(&pats, "Marketing sync"));
        assert!(!denied(&[], "anything"));
    }

    #[test]
    fn an_event_must_match_its_descriptor() {
        let d = crate::descriptor::builtin();
        let cal = d.iter().find(|d| d.name() == "calendar");
        let e = ActivityEvent {
            ts: 1,
            source: "calendar".into(),
            kind: ArtifactKind::CalendarEvent,
            artifact: "calendar:1".into(),
            action: Action::Seen,
            session: None,
            detail: None,
        };
        assert!(check_event(cal, &e).is_ok());
        assert!(check_event(cal, &ActivityEvent { kind: ArtifactKind::Pr, ..e.clone() }).is_err());
        assert!(
            check_event(None, &ActivityEvent { source: "agent".into(), kind: ArtifactKind::File, ..e.clone() }).is_ok()
        );
        assert!(check_event(None, &ActivityEvent { source: "nope".into(), ..e.clone() }).is_err());
        assert!(check_event(cal, &ActivityEvent { ts: 0, ..e }).is_err());
    }
}
