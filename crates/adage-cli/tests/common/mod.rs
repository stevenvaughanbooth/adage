//! What adage-cli's integration suites share (docs/testing.md, "Shared test helpers"): one
//! scrub of the environment a developer's shell or an agent session would otherwise leak into the
//! binary, a hermetic `Command`, and a temporary world of directories. A suite takes it with
//! `mod common;` and uses what it needs.

#![allow(dead_code)] // each suite uses part of the kit

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Variables outside Adage's own that change what the binary does: Claude Code's and Codex's
/// configuration homes (registrations, history), an ssh-agent (`train accept` signs with it) and
/// an active Python environment (the `PreToolUse` rewrite reads it). The agent markers come from
/// `adage_app::person::AGENT_ENV`, and every inherited `ADAGE_*` variable is dropped too.
pub const SCRUB: [&str; 5] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "SSH_AUTH_SOCK", "VIRTUAL_ENV", "CONDA_PREFIX"];

/// Remove from `c` every variable that would make a run depend on the shell running the tests:
/// the agent markers (a person-only verb refuses under them), every `ADAGE_*` switch this
/// process inherited, and [`SCRUB`]. A suite sets what it needs afterwards. The list only grows:
/// a variable removed here cannot hide a regression, only a dependence on the machine.
pub fn scrub(c: &mut Command) -> &mut Command {
    for v in adage_app::person::AGENT_ENV.iter().chain(SCRUB.iter()) {
        c.env_remove(v);
    }
    for (k, _) in std::env::vars_os() {
        if k.to_str().is_some_and(|k| k.starts_with("ADAGE_")) {
            c.env_remove(&k);
        }
    }
    c
}

/// `bin`, scrubbed, run in `cwd` with `adage_home` as its data directory and automatic learning
/// off (a test that wants it sets `ADAGE_AUTO_LEARN` itself).
pub fn hermetic(bin: impl AsRef<OsStr>, adage_home: &Path, cwd: &Path) -> Command {
    let mut c = Command::new(bin);
    scrub(&mut c);
    c.current_dir(cwd).env("ADAGE_HOME", adage_home).env("ADAGE_AUTO_LEARN", "0");
    c
}

/// [`hermetic`] for this crate's `adage`.
pub fn adage(adage_home: &Path, cwd: &Path) -> Command {
    hermetic(env!("CARGO_BIN_EXE_adage"), adage_home, cwd)
}

/// A temporary directory, canonicalised (so paths compare equal to what the binary prints), with
/// the given subdirectories created. Removed when dropped.
pub struct TempWorld {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
}

impl TempWorld {
    pub fn new(dirs: &[&str]) -> TempWorld {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        TempWorld { _dir: dir, root }
    }

    pub fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

/// A run's stdout then stderr, for assertion messages and searches.
pub fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}
