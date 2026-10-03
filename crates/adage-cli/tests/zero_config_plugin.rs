//! The zero-config experience end to end (docs/zero-config.md, acceptance tests 1–9): a person
//! installs the Claude Code plugin and starts a session; no `adage` command is ever typed.
//!
//! Every test runs with an isolated `HOME`, `ADAGE_HOME`, `XDG_CONFIG_HOME` and `CODEX_HOME`, and
//! with `ADAGE_TEST_NO_SPAWN=record` in debug builds: a hook records the learner it would start,
//! and the test runs that exact command line itself. Recorder-dependent cases are explicitly
//! ignored in release, where that instrumentation does not exist. `release_plugin_startup`
//! instead verifies the staged release artifact with a real detached learner and strict budgets.

use adage_sim::{claude, scenario};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

mod common;

struct World {
    _w: common::TempWorld,
    root: PathBuf,
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

#[cfg(unix)]
fn sha256(p: &Path) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(std::fs::read(p).unwrap()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The shim's target triple for this machine (what `session-start` looks for under dist/).
#[cfg(unix)]
fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

impl World {
    fn new() -> World {
        let w = common::TempWorld::new(&["home/.claude/projects", "data", "xdg", "codex", "bin"]);
        World { root: w.root.clone(), _w: w }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    /// A checkout of the demo repository (a Makefile, scripts, schema) at `rel`.
    fn repo(&self, rel: &str) -> PathBuf {
        let at = self.p(rel);
        copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/sandboxes/acme-billing"), &at);
        std::fs::create_dir_all(at.join(".git")).unwrap();
        at
    }
    /// `n` simulated Claude Code traces recorded in `cwd`, in `~/.claude/projects`.
    #[cfg(unix)]
    fn history(&self, cwd: &Path, seed: u64, n: usize) {
        let cwd = cwd.to_string_lossy();
        for s in scenario::history(seed).iter().take(n) {
            let dir = self.p("home/.claude/projects").join(claude::project_dir(&cwd));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{}.jsonl", claude::session_id(s))), claude::transcript(s, &cwd)).unwrap();
        }
    }
    fn env(&self, c: &mut Command) {
        common::scrub(c);
        c.env("HOME", self.p("home"))
            .env("ADAGE_HOME", self.p("data"))
            .env("XDG_CONFIG_HOME", self.p("xdg"))
            .env("CODEX_HOME", self.p("codex"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.p("bin").display()))
            .env("ADAGE_TEST_NO_SPAWN", "record")
            .env("ADAGE_ARM", "treated");
        c.env_remove("XDG_DATA_HOME");
    }
    fn run(&self, exe: &Path, args: &[&str], stdin: &str, cwd: &Path) -> Output {
        let mut c = Command::new(exe);
        c.args(args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        self.env(&mut c);
        let mut child = c.spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        child.wait_with_output().unwrap()
    }
    /// `adage` with extra environment variables.
    #[cfg(unix)]
    fn run_env(&self, args: &[&str], stdin: &str, vars: &[(&str, &str)]) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.args(args).current_dir(&self.root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        self.env(&mut c);
        c.envs(vars.iter().copied());
        let mut child = c.spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        child.wait_with_output().unwrap()
    }
    fn adage(&self, args: &[&str], stdin: &str) -> Output {
        self.run(Path::new(env!("CARGO_BIN_EXE_adage")), args, stdin, &self.root)
    }
    fn adage_in(&self, cwd: &Path, args: &[&str]) -> String {
        let o = self.run(Path::new(env!("CARGO_BIN_EXE_adage")), args, "", cwd);
        assert!(o.status.success(), "adage {args:?}: {}", text(&o));
        String::from_utf8_lossy(&o.stdout).to_string()
    }
    /// One hook event, as Claude Code sends it.
    fn hook(&self, event: &str, cwd: &Path, sid: &str, extra: Value) -> String {
        let mut p = json!({ "session_id": sid, "cwd": cwd, "hook_event_name": event, "transcript_path": "/dev/null" });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let o = self.adage(&["hook", "claude", event], &p.to_string());
        assert!(o.status.success() && o.stderr.is_empty(), "{}", text(&o));
        String::from_utf8_lossy(&o.stdout).to_string()
    }
    fn start(&self, cwd: &Path, sid: &str) -> String {
        self.hook("SessionStart", cwd, sid, json!({ "source": "startup" }))
    }
    fn spawned(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.p("data/learn-queue/spawn-record.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| {
                let v: Value = serde_json::from_str(l).unwrap();
                v["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect()
            })
            .collect()
    }
    /// Run the learner the last hook recorded, exactly as it would have been started (without
    /// its wait: `--after` is dropped).
    fn run_learner(&self) {
        let mut args = self.spawned().pop().expect("a learner was requested");
        if let Some(i) = args.iter().position(|a| a == "--after") {
            args.drain(i..i + 2);
        }
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let o = self.adage(&a, "");
        assert!(o.status.success(), "{}", text(&o));
    }
    /// A copy of the Claude Code plugin with this build's adage, and adage-exec when it is built
    /// beside it, bundled and checksummed, as the release stages it for the marketplace (one
    /// plugin, both binaries: plan A8).
    #[cfg(unix)]
    fn staged_plugin(&self) -> Option<PathBuf> {
        self.staged_plugin_with(true)
    }
    /// [`World::staged_plugin`], with adage-exec left out when `exec` is false.
    #[cfg(unix)]
    fn staged_plugin_with(&self, exec: bool) -> Option<PathBuf> {
        let t = target()?;
        let root = self.p("plugin");
        let _ = std::fs::remove_dir_all(&root);
        copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin/claude-code"), &root);
        let _ = std::fs::remove_dir_all(root.join("dist"));
        let built = Path::new(env!("CARGO_BIN_EXE_adage"));
        let mut sums = String::new();
        for name in ["adage", "adage-exec"] {
            let from = built.with_file_name(name);
            if name == "adage-exec" && !(exec && from.is_file()) {
                continue;
            }
            let bin = root.join("dist").join(t).join(name);
            std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
            std::fs::copy(&from, &bin).unwrap();
            sums.push_str(&format!("{}  {t}/{name}\n", sha256(&bin)));
        }
        std::fs::write(root.join("dist/SHA256SUMS"), sums).unwrap();
        Some(root)
    }
    #[cfg(unix)]
    fn shim(&self, plugin: &Path, data: &Path, payload: &Value) -> (Output, Duration) {
        let t = Instant::now();
        let o = self.run(
            &plugin.join("scripts/session-start"),
            &[&plugin.to_string_lossy(), &data.to_string_lossy()],
            &payload.to_string(),
            &self.root,
        );
        (o, t.elapsed())
    }
    fn card(&self, cwd: &Path) -> String {
        self.adage_in(cwd, &[])
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn context(out: &str) -> String {
    serde_json::from_str::<Value>(out.trim())
        .map(|v| v["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

fn system_message(out: &str) -> Option<String> {
    let v: Value = serde_json::from_str(out.trim()).ok()?;
    v["systemMessage"].as_str().map(str::to_string)
}

/// Product startup budgets apply to the release artifact, including checksum verification,
/// installation and first execution. Dispatch the checked-in registration itself, so a green
/// result requires the plugin's actual command/args paths and a real background bootstrap.
#[test]
#[cfg(all(unix, not(debug_assertions)))]
fn release_plugin_startup() {
    let w = World::new();
    let plugin = w.staged_plugin().expect("the release startup gate requires a supported plugin target");
    let data = w.p("plugin-data");
    let repo = w.repo("work/billing");
    w.history(&repo, adage_sim::CLAUDE_SEED, 12);
    let registration: Value = serde_json::from_slice(&std::fs::read(plugin.join("hooks/hooks.json")).unwrap()).unwrap();
    let dispatch = |event: &str, sid: &str, extra: Value| {
        let spec = &registration["hooks"][event][0]["hooks"][0];
        assert_eq!(spec["type"], "command");
        let expand = |s: &str| {
            s.replace("${CLAUDE_PLUGIN_ROOT}", &plugin.to_string_lossy())
                .replace("${CLAUDE_PLUGIN_DATA}", &data.to_string_lossy())
        };
        let exe = PathBuf::from(expand(spec["command"].as_str().expect("registered hook command")));
        let args: Vec<String> = spec["args"].as_array().unwrap().iter().map(|a| expand(a.as_str().unwrap())).collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut p = json!({ "session_id": sid, "cwd": repo, "hook_event_name": event, "source": "startup" });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let now = Instant::now();
        let out = w.run(&exe, &args, &p.to_string(), &repo);
        let took = now.elapsed();
        assert!(out.status.success() && out.stderr.is_empty(), "{event}: {}", text(&out));
        (out, took)
    };
    let (first, cold) = dispatch("SessionStart", "release-first", json!({}));
    assert!(cold < Duration::from_millis(1500), "release first startup exceeds 1500 ms: {cold:?}");
    let notice = system_message(&String::from_utf8_lossy(&first.stdout)).expect("first-use notice");
    assert!(notice.contains("learning from 12 past sessions in this repo"), "{notice}");
    assert!(data.join("bin/adage").is_file(), "the installed binary must serve later hooks");
    assert!(!w.p("data/learn-queue/spawn-record.jsonl").exists(), "production must not use the debug recorder");

    let waiting = Instant::now();
    loop {
        let learned = std::fs::read_to_string(w.p("data/learn.log")).unwrap_or_default().lines().any(|line| {
            serde_json::from_str::<Value>(line)
                .is_ok_and(|r| r["kind"] == "condition" && r["sessions_read"].as_u64().is_some_and(|n| n >= 12))
        });
        let idle = std::fs::read_dir(w.p("data/learn-queue")).into_iter().flatten().flatten().all(|e| {
            e.path().extension().is_none_or(|x| x != "lock") || !adage_store::lock::is_held(&e.path()).unwrap_or(true)
        });
        if learned && idle {
            break;
        }
        assert!(waiting.elapsed() < Duration::from_secs(120), "the real detached learner did not finish");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut warm = Vec::new();
    for i in 0..10 {
        let (_, took) = dispatch("SessionStart", &format!("release-warm-{i}"), json!({}));
        warm.push(took);
    }
    warm.sort();
    // Nearest-rank p95 with ten samples is their maximum: all ten must meet the existing budget.
    let p95 = warm[9];
    assert!(p95 < Duration::from_millis(200), "release repeated startup exceeds 200 ms: {warm:?}");
    let (failure, _) = dispatch(
        "PostToolUseFailure",
        "release-warm-9",
        json!({
            "tool_name": "Bash", "tool_input": { "command": "python3 -m unittest" },
            "error": "RuntimeError: ACME_ENV is not set"
        }),
    );
    assert!(context(&String::from_utf8_lossy(&failure.stdout)).contains("make test"), "{}", text(&failure));
    eprintln!("release plugin: first={cold:?}; repeated p95={p95:?}; 12 historical sessions learned; registered failure hook delivered make test");
}

/// 1. The plugin, a repository with history, and nothing else: the first session start learns,
///    the first prompt is directed, and another repository's traces are never opened.
#[test]
#[cfg(unix)]
#[cfg_attr(
    not(debug_assertions),
    ignore = "requires the debug-only spawn recorder; release_plugin_startup exercises the release artifact"
)]
fn zero_config_plugin() {
    let w = World::new();
    let Some(plugin) = w.staged_plugin() else { return };
    let repo_w = w.repo("work/billing");
    let repo_x = w.repo("work/other");
    w.history(&repo_w, adage_sim::CLAUDE_SEED, 12);
    w.history(&repo_x, 99, 5);
    let data = w.p("plugin-data");

    // Warm the page cache for the binary copy, as an installed plugin's files would be.
    let _ = std::fs::read(plugin.join("dist").join(target().unwrap()).join("adage"));
    let payload =
        json!({ "session_id": "s-first", "cwd": repo_w, "hook_event_name": "SessionStart", "source": "startup" });
    let (o, took) = w.shim(&plugin, &data, &payload);
    assert!(o.status.success(), "{}", text(&o));
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let msg = system_message(&out).unwrap_or_else(|| panic!("a systemMessage: {out:?}"));
    assert!(msg.contains("learning from 12 past sessions in this repo"), "{msg}");
    // This test uses the instrumented debug binary. Enforce product latency budgets on the
    // shipped release artifact in release_plugin_startup, not on debug symbols and codegen.
    let (o2, took2) = w.shim(
        &plugin,
        &data,
        &json!({ "session_id": "s-again", "cwd": repo_w, "hook_event_name": "SessionStart", "source": "startup" }),
    );
    assert!(o2.status.success());
    eprintln!("debug plugin startup: first {took:?}, repeat {took2:?} (release budgets tested separately)");
    let spawned = w.spawned();
    assert!(spawned[0].contains(&"--bootstrap".to_string()), "{spawned:?}");
    // Launch report, bug 11: the bootstrap session is assigned its arm and spools its
    // `session_start` at once, so the savings estimate counts it (it never did: the repository had
    // no reflexes yet when it started).
    let start: Vec<Value> = adage_store::spool::read_all(&w.p("data/sessions/s-first.spool")).unwrap_or_default();
    assert!(
        start.iter().any(|r| r["kind"] == "session_start" && r["arm"].is_string() && r["repo"].is_string()),
        "{start:?}"
    );

    // The learner, as the hook started it; then the first prompt, through the plugin's binary.
    w.run_learner();
    let bin = data.join("bin/adage");
    let p = json!({ "session_id": "s-first", "cwd": repo_w, "hook_event_name": "UserPromptSubmit", "prompt": "fix W-2042" });
    let o = w.run(&bin, &["hook", "claude", "UserPromptSubmit"], &p.to_string(), &repo_w);
    let ctx = context(&String::from_utf8_lossy(&o.stdout));
    // The lookup is learned, but 9 of the 10 traces it applied to already ran it unaided: telling the
    // agent changes nothing, so it is kept quiet (nascent, with why) at the default thresholds.
    assert!(!ctx.contains("scripts/wi-lookup W-2042"), "a quiet lookup was delivered: {ctx:?}");
    let route = std::fs::read_dir(w.p("data/tracks"))
        .unwrap()
        .flatten()
        .filter_map(|e| adage_tracks::io::read(&e.path().join("learned")).ok())
        .flat_map(|t| t.routes)
        .find(|r| r.id == "lookup-work-item")
        .expect("the lookup is learned");
    let up = route.evidence.uplift.as_ref().expect("its uplift");
    assert!(route.state == adage_tracks::RouteState::Nascent && up.gated && up.traces_took >= 9, "{route:?}");

    // Repository X's traces were never opened: the parse cache has no entry for them.
    let manifest = std::fs::read_to_string(w.p("data/cache/sessions/manifest.json")).unwrap();
    assert!(manifest.contains(&*repo_w.to_string_lossy()), "{manifest}");
    assert!(!manifest.contains(&*repo_x.to_string_lossy()), "X was opened: {manifest}");
    // And the ledger holds W's 12 traces, priced, with no path or prompt in it.
    let slug = std::fs::read_dir(w.p("data/ledger"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|n| n.ends_with(".jsonl"))
        .unwrap();
    let ledger = std::fs::read_to_string(w.p("data/ledger").join(&slug)).unwrap();
    assert_eq!(ledger.lines().count(), 12, "{ledger}");
    assert!(ledger.contains("\"source\":\"priced@"), "{ledger}");
    assert!(!ledger.contains("/work/") && !ledger.contains("W-10"), "{ledger}");
}

/// Real git repositories, for worktrees. `None` without git.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "a")
        .env("GIT_AUTHOR_EMAIL", "a@b")
        .env("GIT_COMMITTER_NAME", "a")
        .env("GIT_COMMITTER_EMAIL", "a@b")
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// A trace in `cwd`: a failing `python3 -m pytest`, then `make test` passing (a dead end).
fn dead_end_trace(dir: &Path, sid: &str, cwd: &Path) -> PathBuf {
    let cwd = cwd.to_string_lossy();
    let mut lines = vec![
        json!({"type":"user","sessionId":sid,"cwd":cwd,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":"Fix the rounding bug."}}),
    ];
    for (n, (cmd, out, err)) in
        [("python3 -m pytest tests", "/usr/bin/python3: No module named pytest", true), ("make test", "OK", false)]
            .iter()
            .enumerate()
    {
        let id = format!("toolu_{n}");
        lines.push(json!({"type":"assistant","sessionId":sid,"cwd":cwd,"message":{"id":format!("msg_{n}"),"model":"claude-test","content":[{"type":"tool_use","id":id,"name":"Bash","input":{"command":cmd}}]}}));
        lines.push(json!({"type":"user","sessionId":sid,"cwd":cwd,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":out,"is_error":err}]}}));
    }
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(format!("{sid}.jsonl"));
    std::fs::write(&p, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
    p
}

/// 2. Reflexes learned in the main checkout reach its linked worktrees (`git worktree add`, and
///    Claude Code's `.claude/worktrees/<name>`) and its subdirectories, with and without `origin`.
#[test]
fn worktree_keys() {
    for origin in [false, true] {
        let w = World::new();
        let main = w.p("app");
        std::fs::create_dir_all(&main).unwrap();
        if git(&main, &["init", "-q", "-b", "main"]).is_none() {
            eprintln!("git not available: skipping");
            return;
        }
        std::fs::write(main.join("README.md"), "app").unwrap();
        std::fs::create_dir_all(main.join("src")).unwrap();
        std::fs::write(main.join("src/lib.py"), "").unwrap();
        git(&main, &["add", "."]).unwrap();
        git(&main, &["commit", "-q", "-m", "init"]).unwrap();
        if origin {
            git(&main, &["remote", "add", "origin", "git@github.com:Acme/App.git"]).unwrap();
        }
        let linked = w.p("app-feature");
        git(&main, &["worktree", "add", "-q", "-b", "feature", &linked.to_string_lossy()]).unwrap();
        let agent = main.join(".claude/worktrees/agent-1");
        git(&main, &["worktree", "add", "-q", "-b", "agent", &agent.to_string_lossy()]).unwrap();

        let files: Vec<String> =
            (0..3).map(|i| dead_end_trace(&w.p("tx"), &format!("s{i}"), &main).to_string_lossy().to_string()).collect();
        let mut args = vec!["condition", "--demo", "--json", "--transcripts"];
        args.extend(files.iter().map(String::as_str));
        let o = w.adage(&args, "");
        assert!(o.status.success(), "{}", text(&o));

        for (cwd, what) in [
            (main.clone(), "the main checkout"),
            (main.join("src"), "a subdirectory"),
            (linked.clone(), "a linked worktree"),
            (linked.join("src"), "a linked worktree's subdirectory"),
            (agent.clone(), "a Claude Code agent worktree"),
        ] {
            let out = w.hook(
                "UserPromptSubmit",
                &cwd,
                &format!("{what}-{origin}"),
                json!({ "prompt": "Fix the rounding bug." }),
            );
            assert!(context(&out).contains("python3 -m pytest"), "origin={origin}, {what}: {out:?}");
        }
    }
}

/// 3. A repository with a Makefile and no history at all: instincts direct the very first session,
///    and the card says how many sessions of its own it needs.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn no_history_instincts() {
    let w = World::new();
    let repo = w.repo("work/acme");
    let msg = system_message(&w.start(&repo, "s1")).expect("the first start says what Adage does");
    assert!(msg.contains("learning from this repo as you work"), "{msg}");
    w.run_learner();
    let out = w.hook("UserPromptSubmit", &repo, "s1", json!({ "prompt": "Fix the invoice rounding" }));
    let _ = out; // instincts act after a failure or an edit, not at the prompt
    let out = w.hook(
        "PostToolUseFailure",
        &repo,
        "s1",
        json!({ "tool_name": "Bash", "tool_input": { "command": "python3 -m unittest tests.test_billing -v" }, "error": "RuntimeError: ACME_ENV is not set" }),
    );
    let ctx = context(&out);
    assert!(
        ctx.contains("origin=\"instinct\"") && ctx.contains("make test"),
        "an instinct directive in session 1: {out:?}"
    );
    let card = w.card(&repo);
    assert!(card.contains("0 of ~5 sessions needed"), "{card}");
    assert!(card.contains("instincts active meanwhile"), "{card}");
}

/// Launch report, bug 7: instincts bound against a checkout without its `.venv` (RepoBench's
/// training checkouts; a per-task worktree) directed nothing useful there. They bind against the
/// session's own checkout and again when its declared files change: here a `.venv` made after the
/// first binding brings U11's `.venv/bin/python -m pytest` at the next session start.
#[cfg(unix)]
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn instincts_rebind_when_the_checkout_changes() {
    let w = World::new();
    let repo = w.p("work/click");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src/click")).unwrap();
    std::fs::write(repo.join("src/click/__init__.py"), "").unwrap();
    std::fs::write(repo.join("pyproject.toml"), "[project]\nname = 'click'\n[tool.pytest.ini_options]\n").unwrap();
    let pytest_failed = || {
        let out = w.hook(
            "PostToolUseFailure",
            &repo,
            "s-fail",
            json!({ "tool_name": "Bash", "tool_input": { "command": "python -m pytest tests -x" }, "error": "/usr/bin/python: No module named pytest" }),
        );
        context(&out)
    };
    w.start(&repo, "s1");
    w.run_learner();
    assert!(!pytest_failed().contains(".venv/bin/python"), "no .venv yet");
    let before = w.spawned().len();
    // The same checkout again, unchanged: nothing to rebind.
    w.start(&repo, "s2");
    assert_eq!(w.spawned().len(), before, "{:?}", w.spawned());
    // A .venv appears (`uv venv`, `python -m venv .venv`): the next session start rebinds.
    std::fs::create_dir_all(repo.join(".venv/bin")).unwrap();
    std::fs::write(repo.join(".venv/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    w.start(&repo, "s3");
    let spawned = w.spawned();
    assert_eq!(spawned.len(), before + 1, "{spawned:?}");
    assert!(spawned.last().unwrap().contains(&"--bootstrap".to_string()), "{spawned:?}");
    w.run_learner();
    assert!(pytest_failed().contains(".venv/bin/python -m pytest"), "U11 bound against the live checkout");
    w.start(&repo, "s4");
    assert_eq!(w.spawned().len(), before + 1, "bound as the files are now: no second rebind");
}

/// PreToolUse delivery of U11: with the project's environment verified at bind time, a test run
/// under the system Python runs with `.venv/bin/python` instead (only the interpreter prefix
/// changes, through Claude Code's `updatedInput`; at the repository root the command names it as
/// the note does, relative, as a person's permission rules would), a `pip install` into the
/// system Python is refused, and the card says how many commands were rewritten. Golden hook
/// output.
#[cfg(unix)]
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn pretool_rewrites_to_the_verified_environment() {
    let w = World::new();
    let repo = w.p("work/click");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src/click")).unwrap();
    std::fs::write(repo.join("src/click/__init__.py"), "").unwrap();
    std::fs::write(repo.join("pyproject.toml"), "[project]\nname = 'click'\n[tool.pytest.ini_options]\n").unwrap();
    // A uv-made environment: its interpreter, pytest, and click installed editable from `src`.
    let site = repo.join(".venv/lib/python3.12/site-packages");
    std::fs::create_dir_all(site.join("pytest")).unwrap();
    std::fs::write(site.join("pytest/__init__.py"), "").unwrap();
    std::fs::write(site.join("click.pth"), format!("{}\n", repo.join("src").display())).unwrap();
    std::fs::create_dir_all(repo.join(".venv/bin")).unwrap();
    std::fs::write(repo.join(".venv/bin/python"), "").unwrap();
    std::fs::write(repo.join(".venv/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    w.start(&repo, "s1");
    w.run_learner();
    let pre = |cmd: &str| {
        w.hook(
            "PreToolUse",
            &repo,
            "s1",
            json!({ "tool_name": "Bash", "tool_input": { "command": cmd, "description": "Run the tests" } }),
        )
    };
    // RepoBench v2, click, a control session's dead end, verbatim.
    let out = pre("python -m pytest tests/test_formatting.py -xvs 2>&1 | head -50");
    let py = repo.join(".venv/bin/python");
    let want = json!({
        "systemMessage": "adage: pointed this test run at .venv/bin/python (the project's environment).",
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "updatedInput": {
                "command": ".venv/bin/python -m pytest tests/test_formatting.py -xvs 2>&1 | head -50",
                "description": "Run the tests"
            },
            "additionalContext": "Adage pointed this test run at the project's environment (.venv/bin/python): the system Python lacks the project and its test dependencies. Nothing else in the command changed."
        }
    });
    assert_eq!(serde_json::from_str::<Value>(out.trim()).unwrap(), want, "{out}");
    // A pip install of a test dependency into the system Python gets advice, never a refusal
    // (review B2): the command runs as written, under the person's own permission rules.
    let out = pre("pip install pytest");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert!(v["hookSpecificOutput"].get("permissionDecision").is_none(), "{out}");
    assert!(v["hookSpecificOutput"].get("updatedInput").is_none(), "{out}");
    let advice = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(advice.contains(&format!("{} -m pip install <package>", py.display())), "{advice}");
    // ... and nothing for an unrelated tool, `--user`, or a heredoc line (the reviewer's cases).
    for cmd in ["pip install --user httpie", "pip install pre-commit && pre-commit install", "pip install numpy"] {
        assert_eq!(pre(cmd).trim(), "", "{cmd}");
    }
    // Review B1, end to end: a heredoc body is data. The CI script is written as the agent wrote
    // it, with no machine path in it; a commit message is left alone.
    for cmd in [
        "cat > ci.sh <<'EOF'\n#!/bin/sh\npytest -q\nEOF",
        "git commit -F - <<'EOF'\nFix rounding\n\npytest tests passes now\nEOF",
        "cat > setup.sh <<'EOF'\npip install -e .\nEOF",
    ] {
        assert_eq!(pre(cmd).trim(), "", "{cmd}");
    }
    // Review S1 and S2: another project's tests, or a pinned interpreter, are left alone.
    assert_eq!(pre("cd /home/me/otherproj && pytest").trim(), "");
    assert_eq!(pre("python3.9 -m pytest tests").trim(), "");
    // The project's own interpreter, and anything else, pass untouched.
    assert_eq!(pre(".venv/bin/python -m pytest tests").trim(), "");
    assert_eq!(pre("ls -la").trim(), "");
    assert_eq!(pre("./pipeline.sh").trim(), "", "`pipeline` is not `pip`");
    // `adage delegate off` turns it off.
    std::fs::create_dir_all(w.p("data/exec")).unwrap();
    std::fs::write(w.p("data/exec/rewrite-off"), "").unwrap();
    assert_eq!(pre("python -m pytest tests").trim(), "");
    std::fs::remove_file(w.p("data/exec/rewrite-off")).unwrap();
    // ... as does `runtime.rewrite = false` (`adage delegate rewrites off`), read at every call.
    w.adage_in(&repo, &["delegate", "rewrites", "off"]);
    assert_eq!(pre("python -m pytest tests").trim(), "");
    w.adage_in(&repo, &["delegate", "rewrites", "on"]);
    // Review S4: the person allowed `pytest` as written; the rewritten command would lose that
    // rule, so the hook advises instead of rewriting.
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    std::fs::write(repo.join(".claude/settings.local.json"), r#"{"permissions":{"allow":["Bash(pytest:*)"]}}"#)
        .unwrap();
    let out = pre("pytest -q tests");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert!(v["hookSpecificOutput"].get("updatedInput").is_none(), "{out}");
    let advice = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(advice.contains("`.venv/bin/python -m pytest -q tests`"), "{advice}");
    // First-run QA item 7: a rule for the interpreter as the person writes it admits the
    // rewrite made at the root.
    std::fs::write(
        repo.join(".claude/settings.local.json"),
        r#"{"permissions":{"allow":["Bash(pytest:*)","Bash(.venv/bin/python -m pytest:*)"]}}"#,
    )
    .unwrap();
    let out = pre("pytest -q tests");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["hookSpecificOutput"]["updatedInput"]["command"], ".venv/bin/python -m pytest -q tests", "{out}");
    // From a subdirectory the relative word would name another file: the absolute interpreter,
    // and both notes name that.
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    let out = w.hook(
        "PreToolUse",
        &repo.join("tests"),
        "s1",
        json!({ "tool_name": "Bash", "tool_input": { "command": "python -m pytest test_formatting.py" } }),
    );
    let v: Value = serde_json::from_str(out.trim()).unwrap_or_else(|_| panic!("{out}"));
    let abs = py.display().to_string();
    assert_eq!(v["hookSpecificOutput"]["updatedInput"]["command"], format!("{abs} -m pytest test_formatting.py"));
    assert!(v["systemMessage"].as_str().unwrap().contains(&abs), "{out}");
    assert!(v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains(&format!("({abs})")), "{out}");
    std::fs::remove_file(repo.join(".claude/settings.local.json")).unwrap();
    // A session started in an activated environment: that environment's `python` is left alone.
    let o = w.run_env(
        &["hook", "claude", "PreToolUse"],
        &json!({ "session_id": "s1", "cwd": repo, "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": { "command": "pytest -q" } }).to_string(),
        &[("VIRTUAL_ENV", "/opt/envs/other")],
    );
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "");
    // Recorded (never the command itself), and visible on the card: rewrites only.
    let recs: Vec<Value> = adage_store::spool::read_all(&w.p("data/sessions/s1.spool")).unwrap();
    let pre: Vec<&Value> = recs.iter().filter(|r| r["kind"] == "pretool").collect();
    assert_eq!(pre.len(), 5, "{recs:?}");
    assert_eq!(pre[0]["pretool"], json!({ "rewrite": "test", "interpreter": ".venv/bin/python" }));
    assert_eq!(pre[1]["pretool"], json!({ "advise": "pip-install", "interpreter": ".venv/bin/python" }));
    assert_eq!(pre[2]["pretool"], json!({ "advise": "permissions", "interpreter": ".venv/bin/python" }));
    assert_eq!(pre[3]["pretool"], json!({ "rewrite": "test", "interpreter": ".venv/bin/python" }));
    assert_eq!(pre[4]["pretool"], json!({ "rewrite": "test", "interpreter": ".venv/bin/python" }));
    assert!(!serde_json::to_string(&recs).unwrap().contains("test_formatting"), "no command text in the spool");
    let key = pre[0]["repo"].as_str().unwrap();
    let slug: String =
        key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    std::fs::create_dir_all(w.p("data/ledger")).unwrap();
    std::fs::write(w.p("data/ledger").join(format!("{slug}.jsonl")), "").unwrap();
    let card = w.card(&repo);
    assert!(card.contains("rewrote 3 commands to use the project's environment"), "{card}");
}

/// Spools and a ledger for a synthetic population of `n_t` treated and `n_c` control sessions of
/// `repo_key`, whose cost is `base` (treated) or `base + delta` (control), jittered. Every fifth
/// treated session's harness also reported it (`adage ledger note`'s file), with two steps Adage
/// took in the model's place: its transcript line counts them as turns, the note does not, and
/// the estimate must read them as turns not spent (T-A4-04).
#[allow(clippy::too_many_arguments)]
fn population(w: &World, repo_key: &str, n_t: usize, n_c: usize, base: f64, delta: f64, spread: f64, seed: u64) {
    let slug: String =
        repo_key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let _ = std::fs::remove_dir_all(w.p("data/sessions"));
    std::fs::create_dir_all(w.p("data/sessions")).unwrap();
    std::fs::create_dir_all(w.p("data/ledger")).unwrap();
    let mut rng = scenario::Rng::new(seed);
    let mut ledger = String::new();
    let mut notes = String::new();
    let old = 1_000_000u64; // long finished
    for i in 0..(n_t + n_c) {
        let control = i >= n_t;
        let id = format!("sess-{seed}-{i:03}");
        let (arm, p) = if control { ("control", 0.1) } else { ("treated", 0.9) };
        let spool = w.p("data/sessions").join(format!("{id}.spool"));
        adage_store::spool::append(&spool, &json!({ "ts": old, "event": "SessionStart", "kind": "session_start", "arm": arm, "propensity": p, "repo": repo_key, "generation": "g" })).unwrap();
        adage_store::spool::append(&spool, &json!({ "ts": old + 1, "event": "SessionEnd", "kind": "session_end", "arm": arm, "propensity": p, "repo": repo_key, "generation": "g", "decision": { "directives": [], "would_have": [], "judgements": [], "observed": [] } })).unwrap();
        let jitter = (rng.below(1000) as f64 / 1000.0 - 0.5) * 2.0 * spread * base;
        let cost = base + jitter + if control { delta } else { 0.0 };
        let turns = 10 + rng.below(6) as u32 + if control { 2 } else { 0 };
        let noted = !control && i % 5 == 0;
        let predicted = if noted { 2 } else { 0 };
        ledger.push_str(&format!(
            "{}\n",
            json!({ "session_id": id, "harness": "claude-code", "models": ["claude-sonnet-4-6"], "started_at": old, "ended_at": old + 1,
                    "model_turns": turns + predicted, "tool_calls": turns, "tokens": { "input": 1, "output": 1, "cache_write": 0, "cache_read": 1 },
                    "cost": { "usd": cost.max(0.0), "source": "priced@2026-06-24" }, "asked_for_help": false })
        ));
        if noted {
            let steps: Vec<Value> = (0..turns + predicted)
                .map(|k| json!({ "index": k, "model": "claude-sonnet-4-6", "input": 1, "output": 1, "cache_read": 1, "cache_write": 0, "predicted": k % 4 == 1 && k < 8 }))
                .collect();
            notes.push_str(&format!(
                "{}\n",
                json!({ "session_id": id, "repo": repo_key, "harness": "claude-code", "harness_version": "2.1.288", "started_at": old,
                        "ended_at": old + 1, "steps": steps, "cost_usd": cost.max(0.0), "noted_at": 9_000_000_000_000u64 })
            ));
        }
    }
    std::fs::write(w.p("data/ledger").join(format!("{slug}.jsonl")), ledger).unwrap();
    std::fs::write(w.p("data/ledger/.harness.jsonl"), notes).unwrap();
}

/// The key hooks and the card use for a repository without origin.
fn key_of(w: &World, repo: &Path) -> String {
    let v: Value = serde_json::from_str(&w.adage_in(repo, &["savings", "--json"])).unwrap();
    v["repo"].as_str().unwrap().to_string()
}

/// 4. Dollars only past the gate, the same numbers on every run, and "appears to add" when that
///    is what the sessions show. (The interval's coverage is checked by simulation in the
///    estimator's own tests, `savings::tests::the_interval_covers_the_true_difference`.)
#[test]
fn savings_gate() {
    let w = World::new();
    let repo = w.repo("work/acme");
    // Connected: a hook has run here.
    w.start(&repo, "connect");
    let key = key_of(&w, &repo);

    population(&w, &key, 25, 4, 0.20, 0.05, 0.5, 1);
    let card = w.card(&repo);
    assert!(card.contains("measuring: 4 of 10 quiet sessions so far"), "{card}");
    assert!(!card.contains('$'), "no dollars below the gate:\n{card}");

    population(&w, &key, 40, 12, 0.20, 0.10, 1.0, 2);
    let a = w.adage_in(&repo, &["savings", "--json"]);
    let b = w.adage_in(&repo, &["savings", "--json"]);
    let strip = |s: &str| {
        let mut v: Value = serde_json::from_str(s).unwrap();
        v["computed_at"] = json!(0);
        v
    };
    assert_eq!(strip(&a), strip(&b), "the same numbers on every run");
    let v = strip(&a);
    assert_eq!(
        (v["n_treated"].as_u64(), v["n_control"].as_u64(), v["gate_met"].as_bool()),
        (Some(40), Some(12), Some(true))
    );
    let c = &v["cost_usd"];
    assert!(c["lo"].as_f64().unwrap() <= 0.10 && 0.10 <= c["hi"].as_f64().unwrap(), "covers the true Δ: {c}");
    // T-A4-04: eight treated sessions reported by their harness, two predicted steps each, read as
    // turns not spent: their ledger lines now count only the steps the model answered.
    assert_eq!(
        (v["harness"]["sessions"].as_u64(), v["harness"]["predicted_steps"].as_u64()),
        (Some(8), Some(16)),
        "{v}"
    );
    assert!(v["harness"]["recent"].as_array().unwrap().iter().all(|s| s["source"] == "harness"), "{v}");
    let slug: String =
        key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let merged = std::fs::read_to_string(w.p("data/ledger").join(format!("{slug}.jsonl"))).unwrap();
    let noted: Value = merged
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|l| l["session_id"] == "sess-2-000")
        .unwrap();
    let note_steps = std::fs::read_to_string(w.p("data/ledger/.harness.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|n| n["session_id"] == "sess-2-000")
        .unwrap()["steps"]
        .as_array()
        .unwrap()
        .len() as u64;
    assert_eq!(noted["model_turns"].as_u64(), Some(note_steps - 2), "{noted}");
    assert_eq!(noted["cost"]["source"], "harness", "{noted}");
    assert!(c["winsorized_at"].as_f64().is_some(), "winsorisation disclosed: {c}");
    // A clear saving: the card says it, with its interval and the guardrail.
    population(&w, &key, 40, 12, 0.20, 0.10, 0.5, 4);
    let card = w.card(&repo);
    assert!(card.contains("✓ ~$") && card.contains("API-list equivalent") && card.contains("95% CI"), "{card}");
    assert!(card.contains("ended asking you for help"), "the guardrail beside it:\n{card}");

    population(&w, &key, 40, 12, 0.30, -0.12, 0.5, 3);
    let card = w.card(&repo);
    assert!(card.contains("Adage appears to add ~$") && card.contains("See adage review"), "{card}");
}

/// 5. A session on a model the price table does not list: tokens and turns, never dollars.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn pricing_honesty() {
    let w = World::new();
    let repo = w.repo("work/acme");
    let dir = w.p("home/.claude/projects").join(claude::project_dir(&repo.to_string_lossy()));
    std::fs::create_dir_all(&dir).unwrap();
    let rec = |mid: &str| json!({"type":"assistant","sessionId":"s-mystery","cwd":repo,"timestamp":"2026-09-20T10:00:00Z","message":{"id":mid,"model":"mystery-model-9","content":[{"type":"text","text":"done"}],"usage":{"input_tokens":120,"output_tokens":40}}});
    let user = json!({"type":"user","sessionId":"s-mystery","cwd":repo,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":"hello"}});
    std::fs::write(dir.join("s-mystery.jsonl"), format!("{user}\n{}\n{}\n", rec("m1"), rec("m2"))).unwrap();
    w.start(&repo, "s-now");
    w.run_learner();
    let ledger_dir = w.p("data/ledger");
    let file = std::fs::read_dir(&ledger_dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .unwrap();
    let line: Value = serde_json::from_str(std::fs::read_to_string(file).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["cost"], json!({ "source": "none" }), "{line}");
    assert_eq!(
        (line["model_turns"].as_u64(), line["tokens"]["input"].as_u64(), line["tokens"]["output"].as_u64()),
        (Some(2), Some(240), Some(80))
    );
    let v: Value = serde_json::from_str(&w.adage_in(&repo, &["savings", "--json"])).unwrap();
    assert_eq!(v["unpriced"], 1);
    assert!(!w.card(&repo).contains('$'));
}

/// 6. A proxy blocks the model download (Hugging Face and the npm fallback): classified, recorded,
///    explained once in doctor, never retried by itself; the bundled model serves meanwhile, and
///    hooks and learning go on.
#[test]
#[cfg(unix)]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn model_blocked() {
    let w = World::new();
    let calls = w.p("curl-calls");
    let stub = w.p("bin/curl");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\necho x >> '{}'\necho 'curl: (22) The requested URL returned error: 403' >&2\nexit 22\n",
            calls.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = w.adage(&["models", "fetch"], "");
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("huggingface.co blocked by proxy (403)"), "{}", text(&o));
    assert!(text(&o).contains("Fallback: registry.npmjs.org blocked by proxy (403)"), "{}", text(&o));
    assert!(text(&o).contains("Still embedding with potion-base-8m-i8@b9c0e329a2ce (bundled"), "{}", text(&o));
    let f: Value =
        serde_json::from_str(&std::fs::read_to_string(w.p("data/models/.fetch-failed.json")).unwrap()).unwrap();
    assert_eq!((f["class"].as_str(), f["host"].as_str()), (Some("blocked"), Some("huggingface.co")), "{f}");

    let o = w.adage(&["doctor", "--json"], "");
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let m = v["checks"].as_array().unwrap().iter().find(|c| c["check"] == "embedding model").unwrap();
    assert_eq!(m["status"], "pass", "{m}");
    let detail = m["detail"].as_str().unwrap();
    assert!(detail.contains("bundled in this binary (int8, SHA-256 verified)"), "{m}");
    assert!(
        detail.contains(
            "the last download failed: huggingface.co blocked by proxy (403). Use a mirror (ADAGE_MODEL_BASE_URL)"
        ),
        "{m}"
    );

    // Hooks and learning are unaffected, and nothing retries the download.
    let repo = w.repo("work/acme");
    w.start(&repo, "s1");
    w.run_learner();
    let out = w.hook(
        "PostToolUseFailure",
        &repo,
        "s1",
        json!({ "tool_name": "Bash", "tool_input": { "command": "python3 -m unittest" }, "error": "RuntimeError: ACME_ENV is not set" }),
    );
    assert!(context(&out).contains("make test"), "{out}");
    w.hook("Stop", &repo, "s1", json!({ "stop_hook_active": false }));
    let card = w.card(&repo);
    assert!(card.contains("embeddings  potion-base-8m-i8 (bundled)"), "{card}");
    assert!(!card.contains("hashed fallback"), "{card}");
    assert_eq!(
        std::fs::read_to_string(&calls).unwrap().lines().count(),
        2,
        "one download attempt, the person's: Hugging Face, then the npm fallback"
    );

    // A mirror is used when named (still checked against the pinned digests).
    let o = w.run(Path::new(env!("CARGO_BIN_EXE_adage")), &["models", "fetch"], "", &w.root);
    let _ = o;
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
    c.args(["models", "fetch"]).current_dir(&w.root);
    w.env(&mut c);
    let o = c.env("ADAGE_MODEL_BASE_URL", "https://mirror.example/potion/").output().unwrap();
    assert!(text(&o).contains("mirror.example blocked by proxy"), "{}", text(&o));
}

/// 7. Execution on (`$ADAGE_HOME/exec/enabled`): the hook binary follows the marker.
///
/// The one plugin's hooks run adage-exec when it is bundled (plan A8); without it, adage keeps
/// serving and says why.
#[test]
#[cfg(unix)]
fn exec_marker_picks_the_hook_binary() {
    let w = World::new();
    let repo = w.repo("work/acme");
    std::fs::create_dir_all(w.p("data/exec")).unwrap();
    std::fs::write(w.p("data/exec/enabled"), "{}").unwrap();
    let start = json!({ "session_id": "s1", "cwd": repo, "hook_event_name": "SessionStart", "source": "startup" });
    let Some(plugin) = w.staged_plugin_with(false) else { return };
    let data = w.p("plugin-data");
    let (o, _) = w.shim(&plugin, &data, &start);
    assert!(o.status.success());
    let link = std::fs::read_link(data.join("bin/adage")).unwrap();
    assert!(link.to_string_lossy().starts_with("adage-"), "the real binary, not quiet: {link:?}");
    assert_eq!(std::fs::read_link(data.join("bin/hook")).unwrap(), Path::new("adage"));
    let msg = system_message(&String::from_utf8_lossy(&o.stdout)).unwrap();
    assert!(msg.contains("execution is on, but no adage-exec binary was found"), "{msg}");
    if !Path::new(env!("CARGO_BIN_EXE_adage")).with_file_name("adage-exec").is_file() {
        eprintln!("skipped the bundled adage-exec leg: it is not built beside adage");
        return;
    }
    let Some(plugin) = w.staged_plugin() else { return };
    let data = w.p("plugin-data-exec");
    let (o, _) = w.shim(&plugin, &data, &start);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(std::fs::read_link(data.join("bin/hook")).unwrap(), Path::new("adage-exec"));
    assert!(std::fs::read_link(data.join("bin/adage-exec")).unwrap().to_string_lossy().starts_with("adage-exec-"));
    assert_eq!(system_message(&String::from_utf8_lossy(&o.stdout)), None, "nothing to say: execution serves");
}

/// 8. At most one notice a day per repository, and only when something is new.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn notice_throttle() {
    let w = World::new();
    let repo = w.repo("work/acme");
    assert!(system_message(&w.start(&repo, "s1")).is_some(), "the first start says what Adage does");
    w.run_learner();
    assert_eq!(system_message(&w.start(&repo, "s2")), None, "again the same day: nothing");

    // Something new: an install step (pnpm) that waits for a person.
    std::fs::write(
        repo.join("package.json"),
        r#"{"name":"acme-web","packageManager":"pnpm@9.1.0","scripts":{"test":"node --test"}}"#,
    )
    .unwrap();
    std::fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    w.hook("Stop", &repo, "s2", json!({ "stop_hook_active": false }));
    w.run_learner();
    assert_eq!(system_message(&w.start(&repo, "s3")), None, "still the same day");

    // The next day.
    let shown = std::fs::read_dir(w.p("data/learn-queue"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with(".notice-shown.json"))
        .unwrap();
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&shown).unwrap()).unwrap();
    v["at"] = json!(v["at"].as_u64().unwrap() - 25 * 3_600_000);
    std::fs::write(&shown, v.to_string()).unwrap();
    let msg = system_message(&w.start(&repo, "s4")).expect("a new notice the next day");
    // Launch report, bug 12: no `adage` is on this PATH (a plugin-only user), so the notice names
    // the binary that ran the learner, and that command works as written.
    assert!(msg.contains("for you: run ") && msg.contains(" review in a terminal"), "{msg}");
    let cmd = msg.split("run ").nth(1).and_then(|r| r.split(" review").next()).unwrap();
    assert_ne!(cmd, "adage", "{msg}");
    let home = w.p("home");
    let exe = PathBuf::from(cmd.trim_matches('\'').replacen('~', &home.to_string_lossy(), 1));
    let o = w.run(&exe, &["review"], "", &repo);
    assert!(o.status.success() && text(&o).contains("pnpm"), "{cmd} review: {}", text(&o));
    assert_eq!(system_message(&w.start(&repo, "s5")), None, "once");

    // `ui.notices = off` silences them.
    let o = w.adage(&["config", "set", "ui.notices", "off"], "");
    assert!(o.status.success(), "{}", text(&o));
}

/// 9. A damaged index is rebuilt at the next session start, silently, and the next event is served.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn index_self_heal() {
    let w = World::new();
    let repo = w.repo("work/acme");
    w.start(&repo, "s0");
    w.run_learner();
    let index = w.p("data/index.json");
    let good = std::fs::read(&index).unwrap();
    std::fs::write(&index, &good[..good.len() / 3]).unwrap();
    let before = w.spawned().len();
    let out = w.start(&repo, "s1");
    assert_eq!(out, "", "no notice for a repair");
    let spawned = w.spawned();
    assert_eq!(spawned.len(), before + 1, "a rebuild was requested");
    assert!(spawned.last().unwrap().contains(&"--bootstrap".to_string()));
    w.run_learner();
    let out = w.hook(
        "PostToolUseFailure",
        &repo,
        "s1",
        json!({ "tool_name": "Bash", "tool_input": { "command": "python3 -m unittest" }, "error": "RuntimeError: ACME_ENV is not set" }),
    );
    assert!(context(&out).contains("make test"), "served again: {out}");
}
