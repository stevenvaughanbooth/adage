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

/// A session that tries a runner the repository does not have, then the one it does: a dead end
/// learning turns into a reflex.
fn dead_end(sid: &str) -> Vec<u8> {
    let cwd = "/nowhere/on/this/machine/acme";
    let call = |id: &str, cmd: &str| {
        json!({"type": "assistant", "sessionId": sid, "cwd": cwd,
               "message": {"id": format!("m-{id}"), "content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": cmd}}]}})
    };
    let result = |id: &str, out: &str, failed: bool| {
        json!({"type": "user", "sessionId": sid, "cwd": cwd,
               "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": out, "is_error": failed}]}})
    };
    [
        json!({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": "2026-09-01T10:00:00Z",
               "message": {"role": "user", "content": "fix the failing test"}}),
        call("t1", "python3 -m pytest"),
        result("t1", "/usr/bin/python3: No module named pytest", true),
        call("t2", "make test"),
        result("t2", "ok", false),
    ]
    .iter()
    .map(|l| format!("{l}\n"))
    .collect::<String>()
    .into_bytes()
}

/// A store that keeps what a learn run materializes in memory (a team service's rows, here).
#[derive(Default)]
struct Rows(std::sync::Mutex<Vec<String>>);

impl Rows {
    fn push(&self, row: String) {
        self.0.lock().unwrap().push(row);
    }
}

impl adage_learn::store::Store for Rows {
    fn install_base_packs(&self) {
        self.push("base packs".into());
    }
    fn write_track(&self, p: &adage_learn::Planned) -> anyhow::Result<()> {
        self.push(format!("track {} ({} routes)", p.track.manifest.track.name, p.track.routes.len()));
        Ok(())
    }
    fn write_binding(&self, repo: &str, _: &adage_pack::binding::Binding) {
        self.push(format!("binding {repo}"));
    }
    fn write_follow_weights(&self, repo: &str, _: &adage_core::judge::Weights) {
        self.push(format!("follow weights {repo}"));
    }
    fn compile_index(&self) -> anyhow::Result<adage_learn::store::Compiled> {
        self.push("index".into());
        Ok(adage_learn::store::Compiled { generation: "g1".into(), conflicts: vec![] })
    }
    fn index_generation(&self) -> String {
        "g0".into()
    }
}

/// Everything a learn run decides reaches state only through its store: with a store that keeps
/// rows in memory, nothing is written to disk, and the index is compiled once, after every row.
#[test]
fn a_learn_run_materializes_only_through_its_store() {
    let source = Memory((0..8).map(|i| (format!("s{i}"), dead_end(&format!("s{i}")))).collect());
    let table = adage_core::CommandTable::load();
    let cx =
        Decode { table: &table, locator: &AsRecorded, repo_override: Some("github.com/acme/app"), subagent: false };
    let mut batch = adage_ingest::batch::Batch::default();
    adage_ingest::ingest(&source, &cx, |_, s| {
        batch.read += 1;
        batch.sessions.extend(s);
    });

    let d = tempfile::tempdir().unwrap();
    let rows = Rows::default();
    let cfg = LearnConfig { instincts: None, ..LearnConfig::demo() };
    let report = adage_learn::learn_sessions_into(batch, None, None, &d.path().join("tracks"), &rows, &cfg).unwrap();
    assert_eq!(report.index_generation, "g1", "the store's index");
    let rows = rows.0.into_inner().unwrap();
    assert!(rows.iter().any(|r| r.starts_with("track github-com-acme-app/learned")), "{rows:?}");
    assert!(rows.contains(&"follow weights github.com/acme/app".to_string()), "{rows:?}");
    assert_eq!(rows.iter().filter(|r| *r == "index").count(), 1, "{rows:?}");
    assert_eq!(rows.last().map(String::as_str), Some("index"), "compiled after every row: {rows:?}");
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0, "nothing on disk");
}
