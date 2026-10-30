//! The previous binaries: before `adage update` replaces anything, the running `adage` and the
//! `adage-exec` and `adage-train` beside it are copied to `$ADAGE_HOME/previous/<version>/`, each
//! with its SHA-256, and `previous.json` records where each came from; `adage update --rollback`
//! puts them back, byte for byte.
//!
//! ```json
//! {"from": "0.1.0", "to": "0.2.0", "channel": "npm", "at": 1759800000000, "via": "terminal",
//!  "bin": "/usr/local/lib/node_modules/@adage-sh/cli-linux-x64/bin/adage",
//!  "paths": {"adage": "/usr/local/lib/node_modules/@adage-sh/cli-linux-x64/bin/adage", "adage-exec": "…"},
//!  "sha256": {"adage": "…", "adage-exec": "…"}, "restored_at": null}
//! ```
//!
//! `to` is the version the update installed (known once it is verified), `bin` where it put
//! `adage`. One set per version is kept; a successful update removes the sets of other versions.
//! Every file is written beside its destination and renamed into place, so a binary is never
//! written in place (a running one, or one hard-linked elsewhere, stays whole).

use super::channel::{exe_name, BINARIES};
use crate::net::{make_executable, sha256_file};
use adage_store::Paths;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) const FILE: &str = "previous.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Previous {
    pub(crate) from: String,
    #[serde(default)]
    pub(crate) to: Option<String>,
    pub(crate) channel: String,
    pub(crate) at: u64,
    /// `terminal`, or `press` when a person's press in the Claude Code mod ran the update.
    #[serde(default)]
    pub(crate) via: String,
    /// Where the update put `adage`: registrations are re-pointed to it.
    #[serde(default)]
    pub(crate) bin: Option<PathBuf>,
    /// Each kept binary's original path, by name.
    pub(crate) paths: BTreeMap<String, PathBuf>,
    pub(crate) sha256: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) restored_at: Option<u64>,
}

/// The directory a version's set is kept in (a version is a few safe characters; anything else is
/// replaced, so the directory stays under `previous/`).
pub(crate) fn dir(paths: &Paths, version: &str) -> PathBuf {
    let safe: String = version
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+') { c } else { '_' })
        .collect();
    paths.previous().join(if safe.is_empty() || safe.starts_with('.') { format!("_{safe}") } else { safe })
}

pub(crate) fn read(dir: &Path) -> Option<Previous> {
    serde_json::from_slice(&std::fs::read(dir.join(FILE)).ok()?).ok()
}

pub(crate) fn write(dir: &Path, p: &Previous) -> Result<()> {
    let body = serde_json::to_vec_pretty(p)?;
    adage_store::config::write_atomic(&dir.join(FILE), &body)
        .with_context(|| format!("cannot write {}", dir.join(FILE).display()))
}

/// The binaries of the install `exe` belongs to: `adage` itself, and each companion beside it.
pub(crate) fn installed(exe: &Path) -> Vec<(String, PathBuf)> {
    let dir = exe.parent().unwrap_or(exe);
    BINARIES
        .iter()
        .map(|&n| (n.to_string(), if n == "adage" { exe.to_path_buf() } else { dir.join(exe_name(n)) }))
        .filter(|(_, p)| p.is_file())
        .collect()
}

/// Keep the install's binaries as version `have`'s set: copied, then checked against the
/// originals' digests, then recorded. Replaces a set kept earlier for the same version.
pub(crate) fn keep(
    paths: &Paths,
    have: &str,
    channel: &str,
    via: &str,
    exe: &Path,
    now: u64,
) -> Result<(PathBuf, Previous)> {
    let dest = dir(paths, have);
    std::fs::create_dir_all(paths.previous())
        .with_context(|| format!("cannot create {}", paths.previous().display()))?;
    let tmp = paths.previous().join(format!(".keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let mut p =
        Previous { from: have.into(), channel: channel.into(), at: now, via: via.into(), ..Previous::default() };
    let made = (|| -> Result<()> {
        for (name, from) in installed(exe) {
            let want = sha256_file(&from).with_context(|| format!("cannot read {}", from.display()))?;
            let to = tmp.join(exe_name(&name));
            std::fs::copy(&from, &to).with_context(|| format!("cannot copy {} to {}", from.display(), to.display()))?;
            let got = sha256_file(&to)?;
            if got != want {
                bail!("the copy of {} does not match it (sha256 {got}, not {want})", from.display());
            }
            p.paths.insert(name.clone(), from);
            p.sha256.insert(name, want);
        }
        write(&tmp, &p)
    })();
    if let Err(e) = made {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&tmp, &dest).with_context(|| format!("cannot move the kept binaries to {}", dest.display()))?;
    Ok((dest, p))
}

/// The set kept most recently, with its directory.
pub(crate) fn newest(paths: &Paths) -> Option<(PathBuf, Previous)> {
    std::fs::read_dir(paths.previous())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|d| !d.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .filter_map(|d| read(&d).map(|p| (d, p)))
        .max_by_key(|(_, p)| p.at)
}

/// Put a kept set back where it came from. Each kept file is checked against its recorded digest
/// before it is used, written beside its destination and renamed into place, then checked again.
/// Returns what was restored, by name and path.
pub(crate) fn restore(dir: &Path, p: &Previous) -> Result<Vec<(String, PathBuf)>> {
    // A damaged manifest must not turn an unchecked file into a replacement. Validate the
    // complete set before touching a destination; checking only the available hashes would
    // discover a missing digest after the corresponding binary had already been replaced.
    validate_restore(p)?;
    // Every kept file is checked before anything is replaced. Publication is atomic per binary,
    // not across the set: an I/O failure during replacement can still require a retry.
    for (name, want) in &p.sha256 {
        let kept = dir.join(exe_name(name));
        let got = sha256_file(&kept).with_context(|| format!("the kept {name} is missing from {}", dir.display()))?;
        if &got != want {
            bail!("the kept {name} in {} has changed since it was kept (sha256 {got}, not {want})", dir.display());
        }
    }
    // Resolve parent directories before publishing any file. Different absolute paths can
    // name the same directory through a symlink, so lexical uniqueness alone is insufficient.
    let mut destinations = std::collections::BTreeSet::new();
    for to in p.paths.values() {
        let parent = to.parent().context("a kept path has no directory")?;
        std::fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
        let destination = parent.canonicalize()?.join(to.file_name().context("a kept path has no filename")?);
        // Be conservative on every platform: a manifest must also be safe on the default
        // case-insensitive macOS/Windows filesystems, even if neither target exists yet.
        if !destinations.insert(destination.as_os_str().to_ascii_lowercase()) {
            bail!("invalid rollback manifest: multiple binaries share a destination");
        }
    }
    let mut done = Vec::new();
    for (name, to) in &p.paths {
        let kept = dir.join(exe_name(name));
        let parent = to.parent().context("a kept path has no directory")?;
        std::fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
        let tmp = parent.join(format!(".{}.adage-restore-{}", exe_name(name), std::process::id()));
        std::fs::copy(&kept, &tmp).with_context(|| format!("cannot write in {}", parent.display()))?;
        let got = sha256_file(&tmp)?;
        if p.sha256.get(name) != Some(&got) {
            let _ = std::fs::remove_file(&tmp);
            bail!("the staged {name} does not match its kept digest; {} was not replaced", to.display());
        }
        make_executable(&tmp)?;
        replace(&tmp, to)?;
        let got = sha256_file(to)?;
        if p.sha256.get(name) != Some(&got) {
            bail!("{} does not match the kept {name} after restoring it (sha256 {got})", to.display());
        }
        done.push((name.clone(), to.clone()));
    }
    Ok(done)
}

/// Manifest integrity is checked independently of the files it names. Missing or extra hashes,
/// unknown binary names and ambiguous destinations are corruption, not a partial restore plan.
fn validate_restore(p: &Previous) -> Result<()> {
    if !p.paths.contains_key("adage") || !p.paths.keys().eq(p.sha256.keys()) {
        bail!("invalid rollback manifest: each binary needs a destination and digest, including adage");
    }
    let mut destinations = std::collections::BTreeSet::new();
    for (name, to) in &p.paths {
        if !BINARIES.contains(&name.as_str()) {
            bail!("invalid rollback manifest: unknown binary {name:?}");
        }
        if !to.is_absolute()
            || to.file_name().is_none()
            || to.components().any(|c| matches!(c, std::path::Component::ParentDir))
            || !destinations.insert(to)
        {
            bail!("invalid rollback manifest: {name} needs a unique absolute destination");
        }
        let digest = &p.sha256[name];
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            bail!("invalid rollback manifest: {name} has an invalid SHA-256 digest");
        }
    }
    Ok(())
}

/// Rename `tmp` over `to`. Windows cannot replace a running executable, but it can rename one:
/// the old file is moved aside first (and removed when it can be).
fn replace(tmp: &Path, to: &Path) -> Result<()> {
    #[cfg(windows)]
    if to.exists() {
        let aside = to.with_extension(format!("old-{}", std::process::id()));
        std::fs::rename(to, &aside).with_context(|| format!("cannot move {} aside", to.display()))?;
        let _ = std::fs::remove_file(&aside);
    }
    std::fs::rename(tmp, to).with_context(|| format!("cannot replace {}", to.display()))
}

/// Remove every kept set but `version`'s (after a successful update, one way back is kept).
pub(crate) fn prune(paths: &Paths, version: &str) {
    let keep = dir(paths, version);
    for e in std::fs::read_dir(paths.previous()).into_iter().flatten().flatten() {
        let d = e.path();
        if d != keep && e.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_set_comes_back_byte_for_byte() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(exe_name("adage"));
        std::fs::write(&exe, b"old adage").unwrap();
        std::fs::write(bin.join(exe_name("adage-exec")), b"old exec").unwrap();
        let (kept, p) = keep(&paths, "0.1.0", "npm", "terminal", &exe, 7).unwrap();
        assert_eq!(kept, paths.previous().join("0.1.0"));
        assert_eq!(p.paths.keys().collect::<Vec<_>>(), ["adage", "adage-exec"], "only what is there");
        assert_eq!(read(&kept), Some(p.clone()));
        assert_eq!(newest(&paths).map(|(d, _)| d), Some(kept.clone()));
        // The update replaces them (a new file renamed into place, as package managers do).
        std::fs::remove_file(&exe).unwrap();
        std::fs::write(&exe, b"new adage").unwrap();
        std::fs::write(bin.join(exe_name("adage-exec")), b"new exec").unwrap();
        let restored = restore(&kept, &p).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(std::fs::read(&exe).unwrap(), b"old adage");
        assert_eq!(std::fs::read(bin.join(exe_name("adage-exec"))).unwrap(), b"old exec");
        // A kept file that changed since is refused, and nothing is replaced.
        std::fs::write(kept.join(exe_name("adage-exec")), b"tampered").unwrap();
        std::fs::write(&exe, b"new adage").unwrap();
        assert!(restore(&kept, &p).unwrap_err().to_string().contains("has changed since it was kept"));
        assert_eq!(std::fs::read(&exe).unwrap(), b"new adage", "all of the set or none of it");
    }

    #[test]
    fn a_version_names_a_directory_under_previous() {
        let paths = Paths::at("/data");
        assert_eq!(dir(&paths, "0.1.0"), Path::new("/data/previous/0.1.0"));
        assert_eq!(dir(&paths, "../../etc"), Path::new("/data/previous/_.._.._etc"));
        assert_eq!(dir(&paths, ""), Path::new("/data/previous/_"));
    }

    #[test]
    fn corrupt_restore_manifests_never_replace_installed_binaries() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(exe_name("adage"));
        let companion = bin.join(exe_name("adage-exec"));
        std::fs::write(&exe, b"old adage").unwrap();
        std::fs::write(&companion, b"old exec").unwrap();
        let (kept, original) = keep(&paths, "0.1.0", "npm", "terminal", &exe, 7).unwrap();
        std::fs::write(&exe, b"new adage").unwrap();
        std::fs::write(&companion, b"new exec").unwrap();

        let mut cases = Vec::new();
        let mut p = original.clone();
        p.sha256.remove("adage-exec");
        cases.push(("missing digest", p));
        let mut p = original.clone();
        p.paths.remove("adage-exec");
        cases.push(("orphan digest", p));
        let mut p = original.clone();
        p.paths.insert("other".into(), bin.join("other"));
        p.sha256.insert("other".into(), "a".repeat(64));
        cases.push(("unknown binary", p));
        let mut p = original.clone();
        p.paths.insert("adage-exec".into(), exe.clone());
        cases.push(("duplicate destination", p));
        let mut p = original.clone();
        p.paths.insert("adage-exec".into(), PathBuf::from("relative"));
        cases.push(("relative destination", p));
        let mut p = original.clone();
        p.paths.insert("adage-exec".into(), bin.join("sub").join("..").join(exe_name("adage")));
        cases.push(("parent-relative alias", p));
        let mut p = original.clone();
        p.sha256.insert("adage-exec".into(), "not-a-digest".into());
        cases.push(("invalid digest", p));
        cases.push(("empty set", Previous::default()));

        for (case, p) in cases {
            assert!(restore(&kept, &p).unwrap_err().to_string().contains("invalid rollback manifest"), "{case}");
            assert_eq!(std::fs::read(&exe).unwrap(), b"new adage", "{case}");
            assert_eq!(std::fs::read(&companion).unwrap(), b"new exec", "{case}");
            assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 2, "no staging files for {case}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn restore_refuses_destinations_aliased_through_parent_symlinks() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("adage");
        std::fs::write(&exe, b"old adage").unwrap();
        std::fs::write(bin.join("adage-exec"), b"old exec").unwrap();
        let (kept, mut p) = keep(&paths, "0.1.0", "npm", "terminal", &exe, 7).unwrap();
        std::fs::write(&exe, b"new adage").unwrap();
        let alias = d.path().join("alias");
        std::os::unix::fs::symlink(&bin, &alias).unwrap();
        p.paths.insert("adage-exec".into(), alias.join("adage"));

        assert!(restore(&kept, &p).unwrap_err().to_string().contains("share a destination"));
        assert_eq!(std::fs::read(&exe).unwrap(), b"new adage");
    }

    #[test]
    fn restore_refuses_case_aliases_even_before_destination_files_exist() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(exe_name("adage"));
        std::fs::write(&exe, b"old adage").unwrap();
        std::fs::write(bin.join(exe_name("adage-exec")), b"old exec").unwrap();
        let (kept, mut p) = keep(&paths, "0.1.0", "npm", "terminal", &exe, 7).unwrap();
        let dest = d.path().join("new-bin");
        p.paths.insert("adage".into(), dest.join(exe_name("adage")));
        p.paths.insert("adage-exec".into(), dest.join(exe_name("adage").to_ascii_uppercase()));

        assert!(restore(&kept, &p).unwrap_err().to_string().contains("share a destination"));
        assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 0, "nothing was published");
    }
}
