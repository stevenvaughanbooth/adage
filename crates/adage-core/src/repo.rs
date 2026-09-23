//! Repository identity and repo-relative paths.

use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

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

/// A stable repository key: the normalised `origin` URL if there is one, else the directory name.
/// Clones of the same repository in different directories share a key.
pub fn repo_key(root: &Path) -> String {
    let origin = std::fs::read_to_string(root.join(".git/config")).ok().and_then(|cfg| {
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
    });
    match origin {
        Some(o) => o,
        None => root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
    }
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
}
