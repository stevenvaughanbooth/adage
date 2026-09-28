//! The lexicon at display time (docs/lexicon.md). Stored values (route states, aspects, the
//! review verbs `crate::review` takes) keep their names; people see these words.

/// A route's stored state as its lifecycle word. Accepts the lexicon's words as well, so tracks
/// written after the on-disk migration read the same.
pub fn lifecycle(state: &str) -> &str {
    match state {
        "draft" | "nascent" => "nascent",
        "probation" | "forming" => "forming",
        "active" | "firm" => "firm",
        "stale" | "dormant" => "dormant",
        "retired" | "extinguished" => "extinguished",
        "demoted" | "weakened" => "weakened",
        other => other,
    }
}

/// Where a lifecycle word sits on the way to firm: nascent 0, forming 1, firm 2 (and the states
/// a firm reflex can fall to after it).
pub fn lifecycle_rank(word: &str) -> usize {
    match word {
        "nascent" => 0,
        "forming" => 1,
        _ => 2,
    }
}

/// The review verb a person sees for the one `crate::review` records.
pub fn verb(internal: &str) -> &'static str {
    match internal {
        "promote" => "strengthen",
        "demote" => "weaken",
        "reject" | "retire" => "extinguish",
        "approve" => "approve",
        "pin" => "pin",
        "dismiss" => "dismiss",
        _ => "review",
    }
}

/// The command that records the same decision from any shell.
pub fn review_command(internal: &str, reference: &str, id: &str) -> String {
    format!("adage review {} {reference} {id}", verb(internal))
}

/// What a decision does, in a sentence.
pub fn verb_effect(internal: &str) -> &'static str {
    match internal {
        "approve" => {
            "Approve: accept the reflex as it is (or apply its proposed change, or put a nascent reflex into service)."
        }
        "reject" => {
            "Extinguish: retire the reflex; conditioning keeps it extinguished unless it strengthens materially."
        }
        "demote" => "Weaken: fire → prime, held across conditioning.",
        "promote" => "Strengthen: prime → fire, never above the reflex's effect ceiling.",
        "pin" => "Pin: mark it authored; conditioning proposes changes instead of overwriting it.",
        "dismiss" => "Dismiss: keep the pinned reflex and silence its proposed change.",
        _ => "",
    }
}

/// `harbor-dispatch/incidents` → `incidents`, for narrow panes.
pub fn short_track(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

/// Cut `s` to `n` display columns, with an ellipsis when it was longer.
pub fn fit(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    if n == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(n - 1).collect();
    out.push('…');
    out
}

/// `s` as a column `n` wide: padded, or cut so at least one space separates it from the next.
pub fn pad(s: &str, n: usize) -> String {
    let f = if s.chars().count() < n { s.to_string() } else { fit(s, n.saturating_sub(1)) };
    let w = f.chars().count();
    format!("{f}{}", " ".repeat(n.saturating_sub(w)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_states_read_as_lifecycle_words() {
        for (s, w) in [
            ("draft", "nascent"),
            ("probation", "forming"),
            ("active", "firm"),
            ("stale", "dormant"),
            ("retired", "extinguished"),
            ("demoted", "weakened"),
            ("firm", "firm"),
        ] {
            assert_eq!(lifecycle(s), w);
        }
        assert_eq!(review_command("demote", "acme/work", "lookup"), "adage review weaken acme/work lookup");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(pad("ab", 4), "ab  ");
    }
}
