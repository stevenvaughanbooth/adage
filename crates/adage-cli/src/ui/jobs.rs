//! Work that must not freeze the screen: the health checks (re-run every few seconds), loading
//! the reflexes, and one-off jobs (fetch the intent model, run `adage condition`). Each runs on its own thread
//! and reports over a channel the event loop drains between frames. Threads end when the screen
//! drops its receiver.

use super::health::{self, Health};
use super::routes::{self, Routes};
use adage_store::Paths;
use std::path::{Path, PathBuf};
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

/// What a job produced: lines to read, or what a card shows.
pub enum Outcome {
    Lines(Vec<String>),
    /// The history conditioning would read, for its confirmation card (`history::preview`).
    Sources(Box<crate::history::Preview>),
    /// A conditioning run's result, for its card (`adage condition --json`).
    Conditioned(Box<super::conditioned::Card>),
}

impl Outcome {
    /// The outcome as lines of text, wherever no card is drawn.
    pub fn into_lines(self) -> Vec<String> {
        match self {
            Outcome::Lines(lines) => lines,
            Outcome::Sources(p) => p.lines(),
            Outcome::Conditioned(c) => c.text_lines(),
        }
    }
}

impl From<Vec<String>> for Outcome {
    fn from(lines: Vec<String>) -> Outcome {
        Outcome::Lines(lines)
    }
}

impl From<crate::history::Preview> for Outcome {
    fn from(p: crate::history::Preview) -> Outcome {
        Outcome::Sources(Box::new(p))
    }
}

pub enum JobMsg {
    /// A share done (0..=1) and a line of status.
    Progress(f64, String),
    /// A conditioning run's stage started or advanced (`adage condition --progress`).
    Stage(adage_learn::progress::Event),
    /// The result, or the error.
    Done(Result<Outcome, String>),
}

/// A one-off job with progress.
pub struct Job {
    pub title: String,
    pub rx: Receiver<JobMsg>,
    pub ratio: f64,
    pub status: String,
    pub cancelable: bool,
    /// A conditioning run's stages, as reported so far (`None` for other jobs).
    pub stages: Option<super::conditioned::Stages>,
}

impl Job {
    pub fn spawn<T: Into<Outcome>>(
        title: &str,
        f: impl FnOnce(&Sender<JobMsg>) -> anyhow::Result<T> + Send + 'static,
    ) -> Job {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&tx)));
            let msg = match r {
                Ok(Ok(out)) => Ok(out.into()),
                Ok(Err(e)) => Err(format!("{e:#}")),
                Err(_) => Err("the job panicked".into()),
            };
            let _ = tx.send(JobMsg::Done(msg));
        });
        Job { title: title.to_string(), rx, ratio: 0.0, status: "starting…".into(), cancelable: false, stages: None }
    }

    /// Drain progress; `Some` once the job is done.
    pub fn poll(&mut self) -> Option<Result<Outcome, String>> {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                JobMsg::Progress(r, s) => {
                    self.ratio = r.clamp(0.0, 1.0);
                    self.status = s;
                }
                JobMsg::Stage(e) => {
                    if let Some(st) = &mut self.stages {
                        st.record(e, Instant::now());
                    }
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

/// Condition each repository chosen (`(label, checkout)`), as child processes (their output is
/// captured, never written to the screen): exactly what a hook's background conditioner runs,
/// `adage condition --auto --repo-root <root>`, so each takes the same per-repository lock and
/// logs to `learn.log` the same way; the result is read back from there.
pub fn learn_now(paths: Paths, roots: Vec<(String, PathBuf)>) -> Job {
    Job::spawn("Conditioning on your agents' traces", move |tx| {
        let n = roots.len();
        if n == 1 {
            let _ = tx.send(JobMsg::Progress(0.1, "reading traces…".into()));
            return learn_one(&paths, &roots[0].1);
        }
        // One repository after another; one that fails is said, and the rest still run.
        let (mut lines, mut failed) = (Vec::new(), 0);
        for (i, (label, root)) in roots.iter().enumerate() {
            let _ = tx.send(JobMsg::Progress((i as f64 + 0.1) / n as f64, format!("{} of {n} · {label}", i + 1)));
            match learn_one(&paths, root) {
                Ok(got) => {
                    lines.push(format!("{label}: {}", got.first().cloned().unwrap_or_default()));
                    lines.extend(got.into_iter().skip(1).map(|l| format!("  {l}")));
                }
                Err(e) => {
                    failed += 1;
                    lines.push(format!("{label}: failed: {e:#}"));
                }
            }
        }
        if failed == n {
            anyhow::bail!("{}", lines.join("; "));
        }
        let head = match failed {
            0 => format!("{n} repositories conditioned"),
            f => format!("{} of {n} repositories conditioned · {f} failed", n - f),
        };
        lines.insert(0, head);
        Ok(lines)
    })
}

/// `adage condition --auto --repo-root <root>`, and what it appended to learn.log for the
/// repository, as lines.
fn learn_one(paths: &Paths, root: &Path) -> anyhow::Result<Vec<String>> {
    let repo = adage_core::repo::repo_key_cached(root, &paths.repo_keys());
    // The background conditioner reads only what `learn.repos` names, and so does this.
    if !adage_store::config::load(paths).config.learns(&repo) {
        return Ok(vec![format!(
            "{repo} is not one of the repositories you chose (learn.repos); `adage init` adds it."
        )]);
    }
    let exe = std::env::current_exe()?;
    let started = crate::learnlog::now_ms();
    let out = std::process::Command::new(exe)
        .args(["condition", "--auto", "--repo-root"])
        .arg(root)
        .env("ADAGE_HOME", &paths.root)
        .stdin(std::process::Stdio::null())
        .output()?;
    let err: Vec<String> = String::from_utf8_lossy(&out.stderr).lines().map(String::from).collect();
    if !out.status.success() {
        let msg = err.last().cloned().unwrap_or_else(|| format!("adage condition exited with {}", out.status));
        anyhow::bail!("{}", msg.trim_start_matches("adage: "));
    }
    // What this run appended for the repository.
    let mine: Vec<serde_json::Value> = crate::learnlog::read(paths)
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
        // The run in the words the conditioning card uses (`Read 104 traces · 2 new`), then each
        // change as the ticker says it.
        lines.extend(ticks.into_iter().map(|t| {
            if t.tone == super::learnlog::Tone::Run {
                run_sentence(l)
            } else {
                t.text
            }
        }));
    }
    if none_read {
        // An empty run names what fills it, instead of a bare 0/0.
        lines.push(
            "No traces for this repository yet: reflexes form as your agents work here (adage watch shows them)."
                .into(),
        );
    }
    Ok(lines)
}

/// A `learn.log` run line as one sentence: `Read 104 traces · 2 new, 1 stronger`.
pub(super) fn run_sentence(l: &serde_json::Value) -> String {
    let strength = |c: &serde_json::Value, k: &str| c[k].as_str().and_then(super::theme::parse_strength);
    let changes = l["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| (c["change"].as_str().unwrap_or(""), strength(c, "from"), strength(c, "to")));
    let read = l["sessions_read"].as_u64().unwrap_or(0) as usize;
    format!("Read {} · {}", crate::words::count(read, "trace"), super::conditioned::changed_words(changes))
}
