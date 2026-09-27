//! The compiled index: everything the hot path needs, in one deterministic file.
//!
//! The runtime never reads track directories. `adage learn` (and `adage add`) compile all
//! installed tracks into `index.json`, written atomically (temp file + rename). The same inputs
//! always produce the same bytes, and `generation` identifies exactly which inputs decided.

use crate::interlock;
use crate::io;
use crate::model::*;
use crate::triggers::Triggers;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub generation: String,
    pub routes: Vec<IndexedRoute>,
    /// Per repository, what can make its routes act ([`Triggers`]), for the hook's no-match fast
    /// path. Derived from `routes`, so not part of `generation`; an index written before it
    /// existed has none, and [`Index::triggers`] derives them on the fly.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub triggers: BTreeMap<String, Triggers>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexedRoute {
    pub repo: String,
    pub track: String,
    pub version: String,
    pub route: Route,
}

/// Tracks installed by `adage add` carry an `imported:` provenance and live under [`IMPORTED_DIR`].
pub fn is_imported(t: &Track) -> bool {
    t.manifest.track.provenance.starts_with("imported")
}

/// Where `adage add` installs tracks, inside the tracks directory. `@` never appears in a learned
/// track's directory, so `adage learn` can never write over an import.
pub const IMPORTED_DIR: &str = "@imported";

impl Index {
    /// Compile tracks into an index (see [`Index::compile_with_conflicts`]).
    pub fn compile(tracks: &[Track]) -> Index {
        Index::compile_with_conflicts(tracks).0
    }

    /// Compile tracks into an index, with the interlocking rules applied
    /// ([`crate::interlock`]): a route a dead end contradicts at the prompt is left out, and of two
    /// greens at one decision point one is compiled yellow. Returns the conflicts found, for
    /// `adage learn` and `adage doctor` to report.
    pub fn compile_with_conflicts(tracks: &[Track]) -> (Index, Vec<interlock::Conflict>) {
        let imported_tracks: BTreeSet<(&str, &str)> = tracks
            .iter()
            .filter(|t| is_imported(t))
            .map(|t| (t.manifest.scope.repo.as_str(), t.manifest.track.name.as_str()))
            .collect();
        let live = |r: &Route| r.state.is_live();
        // A first-hand route a person retired still takes part in de-duplication, so an import of
        // the same route cannot slip back in behind the rejection; it is dropped afterwards.
        let mut routes: Vec<(bool, IndexedRoute)> = tracks
            .iter()
            .flat_map(|t| {
                let imported = is_imported(t);
                t.routes.iter().filter(move |r| live(r) || (!imported && r.state == RouteState::Retired)).map(
                    move |r| {
                        let ir = IndexedRoute {
                            repo: t.manifest.scope.repo.clone(),
                            track: t.manifest.track.name.clone(),
                            version: t.manifest.track.version.clone(),
                            route: r.clone(),
                        };
                        (is_imported(t), ir)
                    },
                )
            })
            .collect();
        // A route learned here and the same route imported from a teammate share a content-derived
        // id: keep one, and prefer first-hand evidence over an import (a live first-hand route over
        // a retired one, and a retired first-hand route over any import).
        routes.sort_by(|(ia, a), (ib, b)| {
            (&a.repo, &a.route.id, ia, !live(&a.route), &a.track).cmp(&(
                &b.repo,
                &b.route.id,
                ib,
                !live(&b.route),
                &b.track,
            ))
        });
        routes.dedup_by(|(_, b), (_, a)| a.repo == b.repo && a.route.id == b.route.id);
        let mut routes: Vec<IndexedRoute> = routes.into_iter().map(|(_, r)| r).filter(|r| live(&r.route)).collect();
        routes.sort_by(|a, b| (&a.repo, &a.track, &a.route.id).cmp(&(&b.repo, &b.track, &b.route.id)));
        let (routes, conflicts) =
            interlock::resolve(routes, |r| imported_tracks.contains(&(r.repo.as_str(), r.track.as_str())));
        let body = serde_json::to_vec(&routes).unwrap_or_default();
        let generation = io::hex(&Sha256::digest(&body))[..16].to_string();
        let mut triggers: BTreeMap<String, Triggers> = BTreeMap::new();
        let repos: BTreeSet<&str> = routes.iter().map(|r| r.repo.as_str()).collect();
        for repo in repos {
            triggers.insert(repo.to_string(), Triggers::of(routes.iter().filter(|r| r.repo == repo).map(|r| &r.route)));
        }
        (Index { generation, routes, triggers }, conflicts)
    }

    /// What can make `repo`'s routes act: precomputed, or derived from the routes for an index
    /// compiled before triggers were stored.
    pub fn triggers(&self, repo: &str) -> Cow<'_, Triggers> {
        match self.triggers.get(repo) {
            Some(t) => Cow::Borrowed(t),
            None => Cow::Owned(Triggers::of(self.for_repo(repo).map(|r| &r.route))),
        }
    }

    pub fn for_repo<'a>(&'a self, repo: &'a str) -> impl Iterator<Item = &'a IndexedRoute> + 'a {
        self.routes.iter().filter(move |r| r.repo == repo)
    }

    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Load the index; any failure yields an empty index (fail open).
    pub fn load(path: &Path) -> Index {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
}
