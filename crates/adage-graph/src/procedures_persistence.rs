//! Weak method recognition only: no execution or correctness classification lives here.
use super::{NamedCaseObservation, Session, StepObs};
use std::collections::BTreeSet;

fn words(text: &str) -> BTreeSet<String> {
    text.chars()
        .take(64 * 1024)
        .collect::<String>()
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn storage(w: &BTreeSet<String>) -> bool {
    [
        "persist",
        "persisted",
        "persistence",
        "persistent",
        "storage",
        "store",
        "serialized",
        "serialization",
        "serializer",
        "serialize",
        "deserialization",
        "json",
        "database",
        "index",
        "snapshot",
        "checkpoint",
        "corpus",
    ]
    .iter()
    .any(|s| w.contains(*s))
}

fn equivalence(w: &BTreeSet<String>) -> bool {
    w.contains("incremental")
        && ["rebuild", "bulk", "batch"].iter().any(|s| w.contains(*s))
        && ["equals", "equal", "equivalence", "equivalent", "matches", "same"].iter().any(|s| w.contains(*s))
}

pub(super) fn purpose(description: &str) -> bool {
    let w = words(description);
    let roundtrip = w.contains("roundtrip")
        || w.contains("roundtrips")
        || (w.contains("round") && (w.contains("trip") || w.contains("trips")));
    storage(&w) && (roundtrip || w.contains("reopen") || equivalence(&w))
}

pub(super) fn named_cases(s: &Session, st: &StepObs, argv: &[String]) -> Vec<NamedCaseObservation> {
    // Initially only Cargo's stable per-case text protocol is recognized. A case name from
    // arbitrary output or another runner is not promoted by matching a keyword.
    if argv.first().map(String::as_str) != Some("cargo")
        || argv.get(1).map(String::as_str) != Some("test")
        || st.call.output.len() > 256 * 1024
    {
        return Vec::new();
    }
    let task = s.prompts.get(st.prompt as usize).map(|p| p.text.as_str()).unwrap_or(&s.prompt);
    let task_context = storage(&words(task))
        || s.delegation.as_ref().and_then(|d| d.task.as_deref()).is_some_and(|t| storage(&words(t)));
    if !task_context {
        return Vec::new();
    }
    let mut seen = BTreeSet::new();
    st.call
        .output
        .lines()
        .filter_map(|line| {
            let (name, result) = line.strip_prefix("test ")?.split_once(" ... ")?;
            if name.is_empty()
                || name.len() > 256
                || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':'))
                || !matches!(result, "ok" | "FAILED")
                || !seen.insert(name)
            {
                return None;
            }
            // Module namespaces locate the case but do not describe its assertion. A pure
            // date-conversion test in a storage module is not a persistence round trip.
            let leaf = name.rsplit("::").next().unwrap_or(name);
            let w = words(leaf);
            if !equivalence(&w) && !purpose(leaf) {
                return None;
            }
            Some(NamedCaseObservation { name: name.into(), reported_failed: result == "FAILED" })
        })
        .take(16)
        .collect()
}
