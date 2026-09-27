//! What a learn run changed, route by route: the `changes` of a `learn.log` line.
//!
//! [`snapshot`] reads a repository's learned tracks (and their proposals) before and after a
//! run; [`diff`] compares the two. One change per route, the most significant:
//!
//! | `change` | When | `from` → `to` |
//! | --- | --- | --- |
//! | `new` | A route in service that was not before (new, or back from retirement) | `null` → aspect |
//! | `draft` | A route that now waits for approval in `adage review` (Design O11) | `null` → aspect |
//! | `retired` | A route in service before that is gone or retired now | aspect → `null` |
//! | `stale` | A route in service no longer found only because it steered its own evidence away; kept on its last evidence, yellow at most (E1) | aspect → aspect |
//! | `aspect` | A route whose aspect changed (e.g. yellow → green) | aspect → aspect |
//! | `evidence` | A route whose sessions of support or opportunity changed | `null` → `null` |
//! | `proposal` | A pinned route with a newly proposed change in `adage review` | `null` → `null` |
//!
//! Summaries are fixed templates over the route kind, aspect and session counts: no prompt text,
//! command, output or path, ever.

use adage_tracks::proposals::Proposals;
use adage_tracks::{io, Aspect, Route, RouteState, Track};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    New,
    Draft,
    Retired,
    Stale,
    Aspect,
    Evidence,
    Proposal,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub route: String,
    pub track: String,
    pub change: ChangeKind,
    pub from: Option<Aspect>,
    pub to: Option<Aspect>,
    pub summary: String,
}

/// A repository's learned tracks at one moment, by track name.
pub type Snapshot = BTreeMap<String, (Track, Proposals)>;

/// The learned tracks for `repo` under `tracks_root` (its `<slug>/` directory; imports live
/// elsewhere and are never learned), with their proposals. Unreadable tracks are left out.
pub fn snapshot(tracks_root: &Path, repo: &str) -> Snapshot {
    read(tracks_root, &tracks_root.join(crate::slug(repo))).remove(repo).unwrap_or_default()
}

/// [`snapshot`] for every repository with learned tracks, by repository key.
pub fn snapshot_all(tracks_root: &Path) -> BTreeMap<String, Snapshot> {
    read(tracks_root, tracks_root)
}

fn read(tracks_root: &Path, dir: &Path) -> BTreeMap<String, Snapshot> {
    let mut out: BTreeMap<String, Snapshot> = BTreeMap::new();
    for t in io::read_all(dir).unwrap_or_default() {
        if adage_tracks::index::is_imported(&t) {
            continue;
        }
        let name = t.manifest.track.name.clone();
        // A learned track's directory is its name under the tracks directory.
        let props = adage_tracks::proposals::load(&tracks_root.join(&name)).unwrap_or_default();
        out.entry(t.manifest.scope.repo.clone()).or_default().insert(name, (t, props));
    }
    out
}

fn in_service(r: &Route) -> bool {
    r.state.is_live()
}

fn aspect(a: Aspect) -> &'static str {
    match a {
        Aspect::Green => "green",
        Aspect::Yellow => "yellow",
        Aspect::Red => "red",
    }
}

/// The kind of route, for summaries: the track's kind (`<repo>/<kind>`), in words.
fn kind(track: &str) -> &'static str {
    match track.rsplit('/').next().unwrap_or("") {
        "work-items" => "work item",
        "finish-line" => "finish line",
        "dead-ends" => "dead end",
        "intents" => "intent",
        _ => "route",
    }
}

fn sessions(r: &Route) -> String {
    format!("{} of {} sessions", r.evidence.support, r.evidence.opportunities)
}

/// The changes from `before` to `after`, sorted by track, route and kind of change.
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut out = Vec::new();
    let names: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    for name in names {
        let k = kind(name);
        let routes = |s: &Snapshot| -> BTreeMap<String, Route> {
            s.get(name).map(|(t, _)| t.routes.iter().map(|r| (r.id.clone(), r.clone())).collect()).unwrap_or_default()
        };
        let (old, new) = (routes(before), routes(after));
        let ids: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
        let change = |id: &str, change, from: Option<Aspect>, to: Option<Aspect>, summary: String| Change {
            route: id.to_string(),
            track: name.clone(),
            change,
            from,
            to,
            summary,
        };
        for id in ids {
            let (o, n) = (old.get(id), new.get(id));
            let was = o.filter(|r| in_service(r));
            match (was, n) {
                (_, Some(n)) if n.state == RouteState::Draft && o.is_none_or(|o| o.state != RouteState::Draft) => out
                    .push(change(
                        id,
                        ChangeKind::Draft,
                        None,
                        Some(n.aspect),
                        format!(
                            "{} {k} route drafted: awaits approval in `adage review` ({})",
                            aspect(n.aspect),
                            sessions(n)
                        ),
                    )),
                (None, Some(n)) if in_service(n) => out.push(change(
                    id,
                    ChangeKind::New,
                    None,
                    Some(n.aspect),
                    format!("new {} {k} route ({})", aspect(n.aspect), sessions(n)),
                )),
                (Some(o), n) if n.is_none_or(|n| !in_service(n)) => out.push(change(
                    id,
                    ChangeKind::Retired,
                    Some(o.aspect),
                    None,
                    format!("{} {k} route retired", aspect(o.aspect)),
                )),
                (Some(o), Some(n)) if n.state == RouteState::Stale && o.state != RouteState::Stale => out.push(change(
                    id,
                    ChangeKind::Stale,
                    Some(o.aspect),
                    Some(n.aspect),
                    format!(
                        "{} {k} route stale: kept on its last evidence ({}), {} of its sessions were steered by it",
                        aspect(n.aspect),
                        sessions(n),
                        n.evidence.exposed
                    ),
                )),
                (Some(o), Some(n)) if o.aspect != n.aspect => out.push(change(
                    id,
                    ChangeKind::Aspect,
                    Some(o.aspect),
                    Some(n.aspect),
                    format!(
                        "{k} route {} → {} ({}, lower bound {:.2})",
                        aspect(o.aspect),
                        aspect(n.aspect),
                        sessions(n),
                        n.evidence.lower_bound
                    ),
                )),
                (Some(o), Some(n))
                    if (o.evidence.support, o.evidence.opportunities)
                        != (n.evidence.support, n.evidence.opportunities) =>
                {
                    out.push(change(
                        id,
                        ChangeKind::Evidence,
                        None,
                        None,
                        format!(
                            "{} {k} route evidence {}/{} → {}/{} sessions",
                            aspect(n.aspect),
                            o.evidence.support,
                            o.evidence.opportunities,
                            n.evidence.support,
                            n.evidence.opportunities
                        ),
                    ))
                }
                _ => {}
            }
        }
        let proposals = |s: &Snapshot| s.get(name).map(|(_, p)| p.routes.clone()).unwrap_or_default();
        let (op, np) = (proposals(before), proposals(after));
        for (id, p) in &np {
            if op.get(id).is_none_or(|o| o.route != p.route) {
                out.push(change(
                    id,
                    ChangeKind::Proposal,
                    None,
                    None,
                    format!("pinned {k} route: a change is proposed in `adage review`"),
                ));
            }
        }
    }
    out.sort_by(|a, b| (&a.track, &a.route, a.change).cmp(&(&b.track, &b.route, b.change)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;
    use adage_tracks::*;

    fn route(id: &str, aspect: Aspect, state: RouteState, support: u32) -> Route {
        Route {
            id: id.into(),
            summary: "run make gen after editing schema/secret.json".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make gen".into() },
            slots: vec![],
            effect: Effect::Verify,
            aspect,
            state,
            exit: vec![],
            evidence: Evidence { support, opportunities: 8, lower_bound: 0.5, ..Default::default() },
            authored: false,
        }
    }

    fn snap(tracks: &[(&str, Vec<Route>)]) -> Snapshot {
        tracks
            .iter()
            .map(|(name, routes)| {
                let t = Track {
                    manifest: Manifest {
                        track: TrackInfo {
                            name: name.to_string(),
                            version: "0.1.0".into(),
                            description: String::new(),
                            provenance: String::new(),
                        },
                        scope: Scope { repo: "acme".into() },
                        gauge: Gauge::default(),
                    },
                    routes: routes.clone(),
                };
                (name.to_string(), (t, Proposals::default()))
            })
            .collect()
    }

    use Aspect::*;
    use RouteState::*;

    #[test]
    fn each_kind_of_change_is_reported_once() {
        let before = snap(&[
            (
                "acme/finish-line",
                vec![
                    route("finish-line-a", Yellow, Active, 5),
                    route("finish-line-b", Green, Active, 6),
                    route("finish-line-c", Green, Active, 6),
                    route("finish-line-d", Green, Active, 6),
                    route("finish-line-e", Yellow, Probation, 4),
                ],
            ),
            ("acme/dead-ends", vec![route("dead-end-1", Red, Active, 5)]),
        ]);
        let mut after = snap(&[
            (
                "acme/finish-line",
                vec![
                    route("finish-line-a", Green, Active, 8),     // aspect
                    route("finish-line-b", Green, Active, 7),     // evidence
                    route("finish-line-c", Green, Retired, 2),    // retired
                    route("finish-line-e", Yellow, Probation, 4), // unchanged
                    route("finish-line-f", Yellow, Active, 5),    // new
                ],
            ),
            ("acme/work-items", vec![route("lookup-x", Yellow, Draft, 5)]), // draft
        ]);
        // finish-line-d is gone; the dead-ends track is gone.
        let pinned = route("finish-line-b", Yellow, Active, 7);
        after.get_mut("acme/finish-line").unwrap().1.routes.insert(
            "finish-line-b".into(),
            adage_tracks::proposals::Proposal {
                on: "2026-09-23".into(),
                reason: "aspect".into(),
                against: adage_tracks::review::basis(&pinned, 0, 0),
                route: pinned,
            },
        );
        let d = diff(&before, &after);
        type Row<'a> = (&'a str, &'a str, ChangeKind, Option<Aspect>, Option<Aspect>);
        let got: Vec<Row> = d.iter().map(|c| (c.track.as_str(), c.route.as_str(), c.change, c.from, c.to)).collect();
        assert_eq!(
            got,
            vec![
                ("acme/dead-ends", "dead-end-1", ChangeKind::Retired, Some(Red), None),
                ("acme/finish-line", "finish-line-a", ChangeKind::Aspect, Some(Yellow), Some(Green)),
                ("acme/finish-line", "finish-line-b", ChangeKind::Evidence, None, None),
                ("acme/finish-line", "finish-line-b", ChangeKind::Proposal, None, None),
                ("acme/finish-line", "finish-line-c", ChangeKind::Retired, Some(Green), None),
                ("acme/finish-line", "finish-line-d", ChangeKind::Retired, Some(Green), None),
                ("acme/finish-line", "finish-line-f", ChangeKind::New, None, Some(Yellow)),
                ("acme/work-items", "lookup-x", ChangeKind::Draft, None, Some(Yellow)),
            ]
        );
        assert_eq!(d[1].summary, "finish line route yellow → green (8 of 8 sessions, lower bound 0.50)");
        assert_eq!(d[2].summary, "green finish line route evidence 6/8 → 7/8 sessions");
        assert_eq!(d[7].summary, "yellow work item route drafted: awaits approval in `adage review` (5 of 8 sessions)");
        for c in &d {
            assert!(!c.summary.contains("schema") && !c.summary.contains("make"), "no route content: {}", c.summary);
        }
        assert!(diff(&after, &after).is_empty(), "no change, nothing reported");
        // A retired route coming back is new again; a draft staying a draft is not news.
        let back = diff(&after, &before);
        assert!(back.iter().any(|c| c.route == "finish-line-c" && c.change == ChangeKind::New));
        assert!(!back.iter().any(|c| c.route == "lookup-x" && c.change == ChangeKind::Draft));

        // Going stale is reported once, as such; staying stale is not news.
        let mut stale = route("finish-line-a", Yellow, Stale, 8);
        stale.evidence.exposed = 11;
        let now = snap(&[("acme/finish-line", vec![stale])]);
        let d = diff(&snap(&[("acme/finish-line", vec![route("finish-line-a", Green, Active, 8)])]), &now);
        assert_eq!(
            d.iter().map(|c| (c.change, c.from, c.to)).collect::<Vec<_>>(),
            vec![(ChangeKind::Stale, Some(Green), Some(Yellow))]
        );
        assert_eq!(
            d[0].summary,
            "yellow finish line route stale: kept on its last evidence (8 of 8 sessions), 11 of its sessions were steered by it"
        );
        assert!(diff(&now, &now).is_empty());
    }
}
