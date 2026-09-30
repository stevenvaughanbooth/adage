//! Live learning end to end through the binary: `config.toml` and `adage config`, the hook's
//! trigger at the end of a turn or session in every harness (recorded rather than spawned, with
//! the debug-only `ADAGE_TEST_NO_SPAWN=record`), debounce and single flight, failing open, the
//! `learn.log` contract, and one real detached learner started by a real hook. Every run uses an
//! isolated `ADAGE_HOME` and `HOME`.

use adage_sim::{claude, codex, opencode, scenario};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    /// A checkout named `acme-billing` with the files the scenario's routes name, and a Claude Code
    /// history of the scenario's twelve sessions in it, in `$HOME/.claude/projects`.
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home", "adage", "acme-billing/.git", "acme-billing/acme", "acme-billing/schema"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["acme/billing.py", "acme/customers.py", "schema/invoice.json", "schema/customer.json", "Makefile"] {
            std::fs::write(root.join("acme-billing").join(f), scenario::body(f)).unwrap();
        }
        World { _dir: dir, root }
    }
    fn repo(&self) -> String {
        self.root.join("acme-billing").to_string_lossy().into_owned()
    }
    fn home(&self) -> PathBuf {
        self.root.join("adage")
    }
    fn q(&self) -> PathBuf {
        self.home().join("learn-queue")
    }
    /// Write the scenario's history, as Claude Code keeps it, for this checkout.
    fn history(&self) -> Vec<PathBuf> {
        let cwd = self.repo();
        let dir = self.root.join("home/.claude/projects").join(claude::project_dir(&cwd));
        std::fs::create_dir_all(&dir).unwrap();
        scenario::history(adage_sim::CLAUDE_SEED)
            .iter()
            .map(|s| {
                let p = dir.join(format!("{}.jsonl", claude::session_id(s)));
                std::fs::write(&p, claude::transcript(s, &cwd)).unwrap();
                p
            })
            .collect()
    }
    fn config(&self, body: &str) {
        std::fs::write(self.home().join("config.toml"), body).unwrap();
    }
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.args(args)
            .current_dir(&self.root)
            .env("ADAGE_HOME", self.home())
            .env("HOME", self.root.join("home"))
            .env("ADAGE_ARM", "treated")
            .env("ADAGE_TEST_NO_SPAWN", "record")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1");
        for v in [
            "ADAGE_DISABLE",
            "ADAGE_AUTO_LEARN",
            "ADAGE_CONTROL",
            "CLAUDECODE",
            "OPENCODE",
            "CODEX_SANDBOX",
            "CODEX_SANDBOX_NETWORK_DISABLED",
        ] {
            c.env_remove(v);
        }
        c
    }
    fn run(&self, mut c: Command, stdin: &str) -> Output {
        let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        child.wait_with_output().unwrap()
    }
    /// Run a hook: it always exits 0 and never writes to stderr. Returns stdout.
    fn hook_with(&self, c: Command, payload: &Value) -> String {
        let o = self.run(c, &payload.to_string());
        assert_eq!(o.status.code(), Some(0), "hooks always exit 0");
        assert!(o.stderr.is_empty(), "hooks never write to stderr: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    }
    fn hook(&self, harness: &str, event: &str, payload: &Value) -> String {
        self.hook_with(self.cmd(&["hook", harness, event]), payload)
    }
    fn claude_stop(&self, sid: &str) -> String {
        self.hook("claude", "Stop", &claude::hook(sid, &self.repo(), "Stop", json!({ "stop_hook_active": false })))
    }
    fn spawns(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.q().join("spawn-record.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .map(|v| v["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect())
            .collect()
    }
    fn requests(&self) -> Vec<Value> {
        let mut v: Vec<Value> = std::fs::read_dir(self.q())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n.ends_with(".json") && n.matches('.').count() == 1
            })
            .map(|e| serde_json::from_slice(&std::fs::read(e.path()).unwrap()).unwrap())
            .collect();
        v.sort_by_key(|r: &Value| r["requested_at"].as_u64());
        v
    }
    fn log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.home().join("learn.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    fn text(o: &Output) -> String {
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
    }
}

/// Every harness's end-of-turn and end-of-session events request a learn and start a learner,
/// without changing what the hook prints; other events, `learn.auto = false`, `ADAGE_AUTO_LEARN=0`
/// and `.adage/disabled` do not.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn every_harness_enqueues_and_starts_a_learner_at_stop_and_session_end() {
    let w = World::new();
    w.config("[learn]\nmin_interval_secs = 0\n");
    let repo = w.repo();
    let cases: Vec<(&str, &str, Value)> = vec![
        ("claude", "Stop", claude::hook("c1", &repo, "Stop", json!({ "stop_hook_active": false }))),
        ("claude", "SessionEnd", claude::hook("c1", &repo, "SessionEnd", json!({ "reason": "exit" }))),
        ("codex", "Stop", codex::hook("x1", &repo, "Stop", "t1", json!({ "stop_hook_active": false }))),
        ("codex", "SessionEnd", codex::hook("x1", &repo, "SessionEnd", "", json!({}))),
        ("opencode", "session.idle", opencode::payload("o1", &repo, json!({ "stop_hook_active": false }))),
        ("opencode", "session.deleted", opencode::payload("o1", &repo, json!({}))),
    ];
    for (i, (h, e, p)) in cases.iter().enumerate() {
        assert_eq!(w.hook(h, e, p), "", "{h} {e}: no routes, nothing to say");
        let spawns = w.spawns();
        assert_eq!(spawns.len(), i + 1, "{h} {e} starts a learner");
        assert_eq!(spawns[i], vec!["condition", "--auto", "--repo-root", &repo], "{h} {e}");
        let reqs = w.requests();
        assert_eq!(reqs.len(), 1, "one request per repository, refreshed");
        let r = &reqs[0];
        assert_eq!((r["root"].as_str(), r["harness"].as_str()), (Some(repo.as_str()), Some(*h)), "{r}");
        assert!(r["repo"].as_str().unwrap().starts_with("acme-billing@"), "{r}");
        assert!(r["requested_at"].as_u64().unwrap() > 0);
        // Claude Code and Codex name their transcript; OpenCode has none.
        assert_eq!(r.get("transcript_path").is_some(), *h != "opencode", "{r}");
    }
    // Events that do not end a turn or a session request nothing.
    let n = w.spawns().len();
    w.hook("claude", "UserPromptSubmit", &claude::hook("c2", &repo, "UserPromptSubmit", json!({ "prompt": "hi" })));
    w.hook("opencode", "chat.message", &opencode::payload("o2", &repo, json!({ "prompt": "hi" })));
    assert_eq!(w.spawns().len(), n);
    // Turned off: nothing is requested or started (file, then environment).
    std::fs::remove_dir_all(w.q()).unwrap();
    w.config("[learn]\nauto = false\nmin_interval_secs = 0\n");
    w.claude_stop("c3");
    let mut c = w.cmd(&["hook", "claude", "Stop"]);
    w.config("[learn]\nmin_interval_secs = 0\n");
    c.env("ADAGE_AUTO_LEARN", "0");
    w.hook_with(c, &claude::hook("c4", &repo, "Stop", json!({})));
    assert!(!w.q().exists(), "auto-learn off: no queue at all");
    // A kill switch stops it too.
    std::fs::create_dir_all(w.root.join("acme-billing/.adage")).unwrap();
    std::fs::write(w.root.join("acme-billing/.adage/disabled"), "").unwrap();
    w.claude_stop("c5");
    assert!(!w.q().exists(), ".adage/disabled: nothing");
}

#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn debounce_and_single_flight() {
    let w = World::new();
    w.config("[learn]\nmin_interval_secs = 120\n");
    let t0 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    w.claude_stop("s1");
    w.claude_stop("s2");
    w.claude_stop("s3");
    let spawns = w.spawns();
    assert_eq!(spawns.len(), 2, "one learner now, one scheduled for the end of the interval: {spawns:?}");
    assert_eq!(spawns[0].len(), 4, "the first starts at once");
    assert_eq!(spawns[1][4], "--after");
    let after: u64 = spawns[1][5].parse().unwrap();
    assert!(after >= t0 + 120_000 && after < t0 + 125_000, "waits out the interval: {after} vs {t0}");
    assert_eq!(w.requests().len(), 1, "the third was recorded for the scheduled learner");

    // A learner holding the repository's lock: requests are recorded, nothing is started.
    std::fs::remove_file(w.q().join("spawn-record.jsonl")).unwrap();
    for f in std::fs::read_dir(w.q()).unwrap().flatten() {
        if f.file_name().to_string_lossy().ends_with(".spawn.json") {
            std::fs::remove_file(f.path()).unwrap();
        }
    }
    let slug = w.requests()[0]["repo"]
        .as_str()
        .unwrap()
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "-");
    let lock = adage_store::lock::try_exclusive(&w.q().join(format!("{slug}.lock"))).unwrap().expect("free");
    let before = w.requests()[0]["requested_at"].as_u64().unwrap();
    std::thread::sleep(Duration::from_millis(5));
    w.claude_stop("s4");
    assert!(w.spawns().is_empty(), "a learner is running: it re-checks the queue itself");
    assert!(w.requests()[0]["requested_at"].as_u64().unwrap() > before, "the request is refreshed");

    // And a learner started now finds the lock held, and leaves without learning or logging.
    let t = Instant::now();
    let o = w.cmd(&["condition", "--auto", "--repo-root", &w.repo()]).output().unwrap();
    assert!(o.status.success(), "{}", World::text(&o));
    assert!(t.elapsed() >= Duration::from_secs(1), "it waited a little for the lock first");
    assert!(w.log().is_empty(), "single flight: the second learner did nothing");
    drop(lock);
}

#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn a_learner_that_cannot_start_fails_open() {
    let w = World::new();
    let mut c = w.cmd(&["hook", "claude", "Stop"]);
    c.env("ADAGE_TEST_NO_SPAWN", "fail");
    let out = w.hook_with(c, &claude::hook("f1", &w.repo(), "Stop", json!({})));
    assert_eq!(out, "", "output unchanged");
    let log = std::fs::read_to_string(w.home().join("fail-open.log")).unwrap();
    let v: Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!((v["class"].as_str(), v["event"].as_str()), (Some("condition"), Some("Stop")), "{v}");
    assert!(!log.contains(&w.repo()), "no paths in the fail-open log");
}

/// `adage condition --auto`, run the way a hook starts it: it learns only this repository, logs the
/// changes in the `learn.log` contract, and a second run with nothing new changes nothing.
#[test]
fn learn_auto_logs_what_changed() {
    let w = World::new();
    w.config("[learn]\nthresholds = \"demo\"\n");
    w.history();
    // A second repository's session, which an automatic learn of acme-billing must not touch.
    let other = w.root.join("acme-ledger");
    std::fs::create_dir_all(other.join(".git")).unwrap();
    let odir = w.root.join("home/.claude/projects").join(claude::project_dir(&other.to_string_lossy()));
    std::fs::create_dir_all(&odir).unwrap();
    for s in scenario::history(44).iter().take(6) {
        std::fs::write(
            odir.join(format!("{}.jsonl", claude::session_id(s))),
            claude::transcript(s, &other.to_string_lossy()),
        )
        .unwrap();
    }
    let o = w.cmd(&["condition", "--auto", "--repo-root", &w.repo()]).output().unwrap();
    assert!(o.status.success() && o.stdout.is_empty(), "quiet: {}", World::text(&o));
    let log = w.log();
    assert_eq!(log.len(), 1, "{log:?}");
    let l = &log[0];
    assert_eq!((l["kind"].as_str(), l["trigger"].as_str()), (Some("condition"), Some("auto")));
    let repo = l["repo"].as_str().unwrap();
    assert!(repo.starts_with("acme-billing@"), "{l}");
    assert_eq!(
        (l["sessions_read"].as_u64(), l["sessions_used"].as_u64()),
        (Some(12), Some(12)),
        "only this repository"
    );
    assert!(l["duration_ms"].is_u64() && l["index_generation"].is_string() && l["ts"].is_u64());
    let changes = l["changes"].as_array().unwrap();
    assert!(changes.len() >= 3, "{l}");
    for c in changes {
        // An instinct the traces confirm gives way to the learned reflex in the same learn.
        if c["change"] == "extinguished" && c["track"].as_str().is_some_and(|t| t.ends_with("/instincts")) {
            assert!(c["summary"].as_str().unwrap().contains("confirmed by traces"), "{c}");
            continue;
        }
        assert!(["new", "nascent"].contains(&c["change"].as_str().unwrap()), "first learn: every route is new: {c}");
        assert!(c["from"].is_null() && c["to"].is_string() && c["summary"].is_string(), "{c}");
        assert!(c["track"].as_str().unwrap().starts_with("acme-billing-path-"), "{c}");
    }
    assert!(changes.iter().any(|c| c["route"] == "lookup-work-item"), "{l}");
    // The legacy fields are still there.
    assert!(l["repos"][repo]["tracks"].is_array() && l["version"].is_string() && l["thresholds"].is_string());
    let text = std::fs::read_to_string(w.home().join("learn.log")).unwrap();
    assert!(!text.contains(&*w.root.to_string_lossy()), "no paths in learn.log");
    assert!(!text.contains("W-") && !text.contains("wi-lookup"), "no prompt text or commands in learn.log");
    let ledger = std::fs::read_dir(w.home().join("tracks"))
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("acme-ledger"));
    assert!(!ledger, "the other repository was not learned");
    let state: Value = serde_json::from_slice(
        &std::fs::read(
            std::fs::read_dir(w.q())
                .unwrap()
                .flatten()
                .find(|e| e.file_name().to_string_lossy().ends_with(".state.json"))
                .unwrap()
                .path(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(state["outcome"], "ok", "{state}");

    // Again, with nothing new: no changes, and every transcript came from the cache.
    let o = w.cmd(&["condition", "--auto", "--repo-root", &w.repo()]).output().unwrap();
    assert!(o.status.success());
    let log = w.log();
    assert_eq!(log[1]["changes"], json!([]), "{}", log[1]);
    assert_eq!(log[1]["index_generation"], log[0]["index_generation"]);

    // A manual learn writes the same kind of line, with trigger manual, one per repository.
    let o = w.cmd(&["condition", "--demo"]).output().unwrap();
    assert!(o.status.success(), "{}", World::text(&o));
    let all = w.log();
    let manual: Vec<&Value> = all.iter().skip(2).collect();
    assert_eq!(manual.len(), 2, "two repositories: {manual:?}");
    assert!(manual.iter().all(|l| l["trigger"] == "manual" && l["kind"] == "condition"));
    let billing = manual.iter().find(|l| l["repo"] == repo).unwrap();
    assert_eq!(billing["changes"], json!([]), "billing was already learned: {billing}");
    let ledger = manual.iter().find(|l| l["repo"] != repo).unwrap();
    assert!(ledger["changes"].as_array().is_some_and(|c| !c.is_empty()), "{ledger}");

    // A repository that is gone: a condition_error line with a fixed message, no path.
    let o = w.cmd(&["condition", "--auto", "--repo-root", &w.root.join("gone").to_string_lossy()]).output().unwrap();
    assert!(o.status.success());
    let last = w.log().pop().unwrap();
    assert_eq!((last["kind"].as_str(), last["class"].as_str()), (Some("condition_error"), Some("repo")), "{last}");
    assert!(!last.to_string().contains(&*w.root.to_string_lossy()));
}

#[test]
fn config_command_validates_and_refuses_agents() {
    let w = World::new();
    let o = w.cmd(&["config", "--json"]).output().unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let keys: Vec<&str> = v["keys"].as_array().unwrap().iter().map(|k| k["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&"learn.auto") && keys.contains(&"runtime.control") && keys.contains(&"learn.thresholds"));
    assert!(v["keys"].as_array().unwrap().iter().all(|k| k["source"] == "default"));

    let o = w.cmd(&["config", "set", "learn.min_interval_secs", "30"]).output().unwrap();
    assert!(o.status.success(), "{}", World::text(&o));
    let o = w.cmd(&["config", "get", "learn.min_interval_secs"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "30");
    let o = w.cmd(&["config", "set", "runtime.control", "2"]).output().unwrap();
    assert!(!o.status.success() && World::text(&o).contains("between 0 and 1"), "{}", World::text(&o));
    let o = w.cmd(&["config", "set", "learn.nope", "1"]).output().unwrap();
    assert!(!o.status.success() && World::text(&o).contains("unknown key"));

    // Environment beats the file, and `config` says so.
    let o = w.cmd(&["config", "set", "runtime.control", "0.3"]).output().unwrap();
    assert!(o.status.success());
    let o = w.cmd(&["config", "get", "runtime.control"]).env("ADAGE_CONTROL", "0.7").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "0.7");
    let o = w.cmd(&["config"]).env("ADAGE_CONTROL", "0.7").output().unwrap();
    assert!(World::text(&o).contains("env ADAGE_CONTROL"), "{}", World::text(&o));

    // Inside an agent session, reading is fine and changing what Adage learns from is refused,
    // whatever else is set.
    let before = std::fs::read(w.home().join("config.toml")).unwrap();
    for var in ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX"] {
        for (k, v) in
            [("learn.thresholds", "demo"), ("learn.claude_projects", "/elsewhere"), ("instincts.packs", "core")]
        {
            let o = w.cmd(&["config", "set", k, v]).env(var, "1").output().unwrap();
            assert!(!o.status.success() && World::text(&o).contains("agent session"), "{var} {k}: {}", World::text(&o));
        }
        assert!(w.cmd(&["config"]).env(var, "1").output().unwrap().status.success());
    }
    assert_eq!(std::fs::read(w.home().join("config.toml")).unwrap(), before, "nothing was written");
    // A guarded key also needs a terminal ...
    let o = w
        .cmd(&["config", "set", "learn.thresholds", "demo"])
        .env_remove("ADAGE_ALLOW_NONINTERACTIVE")
        .output()
        .unwrap();
    assert!(!o.status.success() && World::text(&o).contains("terminal"), "{}", World::text(&o));
    // ... but a setting that only tunes Adage or turns something down does not, anywhere.
    for (k, v) in [("learn.auto", "false"), ("ui.notices", "off"), ("instincts.enabled", "false")] {
        let o = w
            .cmd(&["config", "set", k, v])
            .env_remove("ADAGE_ALLOW_NONINTERACTIVE")
            .env("CLAUDECODE", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "{k}: {}", World::text(&o));
    }
    let o = w.cmd(&["config", "get", "ui.notices"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "off");
}

fn wait_for(what: &str, limit: Duration, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The whole loop, for real: a Stop hook starts a detached learner, which learns the history,
/// logs the new routes and compiles the index; the next prompt is directed. `runtime.control` in
/// the file puts sessions in the control arm unless `ADAGE_CONTROL` says otherwise.
#[test]
fn a_real_stop_hook_learns_in_the_background() {
    let w = World::new();
    w.config("[learn]\nthresholds = \"demo\"\n[runtime]\ncontrol = 1.0\n");
    w.history();
    let mut c = w.cmd(&["hook", "claude", "Stop"]);
    c.env_remove("ADAGE_TEST_NO_SPAWN");
    let t = Instant::now();
    assert_eq!(w.hook_with(c, &claude::hook("live-1", &w.repo(), "Stop", json!({}))), "");
    let hook_ms = t.elapsed().as_millis();
    eprintln!("the Stop hook returned in {hook_ms} ms, without waiting for the learner");
    wait_for("learn.log to show the learner's routes", Duration::from_secs(120), || {
        w.log().iter().any(|l| l["trigger"] == "auto" && l["changes"].as_array().is_some_and(|c| !c.is_empty()))
    });
    let l = w.log().into_iter().find(|l| l["trigger"] == "auto").unwrap();
    assert!(
        l["changes"].as_array().unwrap().iter().any(|c| c["route"] == "lookup-work-item" && c["change"] == "new"),
        "{l}"
    );
    // Wait for the learner to let go of the lock (it re-checks the queue first).
    let lock = std::fs::read_dir(w.q())
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with(".lock"))
        .unwrap()
        .path();
    wait_for("the learner to exit", Duration::from_secs(60), || !adage_store::lock::is_held(&lock).unwrap());
    let index: Value = serde_json::from_slice(&std::fs::read(w.home().join("index.json")).unwrap()).unwrap();
    assert_eq!(index["generation"], l["index_generation"]);

    // The next session's prompt: shadow mode from the file says nothing; the environment wins.
    let prompt =
        |sid: &str| claude::hook(sid, &w.repo(), "UserPromptSubmit", json!({ "prompt": "Please pick up W-2042" }));
    let mut c = w.cmd(&["hook", "claude", "UserPromptSubmit"]);
    c.env_remove("ADAGE_ARM");
    assert_eq!(w.hook_with(c, &prompt("live-2")), "", "runtime.control = 1: held out");
    let mut c = w.cmd(&["hook", "claude", "UserPromptSubmit"]);
    c.env_remove("ADAGE_ARM").env("ADAGE_CONTROL", "0");
    let out = w.hook_with(c, &prompt("live-3"));
    assert!(out.contains("W-2042"), "ADAGE_CONTROL=0 wins over the file: {out}");

    // Doctor reports it.
    let o = w.cmd(&["doctor", "--json"]).output().unwrap();
    let d: Value = serde_json::from_slice(&o.stdout).unwrap();
    let check = |name: &str| d["checks"].as_array().unwrap().iter().find(|c| c["check"] == name).cloned().unwrap();
    assert_eq!(check("auto-learn")["status"], "pass");
    let runs = check("auto-learn runs");
    assert_eq!(runs["status"], "pass", "{runs}");
    assert!(runs["detail"].as_str().unwrap().contains("ok"), "{runs}");
    assert_eq!(check("auto-learn lock")["status"], "pass");
    assert_eq!(check("auto-learn queue")["status"], "pass", "{}", check("auto-learn queue"));
}

/// Doctor's live-learning checks flag a failed learn, a stuck learner and a request left waiting.
#[test]
fn doctor_reports_failed_stuck_and_waiting_learners() {
    let w = World::new();
    w.config("[learn]\nmin_interval_secs = 0\n");
    w.claude_stop("d1");
    let req = w.requests().remove(0);
    let slug = req["repo"].as_str().unwrap().replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "-");
    let file = |s: &str| w.q().join(format!("{slug}{s}"));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let old = now - 3_600_000;
    // A request an hour old that no learner handled, and a failed last learn.
    let mut r = req.clone();
    r["requested_at"] = json!(old);
    std::fs::write(file(".json"), r.to_string()).unwrap();
    std::fs::write(file(".spawn.json"), json!({ "at": old, "after": old }).to_string()).unwrap();
    std::fs::write(
        file(".state.json"),
        json!({ "pid": 1, "started_at": old - 10, "finished_at": old - 5, "outcome": "error", "processed": 0 })
            .to_string(),
    )
    .unwrap();
    let line = json!({ "ts": old, "kind": "condition_error", "trigger": "auto", "repo": req["repo"], "class": "lock", "error": "taking the tracks lock: WouldBlock" });
    std::fs::write(w.home().join("learn.log"), format!("{line}\n")).unwrap();
    let doctor = || -> Value {
        let o = w.cmd(&["doctor", "--json"]).output().unwrap();
        serde_json::from_slice(&o.stdout).unwrap()
    };
    let check =
        |d: &Value, name: &str| d["checks"].as_array().unwrap().iter().find(|c| c["check"] == name).cloned().unwrap();
    let d = doctor();
    assert_eq!(check(&d, "auto-learn runs")["status"], "warn", "{d}");
    assert!(check(&d, "auto-learn runs")["detail"].as_str().unwrap().contains("lock"));
    assert_eq!(check(&d, "auto-learn queue")["status"], "warn", "{d}");
    // A learner holding the lock, "running" for an hour: stuck.
    std::fs::write(
        file(".state.json"),
        json!({ "pid": 4242, "started_at": old, "outcome": "running", "processed": 0 }).to_string(),
    )
    .unwrap();
    let _lock = adage_store::lock::try_exclusive(&file(".lock")).unwrap().unwrap();
    let d = doctor();
    let stuck = check(&d, "auto-learn lock");
    assert_eq!(stuck["status"], "warn", "{d}");
    assert!(stuck["detail"].as_str().unwrap().contains("4242"));
    assert_eq!(check(&d, "auto-learn queue")["status"], "pass", "a held lock is not backlog");
    // Config problems are reported too.
    w.config("[learn]\nauto = \"sometimes\"\n");
    assert_eq!(check(&doctor(), "config")["status"], "warn");
}
