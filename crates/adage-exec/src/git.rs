//! Git hardening (Track C §3.3). Git's behaviour is configurable by the repository, and a hostile
//! `.git/config` can run code (`core.fsmonitor`, `core.sshCommand`, `filter.*`, textconv, aliases).
//!
//! Two controls: every git run gets `-c` overrides before its subcommand ([`HARDENING`]), and a
//! repository whose config sets any hazardous key ([`hazards`]) loses git's inert status, so its
//! git steps are never approved automatically. The config is read without running git, bounded,
//! the way `adage_core::gitobj` reads a repository.

use std::path::{Path, PathBuf};

/// `-c` pairs placed right after `git`, before the subcommand.
pub const HARDENING: &[&str] = &[
    "core.fsmonitor=",
    "core.hooksPath=/dev/null",
    "diff.external=",
    "core.pager=cat",
    "core.sshCommand=false",
    "core.askPass=false",
    "credential.helper=",
    "protocol.allow=never",
    "log.showSignature=false",
    "gpg.program=false",
];

/// The largest config file read. A larger one is itself a hazard.
pub const MAX_CONFIG: u64 = 256 * 1024;

/// The argv for a git run: `git -c k=v … [-c safe.directory=<root>] <rest>`. `safe.directory` is
/// added when the run changes user (root runs as nobody), since git otherwise refuses a
/// repository another user owns; it is honoured only from the command line or global config, so
/// the repository cannot set it.
pub fn harden(argv: &[String], safe_directory: Option<&Path>) -> Vec<String> {
    let mut out = vec![argv[0].clone()];
    for kv in HARDENING {
        out.push("-c".into());
        out.push((*kv).into());
    }
    if let Some(d) = safe_directory {
        out.push("-c".into());
        out.push(format!("safe.directory={}", d.display()));
    }
    out.extend(argv[1..].iter().cloned());
    out
}

/// The git directories whose config applies to a work tree: `.git` (a directory, or a `gitdir:`
/// file for a linked worktree) and, for a worktree, the common directory.
fn git_dirs(root: &Path) -> Vec<PathBuf> {
    let dotgit = root.join(".git");
    let git = if dotgit.is_dir() {
        dotgit
    } else {
        let Ok(text) = read_bounded(&dotgit, 4096) else { return vec![] };
        let Some(p) = text.lines().next().and_then(|l| l.strip_prefix("gitdir:")) else { return vec![] };
        let p = PathBuf::from(p.trim());
        if p.is_absolute() {
            p
        } else {
            root.join(p)
        }
    };
    let mut dirs = vec![git.clone()];
    if let Ok(c) = read_bounded(&git.join("commondir"), 4096) {
        let p = PathBuf::from(c.trim());
        dirs.push(if p.is_absolute() { p } else { git.join(p) });
    }
    dirs
}

fn read_bounded(p: &Path, max: u64) -> std::io::Result<String> {
    use std::io::Read;
    let f = std::fs::File::open(p)?;
    let mut s = String::new();
    f.take(max + 1).read_to_string(&mut s)?;
    if s.len() as u64 > max {
        return Err(std::io::Error::other("too large"));
    }
    Ok(s)
}

/// Is `key` (lower-cased `section[.subsection].name`) one that can make git run code, or pull in
/// config from elsewhere?
pub fn hazardous_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    let (section, rest) = k.split_once('.').unwrap_or((&k, ""));
    let name = rest.rsplit('.').next().unwrap_or("");
    match section {
        "alias" | "filter" | "include" | "includeif" => true,
        "core" => matches!(
            name,
            "fsmonitor" | "sshcommand" | "hookspath" | "gitproxy" | "pager" | "editor" | "askpass" | "worktree"
        ),
        "diff" => name == "textconv" || name == "command" || rest == "external",
        "merge" => name == "driver",
        "gpg" => name == "program",
        "log" => name == "showsignature",
        "uploadpack" => name == "packobjectshook",
        "credential" => name == "helper",
        "sequence" => name == "editor",
        "pager" => true,
        _ => false,
    }
}

/// The hazardous keys a repository's git config sets (empty: none). An unreadable, oversized or
/// unparseable config counts as hazardous.
pub fn hazards(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for dir in git_dirs(root) {
        for name in ["config", "config.worktree"] {
            let p = dir.join(name);
            if !p.exists() {
                continue;
            }
            match read_bounded(&p, MAX_CONFIG) {
                Ok(text) => out.extend(config_keys(&text).into_iter().filter(|k| hazardous_key(k))),
                Err(_) => out.push(format!("{} (unreadable or larger than {MAX_CONFIG} bytes)", p.display())),
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The keys a git config file sets, as `section[.subsection].name`, lower-cased except the
/// subsection. Tolerant: a line it cannot parse is reported as the key `unparsed`, which
/// [`hazardous_key`] does not match, so it is surfaced separately.
pub fn config_keys(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut section = String::new();
    for raw in text.lines() {
        let mut line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            let Some(end) = line.find(']') else {
                out.push("include.unparsed-section".into());
                continue;
            };
            let head = &line[1..end];
            section = match head.find('"') {
                Some(q) => {
                    let base = head[..q].trim().to_ascii_lowercase();
                    let sub = head[q + 1..].trim_end_matches('"');
                    format!("{base}.{sub}")
                }
                None => head.trim().to_ascii_lowercase(),
            };
            line = line[end + 1..].trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
        }
        let name = line.split(['=', ' ', '\t']).next().unwrap_or("").trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        out.push(if section.is_empty() { name } else { format!("{section}.{name}") });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_hazardous_keys() {
        let cfg = "[core]\n\trepositoryformatversion = 0\n\tfsmonitor = ./evil.sh\n[remote \"origin\"]\n\turl = x\n\
                   [diff \"bin\"]\n\ttextconv = ./conv\n[filter \"lfs\"]\n\tclean = x\n[alias]\n\tst = !sh\n\
                   [core]sshCommand = evil\n[include]\n\tpath = ../other\n";
        let keys: Vec<String> = config_keys(cfg).into_iter().filter(|k| hazardous_key(k)).collect();
        assert_eq!(
            keys,
            vec![
                "core.fsmonitor",
                "diff.bin.textconv",
                "filter.lfs.clean",
                "alias.st",
                "core.sshcommand",
                "include.path"
            ]
        );
        assert!(config_keys("[core]\n\tbare = false\n[remote \"origin\"]\n\turl = x\n")
            .iter()
            .all(|k| !hazardous_key(k)));
    }

    #[test]
    fn hazards_reads_the_repository_config() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".git")).unwrap();
        std::fs::write(d.path().join(".git/config"), "[core]\n\tbare = false\n").unwrap();
        assert!(hazards(d.path()).is_empty());
        std::fs::write(d.path().join(".git/config"), "[core]\n\tfsmonitor = true\n").unwrap();
        assert_eq!(hazards(d.path()), vec!["core.fsmonitor"]);
        std::fs::write(d.path().join(".git/config"), "x".repeat(MAX_CONFIG as usize + 10)).unwrap();
        assert_eq!(hazards(d.path()).len(), 1, "oversized config is a hazard");
    }

    #[test]
    fn hardening_goes_before_the_subcommand() {
        let argv: Vec<String> = ["git", "log", "-1"].map(String::from).to_vec();
        let h = harden(&argv, None);
        assert_eq!(h[0], "git");
        assert_eq!(&h[h.len() - 2..], &["log", "-1"]);
        assert!(h.windows(2).any(|w| w == ["-c", "core.fsmonitor="]));
    }
}
