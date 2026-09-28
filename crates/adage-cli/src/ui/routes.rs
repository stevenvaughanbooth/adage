//! The Reflexes and Map tabs' data: every installed reflex (a stored route) and instinct,
//! grouped by track, with its evidence and live counts (from the map's build, `crate::graph`)
//! and the review decisions that apply to it (from `adage review`).

use super::theme::parse_aspect;
use super::words;
use adage_store::Paths;
use adage_tracks::Aspect;
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeSet;

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

/// One arm's distinct traces: where the reflex applied, and where the agent took its step.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Arm {
    pub applied: u64,
    pub took: u64,
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
    /// Ships with Adage (an instinct), rather than conditioned from traces.
    pub instinct: bool,
    pub id: String,
    pub aspect: Aspect,
    pub ceiling: String,
    pub effect: String,
    /// The stored state (`active`, `draft`, …); [`RouteRow::lifecycle`] is its word.
    pub state: String,
    pub authored: bool,
    /// `prompt`, `after_command`, `after_failure`, `after_edit`, `session_start`, `intent`.
    pub trigger_kind: String,
    pub trigger: String,
    /// `run`, `read_batch`, `avoid`, `advise`.
    pub step_kind: String,
    pub step: String,
    pub support: u64,
    pub opportunities: u64,
    pub rate: f64,
    pub lower_bound: f64,
    pub turns_saved: f64,
    pub live: LiveCounts,
    /// Treated traces, and control traces (reflexes withheld).
    pub with: Arm,
    pub control: Arm,
    pub directive: String,
    /// Decisions that apply (`crate::review`'s verbs).
    pub verbs: Vec<&'static str>,
    /// Why it awaits review; empty if it does not.
    pub reasons: Vec<(String, String)>,
}

impl RouteRow {
    pub fn lifecycle(&self) -> &str {
        words::lifecycle(&self.state)
    }

    pub fn nascent(&self) -> bool {
        self.lifecycle() == "nascent"
    }

    pub fn extinguished(&self) -> bool {
        self.lifecycle() == "extinguished"
    }

    /// Evidence as a count: `23/33`, or `–` for an instinct with none of its own.
    pub fn count(&self) -> String {
        if self.opportunities == 0 {
            "–".into()
        } else {
            format!("{}/{}", self.support, self.opportunities)
        }
    }

    /// Text for the fuzzy filter.
    pub fn haystack(&self) -> String {
        format!("{} {} {} {} {}", self.id, self.trigger, self.step, self.track, self.lifecycle())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Routes {
    pub rows: Vec<RouteRow>,
    /// Traces recorded (every spool).
    pub traces: u64,
}

impl Routes {
    pub fn waiting(&self) -> usize {
        self.rows.iter().filter(|r| r.nascent() || !r.reasons.is_empty()).count()
    }
}

fn step_label(s: &Value) -> String {
    let label = s["label"].as_str().unwrap_or("");
    match s["kind"].as_str() {
        Some("avoid") => format!("{label} (not {})", s["avoid"].as_str().unwrap_or("?")),
        _ => label.to_string(),
    }
}

pub fn from_graph(
    g: &Value,
    standings: &std::collections::BTreeMap<(String, String), crate::review::Standing>,
    instincts: &BTreeSet<(String, String)>,
) -> Routes {
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let arm = |a: &Value| Arm { applied: n(&a["applied"]), took: n(&a["followed"]) + n(&a["met"]) };
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
                instinct: instincts.contains(&(reference.clone(), id.clone())),
                aspect: r["aspect"].as_str().and_then(parse_aspect).unwrap_or(Aspect::Yellow),
                ceiling: r["ceiling"].as_str().unwrap_or("").to_string(),
                effect: r["effect"].as_str().unwrap_or("").to_string(),
                state: r["state"].as_str().unwrap_or("").to_string(),
                authored: r["authored"].as_bool().unwrap_or(false),
                trigger_kind: r["trigger"]["kind"].as_str().unwrap_or("").to_string(),
                trigger: r["trigger"]["label"].as_str().unwrap_or("").to_string(),
                step_kind: r["step"]["kind"].as_str().unwrap_or("").to_string(),
                step: step_label(&r["step"]),
                support: n(&e["support"]),
                opportunities: n(&e["opportunities"]),
                rate: e["rate"].as_f64().unwrap_or(0.0),
                lower_bound: e["lower_bound"].as_f64().unwrap_or(0.0),
                turns_saved: e["turns_saved"].as_f64().unwrap_or(0.0),
                live: LiveCounts {
                    emitted: n(&l["emitted"]),
                    followed: n(&l["followed"]),
                    derailed: n(&l["derailed"]),
                    met: n(&l["obligation_met"]),
                    would_have: n(&l["would_have"]),
                    unaided: n(&l["holdout"]["unaided"]),
                    not_reached: n(&l["holdout"]["not_reached"]),
                },
                with: arm(&r["arms"]["treated"]),
                control: arm(&r["arms"]["holdout"]),
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
    Routes { rows, traces: n(&g["sessions"]["total"]) }
}

pub fn load(paths: &Paths) -> Result<Routes> {
    let g = crate::graph::build(paths, 0)?;
    let standings = crate::review::standings(paths)?;
    let instincts: BTreeSet<(String, String)> = crate::review::discover(&paths.tracks())?
        .iter()
        .flat_map(|i| {
            let whole = adage_tracks::index::is_instinct_track(&i.track);
            i.track
                .routes
                .iter()
                .filter(move |r| whole || r.is_instinct())
                .map(|r| (i.reference.clone(), r.id.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    Ok(from_graph(&g, &standings, &instincts))
}

/// A review decision and its key in the Reflexes tab: `(key, verb crate::review records)`. The
/// person sees [`words::verb`] of it.
pub const ACTIONS: [(char, &str); 6] =
    [('s', "promote"), ('w', "demote"), ('x', "reject"), ('a', "approve"), ('n', "pin"), ('m', "dismiss")];
