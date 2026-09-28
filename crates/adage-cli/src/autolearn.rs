//! Live learning: Adage learns while agents work, with no `adage condition` to remember
//! (Architecture §4.7.2).
//!
//! **Trigger (in the hook).** When a session stops or ends (Claude Code and Codex `Stop` and
//! `SessionEnd`, OpenCode `session.idle` and `session.deleted`), after its normal decision, the
//! hook calls [`on_turn_end`]. If `learn.auto` is on (`adage config`), it records a request for the
//! repository and, unless a learner for it is already running or scheduled, starts one: a detached
//! `adage condition --auto --repo-root <root>` in its own process group (Unix) or a detached process
//! group (Windows), with no standard streams, so the harness never waits for it. At most one learn
//! per repository starts per `learn.min_interval_secs`: a request inside the interval starts a
//! learner that waits for the interval to end (`--after <ms>`), so the last session is never left
//! unlearned. Nothing here changes what the hook prints; a failure is recorded in
//! `fail-open.log` (class `learn`) and forgotten.
//!
//! **Files** (`$ADAGE_HOME/learn-queue/`, `<slug>` = the repository key's track slug):
//!
//! | File | Written by | Holds |
//! | --- | --- | --- |
//! | `<slug>.json` | hook | the latest request: `{repo, root, transcript_path?, harness, requested_at}` |
//! | `<slug>.spawn.json` | hook | when a learner was last started, and when it starts learning: `{at, after}` |
//! | `<slug>.lock` | learner | held (`flock`/`LockFileEx`) while a learner runs or waits: one per repository |
//! | `<slug>.state.json` | learner | its last run: `{pid, started_at, finished_at, outcome, processed}` |
//! | `<slug>.sources.json` | learner | transcripts hooks named outside the configured sources |
//!
//! Times are milliseconds since the Unix epoch. `outcome` is `running`, `ok` or `error`;
//! `processed` is the `requested_at` of the last request the learner handled, so a request is
//! pending while `requested_at > processed`. JSON files are replaced atomically.
//!
//! **Learner** ([`run_auto`]). Takes the repository's lock (or exits: another learner has it),
//! waits until `--after`, then learns that repository only, from Claude Code history
//! (`learn.claude_projects`), `learn.codex_sessions` and `learn.opencode_exports` if set, and any
//! transcript a hook named, through the parse cache. It holds `tracks.lock` while it writes, so it
//! never interleaves with `adage condition`, a review decision or `adage add`. It appends the routes
//! that changed to `learn.log` ([`crate::learnlog`]), then re-checks the queue once: a request
//! that arrived while it ran is learned before it exits.

use crate::learnlog;
// The hook's part (the request, the queue files, starting a learner) lives with the hook, in
// adage-hook, so that `adage-exec hook` triggers learning the same way.
#[allow(unused_imports)]
pub use adage_hook::autolearn::{on_turn_end, read_json, write_json, Queue, Request, RunState, Spawned, Triggered};
#[cfg(debug_assertions)]
#[allow(unused_imports)]
pub use adage_hook::autolearn::{SPAWN_RECORD, TEST_SPAWN_ENV};
use adage_learn::{diff, Inputs, LearnConfig};
use adage_store::config::{self, Config, Thresholds};
use adage_store::{lock, Paths};
use anyhow::{bail, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a new learner waits for its repository's lock before concluding that another learner
/// holds it (a hook's probe holds it for microseconds).
const LOCK_PATIENCE: Duration = Duration::from_secs(2);
/// Transcripts remembered per repository from hooks.
const MAX_SOURCES: usize = 2000;
/// The longest a learner waits for its start time.
const MAX_WAIT_MS: u64 = 86_400_000;

/// How learning binds instincts, from `instincts.*` in `config.toml`: judgements are read from
/// this data directory's spools (auto-extinguish).
pub fn instinct_config(cfg: &Config, paths: &Paths) -> adage_learn::instincts::Config {
    adage_learn::instincts::Config {
        enabled: cfg.instincts_enabled,
        packs: cfg.instincts_packs.clone(),
        disabled: cfg.instincts_disabled.iter().cloned().collect(),
        spools: Some(paths.sessions()),
        roots: Default::default(),
    }
}

pub fn thresholds(t: Thresholds) -> LearnConfig {
    match t {
        Thresholds::Default => LearnConfig::default(),
        Thresholds::Demo => LearnConfig::demo(),
    }
}

/// The transcripts automatic learning reads for a repository: the configured sources, plus every
/// transcript a hook has named that they do not cover (remembered, so that evidence does not come
/// and go between runs).
fn sources(cfg: &Config, q: &Queue, req: Option<&Request>) -> Vec<PathBuf> {
    let mut set: BTreeSet<PathBuf> = BTreeSet::new();
    let claude = cfg
        .learn_claude_projects
        .clone()
        .or_else(|| adage_store::home_dir().map(|h| h.join(".claude").join("projects")));
    if let Some(d) = claude {
        set.extend(adage_learn::import::discover(&d));
    }
    if let Some(d) = &cfg.learn_codex_sessions {
        set.extend(adage_learn::codex::discover(d).0);
    }
    if let Some(d) = &cfg.learn_opencode_exports {
        let mut files: Vec<PathBuf> = std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        set.extend(files);
    }
    let mut remembered: Vec<String> = read_json(&q.sources()).unwrap_or_default();
    if let Some(t) = req.and_then(|r| r.transcript_path.as_deref()) {
        if !set.contains(Path::new(t)) && !remembered.iter().any(|r| r == t) {
            remembered.push(t.to_string());
        }
    }
    let before = remembered.len();
    remembered.retain(|p| Path::new(p).is_file());
    if remembered.len() > MAX_SOURCES {
        remembered.drain(..remembered.len() - MAX_SOURCES);
    }
    if remembered.len() != before || req.is_some_and(|r| r.transcript_path.is_some()) {
        let _ = write_json(&q.sources(), &remembered);
    }
    set.extend(remembered.into_iter().map(PathBuf::from));
    set.into_iter().collect()
}

/// A learn failure for `learn.log`: a class and a fixed message.
struct Failed(&'static str, String);

struct Done {
    report: adage_learn::Report,
    changes: Vec<diff::Change>,
}

fn learn_once(
    paths: &Paths,
    cfg: &Config,
    root: &Path,
    repo: &str,
    q: &Queue,
    req: Option<&Request>,
) -> Result<Done, Failed> {
    if !root.is_dir() {
        return Err(Failed("repo", "the repository's directory no longer exists".into()));
    }
    let transcripts = sources(cfg, q, req);
    let _tracks = lock::exclusive(&paths.tracks_lock())
        .map_err(|e| Failed("lock", format!("taking the tracks lock: {:?}", e.kind())))?;
    let before = diff::snapshot(&paths.tracks(), repo);
    let inputs = Inputs {
        transcripts,
        outcomes: Default::default(),
        repo_override: None,
        models: Some(paths.models()),
        cache: cfg.learn_parse_cache.then(|| paths.cache().join("sessions")),
        only_repo: Some(repo.to_string()),
    };
    let mut lc = thresholds(cfg.learn_thresholds);
    // The hook named this repository's root: instincts bind from its files, even before any of
    // its sessions has steps to learn from (day 0).
    let mut ic = instinct_config(cfg, paths);
    ic.roots.insert(repo.to_string(), root.to_path_buf());
    lc.instincts = Some(ic);
    let report = adage_learn::learn(&inputs, &paths.tracks(), &paths.index(), &lc)
        .map_err(|e| Failed("condition", learnlog::describe(&e)))?;
    let after = diff::snapshot(&paths.tracks(), repo);
    Ok(Done { report, changes: diff::diff(&before, &after) })
}

/// `adage condition --auto --repo-root <root> [--after <ms>]`: learn one repository, quietly.
pub fn run_auto(args: &[String], paths: &Paths) -> Result<()> {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let Some(root) = flag("--repo-root").map(PathBuf::from) else { bail!("--auto needs --repo-root <path>") };
    let after: Option<u64> = flag("--after").and_then(|v| v.parse().ok());
    let repo = adage_core::repo::repo_key_cached(&root, &paths.repo_keys());
    let q = Queue::new(paths, &repo);
    let Some(_lock) = lock::exclusive_within(&q.lock(), LOCK_PATIENCE)? else {
        return Ok(()); // another learner has this repository; it re-checks the queue
    };
    if let Some(at) = after {
        let wait = at.saturating_sub(learnlog::now_ms()).min(MAX_WAIT_MS);
        std::thread::sleep(Duration::from_millis(wait));
        if !paths.root.is_dir() || !config::load(paths).config.learn_auto {
            return Ok(()); // the data directory went, or learning was turned off, while waiting
        }
    }
    let cfg = config::load(paths).config;
    let mut state: RunState = read_json(&q.state()).unwrap_or_default();
    for round in 0..2 {
        let req: Option<Request> = read_json(&q.request());
        let seen = req.as_ref().map(|r| r.requested_at).unwrap_or(0);
        let started = learnlog::now_ms();
        state = RunState {
            pid: std::process::id(),
            started_at: started,
            finished_at: None,
            outcome: "running".into(),
            ..state
        };
        let _ = write_json(&q.state(), &state);
        let t = std::time::Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            learn_once(paths, &cfg, &root, &repo, &q, req.as_ref())
        }))
        .unwrap_or_else(|_| Err(Failed("panic", "learning panicked".into())));
        let line = match &result {
            Ok(done) => learnlog::learn_line(
                started,
                &learnlog::Learned {
                    trigger: "auto",
                    repo: Some(&repo),
                    sessions_read: done.report.sessions_read,
                    duration_ms: t.elapsed().as_millis() as u64,
                    changes: &done.changes,
                    report: &done.report,
                },
            ),
            Err(Failed(class, msg)) => learnlog::error_line(started, "auto", Some(&repo), class, msg),
        };
        let _ = learnlog::append(paths, &line);
        state.finished_at = Some(learnlog::now_ms());
        state.outcome = if result.is_ok() { "ok" } else { "error" }.into();
        state.processed = state.processed.max(seen);
        let _ = write_json(&q.state(), &state);
        let newer = read_json::<Request>(&q.request()).is_some_and(|r| r.requested_at > seen);
        if round == 1 || !newer || result.is_err() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_files_are_named_by_the_repository_slug() {
        let q = Queue::new(&Paths::at("/data"), "github.com/acme/billing");
        assert_eq!(q.slug, "github-com-acme-billing");
        assert!(q.request().ends_with("learn-queue/github-com-acme-billing.json"));
        assert!(q.lock().ends_with("learn-queue/github-com-acme-billing.lock"));
    }

    #[test]
    fn remembered_sources_are_bounded_and_pruned() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let q = Queue::new(&paths, "acme");
        let cfg = Config { learn_claude_projects: Some(d.path().join("none")), ..config::load(&paths).config };
        let t = d.path().join("rollout.jsonl");
        std::fs::write(&t, "{}").unwrap();
        let req = Request { transcript_path: Some(t.to_string_lossy().into()), ..Default::default() };
        assert_eq!(sources(&cfg, &q, Some(&req)), vec![t.clone()]);
        // Remembered without the request, until the file is gone.
        assert_eq!(sources(&cfg, &q, None), vec![t.clone()]);
        std::fs::remove_file(&t).unwrap();
        assert!(sources(&cfg, &q, None).is_empty());
        let kept: Vec<String> = read_json(&q.sources()).unwrap();
        assert!(kept.is_empty());
    }
}
