use super::*;
use crate::{format::Decode, locate::AsRecorded};
use adage_core::{tools::normalize_tool, CommandTable};
use adage_trace::By;
use serde_json::json;

#[test]
fn empty_normalizations_preserve_original_call_and_opaque_effect() {
    for command in ["export PROJECT_ENV=test", "PROJECT_ENV=test", "", "# diagnostic comment"] {
        let input = json!({"command":command});
        let call = Call {
            tool: "Bash".into(),
            input: input.clone(),
            output: "original result".into(),
            failed: false,
            by: By::Agent,
            tool_use_id: Some("native-id".into()),
            agent: Some("worker".into()),
        };
        let normalized = normalize_tool("Bash", &input, "/fixture/project", CommandTable::load());
        assert!(normalized.is_empty(), "fixture must exercise the empty-normalization path: {command}");
        let steps = call_steps(7, 2, &call, normalized, Path::new("/fixture/project"));
        assert_eq!(steps.len(), 1);
        let st = &steps[0];
        assert_eq!(st.call, call);
        assert_eq!((st.turn, st.prompt, st.rel_path.as_deref()), (7, 2, None));
        assert!(matches!(st.norm.action, CanonicalAction::Unknown { .. }));
        assert_eq!(st.norm.effect, Effect::Environment);
        assert!(st.norm.low_confidence());
        assert_eq!(st.key, "unknown");
    }
}

#[test]
fn claude_retains_an_opaque_sibling_before_a_test_in_the_same_turn() {
    let lines = [
        json!({"type":"user","sessionId":"opaque-sibling","cwd":"/fixture/project",
            "message":{"content":"verify the project"}}),
        json!({"type":"assistant","message":{"id":"turn-0","content":[
            {"type":"tool_use","id":"failed","name":"Bash","input":{"command":"python -m pytest"}}]}}),
        json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"failed","content":"No module named pytest","is_error":true}]}}),
        json!({"type":"assistant","message":{"id":"turn-1","content":[
            {"type":"tool_use","id":"opaque","name":"Bash","input":{"command":"export PROJECT_ENV=test"}},
            {"type":"tool_use","id":"tests","name":"Bash","input":{"command":".venv/bin/python -m pytest"}}]}}),
        json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"opaque","content":"","is_error":false},
            {"type":"tool_result","tool_use_id":"tests","content":"2 passed in 0.1s","is_error":false}]}}),
    ];
    let text = lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    let cx = Decode {
        table: CommandTable::load(),
        locator: &AsRecorded,
        repo_override: Some("github.com/example/project"),
        subagent: false,
    };
    let s = crate::claude::decode(text.as_bytes(), &cx).unwrap();
    assert_eq!(s.steps.len(), 3);
    assert_eq!(
        s.steps.iter().map(|s| s.call.tool_use_id.as_deref()).collect::<Vec<_>>(),
        [Some("failed"), Some("opaque"), Some("tests")]
    );
    assert_eq!(s.steps[1].turn, s.steps[2].turn);
    assert!(s.steps[0].turn < s.steps[1].turn);
    assert!(s.steps[1].norm.contains_unknown());
    assert_eq!(s.steps[1].call.input["command"], "export PROJECT_ENV=test");
    assert!(s.steps[2].verification().is_some_and(|r| r.tests_run == Some(2)));
}
