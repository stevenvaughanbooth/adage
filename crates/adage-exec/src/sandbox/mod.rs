//! The sandbox (Track C §3.3): one policy struct, [`Profile`], enforced per OS.
//!
//! - Linux: Landlock plus a seccomp filter, applied in-process before `execve`, with user and
//!   network namespaces where available ([`linux`]); bubblewrap when Landlock is missing
//!   ([`bwrap`]).
//! - macOS: `sandbox-exec` with a generated SBPL profile ([`macos`]).
//! - Anything else, or none of the above: unavailable, and E1 and above fail closed.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::limits::Limits;
use crate::request::Level;

pub mod bwrap;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod macos;

/// What the sandboxed process may read, symbolically. Resolved to paths per run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Read {
    /// The repository root.
    Repo,
    /// System and toolchain directories: `/usr`, `/bin`, `/lib*`, the sanitised `PATH`, and the
    /// few `/etc` files the dynamic loader and libc need.
    Toolchain,
    /// `$ADAGE_HOME/cas`, where vendored scripts live.
    Cas,
}

/// What it may write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Write {
    /// Nothing but a fresh scratch directory (`HOME` and `TMPDIR`), removed after the run.
    Scratch,
    /// A writable overlay of the repository (E2): the run sees the repository and may change it,
    /// but every change lands in a staging layer outside the sandbox's reach, never in the
    /// repository ([`crate::stage`]).
    Overlay,
}

/// The sandbox policy. Its [`id`](Profile::id) is part of the approval key: change the profile
/// and approvals lapse.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Profile {
    pub read: Vec<Read>,
    pub write: Write,
    /// Network access. Always false in C2; a profile with `true` is refused.
    pub net: bool,
    /// The environment variable names the run gets (values are fixed or scratch paths).
    pub env: Vec<String>,
    /// Working directory: the repository root, or a safe relative subdirectory.
    pub cwd: Option<String>,
    pub limits: Limits,
}

/// Bumped whenever enforcement changes in a way approvals should notice.
pub const PROFILE_VERSION: u32 = 3;

impl Profile {
    pub fn for_level(level: Level) -> Profile {
        Profile {
            read: vec![Read::Repo, Read::Toolchain, Read::Cas],
            write: if level == Level::E2 { Write::Overlay } else { Write::Scratch },
            net: false,
            env: crate::env::names(),
            cwd: None,
            limits: Limits::for_level(level),
        }
    }

    /// The profile's stable id: SHA-256 of its canonical JSON with the enforcement version.
    pub fn id(&self) -> String {
        let v = serde_json::json!({ "version": PROFILE_VERSION, "profile": self });
        crate::sha256_hex(crate::canonical_json(&v).as_bytes())
    }

    /// One line for review screens.
    pub fn summary(&self) -> String {
        let reads: Vec<&str> = self
            .read
            .iter()
            .map(|r| match r {
                Read::Repo => "repository",
                Read::Toolchain => "toolchain",
                Read::Cas => "cas",
            })
            .collect();
        let writes = match self.write {
            Write::Scratch => "scratch only",
            Write::Overlay => "repository overlay",
        };
        let directory_names = if cfg!(target_os = "macos") { ", /tmp directory names" } else { "" };
        format!(
            "reads: {}{directory_names} · writes: {writes} · network: {} · timeout: {} ms",
            reads.join(", "),
            if self.net { "on" } else { "off" },
            self.limits.wall_ms
        )
    }
}

/// The mechanism that enforces a profile on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Backend {
    /// Landlock (this ABI) plus seccomp, in-process.
    Landlock { abi: u32 },
    /// bubblewrap at this path.
    Bwrap { path: PathBuf },
    /// macOS `sandbox-exec` at this path.
    SandboxExec { path: PathBuf },
    /// No sandbox: E1 and above are unavailable.
    Unavailable { reason: String },
}

/// What this machine can enforce, for `doctor` and for choosing a backend.
#[derive(Debug, Clone, Serialize)]
pub struct Capability {
    pub os: &'static str,
    pub landlock_abi: Option<u32>,
    pub user_namespaces: bool,
    pub bwrap: Option<PathBuf>,
    pub sandbox_exec: Option<PathBuf>,
    pub running_as_root: bool,
    pub backend: Backend,
}

impl Capability {
    pub fn available(&self) -> bool {
        !matches!(self.backend, Backend::Unavailable { .. })
    }

    /// One line: "Linux, Landlock ABI 7: E1–E2 available".
    pub fn line(&self) -> String {
        match &self.backend {
            Backend::Landlock { abi } => format!(
                "{}, Landlock ABI {abi} + seccomp{}: E1-E2 available",
                self.os,
                if self.user_namespaces { " + user/net namespaces" } else { "" }
            ),
            Backend::Bwrap { path } => format!("{}, bubblewrap ({}): E1-E2 available", self.os, path.display()),
            Backend::SandboxExec { path } => format!("{}, sandbox-exec ({}): E1-E2 available", self.os, path.display()),
            Backend::Unavailable { reason } => format!("{}: E1 and above unavailable ({reason})", self.os),
        }
    }
}

/// Look for an executable on the fixed system directories only (never the caller's `PATH`: a
/// planted `bwrap` earlier in `PATH` must not become the sandbox).
pub fn system_binary(name: &str) -> Option<PathBuf> {
    ["/usr/bin", "/bin", "/usr/local/bin", "/usr/sbin", "/sbin"]
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
}

/// Probe this machine. `ADAGE_EXEC_FORCE_BACKEND` (`landlock`, `bwrap`, `none`) narrows the
/// choice for tests and `doctor`; it can only remove options, never add one.
pub fn detect() -> Capability {
    let force = std::env::var("ADAGE_EXEC_FORCE_BACKEND").ok();
    let running_as_root = is_root();
    #[cfg(target_os = "linux")]
    {
        let abi = linux::landlock_abi();
        let userns = linux::user_namespaces_available();
        let bwrap = system_binary("bwrap");
        let want = |b: &str| force.as_deref().is_none_or(|f| f == b);
        let backend = if let (Some(abi), true) = (abi, want("landlock")) {
            Backend::Landlock { abi }
        } else if let (Some(p), true) = (bwrap.clone(), want("bwrap")) {
            Backend::Bwrap { path: p }
        } else {
            Backend::Unavailable {
                reason: match (abi, &bwrap, force.as_deref()) {
                    (_, _, Some("none")) => "sandbox disabled by ADAGE_EXEC_FORCE_BACKEND=none".into(),
                    (None, None, _) => {
                        "the kernel does not offer Landlock and bubblewrap (bwrap) is not installed".into()
                    }
                    _ => format!("the requested backend {:?} is not available", force.unwrap_or_default()),
                },
            }
        };
        Capability {
            os: "Linux",
            landlock_abi: abi,
            user_namespaces: userns,
            bwrap,
            sandbox_exec: None,
            running_as_root,
            backend,
        }
    }
    #[cfg(target_os = "macos")]
    {
        let se = system_binary("sandbox-exec");
        let backend = match (&se, force.as_deref()) {
            (_, Some("none")) => {
                Backend::Unavailable { reason: "sandbox disabled by ADAGE_EXEC_FORCE_BACKEND=none".into() }
            }
            (Some(p), _) => Backend::SandboxExec { path: p.clone() },
            (None, _) => Backend::Unavailable { reason: "sandbox-exec is not present".into() },
        };
        Capability {
            os: "macOS",
            landlock_abi: None,
            user_namespaces: false,
            bwrap: None,
            sandbox_exec: se,
            running_as_root,
            backend,
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = force;
        Capability {
            os: std::env::consts::OS,
            landlock_abi: None,
            user_namespaces: false,
            bwrap: None,
            sandbox_exec: None,
            running_as_root,
            backend: Backend::Unavailable {
                reason: "no supported sandbox on this OS (Windows: E0 only; WSL counts as Linux)".into(),
            },
        }
    }
}

pub fn is_root() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Toolchain paths every backend grants read access to (when they exist). `/etc` is not granted
/// wholesale: only what the loader, libc and time zones need.
pub const SYSTEM_READ: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/localtime",
    "/etc/nsswitch.conf",
    "/etc/passwd",
    "/etc/group",
    "/etc/alternatives",
    "/etc/os-release",
    "/System",
    "/Library/Developer/CommandLineTools",
    "/opt/homebrew",
    "/private/var/db/timezone",
    "/nix/store",
];

/// Device files: read only, except `/dev/null`, which is also writable.
pub const DEV_READ: &[&str] = &["/dev/null", "/dev/zero", "/dev/urandom", "/dev/random", "/dev/full"];

/// A profile resolved to paths for one run.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub repo: PathBuf,
    pub cwd: PathBuf,
    pub scratch: PathBuf,
    /// Read-only paths (repository, toolchain, CAS), all existing and canonical.
    pub read: Vec<PathBuf>,
    /// Device files readable.
    pub dev: Vec<PathBuf>,
    /// System paths that are symlinks (`/bin` → `usr/bin`), for backends that build a new
    /// mount tree (bwrap) and must recreate them. Landlock resolves them itself.
    pub links: Vec<(PathBuf, PathBuf)>,
    pub limits: Limits,
    /// The user the run drops to (root runs as nobody, so `RLIMIT_NPROC` applies).
    pub drop_to: Option<(u32, u32)>,
    /// E2 on Linux: the repository is mounted as a writable overlay whose upper layer is here.
    pub overlay: Option<Overlay>,
    /// E1 on Linux (Landlock), running as root: where the repository is bind-mounted read-only
    /// for a run as nobody who cannot reach it where it is (a mode-700 ancestor): an empty
    /// directory in scratch, which the run's own mount namespace mounts it over
    /// ([`Resolved::bind_repo`]).
    pub bind: Option<PathBuf>,
}

/// An overlay's writable layers (Linux, E2). Both live in a staging directory the sandbox is
/// never granted: the run reaches them only through the overlay mounted over the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    pub upper: PathBuf,
    pub work: PathBuf,
}

/// Whether a user with no ownership or group claim can reach `p`: every ancestor searchable by
/// others, and `p` itself readable by others.
pub fn others_can_reach(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |q: &Path| std::fs::metadata(q).map(|m| m.permissions().mode()).unwrap_or(0);
        let ancestors_ok = p.ancestors().skip(1).all(|a| mode(a) & 0o001 != 0);
        ancestors_ok && mode(p) & 0o004 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        true
    }
}

/// `RLIMIT_NPROC` for a run: the profile's budget on top of the tasks the run's user already has
/// (the limit counts the user's existing tasks/processes, not just the sandbox's). A failed
/// macOS process census refuses the run instead of silently removing its bound.
pub fn nproc_budget(res: &Resolved) -> std::io::Result<u64> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: getuid has no preconditions.
        let uid = res.drop_to.map(|(u, _)| u).unwrap_or_else(|| unsafe { libc::getuid() });
        Ok(res.limits.nproc.saturating_add(linux::tasks_of(uid)))
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: getuid has no preconditions.
        let uid = res.drop_to.map(|(u, _)| u).unwrap_or_else(|| unsafe { libc::getuid() });
        Ok(res.limits.nproc.saturating_add(macos::processes_of(uid)?))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Ok(res.limits.nproc)
    }
}

/// The unprivileged user root runs as.
pub const NOBODY: (u32, u32) = (65534, 65534);

impl Resolved {
    /// Resolve `profile` for a repository, a scratch directory, and the sanitised `PATH`
    /// directories (toolchain). Paths that are `/`, the real home directory, or an ancestor of
    /// either, are never granted, whatever `PATH` holds.
    pub fn new(
        profile: &Profile,
        repo: &Path,
        scratch: &Path,
        path_dirs: &[PathBuf],
        cas: Option<&Path>,
        home: Option<&Path>,
    ) -> std::io::Result<Resolved> {
        let repo = repo.canonicalize()?;
        let cwd = match &profile.cwd {
            Some(sub) if adage_core::repo::is_safe_relative(sub) => {
                let c = repo.join(sub).canonicalize()?;
                if !c.starts_with(&repo) {
                    return Err(std::io::Error::other("cwd resolves outside the repository"));
                }
                c
            }
            Some(_) => return Err(std::io::Error::other("cwd is not a safe relative path")),
            None => repo.clone(),
        };
        let home = home.and_then(|h| h.canonicalize().ok());
        let mut read: Vec<PathBuf> = Vec::new();
        let mut add = |p: &Path| {
            if let Ok(c) = p.canonicalize() {
                let too_broad =
                    c == Path::new("/") || home.as_ref().is_some_and(|h| h.starts_with(&c)) || scratch.starts_with(&c);
                if !too_broad && !read.contains(&c) {
                    read.push(c);
                }
            }
        };
        for r in &profile.read {
            match r {
                Read::Repo => add(&repo),
                Read::Toolchain => {
                    for s in SYSTEM_READ {
                        add(Path::new(s));
                    }
                    for d in path_dirs {
                        add(d);
                    }
                    #[cfg(target_os = "macos")]
                    if let Some(developer) = macos::developer_dir() {
                        add(&developer);
                        // xcodebuild's linked frameworks live beside Developer, inside the
                        // selected app bundle. Never extend this to the Applications directory.
                        if let Some(contents) =
                            developer.parent().filter(|p| p.file_name().is_some_and(|n| n == "Contents"))
                        {
                            if contents.parent().is_some_and(|p| p.extension().is_some_and(|e| e == "app")) {
                                add(contents);
                            }
                        }
                    }
                }
                Read::Cas => {
                    if let Some(c) = cas {
                        add(c);
                    }
                }
            }
        }
        let dev = DEV_READ.iter().map(PathBuf::from).filter(|p| p.exists()).collect();
        let drop_to = if is_root() { Some(NOBODY) } else { None };
        if drop_to.is_some() {
            // Nobody could not reach these anyway (for example `/root/.cargo/bin`), and bwrap,
            // running as nobody, refuses to bind what it cannot see.
            read.retain(|p| p == &repo || others_can_reach(p));
        }
        let links = if profile.read.contains(&Read::Toolchain) {
            SYSTEM_READ
                .iter()
                .filter_map(|s| {
                    let p = Path::new(s);
                    let target = std::fs::read_link(p).ok()?;
                    // Only links whose target is granted (or lies under a granted path).
                    let real = p.canonicalize().ok()?;
                    read.iter().any(|r| real.starts_with(r)).then(|| (p.to_path_buf(), target))
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(Resolved {
            repo,
            cwd,
            scratch: scratch.to_path_buf(),
            read,
            dev,
            links,
            limits: profile.limits,
            drop_to,
            overlay: None,
            bind: None,
        })
    }

    /// Where the run sees the repository: the bind mount when there is one, else where it is.
    pub fn seen_repo(&self) -> &Path {
        self.bind.as_deref().unwrap_or(&self.repo)
    }

    /// For a run as nobody (root drops to it) that cannot reach the repository where it is: bind
    /// the repository read-only at `<scratch>/repo`, a path nobody can reach, in the run's own mount
    /// namespace (Linux, the Landlock backend). The repository directory itself must be readable
    /// and searchable by others: a bind mount changes where it is, not who may read it.
    pub fn bind_repo(&mut self, backend: &Backend) -> Result<(), String> {
        let Some((uid, gid)) = self.drop_to else { return Ok(()) };
        if others_can_reach(&self.repo) {
            return Ok(());
        }
        if !matches!(backend, Backend::Landlock { .. }) {
            return Err(format!(
                "the sandbox runs as nobody, who cannot reach {} (an ancestor is not searchable by others), \
                 and only the Landlock backend can bind it where nobody can; run as an ordinary user, or make \
                 its ancestors searchable (chmod o+x)",
                self.repo.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&self.repo).map(|m| m.permissions().mode()).unwrap_or(0);
            if mode & 0o005 != 0o005 {
                return Err(format!(
                    "the sandbox runs as nobody, and {} itself is not readable by others (mode {:o})",
                    self.repo.display(),
                    mode & 0o777
                ));
            }
            let target = self.scratch.join("repo");
            std::fs::create_dir(&target).map_err(|e| format!("bind mount point: {e}"))?;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
            let _ = std::os::unix::fs::chown(&target, Some(uid), Some(gid));
            self.bind = Some(target);
        }
        #[cfg(not(unix))]
        let _ = (uid, gid);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_id_is_stable_and_sensitive() {
        let a = Profile::for_level(Level::E1);
        assert_eq!(a.id(), Profile::for_level(Level::E1).id());
        assert_eq!(a.id().len(), 64);
        assert_ne!(a.id(), Profile::for_level(Level::E2).id());
        let mut b = a.clone();
        b.net = true;
        assert_ne!(a.id(), b.id());
        assert_eq!(
            a.summary(),
            format!(
                "reads: repository, toolchain, cas{} · writes: scratch only · network: off · timeout: 1500 ms",
                if cfg!(target_os = "macos") { ", /tmp directory names" } else { "" }
            )
        );
    }

    #[test]
    fn version_three_lapses_grants_from_the_previous_enforcement_profile() {
        assert_eq!(PROFILE_VERSION, 3);
        let profile = Profile::for_level(Level::E2);
        let previous = serde_json::json!({ "version": 2, "profile": profile });
        let old_id = crate::sha256_hex(crate::canonical_json(&previous).as_bytes());
        assert_ne!(profile.id(), old_id, "the added directory-node permission requires renewed approval");
    }

    #[test]
    fn never_grants_root_or_home() {
        let d = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let p = Profile::for_level(Level::E1);
        let home = std::env::temp_dir();
        let r = Resolved::new(
            &p,
            d.path(),
            scratch.path(),
            &[PathBuf::from("/"), home.clone(), PathBuf::from("/usr/bin")],
            None,
            Some(&home),
        )
        .unwrap();
        assert!(!r.read.contains(&PathBuf::from("/")));
        assert!(!r.read.iter().any(|x| home.canonicalize().unwrap().starts_with(x)), "{:?}", r.read);
        assert!(r.read.contains(&d.path().canonicalize().unwrap()));
    }
}
