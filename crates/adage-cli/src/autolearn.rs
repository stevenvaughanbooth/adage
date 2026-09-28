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
//! | `<slug>.notice.json` | learner | the one-line notice for the next session start: `{id, text, kind, at, basis, baseline}` |
//! | `<slug>.notice-shown.json` | hook | the last notice shown: `{id, at}` (at most one a day) |
//!
//! Times are milliseconds since the Unix epoch. `outcome` is `running`, `ok` or `error`;
//! `processed` is the `requested_at` of the last request the learner handled, so a request is
//! pending while `requested_at > processed`. JSON files are replaced atomically.
//!
//! **Learner** ([`run_auto`]). Takes the repository's lock (or exits: another learner has it),
//! waits until `--after`, then learns that repository only, from Claude Code history
//! (`learn.claude_projects`), Codex rollouts (`learn.codex_sessions`: by default `auto`, which
//! reads `$CODEX_HOME/sessions` once Codex hooks are registered), `learn.opencode_exports` if set,
//! and any transcript a hook named, through the parse cache. Only this repository's traces are
//! read: they are picked by their recorded working directory before anything is parsed
//! ([`adage_learn::scope`]), so other repositories' files are never opened. `--bootstrap` (a
//! session start in a repository with no reflexes, or a damaged index) first binds instincts
//! and recompiles the index, so the session's first prompt already gets them, then learns.
//! After learning it updates the repository's ledger and savings estimate (`crate::savings`)
//! and composes the next session start's notice. Three failed runs in a row back the next one
//! off (twice the interval per failure) and say so in the notice. It holds `tracks.lock` while it writes, so it
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
/// Transcripts remembered per repository (found in its sources or named by a hook).
const MAX_SOURCES: usize = 5000;
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

/// Where Codex rollouts are read from: the configured directory, or with `auto` (the default)
/// `$CODEX_HOME/sessions` (else `~/.codex/sessions`) once Codex hooks are registered (in user
/// scope or this repository), or a Codex hook asked for this learn.
fn codex_sessions(cfg: &Config, req: Option<&Request>, root: &Path) -> Option<PathBuf> {
    if let Some(d) = &cfg.learn_codex_sessions {
        return Some(d.clone());
    }
    if !cfg.learn_codex_auto {
        return None;
    }
    let registered = req.is_some_and(|r| r.harness == "codex")
        || crate::detect::registrations(Some(root)).iter().any(|r| r.harness == "codex");
    registered.then(|| crate::detect::codex_dir().map(|d| d.join("sessions"))).flatten()
}

/// The transcripts automatic learning reads for a repository: the configured sources, only those
/// recorded inside one of `roots` (the repository's checkouts), plus every transcript a hook has
/// named that they do not cover (remembered, so that evidence does not come and go between runs).
fn sources(cfg: &Config, q: &Queue, req: Option<&Request>, roots: &[PathBuf]) -> Vec<PathBuf> {
    use adage_learn::scope;
    let mut set: BTreeSet<PathBuf> = BTreeSet::new();
    let claude = cfg
        .learn_claude_projects
        .clone()
        .or_else(|| adage_store::home_dir().map(|h| h.join(".claude").join("projects")));
    if let Some(d) = claude {
        set.extend(scope::claude_transcripts(&d, roots));
    }
    if let Some(d) = roots.first().and_then(|root| codex_sessions(cfg, req, root)) {
        set.extend(scope::recorded_inside(adage_learn::codex::discover(&d).0, roots));
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
        set.extend(scope::recorded_inside(files, roots));
    }
    // Additive: every transcript ever found for this repository (in the configured sources or
    // named by a hook) is remembered, and forgotten only when verifiably gone
    // (`adage_learn::scope::gone`) or past the bound, oldest first. A learner started from a
    // sandboxed session that cannot see some of them keeps them, and the parse cache serves them.
    let mut remembered: Vec<String> = read_json(&q.sources()).unwrap_or_default();
    let before = remembered.clone();
    let known: BTreeSet<String> = remembered.iter().cloned().collect();
    let named = req.and_then(|r| r.transcript_path.as_deref()).map(PathBuf::from);
    for p in set.iter().chain(named.iter()) {
        let s = p.to_string_lossy().to_string();
        if !known.contains(&s) && !remembered.contains(&s) {
            remembered.push(s);
        }
    }
    remembered.retain(|p| !adage_learn::scope::gone(Path::new(p)));
    if remembered.len() > MAX_SOURCES {
        remembered.drain(..remembered.len() - MAX_SOURCES);
    }
    if remembered != before {
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

/// The bootstrap's first phase: bind the repository's instincts from its files and compile the
/// index (which also rebuilds a damaged one), in tens of milliseconds, before any trace is read.
fn bootstrap_index(paths: &Paths, cfg: &Config, root: &Path, repo: &str) -> Result<(), Failed> {
    let _tracks = lock::exclusive(&paths.tracks_lock())
        .map_err(|e| Failed("lock", format!("taking the tracks lock: {:?}", e.kind())))?;
    if cfg.instincts_enabled {
        adage_learn::instincts::learn_only(repo, root, &paths.tracks(), &paths.index(), &instinct_config(cfg, paths))
            .map(drop)
            .map_err(|e| Failed("condition", learnlog::describe(&e)))
    } else {
        let all =
            adage_tracks::io::read_all(&paths.tracks()).map_err(|e| Failed("condition", learnlog::describe(&e)))?;
        let (index, _) = adage_tracks::Index::compile_root(&paths.tracks(), &all);
        index.write_atomic(&paths.index()).map_err(|e| Failed("condition", learnlog::describe(&e)))
    }
}

fn learn_once(paths: &Paths, cfg: &Config, root: &Path, repo: &str, transcripts: Vec<PathBuf>) -> Result<Done, Failed> {
    if !root.is_dir() {
        return Err(Failed("repo", "the repository's directory no longer exists".into()));
    }
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

/// `adage condition --auto --repo-root <root> [--after <ms>] [--bootstrap]`: learn one
/// repository, quietly.
pub fn run_auto(args: &[String], paths: &Paths) -> Result<()> {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let Some(root) = flag("--repo-root").map(PathBuf::from) else { bail!("--auto needs --repo-root <path>") };
    let after: Option<u64> = flag("--after").and_then(|v| v.parse().ok());
    let bootstrap = args.iter().any(|a| a == "--bootstrap");
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
    // At a bootstrap, instincts are bound and the index compiled before any trace is read (tens of
    // milliseconds). A bootstrap asked while this learner waited counts too (it held the lock).
    // With no trace of the repository yet this runs every time, so a new Makefile target or
    // lockfile is picked up (learning re-binds instincts itself when there are traces).
    let bootstrap = bootstrap || read_json::<Request>(&q.request()).is_some_and(|r| r.bootstrap);
    let roots = adage_learn::scope::checkouts(&root);
    let untraced = sources(&cfg, &q, None, &roots).is_empty();
    if root.is_dir() && (bootstrap || untraced) {
        if let Err(Failed(class, msg)) = bootstrap_index(paths, &cfg, &root, &repo) {
            let _ =
                learnlog::append(paths, &learnlog::error_line(learnlog::now_ms(), "auto", Some(&repo), class, &msg));
        }
    }
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
        let transcripts = sources(&cfg, &q, req.as_ref(), &roots);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            learn_once(paths, &cfg, &root, &repo, transcripts.clone())
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
        // The ledger and the savings estimate, from the same (scoped) traces; best effort.
        let summary = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let ledger = adage_learn::ledger::Ledger::new(&paths.ledger(), &repo).update(&transcripts);
            crate::savings::refresh(paths, &repo, &ledger)
        }))
        .ok();
        state.finished_at = Some(learnlog::now_ms());
        state.outcome = if result.is_ok() { "ok" } else { "error" }.into();
        state.errors = if result.is_ok() { 0 } else { state.errors.saturating_add(1) };
        state.processed = state.processed.max(seen);
        let _ = write_json(&q.state(), &state);
        let failed = result.as_ref().err().map(|Failed(class, _)| *class);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            notice::update(paths, &repo, &q, summary.as_ref(), state.errors, failed)
        }));
        let newer = read_json::<Request>(&q.request()).is_some_and(|r| r.requested_at > seen);
        if round == 1 || !newer || result.is_err() {
            break;
        }
    }
    Ok(())
}

/// The notice the next session start shows (docs/zero-config.md §3.3): composed after each learn
/// from what is new since the last notice shown, in priority order (a health problem, something
/// waiting for a person, value, new reflexes), in one line.
pub mod notice {
    use super::*;
    use crate::savings::{Summary, Verdict};
    use adage_hook::autolearn::{Notice, Shown};
    use serde::{Deserialize, Serialize};

    /// What a notice is composed from.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Basis {
        /// Reflexes delivered for the repository (the index).
        pub active: BTreeSet<String>,
        /// Reflexes that wait for a person (`adage review`).
        pub nascent: BTreeSet<String>,
        /// The savings verdict (`measuring`, `saved`, `no_difference`, `adds_cost`).
        pub verdict: String,
        /// Failed runs in a row.
        pub errors: u32,
    }

    /// The repository's reflexes now: delivered (the index) and waiting (nascent in its tracks).
    pub fn basis(paths: &Paths, repo: &str, summary: Option<&Summary>, errors: u32) -> Basis {
        let index = adage_tracks::Index::load(&paths.index());
        let active = index.for_repo(repo).map(|r| r.route.id.clone()).collect();
        let dir = paths.tracks().join(adage_learn::slug(repo));
        let nascent = adage_tracks::io::read_all(&dir)
            .unwrap_or_default()
            .iter()
            .flat_map(|t| t.routes.iter())
            .filter(|r| r.state == adage_tracks::RouteState::Nascent)
            .map(|r| r.id.clone())
            .collect();
        let verdict = summary
            .and_then(|s| serde_json::to_value(s.verdict).ok().and_then(|v| v.as_str().map(str::to_string)))
            .unwrap_or_else(|| "measuring".into());
        Basis { active, nascent, verdict, errors }
    }

    fn plural(n: usize, one: &str, many: &str) -> String {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    }

    /// The line for `now` against `was` (what had been shown), or `None` if nothing is new.
    pub fn compose(
        was: &Basis,
        now: &Basis,
        summary: Option<&Summary>,
        failed: Option<&str>,
    ) -> Option<(String, String)> {
        let mut parts: Vec<String> = Vec::new();
        let mut kind = "";
        if now.errors >= 3 {
            parts.push(format!(
                "learning failed {}× here ({}); run adage doctor",
                now.errors,
                failed.unwrap_or("error")
            ));
            kind = "health";
        }
        let waiting = now.nascent.difference(&was.nascent).count();
        if waiting > 0 {
            let n = now.nascent.len();
            parts.push(format!(
                "{} for you: run adage review in a terminal",
                if n == 1 { "1 install step waits".to_string() } else { format!("{n} install steps wait") }
            ));
            if kind.is_empty() {
                kind = "waiting";
            }
        }
        if now.verdict != was.verdict && matches!(now.verdict.as_str(), "saved" | "adds_cost") {
            if let Some(s) = summary {
                let line = crate::savings::lines(s).into_iter().next().unwrap_or_default();
                let line = match s.verdict {
                    Verdict::AddsCost => line.replacen("Adage appears", "it appears", 1),
                    _ => line,
                };
                parts.push(line);
                if kind.is_empty() {
                    kind = "value";
                }
            }
        }
        let new = now.active.difference(&was.active).count();
        if new > 0 {
            parts.push(format!("{} active", plural(new, "new reflex", "new reflexes")));
            if kind.is_empty() {
                kind = "reflexes";
            }
        }
        if parts.is_empty() {
            return None;
        }
        // One line: the two most important things.
        parts.truncate(2);
        Some((format!("adage: {}.", parts.join("; ")), kind.to_string()))
    }

    /// Recompose `<slug>.notice.json` after a learn.
    pub fn update(paths: &Paths, repo: &str, q: &Queue, summary: Option<&Summary>, errors: u32, failed: Option<&str>) {
        let prev: Option<Notice> = read_json(&q.notice());
        let shown: Shown = read_json(&q.shown()).unwrap_or_default();
        let parse = |v: &serde_json::Value| serde_json::from_value::<Basis>(v.clone()).unwrap_or_default();
        // "New" is measured from what the person last saw: the previous notice's basis if it was
        // shown, else the baseline it was itself measured from.
        let baseline = match &prev {
            Some(p) if p.id == shown.id => parse(&p.basis),
            Some(p) => parse(&p.baseline),
            None => Basis::default(),
        };
        let now = basis(paths, repo, summary, errors);
        let Some((text, kind)) = compose(&baseline, &now, summary, failed) else { return };
        if prev.as_ref().is_some_and(|p| p.text == text) {
            return;
        }
        let id = adage_core::repo::short_hash(&format!("{text}\n{}", serde_json::to_string(&now).unwrap_or_default()));
        let n = Notice {
            id,
            text,
            kind,
            at: learnlog::now_ms(),
            basis: serde_json::to_value(&now).unwrap_or_default(),
            baseline: serde_json::to_value(&baseline).unwrap_or_default(),
        };
        let _ = write_json(&q.notice(), &n);
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn b(active: &[&str], nascent: &[&str]) -> Basis {
            Basis {
                active: active.iter().map(|s| s.to_string()).collect(),
                nascent: nascent.iter().map(|s| s.to_string()).collect(),
                verdict: "measuring".into(),
                errors: 0,
            }
        }

        #[test]
        fn only_what_is_new_is_said_in_priority_order() {
            assert_eq!(compose(&b(&["a"], &[]), &b(&["a"], &[]), None, None), None);
            let (t, k) = compose(&b(&["a"], &[]), &b(&["a", "b", "c"], &["n"]), None, None).unwrap();
            assert_eq!(
                t,
                "adage: 1 install step waits for you: run adage review in a terminal; 2 new reflexes active."
            );
            assert_eq!(k, "waiting");
            let mut failing = b(&["a"], &[]);
            failing.errors = 3;
            let (t, k) = compose(&b(&["a"], &[]), &failing, None, Some("lock")).unwrap();
            assert_eq!(
                (t.as_str(), k.as_str()),
                ("adage: learning failed 3× here (lock); run adage doctor.", "health")
            );
        }
    }
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
        let cfg = Config {
            learn_claude_projects: Some(d.path().join("none")),
            learn_codex_auto: false,
            ..config::load(&paths).config
        };
        let t = d.path().join("rollout.jsonl");
        std::fs::write(&t, "{}").unwrap();
        let req = Request { transcript_path: Some(t.to_string_lossy().into()), ..Default::default() };
        let roots = [d.path().join("repo")];
        assert_eq!(sources(&cfg, &q, Some(&req), &roots), vec![t.clone()]);
        // Remembered without the request, until the file is gone.
        assert_eq!(sources(&cfg, &q, None, &roots), vec![t.clone()]);
        std::fs::remove_file(&t).unwrap();
        assert!(sources(&cfg, &q, None, &roots).is_empty());
        let kept: Vec<String> = read_json(&q.sources()).unwrap();
        assert!(kept.is_empty());
    }

    /// A learner that cannot see a transcript (started from a sandboxed session) keeps it: the
    /// list only grows, until a file is verifiably gone.
    #[test]
    fn sources_are_additive_across_confined_learners() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let q = Queue::new(&paths, "acme");
        let projects = d.path().join("projects");
        let repo = d.path().join("repo");
        let cfg = Config {
            learn_claude_projects: Some(projects.clone()),
            learn_codex_auto: false,
            ..config::load(&paths).config
        };
        let slug = adage_learn::scope::claude_slug(&repo);
        let write = |n: usize| {
            let p = projects.join(&slug).join(format!("s{n}.jsonl"));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, format!("{}\n", serde_json::json!({"type":"user","cwd":repo}))).unwrap();
            p
        };
        let all: Vec<PathBuf> = (0..7).map(write).collect();
        let roots = [repo.clone()];
        assert_eq!(sources(&cfg, &q, None, &roots), all);
        // A confined learner: the projects directory is not visible at all (not mounted), and a
        // hook names one transcript elsewhere.
        std::fs::rename(&projects, d.path().join("elsewhere")).unwrap();
        let named = d.path().join("named.jsonl");
        std::fs::write(&named, "{}").unwrap();
        let req = Request { transcript_path: Some(named.to_string_lossy().into()), ..Default::default() };
        let got = sources(&cfg, &q, Some(&req), &roots);
        assert_eq!(got.len(), 8, "nothing forgotten for being out of sight: {got:?}");
        // Visible again, one transcript deleted: only that one is forgotten.
        std::fs::rename(d.path().join("elsewhere"), &projects).unwrap();
        std::fs::remove_file(&all[3]).unwrap();
        let got = sources(&cfg, &q, None, &roots);
        assert_eq!(got.len(), 7);
        assert!(!got.contains(&all[3]) && got.contains(&named));
    }
}
