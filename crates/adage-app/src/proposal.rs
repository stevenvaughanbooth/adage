//! Proposed changes to pinned routes (Implementation plan §4.1): `adage condition` never
//! overwrites a pinned route; when its own version differs materially, it records a proposal
//! ([`adage_tracks::proposals`]), which a person approves or dismisses.

use adage_tracks::proposals::Proposal;
use adage_tracks::review::shape;
use adage_tracks::Route;

/// The proposal for a route, if it still applies to the route as it stands: made against this
/// very pinned route (a hand edit since then makes it stale until the next learn run).
pub fn current<'a>(p: Option<&'a Proposal>, r: &Route) -> Option<&'a Proposal> {
    p.filter(|p| r.authored && p.against.shape == shape(r) && p.against.strength == r.strength)
}

/// The route that approving the proposal installs: the proposed version, still pinned.
pub fn applied(p: &Proposal) -> Route {
    let mut r = p.route.clone();
    r.authored = true;
    r
}

/// All changed fields of the exact route approval installs. Structured values retain their
/// complete contents, including slot contracts, card commands and execution evidence.
pub fn diff(before: &Route, after: &Route) -> Vec<(&'static str, String, String)> {
    let a = serde_json::to_value(before).unwrap_or_default();
    let b = serde_json::to_value(after).unwrap_or_default();
    let text = |v: &serde_json::Value| {
        v.as_str().map(String::from).unwrap_or_else(|| serde_json::to_string_pretty(v).unwrap_or_default())
    };
    ["summary", "trigger", "step", "slots", "effect", "strength", "state", "exit", "evidence", "authored"]
        .into_iter()
        .filter(|field| a[*field] != b[*field])
        .map(|field| (field, text(&a[field]), text(&b[field])))
        .collect()
}
