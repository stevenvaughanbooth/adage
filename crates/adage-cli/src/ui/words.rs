//! The lexicon at display time (docs/lexicon.md). Stored values and the review verbs
//! `crate::review` takes are the lexicon's words; the older spellings (route states `active`,
//! `retired`…, verbs `promote`, `demote`, `reject`) still read as the same words.

/// A route's stored state as its lifecycle word. Accepts the older spellings as well, so tracks
/// written before the on-disk migration read the same.
pub fn lifecycle(state: &str) -> &str {
    match state {
        "draft" | "nascent" => "nascent",
        "probation" | "forming" => "forming",
        "active" | "firm" => "firm",
        "stale" | "dormant" => "dormant",
        "retired" | "extinguished" => "extinguished",
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

/// The review verb, in the lexicon's words (the older spellings map to them).
pub fn verb(internal: &str) -> &'static str {
    match internal {
        "strengthen" | "promote" => "strengthen",
        "weaken" | "demote" => "weaken",
        "extinguish" | "reject" | "retire" => "extinguish",
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
    match verb(internal) {
        "approve" => {
            "Approve: accept the reflex or proposed change, or put a nascent reflex into service. This does not grant script execution."
        }
        "extinguish" => {
            "Extinguish: take it out of service; conditioning keeps it extinguished unless it strengthens materially."
        }
        "weaken" => "Weaken: fire → prime, held across conditioning.",
        "strengthen" => "Strengthen: prime → fire, never above the reflex's effect ceiling.",
        "pin" => "Pin: mark it authored; conditioning proposes changes instead of overwriting it.",
        "dismiss" => "Dismiss: keep the pinned reflex and silence its proposed change.",
        _ => "",
    }
}

/// `harbor-dispatch/incidents` → `incidents`, for narrow panes.
pub fn short_track(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

/// Cut `s` to `n` display columns, with `more` (an ellipsis: `…`, or `...` where that cannot be
/// drawn) when it was longer. Views call it through `Theme::fit`, which picks the ellipsis.
pub fn fit_with(s: &str, n: usize, more: &str) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let m = more.chars().count();
    if n < m {
        return s.chars().take(n).collect();
    }
    let mut out: String = s.chars().take(n - m).collect();
    out.push_str(more);
    out
}

/// [`fit_with`] from the left: the end of `s` (a path's file name) survives, `more` in front.
pub fn fit_left(s: &str, n: usize, more: &str) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let m = more.chars().count();
    if n < m {
        return s.chars().skip(count - n).collect();
    }
    format!("{more}{}", s.chars().skip(count - (n - m)).collect::<String>())
}

/// `s` as a column `n` wide: padded, or cut (with `more`) so at least one space separates it
/// from the next. Views call it through `Theme::pad`.
pub fn pad_with(s: &str, n: usize, more: &str) -> String {
    let f = if s.chars().count() < n { s.to_string() } else { fit_with(s, n.saturating_sub(1), more) };
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
            ("firm", "firm"),
        ] {
            assert_eq!(lifecycle(s), w);
        }
        assert_eq!(review_command("weaken", "acme/work", "lookup"), "adage review weaken acme/work lookup");
        assert_eq!(review_command("demote", "acme/work", "lookup"), "adage review weaken acme/work lookup");
        assert_eq!(verb_effect("promote"), verb_effect("strengthen"));
        assert_eq!(fit_with("abcdef", 4, "…"), "abc…");
        assert_eq!(fit_with("abcdef", 5, "..."), "ab...");
        assert_eq!(fit_with("abcdef", 2, "..."), "ab");
        assert_eq!(fit_with("abcdef", 0, "…"), "");
        assert_eq!(pad_with("ab", 4, "…"), "ab  ");
        assert_eq!(pad_with("abcdef", 5, "..."), "a... ");
        assert_eq!(fit_left("~/a/b/fail-open.log", 16, "…"), "…b/fail-open.log");
        assert_eq!(fit_left("~/a/b/fail-open.log", 16, "..."), "...fail-open.log");
        assert_eq!(fit_left("short", 15, "…"), "short");
    }
}
