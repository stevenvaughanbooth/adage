//! Noticed candidates (plan D2): patterns the learner counted below a reflex's support, walked in
//! their own context net from the same events as the routes'. Recorded on the decision in both
//! arms and never directed.

use crate::{Ctx, Decision, SessionState};
use adage_tracks::context::CtxNet;
use serde::{Deserialize, Serialize};

/// At most this many candidates are noticed per session ([`SessionState::noticed`]).
pub const MAX_NOTICED: usize = 64;

/// A candidate whose context this session's events just completed (architecture §3.10 of the
/// Claude Code mod, "Noticed"): a sequence learning counted below the support a reflex needs, or a
/// nascent reflex waiting for review. A note about learning, never a directive: it reaches neither
/// the model nor `would_have`, and both arms record it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Noticed {
    /// The candidate's id: the id the same pattern gets as a reflex.
    pub candidate: String,
    /// Distinct earlier traces it was seen in.
    pub support: u32,
    /// The support a reflex needs in this repository.
    pub s_min: u32,
    /// A nascent reflex waiting for a person's review.
    pub review: bool,
}

/// The candidates the newest alphabet event completes in the candidates' own net (plan D2), each
/// noticed once per session and at most [`MAX_NOTICED`]: recorded on the decision, in both arms,
/// and never emitted. A candidate is never a route's id (`Index::attach_candidates`), so a
/// pattern already in service is directed, never noticed.
pub(crate) fn notice(ctx: &Ctx, net: &CtxNet, st: &mut SessionState, d: &mut Decision) {
    for id in net.walk(&st.cand_ctx.recent()) {
        if st.noticed.len() >= MAX_NOTICED {
            return;
        }
        if st.noticed.iter().any(|n| n == id) {
            continue;
        }
        let Some(c) = ctx.index.candidate(&ctx.repo, id) else { continue };
        st.noticed.push(c.id.clone());
        d.noticed.push(Noticed { candidate: c.id.clone(), support: c.support, s_min: c.s_min, review: c.review });
    }
}
