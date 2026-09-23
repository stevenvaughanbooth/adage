use std::path::{Path, PathBuf};

/// Adage's data directory. `ADAGE_HOME` overrides the default, which lets tests, the proving
/// ground, and the demo run fully isolated instances.
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn from_env() -> Paths {
        if let Some(h) = std::env::var_os("ADAGE_HOME") {
            return Paths { root: PathBuf::from(h) };
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let root = if cfg!(target_os = "macos") {
            home.join("Library/Application Support/Adage")
        } else {
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("adage")
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
    pub fn learn_log(&self) -> PathBuf {
        self.root.join("learn.log")
    }
}

/// Session ids come from harness payloads; never let one escape the sessions directory.
pub fn sanitise(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(128).collect()
}
