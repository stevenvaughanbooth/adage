//! Instincts: prebuilt routes that ship with Adage and work on day 0, with no session history.
//!
//! *Reflexes* are routes learned from sessions (every route the graph engine learns). An *instinct* is
//! a route template shipped as data (`data/catalogue.toml`) plus a detector. At learn time the
//! binder reads a repository's declared facts — Makefile rules, package.json scripts, lockfiles,
//! pyproject, Cargo and Go metadata, CI `run:` steps, generated-file markers — from bounded reads
//! of its files, never executing anything, and binds each enabled instinct into concrete
//! [`adage_tracks::Route`] values:
//!
//! - provenance `instinct:<pack>@<version>`, no evidence of its own;
//! - prime, in forming (an environment step in nascent, waiting for review);
//! - reactive: after a failure, or after an edit — never at session start, never a Stop block;
//! - ids equal to those the learner would derive, so a learned route for the same behaviour wins
//!   the index's de-duplication, and any learned route that contradicts an instinct suppresses it.
//!
//! Advice (don't hand-edit generated files or lockfiles; use the declared package manager) binds
//! as routes with an advisory step (`Step::Advise`) from a fixed template. `adage condition` writes
//! the routes as the track `<repo>/instincts` (`adage_learn::instincts`), the index gives every
//! other source precedence over them, and the runtime words them as Adage's defaults, never as
//! learned (docs/instincts.md).
//!
//! ```no_run
//! use std::collections::BTreeSet;
//! let result = adage_instincts::bind(std::path::Path::new("."), &[], &BTreeSet::new());
//! for r in &result.routes {
//!     println!("{} {:?}", r.id, r.step);
//! }
//! ```

pub mod bind;
pub mod catalogue;
pub mod declared;
pub mod detect;
pub mod files;
pub mod safety;

pub use bind::{bind, bind_facts, bind_with, Advice, BindOptions, BindResult};
pub use catalogue::{catalogue, Catalogue, Instinct};
pub use declared::{extract, DeclaredFacts};
use std::path::Path;

/// Whether a route came from an instinct (by its provenance).
pub fn is_instinct(r: &adage_tracks::Route) -> bool {
    r.evidence.provenance.starts_with("instinct:")
}

/// Root files whose presence and content decide what binds ([`declared::extract`] reads them),
/// and the virtual environments U11 binds to. Lockfiles come from the catalogue.
pub const DECLARED_FILES: &[&str] = &[
    "GNUmakefile",
    "makefile",
    "Makefile",
    "package.json",
    "justfile",
    "Justfile",
    ".justfile",
    "Taskfile.yml",
    "Taskfile.yaml",
    "taskfile.yml",
    "pyproject.toml",
    "setup.cfg",
    "setup.py",
    "tox.ini",
    "noxfile.py",
    "pytest.ini",
    "conftest.py",
    "Cargo.toml",
    "go.mod",
    ".venv/pyvenv.cfg",
    "venv/pyvenv.cfg",
    ".github/workflows",
];

/// A fingerprint of a checkout's declared files (the set of [`DECLARED_FILES`] and root lockfiles
/// that exist, with their sizes and modification times): cheap enough for a session start (a
/// few dozen `stat`s, nothing read), and it changes when what binds may change: a `.venv` made or
/// removed, a Makefile or pyproject.toml edited. The checkout's own path is part of it, so another
/// checkout (a per-task worktree) is bound on its own files.
pub fn fingerprint(root: &Path) -> String {
    use std::fmt::Write;
    let mut s = root.to_string_lossy().to_string();
    let lockfiles = catalogue().lockfiles.keys().map(String::as_str);
    for f in DECLARED_FILES.iter().copied().chain(lockfiles) {
        let Ok(m) = std::fs::metadata(root.join(f)) else { continue };
        let at =
            m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        let _ = write!(s, "\n{f} {} {at}", m.len());
    }
    adage_core::repo::short_hash(&s)
}

/// The instinct pack's name and version (`core`, `0.1.0`).
pub fn pack() -> (&'static str, &'static str) {
    let c = catalogue();
    (&c.pack.name, &c.pack.version)
}
