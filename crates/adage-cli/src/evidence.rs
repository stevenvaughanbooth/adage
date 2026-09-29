//! A reflex's evidence, as a person reads it (`adage status`, `adage review`): the evidence its
//! strength was actually decided on, not a count that played no part.
//!
//! Most reflexes form on how often traces did what they direct after the trigger: `support` of
//! `opportunities` traces, and the Wilson lower bound of that rate. Some form on **value** instead
//! (`adage_learn::value`, `adage_learn::testmap`): the calls saved when the step is taken, estimated
//! from the traces that took it and those that did not. For those, few or no traces took the step
//! (a *desire path*), so a follow rate of `0/7` says nothing about why the reflex exists; the value
//! estimate does. Its `.why.json` entry records it (`gain` = calls saved, `conf` = P(saves),
//! `conf_lb` = its one-sided 90% lower bound), and that is what is shown.
//!
//! A finish line (a learned reflex run after an edit) is also an **obligation**: its stop check
//! asks the agent to run the step before finishing if it has not. That is said beside it, because
//! it is the one thing a prime reflex does beyond advising.

use adage_tracks::why::Why;
use adage_tracks::{Route, Step, Trigger};

/// Two decimals, never `-0.00`.
pub fn num(x: f64) -> String {
    let x = if x.abs() < 0.005 { 0.0 } else { x };
    format!("{x:.2}")
}

/// Was this reflex's strength decided on its value estimate (what its `.why.json` records)?
fn valued(why: Option<&Why>) -> Option<(&Why, &'static str)> {
    let w = why?;
    if w.pattern.contains("=tests_of=>") {
        Some((w, "ran it"))
    } else if w.pattern.contains("=value=>") {
        Some((w, "took it"))
    } else {
        None
    }
}

/// [`describe`]'s line when the strength was decided on value (a follow count would mislead),
/// else `None`: the caller shows its own counts.
pub fn value_line(r: &Route, why: Option<&Why>) -> Option<String> {
    (!r.is_instinct() && valued(why).is_some()).then(|| describe(r, why))
}

/// Why a reflex is quiet, in one line, when it is: `quiet: uplift +0.04 < 0.10 · approve to
/// deliver`. Quiet means its uplift is below the bar, so it waits for a person rather than being
/// delivered; it is not an install step.
pub fn quiet_line(r: &Route) -> Option<String> {
    let u = r.evidence.uplift.as_ref().filter(|u| u.gated)?;
    Some(format!(
        "quiet: uplift {:+.2} < {:.2} · approve to deliver",
        if u.mean.abs() < 0.005 { 0.0 } else { u.mean },
        adage_tracks::params::UPLIFT_MIN
    ))
}

/// The evidence behind a reflex's strength, in one line.
pub fn describe(r: &Route, why: Option<&Why>) -> String {
    let e = &r.evidence;
    if r.is_instinct() {
        let judged: Vec<String> = ["followed", "ignored", "misfired"]
            .iter()
            .filter_map(|k| e.by_origin.get(*k).filter(|n| **n > 0).map(|n| format!("{n} {k}")))
            .collect();
        let judged = if judged.is_empty() { String::new() } else { format!("; {}", judged.join(", ")) };
        return format!("instinct: bound from the repository's own files{judged}");
    }
    if let Some((w, did)) = valued(why) {
        let path = if e.support == 0 {
            " — a desire path: no trace took the step yet, so its strength rests on the value estimate".to_string()
        } else {
            String::new()
        };
        return format!(
            "value: saves ~{} calls when followed (P(saves) {}, 90% lower bound {}); {} triggers, {} {did}{path}",
            num(w.gain),
            num(w.conf),
            num(w.conf_lb),
            e.opportunities,
            e.support
        );
    }
    format!(
        "{}/{} traces over {} day(s) · rate {} · Wilson lower bound {}",
        e.support,
        e.opportunities,
        e.days,
        num(e.rate),
        num(e.lower_bound)
    )
}

/// What a reflex does beyond advising, when it does: a learned finish line is enforced at Stop.
pub fn obligation(r: &Route) -> Option<String> {
    if r.is_instinct() {
        return None; // an instinct's finish line is advice only
    }
    match (&r.trigger, &r.step) {
        (Trigger::AfterEdit { paths }, Step::Run { command }) => Some(format!(
            "finish line: after an edit to {paths} the agent is told to run `{command}`; if it then stops without running it, Adage asks it once to run it before finishing (at most {} such requests a session)",
            adage_tracks::params::MAX_STOP_REQUESTS
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;
    use adage_tracks::{Evidence, RouteState, Strength};

    fn route(trigger: Trigger, support: u32, opportunities: u32) -> Route {
        Route {
            id: "run-3e72af".into(),
            summary: String::new(),
            trigger,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: Effect::Verify,
            strength: Strength::Prime,
            state: RouteState::Firm,
            exit: vec![],
            evidence: Evidence { support, opportunities, lower_bound: -0.0001, ..Default::default() },
            authored: false,
        }
    }

    fn why(pattern: &str) -> Why {
        Why {
            shape: "run".into(),
            pattern: pattern.into(),
            backoff: 1,
            gain: 1.25,
            lift: 0.0,
            conf: 0.93,
            conf_lb: 0.4,
            support: 0,
            days: 0,
            members: vec![],
            examples: vec![],
            prior: None,
        }
    }

    #[test]
    fn a_valued_reflex_shows_its_value_not_a_zero_follow_rate() {
        let r = route(Trigger::AfterEdit { paths: "src/*.py".into() }, 0, 7);
        let w = why("[edit src/*.py] =tests_of=> [pytest {tests}]: A 1.25 calls; 7 triggers, 0 ran it");
        let line = describe(&r, Some(&w));
        assert!(line.starts_with("value: saves ~1.25 calls when followed (P(saves) 0.93"), "{line}");
        assert!(line.contains("7 triggers, 0 ran it") && line.contains("desire path"), "{line}");
        assert!(!line.contains("lower bound -0.00") && !line.contains("0/7"), "{line}");
        // Without its reasons, the counts, and never a negative zero.
        let plain = describe(&r, None);
        assert!(plain.starts_with("0/7 traces") && plain.contains("lower bound 0.00"), "{plain}");
        assert_eq!(num(-0.001), "0.00");
        // A learned finish line says it is enforced at Stop.
        assert!(obligation(&r).is_some_and(|o| o.contains("asks it once")));
        assert!(obligation(&route(Trigger::SessionStart, 1, 1)).is_none());
        // Only a valued reflex replaces its counts.
        assert!(value_line(&r, Some(&w)).is_some_and(|l| l.starts_with("value: saves ~1.25")));
        assert!(value_line(&r, None).is_none());
    }

    #[test]
    fn a_quiet_reflex_says_its_uplift_and_the_bar() {
        let mut r = route(Trigger::SessionStart, 5, 6);
        assert!(quiet_line(&r).is_none());
        r.evidence.uplift = Some(adage_tracks::Uplift { mean: 0.04, gated: true, ..Default::default() });
        assert_eq!(quiet_line(&r).unwrap(), "quiet: uplift +0.04 < 0.10 · approve to deliver");
        r.evidence.uplift.as_mut().unwrap().gated = false;
        assert!(quiet_line(&r).is_none(), "above the bar it speaks");
    }
}
