//! OpenCode import over synthetic `opencode export` files written from the documented format
//! (opencode.ai/docs/cli `export`; SDK `Session`, `Message`, `Part` types). Golden files in
//! `tests/golden/`; regenerate with `ADAGE_BLESS=1` and review the diff.

use adage_core::CommandTable;
use adage_learn::opencode::{parse_export, read_export};
use adage_learn::{learn, Inputs, LearnConfig};
use adage_tracks::{Aspect, Index, Step, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode-export.json")
}

fn golden(name: &str, got: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    if std::env::var("ADAGE_BLESS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
    }
    assert_eq!(
        got,
        std::fs::read_to_string(&path).expect("missing golden; run with ADAGE_BLESS=1"),
        "golden {name} changed"
    );
}

#[test]
fn export_fixture_golden() {
    let v = read_export(&fixture()).expect("recognised as an export");
    let s = parse_export(&v, &CommandTable::load(), None).expect("a usable session");
    let mut got = format!(
        "id {}\nrepo {}\nroot {}\nmodel {:?}\nday {}\nprompt {:?}\nsteered {}\ndirected {:?}\n",
        s.id,
        s.repo,
        s.root.display(),
        s.model,
        s.day,
        s.prompt,
        s.steered,
        s.directed
    );
    for st in &s.steps {
        got.push_str(&format!("turn {} {} failed={} path={:?}\n", st.turn, st.key, st.call.failed, st.rel_path));
    }
    golden("opencode-session.txt", &got);
    assert_eq!(s.prompt, "Pick up W-1040 and get it done.", "synthetic parts are never the prompt");
    assert!(s.directed.contains("lookup-work-item") && s.directed.contains("read-linked-files"));
    assert!(!got.contains("sleep 100"), "unfinished calls are skipped");
}

#[test]
fn claude_transcripts_and_non_exports_are_not_mistaken_for_exports() {
    let d = tempfile::tempdir().unwrap();
    let cases = [
        ("claude.jsonl", "{\"type\":\"user\",\"sessionId\":\"s\",\"info\":1}\n{\"type\":\"assistant\"}\n".to_string()),
        ("array.json", "[{\"info\":{}}]".to_string()),
        ("nomsgs.json", json!({ "info": { "id": "x" } }).to_string()),
        ("broken.json", "{\"info\": {\"id\": ".to_string()),
        ("empty.json", String::new()),
    ];
    for (name, body) in cases {
        let p = d.path().join(name);
        std::fs::write(&p, body).unwrap();
        assert!(read_export(&p).is_none(), "{name}");
    }
}

#[test]
fn subagents_promptless_and_hostile_exports_yield_nothing_or_stay_bounded() {
    let t = CommandTable::load();
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(fixture()).unwrap()).unwrap();
    v["info"]["parentID"] = json!("ses_parent");
    assert!(parse_export(&v, &t, None).is_none(), "sub-agent sessions are skipped");
    let promptless = json!({ "info": { "id": "s", "directory": "/x" }, "messages": [
        { "info": { "role": "user" }, "parts": [{ "type": "text", "text": "Adage finish-line check: run make gen", "synthetic": false }] }
    ] });
    assert!(parse_export(&promptless, &t, None).is_none(), "Adage's own text is never a prompt");
    let hostile = json!({ "info": { "id": 5, "directory": [] }, "messages": "no" });
    assert!(parse_export(&hostile, &t, None).is_none());
    let weird = json!({ "info": { "id": "s", "directory": "/x", "time": { "created": "soon" } }, "messages": [
        { "info": { "role": "user" }, "parts": [{ "type": "text", "text": "do W-1" }] },
        { "info": { "role": "assistant" }, "parts": [
            { "type": "tool", "tool": 7, "state": { "status": "completed", "input": [], "output": {} } },
            { "type": "tool", "tool": "apply_patch", "state": { "status": "completed", "input": { "patchText": "*** Add File: ".repeat(10_000) } } },
            { "type": "tool", "tool": "bash", "state": null }
        ] }
    ] });
    let s = parse_export(&weird, &t, None).expect("parses what it can");
    assert!(s.steps.len() <= 2 + 64);
}

/// The same corpus the Claude Code golden test learns from, as OpenCode exports: the same four
/// route kinds must come out.
fn export(sid: &str, id: &str, schema: bool, i: usize) -> Value {
    let cwd = "/fixture/acme-billing";
    let tool = |n: usize, tool: &str, input: Value, output: &str, exit: i64| json!({ "type": "tool", "callID": format!("c{n}"), "tool": tool, "state": { "status": "completed", "input": input, "output": output, "title": "", "metadata": { "exit": exit }, "time": { "start": 0, "end": 1 } } });
    let turn = |parts: Vec<Value>| json!({ "info": { "role": "assistant", "modelID": "gpt-test", "path": { "cwd": cwd, "root": cwd } }, "parts": parts });
    let runner = ["python3 -m unittest discover -s tests", "python3 -m pytest tests", "python -m pytest -v"][i % 3];
    let edit = if schema {
        tool(
            4,
            "apply_patch",
            json!({ "patchText": "*** Begin Patch\n*** Update File: schema/invoice.json\n*** End Patch" }),
            "Success",
            0,
        )
    } else {
        tool(
            4,
            "edit",
            json!({ "filePath": format!("{cwd}/acme/billing.py"), "oldString": "a", "newString": "b" }),
            "ok",
            0,
        )
    };
    let mut msgs = vec![
        json!({ "info": { "role": "user" }, "parts": [{ "type": "text", "text": format!("Pick up {id} and get it done.") }] }),
        turn(vec![tool(1, "bash", json!({ "command": "ls" }), "README.md\nMakefile\nacme\nscripts", 0)]),
        turn(vec![tool(
            2,
            "bash",
            json!({ "command": format!("python3 scripts/wi-lookup {id}") }),
            &format!("ID: {id}\nLinked files: acme/billing.py\nAcceptance: ok"),
            0,
        )]),
        turn(vec![tool(3, "read", json!({ "filePath": format!("{cwd}/acme/billing.py") }), "code", 0)]),
        turn(vec![edit]),
    ];
    if schema {
        msgs.push(turn(vec![tool(5, "bash", json!({ "command": "make gen" }), "generated", 0)]));
    }
    msgs.push(turn(vec![tool(
        6,
        "bash",
        json!({ "command": runner }),
        "Traceback\nRuntimeError: ACME_ENV is not set — run tests with 'make test'",
        1,
    )]));
    msgs.push(turn(vec![tool(7, "bash", json!({ "command": "make test" }), "OK", 0)]));
    json!({ "info": { "id": sid, "directory": cwd, "time": { "created": 1790000000000_i64 + i as i64 * 86_400_000 } }, "messages": msgs })
}

#[test]
fn learns_all_four_route_kinds_from_opencode_exports() {
    let d = tempfile::tempdir().unwrap();
    let mut files = vec![];
    for i in 0..6 {
        let p = d.path().join(format!("ses_{i}.json"));
        std::fs::write(
            &p,
            serde_json::to_string_pretty(&export(&format!("ses_{i}"), &format!("W-10{:02}", 40 + i), i < 4, i))
                .unwrap(),
        )
        .unwrap();
        files.push(p);
    }
    let home = d.path().join("home");
    let inputs = Inputs {
        transcripts: files,
        outcomes: BTreeMap::new(),
        repo_override: Some("acme-billing".into()),
        models: None,
    };
    let rep = learn(&inputs, &home.join("tracks"), &home.join("index.json"), &LearnConfig::demo()).unwrap();
    assert_eq!((rep.sessions_read, rep.sessions_used), (6, 6));
    let index = Index::load(&home.join("index.json"));
    let routes: Vec<_> = index.routes.iter().map(|r| &r.route).collect();
    let lookup = routes.iter().find(|r| matches!(r.trigger, Trigger::Prompt { .. })).expect("prompt route");
    assert_eq!(lookup.step, Step::Run { command: "python3 scripts/wi-lookup {item}".into() });
    assert_eq!(lookup.aspect, Aspect::Green);
    assert!(routes.iter().any(|r| matches!(r.step, Step::ReadBatch { .. })), "linked-files route");
    assert!(
        routes.iter().any(|r| matches!(&r.step, Step::Avoid { instead, .. } if instead == "make test")),
        "dead-end route"
    );
    let fin = routes.iter().find(|r| matches!(r.trigger, Trigger::AfterEdit { .. })).expect("finish-line route");
    assert_eq!(fin.trigger, Trigger::AfterEdit { paths: "schema/*.json".into() });
    assert_eq!(fin.step, Step::Run { command: "make gen".into() });
    // The tracks record the harness the sessions came from, not Claude Code by default.
    for t in adage_tracks::io::read_all(&home.join("tracks")).unwrap() {
        assert_eq!(t.manifest.gauge.harness, vec!["opencode".to_string()], "{}", t.manifest.track.name);
    }
}
