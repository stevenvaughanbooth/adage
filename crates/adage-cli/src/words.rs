//! The lexicon's words for stored values (docs/lexicon.md). Stored formats keep their values;
//! people see these words.

use adage_term::Strength;
use adage_tracks::RouteState;

/// A stored strength as the terminal's strength (fire, prime, inhibit).
pub fn strength(s: adage_tracks::Strength) -> Strength {
    match s {
        adage_tracks::Strength::Fire => Strength::Fire,
        adage_tracks::Strength::Prime => Strength::Prime,
        adage_tracks::Strength::Inhibit => Strength::Inhibit,
    }
}

/// A stored route state as a lifecycle word.
pub fn lifecycle(s: RouteState) -> &'static str {
    match s {
        RouteState::Nascent => "nascent",
        RouteState::Forming => "forming",
        RouteState::Firm => "firm",
        RouteState::Extinguished => "extinguished",
        RouteState::Dormant => "dormant",
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

/// When a reflex fires, in a few words: `` `python3 -m pytest` fails with "No module named pytest" ``.
pub fn trigger(t: &adage_tracks::Trigger) -> String {
    use adage_tracks::Trigger;
    let cmd = |c: &str| c.strip_prefix("exec ").unwrap_or(c).to_string();
    match t {
        Trigger::AfterFailure { command, signature } => {
            format!("`{}` fails with \"{signature}\"", cmd(command))
        }
        Trigger::AfterCommand { command } => format!("`{}` succeeds", cmd(command)),
        Trigger::AfterEdit { paths } => format!("an edit touches {paths}"),
        Trigger::SessionStart => "a session starts".into(),
        other => crate::graph::trigger(other)["label"].as_str().unwrap_or_default().to_string(),
    }
}

/// Items with the same `key` together, groups in the order first seen: reflexes that do the same
/// thing (one instinct, one step), listed once with what sets each apart.
pub fn group<T>(items: impl IntoIterator<Item = T>, key: impl Fn(&T) -> String) -> Vec<Vec<T>> {
    let mut keys: Vec<String> = Vec::new();
    let mut out: Vec<Vec<T>> = Vec::new();
    for it in items {
        let k = key(&it);
        match keys.iter().position(|x| *x == k) {
            Some(i) => out[i].push(it),
            None => {
                keys.push(k);
                out.push(vec![it]);
            }
        }
    }
    out
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
            (r"\broute\(s\)", "reflex(es)"),
            (r"\bsession\(s\)", "trace(s)"),
            (r"\bgreen\b", "fire"),
            (r"\byellow\b", "prime"),
            (r"\bred\b", "inhibit"),
            (r"\broutes\b", "reflexes"),
            (r"\broute\b", "reflex"),
            (r"\bsessions\b", "traces"),
            (r"\bholdout\b", "control"),
            (r"\bretired\b", "extinguished"),
            (r"\bdrafted\b", "nascent"),
            (r"\bprobation\b", "forming"),
            (r"\blearned\b", "conditioned"),
            (r"\blearning\b", "conditioning"),
            (r"\brejected\b", "extinguished"),
            (r"\bdemoted\b", "weakened"),
            (r"\bpromoted\b", "strengthened"),
            (r"\bdemoting\b", "weakening"),
            (r"\bretiring\b", "extinguishing"),
            (r"\bderailed\b", "ignored"),
        ]
        .into_iter()
        .map(|(p, w)| (regex::Regex::new(p).expect("static pattern"), w))
        .collect()
    });
    // Commands (between backticks) and paths (any word with a slash) are never touched.
    let words = |t: &str| {
        t.split(' ')
            .map(|w| {
                if w.contains('/') || w.contains('\\') {
                    return w.to_string();
                }
                let mut out = w.to_string();
                for (re, rep) in subs {
                    out = re.replace_all(&out, *rep).into_owned();
                }
                out
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    s.split('`')
        .enumerate()
        .map(|(i, part)| if i % 2 == 1 { part.to_string() } else { words(part) })
        .collect::<Vec<_>>()
        .join("`")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_word() {
        for s in
            [RouteState::Nascent, RouteState::Forming, RouteState::Firm, RouteState::Extinguished, RouteState::Dormant]
        {
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
        assert_eq!(
            prose("rejected on 2026-09-01; agents derailed, consider demoting it"),
            "extinguished on 2026-09-01; agents ignored, consider weakening it"
        );
        assert_eq!(prose("3 spools in /home/me/sessions"), "3 spools in /home/me/sessions", "paths are left alone");
        assert_eq!(prose("run `adage review promote` for 2 routes"), "run `adage review promote` for 2 reflexes");
    }
}
