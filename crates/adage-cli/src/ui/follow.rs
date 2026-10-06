//! `adage watch --follow --json [--session <id>] [--since <cursor>]`: the live pane's feed as
//! NDJSON, for a program (the Claude Code mod's pane and band) rather than a terminal.
//!
//! The same sources as `adage watch` (`Sources`): the session spools, `learn.log`, health as
//! `health::gather` builds it, and the background learner's queue files. One JSON object per line:
//!
//! * `{"kind":"decision", …}`: one per spool record (one hook call), the [`FeedItem`] the terminal
//!   pane draws, as data;
//! * `{"kind":"learn", …}`: one per `learn.log` line, the line itself and its ticker rows;
//! * `{"kind":"health", …}`: the lamps `adage watch` shows, whenever they change;
//! * `{"kind":"learner", …}`: `waiting` (with `next_at`), `running` or `idle`, whenever it changes.
//!
//! **Cursors.** Every line carries `cursor`, an opaque string: the position just after that line
//! in every file the stream reads. `--since <cursor>` starts a new process exactly there, so a
//! restarted follower neither repeats nor skips a row (a per-run sequence number would mean
//! nothing to a new process). Inside a cursor a spool's position is the offset a read started at
//! and how many records of that read were already passed: records are framed, so an offset alone
//! cannot point between two records of one read. Only spools that held a record for the
//! followed repository are named, with the time the cursor was made; a spool it does not name and
//! that changed since then is read from its start, so a session that began while no follower ran
//! is not missed.
//!
//! **The current session never names the arm** (docs/claude-mod/ux-design.md §1.7): for the
//! `--session` id the stream leaves out the `control` flag, every withheld act (`withheld`,
//! `unaided`, `not_reached`, `stop_withheld`, `would_predict`, a withheld pre-tool record) and,
//! in the control arm, every judgement (each one exists only because a directive was withheld);
//! a record left with nothing to say after that is not sent at all, since a treated session would
//! not have written it. Other sessions' rows are sent as `adage watch` shows them.
//!
//! It never draws: no escape sequence is written. It ends, exit 0, when stdin (a pipe) reaches
//! end of file, on SIGTERM, when the parent process dies, or when the reader goes away. With
//! `--stdin ignore` stdin is never read: a parent that closes the child's stdin from the start
//! (the Claude Code mod's `$.process.spawn` with no input) keeps the stream until SIGTERM or its
//! own death.

use super::feed::{self, Act, FeedItem};
use super::health::{self, Health};
use super::jobs::{Loaded, Loader};
use super::learnlog::{self, Tone};
use super::tail::{BACKFILL_AGE, BACKFILL_BYTES, LOG_BACKFILL_BYTES};
use super::theme::Lamp;
use crate::autolearn::{read_json, Queue, Spawned};
use adage_core::time::now_ms;
use adage_store::{lock, spool, Paths};
use adage_tracks::Strength;
use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

/// How often the files are polled.
const POLL: Duration = Duration::from_millis(250);
/// How often the learner's queue files and lock are probed: every other poll.
const LEARNER_POLLS: u32 = 2;
/// A test-only knob: in a debug build, `ADAGE_TEST_FOLLOW_POLL_MS=<1..=250>` polls that often
/// instead, so the integration tests that wait on the follower's lines are not paced by
/// [`POLL`]. A release build never reads it, and a value outside the range is ignored.
const TEST_POLL_ENV: &str = "ADAGE_TEST_FOLLOW_POLL_MS";

/// The poll interval: [`POLL`], or in a debug build [`TEST_POLL_ENV`]'s.
fn poll_every() -> Duration {
    let knob = cfg!(debug_assertions)
        .then(|| std::env::var(TEST_POLL_ENV).ok()?.parse::<u64>().ok())
        .flatten()
        .filter(|ms| (1..=POLL.as_millis() as u64).contains(ms));
    knob.map_or(POLL, Duration::from_millis)
}
/// A spool the cursor does not name, changed this close to (or after) the cursor's time, is read
/// from its start on resume: reading one with no record for this repository costs nothing.
const UNNAMED_SLACK_MS: u64 = 5_000;
/// The cursor format; a cursor of another version is refused rather than misread.
const CURSOR_VERSION: &str = "1";

/// Where the stream stands in every file it reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursor {
    /// When this position was taken (ms since the epoch).
    pub at: u64,
    /// `learn.log`'s byte offset (after the last whole line read).
    pub log: u64,
    /// Per spool (file stem): the offset of the read in progress and how many of its records
    /// have been passed. Only spools that held a record for the followed repository.
    pub spools: BTreeMap<String, (u64, u32)>,
}

impl Cursor {
    /// `1~<at>~<log>~<stem>:<offset>:<passed>,…`. Spool stems are sanitised session ids
    /// (`[A-Za-z0-9_-]`), so the separators never occur in them.
    pub fn encode(&self) -> String {
        let spools: Vec<String> = self.spools.iter().map(|(s, (o, n))| format!("{s}:{o}:{n}")).collect();
        format!("{CURSOR_VERSION}~{}~{}~{}", self.at, self.log, spools.join(","))
    }

    pub fn decode(s: &str) -> Option<Cursor> {
        let mut parts = s.split('~');
        if parts.next()? != CURSOR_VERSION {
            return None;
        }
        let at = parts.next()?.parse().ok()?;
        let log = parts.next()?.parse().ok()?;
        let list = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        let mut spools = BTreeMap::new();
        for e in list.split(',').filter(|e| !e.is_empty()) {
            let mut f = e.split(':');
            let (stem, off, n) = (f.next()?, f.next()?.parse().ok()?, f.next()?.parse().ok()?);
            if f.next().is_some()
                || stem.is_empty()
                || !stem.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
            {
                return None;
            }
            spools.insert(stem.to_string(), (off, n));
        }
        Some(Cursor { at, log, spools })
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

/// One spool as the follower reads it.
#[derive(Debug, Clone, Copy)]
struct SpoolPos {
    /// Where the read in progress starts.
    base: u64,
    /// Records of that read already passed.
    passed: u32,
    /// Whether it has held a record for the followed repository (then the cursor names it).
    named: bool,
}

/// The files and positions; the cursor is derived from them.
struct Tail {
    sessions: PathBuf,
    log_path: PathBuf,
    spools: BTreeMap<String, SpoolPos>,
    log: u64,
    /// Unseen spools start at the backfill window (a fresh follower) or by the resume rule.
    fresh: bool,
    /// The resumed cursor's time, for spools it did not name.
    resumed_at: Option<u64>,
    log_primed: bool,
    repo: Option<String>,
}

impl Tail {
    fn new(paths: &Paths, repo: Option<String>, since: Option<Cursor>) -> Tail {
        let mut t = Tail {
            sessions: paths.sessions(),
            log_path: paths.learn_log(),
            spools: BTreeMap::new(),
            log: 0,
            fresh: since.is_none(),
            resumed_at: None,
            log_primed: false,
            repo,
        };
        if let Some(c) = since {
            t.log = c.log;
            t.log_primed = true;
            t.resumed_at = Some(c.at);
            for (stem, (base, passed)) in c.spools {
                t.spools.insert(stem, SpoolPos { base, passed, named: true });
            }
        }
        t
    }

    fn cursor(&self, at: u64) -> Cursor {
        Cursor {
            at,
            log: self.log,
            spools: self.spools.iter().filter(|(_, p)| p.named).map(|(s, p)| (s.clone(), (p.base, p.passed))).collect(),
        }
    }

    /// Where an unseen spool starts.
    fn start_of(&self, meta: &std::fs::Metadata) -> u64 {
        let len = meta.len();
        let changed = mtime_ms(meta);
        match self.resumed_at {
            // Resumed: a spool the cursor did not name held nothing for this repository then.
            Some(at) if changed + UNNAMED_SLACK_MS >= at => 0,
            Some(_) => len,
            None if self.fresh && now_ms().saturating_sub(changed) <= BACKFILL_AGE.as_millis() as u64 => {
                len.saturating_sub(BACKFILL_BYTES)
            }
            // A spool that appears while following is read whole.
            None if !self.fresh => 0,
            None => len,
        }
    }

    /// New records, merged by time with each spool's own order kept (a cursor counts records of a
    /// spool in its file order), each with the spool's stem.
    fn poll_spools(&mut self) -> Vec<(String, Vec<Value>, u64)> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.sessions) else {
            self.fresh = false;
            return out;
        };
        let mut seen = Vec::new();
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "spool") {
                continue;
            }
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
            let Ok(meta) = e.metadata() else { continue };
            seen.push(stem.clone());
            let pos = match self.spools.get(&stem) {
                Some(p) => *p,
                None => {
                    let p = SpoolPos { base: self.start_of(&meta), passed: 0, named: false };
                    self.spools.insert(stem.clone(), p);
                    p
                }
            };
            let (base, passed) = if meta.len() < pos.base { (0, 0) } else { (pos.base, pos.passed) };
            if base >= meta.len() && passed == 0 {
                continue;
            }
            let Ok((recs, next)) = spool::read_from::<Value>(&path, base) else { continue };
            let p = self.spools.get_mut(&stem).expect("inserted above");
            p.base = base;
            p.passed = passed;
            let fresh: Vec<Value> = recs.into_iter().skip(passed as usize).collect();
            if fresh.is_empty() {
                // Nothing new: the read is done.
                if next > base {
                    p.base = next;
                    p.passed = 0;
                }
                continue;
            }
            out.push((stem, fresh, next));
        }
        // A spool deleted (an old session cleaned up) is forgotten.
        self.spools.retain(|s, _| seen.contains(s));
        self.fresh = false;
        self.resumed_at = None;
        out
    }

    fn poll_log(&mut self) -> Vec<(String, u64)> {
        let Ok(mut f) = std::fs::File::open(&self.log_path) else {
            // Not there (yet), or rotated away: start from the top of whatever appears next.
            self.log_primed = true;
            self.log = 0;
            return vec![];
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let mut skip_first = false;
        if !self.log_primed {
            self.log_primed = true;
            self.log = len.saturating_sub(LOG_BACKFILL_BYTES);
            if self.log > 0 {
                let mut b = [0u8; 1];
                let at_start =
                    f.seek(SeekFrom::Start(self.log - 1)).is_ok() && f.read_exact(&mut b).is_ok() && b[0] == b'\n';
                skip_first = !at_start;
            }
        }
        if len < self.log {
            self.log = 0;
        }
        if len == self.log || f.seek(SeekFrom::Start(self.log)).is_err() {
            return vec![];
        }
        let mut buf = Vec::new();
        if f.take((len - self.log).min(4 << 20)).read_to_end(&mut buf).is_err() {
            return vec![];
        }
        let mut out = Vec::new();
        let mut start = 0usize;
        let mut first = true;
        while let Some(nl) = buf[start..].iter().position(|b| *b == b'\n') {
            let end = start + nl;
            let line = String::from_utf8_lossy(&buf[start..end]).trim_end_matches('\r').to_string();
            let offset = self.log + end as u64 + 1;
            if !(first && skip_first) && !line.trim().is_empty() {
                out.push((line, offset));
            }
            first = false;
            start = end + 1;
        }
        if out.is_empty() {
            // Only blank or skipped lines: past them all the same.
            self.log += start as u64;
        }
        out
    }
}

/// Merge each spool's new records by `ts`, keeping every spool's own order.
fn merge(batches: Vec<(String, Vec<Value>, u64)>) -> Vec<(String, Value)> {
    let mut heads: Vec<(String, std::collections::VecDeque<Value>)> =
        batches.into_iter().map(|(s, v, _)| (s, v.into())).collect();
    let mut out = Vec::new();
    loop {
        let best = heads
            .iter()
            .enumerate()
            .filter_map(|(i, (s, q))| q.front().map(|v| (v["ts"].as_u64().unwrap_or(0), s.clone(), i)))
            .min();
        let Some((_, _, i)) = best else { break };
        let v = heads[i].1.pop_front().expect("non-empty");
        out.push((heads[i].0.clone(), v));
    }
    out
}

fn strength(s: Strength) -> &'static str {
    match s {
        Strength::Fire => "fire",
        Strength::Prime => "prime",
        Strength::Inhibit => "inhibit",
    }
}

fn act_json(a: &Act) -> Value {
    match a {
        Act::Directed { route, strength: s, detail, instinct } => {
            json!({ "act": "directed", "route": route, "strength": strength(*s), "detail": detail, "instinct": instinct })
        }
        Act::Withheld { route, strength: s } => json!({ "act": "withheld", "route": route, "strength": strength(*s) }),
        Act::Followed(r) => json!({ "act": "followed", "route": r }),
        Act::Ignored(r) => json!({ "act": "ignored", "route": r }),
        Act::Misfired(r) => json!({ "act": "misfired", "route": r }),
        Act::Met(r) => json!({ "act": "met", "route": r }),
        Act::Unaided(r) => json!({ "act": "unaided", "route": r }),
        Act::NotReached(r) => json!({ "act": "not_reached", "route": r }),
        Act::StopRequest => json!({ "act": "stop_request" }),
        Act::StopWithheld => json!({ "act": "stop_withheld" }),
        Act::Pretool { rewrite, kind, interpreter, withheld } => json!({
            "act": "pretool", "rewrite": rewrite, "what": kind, "interpreter": interpreter, "withheld": withheld
        }),
        Act::Noticed { candidate, support, s_min, review } => {
            json!({ "act": "noticed", "candidate": candidate, "support": support, "s_min": s_min, "review": review })
        }
        Act::Answered { operation, command, summary, tests, more, ran_at, ago } => json!({
            "act": "answered", "receipt": operation, "command": command, "summary": summary, "tests": tests,
            "more": more, "ran_at": ran_at, "ago": ago
        }),
        Act::Recovery { operation } => json!({
            "act": "recovery", "operation": operation,
            "command": operation.as_ref().map(|id| format!("adage-exec recover --id {id} --acknowledge")),
        }),
        Act::Other(k, r) if k == "chose" => json!({ "act": "predicted", "route": r }),
        Act::Other(k, r) if k == "would choose" => json!({ "act": "would_predict", "route": r }),
        Act::Other(k, r) => json!({ "act": "judged", "what": k, "route": r }),
    }
}

/// Whether an act exists only because something was withheld (the control arm, or past a cap).
fn names_the_arm(a: &Act) -> bool {
    match a {
        Act::Withheld { .. } | Act::Unaided(_) | Act::NotReached(_) | Act::StopWithheld => true,
        Act::Pretool { withheld, .. } => *withheld,
        Act::Other(k, _) => k == "would choose",
        _ => false,
    }
}

/// The record as the current session's stream may show it, or `None` when nothing is left that
/// a treated session would also have written.
pub fn scrub(mut item: FeedItem) -> Option<FeedItem> {
    let before = item.acts.len();
    if item.control {
        // No directive reached the agent, so every judgement is of a withheld one.
        item.acts.retain(|a| match a {
            // A note about learning is recorded alike in both arms.
            // An interrupted action needs a person whichever arm the session is in.
            Act::Directed { .. } | Act::StopRequest | Act::Noticed { .. } | Act::Recovery { .. } => true,
            Act::Other(k, _) => k == "chose",
            _ => false,
        });
    }
    item.acts.retain(|a| !names_the_arm(a));
    let directed: Vec<&str> = item
        .acts
        .iter()
        .filter_map(|a| match a {
            Act::Directed { route, .. } => Some(route.as_str()),
            _ => None,
        })
        .collect();
    let texts = item.texts.iter().filter(|(r, _)| directed.contains(&r.as_str())).cloned().collect();
    item.texts = texts;
    item.control = false;
    if item.acts.len() < before && item.acts.is_empty() {
        return None;
    }
    Some(item)
}

/// A feed item as a stream line (without the cursor).
pub fn decision_json(item: &FeedItem) -> Value {
    let mut m = Map::new();
    m.insert("kind".into(), json!("decision"));
    m.insert("ts".into(), json!(item.ts));
    m.insert("session".into(), json!(item.session));
    m.insert("harness".into(), json!(item.harness));
    m.insert("event".into(), json!(item.event));
    m.insert("repo".into(), json!(item.repo));
    if item.control {
        m.insert("control".into(), json!(true));
    }
    m.insert("acts".into(), Value::Array(item.acts.iter().map(act_json).collect()));
    if !item.tool_use_ids.is_empty() {
        m.insert("tool_use_ids".into(), json!(item.tool_use_ids));
    }
    m.insert("observed".into(), json!(item.observed));
    m.insert("unrouted".into(), json!(item.unrouted));
    m.insert("texts".into(), Value::Array(item.texts.iter().map(|(r, t)| json!({ "route": r, "text": t })).collect()));
    Value::Object(m)
}

fn learn_json(line: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    if !v.is_object() {
        return None;
    }
    let ticks: Vec<Value> = learnlog::parse(line)
        .into_iter()
        .map(|t| {
            json!({
                "text": t.text,
                "short": t.short,
                "tone": match t.tone { Tone::Run => "run", Tone::Change => "change", Tone::Error => "error" },
                "strength": t.strength.map(strength),
                "repo": t.repo,
            })
        })
        .collect();
    Some(json!({ "kind": "learn", "ts": v["ts"].as_u64().unwrap_or(0), "repo": v["repo"], "line": v, "ticks": ticks }))
}

fn lamp(l: Lamp) -> &'static str {
    match l {
        Lamp::Ok => "ok",
        Lamp::Warn => "warn",
        Lamp::Fail => "fail",
        Lamp::Off => "off",
    }
}

/// The health line: the lamps `adage watch` shows, the worst check as the status card words it,
/// and whether Adage is off here.
pub fn health_json(h: &Health, root: Option<&Path>) -> Value {
    use crate::doctor::Status;
    let lights: Vec<Value> =
        h.lights.iter().map(|l| json!({ "label": l.label, "lamp": lamp(l.lamp), "hint": l.hint })).collect();
    let worst = h
        .checks
        .iter()
        .find(|c| c.status == Status::Fail)
        .or_else(|| h.checks.iter().find(|c| c.status == Status::Warn));
    let state = match worst {
        None => json!({ "word": "healthy", "lamp": "fire" }),
        Some(c) if c.status == Status::Fail => json!({ "word": "problem", "lamp": "inhibit", "check": c.label }),
        Some(c) => json!({ "word": "check", "lamp": "prime", "check": c.label }),
    };
    json!({ "kind": "health", "lights": lights, "state": state, "off": crate::switch::is_off(root) })
}

/// The learner for `repo`: `waiting` until `next_at`, `running` while it holds the lock, `idle`.
pub fn learner_json(paths: &Paths, repo: &str, now: u64) -> Value {
    let q = Queue::new(paths, repo);
    let spawned: Option<Spawned> = read_json(&q.spawned());
    let held = lock::is_held(&q.lock()).unwrap_or(false);
    match spawned.filter(|s| s.after > now) {
        Some(s) => json!({ "kind": "learner", "repo": repo, "state": "waiting", "next_at": s.after }),
        None if held => json!({ "kind": "learner", "repo": repo, "state": "running", "next_at": null }),
        None => json!({ "kind": "learner", "repo": repo, "state": "idle", "next_at": null }),
    }
}

/// Ends the stream: stdin closed (only when it is a pipe and `watch_stdin`: a stream started with no
/// stdin at all, or told to ignore it, follows until it is stopped), the parent gone, or SIGTERM.
mod lifeline {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    pub struct Lifeline {
        stdin_closed: Arc<AtomicBool>,
        #[cfg(unix)]
        parent: i32,
    }

    #[cfg(unix)]
    extern "C" fn on_term(_: libc::c_int) {
        // Async-signal-safe: nothing buffered is lost (every line is flushed as it is written).
        unsafe { libc::_exit(0) }
    }

    impl Lifeline {
        pub fn start(watch_stdin: bool) -> Lifeline {
            let stdin_closed = Arc::new(AtomicBool::new(false));
            #[cfg(unix)]
            let parent = unsafe {
                libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
                #[cfg(target_os = "linux")]
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0);
                libc::getppid()
            };
            if watch_stdin && stdin_is_pipe() {
                let flag = stdin_closed.clone();
                std::thread::spawn(move || {
                    use std::io::Read;
                    let mut buf = [0u8; 256];
                    let mut stdin = std::io::stdin();
                    loop {
                        match stdin.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {}
                        }
                    }
                    flag.store(true, Ordering::SeqCst);
                });
            }
            Lifeline {
                stdin_closed,
                #[cfg(unix)]
                parent,
            }
        }

        /// Whether the stream should end.
        pub fn ended(&self) -> bool {
            if self.stdin_closed.load(Ordering::SeqCst) {
                return true;
            }
            // Reparented (to init or a subreaper): the parent died.
            #[cfg(unix)]
            if unsafe { libc::getppid() } != self.parent {
                return true;
            }
            false
        }
    }

    #[cfg(unix)]
    fn stdin_is_pipe() -> bool {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        unsafe { libc::fstat(0, &mut st) == 0 && (st.st_mode & libc::S_IFMT) == libc::S_IFIFO }
    }

    #[cfg(not(unix))]
    fn stdin_is_pipe() -> bool {
        false
    }
}

/// Follow until the stream ends. `session` is the current session's id (its records are
/// scrubbed); `since` resumes from a cursor; `watch_stdin` ends it when a piped stdin closes.
pub fn run(
    paths: &Paths,
    root: Option<PathBuf>,
    session: Option<&str>,
    since: Option<&str>,
    watch_stdin: bool,
) -> Result<()> {
    let since = match since {
        Some(s) => match Cursor::decode(s) {
            Some(c) => Some(c),
            None => bail!("--since takes a cursor from a line of `adage watch --follow --json`"),
        },
        None => None,
    };
    let life = lifeline::Lifeline::start(watch_stdin);
    let index = adage_tracks::Index::load(&paths.index());
    let repo = root.as_deref().map(|r| health::repo_key(paths, r, &index));
    let current = session.map(adage_store::paths::sanitise);
    let mut tail = Tail::new(paths, repo.clone(), since);
    let loader = Loader::spawn(paths.clone(), root.clone());
    let mut out = std::io::stdout().lock();
    let mut last_health: Option<Value> = None;
    let mut last_learner: Option<Value> = None;
    let mut learner_at: Option<Instant> = None;
    let poll = poll_every();
    loop {
        if life.ended() {
            return Ok(());
        }
        let mut lines: Vec<Value> = Vec::new();
        // Health and the learner say where things stand; they move no position.
        while let Ok(m) = loader.rx.try_recv() {
            if let Loaded::Health(h) = m {
                let v = health_json(&h, root.as_deref());
                if last_health.as_ref() != Some(&v) {
                    last_health = Some(v.clone());
                    lines.push(with_cursor(v, &tail.cursor(now_ms())));
                }
            }
        }
        if let Some(r) = &repo {
            if learner_at.is_none_or(|t| t.elapsed() >= poll * LEARNER_POLLS) {
                learner_at = Some(Instant::now());
                let v = learner_json(paths, r, now_ms());
                if last_learner.as_ref() != Some(&v) {
                    last_learner = Some(v.clone());
                    lines.push(with_cursor(v, &tail.cursor(now_ms())));
                }
            }
        }
        let batches = tail.poll_spools();
        let ends: BTreeMap<String, u64> = batches.iter().map(|(s, _, n)| (s.clone(), *n)).collect();
        let merged = merge(batches);
        let mut left: BTreeMap<String, usize> = BTreeMap::new();
        for (s, _) in &merged {
            *left.entry(s.clone()).or_default() += 1;
        }
        let at = now_ms();
        for (stem, rec) in merged {
            let mine = tail.repo.as_deref().is_none_or(|k| rec["repo"].as_str() == Some(k));
            {
                let p = tail.spools.get_mut(&stem).expect("polled");
                p.passed += 1;
                p.named |= mine;
                let n = left.get_mut(&stem).expect("counted");
                *n -= 1;
                if *n == 0 {
                    // The read is done: the next one starts where it ended.
                    p.base = ends[&stem];
                    p.passed = 0;
                }
            }
            if !mine {
                continue;
            }
            let mut item = feed::from_record(&stem, &rec);
            if current.as_deref() == Some(stem.as_str()) {
                match scrub(item) {
                    Some(i) => item = i,
                    None => continue,
                }
            }
            lines.push(with_cursor(decision_json(&item), &tail.cursor(at)));
        }
        for (line, offset) in tail.poll_log() {
            tail.log = offset;
            let Some(v) = learn_json(&line) else { continue };
            let theirs = v["repo"].as_str().filter(|r| !r.is_empty());
            if let (Some(k), Some(r)) = (repo.as_deref(), theirs) {
                if k != r {
                    continue;
                }
            }
            lines.push(with_cursor(v, &tail.cursor(at)));
        }
        for v in lines {
            let mut s = serde_json::to_string(&v)?;
            s.push('\n');
            // The reader went away: nothing is wrong, and no one is listening.
            if out.write_all(s.as_bytes()).and_then(|_| out.flush()).is_err() {
                return Ok(());
            }
        }
        std::thread::sleep(poll);
    }
}

fn with_cursor(mut v: Value, c: &Cursor) -> Value {
    if let Value::Object(m) = &mut v {
        m.insert("cursor".into(), json!(c.encode()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_and_refuse_what_they_did_not_write() {
        let c = Cursor {
            at: 1_790_000_000_000,
            log: 4096,
            spools: [("abc-1".to_string(), (120, 2)), ("s_2".to_string(), (0, 0))].into(),
        };
        let s = c.encode();
        assert_eq!(s, "1~1790000000000~4096~abc-1:120:2,s_2:0:0");
        assert_eq!(Cursor::decode(&s), Some(c));
        assert_eq!(Cursor::decode("1~5~0~"), Some(Cursor { at: 5, log: 0, spools: BTreeMap::new() }));
        for bad in ["", "2~1~0~", "1~x~0~", "1~1~0~a:1", "1~1~0~a/b:1:0", "1~1~0~a:1:0~x"] {
            assert_eq!(Cursor::decode(bad), None, "{bad}");
        }
    }

    #[test]
    fn merging_keeps_each_spools_order() {
        let r = |ts: u64, n: u32| json!({ "ts": ts, "n": n });
        let m = merge(vec![("a".into(), vec![r(5, 1), r(3, 2), r(9, 3)], 0), ("b".into(), vec![r(4, 4), r(6, 5)], 0)]);
        let order: Vec<u64> = m.iter().map(|(_, v)| v["n"].as_u64().unwrap()).collect();
        // `a` keeps 1 before 2 even though 2 is older.
        assert_eq!(order, vec![4, 1, 2, 5, 3]);
    }

    fn rec(arm: &str, decision: Value) -> Value {
        json!({ "ts": 1, "event": "PostToolBatch", "kind": "batch", "arm": arm, "repo": "r", "decision": decision })
    }

    /// T-D2-05's feed half: a noticed candidate is a row of its own in both arms, the same for the
    /// current session whatever its arm (a note about learning names no arm).
    #[test]
    fn a_noticed_candidate_is_shown_alike_in_both_arms() {
        let noticed = json!({ "observed": ["exec make test"],
                              "noticed": [{ "candidate": "run-a54d7c", "support": 2, "s_min": 3, "review": false }] });
        let rows: Vec<Value> = ["treated", "control"]
            .iter()
            .map(|arm| {
                let item = feed::from_record("s", &rec(arm, noticed.clone()));
                assert_eq!(
                    item.acts,
                    vec![Act::Noticed { candidate: "run-a54d7c".into(), support: 2, s_min: 3, review: false }]
                );
                decision_json(&scrub(item).expect("kept in either arm"))
            })
            .collect();
        assert_eq!(rows[0], rows[1]);
        assert_eq!(
            rows[0]["acts"],
            json!([{ "act": "noticed", "candidate": "run-a54d7c", "support": 2, "s_min": 3, "review": false }])
        );
    }

    #[test]
    fn the_current_sessions_rows_never_name_the_arm() {
        let withheld = rec(
            "control",
            json!({ "directives": [], "would_have": [{ "route": "lookup", "strength": "fire", "text": "Run x." }],
                    "judgements": [{ "route": "lookup", "kind": "followed" }, { "route": "gen", "kind": "obligation_met" }],
                    "would_continue": "finish", "would_predict": { "route": "gen" } }),
        );
        let item = feed::from_record("s", &withheld);
        assert!(item.control && item.acts.len() >= 4, "{item:?}");
        assert_eq!(scrub(item.clone()), None, "only withheld acts: no row at all");
        let other = decision_json(&item);
        assert_eq!(other["control"], true, "another session's row is shown as adage watch shows it");

        // A control record with nothing withheld is the same row a treated one would be.
        let quiet = feed::from_record("s", &rec("control", json!({ "observed": ["a", "b"] })));
        let v = decision_json(&scrub(quiet).unwrap());
        assert!(v.get("control").is_none() && v["observed"] == 2, "{v}");

        // Treated: the directive stays, and the would-have duplicate never shows.
        let treated = rec(
            "treated",
            json!({ "directives": [{ "route": "lookup", "strength": "fire", "text": "Run:\n  x\n" }],
                    "would_have": [{ "route": "lookup", "strength": "fire", "text": "Run:\n  x\n" }],
                    "would_predict": { "route": "gen" } }),
        );
        let v = decision_json(&scrub(feed::from_record("s", &treated)).unwrap());
        let s = v.to_string();
        assert!(s.contains("\"directed\"") && !s.contains("would") && !s.contains("control"), "{s}");
        assert_eq!(v["texts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_withheld_pretool_record_is_not_sent_for_the_current_session() {
        let r = json!({ "ts": 1, "event": "PreToolUse", "kind": "pretool", "arm": "control", "repo": "r",
                        "pretool": { "rewrite": "test", "interpreter": ".venv/bin/python" } });
        assert_eq!(scrub(feed::from_record("s", &r)), None);
        let mut t = r.clone();
        t["arm"] = json!("treated");
        let v = decision_json(&scrub(feed::from_record("s", &t)).unwrap());
        assert_eq!(
            v["acts"][0],
            json!({ "act": "pretool", "rewrite": true, "what": "test", "interpreter": ".venv/bin/python", "withheld": false })
        );
    }

    /// The mod's receipt card and its `tool.avoided` mark read the feed: a call answered by a
    /// receipt carries the card's data and the call's id; the same answer withheld in control is
    /// no row; a tool batch's record names its calls' ids in order.
    #[test]
    fn a_receipt_answer_carries_the_card_and_judgements_name_their_calls() {
        let answered = json!({ "ts": 1, "event": "PreToolUse", "kind": "pretool", "arm": "treated", "repo": "r",
            "answered": "receipt", "receipt": true, "command": "python -m pytest tests -q", "tool_use_id": "toolu_1",
            "card": { "operation": "op", "ran_at": "2026-10-03T09:00:00Z", "ago": "4 min ago",
                      "summary": "passed on files identical to the current ones · 1 test this command selects",
                      "tests": ["tests/test_a.py::t"], "more": 0 },
            "decision": { "judgements": [{ "route": "tests-after-edit", "kind": "obligation_met" }] } });
        let v = decision_json(&scrub(feed::from_record("s", &answered)).unwrap());
        assert_eq!(v["tool_use_ids"], json!(["toolu_1"]));
        assert_eq!(
            v["acts"],
            json!([
                { "act": "answered", "receipt": "op", "command": "python -m pytest tests -q",
                  "summary": "passed on files identical to the current ones · 1 test this command selects",
                  "tests": ["tests/test_a.py::t"], "more": 0, "ran_at": "2026-10-03T09:00:00Z", "ago": "4 min ago" },
                { "act": "met", "route": "tests-after-edit" }
            ])
        );
        let mut withheld = answered.clone();
        withheld["arm"] = json!("control");
        withheld["withheld"] = json!(true);
        withheld.as_object_mut().unwrap().remove("decision");
        assert!(feed::from_record("s", &withheld).acts.is_empty(), "the call ran: nothing was answered");

        let mut batch = rec("treated", json!({ "judgements": [{ "route": "lookup", "kind": "followed" }] }));
        batch["tool_use_ids"] = json!(["toolu_2", "toolu_3"]);
        assert_eq!(decision_json(&feed::from_record("s", &batch))["tool_use_ids"], json!(["toolu_2", "toolu_3"]));
        let plain = decision_json(&feed::from_record("s", &rec("treated", json!({}))));
        assert!(plain.get("tool_use_ids").is_none(), "{plain}");
    }

    /// ux-design §9.9: an interrupted action is a row in either arm, naming what to run once the
    /// worktree is inspected.
    #[test]
    fn an_interrupted_action_is_reported_in_either_arm() {
        for arm in ["treated", "control"] {
            let mut r = rec(arm, json!({}));
            r["recovery_required"] = json!(true);
            r["exec"] =
                json!([{ "route": "learned/run-tests", "level": "e2", "outcome": "unknown", "operation_id": "op-7" }]);
            let v = decision_json(&scrub(feed::from_record("s", &r)).expect("kept"));
            assert_eq!(
                v["acts"],
                json!([{ "act": "recovery", "operation": "op-7", "command": "adage-exec recover --id op-7 --acknowledge" }]),
                "{arm}"
            );
        }
        let mut unreadable = rec("treated", json!({}));
        unreadable["recovery_required"] = json!(true);
        unreadable["exec"] = json!([{ "route": "*", "level": "e2", "outcome": "unknown", "withheld": "journal" }]);
        let v = decision_json(&feed::from_record("s", &unreadable));
        assert_eq!(v["acts"], json!([{ "act": "recovery", "operation": null, "command": null }]));
    }
}
