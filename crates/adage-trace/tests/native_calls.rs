use adage_trace::{By, StepObs};
use serde_json::json;

fn step() -> StepObs {
    serde_json::from_value(json!({
        "turn":1,"prompt":0,"key":"exec ls .","rel_path":null,
        "norm":{"action":{"op":"exec","argv":["ls","."],"cwd":"/fixture"},
            "effect":"observe","mutating":false,"confidence":1.0,"note":""},
        "call":{"tool":"Bash","input":{"command":"ls .; ls .venv"},
            "output":"ls: missing directory","failed":false,"tool_use_id":"native-call"}
    }))
    .unwrap()
}

#[test]
fn normalized_failure_projection_does_not_split_one_native_invocation() {
    let first = step();
    let mut last = first.clone();
    last.call.failed = true;
    last.key = "exec ls .venv".into();
    assert_ne!(first.call, last.call, "the projected subaction result remains distinct");
    assert!(first.same_native_call(&last));
    assert!(last.same_native_call(&first));
}

#[test]
fn every_original_native_identity_field_and_context_must_agree() {
    let original = step();
    for changed in ["missing", "empty", "long", "id", "tool", "input", "output", "by", "agent", "turn", "prompt"] {
        let mut other = original.clone();
        match changed {
            "missing" => other.call.tool_use_id = None,
            "empty" => other.call.tool_use_id = Some(String::new()),
            "long" => other.call.tool_use_id = Some("x".repeat(257)),
            "id" => other.call.tool_use_id = Some("another-call".into()),
            "tool" => other.call.tool = "Read".into(),
            "input" => other.call.input["command"] = json!("ls elsewhere"),
            "output" => other.call.output = "different result".into(),
            "by" => other.call.by = By::Adage,
            "agent" => other.call.agent = Some("worker".into()),
            "turn" => other.turn += 1,
            "prompt" => other.prompt += 1,
            _ => unreachable!(),
        }
        assert!(!original.same_native_call(&other), "{changed}");
        assert!(!other.same_native_call(&original), "{changed}");
    }
    let mut missing = original;
    missing.call.tool_use_id = None;
    assert!(!missing.same_native_call(&missing), "payload equality without native identity is insufficient");
}
