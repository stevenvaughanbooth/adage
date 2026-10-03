//! Where a learn run's results are materialized: the store port.
//!
//! Learning reads sessions and the reflexes already learned, and decides what to write
//! ([`crate::plan`]); the [`Store`] is the only way those decisions become state. [`Files`] is
//! the store under `$ADAGE_HOME` (tracks as directories, the compiled index as one file), which
//! every front end uses today. A team service (rows in a database) implements the same methods;
//! tests record the writes in memory.

use crate::Planned;
use adage_core::judge::Weights;
use adage_pack::binding::Binding;
use adage_tracks::candidates::CandidateFile;
use adage_tracks::{interlock::Conflict, io, Index};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// The index a store compiled from everything it holds.
#[derive(Debug, Clone, Default)]
pub struct Compiled {
    pub generation: String,
    pub conflicts: Vec<Conflict>,
}

/// What a learn run writes, and nothing else.
pub trait Store {
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
    /// Record the repository's candidates (`adage_tracks::candidates`, plan D1), replacing what
    /// the last learn recorded, before the index is compiled (which reads them). Nothing else
    /// writes them.
    fn write_candidates(&self, candidates: &CandidateFile) -> Result<()>;
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

impl Store for Files {
    fn install_base_packs(&self) {
        crate::instincts::preinstall(&self.tracks_root);
    }

    fn write_track(&self, planned: &Planned) -> Result<()> {
        planned.write()
    }

    fn write_binding(&self, repo: &str, binding: &Binding) {
        write_binding_at(&self.tracks_root, repo, binding);
    }

    fn write_follow_weights(&self, repo: &str, weights: &Weights) {
        write_json(&adage_tracks::layout::follow_weights_path(&self.tracks_root, repo), weights);
    }

    fn write_candidates(&self, candidates: &CandidateFile) -> Result<()> {
        adage_tracks::candidates::write(&self.tracks_root, candidates)
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
