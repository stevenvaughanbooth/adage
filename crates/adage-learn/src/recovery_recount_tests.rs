//! Synthetic contracts for the existing default admission gates, never live study evidence.

use super::*;
use adage_ingest::{format::Decode, locate::AsRecorded};
use serde_json::{json, Value};

const BAD: &str = "python3.11 -m pytest -q -p no:cacheprovider";
const GOOD: &str = ".venv/bin/python -m pytest -q -p no:cacheprovider";

fn session(id: &str, middle: &[(&str, Value, &str, bool)], tail: &str, output: &str) -> Session {
    let mut lines = vec![json!({"type":"user","sessionId":id,"cwd":"/fixture/recovery",
        "message":{"content":"Verify the prepared checkout without modifying it"}})];
    let mut calls = vec![("Bash", json!({"command":BAD}), "No module named pytest", true)];
    calls.extend(middle.iter().cloned());
    calls.push(("Bash", json!({"command":tail}), output, false));
    for (turn, (tool, input, output, failed)) in calls.into_iter().enumerate() {
        let call_id = format!("{id}-call-{turn}");
        lines.push(json!({"type":"assistant","cwd":"/fixture/recovery","message":{
            "id":format!("{id}-turn-{turn}"),"content":[{"type":"tool_use","id":call_id,"name":tool,"input":input}]}}));
        lines.push(json!({"type":"user","cwd":"/fixture/recovery","message":{"content":[
            {"type":"tool_result","tool_use_id":call_id,"content":output,"is_error":failed}]}}));
    }
    let bytes = lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    adage_ingest::claude::decode(
        bytes.as_bytes(),
        &Decode {
            table: adage_core::CommandTable::load(),
            locator: &AsRecorded,
            repo_override: Some("example/recovery"),
            subagent: false,
        },
    )
    .unwrap()
}

fn delayed(id: &str) -> Session {
    let reads: Vec<_> = (0..6)
        .map(|i| ("Read", json!({"file_path":format!("/fixture/recovery/docs/check-{i}.md")}), "documentation", false))
        .collect();
    session(id, &reads, GOOD, "2 passed in 0.1s")
}

fn refs(sessions: &[Session]) -> Vec<&Session> {
    sessions.iter().collect()
}

fn compiled() -> compile::Compiled {
    let key = adage_instincts::safety::command_key(GOOD, adage_core::CommandTable::load()).unwrap();
    let route = Route {
        id: compile::stable::dead_end_id(&key, "No module named pytest", 0),
        summary: "A verification command that fails here and its observed replacement".into(),
        trigger: Trigger::SessionStart,
        step: Step::Avoid {
            avoid: "python3.11 -m pytest".into(),
            instead: GOOD.into(),
            signature: "No module named pytest".into(),
        },
        slots: vec![],
        effect: Effect::Verify,
        strength: Strength::Inhibit,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence::default(),
        authored: false,
    };
    compile::Compiled {
        shape: "avoid",
        route,
        fixed_id: true,
        execution_candidate: false,
        advisory_eligible: true,
        execution_only: false,
        candidate: false,
        sessions: vec![],
        why: Why {
            shape: "avoid".into(),
            pattern: "synthetic failed-runner to observed working check".into(),
            backoff: 1,
            gain: 0.0,
            lift: 0.0,
            conf: 1.0,
            conf_lb: 0.0,
            support: 5,
            days: 1,
            members: vec![],
            examples: vec![],
            prior: None,
        },
    }
}

#[test]
fn delayed_native_recovery_passes_the_existing_five_session_recount_only() {
    let sessions: Vec<_> = (0..5).map(|i| delayed(&format!("s{i}"))).collect();
    let cfg = LearnConfig { instincts: None, ..Default::default() };
    assert_eq!(cfg.min_support, 5);
    let c = compiled();
    assert!(judge(&c, &refs(&sessions[..4]), &cfg, &[], false).is_err());
    let r = judge(&c, &refs(&sessions), &cfg, &[], false).unwrap();
    assert_eq!(r.id, c.route.id);
    assert_eq!((r.evidence.support, r.evidence.opportunities), (5, 5));
    assert_eq!((r.strength, r.state), (Strength::Inhibit, RouteState::Firm));
    assert_eq!(crate::value::trace_counts(&r, &refs(&sessions)), Some((0, 5)));
    for s in &sessions {
        let w = crate::outcome::recovery_witness(s, 0).unwrap();
        assert_eq!((w.at, w.goal_at, w.calls), (0, 7, 7));
        assert!(!crate::outcome::situations(s, "venv")[0].immediate_next);
    }
    let duplicate = vec![&sessions[0]; 5];
    assert!(judge(&c, &duplicate, &cfg, &[], false).is_err());
    let mut exposed = sessions.clone();
    exposed[4].influence.push(adage_trace::session::Directed { commands: vec![GOOD.into()], ..Default::default() });
    assert!(judge(&c, &refs(&exposed), &cfg, &[], false).is_err());
}

#[test]
fn failed_compound_observation_preserves_native_cost_and_recovery_evidence() {
    let s = session(
        "failed-observation",
        &[("Bash", json!({"command":"ls -la; ls .venv/bin 2>/dev/null | head -30"}), "ls: command not found", true)],
        GOOD,
        "2 passed in 0.1s",
    );
    assert!(s.steps.len() > 3, "the decoder must flatten the actual compound input");
    assert!(!s.steps[1].call.failed);
    assert!(s.steps[s.steps.len() - 2].call.failed, "only the last normalized action inherits the failure");
    let w = crate::outcome::recovery_witness(&s, 0).unwrap();
    assert_eq!((w.goal_at, w.calls), (s.steps.len() - 1, 2));
    let x = &crate::outcome::situations(&s, "venv")[0];
    assert_eq!(x.reward.unit, crate::outcome::RewardUnit::NativeCalls);
    assert!(!x.immediate_next, "one diagnostic call still intervened before the repair");
    assert_eq!(count_avoid(&["python3.11 -m pytest"], GOOD, &[&s]).support.len(), 1);

    for conflict in ["input", "output", "tool", "actor", "turn", "prompt"] {
        let mut changed = s.clone();
        match conflict {
            "input" => changed.steps[2].call.input["command"] = json!("ls elsewhere"),
            "output" => changed.steps[2].call.output = "different result".into(),
            "tool" => changed.steps[2].call.tool = "Read".into(),
            "actor" => changed.steps[2].call.agent = Some("different".into()),
            "turn" => changed.steps[2].turn += 1,
            "prompt" => changed.steps[2].prompt += 1,
            _ => unreachable!(),
        }
        assert!(crate::outcome::recovery_witness(&changed, 0).is_none(), "conflicting native {conflict}");
    }
    let refused = session(
        "refused-compound",
        &[("Bash", json!({"command":"ls .; pip install pytest"}), "<tool_use_error>denied</tool_use_error>", true)],
        GOOD,
        "2 passed in 0.1s",
    );
    assert_eq!(crate::outcome::recovery_witness(&refused, 0).unwrap().calls, 2);
    assert!(!crate::outcome::situations(&refused, "venv")[0].reward.harm, "the whole native call was refused");
    let not_refused = session(
        "ordinary-compound",
        &[("Bash", json!({"command":"ls .; pip install pytest"}), "permission denied", true)],
        GOOD,
        "2 passed in 0.1s",
    );
    assert!(crate::outcome::recovery_witness(&not_refused, 0).is_none(), "ordinary text is not a proven refusal");
}

#[test]
fn the_normal_engine_keeps_a_delayed_recovery_without_a_new_route_generator() {
    let sessions: Vec<_> = (0..5).map(|i| delayed(&format!("engine-{i}"))).collect();
    let result = run(&refs(&sessions), &LearnConfig { instincts: None, ..Default::default() }, &[], None);
    let r = result
        .routes
        .iter()
        .find(|j| j.route.id == compiled().route.id)
        .expect("existing mined avoid survives recount");
    assert_eq!(r.route.step, compiled().route.step);
    assert_eq!(r.route.evidence.support, 5);
    assert!(r.route.evidence.execution.is_none(), "advice is not execution authority");
}

#[test]
fn typed_environment_failures_never_fall_back_across_a_barrier() {
    for (tool, input, output, failed) in [
        ("Edit", json!({"file_path":"/fixture/recovery/src/a.py","old_string":"a","new_string":"b"}), "updated", false),
        ("Bash", json!({"command":"pip install pytest"}), "Successfully installed pytest", false),
        ("Bash", json!({"command":"export PYTHONPATH=src"}), "", false),
        ("Bash", json!({"command":"cat source > target"}), "", false),
        ("Bash", json!({"command":"rg --pre ./repair.py check src"}), "match", false),
    ] {
        let s = session("barrier", &[(tool, input, output, failed)], GOOD, "2 passed in 0.1s");
        assert!(crate::outcome::recovery_witness(&s, 0).is_none(), "{tool} {output}");
        assert!(count_avoid(&["python3.11 -m pytest"], GOOD, &[&s]).support.is_empty());
    }
}

#[test]
fn recovery_keeps_actual_scope_and_first_productive_result() {
    for (tail, output) in [
        (".venv/bin/python -m pytest -q -p no:cacheprovider tests/test_a.py", "2 passed in 0.1s"),
        (".venv/bin/python -m pytest -q -p no:cacheprovider -k one", "1 passed in 0.1s"),
        (".venv/bin/python -m unittest", "Ran 2 tests in 0.1s\nOK"),
        ("python3 -m pytest -q -p no:cacheprovider", "2 passed in 0.1s"),
        (GOOD, "no tests ran in 0.1s"),
        (GOOD, "1 failed, 1 passed in 0.1s"),
        (".venv/bin/python -m pytest -q -p no:cacheprovider | tail -10", "2 passed in 0.1s"),
        (".venv/bin/python -m pytest -q -p no:cacheprovider && true", "2 passed in 0.1s"),
    ] {
        let s = session("scope", &[], tail, output);
        assert!(crate::outcome::recovery_witness(&s, 0).is_none(), "{tail} {output}");
        if tail.starts_with(".venv/bin/python -m pytest") {
            assert!(count_avoid(&["python3.11 -m pytest"], tail, &[&s]).support.is_empty());
        }
    }
    let s = session(
        "red-first",
        &[("Bash", json!({"command":GOOD}), "1 failed, 1 passed in 0.1s", true)],
        GOOD,
        "2 passed in 0.1s",
    );
    assert!(crate::outcome::recovery_witness(&s, 0).is_none(), "a later green does not replace the first goal");
    let s = session(
        "setup-first",
        &[("Bash", json!({"command":GOOD}), "No module named pytest", true)],
        GOOD,
        "2 passed in 0.1s",
    );
    assert_eq!(crate::outcome::recovery_witness(&s, 0).unwrap().goal_at, 2, "next_at is not the first productive goal");
}

#[test]
fn native_boundaries_and_the_existing_horizon_remain_required() {
    for boundary in ["missing-id", "duplicate-id", "actor", "prompt", "cwd", "turn-gap", "same-turn"] {
        let mut s = delayed("boundary");
        match boundary {
            "missing-id" => s.steps[2].call.tool_use_id = None,
            "duplicate-id" => s.steps[2].call.tool_use_id = s.steps[0].call.tool_use_id.clone(),
            "actor" => s.steps[2].call.agent = Some("different".into()),
            "prompt" => s.steps[2].prompt += 1,
            "cwd" => s.steps[2].call.input["cwd"] = json!("/elsewhere"),
            "turn-gap" => s.steps[2].turn += 2,
            "same-turn" => s.steps.last_mut().unwrap().turn = 6,
            _ => unreachable!(),
        }
        assert!(crate::outcome::recovery_witness(&s, 0).is_none(), "{boundary}");
    }
    let reads: Vec<_> = (0..15)
        .map(|i| ("Read", json!({"file_path":format!("/fixture/recovery/docs/{i}.md")}), "docs", false))
        .collect();
    let s = session("past-horizon", &reads, GOOD, "2 passed in 0.1s");
    assert!(crate::outcome::recovery_witness(&s, 0).is_none());
    let s = session(
        "refused",
        &[("Bash", json!({"command":"/bin/ls -la"}), "<tool_use_error>permission denied</tool_use_error>", true)],
        GOOD,
        "2 passed in 0.1s",
    );
    assert_eq!(crate::outcome::recovery_witness(&s, 0).unwrap().calls, 2, "a proven refusal counts cost but no repair");
}

#[test]
fn a_different_repository_check_keeps_its_existing_short_window_contract() {
    let s = session("declared-wrapper", &[], "make test", "all checks passed");
    assert_eq!(count_avoid(&["python3.11 -m pytest"], "make test", &[&s]).support.len(), 1);
}

#[test]
fn the_graphs_original_dead_end_consistency_gate_is_not_bypassed() {
    let mut sessions: Vec<_> = (0..5).map(|i| delayed(&format!("failed-{i}"))).collect();
    sessions.push(session("runner-also-passes", &[], BAD, "2 passed in 0.1s"));
    let result = run(&refs(&sessions), &LearnConfig { instincts: None, ..Default::default() }, &[], None);
    assert!(!result.routes.iter().any(|j| j.route.id == compiled().route.id));
}
