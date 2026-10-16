use adage_trace::{OutcomeBasis, Session};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Case {
    name: String,
    steps: Vec<String>,
    expect: Option<bool>,
    basis: OutcomeBasis,
    #[serde(default)]
    same_turn: bool,
    #[serde(default)]
    new_prompt: bool,
    #[serde(default)]
    cut_off: bool,
    #[serde(default)]
    asked_for_help: bool,
    #[serde(default)]
    structured_handback: bool,
    #[serde(default)]
    recorded: Option<bool>,
}

#[test]
fn reviewed_completion_fixtures_distinguish_recorded_fresh_stale_and_unknown_outcomes() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/outcomes.json")).unwrap();
    for c in cases {
        let steps: Vec<_> = c
            .steps
            .iter()
            .enumerate()
            .map(|(i, kind)| {
                let (action, effect, mutating) = match kind.as_str() {
                    "edit" => (json!({"op":"edit","path":"src/lib.rs"}), "author", true),
                    "read" => (json!({"op":"read","path":"src/lib.rs","range":null}), "observe", false),
                    "environment" => {
                        (json!({"op":"exec","argv":["pip","install","-e","."],"cwd":"."}), "environment", true)
                    }
                    "regenerate" => (json!({"op":"exec","argv":["make","gen"],"cwd":"."}), "regenerate", true),
                    _ => (json!({"op":"exec","argv":["cargo","test"],"cwd":"."}), "verify", false),
                };
                let call = if effect == "verify" {
                    let output = match kind.as_str() {
                        "pass" => "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
                        "fail" => "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
                        "empty" => "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
                        _ => "incomplete output",
                    };
                    json!({"tool":"Bash","input":{"command":"cargo test"},"output":output,"failed":kind=="fail"})
                } else {
                    json!({"tool":"fixture","input":{},"output":"fixture","failed":false})
                };
                json!({"turn":if c.same_turn {0} else {i}, "key":"fixture", "rel_path":null,
                "norm":{"action":action,"effect":effect,"mutating":mutating,"confidence":1.0,"note":""},
                "call":call})
            })
            .collect();
        let prompts = if c.new_prompt {
            json!([{"text":"first task","at_step":0},{"text":"new task","at_step":steps.len()}])
        } else {
            json!([])
        };
        let s: Session = serde_json::from_value(json!({
            "id":"fixture","root":"/fixture","repo":"repo","model":null,"day":"2026-10-01",
            "prompt":"first task","steps":steps,"steered":false,"directed":[],"outcome":c.recorded,
            "harness":"codex","prompts":prompts,"ending":{"cut_off":c.cut_off,"asked_for_help":c.asked_for_help,
                "structured_handback":c.structured_handback}
        }))
        .unwrap();
        let got = s.outcome_evidence();
        assert_eq!((got.value, got.basis), (c.expect, c.basis), "{}", c.name);
    }
}
