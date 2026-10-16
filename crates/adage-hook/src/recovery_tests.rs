//! Native hook transport: no mod or command-execution extension is needed for E0 advice.
use crate::testkit::{bash_call, ObligationWorld, NO_EXTENSION};
use adage_core::repo;
use adage_tracks::{
    testkit::{route, IndexBuilder},
    Step, Strength, Trigger,
};
use serde_json::json;

#[test]
fn native_failed_call_then_retry_survives_pretool_and_delivers_once() {
    let w = ObligationWorld::new();
    let key = repo::repo_key_cached(&w.root, &w.paths.repo_keys());
    let command = "python3 -m pytest tests/test_a.py";
    let r = route(
        "retry",
        Trigger::AfterCommand { command: adage_runtime::command_key(command, adage_core::CommandTable::load()) },
        Step::Advise {
            template: adage_tracks::recovery::TEMPLATE.into(),
            slots: [("command".into(), command.into())].into(),
        },
    )
    .strength(Strength::Prime)
    .effect(adage_core::Effect::Observe);
    IndexBuilder::new("recovery-world").route(&key, "acme/recovery", r).write(&w.paths.index());
    for failed in [
        json!({"tool_name":"Bash", "tool_use_id":"red", "tool_input":{"command":command}, "error":"Exit code 1\n1 failed, 1 passed in 0.1s"}),
        json!({"tool_name":"Bash", "tool_use_id":"red", "tool_input":{"command":command}, "tool_response":{"stdout":"1 failed, 1 passed in 0.1s", "stderr":""}}),
    ] {
        w.home.seed::<adage_runtime::SessionState>("s", |st| {
            *st = Default::default();
            st.arm = Some(adage_runtime::Arm::Treated);
        });
        assert!(w.event(&NO_EXTENSION, "PostToolUseFailure", failed).is_none());
        assert!(!w.state().recovery.is_empty());
        let before = w.state().recovery;
        bash_call(&w, &NO_EXTENSION, "green", command, json!({}));
        assert_eq!(w.state().recovery, before, "PreToolUse is not a new completed action");
        let green = json!({"tool_name":"Bash", "tool_use_id":"green", "tool_input":{"command":command}, "tool_response":{"stdout":"2 passed in 0.1s", "stderr":""}});
        let out = w.event(&NO_EXTENSION, "PostToolUse", green.clone()).unwrap();
        let text = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        assert!(text.contains("without an observed repair"), "{out}");
        assert!(text.contains("Source and input identity were not established"));
        assert!(w.state().pending.is_empty() && w.state().obligations.is_empty());
        assert!(w.event(&NO_EXTENSION, "PostToolUse", green).is_none());
    }
    w.home.seed::<adage_runtime::SessionState>("s", |st| {
        *st = Default::default();
        st.arm = Some(adage_runtime::Arm::Treated);
    });
    w.event(&NO_EXTENSION, "PostToolUseFailure", json!({"tool_name":"Bash", "tool_use_id":"resumed-red", "tool_input":{"command":command}, "error":"1 failed, 1 passed in 0.1s"}));
    assert!(!w.state().recovery.is_empty());
    w.event(&NO_EXTENSION, "SessionStart", json!({"source":"resume"}));
    assert!(w.state().recovery.is_empty());
    assert!(w.event(&NO_EXTENSION, "PostToolUse", json!({"tool_name":"Bash", "tool_use_id":"resumed-green", "tool_input":{"command":command}, "tool_response":"2 passed in 0.1s"})).is_none());
    w.home.seed::<adage_runtime::SessionState>("s", |st| {
        *st = Default::default();
        st.arm = Some(adage_runtime::Arm::Treated);
    });
    w.event(&NO_EXTENSION, "PostToolUseFailure", json!({"tool_name":"Bash", "tool_use_id":"cwd-red", "tool_input":{"command":command}, "error":"1 failed, 1 passed in 0.1s"}));
    assert!(!w.state().recovery.is_empty());
    let subdir = w.root.join("subdir");
    std::fs::create_dir(&subdir).unwrap();
    assert!(w.event(&NO_EXTENSION, "PostToolUse", json!({"cwd":subdir, "tool_name":"Bash", "tool_use_id":"cwd-green", "tool_input":{"command":command}, "tool_response":"2 passed in 0.1s"})).is_none());
    assert!(w.state().recovery.is_empty(), "same repository, different relative test target");
}
