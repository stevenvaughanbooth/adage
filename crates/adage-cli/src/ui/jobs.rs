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
    /// The worker observes a dropped receiver and stops safely. Mutating jobs stay attached
    /// until completion so leaving a progress dialog cannot interrupt a file update.
    pub cancelable: bool,
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
        Job { title: title.to_string(), rx, ratio: 0.0, status: "starting…".into(), cancelable: false }
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

/// Read and count sessions before offering Learn. This is intentionally a background job: a
/// large history must not block keyboard navigation while sources are inspected.
pub fn preview_learning(paths: Paths, root: PathBuf) -> Job {
    let mut job = Job::spawn("Review learning sources", move |_tx| learning_preview(&paths, &root));
    job.cancelable = true;
    job
}

pub fn learning_preview(paths: &Paths, root: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let repo = health::repo_key(paths, root, &adage_tracks::Index::load(&paths.index()));
    let cfg = adage_store::config::load(paths).config;
    let mut lines = crate::detect::history_summary(&cfg);
    let table = adage_core::CommandTable::load();
    let files = crate::autolearn::history_for_repo(paths, Some(&repo));
    let configured: std::collections::BTreeSet<PathBuf> =
        crate::detect::history_sources(&cfg).into_iter().flat_map(|s| s.files).collect();
    let additional = files.iter().filter(|p| !configured.contains(*p)).count();
    if additional > 0 {
        lines.push(format!(
            "{additional} additional transcript file(s) provided by hooks in this repository are included."
        ));
    }
    let mut sessions = 0;
    let mut eligible = 0;
    let mut guided = 0;
    for file in &files {
        if let Some(session) = adage_learn::read_session(file, &table, None).filter(|s| s.repo == repo) {
            sessions += 1;
            guided += usize::from(session.steered);
            if !session.steps.is_empty() {
                eligible += 1;
            }
        }
    }
    lines.push(String::new());
    lines.push(format!("Repository: {repo}"));
    lines.push(format!("{sessions} readable session(s) here; {eligible} session(s) with actions available for analysis; {guided} previously guided."));
    if eligible == 0 {
        anyhow::bail!("{}\n\nNothing to learn for this repository yet. Finish an agent session with useful actions, or use Configure history / Import OpenCode history, then try Survey session history. Evidence influenced by Adage is excluded by the corresponding reflex miner.", lines.join("\n"));
    }
    lines.push("Evidence influenced by Adage is excluded by the corresponding reflex miner. Learning may find no repeated pattern strong enough to become a reflex.".into());
    Ok(lines)
}

/// Learn now, as a child process (its output is captured, never written to the screen). In a
/// repository this is exactly what a hook's background learner runs, `adage learn --auto
/// --repo-root <root>`, so it takes the same per-repository lock and logs to `learn.log` the same
/// way; the result is read back from there. Outside one it is `adage learn` (every repository).
pub fn learn_now(paths: Paths, root: Option<PathBuf>) -> Job {
    Job::spawn("Learning from your agents' sessions", move |tx| {
        let exe = std::env::current_exe()?;
        if let Some(root) = &root {
            // Recheck after confirmation, since history can move or change while the dialog is open.
            let _ = learning_preview(&paths, root)?;
        }
        let _ = tx.send(JobMsg::Progress(0.1, "reading transcripts…".into()));
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("learn").env("ADAGE_HOME", &paths.root).stdin(std::process::Stdio::null());
        if let Some(r) = &root {
            cmd.args(["--auto", "--repo-root"]).arg(r);
        }
        let started = crate::learnlog::now_ms();
        let out = cmd.output()?;
        let err: Vec<String> = String::from_utf8_lossy(&out.stderr).lines().map(String::from).collect();
        if !out.status.success() {
            let msg = err.last().cloned().unwrap_or_else(|| format!("adage learn exited with {}", out.status));
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
                "A learner is already working on {repo}; what it changes appears in the learning ticker."
            )]);
        }
        let mut lines = Vec::new();
        for l in &mine {
            let ticks = super::learnlog::parse(&l.to_string());
            if let Some(e) = ticks.iter().find(|t| t.tone == super::learnlog::Tone::Error) {
                anyhow::bail!("{}", e.text);
            }
            lines.extend(ticks.into_iter().map(|t| super::view::learning_label(&t.text)));
        }
        Ok(lines)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_preview_counts_only_this_repository_and_refuses_empty_history() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let paths = Paths::at(temp.path().join("data"));
        let history = temp.path().join("history");
        std::fs::create_dir_all(&history).unwrap();
        for key in ["learn.claude_projects", "learn.codex_sessions", "learn.opencode_exports"] {
            let folder = if key == "learn.codex_sessions" { history.join("sessions") } else { history.clone() };
            adage_store::config::set(&paths, key, &folder.to_string_lossy()).unwrap();
        }
        let err = learning_preview(&paths, &root).unwrap_err().to_string();
        assert!(err.contains("Nothing to learn for this repository"));
        let scenario = adage_sim::scenario::history(adage_sim::CODEX_SEED);
        for (i, session) in scenario.iter().take(2).enumerate() {
            let cwd = if i == 0 { root.clone() } else { temp.path().join("other") };
            std::fs::create_dir_all(cwd.join(".git")).unwrap();
            let p = history.join(adage_sim::codex::rel_path(session));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, adage_sim::codex::rollout(session, &cwd.to_string_lossy())).unwrap();
        }
        let lines = learning_preview(&paths, &root).unwrap();
        assert!(lines.iter().any(|l| l.contains("1 readable session(s) here; 1 session(s) with actions")), "{lines:?}");
        assert!(!paths.tracks().exists(), "preview must not learn or modify reflexes");
    }
}
