//! Candidates in `adage status --json` (plan D1): the patterns learning counted that are not
//! reflexes yet, as the hook would notice them (the compiled index's
//! [`adage_tracks::Index::candidates`]), each through the one reflex shape
//! ([`adage_app::reflex::Reflex`]) with what makes it a candidate beside it.
//!
//! ```json
//! "candidates": [{ "id": "run-1a2b3c", "repo": "acme-billing", "track": "acme-billing/learned", …,
//!                  "gist": "after make test, run make lint", "state": "candidate", "directive": null,
//!                  "candidate": { "support": 2, "s_min": 3, "days": 2, "last_seen": "2026-09-09",
//!                                 "review": false },
//!                  "next": null }]
//! ```
//!
//! A candidate is never delivered, so it has no directive. One that waits for review (a nascent
//! reflex with a context trigger) is the learned track's own route, with its state, evidence and
//! the review command that puts it into service as `next`; any other has the state `candidate`.

use adage_app::reflex::Reflex;
use adage_app::review::Installed;
use adage_store::Paths;
use adage_tracks::candidates::{Candidate, LEARNED_TRACK};
use adage_tracks::{layout, Index, Manifest, Scope, TrackInfo};
use serde_json::{json, Value};
use std::path::Path;

/// Every repository's candidates in the compiled index, by repository then id.
pub(crate) fn status_json(paths: &Paths, installed: &[Installed]) -> Value {
    let index = Index::load(&paths.index());
    let mut out = Vec::new();
    for (repo, cs) in &index.candidates {
        for c in cs {
            out.push(one(&paths.tracks(), repo, c, installed));
        }
    }
    Value::Array(out)
}

fn one(tracks_root: &Path, repo: &str, c: &Candidate, installed: &[Installed]) -> Value {
    let name = format!("{}/{LEARNED_TRACK}", layout::slug(repo));
    // A nascent reflex is its track's own route; any other candidate is shown in the track it
    // would join.
    let own = installed
        .iter()
        .filter(|i| i.track.manifest.scope.repo == repo && i.track.manifest.track.name == name)
        .find_map(|i| i.track.routes.iter().find(|r| r.id == c.id).map(|r| (i, r)));
    let mut reflex = match own {
        Some((inst, r)) => Reflex::of(inst, r, adage_tracks::why::load(&inst.dir).get(&r.id)),
        None => {
            let learned = installed.iter().find(|i| i.track.manifest.track.name == name);
            let inst = Installed {
                dir: tracks_root.join(&name),
                reference: name.clone(),
                track: adage_tracks::Track {
                    manifest: Manifest {
                        track: TrackInfo {
                            name: name.clone(),
                            version: learned.map(|i| i.track.manifest.track.version.clone()).unwrap_or_default(),
                            description: String::new(),
                            provenance: "desire-path".into(),
                        },
                        scope: Scope { repo: repo.to_string() },
                        gauge: Default::default(),
                    },
                    routes: vec![],
                },
                imported: false,
                reviews: Default::default(),
                proposals: Default::default(),
            };
            let mut r = Reflex::of(&inst, &c.route(), None);
            r.state = "candidate".into();
            r
        }
    };
    // Counted, never delivered.
    reflex.directive = None;
    reflex.obligation = None;
    let mut v = serde_json::to_value(&reflex).unwrap_or(Value::Null);
    v["candidate"] = json!({
        "support": c.support,
        "s_min": c.s_min,
        "days": c.days,
        "last_seen": c.last_seen,
        "review": c.review,
    });
    v["next"] = json!(c.review.then(|| format!("adage review approve {} {}", reflex.reference, c.id)));
    v
}
