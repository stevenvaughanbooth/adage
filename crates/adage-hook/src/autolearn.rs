//! The hook's part of live learning (Architecture §4.7.2; the learner itself is `adage learn
//! --auto`, in `adage-cli/src/autolearn.rs`, which documents the queue files).
//!
//! When a session stops or ends, after its normal decision, the hook calls [`on_turn_end`]. If
//! `learn.auto` is on it records a request for the repository and, unless a learner for it is
//! already running or scheduled, starts one: the adage binary (`learner`), detached, in its own
//! process group, with no standard streams. Nothing here changes what the hook prints.

use crate::failopen::Failure;
use adage_store::config;
use adage_store::{lock, Paths};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Debug builds only: `record` writes the learner command a hook would start to
/// `learn-queue/spawn-record.jsonl` instead of starting it; `fail` makes starting it fail.
#[cfg(debug_assertions)]
pub const TEST_SPAWN_ENV: &str = "ADAGE_TEST_NO_SPAWN";
#[cfg(debug_assertions)]
pub const SPAWN_RECORD: &str = "spawn-record.jsonl";

/// A request to learn a repository: the latest session end there.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub repo: String,
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    pub harness: String,
    pub requested_at: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Spawned {
    pub at: u64,
    pub after: u64,
}

/// A learner's last run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunState {
    pub pid: u32,
    pub started_at: u64,
    #[serde(default)]
    pub finished_at: Option<u64>,
    pub outcome: String,
    #[serde(default)]
    pub processed: u64,
}

/// One repository's queue files.
pub struct Queue {
    dir: PathBuf,
    pub slug: String,
}

impl Queue {
    pub fn new(paths: &Paths, repo: &str) -> Queue {
        Queue { dir: paths.learn_queue(), slug: adage_learn::slug(repo) }
    }
    fn file(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!("{}{suffix}", self.slug))
    }
    pub fn request(&self) -> PathBuf {
        self.file(".json")
    }
    pub fn lock(&self) -> PathBuf {
        self.file(".lock")
    }
    pub fn spawned(&self) -> PathBuf {
        self.file(".spawn.json")
    }
    pub fn state(&self) -> PathBuf {
        self.file(".state.json")
    }
    pub fn sources(&self) -> PathBuf {
        self.file(".sources.json")
    }
}

pub fn read_json<T: DeserializeOwned>(p: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

pub fn write_json<T: Serialize>(p: &Path, v: &T) -> std::io::Result<()> {
    config::write_atomic(p, &serde_json::to_vec(v).map_err(std::io::Error::other)?)
}

/// What a hook's trigger did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Triggered {
    /// `learn.auto` is off.
    Off,
    /// Recorded; a learner is running (it re-checks the queue before it exits).
    Running,
    /// Recorded; a learner has already been started to learn at a later time.
    Scheduled,
    /// Recorded, and a learner started: at once (`None`), or waiting until the given time.
    Started(Option<u64>),
}

/// The hook's part: record the request and start a learner if none is running or scheduled.
pub fn on_turn_end(
    paths: &Paths,
    root: &Path,
    harness: &str,
    transcript_path: Option<&str>,
    learner: &Path,
) -> Result<Triggered, Failure> {
    let cfg = config::load(paths).config;
    if !cfg.learn_auto {
        return Ok(Triggered::Off);
    }
    let repo = adage_core::repo::repo_key_cached(root, &paths.repo_keys());
    let q = Queue::new(paths, &repo);
    let now = now_ms();
    let transcript_path = transcript_path
        .filter(|t| t.len() <= 4096 && !t.chars().any(char::is_control) && Path::new(t).is_absolute())
        .map(str::to_string);
    let req = Request {
        repo,
        root: root.to_string_lossy().into_owned(),
        transcript_path,
        harness: harness.to_string(),
        requested_at: now,
    };
    write_json(&q.request(), &req).map_err(|e| Failure::io("learn", "recording a learn request", &e))?;
    if lock::is_held(&q.lock()).map_err(|e| Failure::io("learn", "checking the learner lock", &e))? {
        return Ok(Triggered::Running);
    }
    let last: Spawned = read_json(&q.spawned()).unwrap_or_default();
    if last.after > now {
        return Ok(Triggered::Scheduled);
    }
    let started = read_json::<RunState>(&q.state()).map(|s| s.started_at).unwrap_or(0);
    let earliest = started.max(last.after).saturating_add(cfg.learn_min_interval_secs * 1000);
    let after = (earliest > now).then_some(earliest);
    spawn(paths, root, after, learner)?;
    let _ = write_json(&q.spawned(), &Spawned { at: now, after: after.unwrap_or(now) });
    Ok(Triggered::Started(after))
}

/// Start `<learner> learn --auto --repo-root <root> [--after <ms>]` (the adage binary), detached,
/// and do not wait.
fn spawn(paths: &Paths, root: &Path, after: Option<u64>, learner: &Path) -> Result<(), Failure> {
    let mut args: Vec<String> =
        vec!["learn".into(), "--auto".into(), "--repo-root".into(), root.to_string_lossy().into()];
    if let Some(a) = after {
        args.extend(["--after".to_string(), a.to_string()]);
    }
    #[cfg(debug_assertions)]
    match std::env::var(TEST_SPAWN_ENV).as_deref() {
        Ok("record") => {
            use std::io::Write;
            let line = serde_json::json!({ "ts": now_ms(), "args": args });
            let _ = std::fs::create_dir_all(paths.learn_queue());
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(paths.learn_queue().join(SPAWN_RECORD))
                .map_err(|e| Failure::io("learn", "recording a spawn", &e))?;
            return f
                .write_all(format!("{line}\n").as_bytes())
                .map_err(|e| Failure::io("learn", "recording a spawn", &e));
        }
        Ok("fail") => return Err(Failure::new("learn", "starting the learner: injected failure")),
        _ => {}
    }
    let _ = paths;
    let mut c = std::process::Command::new(learner);
    c.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: a terminal's Ctrl-C or a harness killing the hook's group does
        // not reach it.
        c.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        c.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    c.spawn().map(drop).map_err(|e| Failure::io("learn", "starting the learner", &e))
}
