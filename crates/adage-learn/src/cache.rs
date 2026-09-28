//! Per-transcript parse cache: `$ADAGE_HOME/cache/sessions/`.
//!
//! Parsing a transcript (JSON lines, then every shell command through tree-sitter and the command
//! table) is most of what `adage learn` costs. Transcripts only grow, and most never change again,
//! so a parse is kept and reused while the file is the same:
//!
//! - `manifest.json` maps each transcript path to its size and modification time when parsed, the
//!   repository root the parse found (`null` if the file held no usable session), and the file
//!   holding the parsed session. It is stamped with the parser's identity (this build's version,
//!   the session format [`SESSION_FORMAT`], and the running executable's size and modification
//!   time), and a different stamp empties the cache: a new build may parse differently.
//! - `<16 hex digits>.json` holds one parsed [`Session`] (SHA-256 of the path names it).
//!
//! A hit gives exactly what parsing would: the session's repository key is derived afresh from its
//! root (and `--repo`), and outcomes are applied after reading, as for a fresh parse. So learning
//! with and without the cache writes identical tracks and index bytes (tested). The manifest also
//! lets automatic learning pick one repository's transcripts without opening the others.
//!
//! Everything here can be deleted at any time; the next learn re-parses. The files hold what the
//! transcripts hold (prompts and tool output), so they are created readable by the owner only on
//! Unix. Entries whose transcript no longer exists are dropped when the manifest is saved.

use crate::import::Session;
use adage_core::CommandTable;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    size: u64,
    mtime_ns: u128,
    /// The session's root; `None`: the file held no usable session.
    root: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    parser: String,
    entries: BTreeMap<PathBuf, Entry>,
}

/// The parse cache for one learn run. Open it, read sessions through it, then [`save`](Self::save).
pub struct Cache {
    dir: PathBuf,
    parser: String,
    manifest: Manifest,
    /// Entries this run added or replaced (merged into the manifest on disk when saving).
    fresh: BTreeMap<PathBuf, Entry>,
    pub hits: usize,
    pub misses: usize,
}

/// What a learn run reads a transcript as.
pub enum Read {
    /// A session, and whether it came from the cache.
    Session(Box<Session>),
    /// No usable session in the file.
    Nothing,
    /// The session belongs to another repository than the one asked for (it was not loaded).
    Elsewhere,
}

/// The version of the parsed session's format. Bumped whenever [`Session`] gains or changes what
/// a parse records, so an entry written before is never read as if it had it, even by a build
/// whose executable happens to match (format 2: per-prompt import, `Session::prompts` and
/// `StepObs::prompt`, Track G G0; format 3: `Session::ending`, the outcome proxies).
pub const SESSION_FORMAT: u32 = 3;

/// The identity of this parser: a cache written by any other build is not trusted.
fn parser_id() -> String {
    let exe = std::env::current_exe().ok().and_then(|p| std::fs::metadata(p).ok());
    let (size, mtime) = exe.map(|m| (m.len(), stamp(&m))).unwrap_or((0, 0));
    format!("{} format {SESSION_FORMAT} exe {size} {mtime}", env!("CARGO_PKG_VERSION"))
}

fn stamp(m: &std::fs::Metadata) -> u128 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
}

fn file_name(path: &Path) -> String {
    let d = Sha256::digest(path.to_string_lossy().as_bytes());
    format!("{}.json", d.iter().take(8).map(|b| format!("{b:02x}")).collect::<String>())
}

/// Write a file readable by the owner only (on Unix).
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)?.write_all(bytes)
}

/// [`write_private`] through a temporary file renamed into place, for the manifest: readers
/// never see half of it.
fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    write_private(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

impl Cache {
    /// The cache under `dir` (`$ADAGE_HOME/cache/sessions`). An unreadable or foreign manifest is
    /// an empty cache.
    pub fn open(dir: &Path) -> Cache {
        let parser = parser_id();
        let manifest = std::fs::read(dir.join(MANIFEST))
            .ok()
            .and_then(|b| serde_json::from_slice::<Manifest>(&b).ok())
            .filter(|m| m.parser == parser)
            .unwrap_or_else(|| Manifest { parser: parser.clone(), entries: BTreeMap::new() });
        Cache { dir: dir.to_path_buf(), parser, manifest, fresh: BTreeMap::new(), hits: 0, misses: 0 }
    }

    /// Read `path` as [`crate::read_session`] would. With `only_repo`, a session of another
    /// repository is reported as [`Read::Elsewhere`], without loading it when the cache already
    /// knows its root.
    pub fn read(
        &mut self,
        path: &Path,
        table: &CommandTable,
        repo_override: Option<&str>,
        only_repo: Option<&str>,
        repo_of: &mut dyn FnMut(&Path) -> String,
    ) -> Read {
        let Ok(meta) = std::fs::metadata(path) else { return Read::Nothing };
        let (size, mtime_ns) = (meta.len(), stamp(&meta));
        let repo_for = |root: &Path, repo_of: &mut dyn FnMut(&Path) -> String| {
            adage_core::repo::key_with(repo_override, || repo_of(root))
        };
        if let Some(e) = self.manifest.entries.get(path).filter(|e| e.size == size && e.mtime_ns == mtime_ns) {
            let Some(root) = e.root.clone() else {
                self.hits += 1;
                return Read::Nothing;
            };
            let repo = repo_for(&root, repo_of);
            if only_repo.is_some_and(|r| r != repo) {
                self.hits += 1;
                return Read::Elsewhere;
            }
            let loaded = std::fs::read(self.dir.join(file_name(path)))
                .ok()
                .and_then(|b| serde_json::from_slice::<Session>(&b).ok())
                .filter(|s| s.root == root);
            if let Some(mut s) = loaded {
                self.hits += 1;
                s.repo = repo;
                return Read::Session(Box::new(s));
            }
        }
        // A miss: parse, and remember the parse (with the repository left to be derived again).
        self.misses += 1;
        let parsed = crate::read_session(path, table, repo_override);
        let entry = Entry { size, mtime_ns, root: parsed.as_ref().map(|s| s.root.clone()) };
        let stored = match &parsed {
            Some(s) => {
                let _ = std::fs::create_dir_all(&self.dir);
                serde_json::to_vec(s).ok().is_some_and(|b| write_private(&self.dir.join(file_name(path)), &b).is_ok())
            }
            None => true,
        };
        if stored {
            self.manifest.entries.insert(path.to_path_buf(), entry.clone());
            self.fresh.insert(path.to_path_buf(), entry);
        }
        match parsed {
            None => Read::Nothing,
            Some(s) if only_repo.is_some_and(|r| r != s.repo) => Read::Elsewhere,
            Some(s) => Read::Session(Box::new(s)),
        }
    }

    /// Write the manifest: the one on disk now (another learner may have saved since this one
    /// opened it), plus this run's entries, minus transcripts that no longer exist. Best effort: a
    /// cache that cannot be saved costs a re-parse next time.
    pub fn save(self) {
        if self.fresh.is_empty() {
            return;
        }
        let mut m = std::fs::read(self.dir.join(MANIFEST))
            .ok()
            .and_then(|b| serde_json::from_slice::<Manifest>(&b).ok())
            .filter(|m| m.parser == self.parser)
            .unwrap_or_else(|| Manifest { parser: self.parser.clone(), entries: BTreeMap::new() });
        m.entries.extend(self.fresh);
        m.entries.retain(|p, _| p.exists());
        // Session files no entry names: transcripts that are gone, and parses by another build.
        let named: std::collections::BTreeSet<String> =
            m.entries.iter().filter(|(_, e)| e.root.is_some()).map(|(p, _)| file_name(p)).collect();
        for e in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name != MANIFEST && name.ends_with(".json") && !named.contains(&name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        if let Ok(b) = serde_json::to_vec(&m) {
            let _ = std::fs::create_dir_all(&self.dir);
            let _ = write_private_atomic(&self.dir.join(MANIFEST), &b);
        }
    }
}
