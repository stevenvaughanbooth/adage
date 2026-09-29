//! The Reflexes and Map tabs' data: every installed reflex (a stored route) and instinct,
//! grouped by track, with its evidence and live counts (from the map's build, `crate::graph`)
//! and the review decisions that apply to it (from `adage review`).
//!
//! The map's JSON uses the lexicon's keys (`strength`, `ignored`, `control`); the older ones
//! (`aspect`, `derailed`, `holdout`) are read when the new ones are missing.

use super::theme::parse_strength;
use super::words;
use adage_store::Paths;
use adage_tracks::Strength;
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveCounts {
    pub emitted: u64,
    pub followed: u64,
    pub ignored: u64,
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
    pub strength: Strength,
    pub ceiling: String,
    pub effect: String,
    /// The stored state (`firm`, `nascent`, …); [`RouteRow::lifecycle`] is its word.
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
    /// What the map's JSON does not carry, read from the track itself ([`load`]).
    pub more: More,
}

/// A reflex's facts beyond the map's JSON: its uplift (and whether that keeps it quiet), the
/// evidence its strength was decided on when that is a value estimate, a finish line's
/// obligation, a procedure card's contents, and whether it was compiled from a skill.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct More {
    /// Kept quiet by its uplift: agents already take its step unaided, so it is not delivered
    /// until a person approves it. Not an install step.
    pub quiet: bool,
    /// `quiet: uplift +0.04 < 0.10 · approve to deliver` (`crate::evidence::quiet_line`).
    pub quiet_line: Option<String>,
    /// `Uplift::line`: what delivering it changes.
    pub uplift: Option<String>,
    /// `crate::evidence::describe` when the strength rests on a value estimate: a follow count
    /// (`0/7`) would say nothing about why the reflex exists.
    pub value: Option<String>,
    /// `crate::evidence::obligation`: a learned finish line is enforced at Stop.
    pub obligation: Option<String>,
    /// A procedure card, and the similarity a prompt needs to receive it.
    pub card: Option<Box<adage_tracks::card::Card>>,
    pub card_threshold: Option<f32>,
    /// Compiled from a skill (`adage train`) and accepted here.
    pub skill: bool,
}

impl RouteRow {
    pub fn lifecycle(&self) -> &str {
        words::lifecycle(&self.state)
    }

    pub fn nascent(&self) -> bool {
        self.lifecycle() == "nascent"
    }

    /// Quiet: nascent because its uplift is below the bar (not an install step).
    pub fn quiet(&self) -> bool {
        self.nascent() && (self.more.quiet || self.reasons.iter().any(|(k, _)| k == "quiet"))
    }

    /// The lifecycle as the Reflexes tab shows and filters it: `quiet` for a quiet nascent
    /// reflex, else [`RouteRow::lifecycle`].
    pub fn stage(&self) -> &str {
        if self.quiet() {
            "quiet"
        } else {
            self.lifecycle()
        }
    }

    pub fn is_card(&self) -> bool {
        self.step_kind == "card" || self.more.card.is_some()
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
        format!("{} {} {} {} {}", self.id, self.trigger, self.step, self.track, self.stage())
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

    /// Of those waiting: quiet ones (uplift below the bar), and install steps (every other
    /// nascent reflex).
    pub fn quiet(&self) -> usize {
        self.rows.iter().filter(|r| r.quiet()).count()
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
    // The first of `keys` present in `v`: the lexicon's key, then the older one.
    let either = |v: &Value, keys: [&str; 2]| -> Value {
        keys.iter().map(|k| &v[*k]).find(|x| !x.is_null()).cloned().unwrap_or(Value::Null)
    };
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
                strength: either(r, ["strength", "aspect"])
                    .as_str()
                    .and_then(parse_strength)
                    .unwrap_or(Strength::Prime),
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
                    ignored: n(&either(l, ["ignored", "derailed"])),
                    met: n(&l["obligation_met"]),
                    would_have: n(&l["would_have"]),
                    unaided: n(&either(l, ["control", "holdout"])["unaided"]),
                    not_reached: n(&either(l, ["control", "holdout"])["not_reached"]),
                },
                with: arm(&r["arms"]["treated"]),
                control: arm(&either(&r["arms"], ["control", "holdout"])),
                directive: r["directive"].as_str().unwrap_or("").to_string(),
                verbs: st.map(|s| s.verbs.clone()).unwrap_or_default(),
                reasons: st
                    .map(|s| s.reasons.iter().map(|(k, d)| (k.to_string(), d.clone())).collect())
                    .unwrap_or_default(),
                more: More::default(),
                reference,
                id,
            }
        })
        .collect();
    Routes { rows, traces: n(&g["sessions"]["total"]) }
}

/// [`More`] for one stored route.
pub fn more_of(r: &adage_tracks::Route, why: Option<&adage_tracks::why::Why>, skill: bool) -> More {
    let (card, card_threshold) = match (&r.step, &r.trigger) {
        (adage_tracks::Step::Card { card }, adage_tracks::Trigger::Intent { threshold, .. }) => {
            (Some(Box::new(card.clone())), Some(*threshold))
        }
        (adage_tracks::Step::Card { card }, _) => (Some(Box::new(card.clone())), None),
        _ => (None, None),
    };
    More {
        quiet: r.state == adage_tracks::RouteState::Nascent && crate::evidence::quiet_line(r).is_some(),
        quiet_line: crate::evidence::quiet_line(r),
        uplift: r.evidence.uplift.as_ref().map(|u| u.line()),
        value: crate::evidence::value_line(r, why),
        obligation: crate::evidence::obligation(r),
        card,
        card_threshold,
        skill,
    }
}

pub fn load(paths: &Paths) -> Result<Routes> {
    let g = crate::graph::build(paths, 0)?;
    let standings = crate::review::standings(paths)?;
    let installed = crate::review::discover(&paths.tracks())?;
    let learned = format!("/{}", adage_learn::TRACK);
    let mut more: std::collections::BTreeMap<(String, String), More> = Default::default();
    for i in &installed {
        let whys = adage_tracks::why::load(&i.dir);
        let from_skills = !i.track.manifest.track.name.ends_with(&learned);
        for r in &i.track.routes {
            let skill = from_skills && r.evidence.origin == Some(adage_tracks::Origin::Authored);
            more.insert((i.reference.clone(), r.id.clone()), more_of(r, whys.get(&r.id), skill));
        }
    }
    let instincts: BTreeSet<(String, String)> = installed
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
    let mut routes = from_graph(&g, &standings, &instincts);
    for row in &mut routes.rows {
        if let Some(m) = more.remove(&(row.reference.clone(), row.id.clone())) {
            row.more = m;
        }
    }
    Ok(routes)
}

/// A review decision and its key in the Reflexes tab: `(key, verb crate::review takes)`.
pub const ACTIONS: [(char, &str); 6] =
    [('s', "strengthen"), ('w', "weaken"), ('x', "extinguish"), ('a', "approve"), ('n', "pin"), ('m', "dismiss")];
