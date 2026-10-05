//! Proposed changes to pinned routes in `adage review` (Implementation plan §4.1).
//!
//! `adage learn` (and a re-run of `adage add`) never overwrite a pinned route; when their own
//! version of it differs materially, they record a proposal ([`adage_tracks::proposals`]). The
//! review queue lists it with the reason "proposed" and a field-by-field diff;
//! `adage review approve` applies it (the route stays pinned), and `adage review dismiss` keeps the
//! pinned route and silences the proposal until the mined version changes materially again.

use crate::share::describe_step;
use adage_tracks::proposals::Proposal;
use adage_tracks::review::{aspect_str, shape};
use adage_tracks::{Contract, Route, SlotSource, Step, Trigger};

fn slots(r: &Route) -> String {
    if r.slots.is_empty() {
        return "none".into();
    }
    r.slots
        .iter()
        .map(|s| {
            let from = match &s.source {
                SlotSource::Prompt { span } => format!("prompt {span:?}").to_lowercase(),
                SlotSource::ResultField { field } => format!("result field {field}"),
            };
            let contract = match &s.contract {
                Contract::Pattern { regex } => regex.clone(),
                Contract::RepoPath => "repository path".into(),
            };
            format!("{{{}}} from {from}, {contract}", s.name)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn evidence(r: &Route) -> String {
    let e = &r.evidence;
    format!(
        "{}/{} sessions over {} day(s) · rate {} · Wilson lower bound {} · successes {} · deviations failed {} · median position {} · turns saved {} · source {} · models {}",
        e.support, e.opportunities, e.days, e.rate, e.lower_bound, e.successes, e.deviations_failed,
        e.median_position, e.turns_saved, e.provenance, e.models.join(", ")
    )
}

/// The fields a person reads, as (label, text).
fn fields(r: &Route) -> Vec<(&'static str, String)> {
    vec![
        ("summary", r.summary.clone()),
        (
            "trigger",
            match &r.trigger {
                Trigger::Intent { family, model, centroid, threshold } => {
                    format!("prompt means {family}; model {model}; threshold {threshold}; centroid {centroid:?}")
                }
                _ => crate::graph::trigger(&r.trigger)["label"].as_str().unwrap_or_default().to_string(),
            },
        ),
        (
            "step",
            match &r.step {
                Step::Avoid { signature, .. } => format!("{}; failure signature {signature}", describe_step(&r.step)),
                _ => describe_step(&r.step),
            },
        ),
        ("slots", slots(r)),
        ("effect", r.effect.as_str().to_string()),
        ("aspect", aspect_str(r.aspect).to_string()),
        ("state", format!("{:?}", r.state).to_lowercase()),
        ("authored", r.authored.to_string()),
        ("exit", format!("{:?}", r.exit).to_lowercase()),
        ("evidence", evidence(r)),
    ]
}

/// What changes if the proposal is applied, as `(field, pinned, proposed)` for differing fields.
pub(crate) fn diff(pinned: &Route, proposed: &Route) -> Vec<(&'static str, String, String)> {
    let before = serde_json::to_value(pinned).expect("route fields serialize");
    let after = serde_json::to_value(proposed).expect("route fields serialize");
    fields(pinned)
        .into_iter()
        .zip(fields(proposed))
        .filter(|((key, _), _)| before[*key] != after[*key])
        .map(|((key, a), (_, b))| {
            // Friendly labels can omit details (intent thresholds, failure signatures, or
            // evidence fields). Never hide a change behind two identical summaries.
            if a == b {
                (key, before[key].to_string(), after[key].to_string())
            } else {
                (key, a, b)
            }
        })
        .collect()
}

/// The proposal for a route, if it still applies to the route as it stands: made against this
/// very pinned route (a hand edit since then makes it stale until the next learn run).
pub(crate) fn current<'a>(p: Option<&'a Proposal>, r: &Route) -> Option<&'a Proposal> {
    p.filter(|p| r.authored && p.against.shape == shape(r) && p.against.aspect == r.aspect)
}

/// The route that approving the proposal installs: the proposed version, still pinned.
pub(crate) fn applied(p: &Proposal) -> Route {
    let mut r = p.route.clone();
    r.authored = true;
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;
    use adage_tracks::{Aspect, Evidence, RouteState, Step, Trigger};

    #[test]
    fn diffs_only_what_changed() {
        let a = Route {
            id: "a".into(),
            summary: "s".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: Effect::Verify,
            aspect: Aspect::Yellow,
            state: RouteState::Active,
            exit: vec![],
            evidence: Evidence { support: 5, opportunities: 6, days: 2, ..Default::default() },
            authored: true,
        };
        let mut b = a.clone();
        b.step = Step::Run { command: "make check".into() };
        let d = diff(&a, &b);
        assert_eq!(d, vec![("step", "run `make test`".to_string(), "run `make check`".to_string())]);
    }
}
