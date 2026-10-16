//! Cost units and immediate action choices are separate from eventual repair evidence.

use super::*;
use adage_core::CommandTable;
use adage_ingest::{format::Decode, locate::AsRecorded};
use serde_json::{json, Value};

fn session(calls: &[(&str, Value, &str, bool)]) -> Session {
    let mut lines = vec![json!({"type":"user", "sessionId":"value-accounting", "cwd":"/fixture/project",
        "message":{"content":"check the parser"}})];
    for (i, (tool, input, output, failed)) in calls.iter().enumerate() {
        let id = format!("call-{i}");
        lines.push(json!({"type":"assistant", "cwd":"/fixture/project",
            "message":{"id":format!("turn-{i}"), "content":[
                {"type":"tool_use", "id":id, "name":tool, "input":input}]}}));
        lines.push(json!({"type":"user", "cwd":"/fixture/project", "message":{"content":[
            {"type":"tool_result", "tool_use_id":id, "content":output, "is_error":failed}]}}));
    }
    let text = lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    let cx = Decode {
        table: CommandTable::load(),
        locator: &AsRecorded,
        repo_override: Some("github.com/example/project"),
        subagent: false,
    };
    adage_ingest::claude::decode(text.as_bytes(), &cx).unwrap()
}

fn failure() -> (&'static str, Value, &'static str, bool) {
    ("Bash", json!({"command":"python -m pytest"}), "No module named pytest", true)
}

fn success() -> (&'static str, Value, &'static str, bool) {
    ("Bash", json!({"command":".venv/bin/python -m pytest"}), "2 passed in 0.1s", false)
}

fn statics() -> Statics {
    Statics {
        env_kind: "venv".into(),
        declared: BTreeMap::from([("pytest".into(), (Env::Venv(".venv".into()), ".venv/bin/python -m pytest".into()))]),
    }
}

fn valued(s: &Session, p: &Priors) -> Valued {
    value_failures(&[s], &statics(), p, UPLIFT_MIN)
        .into_iter()
        .find(|v| v.command == ".venv/bin/python -m pytest")
        .expect("declared candidate remains visible")
}

#[test]
fn delayed_repair_is_evidence_but_not_an_immediate_choice_or_prior_only_promotion() {
    let s = session(&[
        failure(),
        ("Read", json!({"file_path":"/fixture/project/.venv/pyvenv.cfg"}), "version = 3.12", false),
        success(),
    ]);
    // Even an unusually favorable fitted cost prior cannot stand in for an observed immediate
    // choice. This one real-shaped example must not be repeated to manufacture support.
    let mut p = Priors::default();
    p.cells.insert("*|*|*|null".into(), Cell { n: 100.0, sum: 2_000.0, kappa: Some(100.0), misfire: 0.0 });
    p.cells.insert("*|*|*|venv".into(), Cell { n: 100.0, sum: 100.0, kappa: Some(100.0), misfire: 0.0 });
    let v = valued(&s, &p);
    assert_eq!((v.n_null, v.n_sat, v.ran_tests, v.declared), (1, 0, 1, true));
    assert!(v.advantage.mean > 0.0, "test must exercise the otherwise favorable prior: {v:?}");
    assert_eq!(v.strength, None);
    assert!(failure_routes(&[v], &[&s], "test").is_empty());
    let rows = rows(&s, "venv");
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].calls, rows[0].unit, rows[0].role.as_deref()), (2, RewardUnit::NativeCalls, None));
}

#[test]
fn immediate_choice_requires_the_same_runner_family_and_environment() {
    let immediate = session(&[failure(), success()]);
    let v = valued(&immediate, &Priors::default());
    assert_eq!((v.n_null, v.n_sat, v.ran_tests), (1, 1, 1));
    assert_eq!(rows(&immediate, "venv")[0].role.as_deref(), Some("venv"));

    let other_family = session(&[
        failure(),
        ("Bash", json!({"command":".venv/bin/python -m unittest"}), "Ran 1 test in 0.1s\nOK", false),
        success(),
    ]);
    let v = valued(&other_family, &Priors::default());
    assert_eq!((v.n_null, v.n_sat, v.ran_tests), (1, 0, 1));
    assert_eq!(rows(&other_family, "venv")[0].role, None);

    let other_environment = session(&[
        failure(),
        ("Bash", json!({"command":".other/bin/python -m pytest"}), "2 passed in 0.1s", false),
        success(),
    ]);
    let v = valued(&other_environment, &Priors::default());
    assert_eq!(v.n_sat, 0);
}

#[test]
fn flattened_observation_counts_one_native_call_without_becoming_an_immediate_choice() {
    for failed in [false, true] {
        let s = session(&[
            failure(),
            ("Bash", json!({"command":"ls .; ls .venv"}), "src tests\nbin lib", failed),
            success(),
        ]);
        assert_eq!(s.steps.len(), 4, "two lists from one native call");
        assert!(!s.steps[1].call.failed);
        assert_eq!(s.steps[2].call.failed, failed);
        let row = rows(&s, "venv").remove(0);
        assert_eq!((row.calls, row.unit, row.role), (2, RewardUnit::NativeCalls, None));
        let v = valued(&s, &Priors::default());
        assert_eq!((v.n_null, v.n_sat), (1, 0));
    }
}

#[test]
fn old_steps_without_native_identity_do_not_fit_native_costs_or_promote() {
    let mut s = session(&[failure(), success()]);
    for st in &mut s.steps {
        st.call.tool_use_id = None;
    }
    let v = valued(&s, &Priors::default());
    assert_eq!((v.n_null, v.n_sat, v.ran_tests), (0, 0, 1));
    assert_eq!(v.strength, None);
    let rows = rows(&s, "venv");
    assert_eq!(rows[0].unit, RewardUnit::NormalizedSteps);
    assert_eq!(rows[0].role, None);
    let mut p = Priors::default();
    p.add(&rows);
    assert!(p.cells.is_empty());
}

#[test]
fn an_opaque_native_sibling_is_not_lost_to_immediate_recovery_accounting() {
    let mut s = session(&[failure(), ("Bash", json!({"command":"export PROJECT_ENV=test"}), "", false), success()]);
    assert_eq!(s.steps.len(), 3, "ingestion must retain the otherwise empty native call");
    // Two calls issued in one turn: the opaque call cannot disappear just because the test's
    // result is attributable. The decoder's corresponding native-envelope regression is in
    // adage-ingest::signals_tests.
    s.steps[2].turn = s.steps[1].turn;
    let situations = outcome::situations(&s, "venv");
    assert_eq!(situations.len(), 1);
    assert!(!situations[0].immediate_next);
    assert_eq!(situations[0].next_at, None);
    let v = valued(&s, &Priors::default());
    assert_eq!(v.n_sat, 0);
    assert_eq!(v.strength, None);
}

#[test]
fn legacy_fitted_costs_are_readable_but_not_reused_or_mixed() {
    let mut p: Priors = serde_json::from_value(json!({
        "cells":{"*|*|*|venv":{"n":100,"sum":1,"misfire":0,"kappa":100}},
        "uptake":{"read-batch":{"n":40,"took":10,"kappa":6},"avoid":{"n":100,"took":99,"kappa":50}}
    }))
    .unwrap();
    let s = session(&[failure(), success()]);
    let rows = rows(&s, "venv");
    assert_eq!(p.outcome_format, 0);
    assert_eq!(p.prior(&rows[0].key, "venv"), (DEFAULT_CALLS, DEFAULT_KAPPA, MISFIRE_PRIOR.0, MISFIRE_PRIOR.1));
    // Uptake is not a fitted cost: its existing route-following semantics stay separate.
    assert_eq!(p.self_prior("read-batch"), (0.25, 6.0, true));
    assert_eq!(p.self_prior("avoid"), (0.29, SELF_KAPPA, false));
    p.add(&rows);
    assert_eq!(p.outcome_format, OUTCOME_FORMAT);
    assert!(!p.uptake.contains_key("avoid"));
    assert_eq!(p.self_prior("read-batch"), (0.25, 6.0, true));
    assert_eq!((p.cells["*|*|*|venv"].n, p.cells["*|*|*|venv"].sum), (1.0, 1.0));
    let roundtrip: Priors = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
    assert_eq!(roundtrip, p);
}

#[test]
fn legacy_rows_and_mixed_units_cannot_be_relabeled_as_current_fit_data() {
    let s = session(&[failure(), success()]);
    let current = rows(&s, "venv").remove(0);
    let mut old = serde_json::to_value(&current).unwrap();
    old.as_object_mut().unwrap().remove("unit");
    old.as_object_mut().unwrap().remove("outcome_format");
    let old: Row = serde_json::from_value(old).unwrap();
    let mut normalized = current.clone();
    normalized.unit = RewardUnit::NormalizedSteps;
    let mut future = current.clone();
    future.outcome_format += 1;
    let mut p = Priors::default();
    p.add(&[old, normalized, future, current]);
    assert_eq!((p.cells["*|*|*|null"].n, p.cells["*|*|*|null"].sum), (1.0, 1.0));
    assert_eq!((p.cells["*|*|*|venv"].n, p.cells["*|*|*|venv"].sum), (1.0, 1.0));
}

fn startup_avoid() -> Route {
    Route {
        id: "startup-avoid".into(),
        summary: "A learned environment dead end".into(),
        trigger: Trigger::SessionStart,
        step: Step::Avoid {
            avoid: "python -m pytest".into(),
            instead: ".venv/bin/python -m pytest".into(),
            signature: "No module named pytest".into(),
        },
        effect: Effect::Verify,
        slots: vec![],
        strength: Strength::Inhibit,
        state: RouteState::Firm,
        exit: vec![],
        evidence: Evidence { support: 6, opportunities: 6, provenance: "desire-path".into(), ..Default::default() },
        authored: false,
    }
}

#[test]
fn startup_recovery_support_is_not_unaided_prevention_even_when_previously_cached() {
    let s = session(&[
        failure(),
        ("Read", json!({"file_path":"/fixture/project/.venv/pyvenv.cfg"}), "version = 3.12", false),
        success(),
    ]);
    let mut r = startup_avoid();
    r.evidence.uplift = Some(
        uplift("avoid", &Unaided { traces: 6, traces_took: 6, ..Default::default() }, &Priors::default(), UPLIFT_MIN)
            .summary,
    );
    assert_eq!(unaided_in_traces(&r, &[&s]), Some((0, 1)));
    assert_eq!(trace_counts(&r, &[&s]), Some((0, 1)));
    assert_eq!(uptake_units(&[r.clone()], &[&s]), vec![("avoid".into(), 0.0, 1.0)]);
    assess(std::slice::from_mut(&mut r), &[&s], "repo", &BTreeMap::new(), &Priors::default(), UPLIFT_MIN);
    let u = r.evidence.uplift.unwrap();
    assert_eq!((u.traces_took, u.traces), (0, 1));
}

#[test]
fn startup_working_choice_can_follow_safe_observations_without_being_delayed_recovery() {
    let s = session(&[
        ("Read", json!({"file_path":"/fixture/project/README.md"}), "Project documentation", false),
        ("Bash", json!({"command":"ls .venv/bin 2>/dev/null | head -30"}), "python", false),
        success(),
        failure(), // A later decision does not change the first relevant choice.
    ]);
    assert_eq!(trace_counts(&startup_avoid(), &[&s]), Some((1, 1)));
    let verbose = session(&[("Bash", json!({"command":".venv/bin/python -m pytest -q"}), "2 passed in 0.1s", false)]);
    assert!(verbose.steps[0].project_pytest_root_cwd);
    assert_eq!(trace_counts(&startup_avoid(), &[&verbose]), Some((1, 1)));
    let red = session(&[("Bash", json!({"command":".venv/bin/python -m pytest"}), "1 failed, 1 passed in 0.1s", true)]);
    assert_eq!(
        trace_counts(&startup_avoid(), &[&red]),
        Some((1, 1)),
        "test failure is still the intended runner choice"
    );
}

#[test]
fn startup_choice_counts_a_failed_compound_observation_once_but_rejects_conflicting_identity() {
    let observation = ("Bash", json!({"command":"ls .; ls .venv"}), "ls: missing directory", true);
    let s = session(&[observation.clone(), success()]);
    assert_eq!(s.steps.len(), 3);
    assert!(!s.steps[0].call.failed);
    assert!(s.steps[1].call.failed);
    assert_eq!(trace_counts(&startup_avoid(), &[&s]), Some((1, 1)));
    let failed_first = session(&[observation, failure(), success()]);
    assert_eq!(trace_counts(&startup_avoid(), &[&failed_first]), Some((0, 1)));
    for conflict in ["input", "output", "reused", "refused"] {
        let mut changed = s.clone();
        match conflict {
            "input" => changed.steps[1].call.input["command"] = json!("ls elsewhere"),
            "output" => changed.steps[1].call.output = "different result".into(),
            "reused" => changed.steps[2].call.tool_use_id = changed.steps[0].call.tool_use_id.clone(),
            "refused" => changed.steps[..2]
                .iter_mut()
                .for_each(|st| st.call.output = "<tool_use_error>denied</tool_use_error>".into()),
            _ => unreachable!(),
        }
        assert_eq!(trace_counts(&startup_avoid(), &[&changed]), Some((0, 0)), "{conflict}");
    }
}

#[test]
fn startup_avoid_requires_real_complete_target_execution_and_exact_scope() {
    for call in [
        ("Bash", json!({"command":".venv/bin/python -m pytest tests/test_one.py"}), "1 passed in 0.1s", false),
        ("Bash", json!({"command":".venv/bin/python -m pytest"}), "no tests ran in 0.1s", false),
        ("Bash", json!({"command":".venv/bin/python -m pytest"}), "2 skipped in 0.1s", false),
        ("Bash", json!({"command":".venv/bin/python -m pytest"}), "No module named pytest", true),
        ("Bash", json!({"command":".venv/bin/python -m pytest | head -30"}), "2 passed in 0.1s", false),
        ("Bash", json!({"command":"echo '2 passed in 0.1s'"}), "2 passed in 0.1s", false),
        ("Bash", json!({"command":"python -m pytest"}), "<tool_use_error>permission denied</tool_use_error>", true),
    ] {
        let s = session(&[call.clone(), success()]);
        assert_eq!(trace_counts(&startup_avoid(), &[&s]), Some((0, 0)), "{call:?}");
    }
    let s = session(&[("Bash", json!({"command":"python -m pytest"}), "2 passed in 0.1s", false), success()]);
    assert_eq!(trace_counts(&startup_avoid(), &[&s]), Some((0, 1)), "an actual avoided attempt is not prevention");
}

#[test]
fn startup_avoid_censors_opaque_or_mutating_work_before_the_choice() {
    for barrier in [
        ("Edit", json!({"file_path":"/fixture/project/src/a.py","old_string":"a","new_string":"b"}), "updated", false),
        ("Bash", json!({"command":"export PROJECT_ENV=test"}), "", false),
        ("Bash", json!({"command":"cat README.md > src/a.py"}), "", false),
        ("Bash", json!({"command":"rg --pre ./fix.py parser src"}), "found", false),
        ("Bash", json!({"command":"pip install pytest"}), "Successfully installed pytest", false),
    ] {
        let s = session(&[barrier.clone(), success()]);
        assert_eq!(trace_counts(&startup_avoid(), &[&s]), Some((0, 0)), "{barrier:?}");
    }
}

#[test]
fn startup_avoid_requires_one_attributable_native_chronology() {
    let original = session(&[("Read", json!({"file_path":"/fixture/project/README.md"}), "docs", false), success()]);
    for boundary in ["id", "reused", "actor", "prompt", "cwd", "turn-gap", "prefix-gap", "same-turn", "adage"] {
        let mut s = original.clone();
        match boundary {
            "id" => s.steps[0].call.tool_use_id = None,
            "reused" => s.steps[1].call.tool_use_id = s.steps[0].call.tool_use_id.clone(),
            "actor" => s.steps[1].call.agent = Some("other-agent".into()),
            "prompt" => s.steps[1].prompt += 1,
            "cwd" => s.steps[1].call.input["cwd"] = json!("/fixture/project/subdir"),
            "turn-gap" => s.steps[1].turn += 1,
            "prefix-gap" => s.steps.iter_mut().for_each(|st| st.turn += 5),
            "same-turn" => s.steps[1].turn = s.steps[0].turn,
            "adage" => s.steps[1].call.by = adage_trace::By::Adage,
            _ => unreachable!(),
        }
        assert_eq!(trace_counts(&startup_avoid(), &[&s]), Some((0, 0)), "{boundary}");
    }
    let mut s = session(&[success()]);
    s.influence.push(serde_json::from_value(json!({"at_step":0,"commands":[".venv/bin/python -m pytest"]})).unwrap());
    assert_eq!(trace_counts(&startup_avoid(), &[&s]), Some((0, 0)), "directed traces are not unaided");
}

#[test]
fn startup_other_shapes_keep_their_existing_counts_and_old_avoid_priors_are_invalidated() {
    let r = Route {
        step: Step::Run { command: "make test".into() },
        evidence: Evidence { support: 4, opportunities: 5, ..Default::default() },
        ..startup_avoid()
    };
    assert_eq!(trace_counts(&r, &[]), Some((4, 5)));
    let mut p = Priors { outcome_format: 2, ..Priors::default() };
    p.uptake.insert("avoid".into(), SelfCell { n: 6.0, took: 6.0, kappa: Some(6.0) });
    assert_eq!(p.self_prior("avoid"), (0.29, SELF_KAPPA, false));
    p.add(&rows(&session(&[failure(), success()]), "venv"));
    assert_eq!(p.outcome_format, 3);
    assert!(!p.uptake.contains_key("avoid"));
}
