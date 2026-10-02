//! `cargo xtask affected [--base <ref>] [--files <path>...] [--json]`: which workspace packages a
//! change can affect, so CI tests those and only those.
//!
//! A package is affected when a file it builds from or its tests read changed:
//!
//! 1. a file inside its own directory;
//! 2. a file outside every package that it reads ([`READS`]: fixtures, sandboxes, plugin sources);
//! 3. anything that affects a package it depends on, in any role (normal, build or dev): the
//!    reverse-dependency closure. This is what makes it safe: changing `adage-trace` tests every
//!    crate that can observe the change, not just `adage-trace`.
//!
//! Anything else decides the whole workspace: a changed path that is not inside a package, not in
//! [`READS`] and not in [`INERT`] (the lockfile, the workspace manifest, CI itself, an unknown
//! directory) affects every package. Paths in [`INERT`] (prose, the web site, research notes)
//! affect none. When in doubt the answer is "everything": a path is only ever narrowed by a rule
//! here, never by omission.
//!
//! `tests::every_path_a_crate_reads_is_declared` keeps [`READS`] and [`INERT`] honest: it scans
//! every crate's sources for paths that leave the crate and fails if one points into an inert
//! directory without a rule for it.
//!
//! Output: `all`, `none`, or the affected packages one per line (`--json`: an object with `all`
//! and `packages`).

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Paths outside every package that packages read, and which packages read them. A prefix ending
/// in `/` matches a directory; any other prefix matches that file exactly.
pub const READS: &[(&str, &[&str])] = &[
    ("evals/sandboxes/", &["adage-cli", "adage-exec", "adage-learn"]),
    ("evals/datasets/", &["adage-graph", "adage-eval"]),
    ("evals/gen/", &["adage-cli", "adage-instincts"]),
    ("evals/lab/", &["adage-eval", "adage-train"]),
    ("sim/", &["adage-sim"]),
    ("plugin/", &["adage-cli", "adage-hook"]),
    ("packs/", &["adage-pack"]),
    (".claude-plugin/", &["adage-pack"]),
    ("docs/instincts.md", &["adage-instincts"]),
    ("design/", &["xtask"]),
    ("demo/sandbox/", &["adage-eval"]),
    ("web/map/e2e/fixtures/", &["adage-app"]),
];

/// Paths no package builds from or reads: changing only these tests nothing.
pub const INERT: &[&str] = &["docs/", "web/", "research/", "spikes/", "demo/", ".gitattributes", "about.hbs"];

/// Root files that are prose (`README.md`, `CHANGELOG.md`, …): inert.
fn root_prose(path: &str) -> bool {
    !path.contains('/') && path.ends_with(".md")
}

/// A workspace member: its name, its directory relative to the root, and the members it depends on.
#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub dir: String,
    /// Members it depends on in any role (normal, build, dev).
    pub deps: BTreeSet<String>,
    /// Members it depends on to build its library and binaries (normal and build dependencies):
    /// what the layering rules (`layers.rs`) constrain.
    #[cfg_attr(not(test), allow(dead_code))]
    pub links: BTreeSet<String>,
}

/// What a change affects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Affected {
    All,
    Some(BTreeSet<String>),
}

fn matches(prefix: &str, path: &str) -> bool {
    if prefix.ends_with('/') {
        path.starts_with(prefix)
    } else {
        path == prefix
    }
}

/// The packages `changed` (paths relative to the workspace root, `/`-separated) affects.
pub fn affected(members: &[Member], changed: &[String]) -> Affected {
    let mut direct: BTreeSet<String> = BTreeSet::new();
    for path in changed {
        if let Some(m) = members.iter().find(|m| path.starts_with(&format!("{}/", m.dir))) {
            direct.insert(m.name.clone());
            continue;
        }
        let read: Vec<&str> =
            READS.iter().filter(|(p, _)| matches(p, path)).flat_map(|(_, pkgs)| pkgs.iter().copied()).collect();
        if !read.is_empty() {
            direct.extend(read.into_iter().map(str::to_string));
            continue;
        }
        if INERT.iter().any(|p| matches(p, path)) || root_prose(path) {
            continue;
        }
        return Affected::All;
    }
    // Reverse-dependency closure: whoever depends on an affected package is affected.
    let mut out = direct;
    loop {
        let more: Vec<String> = members
            .iter()
            .filter(|m| !out.contains(&m.name) && m.deps.iter().any(|d| out.contains(d)))
            .map(|m| m.name.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        out.extend(more);
    }
    if !out.is_empty() && members.iter().all(|m| out.contains(&m.name)) {
        return Affected::All;
    }
    Affected::Some(out)
}

/// The workspace members, from `cargo metadata`.
pub fn members(root: &Path) -> Result<Vec<Member>> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .context("running cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let meta: Value = serde_json::from_slice(&out.stdout)?;
    let root = root.canonicalize()?;
    let mut members = Vec::new();
    let pkgs = meta["packages"].as_array().context("cargo metadata: no packages")?;
    let names: BTreeSet<&str> = pkgs.iter().filter_map(|p| p["name"].as_str()).collect();
    for p in pkgs {
        let name = p["name"].as_str().context("package without a name")?.to_string();
        let manifest = Path::new(p["manifest_path"].as_str().context("package without a manifest")?);
        let dir = manifest.parent().context("manifest without a directory")?;
        let dir = dir.strip_prefix(&root).unwrap_or(dir).to_string_lossy().replace('\\', "/");
        let internal = || {
            p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["name"].as_str().is_some_and(|n| names.contains(n)))
        };
        let deps = internal().filter_map(|d| d["name"].as_str()).map(str::to_string).collect();
        let links = internal()
            .filter(|d| d["kind"].as_str() != Some("dev"))
            .filter_map(|d| d["name"].as_str())
            .map(str::to_string)
            .collect();
        members.push(Member { name, dir, deps, links });
    }
    Ok(members)
}

/// Paths changed since the merge base of `base` and `HEAD`, plus uncommitted ones.
fn changed_since(root: &Path, base: &str) -> Result<Vec<String>> {
    let git = |args: &[&str]| -> Result<String> {
        let out = Command::new("git").args(args).current_dir(root).output().context("running git")?;
        if !out.status.success() {
            bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let merge_base = git(&["merge-base", base, "HEAD"])?;
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for args in [vec!["diff", "--name-only", merge_base.trim()], vec!["diff", "--name-only", "HEAD"]] {
        paths.extend(git(&args)?.lines().filter(|l| !l.is_empty()).map(str::to_string));
    }
    Ok(paths.into_iter().collect())
}

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let flag = |name: &str| args.iter().position(|a| a == name);
    let changed: Vec<String> = match flag("--files") {
        Some(i) => args[i + 1..].iter().take_while(|a| !a.starts_with("--")).cloned().collect(),
        None => {
            let base = flag("--base").and_then(|i| args.get(i + 1)).map(String::as_str).unwrap_or("origin/main");
            changed_since(root, base)?
        }
    };
    let result = affected(&members(root)?, &changed);
    if flag("--json").is_some() {
        let (all, packages) = match &result {
            Affected::All => (true, vec![]),
            Affected::Some(p) => (false, p.iter().cloned().collect()),
        };
        println!("{}", serde_json::json!({ "all": all, "packages": packages, "changed": changed }));
        return Ok(());
    }
    match result {
        Affected::All => println!("all"),
        Affected::Some(p) if p.is_empty() => println!("none"),
        Affected::Some(p) => p.iter().for_each(|n| println!("{n}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ws() -> Vec<Member> {
        let m = |name: &str, deps: &[&str]| Member {
            name: name.into(),
            dir: format!("crates/{name}"),
            deps: deps.iter().map(|d| d.to_string()).collect(),
            links: BTreeSet::new(),
        };
        vec![
            m("core", &[]),
            m("trace", &["core"]),
            m("ingest", &["core", "trace"]),
            m("learn", &["ingest", "trace"]),
            m("cli", &["learn"]),
            m("sign", &[]),
            m("adage-sim", &[]),
            m("adage-cli", &["adage-sim"]),
        ]
    }

    fn some(xs: &[&str]) -> Affected {
        Affected::Some(xs.iter().map(|x| x.to_string()).collect())
    }

    fn of(paths: &[&str]) -> Affected {
        affected(&ws(), &paths.iter().map(|p| p.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_change_affects_its_package_and_every_dependent() {
        assert_eq!(of(&["crates/ingest/src/lib.rs"]), some(&["cli", "ingest", "learn"]));
        assert_eq!(of(&["crates/sign/src/lib.rs"]), some(&["sign"]));
        assert_eq!(of(&["crates/cli/tests/x.rs"]), some(&["cli"]));
    }

    #[test]
    fn a_crate_named_as_a_prefix_of_another_is_not_confused_with_it() {
        let mut ws = ws();
        ws.push(Member {
            name: "core-extra".into(),
            dir: "crates/core-extra".into(),
            deps: BTreeSet::new(),
            links: BTreeSet::new(),
        });
        let got = affected(&ws, &["crates/core-extra/src/lib.rs".into()]);
        assert_eq!(got, some(&["core-extra"]));
    }

    #[test]
    fn a_read_outside_every_crate_affects_its_readers_and_their_dependents() {
        assert_eq!(of(&["sim/fixtures/codex/a.jsonl"]), some(&["adage-cli", "adage-sim"]));
    }

    #[test]
    fn inert_paths_affect_nothing() {
        assert_eq!(of(&["docs/usage.md", "README.md", "web/site/index.html"]), some(&[]));
        assert_eq!(of(&[]), some(&[]));
    }

    #[test]
    fn anything_unknown_affects_everything() {
        for p in ["Cargo.lock", "Cargo.toml", ".github/workflows/ci.yml", "scripts/x.sh", "newdir/a", "rustfmt.toml"] {
            assert_eq!(of(&["crates/sign/src/lib.rs", p]), Affected::All, "{p}");
        }
    }

    #[test]
    fn a_change_reaching_every_package_is_all() {
        let ws = vec![
            Member { name: "a".into(), dir: "crates/a".into(), deps: BTreeSet::new(), links: BTreeSet::new() },
            Member { name: "b".into(), dir: "crates/b".into(), deps: ["a".to_string()].into(), links: BTreeSet::new() },
        ];
        assert_eq!(affected(&ws, &["crates/a/src/lib.rs".into()]), Affected::All);
    }

    /// Every path a crate's code reads outside its own directory (`"../../<dir>/…"`, or the root
    /// joined with `"<dir>/…"`) lies in a declared read, or in a directory that already affects
    /// everything. None may point into an inert directory: a change there would be skipped.
    #[test]
    fn every_path_a_crate_reads_is_declared() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let members = members(root).unwrap();
        let mut undeclared: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for m in &members {
            let crate_dir = root.join(&m.dir);
            // This file names inert paths as rules, not as reads.
            for file in walk(&crate_dir).into_iter().filter(|f| !f.ends_with("xtask/src/affected.rs")) {
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                for target in outside_paths(root, &crate_dir, &file, &text) {
                    seen.insert(target.clone());
                    let declared = READS.iter().any(|(p, pkgs)| matches(p, &target) && pkgs.contains(&m.name.as_str()));
                    let inert = INERT.iter().any(|p| matches(p, &target)) || root_prose(&target);
                    if inert && !declared {
                        undeclared.entry(target).or_default().insert(m.name.clone());
                    }
                }
            }
        }
        // The scan sees the reads it must: one through `include_str!` from `src/`, one joined to
        // `CARGO_MANIFEST_DIR`. A scan that finds nothing would pass vacuously.
        for known in ["demo/sandbox/conventions.json", "docs/instincts.md", "evals/sandboxes/acme-billing"] {
            assert!(seen.contains(known), "the scan missed {known}");
        }
        assert!(
            undeclared.is_empty(),
            "crates read these inert paths without a READS rule (xtask/src/affected.rs): {undeclared:?}"
        );
    }

    /// Every READS rule names real packages.
    #[test]
    fn rules_name_real_packages() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let names: BTreeSet<String> = members(root).unwrap().into_iter().map(|m| m.name).collect();
        for (p, pkgs) in READS {
            for pkg in *pkgs {
                assert!(names.contains(*pkg), "{p}: no package {pkg}");
            }
        }
    }

    fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() && p.file_name().is_some_and(|n| n != "target" && n != "node_modules") {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Workspace-relative paths a source names outside its crate: a literal climbing out of it
    /// (`"../../docs/x.md"`, `include_str!("../../../demo/a.json")`), resolved against both the
    /// source's directory and the crate's (where `CARGO_MANIFEST_DIR` joins start); or a
    /// root-relative literal (`"docs/x.md"`) in a source that climbs to the workspace root.
    /// Comments are skipped. Over-reporting is harmless: only reads into inert paths fail.
    fn outside_paths(root: &Path, crate_dir: &Path, file: &Path, text: &str) -> Vec<String> {
        let code: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let lits = literals(&code);
        let climbs_to_root = lits.iter().any(|l| l == "../..") || code.contains("parent().expect(\"xtask");
        let mut out = Vec::new();
        for lit in lits.iter().map(String::as_str) {
            if lit.starts_with("../") {
                for base in [file.parent().unwrap_or(crate_dir), crate_dir] {
                    let resolved = normalise(&base.join(lit));
                    if let Ok(rel) = resolved.strip_prefix(root) {
                        if !resolved.starts_with(crate_dir) && rel.components().next().is_some() {
                            out.push(rel.to_string_lossy().replace('\\', "/"));
                        }
                    }
                }
            } else if climbs_to_root
                && lit.contains('/')
                && !lit.starts_with('/')
                && !lit.starts_with('.')
                && lit.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
            {
                out.push(lit.to_string());
            }
        }
        out
    }

    /// The string literals in Rust source: plain (escapes kept as written), raw (`r"…"`, `r#"…"#`),
    /// with char literals such as `'"'` skipped.
    fn literals(code: &str) -> Vec<String> {
        let b = code.as_bytes();
        let (mut out, mut i) = (Vec::new(), 0);
        while i < b.len() {
            match b[i] {
                b'\'' if i + 2 < b.len() && b[i + 2] == b'\'' => i += 3,
                b'\'' if i + 3 < b.len() && b[i + 1] == b'\\' && b[i + 3] == b'\'' => i += 4,
                b'r' if i + 1 < b.len()
                    && (b[i + 1] == b'"' || b[i + 1] == b'#')
                    && (i == 0 || !b[i - 1].is_ascii_alphanumeric() && b[i - 1] != b'_') =>
                {
                    let hashes = b[i + 1..].iter().take_while(|c| **c == b'#').count();
                    let open = i + 1 + hashes;
                    if b.get(open) != Some(&b'"') {
                        i += 1;
                        continue;
                    }
                    let close: Vec<u8> = std::iter::once(b'"').chain(std::iter::repeat_n(b'#', hashes)).collect();
                    let body = open + 1;
                    let end = b[body..]
                        .windows(close.len())
                        .position(|w| w == close.as_slice())
                        .map_or(b.len(), |p| body + p);
                    out.push(String::from_utf8_lossy(&b[body..end]).into_owned());
                    i = end + close.len();
                }
                b'"' => {
                    let mut j = i + 1;
                    while j < b.len() && b[j] != b'"' {
                        j += if b[j] == b'\\' { 2 } else { 1 };
                    }
                    out.push(String::from_utf8_lossy(&b[i + 1..j.min(b.len())]).into_owned());
                    i = j + 1;
                }
                _ => i += 1,
            }
        }
        out
    }

    #[test]
    fn literals_are_read_whole() {
        let code = r##"let q = '"'; let e = '\''; f("a\"b", r#"c"d"#, "../../x"); let r = br"y";"##;
        assert_eq!(literals(code), vec!["a\\\"b", "c\"d", "../../x", "y"]);
    }

    /// `p` with `.` and `..` resolved lexically.
    fn normalise(p: &Path) -> std::path::PathBuf {
        let mut out = std::path::PathBuf::new();
        for c in p.components() {
            match c {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                c => out.push(c),
            }
        }
        out
    }
}
