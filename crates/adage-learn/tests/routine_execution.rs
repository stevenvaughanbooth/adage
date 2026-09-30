//! Synthetic contract tests, not live performance evidence. All thresholds use production defaults.
use adage_learn::{learn, Inputs, LearnConfig};
use adage_tracks::{Index, Step, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

fn learn_routine(root: &Path, count: usize, mode: &str) -> Index {
    let cwd = "/fixture/routine";
    let files = (0..count).map(|i| {
        let sid = format!("routine-{i}");
        let mut lines = vec![json!({"type":"user","sessionId":sid,"cwd":cwd,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":format!("Implement feature {} with its regression test", if mode == "one-task" { 0 } else { i })}})];
        if mode == "exposed" || (mode == "partly-exposed" && i == count - 1) {
            lines.push(json!({"type":"attachment","sessionId":sid,"cwd":cwd,"attachment":{"type":"hook_additional_context","content":["<adage-reflex track=\"routine/learned@0.1.0\" route=\"check\" strength=\"prime\" kind=\"run\">Run python -m pytest tests/test_feature.py</adage-reflex>"]}}));
        }
        let file = if mode.starts_with("varying") { format!("test_feature{i}.py") } else { "test_feature.py".into() };
        let mut calls = vec![("Read",json!({"file_path":format!("{cwd}/src/lib.py")})), ("Write",json!({"file_path":format!("{cwd}/tests/{file}"),"content":"def test_feature(): pass"}))];
        if mode == "intermediate" {
            calls.push(("Write",json!({"file_path":format!("{cwd}/tests/{file}"),"content":"def test_feature(): assert True"})));
        }
        let python = if mode == "project-python" { ".venv/bin/python" } else { "python" };
        calls.push(("Bash",json!({"command":format!("{python} -m pytest tests/{file}{}", if mode == "varying-narrowed" { format!("::test_{i}") } else { String::new() })})));
        for (n,(tool,input)) in calls.into_iter().enumerate() {
            let id = format!("toolu_{n}");
            lines.push(json!({"type":"assistant","sessionId":sid,"cwd":cwd,"message":{"id":format!("msg_{}", if mode == "same-turn" { 0 } else { n }),"model":"m","content":[{"type":"tool_use","id":id,"name":tool,"input":input}]}}));
            lines.push(json!({"type":"user","sessionId":sid,"cwd":cwd,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":"done","is_error":mode == "failed" && tool=="Bash"}]}}));
        }
        let p=root.join(format!("{sid}.jsonl"));
        std::fs::write(&p,lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        p
    }).collect();
    let home = root.join("home");
    let inputs = Inputs {
        transcripts: files,
        outcomes: BTreeMap::new(),
        repo_override: Some("routine".into()),
        models: None,
        cache: None,
        only_repo: None,
    };
    learn(&inputs, &home.join("tracks"), &home.join("index.json"), &LearnConfig::default()).unwrap();
    Index::load(&home.join("index.json"))
}

#[test]
fn habitual_check_is_learned_for_execution_without_advisory_uplift() {
    let d = tempfile::tempdir().unwrap();
    assert_routine(learn_routine(d.path(), 5, "routine"), "python -m pytest tests/test_feature.py");
}

#[test]
fn explicit_project_python_is_learned_as_verify_without_changing_the_command() {
    let d = tempfile::tempdir().unwrap();
    assert_routine(learn_routine(d.path(), 5, "project-python"), ".venv/bin/python -m pytest tests/test_feature.py");
}

fn assert_routine(index: Index, expected: &str) {
    let route =
        index.routes.iter().find(|r| r.route.evidence.execution.is_some()).expect("a routine verification candidate");
    assert!(matches!(route.route.trigger, Trigger::AfterEdit { .. }));
    assert!(matches!(&route.route.step,Step::Run { command } if command == expected));
    assert_eq!(route.route.effect, adage_core::Effect::Verify);
    let e = route.route.evidence.execution.as_ref().unwrap();
    assert_eq!((e.opportunities, e.immediate, e.sessions), (5, 5, 5));
    assert!(route.exec_only(), "knowledge alone must not deliver E0 spam or authorize execution");
    assert!(route.route.state.is_live(), "zero advisory uplift must not extinguish executable knowledge");
    let table = adage_core::CommandTable::load();
    let ctx = adage_runtime::Ctx {
        index: &index,
        repo: "routine".into(),
        root: Path::new("/fixture/routine"),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let edit = adage_runtime::Event::Batch {
        calls: vec![adage_runtime::Call {
            tool: "Write".into(),
            input: json!({"file_path":"/fixture/routine/tests/test_feature.py","content":"changed"}),
            output: "done".into(),
            failed: false,
        }],
    };
    let mut state = adage_runtime::SessionState::default();
    adage_runtime::decide(&ctx, &mut state, &edit);
    assert!(state.obligations.is_empty(), "without execution admission the candidate creates no E0 obligation");
    adage_runtime::with_exec_only(vec![(route.track.clone(), route.route.id.clone())], || {
        adage_runtime::decide(&ctx, &mut state, &edit);
    });
    assert!(
        state.obligations.iter().any(|o| o.route == route.route.id && o.fresh),
        "the existing execution admission arms the learned action"
    );
}

#[test]
fn routine_execution_preserves_support_and_immediate_success_gates() {
    for (count, mode) in [
        (4, "routine"),
        (5, "intermediate"),
        (5, "failed"),
        (5, "same-turn"),
        (5, "exposed"),
        (5, "partly-exposed"),
        (5, "one-task"),
        (5, "varying-narrowed"),
    ] {
        let d = tempfile::tempdir().unwrap();
        let index = learn_routine(d.path(), count, mode);
        assert!(!index.routes.iter().any(|r| r.route.evidence.execution.is_some()), "{count} sessions, {mode}");
    }
}

#[test]
fn routine_test_path_is_bound_from_the_edited_file() {
    let d = tempfile::tempdir().unwrap();
    let index = learn_routine(d.path(), 5, "varying");
    let route =
        index.routes.iter().find(|r| r.route.evidence.execution.is_some()).expect("a learned path-bound routine");
    assert!(matches!(&route.route.step, Step::Run { command } if command == "python -m pytest tests/{part}"));
    assert_eq!(route.route.slots.len(), 1);
    assert_eq!(route.route.evidence.execution.as_ref().unwrap().immediate, 5);
}
