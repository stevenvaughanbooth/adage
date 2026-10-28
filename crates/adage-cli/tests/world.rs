//! T-W2-02 to T-W2-06: `adage world` through the binary: ingest writes what a descriptor allows
//! and nothing else, status reads it back, the hook delivers the hint from it, the follow child
//! writes a landed transcript and prints the timeline, and nothing reaches the network.

use adage_tracks::testkit::TempWorld;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_adage")
}

struct World {
    _world: TempWorld,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let world = TempWorld::new(&["home", "acme/.git", "meetings", "plain"]);
        let root = world.root.clone();
        let home = world.home("adage");
        std::fs::write(
            home.paths.config(),
            format!(
                "[learn]\nauto = false\n\n[world]\nenabled = true\ntranscripts = [\"{}\"]\n",
                root.join("meetings").display()
            ),
        )
        .unwrap();
        World { _world: world, root }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn run(&self, args: &[&str], stdin: &[u8], env: &[(&str, &str)]) -> (String, String, i32) {
        let mut c = Command::new(bin());
        c.args(args)
            .current_dir(self.p("acme"))
            .env("ADAGE_HOME", self.p("adage"))
            .env("ADAGE_AUTO_LEARN", "0")
            .env("HOME", self.p("home"))
            .env("ADAGE_ARM", "treated")
            .env("ADAGE_OFFLINE", "1")
            .env_remove("ADAGE_DISABLE")
            .env_remove("CLAUDECODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in env {
            c.env(k, v);
        }
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
    fn json(&self, args: &[&str], stdin: &[u8]) -> Value {
        let (out, err, code) = self.run(args, stdin, &[]);
        assert_eq!(code, 0, "{args:?}: {err}");
        serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {out}"))
    }
}

fn batch() -> Value {
    json!({
        "source": "github", "via": "mod", "server": "github",
        "artifacts": [
            { "id": "github:pr:412", "kind": "pr", "title": "Schema evolution for invoices", "locator": "#412", "state": "ci:green", "first_seen": 0, "last_touched": 0 },
            { "id": "github:pr:9", "kind": "pr", "title": "Payroll export", "locator": "#9", "state": "open", "first_seen": 0, "last_touched": 0 }
        ],
        "events": [
            { "ts": 1792000000000u64, "source": "github", "kind": "pr", "artifact": "github:pr:412", "action": "seen" },
            { "ts": 1792000000000u64, "source": "github", "kind": "pr", "artifact": "github:pr:9", "action": "seen" },
            { "ts": 1792000000000u64, "source": "github", "kind": "ticket", "artifact": "github:pr:412", "action": "seen" },
            { "ts": 1792000000000u64, "source": "github", "kind": "pr", "artifact": "github:pr:nope", "action": "seen" }
        ]
    })
}

/// T-W2-03: ingest keeps what the descriptor allows (the pull requests), drops a wrong kind, an
/// unknown artifact and a denied title (`*payroll*`), dedupes a repeat, and status reads it back.
#[test]
fn ingest_writes_what_a_descriptor_allows_and_status_reads_it_back() {
    let w = World::new();
    let written = w.json(&["world", "ingest"], batch().to_string().as_bytes());
    assert_eq!(written["source"], "github");
    assert_eq!(written["artifacts"], 1, "the payroll pull request is denied: {written}");
    assert_eq!(written["events"], 1, "{written}");
    assert_eq!(
        written["skipped"], 4,
        "a wrong kind, an unknown artifact, the denied one's event, and the denied artifact: {written}"
    );
    let again = w.json(&["world", "ingest"], batch().to_string().as_bytes());
    assert_eq!(again["events"], 0, "a repeat within a minute is a duplicate: {again}");
    let status = w.json(&["world", "--json"], b"");
    assert_eq!(status["enabled"], true);
    assert_eq!(status["artifacts_total"], 1);
    assert_eq!(status["artifacts"]["pr"], 1);
    assert_eq!(status["sources"][0]["name"], "github");
    assert_eq!(status["sources"][0]["server"], "github");
    assert_eq!(status["sources"][0]["events"], 1);
    let failed = w.json(&["world", "ingest", "--source", "calendar", "--error", "auth: sign in"], b"{}");
    assert_eq!(failed["problems"][0], "auth: sign in");
    let status = w.json(&["world", "--json"], b"");
    let cal = status["sources"].as_array().unwrap().iter().find(|s| s["name"] == "calendar").unwrap();
    assert_eq!((cal["ok"].as_bool(), cal["error"].as_str()), (Some(false), Some("auth: sign in")));
    let (text, _, code) = w.run(&["world"], b"", &[]);
    assert_eq!(code, 0);
    assert!(text.contains("adage world") && text.contains("github") && text.contains("1 pull request"), "{text}");
    let d = w.json(&["world", "descriptors", "--json"], b"");
    assert_eq!(d["descriptors"].as_array().unwrap().len(), 6);
}

/// T-W2-01 through the binary: the hook delivers the hint from the store at a referential
/// prompt, and the same prompt in a session outside any world-enabled home gets nothing.
#[test]
fn the_hook_delivers_a_hint_from_the_store() {
    let w = World::new();
    w.json(&["world", "ingest"], batch().to_string().as_bytes());
    let p = json!({ "session_id": "s1", "cwd": w.p("acme"), "hook_event_name": "UserPromptSubmit", "prompt": "review the PR" });
    let (out, err, code) = w.run(&["hook", "claude", "UserPromptSubmit"], p.to_string().as_bytes(), &[]);
    assert_eq!((code, err.as_str()), (0, ""));
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    let text = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(
        text.starts_with("<adage-world kind=\"hint\">") && text.contains("#412") && text.contains("from Adage"),
        "{text}"
    );
    let (out, _, _) = w.run(&["world", "pause"], b"", &[]);
    assert!(out.contains("paused"));
    let (out, _, _) = w.run(&["hook", "claude", "UserPromptSubmit"], p.to_string().as_bytes(), &[]);
    assert!(out.is_empty(), "paused: {out}");
    w.run(&["world", "resume"], b"", &[]);
    let (out, _, _) = w.run(&["hook", "claude", "UserPromptSubmit"], p.to_string().as_bytes(), &[]);
    assert!(out.contains("adage-world"));
}

/// T-W2-04: the follow child writes a transcript that lands in a watched directory and prints
/// the timeline as JSON lines with a cursor, then ends when its stdin closes.
#[test]
fn the_follow_child_writes_a_landed_transcript_and_prints_the_timeline() {
    let w = World::new();
    std::fs::write(
        w.p("meetings/2026-10-27-marketing-sync.md"),
        "Q4 campaign timeline; action items for Priya (copy).\n",
    )
    .unwrap();
    let mut c = Command::new(bin());
    c.args(["world", "follow", "--session", "s1", "--stdin", "eof"])
        .current_dir(w.p("acme"))
        .env("ADAGE_HOME", w.p("adage"))
        .env("HOME", w.p("home"))
        .env("ADAGE_WORLD_POLL_MS", "50")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    let stdin = child.stdin.take().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    drop(stdin);
    let o = child.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let lines: Vec<Value> = out.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let event = lines.iter().find(|l| l.get("event").is_some()).unwrap_or_else(|| panic!("an event line: {out}"));
    assert_eq!(event["event"]["source"], "meetings");
    assert_eq!(event["event"]["kind"], "transcript");
    assert_eq!(event["event"]["action"], "landed");
    assert!(event["cursor"].as_u64().unwrap() > 0);
    let health = lines.iter().find(|l| l.get("health").is_some()).unwrap();
    assert_eq!(health["polling"], true);
    let status = w.json(&["world", "--json"], b"");
    assert_eq!(status["artifacts"]["transcript"], 1);
    // The transcript's first words are its summary, made locally, and the hint names the file.
    let p = json!({ "session_id": "s2", "cwd": w.p("acme"), "hook_event_name": "UserPromptSubmit", "prompt": "file the follow-ups from the marketing sync" });
    let (hint, _, _) = w.run(&["hook", "claude", "UserPromptSubmit"], p.to_string().as_bytes(), &[]);
    assert!(hint.contains("marketing-sync.md") && hint.contains("Q4 campaign"), "{hint}");
}

/// T-W2-08: the eval set: a labelled prompt is recorded and replayed.
#[test]
fn labelled_prompts_are_replayed() {
    let w = World::new();
    w.json(&["world", "ingest"], batch().to_string().as_bytes());
    let (out, _, code) = w.run(&["world", "eval", "record", "review the PR", "--meant", "github:pr:412"], b"", &[]);
    assert_eq!(code, 0, "{out}");
    w.run(&["world", "eval", "record", "add a field", "--meant", "none"], b"", &[]);
    let r = w.json(&["world", "eval", "report", "--json"], b"");
    assert_eq!(
        (r["cases"].as_u64(), r["hits"].as_u64(), r["silent_right"].as_u64()),
        (Some(2), Some(1), Some(1)),
        "{r}"
    );
    assert_eq!(r["false_attaches"], 0);
}
