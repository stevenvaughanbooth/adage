//! What adage-cli's integration suites share (docs/testing.md, "Shared test helpers"): one
//! scrub of the environment a developer's shell or an agent session would otherwise leak into the
//! binary, a hermetic `Command`, and, from the workspace test kit, a temporary world of
//! directories and a learned home. A suite takes it with `mod common;` and uses what it needs;
//! routes, tracks and indexes come from `adage_tracks::testkit` directly.

#![allow(dead_code)] // each suite uses part of the kit

use std::ffi::OsStr;
use std::path::Path;
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

/// A temporary directory with subdirectories, repositories, files and fake tools; and, with
/// [`LearnedHome`], an `ADAGE_HOME` with its index, tracks, session state and spools. Both come
/// from the workspace test kit (`adage_tracks::testkit`, docs/claude-mod/impl/testkit.md), which
/// also builds routes, tracks and indexes.
#[allow(unused_imports)] // each suite uses part of the kit
pub use adage_tracks::testkit::{LearnedHome, TempWorld};

/// A run's stdout then stderr, for assertion messages and searches.
pub fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}
