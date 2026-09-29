//! Review decisions (Design §10.4, §10.9): what a person decided about each route, stored beside
//! the routes in `review.yaml` so decisions travel with the track and survive re-learning.
//!
//! A decision records the route's **basis** when it was made: a hash of its shape, its strength,
//! and its evidence. Later changes are judged against that basis, never against the previous
//! learn run, so slow drift accumulates until it becomes material.
//!
//! **Material change** (precise): relative to the basis, any of
//! - the shape changed: trigger, step, slots, effect, or exit conditions;
//! - the strength changed;
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
    /// Extinguished by a person; learning keeps it extinguished until it strengthens materially.
    Extinguished,
    /// Held at prime by a person; learn never raises it above that on its own.
    Weakened,
    /// Raised to fire by a person, within its effect ceiling; held while evidence is unchanged.
    Strengthened,
    /// Edit-locked (`authored`); learn never overwrites it.
    Pinned,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Approved => "approved",
            Decision::Extinguished => "extinguished",
            Decision::Weakened => "weakened",
            Decision::Strengthened => "strengthened",
            Decision::Pinned => "pinned",
        }
    }
}

/// The route as it stood when a person decided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Basis {
    /// [`shape`] of the route.
    pub shape: String,
    pub strength: Strength,
    pub support: u32,
    pub opportunities: u32,
    pub lower_bound: f64,
    /// Runtime judgements (treated sessions) counted when the decision was made, so later
    /// weakening signals count only what happened since.
    #[serde(default)]
    pub followed: u32,
    #[serde(default)]
    pub ignored: u32,
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
    /// Pack-level approvals (instincts): approving one environment-effect instinct of a pack in
    /// `adage review` approves every environment-effect instinct of that pack for the
    /// repository, which otherwise stay nascent (Design O11). Pack name → the approval.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub packs: BTreeMap<String, PackApproval>,
}

/// A person approved an instinct pack's environment steps for one repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackApproval {
    /// UTC calendar date, `YYYY-MM-DD`.
    pub on: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

/// A stable hash of what a route directs and when: trigger, step, slots, effect, exit.
pub fn shape(r: &Route) -> String {
    let body = serde_json::to_vec(&(&r.trigger, &r.step, &r.slots, &r.effect, &r.exit)).unwrap_or_default();
    crate::io::hex(&Sha256::digest(&body))[..16].to_string()
}

pub fn basis(r: &Route, followed: u32, ignored: u32) -> Basis {
    Basis {
        shape: shape(r),
        strength: r.strength,
        support: r.evidence.support,
        opportunities: r.evidence.opportunities,
        lower_bound: (r.evidence.lower_bound * 1e4).round() / 1e4,
        followed,
        ignored,
    }
}

/// Why the evidence alone changed materially since `b`, if it did. `material` is the lower-bound
/// movement that counts ([`MATERIAL_LOWER_BOUND`] unless tuned).
fn evidence_change(b: &Basis, r: &Route, material: f64) -> Option<String> {
    let e = &r.evidence;
    if (e.lower_bound - b.lower_bound).abs() >= material - 1e-9 {
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
    material_change_with(b, r, MATERIAL_LOWER_BOUND)
}

/// [`material_change`] with a tuned lower-bound threshold (`LearnParams::material_lower_bound`).
pub fn material_change_with(b: &Basis, r: &Route, material: f64) -> Option<String> {
    if shape(r) != b.shape {
        return Some("its trigger, step, slots, effect, or exit changed".into());
    }
    if r.strength != b.strength {
        return Some(format!("strength changed from {} to {}", strength_str(b.strength), strength_str(r.strength)));
    }
    evidence_change(b, r, material)
}

/// Whether a rejected route has strengthened enough to be proposed again.
pub fn strengthened(b: &Basis, r: &Route) -> bool {
    strengthened_with(b, r, MATERIAL_LOWER_BOUND)
}

/// [`strengthened`] with a tuned lower-bound threshold.
pub fn strengthened_with(b: &Basis, r: &Route, material: f64) -> bool {
    shape(r) != b.shape
        || r.evidence.lower_bound >= b.lower_bound + material - 1e-9
        || r.evidence.support >= MATERIAL_SUPPORT_FACTOR * b.support.max(1)
}

pub fn strength_str(a: Strength) -> &'static str {
    match a {
        Strength::Fire => "fire",
        Strength::Prime => "prime",
        Strength::Inhibit => "inhibit",
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
    for (pack, a) in &r.packs {
        if pack.is_empty() || pack.len() > 32 || !pack.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            bail!("{FILE}: pack {pack:?} is not allowed");
        }
        if !safe_date(&a.on) || a.by.as_deref().is_some_and(|b| !safe_by(b)) {
            bail!("{FILE}: pack {pack}: malformed approval");
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
    if r.routes.is_empty() && r.packs.is_empty() {
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
/// `prev` is the track as it was before this learn run; `mined` holds the learner's routes plus any
/// authored routes already carried over (those are never touched here).
///
/// - **Rejected:** a re-mined route stays extinguished unless it [`strengthened`]; then it returns at
///   forming, prime at most, and waits in the queue again. A rejected route the learner no longer
///   finds is kept, extinguished, so the decision stays visible and travels.
/// - **Weakened:** held at prime, whatever the evidence says.
/// - **Strengthened:** held at the strengthened strength while its shape and evidence are materially
///   unchanged and its effect ceiling allows; otherwise learning decides and the queue shows why.
/// - **Approved:** an environment route, which the learner learns as nascent (O11), is put into
///   service while its shape is the one approved. Otherwise nothing to hold.
/// - **Pinned:** nothing to hold (pinned routes are `authored` and carried over as is).
///
/// Losing authority stays automatic: a reviewed route the learner no longer finds is dropped like
/// any other, and its decision re-applies if it comes back.
pub fn reconcile(prev: Option<&Track>, reviews: &Reviews, mined: Vec<Route>) -> Vec<Route> {
    reconcile_with(prev, reviews, mined, MATERIAL_LOWER_BOUND)
}

/// [`reconcile`] with a tuned material lower-bound threshold (`LearnParams::material_lower_bound`).
pub fn reconcile_with(prev: Option<&Track>, reviews: &Reviews, mut mined: Vec<Route>, material: f64) -> Vec<Route> {
    for (id, rv) in &reviews.routes {
        let authored = |r: &Route| r.id == *id && r.authored;
        if mined.iter().any(authored) {
            continue;
        }
        let Some(m) = mined.iter_mut().find(|r| r.id == *id) else {
            if rv.decision == Decision::Extinguished {
                if let Some(p) =
                    prev.and_then(|p| p.routes.iter().find(|r| r.id == *id && r.state == RouteState::Extinguished))
                {
                    mined.push(p.clone());
                }
            }
            continue;
        };
        match rv.decision {
            Decision::Extinguished if strengthened_with(&rv.basis, m, material) => {
                m.state = RouteState::Forming;
                if m.strength == Strength::Fire {
                    m.strength = Strength::Prime;
                }
            }
            Decision::Extinguished => m.state = RouteState::Extinguished,
            Decision::Weakened if m.strength == Strength::Fire => m.strength = Strength::Prime,
            // An environment step waits as nascent until a person approves it (O11). Approval holds
            // for the step that was approved: a different command waits again.
            Decision::Approved if m.state == RouteState::Nascent && shape(m) == rv.basis.shape => {
                m.state = RouteState::Firm
            }
            Decision::Strengthened
                if m.strength == Strength::Prime
                    && shape(m) == rv.basis.shape
                    && evidence_change(&rv.basis, m, material).is_none()
                    && ceiling(m.effect, matches!(m.trigger, Trigger::AfterEdit { .. })) == Some(rv.basis.strength) =>
            {
                m.strength = rv.basis.strength;
            }
            _ => {}
        }
    }
    mined
}

/// Keep, as [`RouteState::Dormant`], the routes of `prev` that learning no longer finds only because
/// of their own exposure (E1: evidence integrity).
///
/// The learner ignores sessions steered by the route (Design §11), so a route that is
/// delivered, and followed, starves itself of evidence: once the transcripts from before it expire,
/// only the control is left to confirm it. `routes` is the track as reconciled from this run;
/// `verdict` is asked about each route of `prev` that was in service (firm, forming or already
/// dormant), is not in `routes`, and was not rejected by a person. It answers `Some(exposed)` when
/// the route's absence is explained by exposure and nothing unsteered contradicts it
/// (`adage_learn` decides), and `None` to drop it as before.
///
/// A kept route keeps its last evidence (with `exposed` refreshed), is marked `dormant_since`
/// (today, or the date it first went dormant), and is held at prime at most; an inhibit dead end stays
/// inhibit. A dormant route the learner finds again is simply replaced by the mined one, which returns it
/// to its normal state.
pub fn keep_dormant(
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
            || reviews.routes.get(&p.id).is_some_and(|rv| rv.decision == Decision::Extinguished)
        {
            continue;
        }
        let Some(exposed) = verdict(p) else { continue };
        let mut s = p.clone();
        s.state = RouteState::Dormant;
        if s.strength == Strength::Fire {
            s.strength = Strength::Prime;
        }
        s.evidence.exposed = exposed;
        s.evidence.dormant_since = Some(p.evidence.dormant_since.clone().unwrap_or_else(|| today.to_string()));
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

    fn route(id: &str, strength: Strength, support: u32, opp: u32) -> Route {
        Route {
            id: id.into(),
            summary: "s".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: Effect::Verify,
            strength,
            state: RouteState::Firm,
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
        let r = route("a", Strength::Prime, 10, 12);
        let b = basis(&r, 0, 0);
        assert!(material_change(&b, &r).is_none());
        // Small drift is not material.
        assert!(material_change(&b, &route("a", Strength::Prime, 11, 13)).is_none());
        // Doubling support is.
        assert!(material_change(&b, &route("a", Strength::Prime, 20, 30)).unwrap().contains("support grew"));
        // Halving support is.
        assert!(material_change(&b, &route("a", Strength::Prime, 5, 12)).is_some());
        // A lower bound moving by 0.10 is.
        assert!(material_change(&b, &route("a", Strength::Prime, 10, 20)).unwrap().contains("lower bound"));
        // Any strength change is.
        assert!(material_change(&b, &route("a", Strength::Fire, 10, 12)).unwrap().contains("strength"));
        // Any shape change is.
        let mut s = r.clone();
        s.step = Step::Run { command: "make check".into() };
        assert!(material_change(&b, &s).is_some());
        // Strengthening is one-directional.
        assert!(!strengthened(&b, &route("a", Strength::Prime, 5, 12)));
        assert!(strengthened(&b, &route("a", Strength::Prime, 20, 20)));
    }

    #[test]
    fn reconcile_holds_decisions() {
        let r = route("a", Strength::Prime, 10, 12);
        // Rejected, re-mined unchanged: extinguished.
        let out = reconcile(None, &reviewed(&r, Decision::Extinguished), vec![r.clone()]);
        assert_eq!(out[0].state, RouteState::Extinguished);
        // Rejected, not re-mined: the extinguished route is kept.
        let mut extinguished = r.clone();
        extinguished.state = RouteState::Extinguished;
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
            routes: vec![extinguished.clone()],
        };
        assert_eq!(reconcile(Some(&prev), &reviewed(&r, Decision::Extinguished), vec![]), vec![extinguished]);
        // Rejected, strengthened: back at forming, never fire.
        let strong = route("a", Strength::Fire, 30, 30);
        let out = reconcile(None, &reviewed(&r, Decision::Extinguished), vec![strong]);
        assert_eq!((out[0].state, out[0].strength), (RouteState::Forming, Strength::Prime));
        // Weakened: held at prime when learn says fire.
        let g = route("a", Strength::Fire, 10, 12);
        let mut d = g.clone();
        d.strength = Strength::Prime;
        let out = reconcile(None, &reviewed(&d, Decision::Weakened), vec![g.clone()]);
        assert_eq!(out[0].strength, Strength::Prime);
        // Strengthened: held at fire while unchanged; released when evidence moves materially.
        let out = reconcile(None, &reviewed(&g, Decision::Strengthened), vec![r.clone()]);
        assert_eq!(out[0].strength, Strength::Fire);
        let weak = route("a", Strength::Prime, 4, 12);
        let out = reconcile(None, &reviewed(&g, Decision::Strengthened), vec![weak]);
        assert_eq!(out[0].strength, Strength::Prime);
        // Authored routes are never touched.
        let mut a = g.clone();
        a.authored = true;
        let out = reconcile(None, &reviewed(&r, Decision::Extinguished), vec![a.clone()]);
        assert_eq!(out, vec![a]);
    }

    #[test]
    fn keep_dormant_holds_exposed_routes_at_prime_on_their_last_evidence() {
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
        let g = route("a", Strength::Fire, 10, 12);
        let mut inhibit = route("b", Strength::Inhibit, 6, 6);
        inhibit.step = Step::Avoid { avoid: "x".into(), instead: "y".into(), signature: "s".into() };
        let prev = prev_track(vec![g.clone(), inhibit.clone()]);
        // Both exposed: kept, dormant, fire held at prime, inhibit stays inhibit, evidence as it was.
        let out = keep_dormant(Some(&prev), &Reviews::default(), vec![], "2026-09-27", |_| Some(4));
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].state, out[0].strength), (RouteState::Dormant, Strength::Prime));
        assert_eq!((out[1].state, out[1].strength), (RouteState::Dormant, Strength::Inhibit));
        assert_eq!((out[0].evidence.support, out[0].evidence.exposed), (10, 4));
        assert_eq!(out[0].evidence.dormant_since.as_deref(), Some("2026-09-27"));
        // Dormant again next run: the date it first went dormant is kept.
        let out2 = keep_dormant(Some(&prev_track(out.clone())), &Reviews::default(), vec![], "2026-10-30", |_| Some(5));
        assert_eq!(out2[0].evidence.dormant_since.as_deref(), Some("2026-09-27"));
        // Re-found: the mined route stands, nothing is added.
        let out = keep_dormant(Some(&prev), &Reviews::default(), vec![g.clone()], "2026-09-27", |r| {
            assert_ne!(r.id, "a", "a route found again is not judged");
            None
        });
        assert_eq!(out, vec![g.clone()]);
        // Not exposed, rejected by a person, extinguished, or authored: never kept dormant.
        assert!(keep_dormant(Some(&prev), &Reviews::default(), vec![], "2026-09-27", |_| None).is_empty());
        let out = keep_dormant(Some(&prev), &reviewed(&g, Decision::Extinguished), vec![], "2026-09-27", |_| Some(4));
        assert_eq!(out.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["b"]);
        let mut extinguished = g.clone();
        extinguished.state = RouteState::Extinguished;
        let mut authored = inhibit.clone();
        authored.authored = true;
        let prev = prev_track(vec![extinguished, authored]);
        assert!(keep_dormant(Some(&prev), &Reviews::default(), vec![], "2026-09-27", |_| Some(4)).is_empty());
        assert!(keep_dormant(None, &Reviews::default(), vec![], "2026-09-27", |_| Some(4)).is_empty());
    }

    #[test]
    fn an_approved_environment_step_leaves_nascent_only_while_unchanged() {
        let mut env = route("e", Strength::Prime, 10, 12);
        env.effect = Effect::Environment;
        env.state = initial_state(env.effect);
        assert_eq!(env.state, RouteState::Nascent);
        // Not reviewed: stays nascent.
        assert_eq!(reconcile(None, &Reviews::default(), vec![env.clone()])[0].state, RouteState::Nascent);
        // Approved, re-mined unchanged (evidence may drift): in service.
        let drifted =
            Route { evidence: Evidence { support: 30, opportunities: 30, ..env.evidence.clone() }, ..env.clone() };
        assert_eq!(reconcile(None, &reviewed(&env, Decision::Approved), vec![drifted])[0].state, RouteState::Firm);
        // Approved, but the step is now a different command: a nascent again.
        let mut other = env.clone();
        other.step = Step::Run { command: "make setup".into() };
        assert_eq!(reconcile(None, &reviewed(&env, Decision::Approved), vec![other])[0].state, RouteState::Nascent);
        // Other effects start in service.
        assert_eq!(initial_state(Effect::Verify), RouteState::Firm);
    }

    #[test]
    fn dates_and_validation() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(20_718), "2026-09-22");
        assert_eq!(civil_date(11_016), "2000-02-29");
        let r = route("a", Strength::Prime, 10, 12);
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
