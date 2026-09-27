//! Isolation for training sessions: who may start them, the environment they see, the home
//! directory they get, and the disposable clone they work in. Mirrors `evals/live/runner.py`.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Variables that mark an agent session (every harness Adage supports), and Adage's own markers
/// for sessions it started (a proving-ground arm, a training session). None of them may be set
/// when `run --execute` starts agents.
pub const AGENT_ENV: &[&str] =
    &["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED", "ADAGE_ARM", "ADAGE_TRAIN_SESSION"];

/// Refuse unless a person is at a terminal and not inside an agent session: `adage`'s
/// `require_person`, plus Adage's own session markers. `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only
/// the terminal requirement (for CI); nothing lifts the agent-session refusal.
pub fn require_person(env: &BTreeMap<String, String>, stdin_is_tty: bool) -> Result<()> {
    if let Some(v) = AGENT_ENV.iter().find(|v| env.get(**v).is_some_and(|x| !x.is_empty())) {
        bail!(
            "`adage-train run --execute` starts agents that spend money, so it will not run inside an agent \
             session ({v} is set). Run it yourself, in a terminal."
        );
    }
    if env.get("ADAGE_ALLOW_NONINTERACTIVE").map(String::as_str) == Some("1") {
        return Ok(());
    }
    if !stdin_is_tty {
        bail!(
            "`adage-train run --execute` needs a person at a terminal (stdin is not a terminal). For CI, set \
             ADAGE_ALLOW_NONINTERACTIVE=1."
        );
    }
    Ok(())
}

pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

pub fn process_env() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

/// The operator's variables an isolated session may see: the path, locale, terminal, time zone,
/// proxies and CA bundles. Everything else (HOME, CLAUDE_CONFIG_DIR, CLAUDECODE, ADAGE_*, tokens
/// of other tools) is dropped; the harness adds only its own auth variables.
pub const PASS_ENV: &[&str] = &[
    "PATH",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "SHELL",
    "USER",
    "LOGNAME",
    "TZ",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
];

/// Where an isolated session's HOME, harness config directory, XDG directories and TMPDIR point.
pub fn home_dirs(home: &Path) -> BTreeMap<String, PathBuf> {
    [
        ("HOME", home.to_path_buf()),
        ("CLAUDE_CONFIG_DIR", home.join(".claude")),
        ("XDG_CONFIG_HOME", home.join(".config")),
        ("XDG_DATA_HOME", home.join(".local/share")),
        ("XDG_STATE_HOME", home.join(".local/state")),
        ("XDG_CACHE_HOME", home.join(".cache")),
        ("CODEX_HOME", home.join(".codex")),
        ("TMPDIR", home.join("tmp")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// The whole environment of an isolated session: [`PASS_ENV`] and `auth` from `base`, the
/// session's own home directories, and a pinned harness version.
pub fn isolation_env(home: &Path, base: &BTreeMap<String, String>, auth: &[&str]) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> =
        PASS_ENV.iter().chain(auth).filter_map(|k| base.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
    for (k, v) in home_dirs(home) {
        env.insert(k, v.to_string_lossy().to_string());
    }
    env.insert("DISABLE_AUTOUPDATER".into(), "1".into());
    env
}

/// Create an isolated home: its directories and a git identity (agents may commit in the clone).
pub fn prepare_home(home: &Path) -> Result<()> {
    for d in home_dirs(home).values() {
        std::fs::create_dir_all(d).with_context(|| format!("creating {}", d.display()))?;
    }
    std::fs::write(home.join(".gitconfig"), "[user]\n\tname = adage-train\n\temail = adage-train@localhost\n")?;
    Ok(())
}

fn git(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let mut c = Command::new("git");
    c.args(args).env("GIT_TERMINAL_PROMPT", "0").env_remove("GIT_DIR").env_remove("GIT_WORK_TREE");
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let out = c.output().context("running git")?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A disposable copy of the repository's HEAD: `git clone --local --no-hardlinks`, then the
/// `origin` remote removed so nothing can be pushed from it. `dest` should end in the repository's
/// own directory name, so the clone's repository key (without an origin: name and root commit)
/// is the same for every clone.
pub fn clone_worktree(repo: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let src = repo.to_string_lossy().to_string();
    let dst = dest.to_string_lossy().to_string();
    git(&["clone", "--quiet", "--local", "--no-hardlinks", &src, &dst], None)?;
    git(&["remote", "remove", "origin"], Some(dest))?;
    Ok(())
}

/// Remove a directory tree, best effort (a clone or a session home, after the session).
pub fn remove(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn only_a_person_at_a_terminal_may_execute() {
        assert!(require_person(&env(&[]), true).is_ok());
        assert!(require_person(&env(&[]), false).is_err());
        assert!(require_person(&env(&[("ADAGE_ALLOW_NONINTERACTIVE", "1")]), false).is_ok());
        for v in AGENT_ENV {
            let e = env(&[(v, "1"), ("ADAGE_ALLOW_NONINTERACTIVE", "1")]);
            let err = require_person(&e, true).unwrap_err().to_string();
            assert!(err.contains(v), "{err}");
        }
        // An empty marker is not a marker.
        assert!(require_person(&env(&[("CLAUDECODE", "")]), true).is_ok());
    }

    #[test]
    fn the_isolated_environment_is_an_allowlist() {
        let base = env(&[
            ("PATH", "/bin"),
            ("HOME", "/home/me"),
            ("CLAUDE_CONFIG_DIR", "/home/me/.claude"),
            ("CLAUDECODE", "1"),
            ("ADAGE_HOME", "/home/me/.adage"),
            ("GITHUB_TOKEN", "secret"),
            ("ANTHROPIC_API_KEY", "k"),
            ("HTTPS_PROXY", "http://p"),
        ]);
        let e = isolation_env(Path::new("/run/h"), &base, &["ANTHROPIC_API_KEY"]);
        assert_eq!(e.get("HOME").map(String::as_str), Some("/run/h"));
        assert_eq!(e.get("CLAUDE_CONFIG_DIR").map(String::as_str), Some("/run/h/.claude"));
        assert_eq!(e.get("PATH").map(String::as_str), Some("/bin"));
        assert_eq!(e.get("ANTHROPIC_API_KEY").map(String::as_str), Some("k"));
        assert_eq!(e.get("HTTPS_PROXY").map(String::as_str), Some("http://p"));
        for gone in ["CLAUDECODE", "ADAGE_HOME", "GITHUB_TOKEN"] {
            assert!(!e.contains_key(gone), "{gone}");
        }
    }
}
