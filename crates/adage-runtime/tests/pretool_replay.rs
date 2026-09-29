//! A real RepoBench v2 control trace replayed through the `PreToolUse` planner
//! (`adage_runtime::pretool`): the one call that hit U11's dead end (`python -m pytest … 2>&1 |
//! head -50`, `No module named pytest`, invisible to the post-hoc hooks because of the pipe) is
//! rewritten to the project's interpreter, and nothing else in the session is touched: not the
//! agent's searches, not its runs under an activated `.venv`, not its commits.

use adage_runtime::pretool::{plan, Env, Pre};
use serde_json::Value;

#[test]
fn a_real_control_trace_is_rewritten_only_at_its_dead_end() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/click-0cd28d65-r1-control.json")).unwrap();
    let env = Env {
        interpreter: "/w/click/.venv/bin/python".into(),
        shown: ".venv/bin/python".into(),
        pytest: true,
        modules: vec!["click".into(), "pytest".into()],
        root: "/w/click".into(),
        cwd: "/w/click".into(),
        version: Some("3.12".into()),
        uv: true,
    };
    let mut rewritten = Vec::new();
    for c in fixture["calls"].as_array().unwrap() {
        let cmd = c["command"].as_str().unwrap();
        match plan(cmd, &env, |_| None) {
            Some(Pre::Rewrite { command, kind, .. }) => {
                assert_eq!(kind, "test");
                assert_eq!(command, cmd.replacen("python", "/w/click/.venv/bin/python", 1), "only the prefix");
                rewritten.push(c["dead_end"].as_bool().unwrap());
            }
            Some(Pre::Advise { reason, .. }) => panic!("{cmd}: {reason}"),
            None => assert!(!c["dead_end"].as_bool().unwrap(), "a dead end left alone: {cmd}"),
        }
    }
    assert_eq!(rewritten, [true], "exactly the dead end");
}
