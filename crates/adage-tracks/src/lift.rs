//! Outcome lift: whether traces that took a step succeeded more often than traces that did not.
//!
//! The rate gate (`learn.yellow_rate`) learns only what most agents did. A convention that few
//! agents find, but that the successful ones used, fails it: in the first live corpus, Haiku
//! sessions that used the repository's lookup passed 40 of 44 against 5 of 52 for those that did
//! not, and no reflex formed. The outcome-lift path is a second gate for such a step:
//!
//! - `with` are the traces where the trigger occurred and the step was taken, `without` those
//!   where the trigger occurred and it was not; each counts only traces whose outcome is known;
//! - the lift is `p_with − p_without`, and its lower bound is Newcombe's hybrid score interval
//!   (method 10, from two Wilson intervals) at 95%: `diff − √((p₁ − l₁)² + (u₂ − p₂)²)`;
//! - the step qualifies when at least `min_support` traces took it, and the lower bound is at
//!   least `lower_bound`.
//!
//! A reflex that forms on lift alone is capped at yellow (prime): it says that successful agents
//! did this, not that most agents do.

use serde::{Deserialize, Serialize};

/// The 95% normal quantile Wilson intervals use here.
const Z: f64 = 1.959_964;

/// The lift a step showed on the traces' outcomes (recorded in a route's evidence).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeLift {
    /// Traces that took the step, and how many of them succeeded.
    pub with: u32,
    pub with_passed: u32,
    /// Traces where the step was open but not taken, and how many of them succeeded.
    pub without: u32,
    pub without_passed: u32,
    /// `with_passed/with − without_passed/without`.
    pub diff: f64,
    /// The 95% lower bound on `diff` (Newcombe's hybrid score interval).
    pub lower_bound: f64,
    /// Where the outcomes came from: `outcomes` (recorded, `--outcomes` or a dataset's
    /// `passed`) or `proxies` (read from the transcripts: the last check passed, the agent did
    /// not end by asking for help, the session was not cut off).
    pub source: String,
}

/// Wilson score interval at 95%: `(lower, upper)`.
pub fn wilson(successes: u32, n: u32) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let n_f = n as f64;
    let p = successes as f64 / n_f;
    let d = 1.0 + Z * Z / n_f;
    let centre = p + Z * Z / (2.0 * n_f);
    let half = Z * ((p * (1.0 - p) / n_f) + Z * Z / (4.0 * n_f * n_f)).sqrt();
    (((centre - half) / d).max(0.0), ((centre + half) / d).min(1.0))
}

/// Newcombe's lower bound on `p₁ − p₂` (method 10).
pub fn diff_lower_bound(s1: u32, n1: u32, s2: u32, n2: u32) -> f64 {
    if n1 == 0 || n2 == 0 {
        return -1.0;
    }
    let (p1, p2) = (s1 as f64 / n1 as f64, s2 as f64 / n2 as f64);
    let (l1, _) = wilson(s1, n1);
    let (_, u2) = wilson(s2, n2);
    p1 - p2 - ((p1 - l1).powi(2) + (u2 - p2).powi(2)).sqrt()
}

fn round4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

impl OutcomeLift {
    /// The lift from outcomes of the traces that took the step (`with`) and those that did not
    /// (`without`); `None` when either side has no known outcome.
    pub fn of(with: &[bool], without: &[bool], source: &str) -> Option<OutcomeLift> {
        if with.is_empty() || without.is_empty() {
            return None;
        }
        let (n1, n2) = (with.len() as u32, without.len() as u32);
        let s1 = with.iter().filter(|x| **x).count() as u32;
        let s2 = without.iter().filter(|x| **x).count() as u32;
        let diff = s1 as f64 / n1 as f64 - s2 as f64 / n2 as f64;
        Some(OutcomeLift {
            with: n1,
            with_passed: s1,
            without: n2,
            without_passed: s2,
            diff: round4(diff),
            lower_bound: round4(diff_lower_bound(s1, n1, s2, n2)),
            source: source.to_string(),
        })
    }

    /// Whether the lift is enough to form a reflex on: at least `min_support` traces took the
    /// step, and the lower bound on the difference is at least `lower_bound`.
    pub fn qualifies(&self, min_support: u32, lower_bound: f64) -> bool {
        self.with >= min_support.max(1) && self.lower_bound >= lower_bound && self.diff > 0.0
    }
}

/// The aspect a step gets on the outcome-lift path: yellow (prime) where its effect may be
/// directed at all, never green.
pub fn aspect(effect: adage_core::Effect, by_edit: bool) -> Option<crate::Aspect> {
    crate::ceiling(effect, by_edit).map(|_| crate::Aspect::Yellow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcomes(passed: usize, failed: usize) -> Vec<bool> {
        std::iter::repeat_n(true, passed).chain(std::iter::repeat_n(false, failed)).collect()
    }

    #[test]
    fn the_corpus_numbers_are_a_significant_lift() {
        // Haiku, all sandboxes: with the lookup 40/44 passed, without 5/52.
        let l = OutcomeLift::of(&outcomes(40, 4), &outcomes(5, 47), "outcomes").unwrap();
        assert!(l.lower_bound > 0.6, "{l:?}");
        assert!(l.qualifies(4, 0.2));
        // acme-billing's training sessions: 3/3 against 1/9.
        let l = OutcomeLift::of(&outcomes(3, 0), &outcomes(1, 8), "outcomes").unwrap();
        assert!(l.lower_bound > 0.2 && l.lower_bound < 0.3, "{l:?}");
        assert!(l.qualifies(3, 0.2) && !l.qualifies(4, 0.2));
    }

    #[test]
    fn no_lift_or_noise_does_not_qualify() {
        // 3/3 against 3/6: a difference of 0.5 on so few traces is not significant.
        let l = OutcomeLift::of(&outcomes(3, 0), &outcomes(3, 3), "outcomes").unwrap();
        assert!(!l.qualifies(3, 0.2), "{l:?}");
        // The step's takers did worse.
        let l = OutcomeLift::of(&outcomes(0, 5), &outcomes(3, 3), "outcomes").unwrap();
        assert!(!l.qualifies(1, -1.0));
        // Nothing to contrast with.
        assert!(OutcomeLift::of(&outcomes(5, 0), &[], "outcomes").is_none());
    }

    #[test]
    fn wilson_matches_the_tracks_lower_bound() {
        for (s, n) in [(0, 5), (3, 3), (7, 9), (40, 44)] {
            assert!((wilson(s, n).0 - crate::wilson_lower(s, n)).abs() < 1e-9);
        }
    }
}
