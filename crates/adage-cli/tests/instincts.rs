//! Instincts end to end through the binary: `adage condition --instincts-only` binds a repository's
//! built-in defaults from its files with no session history, the hooks deliver them with their
//! own wrapper (golden per harness), they never ask the agent to keep going, `adage doctor` and
//! `adage review` show them, a person's rejection holds, pack-level approval works, and agents
//! that follow an instinct into a failure extinguish it at the next learn.
//!
//! Goldens: `tests/golden/instincts-<harness>.txt`; regenerate with `ADAGE_BLESS=1 cargo test -p
//! adage-cli --test instincts` and review the diff.

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

impl World {
    /// The demo repository (`evals/sandboxes/acme-billing`), with a pnpm lockfile added so that
    /// every advisory template and an environment step bind.
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home", "adage", "plain"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/sandboxes/acme-billing");
        copy_dir(&src, &root.join("acme"));
        std::fs::create_dir_all(root.join("acme/.git")).unwrap();
        // An origin gives the repository a key that does not depend on where the test runs.
        std::fs::write(
            root.join("acme/.git/config"),
            "[remote \"origin\"]\n\turl = https://github.com/acme/billing.git\n",
        )
        .unwrap();
        World { _dir: dir, root }
    }
    fn with_pnpm(self) -> World {
        std::fs::write(
            self.p("acme/package.json"),
            r#"{"name":"acme-web","packageManager":"pnpm@9.1.0","scripts":{"test":"node --test"}}"#,
        )
        .unwrap();
        std::fs::write(self.p("acme/pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        self
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn repo(&self) -> String {
        self.p("acme").to_string_lossy().to_string()
    }
    fn run(&self, args: &[&str], stdin: &[u8]) -> (String, String, i32) {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.args(args)
            .current_dir(self.p("plain"))
            .env("ADAGE_HOME", self.p("adage"))
            .env("ADAGE_AUTO_LEARN", "0")
            .env("HOME", self.p("home"))
            .env("XDG_CONFIG_HOME", self.p("home/.config"))
            .env("CODEX_HOME", self.p("home/.codex"))
            .env("ADAGE_ARM", "treated")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .env_remove("ADAGE_DISABLE")
            .env_remove("CLAUDECODE")
            .env_remove("OPENCODE")
            .env_remove("CODEX_SANDBOX")
            .env_remove("CODEX_SANDBOX_NETWORK_DISABLED")
            .env_remove("VIRTUAL_ENV")
            .env_remove("CONDA_PREFIX")
            .env_remove("ADAGE_EMBED_MODEL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        {
            let mut sin = child.stdin.take().unwrap();
            let _ = sin.write_all(stdin);
        }
        let o = child.wait_with_output().unwrap();
        (
            String::from_utf8_lossy(&o.stdout).to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
            o.status.code().unwrap_or(-1),
        )
    }
    fn ok(&self, args: &[&str]) -> String {
        let (out, err, code) = self.run(args, b"");
        assert_eq!(code, 0, "adage {args:?} failed:\n{out}\n{err}");
        out
    }
    fn bind(&self) -> String {
        self.ok(&["condition", "--instincts-only", "--repo-path", &self.repo()])
    }
    fn hook(&self, harness: &str, event: &str, payload: &Value) -> String {
        let (out, err, code) = self.run(&["hook", harness, event], payload.to_string().as_bytes());
        assert_eq!(code, 0, "hooks always exit 0");
        assert!(err.is_empty(), "hooks never write to stderr: {err}");
        out
    }
}

fn golden(name: &str, w: &World, harness: &str, steps: &[(&str, Value)]) -> String {
    let mut transcript = String::new();
    for (event, payload) in steps {
        let out = w.hook(harness, event, payload);
        transcript
            .push_str(&format!("### {event}\n{}\n", if out.is_empty() { "(nothing)\n".to_string() } else { out }));
    }
    let transcript = transcript.replace(&w.root.to_string_lossy().to_string(), "{ROOT}");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.txt"));
    if std::env::var("ADAGE_BLESS").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &transcript).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing golden {}; run with ADAGE_BLESS=1", path.display()));
    assert_eq!(transcript, want, "golden {name} changed");
    transcript
}

/// The repository's key and track directory.
const KEY: &str = "github.com/acme/billing";
const TRACK: &str = "github-com-acme-billing/instincts";

/// What every instinct directive promises, whatever the harness.
fn assert_instinct_text(t: &str) {
    assert!(t.contains("origin=\\\"instinct\\\""), "{t}");
    assert!(t.contains("Adage default for"), "{t}");
    assert!(!t.contains("Learned by Adage"), "an instinct never claims to be learned:\n{t}");
    assert!(!t.contains("CI fails") && !t.contains("Agents in this repository run it"), "{t}");
    assert!(!t.contains("strength=\\\"fire\\\"") && !t.contains("strength=\\\"inhibit\\\""), "prime only:\n{t}");
    assert!(!t.contains("\"decision\":\"block\"") && !t.contains("{\"continue\""), "never a stop request:\n{t}");
}

/// The launch entry point, with no sessions or hand-written tracks: six distinct shipped
/// defaults bind, and their real hooks deliver useful, correctly scoped suggestions. None of
/// these suggestions constitutes permission to execute a repository command.
#[test]
fn init_without_history_delivers_six_shipped_instincts() {
    let python = World::new();
    let node = World::new().with_pnpm();
    for (w, expected) in [(&python, &["U1", "U3", "U4", "U5"][..]), (&node, &["U6", "N1"][..])] {
        let out = w.ok(&["init", "--yes", "--repo", &w.repo()]);
        assert!(out.contains("no traces") || out.contains("No traces"), "{out}");
        let review: Value = serde_json::from_str(&w.ok(&["review", "--json"])).unwrap();
        let rows = review["queue"].as_array().unwrap();
        for instinct in expected {
            assert!(rows.iter().any(|r| r["origin"]["instinct"] == *instinct), "{instinct}: {review:#}");
        }
    }
    let cases = [
        ("U1", "failure", "python3 -m unittest", "RuntimeError: ACME_ENV is not set", "make test"),
        ("U3", "failure", "pytest", "RuntimeError: ACME_ENV is not set", "make test"),
        ("U4", "edit", "acme/generated/models.py", "", "is generated"),
        ("U5", "edit", "schema/invoice.json", "", "make gen"),
        ("U6", "edit", "pnpm-lock.yaml", "", "maintained by pnpm"),
        ("N1", "failure", "yarn test", "yarn: command not found", "pnpm test"),
    ];
    for (instinct, kind, input, error, expected) in cases {
        let w = if ["U6", "N1"].contains(&instinct) { &node } else { &python };
        let cwd = w.repo();
        let session = format!("launch-{instinct}");
        let mut p = json!({ "session_id": session, "cwd": cwd, "transcript_path": "/dev/null" });
        let event = if kind == "failure" {
            p["tool_name"] = json!("Bash");
            p["tool_input"] = json!({ "command": input });
            p["error"] = json!(error);
            "PostToolUseFailure"
        } else {
            p["tool_calls"] = json!([{ "tool_name": "Edit", "tool_input": {
                "file_path": format!("{cwd}/{input}"), "old_string": "a", "new_string": "b"
            }, "tool_response": "ok" }]);
            "PostToolBatch"
        };
        p["hook_event_name"] = json!(event);
        let delivered = w.hook("claude", event, &p);
        assert_instinct_text(&delivered);
        assert!(delivered.contains(expected), "{instinct}: {delivered}");
        assert!(!delivered.contains("updatedInput"), "an advisory is not a CPU rewrite");
        let stop = w.hook(
            "claude",
            "Stop",
            &json!({
                "session_id": session, "cwd": cwd, "hook_event_name": "Stop", "stop_hook_active": false
            }),
        );
        assert!(!stop.contains("\"decision\":\"block\""), "instincts never block completion: {stop}");
    }
    // An assertion failure has the same runner but not the declared failure signature.
    let miss = python.hook(
        "claude",
        "PostToolUseFailure",
        &json!({
            "session_id": "launch-assertion", "cwd": python.repo(), "hook_event_name": "PostToolUseFailure",
            "tool_name": "Bash", "tool_input": { "command": "python3 -m unittest" },
            "error": "FAIL: test_total\nAssertionError: 1 != 2"
        }),
    );
    assert!(miss.trim().is_empty(), "real test failures must not redirect the runner: {miss}");
    assert!(!python.p("adage/approvals.json").exists(), "init must not approve arbitrary scripts");
    assert!(!node.p("adage/approvals.json").exists(), "init must not approve arbitrary scripts");
}

/// U11's CPU rewrite through the same fresh-install path, followed by execution of the actual
/// returned shell command. No model or transcript is involved; a real local virtualenv makes
/// the assertion depend on using the corrected interpreter, not merely on returning JSON.
#[cfg(unix)]
#[test]
fn init_environment_instinct_rewrites_and_runs_without_inference() {
    let w = World::new();
    std::fs::write(w.p("acme/pyproject.toml"), "[project]\nname = 'acme'\n[tool.pytest.ini_options]\n").unwrap();
    let setup = Command::new("python3")
        .args(["-m", "venv", "--without-pip"])
        .arg(w.p("acme/.venv"))
        .output()
        .expect("the execution proof requires python3");
    assert!(setup.status.success(), "{}", String::from_utf8_lossy(&setup.stderr));
    w.ok(&["init", "--yes", "--repo", &w.repo()]);
    let command =
        "python -c 'import acme, sys; assert sys.prefix != sys.base_prefix; print(\"project environment used\")'";
    let payload = json!({
        "session_id": "launch-cpu", "cwd": w.repo(), "hook_event_name": "PreToolUse",
        "tool_name": "Bash", "tool_input": { "command": command, "description": "Reproduce in the project" }
    });
    let delivered = w.hook("claude", "PreToolUse", &payload);
    let v: Value = serde_json::from_str(&delivered).unwrap_or_else(|_| panic!("{delivered}"));
    let updated = &v["hookSpecificOutput"]["updatedInput"];
    let rewritten = updated["command"].as_str().expect("a CPU rewrite, not advice");
    assert!(rewritten.contains(".venv/bin/python"), "{rewritten}");
    assert_eq!(updated["description"], payload["tool_input"]["description"]);
    let executed = Command::new("/bin/sh").args(["-c", rewritten]).current_dir(w.p("acme")).output().unwrap();
    assert!(executed.status.success(), "{}", String::from_utf8_lossy(&executed.stderr));
    assert_eq!(String::from_utf8_lossy(&executed.stdout).trim(), "project environment used");
    // Human opt-out takes effect on the next hook, without another bind or a restart.
    w.ok(&["delegate", "rewrites", "off"]);
    assert!(w.hook("claude", "PreToolUse", &payload).trim().is_empty());
}

#[test]
fn claude_code_instinct_golden() {
    let w = World::new();
    let out = w.bind();
    assert!(out.lines().any(|line| line.contains(&format!("{TRACK}@0.1.0")) && line.ends_with("created")), "{out}");
    let cwd = w.repo();
    let with = |ev: &str, extra: Value| {
        let mut b =
            json!({ "session_id": "s-instinct", "transcript_path": "/dev/null", "cwd": cwd, "hook_event_name": ev });
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    let edit = |p: &str| json!({ "tool_calls": [{ "tool_name": "Edit", "tool_input": { "file_path": format!("{cwd}/{p}"), "old_string": "a", "new_string": "b" }, "tool_response": "ok" }] });
    let t = golden(
        "instincts-claude-code",
        &w,
        "claude",
        &[
            ("SessionStart", with("SessionStart", json!({ "source": "startup" }))),
            ("UserPromptSubmit", with("UserPromptSubmit", json!({ "prompt": "Fix the invoice rounding" }))),
            (
                "PostToolUseFailure",
                with(
                    "PostToolUseFailure",
                    json!({ "tool_name": "Bash", "tool_input": { "command": "python3 -m unittest tests.test_billing -v" }, "error": "RuntimeError: ACME_ENV is not set" }),
                ),
            ),
            (
                "PostToolBatch",
                with(
                    "PostToolBatch",
                    json!({ "tool_calls": [{ "tool_name": "Bash", "tool_input": { "command": "python3 -m unittest tests.test_billing -v" }, "tool_response": { "stdout": "", "stderr": "RuntimeError: ACME_ENV is not set" } }] }),
                ),
            ),
            ("PostToolBatch", with("PostToolBatch", edit("schema/invoice.json"))),
            ("PostToolBatch", with("PostToolBatch", edit("acme/generated/models.py"))),
            ("Stop", with("Stop", json!({ "stop_hook_active": false }))),
            ("SessionEnd", with("SessionEnd", json!({ "reason": "other" }))),
        ],
    );
    assert_instinct_text(&t);
    assert!(t.contains("That failed. In this repository, run this instead:\\n  make test"), "{t}");
    assert!(t.contains("Makefile declares this."), "{t}");
    assert!(t.contains("acme/generated/models.py is generated: change its inputs and run make gen"), "{t}");
    assert!(t.contains("Adage default for Python repositories (from this repository's Makefile)"), "{t}");
}

#[test]
fn opencode_instinct_golden() {
    let w = World::new();
    w.bind();
    let cwd = w.repo();
    let s = |extra: Value| {
        let mut b = json!({ "session_id": "ses_instinct", "cwd": cwd });
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    let patch = "*** Begin Patch\n*** Update File: schema/invoice.json\n@@\n-{}\n+{\"a\":1}\n*** End Patch\n";
    let t = golden(
        "instincts-opencode",
        &w,
        "opencode",
        &[
            ("session.created", s(json!({}))),
            ("chat.message", s(json!({ "prompt": "Fix the invoice rounding" }))),
            (
                "tool.execute.after",
                s(
                    json!({ "tool": "bash", "args": { "command": "python3 -m unittest tests.test_billing -v" }, "output": "RuntimeError: ACME_ENV is not set", "exit": 1 }),
                ),
            ),
            (
                "tool.execute.after",
                s(json!({ "tool": "apply_patch", "args": { "patchText": patch }, "output": "Success", "exit": null })),
            ),
            ("session.idle", s(json!({ "stop_hook_active": false }))),
            ("session.deleted", s(json!({}))),
        ],
    );
    assert_instinct_text(&t);
    assert!(t.contains("run this instead") && t.contains("declares this."), "{t}");
}

#[test]
fn codex_instinct_golden() {
    let w = World::new();
    w.bind();
    let cwd = w.repo();
    let cx = |ev: &str, extra: Value| {
        let mut b = json!({ "session_id": "019a-instinct", "transcript_path": "/dev/null", "cwd": cwd, "hook_event_name": ev, "model": "gpt-test", "permission_mode": "default" });
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    let patch = "*** Begin Patch\n*** Update File: schema/invoice.json\n@@\n-{}\n+{}\n*** End Patch\n";
    let t = golden(
        "instincts-codex",
        &w,
        "codex",
        &[
            ("SessionStart", cx("SessionStart", json!({ "source": "startup" }))),
            (
                "UserPromptSubmit",
                cx("UserPromptSubmit", json!({ "turn_id": "t1", "prompt": "Fix the invoice rounding" })),
            ),
            (
                "PostToolUse",
                cx(
                    "PostToolUse",
                    json!({ "turn_id": "t1", "tool_name": "apply_patch", "tool_use_id": "c1", "tool_input": { "command": patch }, "tool_response": "Success. Updated the following files:\nM schema/invoice.json" }),
                ),
            ),
            (
                "Stop",
                cx("Stop", json!({ "turn_id": "t1", "stop_hook_active": false, "last_assistant_message": "Done." })),
            ),
            ("SessionEnd", cx("SessionEnd", json!({ "reason": "other" }))),
        ],
    );
    assert_instinct_text(&t);
    assert!(t.contains("Makefile declares this."), "{t}");
}

#[test]
fn doctor_review_and_pack_approval() {
    let w = World::new().with_pnpm();
    let out = w.bind();
    assert!(out.contains("wait for review"), "{out}");
    // doctor counts what is bound per repository.
    let (d, _, _) = w.run(&["doctor", "--json"], b"");
    let v: Value = serde_json::from_str(&d).unwrap();
    let c = v["checks"].as_array().unwrap().iter().find(|c| c["check"] == "instincts").expect("an instincts check");
    let detail = c["detail"].as_str().unwrap();
    assert!(
        detail.starts_with(&format!("on; {KEY}: "))
            && detail.contains(" bound, ")
            && detail.contains("4 awaiting approval"),
        "{detail}"
    );
    // review lists instinct routes with their origin; environment steps wait for approval.
    let q = w.ok(&["review", "--json"]);
    let q: Value = serde_json::from_str(&q).unwrap();
    let rows = q["queue"].as_array().unwrap();
    let inst: Vec<&Value> = rows.iter().filter(|r| r["origin"]["kind"] == "instinct").collect();
    assert!(!inst.is_empty(), "{q:#}");
    assert!(inst.iter().all(|r| r["track"] == TRACK && r["strength"] == "prime"), "{q:#}");
    assert!(inst.iter().all(|r| !r["decisions"].as_array().unwrap().iter().any(|d| d == "strengthen")));
    let nascent: Vec<&str> =
        inst.iter().filter(|r| r["state"] == "nascent").map(|r| r["route"].as_str().unwrap()).collect();
    // `npm ci` (no package-lock.json) and `yarn install` (missing, or refused by corepack) →
    // `pnpm install`; `npm install` refuses nothing here, so it binds nothing.
    assert_eq!(nascent.len(), 4, "npm ci and yarn install → pnpm install: {q:#}");
    let text = w.ok(&["review"]);
    assert!(text.contains("instinct U1 (from Makefile)"), "{text}");
    // Approving one environment step approves the pack's for this repository.
    let out = w.ok(&["review", "approve", TRACK, nascent[0]]);
    assert!(out.contains(&format!("approved    pack core's environment steps for {KEY}")), "{out}");
    let yaml = std::fs::read_to_string(w.p(&format!("adage/tracks/{TRACK}/review.yaml"))).unwrap();
    assert!(yaml.contains("packs:") && yaml.contains("core:"), "{yaml}");
    // Re-binding keeps both in service.
    w.bind();
    let idx: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/index.json")).unwrap()).unwrap();
    let install =
        idx["routes"].as_array().unwrap().iter().filter(|r| r["route"]["step"]["command"] == "pnpm install").count();
    assert_eq!(install, 4, "every environment step is delivered after pack approval");
    // An extinguished instinct stays extinguished across re-binds.
    let out = w.ok(&["review", "extinguish", TRACK, "finish-line-e7e809"]);
    assert!(out.lines().any(|line| line.contains("lifecycle") && line.ends_with("extinguished")), "{out}");
    w.bind();
    let idx = std::fs::read_to_string(w.p("adage/index.json")).unwrap();
    assert!(!idx.contains("finish-line-e7e809"), "an extinguish decision holds");
}

#[test]
fn followed_and_failed_instincts_extinguish_at_the_next_learn() {
    let w = World::new();
    w.bind();
    let cwd = w.repo();
    // Three sessions follow the instinct's `make test` after the unittest failure, and it cannot
    // run (the target is gone): misfires. A test run that fails its tests is followed, not misfired.
    for i in 0..3 {
        let base = |ev: &str, extra: Value| {
            let mut b = json!({ "session_id": format!("s-ff{i}"), "transcript_path": "/dev/null", "cwd": cwd, "hook_event_name": ev });
            for (k, v) in extra.as_object().unwrap() {
                b[k] = v.clone();
            }
            b
        };
        w.hook("claude", "UserPromptSubmit", &base("UserPromptSubmit", json!({ "prompt": "Fix it" })));
        let out = w.hook(
            "claude",
            "PostToolUseFailure",
            &base(
                "PostToolUseFailure",
                json!({ "tool_name": "Bash", "tool_input": { "command": "python3 -m unittest" }, "error": "RuntimeError: ACME_ENV is not set" }),
            ),
        );
        assert!(out.contains("make test"), "{out}");
        w.hook(
            "claude",
            "PostToolUseFailure",
            &base(
                "PostToolUseFailure",
                json!({ "tool_name": "Bash", "tool_input": { "command": "make test" }, "error": "make: *** No rule to make target 'test'.  Stop." }),
            ),
        );
    }
    let spools: String = std::fs::read_dir(w.p("adage/sessions"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "spool"))
        .map(|e| String::from_utf8_lossy(&std::fs::read(e.path()).unwrap()).to_string())
        .collect();
    assert_eq!(spools.matches("misfired").count(), 3, "{spools}");
    let out = w.bind();
    assert!(out.contains("1 extinguished after failing"), "{out}");
    assert!(out.contains("extinguished automatically: agents followed it and it failed"), "{out}");
    let yaml = std::fs::read_to_string(w.p(&format!("adage/tracks/{TRACK}/review.yaml"))).unwrap();
    assert!(yaml.contains("extinguished") && yaml.contains("adage: misfired 3 times"), "{yaml}");
    let log = std::fs::read_to_string(w.p("adage/learn.log")).unwrap();
    assert!(log.lines().last().unwrap().contains("\"change\":\"extinguished\""), "{log}");
    // It stays extinguished: the next bind does not bring it back.
    w.bind();
    let idx: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/index.json")).unwrap()).unwrap();
    let extinguished = idx["routes"].as_array().unwrap().iter().any(|r| r["route"]["id"] == "dead-end-d41b1e");
    assert!(!extinguished, "the extinguished route is not delivered");
}
