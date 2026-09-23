//! Proposed changes to pinned routes (Implementation plan §4.1): `proposals.yaml` beside a track.
//!
//! A pinned (`authored`) route is edit-locked: `adage learn` never overwrites it. When the miner's
//! own version of that route differs from it *materially* — by [`crate::review::material_change`],
//! the same definition the review queue uses — learn records the mined version here as a
//! proposal, and `adage review` shows it with a diff. Approving applies it (the route stays
//! pinned); dismissing it keeps the pinned route and suppresses the proposal until the mined
//! version changes materially again.
//!
//! The file is local state, like the index: it is not part of the track format, is never shared
//! (`adage share` copies only track files), and is rewritten by every learn run.

use crate::model::*;
use crate::review::{basis, material_change, Basis};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE: &str = "proposals.yaml";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    /// UTC date the proposal was first made, `YYYY-MM-DD`.
    pub on: String,
    /// Why the mined version differs materially from the pinned route.
    pub reason: String,
    /// The pinned route the proposal was made against. If the pinned route has changed since
    /// (edited by hand), the proposal is stale and is not applied.
    pub against: Basis,
    /// The miner's version of the route.
    pub route: Route,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Proposals {
    #[serde(default)]
    pub routes: BTreeMap<String, Proposal>,
    /// Dismissed proposals: the mined route's basis when a person dismissed it. A proposal returns
    /// only when the mined route changes materially from this.
    #[serde(default)]
    pub dismissed: BTreeMap<String, Basis>,
}

impl Proposals {
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty() && self.dismissed.is_empty()
    }
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

pub fn validate(p: &Proposals) -> Result<()> {
    for (id, v) in &p.routes {
        if !safe_id(id) || v.route.id != *id {
            bail!("{FILE}: route id {id:?} is not allowed");
        }
        if v.reason.len() > 200 || v.reason.chars().any(char::is_control) || v.on.len() != 10 {
            bail!("{FILE}: {id}: malformed proposal");
        }
    }
    if let Some(id) = p.dismissed.keys().find(|id| !safe_id(id)) {
        bail!("{FILE}: route id {id:?} is not allowed");
    }
    Ok(())
}

/// The proposals stored in a track directory; none if it has no `proposals.yaml`.
pub fn load(dir: &Path) -> Result<Proposals> {
    let p = dir.join(FILE);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Proposals::default()),
        Err(e) => return Err(e).with_context(|| p.display().to_string()),
    };
    let v: Proposals = serde_yaml::from_str(&text).with_context(|| p.display().to_string())?;
    validate(&v)?;
    Ok(v)
}

/// Write `proposals.yaml` atomically; an empty set removes the file.
pub fn save(dir: &Path, p: &Proposals) -> Result<()> {
    validate(p)?;
    let f = dir.join(FILE);
    if p.is_empty() {
        return match std::fs::remove_file(&f) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        };
    }
    let tmp = dir.join(format!(".{FILE}.tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_yaml::to_string(p)?)?;
    std::fs::rename(tmp, f)?;
    Ok(())
}

/// Recompute a track's proposals after a learn run.
///
/// For each pinned route, `mined` holds the miner's version of it, if the miner still finds it.
/// A proposal is made when that version differs materially from the pinned route, unless a person
/// dismissed an equivalent proposal. An unchanged proposal keeps its original date, so re-learning
/// the same sessions writes the same bytes.
pub fn propose(prev: &Proposals, pinned: &[(Route, Option<Route>)], today: &str) -> Proposals {
    let mut out = Proposals::default();
    for (p, mined) in pinned {
        let Some(m) = mined else {
            // The miner no longer finds it: nothing to propose, but a dismissal still stands.
            if let Some(d) = prev.dismissed.get(&p.id) {
                out.dismissed.insert(p.id.clone(), d.clone());
            }
            continue;
        };
        let mut m = m.clone();
        m.authored = false;
        let against = basis(p, 0, 0);
        let Some(reason) = material_change(&against, &m) else { continue };
        if let Some(d) = prev.dismissed.get(&p.id) {
            if material_change(d, &m).is_none() {
                out.dismissed.insert(p.id.clone(), d.clone());
                continue;
            }
        }
        let on = match prev.routes.get(&p.id) {
            Some(old) if old.route == m && old.against == against => old.on.clone(),
            _ => today.to_string(),
        };
        out.routes.insert(p.id.clone(), Proposal { on, reason, against, route: m });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;

    fn route(id: &str, command: &str, support: u32, opp: u32) -> Route {
        Route {
            id: id.into(),
            summary: "s".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: command.into() },
            slots: vec![],
            effect: Effect::Verify,
            aspect: Aspect::Yellow,
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

    #[test]
    fn proposes_only_material_changes_and_respects_dismissals() {
        let mut pinned = route("a", "make test", 10, 12);
        pinned.authored = true;
        // Small drift: no proposal.
        let p =
            propose(&Proposals::default(), &[(pinned.clone(), Some(route("a", "make test", 11, 13)))], "2026-09-23");
        assert!(p.routes.is_empty());
        // A different step: a proposal, dated today.
        let changed = route("a", "make check", 10, 12);
        let p = propose(&Proposals::default(), &[(pinned.clone(), Some(changed.clone()))], "2026-09-23");
        assert_eq!(p.routes["a"].on, "2026-09-23");
        assert!(p.routes["a"].reason.contains("step"));
        assert!(!p.routes["a"].route.authored);
        // Unchanged on the next run: keeps its date.
        let again = propose(&p, &[(pinned.clone(), Some(changed.clone()))], "2026-09-30");
        assert_eq!(again, p);
        // Dismissed: suppressed while the mined route is materially the same, back when it moves.
        let mut dismissed = Proposals::default();
        dismissed.dismissed.insert("a".into(), basis(&changed, 0, 0));
        let p = propose(&dismissed, &[(pinned.clone(), Some(route("a", "make check", 11, 13)))], "2026-09-30");
        assert!(p.routes.is_empty() && p.dismissed.contains_key("a"));
        let p = propose(&dismissed, &[(pinned.clone(), Some(route("a", "make check", 30, 30)))], "2026-09-30");
        assert!(p.routes.contains_key("a"));
        // Not mined any more: no proposal.
        assert!(propose(&Proposals::default(), &[(pinned, None)], "x").is_empty());
    }

    #[test]
    fn round_trips_and_validates() {
        let d = tempfile::tempdir().unwrap();
        let mut p = Proposals::default();
        let r = route("a", "make check", 10, 12);
        p.routes.insert(
            "a".into(),
            Proposal { on: "2026-09-23".into(), reason: "why".into(), against: basis(&r, 0, 0), route: r.clone() },
        );
        save(d.path(), &p).unwrap();
        assert_eq!(load(d.path()).unwrap(), p);
        let mut bad = p.clone();
        bad.routes.insert("b".into(), bad.routes["a"].clone());
        assert!(save(d.path(), &bad).is_err(), "a proposal's key must be its route's id");
        save(d.path(), &Proposals::default()).unwrap();
        assert!(!d.path().join(FILE).exists());
    }
}
