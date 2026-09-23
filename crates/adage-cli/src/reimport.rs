//! Re-running `adage add` for a track already imported (Implementation plan §4.1): the recipient's
//! own review decisions carry across for every route whose id survives in the new version.
//!
//! - **Rejected / demoted / promoted / approved:** recorded again and re-applied exactly as
//!   `adage learn` re-applies them ([`review::reconcile`]): a rejected route stays retired unless
//!   the new version strengthened materially, a demotion holds, and a promotion holds only while the
//!   route is materially unchanged *and* local first-hand evidence still allows an import to be
//!   green (Design O2).
//! - **Pinned:** the recipient's pinned copy is kept, as learn keeps it; where the new version
//!   differs materially it is recorded as a proposal for `adage review`.
//!
//! Decisions about routes the new version no longer has are dropped with them.

use crate::review::{discover, first_hand};
use adage_tracks::proposals::{self, Proposals};
use adage_tracks::review::{self, Decision, Reviews};
use adage_tracks::{io, Aspect, Route, Track};
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) struct Carried {
    pub track: Track,
    pub reviews: Reviews,
    pub proposals: Proposals,
    /// `(route id, decision)` for every decision carried over.
    pub kept: Vec<(String, Decision)>,
}

/// Carry the recipient's decisions from the installed copy at `dest` (if any) onto `incoming`,
/// which is already at probation.
pub(crate) fn carry(dest: &Path, incoming: Track, tracks_root: &Path) -> Result<Carried> {
    let fresh = |track| Carried { track, reviews: Reviews::default(), proposals: Proposals::default(), kept: vec![] };
    if !dest.join("track.toml").is_file() {
        return Ok(fresh(incoming));
    }
    let Ok(prev) = io::read(dest) else { return Ok(fresh(incoming)) };
    let mut reviews = review::load(dest).with_context(|| {
        format!(
            "cannot read your review decisions in {}; fix or remove that file, then add again",
            dest.join(review::FILE).display()
        )
    })?;
    let prev_proposals = proposals::load(dest).unwrap_or_default();
    let mut track = incoming;
    reviews.routes.retain(|id, _| track.routes.iter().any(|r| r.id == *id));

    // Pinned routes: keep the recipient's copy, propose the new version where it differs materially.
    let mut pinned: Vec<(Route, Option<Route>)> = Vec::new();
    for (id, rv) in &reviews.routes {
        if rv.decision != Decision::Pinned {
            continue;
        }
        let Some(mine) = prev.routes.iter().find(|r| r.id == *id && r.authored) else { continue };
        if let Some(slot) = track.routes.iter_mut().find(|r| r.id == *id) {
            let theirs = std::mem::replace(slot, mine.clone());
            pinned.push((mine.clone(), Some(theirs)));
        }
    }
    let proposals = proposals::propose(&prev_proposals, &pinned, &review::today());

    // Everything else, as learning re-applies it.
    let routes = std::mem::take(&mut track.routes);
    track.routes = review::reconcile(Some(&prev), &reviews, routes);
    let all = discover(tracks_root)?;
    let repo = track.manifest.scope.repo.clone();
    for r in &mut track.routes {
        // An import is green only by a person's promotion, and only with local first-hand evidence.
        let promoted = reviews.routes.get(&r.id).is_some_and(|v| v.decision == Decision::Promoted);
        if r.aspect == Aspect::Green && !(promoted && first_hand(&all, &repo, &r.id).is_some()) {
            r.aspect = Aspect::Yellow;
        }
    }
    let kept = reviews.routes.iter().map(|(id, v)| (id.clone(), v.decision)).collect();
    Ok(Carried { track, reviews, proposals, kept })
}
