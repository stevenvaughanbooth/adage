//! Sanitized regressions for native shell status differing from the runner's own result.

use adage_core::outcome::VerificationOutcome;
use adage_core::CommandTable;
use adage_graph::{canonical, compile::Corpus, embed::Embedder, events::NodeType, facts};
use adage_trace::{By, Call, Session};
use serde_json::json;

fn session(command: &str, output: &str, transport_failed: bool) -> Session {
    let call = Call {
        tool: "Bash".into(),
        input: json!({"command":command}),
        output: output.into(),
        failed: transport_failed,
        by: By::Agent,
        tool_use_id: Some("test-call".into()),
        agent: None,
    };
    let norms = adage_core::tools::normalize_tool("Bash", &call.input, "/project", CommandTable::load());
    let steps = adage_ingest::signals::call_steps(0, 0, &call, norms, std::path::Path::new("/project"));
    Session {
        id: "s".into(),
        identity: Default::default(),
        delegation: None,
        root: "/project".into(),
        repo: "project".into(),
        model: None,
        day: "2026-01-01".into(),
        prompt: "repair the parser".into(),
        steps,
        steered: false,
        directed: Default::default(),
        influence: vec![],
        outcome: None,
        harness: "claude-code".into(),
        prompts: vec![],
        ending: Default::default(),
    }
}

#[test]
fn piped_javascript_and_cargo_failures_cannot_supply_successful_commands() {
    for (command, output) in [
        ("cd /project && pnpm test 2>&1 | tail -30", "Test Files 1 failed | 2 passed (3)\nTests 1 failed | 14 passed (15)"),
        ("cargo test --workspace 2>&1 | grep -E 'test result|FAILED' | head -40", "test result: ok. 2 passed; 0 failed; 0 ignored\ntest result: FAILED. 3 passed; 1 failed; 0 ignored"),
        ("cargo build --all-targets 2>&1 | tail -20", "error[E0063]: missing field `entry` in initializer of `Record`\nerror: could not compile `sample` (bin test) due to 1 previous error"),
    ] {
        let s = session(command, output, false);
        assert!(!s.steps[0].call.failed, "original tool transport is retained");
        let a = canonical::annotate(&s);
        assert_eq!(a.meta[0].verification.unwrap().outcome, VerificationOutcome::Failed, "{command}");
        let e = canonical::events(&a, &s);
        let acts: Vec<_> = e.events.iter().filter(|e| e.ty == NodeType::Act).collect();
        assert_eq!(acts.len(), 1, "{command}");
        assert!(!acts[0].ok);
        assert!(acts[0].signature.is_some());
        let corpus = Corpus::of(std::slice::from_ref(&a), &[]);
        assert!(corpus.acts.values().all(|a| a.ok_sessions == 0 && a.literal_ok.is_empty()));
        assert!(corpus.runners_passed.is_empty());
        let f = facts::facts(&a, &Embedder::new(None));
        assert!(f.events.iter().any(|e| matches!(e, facts::Event::Act { ok: false, .. })));
        assert_eq!(f.fails.len(), 1);
    }
}

#[test]
fn unknown_and_no_tests_are_neither_successes_nor_mined_failure_examples() {
    for (command, output) in [
        ("pytest", ""),
        ("pytest", "no tests ran in 0.01s"),
        ("pytest --collect-only", "2 tests collected in 0.01s"),
        ("pytest && pytest", "1 passed in 0.01s"),
        ("cat fixture.txt; pytest", "1 failed in 0.01s"),
        ("pytest | head", "1 passed in 0.01s"),
    ] {
        let s = session(command, output, false);
        let a = canonical::annotate(&s);
        let e = canonical::events(&a, &s);
        assert!(e.events.iter().all(|e| e.ty != NodeType::Act), "{command}");
        let corpus = Corpus::of(std::slice::from_ref(&a), &[]);
        assert!(corpus.acts.is_empty(), "{command}");
        assert!(!corpus.runners.is_empty(), "unknown still counts as a runner opportunity");
        assert!(corpus.runners_passed.is_empty());
        let f = facts::facts(&a, &Embedder::new(None));
        assert!(f.fails.is_empty(), "{command}");
    }
}

#[test]
fn a_read_of_failing_fixture_text_is_not_a_verification_and_real_direct_passes_survive() {
    let read = session("cat fixture.txt", "Tests: 1 failed, 1 total", false);
    assert!(read.steps.iter().all(|s| s.verification().is_none()));
    let input = canonical::annotate(&read);
    assert!(Corpus::of(&[input], &[]).acts.is_empty());

    let s = session("pytest -q", "2 passed in 0.01s", false);
    let a = canonical::annotate(&s);
    assert_eq!(a.meta[0].verification.unwrap().outcome, VerificationOutcome::Passed);
    let e = canonical::events(&a, &s);
    assert!(e.events.iter().any(|e| e.ty == NodeType::Act && e.ok));
    let corpus = Corpus::of(&[a], &[]);
    assert!(corpus.acts.values().all(|a| a.ok_sessions == 1));
    assert_eq!(s.steps[0].call.output, "2 passed in 0.01s");
}
