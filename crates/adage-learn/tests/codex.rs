//! Codex import over the simulator's mock rollouts (`sim/fixtures/codex`, written by `adage-sim`
//! in the record shapes of openai/codex rust-v0.156.1, and the older `shell` shape) and over
//! hand-built edge cases. Golden files in `tests/golden/`; regenerate with `ADAGE_BLESS=1`.

use adage_core::CommandTable;
use adage_learn::codex::{discover, parse_rollout, read_rollout};
use adage_learn::import::Exposure as _;
use adage_learn::opencode::read_export;
use adage_learn::{learn, Inputs, LearnConfig};
use adage_sim::{codex as sim, scenario, CODEX_SEED};
use adage_tracks::{Index, SlotSource, Step, Strength, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn golden(name: &str, got: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    if std::env::var("ADAGE_BLESS").is_ok_and(|v| v == "1") {
        std::fs::write(&path, got).unwrap();
    }
    assert_eq!(
        got,
        std::fs::read_to_string(&path).expect("missing golden; run with ADAGE_BLESS=1"),
        "golden {name} changed"
    );
}

/// The committed rollout for a scenario session, and whether it is in the older `shell` shape.
fn fixtures() -> Vec<(PathBuf, bool, scenario::Session)> {
    scenario::history(CODEX_SEED)
        .into_iter()
        .map(|s| (adage_sim::fixtures_dir().join("codex").join(sim::rel_path(&s)), sim::is_legacy(&s), s))
        .collect()
}

fn describe(path: &Path) -> String {
    let lines = read_rollout(path).expect("recognised as a rollout");
    let s = parse_rollout(&lines, &CommandTable::load(), None).expect("a usable session");
    let mut got = format!(
        "file {}\nid {}\nrepo {}\nroot {}\nmodel {:?}\nday {}\nprompt {:?}\nsteered {}\n",
        path.file_name().unwrap().to_string_lossy(),
        s.id,
        s.repo,
        s.root.display(),
        s.model,
        s.day,
        s.prompt,
        s.steered
    );
    for st in &s.steps {
        got.push_str(&format!("turn {} {} failed={} path={:?}\n", st.turn, st.key, st.call.failed, st.rel_path));
    }
    got
}

#[test]
fn rollout_fixtures_golden() {
    let fx = fixtures();
    let current =
        fx.iter().find(|(_, legacy, s)| !legacy && s.name.contains("W-1045")).expect("a current-format schema session");
    let legacy =
        fx.iter().find(|(_, legacy, s)| *legacy && s.name.contains("W-")).expect("a legacy-format work-item session");
    let got = format!("{}\n{}", describe(&current.0), describe(&legacy.0));
    golden("codex-session.txt", &got);
    // What the golden must show, independent of blessing.
    assert!(got.contains("prompt \"") && !got.contains("environment_context"), "context messages are never the prompt");
    assert!(got.contains("failed=true"), "exit codes recorded in rollouts mark failures");
    assert!(got.contains("path=Some(\"schema/invoice.json\")"), "apply_patch edits name their files");
    assert!(got.contains("model Some(\"gpt-5.3-codex\")"));
}

#[test]
fn every_fixture_parses_with_its_prompt_and_calls() {
    let t = CommandTable::load();
    for (path, _, s) in fixtures() {
        let lines = read_rollout(&path).unwrap();
        let got = parse_rollout(&lines, &t, None).unwrap_or_else(|| panic!("{}", path.display()));
        assert_eq!(got.prompt, s.prompt, "{}", s.name);
        let calls: usize = s.turns.iter().map(|t| t.calls.len()).sum();
        let distinct: std::collections::BTreeSet<u32> = got.steps.iter().map(|x| x.turn).collect();
        assert_eq!(distinct.len(), s.turns.len(), "{}: one model turn per scenario turn", s.name);
        assert!(got.steps.len() >= calls, "{}: every call becomes at least one step", s.name);
        let failed = s.turns.iter().flat_map(|t| &t.calls).filter(|c| c.act.failed()).count();
        assert_eq!(got.steps.iter().filter(|x| x.call.failed).count(), failed, "{}: failures from exit codes", s.name);
    }
}

#[test]
fn learns_all_four_route_kinds_from_codex_rollouts() {
    let d = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = fixtures().into_iter().map(|f| f.0).collect();
    let inputs = Inputs {
        transcripts: files,
        outcomes: BTreeMap::new(),
        repo_override: None,
        models: None,
        cache: None,
        only_repo: None,
    };
    let rep = learn(&inputs, &d.path().join("tracks"), &d.path().join("index.json"), &LearnConfig::demo()).unwrap();
    assert_eq!((rep.sessions_read, rep.sessions_used), (12, 12));
    assert!(rep.repos.contains_key("acme-billing"), "the repository key is the checkout's directory name");
    let index = Index::load(&d.path().join("index.json"));
    let routes: Vec<_> = index.routes.iter().map(|r| &r.route).collect();
    let lookup = routes.iter().find(|r| matches!(r.trigger, Trigger::Prompt { .. })).expect("prompt route");
    assert_eq!(lookup.step, Step::Run { command: "python3 scripts/wi-lookup {item}".into() });
    assert_eq!(lookup.strength, Strength::Fire);
    let linked = routes
        .iter()
        .find(|r| matches!(r.step, Step::ReadBatch { .. }))
        .expect("linked-files route from shell reads (sed -n, cat)");
    assert!(matches!(&linked.slots[0].source, SlotSource::ResultField { field } if field == "*paths"));
    let dead = routes
        .iter()
        .find(|r| matches!(r.step, Step::Avoid { .. }))
        .expect("dead-end route: rollouts record exit codes");
    let Step::Avoid { avoid, instead, signature } = &dead.step else { unreachable!() };
    assert_eq!(instead, "make test");
    assert!(avoid.contains("python3 -m unittest") || avoid.contains("python3 -m pytest"), "{avoid}");
    assert!(signature.contains("ACME_ENV is not set") || signature.contains("No module named pytest"), "{signature}");
    assert_eq!(
        dead.trigger,
        Trigger::SessionStart,
        "announced at the first prompt, so it needs no failure signal at runtime"
    );
    let fin = routes.iter().find(|r| matches!(r.trigger, Trigger::AfterEdit { .. })).expect("finish-line route");
    assert_eq!(fin.trigger, Trigger::AfterEdit { paths: "schema/*.json".into() });
    assert_eq!(fin.step, Step::Run { command: "make gen".into() });
}

#[test]
fn other_formats_are_not_mistaken_for_rollouts_and_vice_versa() {
    for f in adage_sim::fixture_files("claude").into_iter().chain(adage_sim::fixture_files("opencode")) {
        assert!(read_rollout(&f).is_none(), "{}", f.display());
    }
    for (f, _, _) in fixtures() {
        assert!(read_export(&f).is_none(), "a rollout is not an OpenCode export: {}", f.display());
    }
    let d = tempfile::tempdir().unwrap();
    let cases = [
        ("empty.jsonl", String::new()),
        ("claude.jsonl", "{\"type\":\"user\",\"sessionId\":\"s\",\"payload\":{}}\n".to_string()),
        ("meta-no-ts.jsonl", "{\"type\":\"session_meta\",\"payload\":{\"id\":\"x\"}}\n".to_string()),
        ("huge.jsonl", "x".repeat(2 << 20)),
        ("binary.jsonl", String::from_utf8_lossy(&[0xff, 0xfe, 0, 1]).to_string()),
    ];
    for (name, body) in cases {
        let p = d.path().join(name);
        std::fs::write(&p, body).unwrap();
        assert!(read_rollout(&p).is_none(), "{name}");
    }
}

fn line(ty: &str, payload: Value) -> Value {
    json!({ "timestamp": "2026-09-20T10:00:00.000Z", "type": ty, "payload": payload })
}

fn meta(extra: Value) -> Value {
    let mut p = json!({ "id": "019a0000-0000-7000-a000-000000000001", "timestamp": "2026-09-20T10:00:00.000Z", "cwd": "/fixture/acme-billing", "originator": "codex_cli_rs", "cli_version": "0.156.1", "source": "cli" });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    line("session_meta", p)
}

fn user(text: &str) -> Value {
    line(
        "response_item",
        json!({ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": text }] }),
    )
}

fn exec(call: &str, cmd: &str) -> Value {
    line(
        "response_item",
        json!({ "type": "function_call", "name": "exec_command", "arguments": json!({ "cmd": cmd }).to_string(), "call_id": call }),
    )
}

fn out(call: &str, code: i32, body: &str) -> Value {
    line(
        "response_item",
        json!({ "type": "function_call_output", "call_id": call, "output": format!("Wall time: 0.1000 seconds\nProcess exited with code {code}\nOutput:\n{body}") }),
    )
}

/// F3: Codex turns a Stop `block` reason into the next user prompt. The finish-line gate's
/// failure is Adage's direction there, never the person's prompt, and the session is steered.
#[test]
fn a_stop_gate_failure_continuation_is_adages_text_not_a_prompt() {
    let t = CommandTable::load();
    let cmd = ".venv/bin/python -m pytest tests -q";
    let gate = adage_runtime::render::stop_gate_failed(cmd, Some(1), None);
    let lines = vec![
        meta(json!({})),
        user("Fix W-2042"),
        exec("c1", "ls"),
        out("c1", 0, "Makefile"),
        user(&gate),
        exec("c2", cmd),
        out("c2", 1, "1 failed"),
    ];
    let s = parse_rollout(&lines, &t, None).expect("parses");
    assert_eq!((s.prompt.as_str(), s.prompt_count()), ("Fix W-2042", 1));
    assert!(s.steered && s.directed.contains("finish-line-stop"), "{:?}", s.directed);
    assert!(s.exposed_to(&[cmd], false), "the rerun after the continuation is not the agent's own");
}

#[test]
fn every_prompt_is_kept_and_anchored() {
    let t = CommandTable::load();
    let lines = vec![
        meta(json!({})),
        user("Pick up W-2042 please"),
        exec("c1", "python3 scripts/wi-lookup W-2042"),
        out("c1", 0, "ID: W-2042"),
        user("now run the tests"),
        exec("c2", "make test"),
        exec("c3", "ls"),
        out("c2", 0, "ok"),
        out("c3", 0, "Makefile"),
        user("thanks"),
    ];
    let s = parse_rollout(&lines, &t, None).expect("parses");
    let got: Vec<(&str, u32)> = s.prompts.iter().map(|p| (p.text.as_str(), p.at_step)).collect();
    assert_eq!(got, [("Pick up W-2042 please", 0), ("now run the tests", 1), ("thanks", 3)]);
    assert_eq!(s.steps.iter().map(|x| x.prompt).collect::<Vec<_>>(), [0, 1, 1]);
    assert_eq!(s.prompt, s.prompts[0].text);
    // The committed fixtures: every step serves a prompt the session has.
    for (path, _, _) in fixtures() {
        let s = parse_rollout(&read_rollout(&path).unwrap(), &t, None).unwrap();
        assert_eq!(s.prompts[0].text, s.prompt, "{}", path.display());
        assert!(s.steps.iter().all(|x| (x.prompt as usize) < s.prompts.len()));
    }
}

#[test]
fn steering_subagents_paginated_prompts_and_hostile_rollouts() {
    let t = CommandTable::load();
    // Paginated history mode records no `user_message` event: the prompt comes from the response
    // item, skipping Codex's contextual user messages. Adage's own text is never the prompt.
    let steered = vec![
        meta(json!({})),
        user("<environment_context>\n  <cwd>/fixture/acme-billing</cwd>\n</environment_context>"),
        user("# AGENTS.md instructions for /fixture/acme-billing\n\n<INSTRUCTIONS>\nUse make.\n</INSTRUCTIONS>"),
        user("Pick up W-2042 please"),
        line(
            "response_item",
            json!({ "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": "<adage-reflex track=\"acme-billing/work-items@0.1.0\" route=\"lookup-work-item\" strength=\"fire\">\nIn this repository…\n</adage-reflex>" }] }),
        ),
        exec("c1", "python3 scripts/wi-lookup W-2042"),
        exec("c2", "ls"),
        out("c1", 0, "ID: W-2042"),
        out("c2", 0, "Makefile"),
        exec("c3", "sleep 100"),
        user("Adage finish-line check: this session changed files matching schema/*.json …"),
    ];
    let s = parse_rollout(&steered, &t, None).expect("parses");
    assert_eq!(s.prompt, "Pick up W-2042 please");
    assert!(s.steered);
    assert!(s.directed.contains("lookup-work-item") && s.directed.contains("finish-line-stop"), "{:?}", s.directed);
    assert_eq!(
        s.steps.iter().map(|x| x.turn).collect::<Vec<_>>(),
        vec![0, 0],
        "parallel calls share a turn; the unfinished call is skipped"
    );

    for sub in [
        json!({ "source": { "subagent": { "thread_spawn": { "parent_thread_id": "x" } } } }),
        json!({ "parent_thread_id": "019a" }),
    ] {
        let lines = vec![meta(sub), user("do W-1"), exec("c", "ls"), out("c", 0, "")];
        assert!(parse_rollout(&lines, &t, None).is_none(), "sub-agent threads are skipped");
    }
    let promptless =
        vec![meta(json!({})), user("Adage finish-line check: run make gen"), exec("c", "make gen"), out("c", 0, "")];
    assert!(parse_rollout(&promptless, &t, None).is_none());
    let hostile = vec![
        meta(json!({})),
        user("fix W-1"),
        line(
            "response_item",
            json!({ "type": "function_call", "name": "exec_command", "arguments": "{not json", "call_id": "a" }),
        ),
        line("response_item", json!({ "type": "function_call", "name": 7, "arguments": 8, "call_id": ["b"] })),
        line(
            "response_item",
            json!({ "type": "custom_tool_call", "name": "apply_patch", "input": "*** Add File: ".repeat(10_000), "call_id": "p" }),
        ),
        line(
            "response_item",
            json!({ "type": "local_shell_call", "call_id": "l", "status": "completed", "action": { "type": "exec", "command": ["apply_patch", "*** Begin Patch\n*** Add File: schema/x.json\n*** End Patch"] } }),
        ),
        line(
            "response_item",
            json!({ "type": "custom_tool_call_output", "call_id": "p", "output": [{ "type": "input_text", "text": "Success." }] }),
        ),
        line(
            "response_item",
            json!({ "type": "function_call_output", "call_id": "l", "output": "{\"output\":\"\",\"metadata\":{\"exit_code\":0}}" }),
        ),
        line("response_item", json!({ "type": "function_call_output", "call_id": "a", "output": null })),
        line("event_msg", json!({ "type": "user_message", "message": 5 })),
        json!("not an object"),
    ];
    let s = parse_rollout(&hostile, &t, None).expect("parses what it can");
    assert!(s.steps.len() <= 64 + 2);
    assert!(
        s.steps.iter().any(|x| x.rel_path.as_deref() == Some("schema/x.json")),
        "apply_patch through a local shell call"
    );
    assert!(parse_rollout(&[json!({ "type": "session_meta", "payload": [] })], &t, None).is_none());
    assert!(parse_rollout(&[], &t, None).is_none());
}

#[test]
fn discovery_walks_the_sessions_layout_and_skips_compressed_rollouts() {
    let d = tempfile::tempdir().unwrap();
    let day = d.path().join("2026/09/20");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join("rollout-2026-09-20T10-00-00-a.jsonl"), "").unwrap();
    std::fs::write(day.join("rollout-2026-09-13T10-00-00-b.jsonl.zst"), "").unwrap();
    std::fs::write(day.join("notes.jsonl"), "").unwrap();
    std::fs::create_dir_all(day.join("deeper/still")).unwrap();
    std::fs::write(day.join("deeper/still/rollout-x.jsonl"), "").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(d.path(), day.join("loop")).unwrap();
    let (found, compressed) = discover(d.path());
    assert_eq!(found, vec![day.join("rollout-2026-09-20T10-00-00-a.jsonl")]);
    assert_eq!(compressed, 1);
    let (fx, _) = discover(&adage_sim::fixtures_dir().join("codex/sessions"));
    assert_eq!(fx.len(), 12);
}
