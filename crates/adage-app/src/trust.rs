//! Team packs committed in a repository (docs/zero-config.md, persona (c): `.adage/packs/<name>/`),
//! and whether one can be trusted with a press from the session (architecture §3.12): the same
//! trust as the repository's `CLAUDE.md`, so only for a pack committed there, unchanged since.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Where a repository keeps the packs a team committed.
pub const PACKS_DIR: &str = ".adage/packs";

/// The most files, and the largest file, a committed pack is compared over.
const MAX_PACK_FILES: usize = 512;
const MAX_PACK_FILE: usize = 8 << 20;

/// Whether the pack directory `src` lies inside the repository at `root` and every file in it is
/// committed at `HEAD` with exactly the bytes in the work tree (read from git's own object store,
/// spawning nothing). What `status --json` offers for trust; a press is then held to
/// [`committed_before`].
pub fn committed_pack(src: &Path, root: &Path) -> bool {
    committed(src, root, None)
}

/// [`committed_pack`], and the pack is older than the press session (review M8): the commit that
/// brought the pack's content to `HEAD` was made before `since` (ms since the epoch, when the
/// session's press window opened; the commit's whole second before it), and no file or directory of the pack has changed on disk since
/// (its status-change time, which no command short of the system clock sets; the modification
/// time where the platform has none). So a pack the agent wrote, or committed, in this session is
/// not trusted with its press. A pack pulled in during the session waits for the next one, or a
/// terminal. The commit's date is what its maker chose, so it alone would not hold: the on-disk
/// time is the check an agent cannot set back.
pub fn committed_before(src: &Path, root: &Path, since: u64) -> bool {
    committed(src, root, Some(since))
}

/// When the file at `p` last changed on disk, in ms since the epoch.
fn changed_ms(p: &Path) -> Option<u64> {
    let m = std::fs::symlink_metadata(p).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let secs = u64::try_from(m.ctime()).ok()?;
        Some(secs * 1000 + u64::try_from(m.ctime_nsec()).ok()? / 1_000_000)
    }
    #[cfg(not(unix))]
    {
        let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
        u64::try_from(t.as_millis()).ok()
    }
}

fn committed(src: &Path, root: &Path, since: Option<u64>) -> bool {
    let (Ok(src), Ok(root)) = (src.canonicalize(), root.canonicalize()) else { return false };
    let Ok(rel) = src.strip_prefix(&root) else { return false };
    let Some(mut repo) = adage_core::gitobj::Repo::open(&root) else { return false };
    let key =
        |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
    let mut files = Vec::new();
    let mut dirs = vec![src.clone()];
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { return false };
        for e in rd.flatten() {
            let Ok(t) = e.file_type() else { return false };
            if t.is_dir() {
                stack.push(e.path());
                dirs.push(e.path());
            } else if t.is_file() {
                files.push(e.path());
            } else {
                return false;
            }
            if files.len() > MAX_PACK_FILES {
                return false;
            }
        }
    }
    let same = !files.is_empty()
        && files.iter().all(|f| {
            let Ok(inside) = f.strip_prefix(&src) else { return false };
            match (std::fs::read(f), repo.committed(&key(&rel.join(inside)), MAX_PACK_FILE)) {
                (Ok(now), Some(at_head)) => now == at_head,
                _ => false,
            }
        });
    let Some(since) = since else { return same };
    same && repo.committed_at(&key(rel)).is_some_and(|secs| (secs + 1).saturating_mul(1000) <= since)
        && files.iter().chain(&dirs).all(|p| changed_ms(p).is_some_and(|t| t < since))
}

/// A committed team pack this machine has not installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Awaiting {
    /// The pack's name, from its manifest.
    pub name: String,
    /// Its directory, relative to the repository root.
    pub path: String,
    /// The command a press runs to trust it.
    pub command: String,
}

/// The packs committed under [`PACKS_DIR`] in the repository at `root` that are not installed
/// under `tracks` (`@imported/<owner>/<name>`, the pack's name being `owner/name`): what `status
/// --json` lists as awaiting trust.
pub fn awaiting(root: &Path, tracks: &Path) -> Vec<Awaiting> {
    let Ok(rd) = std::fs::read_dir(root.join(PACKS_DIR)) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|dir| {
            let text = std::fs::read_to_string(dir.join(adage_pack::validate::MANIFEST)).ok()?;
            // Only the name is read here; `adage add` validates the whole pack.
            let manifest: toml::Value = toml::from_str(&text).ok()?;
            let name = manifest.get("pack")?.get("name")?.as_str()?.to_string();
            let installed = name.split('/').count() == 2 && tracks.join("@imported").join(&name).is_dir();
            if installed || !committed_pack(&dir, root) {
                return None;
            }
            let path = dir.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
            Some(Awaiting { command: format!("adage add {path} --trust"), name, path })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "a")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "a")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// A committed pack awaits trust until it is installed; a pack changed since its commit, or
    /// only in the work tree, is never offered to a press.
    #[test]
    fn committed_packs_await_trust() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap().join("repo");
        let tracks = d.path().join("tracks");
        let pack = root.join(PACKS_DIR).join("team");
        std::fs::create_dir_all(&pack).unwrap();
        if !git(&root, &["init", "-q", "-b", "main"]) {
            eprintln!("git not available: skipping");
            return;
        }
        let manifest = "format = \"adage-pack/1\"\n[pack]\nname = \"alice/team\"\nversion = \"1.0.0\"\n";
        std::fs::write(pack.join(adage_pack::validate::MANIFEST), manifest).unwrap();
        assert!(!committed_pack(&pack, &root), "not committed yet");
        assert!(git(&root, &["add", "."]) && git(&root, &["commit", "-q", "-m", "team"]));
        assert!(committed_pack(&pack, &root));
        assert_eq!(
            awaiting(&root, &tracks),
            vec![Awaiting {
                name: "alice/team".into(),
                path: ".adage/packs/team".into(),
                command: "adage add .adage/packs/team --trust".into()
            }]
        );
        std::fs::create_dir_all(tracks.join("@imported/alice/team")).unwrap();
        assert!(awaiting(&root, &tracks).is_empty(), "installed");
        std::fs::write(pack.join("extra"), "x").unwrap();
        assert!(!committed_pack(&pack, &root), "a file only in the work tree");
        std::fs::remove_file(pack.join("extra")).unwrap();
        std::fs::write(pack.join(adage_pack::validate::MANIFEST), "changed").unwrap();
        assert!(!committed_pack(&pack, &root), "changed since its commit");
        assert!(!committed_pack(d.path(), &root), "outside the repository");
    }

    /// Review M8: a press holds a committed pack to the session's start: one committed (and so
    /// written) before the session opened is trusted, one written or committed since is not,
    /// whatever date its commit claims.
    #[test]
    fn a_pack_committed_in_the_session_is_not_trusted_with_its_press() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap().join("repo");
        let pack = root.join(PACKS_DIR).join("team");
        std::fs::create_dir_all(&pack).unwrap();
        if !git(&root, &["init", "-q", "-b", "main"]) {
            eprintln!("git not available: skipping");
            return;
        }
        std::fs::write(pack.join(adage_pack::validate::MANIFEST), "[pack]\nname = \"alice/team\"\n").unwrap();
        assert!(git(&root, &["add", "."]) && git(&root, &["commit", "-q", "-m", "team"]));
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let opened = adage_core::time::now_ms();
        assert!(committed_pack(&pack, &root));
        assert!(committed_before(&pack, &root, opened), "committed before the session");
        // In the session: a second pack written and committed, its commit dated a year back. (The
        // pause: a file's change time comes from a clock that may lag the wall clock by a tick.)
        std::thread::sleep(std::time::Duration::from_millis(50));
        let evil = root.join(PACKS_DIR).join("evil");
        std::fs::create_dir_all(&evil).unwrap();
        std::fs::write(evil.join(adage_pack::validate::MANIFEST), "[pack]\nname = \"mallory/evil\"\n").unwrap();
        let old = "2025-01-01T00:00:00Z";
        assert!(git(&root, &["add", "."]));
        let ok = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["-c", "user.name=a", "-c", "user.email=a@b", "commit", "-q", "--date", old, "-m", "evil"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_COMMITTER_DATE", old)
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(ok);
        assert!(committed_pack(&evil, &root), "committed, so offered");
        assert!(!committed_before(&evil, &root, opened), "written in the session");
        assert!(committed_before(&pack, &root, opened), "the earlier pack is untouched");
        // Committed in the session, its files older: the commit's own time refuses it.
        let late = root.join(PACKS_DIR).join("late");
        std::fs::create_dir_all(&late).unwrap();
        std::fs::write(late.join(adage_pack::validate::MANIFEST), "[pack]\nname = \"bob/late\"\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let opened = adage_core::time::now_ms();
        assert!(git(&root, &["add", "."]) && git(&root, &["commit", "-q", "-m", "late"]));
        assert!(!committed_before(&late, &root, opened), "committed in the session");
    }
}
