//! The compiled index: everything the hot path needs, in one deterministic file.
//!
//! The runtime never reads track directories. `adage learn` (and `adage add`) compile all
//! installed tracks into `index.json`, written atomically (temp file + rename). The same inputs
//! always produce the same bytes, and `generation` identifies exactly which inputs decided.

use crate::io;
use crate::model::*;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub generation: String,
    pub routes: Vec<IndexedRoute>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexedRoute {
    pub repo: String,
    pub track: String,
    pub version: String,
    pub route: Route,
}

impl Index {
    pub fn compile(tracks: &[Track]) -> Index {
        let mut routes: Vec<IndexedRoute> = tracks
            .iter()
            .flat_map(|t| {
                t.routes.iter().filter(|r| matches!(r.state, RouteState::Active | RouteState::Probation)).map(
                    move |r| IndexedRoute {
                        repo: t.manifest.scope.repo.clone(),
                        track: t.manifest.track.name.clone(),
                        version: t.manifest.track.version.clone(),
                        route: r.clone(),
                    },
                )
            })
            .collect();
        routes.sort_by(|a, b| (&a.repo, &a.track, &a.route.id).cmp(&(&b.repo, &b.track, &b.route.id)));
        let body = serde_json::to_vec(&routes).unwrap_or_default();
        let generation = io::hex(&Sha256::digest(&body))[..16].to_string();
        Index { generation, routes }
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
