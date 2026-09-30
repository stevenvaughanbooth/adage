//! Work that must not freeze the screen: the health checks (re-run every few seconds), loading
//! the reflexes, and one-off jobs (fetch the intent model, run `adage condition`). Each runs on its own thread
//! and reports over a channel the event loop drains between frames. Threads end when the screen
//! drops its receiver.

use super::health::{self, Health};
use super::routes::{self, Routes};
use adage_store::Paths;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// How often health is re-checked.
pub const HEALTH_EVERY: Duration = Duration::from_secs(3);
/// How often the reflexes are reloaded while a tab wants them.
pub const ROUTES_EVERY: Duration = Duration::from_secs(10);

pub enum Loaded {
    Health(Box<Health>),
    Routes(Result<Routes, String>),
}

#[derive(Clone, Copy)]
pub enum Want {
    Health,
    Routes,
}

/// The background loaders, one thread each so a slow one never holds up the other: health every
/// [`HEALTH_EVERY`] (and on request), routes on request (and every [`ROUTES_EVERY`] once
/// requested).
pub struct Loader {
    pub rx: Receiver<Loaded>,
    health: Sender<()>,
    routes: Sender<()>,
}

/// Run `load` at once, then every `every` (once `periodic`, or once asked), and whenever asked;
/// stop when either channel closes.
fn worker(asks: Receiver<()>, periodic: bool, every: Duration, mut load: impl FnMut() -> bool + Send + 'static) {
    std::thread::spawn(move || {
        let mut wanted = periodic;
        let mut at: Option<Instant> = None;
        let mut force = periodic;
        loop {
            if force || (wanted && at.is_none_or(|t| t.elapsed() >= every)) {
                if !load() {
                    return;
                }
                at = Some(Instant::now());
                force = false;
            }
            match asks.recv_timeout(Duration::from_millis(250)) {
                Ok(()) => {
                    wanted = true;
                    force = true;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

impl Loader {
    pub fn spawn(paths: Paths, root: Option<PathBuf>) -> Loader {
        let (out, rx) = mpsc::channel();
        let (health, health_asks) = mpsc::channel();
        let (routes, routes_asks) = mpsc::channel();
        {
            let (out, paths, root) = (out.clone(), paths.clone(), root.clone());
            worker(health_asks, true, HEALTH_EVERY, move || {
                out.send(Loaded::Health(Box::new(health::gather(&paths, root.as_deref())))).is_ok()
            });
        }
        worker(routes_asks, false, ROUTES_EVERY, move || {
            out.send(Loaded::Routes(routes::load(&paths, root.as_deref()).map_err(|e| format!("{e:#}")))).is_ok()
        });
        Loader { rx, health, routes }
    }

    pub fn want(&self, w: Want) {
        let _ = match w {
            Want::Health => self.health.send(()),
            Want::Routes => self.routes.send(()),
        };
    }
}

pub enum JobMsg {
    /// A share done (0..=1) and a line of status.
    Progress(f64, String),
    /// The result: lines to show, or the error.
    Done(Result<Vec<String>, String>),
}

/// A one-off job with progress.
pub struct Job {
    pub title: String,
    pub rx: Receiver<JobMsg>,
    pub ratio: f64,
    pub status: String,
}

impl Job {
    pub fn spawn(title: &str, f: impl FnOnce(&Sender<JobMsg>) -> anyhow::Result<Vec<String>> + Send + 'static) -> Job {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&tx)));
            let msg = match r {
                Ok(Ok(lines)) => Ok(lines),
                Ok(Err(e)) => Err(format!("{e:#}")),
                Err(_) => Err("the job panicked".into()),
            };
            let _ = tx.send(JobMsg::Done(msg));
        });
        Job { title: title.to_string(), rx, ratio: 0.0, status: "starting…".into() }
    }

    /// Drain progress; `Some` once the job is done.
    pub fn poll(&mut self) -> Option<Result<Vec<String>, String>> {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                JobMsg::Progress(r, s) => {
                    self.ratio = r.clamp(0.0, 1.0);
                    self.status = s;
                }
                JobMsg::Done(r) => return Some(r),
            }
        }
        None
    }
}

/// Fetch the embedding model, with progress.
pub fn fetch_model(paths: Paths) -> Job {
    Job::spawn("Fetching the intent model", move |tx| {
        let id = crate::models::fetch_with(&paths, &mut |p| {
            let mb = |b: u64| b as f64 / (1 << 20) as f64;
            let _ = tx.send(JobMsg::Progress(
                p.done as f64 / p.total.max(1) as f64,
                format!("{} — {:.1} of {:.1} MB", p.url.rsplit('/').next().unwrap_or(p.url), mb(p.done), mb(p.total)),
            ));
        })?;
        Ok(vec![
            format!("installed {id}"),
            "intent families are conditioned from the next `adage condition` on.".into(),
        ])
    })
}

/// Condition now, as a child process (its output is captured, never written to the screen). In a
/// repository this is exactly what a hook's background conditioner runs, `adage condition --auto
/// --repo-root <root>`, so it takes the same per-repository lock and logs to `learn.log` the same
/// way; the result is read back from there. Outside one it is `adage condition` (every
/// repository).
pub fn learn_now(paths: Paths, root: Option<PathBuf>) -> Job {
    Job::spawn("Conditioning on your agents' traces", move |tx| {
        let exe = std::env::current_exe()?;
        let _ = tx.send(JobMsg::Progress(0.1, "reading traces…".into()));
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("condition").env("ADAGE_HOME", &paths.root).stdin(std::process::Stdio::null());
        if let Some(r) = &root {
            cmd.args(["--auto", "--repo-root"]).arg(r);
        }
        let started = crate::learnlog::now_ms();
        let out = cmd.output()?;
        let err: Vec<String> = String::from_utf8_lossy(&out.stderr).lines().map(String::from).collect();
        if !out.status.success() {
            let msg = err.last().cloned().unwrap_or_else(|| format!("adage condition exited with {}", out.status));
            anyhow::bail!("{}", msg.trim_start_matches("adage: "));
        }
        let Some(root) = root else {
            let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect();
            lines.extend(err);
            return Ok(lines);
        };
        // What this run appended for the repository.
        let repo = adage_core::repo::repo_key_cached(&root, &paths.repo_keys());
        let mine: Vec<serde_json::Value> = crate::learnlog::read(&paths)
            .into_iter()
            .filter(|l| l["ts"].as_u64().is_some_and(|t| t >= started) && l["repo"] == repo.as_str())
            .collect();
        if mine.is_empty() {
            return Ok(vec![format!(
                "A conditioner is already working on {repo}; what it changes appears under conditioning."
            )]);
        }
        let mut lines = Vec::new();
        let mut none_read = false;
        for l in &mine {
            let ticks = super::learnlog::parse(&l.to_string());
            if let Some(e) = ticks.iter().find(|t| t.tone == super::learnlog::Tone::Error) {
                anyhow::bail!("{}", e.text);
            }
            none_read |= ticks.iter().any(|t| t.run.as_ref().is_some_and(|r| r.read == Some(0)));
            lines.extend(ticks.into_iter().map(|t| t.text));
        }
        if none_read {
            // An empty run names what fills it, instead of a bare 0/0.
            lines.push(
                "No traces for this repository yet: reflexes form as your agents work here (adage watch shows them)."
                    .into(),
            );
        }
        Ok(lines)
    })
}
