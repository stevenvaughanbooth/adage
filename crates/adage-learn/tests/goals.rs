//! Local execution rewards use attributable runner evidence, not arbitrary output or exit codes.

use adage_core::CommandTable;
use adage_ingest::{format::Decode, locate::AsRecorded};
use adage_learn::{
    cases,
    outcome::{self, Goal, RewardUnit},
};
use adage_trace::Session;
use serde_json::json;

fn session(calls: &[(&str, &str, bool)]) -> Session {
    let mut lines = vec![json!({"type":"user", "sessionId":"goal-test", "cwd":"/fixture/project",
        "message":{"content":"repair the parser"}})];
    for (i, (command, output, failed)) in calls.iter().enumerate() {
        let id = format!("call-{i}");
        lines.push(json!({"type":"assistant", "message":{"id":format!("turn-{i}"), "content":[
            {"type":"tool_use", "id":id, "name":"Bash", "input":{"command":command}}]}}));
        lines.push(json!({"type":"user", "message":{"content":[
            {"type":"tool_result", "tool_use_id":id, "content":output, "is_error":failed}]}}));
    }
    let text = lines.iter().map(serde_json::Value::to_string).collect::<Vec<_>>().join("\n");
    let cx = Decode {
        table: CommandTable::load(),
        locator: &AsRecorded,
        repo_override: Some("github.com/example/project"),
        subagent: false,
    };
    adage_ingest::claude::decode(text.as_bytes(), &cx).unwrap()
}

#[test]
fn collection_zero_setup_and_shared_output_cannot_reach_the_test_goal() {
    for (command, output) in [
        ("pytest", "collected 12 items"),
        ("pytest --collect-only", "12 tests collected in 0.1s"),
        ("pytest", "no tests ran in 0.1s"),
        ("pytest", "3 skipped in 0.1s"),
        ("pytest", "1 error in 0.1s"),
        ("python -m unittest", "Ran 1 test in 0.1s\nFAILED (errors=1)"),
        ("cargo test", "test result: ok. 0 passed; 0 failed; 2 ignored"),
        ("pnpm test", "No test files found, exiting with code 1"),
        ("vitest run", "Test Files 1 passed (1)"),
        ("go test ./...", "ok example.org/pkg 0.2s"),
        ("pytest && pytest", "2 passed in 0.1s"),
        ("cat fixture.txt; pytest", "2 passed in 0.1s"),
        ("cat fixture.txt", "2 passed in 0.1s"),
        ("python show_report.py", "2 passed in 0.1s"),
        ("make check", "2 passed in 0.1s"),
    ] {
        let s = session(&[(command, output, false)]);
        assert!(!s.steps.is_empty(), "{command}");
        assert!(s.steps.iter().all(|st| !outcome::test_goal(st) && !cases::tests_passed(st)), "{command}");
    }
}

#[test]
fn failed_assertions_are_execution_goals_but_not_passing_cases() {
    for (command, output, transport_failed) in [
        ("pytest", "E ModuleNotFoundError: No module named 'fixture_dep'\n1 failed, 3 passed in 0.2s", true),
        (
            "cd /fixture/project && pnpm test 2>&1 | tail -30",
            "Test Files 1 failed (1)\nTests 1 failed | 2 passed (3)",
            false,
        ),
        (
            "cargo test 2>&1 | grep -E 'test result' | head -20",
            "test result: FAILED. 3 passed; 1 failed; 0 ignored",
            false,
        ),
        ("python -m unittest", "Ran 2 tests in 0.1s\nFAILED (failures=1)", true),
    ] {
        let s = session(&[(command, output, transport_failed)]);
        assert!(s.steps.iter().any(outcome::test_goal), "{command}");
        assert!(s.steps.iter().all(|st| !cases::tests_passed(st)), "{command}");
    }
}

#[test]
fn a_partial_positive_report_proves_execution_without_proving_overall_success() {
    for (command, output, failed, passed) in [
        ("pytest", "2 passed in 0.1s", false, true),
        ("vitest run", "Test Files 1 passed (1)\nTests 3 passed (3)", false, true),
        ("go test -v ./...", "--- PASS: TestParser (0.00s)\nok example.org/pkg 0.1s", false, true),
        ("pytest | head", "2 passed in 0.1s", false, false),
        ("pytest", "2 passed in 0.1s", true, false),
    ] {
        let s = session(&[(command, output, failed)]);
        assert_eq!(s.steps.len(), 1);
        assert!(outcome::test_goal(&s.steps[0]), "{command}");
        assert_eq!(cases::tests_passed(&s.steps[0]), passed, "{command}");
    }
}

#[test]
fn build_goals_use_runner_evidence_and_keep_only_direct_legacy_wrapper_status() {
    for (command, output, failed, goal) in [
        ("cargo check", "", false, true),
        ("cargo check", "", true, false),
        ("cargo check | tail", "", false, false),
        ("cargo build 2>&1 | head -20", "error: could not compile `example` due to 1 previous error", false, false),
        ("cargo check && cargo check", "", false, false),
        ("make lint", "ok", false, true),
        ("make lint", "failed", true, false),
        ("make lint | tail", "ok", false, false),
        ("make lint && make lint", "ok", false, false),
    ] {
        let s = session(&[(command, output, failed)]);
        assert!(!s.steps.is_empty(), "{command}");
        assert_eq!(s.steps.iter().any(|st| outcome::is_goal(st, Goal::Build)), goal, "{command}");
    }
}

#[test]
fn environment_reward_waits_for_execution_and_ignores_quoted_setup_errors() {
    let s = session(&[
        ("pytest", "No module named pytest", true),
        ("pytest --collect-only", "2 tests collected in 0.1s", false),
        ("pytest", "1 failed, 1 passed in 0.1s", true),
    ]);
    let found = outcome::situations(&s, "venv");
    assert_eq!(found.len(), 1);
    assert!(found[0].reward.reached);
    assert_eq!(found[0].reward.calls, 2);
    assert!(!found[0].next_misfired);
    for command in ["cat fixture.txt; pytest", "pytest && pytest"] {
        let s = session(&[(command, "No module named pytest", false)]);
        assert!(outcome::situations(&s, "venv").is_empty(), "{command}");
    }
    let s = session(&[("pytest", "E ModuleNotFoundError: No module named 'fixture_dep'\n1 failed in 0.1s", true)]);
    assert!(outcome::situations(&s, "venv").is_empty(), "an assertion is not an environment misfire");
}

#[test]
fn immediate_repair_and_delayed_discovery_are_distinct() {
    let direct = session(&[
        ("python3.11 -m pytest", "No module named pytest", true),
        (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
    ]);
    let found = outcome::situations(&direct, "venv");
    assert_eq!(found.len(), 1);
    assert!(found[0].immediate_next);
    assert_eq!(found[0].reward.calls, 1);
    assert_eq!(found[0].reward.unit, RewardUnit::NativeCalls);

    let delayed = session(&[
        ("python3.11 -m pytest", "No module named pytest", true),
        ("ls .; ls .venv", "bin", false),
        (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
    ]);
    assert_eq!(delayed.steps.len(), 4, "one listing call becomes two normalized actions");
    let found = outcome::situations(&delayed, "venv");
    assert_eq!(found[0].reward.calls, 2);
    assert_eq!(found[0].reward.unit, RewardUnit::NativeCalls);
    assert!(found[0].reward.reached);
    assert!(!found[0].immediate_next);
    assert_eq!(found[0].next_at, Some(3), "the eventual recovery is still retained");
}

#[test]
fn recovery_does_not_cross_task_actor_cwd_or_mutation_boundaries() {
    let direct = session(&[
        ("python -m pytest", "No module named pytest", true),
        (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
    ]);
    for variant in 0..5 {
        let mut s = direct.clone();
        match variant {
            0 => s.steps[1].prompt = 1,
            1 => s.prompts.push(adage_trace::PromptObs { text: "new task".into(), at_step: 1 }),
            2 => s.steps[1].call.agent = Some("another-worker".into()),
            3 => s.steps[1].call.by = adage_trace::By::Adage,
            _ => {
                let adage_core::CanonicalAction::Exec { cwd, .. } = &mut s.steps[1].norm.action else { panic!() };
                *cwd = "/another-project".into();
            }
        }
        let found = outcome::situations(&s, "venv");
        assert!(found[0].next.is_none(), "variant {variant}");
        assert!(!found[0].reward.reached, "variant {variant}");
        assert!(!found[0].immediate_next, "variant {variant}");
    }
    for command in [
        "pip install pytest",
        "echo changed > src/core.py",
        "cat source.py > target.py",
        "rg --pre arbitrary-code needle",
        "find . -exec repair {} \\;",
        "python -c 'repair()'",
        "mystery-fix",
    ] {
        let s = session(&[
            ("python -m pytest", "No module named pytest", true),
            (command, "", false),
            (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
        ]);
        let found = outcome::situations(&s, "venv");
        assert!(found[0].next.is_none(), "{command}");
        assert!(!found[0].reward.reached, "{command}");
        if command == "pip install pytest" {
            assert!(found[0].reward.harm);
        }
    }
}

#[test]
fn incomplete_or_reused_call_identity_cannot_supply_native_cost_evidence() {
    let base = session(&[
        ("python -m pytest", "No module named pytest", true),
        ("ls .", "src", false),
        (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
    ]);
    for variant in 0..3 {
        let mut s = base.clone();
        match variant {
            0 => s.steps.iter_mut().for_each(|st| st.call.tool_use_id = None),
            1 => s.steps[2].call.tool_use_id = s.steps[0].call.tool_use_id.clone(),
            _ => s.steps[2].call.tool_use_id = s.steps[1].call.tool_use_id.clone(),
        }
        let found = outcome::situations(&s, "venv");
        assert_eq!(found[0].reward.unit, RewardUnit::NormalizedSteps, "variant {variant}");
        assert_eq!(found[0].reward.calls, 2);
        assert!(!found[0].immediate_next);
    }
}

#[test]
fn preplanned_calls_and_collection_only_are_not_immediate_repairs() {
    let mut preplanned = session(&[
        ("python -m pytest", "No module named pytest", true),
        (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
    ]);
    preplanned.steps[1].turn = preplanned.steps[0].turn;
    let found = outcome::situations(&preplanned, "venv");
    assert!(!found[0].immediate_next);
    assert!(found[0].next.is_none());
    assert!(!found[0].reward.reached);

    // A missing tool-bearing turn may contain an export/cd that ingest did not normalize.
    preplanned.steps[1].turn = preplanned.steps[0].turn + 2;
    let found = outcome::situations(&preplanned, "venv");
    assert!(!found[0].immediate_next);
    assert!(found[0].next.is_none());
    assert!(!found[0].reward.reached);

    let collection = session(&[
        ("python -m pytest", "No module named pytest", true),
        (".venv/bin/python -m pytest --collect-only", "2 tests collected in 0.1s", false),
    ]);
    let found = outcome::situations(&collection, "venv");
    assert!(!found[0].immediate_next);
    assert!(!found[0].reward.reached);
}

#[test]
fn a_passing_test_cannot_hide_a_parallel_mutation_later_in_transcript_order() {
    let mut s = session(&[
        ("python -m pytest", "No module named pytest", true),
        (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
        ("pip install pytest", "installed", false),
    ]);
    s.steps[2].turn = s.steps[1].turn;
    let found = outcome::situations(&s, "venv");
    assert!(!found[0].immediate_next);
    assert!(found[0].next.is_none());
    assert!(!found[0].reward.reached);
}

#[test]
fn denied_attempts_cost_a_call_but_do_not_prove_execution() {
    let s = session(&[
        ("python -m pytest", "No module named pytest", true),
        ("ls .venv", "<tool_use_error>Permission denied</tool_use_error>", true),
        (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
    ]);
    let found = outcome::situations(&s, "venv");
    assert_eq!(found[0].reward.calls, 2);
    assert!(found[0].reward.reached);
    assert!(!found[0].immediate_next);
    assert!(!outcome::local(&s, 0, Goal::Test, 1).reached);

    let denied_install = session(&[
        ("python -m pytest", "No module named pytest", true),
        ("pip install pytest", "<tool_use_error>Permission denied</tool_use_error>", true),
        (".venv/bin/python -m pytest", "2 passed in 0.1s", false),
    ]);
    let found = outcome::situations(&denied_install, "venv");
    assert_eq!(found[0].reward.calls, 2);
    assert!(!found[0].reward.harm, "a denied command did not execute");
}

#[test]
fn discovery_keeps_standard_system_readers_and_positive_partial_execution_reports() {
    let s = session(&[
        ("python3.11 -m pytest", "No module named pytest", true),
        ("ls -la; ls .venv/bin 2>/dev/null | head -30", "bin", false),
        ("/bin/ls -la .venv .venv/bin", "Permission to use Bash has been denied", true),
        (".venv/bin/python -m pytest | head", "2 passed in 0.1s", false),
    ]);
    let found = outcome::situations(&s, "venv");
    assert_eq!(found[0].reward.unit, RewardUnit::NativeCalls);
    assert_eq!(found[0].reward.calls, 3);
    assert!(found[0].reward.reached, "positive execution is the goal, not proof of overall pass status");
    assert!(!found[0].immediate_next);
    assert!(found[0].next.is_some());
}
