//! A session whose state file is corrupted (both slots fail their CRC) carries on from its spool:
//! a dead end already announced is not announced again, and a finish-line obligation made before
//! the corruption is still enforced at Stop. Without a spool, the state starts over.

use adage_core::Effect;
use adage_tracks::*;
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn route(id: &str, trigger: Trigger, step: Step, effect: Effect, aspect: Aspect) -> Route {
    Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step,
        slots: vec![],
        effect,
        aspect,
        state: RouteState::Active,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 5, ..Default::default() },
        authored: false,
    }
}

struct World {
    _d: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
}

fn world() -> World {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let (home, repo) = (root.join("home"), root.join("acme-state"));
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("schema")).unwrap();
    let track = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme-state/t".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme-state".into() },
            gauge: Gauge::default(),
        },
        routes: vec![
            route(
                "dead-end-x",
                Trigger::SessionStart,
                Step::Avoid {
                    avoid: "pytest".into(),
                    instead: "make test".into(),
                    signature: "No module named pytest".into(),
                },
                Effect::Verify,
                Aspect::Red,
            ),
            route(
                "finish-line-x",
                Trigger::AfterEdit { paths: "schema/*.json".into() },
                Step::Run { command: "make gen".into() },
                Effect::Regenerate,
                Aspect::Green,
            ),
        ],
    };
    io::validate(&track).unwrap();
    Index::compile(&[track]).write_atomic(&home.join("index.json")).unwrap();
    World { _d: d, home, repo }
}

fn hook(w: &World, event: &str, extra: serde_json::Value) -> String {
    let mut payload = json!({ "session_id": "s-1", "cwd": w.repo });
    payload.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(["hook", "claude", event])
        .env("ADAGE_HOME", &w.home)
        .env("ADAGE_ARM", "treated")
        .env_remove("ADAGE_DISABLE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success() && o.stderr.is_empty());
    String::from_utf8(o.stdout).unwrap()
}

/// Overwrite both slots' payloads (64 KiB slots, 20-byte headers): neither CRC holds.
fn corrupt(state: &Path) {
    let mut bytes = std::fs::read(state).unwrap();
    for at in [20, 64 * 1024 + 20] {
        if bytes.len() > at + 4 {
            bytes[at..at + 4].copy_from_slice(b"XXXX");
        }
    }
    std::fs::write(state, bytes).unwrap();
}

fn session_up_to_the_edit(w: &World) {
    assert!(hook(w, "UserPromptSubmit", json!({ "prompt": "Fix the invoice." })).contains("dead-end-x"));
    let edit = json!({ "tool_calls": [{ "tool_name": "Edit", "tool_input": { "file_path": w.repo.join("schema/invoice.json") }, "tool_response": "ok" }] });
    assert!(hook(w, "PostToolBatch", edit).contains("finish-line-x"));
    // Two more writes, so both slots hold a state.
    hook(
        w,
        "PostToolBatch",
        json!({ "tool_calls": [{ "tool_name": "Bash", "tool_input": { "command": "ls" }, "tool_response": "x" }] }),
    );
}

#[test]
fn a_corrupt_state_is_rebuilt_from_the_spool() {
    let w = world();
    session_up_to_the_edit(&w);
    let state = w.home.join("sessions/s-1.state");
    corrupt(&state);
    // The obligation survives: Stop asks for `make gen` ...
    let stop = hook(&w, "Stop", json!({ "stop_hook_active": false }));
    assert!(stop.contains("\"block\"") && stop.contains("make gen"), "{stop}");
    // ... once, as before the corruption ...
    assert_eq!(hook(&w, "Stop", json!({ "stop_hook_active": false })), "");
    // ... and the dead end, announced at the first prompt, is not announced again.
    assert_eq!(hook(&w, "UserPromptSubmit", json!({ "prompt": "Also the totals." })), "");
    let st: adage_runtime::SessionState = adage_store::state::read(&state).unwrap();
    assert_eq!((st.prompts, st.batches, st.stop_requests), (2, 2, 1));
    assert_eq!(st.emitted, vec!["dead-end-x", "finish-line-x"]);
}

#[test]
fn without_a_spool_a_corrupt_state_starts_over() {
    let w = world();
    session_up_to_the_edit(&w);
    corrupt(&w.home.join("sessions/s-1.state"));
    std::fs::remove_file(w.home.join("sessions/s-1.spool")).unwrap();
    assert_eq!(hook(&w, "Stop", json!({ "stop_hook_active": false })), "", "no obligation is known");
    assert!(hook(&w, "UserPromptSubmit", json!({ "prompt": "Again." })).contains("dead-end-x"), "a new session");
}
