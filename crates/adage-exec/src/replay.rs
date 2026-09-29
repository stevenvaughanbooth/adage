//! Replay: run one command of a skill in a disposable checkout of the repository, in the sandbox,
//! and report what it did (exit, output). `adage train` uses it to verify a compiled skill: a
//! directed command must work on a clean checkout, and a command the skill forbids must fail
//! with the signature the skill gives (docs/training.md).
//!
//! A replay is not an approval and delivers nothing to an agent. It runs only when a person asks
//! (`adage train <skill>` at a terminal), and every run goes through the same sandbox as E1 and E2:
//!
//! - **A disposable checkout**: `HEAD` of the repository, cloned shallow (`--depth 1`) into a
//!   fresh scratch directory with git hardened ([`crate::git::HARDENING`], no hooks). The person's
//!   working tree, index and uncommitted changes are never read or touched.
//! - **The sandbox**: the scratch directory (the checkout inside it) is the only writable place;
//!   the toolchain is read-only; the network is off; the environment is built from nothing
//!   ([`crate::env::build`]); the run is bounded ([`ReplayLimits`]). Root runs as nobody.
//! - **Environment directories** a clean checkout lacks (`.venv`, `node_modules`) may be lent
//!   read-only from the repository ([`Replay::lend`]), so a skill that runs `.venv/bin/python`
//!   can be replayed. What was lent is reported.
//!
//! Without a sandbox nothing runs: [`Replay::new`] refuses, and the caller reports every command
//! as not replayed.

use crate::limits::{Limits, GIB, MIB};
use crate::run::{Refusal, Scratch};
use crate::sandbox::{self, Profile, Read, Resolved, Write};
use crate::{env, git};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Bounds on one replayed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayLimits {
    pub wall_ms: u64,
    /// Bytes of stdout, and separately of stderr, kept.
    pub capture_bytes: u64,
}

impl Default for ReplayLimits {
    fn default() -> Self {
        ReplayLimits { wall_ms: 300_000, capture_bytes: 4 * MIB }
    }
}

/// What a replayed command did.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Replayed {
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    /// Stdout, then stderr, as text (lossy), each at most the capture limit.
    #[serde(skip)]
    pub output: String,
    pub duration_ms: u64,
}

impl Replayed {
    /// Exited 0, in time.
    pub fn ok(&self) -> bool {
        self.exit == Some(0) && !self.timed_out
    }

    /// Ran to an exit status other than 0 (not killed, not timed out).
    pub fn failed(&self) -> bool {
        matches!(self.exit, Some(c) if c != 0) && !self.timed_out
    }

    /// One line: `exit 0 in 1.2 s`, `exit 2`, `timed out`, `killed by signal 9`.
    pub fn summary(&self) -> String {
        let secs = self.duration_ms as f64 / 1000.0;
        match (self.exit, self.signal, self.timed_out) {
            (_, _, true) => format!("timed out after {secs:.0} s"),
            (Some(c), _, _) => format!("exit {c} in {secs:.1} s"),
            (None, Some(s), _) => format!("killed by signal {s}"),
            _ => "exit status unknown".into(),
        }
    }
}

/// A disposable, sandboxed checkout of one repository's `HEAD`.
pub struct Replay {
    scratch: Scratch,
    work: PathBuf,
    repo: PathBuf,
    capability: sandbox::Capability,
    lent: Vec<PathBuf>,
    limits: ReplayLimits,
    inherited_path: String,
}

fn refusal(stage: &'static str, reason: impl Into<String>) -> Refusal {
    Refusal { stage, reason: reason.into() }
}

/// The environment git itself runs with (checkout and status): nothing inherited but `PATH`.
fn git_env(scratch: &Path, path: &str) -> BTreeMap<String, String> {
    let s = scratch.to_string_lossy().into_owned();
    BTreeMap::from([
        ("PATH".to_string(), path.to_string()),
        ("HOME".to_string(), s),
        ("LANG".to_string(), "C.UTF-8".to_string()),
        ("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string()),
        ("GIT_CONFIG_GLOBAL".to_string(), "/dev/null".to_string()),
        ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
    ])
}

fn run_git(args: &[String], cwd: &Path, env: &BTreeMap<String, String>) -> Result<String, Refusal> {
    let git = sandbox::system_binary("git").ok_or_else(|| refusal("checkout", "git is not installed"))?;
    let mut argv = vec!["git".to_string()];
    argv.extend(args.iter().cloned());
    let argv = git::harden(&argv, None);
    let out = std::process::Command::new(git)
        .args(&argv[1..])
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| refusal("checkout", format!("running git: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(refusal("checkout", format!("git {}: {}", args.first().map(String::as_str).unwrap_or(""), err.trim())));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Give every entry under `dir` to `owner` (the run's user, when root drops to nobody).
#[cfg(unix)]
fn chown_tree(dir: &Path, owner: (u32, u32)) -> std::io::Result<()> {
    std::os::unix::fs::lchown(dir, Some(owner.0), Some(owner.1))?;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            std::os::unix::fs::lchown(e.path(), Some(owner.0), Some(owner.1))?;
            if e.file_type()?.is_dir() {
                stack.push(e.path());
            }
        }
    }
    Ok(())
}

impl Replay {
    /// A fresh checkout of `repo`'s `HEAD`, ready to replay commands in. Refuses when this machine
    /// has no sandbox, when `repo` has no commit, or when the checkout fails.
    pub fn new(repo: &Path, limits: ReplayLimits) -> Result<Replay, Refusal> {
        let capability = sandbox::detect();
        if !capability.available() {
            let why = match &capability.backend {
                sandbox::Backend::Unavailable { reason } => reason.clone(),
                _ => String::new(),
            };
            return Err(refusal("sandbox", format!("no sandbox on this machine: {why}")));
        }
        let repo = repo.canonicalize().map_err(|e| refusal("checkout", format!("repository: {e}")))?;
        let owner = sandbox::is_root().then_some(sandbox::NOBODY);
        let scratch = Scratch::new(owner).map_err(|e| refusal("sandbox", format!("scratch directory: {e}")))?;
        let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
        let work = scratch.path.join("work").join(&name);
        let inherited_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
        let genv = git_env(&scratch.path, &inherited_path);
        let url = format!("file://{}", repo.display());
        let args: Vec<String> = [
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--quiet",
            "--no-local",
            "--depth",
            "1",
            "--no-tags",
            "--single-branch",
            "--",
            &url,
            &work.to_string_lossy(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        run_git(&args, &scratch.path, &genv)?;
        // Nothing can be pushed from the checkout.
        let _ = run_git(&["remote".into(), "remove".into(), "origin".into()], &work, &genv);
        #[cfg(unix)]
        if let Some(o) = owner {
            chown_tree(&scratch.path.join("work"), o)
                .map_err(|e| refusal("checkout", format!("handing the checkout to the sandbox user: {e}")))?;
        }
        Ok(Replay { scratch, work, repo, capability, lent: Vec::new(), limits, inherited_path })
    }

    /// The checkout (inside scratch).
    pub fn dir(&self) -> &Path {
        &self.work
    }

    /// The sandbox this machine enforces, in one line.
    pub fn sandbox_line(&self) -> String {
        self.capability.line()
    }

    /// Lend the repository's `rel` directory (an environment a clean checkout lacks: `.venv`,
    /// `node_modules`) to the checkout, read-only. Returns whether it was lent: the directory
    /// exists in the repository (not a symlink), is missing from the checkout, and the sandbox's
    /// user can reach it.
    pub fn lend(&mut self, rel: &str) -> bool {
        if !adage_core::repo::is_safe_relative(rel) || rel.contains('/') {
            return false;
        }
        let src = self.repo.join(rel);
        let dest = self.work.join(rel);
        let is_dir = std::fs::symlink_metadata(&src).is_ok_and(|m| m.is_dir());
        if !is_dir || std::fs::symlink_metadata(&dest).is_ok() {
            return false;
        }
        if sandbox::is_root() && !sandbox::others_can_reach(&src) {
            return false;
        }
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(&src, &dest).is_err() {
                return false;
            }
            if let Some((u, g)) = sandbox::is_root().then_some(sandbox::NOBODY) {
                let _ = std::os::unix::fs::lchown(&dest, Some(u), Some(g));
            }
            self.lent.push(src);
            true
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// Write a file into the checkout (a fixture a step expects, such as the agent's own script).
    pub fn write_file(&self, rel: &str, bytes: &[u8]) -> std::io::Result<()> {
        if !adage_core::repo::is_safe_relative(rel) {
            return Err(std::io::Error::other("not a repository-relative path"));
        }
        let p = self.work.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, bytes)?;
        #[cfg(unix)]
        if let Some((u, g)) = sandbox::is_root().then_some(sandbox::NOBODY) {
            let _ = std::os::unix::fs::lchown(&p, Some(u), Some(g));
        }
        Ok(())
    }

    fn resolved(&self) -> Result<Resolved, Refusal> {
        let limits = Limits {
            wall_ms: self.limits.wall_ms,
            capture_bytes: self.limits.capture_bytes,
            return_bytes: 0,
            address_space: 8 * GIB,
            nproc: 512,
            file_size: GIB,
            open_files: 4096,
        };
        // The checkout lives in scratch, the one writable place; the repository itself is never
        // granted (only what `lend` lent, read-only).
        let profile =
            Profile { read: vec![Read::Toolchain], write: Write::Scratch, net: false, env: env::names(), cwd: None, limits };
        let environment = self.environment();
        let path_dirs: Vec<PathBuf> = environment["PATH"].split(':').map(PathBuf::from).collect();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut res = Resolved::new(&profile, &self.work, &self.scratch.path, &path_dirs, None, home.as_deref())
            .map_err(|e| refusal("sandbox", format!("resolving the profile: {e}")))?;
        for l in &self.lent {
            if let Ok(c) = l.canonicalize() {
                if !res.read.contains(&c) {
                    res.read.push(c);
                }
            }
        }
        Ok(res)
    }

    fn environment(&self) -> BTreeMap<String, String> {
        let inherited = BTreeMap::from([("PATH".to_string(), self.inherited_path.clone())]);
        env::build(&inherited, &self.work, &self.scratch.path, &[])
    }

    /// Run `argv` in the checkout, in the sandbox. `argv[0]` is a program on the sanitised `PATH`,
    /// or a path inside the checkout (`scripts/wi-lookup`, `.venv/bin/python`).
    pub fn run(&self, argv: &[String]) -> Result<Replayed, Refusal> {
        let Some(first) = argv.first() else { return Err(refusal("request", "an empty command")) };
        let environment = self.environment();
        let program = if first.contains('/') {
            let rel = first.trim_start_matches("./");
            if !adage_core::repo::is_safe_relative(rel) {
                return Err(refusal("request", format!("{first} is not a path inside the repository")));
            }
            let p = self.work.join(rel);
            if !p.exists() {
                return Err(refusal("request", format!("{rel} does not exist in a clean checkout")));
            }
            p
        } else {
            env::which(first, &environment["PATH"])
                .ok_or_else(|| refusal("request", format!("{first} is not on the sanitised PATH")))?
        };
        let is_git = program.file_name().is_some_and(|n| n == "git");
        let full = if is_git { git::harden(argv, Some(&self.work)) } else { argv.to_vec() };
        let res = self.resolved()?;
        let o = crate::run::spawn_sandboxed_with(
            &program,
            &full[1..],
            &environment,
            &res,
            &self.capability,
            self.limits.capture_bytes,
        )?;
        let mut output = String::from_utf8_lossy(&o.stdout).into_owned();
        if !o.stderr.is_empty() {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&String::from_utf8_lossy(&o.stderr));
        }
        Ok(Replayed { exit: o.exit, signal: o.signal, timed_out: o.timed_out, output, duration_ms: o.duration_ms })
    }

    /// Paths the replays changed in the checkout, as `git status --porcelain` lists them (ignored
    /// files aside): what a regenerate step wrote on a clean tree.
    pub fn changed(&self) -> Result<Vec<String>, Refusal> {
        let genv = git_env(&self.scratch.path, &self.inherited_path);
        let args: Vec<String> = ["-c".to_string(), format!("safe.directory={}", self.work.display())]
            .into_iter()
            .chain(["status", "--porcelain", "--untracked-files=all"].iter().map(|s| s.to_string()))
            .collect();
        let out = run_git(&args, &self.work, &genv)?;
        Ok(out.lines().filter_map(|l| l.get(3..)).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_are_one_line() {
        let r = Replayed { exit: Some(0), duration_ms: 1200, ..Default::default() };
        assert!(r.ok() && !r.failed());
        assert_eq!(r.summary(), "exit 0 in 1.2 s");
        let r = Replayed { exit: Some(2), ..Default::default() };
        assert!(r.failed());
        let r = Replayed { timed_out: true, duration_ms: 300_000, ..Default::default() };
        assert!(!r.ok() && !r.failed());
        assert_eq!(r.summary(), "timed out after 300 s");
    }
}
