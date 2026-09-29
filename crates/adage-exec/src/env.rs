//! The environment of a sandboxed run, built from nothing (Track C §3.3). Nothing is inherited
//! except a sanitised `PATH`; everything else is fixed here. `SSH_AUTH_SOCK`, tokens and cloud
//! credentials never pass, whatever policy asks for.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The fixed variables, in addition to `PATH`, `HOME`, `TMPDIR` and `XDG_*` (which point into
/// scratch).
pub const FIXED: &[(&str, &str)] = &[
    ("LANG", "C.UTF-8"),
    ("TZ", "UTC"),
    ("TERM", "dumb"),
    ("NO_COLOR", "1"),
    ("PAGER", "cat"),
    ("GIT_PAGER", "cat"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_TERMINAL_PROMPT", "0"),
    // Read-only: never take the index lock to refresh stat information.
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_ASKPASS", "/bin/false"),
    ("SSH_ASKPASS", "/bin/false"),
];

/// The names every run gets, for the sandbox profile (and so its id).
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = ["PATH", "HOME", "TMPDIR", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"]
        .iter()
        .chain(FIXED.iter().map(|(k, _)| k))
        .map(|s| s.to_string())
        .collect();
    v.sort();
    v
}

/// Variables that may never be passed, even when local policy names them: agent sockets,
/// credentials, tokens, loader and interpreter hooks, and tool configuration that a repository or
/// agent could point at code (`RIPGREP_CONFIG_PATH`, `GIT_*`).
pub fn forbidden(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    const EXACT: &[&str] = &[
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "GPG_AGENT_INFO",
        "RIPGREP_CONFIG_PATH",
        "GREP_OPTIONS",
        "JQ_LIBRARY_PATH",
        "BASH_ENV",
        "ENV",
        "PROMPT_COMMAND",
        "PYTHONSTARTUP",
        "PYTHONPATH",
        "NODE_OPTIONS",
        "PERL5OPT",
        "RUBYOPT",
        "IFS",
        "CDPATH",
        "HOME",
        "PATH",
        "SHELLOPTS",
        "DOCKER_HOST",
        "KUBECONFIG",
        "NETRC",
    ];
    const PREFIX: &[&str] = &[
        "GIT_",
        "LD_",
        "DYLD_",
        "AWS_",
        "AZURE_",
        "GOOGLE_",
        "GCP_",
        "GCLOUD_",
        "CLOUDSDK_",
        "GH_",
        "GITHUB_",
        "GITLAB_",
        "ANTHROPIC_",
        "OPENAI_",
        "CLAUDE",
        "NPM_CONFIG_",
        "ADAGE_",
        "VAULT_",
        "SSH_",
        "DIGITALOCEAN_",
        "HEROKU_",
    ];
    const INFIX: &[&str] =
        &["TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "_KEY", "APIKEY", "AUTH", "COOKIE", "SESSION"];
    EXACT.contains(&n.as_str())
        || PREFIX.iter().any(|p| n.starts_with(p))
        || INFIX.iter().any(|p| n.contains(p))
        || n.ends_with("KEY")
        || n.is_empty()
        || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `PATH` with relative, empty and `.` entries removed, and entries inside the repository or the
/// scratch directory removed (a repository must not be able to shadow `git` or `cat` with its own
/// `node_modules/.bin`). Duplicates are dropped; order is kept.
pub fn sanitise_path(path: &str, repo: &Path, scratch: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in path.split(':') {
        let p = Path::new(entry);
        if entry.is_empty() || !p.is_absolute() || entry.split('/').any(|s| s == "." || s == "..") {
            continue;
        }
        let canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        if canon.starts_with(repo) || canon.starts_with(scratch) || p.starts_with(repo) || p.starts_with(scratch) {
            continue;
        }
        if !out.iter().any(|q| q == p) {
            out.push(p.to_path_buf());
        }
    }
    out
}

/// The complete environment for a run. `inherited` is the caller's environment, from which only
/// `PATH` is read, plus any names in `policy_env` that are not [`forbidden`].
pub fn build(
    inherited: &BTreeMap<String, String>,
    repo: &Path,
    scratch: &Path,
    policy_env: &[String],
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    let path = inherited.get("PATH").cloned().unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".into());
    let dirs = sanitise_path(&path, repo, scratch);
    let joined = dirs.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join(":");
    env.insert("PATH".into(), if joined.is_empty() { "/usr/bin:/bin".into() } else { joined });
    let s = scratch.to_string_lossy().into_owned();
    env.insert("HOME".into(), s.clone());
    env.insert("TMPDIR".into(), format!("{s}/tmp"));
    env.insert("XDG_CONFIG_HOME".into(), format!("{s}/.config"));
    env.insert("XDG_CACHE_HOME".into(), format!("{s}/.cache"));
    for (k, v) in FIXED {
        env.insert(k.to_string(), v.to_string());
    }
    for name in policy_env {
        if forbidden(name) || env.contains_key(name) {
            continue;
        }
        if let Some(v) = inherited.get(name) {
            if !v.contains('\0') && v.len() <= 4096 {
                env.insert(name.clone(), v.clone());
            }
        }
    }
    env
}

/// Resolve `program` against the sanitised `PATH` to an absolute path. A program containing `/`
/// is not looked up (scripts are resolved by the caller, against the repository).
pub fn which(program: &str, path: &str) -> Option<PathBuf> {
    if program.contains('/') || program.is_empty() {
        return None;
    }
    for dir in path.split(':') {
        let cand = Path::new(dir).join(program);
        if is_executable(&cand) {
            return Some(cand);
        }
    }
    None
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inherited() -> BTreeMap<String, String> {
        [
            ("PATH", "/usr/bin:.:bin::/repo/node_modules/.bin:/bin:/usr/bin"),
            ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
            ("GITHUB_TOKEN", "ghp_x"),
            ("AWS_SECRET_ACCESS_KEY", "x"),
            ("RIPGREP_CONFIG_PATH", "/repo/.ripgreprc"),
            ("GIT_DIR", "/elsewhere"),
            ("LD_PRELOAD", "/tmp/evil.so"),
            ("JAVA_HOME", "/usr/lib/jvm"),
            ("HOME", "/home/alice"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn built_from_nothing() {
        let env = build(&inherited(), Path::new("/repo"), Path::new("/tmp/scratch"), &[]);
        assert_eq!(env["PATH"], "/usr/bin:/bin");
        assert_eq!(env["HOME"], "/tmp/scratch");
        assert_eq!(env["TMPDIR"], "/tmp/scratch/tmp");
        assert_eq!(env["GIT_CONFIG_GLOBAL"], "/dev/null");
        for gone in [
            "SSH_AUTH_SOCK",
            "GITHUB_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "RIPGREP_CONFIG_PATH",
            "GIT_DIR",
            "LD_PRELOAD",
            "JAVA_HOME",
        ] {
            assert!(!env.contains_key(gone), "{gone} passed");
        }
        let mut names: Vec<String> = env.keys().cloned().collect();
        names.sort();
        assert_eq!(names, super::names());
    }

    #[test]
    fn policy_names_pass_unless_forbidden() {
        let want: Vec<String> =
            ["JAVA_HOME", "SSH_AUTH_SOCK", "GITHUB_TOKEN", "RIPGREP_CONFIG_PATH", "LD_PRELOAD", "HOME"]
                .map(String::from)
                .to_vec();
        let env = build(&inherited(), Path::new("/repo"), Path::new("/tmp/scratch"), &want);
        assert_eq!(env["JAVA_HOME"], "/usr/lib/jvm");
        assert_eq!(env["HOME"], "/tmp/scratch");
        for gone in ["SSH_AUTH_SOCK", "GITHUB_TOKEN", "RIPGREP_CONFIG_PATH", "LD_PRELOAD"] {
            assert!(!env.contains_key(gone), "{gone} passed through policy");
        }
        for f in ["MY_API_KEY", "STRIPE_KEY", "DB_PASSWORD", "OPENAI_API_KEY", "GCP_SA", "npm_token", "A-B"] {
            assert!(forbidden(f), "{f}");
        }
        assert!(!forbidden("JAVA_HOME") && !forbidden("CARGO_TARGET_DIR"));
    }
}
