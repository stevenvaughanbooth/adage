//! The glob lattice (Track G §6.2): an artifact prototype's member paths become the least general
//! glob that covers them and is precise over the repository tree.
//!
//! Candidates, least general first: `dir/*.ext` (the members' one directory), then
//! `p/**/*.ext` (their longest common directory prefix `p`, never empty, so `**/*.ext` is never
//! produced). A candidate's **precision** is the share of the tree's files it matches that some
//! trace touched the same way (edited, for an edit item); the least general candidate with
//! precision ≥ `min_precision` (0.9) wins. Members without a common prefix, or whose common glob
//! is not precise, split by directory into several globs. A directory glob that is not precise
//! either falls back to the member files themselves; so do files at the repository root and files
//! with no extension (`requirements.txt`, `Makefile`), which never widen.
//!
//! The lattice starts at `dir/*.ext`, not at the single file: one edited file stands for its
//! directory's files of its kind, as the retired finish-line miner's `glob_for` always did (a
//! deviation kept for parity: the miners' routes and ids are reproduced exactly).
//!
//! The tree is every path the traces read or edited, plus any repository files the caller passes
//! (`Corpus::of`), so the result is a pure function of the traces (and that list). `adage learn`
//! passes none in G3: over the touched paths alone a directory glob is always precise, so the
//! lattice gives the retired finish-line miner's `dir/*.ext` (and `p/**/*.ext` across sibling
//! directories, whose evidence the caller then counts on that glob). A repository tree makes the
//! precision bar bite (a directory most of whose files no trace touched keeps its files); G4 can
//! pass one once the index records it.

use crate::select::glob_match;
use std::collections::{BTreeMap, BTreeSet};

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn ext(path: &str) -> Option<&str> {
    let (_, file) = split(path);
    match file.rsplit_once('.') {
        Some((stem, e)) if !stem.is_empty() && !e.is_empty() => Some(e),
        _ => None,
    }
}

/// Precision of `glob` over `tree`: matched files that are in `touched`, over matched files.
pub fn precision(glob: &str, tree: &BTreeSet<String>, touched: &BTreeSet<String>) -> f64 {
    let matched: Vec<&String> = tree.iter().filter(|f| glob_match(glob, f)).collect();
    if matched.is_empty() {
        return 0.0;
    }
    matched.iter().filter(|f| touched.contains(f.as_str())).count() as f64 / matched.len() as f64
}

/// The globs for `members` (repository-relative paths), sorted. `tree` should hold the members.
pub fn lattice(
    members: &[&str],
    tree: &BTreeSet<String>,
    touched: &BTreeSet<String>,
    min_precision: f64,
) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let ok = |g: &str| precision(g, tree, touched) >= min_precision;
    // By extension: files of different kinds are different routes.
    let mut by_ext: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for m in members.iter().copied().filter(|m| !m.is_empty() && !m.starts_with('/') && !m.contains("..")) {
        match ext(m) {
            Some(e) if !split(m).0.is_empty() => by_ext.entry(e).or_default().push(m),
            // Root files and files without an extension never widen.
            _ => {
                out.insert(m.to_string());
            }
        }
    }
    for (e, files) in by_ext {
        let mut dirs: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for f in &files {
            dirs.entry(split(f).0).or_default().push(f);
        }
        let dir_glob = |d: &str, fs: &[&str], out: &mut BTreeSet<String>| {
            let g = format!("{d}/*.{e}");
            if ok(&g) {
                out.insert(g);
            } else {
                out.extend(fs.iter().map(|f| f.to_string()));
            }
        };
        if dirs.len() == 1 {
            let (d, fs) = dirs.into_iter().next().expect("one directory");
            dir_glob(d, &fs, &mut out);
            continue;
        }
        // The longest common directory prefix, by segments.
        let segs: Vec<Vec<&str>> = dirs.keys().map(|d| d.split('/').collect()).collect();
        let mut prefix: Vec<&str> = segs[0].clone();
        for s in &segs[1..] {
            let n = prefix.iter().zip(s).take_while(|(a, b)| a == b).count();
            prefix.truncate(n);
        }
        let general = (!prefix.is_empty()).then(|| format!("{}/**/*.{e}", prefix.join("/")));
        match general.filter(|g| ok(g)) {
            Some(g) => {
                out.insert(g);
            }
            None => {
                for (d, fs) in dirs {
                    dir_glob(d, &fs, &mut out);
                }
            }
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn least_general_precise_glob() {
        let tree = set(&["schema/a.json", "schema/b.json", "src/x.py", "packages/a/core.py", "packages/b/core.py"]);
        let edited = set(&["schema/a.json", "schema/b.json", "packages/a/core.py", "packages/b/core.py"]);
        assert_eq!(lattice(&["schema/a.json"], &tree, &edited, 0.9), ["schema/*.json"]);
        assert_eq!(lattice(&["packages/a/core.py", "packages/b/core.py"], &tree, &edited, 0.9), ["packages/**/*.py"]);
        // No common prefix: one glob per directory, never `**/*.ext`.
        let tree2 = set(&["api/x.proto", "rpc/y.proto"]);
        assert_eq!(lattice(&["api/x.proto", "rpc/y.proto"], &tree2, &tree2, 0.9), ["api/*.proto", "rpc/*.proto"]);
        // Imprecise: most of the directory was never edited, so the files stand alone.
        let tree3 = set(&["lib/a.py", "lib/b.py", "lib/c.py", "lib/d.py"]);
        assert_eq!(lattice(&["lib/a.py"], &tree3, &set(&["lib/a.py"]), 0.9), ["lib/a.py"]);
        // Root files and files without an extension never widen.
        assert_eq!(lattice(&["requirements.txt", "Makefile"], &tree, &edited, 0.9), ["Makefile", "requirements.txt"]);
    }
}
