use super::*;
use adage_ingest::format::Decode;
use serde_json::{json, Value};

fn bash(command: &str, output: &str) -> (&'static str, Value, String) {
    ("Bash", json!({"command":command,"description":"Run tests"}), output.into())
}

fn worker(id: &str, calls: &[(&str, Value, String)]) -> Session {
    let mut records = vec![json!({"type":"user","sessionId":"parent","agentId":id,
        "isSidechain":true,"cwd":"/repo","message":{"content":"Check parser behavior"}})];
    for (i, (tool, input, output)) in calls.iter().enumerate() {
        records.push(json!({"type":"assistant","message":{"id":format!("m{i}"),"content":[
            {"type":"tool_use","id":format!("call-{i}"),"name":tool,"input":input}]}}));
        records.push(json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":format!("call-{i}"),"content":output,"is_error":false}]}}));
    }
    let raw = records.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    let cx = Decode {
        table: adage_core::CommandTable::load(),
        locator: &adage_ingest::locate::AsRecorded,
        repo_override: Some("example/parser"),
        subagent: true,
    };
    adage_ingest::claude::decode(raw.as_bytes(), &cx).unwrap()
}

fn red() -> (&'static str, Value, String) {
    bash("pytest tests/test_parser.py", "1 failed, 1 passed in 0.1s")
}

fn green() -> (&'static str, Value, String) {
    bash("pytest tests/test_parser.py", "2 passed in 0.1s")
}

fn recoveries(s: &Session) -> Vec<Proposal> {
    learn(&[s]).proposals.into_iter().filter(|p| p.kind == ProcedureKind::UnexplainedRecovery).collect()
}

#[test]
fn an_attributable_retry_is_a_qualified_observation_not_a_repair_or_task_grade() {
    let s = worker(
        "worker",
        &[red(), ("Read", json!({"file_path":"tests/test_parser.py"}), "assert parse()".into()), green()],
    );
    let proposals = recoveries(&s);
    assert_eq!(proposals.len(), 1);
    let proposal = &proposals[0];
    assert_eq!(proposal.status, "needs_evidence_review");
    assert_eq!(proposal.template.as_deref(), Some(adage_tracks::recovery::TEMPLATE));
    assert_eq!(proposal.evidence_groups, 1);
    assert_eq!(proposal.episodes[0].checks.len(), 2);
    assert_eq!(proposal.episodes[0].checks[0].call_id.as_deref(), Some("call-0"));
    assert_eq!(proposal.episodes[0].checks[1].call_id.as_deref(), Some("call-2"));
    assert_eq!(proposal.episodes[0].checks[0].reported.tests_run, Some(2));
    assert_eq!(proposal.episodes[0].checks[1].outcome, VerificationOutcome::Passed);
    assert!(proposal.episodes[0].closure.contains("source and environment equality are unverified"));
    assert!(proposal.requirements.iter().any(|s| s.contains("neither a repair nor flakiness")));
    assert_eq!(s.outcome, None);
}

#[test]
fn a_normal_red_edit_green_cycle_and_opaque_work_are_not_unexplained_recovery() {
    let invalidators = [
        ("Edit", json!({"file_path":"src/parser.py","old_string":"bad","new_string":"fixed"}), "edited".into()),
        ("Write", json!({"file_path":"/tmp/run.log","content":"log"}), "written".into()),
        ("mcp__example__custom", json!({}), "done".into()),
        ("Agent", json!({"subagent_type":"worker"}), "done".into()),
        bash("python -c 'repair()'", ""),
        bash("git checkout other", "Switched to branch other"),
        bash("cat src/a.py > src/b.py", ""),
        bash("pytest tests/test_parser.py", "output incomplete"),
    ];
    for invalidator in invalidators {
        let s = worker("worker", &[red(), invalidator.clone(), green()]);
        assert!(recoveries(&s).is_empty(), "must invalidate: {invalidator:?}");
    }
}

#[test]
fn setup_zero_unknown_and_changed_test_counts_do_not_seed_recovery() {
    for output in
        ["ImportError while loading conftest.py", "no tests ran in 0.1s", "command not found", "ok", "1 failed in 0.1s"]
    {
        let s = worker("worker", &[bash("pytest tests/test_parser.py", output), green()]);
        assert!(recoveries(&s).is_empty(), "not matching assertion-test evidence: {output}");
    }
}

#[test]
fn ambiguous_shell_and_copied_log_reports_never_qualify() {
    for command in [
        "pytest tests/test_parser.py 2>&1 | tail -n 10",
        "pytest tests/test_parser.py; echo done",
        "cat /tmp/test.log",
        "python verify_wrapper.py",
    ] {
        let s = worker("worker", &[bash(command, "1 failed, 1 passed in 0.1s"), bash(command, "2 passed in 0.1s")]);
        assert!(recoveries(&s).is_empty(), "ambiguous ownership: {command}");
    }
}

#[test]
fn changed_scope_prompt_agent_and_concurrent_calls_do_not_form_pairs() {
    let changed_target = worker("worker", &[red(), bash("pytest tests/test_other.py", "2 passed in 0.1s")]);
    assert!(recoveries(&changed_target).is_empty());
    for change in 0..4 {
        let mut s = worker("worker", &[red(), green()]);
        match change {
            0 => s.steps[1].prompt = 1,
            1 => s.steps[1].call.agent = Some("different-worker".into()),
            2 => s.steps[1].turn = s.steps[0].turn,
            3 => {
                let CanonicalAction::Exec { cwd, .. } = &mut s.steps[1].norm.action else { panic!("exec") };
                *cwd = "another/checkout".into();
            }
            _ => unreachable!(),
        }
        assert!(recoveries(&s).is_empty(), "scope change {change}");
    }
    let failed_worker = worker("one", &[red()]);
    let passed_worker = worker("two", &[green()]);
    assert!(learn(&[&failed_worker, &passed_worker])
        .proposals
        .iter()
        .all(|p| p.kind != ProcedureKind::UnexplainedRecovery));
}

#[test]
fn directed_spans_and_missing_or_conflicting_call_identity_abstain() {
    for change in 0..4 {
        let mut s = worker("worker", &[red(), green()]);
        match change {
            0 => s.influence.push(adage_trace::Directed { at_step: 1, all: true, ..Default::default() }),
            1 => s.steered = true,
            2 => s.steps[0].call.tool_use_id = None,
            3 => {
                let mut conflicting = s.steps[0].clone();
                conflicting.call.output = "different result".into();
                s.steps.push(conflicting);
            }
            _ => unreachable!(),
        }
        assert!(recoveries(&s).is_empty(), "unsupported evidence {change}");
    }
}

#[test]
fn duplicate_projections_do_not_duplicate_episodes_and_siblings_share_support() {
    let mut a = worker("a", &[red(), green()]);
    let duplicate = a.steps[0].clone();
    a.steps.insert(1, duplicate);
    let b = worker("b", &[red(), green()]);
    let forward = learn(&[&a, &b]);
    assert_eq!(forward, learn(&[&b, &a]));
    let p = forward.proposals.iter().find(|p| p.kind == ProcedureKind::UnexplainedRecovery).unwrap();
    assert_eq!(p.sessions, 2);
    assert_eq!(p.episodes.len(), 2);
    assert_eq!(p.evidence_groups, 1);
}
