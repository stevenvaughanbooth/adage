//! Where a learn run reads what earlier runs left, and materializes what it decides: the store
//! port.
//!
//! Learning reads sessions and, from the store, the reflexes already learned with a person's
//! decisions about them; it decides what to write ([`crate::plan`]), and the [`Store`] is the only
//! way those decisions become state. Tracks are named as their index names them
//! (`<repo slug>/learned`). [`Files`] is the store under `$ADAGE_HOME` (tracks as directories, the
//! compiled index as one file), which every front end uses today. A team service (rows in a
//! database) implements the same methods; tests keep them in memory.

use crate::Planned;
use adage_core::judge::Weights;
use adage_pack::binding::Binding;
use adage_tracks::proposals::Proposals;
use adage_tracks::review::Reviews;
use adage_tracks::why::Why;
use adage_tracks::{interlock::Conflict, io, Index, Track};
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The index a store compiled from everything it holds.
#[derive(Debug, Clone, Default)]
pub struct Compiled {
    pub generation: String,
    pub conflicts: Vec<Conflict>,
}

/// What a learn run reads of earlier runs, and writes, and nothing else.
pub trait Store {
    /// The track named `name`, when the store holds a readable one.
    fn track(&self, name: &str) -> Option<Track>;
    /// Whether the store holds anything under `name` (a track, or its decisions alone).
    fn holds(&self, name: &str) -> bool;
    /// A person's decisions about the routes of `name` (empty when there are none). An error when
    /// they exist and cannot be read: learning must not silently undo them.
    fn reviews(&self, name: &str) -> Result<Reviews>;
    /// The proposals recorded for `name`: their dates and dismissals.
    fn proposals(&self, name: &str) -> Result<Proposals>;
    /// Why each route of `name` exists, as the run that learned it recorded.
    fn why(&self, name: &str) -> BTreeMap<String, Why>;
    /// The names of the tracks directly under `prefix` (a repository's slug).
    fn names_under(&self, prefix: &str) -> Vec<String>;
    /// Every readable track under `prefix` (a repository's slug, or the imports), sorted by name.
    fn tracks_under(&self, prefix: &str) -> Vec<Track>;
    /// The checkout `repo`'s instincts were last bound against.
    fn binding(&self, repo: &str) -> Option<Binding>;
    /// The instinct packs available to bind (built in, and installed into the store), and why any
    /// installed pack cannot be read (`who`, `why`).
    fn packs(&self) -> (Vec<adage_pack::Pack>, Vec<(String, String)>);

    /// Install the base instinct packs, before planning reads what is installed.
    fn install_base_packs(&self);
    /// Write one planned track with its proposals, decisions and explanations, replacing the
    /// directories it folds in.
    fn write_track(&self, planned: &Planned) -> Result<()>;
    /// Record the checkout a repository's instincts were bound against. Best effort: a binding
    /// that cannot be recorded is bound again next run.
    fn write_binding(&self, repo: &str, binding: &Binding);
    /// Record the repository's learned follow-judge weights. Best effort, as a binding is.
    fn write_follow_weights(&self, repo: &str, weights: &Weights);
    /// Compile the index from every track the store holds and publish it.
    fn compile_index(&self) -> Result<Compiled>;
    /// The generation of the index published now (empty when there is none).
    fn index_generation(&self) -> String;
}

/// Tracks under `tracks_root`, the index at `index_path`: `$ADAGE_HOME`'s layout.
#[derive(Debug, Clone)]
pub struct Files {
    pub tracks_root: PathBuf,
    pub index_path: PathBuf,
}

impl Files {
    pub fn new(tracks_root: &Path, index_path: &Path) -> Files {
        Files { tracks_root: tracks_root.to_path_buf(), index_path: index_path.to_path_buf() }
    }
}

/// Record `binding` for `repo` under `tracks_root` ([`adage_pack::binding::binding_path`]).
pub(crate) fn write_binding_at(tracks_root: &Path, repo: &str, binding: &Binding) {
    write_json(&adage_pack::binding::binding_path(tracks_root, repo), binding);
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    if let (Some(dir), Ok(body)) = (path.parent(), serde_json::to_vec(value)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = adage_store::config::write_atomic(path, &body);
    }
}

impl Files {
    fn dir(&self, name: &str) -> PathBuf {
        self.tracks_root.join(name)
    }
}

impl Store for Files {
    fn track(&self, name: &str) -> Option<Track> {
        io::read(&self.dir(name)).ok()
    }

    fn holds(&self, name: &str) -> bool {
        self.dir(name).is_dir()
    }

    fn reviews(&self, name: &str) -> Result<Reviews> {
        adage_tracks::review::load(&self.dir(name))
    }

    fn proposals(&self, name: &str) -> Result<Proposals> {
        adage_tracks::proposals::load(&self.dir(name))
    }

    fn why(&self, name: &str) -> BTreeMap<String, Why> {
        adage_tracks::why::load(&self.dir(name))
    }

    fn names_under(&self, prefix: &str) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(self.dir(prefix))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| format!("{prefix}/{}", e.file_name().to_string_lossy()))
            .collect();
        out.sort();
        out
    }

    fn tracks_under(&self, prefix: &str) -> Vec<Track> {
        io::read_all(&self.dir(prefix)).unwrap_or_default()
    }

    fn binding(&self, repo: &str) -> Option<Binding> {
        adage_pack::binding::binding(&self.tracks_root, repo)
    }

    fn packs(&self) -> (Vec<adage_pack::Pack>, Vec<(String, String)>) {
        let (available, problems) = adage_pack::instincts::available(&self.tracks_root);
        (available.into_iter().map(|(p, _)| p).collect(), problems)
    }

    fn install_base_packs(&self) {
        crate::instincts::preinstall(&self.tracks_root);
    }

    fn write_track(&self, planned: &Planned) -> Result<()> {
        planned.write_under(&self.tracks_root)
    }

    fn write_binding(&self, repo: &str, binding: &Binding) {
        write_binding_at(&self.tracks_root, repo, binding);
    }

    fn write_follow_weights(&self, repo: &str, weights: &Weights) {
        write_json(&adage_tracks::layout::follow_weights_path(&self.tracks_root, repo), weights);
    }

    fn compile_index(&self) -> Result<Compiled> {
        let all = io::read_all(&self.tracks_root)?;
        let (index, conflicts) = Index::compile_root(&self.tracks_root, &all);
        index.write_atomic(&self.index_path)?;
        Ok(Compiled { generation: index.generation, conflicts })
    }

    fn index_generation(&self) -> String {
        Index::load(&self.index_path).generation
    }
}
