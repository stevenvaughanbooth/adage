//! Running a request: the gates in order, the sandboxed spawn with its limits, then projection
//! and the data block (Track C §3, §4). [`check`] is everything short of running (and is what
//! `--dry-run` shows); [`execute`] runs; [`spawn_sandboxed`] is the bounded, sandboxed spawn on
//! its own, which the escape suite drives directly.

use adage_core::{CommandTable, Effect};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

use crate::approvals::{self, ApprovalKey, AutoContext, Store};
use crate::policy::Policy;
use crate::request::{ExecRequest, Level};
use crate::sandbox::{self, Backend, Capability, Resolved, Write};
use crate::{classify, env, git, project, render};

/// Why nothing was delivered. `stage` names the gate; `reason` never contains output text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub stage: &'static str,
    pub reason: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.reason)
    }
}

fn refuse<T>(stage: &'static str, reason: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal { stage, reason: reason.into() })
}

/// A fresh scratch directory: `HOME` and `TMPDIR` for the run, the only writable place, removed
/// afterwards. Created with `O_EXCL` semantics under a fixed system directory (never `$TMPDIR`,
/// which the caller controls and could point into the repository).
pub struct Scratch {
    pub path: PathBuf,
}

impl Scratch {
    pub fn new(owner: Option<(u32, u32)>) -> std::io::Result<Scratch> {
        static N: AtomicU64 = AtomicU64::new(0);
        let base = if cfg!(target_os = "linux") { PathBuf::from("/tmp") } else { std::env::temp_dir() };
        let base = base.canonicalize()?;
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let path =
            base.join(format!("adage-exec-{}-{nanos:x}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        #[allow(unused_mut)] // configured on Unix only
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(&path)?; // fails if anything already exists there, symlinks included
        let s = Scratch { path };
        for sub in ["tmp", ".config", ".cache", "bin"] {
            b.create(s.path.join(sub))?;
        }
        #[cfg(unix)]
        if let Some((uid, gid)) = owner {
            for p in
                [s.path.clone(), s.path.join("tmp"), s.path.join(".config"), s.path.join(".cache"), s.path.join("bin")]
            {
                std::os::unix::fs::chown(&p, Some(uid), Some(gid))?;
            }
        }
        #[cfg(not(unix))]
        let _ = owner;
        Ok(s)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// What a sandboxed spawn produced.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    #[serde(skip)]
    pub stdout: Vec<u8>,
    /// Bytes of stdout seen (at most one read past the cap).
    pub stdout_bytes: u64,
    #[serde(skip)]
    pub stderr: Vec<u8>,
    pub truncated: bool,
    pub timed_out: bool,
    pub duration_ms: u64,
}

#[cfg(unix)]
/// Read a pipe, keeping at most `cap` bytes. Past the cap, set `over` and stop (closing the pipe,
/// so the writer gets `EPIPE`) when `stop_at_cap`; otherwise keep draining and discard.
fn read_capped(mut r: impl Read, cap: u64, over: Arc<AtomicBool>, stop_at_cap: bool) -> (Vec<u8>, u64) {
    let mut kept = Vec::new();
    let mut total = 0u64;
    let mut buf = [0u8; 16 * 1024];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                total += n as u64;
                let room = cap.saturating_sub(kept.len() as u64) as usize;
                kept.extend_from_slice(&buf[..n.min(room)]);
                if total > cap {
                    over.store(true, Ordering::SeqCst);
                    if stop_at_cap {
                        break;
                    }
                }
            }
        }
    }
    (kept, total)
}

#[cfg(unix)]
fn kill_group(pgid: u32) {
    // SAFETY: kill has no memory preconditions; a negative pid names the process group.
    unsafe {
        libc::kill(-(pgid as i32), libc::SIGKILL);
    }
}

#[cfg(unix)]
/// Become a child subreaper (Linux), so descendants orphaned by the run are re-parented to this
/// process rather than to init, and [`reap_group`] can collect them: a zombie still counts
/// against `RLIMIT_NPROC`, and not every init reaps.
fn subreaper() {
    #[cfg(target_os = "linux")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        // SAFETY: prctl with integer arguments.
        ONCE.call_once(|| unsafe {
            libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        });
    }
}

/// Kill and reap every process left in the run's process group. Only that group is waited for,
/// so other children of this process are untouched.
#[cfg(unix)]
fn reap_group(pgid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        kill_group(pgid);
        // SAFETY: waitpid on a process group with WNOHANG; the status is a local.
        let r = unsafe {
            let mut status = 0;
            libc::waitpid(-(pgid as i32), &mut status, libc::WNOHANG)
        };
        if r < 0 || Instant::now() > deadline {
            break; // ECHILD: nothing left in the group
        }
        if r == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// `setsid`, rlimits and (for root) dropping to nobody, for backends that bring their own
/// isolation (bwrap, sandbox-exec).
#[cfg(unix)]
fn basic_pre_exec(cmd: &mut Command, limits: crate::limits::Limits, nproc: u64, drop_to: Option<(u32, u32)>) {
    use std::os::unix::process::CommandExt;
    // SAFETY: only system calls on values copied into the closure.
    unsafe {
        cmd.pre_exec(move || {
            let check = |r: libc::c_int| if r < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) };
            check(libc::setsid())?;
            if let Some((uid, gid)) = drop_to {
                #[cfg(target_os = "linux")]
                for cap in 0..64 {
                    libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
                }
                check(libc::setgroups(0, std::ptr::null()))?;
                check(libc::setgid(gid))?;
                check(libc::setuid(uid))?;
            }
            let set = |res, v: u64| {
                let lim = libc::rlimit { rlim_cur: v as libc::rlim_t, rlim_max: v as libc::rlim_t };
                libc::setrlimit(res, &lim)
            };
            // RLIMIT_AS is not enforced on macOS; failures there are ignored.
            let _ = set(libc::RLIMIT_AS, limits.address_space);
            // Respect an inherited hard ceiling, and refuse if this essential bound cannot
            // be installed. Never silently run without a process-count limit.
            let mut inherited = std::mem::zeroed::<libc::rlimit>();
            check(libc::getrlimit(libc::RLIMIT_NPROC, &mut inherited))?;
            check(set(libc::RLIMIT_NPROC, nproc.min(inherited.rlim_max as u64)))?;
            let _ = set(libc::RLIMIT_FSIZE, limits.file_size);
            let _ = set(libc::RLIMIT_NOFILE, limits.open_files);
            let _ = set(libc::RLIMIT_CORE, 0);
            Ok(())
        });
    }
}

/// Spawn `program args…` under the capability's backend, with `env` exactly, stdin from
/// `/dev/null`, in `res.cwd`, bounded by `res.limits`. The whole process group is killed on
/// timeout, on output past the capture cap, and after the program exits (stragglers).
#[cfg(unix)]
pub fn spawn_sandboxed(
    program: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    res: &Resolved,
    cap: &Capability,
) -> Result<Outcome, Refusal> {
    spawn_sandboxed_with(program, args, env, res, cap, 8 * 1024)
}

/// [`spawn_sandboxed`], keeping at most `stderr_cap` bytes of stderr (the first ones). A replay
/// ([`crate::replay`]) keeps more: a test runner's verdict is often on stderr.
#[cfg(unix)]
pub(crate) fn spawn_sandboxed_with(
    program: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    res: &Resolved,
    cap: &Capability,
    stderr_cap: u64,
) -> Result<Outcome, Refusal> {
    let limits = res.limits;
    let mut cmd = match &cap.backend {
        Backend::Unavailable { reason } => return refuse("sandbox", format!("no sandbox available: {reason}")),
        #[cfg(target_os = "linux")]
        Backend::Landlock { .. } => {
            let prepared = sandbox::linux::prepare(res, cap.user_namespaces)
                .map_err(|e| Refusal { stage: "sandbox", reason: e })?;
            let mut c = Command::new(program);
            c.args(args);
            sandbox::linux::install(&mut c, prepared);
            c
        }
        #[cfg(unix)]
        Backend::Bwrap { path } => {
            let (p, a) = sandbox::bwrap::wrap(path, program, args, res);
            let mut c = Command::new(p);
            c.args(a);
            let nproc = sandbox::nproc_budget(res)
                .map_err(|e| Refusal { stage: "limits", reason: format!("process limit: {e}") })?;
            basic_pre_exec(&mut c, limits, nproc, res.drop_to);
            c
        }
        #[cfg(unix)]
        Backend::SandboxExec { path } => {
            let (p, a) = sandbox::macos::wrap(path, program, args, res);
            let mut c = Command::new(p);
            c.args(a);
            let nproc = sandbox::nproc_budget(res)
                .map_err(|e| Refusal { stage: "limits", reason: format!("process limit: {e}") })?;
            basic_pre_exec(&mut c, limits, nproc, res.drop_to);
            c
        }
        #[allow(unreachable_patterns)]
        other => return refuse("sandbox", format!("backend {other:?} is not supported on this OS")),
    };
    subreaper();
    cmd.env_clear().envs(env).current_dir(&res.cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| Refusal { stage: "sandbox", reason: format!("could not start the sandboxed process: {e}") })?;
    let pid = child.id();
    let over = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<(bool, Vec<u8>, u64)>();
    if let Some(out) = child.stdout.take() {
        let (tx, over) = (tx.clone(), over.clone());
        std::thread::spawn(move || {
            let (b, n) = read_capped(out, limits.capture_bytes, over, true);
            let _ = tx.send((true, b, n));
        });
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let (b, n) = read_capped(err, stderr_cap, Arc::new(AtomicBool::new(false)), false);
            let _ = tx.send((false, b, n));
        });
    }
    drop(tx);
    let wall = Duration::from_millis(limits.wall_ms);
    let mut o = Outcome::default();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if start.elapsed() > wall {
            o.timed_out = true;
            #[cfg(unix)]
            kill_group(pid);
            #[cfg(not(unix))]
            let _ = child.kill();
        }
        if over.load(Ordering::SeqCst) {
            o.truncated = true;
            #[cfg(unix)]
            kill_group(pid);
            #[cfg(not(unix))]
            let _ = child.kill();
        }
        std::thread::sleep(Duration::from_millis(3));
    };
    // Stragglers: anything left in the group (it cannot leave: setsid and setpgid are denied).
    #[cfg(unix)]
    kill_group(pid);
    let status = status.or_else(|| child.wait().ok());
    #[cfg(unix)]
    reap_group(pid);
    let deadline = Instant::now() + Duration::from_secs(2);
    while let Ok((is_out, bytes, n)) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if is_out {
            o.stdout = bytes;
            o.stdout_bytes = n;
        } else {
            o.stderr = bytes;
        }
    }
    o.truncated |= over.load(Ordering::SeqCst);
    o.duration_ms = start.elapsed().as_millis() as u64;
    if let Some(s) = status {
        o.exit = s.code();
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            o.signal = s.signal();
        }
    }
    Ok(o)
}

/// No sandbox exists on this OS: nothing runs (Windows: E0 only).
#[cfg(not(unix))]
pub(crate) fn spawn_sandboxed_with(
    _program: &Path,
    _args: &[String],
    _env: &BTreeMap<String, String>,
    _res: &Resolved,
    _cap: &Capability,
    _stderr_cap: u64,
) -> Result<Outcome, Refusal> {
    refuse("sandbox", "no sandbox is available on this OS (E0 only)")
}

/// No sandbox exists on this OS: nothing runs (Windows: E0 only).
#[cfg(not(unix))]
pub fn spawn_sandboxed(
    _program: &Path,
    _args: &[String],
    _env: &BTreeMap<String, String>,
    _res: &Resolved,
    _cap: &Capability,
) -> Result<Outcome, Refusal> {
    refuse("sandbox", "no sandbox is available on this OS (E0 only)")
}

/// Everything a run needs from its surroundings.
pub struct Context<'a> {
    /// The caller's environment: only `PATH`, and names policy allows, are read from it.
    pub inherited_env: BTreeMap<String, String>,
    pub policy: &'a Policy,
    pub store: &'a Store,
    pub capability: &'a Capability,
    pub table: &'a CommandTable,
    /// `repo_key` of the repository.
    pub repo_key: String,
    pub cas: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub canaries: Vec<String>,
}

/// The approval state of a request.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ApprovalState {
    Approved { approved_on: String },
    Auto,
    NotApproved { auto_reasons: Vec<String>, lapsed: Vec<Vec<approvals::Change>> },
}

/// The result of the gates short of running: what would run, and whether it may.
#[derive(Debug, Clone, Serialize)]
pub struct Checked {
    pub bound: Vec<String>,
    pub classified_effect: Effect,
    pub key: ApprovalKey,
    pub approval: ApprovalState,
    pub git_hazards: Vec<String>,
    pub profile: String,
    pub profile_id: String,
    pub sandbox: String,
}

/// Gates 1–5: binding, classification, policy, approval, sandbox. `Ok` only when the request may
/// run; the [`Checked`] is also what `--dry-run` prints.
pub fn check(req: &ExecRequest, ctx: &Context) -> Result<Checked, (Refusal, Option<Box<Checked>>)> {
    let plain = |r: Refusal| (r, None);
    let request = |reason: String| plain(Refusal { stage: "request", reason });
    let bound = req.bind().map_err(|e| request(e.to_string()))?;
    crate::project_python::validate(req).map_err(request)?;
    // An action's summary needs no typed fields (it reports its outcome and changed paths).
    if !(req.level == Level::E2 && req.output.mode == crate::request::OutputMode::Typed && req.output.fields.is_empty())
    {
        req.output.validate().map_err(|e| request(format!("output spec: {e}")))?;
    }
    if req.profile.net {
        return Err(request("a profile with network access is never run".into()));
    }
    if req.level == Level::E2 {
        let Some(action) = &req.action else {
            return Err(request(
                "an E2 request declares its action (regenerate or verify) and the outputs it may change".into(),
            ));
        };
        action.validate().map_err(|e| request(format!("action: {e}")))?;
        if req.profile.write != Write::Overlay {
            return Err(request("an E2 action runs in a staging overlay (profile write: overlay)".into()));
        }
        match req.effect {
            Effect::Verify | Effect::Regenerate => {}
            Effect::Environment if ctx.policy.allow_environment_actions => {}
            Effect::Environment => {
                return Err(request(
                    "an environment action needs the managed policy's opt-in (exec.allow_environment_actions)".into(),
                ))
            }
            Effect::Observe => return Err(request("an observe step is an E1 prefetch, not an E2 action".into())),
            e => return Err(request(format!("an E2 action never has the {} effect", e.as_str()))),
        }
    } else {
        if req.profile.write == Write::Overlay || req.action.is_some() {
            return Err(request("only an E2 action runs in a writable overlay".into()));
        }
        if req.effect > Effect::Observe {
            return Err(request(format!("effect {} needs E2 staging; E1 runs observe only", req.effect.as_str())));
        }
    }
    let classified = classify::effect(&bound, ctx.table);
    let has_scripts = !req.scripts.is_empty() || req.project_python.is_some();
    // Declaring less than the computed effect refuses, except for scripts a person approved at a
    // lower effect (§2.2): the sandbox then enforces the approved profile.
    if classified > req.effect && !has_scripts {
        return Err(plain(Refusal {
            stage: "classify",
            reason: format!(
                "declared effect {} is lower than the classified {}",
                req.effect.as_str(),
                classified.as_str()
            ),
        }));
    }
    ctx.policy.permits(req, classified).map_err(|e| plain(Refusal { stage: "policy", reason: e }))?;
    let git_hazards = git::hazards(&req.repo_root);
    let key = ApprovalKey::for_request(req, &ctx.repo_key);
    let route = approvals::route_label(req);
    let approval = match ctx.store.find(&key) {
        Some(a) => ApprovalState::Approved { approved_on: a.approved_on.clone() },
        None => {
            let actx = AutoContext {
                table: ctx.table,
                git_hazards: &git_hazards,
                sandbox_enforced: ctx.capability.available(),
                policy_auto: ctx.policy.auto_approve,
            };
            match approvals::auto_eligible(req, &actx) {
                Ok(()) => ApprovalState::Auto,
                Err(reasons) => ApprovalState::NotApproved {
                    auto_reasons: reasons,
                    lapsed: ctx.store.lapsed(&key, &route).into_iter().map(|(_, d)| d).collect(),
                },
            }
        }
    };
    let checked = Checked {
        bound,
        classified_effect: classified,
        key,
        approval: approval.clone(),
        git_hazards,
        profile: req.profile.summary(),
        profile_id: req.profile.id(),
        sandbox: ctx.capability.line(),
    };
    if let ApprovalState::NotApproved { lapsed, .. } = &approval {
        let reason = if lapsed.is_empty() {
            "no approval matches this route on this machine (adage-exec approve …)".to_string()
        } else {
            let fields: Vec<&str> = lapsed.iter().flatten().map(|c| c.field).collect();
            format!("exec-changed: the approval for this route no longer matches ({})", fields.join(", "))
        };
        return Err((Refusal { stage: "approval", reason }, Some(Box::new(checked))));
    }
    if !ctx.capability.available() {
        let reason = match &ctx.capability.backend {
            Backend::Unavailable { reason } => reason.clone(),
            _ => String::new(),
        };
        return Err((
            Refusal { stage: "sandbox", reason: format!("E1 and above are unavailable: {reason}") },
            Some(Box::new(checked)),
        ));
    }
    Ok(checked)
}

/// A delivered block, and the spool record (never the output itself).
#[derive(Debug, Clone, Serialize)]
pub struct Delivered {
    pub block: String,
    pub record: Record,
}

/// `exec: { level, exit, bytes, sha256(output), truncated, duration }` (§3.3).
#[derive(Debug, Clone, Serialize)]
pub struct Record {
    pub level: Level,
    pub exit: Option<i32>,
    pub bytes: u64,
    pub sha256: String,
    pub truncated: bool,
    pub duration_ms: u64,
    pub auto: bool,
}

/// Most entries mirrored from one repository directory into the shadow.
const SHADOW_ENTRIES: usize = 4096;

/// Write a hash-checked script's bytes to `<shadow>/<rel>`, inside a shadow of the repository:
/// along the script's path, every other entry of each directory is a symlink to the real one. The
/// private copy is what runs (a change to the repository file after the check cannot reach it),
/// yet a script that finds its data relative to itself (`Path(__file__).parent.parent /
/// ".tracker"`) still finds the repository's. The shadow lives in scratch; the symlinks lead only
/// into the repository, which the sandbox already lets the run read.
fn shadow_copy(repo: &Path, link_to: &Path, shadow: &Path, rel: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let parts: Vec<&str> = rel.split('/').collect();
    // `real` is read here; the links lead to `link_to`, where the run sees the same directory
    // (the repository itself, or its bind mount).
    let (mut real, mut seen, mut fake) = (repo.to_path_buf(), link_to.to_path_buf(), shadow.to_path_buf());
    let make_dir = |d: &Path| -> std::io::Result<()> {
        match std::fs::symlink_metadata(d) {
            Ok(m) if m.file_type().is_symlink() => {
                std::fs::remove_file(d)?;
                std::fs::create_dir(d)
            }
            Ok(_) => Ok(()),
            Err(_) => std::fs::create_dir(d),
        }
    };
    make_dir(&fake)?;
    for (i, part) in parts.iter().enumerate() {
        for e in std::fs::read_dir(&real)?.take(SHADOW_ENTRIES).flatten() {
            let name = e.file_name();
            let link = fake.join(&name);
            if name.to_string_lossy() == *part || std::fs::symlink_metadata(&link).is_ok() {
                continue;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(seen.join(&name), &link)?;
        }
        if i + 1 == parts.len() {
            let copy = fake.join(part);
            if std::fs::symlink_metadata(&copy).is_ok() {
                std::fs::remove_file(&copy)?;
            }
            std::fs::write(&copy, bytes)?;
            return Ok(copy);
        }
        real = real.join(part);
        seen = seen.join(part);
        fake = fake.join(part);
        make_dir(&fake)?;
    }
    Err(std::io::Error::other("empty script path"))
}

fn sha256_file(p: &Path) -> std::io::Result<(String, Vec<u8>)> {
    let bytes = std::fs::read(p)?;
    Ok((crate::sha256_hex(&bytes), bytes))
}

/// A vendored script's bytes from the CAS, hash-checked **now**, and the path the run reads it
/// from: the read-only CAS entry itself when the sandbox is granted the CAS, else a read-only
/// private copy in scratch (a run as nobody cannot reach a CAS under root's home).
fn vendored(
    s: &crate::request::ScriptRef,
    hex: &str,
    argv: &[String],
    ctx: &Context,
    scratch: &Path,
    res: &Resolved,
) -> Result<PathBuf, Refusal> {
    if !adage_pack::validate::sha256_hex_ok(hex) || hex != s.sha256 {
        return refuse("script", format!("vendored script {:?} is not named by its SHA-256", s.path));
    }
    let interpreter = argv[0].rsplit('/').next().unwrap_or_default();
    if !adage_pack::validate::VENDOR_INTERPRETERS.contains(&interpreter) {
        return refuse(
            "script",
            format!(
                "a vendored script runs under an interpreter named in argv ({}); its shebang is ignored",
                adage_pack::validate::VENDOR_INTERPRETERS.join(", ")
            ),
        );
    }
    let short = &hex[..12];
    let cas = ctx
        .cas
        .as_ref()
        .ok_or_else(|| Refusal { stage: "script", reason: "there is no CAS on this machine".into() })?;
    let cas = cas.canonicalize().map_err(|_| Refusal { stage: "script", reason: "the CAS is missing".into() })?;
    let entry = cas.join(hex);
    match std::fs::symlink_metadata(&entry) {
        Ok(m) if m.file_type().is_file() => {}
        _ => return refuse("script", format!("vendored script {short} is not in the CAS")),
    }
    let (hash, bytes) =
        sha256_file(&entry).map_err(|e| Refusal { stage: "script", reason: format!("reading {short}: {e}") })?;
    if hash != s.sha256 {
        return refuse("script", format!("vendored script {short} in the CAS does not match its hash"));
    }
    if res.read.iter().any(|r| cas.starts_with(r)) {
        return Ok(entry);
    }
    let dir = scratch.join("vendor");
    let _ = std::fs::create_dir(&dir);
    let copy = dir.join(hex);
    std::fs::write(&copy, &bytes).map_err(|e| Refusal { stage: "script", reason: format!("copying {short}: {e}") })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o444));
        if let Some((u, g)) = res.drop_to {
            let _ = std::os::unix::fs::chown(&dir, Some(u), Some(g));
        }
    }
    Ok(copy)
}

/// The program to spawn and its full argv, for a checked request: L-ref scripts are re-read and
/// hash-checked **now** and run from a private copy in scratch; vendored scripts are hash-checked
/// in the CAS and run by the interpreter named in argv; git is hardened; anything else is resolved
/// on the sanitised `PATH`.
pub(crate) fn resolve_program(
    req: &ExecRequest,
    bound: &[String],
    repo: &Path,
    scratch: &Path,
    res: &Resolved,
    environment: &BTreeMap<String, String>,
    ctx: &Context,
) -> Result<(PathBuf, Vec<String>), Refusal> {
    let mut argv = bound.to_vec();
    let mut vendored_paths = Vec::new();
    for s in &req.scripts {
        if let Some(hex) = s.path.strip_prefix("cas:") {
            let path = vendored(s, hex, &argv, ctx, scratch, res)?;
            let token = adage_pack::vendor::script_path(hex);
            let mut found = false;
            for a in argv.iter_mut().skip(1) {
                if a.trim_start_matches("./") == token {
                    *a = path.to_string_lossy().into_owned();
                    found = true;
                }
            }
            if !found {
                return refuse("script", format!("argv does not run the vendored script {}", &hex[..12]));
            }
            vendored_paths.push(path);
            continue;
        }
        let rel = s.path.trim_start_matches("./");
        if !adage_core::repo::is_safe_relative(rel) {
            return refuse("script", format!("script {:?} is not a repository-relative path", s.path));
        }
        let real = repo
            .join(rel)
            .canonicalize()
            .map_err(|_| Refusal { stage: "script", reason: format!("script {rel} is missing") })?;
        if !real.starts_with(repo) || !real.is_file() {
            return refuse("script", format!("script {rel} does not resolve to a file inside the repository"));
        }
        let (hash, bytes) =
            sha256_file(&real).map_err(|e| Refusal { stage: "script", reason: format!("reading {rel}: {e}") })?;
        if hash != s.sha256 {
            return refuse("script", format!("script {rel} changed since it was approved (sha256 {hash})"));
        }
        let copy = shadow_copy(repo, res.seen_repo(), &scratch.join("shadow"), rel, &bytes)
            .map_err(|e| Refusal { stage: "script", reason: format!("copying {rel} into scratch: {e}") })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o555));
            if let Some((u, g)) = res.drop_to {
                let _ = std::os::unix::fs::chown(&copy, Some(u), Some(g));
            }
        }
        for a in argv.iter_mut() {
            if a.trim_start_matches("./") == rel {
                *a = copy.to_string_lossy().into_owned();
            }
        }
    }
    // A path left in argv that no script entry covers is a dangling reference (never a vendored
    // script's own path: argv[0] is its interpreter).
    for (i, a) in argv.iter().enumerate() {
        let is_ours = a.starts_with(scratch.to_string_lossy().as_ref())
            || vendored_paths.iter().any(|p| p.as_os_str() == a.as_str());
        if a.starts_with("scripts/sha256-") && !is_ours {
            return refuse(
                "script",
                format!("argv[{i}] names a vendored script with no script entry (a dangling reference)"),
            );
        }
    }
    let program: PathBuf = if argv[0].starts_with('/') && argv[0].starts_with(scratch.to_string_lossy().as_ref()) {
        PathBuf::from(&argv[0])
    } else if argv[0].contains('/') {
        return refuse("script", format!("{} names a path with no script entry (a dangling reference)", bound[0]));
    } else {
        env::which(&argv[0], &environment["PATH"])
            .ok_or_else(|| Refusal { stage: "request", reason: format!("{} is not on the sanitised PATH", argv[0]) })?
    };
    let is_git = program.file_name().is_some_and(|n| n == "git");
    #[cfg(target_os = "macos")]
    let program = sandbox::macos::interpreter(program);
    let full = if is_git { git::harden(&argv, res.drop_to.map(|_| res.seen_repo())) } else { argv };
    Ok((program, full))
}

/// Run a checked request: resolve the program ([`resolve_program`]: scripts are hash-checked
/// **now**, closing the gap between check and use), build the environment, spawn in the sandbox,
/// project, render. E2 actions run through [`crate::stage`] instead.
pub fn execute(req: &ExecRequest, ctx: &Context) -> Result<Delivered, Refusal> {
    if req.level == Level::E2 {
        return refuse("request", "an E2 action is staged and applied by adage_exec::stage, never run directly");
    }
    let checked = check(req, ctx).map_err(|(r, _)| r)?;
    let res_probe = sandbox::is_root().then_some(sandbox::NOBODY);
    let scratch =
        Scratch::new(res_probe).map_err(|e| Refusal { stage: "sandbox", reason: format!("scratch directory: {e}") })?;
    let repo = req
        .repo_root
        .canonicalize()
        .map_err(|e| Refusal { stage: "request", reason: format!("repository root: {e}") })?;
    let environment = env::build(&ctx.inherited_env, &repo, &scratch.path, &ctx.policy.env);
    let path_dirs: Vec<PathBuf> = environment["PATH"].split(':').map(PathBuf::from).collect();
    let mut res =
        Resolved::new(&req.profile, &repo, &scratch.path, &path_dirs, ctx.cas.as_deref(), ctx.home.as_deref())
            .map_err(|e| Refusal { stage: "sandbox", reason: format!("resolving the profile: {e}") })?;
    // As root the run is nobody's: a repository nobody can reach is bound where it can.
    res.bind_repo(&ctx.capability.backend).map_err(|e| Refusal { stage: "sandbox", reason: e })?;
    let (program, full) = resolve_program(req, &checked.bound, &repo, &scratch.path, &res, &environment, ctx)?;
    let o = spawn_sandboxed(&program, &full[1..], &environment, &res, ctx.capability)?;
    if o.timed_out {
        return refuse("limits", format!("timed out after {} ms", req.profile.limits.wall_ms));
    }
    if o.truncated {
        return refuse("limits", format!("output passed the {}-byte capture limit", req.profile.limits.capture_bytes));
    }
    let exit = match (o.exit, o.signal) {
        (Some(c), _) => c,
        (None, Some(s)) => return refuse("run", format!("killed by signal {s}")),
        _ => return refuse("run", "exit status unknown"),
    };
    // A complete-output contract replaces a lookup only after it succeeded. Error text emitted
    // before a nonzero exit is not a completed result, even if it passes the text scanner.
    if exit != 0 && req.output.unstructured() {
        return refuse("run", format!("lookup exited {exit}; complete output was withheld"));
    }
    let payload = project::project(&o.stdout, &req.output, &repo, &ctx.canaries, req.profile.limits.return_bytes)
        .map_err(|w| Refusal { stage: "output", reason: format!("{w} (exit {exit})") })?;
    let block = render::data_block_checked(
        &req.labels.track,
        &req.labels.route,
        &req.labels.step,
        exit,
        req.output.mode,
        &checked.bound,
        &payload,
    )
    .map_err(|why| Refusal { stage: "render", reason: why.into() })?;
    Ok(Delivered {
        block,
        record: Record {
            level: req.level,
            exit: Some(exit),
            bytes: o.stdout_bytes,
            sha256: crate::sha256_hex(&o.stdout),
            truncated: o.truncated,
            duration_ms: o.duration_ms,
            auto: matches!(checked.approval, ApprovalState::Auto),
        },
    })
}
