//! `learn.engine = shadow` (Track G G3): the miners' routes are learned and written as usual; the
//! graph engine's are written to `<repo>/.shadow/<track>` (hidden, so never compiled into the
//! index), and this report compares the two by route id: routes only the miners learned, routes
//! only the graph engine learned, and routes both learned that differ in what they say (trigger,
//! step, slots, aspect or state). Evidence counts are not compared: the engines count differently,
//! and only what would reach the agent matters here. `adage learn --engine shadow` prints it and
//! writes it to `<repo>/.shadow/shadow-report.json`.

use adage_tracks::{Route, RouteState};
use serde::Serialize;
use std::collections::BTreeMap;

pub const REPORT_FILE: &str = "shadow-report.json";

/// One route, as the report shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Seen {
    pub id: String,
    pub trigger: serde_json::Value,
    pub step: serde_json::Value,
    pub aspect: String,
    pub state: String,
}

fn state_word(s: RouteState) -> &'static str {
    // docs/lexicon.md
    match s {
        RouteState::Draft => "nascent",
        RouteState::Probation => "forming",
        RouteState::Active => "firm",
        RouteState::Demoted => "demoted",
        RouteState::Retired => "extinguished",
        RouteState::Stale => "dormant",
    }
}

fn aspect_word(a: adage_tracks::Aspect) -> &'static str {
    match a {
        adage_tracks::Aspect::Green => "fire",
        adage_tracks::Aspect::Yellow => "prime",
        adage_tracks::Aspect::Red => "inhibit",
    }
}

fn seen(r: &Route) -> Seen {
    let mut trigger = serde_json::to_value(&r.trigger).unwrap_or_default();
    // An intent centroid is hundreds of numbers; its family says which it is.
    if let Some(o) = trigger.as_object_mut() {
        o.remove("centroid");
    }
    Seen {
        id: r.id.clone(),
        trigger,
        step: serde_json::to_value(&r.step).unwrap_or_default(),
        aspect: aspect_word(r.aspect).into(),
        state: state_word(r.state).into(),
    }
}

/// A reflex both engines learned, and what differs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Differing {
    pub id: String,
    /// The fields that differ: `trigger`, `step`, `slots`, `aspect`, `state`.
    pub fields: Vec<String>,
    pub miners: Seen,
    pub graph: Seen,
}

/// The shadow report for one repository.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ShadowReport {
    pub miners: usize,
    pub graph: usize,
    pub same: usize,
    pub only_miners: Vec<Seen>,
    pub only_graph: Vec<Seen>,
    pub differing: Vec<Differing>,
}

/// Compare the miners' routes with the graph engine's (both as planned for one repository).
pub fn diff(miners: &[&Route], graph: &[&Route]) -> ShadowReport {
    let m: BTreeMap<&str, &Route> = miners.iter().map(|r| (r.id.as_str(), *r)).collect();
    let g: BTreeMap<&str, &Route> = graph.iter().map(|r| (r.id.as_str(), *r)).collect();
    let mut rep = ShadowReport { miners: m.len(), graph: g.len(), ..Default::default() };
    for (id, a) in &m {
        match g.get(id) {
            None => rep.only_miners.push(seen(a)),
            Some(b) => {
                let mut fields = Vec::new();
                let intent_free = |r: &Route| {
                    let mut t = serde_json::to_value(&r.trigger).unwrap_or_default();
                    if let Some(o) = t.as_object_mut() {
                        o.remove("centroid");
                        o.remove("threshold");
                    }
                    t
                };
                if intent_free(a) != intent_free(b) {
                    fields.push("trigger".to_string());
                }
                if a.step != b.step {
                    fields.push("step".into());
                }
                if a.slots != b.slots {
                    fields.push("slots".into());
                }
                if a.aspect != b.aspect {
                    fields.push("aspect".into());
                }
                if a.state != b.state {
                    fields.push("state".into());
                }
                if fields.is_empty() {
                    rep.same += 1;
                } else {
                    rep.differing.push(Differing { id: id.to_string(), fields, miners: seen(a), graph: seen(b) });
                }
            }
        }
    }
    rep.only_graph = g.iter().filter(|(id, _)| !m.contains_key(*id)).map(|(_, r)| seen(r)).collect();
    rep
}

/// The report as text (`adage learn --engine shadow`).
pub fn render(repo: &str, r: &ShadowReport) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "  shadow ({repo}): miners {} reflexes, graph engine {}; {} the same, {} differ, {} only from the miners, {} only from the graph engine",
        r.miners,
        r.graph,
        r.same,
        r.differing.len(),
        r.only_miners.len(),
        r.only_graph.len()
    );
    for d in &r.differing {
        let _ = writeln!(s, "    differs  {} ({})", d.id, d.fields.join(", "));
    }
    for x in &r.only_miners {
        let _ = writeln!(s, "    miners   {} [{}, {}]", x.id, x.aspect, x.state);
    }
    for x in &r.only_graph {
        let _ = writeln!(s, "    graph    {} [{}, {}]", x.id, x.aspect, x.state);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;
    use adage_tracks::*;

    fn r(id: &str, cmd: &str, aspect: Aspect) -> Route {
        Route {
            id: id.into(),
            summary: String::new(),
            trigger: Trigger::AfterEdit { paths: "schema/*.json".into() },
            step: Step::Run { command: cmd.into() },
            slots: vec![],
            effect: Effect::Regenerate,
            aspect,
            state: RouteState::Active,
            exit: vec![],
            evidence: Evidence::default(),
            authored: false,
        }
    }

    #[test]
    fn diffs_by_id() {
        let (a, b, c) =
            (r("x", "make gen", Aspect::Green), r("x", "make gen", Aspect::Yellow), r("y", "make a", Aspect::Green));
        let d = diff(&[&a, &c], &[&b]);
        assert_eq!((d.miners, d.graph, d.same), (2, 1, 0));
        assert_eq!(d.differing[0].fields, ["aspect"]);
        assert_eq!(d.only_miners[0].id, "y");
        assert!(d.only_graph.is_empty());
        assert!(render("acme", &d).contains("1 differ"));
    }
}
