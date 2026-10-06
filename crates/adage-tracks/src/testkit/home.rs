//! A temporary world of directories, and a learned home in it: the index, the tracks, the
//! session state files and the spools a test seeds and reads, at the paths `adage_store::Paths`
//! gives (never a joined string, so a layout change moves the fixtures with it).

use crate::{io, Index, Track};
use adage_store::{spool, state, Paths};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// A temporary directory, canonicalised (so paths compare equal to what a binary prints), with
/// the given subdirectories created. Removed when dropped.
pub struct TempWorld {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
}

impl TempWorld {
    pub fn new(dirs: &[&str]) -> TempWorld {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path().canonicalize().expect("a canonical temporary directory");
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).expect("a fixture directory");
        }
        TempWorld { _dir: dir, root }
    }

    /// The world's root, by the name `tempfile::TempDir` gives it, so a fixture that held a
    /// `TempDir` keeps its callers when it holds a world instead.
    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// A repository (a directory with `.git`) at `rel`.
    pub fn repo(&self, rel: &str) -> PathBuf {
        let at = self.p(rel);
        std::fs::create_dir_all(at.join(".git")).expect("a fixture repository");
        at
    }

    /// Write `body` to `rel`, creating its directory.
    pub fn write(&self, rel: &str, body: impl AsRef<[u8]>) -> PathBuf {
        let at = self.p(rel);
        std::fs::create_dir_all(at.parent().expect("a file in the world")).expect("a fixture directory");
        std::fs::write(&at, body).expect("a fixture file");
        at
    }

    /// [`Self::write`], made executable on Unix: a fake tool on a test's `PATH`.
    pub fn executable(&self, rel: &str, body: impl AsRef<[u8]>) -> PathBuf {
        let at = self.write(rel, body);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755)).expect("an executable fixture");
        }
        at
    }

    /// A learned home (an `ADAGE_HOME`) at `rel`, created.
    pub fn home(&self, rel: &str) -> LearnedHome {
        LearnedHome::at(self.p(rel))
    }
}

/// An `ADAGE_HOME` a test fills: its index (hand-written or compiled), its tracks, and per
/// session the state file and the spool.
pub struct LearnedHome {
    pub paths: Paths,
}

impl LearnedHome {
    /// The home at `root`, created if missing.
    pub fn at(root: impl AsRef<Path>) -> LearnedHome {
        let paths = Paths::at(root.as_ref());
        std::fs::create_dir_all(&paths.root).expect("a fixture home");
        LearnedHome { paths }
    }

    pub fn root(&self) -> &Path {
        &self.paths.root
    }

    /// Learning off in `config.toml`, so a hook run in a test never queues a learner.
    pub fn without_auto_learn(self) -> Self {
        std::fs::write(self.paths.config(), "[learn]\nauto = false\n").expect("the fixture config");
        self
    }

    /// Write `index` as the home's index.
    pub fn write_index(&self, index: &Index) -> &Self {
        index.write_atomic(&self.paths.index()).expect("writing the fixture index");
        self
    }

    /// Compile `tracks` into the home's index ([`Index::compile`]: no track directories read).
    pub fn compile(&self, tracks: &[Track]) -> Index {
        let index = Index::compile(tracks);
        self.write_index(&index);
        index
    }

    /// Compile `tracks` as the learner does, beside the home's track directories (candidates and
    /// conflicts included: [`Index::compile_root`]).
    pub fn compile_root(&self, tracks: &[Track]) -> Index {
        let (index, _) = Index::compile_root(&self.paths.tracks(), tracks);
        self.write_index(&index);
        index
    }

    /// Write `track` under the home's tracks at `reference` (`acme/work`, `@imported/…`).
    pub fn install(&self, reference: &str, track: &Track) -> PathBuf {
        let dir = self.paths.tracks().join(reference);
        io::write(&dir, track).expect("writing the fixture track");
        dir
    }

    /// Compile every track under the home's tracks into its index.
    pub fn recompile(&self) -> Index {
        let all = io::read_all(&self.paths.tracks()).expect("reading the fixture tracks");
        self.compile(&all)
    }

    /// The home's index as the hook loads it.
    pub fn loaded(&self) -> Index {
        Index::load(&self.paths.index())
    }

    /// Change `session`'s state file through `f`, as the hook does (`T` is the runtime's
    /// `SessionState`, which this layer cannot name).
    pub fn seed<T: Serialize + DeserializeOwned + Default>(&self, session: &str, f: impl FnOnce(&mut T)) -> &Self {
        state::update::<T, _>(&self.paths.state(session), f).expect("seeding the session state");
        self
    }

    /// `session`'s state as the hook would read it.
    pub fn state<T: DeserializeOwned + Default>(&self, session: &str) -> T {
        state::try_read::<T>(&self.paths.state(session))
            .expect("reading the session state")
            .expect("a session state that can be read")
    }

    /// Append `record` to `session`'s spool.
    pub fn spool(&self, session: &str, record: &impl Serialize) -> &Self {
        spool::append(&self.paths.spool(session), record).expect("appending to the fixture spool");
        self
    }

    /// Every record in `session`'s spool (none when it has no spool).
    pub fn spooled(&self, session: &str) -> Vec<Value> {
        spool::read_all(&self.paths.spool(session)).unwrap_or_default()
    }

    /// The records of `kind` in `session`'s spool.
    pub fn spooled_kind(&self, session: &str, kind: &str) -> Vec<Value> {
        self.spooled(session).into_iter().filter(|r| r["kind"] == kind).collect()
    }
}
