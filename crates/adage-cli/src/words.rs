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

/// `n` and a noun, pluralised (`1 trace`, `3 reflexes`).
pub fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else if noun.ends_with('x') || noun.ends_with('s') {
        format!("{n} {noun}es")
    } else {
        format!("{n} {noun}s")
    }
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
    }
}
