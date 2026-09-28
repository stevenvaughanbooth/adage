//! Fixtures for the escape suite: temporary repositories and targets, and a runner per backend.

use adage_exec::request::Level;
use adage_exec::run::{spawn_sandboxed, Outcome, Refusal, Scratch};
use adage_exec::sandbox::{self, Backend, Capability, Profile, Resolved};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Fixture {
    pub repo: tempfile::TempDir,
    pub outside: tempfile::TempDir,
    pub fake_home: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Fixture {
        let f = Fixture {
            repo: tempfile::tempdir().unwrap(),
            outside: tempfile::tempdir().unwrap(),
            fake_home: tempfile::tempdir().unwrap(),
        };
        // Readable by anyone, so file permissions are not what stops a read: the sandbox is.
        for d in [f.repo.path(), f.outside.path(), f.fake_home.path()] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        f
    }
}

/// `chmod -R a+rX`.
pub fn world_readable(p: &Path) {
    for e in walk(p) {
        let m = std::fs::symlink_metadata(&e).unwrap();
        if m.file_type().is_symlink() {
            continue;
        }
        let mode = m.permissions().mode();
        let add = if m.is_dir() || mode & 0o100 != 0 { 0o555 } else { 0o444 };
        std::fs::set_permissions(&e, std::fs::Permissions::from_mode(mode | add)).unwrap();
    }
}

pub fn world_writable(p: &Path) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o777)).unwrap();
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut out = vec![p.to_path_buf()];
    if std::fs::symlink_metadata(p).map(|m| m.is_dir()).unwrap_or(false) {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            out.extend(walk(&e.path()));
        }
    }
    out
}

pub fn has(prog: &str) -> bool {
    std::env::var("PATH").unwrap_or_default().split(':').any(|d| Path::new(d).join(prog).is_file())
}

pub fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

pub fn git_init(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
}

/// Processes whose command line contains `marker`.
pub fn processes_with(marker: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        if pid == std::process::id() {
            continue;
        }
        if let Ok(cmd) = std::fs::read(e.path().join("cmdline")) {
            if String::from_utf8_lossy(&cmd).contains(marker) {
                out.push(pid);
            }
        }
    }
    out
}

/// A backend to run payloads under.
pub struct Backendish {
    pub name: &'static str,
    pub cap: Capability,
    pub capture: u64,
}

fn backends() -> Vec<Backendish> {
    let base = sandbox::detect();
    let capture = Profile::for_level(Level::E2).limits.capture_bytes;
    let mut v = Vec::new();
    if let Some(abi) = base.landlock_abi {
        v.push(Backendish {
            name: "landlock",
            cap: Capability { backend: Backend::Landlock { abi }, ..base.clone() },
            capture,
        });
        if base.user_namespaces {
            // Also without the network namespace: Landlock and seccomp alone must hold.
            let cap = Capability { backend: Backend::Landlock { abi }, user_namespaces: false, ..base.clone() };
            v.push(Backendish { name: "landlock-no-userns", cap, capture });
        }
    }
    if let Some(p) = &base.bwrap {
        v.push(Backendish {
            name: "bwrap",
            cap: Capability { backend: Backend::Bwrap { path: p.clone() }, ..base.clone() },
            capture,
        });
    }
    v
}

/// Run `f` under every available backend. With none, assert that nothing runs (fail closed).
pub fn for_each_backend(what: &str, f: impl Fn(&Backendish)) {
    // One payload at a time: RLIMIT_NPROC is per user, so a fork bomb in one test would starve
    // the others (all run as nobody when the suite runs as root).
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let list = backends();
    if list.is_empty() {
        let fx = Fixture::new();
        let r = Backendish::unavailable().try_run(&fx, &["/bin/sh".into(), "-c".into(), "echo ran".into()], 5_000);
        assert_eq!(r.expect_err("ran with no sandbox").stage, "sandbox");
        eprintln!("contained: {what} [no sandbox on this machine: asserted fail-closed]");
        return;
    }
    for b in &list {
        f(b);
        eprintln!("contained: {what} [{}]", b.name);
    }
}

impl Backendish {
    pub fn unavailable() -> Backendish {
        let base = sandbox::detect();
        Backendish {
            name: "none",
            cap: Capability { backend: Backend::Unavailable { reason: "test".into() }, ..base },
            capture: 0,
        }
    }

    fn inherited(extra: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut m: BTreeMap<String, String> = std::env::vars().collect();
        for (k, v) in extra {
            m.insert(k.to_string(), v.to_string());
        }
        m
    }

    pub fn env_for(&self, f: &Fixture, extra: &[(&str, &str)]) -> BTreeMap<String, String> {
        adage_exec::env::build(&Self::inherited(extra), f.repo.path(), Path::new("/tmp/adage-exec-probe"), &[])
    }

    pub fn try_run_with(
        &self,
        f: &Fixture,
        argv: &[String],
        wall_ms: u64,
        extra: &[(&str, &str)],
        force_extra: bool,
    ) -> Result<Outcome, Refusal> {
        let scratch = Scratch::new(sandbox::is_root().then_some(sandbox::NOBODY)).unwrap();
        let repo = f.repo.path().canonicalize().unwrap();
        let mut env = adage_exec::env::build(&Self::inherited(extra), &repo, &scratch.path, &[]);
        if force_extra {
            for (k, v) in extra {
                env.insert(k.to_string(), v.to_string());
            }
        }
        let mut profile = Profile::for_level(Level::E2);
        profile.limits.wall_ms = wall_ms;
        let dirs: Vec<PathBuf> = env["PATH"].split(':').map(PathBuf::from).collect();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let res = Resolved::new(&profile, &repo, &scratch.path, &dirs, None, home.as_deref()).unwrap();
        let program = if argv[0].starts_with('/') {
            PathBuf::from(&argv[0])
        } else {
            adage_exec::env::which(&argv[0], &env["PATH"]).unwrap_or_else(|| panic!("{} not on PATH", argv[0]))
        };
        spawn_sandboxed(&program, &argv[1..], &env, &res, &self.cap)
    }

    pub fn try_run(&self, f: &Fixture, argv: &[String], wall_ms: u64) -> Result<Outcome, Refusal> {
        self.try_run_with(f, argv, wall_ms, &[], false)
    }

    pub fn run_with(&self, f: &Fixture, argv: &[String], wall_ms: u64, extra: &[(&str, &str)], force: bool) -> Outcome {
        self.try_run_with(f, argv, wall_ms, extra, force).unwrap_or_else(|r| panic!("[{}] refused: {r}", self.name))
    }

    pub fn run(&self, f: &Fixture, argv: &[String], wall_ms: u64) -> Outcome {
        self.run_with(f, argv, wall_ms, &[], false)
    }

    pub fn sh(&self, f: &Fixture, script: &str, wall_ms: u64) -> Outcome {
        self.run(f, &["/bin/sh".into(), "-c".into(), script.into()], wall_ms)
    }

    pub fn py(&self, f: &Fixture, script: &str, wall_ms: u64) -> Outcome {
        self.run(f, &["python3".into(), "-c".into(), script.into()], wall_ms)
    }
}
