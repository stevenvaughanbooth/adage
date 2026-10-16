use super::*;
use adage_ingest::format::Decode;
use serde_json::json;

fn session(task: &str, command: &str, description: &str, output: &str) -> Session {
    let records = [
        json!({"type":"user","sessionId":"persistence-worker","cwd":"/repo","message":{"content":task}}),
        json!({"type":"assistant","message":{"id":"m1","content":[
            {"type":"tool_use","id":"check-1","name":"Bash","input":{"command":command,"description":description}}]}}),
        json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"check-1","content":output,"is_error":false}]}}),
    ];
    let raw = records.iter().map(serde_json::Value::to_string).collect::<Vec<_>>().join("\n");
    let cx = Decode {
        table: adage_core::CommandTable::load(),
        locator: &adage_ingest::locate::AsRecorded,
        repo_override: Some("example/store"),
        subagent: false,
    };
    adage_ingest::claude::decode(raw.as_bytes(), &cx).unwrap()
}

fn proposals(s: &Session) -> Vec<Proposal> {
    learn(&[s]).proposals.into_iter().filter(|p| p.kind == ProcedureKind::PersistenceEquivalence).collect()
}

const PASS: &str = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s";
const FAIL: &str = "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s";

#[test]
fn an_owned_roundtrip_description_with_a_runner_report_is_an_investigative_proposal() {
    let s = session(
        "Preserve the serialized index",
        "cargo test --test roundtrip | tail -n 5",
        "Run index round-trip regression test",
        PASS,
    );
    let p = proposals(&s);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].status, "needs_evidence_review");
    assert_eq!(p[0].template, None, "no qualified live binding is inferred");
    let c = &p[0].episodes[0].checks[0];
    assert_eq!(c.claimed_purpose, Some(ProcedureKind::PersistenceEquivalence));
    assert!(c.named_case_observations.is_empty());
    assert_eq!(c.outcome, VerificationOutcome::Unknown, "filtered output ownership stays unknown");
    assert_eq!(c.reported.outcome, VerificationOutcome::Passed);
    assert!(p[0].episodes[0].observation.contains("setup, assertions and any repair remain unverified"));
    assert_eq!(s.outcome, None);
}

#[test]
fn a_runner_case_line_and_task_context_form_a_distinct_weak_observation() {
    let output = format!("test incremental_equals_rebuild_equals_bulk ... FAILED\n{FAIL}");
    let s = session(
        "Implement incremental persistent storage",
        "cargo test -p record_store | tail -n 20",
        "Run tests",
        &output,
    );
    let p = proposals(&s);
    assert_eq!(p.len(), 1);
    let c = &p[0].episodes[0].checks[0];
    assert_eq!(c.claimed_purpose, None, "stdout is never an agent-owned claim");
    assert_eq!(
        c.named_case_observations,
        [NamedCaseObservation { name: "incremental_equals_rebuild_equals_bulk".into(), reported_failed: true }]
    );
    assert!(p[0].episodes[0].observation.contains("invocation ownership and the invariant remain unverified"));
    assert!(p[0].requirements.iter().any(|s| s.contains("actual test assertions")));
}

#[test]
fn fixture_text_quoted_lines_and_unrecognized_runners_do_not_supply_named_case_evidence() {
    for line in [
        "\"test incremental_equals_bulk ... FAILED\"",
        "const OUTPUT: &str = \"test incremental_equals_bulk ... FAILED\";",
        "test incremental_equals_bulk ... ignored",
        "test result: incremental equals bulk",
    ] {
        let s = session("Implement persistent storage", "cargo test", "Run tests", &format!("{line}\n{PASS}"));
        assert!(proposals(&s).is_empty(), "not a per-case line: {line}");
    }
    for command in ["cat test.log", "python print_fixture.py", "printf test"] {
        let s = session(
            "Implement persistent storage",
            command,
            "Run index roundtrip test",
            &format!("test incremental_equals_bulk ... FAILED\n{FAIL}"),
        );
        assert!(proposals(&s).is_empty(), "not a recognized test invocation: {command}");
    }
}

#[test]
fn network_roundtrips_generic_rebuilds_and_missing_method_context_abstain() {
    for (task, description, line) in [
        ("Measure network latency", "Test network round trip", "test network_roundtrip ... ok"),
        ("Rebuild the compiler", "Run incremental rebuild compilation", "test incremental_rebuild_compiles ... ok"),
        ("Compare network requests", "Run tests", "test incremental_equals_bulk ... ok"),
        ("Test persisted state", "Run tests", "test a_torn_state_write_reads_as_a_written_state ... ok"),
        ("Implement persistent corpus storage", "Run tests", "test serve::fsm::corpus::tests::days_round_trip ... ok"),
    ] {
        let s = session(task, "cargo test", description, &format!("{line}\n{PASS}"));
        assert!(proposals(&s).is_empty(), "unqualified method: {description}");
    }
}

#[test]
fn directed_persistence_leads_and_method_descriptions_without_reports_are_excluded() {
    let mut s = session("Preserve persisted state", "cargo test", "Run JSON roundtrip regression", PASS);
    s.influence.push(adage_trace::Directed { at_step: 0, all: true, ..Default::default() });
    assert!(proposals(&s).is_empty());
    let s = session("Preserve persisted state", "cargo test", "Run JSON roundtrip regression", "roundtrip true");
    assert!(proposals(&s).is_empty(), "descriptions cannot invent a runner report");
}

#[test]
fn a_namespaced_case_keeps_its_full_anchor_but_uses_only_its_leaf_method() {
    let name = "engine::tests::incremental_equals_rebuild_equals_bulk";
    let s = session(
        "Implement incremental persistent storage",
        "cargo test",
        "Run tests",
        &format!("test {name} ... FAILED\n{FAIL}"),
    );
    let p = proposals(&s);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].episodes[0].checks[0].named_case_observations[0].name, name);
}
