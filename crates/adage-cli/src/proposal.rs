//! Proposed changes to pinned routes in `adage review` (Implementation plan §4.1).
//!
//! `adage condition` (and a re-run of `adage add`) never overwrite a pinned route; when their own
//! version of it differs materially, they record a proposal ([`adage_tracks::proposals`]). The
//! review queue lists it with the reason "proposed" and a field-by-field diff;
//! `adage review approve` applies it (the route stays pinned), and `adage review dismiss` keeps the
//! pinned route and silences the proposal until the mined version changes materially again.

use crate::share::describe_step;
use adage_tracks::review::strength_str;
use adage_tracks::{Contract, Route, SlotSource};

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
                SlotSource::PromptValue { vtype, .. } => format!("prompt value of type {}", vtype.id),
                SlotSource::OutputValue { field: Some(f), vtype } => {
                    format!("output field {f} of type {}", vtype.id)
                }
                SlotSource::OutputValue { field: None, vtype } => format!("output value of type {}", vtype.id),
                SlotSource::EditSegment { glob, depth } => format!("segment {depth} of the edited {glob}"),
                SlotSource::EditTests { glob, k, map } => {
                    format!("at most {k} tests of the edited {glob} (a map of {} files)", map.len())
                }
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
        "{}/{} traces over {} day(s) · Wilson lower bound {}",
        e.support,
        e.opportunities,
        e.days,
        crate::evidence::num(e.lower_bound)
    )
}

/// The fields a person reads, as (label, text).
fn fields(r: &Route) -> Vec<(&'static str, String)> {
    vec![
        ("trigger", adage_app::graph::trigger(&r.trigger)["label"].as_str().unwrap_or_default().to_string()),
        ("step", describe_step(&r.step)),
        ("slots", slots(r)),
        ("effect", r.effect.as_str().to_string()),
        ("strength", strength_str(r.strength).to_string()),
        ("exit", format!("{:?}", r.exit).to_lowercase()),
        ("evidence", evidence(r)),
    ]
}

/// What changes if the proposal is applied, as `(field, pinned, proposed)` for differing fields.
pub(crate) fn diff(pinned: &Route, proposed: &Route) -> Vec<(&'static str, String, String)> {
    fields(pinned)
        .into_iter()
        .zip(fields(proposed))
        .filter(|((_, a), (_, b))| a != b)
        .map(|((k, a), (_, b))| (k, a, b))
        .collect()
}

pub(crate) use adage_app::proposal::applied;

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;
    use adage_tracks::{Evidence, RouteState, Step, Strength, Trigger};

    #[test]
    fn diffs_only_what_changed() {
        let a = Route {
            id: "a".into(),
            summary: "s".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: Effect::Verify,
            strength: Strength::Prime,
            state: RouteState::Firm,
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
