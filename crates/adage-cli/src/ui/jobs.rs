//! Work that must not freeze the screen: the health checks (re-run every few seconds), loading
//! the routes, and one-off jobs (fetch the model, run `adage learn`). Each runs on its own thread
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
/// How often the routes are reloaded while their tab wants them.
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
            let (out, paths) = (out.clone(), paths.clone());
            worker(health_asks, true, HEALTH_EVERY, move || {
                out.send(Loaded::Health(Box::new(health::gather(&paths, root.as_deref())))).is_ok()
            });
        }
        worker(routes_asks, false, ROUTES_EVERY, move || {
            out.send(Loaded::Routes(routes::load(&paths).map_err(|e| format!("{e:#}")))).is_ok()
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
    Job::spawn("Fetching the embedding model", move |tx| {
        let id = crate::models::fetch_with(&paths, &mut |p| {
            let mb = |b: u64| b as f64 / (1 << 20) as f64;
            let _ = tx.send(JobMsg::Progress(
                p.done as f64 / p.total.max(1) as f64,
                format!("{} — {:.1} of {:.1} MB", p.url.rsplit('/').next().unwrap_or(p.url), mb(p.done), mb(p.total)),
            ));
        })?;
        Ok(vec![format!("installed {id}"), "intent families are learned from the next `adage learn` on.".into()])
    })
}

/// Run `adage learn` as a child process (its output is captured, never written to the screen).
pub fn learn_now(paths: Paths, extra: Vec<String>) -> Job {
    Job::spawn("Learning from your agents' sessions", move |tx| {
        let exe = std::env::current_exe()?;
        let _ = tx.send(JobMsg::Progress(0.1, "reading transcripts…".into()));
        let out = std::process::Command::new(exe)
            .arg("learn")
            .args(&extra)
            .env("ADAGE_HOME", &paths.root)
            .stdin(std::process::Stdio::null())
            .output()?;
        let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect();
        let err: Vec<String> = String::from_utf8_lossy(&out.stderr).lines().map(String::from).collect();
        if !out.status.success() {
            let msg = err.last().cloned().unwrap_or_else(|| format!("adage learn exited with {}", out.status));
            anyhow::bail!("{}", msg.trim_start_matches("adage: "));
        }
        lines.extend(err);
        Ok(lines)
    })
}
