//! The lexicon's words for stored values (docs/lexicon.md). Stored formats keep their values;
//! people see these words.

use adage_term::Strength;
use adage_tracks::{Aspect, RouteState};

/// A stored aspect as a strength: green is fire, yellow is prime, red is inhibit.
pub fn strength(a: Aspect) -> Strength {
    match a {
        Aspect::Green => Strength::Fire,
        Aspect::Yellow => Strength::Prime,
        Aspect::Red => Strength::Inhibit,
    }
}

/// A stored route state as a lifecycle word.
pub fn lifecycle(s: RouteState) -> &'static str {
    match s {
        RouteState::Draft => "nascent",
        RouteState::Probation => "forming",
        RouteState::Active => "firm",
        RouteState::Demoted => "weakened",
        RouteState::Retired => "extinguished",
        RouteState::Stale => "dormant",
    }
}

/// A reflex's evidence as a row shows it: `23/33  lb 0.53`, or a dash when no trace has had
/// the chance yet (a new instinct).
pub fn evidence(e: &adage_tracks::Evidence) -> adage_term::Line {
    if e.opportunities == 0 {
        adage_term::Line::new().faint("  —   ").faint("  no traces yet")
    } else {
        adage_term::Line::new()
            .text(format!("{:>3}/{:<3}", e.support, e.opportunities))
            .faint(format!("  lb {:.2}", e.lower_bound))
    }
}

/// `n` and a noun, pluralised (`1 trace`, `3 reflexes`).
pub fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else if noun.ends_with('x') || noun.ends_with('s') {
        format!("{n} {noun}es")
    } else if let Some(stem) = noun.strip_suffix('y').filter(|s| !s.ends_with(['a', 'e', 'o', 'u'])) {
        format!("{n} {stem}ies")
    } else {
        format!("{n} {noun}s")
    }
}

/// Stored prose (change summaries, reasons written by the learner) in the lexicon's words, for
/// display. The stored text itself is unchanged.
pub fn prose(s: &str) -> String {
    use std::sync::OnceLock;
    static SUBS: OnceLock<Vec<(regex::Regex, &'static str)>> = OnceLock::new();
    let subs = SUBS.get_or_init(|| {
        [
            (r"\bgreen\b", "fire"),
            (r"\byellow\b", "prime"),
            (r"\bred\b", "inhibit"),
            (r"\broutes\b", "reflexes"),
            (r"\broute\b", "reflex"),
            (r"\bsessions\b", "traces"),
            (r"\bsession\b", "trace"),
            (r"\bholdout\b", "control"),
            (r"\bretired\b", "extinguished"),
            (r"\bdrafted\b", "nascent"),
            (r"\bprobation\b", "forming"),
        ]
        .into_iter()
        .map(|(p, w)| (regex::Regex::new(p).expect("static pattern"), w))
        .collect()
    });
    let mut out = s.to_string();
    for (re, w) in subs {
        out = re.replace_all(&out, *w).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_word() {
        for s in [
            RouteState::Draft,
            RouteState::Probation,
            RouteState::Active,
            RouteState::Demoted,
            RouteState::Retired,
            RouteState::Stale,
        ] {
            assert!(!lifecycle(s).is_empty());
        }
        assert_eq!(count(1, "trace"), "1 trace");
        assert_eq!(count(3, "reflex"), "3 reflexes");
        assert_eq!(count(0, "instinct"), "0 instincts");
        assert_eq!(count(2, "intent family"), "2 intent families");
        assert_eq!(count(2, "day"), "2 days");
    }

    #[test]
    fn stored_prose_reads_in_the_lexicon() {
        assert_eq!(
            prose("finish line route yellow → green (8 of 8 sessions, lower bound 0.50)"),
            "finish line reflex prime → fire (8 of 8 traces, lower bound 0.50)"
        );
        assert_eq!(prose("reduced credit"), "reduced credit", "whole words only");
    }
}
