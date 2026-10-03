use std::path::{Path, PathBuf};

/// Adage's data directory. `ADAGE_HOME` overrides the default, which lets tests, the proving
/// ground, and the demo run fully isolated instances. Defaults:
///
/// * macOS: `~/Library/Application Support/Adage`
/// * Windows: `%APPDATA%\Adage`
/// * elsewhere: `$XDG_DATA_HOME/adage`, else `~/.local/share/adage`
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

/// A non-empty environment variable, as a path.
fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The user's home directory: `HOME` when set (always on Unix; tests and Git Bash set it on
/// Windows too, which keeps test runs isolated), else `USERPROFILE` on Windows.
pub fn home_dir() -> Option<PathBuf> {
    env_path("HOME").or_else(|| if cfg!(windows) { env_path("USERPROFILE") } else { None })
}

impl Paths {
    pub fn from_env() -> Paths {
        if let Some(h) = env_path("ADAGE_HOME") {
            return Paths { root: h };
        }
        let home = || home_dir().unwrap_or_else(|| PathBuf::from("."));
        let root = if cfg!(target_os = "macos") {
            home().join("Library/Application Support/Adage")
        } else if cfg!(windows) {
            env_path("APPDATA").unwrap_or_else(|| home().join("AppData").join("Roaming")).join("Adage")
        } else {
            env_path("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local/share")).join("adage")
        };
        Paths { root }
    }

    pub fn at(root: impl AsRef<Path>) -> Paths {
        Paths { root: root.as_ref().to_path_buf() }
    }

    pub fn sessions(&self) -> PathBuf {
        self.root.join("sessions")
    }
    pub fn state(&self, session: &str) -> PathBuf {
        self.sessions().join(format!("{}.state", sanitise(session)))
    }
    pub fn spool(&self, session: &str) -> PathBuf {
        self.sessions().join(format!("{}.spool", sanitise(session)))
    }
    pub fn index(&self) -> PathBuf {
        self.root.join("index.json")
    }
    pub fn tracks(&self) -> PathBuf {
        self.root.join("tracks")
    }
    /// Downloaded embedding models (`adage models fetch`); never shipped with Adage.
    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }
    /// Root commits already found, per repository path (`adage_core::repo::repo_key_cached`).
    pub fn repo_keys(&self) -> PathBuf {
        self.root.join("repo-keys")
    }
    pub fn learn_log(&self) -> PathBuf {
        self.root.join("learn.log")
    }
    /// The person's settings (`adage config`).
    pub fn config(&self) -> PathBuf {
        self.root.join(crate::config::FILE)
    }
    /// Live learning's queue: per repository, a request marker (`<slug>.json`), the learner's lock
    /// (`<slug>.lock`) and its last run (`<slug>.state.json`).
    pub fn learn_queue(&self) -> PathBuf {
        self.root.join("learn-queue")
    }
    /// Per repository: one summary line per trace (`<slug>.jsonl`, no prompt, command or path)
    /// and the savings estimate every surface reads (`<slug>.summary.json`).
    pub fn ledger(&self) -> PathBuf {
        self.root.join("ledger")
    }
    /// Caches that can always be rebuilt (per-transcript parses under `cache/sessions`).
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    /// Committed canonical evidence, separate from caches that may be deleted.
    pub fn corpus(&self) -> PathBuf {
        self.root.join("corpus")
    }
    /// Held by whatever writes the tracks directory and the index (`adage condition`, automatic
    /// learning, review decisions, `adage add`), so writers never interleave.
    pub fn tracks_lock(&self) -> PathBuf {
        self.root.join("tracks.lock")
    }
}

/// Session ids come from harness payloads; never let one escape the sessions directory. Ids that
/// had to be altered (or truncated) get a hash suffix, so two different ids never share files.
pub fn sanitise(s: &str) -> String {
    let clean: String =
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(96).collect();
    if clean == s {
        return clean;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{clean}-{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::sanitise;

    #[test]
    fn sanitised_ids_do_not_collide_or_escape() {
        assert_eq!(sanitise("abc-123"), "abc-123");
        assert_ne!(sanitise("a.b"), sanitise("a_b"));
        assert!(!sanitise("../../etc/passwd").contains('/'));
        let long_a = format!("{}a", "x".repeat(200));
        let long_b = format!("{}b", "x".repeat(200));
        assert_ne!(sanitise(&long_a), sanitise(&long_b));
    }
}
