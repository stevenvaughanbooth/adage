use adage_trace::{EvidenceIdentity, GroupBasis, Session};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Case {
    name: String,
    repo: String,
    prompt: String,
    identity: EvidenceIdentity,
    expect: Option<String>,
    basis: Option<GroupBasis>,
}

#[test]
fn reviewed_identity_fixtures_form_exactly_the_expected_groups() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/identity.json")).unwrap();
    let mut actual_to_expected = BTreeMap::new();
    let mut expected_to_actual = BTreeMap::new();
    for c in cases {
        let group = c.identity.group(&c.repo, &c.prompt);
        assert_eq!(group.as_ref().map(|g| g.basis), c.basis, "{}", c.name);
        match (group, c.expect) {
            (Some(g), Some(expected)) => {
                if let Some(before) = actual_to_expected.insert(g.key.clone(), expected.clone()) {
                    assert_eq!(before, expected, "different tasks merged: {}", c.name);
                }
                if let Some(before) = expected_to_actual.insert(expected, g.key.clone()) {
                    assert_eq!(before, g.key, "related attempts split: {}", c.name);
                }
            }
            (None, None) => {}
            _ => panic!("wrong identity presence: {}", c.name),
        }
    }
    assert_eq!(actual_to_expected.len(), 5);
}

#[test]
fn full_prompt_suffix_and_declared_namespace_are_part_of_identity() {
    let prefix = "Shared enterprise task instructions. ".repeat(20);
    let id = EvidenceIdentity::default();
    let a = id.group("repo", &format!("{prefix}fix billing")).unwrap();
    let b = id.group("repo", &format!("{prefix}fix search")).unwrap();
    assert_ne!(a.key, b.key, "the old 240-character prefix erased the actual task");
    let declared = EvidenceIdentity { task_id: Some("fix billing".into()), ..Default::default() };
    assert_ne!(declared.group("repo", "").unwrap(), id.group("repo", "fix billing").unwrap());
    assert!(!a.key.contains("billing"));
}

#[test]
fn legacy_sessions_remain_readable_and_identity_round_trips() {
    let mut session: Session = serde_json::from_value(serde_json::json!({
        "id":"old", "root":"/fixture", "repo":"repo", "model":null, "day":"2026-10-01",
        "prompt":"fix billing", "steps":[], "steered":false, "directed":[], "outcome":null,
        "harness":"codex"
    }))
    .unwrap();
    assert!(session.identity.is_empty());
    assert_eq!(session.evidence_group().unwrap().basis, GroupBasis::PromptFingerprint);
    session.identity = EvidenceIdentity { task_id: Some("TASK-7".into()), ..Default::default() };
    let round_trip: Session = serde_json::from_slice(&serde_json::to_vec(&session).unwrap()).unwrap();
    assert_eq!(round_trip, session);
}
