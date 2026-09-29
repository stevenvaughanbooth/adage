//! The compiled index: everything the hot path needs, in one deterministic file.
//!
//! The runtime never reads track directories. `adage condition` (and `adage add`) compile all
//! installed tracks into `index.json`, written atomically (temp file + rename). The same inputs
//! always produce the same bytes, and `generation` identifies exactly which inputs decided.

use crate::context::CtxNet;
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
    /// Per repository with context triggers, their alphabet and discrimination net
    /// ([`CtxNet`], Track G §7.2). Derived from `routes` like `triggers`; an index written before
    /// it existed has none, and [`Index::contexts`] derives it on the fly.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contexts: BTreeMap<String, CtxNet>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexedRoute {
    pub repo: String,
    pub track: String,
    pub version: String,
    pub route: Route,
    /// For a route bound from a pack (Track C §2.3): what an execution approval would be keyed by.
    /// Filled at compile time from the bound track's [`EXEC_FILE`]; the E0 hot path ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec: Option<ExecBinding>,
}

impl IndexedRoute {
    /// A route only `adage-exec` can carry out: it runs a vendored script (`cas/<hex>`), which
    /// has no path in the repository an agent could run, so E0 never directs it (Track C §1.7).
    pub fn exec_only(&self) -> bool {
        self.exec.as_ref().is_some_and(|b| b.scripts.iter().any(|(p, _)| p.starts_with("cas/")))
    }
}

/// Execution levels (Track C §3.1). A pack may only *request* one; only a machine grants it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Level {
    /// Directive text only (today).
    #[default]
    E0,
    /// Prefetch: run at the trigger, inject typed output.
    E1,
    /// An action a hook runs when its stimulus fires: staged, verified against its declared
    /// outputs, applied atomically (Track C §3.6).
    E2,
}

/// A pack route's binding on this machine (Track C §2.3): the route exec digest, the scripts it
/// resolved to with their SHA-256 (repository-relative for L-ref, `cas/<hex>` under `$ADAGE_HOME`
/// for L-vendor), and the highest level it may be approved at. Grants nothing by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecBinding {
    pub route_exec_digest: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<(String, String)>,
    pub max_level: Level,
}

/// Tracks bound from installed packs live under `<tracks>/@imported/@bound/<owner>/<name>/<repo>`
/// (`@` never starts an owner name, so no import can collide with it).
pub const BOUND_DIR: &str = "@bound";
/// Beside a bound track's `track.toml`: route id → [`ExecBinding`].
pub const EXEC_FILE: &str = "exec.json";

/// Every bound track's exec bindings under `tracks_root`, keyed by (repository, track, route id).
/// Symlinks are never followed; unreadable files are skipped (fail open: no binding).
pub fn exec_bindings(tracks_root: &Path) -> BTreeMap<(String, String, String), ExecBinding> {
    let mut out = BTreeMap::new();
    let mut stack = vec![tracks_root.join(IMPORTED_DIR).join(BOUND_DIR)];
    while let Some(d) = stack.pop() {
        let (toml, exec) = (d.join("track.toml"), d.join(EXEC_FILE));
        if toml.is_file() {
            let manifest = std::fs::read_to_string(&toml).ok().and_then(|t| toml::from_str::<Manifest>(&t).ok());
            let routes = std::fs::read(&exec)
                .ok()
                .and_then(|b| serde_json::from_slice::<BTreeMap<String, ExecBinding>>(&b).ok());
            if let (Some(m), Some(routes)) = (manifest, routes) {
                for (id, b) in routes {
                    out.insert((m.scope.repo.clone(), m.track.name.clone(), id), b);
                }
            }
            continue;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    out
}

/// Tracks installed by `adage add` carry an `imported:` provenance and live under [`IMPORTED_DIR`].
pub fn is_imported(t: &Track) -> bool {
    t.manifest.track.provenance.starts_with("imported")
}

/// Tracks bound by instincts (`<repo>/instincts`) carry an `instinct:<pack>@<version>` provenance.
pub fn is_instinct_track(t: &Track) -> bool {
    t.manifest.track.provenance.starts_with("instinct:")
}

/// Precedence in de-duplication: first-hand (learned or authored) routes, then imports, then
/// instincts. An instinct never shadows a route from any other source with its id, whatever the
/// track names sort to.
fn precedence(t: &Track, r: &Route) -> u8 {
    if is_instinct_track(t) || r.is_instinct() {
        2
    } else if is_imported(t) {
        1
    } else {
        0
    }
}

/// Where `adage add` installs tracks, inside the tracks directory. `@` never appears in a learned
/// track's directory, so `adage condition` can never write over an import.
pub const IMPORTED_DIR: &str = "@imported";

impl Index {
    /// Compile tracks into an index (see [`Index::compile_with_conflicts`]).
    pub fn compile(tracks: &[Track]) -> Index {
        Index::compile_with_conflicts(tracks).0
    }

    /// Compile tracks into an index, with the interlocking rules applied
    /// ([`crate::interlock`]): a route a dead end contradicts at the prompt is left out, and of two
    /// fires at one decision point one is compiled prime. Returns the conflicts found, for
    /// `adage condition` and `adage doctor` to report.
    pub fn compile_with_conflicts(tracks: &[Track]) -> (Index, Vec<interlock::Conflict>) {
        let imported_tracks: BTreeSet<(&str, &str)> = tracks
            .iter()
            .filter(|t| is_imported(t))
            .map(|t| (t.manifest.scope.repo.as_str(), t.manifest.track.name.as_str()))
            .collect();
        let live = |r: &Route| r.state.is_live();
        // A first-hand route a person extinguished still takes part in de-duplication, so an import of
        // the same route cannot slip back in behind the rejection; it is dropped afterwards.
        let mut routes: Vec<(u8, IndexedRoute)> = tracks
            .iter()
            .flat_map(|t| {
                let imported = is_imported(t);
                t.routes.iter().filter(move |r| live(r) || (!imported && r.state == RouteState::Extinguished)).map(
                    move |r| {
                        let ir = IndexedRoute {
                            repo: t.manifest.scope.repo.clone(),
                            track: t.manifest.track.name.clone(),
                            version: t.manifest.track.version.clone(),
                            route: r.clone(),
                            exec: None,
                        };
                        (precedence(t, r), ir)
                    },
                )
            })
            .collect();
        // A route learned here and the same route imported from a teammate, or bound by an
        // instinct, share a content-derived id: keep one, and prefer first-hand evidence over an
        // import, and an import over an instinct (a live first-hand route over an extinguished one, and
        // an extinguished first-hand route over any import or instinct). By provenance, never by track
        // name.
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
        let mut contexts: BTreeMap<String, CtxNet> = BTreeMap::new();
        for repo in repos {
            let of_repo = || routes.iter().filter(|r| r.repo == repo).map(|r| &r.route);
            triggers.insert(repo.to_string(), Triggers::of(of_repo()));
            let net = CtxNet::of(of_repo());
            if !net.is_empty() {
                contexts.insert(repo.to_string(), net);
            }
        }
        (Index { generation, routes, triggers, contexts }, conflicts)
    }

    /// [`Index::compile_with_conflicts`], then the exec bindings of pack-sourced routes read from
    /// the bound tracks under `tracks_root` ([`exec_bindings`]). What `adage condition`, `add`,
    /// `review` and `doctor` compile.
    pub fn compile_root(tracks_root: &Path, tracks: &[Track]) -> (Index, Vec<interlock::Conflict>) {
        let (mut index, conflicts) = Index::compile_with_conflicts(tracks);
        let bindings = exec_bindings(tracks_root);
        if !bindings.is_empty() {
            index.attach_exec(&bindings);
        }
        (index, conflicts)
    }

    /// Set each route's [`IndexedRoute::exec`] from `bindings` and recompute the generation (which
    /// covers the bindings: an approval keyed by them must see a new index when they change).
    pub fn attach_exec(&mut self, bindings: &BTreeMap<(String, String, String), ExecBinding>) {
        for r in &mut self.routes {
            r.exec = bindings.get(&(r.repo.clone(), r.track.clone(), r.route.id.clone())).cloned();
        }
        let body = serde_json::to_vec(&self.routes).unwrap_or_default();
        self.generation = io::hex(&Sha256::digest(&body))[..16].to_string();
    }

    /// What can make `repo`'s routes act: precomputed, or derived from the routes for an index
    /// compiled before triggers were stored.
    pub fn triggers(&self, repo: &str) -> Cow<'_, Triggers> {
        match self.triggers.get(repo) {
            Some(t) => Cow::Borrowed(t),
            None => Cow::Owned(Triggers::of(self.for_repo(repo).map(|r| &r.route))),
        }
    }

    /// `repo`'s context alphabet and net: precomputed, or derived from the routes for an index
    /// compiled before they were stored (empty when the repository has no context trigger).
    pub fn contexts(&self, repo: &str) -> Cow<'_, CtxNet> {
        match self.contexts.get(repo) {
            Some(n) => Cow::Borrowed(n),
            None if self.triggers.contains_key(repo) => Cow::Owned(CtxNet::default()),
            None => Cow::Owned(CtxNet::of(self.for_repo(repo).map(|r| &r.route))),
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
        // Owner-only: procedure cards quote the person's traces (review B3).
        crate::io::write_private(&tmp, &serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Load the index; any failure yields an empty index (fail open).
    pub fn load(path: &Path) -> Index {
        Index::try_load(path).unwrap_or_default()
    }

    /// Load the index, saying why it could not be: [`Unloaded::Missing`] when there is no file (a
    /// fresh machine), [`Unloaded::Damaged`] when there is one that cannot be read or parsed (a
    /// truncated write, a disk error). The hook treats a damaged index as a request to rebuild it.
    pub fn try_load(path: &Path) -> std::result::Result<Index, Unloaded> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            // No index there (a data directory not created yet, or that cannot be: a file in the
            // way) is missing, not damaged: there is nothing to rebuild.
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => {
                return Err(Unloaded::Missing)
            }
            Err(_) => return Err(Unloaded::Damaged),
        };
        serde_json::from_slice(&bytes).map_err(|_| Unloaded::Damaged)
    }
}

/// Why [`Index::try_load`] found no index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unloaded {
    Missing,
    Damaged,
}
