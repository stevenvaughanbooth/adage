//! Review decisions (Design §10.4, §10.9): what a person decided about each route, stored beside
//! the routes in `review.yaml` so decisions travel with the track and survive re-learning.
//!
//! A decision records the route's **basis** when it was made: a hash of its shape, its aspect,
//! and its evidence. Later changes are judged against that basis, never against the previous
//! learn run, so slow drift accumulates until it becomes material.
//!
//! **Material change** (precise): relative to the basis, any of
//! - the shape changed: trigger, step, slots, effect, or exit conditions;
//! - the aspect changed;
//! - the Wilson lower bound moved by at least [`MATERIAL_LOWER_BOUND`] in either direction;
//! - distinct supporting sessions at least doubled, or at least halved.
//!
//! A **rejected** route comes back only if it *strengthened* materially: its shape changed, its
//! lower bound rose by at least [`MATERIAL_LOWER_BOUND`], or its support at least doubled.

use crate::model::*;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE: &str = "review.yaml";
/// Absolute movement of the Wilson lower bound that counts as material.
pub const MATERIAL_LOWER_BOUND: f64 = 0.10;
/// Support growing (or shrinking) by this factor counts as material.
pub const MATERIAL_SUPPORT_FACTOR: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Accepted as it stands; learning continues to maintain it automatically.
    Approved,
    /// Retired by a person; learn keeps it retired until it strengthens materially.
    Rejected,
    /// Held at yellow by a person; learn never raises it above that on its own.
    Demoted,
    /// Raised to green by a person, within its effect ceiling; held while evidence is unchanged.
    Promoted,
    /// Edit-locked (`authored`); learn never overwrites it.
    Pinned,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Approved => "approved",
            Decision::Rejected => "rejected",
            Decision::Demoted => "demoted",
            Decision::Promoted => "promoted",
            Decision::Pinned => "pinned",
        }
    }
}

/// The route as it stood when a person decided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Basis {
    /// [`shape`] of the route.
    pub shape: String,
    pub aspect: Aspect,
    pub support: u32,
    pub opportunities: u32,
    pub lower_bound: f64,
    /// Runtime judgements (treated sessions) counted when the decision was made, so later
    /// demotion signals count only what happened since.
    #[serde(default)]
    pub followed: u32,
    #[serde(default)]
    pub derailed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub decision: Decision,
    /// UTC calendar date of the decision, `YYYY-MM-DD`.
    pub on: String,
    /// Who decided — only what the person chose to give with `--by`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    pub basis: Basis,
}

/// `review.yaml`: the latest decision per route id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reviews {
    #[serde(default)]
    pub routes: BTreeMap<String, Review>,
}

/// A stable hash of what a route directs and when: trigger, step, slots, effect, exit.
pub fn shape(r: &Route) -> String {
    let body = serde_json::to_vec(&(&r.trigger, &r.step, &r.slots, &r.effect, &r.exit)).unwrap_or_default();
    crate::io::hex(&Sha256::digest(&body))[..16].to_string()
}

pub fn basis(r: &Route, followed: u32, derailed: u32) -> Basis {
    Basis {
        shape: shape(r),
        aspect: r.aspect,
        support: r.evidence.support,
        opportunities: r.evidence.opportunities,
        lower_bound: (r.evidence.lower_bound * 1e4).round() / 1e4,
        followed,
        derailed,
    }
}

/// Why the evidence alone changed materially since `b`, if it did.
fn evidence_change(b: &Basis, r: &Route) -> Option<String> {
    let e = &r.evidence;
    if (e.lower_bound - b.lower_bound).abs() >= MATERIAL_LOWER_BOUND - 1e-9 {
        return Some(format!("lower bound moved from {:.2} to {:.2}", b.lower_bound, e.lower_bound));
    }
    if e.support >= MATERIAL_SUPPORT_FACTOR * b.support.max(1) {
        return Some(format!("support grew from {} to {} sessions", b.support, e.support));
    }
    if e.support * MATERIAL_SUPPORT_FACTOR <= b.support {
        return Some(format!("support fell from {} to {} sessions", b.support, e.support));
    }
    None
}

/// Why `r` changed materially since the decision recorded in `b`, if it did.
pub fn material_change(b: &Basis, r: &Route) -> Option<String> {
    if shape(r) != b.shape {
        return Some("its trigger, step, slots, effect, or exit changed".into());
    }
    if r.aspect != b.aspect {
        return Some(format!("aspect changed from {} to {}", aspect_str(b.aspect), aspect_str(r.aspect)));
    }
    evidence_change(b, r)
}

/// Whether a rejected route has strengthened enough to be proposed again.
pub fn strengthened(b: &Basis, r: &Route) -> bool {
    shape(r) != b.shape
        || r.evidence.lower_bound >= b.lower_bound + MATERIAL_LOWER_BOUND - 1e-9
        || r.evidence.support >= MATERIAL_SUPPORT_FACTOR * b.support.max(1)
}

pub fn aspect_str(a: Aspect) -> &'static str {
    match a {
        Aspect::Green => "green",
        Aspect::Yellow => "yellow",
        Aspect::Red => "red",
    }
}

fn safe_by(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && !s.chars().any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
}

fn safe_date(s: &str) -> bool {
    s.len() == 10 && s.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() })
}

pub fn validate(r: &Reviews) -> Result<()> {
    for (id, v) in &r.routes {
        if id.is_empty()
            || id.len() > 64
            || !id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            bail!("{FILE}: route id {id:?} is not allowed");
        }
        if !safe_date(&v.on) {
            bail!("{FILE}: {id}: date {:?} is not YYYY-MM-DD", v.on);
        }
        if v.by.as_deref().is_some_and(|b| !safe_by(b)) {
            bail!("{FILE}: {id}: `by` must be 1-64 printable characters");
        }
        if !v.basis.lower_bound.is_finite() || v.basis.shape.len() > 64 {
            bail!("{FILE}: {id}: malformed basis");
        }
    }
    Ok(())
}

/// Check a `--by` value before it is recorded.
pub fn check_by(s: &str) -> Result<()> {
    if !safe_by(s) {
        bail!("--by must be 1-64 printable characters");
    }
    Ok(())
}

/// The decisions stored in a track directory; none if it has no `review.yaml`.
pub fn load(dir: &Path) -> Result<Reviews> {
    let p = dir.join(FILE);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Reviews::default()),
        Err(e) => return Err(e).with_context(|| p.display().to_string()),
    };
    let r: Reviews = serde_yaml::from_str(&text).with_context(|| p.display().to_string())?;
    validate(&r)?;
    Ok(r)
}

/// Write `review.yaml` atomically (temp file + rename); an empty set removes the file.
pub fn save(dir: &Path, r: &Reviews) -> Result<()> {
    validate(r)?;
    let p = dir.join(FILE);
    if r.routes.is_empty() {
        return match std::fs::remove_file(&p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        };
    }
    let tmp = dir.join(format!(".{FILE}.tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_yaml::to_string(r)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

/// Apply recorded decisions to freshly mined routes, so re-learning respects them.
///
/// `prev` is the track as it was before this learn run; `mined` holds the miner's routes plus any
/// authored routes already carried over (those are never touched here).
///
/// - **Rejected:** a re-mined route stays retired unless it [`strengthened`]; then it returns at
///   probation, yellow at most, and waits in the queue again. A rejected route the miner no longer
///   finds is kept, retired, so the decision stays visible and travels.
/// - **Demoted:** held at yellow, whatever the evidence says.
/// - **Promoted:** held at the promoted aspect while its shape and evidence are materially
///   unchanged and its effect ceiling allows; otherwise learning decides and the queue shows why.
/// - **Approved:** an environment route, which the miner learns as a draft (O11), is put into
///   service while its shape is the one approved. Otherwise nothing to hold.
/// - **Pinned:** nothing to hold (pinned routes are `authored` and carried over as is).
///
/// Losing authority stays automatic: a reviewed route the miner no longer finds is dropped like
/// any other, and its decision re-applies if it comes back.
pub fn reconcile(prev: Option<&Track>, reviews: &Reviews, mut mined: Vec<Route>) -> Vec<Route> {
    for (id, rv) in &reviews.routes {
        let authored = |r: &Route| r.id == *id && r.authored;
        if mined.iter().any(authored) {
            continue;
        }
        let Some(m) = mined.iter_mut().find(|r| r.id == *id) else {
            if rv.decision == Decision::Rejected {
                if let Some(p) =
                    prev.and_then(|p| p.routes.iter().find(|r| r.id == *id && r.state == RouteState::Retired))
                {
                    mined.push(p.clone());
                }
            }
            continue;
        };
        match rv.decision {
            Decision::Rejected if strengthened(&rv.basis, m) => {
                m.state = RouteState::Probation;
                if m.aspect == Aspect::Green {
                    m.aspect = Aspect::Yellow;
                }
            }
            Decision::Rejected => m.state = RouteState::Retired,
            Decision::Demoted if m.aspect == Aspect::Green => m.aspect = Aspect::Yellow,
            // An environment step waits as a draft until a person approves it (O11). Approval holds
            // for the step that was approved: a different command waits again.
            Decision::Approved if m.state == RouteState::Draft && shape(m) == rv.basis.shape => {
                m.state = RouteState::Active
            }
            Decision::Promoted
                if m.aspect == Aspect::Yellow
                    && shape(m) == rv.basis.shape
                    && evidence_change(&rv.basis, m).is_none()
                    && ceiling(m.effect, matches!(m.trigger, Trigger::AfterEdit { .. })) == Some(rv.basis.aspect) =>
            {
                m.aspect = rv.basis.aspect;
            }
            _ => {}
        }
    }
    mined
}

/// Keep, as [`RouteState::Stale`], the routes of `prev` that learning no longer finds only because
/// of their own exposure (E1: evidence integrity).
///
/// Each miner ignores sessions steered by its own kind of route (Design §11), so a route that is
/// delivered, and followed, starves itself of evidence: once the transcripts from before it expire,
/// only the holdout is left to confirm it. `routes` is the track as reconciled from this run;
/// `verdict` is asked about each route of `prev` that was in service (active, probation or already
/// stale), is not in `routes`, and was not rejected by a person. It answers `Some(exposed)` when
/// the route's absence is explained by exposure and nothing unsteered contradicts it
/// (`adage_learn` decides), and `None` to drop it as before.
///
/// A kept route keeps its last evidence (with `exposed` refreshed), is marked `stale_since`
/// (today, or the date it first went stale), and is held at yellow at most; a red dead end stays
/// red. A stale route the miner finds again is simply replaced by the mined one, which returns it
/// to its normal state.
pub fn keep_stale(
    prev: Option<&Track>,
    reviews: &Reviews,
    mut routes: Vec<Route>,
    today: &str,
    mut verdict: impl FnMut(&Route) -> Option<u32>,
) -> Vec<Route> {
    let Some(prev) = prev else { return routes };
    for p in &prev.routes {
        if !p.state.is_live()
            || p.authored
            || routes.iter().any(|r| r.id == p.id)
            || reviews.routes.get(&p.id).is_some_and(|rv| rv.decision == Decision::Rejected)
        {
            continue;
        }
        let Some(exposed) = verdict(p) else { continue };
        let mut s = p.clone();
        s.state = RouteState::Stale;
        if s.aspect == Aspect::Green {
            s.aspect = Aspect::Yellow;
        }
        s.evidence.exposed = exposed;
        s.evidence.stale_since = Some(p.evidence.stale_since.clone().unwrap_or_else(|| today.to_string()));
        routes.push(s);
    }
    routes
}

/// Today's UTC date as `YYYY-MM-DD`.
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    civil_date((secs / 86_400) as i64)
}

/// Days since 1970-01-01 → `YYYY-MM-DD` (Howard Hinnant's civil-from-days).
pub fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;

    fn route(id: &str, aspect: Aspect, support: u32, opp: u32) -> Route {
        Route {
            id: id.into(),
            summary: "s".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: Effect::Verify,
            aspect,
            state: RouteState::Active,
            exit: vec![],
            evidence: Evidence {
                support,
                opportunities: opp,
                lower_bound: wilson_lower(support, opp),
                ..Default::default()
            },
            authored: false,
        }
    }

    fn reviewed(r: &Route, d: Decision) -> Reviews {
        let mut rs = Reviews::default();
        rs.routes
            .insert(r.id.clone(), Review { decision: d, on: "2026-09-22".into(), by: None, basis: basis(r, 0, 0) });
        rs
    }

    #[test]
    fn material_change_is_precise() {
        let r = route("a", Aspect::Yellow, 10, 12);
        let b = basis(&r, 0, 0);
        assert!(material_change(&b, &r).is_none());
        // Small drift is not material.
        assert!(material_change(&b, &route("a", Aspect::Yellow, 11, 13)).is_none());
        // Doubling support is.
        assert!(material_change(&b, &route("a", Aspect::Yellow, 20, 30)).unwrap().contains("support grew"));
        // Halving support is.
        assert!(material_change(&b, &route("a", Aspect::Yellow, 5, 12)).is_some());
        // A lower bound moving by 0.10 is.
        assert!(material_change(&b, &route("a", Aspect::Yellow, 10, 20)).unwrap().contains("lower bound"));
        // Any aspect change is.
        assert!(material_change(&b, &route("a", Aspect::Green, 10, 12)).unwrap().contains("aspect"));
        // Any shape change is.
        let mut s = r.clone();
        s.step = Step::Run { command: "make check".into() };
        assert!(material_change(&b, &s).is_some());
        // Strengthening is one-directional.
        assert!(!strengthened(&b, &route("a", Aspect::Yellow, 5, 12)));
        assert!(strengthened(&b, &route("a", Aspect::Yellow, 20, 20)));
    }

    #[test]
    fn reconcile_holds_decisions() {
        let r = route("a", Aspect::Yellow, 10, 12);
        // Rejected, re-mined unchanged: retired.
        let out = reconcile(None, &reviewed(&r, Decision::Rejected), vec![r.clone()]);
        assert_eq!(out[0].state, RouteState::Retired);
        // Rejected, not re-mined: the retired route is kept.
        let mut retired = r.clone();
        retired.state = RouteState::Retired;
        let prev = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "x/y".into(),
                    version: "0.1.0".into(),
                    description: "d".into(),
                    provenance: "p".into(),
                },
                scope: Scope { repo: "x".into() },
                gauge: Gauge::default(),
            },
            routes: vec![retired.clone()],
        };
        assert_eq!(reconcile(Some(&prev), &reviewed(&r, Decision::Rejected), vec![]), vec![retired]);
        // Rejected, strengthened: back at probation, never green.
        let strong = route("a", Aspect::Green, 30, 30);
        let out = reconcile(None, &reviewed(&r, Decision::Rejected), vec![strong]);
        assert_eq!((out[0].state, out[0].aspect), (RouteState::Probation, Aspect::Yellow));
        // Demoted: held at yellow when learn says green.
        let g = route("a", Aspect::Green, 10, 12);
        let mut d = g.clone();
        d.aspect = Aspect::Yellow;
        let out = reconcile(None, &reviewed(&d, Decision::Demoted), vec![g.clone()]);
        assert_eq!(out[0].aspect, Aspect::Yellow);
        // Promoted: held at green while unchanged; released when evidence moves materially.
        let out = reconcile(None, &reviewed(&g, Decision::Promoted), vec![r.clone()]);
        assert_eq!(out[0].aspect, Aspect::Green);
        let weak = route("a", Aspect::Yellow, 4, 12);
        let out = reconcile(None, &reviewed(&g, Decision::Promoted), vec![weak]);
        assert_eq!(out[0].aspect, Aspect::Yellow);
        // Authored routes are never touched.
        let mut a = g.clone();
        a.authored = true;
        let out = reconcile(None, &reviewed(&r, Decision::Rejected), vec![a.clone()]);
        assert_eq!(out, vec![a]);
    }

    #[test]
    fn keep_stale_holds_exposed_routes_at_yellow_on_their_last_evidence() {
        let prev_track = |routes: Vec<Route>| Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "x/y".into(),
                    version: "0.1.0".into(),
                    description: "d".into(),
                    provenance: "p".into(),
                },
                scope: Scope { repo: "x".into() },
                gauge: Gauge::default(),
            },
            routes,
        };
        let g = route("a", Aspect::Green, 10, 12);
        let mut red = route("b", Aspect::Red, 6, 6);
        red.step = Step::Avoid { avoid: "x".into(), instead: "y".into(), signature: "s".into() };
        let prev = prev_track(vec![g.clone(), red.clone()]);
        // Both exposed: kept, stale, green held at yellow, red stays red, evidence as it was.
        let out = keep_stale(Some(&prev), &Reviews::default(), vec![], "2026-09-27", |_| Some(4));
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].state, out[0].aspect), (RouteState::Stale, Aspect::Yellow));
        assert_eq!((out[1].state, out[1].aspect), (RouteState::Stale, Aspect::Red));
        assert_eq!((out[0].evidence.support, out[0].evidence.exposed), (10, 4));
        assert_eq!(out[0].evidence.stale_since.as_deref(), Some("2026-09-27"));
        // Stale again next run: the date it first went stale is kept.
        let out2 = keep_stale(Some(&prev_track(out.clone())), &Reviews::default(), vec![], "2026-10-30", |_| Some(5));
        assert_eq!(out2[0].evidence.stale_since.as_deref(), Some("2026-09-27"));
        // Re-found: the mined route stands, nothing is added.
        let out = keep_stale(Some(&prev), &Reviews::default(), vec![g.clone()], "2026-09-27", |r| {
            assert_ne!(r.id, "a", "a route found again is not judged");
            None
        });
        assert_eq!(out, vec![g.clone()]);
        // Not exposed, rejected by a person, retired, or authored: never kept stale.
        assert!(keep_stale(Some(&prev), &Reviews::default(), vec![], "2026-09-27", |_| None).is_empty());
        let out = keep_stale(Some(&prev), &reviewed(&g, Decision::Rejected), vec![], "2026-09-27", |_| Some(4));
        assert_eq!(out.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["b"]);
        let mut retired = g.clone();
        retired.state = RouteState::Retired;
        let mut authored = red.clone();
        authored.authored = true;
        let prev = prev_track(vec![retired, authored]);
        assert!(keep_stale(Some(&prev), &Reviews::default(), vec![], "2026-09-27", |_| Some(4)).is_empty());
        assert!(keep_stale(None, &Reviews::default(), vec![], "2026-09-27", |_| Some(4)).is_empty());
    }

    #[test]
    fn an_approved_environment_step_leaves_draft_only_while_unchanged() {
        let mut env = route("e", Aspect::Yellow, 10, 12);
        env.effect = Effect::Environment;
        env.state = initial_state(env.effect);
        assert_eq!(env.state, RouteState::Draft);
        // Not reviewed: stays a draft.
        assert_eq!(reconcile(None, &Reviews::default(), vec![env.clone()])[0].state, RouteState::Draft);
        // Approved, re-mined unchanged (evidence may drift): in service.
        let drifted =
            Route { evidence: Evidence { support: 30, opportunities: 30, ..env.evidence.clone() }, ..env.clone() };
        assert_eq!(reconcile(None, &reviewed(&env, Decision::Approved), vec![drifted])[0].state, RouteState::Active);
        // Approved, but the step is now a different command: a draft again.
        let mut other = env.clone();
        other.step = Step::Run { command: "make setup".into() };
        assert_eq!(reconcile(None, &reviewed(&env, Decision::Approved), vec![other])[0].state, RouteState::Draft);
        // Other effects start in service.
        assert_eq!(initial_state(Effect::Verify), RouteState::Active);
    }

    #[test]
    fn dates_and_validation() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(20_718), "2026-09-22");
        assert_eq!(civil_date(11_016), "2000-02-29");
        let r = route("a", Aspect::Yellow, 10, 12);
        let mut rs = reviewed(&r, Decision::Approved);
        assert!(validate(&rs).is_ok());
        rs.routes.get_mut("a").unwrap().by = Some("line\nbreak".into());
        assert!(validate(&rs).is_err());
        let mut rs = reviewed(&r, Decision::Approved);
        rs.routes.get_mut("a").unwrap().on = "yesterday".into();
        assert!(validate(&rs).is_err());
        let d = tempfile::tempdir().unwrap();
        let rs = reviewed(&r, Decision::Approved);
        save(d.path(), &rs).unwrap();
        assert_eq!(load(d.path()).unwrap(), rs);
        save(d.path(), &Reviews::default()).unwrap();
        assert!(!d.path().join(FILE).exists());
    }
}
