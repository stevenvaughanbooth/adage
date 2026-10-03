//! What the Claude Code mod reads and runs, through the binary (docs/claude-mod/implementation-plan.md
//! A3–A6): the live feed `adage watch --follow --json`, `adage ledger note` and the savings it
//! feeds, the status card as JSON, and `adage off` / `adage on`. Test ids are the test plan's
//! (docs/claude-mod/test-plan.md).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

mod common;

struct World {
    w: common::TempWorld,
}

impl World {
    fn new() -> World {
        World { w: common::TempWorld::new(&["home", "xdg", "codex", "data", "work/.git", "outside"]) }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.w.p(rel)
    }
    fn repo(&self) -> PathBuf {
        self.p("work")
    }
    fn paths(&self) -> adage_store::Paths {
        adage_store::Paths::at(self.p("data"))
    }
    fn cmd(&self, args: &[&str], cwd: &Path) -> Command {
        let mut c = common::adage(&self.p("data"), cwd);
        c.args(args)
            .env("ADAGE_ARM", "treated")
            .env("HOME", self.p("home"))
            .env("XDG_CONFIG_HOME", self.p("xdg"))
            .env("CODEX_HOME", self.p("codex"))
            // No agent on PATH: the card says the same on every machine.
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C.UTF-8")
            .env("NO_COLOR", "1")
            .env("ADAGE_ASCII", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        c
    }
    fn run(&self, args: &[&str], cwd: &Path, stdin: &str, env: &[(&str, &str)], unset: &[&str]) -> Output {
        let mut c = self.cmd(args, cwd);
        for (k, v) in env {
            c.env(k, v);
        }
        for k in unset {
            c.env_remove(k);
        }
        let mut child = c.spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        child.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str], cwd: &Path) -> String {
        let o = self.run(args, cwd, "", &[], &[]);
        assert!(o.status.success(), "adage {args:?}: {}", text(&o));
        String::from_utf8_lossy(&o.stdout).to_string()
    }
    /// The repository key hooks use for `work`.
    fn key(&self) -> String {
        let v: Value = serde_json::from_str(&self.ok(&["savings", "--json"], &self.repo())).unwrap();
        v["repo"].as_str().unwrap().to_string()
    }
    fn spool(&self, session: &str, rec: Value) {
        common::LearnedHome::at(self.p("data")).spool(session, &rec);
    }
    fn learn_line(&self, v: Value) {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(self.paths().learn_log()).unwrap();
        writeln!(f, "{v}").unwrap();
    }
}

use common::text;

fn record(ts: u64, kind: &str, arm: &str, repo: &str, decision: Value) -> Value {
    let event = match kind {
        "session_start" => "SessionStart",
        "prompt" => "UserPromptSubmit",
        "stop" => "Stop",
        "turn_step" => "TurnStep",
        "pretool" => "PreToolUse",
        _ => "PostToolBatch",
    };
    json!({ "ts": ts, "event": event, "kind": kind, "arm": arm, "repo": repo, "generation": "g", "harness": "claude",
            "decision": decision })
}

/// A running follower: its lines as they come.
struct Feed {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    all: Vec<String>,
}

impl Feed {
    fn start(w: &World, extra: &[&str]) -> Feed {
        let mut args = vec!["watch", "--follow", "--json"];
        args.extend_from_slice(extra);
        // Polled every 20 ms, not every 250 (`ui/follow.rs`, a debug-build test knob).
        let mut child = w.cmd(&args, &w.repo()).env("ADAGE_TEST_FOLLOW_POLL_MS", "20").spawn().unwrap();
        let stdin = child.stdin.take();
        let out = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines() {
                let Ok(l) = l else { break };
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        Feed { child, stdin, lines: rx, all: Vec::new() }
    }

    /// The next line matching `want`, within `within`.
    fn until(&mut self, within: Duration, want: impl Fn(&Value) -> bool) -> Option<Value> {
        let end = Instant::now() + within;
        loop {
            let left = end.checked_duration_since(Instant::now())?;
            let l = self.lines.recv_timeout(left).ok()?;
            self.all.push(l.clone());
            let v: Value = serde_json::from_str(&l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}"));
            assert!(v["kind"].is_string() && v["cursor"].as_str().is_some_and(|c| !c.is_empty()), "{l}");
            if want(&v) {
                return Some(v);
            }
        }
    }

    fn decision(&mut self, marker: u64) -> Value {
        self.until(Duration::from_secs(5), |v| v["kind"] == "decision" && v["ts"] == marker)
            .unwrap_or_else(|| panic!("no decision row {marker}; got:\n{}", self.all.join("\n")))
    }

    /// Close stdin: the follower ends, exit 0.
    fn close(mut self) -> String {
        drop(self.stdin.take());
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                assert!(s.success(), "exit {s:?}");
                break;
            }
            assert!(Instant::now() < end, "the follower did not end on EOF");
            std::thread::sleep(Duration::from_millis(50));
        }
        while let Ok(l) = self.lines.recv_timeout(Duration::from_millis(200)) {
            self.all.push(l);
        }
        self.all.join("\n")
    }

    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// T-A3-01: a spool record and a `learn.log` line arrive within 2 s, each with a cursor; EOF on
/// stdin ends the follower with exit 0.
#[test]
fn the_feed_streams() {
    let w = World::new();
    let key = w.key();
    let base = 1_790_000_000_000u64;
    w.spool("S", record(base, "session_start", "treated", &key, json!({})));
    let mut feed = Feed::start(&w, &["--session", "S"]);
    // Recent history first.
    feed.decision(base);
    w.spool(
        "S",
        record(
            base + 1,
            "prompt",
            "treated",
            &key,
            json!({ "directives": [{ "route": "lookup-work-item", "strength": "fire", "text": "Run:\n  scripts/wi-lookup W-1\n" }],
                    "judgements": [], "observed": [] }),
        ),
    );
    // Within 2 s (the test plan's bound, as for the learn line below): the wait is the bound.
    let d = feed
        .until(Duration::from_secs(2), |v| v["kind"] == "decision" && v["ts"] == base + 1)
        .unwrap_or_else(|| panic!("no decision row within 2 s; got:\n{}", feed.all.join("\n")));
    assert_eq!(
        d["acts"][0],
        json!({ "act": "directed", "route": "lookup-work-item", "strength": "fire", "detail": "scripts/wi-lookup W-1", "instinct": false })
    );
    assert_eq!(
        (d["session"].as_str(), d["event"].as_str(), d["repo"].as_str()),
        (Some("S"), Some("prompt"), Some(key.as_str()))
    );
    w.learn_line(json!({ "ts": base + 2, "kind": "condition", "trigger": "auto", "repo": key, "sessions_read": 3, "sessions_used": 3,
                         "duration_ms": 40, "changes": [{ "route": "lookup-work-item", "track": "t/learned", "change": "strength", "from": "prime", "to": "fire" }] }));
    let l = feed.until(Duration::from_secs(2), |v| v["kind"] == "learn").expect("a learn line within 2 s");
    assert_eq!(l["line"]["ts"], base + 2);
    assert!(l["ticks"].as_array().is_some_and(|t| !t.is_empty()), "{l}");
    let all = feed.close();
    assert!(!all.contains("\u{1b}["), "never an escape sequence:\n{all}");
}

/// The Claude Code mod spawns the follower with its stdin closed from the start (`$.process.spawn`
/// with no input): with `--stdin ignore` the closed pipe does not end it, it streams a record
/// written after the close, and SIGTERM still ends it with exit 0. Without the flag it ends at
/// once, as A3 built.
#[test]
fn a_follower_told_to_ignore_stdin_keeps_streaming_with_its_pipe_closed() {
    let w = World::new();
    let key = w.key();
    let base = 1_790_000_100_000u64;
    w.spool("S", record(base, "session_start", "treated", &key, json!({})));
    let mut feed = Feed::start(&w, &["--session", "S", "--stdin", "ignore"]);
    drop(feed.stdin.take());
    feed.decision(base);
    // A record written after the close comes through: the follower went round its loop, which
    // asks first whether the stream has ended, at least once after its stdin closed.
    w.spool("S", record(base + 1, "batch", "treated", &key, json!({ "observed": ["exec ls"] })));
    feed.decision(base + 1);
    assert!(feed.child.try_wait().unwrap().is_none(), "a closed stdin does not end it");
    #[cfg(unix)]
    {
        let pid = feed.child.id() as i32;
        assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
        let status = feed.child.wait().unwrap();
        assert!(status.success(), "SIGTERM ends it with exit 0: {status:?}");
    }
    #[cfg(not(unix))]
    feed.kill();
    // Without the flag a closed pipe ends it at once.
    let ended = Feed::start(&w, &["--session", "S"]);
    ended.close();
    let bad = w.run(&["watch", "--follow", "--json", "--stdin", "never"], &w.repo(), "", &[], &[]);
    assert!(!bad.status.success() && text(&bad).contains("possible values: eof, ignore"), "{}", text(&bad));
}

/// T-A3-02: a new process started from row 3's cursor sends rows 4 and 5 first, then follows.
#[test]
fn the_feed_resumes_across_processes() {
    let w = World::new();
    let key = w.key();
    let base = 1_790_000_000_000u64;
    let mut feed = Feed::start(&w, &[]);
    let mut cursors = Vec::new();
    for i in 1..=5u64 {
        // Two sessions, so a cursor must hold the position in each.
        let session = if i % 2 == 0 { "A" } else { "B" };
        w.spool(session, record(base + i, "batch", "treated", &key, json!({ "observed": ["x"] })));
        cursors.push(feed.decision(base + i)["cursor"].as_str().unwrap().to_string());
    }
    feed.kill();
    let mut again = Feed::start(&w, &["--since", &cursors[2]]);
    let mut got = Vec::new();
    while got.len() < 2 {
        let v = again
            .until(Duration::from_secs(5), |v| v["kind"] == "decision")
            .unwrap_or_else(|| panic!("rows 4 and 5 did not come: {}", again.all.join("\n")));
        got.push(v["ts"].as_u64().unwrap() - base);
    }
    assert_eq!(got, vec![4, 5], "exactly the rows after row 3, in order");
    w.spool("A", record(base + 6, "batch", "treated", &key, json!({ "observed": ["x"] })));
    let next = again.until(Duration::from_secs(5), |v| v["kind"] == "decision").unwrap();
    assert_eq!(next["ts"], base + 6, "then it follows");
    // A cursor this stream did not write is refused.
    let o = w.run(&["watch", "--follow", "--json", "--since", "nonsense"], &w.repo(), "", &[], &[]);
    assert!(!o.status.success() && text(&o).contains("--since takes a cursor"), "{}", text(&o));
    again.close();
}

/// T-A3-03: the learner's state (waiting with `next_at`, then running) and health as
/// `health::gather` builds it, again when it changes.
#[test]
fn the_feed_says_where_the_learner_and_health_stand() {
    let w = World::new();
    let key = w.key();
    let mut feed = Feed::start(&w, &[]);
    let h = feed.until(Duration::from_secs(10), |v| v["kind"] == "health").expect("a health line");
    let lights = h["lights"].as_array().unwrap();
    assert!(!lights.is_empty(), "{h}");
    for l in lights {
        assert!(l["label"].is_string() && l["lamp"].is_string() && l.get("hint").is_some(), "{l}");
    }
    assert_eq!(h["off"], false);
    // The learner's first line may come before health's (doctor's checks take a moment).
    let early = feed.all.iter().map(|l| serde_json::from_str::<Value>(l).unwrap()).find(|v| v["kind"] == "learner");
    let idle =
        early.or_else(|| feed.until(Duration::from_secs(5), |v| v["kind"] == "learner")).expect("a learner line");
    assert_eq!(idle["state"], "idle");

    let q = adage_hook::autolearn::Queue::new(&w.paths(), &key);
    std::fs::create_dir_all(w.paths().learn_queue()).unwrap();
    let now = adage_core::time::now_ms();
    let after = now + 600_000;
    std::fs::write(q.spawned(), json!({ "at": now, "after": after }).to_string()).unwrap();
    let waiting = feed.until(Duration::from_secs(5), |v| v["kind"] == "learner").expect("waiting");
    assert_eq!((waiting["state"].as_str(), waiting["next_at"].as_u64()), (Some("waiting"), Some(after)));
    std::fs::write(q.spawned(), json!({ "at": now, "after": now - 1 }).to_string()).unwrap();
    let _held = adage_store::lock::exclusive(&q.lock()).unwrap();
    let running = feed.until(Duration::from_secs(5), |v| v["kind"] == "learner").expect("running");
    assert_eq!((running["state"].as_str(), running["next_at"].clone()), (Some("running"), Value::Null));

    // A change in health: the repository is turned off.
    w.ok(&["off"], &w.repo());
    let off = feed.until(Duration::from_secs(10), |v| v["kind"] == "health").expect("a second health line");
    assert_eq!(off["off"], true, "{off}");
    feed.close();
}

/// T-A3-04: the current session's rows never name the arm; another session's are shown as
/// `adage watch` shows them.
#[test]
fn the_current_session_never_names_the_arm() {
    let w = World::new();
    let key = w.key();
    let base = 1_790_000_000_000u64;
    for (session, offset) in [("S", 0u64), ("T", 100)] {
        let t = base + offset;
        w.spool(session, record(t, "session_start", "control", &key, json!({})));
        w.spool(
            session,
            record(
                t + 1,
                "prompt",
                "control",
                &key,
                json!({ "directives": [], "would_have": [{ "route": "lookup-work-item", "strength": "fire", "text": "Run x." }],
                        "judgements": [], "observed": [] }),
            ),
        );
        w.spool(
            session,
            record(
                t + 2,
                "batch",
                "control",
                &key,
                json!({ "directives": [], "would_have": [], "observed": ["a"],
                        "judgements": [{ "route": "lookup-work-item", "kind": "followed" }, { "route": "gen", "kind": "ignored" }] }),
            ),
        );
        w.spool(
            session,
            record(
                t + 3,
                "turn_step",
                "control",
                &key,
                json!({ "would_predict": { "route": "gen", "kind": "obligation" } }),
            ),
        );
        w.spool(session, record(t + 4, "stop", "control", &key, json!({ "would_continue": "finish line" })));
        let mut pre = record(t + 5, "pretool", "control", &key, Value::Null);
        pre["pretool"] = json!({ "rewrite": "test", "interpreter": ".venv/bin/python" });
        pre["withheld"] = json!(true);
        w.spool(session, pre);
        w.spool(session, record(t + 6, "batch", "control", &key, json!({ "observed": ["b"] })));
    }
    let mut feed = Feed::start(&w, &["--session", "S"]);
    feed.decision(base + 106);
    let all = feed.close();
    let rows: Vec<Value> =
        all.lines().map(|l| serde_json::from_str(l).unwrap()).filter(|v: &Value| v["kind"] == "decision").collect();
    let mine: Vec<&Value> = rows.iter().filter(|v| v["session"] == "S").collect();
    let theirs: Vec<&Value> = rows.iter().filter(|v| v["session"] == "T").collect();
    let mine_text = serde_json::to_string(&mine).unwrap();
    for banned in ["control", "would", "withheld", "unaided", "not_reached", "stop_withheld"] {
        assert!(!mine_text.contains(banned), "S's rows say {banned:?}: {mine_text}");
    }
    // What a treated session with nothing fired would send: its start and its quiet batches.
    let kept: Vec<u64> = mine.iter().map(|v| v["ts"].as_u64().unwrap() - base).collect();
    assert_eq!(kept, vec![0, 6], "{mine_text}");
    // T is someone else's session: shown as adage watch shows it.
    let theirs_text = serde_json::to_string(&theirs).unwrap();
    assert_eq!(theirs.len(), 7, "{theirs_text}");
    assert!(theirs.iter().all(|v| v["control"] == true), "{theirs_text}");
    for act in ["withheld", "unaided", "not_reached", "would_predict", "stop_withheld"] {
        assert!(theirs_text.contains(&format!("\"act\":\"{act}\"")), "T lacks {act}: {theirs_text}");
    }
}

/// T-A3-05: no escape sequence, ever; killing the parent ends the follower within 2 s.
#[test]
#[cfg(unix)]
fn the_feed_ends_with_its_parent() {
    let w = World::new();
    let pidfile = w.p("follower.pid");
    let script = format!(
        "'{}' watch --follow --json < /dev/null > '{}' 2>&1 & echo $! > '{}'; wait",
        env!("CARGO_BIN_EXE_adage"),
        w.p("out.jsonl").display(),
        pidfile.display()
    );
    // The follower gets the World's environment through the shell.
    let env = w.cmd(&[], &w.repo());
    let mut parent = Command::new("/bin/sh");
    parent.arg("-c").arg(&script).current_dir(w.repo()).stdin(Stdio::null()).stdout(Stdio::null());
    for (k, v) in env.get_envs() {
        match v {
            Some(v) => parent.env(k, v),
            None => parent.env_remove(k),
        };
    }
    let mut parent = parent.spawn().unwrap();
    let pid = {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(p) = std::fs::read_to_string(&pidfile).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
                break p;
            }
            assert!(Instant::now() < end, "no follower started");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    // Stdin is /dev/null (not a pipe): it follows until stopped. Once it has written a line it
    // has been round its loop; a record written after that line comes through on a later round,
    // each of which asks first whether the stream has ended.
    let out = w.p("out.jsonl");
    let read = || std::fs::read_to_string(&out).unwrap_or_default();
    let wait = |what: &str, done: &dyn Fn(&str) -> bool| {
        let end = Instant::now() + Duration::from_secs(10);
        while !done(&read()) {
            assert!(Instant::now() < end, "the follower wrote no {what}:\n{}", read());
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait("line", &|s| s.contains('\n'));
    let key = w.key();
    let ts = 1_790_000_200_000u64;
    w.spool("S", record(ts, "batch", "treated", &key, json!({ "observed": ["x"] })));
    wait("row for a record written after its first line", &|s| s.contains(&format!("\"ts\":{ts}")));
    let alive = |pid: u32| {
        std::fs::read_to_string(format!("/proc/{pid}/status"))
            .map(|s| {
                !s.lines().any(|l| l.starts_with("State:") && (l.contains("Z (zombie)") || l.contains("X (dead)")))
            })
            .unwrap_or(false)
    };
    if std::path::Path::new("/proc").exists() {
        assert!(alive(pid), "the follower runs on with no stdin");
    }
    parent.kill().unwrap();
    let _ = parent.wait();
    let end = Instant::now() + Duration::from_secs(2);
    while std::path::Path::new("/proc").exists() && alive(pid) {
        assert!(Instant::now() < end, "the follower outlived its parent by 2 s");
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = std::fs::read_to_string(w.p("out.jsonl")).unwrap_or_default();
    assert!(!out.contains('\u{1b}'), "{out}");
}

/// T-A3-06 (L5): 200 rows at 2 Hz for a minute with one forced restart: no gap and no repeat
/// across the cursor, and memory flat. A minute long, so it runs on request:
/// `cargo test -p adage-cli --test mod_surfaces -- --ignored`.
#[test]
#[cfg(target_os = "linux")]
#[ignore = "L5: runs for 100 s"]
fn the_feed_is_steady() {
    let w = World::new();
    let key = w.key();
    let base = 1_790_000_000_000u64;
    let rss = |pid: u32| -> u64 {
        std::fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()
            .and_then(|s| {
                s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
            })
            .unwrap_or(0)
    };
    let mut feed = Feed::start(&w, &[]);
    let mut seen = Vec::new();
    let mut cursor = String::new();
    let mut samples = Vec::new();
    for i in 1..=200u64 {
        w.spool(&format!("s{}", i % 7), record(base + i, "batch", "treated", &key, json!({ "observed": ["x"] })));
        if i == 100 {
            feed.kill();
            feed = Feed::start(&w, &["--since", &cursor]);
        }
        std::thread::sleep(Duration::from_millis(500));
        while let Some(v) = feed.until(Duration::from_millis(1), |v| v["kind"] == "decision") {
            seen.push(v["ts"].as_u64().unwrap() - base);
            cursor = v["cursor"].as_str().unwrap().to_string();
        }
        if i > 100 && i % 10 == 0 {
            samples.push(rss(feed.child.id()));
        }
    }
    while let Some(v) = feed.until(Duration::from_secs(2), |v| v["kind"] == "decision") {
        seen.push(v["ts"].as_u64().unwrap() - base);
    }
    assert_eq!(seen, (1..=200).collect::<Vec<u64>>(), "no gap, no repeat");
    let (lo, hi) = (*samples.iter().min().unwrap(), *samples.iter().max().unwrap());
    assert!(hi - lo <= 10 * 1024, "RSS {lo}..{hi} KiB");
    feed.close();
}

// ---------------------------------------------------------------------------------------------
// adage ledger note

fn note(session: &str, repo: &str, predicted: &[bool]) -> Value {
    let steps: Vec<Value> = predicted
        .iter()
        .enumerate()
        .map(|(i, p)| json!({ "index": i, "model": "claude-sonnet-4-6", "input": 10, "output": 50, "cache_read": 1000, "cache_write": 20, "predicted": p }))
        .collect();
    json!({ "session_id": session, "repo": repo, "harness": "claude-code", "harness_version": "2.1.288",
            "started_at": 1_790_000_000_000u64, "ended_at": 1_790_000_060_000u64, "steps": steps, "cost_usd": 0.0421, "hook_ms": [3, 2] })
}

/// T-A4-03: the command, inside an agent session; refusals write nothing; the savings estimate
/// shows the session with its harness's cost.
#[test]
fn ledger_note_records_a_session_its_spool_knows() {
    let w = World::new();
    let key = w.key();
    let harness = w.p("data/ledger/.harness.jsonl");
    let in_session = [("CLAUDECODE", "1")];
    // No session_start for S yet: refused, nothing written.
    let o = w.run(&["ledger", "note"], &w.repo(), &note("S", &key, &[false]).to_string(), &in_session, &[]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("no session S started"), "{}", text(&o));
    assert!(!harness.exists());
    // Malformed: refused, nothing written.
    for bad in ["{", "[]", &json!({ "session_id": "S" }).to_string()] {
        let o = w.run(&["ledger", "note"], &w.repo(), bad, &in_session, &[]);
        assert_eq!(o.status.code(), Some(2), "{bad}: {}", text(&o));
        assert!(!harness.exists());
    }
    // Recent: the spools' counts cover the last 30 days.
    let now = adage_core::time::now_ms();
    w.spool("S", record(now - 60_000, "session_start", "treated", &key, json!({})));
    w.spool(
        "S",
        record(
            now - 30_000,
            "turn_step",
            "treated",
            &key,
            json!({ "predicted": { "kind": "obligation", "route": "gen" } }),
        ),
    );
    // The repository may be named by the session's working directory.
    let line = note("S", &w.repo().to_string_lossy(), &[false, true, false]);
    let o = w.run(&["ledger", "note"], &w.repo(), &line.to_string(), &in_session, &[]);
    assert!(o.status.success() && o.stdout.is_empty(), "{}", text(&o));
    let lines: Vec<Value> =
        std::fs::read_to_string(&harness).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!((lines[0]["session_id"].as_str(), lines[0]["repo"].as_str()), (Some("S"), Some(key.as_str())));
    // A second note replaces the first.
    let o = w.run(
        &["ledger", "note"],
        &w.repo(),
        &note("S", &key, &[false, true, false, false]).to_string(),
        &in_session,
        &[],
    );
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(&harness).unwrap().lines().count(), 1);

    let v: Value = serde_json::from_str(&w.ok(&["savings", "--json"], &w.repo())).unwrap();
    let h = &v["harness"];
    assert_eq!((h["sessions"].as_u64(), h["predicted_steps"].as_u64()), (Some(1), Some(1)), "{v}");
    assert_eq!(
        h["recent"][0],
        json!({ "session_id": "S", "source": "harness", "model_turns": 3, "predicted_steps": 1 })
    );
    assert_eq!(v["counts"]["predicted_steps"], 1, "the spool's predicted step: {v}");
    let slug: String =
        key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let ledger: Value = serde_json::from_str(
        std::fs::read_to_string(w.p("data/ledger").join(format!("{slug}.jsonl"))).unwrap().lines().next().unwrap(),
    )
    .unwrap();
    assert_eq!(ledger["cost"], json!({ "usd": 0.0421, "source": "harness" }));
    assert_eq!(ledger["model_turns"], 3);
    let o = w.run(&["savings"], &w.repo(), "", &[], &[]);
    assert!(text(&o).contains("1 step without a model request"), "{}", text(&o));
}

/// T-A4-05: merges racing (`adage savings` refreshes and notes at once) leave a well-formed ledger
/// with one line per session.
#[test]
fn concurrent_merges_leave_one_line_per_session() {
    let w = World::new();
    let key = w.key();
    for i in 0..8 {
        w.spool(&format!("s{i}"), record(1_790_000_000_000, "session_start", "treated", &key, json!({})));
    }
    let mut children = Vec::new();
    for i in 0..8 {
        let mut c = w.cmd(&["ledger", "note"], &w.repo());
        let mut child = c.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(note(&format!("s{i}"), &key, &[false, true]).to_string().as_bytes())
            .unwrap();
        children.push(child);
        children.push(w.cmd(&["savings", "--json"], &w.repo()).stdin(Stdio::null()).spawn().unwrap());
    }
    for c in children {
        let o = c.wait_with_output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
    }
    w.ok(&["savings", "--json"], &w.repo());
    let slug: String =
        key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let body = std::fs::read_to_string(w.p("data/ledger").join(format!("{slug}.jsonl"))).unwrap();
    let ids: Vec<String> = body
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap_or_else(|e| panic!("{e}: {l}"))["session_id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!((ids.len(), sorted.len()), (8, 8), "{body}");
    assert_eq!(std::fs::read_to_string(w.p("data/ledger/.harness.jsonl")).unwrap().lines().count(), 8);
}

// ---------------------------------------------------------------------------------------------
// The status card as JSON

/// T-A5-02: `adage --json` parses and its rows are the card's rows one to one; `--for-session`
/// names `/adage watch`, not `adage ui`.
#[test]
fn the_card_as_json() {
    let w = World::new();
    let key = w.key();
    // Connected: a hook has run here.
    w.spool("S", record(1_790_000_000_000, "session_start", "treated", &key, json!({})));
    let card = w.ok(&[], &w.repo());
    let v: Value = serde_json::from_str(&w.ok(&["--json"], &w.repo())).unwrap();
    let mut lines = card.lines();
    assert_eq!(lines.next(), v["header"].as_str());
    assert_eq!(lines.next(), Some(""));
    for row in v["rows"].as_array().unwrap() {
        let want = match row["key"].as_str() {
            Some(k) => format!("{k:<12}{}", row["text"].as_str().unwrap()).trim_end().to_string(),
            None => row["text"].as_str().unwrap().to_string(),
        };
        assert_eq!(lines.next(), Some(want.as_str()), "{card}\n{v:#}");
    }
    assert_eq!(lines.next(), Some(""));
    for (i, n) in v["next"].as_array().unwrap().iter().enumerate() {
        let lead = if i == 0 { "next  " } else { "      " };
        let want = format!("{lead}{} · {}", n["command"].as_str().unwrap(), n["why"].as_str().unwrap());
        assert_eq!(lines.next(), Some(want.as_str()), "{card}");
    }
    assert_eq!(lines.next(), None);
    assert_eq!(
        (v["repo"].as_str(), v["connected"].as_bool(), v["off"].as_bool()),
        (Some(key.as_str()), Some(true), Some(false))
    );
    assert!(v["health"]["word"].is_string() && v["health"]["lamp"].is_string(), "{v}");
    let redact = |s: &str| s.replace(&key, "{REPO}").replace(env!("CARGO_PKG_VERSION"), "{VERSION}");
    insta::assert_snapshot!("card-text", redact(&card));
    insta::assert_snapshot!("card-json", redact(&serde_json::to_string_pretty(&v).unwrap()));

    let s: Value = serde_json::from_str(&w.ok(&["--json", "--for-session"], &w.repo())).unwrap();
    let cmds: Vec<&str> = s["next"].as_array().unwrap().iter().filter_map(|n| n["command"].as_str()).collect();
    assert!(cmds.contains(&"/adage watch") && !cmds.contains(&"adage ui"), "{cmds:?}");
    // The mod's own lines, off by default, as the person sets them.
    assert_eq!(v["ui"], json!({ "spinner": false, "turn_line": false, "noticed": false }));
    w.ok(&["config", "set", "ui.spinner", "true"], &w.repo());
    w.ok(&["config", "set", "ui.turn_line", "true"], &w.repo());
    let on: Value = serde_json::from_str(&w.ok(&["--json"], &w.repo())).unwrap();
    assert_eq!(on["ui"], json!({ "spinner": true, "turn_line": true, "noticed": false }));
    // The mod's routing setting (`runtime.routine_model`): which model is the mod's to read from
    // the card, never the hook's; a name that is not a model is refused.
    assert_eq!(v["runtime"], json!({ "routine_model": "" }));
    w.ok(&["config", "set", "runtime.routine_model", "haiku"], &w.repo());
    let routed: Value = serde_json::from_str(&w.ok(&["--json"], &w.repo())).unwrap();
    assert_eq!(routed["runtime"], json!({ "routine_model": "haiku" }));
    assert!(!w
        .run(&["config", "set", "runtime.routine_model", "not a model"], &w.repo(), "", &[], &[])
        .status
        .success());
    // An interrupted action here (ux-design §9.9): the card names it and what to run once the
    // worktree is inspected.
    assert_eq!(on["recovery"], Value::Null);
    let receipt = adage_exec::journal::Receipt {
        id: "op-7".into(),
        ts: "2026-10-03T09:00:00Z".into(),
        session: "S".into(),
        repo: key.clone(),
        worktree: w.repo().to_string_lossy().into_owned(),
        route: "learned/run-tests".into(),
        approved_on: "fixture".into(),
        level: "e2".into(),
        phase: adage_exec::journal::Phase::Prepared,
        outcome: adage_exec::stage::Outcome::Unknown { reason: "interrupted".into() },
        input_digest: String::new(),
        outputs: Default::default(),
        origin: None,
    };
    adage_store::spool::append(&adage_exec::stage::spool_path(&w.paths().root), &receipt).unwrap();
    let paused: Value = serde_json::from_str(&w.ok(&["--json"], &w.repo())).unwrap();
    assert_eq!(
        paused["recovery"],
        json!({ "operation": "op-7", "route": "learned/run-tests", "command": "adage-exec recover --id op-7 --acknowledge" })
    );
}

// ---------------------------------------------------------------------------------------------
// adage off, adage on

/// T-A6-01: `adage off` from inside an agent session: exit 0, the file, idempotent; then every
/// hook here does nothing, and the card and `adage doctor` say Adage is off.
#[test]
fn off_from_anywhere() {
    let w = World::new();
    let repo = w.repo();
    let marker = repo.join(".adage/disabled");
    let hook = |sid: &str| {
        let payload = json!({ "session_id": sid, "cwd": repo, "hook_event_name": "SessionStart", "source": "startup", "transcript_path": "/dev/null" });
        let o = w.run(&["hook", "claude", "SessionStart"], &repo, &payload.to_string(), &[], &[]);
        assert!(o.status.success());
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    hook("before");
    assert!(w.paths().spool("before").exists(), "a hook here records the session");
    for _ in 0..2 {
        let o = w.run(&["off"], &repo, "", &[("CLAUDECODE", "1")], &[]);
        assert!(o.status.success(), "{}", text(&o));
        let out = String::from_utf8_lossy(&o.stdout).to_string();
        assert_eq!(out, "adage is off for this repository (.adage/disabled). adage on turns it back on.\n");
        assert!(marker.exists());
    }
    assert_eq!(hook("after"), "", "hooks print nothing");
    assert!(!w.paths().spool("after").exists(), "and record nothing");
    let card = w.ok(&[], &repo);
    let first = card.lines().next().unwrap();
    assert!(first.ends_with("○ off in this repository"), "{card}");
    assert!(card.contains("next  adage on · Adage is off in this repository (.adage/disabled)"), "{card}");
    let v: Value = serde_json::from_str(&w.ok(&["--json", "--for-session"], &repo)).unwrap();
    assert_eq!((v["off"].as_bool(), v["next"][0]["command"].as_str()), (Some(true), Some("/adage on")));
    let doctor = w.run(&["doctor"], &repo, "", &[], &[]);
    assert!(text(&doctor).contains("Adage is off in this repository"), "{}", text(&doctor));
}

/// T-A6-02: `adage on` is a person's: refused under every agent-session marker with the file
/// left, done at a terminal; both verbs say one line and exit 0 outside a repository.
#[test]
fn on_is_a_persons() {
    let w = World::new();
    let repo = w.repo();
    let marker = repo.join(".adage/disabled");
    w.ok(&["off"], &repo);
    for var in ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED"] {
        let o = w.run(&["on"], &repo, "", &[(var, "1"), ("ADAGE_ALLOW_NONINTERACTIVE", "1")], &[]);
        assert!(!o.status.success(), "{var}: {}", text(&o));
        assert!(text(&o).contains("is a decision for a person"), "{var}: {}", text(&o));
        assert!(marker.exists(), "{var}: the file stays");
    }
    let o = w.run(&["on"], &repo, "", &[("ADAGE_ALLOW_NONINTERACTIVE", "1")], &["ADAGE_ARM"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "adage is on for this repository.\n");
    assert!(!marker.exists());
    for verb in ["off", "on"] {
        let o = w.run(&[verb], &w.p("outside"), "", &[("CLAUDECODE", "1")], &[]);
        assert!(o.status.success(), "{}", text(&o));
        let out = String::from_utf8_lossy(&o.stdout);
        assert_eq!(out.lines().count(), 1, "{out}");
        assert!(out.starts_with("not inside a repository"), "{out}");
    }
}
