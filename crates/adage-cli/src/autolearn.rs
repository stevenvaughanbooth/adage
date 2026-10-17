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
//! that arrived while it ran is learned before it exits. Last, with the repository's lock released,
//! it asks whether a newer Adage is out, at most once a day per data directory (`update.check`;
//! `crate::update::latest`): the one network request live learning makes, and a release it finds
//! is put in the notice at once (its lowest part, [`notice::compose`]). Over an index a newer
//! Adage wrote it conditions nothing (that index would be replaced by one that knows less).

use crate::learnlog;
// The hook's part (the request, the queue files, starting a learner) lives with the hook, in
// adage-hook, so that `adage-exec hook` triggers learning the same way.
#[allow(unused_imports)]
pub use adage_hook::autolearn::{on_turn_end, read_json, write_json, Queue, Request, RunState, Spawned, Triggered};
#[cfg(debug_assertions)]
#[allow(unused_imports)]
pub use adage_hook::autolearn::{SPAWN_RECORD, TEST_SPAWN_ENV};
use adage_learn::{diff, Inputs, LearnConfig};
use adage_store::config::{self, Config};
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

/// Where Codex rollouts are read from: the configured directory, or with `auto` (the default)
/// `$CODEX_HOME/sessions` (else `~/.codex/sessions`) once Codex hooks are registered (in user
/// scope or this repository), or a Codex hook asked for this learn.
fn codex_sessions(cfg: &Config, req: Option<&Request>, root: &Path) -> Option<PathBuf> {
    let asked = cfg.learn_codex_auto && cfg.learn_codex_sessions.is_none() && req.is_some_and(|r| r.harness == "codex");
    if asked {
        return crate::detect::codex_dir().map(|d| d.join("sessions"));
    }
    crate::history::codex_sessions(cfg, Some(root))
}

/// The transcripts automatic learning reads for a repository: the configured sources, only those
/// recorded inside one of `roots` (the repository's checkouts), plus every transcript a hook has
/// named that they do not cover (remembered, so that evidence does not come and go between runs).
fn sources(cfg: &Config, q: &Queue, req: Option<&Request>, roots: &[PathBuf]) -> Vec<PathBuf> {
    use adage_learn::scope;
    let mut set: BTreeSet<PathBuf> = BTreeSet::new();
    let claude = crate::history::claude_projects(cfg);
    if let Some(d) = claude {
        set.extend(scope::claude_transcripts(&d, roots));
    }
    if let Some(d) = roots.first().and_then(|root| codex_sessions(cfg, req, root)) {
        set.extend(scope::recorded_inside(adage_learn::codex::discover(&d).0, roots));
    }
    if let Some(d) = &cfg.learn_opencode_exports {
        let files = crate::history::opencode_exports(d);
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
        identities: Default::default(),
        transcripts,
        outcomes: Default::default(),
        repo_override: None,
        models: Some(paths.models()),
        cache: cfg.learn_parse_cache.then(|| paths.cache().join("sessions")),
        only_repo: Some(repo.to_string()),
    };
    let mut lc = LearnConfig::for_thresholds(cfg.learn_thresholds);
    // The hook named this repository's root: instincts bind from its files, even before any of
    // its sessions has steps to learn from (day 0).
    let mut ic = instinct_config(cfg, paths);
    ic.roots.insert(repo.to_string(), root.to_path_buf());
    lc.instincts = Some(ic);
    lc.cards = card_params(cfg, paths);
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
    // A directory that is not a repository (`~`, `/`): nothing to learn, and nothing to record
    // (a request queued before hooks checked for one). A deleted directory is recorded below.
    if root.is_dir() && !root.join(".git").exists() {
        return Ok(());
    }
    let repo = adage_core::repo::repo_key_cached(&root, &paths.repo_keys());
    let q = Queue::new(paths, &repo);
    let Some(held) = lock::exclusive_within(&q.lock(), LOCK_PATIENCE)? else {
        return Ok(()); // another learner has this repository; it re-checks the queue
    };
    // An index a newer Adage wrote (docs/live-learning.md, "two binaries on one machine"): this
    // learner's index would replace it with one that knows less, so it conditions nothing. Its one
    // network request still runs, so this binary learns that a newer one is out.
    if matches!(adage_tracks::Index::try_load(&paths.index()), Err(adage_tracks::index::Unloaded::Newer(_))) {
        drop(held);
        release_check(paths);
        return Ok(());
    }
    // What was in effect before this learner ran: a first notice says only what it adds (a
    // repository set up from a terminal already has its reflexes and instincts).
    let had = notice::basis(paths, &repo, None, 0).active;
    if let Some(at) = after {
        let wait = at.saturating_sub(learnlog::now_ms()).min(MAX_WAIT_MS);
        std::thread::sleep(Duration::from_millis(wait));
        if !paths.root.is_dir() || !config::load(paths).config.learn_auto {
            return Ok(()); // the data directory went, or learning was turned off, while waiting
        }
    }
    let cfg = config::load(paths).config;
    // Taken out of `learn.repos` since this learner was asked for: nothing is read, and the
    // request is dropped rather than left waiting.
    if !cfg.learns(&repo) {
        let _ = std::fs::remove_file(q.request());
        return Ok(());
    }
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
    // What the last round's notice was composed from, for composing it again when the release
    // check below finds a newer Adage.
    let mut last: Option<(Option<crate::savings::Summary>, u32, Option<&'static str>)> = None;
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
                    input_repo: Some(&repo),
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
            // The transcript pass and a harness note's merge never write the ledger at once.
            let held = adage_app::harness_ledger::ledger_lock(paths, &repo);
            let ledger = adage_learn::ledger::Ledger::new(&paths.ledger(), &repo).update(&transcripts);
            drop(held);
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
            notice::update(paths, &repo, &q, summary.as_ref(), state.errors, failed, &had)
        }));
        last = Some((summary, state.errors, failed));
        let newer = read_json::<Request>(&q.request()).is_some_and(|r| r.requested_at > seen);
        if round == 1 || !newer || result.is_err() {
            break;
        }
    }
    // The repository is free for the next learner before the network is asked anything.
    drop(held);
    let known = crate::update::newer(paths).map(|r| r.version);
    release_check(paths);
    // A release the check just found is said at the next session start, not a learn later.
    if let Some((summary, errors, failed)) = last {
        if crate::update::newer(paths).map(|r| r.version) != known {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                notice::update(paths, &repo, &q, summary.as_ref(), errors, failed, &had)
            }));
        }
    }
    Ok(())
}

/// The daily release check (`update.check`, onboarding plan O5): after a learn, at most once a day
/// per data directory, never on a hook. Whatever happens is cached or forgotten: a learn never
/// fails because the network did.
fn release_check(paths: &Paths) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::update::latest::check_if_due(paths)));
}

/// The notice the next session start shows (docs/zero-config.md §3.3): composed after each learn
/// from what is new since the last notice shown, in priority order (a health problem, something
/// waiting for a person, value, new reflexes, and last a newer Adage), in one line.
pub mod notice {
    use super::*;
    use crate::helpers::plural;
    use crate::savings::{Summary, Verdict};
    use adage_hook::autolearn::{Notice, Shown};
    use serde::{Deserialize, Serialize};

    /// How a notice words its commands: as a terminal command (`adage`, or the binary's own path
    /// for a plugin-only user with no `adage` on `PATH`, [`crate::detect::adage_command`]), or, from
    /// the Claude Code plugin's own binary, as the mod's `/adage` verbs (onboarding plan O9: no
    /// notice composed for a plugin binary says `in a terminal`).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Voice {
        Terminal(String),
        Plugin,
    }

    impl Voice {
        /// The voice of the running binary.
        pub fn here() -> Voice {
            let exe = std::env::current_exe().ok();
            if exe.as_deref().is_some_and(adage_store::release::plugin_binary) {
                Voice::Plugin
            } else {
                Voice::Terminal(crate::detect::adage_command())
            }
        }
    }

    /// A composed notice: its line, its kind (the highest part's), and whether the line says a
    /// newer Adage is out (the lowest part, dropped first).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Composed {
        pub text: String,
        pub kind: String,
        pub update: bool,
    }

    /// What a notice is composed from.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Basis {
        /// Reflexes delivered for the repository (the index; not those only `adage-exec` runs,
        /// which are never said to an agent).
        pub active: BTreeSet<String>,
        /// Of `active`, the instincts (Adage's built-in defaults): counted apart from learned
        /// reflexes.
        #[serde(default)]
        pub instincts: BTreeSet<String>,
        /// Install steps that wait for a person (`adage review`): nascent, and not merely quiet.
        pub nascent: BTreeSet<String>,
        /// Quiet reflexes: formed, but their uplift is under the bar, so they wait for approval.
        #[serde(default)]
        pub quiet: BTreeSet<String>,
        /// The savings verdict (`measuring`, `saved`, `no_difference`, `adds_cost`).
        pub verdict: String,
        /// Failed runs in a row.
        pub errors: u32,
        /// The newest Adage this repository's notices have said is out (or one already said in
        /// another repository on this machine): a release is said once per machine.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub latest: Option<String>,
    }

    /// The repository's reflexes now: delivered (the index) and waiting (nascent in its tracks).
    pub fn basis(paths: &Paths, repo: &str, summary: Option<&Summary>, errors: u32) -> Basis {
        let index = adage_tracks::Index::load(&paths.index());
        let said: Vec<_> = index.for_repo(repo).filter(|r| !r.exec_only()).collect();
        let active = said.iter().map(|r| r.route.id.clone()).collect();
        let instincts = said.iter().filter(|r| r.route.is_instinct()).map(|r| r.route.id.clone()).collect();
        let dir = paths.tracks().join(adage_learn::slug(repo));
        let (mut nascent, mut quiet) = (BTreeSet::new(), BTreeSet::new());
        for t in adage_tracks::io::read_all(&dir).unwrap_or_default() {
            for r in t.routes.iter().filter(|r| r.state == adage_tracks::RouteState::Nascent) {
                // A quiet reflex is not an install step: it waits only because agents already do it.
                if crate::evidence::quiet_line(r).is_some() { &mut quiet } else { &mut nascent }.insert(r.id.clone());
            }
        }
        let verdict = summary
            .and_then(|s| serde_json::to_value(s.verdict).ok().and_then(|v| v.as_str().map(str::to_string)))
            .unwrap_or_else(|| "measuring".into());
        Basis { active, instincts, nascent, quiet, verdict, errors, latest: None }
    }

    /// The line for `now` against `was` (what had been shown), or `None` if nothing is new, worded
    /// in `voice`. `update` is `(have, latest)` when a newer Adage is out and was not said yet: the
    /// fifth and lowest part (`kind` `update` only when it is the one part), dropped first when two
    /// higher parts fill the line (onboarding plan O6).
    pub fn compose(
        was: &Basis,
        now: &Basis,
        summary: Option<&Summary>,
        failed: Option<&str>,
        voice: &Voice,
        update: Option<(&str, &str)>,
    ) -> Option<Composed> {
        let (review, doctor) = match voice {
            Voice::Terminal(adage) => (format!("{adage} review"), format!("run {adage} doctor")),
            Voice::Plugin => ("/adage review".to_string(), "/adage health".to_string()),
        };
        let mut parts: Vec<String> = Vec::new();
        let mut kind = "";
        if now.errors >= 3 {
            parts.push(format!("conditioning failed {}× here ({}); {doctor}", now.errors, failed.unwrap_or("error")));
            kind = "health";
        }
        let waiting = now.nascent.difference(&was.nascent).count();
        if waiting > 0 {
            let n = now.nascent.len();
            let steps = if n == 1 { "1 install step waits".to_string() } else { format!("{n} install steps wait") };
            parts.push(match voice {
                Voice::Terminal(_) => format!("{steps} for you: run {review} in a terminal"),
                Voice::Plugin => format!("{steps} for you: {review}"),
            });
            if kind.is_empty() {
                kind = "waiting";
            }
        }
        if now.quiet.difference(&was.quiet).count() > 0 {
            let n = now.quiet.len();
            parts.push(format!(
                "{} (agents already do {}); approve with {review} to deliver {}",
                plural(n, "quiet reflex waits", "quiet reflexes wait"),
                if n == 1 { "it" } else { "them" },
                if n == 1 { "it" } else { "them" }
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
                let line = line.replace("See adage review.", &format!("See {review}."));
                parts.push(line);
                if kind.is_empty() {
                    kind = "value";
                }
            }
        }
        // New learned reflexes and new instincts, counted apart: an instinct is a built-in default
        // bound from the repository's files, not something learned from its traces.
        let new: Vec<&String> = now.active.difference(&was.active).collect();
        let instincts = new.iter().filter(|id| now.instincts.contains(**id)).count();
        let learned = new.len() - instincts;
        let said = match (learned, instincts) {
            (0, 0) => None,
            (l, 0) => Some(plural(l, "new reflex", "new reflexes")),
            (0, i) => Some(plural(i, "instinct", "instincts")),
            (l, i) => {
                Some(format!("{} and {}", plural(l, "new reflex", "new reflexes"), plural(i, "instinct", "instincts")))
            }
        };
        if let Some(s) = said {
            parts.push(format!("{s} active"));
            if kind.is_empty() {
                kind = "reflexes";
            }
        }
        // A newer Adage: information, said last (never above a problem or something waiting).
        if let Some((have, latest)) = update {
            parts.push(match voice {
                Voice::Terminal(adage) => format!("{latest} is out (you have {have}); run {adage} update"),
                Voice::Plugin => format!("{latest} is out (you have {have}); /adage update brings it"),
            });
            if kind.is_empty() {
                kind = "update";
            }
        }
        if parts.is_empty() {
            return None;
        }
        // One line: the two most important things.
        let said_update = update.is_some() && parts.len() <= 2;
        parts.truncate(2);
        Some(Composed { text: format!("adage: {}.", parts.join("; ")), kind: kind.to_string(), update: said_update })
    }

    /// Does a notice in another repository on this machine say, or did it say, that `latest` is
    /// out (its basis: shown, or waiting for that repository's next start; or the baseline it was
    /// measured from)? A release is said by one repository's notice, not by every one's.
    pub fn said_elsewhere(paths: &Paths, q: &Queue, latest: &str) -> bool {
        let own = q.notice();
        let Ok(entries) = std::fs::read_dir(paths.learn_queue()) else { return false };
        entries.flatten().any(|e| {
            let path = e.path();
            let notice = e.file_name().to_string_lossy().ends_with(".notice.json") && path != own;
            notice
                && read_json::<Notice>(&path).is_some_and(|n| {
                    [&n.basis, &n.baseline].iter().any(|b| b.get("latest").and_then(|v| v.as_str()) == Some(latest))
                })
        })
    }

    /// Recompose `<slug>.notice.json` after a learn. `had`: the reflexes in effect before this
    /// learner ran, the baseline of a first notice.
    pub fn update(
        paths: &Paths,
        repo: &str,
        q: &Queue,
        summary: Option<&Summary>,
        errors: u32,
        failed: Option<&str>,
        had: &BTreeSet<String>,
    ) {
        let prev: Option<Notice> = read_json(&q.notice());
        let shown: Shown = read_json(&q.shown()).unwrap_or_default();
        let parse = |v: &serde_json::Value| serde_json::from_value::<Basis>(v.clone()).unwrap_or_default();
        // "New" is measured from what the person last saw: the previous notice's basis if it was
        // shown, else the baseline it was itself measured from; with no notice yet, what was in
        // effect before this learner ran (a repository set up from a terminal is not new).
        let baseline = match &prev {
            Some(p) if p.id == shown.id => parse(&p.basis),
            Some(p) => parse(&p.baseline),
            None => Basis { active: had.clone(), ..Basis::default() },
        };
        let mut now = basis(paths, repo, summary, errors);
        // A newer Adage (the release check's cache: no request), unless this repository's person,
        // or another repository's on this machine, was told already.
        let newer = crate::update::newer(paths).map(|r| r.version);
        let due = newer.as_deref().filter(|v| baseline.latest.as_deref() != Some(*v) && !said_elsewhere(paths, q, v));
        let update = due.map(|v| (adage_store::release::VERSION, v));
        let Some(Composed { text, kind, update: said }) =
            compose(&baseline, &now, summary, failed, &Voice::here(), update)
        else {
            return;
        };
        // What this repository's person will have been told once it is shown: a release this line
        // says, or one said before (here or elsewhere); not one two higher parts crowded out of
        // the line, which a later notice says.
        let crowded = update.is_some() && !said;
        now.latest = if crowded || newer.is_none() { baseline.latest.clone() } else { newer };
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
                instincts: BTreeSet::new(),
                nascent: nascent.iter().map(|s| s.to_string()).collect(),
                quiet: BTreeSet::new(),
                verdict: "measuring".into(),
                errors: 0,
                latest: None,
            }
        }

        fn adage() -> Voice {
            Voice::Terminal("adage".into())
        }

        /// `compose` with no newer release: its line and kind.
        fn said(was: &Basis, now: &Basis, failed: Option<&str>, voice: &Voice) -> Option<(String, String)> {
            compose(was, now, None, failed, voice, None).map(|c| (c.text, c.kind))
        }

        #[test]
        fn a_quiet_reflex_is_not_counted_as_an_install_step() {
            let was = b(&[], &[]);
            let mut now = b(&[], &[]);
            now.quiet.insert("lookup".into());
            let (t, kind) = said(&was, &now, None, &adage()).unwrap();
            assert_eq!(
                t,
                "adage: 1 quiet reflex waits (agents already do it); approve with adage review to deliver it."
            );
            assert_eq!(kind, "waiting");
            assert!(!t.contains("install step"));
        }

        /// QA first-run #8: instincts are not counted as new learned reflexes, and what was in
        /// effect before (set up from a terminal) is not new.
        #[test]
        fn instincts_are_counted_apart_and_what_was_there_is_not_new() {
            let mut now = b(&["i1", "i2", "r1"], &[]);
            now.instincts = ["i1", "i2"].iter().map(|s| s.to_string()).collect();
            let (t, _) = said(&b(&[], &[]), &now, None, &adage()).unwrap();
            assert_eq!(t, "adage: 1 new reflex and 2 instincts active.");
            assert_eq!(said(&b(&["i1", "i2", "r1"], &[]), &now, None, &adage()), None);
            let (t, _) = said(&b(&["r1"], &[]), &now, None, &adage()).unwrap();
            assert_eq!(t, "adage: 2 instincts active.");
        }

        #[test]
        fn only_what_is_new_is_said_in_priority_order() {
            assert_eq!(said(&b(&["a"], &[]), &b(&["a"], &[]), None, &adage()), None);
            let (t, k) = said(&b(&["a"], &[]), &b(&["a", "b", "c"], &["n"]), None, &adage()).unwrap();
            assert_eq!(
                t,
                "adage: 1 install step waits for you: run adage review in a terminal; 2 new reflexes active."
            );
            assert_eq!(k, "waiting");
            let mut failing = b(&["a"], &[]);
            failing.errors = 3;
            // No `adage` on PATH and not the plugin's binary: the notice names the binary that ran.
            let path = Voice::Terminal("~/tools/adage".into());
            let (t, _) = said(&b(&["a"], &[]), &b(&["a"], &["n"]), None, &path).unwrap();
            assert_eq!(t, "adage: 1 install step waits for you: run ~/tools/adage review in a terminal.");
            let (t, k) = said(&b(&["a"], &[]), &failing, Some("lock"), &adage()).unwrap();
            assert_eq!(
                (t.as_str(), k.as_str()),
                ("adage: conditioning failed 3× here (lock); run adage doctor.", "health")
            );
        }

        /// T-O9-10: the plugin's own binary words every part as the mod's `/adage` verbs, and
        /// never says `in a terminal`; a terminal install keeps its commands.
        #[test]
        fn a_plugin_binary_says_the_slash_verbs() {
            let mut now = b(&["a"], &["n"]);
            now.quiet.insert("q".into());
            let (t, _) = said(&b(&["a"], &[]), &now, None, &Voice::Plugin).unwrap();
            assert_eq!(
                t,
                "adage: 1 install step waits for you: /adage review; 1 quiet reflex waits (agents already do it); approve with /adage review to deliver it."
            );
            let mut failing = b(&["a"], &[]);
            failing.errors = 3;
            let (t, _) = said(&b(&["a"], &[]), &failing, Some("lock"), &Voice::Plugin).unwrap();
            assert_eq!(t, "adage: conditioning failed 3× here (lock); /adage health.");
            let c = compose(&b(&[], &[]), &b(&[], &[]), None, None, &Voice::Plugin, Some(("0.1.0", "0.2.0"))).unwrap();
            assert_eq!(c.text, "adage: 0.2.0 is out (you have 0.1.0); /adage update brings it.");
            for t in [&t, &c.text] {
                assert!(!t.contains("in a terminal"), "{t}");
            }
            let (t, _) = said(&b(&["a"], &[]), &b(&["a"], &["n"]), None, &adage()).unwrap();
            assert_eq!(t, "adage: 1 install step waits for you: run adage review in a terminal.");
        }

        /// T-O6-04: a newer Adage is the fifth and lowest part: last, worded per channel, its own
        /// kind only alone, and dropped first when two higher parts fill the line.
        #[test]
        fn a_newer_release_is_said_last_and_dropped_first() {
            let none = b(&["a"], &[]);
            let pair = Some(("0.1.0", "0.2.0"));
            let c = compose(&none, &none, None, None, &adage(), pair).unwrap();
            assert_eq!(
                (c.text.as_str(), c.kind.as_str(), c.update),
                ("adage: 0.2.0 is out (you have 0.1.0); run adage update.", "update", true)
            );
            let c = compose(&none, &none, None, None, &Voice::Plugin, pair).unwrap();
            assert_eq!(c.text, "adage: 0.2.0 is out (you have 0.1.0); /adage update brings it.");
            // After one higher part: last, and the kind is the higher part's.
            let c = compose(&none, &b(&["a", "b"], &[]), None, None, &adage(), pair).unwrap();
            assert_eq!(
                (c.text.as_str(), c.kind.as_str(), c.update),
                ("adage: 1 new reflex active; 0.2.0 is out (you have 0.1.0); run adage update.", "reflexes", true)
            );
            // Two higher parts: the line is full and the release waits for a later notice.
            let c = compose(&none, &b(&["a", "b"], &["n"]), None, None, &adage(), pair).unwrap();
            assert_eq!(
                (c.text.as_str(), c.kind.as_str(), c.update),
                (
                    "adage: 1 install step waits for you: run adage review in a terminal; 1 new reflex active.",
                    "waiting",
                    false
                )
            );
            // Nothing newer: nothing to say.
            assert_eq!(compose(&none, &none, None, None, &adage(), None), None);
        }

        /// T-O6-04's other half: a release is said by one repository's notice on this machine,
        /// once; a repository whose line was full says it at its next notice.
        #[test]
        fn a_release_is_said_once_per_machine() {
            let d = tempfile::tempdir().unwrap();
            let paths = Paths::at(d.path());
            std::fs::create_dir_all(paths.learn_queue()).unwrap();
            let latest = adage_store::release::Latest {
                checked_at: learnlog::now_ms(),
                have: adage_store::release::VERSION.into(),
                latest: Some(adage_store::release::Release { version: "99.0.0".into(), ..Default::default() }),
                failure: None,
            };
            latest.write(&paths).unwrap();
            let (qa, qb) = (Queue::new(&paths, "acme"), Queue::new(&paths, "billing"));
            let none = BTreeSet::new();
            update(&paths, "acme", &qa, None, 0, None, &none);
            let a: Notice = read_json(&qa.notice()).unwrap();
            assert!(a.text.contains("99.0.0 is out (you have "), "{}", a.text);
            assert_eq!((a.kind.as_str(), a.basis["latest"].as_str()), ("update", Some("99.0.0")));
            // Another repository's learner: acme's notice says it, so billing's has nothing new.
            update(&paths, "billing", &qb, None, 0, None, &none);
            assert!(read_json::<Notice>(&qb.notice()).is_none());
            // acme's notice shown, then learned again: nothing new to say there either.
            write_json(&qa.shown(), &Shown { id: a.id.clone(), at: learnlog::now_ms() }).unwrap();
            update(&paths, "acme", &qa, None, 0, None, &none);
            assert_eq!(read_json::<Notice>(&qa.notice()).unwrap().id, a.id);
        }
    }
}

/// Procedure cards for families of similar tasks (`learn.cards`, `learn.card_threshold`), with the
/// verdicts `adage train cards` recorded; `None` when turned off.
pub fn card_params(cfg: &adage_store::config::Config, paths: &Paths) -> Option<adage_learn::cards::CardParams> {
    cfg.learn_cards.then(|| adage_learn::cards::CardParams {
        threshold: cfg.learn_card_threshold as f32,
        verdicts: adage_learn::cards::load_verdicts(&paths.root),
        ..Default::default()
    })
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
