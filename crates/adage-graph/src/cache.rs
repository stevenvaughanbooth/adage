//! The session-facts cache (Track G §1.3, stage S2): `$ADAGE_HOME/cache/facts/<hash>.json`.
//!
//! A pure stage keyed by content: `<hash>` is the SHA-256 of the facts format, the embedding
//! scheme's version, the embedding model's id (or `none`) and the annotated session's canonical
//! bytes. A hit is exactly what computing would give; nothing is ever invalidated in place,
//! because any change to an input is a different key. The files hold what transcripts hold (prompt
//! tokens, paths, argv), so the directory and files are created readable by the owner only on
//! Unix, as the parse cache is. Everything here can be deleted at any time.
//!
//! [`FactsCache::prune`] removes files no current key names (sessions that are gone, and entries
//! of other formats or models).

use crate::embed::{Embedder, EMBED_VERSION};
use crate::facts::{facts, input_bytes, SessionFacts, FACTS_FORMAT};
use crate::input::Annotated;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A facts cache directory.
pub struct FactsCache {
    dir: PathBuf,
    pub hits: AtomicUsize,
    pub misses: AtomicUsize,
}

/// The cache key of a session's facts under this embedder.
pub fn key(a: &Annotated, embedder: &Embedder) -> String {
    let mut h = Sha256::new();
    h.update(format!("adage-facts/{FACTS_FORMAT}/embed{EMBED_VERSION}\n").as_bytes());
    h.update(embedder.model_id().unwrap_or("none").as_bytes());
    h.update(b"\n");
    h.update(input_bytes(a));
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(&tmp)?.write_all(bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

impl FactsCache {
    /// The cache under `dir` (`$ADAGE_HOME/cache/facts`); created on the first write.
    pub fn open(dir: &Path) -> FactsCache {
        FactsCache { dir: dir.to_path_buf(), hits: AtomicUsize::new(0), misses: AtomicUsize::new(0) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// The session's facts: from the cache when present and readable, else computed and stored
    /// (best effort: a cache that cannot be written costs a recomputation next time).
    pub fn get(&self, a: &Annotated, embedder: &Embedder) -> SessionFacts {
        let k = key(a, embedder);
        let p = self.path(&k);
        if let Some(f) = std::fs::read(&p)
            .ok()
            .and_then(|b| serde_json::from_slice::<SessionFacts>(&b).ok())
            .filter(|f| f.format == FACTS_FORMAT && f.id == a.session.id)
        {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return f;
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let f = facts(a, embedder);
        if create_private_dir(&self.dir).is_ok() {
            if let Ok(b) = serde_json::to_vec(&f) {
                let _ = write_private(&p, &b);
            }
        }
        f
    }

    /// Remove every entry whose key is not in `keep`. Best effort.
    pub fn prune(&self, keep: &BTreeSet<String>) -> usize {
        let mut removed = 0;
        for e in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".json") else { continue };
            if !keep.contains(stem) && std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Prompt, Session, Step};

    fn a(id: &str) -> Annotated {
        Annotated::bare(Session {
            id: id.into(),
            prompts: vec![Prompt { text: "fix W-1".into(), at_step: 0 }],
            steps: vec![Step::Exec { argv: vec!["tool".into(), "W-1".into()], output: "ok".into(), failed: false }],
        })
    }

    #[test]
    fn hits_are_what_computing_gives_and_files_are_private() {
        let d = tempfile::tempdir().unwrap();
        let c = FactsCache::open(&d.path().join("facts"));
        let e = Embedder::new(None);
        let first = c.get(&a("s1"), &e);
        let second = c.get(&a("s1"), &e);
        assert_eq!(first, second);
        assert_eq!(first, facts(&a("s1"), &e));
        assert_eq!((c.hits.load(Ordering::Relaxed), c.misses.load(Ordering::Relaxed)), (1, 1));
        let mut other = a("s1");
        other.session.prompts[0].text = "fix W-2".into();
        assert_ne!(key(&other, &e), key(&a("s1"), &e));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let f = c.path(&key(&a("s1"), &e));
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(c.dir()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        assert_eq!(c.prune(&BTreeSet::new()), 1);
    }
}
