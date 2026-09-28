//! Repository identity and repo-relative paths.

use crate::gitobj;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Walk up from `start` to the nearest directory containing `.git`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// A stable repository key.
///
/// - With an `origin` remote: the normalised `origin` URL. Clones of the same repository in
///   different directories share a key.
/// - Without one: `<directory name>@<root commit>`, the first 12 hex digits of the root of
///   `HEAD`'s first-parent history, read from the object store without running git
///   ([`crate::gitobj`]). Unrelated repositories that happen to share a directory name get
///   different keys; copies of one repository share a key.
/// - If the root commit cannot be read (no commits yet, a SHA-256 or shallow repository,
///   alternates, a history longer than [`gitobj::MAX_WALK`]), or the directory is not a git
///   repository: `<directory name>@path-<12 hex digits of SHA-256 of the root's path>`. Such a
///   key is not shared with any copy.
/// - A directory that no longer exists (history of a deleted checkout, read by `adage condition`):
///   the directory name alone, the key every repository without `origin` had before. Hooks accept
///   such a *legacy* key only while no repository of that name has a new key ([`lookup_key`]).
pub fn repo_key(root: &Path) -> String {
    if let Some(main) = removed_worktree(root) {
        return repo_key(&main);
    }
    origin_key(root).unwrap_or_else(|| no_origin_key(root, CACHE.get().map(PathBuf::as_path)))
}

/// The repository a removed Claude Code agent worktree belonged to: for a directory that no
/// longer exists at `<repo>/.claude/worktrees/<name>` (from `claude --worktree` or a sub-agent's
/// isolation), `<repo>`, when it is still a repository. Traces recorded there keep their
/// repository's key; a worktree that still exists is keyed through its common git directory
/// ([`origin_key`]).
pub fn removed_worktree(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        return None;
    }
    let s = path.to_string_lossy();
    let marker = "/.claude/worktrees/";
    let i = s.find(marker)?;
    s[i + marker.len()..].split('/').next().filter(|n| !n.is_empty())?;
    let main = PathBuf::from(&s[..i]);
    main.join(".git").exists().then_some(main)
}

/// The one derivation of the key a repository's tracks are stored under, used by every path that
/// learns, binds or shares (`adage condition`, `adage condition --instincts-only`, `adage share`, the
/// importers): the explicit key a person gave (`--repo <key>`) when there is one, used exactly as
/// given, else [`repo_key`] of the root. `derive` computes the latter (so a caller can remember
/// keys per root); [`key_for`] is the plain form.
pub fn key_with(explicit: Option<&str>, derive: impl FnOnce() -> String) -> String {
    match explicit.map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => k.to_string(),
        None => derive(),
    }
}

/// [`key_with`] for a root: `explicit`, else [`repo_key`]`(root)`.
pub fn key_for(root: &Path, explicit: Option<&str>) -> String {
    key_with(explicit, || repo_key(root))
}

/// Where [`repo_key`] remembers root commits for the rest of this process. The CLI sets
/// `$ADAGE_HOME/repo-keys`, so the history `adage condition` walks is not walked again by a hook.
static CACHE: OnceLock<PathBuf> = OnceLock::new();

/// Remember root commits under `dir` for the rest of this process (first call wins).
pub fn use_cache(dir: PathBuf) {
    let _ = CACHE.set(dir);
}

/// [`repo_key`], remembering a root commit once found in `cache` (a directory), so that a long
/// history is walked once, not at every hook. A remembered root commit is used only while the
/// repository's object store still holds it.
pub fn repo_key_cached(root: &Path, cache: &Path) -> String {
    if let Some(main) = removed_worktree(root) {
        return repo_key_cached(&main, cache);
    }
    origin_key(root).unwrap_or_else(|| no_origin_key(root, Some(cache)))
}

/// The key under which a hook looks up routes for `root`, given the repository keys the index
/// holds routes for; `None` when nothing applies. Without `origin`, the object store is read only
/// if the index holds routes for some repository of this directory name.
///
/// A legacy key (the bare directory name, from before keys carried the root commit) is accepted
/// only while the index holds no new-style key for that name. Once `adage condition` has re-keyed a
/// repository of that name, a legacy key could belong to any repository of the name, so it is
/// ignored: that is the collision the new keys exist to prevent.
pub fn lookup_key(root: &Path, cache: Option<&Path>, keys: &BTreeSet<&str>) -> Option<String> {
    if let Some(o) = origin_key(root) {
        return keys.contains(o.as_str()).then_some(o);
    }
    let name = checkout_name(root);
    let prefix = format!("{name}@");
    let scoped = keys
        .range::<str, _>((Bound::Included(prefix.as_str()), Bound::Unbounded))
        .next()
        .is_some_and(|k| k.starts_with(&prefix));
    if scoped {
        let key = no_origin_key(root, cache);
        return keys.contains(key.as_str()).then_some(key);
    }
    keys.contains(name.as_str()).then_some(name)
}

/// The `origin` remote's normalised URL, from the repository's config.
pub fn origin_key(root: &Path) -> Option<String> {
    // A linked worktree's `.git` is a file; the shared config is in the common directory, so a
    // worktree has its repository's key.
    let cfg = match std::fs::read_to_string(root.join(".git/config")) {
        Ok(c) => c,
        Err(_) => std::fs::read_to_string(crate::gitobj::dirs(root)?.1.join("config")).ok()?,
    };
    let mut in_origin = false;
    for line in cfg.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_origin = l == r#"[remote "origin"]"#;
        } else if in_origin {
            if let Some(url) = l.strip_prefix("url =") {
                return Some(normalise_origin(url.trim()));
            }
        }
    }
    None
}

/// The repository's directory name: its whole key before keys carried the root commit.
pub fn dir_name(root: &Path) -> String {
    root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// The name a repository without `origin` is keyed by: its directory's name, or, for a linked
/// worktree (`.git` a `gitdir:` file), the name of the main checkout whose `.git` is its common
/// directory. So `app` and its worktree `.claude/worktrees/agent-1` share `app@<root commit>`.
pub fn checkout_name(root: &Path) -> String {
    if root.join(".git").is_file() {
        if let Some((_, common)) = gitobj::dirs(root) {
            let common = lexical_normalise(&common);
            if common.file_name().is_some_and(|n| n == ".git") {
                if let Some(main) = common.parent().filter(|m| m.file_name().is_some()) {
                    return dir_name(main);
                }
            }
        }
    }
    dir_name(root)
}

fn no_origin_key(root: &Path, cache: Option<&Path>) -> String {
    let name = checkout_name(root);
    if !root.is_dir() {
        return name;
    }
    match root_commit(root, cache) {
        Some(c) => format!("{name}@{}", &c[..12]),
        None => format!("{name}@path-{}", short_hash(&root.to_string_lossy())),
    }
}

/// Root commits found in this process, by repository path: `adage condition` asks once per session.
static FOUND: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// The root commit (40 hex digits): through the cache directory when one is given (hooks, one
/// repository per process), else remembered in this process (`adage condition`, many sessions per
/// repository).
fn root_commit(root: &Path, cache: Option<&Path>) -> Option<String> {
    let path = root.to_string_lossy().to_string();
    if let Some(cache) = cache {
        return root_commit_cached(root, &path, cache);
    }
    if let Some(c) = FOUND.lock().ok().and_then(|m| m.get(&path).cloned()) {
        return Some(c);
    }
    let c = gitobj::hex(&gitobj::Repo::open(root)?.root_commit()?);
    if let Ok(mut m) = FOUND.lock() {
        m.insert(path, c.clone());
    }
    Some(c)
}

fn root_commit_cached(root: &Path, path: &str, cache: &Path) -> Option<String> {
    let mut repo = gitobj::Repo::open(root)?;
    // One file per repository path: the path, then the root commit.
    let file = cache.join(short_hash(path));
    if let Ok(text) = std::fs::read_to_string(&file) {
        let mut lines = text.lines();
        if let (Some(p), Some(c)) = (lines.next(), lines.next()) {
            if p == path && gitobj::parse_hex(c).is_some_and(|id| repo.has(&id)) {
                return Some(c.to_string());
            }
        }
    }
    let c = gitobj::hex(&repo.root_commit()?);
    // Best effort: a cache that cannot be written only costs the walk next time.
    let _ = std::fs::create_dir_all(cache);
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, format!("{path}\n{c}\n")).is_ok() {
        let _ = std::fs::rename(&tmp, &file);
    }
    Some(c)
}

fn normalise_origin(url: &str) -> String {
    let u = url.trim_end_matches(".git").trim_end_matches('/');
    let u = u.strip_prefix("git@").map(|r| r.replacen(':', "/", 1)).unwrap_or_else(|| u.to_string());
    let u = u.split("://").last().unwrap_or(&u).to_string();
    u.to_lowercase()
}

pub fn short_hash(s: &str) -> String {
    let d = Sha256::digest(s.as_bytes());
    d.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Make `path` relative to `root` if it lies inside it. Lexical only — no filesystem access.
/// Repository-relative paths always use `/`, on Windows too, so tracks are portable.
pub fn relativize(root: &Path, path: &str) -> String {
    // Compared as text, the same on every platform: Windows paths arrive with `\\?\` prefixes
    // (from `canonicalize`), mixed `/` and `\\` separators, and either drive-letter case, and in a
    // `\\?\` path `/` is not a separator, so `Path::strip_prefix` would miss the root.
    let root_parts = path_parts(&root.to_string_lossy());
    let parts = path_parts(path);
    if parts.len() >= root_parts.len() && parts[..root_parts.len()] == root_parts[..] && !root_parts.is_empty() {
        parts[root_parts.len()..].join("/")
    } else {
        let joined = parts.join("/");
        if path.starts_with('/') {
            format!("/{joined}")
        } else {
            joined
        }
    }
}

/// A path's components, lexically normalised (`.` dropped, `..` applied). A Windows drive path
/// (`C:\x`, `c:/x`, `\\?\C:\x`) is split on both separators with its drive letter lower-cased;
/// anything else is split on `/` only (a backslash in a Unix path is an ordinary character).
pub(crate) fn path_parts(s: &str) -> Vec<String> {
    let s = s.strip_prefix(r"\\?\").unwrap_or(s);
    let windows = matches!(s.as_bytes(), [d, b':', ..] if d.is_ascii_alphabetic());
    let mut out: Vec<String> = Vec::new();
    for (i, c) in s.split(|ch| ch == '/' || (windows && ch == '\\')).enumerate() {
        match c {
            "" | "." => {}
            ".." => {
                // Never climb above the drive.
                if !(windows && out.len() == 1) {
                    out.pop();
                }
            }
            drive if windows && i == 0 => out.push(drive.to_ascii_lowercase()),
            other => out.push(other.to_string()),
        }
    }
    out
}

pub fn lexical_normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True if `rel` is a safe repository-relative path: no absolute path (nor, on Windows, a drive or
/// UNC prefix), no `..`, no leading `-`.
pub fn is_safe_relative(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.starts_with('\\')
        && !rel.starts_with('-')
        && !rel.starts_with('~')
        && !Path::new(rel)
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_) | Component::RootDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relativizes_and_rejects_escapes() {
        assert_eq!(relativize(Path::new("/r/repo"), "/r/repo/acme/billing.py"), "acme/billing.py");
        assert!(is_safe_relative("acme/billing.py"));
        assert!(!is_safe_relative("../etc/passwd"));
        assert!(!is_safe_relative("--config=/x"));
        assert!(!is_safe_relative(r"\Windows\system.ini"));
        #[cfg(windows)]
        {
            assert!(!is_safe_relative(r"C:\Windows\system.ini"));
            assert!(!is_safe_relative(r"acme\..\..\secret"));
            assert_eq!(relativize(Path::new(r"C:\r\repo"), r"C:\r\repo\acme\billing.py"), "acme/billing.py");
        }
    }

    #[test]
    fn relativizes_windows_paths_on_any_platform() {
        let root = Path::new(r"\\?\C:\Users\a\acme-billing");
        for p in [
            r"\\?\C:\Users\a\acme-billing/schema/invoice.json",
            r"C:\Users\a\acme-billing\schema\invoice.json",
            r"c:/Users/a/acme-billing/schema/invoice.json",
            r"C:\Users\a\acme-billing\src\..\schema\.\invoice.json",
        ] {
            assert_eq!(relativize(root, p), "schema/invoice.json", "{p}");
        }
        assert_eq!(relativize(root, "schema/invoice.json"), "schema/invoice.json");
        assert_eq!(relativize(root, r"C:\Users\a\other\x.json"), "c:/Users/a/other/x.json");
        assert_eq!(relativize(Path::new("/r/repo"), "/r/other/x"), "/r/other/x");
        assert_eq!(relativize(Path::new("/r/repo"), "/r/repo/a/../b"), "b");
        assert_eq!(relativize(Path::new("/r/repo"), "/r/repository/x"), "/r/repository/x", "component boundary");
    }

    /// A linked worktree (`.git` a `gitdir:` file) has its repository's key: the shared config is
    /// in the common directory.
    #[test]
    fn a_linked_worktree_has_its_repositorys_origin_key() {
        let d = tempfile::tempdir().unwrap();
        let main = d.path().join("repo");
        std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        std::fs::write(main.join(".git/config"), "[remote \"origin\"]\n\turl = https://github.com/Acme/Billing\n")
            .unwrap();
        std::fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
        let wt = d.path().join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", main.join(".git/worktrees/wt").display())).unwrap();
        assert_eq!(origin_key(&wt).as_deref(), Some("github.com/acme/billing"));
        assert_eq!(origin_key(&wt), origin_key(&main));
    }

    #[test]
    fn origin_normalisation() {
        assert_eq!(normalise_origin("git@github.com:Acme/Billing.git"), "github.com/acme/billing");
        assert_eq!(normalise_origin("https://github.com/acme/billing"), "github.com/acme/billing");
    }
}
