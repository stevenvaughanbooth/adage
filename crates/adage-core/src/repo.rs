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
///   different directories share a key, and so do its linked worktrees, which read `origin` from
///   the git directory they share with the main checkout ([`origin_key`]).
/// - Without one: `<directory name>@<root commit>`, the first 12 hex digits of the root of
///   `HEAD`'s first-parent history, read from the object store without running git
///   ([`crate::gitobj`]). Unrelated repositories that happen to share a directory name get
///   different keys; copies of one repository share a key.
/// - If the root commit cannot be read (no commits yet, a SHA-256 or shallow repository,
///   alternates, a history longer than [`gitobj::MAX_WALK`]), or the directory is not a git
///   repository: `<directory name>@path-<12 hex digits of SHA-256 of the root's path>`. Such a
///   key is not shared with any copy.
/// - A directory that no longer exists (history of a deleted checkout, such as a removed worktree,
///   read by `adage learn`): the key remembered for it while it existed ([`remember_key`]), else
///   the directory name alone, the key every repository without `origin` had before. Hooks accept
///   such a *legacy* key only while no repository of that name has a new key ([`lookup_key`]).
pub fn repo_key(root: &Path) -> String {
    key(root, CACHE.get().map(PathBuf::as_path))
}

/// Where [`repo_key`] remembers root commits, and [`remember_key`] keys, for the rest of this
/// process. The CLI sets `$ADAGE_HOME/repo-keys`, so the history `adage learn` walks is not walked
/// again by a hook.
static CACHE: OnceLock<PathBuf> = OnceLock::new();

/// Remember root commits under `dir` for the rest of this process (first call wins).
pub fn use_cache(dir: PathBuf) {
    let _ = CACHE.set(dir);
}

/// [`repo_key`], remembering a root commit once found in `cache` (a directory), so that a long
/// history is walked once, not at every hook. A remembered root commit is used only while the
/// repository's object store still holds it.
pub fn repo_key_cached(root: &Path, cache: &Path) -> String {
    key(root, Some(cache))
}

fn key(root: &Path, cache: Option<&Path>) -> String {
    if !root.is_dir() {
        return cache.and_then(|c| recall(c, &root.to_string_lossy())).unwrap_or_else(|| dir_name(root));
    }
    origin_key(root).unwrap_or_else(|| no_origin_key(root, cache))
}

/// Remember `key` as the key of the checkout at `root`, in the cache directory of [`use_cache`], so
/// that its sessions keep their repository once the directory is gone (a removed worktree). Called
/// where Adage learns (`adage learn`, and the hook at the end of a turn), never by a key lookup.
pub fn remember_key(root: &Path, key: &str) {
    if let Some(cache) = CACHE.get() {
        remember_key_cached(root, key, cache);
    }
}

/// [`remember_key`] in `cache`. Best effort: a key that cannot be written is only not remembered.
pub fn remember_key_cached(root: &Path, key: &str, cache: &Path) {
    let path = root.to_string_lossy();
    if !root.is_dir() || recall(cache, &path).as_deref() == Some(key) {
        return;
    }
    let _ = std::fs::create_dir_all(cache);
    let tmp = cache.join(format!("{}.key.tmp{}", short_hash(&path), std::process::id()));
    if std::fs::write(&tmp, format!("{path}\n{key}\n")).is_ok() {
        let _ = std::fs::rename(&tmp, key_file(cache, &path));
    }
}

/// One file per repository path, beside its root commit: the path, then the key.
fn key_file(cache: &Path, path: &str) -> PathBuf {
    cache.join(format!("{}.key", short_hash(path)))
}

fn recall(cache: &Path, path: &str) -> Option<String> {
    let text = std::fs::read_to_string(key_file(cache, path)).ok()?;
    let mut lines = text.lines();
    match (lines.next(), lines.next()) {
        (Some(p), Some(k)) if p == path && !k.is_empty() => Some(k.to_string()),
        _ => None,
    }
}

/// The key under which a hook looks up routes for `root`, given the repository keys the index
/// holds routes for; `None` when nothing applies. Without `origin`, the object store is read only
/// if the index holds routes for some repository of this directory name.
///
/// A legacy key (the bare directory name, from before keys carried the root commit) is accepted
/// only while the index holds no new-style key for that name. Once `adage learn` has re-keyed a
/// repository of that name, a legacy key could belong to any repository of the name, so it is
/// ignored: that is the collision the new keys exist to prevent.
pub fn lookup_key(root: &Path, cache: Option<&Path>, keys: &BTreeSet<&str>) -> Option<String> {
    if let Some(o) = origin_key(root) {
        return keys.contains(o.as_str()).then_some(o);
    }
    let name = dir_name(root);
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

/// The `origin` remote's normalised URL, from the repository's config. The config is in the common
/// git directory ([`gitobj::dirs`]): a linked worktree's `.git` is a file, and its remotes are the
/// main checkout's.
pub fn origin_key(root: &Path) -> Option<String> {
    let (_, common) = gitobj::dirs(root)?;
    let cfg = std::fs::read_to_string(common.join("config")).ok()?;
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

fn no_origin_key(root: &Path, cache: Option<&Path>) -> String {
    let name = dir_name(root);
    if !root.is_dir() {
        return name;
    }
    match root_commit(root, cache) {
        Some(c) => format!("{name}@{}", &c[..12]),
        None => format!("{name}@path-{}", short_hash(&root.to_string_lossy())),
    }
}

/// Root commits found in this process, by repository path: `adage learn` asks once per session.
static FOUND: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// The root commit (40 hex digits): through the cache directory when one is given (hooks, one
/// repository per process), else remembered in this process (`adage learn`, many sessions per
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

    #[test]
    fn origin_normalisation() {
        assert_eq!(normalise_origin("git@github.com:Acme/Billing.git"), "github.com/acme/billing");
        assert_eq!(normalise_origin("https://github.com/acme/billing"), "github.com/acme/billing");
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn worktrees_and_submodules_read_origin_from_their_common_directory() {
        let d = tempfile::tempdir().unwrap();
        let main = d.path().join("app");
        write(
            &main.join(".git/config"),
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com:Acme/App.git\n",
        );
        // What `git worktree add` writes: a `gitdir:` file (absolute, or relative with
        // `worktree.useRelativePaths`), and a git directory naming the common one in `commondir`.
        write(&main.join(".git/worktrees/wt/commondir"), "../..\n");
        let (abs, rel) = (d.path().join("istanbul"), d.path().join("paris"));
        write(&abs.join(".git"), &format!("gitdir: {}\n", main.join(".git/worktrees/wt").display()));
        write(&rel.join(".git"), "gitdir: ../app/.git/worktrees/wt\n");
        // A submodule: its `gitdir:` names its own git directory, which has no `commondir`.
        let sub = main.join("vendor/lib");
        write(&main.join(".git/modules/lib/config"), "[remote \"origin\"]\n\turl = https://github.com/acme/lib\n");
        write(&sub.join(".git"), "gitdir: ../../.git/modules/lib\n");

        let cache = d.path().join("repo-keys");
        assert_eq!(repo_key_cached(&main, &cache), "github.com/acme/app");
        assert_eq!(repo_key_cached(&abs, &cache), "github.com/acme/app", "absolute gitdir");
        assert_eq!(repo_key_cached(&rel, &cache), "github.com/acme/app", "relative gitdir");
        assert_eq!(repo_key_cached(&sub, &cache), "github.com/acme/lib", "a submodule is its own repository");
        let keys = BTreeSet::from(["github.com/acme/app"]);
        assert_eq!(lookup_key(&rel, Some(&cache), &keys).as_deref(), Some("github.com/acme/app"), "hooks");
    }

    #[test]
    fn a_removed_checkout_keeps_the_key_it_had() {
        let d = tempfile::tempdir().unwrap();
        let cache = d.path().join("repo-keys");
        let wt = d.path().join("istanbul");
        let origin = |url: &str| write(&wt.join(".git/config"), &format!("[remote \"origin\"]\n\turl = {url}\n"));
        let learn = || {
            let key = repo_key_cached(&wt, &cache);
            remember_key_cached(&wt, &key, &cache);
            key
        };
        origin("https://github.com/acme/app.git");
        assert_eq!(repo_key_cached(&wt, &cache), "github.com/acme/app");
        assert!(!cache.exists(), "a lookup writes no key");
        assert_eq!(learn(), "github.com/acme/app");
        std::fs::remove_dir_all(&wt).unwrap();
        assert_eq!(repo_key_cached(&wt, &cache), "github.com/acme/app", "remembered");
        remember_key_cached(&wt, "istanbul", &cache);
        assert_eq!(repo_key_cached(&wt, &cache), "github.com/acme/app", "nothing is remembered for a deleted path");
        assert_eq!(repo_key_cached(&d.path().join("paris"), &cache), "paris", "never seen: the bare name, as before");

        // The path is reused by another repository: its key replaces the remembered one.
        origin("https://github.com/acme/other");
        assert_eq!(learn(), "github.com/acme/other");
        std::fs::remove_dir_all(&wt).unwrap();
        assert_eq!(repo_key_cached(&wt, &cache), "github.com/acme/other");

        // A file that is not this path's (a hash collision, or damage) is not believed.
        let file = key_file(&cache, &wt.to_string_lossy());
        std::fs::write(&file, "/somewhere/else\ngithub.com/acme/app\n").unwrap();
        assert_eq!(repo_key_cached(&wt, &cache), "istanbul");
        std::fs::write(&file, format!("{}\n", wt.display())).unwrap();
        assert_eq!(repo_key_cached(&wt, &cache), "istanbul");
    }
}
