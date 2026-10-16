//! A deterministic product proof, not a model/token benchmark: every rehearsal command really
//! runs, native traces enter through the canonical corpus CLI, and a learned (never authored)
//! reflex reaches a fresh task through the real hook after the normal review gate.
#![cfg(unix)]

use adage_tracks::{io, Index, RouteState, Step, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const REPO: &str = "fixture-dependencies";
const INITIAL: &str = "base-library==1.0\n";
const HELD_OUT: &str = "base-library==1.0\nheld-out-library==7.3\n";

fn checkout(path: &Path) {
    std::fs::create_dir_all(path.join(".git")).unwrap();
    std::fs::write(path.join("requirements.txt"), INITIAL).unwrap();
    std::fs::write(path.join("requirements.lock"), INITIAL).unwrap();
    std::fs::write(path.join("README.md"), "Offline fixture dependencies.\n").unwrap();
    std::fs::write(
        path.join("Makefile"),
        ".PHONY: deps check\ndeps:\n\t@cp requirements.txt requirements.lock\ncheck:\n\t@cmp -s requirements.txt requirements.lock || (echo 'stale requirements.lock'; exit 1)\n\t@echo 'environment coherent'\n",
    )
    .unwrap();
}

fn command(cwd: &Path, text: &str) -> Output {
    Command::new("sh").args(["-c", text]).current_dir(cwd).output().unwrap()
}

fn graded(cwd: &Path, expected: &str) -> bool {
    // Independent of the command's exit status and printed success: inspect the task's required
    // artifacts directly, including the new dependency that no training task contained.
    ["requirements.txt", "requirements.lock"]
        .iter()
        .all(|file| std::fs::read_to_string(cwd.join(file)).is_ok_and(|v| v == expected))
}

struct NativeTrace {
    sid: String,
    cwd: PathBuf,
    rows: Vec<Value>,
    calls: usize,
}

impl NativeTrace {
    fn new(sid: String, cwd: PathBuf, prompt: String) -> Self {
        let rows = vec![json!({"type":"user","sessionId":sid,"cwd":cwd,
            "timestamp":"2026-10-04T10:00:00Z","message":{"role":"user","content":prompt}})];
        Self { sid, cwd, rows, calls: 0 }
    }

    fn call(&mut self, tool: &str, input: Value, output: &str, failed: bool) {
        self.calls += 1;
        let id = format!("call-{}", self.calls);
        self.rows.push(json!({"type":"assistant","sessionId":self.sid,"cwd":self.cwd,
            "message":{"id":format!("message-{}",self.calls),"role":"assistant","model":"scripted-rehearsal",
                "content":[{"type":"tool_use","id":id,"name":tool,"input":input}]}}));
        self.rows.push(json!({"type":"user","sessionId":self.sid,"cwd":self.cwd,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,
                "content":output,"is_error":failed}]}}));
    }

    fn edit(&mut self, file: &str, replacement: &str) {
        let path = self.cwd.join(file);
        let before = std::fs::read_to_string(&path).unwrap();
        self.call("Read", json!({"file_path":path}), &before, false);
        std::fs::write(&path, replacement).unwrap();
        self.call(
            "Edit",
            json!({"file_path":path,"old_string":before,"new_string":replacement}),
            "File updated successfully.",
            false,
        );
    }

    fn bash(&mut self, text: &str) {
        let out = command(&self.cwd, text);
        let output = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        self.call("Bash", json!({"command":text}), &output, !out.status.success());
        assert!(out.status.success(), "rehearsal command {text}: {output}");
    }

    fn write(&self, path: &Path) {
        std::fs::write(path, self.rows.iter().map(|row| format!("{row}\n")).collect::<String>()).unwrap();
    }
}

struct World {
    root: tempfile::TempDir,
    home: PathBuf,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        Self { root, home }
    }

    fn adage(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.env("ADAGE_HOME", &self.home)
            .env("HOME", &self.home)
            .env("ADAGE_AUTO_LEARN", "0")
            .env("ADAGE_ARM", "treated")
            .env("ADAGE_EMBED_MODEL", "none")
            .env("ADAGE_COLOR", "never")
            .env_remove("ADAGE_DISABLE")
            .current_dir(self.root.path());
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.adage().args(args).output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        json_output(self.run(args))
    }

    fn hook(&self, cwd: &Path, sid: &str, event: &str, extra: Value, disabled: bool) -> String {
        let mut payload = json!({"session_id":sid,"cwd":cwd,"hook_event_name":event});
        payload.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let mut cmd = self.adage();
        cmd.args(["hook", "claude", event]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if disabled {
            cmd.env("ADAGE_DISABLE", "1");
        }
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success() && out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    fn edit_hook(&self, cwd: &Path, sid: &str, file: &str, disabled: bool) -> String {
        self.hook(
            cwd,
            sid,
            "PostToolUse",
            json!({"tool_name":"Edit","tool_input":{"file_path":cwd.join(file)},"tool_response":"Updated."}),
            disabled,
        )
    }
}

fn json_output(out: Output) -> Value {
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn native_evidence_becomes_a_reviewed_reflex_that_repairs_a_fresh_held_out_environment() {
    let w = World::new();
    let mut identities = BTreeMap::new();
    let mut outcomes = BTreeMap::new();
    let mut raw = Vec::new();
    for i in 0..16 {
        let cwd = w.root.path().join(format!("training-{i}")).join(REPO);
        checkout(&cwd);
        let sid = format!("training-session-{i}");
        let expected = if i < 8 { format!("{INITIAL}training-library-{i}==1.{i}\n") } else { INITIAL.into() };
        let prompt = if i < 8 {
            format!("Pin training-library-{i} to 1.{i} in requirements.txt")
        } else {
            format!("Document workflow {i} in README.md")
        };
        let mut trace = NativeTrace::new(sid.clone(), cwd.clone(), prompt);
        if i < 8 {
            trace.edit("requirements.txt", &expected);
            trace.bash("make deps");
        } else {
            trace.edit("README.md", &format!("Workflow {i} documented.\n"));
        }
        trace.bash("make check");
        let passed = graded(&cwd, &expected)
            && (i < 8
                || std::fs::read_to_string(cwd.join("README.md")).unwrap() == format!("Workflow {i} documented.\n"));
        assert!(passed, "the independent rehearsal grader must pass");
        identities.insert(sid.clone(), json!({"task_id":format!("training-task-{i}"),"attempt_id":"1"}));
        outcomes.insert(sid.clone(), passed);
        let path = w.root.path().join(format!("{sid}.jsonl"));
        trace.write(&path);
        raw.push(path);
    }
    let identities_path = w.root.path().join("identities.json");
    let outcomes_path = w.root.path().join("outcomes.json");
    std::fs::write(&identities_path, serde_json::to_vec(&identities).unwrap()).unwrap();
    std::fs::write(&outcomes_path, serde_json::to_vec(&outcomes).unwrap()).unwrap();
    let exported = w
        .adage()
        .args(["corpus", "export"])
        .args(&raw)
        .args(["--repo", REPO, "--identities"])
        .arg(&identities_path)
        .arg("--outcomes")
        .arg(&outcomes_path)
        .output()
        .unwrap();
    assert!(exported.status.success(), "{}", String::from_utf8_lossy(&exported.stderr));
    let canonical = w.root.path().join("canonical.jsonl");
    std::fs::write(&canonical, &exported.stdout).unwrap();
    assert_eq!(String::from_utf8(exported.stdout).unwrap().lines().count(), 16);
    let import = ["corpus", "import", canonical.to_str().unwrap(), "--repo", REPO];
    let first = w.json(&import);
    assert_eq!(first["added"], 16);
    let duplicate = w.json(&import);
    assert_eq!(duplicate["duplicates"], 16);
    assert_eq!(duplicate["generation"], first["generation"]);

    let learned = w.json(&["corpus", "learn", "--repo", REPO]);
    assert_eq!(learned["sessions_used"], 16);
    let audit = w.json(&["corpus", "inspect", "--repo", REPO, "--evidence"]);
    assert_eq!(audit["training_declared_identity"], 16);
    assert_eq!(audit["recorded_outcomes"], 16);
    assert_eq!(audit["training_groups"].as_array().unwrap().len(), 16);
    assert_eq!(audit["training_inferred_identity"], 0);
    assert_eq!(audit["proxy_outcomes"], 0);

    let tracks = io::read_all(&w.home.join("tracks")).unwrap();
    let (track, reflex) = tracks
        .iter()
        .flat_map(|t| t.routes.iter().map(move |r| (t, r)))
        .find(|(_, r)| {
            r.trigger == Trigger::AfterEdit { paths: "requirements.txt".into() }
                && matches!(&r.step, Step::Sequence { commands } if commands == &["make deps", "make check"])
        })
        .unwrap_or_else(|| panic!("no learned dependency sequence: {tracks:#?}"));
    assert!(!reflex.authored);
    assert_eq!(reflex.state, RouteState::Nascent, "environment mutation requires the normal human review");
    assert!(!Index::load(&w.home.join("index.json")).routes.iter().any(|r| r.route.id == reflex.id));

    let held = w.root.path().join("held-out").join(REPO);
    checkout(&held);
    std::fs::write(held.join("requirements.txt"), HELD_OUT).unwrap();
    assert!(!graded(&held, HELD_OUT), "the task starts with an independently observable stale lockfile");
    assert!(!command(&held, "make check").status.success());
    assert!(w.edit_hook(&held, "before-review", "requirements.txt", false).is_empty());

    let approved = w
        .adage()
        .args(["review", "approve", &track.manifest.track.name, &reflex.id, "--by", "fixture-reviewer"])
        .env_remove("CLAUDECODE")
        .env_remove("OPENCODE")
        .env_remove("CODEX_SANDBOX")
        .env_remove("CODEX_SANDBOX_NETWORK_DISABLED")
        .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(approved.status.success(), "{}", String::from_utf8_lossy(&approved.stderr));
    assert!(Index::load(&w.home.join("index.json")).routes.iter().any(|r| r.route.id == reflex.id));
    let artifact = w.json(&[
        "eval",
        "experiment",
        "artifact",
        w.home.join("index.json").to_str().unwrap(),
        "--repo",
        REPO,
        "--checkout",
        held.to_str().unwrap(),
    ]);
    assert_eq!(artifact["runtime_repo"], REPO);
    assert!(artifact["directive_reflexes"].as_u64().unwrap() > 0);

    assert!(w.edit_hook(&held, "disabled", "requirements.txt", true).is_empty());
    assert!(w.edit_hook(&held, "unrelated", "README.md", false).is_empty());
    assert!(!graded(&held, HELD_OUT), "controls must not change the checkout");
    let output = w.edit_hook(&held, "held-out-new-task", "requirements.txt", false);
    let payload: Value = serde_json::from_str(&output).unwrap();
    let context = payload["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(context.contains(&format!("route=\"{}\"", reflex.id)), "{context}");
    let delivered: Vec<&str> =
        context.lines().filter_map(|line| line.trim().split_once(". ")).map(|(_, c)| c).collect();
    assert_eq!(delivered, ["make deps", "make check"], "the actual hook supplies the ordered continuation");
    assert!(!graded(&held, HELD_OUT), "advisory hooks never secretly execute the step");
    for cmd in delivered {
        let result = command(&held, cmd);
        assert!(result.status.success(), "delivered command {cmd}: {}", String::from_utf8_lossy(&result.stderr));
        w.hook(
            &held,
            "held-out-new-task",
            "PostToolUse",
            json!({"tool_name":"Bash","tool_input":{"command":cmd},"tool_response":String::from_utf8_lossy(&result.stdout)}),
            false,
        );
    }
    assert!(graded(&held, HELD_OUT), "the learned continuation solves the previously unseen dependency task");
    assert!(w.edit_hook(&held, "held-out-new-task", "requirements.txt", false).is_empty(), "no duplicate advice");
    assert!(w.hook(&held, "held-out-new-task", "Stop", json!({"stop_hook_active":false}), false).is_empty());

    let index = std::fs::read(w.home.join("index.json")).unwrap();
    let reviewed = w.json(&["review", "--json"]);
    w.json(&["corpus", "learn", "--repo", REPO]);
    assert_eq!(std::fs::read(w.home.join("index.json")).unwrap(), index, "identical evidence preserves compilation");
    assert_eq!(w.json(&["review", "--json"]), reviewed, "identical evidence preserves human review");
    assert_eq!(
        w.json(&["corpus", "inspect", "--repo", REPO, "--evidence"]),
        audit,
        "held-out work never enters training"
    );
    eprintln!(
        "learning_delivery_proof={}",
        json!({
            "evidence_kind":"deterministic_local_rehearsal",
            "native_sessions":16,
            "declared_training_tasks":16,
            "independently_graded_training_outcomes":16,
            "duplicate_records_ignored":16,
            "learned_reflex_support":reflex.evidence.support,
            "authored_reflexes_inserted":0,
            "review_gate_checked":true,
            "compiled_artifact_and_repository_verified":true,
            "held_out_tasks":1,
            "held_out_initial_check_passed":false,
            "delivered_commands":["make deps","make check"],
            "held_out_final_independent_grade":true,
            "disabled_and_unrelated_controls_abstained":true,
            "duplicate_hook_delivery_suppressed":true,
            "relearning_preserved_index_and_review":true,
            "held_out_evidence_excluded_from_training":true,
            "model_calls":0,
            "token_savings_measured":false
        })
    );
}
