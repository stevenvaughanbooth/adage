//! `cargo xtask test python [<suite>...]`: every Python test suite, run one way.
//!
//! Each suite is a directory of `test_*.py` files and the directory its imports are rooted at.
//! The runner passes every test file to `python3 -m unittest`, from the suite's root, so a suite
//! runs the same locally and in CI whatever directory you start in. A test file that belongs to
//! no suite fails `tests::every_python_test_belongs_to_a_suite`, so none is silently never run.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A Python test suite.
pub struct Suite {
    pub name: &'static str,
    /// Where its imports are rooted: the working directory `unittest` runs in.
    pub root: &'static str,
    /// The directory holding its `test_*.py` files.
    pub tests: &'static str,
}

/// Every Python test suite in the repository.
pub const PYTHON: &[Suite] = &[
    Suite { name: "reflex-proof", root: "evals/reflex-proof/harness", tests: "evals/reflex-proof/harness" },
    Suite { name: "reflex-proof-traces", root: "evals/reflex-proof/traces", tests: "evals/reflex-proof/traces" },
    Suite { name: "live", root: ".", tests: "evals/live" },
    Suite { name: "gen", root: "evals/gen", tests: "evals/gen" },
    Suite { name: "external", root: "evals/external", tests: "evals/external" },
    Suite { name: "replay", root: "evals/replay", tests: "evals/replay" },
    Suite { name: "repobench", root: "evals/repobench", tests: "evals/repobench" },
    Suite { name: "datasets-tools", root: "evals/datasets/tools", tests: "evals/datasets/tools" },
    Suite { name: "lab-research", root: "evals/lab/research", tests: "evals/lab/research/harness/tests" },
    Suite { name: "lab-skills", root: "evals/lab/skills", tests: "evals/lab/skills" },
    Suite { name: "demo", root: "demo", tests: "demo" },
];

/// Directories whose `test_*.py` files are fixtures (the tests of sandbox repositories agents work
/// in), not Adage's tests.
#[cfg_attr(not(test), allow(dead_code))]
pub const FIXTURE_DIRS: &[&str] = &["crates/", "demo/sandbox/", "evals/sandboxes/", "spikes/", "research/"];

/// The test files of a suite, relative to its root, sorted.
pub fn files(repo: &Path, s: &Suite) -> Result<Vec<String>> {
    let dir = repo.join(s.tests);
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).with_context(|| format!("{}: {}", s.name, dir.display()))? {
        let p = e?.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.starts_with("test_") && name.ends_with(".py") {
            let rel = p.strip_prefix(repo.join(s.root)).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            out.push(rel);
        }
    }
    out.sort();
    Ok(out)
}

/// `cargo xtask test python [<suite>...]`.
pub fn run(repo: &Path, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("python") => {}
        _ => bail!("usage: cargo xtask test python [<suite>...]   (suites: {})", names()),
    }
    let wanted: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    if let Some(w) = wanted.iter().find(|w| !PYTHON.iter().any(|s| s.name == **w)) {
        bail!("no Python suite {w:?} (suites: {})", names());
    }
    let mut failed = Vec::new();
    for s in PYTHON.iter().filter(|s| wanted.is_empty() || wanted.contains(&s.name)) {
        let files = files(repo, s)?;
        eprintln!("== {} ({} files, from {})", s.name, files.len(), s.root);
        let root: PathBuf = repo.join(s.root);
        let ok = Command::new("python3")
            .args(["-m", "unittest"])
            .args(&files)
            .current_dir(&root)
            .status()
            .with_context(|| format!("running python3 for {}", s.name))?
            .success();
        if !ok {
            failed.push(s.name);
        }
    }
    if !failed.is_empty() {
        bail!("failed: {}", failed.join(", "));
    }
    Ok(())
}

fn names() -> String {
    PYTHON.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
    }

    #[test]
    fn every_python_test_belongs_to_a_suite() {
        let out = Command::new("git").args(["ls-files"]).current_dir(repo()).output().unwrap();
        let tracked = String::from_utf8_lossy(&out.stdout);
        let orphans: Vec<&str> = tracked
            .lines()
            .filter(|p| p.rsplit('/').next().is_some_and(|n| n.starts_with("test_") && n.ends_with(".py")))
            .filter(|p| !FIXTURE_DIRS.iter().any(|d| p.starts_with(d)))
            .filter(|p| !PYTHON.iter().any(|s| p.rsplit_once('/').is_some_and(|(dir, _)| dir == s.tests)))
            .collect();
        assert!(orphans.is_empty(), "Python tests in no suite (xtask/src/suites.rs, PYTHON): {orphans:?}");
    }

    #[test]
    fn every_suite_has_tests_and_unique_name() {
        let mut names: Vec<&str> = PYTHON.iter().map(|s| s.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), PYTHON.len());
        for s in PYTHON {
            assert!(!files(&repo(), s).unwrap().is_empty(), "{}: no test files in {}", s.name, s.tests);
            assert!(Path::new(s.tests).starts_with(s.root) || s.root == ".", "{}: tests outside its root", s.name);
        }
    }
}
