//! Instincts: prebuilt routes that ship with Adage and work on day 0, with no session history.
//!
//! *Reflexes* are routes learned from sessions (every route the graph engine learns). An *instinct*
//! ships as a pack of templates (`packs/<name>/` in the Adage repository, `adage_pack::builtin`;
//! `docs/marketplace.md`), each naming a detector of this crate's catalogue (`data/catalogue.toml`):
//! this crate is the detectors. At learn time the binder reads a repository's declared facts —
//! Makefile rules, package.json scripts, lockfiles, pyproject, Cargo and Go metadata, CI `run:`
//! steps, generated-file markers — from bounded reads of its files, never executing anything, and
//! binds each enabled detector into concrete [`adage_tracks::Route`] values, which
//! `adage_pack::instincts::bind` keeps where a template of a pack turned on admits them:
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
    // What the environments hold (`pip install` into one changes its site-packages).
    for v in [".venv", "venv"] {
        for e in std::fs::read_dir(root.join(v).join("lib")).into_iter().flatten().flatten() {
            let sp = e.path().join("site-packages");
            if let Ok(t) = std::fs::metadata(&sp).and_then(|m| m.modified()) {
                let at = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
                let _ = write!(s, "\n{} {at}", sp.display());
            }
        }
    }
    adage_core::repo::short_hash(&s)
}

/// A project Python environment checked, from its files alone, to run the project's tests and
/// import the project: what makes a `PreToolUse` interpreter rewrite strictly equivalent.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VerifiedEnv {
    /// The environment's interpreter, repository-relative (`.venv/bin/python`).
    pub interpreter: String,
    /// pytest is installed in it.
    pub pytest: bool,
    /// The project's own packages it imports from this checkout (a package directory, or a `.pth`
    /// or editable install pointing inside the checkout), and the declared test dependencies
    /// installed in it.
    pub modules: Vec<String>,
}

/// Verify the checkout's own virtual environment (`.venv` or `venv`, with its `pyvenv.cfg` and
/// interpreter), without running anything: its `site-packages` must hold pytest, or import the
/// project's own package from this checkout. `None` when there is no such environment or it holds
/// neither. Done at bind time (never in a hook), and recorded with the binding.
pub fn verify_env(root: &Path) -> Option<VerifiedEnv> {
    let facts = extract(root);
    let dir = facts.python.venv.as_deref()?;
    let interpreter = format!("{dir}/bin/python");
    // The interpreter itself (a symlink to a base interpreter that must still exist).
    if !root.join(&interpreter).is_file() {
        return None;
    }
    let lib = std::fs::read_dir(root.join(dir).join("lib")).ok()?;
    let site: Vec<std::path::PathBuf> =
        lib.flatten().map(|e| e.path().join("site-packages")).filter(|p| p.is_dir()).collect();
    let has = |name: &str| {
        site.iter().any(|s| s.join(name).join("__init__.py").is_file() || s.join(format!("{name}.py")).is_file())
    };
    let root_real = root.canonicalize().ok()?;
    // `.pth` lines naming a directory inside this checkout (an editable install's `src`).
    let mut pth_dirs: Vec<std::path::PathBuf> = Vec::new();
    for s in &site {
        for e in std::fs::read_dir(s).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "pth") {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                for l in text.lines().take(64).filter(|l| !l.starts_with("import ") && !l.trim().is_empty()) {
                    if let Ok(d) = std::path::Path::new(l.trim()).canonicalize() {
                        if d.starts_with(&root_real) && d.is_dir() {
                            pth_dirs.push(d);
                        }
                    }
                }
            }
        }
    }
    // The project's package from this checkout: an editable install into it, or a package at the
    // checkout's root (a script run from the root imports it). A copy installed into
    // site-packages is not the code being edited, so it does not count.
    let project = |name: &str| {
        pth_dirs.iter().any(|d| d.join(name).join("__init__.py").is_file())
            || root.join(name).join("__init__.py").is_file()
    };
    let mut modules: Vec<String> = facts.python.packages.iter().filter(|p| project(p)).cloned().collect();
    modules.extend(facts.python.test_deps.iter().filter(|d| has(d)).cloned());
    modules.sort();
    modules.dedup();
    let pytest = has("pytest");
    (pytest || !modules.is_empty()).then_some(VerifiedEnv { interpreter, pytest, modules })
}

/// The instinct pack's name and version (`core`, `0.1.0`).
pub fn pack() -> (&'static str, &'static str) {
    let c = catalogue();
    (&c.pack.name, &c.pack.version)
}
