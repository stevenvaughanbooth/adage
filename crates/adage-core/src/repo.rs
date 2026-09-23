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
pub fn relativize(root: &Path, path: &str) -> String {
    let p = Path::new(path);
    let norm = lexical_normalise(p);
    match norm.strip_prefix(lexical_normalise(root)) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => norm.to_string_lossy().to_string(),
    }
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

/// True if `rel` is a safe repository-relative path: no absolute path, no `..`, no leading `-`.
pub fn is_safe_relative(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.starts_with('-')
        && !rel.starts_with('~')
        && !Path::new(rel).components().any(|c| matches!(c, Component::ParentDir))
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
    }

    #[test]
    fn origin_normalisation() {
        assert_eq!(normalise_origin("git@github.com:Acme/Billing.git"), "github.com/acme/billing");
        assert_eq!(normalise_origin("https://github.com/acme/billing"), "github.com/acme/billing");
    }
}
