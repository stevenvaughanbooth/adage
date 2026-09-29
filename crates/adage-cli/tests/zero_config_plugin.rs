//! The zero-config experience end to end (docs/zero-config.md, acceptance tests 1–9): a person
//! installs the Claude Code plugin and starts a session; no `adage` command is ever typed.
//!
//! Every test runs with an isolated `HOME`, `ADAGE_HOME`, `XDG_CONFIG_HOME` and `CODEX_HOME`, and
//! with `ADAGE_TEST_NO_SPAWN=record`: a hook records the learner it would start, and the test runs
//! that exact command line itself, as the detached learner would.

use adage_sim::{claude, scenario};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct World {
    _dir: tempfile::TempDir,
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

fn sha256(p: &Path) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(std::fs::read(p).unwrap()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The shim's target triple for this machine (what `session-start` looks for under dist/).
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home/.claude/projects", "data", "xdg", "codex", "bin"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        World { _dir: dir, root }
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
    fn history(&self, cwd: &Path, seed: u64, n: usize) {
        let cwd = cwd.to_string_lossy();
        for s in scenario::history(seed).iter().take(n) {
            let dir = self.p("home/.claude/projects").join(claude::project_dir(&cwd));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{}.jsonl", claude::session_id(s))), claude::transcript(s, &cwd)).unwrap();
        }
    }
    fn env(&self, c: &mut Command) {
        c.env("HOME", self.p("home"))
            .env("ADAGE_HOME", self.p("data"))
            .env("XDG_CONFIG_HOME", self.p("xdg"))
            .env("CODEX_HOME", self.p("codex"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.p("bin").display()))
            .env("ADAGE_TEST_NO_SPAWN", "record")
            .env("ADAGE_ARM", "treated");
        for v in [
            "ADAGE_DISABLE",
            "ADAGE_AUTO_LEARN",
            "ADAGE_CONTROL",
            "ADAGE_MODEL_BASE_URL",
            "CLAUDECODE",
            "OPENCODE",
            "CODEX_SANDBOX",
            "CODEX_SANDBOX_NETWORK_DISABLED",
            "CLAUDE_CONFIG_DIR",
            "XDG_DATA_HOME",
        ] {
            c.env_remove(v);
        }
    }
    fn run(&self, exe: &Path, args: &[&str], stdin: &str, cwd: &Path) -> Output {
        let mut c = Command::new(exe);
        c.args(args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        self.env(&mut c);
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
    /// A copy of the Claude Code plugin with this build's adage bundled and checksummed, as the
    /// release stages it for the marketplace.
    fn staged_plugin(&self) -> Option<PathBuf> {
        let t = target()?;
        let root = self.p("plugin");
        copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin/claude-code"), &root);
        let _ = std::fs::remove_dir_all(root.join("dist"));
        let bin = root.join("dist").join(t).join("adage");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::copy(env!("CARGO_BIN_EXE_adage"), &bin).unwrap();
        std::fs::write(root.join("dist/SHA256SUMS"), format!("{}  {t}/adage\n", sha256(&bin))).unwrap();
        Some(root)
    }
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

/// 1. The plugin, a repository with history, and nothing else: the first session start learns,
///    the first prompt is directed, and another repository's traces are never opened.
#[test]
#[cfg(unix)]
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
    assert!(took < Duration::from_millis(1500), "the first start (copying the binary) took {took:?}");
    // Every later start is the plain hook path: well under 200 ms.
    let (o2, took2) = w.shim(
        &plugin,
        &data,
        &json!({ "session_id": "s-again", "cwd": repo_w, "hook_event_name": "SessionStart", "source": "startup" }),
    );
    assert!(o2.status.success());
    assert!(took2 < Duration::from_millis(200), "a session start took {took2:?}");
    let spawned = w.spawned();
    assert!(spawned[0].contains(&"--bootstrap".to_string()), "{spawned:?}");

    // The learner, as the hook started it; then the first prompt, through the plugin's binary.
    w.run_learner();
    let bin = data.join("bin/adage");
    let p = json!({ "session_id": "s-first", "cwd": repo_w, "hook_event_name": "UserPromptSubmit", "prompt": "fix W-2042" });
    let o = w.run(&bin, &["hook", "claude", "UserPromptSubmit"], &p.to_string(), &repo_w);
    let ctx = context(&String::from_utf8_lossy(&o.stdout));
    assert!(ctx.contains("scripts/wi-lookup W-2042"), "the lookup directive: {ctx:?}");

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
    assert!(card.contains("learning: 0 of ~5 sessions needed"), "{card}");
    assert!(card.contains("instincts active meanwhile"), "{card}");
}

/// Spools and a ledger for a synthetic population of `n_t` treated and `n_c` control sessions of
/// `repo_key`, whose cost is `base` (treated) or `base + delta` (control), jittered.
#[allow(clippy::too_many_arguments)]
fn population(w: &World, repo_key: &str, n_t: usize, n_c: usize, base: f64, delta: f64, spread: f64, seed: u64) {
    let slug: String =
        repo_key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let _ = std::fs::remove_dir_all(w.p("data/sessions"));
    std::fs::create_dir_all(w.p("data/sessions")).unwrap();
    std::fs::create_dir_all(w.p("data/ledger")).unwrap();
    let mut rng = scenario::Rng::new(seed);
    let mut ledger = String::new();
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
        ledger.push_str(&format!(
            "{}\n",
            json!({ "session_id": id, "harness": "claude-code", "models": ["claude-sonnet-4-6"], "started_at": old, "ended_at": old + 1,
                    "model_turns": turns, "tool_calls": turns, "tokens": { "input": 1, "output": 1, "cache_write": 0, "cache_read": 1 },
                    "cost": { "usd": cost.max(0.0), "source": "priced@2026-06-24" }, "asked_for_help": false })
        ));
    }
    std::fs::write(w.p("data/ledger").join(format!("{slug}.jsonl")), ledger).unwrap();
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
    assert!(card.contains("measuring savings: 4 of 10 quiet sessions so far"), "{card}");
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
    assert!(c["winsorized_at"].as_f64().is_some(), "winsorisation disclosed: {c}");
    // A clear saving: the card says it, with its interval and the guardrail.
    population(&w, &key, 40, 12, 0.20, 0.10, 0.5, 4);
    let card = w.card(&repo);
    assert!(card.contains("savings: ~$") && card.contains("API-list equivalent") && card.contains("95% CI"), "{card}");
    assert!(card.contains("ended asking you for help"), "the guardrail beside it:\n{card}");

    population(&w, &key, 40, 12, 0.30, -0.12, 0.5, 3);
    let card = w.card(&repo);
    assert!(card.contains("Adage appears to add ~$") && card.contains("See adage review"), "{card}");
}

/// 5. A session on a model the price table does not list: tokens and turns, never dollars.
#[test]
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
    assert!(text(&o).contains("Still embedding with potion-base-8m@f65d0f325faa+i8 (bundled"), "{}", text(&o));
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
        detail.contains("the last download failed: huggingface.co blocked by proxy (403). Use a mirror (ADAGE_MODEL_BASE_URL)"),
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
    assert!(card.contains("embeddings: potion-base-8m (bundled)"), "{card}");
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

/// 7. `delegate on` without the adage-exec plugin: the Adage plugin keeps serving, and says why.
#[test]
#[cfg(unix)]
fn exec_marker_without_plugin() {
    let w = World::new();
    let Some(plugin) = w.staged_plugin() else { return };
    let repo = w.repo("work/acme");
    std::fs::create_dir_all(w.p("data/exec")).unwrap();
    std::fs::write(w.p("data/exec/enabled"), "{}").unwrap();
    let data = w.p("plugin-data");
    let (o, _) = w.shim(
        &plugin,
        &data,
        &json!({ "session_id": "s1", "cwd": repo, "hook_event_name": "SessionStart", "source": "startup" }),
    );
    assert!(o.status.success());
    let link = std::fs::read_link(data.join("bin/adage")).unwrap();
    assert!(link.to_string_lossy().starts_with("adage-"), "the real binary, not quiet: {link:?}");
    let msg = system_message(&String::from_utf8_lossy(&o.stdout)).unwrap();
    assert!(
        msg.contains("delegation is on but the adage-exec plugin isn't installed; serving without execution"),
        "{msg}"
    );
}

/// 8. At most one notice a day per repository, and only when something is new.
#[test]
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
    assert!(msg.contains("for you: run adage review in a terminal"), "{msg}");
    assert_eq!(system_message(&w.start(&repo, "s5")), None, "once");

    // `ui.notices = off` silences them.
    let o = w.adage(&["config", "set", "ui.notices", "off"], "");
    assert!(o.status.success(), "{}", text(&o));
}

/// 9. A damaged index is rebuilt at the next session start, silently, and the next event is served.
#[test]
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
