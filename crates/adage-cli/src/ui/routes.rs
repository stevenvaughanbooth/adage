//! The Routes tab's data: every installed route, grouped by track, with its evidence and live
//! counts (from `adage graph`'s build) and the review decisions that apply to it (from `adage
//! review`).

use super::theme::parse_aspect;
use adage_store::Paths;
use adage_tracks::Aspect;
use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveCounts {
    pub emitted: u64,
    pub followed: u64,
    pub derailed: u64,
    pub met: u64,
    pub would_have: u64,
    pub unaided: u64,
    pub not_reached: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteRow {
    /// The track's directory under the tracks directory: what `adage review` takes.
    pub reference: String,
    pub track: String,
    pub version: String,
    pub repo: String,
    pub imported: bool,
    pub in_index: bool,
    pub id: String,
    pub aspect: Aspect,
    pub ceiling: String,
    pub effect: String,
    pub state: String,
    pub authored: bool,
    pub trigger: String,
    pub step: String,
    pub support: u64,
    pub opportunities: u64,
    pub rate: f64,
    pub lower_bound: f64,
    pub live: LiveCounts,
    pub directive: String,
    /// Decisions that apply (`adage review <verb>`).
    pub verbs: Vec<&'static str>,
    /// Why it awaits review; empty if it does not.
    pub reasons: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct Routes {
    pub rows: Vec<RouteRow>,
}

fn step_label(s: &Value) -> String {
    let label = s["label"].as_str().unwrap_or("");
    match s["kind"].as_str() {
        Some("run") => format!("run `{label}`"),
        Some("avoid") => format!("avoid `{}`, use `{label}`", s["avoid"].as_str().unwrap_or("?")),
        _ => label.to_string(),
    }
}

pub fn from_graph(
    g: &Value,
    standings: &std::collections::BTreeMap<(String, String), crate::review::Standing>,
) -> Routes {
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let rows = g["routes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let reference = r["reference"].as_str().unwrap_or("").to_string();
            let id = r["id"].as_str().unwrap_or("").to_string();
            let st = standings.get(&(reference.clone(), id.clone()));
            let (l, e) = (&r["live"], &r["evidence"]);
            RouteRow {
                track: r["track"].as_str().unwrap_or("").to_string(),
                version: r["version"].as_str().unwrap_or("").to_string(),
                repo: r["repo"].as_str().unwrap_or("").to_string(),
                imported: r["imported"].as_bool().unwrap_or(false),
                in_index: r["in_index"].as_bool().unwrap_or(false),
                aspect: r["aspect"].as_str().and_then(parse_aspect).unwrap_or(Aspect::Yellow),
                ceiling: r["ceiling"].as_str().unwrap_or("").to_string(),
                effect: r["effect"].as_str().unwrap_or("").to_string(),
                state: r["state"].as_str().unwrap_or("").to_string(),
                authored: r["authored"].as_bool().unwrap_or(false),
                trigger: r["trigger"]["label"].as_str().unwrap_or("").to_string(),
                step: step_label(&r["step"]),
                support: n(&e["support"]),
                opportunities: n(&e["opportunities"]),
                rate: e["rate"].as_f64().unwrap_or(0.0),
                lower_bound: e["lower_bound"].as_f64().unwrap_or(0.0),
                live: LiveCounts {
                    emitted: n(&l["emitted"]),
                    followed: n(&l["followed"]),
                    derailed: n(&l["derailed"]),
                    met: n(&l["obligation_met"]),
                    would_have: n(&l["would_have"]),
                    unaided: n(&l["holdout"]["unaided"]),
                    not_reached: n(&l["holdout"]["not_reached"]),
                },
                directive: r["directive"].as_str().unwrap_or("").to_string(),
                verbs: st.map(|s| s.verbs.clone()).unwrap_or_default(),
                reasons: st
                    .map(|s| s.reasons.iter().map(|(k, d)| (k.to_string(), d.clone())).collect())
                    .unwrap_or_default(),
                reference,
                id,
            }
        })
        .collect();
    Routes { rows }
}

pub fn load(paths: &Paths) -> Result<Routes> {
    let g = crate::graph::build(paths, 0)?;
    let standings = crate::review::standings(paths)?;
    Ok(from_graph(&g, &standings))
}

/// A review verb and its key in the Routes tab.
pub const ACTIONS: [(char, &str); 6] =
    [('a', "approve"), ('r', "reject"), ('d', "demote"), ('p', "promote"), ('n', "pin"), ('x', "dismiss")];
