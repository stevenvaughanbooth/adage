//! The Linux sandbox: Landlock plus seccomp, applied in the child between `fork` and `execve`
//! (Track C §3.3).
//!
//! Everything that allocates (the Landlock ruleset and its rules, the BPF programs, the uid map
//! text) is built in the parent. The `pre_exec` hook then only makes system calls, in this order:
//!
//! 1. `setsid`: a new session and process group, so a timeout kills the whole group, and no
//!    controlling terminal;
//! 2. rlimits: `RLIMIT_AS` 2 GiB, `RLIMIT_NPROC` 256, `RLIMIT_FSIZE` 64 MiB, `RLIMIT_NOFILE`,
//!    no core dumps;
//! 3. when running as root: drop every capability from the bounding set, then all groups, then
//!    become nobody (65534). Root is exempt from `RLIMIT_NPROC`, and a sandbox should not rest on
//!    root's file ownership;
//! 4. where unprivileged user namespaces work: `unshare(CLONE_NEWUSER | CLONE_NEWNET)` with an
//!    identity map, so the process has no network interfaces at all. For an E2 stage also
//!    `CLONE_NEWNS`: the mount tree is made private and an overlay (`userxattr`) is mounted over
//!    the repository, its upper layer in a staging directory the sandbox is never granted, and
//!    the working directory is entered again through it;
//! 5. `PR_SET_NO_NEW_PRIVS` and Landlock: read-only access to the repository, the toolchain and
//!    the CAS, read-write to the scratch directory only; TCP bind and connect denied (ABI ≥ 4);
//!    signals and abstract Unix sockets scoped to the sandbox (ABI ≥ 6);
//! 6. seccomp: `socket` (every family: no AF_INET, AF_INET6, AF_NETLINK, AF_PACKET, nor a Unix
//!    socket to reach an agent), `ptrace`, `process_vm_*`, `mount` and the new mount API,
//!    `bpf`, `keyctl`, `unshare`, `setns`, `clone` with namespace flags, `io_uring`,
//!    `setsid`/`setpgid` (the process group cannot be left), `open_by_handle_at` and module
//!    loading all fail with `EPERM`; `clone3` fails with `ENOSYS`, so libc falls back to `clone`,
//!    whose flags the filter can see.

use landlock::{
    path_beneath_rules, Access, AccessFs, AccessNet, Ruleset, RulesetAttr, RulesetCreated, RulesetCreatedAttr,
    RulesetStatus, Scope, ABI,
};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule,
};
use std::collections::BTreeMap;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::OnceLock;

use super::Resolved;

/// The Landlock ABI the kernel offers, or `None` when Landlock is missing or disabled.
pub fn landlock_abi() -> Option<u32> {
    const LANDLOCK_CREATE_RULESET_VERSION: libc::c_uint = 1;
    // SAFETY: with a null attribute pointer, size 0 and the VERSION flag, the call only returns
    // the ABI version (or an error); it touches no memory.
    let r = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<libc::c_void>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    (r >= 1).then_some(r as u32)
}

/// Whether a sandboxed child could enter a user and a network namespace *with a working uid map*,
/// as the user it will run as. Probed once, in a forked child that does exactly what the
/// `pre_exec` hook does (drop root, unshare, write the maps) and exits. Some systems allow the
/// `unshare` but not the map (Ubuntu's AppArmor userns restriction), which must count as
/// unavailable.
pub fn user_namespaces_available() -> bool {
    static PROBE: OnceLock<bool> = OnceLock::new();
    *PROBE.get_or_init(|| {
        let drop_to = super::is_root().then_some(super::NOBODY);
        // SAFETY: getters with no preconditions.
        let (uid, gid) = drop_to.unwrap_or_else(|| unsafe { (libc::geteuid(), libc::getegid()) });
        let uid_map = format!("{uid} {uid} 1").into_bytes();
        let gid_map = format!("{gid} {gid} 1").into_bytes();
        // SAFETY: the child only makes system calls on data prepared before the fork.
        unsafe {
            let pid = libc::fork();
            if pid == 0 {
                let ok = drop_to.is_none_or(|(u, g)| drop_root(u, g).is_ok())
                    && matches!(enter_namespaces(&uid_map, &gid_map, false), Ok(true));
                libc::_exit(if ok { 0 } else { 1 });
            }
            if pid < 0 {
                return false;
            }
            let mut status = 0;
            libc::waitpid(pid, &mut status, 0);
            libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0
        }
    })
}

/// Drop every capability from the bounding set, all supplementary groups, then become `uid`/`gid`
/// (root only). Keeps the process dumpable, so `/proc/self` stays owned by it.
///
/// # Safety
/// Call only in a forked child before `execve`.
unsafe fn drop_root(uid: u32, gid: u32) -> io::Result<()> {
    for cap in 0..64 {
        // EINVAL past the last capability the kernel knows; nothing else fails while root.
        libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
    }
    check(libc::setgroups(0, std::ptr::null()))?;
    check(libc::setresgid(gid, gid, gid))?;
    check(libc::setresuid(uid, uid, uid))?;
    // Changing uid makes the process non-dumpable, which would leave /proc/self owned by root and
    // the uid map unwritable.
    check(libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0))
}

/// `unshare(CLONE_NEWUSER | CLONE_NEWNET)` (and `CLONE_NEWNS` when `mounts`) with an identity
/// map. `Ok(false)` when the kernel refuses the `unshare` (nothing changed); an error when it
/// allowed it but the map failed.
///
/// # Safety
/// Call only in a forked child before `execve`.
unsafe fn enter_namespaces(uid_map: &[u8], gid_map: &[u8], mounts: bool) -> io::Result<bool> {
    let flags = libc::CLONE_NEWUSER | libc::CLONE_NEWNET | if mounts { libc::CLONE_NEWNS } else { 0 };
    if libc::unshare(flags) != 0 {
        return Ok(false);
    }
    write_proc(c"/proc/self/setgroups", b"deny")?;
    write_proc(c"/proc/self/uid_map", uid_map)?;
    write_proc(c"/proc/self/gid_map", gid_map)?;
    Ok(true)
}

/// Tasks (threads) whose real uid is `uid`: `RLIMIT_NPROC` counts them all, so the budget a
/// sandbox gets is on top of what the user already runs.
pub(crate) fn tasks_of(uid: u32) -> u64 {
    let mut n = 0u64;
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        if !e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(status) = std::fs::read_to_string(e.path().join("status")) else { continue };
        let real = status.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|v| v.split_whitespace().next());
        if real.and_then(|r| r.parse::<u32>().ok()) == Some(uid) {
            let threads = status.lines().find_map(|l| l.strip_prefix("Threads:")).and_then(|v| v.trim().parse().ok());
            n += threads.unwrap_or(1);
        }
    }
    n
}

/// The highest ABI this build knows; the ruleset asks for everything up to it, best effort, and
/// the kernel enforces what it has.
const REQUESTED: ABI = ABI::V9;

/// Build the Landlock ruleset for a resolved profile (in the parent).
pub fn ruleset(res: &Resolved) -> Result<RulesetCreated, String> {
    let err = |e: landlock::RulesetError| format!("landlock: {e}");
    let read = AccessFs::from_read(REQUESTED);
    let all = AccessFs::from_all(REQUESTED);
    let dev_rw = read | AccessFs::WriteFile | AccessFs::Truncate;
    let created = Ruleset::default()
        .handle_access(all)
        .map_err(err)?
        .handle_access(AccessNet::from_all(REQUESTED))
        .map_err(err)?
        .scope(Scope::from_all(REQUESTED))
        .map_err(err)?
        .create()
        .map_err(err)?
        // E2: the repository is an overlay mounted in the child. Landlock treats an overlay as
        // distinct from its layers, so its rule is added there, on the mount itself.
        .add_rules(path_beneath_rules(res.read.iter().filter(|p| res.overlay.is_none() || **p != res.repo), read))
        .map_err(err)?
        .add_rules(path_beneath_rules(res.dev.iter().filter(|p| !p.ends_with("null")), read))
        .map_err(err)?
        .add_rules(path_beneath_rules(res.dev.iter().filter(|p| p.ends_with("null")), dev_rw))
        .map_err(err)?
        .add_rules(path_beneath_rules([&res.scratch], all))
        .map_err(err)?;
    Ok(created)
}

fn arch() -> Result<seccompiler::TargetArch, String> {
    std::env::consts::ARCH
        .try_into()
        .map_err(|_| format!("seccomp: unsupported architecture {}", std::env::consts::ARCH))
}

/// A system call number as seccompiler keys it (`c_long` is `i64` on every supported target).
#[allow(clippy::useless_conversion)]
fn nr(n: libc::c_long) -> i64 {
    i64::from(n)
}

/// System calls that always fail with `EPERM` inside the sandbox.
fn denied() -> Vec<i64> {
    #[allow(unused_mut)] // extended on x86_64 only
    let mut v: Vec<libc::c_long> = vec![
        libc::SYS_socket,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_bpf,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_perf_event_open,
        libc::SYS_userfaultfd,
        libc::SYS_kexec_load,
        libc::SYS_kexec_file_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_open_by_handle_at,
        libc::SYS_name_to_handle_at,
        libc::SYS_fsopen,
        libc::SYS_fsconfig,
        libc::SYS_fsmount,
        libc::SYS_fspick,
        libc::SYS_move_mount,
        libc::SYS_open_tree,
        libc::SYS_mount_setattr,
        libc::SYS_setsid,
        libc::SYS_setpgid,
        libc::SYS_swapon,
        libc::SYS_swapoff,
        libc::SYS_reboot,
        libc::SYS_acct,
        libc::SYS_quotactl,
        libc::SYS_syslog,
        libc::SYS_pidfd_getfd,
    ];
    #[cfg(target_arch = "x86_64")]
    v.extend([libc::SYS_iopl, libc::SYS_ioperm]);
    v.into_iter().map(nr).collect()
}

/// Namespace flags `clone` may not carry.
const NS_FLAGS: [u64; 7] = [
    libc::CLONE_NEWNS as u64,
    libc::CLONE_NEWCGROUP as u64,
    libc::CLONE_NEWUTS as u64,
    libc::CLONE_NEWIPC as u64,
    libc::CLONE_NEWUSER as u64,
    libc::CLONE_NEWPID as u64,
    libc::CLONE_NEWNET as u64,
];

/// On x86_64, the x32 ABI reaches the same calls with bit 30 set; deny those numbers too (the
/// filter's architecture check already kills i386 calls).
const X32_BIT: i64 = 0x4000_0000;

/// The seccomp programs, compiled in the parent.
pub fn filters() -> Result<Vec<BpfProgram>, String> {
    let arch = arch()?;
    let err = |e: seccompiler::BackendError| format!("seccomp: {e}");
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    for n in denied() {
        rules.insert(n, vec![]);
        if cfg!(target_arch = "x86_64") {
            rules.insert(n | X32_BIT, vec![]);
        }
    }
    let clone_rules = NS_FLAGS
        .iter()
        .map(|f| {
            SeccompRule::new(vec![
                SeccompCondition::new(0, SeccompCmpArgLen::Qword, SeccompCmpOp::MaskedEq(*f), *f).map_err(err)?
            ])
            .map_err(err)
        })
        .collect::<Result<Vec<_>, String>>()?;
    rules.insert(nr(libc::SYS_clone), clone_rules);
    if cfg!(target_arch = "x86_64") {
        rules.insert(nr(libc::SYS_clone) | X32_BIT, vec![]);
    }
    let deny: BpfProgram =
        SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), arch)
            .map_err(err)?
            .try_into()
            .map_err(err)?;
    let mut nosys = BTreeMap::new();
    nosys.insert(nr(libc::SYS_clone3), vec![]);
    if cfg!(target_arch = "x86_64") {
        nosys.insert(nr(libc::SYS_clone3) | X32_BIT, vec![]);
    }
    let clone3: BpfProgram =
        SeccompFilter::new(nosys, SeccompAction::Allow, SeccompAction::Errno(libc::ENOSYS as u32), arch)
            .map_err(err)?
            .try_into()
            .map_err(err)?;
    Ok(vec![deny, clone3])
}

/// An E2 overlay mount, as C strings built in the parent.
pub struct OverlayMount {
    target: std::ffi::CString,
    options: std::ffi::CString,
    cwd: std::ffi::CString,
    /// Every filesystem right this kernel's Landlock handles, granted beneath the overlay.
    access: u64,
}

/// Every filesystem access right of a Landlock ABI (a subset of what the ruleset handles, which
/// is all this build requests, best effort).
fn fs_rights(abi: u32) -> u64 {
    match abi {
        0 => 0,
        1 => (1 << 13) - 1,     // execute … make_sym
        2 => (1 << 14) - 1,     // + refer
        3 | 4 => (1 << 15) - 1, // + truncate
        _ => (1 << 16) - 1,     // + ioctl_dev
    }
}

/// `struct landlock_path_beneath_attr` (packed, as the kernel declares it).
#[repr(C, packed)]
struct PathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

/// Characters overlayfs's option parser gives a meaning to; a path holding one is refused.
fn overlay_path_ok(p: &std::path::Path) -> bool {
    let s = p.to_string_lossy();
    !s.is_empty() && !s.contains([',', ':', '\\', '=', '\0'])
}

fn overlay_mount(res: &Resolved) -> Result<Option<OverlayMount>, String> {
    let Some(o) = &res.overlay else { return Ok(None) };
    for p in [&res.repo, &o.upper, &o.work, &res.cwd] {
        if !overlay_path_ok(p) {
            return Err(format!("{} holds a character an overlay mount cannot take (, : \\ =)", p.display()));
        }
    }
    let c = |s: String| std::ffi::CString::new(s).map_err(|_| "a path holds NUL".to_string());
    Ok(Some(OverlayMount {
        target: c(res.repo.to_string_lossy().into_owned())?,
        options: c(format!(
            "lowerdir={},upperdir={},workdir={},userxattr",
            res.repo.display(),
            o.upper.display(),
            o.work.display()
        ))?,
        cwd: c(res.cwd.to_string_lossy().into_owned())?,
        access: fs_rights(landlock_abi().unwrap_or(0)),
    }))
}

/// E1 as root: the repository bind-mounted read-only where the run (as nobody) can reach it,
/// in a mount namespace of its own (`Resolved::bind`).
struct BindMount {
    source: std::ffi::CString,
    target: std::ffi::CString,
    /// The working directory, entered again through the bind mount.
    cwd: std::ffi::CString,
}

fn bind_mount(res: &Resolved) -> Result<Option<BindMount>, String> {
    let Some(target) = &res.bind else { return Ok(None) };
    if res.overlay.is_some() {
        return Err("a repository is bind-mounted for E1 only".into());
    }
    let rel = res.cwd.strip_prefix(&res.repo).map_err(|_| "the working directory is outside the repository")?;
    let c =
        |p: &std::path::Path| std::ffi::CString::new(p.to_string_lossy().into_owned()).map_err(|_| "a path holds NUL");
    Ok(Some(BindMount { source: c(&res.repo)?, target: c(target)?, cwd: c(&target.join(rel))? }))
}

/// Everything the `pre_exec` hook needs, prepared in the parent.
pub struct Prepared {
    ruleset: Option<RulesetCreated>,
    filters: Vec<BpfProgram>,
    limits: crate::limits::Limits,
    drop_to: Option<(u32, u32)>,
    userns: bool,
    uid_map: Vec<u8>,
    gid_map: Vec<u8>,
    /// `RLIMIT_NPROC`: the profile's budget on top of the tasks the user already runs.
    nproc: u64,
    /// E2: the overlay to mount over the repository.
    overlay: Option<OverlayMount>,
    /// E1 as root: the repository bind-mounted where nobody can reach it.
    bind: Option<BindMount>,
}

pub fn prepare(res: &Resolved, userns: bool) -> Result<Prepared, String> {
    // SAFETY: getters with no preconditions.
    let (uid, gid) = res.drop_to.unwrap_or_else(|| unsafe { (libc::geteuid(), libc::getegid()) });
    let overlay = overlay_mount(res)?;
    if overlay.is_some() && !userns {
        return Err("an E2 stage mounts its overlay in a user namespace, and user namespaces are not available".into());
    }
    Ok(Prepared {
        ruleset: Some(ruleset(res)?),
        filters: filters()?,
        limits: res.limits,
        drop_to: res.drop_to,
        userns,
        uid_map: format!("{uid} {uid} 1").into_bytes(),
        gid_map: format!("{gid} {gid} 1").into_bytes(),
        nproc: super::nproc_budget(res).map_err(|e| format!("process limit: {e}"))?,
        overlay,
        bind: bind_mount(res)?,
    })
}

fn check(r: libc::c_int) -> io::Result<()> {
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Write `data` to a `/proc/self` file with raw system calls (async-signal-safe).
///
/// # Safety
/// Call only in the child between `fork` and `execve`.
unsafe fn write_proc(path: &std::ffi::CStr, data: &[u8]) -> io::Result<()> {
    let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let n = libc::write(fd, data.as_ptr().cast(), data.len());
    libc::close(fd);
    if n != data.len() as isize {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_env = "gnu")]
type Resource = libc::__rlimit_resource_t;
#[cfg(not(target_env = "gnu"))]
type Resource = libc::c_int;

unsafe fn rlimit(res: Resource, v: u64) -> io::Result<()> {
    let lim = libc::rlimit { rlim_cur: v as libc::rlim_t, rlim_max: v as libc::rlim_t };
    check(libc::setrlimit(res, &lim))
}

/// Install the sandbox on `cmd`: the hook runs in the child, after `fork`, before `execve`.
pub fn install(cmd: &mut Command, prepared: Prepared) {
    let Prepared { mut ruleset, filters, limits, drop_to, userns, uid_map, gid_map, nproc, overlay, bind } = prepared;
    // SAFETY: the closure only makes system calls on data prepared before the fork. The one
    // library call, `restrict_self`, wraps `prctl` and `landlock_restrict_self`.
    unsafe {
        cmd.pre_exec(move || {
            check(libc::setsid())?;
            rlimit(libc::RLIMIT_AS, limits.address_space)?;
            rlimit(libc::RLIMIT_NPROC, nproc)?;
            rlimit(libc::RLIMIT_FSIZE, limits.file_size)?;
            rlimit(libc::RLIMIT_NOFILE, limits.open_files)?;
            rlimit(libc::RLIMIT_CORE, 0)?;
            if let Some(b) = &bind {
                // Still root: a mount namespace of the run's own, the repository bound read-only
                // at a path nobody can reach, and the working directory entered through it.
                check(libc::unshare(libc::CLONE_NEWNS))?;
                check(libc::mount(
                    std::ptr::null(),
                    c"/".as_ptr(),
                    std::ptr::null(),
                    libc::MS_REC | libc::MS_PRIVATE,
                    std::ptr::null(),
                ))?;
                check(libc::mount(
                    b.source.as_ptr(),
                    b.target.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND | libc::MS_REC,
                    std::ptr::null(),
                ))?;
                check(libc::mount(
                    std::ptr::null(),
                    b.target.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV,
                    std::ptr::null(),
                ))?;
                check(libc::chdir(b.cwd.as_ptr()))?;
            }
            if let Some((uid, gid)) = drop_to {
                drop_root(uid, gid)?;
            }
            if userns && !enter_namespaces(&uid_map, &gid_map, overlay.is_some())? && overlay.is_some() {
                return Err(io::Error::from_raw_os_error(libc::EPERM));
            }
            if let Some(o) = &overlay {
                check(libc::mount(
                    std::ptr::null(),
                    c"/".as_ptr(),
                    std::ptr::null(),
                    libc::MS_REC | libc::MS_PRIVATE,
                    std::ptr::null(),
                ))?;
                check(libc::mount(
                    c"overlay".as_ptr(),
                    o.target.as_ptr(),
                    c"overlay".as_ptr(),
                    0,
                    o.options.as_ptr().cast(),
                ))?;
                // `Command::current_dir` entered the directory under the mount; enter it again.
                check(libc::chdir(o.cwd.as_ptr()))?;
            }
            check(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0))?;
            let rs = ruleset.take().ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))?;
            if let Some(o) = &overlay {
                // The overlay's rule, on the mount itself (opened now that it exists), then the
                // domain, with raw system calls: nothing here may allocate.
                let fd: Option<std::os::fd::OwnedFd> = rs.into();
                let fd = fd.ok_or_else(|| io::Error::from_raw_os_error(libc::ENOTSUP))?;
                let rfd = std::os::fd::AsRawFd::as_raw_fd(&fd);
                let dir = libc::open(o.target.as_ptr(), libc::O_PATH | libc::O_CLOEXEC | libc::O_DIRECTORY);
                if dir < 0 {
                    return Err(io::Error::last_os_error());
                }
                let attr = PathBeneathAttr { allowed_access: o.access, parent_fd: dir };
                let added = libc::syscall(
                    libc::SYS_landlock_add_rule,
                    rfd,
                    1 as libc::c_int,
                    &attr as *const PathBeneathAttr,
                    0 as libc::c_uint,
                );
                libc::close(dir);
                if added != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::syscall(libc::SYS_landlock_restrict_self, rfd, 0 as libc::c_uint) != 0 {
                    return Err(io::Error::last_os_error());
                }
                drop(fd);
            } else {
                match rs.restrict_self() {
                    Ok(s) if s.ruleset != RulesetStatus::NotEnforced => {}
                    _ => return Err(io::Error::from_raw_os_error(libc::ENOTSUP)),
                }
            }
            for f in &filters {
                seccompiler::apply_filter(f).map_err(|_| io::Error::from_raw_os_error(libc::ENOTSUP))?;
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_compile_for_this_architecture() {
        let f = filters().unwrap();
        assert_eq!(f.len(), 2);
        assert!(!f[0].is_empty());
    }
}
