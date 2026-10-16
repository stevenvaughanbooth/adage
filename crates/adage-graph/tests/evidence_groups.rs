use adage_graph::{canonical, mine::DbSession, score::inner_folds};
use adage_trace::Session;

fn session(id: &str, prompt: &str, identity: serde_json::Value) -> Session {
    serde_json::from_value(serde_json::json!({
        "id":id, "identity":identity, "root":"/fixture", "repo":"repo", "model":null,
        "day":"2026-10-01", "prompt":prompt, "steps":[], "steered":false, "directed":[],
        "outcome":null, "harness":"codex"
    }))
    .unwrap()
}

#[test]
fn canonical_bridge_retains_declared_and_inferred_identity() {
    let cases = [
        session("original", "Fix billing", serde_json::json!({"task_id":"TASK-7"})),
        session("retry", "Repair billing", serde_json::json!({"task_id":"TASK-7"})),
        session("inferred", "Fix search", serde_json::json!({})),
        session("unknown", "", serde_json::json!({})),
    ];
    let events: Vec<_> = cases.iter().map(|s| canonical::events(&canonical::annotate(s), s)).collect();
    assert!(events[0].task.is_some());
    assert_eq!(events[0].task, events[1].task);
    assert!(events[2].task.is_some());
    assert_ne!(events[0].task, events[2].task);
    assert_eq!(events[3].task, None);
}

#[test]
fn inner_folds_keep_retries_and_unknowns_together_in_any_order() {
    let make = |id: &str, task: Option<&str>| DbSession {
        id: id.into(),
        day: 0,
        task: task.map(str::to_string),
        ev: vec![],
        orig: vec![],
        flows: vec![],
        same_art: vec![],
        last: vec![],
        exposed: vec![],
    };
    let mut sessions = vec![
        make("attempt-1", Some("A")),
        make("attempt-2", Some("A")),
        make("independent", Some("B")),
        make("unknown-1", None),
        make("unknown-2", None),
    ];
    assert_eq!(inner_folds(&sessions, 3), vec![1, 1, 2, 0, 0]);
    sessions.reverse();
    assert_eq!(inner_folds(&sessions, 3), vec![0, 0, 2, 1, 1]);
    assert_eq!(inner_folds(&sessions, 0), vec![0; 5]);
}
