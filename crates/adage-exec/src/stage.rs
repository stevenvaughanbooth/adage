//! E2: actions, staged, verified, then applied (Track C §3.6).
//!
//! An action runs from a hook Adage already owns: when an approved action reflex's stimulus fires
//! (an edit matching its paths, or its unmet finish line at Stop), `adage-exec hook` runs it
//! ([`crate::hook`]). Every run goes through the gates of [`crate::run::check`] (an explicit E2
//! approval matching on every field; never automatic), then:
//!
//! 1. **Stage.** The command runs in the sandbox with the repository as a *writable overlay*:
//!    - Linux: `bwrap --overlay-src <repo> --overlay <upper> <work> <repo>` when the installed
//!      bubblewrap has overlays (0.10 and later), else an overlayfs (`userxattr`) mounted in the
//!      sandbox's own user and mount namespaces, under Landlock and seccomp as every run is. The
//!      upper layer lives in a staging directory the sandbox is never granted, so every change
//!      the command makes lands there and none reaches the repository;
//!    - macOS: an APFS `clonefile` copy of the worktree in scratch (compile-checked here; running
//!      it needs the macOS runner);
//!    - Linux as root: sandboxed commands run as nobody, who cannot write root's files through an
//!      overlay (overlayfs keeps the lower layer's owners), so the worktree is copied into
//!      scratch, owned by nobody, and the copy is compared with the worktree afterwards.
//! 2. **Verify** the change set ([`changes`], [`judge`]). A path under `.git` always refuses. For
//!    `regenerate`, every change must match a declared output glob and be a regular file written
//!    or removed; changes under cache globs are discarded; anything else refuses the whole
//!    application. For `verify`, no source may change; changes under cache globs are kept.
//! 3. **Apply** ([`apply`]): the kept changes are written into the real worktree atomically per
//!    file (a temporary file beside it, fsync, rename), and only if each path's state still
//!    equals the state recorded before the run (optimistic concurrency against the agent's own
//!    edits). Any mismatch applies nothing.
//! 4. **Record** `outcome: applied | refused(reason) | failed(exit)` in
//!    `$ADAGE_HOME/exec/actions.spool` (never file contents). After three refusals of the change
//!    set under one approval, that approval is revoked automatically ([`REVOKE_AFTER`]).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::approvals::{self, Store};
use crate::request::{ActionKind, ActionSpec, ExecRequest, Level};
use crate::run::{self, Context, Refusal, Scratch};
use crate::sandbox::{self, Backend, Capability, Overlay, Resolved};
use crate::{env, project};

/// Refusals of the change set under one approval that revoke it.
pub const REVOKE_AFTER: usize = 3;
/// Most entries a change-set walk visits.
pub const MAX_ENTRIES: usize = 20_000;
/// Most bytes an application writes in total.
pub const MAX_APPLY_BYTES: u64 = 256 << 20;
/// Most paths a result lists.
pub const MAX_LISTED: usize = 50;

fn refuse<T>(stage: &'static str, reason: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal { stage, reason: reason.into() })
}

// ---------------------------------------------------------------------------------------------
// Which staging mechanism
// ---------------------------------------------------------------------------------------------

/// How this machine stages an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StageBackend {
    /// bubblewrap with `--overlay-src` (0.10 and later).
    BwrapOverlay {
        path: PathBuf,
    },
    /// An overlayfs mounted in the sandbox's own user and mount namespaces (Landlock backend).
    UsernsOverlay,
    /// An APFS `clonefile` copy of the worktree (macOS).
    Clonefile,
    /// A copy of the worktree in scratch (Linux, when running as root).
    Copy,
    Unavailable {
        reason: String,
    },
}

impl StageBackend {
    pub fn available(&self) -> bool {
        !matches!(self, StageBackend::Unavailable { .. })
    }

    pub fn line(&self) -> String {
        match self {
            StageBackend::BwrapOverlay { path } => format!("bubblewrap overlay ({})", path.display()),
            StageBackend::UsernsOverlay => "overlayfs in a user namespace, under Landlock and seccomp".into(),
            StageBackend::Clonefile => "APFS clonefile copy".into(),
            StageBackend::Copy => "a copy of the worktree (running as root: commands run as nobody)".into(),
            StageBackend::Unavailable { reason } => format!("unavailable ({reason})"),
        }
    }
}

/// Whether a bubblewrap binary has `--overlay-src` (bubblewrap 0.10 and later). Asked once.
pub fn bwrap_has_overlay(path: &Path) -> bool {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<BTreeMap<PathBuf, bool>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(path).copied()) {
        return v;
    }
    let found = std::process::Command::new(path)
        .arg("--help")
        .env_clear()
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("--overlay-src"))
        .unwrap_or(false);
    if let Ok(mut c) = cache.lock() {
        c.insert(path.to_path_buf(), found);
    }
    found
}

/// The staging mechanism for this machine. bubblewrap with overlays is preferred when present;
/// `ADAGE_EXEC_FORCE_BACKEND` narrows the choice as it does for the sandbox.
pub fn backend(cap: &Capability) -> StageBackend {
    if !cap.available() {
        return StageBackend::Unavailable { reason: "no sandbox is available on this machine".into() };
    }
    if cfg!(target_os = "macos") {
        return StageBackend::Clonefile;
    }
    if cap.running_as_root {
        return StageBackend::Copy;
    }
    let force = std::env::var("ADAGE_EXEC_FORCE_BACKEND").ok();
    if let Some(b) = cap.bwrap.as_ref().filter(|_| force.as_deref() != Some("landlock")) {
        if bwrap_has_overlay(b) {
            return StageBackend::BwrapOverlay { path: b.clone() };
        }
    }
    match &cap.backend {
        Backend::Landlock { .. } if cap.user_namespaces => StageBackend::UsernsOverlay,
        Backend::Landlock { .. } => StageBackend::Unavailable {
            reason: "the overlay is mounted in a user namespace, and user namespaces are not available".into(),
        },
        _ => StageBackend::Unavailable {
            reason: "bubblewrap has no --overlay-src (0.10 or later) and Landlock is not available".into(),
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Paths and states
// ---------------------------------------------------------------------------------------------

/// A glob over repository paths: `*` stays within a segment, `**` crosses them, and a leading
/// dot is matched only literally.
fn pattern(g: &str) -> Option<glob::Pattern> {
    glob::Pattern::new(g).ok()
}

const MATCH: glob::MatchOptions =
    glob::MatchOptions { case_sensitive: true, require_literal_separator: true, require_literal_leading_dot: true };

/// Whether `rel` matches any of `globs`.
pub fn matches_any(globs: &[String], rel: &str) -> bool {
    globs.iter().filter_map(|g| pattern(g)).any(|p| p.matches_with(rel, MATCH))
}

/// A path as a person or agent may see it: plain characters only.
fn shown(rel: &str) -> String {
    if !rel.is_empty() && rel.len() <= 200 && rel.chars().all(|c| c.is_ascii_alphanumeric() || "_./@+-".contains(c)) {
        rel.to_string()
    } else {
        "(a path with unprintable characters)".into()
    }
}

/// What a path holds in the worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FileState {
    Absent,
    File {
        sha256: String,
        exec: bool,
    },
    /// A directory, a symlink, anything else, or a path through a symlink.
    Other,
}

#[cfg(unix)]
fn exec_bit(m: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn exec_bit(_: &std::fs::Metadata) -> bool {
    false
}

/// The state of `repo/<rel>`, without following any symlink on the way.
pub fn state_of(repo: &Path, rel: &str) -> FileState {
    let parts: Vec<&str> = rel.split('/').collect();
    let mut p = repo.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        p.push(part);
        let m = match std::fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(_) => return FileState::Absent,
        };
        let last = i + 1 == parts.len();
        if !last && !m.file_type().is_dir() {
            return FileState::Other;
        }
        if last {
            if !m.file_type().is_file() {
                return FileState::Other;
            }
            return match std::fs::read(&p) {
                Ok(b) => FileState::File { sha256: crate::sha256_hex(&b), exec: exec_bit(&m) },
                Err(_) => FileState::Other,
            };
        }
    }
    FileState::Other
}

/// Every path under `dir` (relative to `base`, prefixed by `prefix`), without following symlinks:
/// files, symlinks and special files (not directories), bounded by `budget`.
fn walk(dir: &Path, prefix: &str, skip_git: bool, out: &mut Vec<String>, budget: &mut usize) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("reading directory: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("reading directory entry: {e}"))?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if *budget == 0 {
            return Err(format!("more than {MAX_ENTRIES} entries"));
        }
        *budget -= 1;
        let name = e.file_name().to_string_lossy().into_owned();
        if skip_git && prefix.is_empty() && name == ".git" {
            continue;
        }
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&e.path(), &rel, skip_git, out, budget)?,
            Ok(_) => out.push(rel),
            Err(e) => return Err(format!("reading entry type: {e}")),
        }
    }
    Ok(())
}

/// The states, before the run, of every worktree path an action could apply: those matching its
/// outputs (regenerate) or cache globs (both), outside `.git`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreState(pub BTreeMap<String, FileState>);

impl PreState {
    pub fn take(repo: &Path, action: &ActionSpec) -> Result<PreState, Refusal> {
        let mut files = Vec::new();
        let mut budget = MAX_ENTRIES;
        walk(repo, "", true, &mut files, &mut budget)
            .map_err(|e| Refusal { stage: "stage", reason: format!("the worktree has {e}") })?;
        let mut m = BTreeMap::new();
        for rel in files {
            let kept = matches_any(&action.cache, &rel)
                || (action.kind == ActionKind::Regenerate && matches_any(&action.outputs, &rel));
            if kept {
                m.insert(rel.clone(), state_of(repo, &rel));
            }
        }
        Ok(PreState(m))
    }

    pub fn expected(&self, rel: &str) -> FileState {
        self.0.get(rel).cloned().unwrap_or(FileState::Absent)
    }
}

/// Repository inputs (outside declared outputs/cache and .git), captured before the process
/// starts and checked immediately before applying. Bounded like the staged change set; an
/// unreadable or oversized input set refuses rather than blessing a stale result.
pub fn input_snapshot(repo: &Path, action: &ActionSpec) -> Result<BTreeMap<String, String>, Refusal> {
    input_snapshot_until(repo, action, None)
}

/// [`input_snapshot`], abandoned (stage `time`) once `deadline` passes: for a hook that must
/// answer within its timeout and can do without the snapshot (a receipt note, the Stop gate).
pub fn input_snapshot_until(
    repo: &Path,
    action: &ActionSpec,
    deadline: Option<std::time::Instant>,
) -> Result<BTreeMap<String, String>, Refusal> {
    let late = || deadline.is_some_and(|d| std::time::Instant::now() >= d);
    let over = || Refusal { stage: "time", reason: "the input snapshot did not finish within the time left".into() };
    let mut files = Vec::new();
    walk(repo, "", true, &mut files, &mut MAX_ENTRIES.clone()).map_err(|e| Refusal { stage: "inputs", reason: e })?;
    let mut bytes = 0u64;
    let mut out = BTreeMap::new();
    for rel in files {
        if late() {
            return Err(over());
        }
        if matches_any(&action.cache, &rel)
            || (action.kind == ActionKind::Regenerate && matches_any(&action.outputs, &rel))
        {
            continue;
        }
        let path = repo.join(&rel);
        let read = || -> std::io::Result<(String, u64)> {
            use std::io::Read;
            let m = std::fs::symlink_metadata(&path)?;
            if m.file_type().is_symlink() {
                return Ok((format!("link:{}", std::fs::read_link(&path)?.to_string_lossy()), 0));
            }
            if !m.is_file() {
                return Err(std::io::Error::other("input is not a regular file or symlink"));
            }
            let left = MAX_APPLY_BYTES.saturating_sub(bytes);
            if m.len() > left {
                return Err(std::io::Error::other("repository inputs exceed the 256 MiB snapshot bound"));
            }
            let mut data = Vec::new();
            std::fs::File::open(&path)?.take(left + 1).read_to_end(&mut data)?;
            if data.len() as u64 > left {
                return Err(std::io::Error::other("input grew past the 256 MiB snapshot bound"));
            }
            Ok((format!("{}:{}", crate::sha256_hex(&data), exec_bit(&m)), data.len() as u64))
        };
        let (digest, n) = read().map_err(|e| Refusal { stage: "inputs", reason: format!("reading an input: {e}") })?;
        bytes += n;
        out.insert(rel, digest);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The change set
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

/// What a changed path holds after the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    File {
        bytes: Vec<u8>,
        exec: bool,
    },
    Absent,
    Symlink,
    /// A FIFO, a socket, a device, or anything else that is never applied.
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
    pub after: After,
    /// Whether the worktree held a regular file there before.
    pub before_file: bool,
}

#[cfg(target_os = "linux")]
fn xattr(p: &Path, name: &str) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    let name = std::ffi::CString::new(name).ok()?;
    let mut buf = [0u8; 256];
    // SAFETY: both strings are NUL-terminated; the buffer and its length are ours.
    let n = unsafe { libc::lgetxattr(path.as_ptr(), name.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    (n >= 0).then(|| buf[..n as usize].to_vec())
}

#[cfg(not(target_os = "linux"))]
fn xattr(_: &Path, _: &str) -> Option<Vec<u8>> {
    None
}

/// Overlay metadata in either namespace (`userxattr` mounts use `user.`).
fn ovl_attr(p: &Path, name: &str) -> Option<Vec<u8>> {
    xattr(p, &format!("user.overlay.{name}")).or_else(|| xattr(p, &format!("trusted.overlay.{name}")))
}

fn lower_kind(repo: &Path, rel: &str) -> Option<std::fs::FileType> {
    let mut p = repo.to_path_buf();
    let parts: Vec<&str> = rel.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        p.push(part);
        let m = std::fs::symlink_metadata(&p).ok()?;
        if i + 1 < parts.len() && !m.file_type().is_dir() {
            return None;
        }
        if i + 1 == parts.len() {
            return Some(m.file_type());
        }
    }
    None
}

fn read_after(p: &Path, m: &std::fs::Metadata) -> Result<After, String> {
    let t = m.file_type();
    if t.is_symlink() {
        return Ok(After::Symlink);
    }
    if !t.is_file() {
        return Ok(After::Special);
    }
    let bytes = std::fs::read(p).map_err(|e| format!("reading a staged file: {e}"))?;
    Ok(After::File { bytes, exec: exec_bit(m) })
}

/// Compare a staged path with the worktree's: `None` when nothing changed.
fn compare(repo: &Path, rel: &str, after: After) -> Option<Change> {
    let before = lower_kind(repo, rel);
    let before_file = before.is_some_and(|t| t.is_file());
    if let (After::File { bytes, exec }, true) = (&after, before_file) {
        let same = std::fs::symlink_metadata(repo.join(rel)).is_ok_and(|m| exec_bit(&m) == *exec)
            && std::fs::read(repo.join(rel)).is_ok_and(|b| &b == bytes);
        if same {
            return None;
        }
    }
    let kind = match (&after, before.is_some()) {
        (After::Absent, _) => ChangeKind::Deleted,
        (_, true) => ChangeKind::Modified,
        (_, false) => ChangeKind::Created,
    };
    Some(Change { path: rel.to_string(), kind, after, before_file })
}

/// Every worktree path under `rel` (a directory there), as deletions.
fn deleted_subtree(repo: &Path, rel: &str, out: &mut Vec<Change>, budget: &mut usize) -> Result<(), String> {
    let mut files = Vec::new();
    walk(&repo.join(rel), rel, false, &mut files, budget)?;
    for f in files {
        let before_file = lower_kind(repo, &f).is_some_and(|t| t.is_file());
        out.push(Change { path: f, kind: ChangeKind::Deleted, after: After::Absent, before_file });
    }
    Ok(())
}

/// The change set of an overlay run: read the upper layer against the worktree (the lower
/// layer). Whiteouts are deletions; an opaque directory deletes what the worktree held under it;
/// redirects and metacopy (which this mount never enables) refuse the whole set.
pub fn overlay_changes(upper: &Path, repo: &Path) -> Result<Vec<Change>, String> {
    overlay_changes_with(upper, repo, &mut Vec::new())
}

/// [`overlay_changes`], also collecting the regular files the run wrote with the worktree's exact
/// bytes (`rewritten`: in the upper layer, so written, yet no change).
fn overlay_changes_with(upper: &Path, repo: &Path, rewritten: &mut Vec<String>) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    let mut budget = MAX_ENTRIES;
    upper_dir(upper, repo, "", &mut out, &mut budget, rewritten)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    Ok(out)
}

fn upper_dir(
    dir: &Path,
    repo: &Path,
    prefix: &str,
    out: &mut Vec<Change>,
    budget: &mut usize,
    rewritten: &mut Vec<String>,
) -> Result<(), String> {
    let mut entries: Vec<_> =
        std::fs::read_dir(dir).map_err(|e| format!("reading the stage: {e}"))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if *budget == 0 {
            return Err(format!("more than {MAX_ENTRIES} changed entries"));
        }
        *budget -= 1;
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let p = e.path();
        let m = std::fs::symlink_metadata(&p).map_err(|e| format!("reading the stage: {e}"))?;
        if ovl_attr(&p, "redirect").is_some() || ovl_attr(&p, "metacopy").is_some() {
            return Err(format!("{} carries overlay metadata this stage never enables", shown(&rel)));
        }
        let t = m.file_type();
        #[cfg(unix)]
        let whiteout = {
            use std::os::unix::fs::{FileTypeExt, MetadataExt};
            (t.is_char_device() && m.rdev() == 0) || (t.is_file() && ovl_attr(&p, "whiteout").is_some())
        };
        #[cfg(not(unix))]
        let whiteout = false;
        let lower = lower_kind(repo, &rel);
        if whiteout {
            match lower {
                Some(lt) if lt.is_dir() => deleted_subtree(repo, &rel, out, budget)?,
                Some(lt) => out.push(Change {
                    path: rel,
                    kind: ChangeKind::Deleted,
                    after: After::Absent,
                    before_file: lt.is_file(),
                }),
                None => {}
            }
            continue;
        }
        if t.is_dir() {
            match lower {
                Some(lt) if !lt.is_dir() => out.push(Change {
                    path: rel.clone(),
                    kind: ChangeKind::Deleted,
                    after: After::Absent,
                    before_file: lt.is_file(),
                }),
                Some(_) if ovl_attr(&p, "opaque").as_deref() == Some(b"y") => {
                    // Everything the worktree held under it is gone, except what the stage holds.
                    let mut staged = Vec::new();
                    let mut b = MAX_ENTRIES;
                    walk(&p, &rel, false, &mut staged, &mut b)?;
                    let staged: BTreeSet<String> = staged.into_iter().collect();
                    let mut gone = Vec::new();
                    deleted_subtree(repo, &rel, &mut gone, budget)?;
                    out.extend(gone.into_iter().filter(|c| !staged.contains(&c.path)));
                }
                _ => {}
            }
            upper_dir(&p, repo, &rel, out, budget, rewritten)?;
            continue;
        }
        if lower.is_some_and(|lt| lt.is_dir()) {
            // A directory replaced by a file: everything under it is gone.
            deleted_subtree(repo, &rel, out, budget)?;
        }
        match compare(repo, &rel, read_after(&p, &m)?) {
            Some(c) => out.push(c),
            None => rewritten.push(rel),
        }
    }
    Ok(())
}

/// The change set of a copied tree (the macOS clone): every difference between `staged` and the
/// worktree, `.git` included.
pub fn tree_changes(staged: &Path, repo: &Path) -> Result<Vec<Change>, String> {
    tree_changes_with(staged, repo, &mut Vec::new())
}

/// [`tree_changes`], also collecting the regular files the run rewrote with the worktree's exact
/// bytes (`rewritten`: the copy's modification time moved, the content did not).
fn tree_changes_with(staged: &Path, repo: &Path, rewritten: &mut Vec<String>) -> Result<Vec<Change>, String> {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let mut budget = MAX_ENTRIES * 4;
    walk(repo, "", false, &mut a, &mut budget)?;
    walk(staged, "", false, &mut b, &mut budget)?;
    let before: BTreeSet<String> = a.into_iter().collect();
    let after: BTreeSet<String> = b.into_iter().collect();
    let mut out = Vec::new();
    for rel in before.union(&after) {
        let after_state = if after.contains(rel) {
            let p = staged.join(rel);
            let m = std::fs::symlink_metadata(&p).map_err(|e| format!("reading the stage: {e}"))?;
            read_after(&p, &m)?
        } else {
            After::Absent
        };
        if !before.contains(rel) || after_state == After::Absent {
            if let Some(c) = compare(repo, rel, after_state) {
                out.push(c);
            }
            continue;
        }
        // Present on both sides. A regular file whose size, modification time and mode are the
        // copy's is unchanged (a command that hides a change this way only keeps it from being
        // applied). Symlinks compare by target, special files by kind.
        let (a, b) = (std::fs::symlink_metadata(repo.join(rel)), std::fs::symlink_metadata(staged.join(rel)));
        if let (Ok(a), Ok(b)) = (&a, &b) {
            let same = a.file_type().is_file()
                && b.file_type().is_file()
                && a.len() == b.len()
                && exec_bit(a) == exec_bit(b)
                && a.modified().ok().is_some_and(|t| b.modified().ok() == Some(t));
            if same {
                continue;
            }
        }
        let lower = lower_kind(repo, rel);
        match (&after_state, lower) {
            (After::File { .. }, _) => match compare(repo, rel, after_state) {
                Some(c) => out.push(c),
                None => rewritten.push(rel.clone()),
            },
            (After::Symlink, Some(t)) if t.is_symlink() => {
                let same = std::fs::read_link(repo.join(rel)).ok() == std::fs::read_link(staged.join(rel)).ok();
                if !same {
                    out.extend(compare(repo, rel, after_state));
                }
            }
            (After::Special, Some(t)) if !t.is_file() && !t.is_symlink() => {}
            _ => out.extend(compare(repo, rel, after_state)),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------------------------

/// The verdict on a change set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Changes to apply to the worktree.
    pub apply: Vec<Change>,
    /// Paths changed in the stage that are dropped (cache paths of a regenerate action, special
    /// files under cache).
    pub discarded: Vec<String>,
    /// Paths whose change refuses the application, with why.
    pub refused: Vec<(String, &'static str)>,
}

/// Judge a change set against the action's declared outputs and cache globs.
pub fn judge(changes: Vec<Change>, action: &ActionSpec, file_limit: u64) -> Plan {
    let mut plan = Plan::default();
    let mut total = 0u64;
    for c in changes {
        let git = c.path.split('/').next() == Some(".git");
        let cache = matches_any(&action.cache, &c.path);
        let output = action.kind == ActionKind::Regenerate && matches_any(&action.outputs, &c.path);
        let applicable = match &c.after {
            After::File { bytes, .. } => {
                total += bytes.len() as u64;
                (bytes.len() as u64) <= file_limit
            }
            After::Absent => c.before_file,
            After::Symlink | After::Special => false,
        };
        if git {
            plan.refused.push((c.path, "under .git"));
        } else if cache && (action.kind == ActionKind::Regenerate || !applicable) {
            // A regenerate action drops cache paths; a verify action keeps only what it can apply.
            plan.discarded.push(c.path);
        } else if cache || output {
            if !applicable {
                plan.refused.push((c.path, "not a regular file (a symlink or special file is never applied)"));
            } else {
                plan.apply.push(c);
            }
        } else if action.kind == ActionKind::Verify {
            plan.refused.push((c.path, "a verify action may not change sources"));
        } else {
            plan.refused.push((c.path, "outside the declared outputs"));
        }
    }
    if total > MAX_APPLY_BYTES {
        plan.refused.push(("*".into(), "the change set is larger than 256 MiB"));
    }
    plan
}

// ---------------------------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------------------------

static TMP: AtomicU64 = AtomicU64::new(0);

/// Make sure every parent directory of `rel` is a real directory inside the repository,
/// creating missing ones. Returns the directories created.
fn parents(repo: &Path, rel: &str) -> Result<Vec<PathBuf>, String> {
    let parts: Vec<&str> = rel.split('/').collect();
    let mut p = repo.to_path_buf();
    let mut made = Vec::new();
    for part in &parts[..parts.len().saturating_sub(1)] {
        p.push(part);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_dir() => {}
            Ok(_) => return Err(format!("{} is not a directory in the worktree", shown(rel))),
            Err(_) => {
                std::fs::create_dir(&p).map_err(|e| format!("creating a directory for {}: {e}", shown(rel)))?;
                made.push(p.clone());
            }
        }
    }
    Ok(made)
}

fn write_temp(dir: &Path, bytes: &[u8], exec: bool) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let tmp = dir.join(format!(".adage-apply-{}-{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(if exec { 0o755 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = exec;
    let mut f = opts.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(tmp)
}

/// Apply a plan to the worktree: every write staged beside its target first, then every path's
/// state compared with `pre`, then each file renamed into place (or removed). A path whose state
/// changed since the run began applies nothing (stage `conflict`). Returns the paths applied.
pub fn apply(repo: &Path, plan: &Plan, pre: &PreState) -> Result<Vec<String>, Refusal> {
    let mut temps: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut made: Vec<PathBuf> = Vec::new();
    let cleanup = |temps: &[(PathBuf, PathBuf)], made: &[PathBuf]| {
        for (t, _) in temps {
            let _ = std::fs::remove_file(t);
        }
        for d in made.iter().rev() {
            let _ = std::fs::remove_dir(d);
        }
    };
    // 1. Stage every write beside its target.
    for c in &plan.apply {
        let target = repo.join(&c.path);
        if let After::File { bytes, exec } = &c.after {
            match parents(repo, &c.path) {
                Ok(m) => made.extend(m),
                Err(e) => {
                    cleanup(&temps, &made);
                    return refuse("conflict", e);
                }
            }
            let dir = target.parent().unwrap_or(repo);
            match write_temp(dir, bytes, *exec) {
                Ok(t) => temps.push((t, target)),
                Err(e) => {
                    cleanup(&temps, &made);
                    return refuse("apply", format!("writing {}: {e}", shown(&c.path)));
                }
            }
        }
    }
    // 2. Optimistic concurrency: nothing changed under the run.
    for c in &plan.apply {
        let now = state_of(repo, &c.path);
        if now != pre.expected(&c.path) {
            cleanup(&temps, &made);
            return refuse(
                "conflict",
                format!("{} changed in the worktree while the action ran; nothing was applied", shown(&c.path)),
            );
        }
    }
    // 3. Into place, one file at a time, each atomically.
    let mut done = Vec::new();
    let mut temps_iter = temps.iter();
    for c in &plan.apply {
        let target = repo.join(&c.path);
        let r = match &c.after {
            After::File { .. } => match temps_iter.next() {
                Some((t, _)) => std::fs::rename(t, &target),
                None => Err(std::io::Error::other("lost a staged file")),
            },
            _ => std::fs::remove_file(&target),
        };
        if let Err(e) = r {
            let rest: Vec<(PathBuf, PathBuf)> = temps_iter.cloned().collect();
            cleanup(&rest, &[]);
            return refuse(
                "apply",
                format!("{} of {} change(s) applied, then {}: {e}", done.len(), plan.apply.len(), shown(&c.path)),
            );
        }
        done.push(c.path.clone());
    }
    // File data was synced before rename; persist directory entries before a successful
    // terminal receipt can claim the apply completed durably.
    #[cfg(unix)]
    for c in &plan.apply {
        for dir in repo.join(&c.path).ancestors().skip(1).take_while(|p| p.starts_with(repo)) {
            if let Err(e) = std::fs::File::open(dir).and_then(|f| f.sync_all()) {
                return refuse("apply", format!("changes applied but directory durability is uncertain: {e}"));
            }
        }
    }
    Ok(done)
}

// ---------------------------------------------------------------------------------------------
// Stage a run
// ---------------------------------------------------------------------------------------------

/// A private staging directory for an overlay's upper and work layers: under a fixed system
/// directory, created exclusively, never granted to the sandbox, removed afterwards.
struct StageDir {
    path: PathBuf,
}

impl StageDir {
    fn new(owner: Option<(u32, u32)>) -> std::io::Result<StageDir> {
        static N: AtomicU64 = AtomicU64::new(0);
        let base = if cfg!(target_os = "linux") { PathBuf::from("/tmp") } else { std::env::temp_dir() };
        let base = base.canonicalize()?;
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let path =
            base.join(format!("adage-stage-{}-{nanos:x}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        #[allow(unused_mut)]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(&path)?;
        let s = StageDir { path };
        for sub in ["upper", "work"] {
            b.create(s.path.join(sub))?;
        }
        #[cfg(unix)]
        if let Some((uid, gid)) = owner {
            for p in [s.path.clone(), s.path.join("upper"), s.path.join("work")] {
                std::os::unix::fs::chown(&p, Some(uid), Some(gid))?;
            }
        }
        #[cfg(not(unix))]
        let _ = owner;
        Ok(s)
    }
}

impl Drop for StageDir {
    fn drop(&mut self) {
        // overlayfs leaves `work/work` with no permissions; open it up so it can be removed.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(self.path.join("work/work"), std::fs::Permissions::from_mode(0o700));
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(target_os = "macos")]
fn clonefile(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn clonefile(src: *const libc::c_char, dst: *const libc::c_char, flags: u32) -> libc::c_int;
    }
    let s = std::ffi::CString::new(src.as_os_str().as_bytes())?;
    let d = std::ffi::CString::new(dst.as_os_str().as_bytes())?;
    // SAFETY: two NUL-terminated paths; CLONE_NOFOLLOW (1) never follows a symlink at `src`.
    if unsafe { clonefile(s.as_ptr(), d.as_ptr(), 1) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn clonefile(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("clonefile is macOS only"))
}

/// Most bytes a copied stage holds.
const COPY_BYTES: u64 = 2 << 30;

/// Copy a worktree (`.git` included): directories, regular files with their mode and
/// modification time, symlinks as symlinks; special files are left out. Bounded.
pub(crate) fn copy_tree(from: &Path, to: &Path, entries: &mut usize, bytes: &mut u64) -> Result<(), String> {
    std::fs::create_dir(to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        crate::project_python::checkpoint()?;
        if *entries == 0 {
            return Err(format!("the worktree has more than {} entries", MAX_ENTRIES * 4));
        }
        *entries -= 1;
        let m = std::fs::symlink_metadata(e.path()).map_err(|e| e.to_string())?;
        let dest = to.join(e.file_name());
        let t = m.file_type();
        if t.is_dir() {
            copy_tree(&e.path(), &dest, entries, bytes)?;
        } else if t.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(e.path()).map_err(|e| e.to_string())?, &dest)
                .map_err(|e| e.to_string())?;
        } else if t.is_file() {
            *bytes = bytes.checked_sub(m.len()).ok_or("the worktree is larger than 2 GiB")?;
            std::fs::copy(e.path(), &dest).map_err(|e| e.to_string())?;
            if let (Ok(f), Ok(t)) = (std::fs::File::options().write(true).open(&dest), m.modified()) {
                let _ = f.set_modified(t);
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn chown_tree(p: &Path, uid: u32, gid: u32) -> std::io::Result<()> {
    std::os::unix::fs::lchown(p, Some(uid), Some(gid))?;
    if std::fs::symlink_metadata(p)?.file_type().is_dir() {
        for e in std::fs::read_dir(p)?.flatten() {
            chown_tree(&e.path(), uid, gid)?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn chown_tree(_: &Path, _: u32, _: u32) -> std::io::Result<()> {
    Ok(())
}

/// A staged run, verified, not yet applied.
#[derive(Debug, Clone)]
pub struct Staged {
    pub repo: PathBuf,
    pub action: ActionSpec,
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    pub duration_ms: u64,
    pub plan: Plan,
    pub pre: PreState,
    pub inputs: BTreeMap<String, String>,
    /// Typed fields projected from stdout, when the request declares some and the run passed.
    pub fields: Option<Value>,
    pub backend: StageBackend,
    /// The first 8 KiB of stderr, for a person (approval previews); never returned to an agent.
    pub stderr: Vec<u8>,
    /// Declared cache files the run wrote with the worktree's exact bytes: refreshed by this run,
    /// yet not in the plan (there is nothing to apply). A cache file in neither the plan nor here
    /// was not written by this run.
    pub rewritten: Vec<String>,
}

/// Run an E2 request in a stage and judge its change set. Nothing touches the worktree. `Err`
/// when a gate refuses, the sandbox or stage is unavailable, or the run cannot be read.
pub fn stage(req: &ExecRequest, ctx: &Context) -> Result<Staged, Refusal> {
    stage_with(req, ctx, backend(ctx.capability))
}

/// [`stage`] with a given staging mechanism (the escape and G4 suites run each one this machine
/// has).
pub fn stage_with(req: &ExecRequest, ctx: &Context, backend: StageBackend) -> Result<Staged, Refusal> {
    let started = std::time::Instant::now();
    let _python_budget = req.project_python.as_ref().map(|_| crate::project_python::budget(req.profile.limits.wall_ms));
    let preparation_check =
        || crate::project_python::checkpoint().map_err(|reason| Refusal { stage: "limits", reason });
    if req.level != Level::E2 {
        return refuse("request", "only an E2 action is staged");
    }
    let checked = run::check(req, ctx).map_err(|(r, _)| r)?;
    let action = req.action.clone().ok_or_else(|| Refusal { stage: "request", reason: "no action".into() })?;
    if !backend.available() {
        return refuse("stage", format!("staging is {}", backend.line()));
    }
    let owner = sandbox::is_root().then_some(sandbox::NOBODY);
    let scratch =
        Scratch::new(owner).map_err(|e| Refusal { stage: "sandbox", reason: format!("scratch directory: {e}") })?;
    let repo = req
        .repo_root
        .canonicalize()
        .map_err(|e| Refusal { stage: "request", reason: format!("repository root: {e}") })?;
    let pre = PreState::take(&repo, &action)?;
    let inputs = input_snapshot(&repo, &action)?;
    preparation_check()?;
    let mut environment = env::build(&ctx.inherited_env, &repo, &scratch.path, &ctx.policy.env);
    let path_dirs: Vec<PathBuf> = environment["PATH"].split(':').map(PathBuf::from).collect();
    let stage_dir =
        StageDir::new(owner).map_err(|e| Refusal { stage: "stage", reason: format!("staging directory: {e}") })?;
    let tree = scratch.path.join("tree");
    let copied = matches!(backend, StageBackend::Clonefile | StageBackend::Copy);
    let run_root = if copied {
        let r = match backend {
            StageBackend::Clonefile => clonefile(&repo, &tree).map_err(|e| e.to_string()),
            _ => copy_tree(&repo, &tree, &mut (MAX_ENTRIES * 4), &mut COPY_BYTES.clone()),
        };
        r.map_err(|e| Refusal { stage: "stage", reason: format!("copying the worktree: {e}") })?;
        if let Some((u, g)) = owner {
            chown_tree(&tree, u, g)
                .map_err(|e| Refusal { stage: "stage", reason: format!("the copy's owner: {e}") })?;
        }
        tree.clone()
    } else {
        repo.clone()
    };
    let mut res =
        Resolved::new(&req.profile, &run_root, &scratch.path, &path_dirs, ctx.cas.as_deref(), ctx.home.as_deref())
            .map_err(|e| Refusal { stage: "sandbox", reason: format!("resolving the profile: {e}") })?;
    let mut cap = ctx.capability.clone();
    match &backend {
        StageBackend::BwrapOverlay { path } => {
            cap.backend = Backend::Bwrap { path: path.clone() };
            res.overlay = Some(Overlay { upper: stage_dir.path.join("upper"), work: stage_dir.path.join("work") });
        }
        StageBackend::UsernsOverlay => {
            res.overlay = Some(Overlay { upper: stage_dir.path.join("upper"), work: stage_dir.path.join("work") });
        }
        _ => {}
    }
    preparation_check()?;
    let mut bound = checked.bound.clone();
    if req.project_python.is_some() {
        let python = crate::project_python::prepare(req, &run_root, &scratch.path, ctx.home.as_deref())
            .map_err(|reason| Refusal { stage: "project-python", reason })?;
        if let Some((u, g)) = owner {
            chown_tree(&scratch.path.join("python-runtime"), u, g)
                .map_err(|e| Refusal { stage: "project-python", reason: format!("the private runtime owner: {e}") })?;
            chown_tree(&scratch.path.join("project-python"), u, g).map_err(|e| Refusal {
                stage: "project-python",
                reason: format!("the private environment owner: {e}"),
            })?;
        }
        bound[0] = python.to_string_lossy().into_owned();
        bound.splice(
            1..1,
            ["-B".into(), "-X".into(), format!("pycache_prefix={}", scratch.path.join("python-bytecode").display())],
        );
        environment.insert("PYTHONDONTWRITEBYTECODE".into(), "1".into());
        environment.insert("PYTHONNOUSERSITE".into(), "1".into());
        environment.remove("PYTHONHOME");
        environment.insert("PATH".into(), format!("{}:{}", python.parent().unwrap().display(), environment["PATH"]));
    }
    let (program, full) = run::resolve_program(req, &bound, &run_root, &scratch.path, &res, &environment, ctx)?;
    preparation_check()?;
    if req.project_python.is_some() {
        res.limits.wall_ms = req.profile.limits.wall_ms.saturating_sub(started.elapsed().as_millis() as u64).max(1);
    }
    let o = run::spawn_sandboxed(&program, &full[1..], &environment, &res, &cap)?;
    preparation_check()?;
    if o.timed_out {
        return refuse("limits", format!("timed out after {} ms", req.profile.limits.wall_ms));
    }
    let mut rewritten = Vec::new();
    let changes = if copied {
        tree_changes_with(&tree, &repo, &mut rewritten)
    } else {
        overlay_changes_with(&stage_dir.path.join("upper"), &repo, &mut rewritten)
    }
    .map_err(|e| Refusal { stage: "verify", reason: e })?;
    rewritten.retain(|p| matches_any(&action.cache, p));
    preparation_check()?;
    let plan = judge(changes, &action, req.profile.limits.file_size);
    let fields = (o.exit == Some(0) && !req.output.fields.is_empty())
        .then(|| project::typed(&o.stdout, &req.output, &repo, req.profile.limits.return_bytes).ok())
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok());
    Ok(Staged {
        repo,
        action,
        exit: o.exit,
        signal: o.signal,
        duration_ms: o.duration_ms,
        plan,
        pre,
        inputs,
        fields,
        backend,
        stderr: o.stderr,
        rewritten,
    })
}

// ---------------------------------------------------------------------------------------------
// The outcome
// ---------------------------------------------------------------------------------------------

/// One changed path, as a result lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub path: String,
    pub change: ChangeKind,
}

/// `applied | refused(reason) | failed(exit)` (Track C §3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum Outcome {
    /// Effects may be present but completion is unconfirmed, including interrupted/partial
    /// apply. Never automatically retry or treat this as a refusal that applied nothing.
    Unknown {
        reason: String,
    },
    Applied {
        changed: Vec<Line>,
        discarded: usize,
    },
    Refused {
        stage: String,
        reason: String,
        /// Whether this refusal counts toward automatic revocation (the action's own change
        /// set; not a conflict with the agent's edits).
        counts: bool,
    },
    Failed {
        exit: Option<i32>,
        signal: Option<i32>,
    },
}

impl Outcome {
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Unknown { .. } => "unknown",
            Outcome::Applied { .. } => "applied",
            Outcome::Refused { .. } => "refused",
            Outcome::Failed { .. } => "failed",
        }
    }
}

fn lines(changes: &[Change]) -> Vec<Line> {
    changes.iter().take(MAX_LISTED).map(|c| Line { path: shown(&c.path), change: c.kind }).collect()
}

/// The outcome of a staged run, applying its plan when it passed.
pub fn finish(s: &Staged) -> Outcome {
    if s.exit != Some(0) {
        return Outcome::Failed { exit: s.exit, signal: s.signal };
    }
    if !s.plan.refused.is_empty() {
        let n = s.plan.refused.len();
        let shown_paths: Vec<String> =
            s.plan.refused.iter().take(5).map(|(p, why)| format!("{} ({why})", shown(p))).collect();
        return Outcome::Refused {
            stage: "verify".into(),
            reason: format!(
                "{n} change(s) the approval does not cover: {}{}; nothing was applied",
                shown_paths.join(", "),
                if n > 5 { ", …" } else { "" }
            ),
            counts: true,
        };
    }
    match input_snapshot(&s.repo, &s.action) {
        Ok(now) if now == s.inputs => {}
        Ok(_) => {
            return Outcome::Refused {
                stage: "inputs".into(),
                reason: "repository inputs changed while the action ran; nothing was applied".into(),
                counts: false,
            }
        }
        Err(e) => {
            return Outcome::Refused {
                stage: "inputs".into(),
                reason: format!("repository inputs cannot be revalidated: {e}; nothing was applied"),
                counts: false,
            }
        }
    }
    match apply(&s.repo, &s.plan, &s.pre) {
        Ok(_) => Outcome::Applied { changed: lines(&s.plan.apply), discarded: s.plan.discarded.len() },
        Err(r) if r.stage == "apply" => Outcome::Unknown { reason: r.reason },
        Err(r) => Outcome::Refused { stage: r.stage.into(), reason: r.reason, counts: false },
    }
}

/// Where outcomes are recorded.
pub fn spool_path(home: &Path) -> PathBuf {
    home.join("exec").join("actions.spool")
}

/// Refusals that count, recorded for this approval (repository, route, approval time).
pub fn counted_refusals(home: &Path, repo: &str, route: &str, approved_on: &str) -> usize {
    let recs: Vec<Value> = adage_store::spool::read_all(&spool_path(home)).unwrap_or_default();
    recs.iter()
        .filter(|r| {
            r["repo"] == repo
                && r["route"] == route
                && r["approved_on"] == approved_on
                && r["outcome"]["outcome"] == "refused"
                && r["outcome"]["counts"] == true
        })
        .count()
}

pub(crate) fn revoke_if_needed(home: &Path, repo: &str, route: &str, approved_on: &str, outcome: &Outcome) -> bool {
    if !matches!(outcome, Outcome::Refused { counts: true, .. }) {
        return false;
    }
    if counted_refusals(home, repo, route, approved_on) < REVOKE_AFTER {
        return false;
    }
    let path = approvals::path(home);
    let Ok(mut store) = Store::load(&path) else { return false };
    let cause = format!("its E2 action was refused {REVOKE_AFTER} times: it changed paths its approval does not cover");
    let revoked = store.revoke_automatically(repo, route, Level::E2, &cause);
    revoked && store.save(&path).is_ok()
}

/// The result an agent gets, as JSON (paths plain, never file contents).
pub fn result_json(route: &str, kind: ActionKind, s: Option<&Staged>, outcome: &Outcome, revoked: bool) -> Value {
    let mut v = json!({ "route": route, "action": kind.as_str() });
    if let Value::Object(m) = serde_json::to_value(outcome).unwrap_or(Value::Null) {
        for (k, x) in m {
            v[k] = x;
        }
    }
    if let Some(s) = s {
        v["exit"] = json!(s.exit);
        if let Some(f) = &s.fields {
            v["fields"] = f.clone();
        }
        if !s.plan.refused.is_empty() {
            v["refused_paths"] = json!(s
                .plan
                .refused
                .iter()
                .take(MAX_LISTED)
                .map(|(p, why)| json!({ "path": shown(p), "why": why }))
                .collect::<Vec<_>>());
        }
    }
    if revoked {
        v["revoked"] = json!(true);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: ActionKind, outputs: &[&str], cache: &[&str]) -> ActionSpec {
        ActionSpec {
            kind,
            outputs: outputs.iter().map(|s| s.to_string()).collect(),
            cache: cache.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn file(path: &str, kind: ChangeKind) -> Change {
        Change { path: path.into(), kind, after: After::File { bytes: b"x".to_vec(), exec: false }, before_file: true }
    }

    #[test]
    fn globs_stay_in_their_segments() {
        let g = vec!["gen/*.py".to_string(), "out/**".to_string()];
        assert!(matches_any(&g, "gen/a.py"));
        assert!(!matches_any(&g, "gen/sub/a.py"));
        assert!(matches_any(&g, "out/a/b/c.txt"));
        assert!(!matches_any(&g, "src/gen/a.py"));
        assert!(!matches_any(&["*".to_string()], ".git"));
    }

    #[test]
    fn regenerate_applies_only_declared_outputs() {
        let a = spec(ActionKind::Regenerate, &["gen/*.py"], &["build/**"]);
        let p =
            judge(vec![file("gen/a.py", ChangeKind::Modified), file("build/x.o", ChangeKind::Created)], &a, 1 << 20);
        assert_eq!(p.apply.len(), 1);
        assert_eq!(p.discarded, vec!["build/x.o"]);
        assert!(p.refused.is_empty());
        for (c, why) in [
            (file("src/main.py", ChangeKind::Modified), "outside the declared outputs"),
            (file(".git/config", ChangeKind::Modified), "under .git"),
            (
                Change {
                    path: "gen/l.py".into(),
                    kind: ChangeKind::Created,
                    after: After::Symlink,
                    before_file: false,
                },
                "not a regular file (a symlink or special file is never applied)",
            ),
            (
                Change {
                    path: "gen/old.py".into(),
                    kind: ChangeKind::Deleted,
                    after: After::Absent,
                    before_file: false,
                },
                "not a regular file (a symlink or special file is never applied)",
            ),
        ] {
            let p = judge(vec![c], &a, 1 << 20);
            assert_eq!(p.refused[0].1, why);
            assert!(p.apply.is_empty());
        }
        let big = Change {
            path: "gen/big.py".into(),
            kind: ChangeKind::Created,
            after: After::File { bytes: vec![0; 64], exec: false },
            before_file: false,
        };
        assert!(!judge(vec![big], &a, 10).refused.is_empty(), "past the file size limit");
    }

    #[test]
    fn verify_keeps_only_cache() {
        let a = spec(ActionKind::Verify, &[], &[".cache/**", "target/**"]);
        let p = judge(vec![file("target/debug/x", ChangeKind::Created)], &a, 1 << 20);
        assert_eq!(p.apply.len(), 1);
        let p = judge(vec![file("src/lib.rs", ChangeKind::Modified)], &a, 1 << 20);
        assert_eq!(p.refused[0].1, "a verify action may not change sources");
    }

    #[test]
    fn pytest_hidden_cache_metadata_needs_its_explicit_contract() {
        let broad = spec(ActionKind::Verify, &[], &[".pytest_cache/**"]);
        broad.validate().unwrap();
        let missing = judge(vec![file(".pytest_cache/.gitignore", ChangeKind::Created)], &broad, 1 << 20);
        assert_eq!(missing.refused.len(), 1);
        let exact = spec(ActionKind::Verify, &[], &[".pytest_cache/**", ".pytest_cache/.gitignore"]);
        exact.validate().unwrap();
        let covered = judge(vec![file(".pytest_cache/.gitignore", ChangeKind::Created)], &exact, 1 << 20);
        assert!(covered.refused.is_empty());
        assert_eq!(covered.apply.len(), 1);
        let source = judge(vec![file(".gitignore", ChangeKind::Modified)], &exact, 1 << 20);
        assert_eq!(source.refused.len(), 1);
    }

    #[test]
    fn apply_is_all_or_nothing_against_the_pre_state() {
        let d = tempfile::tempdir().unwrap();
        let repo = d.path();
        std::fs::create_dir_all(repo.join("gen")).unwrap();
        std::fs::write(repo.join("gen/a.py"), "old").unwrap();
        let a = spec(ActionKind::Regenerate, &["gen/**"], &[]);
        let pre = PreState::take(repo, &a).unwrap();
        let plan = Plan {
            apply: vec![
                Change {
                    path: "gen/a.py".into(),
                    kind: ChangeKind::Modified,
                    after: After::File { bytes: b"new".to_vec(), exec: false },
                    before_file: true,
                },
                Change {
                    path: "gen/sub/b.py".into(),
                    kind: ChangeKind::Created,
                    after: After::File { bytes: b"b".to_vec(), exec: true },
                    before_file: false,
                },
            ],
            ..Plan::default()
        };
        // The agent edits the output meanwhile: nothing is applied.
        std::fs::write(repo.join("gen/a.py"), "agent").unwrap();
        let r = apply(repo, &plan, &pre).unwrap_err();
        assert_eq!(r.stage, "conflict");
        assert_eq!(std::fs::read_to_string(repo.join("gen/a.py")).unwrap(), "agent");
        assert!(!repo.join("gen/sub").exists(), "a directory made for the plan is removed again");
        let leftovers: Vec<_> = std::fs::read_dir(repo.join("gen")).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file is left");
        // Against a fresh pre-state it applies.
        let pre = PreState::take(repo, &a).unwrap();
        assert_eq!(apply(repo, &plan, &pre).unwrap(), vec!["gen/a.py", "gen/sub/b.py"]);
        assert_eq!(std::fs::read_to_string(repo.join("gen/a.py")).unwrap(), "new");
        assert!(matches!(state_of(repo, "gen/sub/b.py"), FileState::File { exec: true, .. }));
    }

    #[cfg(unix)]
    #[test]
    fn apply_never_writes_through_a_symlinked_directory() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let repo = d.path();
        std::os::unix::fs::symlink(outside.path(), repo.join("gen")).unwrap();
        let a = spec(ActionKind::Regenerate, &["gen/**"], &[]);
        let pre = PreState::take(repo, &a).unwrap();
        let plan = Plan {
            apply: vec![Change {
                path: "gen/x.py".into(),
                kind: ChangeKind::Created,
                after: After::File { bytes: b"x".to_vec(), exec: false },
                before_file: false,
            }],
            ..Plan::default()
        };
        assert_eq!(apply(repo, &plan, &pre).unwrap_err().stage, "conflict");
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn copied_trees_diff_by_content_and_kind() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        for d in [a.path(), b.path()] {
            std::fs::create_dir_all(d.join("src")).unwrap();
            std::fs::write(d.join("src/same.py"), "same").unwrap();
            std::fs::write(d.join("src/gone.py"), "gone").unwrap();
        }
        std::fs::remove_file(b.path().join("src/gone.py")).unwrap();
        std::fs::write(b.path().join("src/same.py"), "changed").unwrap();
        std::fs::write(b.path().join("new.txt"), "n").unwrap();
        let mut c = tree_changes(b.path(), a.path()).unwrap();
        c.sort_by(|x, y| x.path.cmp(&y.path));
        let got: Vec<(&str, ChangeKind)> = c.iter().map(|c| (c.path.as_str(), c.kind)).collect();
        assert_eq!(
            got,
            vec![
                ("new.txt", ChangeKind::Created),
                ("src/gone.py", ChangeKind::Deleted),
                ("src/same.py", ChangeKind::Modified)
            ]
        );
    }

    /// A file the run wrote with the worktree's exact bytes is no change, but it was refreshed:
    /// both staging diffs report it apart, so a cache record can be told fresh from untouched.
    #[test]
    fn rewritten_identical_files_are_reported_apart_from_changes() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("cache")).unwrap();
        std::fs::write(repo.path().join("cache/record"), "{}").unwrap();
        std::fs::write(repo.path().join("cache/untouched"), "x").unwrap();
        let copy = tempfile::tempdir().unwrap();
        let tree = copy.path().join("tree");
        copy_tree(repo.path(), &tree, &mut 100, &mut (1 << 20)).unwrap();
        // The run rewrites the record with the same bytes, a moment later.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let f = std::fs::File::options().write(true).open(tree.join("cache/record")).unwrap();
        f.set_modified(later).unwrap();
        let mut rewritten = Vec::new();
        assert!(tree_changes_with(&tree, repo.path(), &mut rewritten).unwrap().is_empty());
        assert_eq!(rewritten, ["cache/record"], "the untouched file is not reported");
        // An overlay's upper layer holds exactly what the run wrote.
        let upper = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(upper.path().join("cache")).unwrap();
        std::fs::write(upper.path().join("cache/record"), "{}").unwrap();
        std::fs::write(upper.path().join("cache/new"), "n").unwrap();
        let mut rewritten = Vec::new();
        let changes = overlay_changes_with(upper.path(), repo.path(), &mut rewritten).unwrap();
        assert_eq!(changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["cache/new"]);
        assert_eq!(rewritten, ["cache/record"]);
    }
}
