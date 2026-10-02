//! Traces collected elsewhere (a team's trace store): decoded from bytes with no checkout on this
//! machine, then learned from through the same entry point as local history.

use adage_ingest::{source::Memory, AsRecorded, Decode};
use adage_learn::{learn_sessions, LearnConfig};
use serde_json::json;

fn transcript(sid: &str) -> Vec<u8> {
    let cwd = "/nowhere/on/this/machine/acme";
    [
        json!({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": "2026-09-01T10:00:00Z",
               "message": {"role": "user", "content": "fix the failing test"}}),
        json!({"type": "assistant", "sessionId": sid, "cwd": cwd,
               "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make test"}}]}}),
        json!({"type": "user", "sessionId": sid, "cwd": cwd,
               "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok", "is_error": false}]}}),
    ]
    .iter()
    .map(|l| format!("{l}\n"))
    .collect::<String>()
    .into_bytes()
}

#[test]
fn sessions_from_another_machine_are_learned_like_local_ones() {
    let source = Memory((0..4).map(|i| (format!("s{i}"), transcript(&format!("s{i}")))).collect());
    let table = adage_core::CommandTable::load();
    let cx =
        Decode { table: &table, locator: &AsRecorded, repo_override: Some("github.com/acme/app"), subagent: false };
    let mut batch = adage_ingest::batch::Batch::default();
    adage_ingest::ingest(&source, &cx, |_, s| {
        batch.read += 1;
        batch.sessions.extend(s);
    });
    assert_eq!(batch.sessions.len(), 4);

    let d = tempfile::tempdir().unwrap();
    let (tracks, index) = (d.path().join("tracks"), d.path().join("index.json"));
    let cfg = LearnConfig { instincts: None, ..LearnConfig::default() };
    let report = learn_sessions(batch, None, None, &tracks, &index, &cfg).unwrap();
    assert_eq!((report.sessions_read, report.sessions_used), (4, 4));
    assert_eq!(report.repos.keys().collect::<Vec<_>>(), ["github.com/acme/app"]);
    assert!(index.exists(), "the index is compiled");
}
