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
use adage_tracks::candidates::CandidateFile;
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
    /// Identity of the mining implementation. `None` disables persistent computation reuse.
    fn computation_stamp(&self) -> Option<String> {
        None
    }
    /// At most one pure-mining checkpoint per repository. Cache failures are misses.
    fn mining_checkpoint(&self, _repo: &str) -> Option<Vec<u8>> {
        None
    }
    fn write_mining_checkpoint(&self, _repo: &str, _body: &[u8]) {}
    /// Repository-scoped canonical evidence. A result-only store may leave these unsupported.
    fn corpus_manifest(&self, _repo: &str) -> Result<Option<crate::corpus::Manifest>> {
        Ok(None)
    }
    fn corpus_object(&self, _digest: &str) -> Result<Vec<u8>> {
        anyhow::bail!("this store does not support canonical corpora")
    }
    fn write_corpus_object(&self, _digest: &str, _body: &[u8]) -> Result<()> {
        anyhow::bail!("this store does not support canonical corpora")
    }
    fn write_corpus_manifest(&self, _manifest: &crate::corpus::Manifest) -> Result<()> {
        anyhow::bail!("this store does not support canonical corpora")
    }
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
    /// The candidates the last learn recorded for `repo` (plan D1), when there are readable ones.
    fn candidates(&self, repo: &str) -> Option<CandidateFile>;
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
    /// Told as compiling the index settles overlaps and writes it ([`crate::progress`]).
    pub progress: crate::progress::Progress,
}

impl Files {
    pub fn new(tracks_root: &Path, index_path: &Path) -> Files {
        Files {
            tracks_root: tracks_root.to_path_buf(),
            index_path: index_path.to_path_buf(),
            progress: Default::default(),
        }
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

/// Raw evidence and computation caches contain source text. Write owner-only files and publish
/// through a unique temporary file; readers cannot observe a partial record.
fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or_else(|| anyhow::anyhow!("file has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let (tmp, mut file) = loop {
        let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    let result = (|| -> std::io::Result<()> {
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    Ok(result?)
}

impl Files {
    fn dir(&self, name: &str) -> PathBuf {
        self.tracks_root.join(name)
    }

    fn paths(&self) -> adage_store::Paths {
        adage_store::Paths::at(self.index_path.parent().unwrap_or(Path::new(".")))
    }

    fn corpus_manifest_path(&self, repo: &str) -> PathBuf {
        self.paths().corpus().join("repositories").join(format!("{}.json", crate::corpus::digest(repo.as_bytes())))
    }

    fn corpus_object_path(&self, digest: &str) -> Result<PathBuf> {
        anyhow::ensure!(crate::corpus::valid_digest(digest), "invalid corpus object digest");
        Ok(self.paths().corpus().join("objects").join(&digest[..2]).join(format!("{digest}.json")))
    }

    fn mining_checkpoint_path(&self, repo: &str) -> PathBuf {
        self.paths().cache().join("mining").join(format!("{}.json", crate::corpus::digest(repo.as_bytes())))
    }
}

impl Store for Files {
    fn computation_stamp(&self) -> Option<String> {
        // Hash once per process. A new implementation cannot reuse old calculations merely
        // because its package version or executable size/timestamp happened to stay the same.
        static STAMP: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        STAMP
            .get_or_init(|| {
                use sha2::{Digest, Sha256};
                use std::io::Read;
                let mut file = std::fs::File::open(std::env::current_exe().ok()?).ok()?;
                let mut hash = Sha256::new();
                let mut buffer = [0; 64 * 1024];
                loop {
                    let n = file.read(&mut buffer).ok()?;
                    if n == 0 {
                        break;
                    }
                    hash.update(&buffer[..n]);
                }
                Some(format!("{:x}", hash.finalize()))
            })
            .clone()
    }

    fn mining_checkpoint(&self, repo: &str) -> Option<Vec<u8>> {
        use std::io::Read;
        let file = std::fs::File::open(self.mining_checkpoint_path(repo)).ok()?;
        if file.metadata().ok()?.len() > crate::checkpoint::MAX_BYTES as u64 {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(crate::checkpoint::MAX_BYTES as u64 + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() <= crate::checkpoint::MAX_BYTES).then_some(bytes)
    }

    fn write_mining_checkpoint(&self, repo: &str, body: &[u8]) {
        if body.len() <= crate::checkpoint::MAX_BYTES {
            let _ = write_private(&self.mining_checkpoint_path(repo), body);
        }
    }

    fn corpus_manifest(&self, repo: &str) -> Result<Option<crate::corpus::Manifest>> {
        let body = match std::fs::read(self.corpus_manifest_path(repo)) {
            Ok(body) => body,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let manifest: crate::corpus::Manifest = serde_json::from_slice(&body)?;
        manifest.validate(repo)?;
        Ok(Some(manifest))
    }

    fn corpus_object(&self, digest: &str) -> Result<Vec<u8>> {
        let file = std::fs::File::open(self.corpus_object_path(digest)?)?;
        let max = adage_ingest::corpus::Limits::default().record_bytes as u64;
        anyhow::ensure!(file.metadata()?.len() <= max, "oversized corpus object");
        let mut body = Vec::new();
        use std::io::Read;
        file.take(max + 1).read_to_end(&mut body)?;
        anyhow::ensure!(crate::corpus::digest(&body) == digest, "corpus object digest mismatch");
        Ok(body)
    }

    fn write_corpus_object(&self, digest: &str, body: &[u8]) -> Result<()> {
        anyhow::ensure!(crate::corpus::digest(body) == digest, "corpus object digest mismatch");
        let path = self.corpus_object_path(digest)?;
        // Verify an existing object too: a damaged object must never become committed evidence.
        if path.exists() && self.corpus_object(digest).is_ok() {
            return Ok(());
        }
        std::fs::create_dir_all(path.parent().expect("object has parent"))?;
        write_private(&path, body)?;
        Ok(())
    }

    fn write_corpus_manifest(&self, manifest: &crate::corpus::Manifest) -> Result<()> {
        manifest.validate(&manifest.repo)?;
        let path = self.corpus_manifest_path(&manifest.repo);
        std::fs::create_dir_all(path.parent().expect("manifest has parent"))?;
        write_private(&path, &serde_json::to_vec(manifest)?)?;
        Ok(())
    }

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

    fn candidates(&self, repo: &str) -> Option<CandidateFile> {
        adage_tracks::candidates::load(&self.tracks_root, repo)
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

    fn write_candidates(&self, candidates: &CandidateFile) -> Result<()> {
        adage_tracks::candidates::write(&self.tracks_root, candidates)
    }

    fn compile_index(&self) -> Result<Compiled> {
        self.progress.report(crate::progress::Stage::Settle, 0, 0);
        let all = io::read_all(&self.tracks_root)?;
        let (index, conflicts) = Index::compile_root(&self.tracks_root, &all);
        self.progress.report(crate::progress::Stage::Index, 0, 0);
        index.write_atomic(&self.index_path)?;
        Ok(Compiled { generation: index.generation, conflicts })
    }

    fn index_generation(&self) -> String {
        Index::load(&self.index_path).generation
    }
}
