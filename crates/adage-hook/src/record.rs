//! The spool writer's shared steps. Every hook path that writes a spool record builds its own
//! fields (each kind's shape is its own), then goes through here for what all of them share: the
//! wall-clock stamp (`ts`), the hook's own time (`duration_us`), the harness's name on records from
//! harnesses other than Claude Code, and the append, whose failure is either returned to a caller
//! that must report it or recorded in `fail-open.log` (a lost record loses measurement, never the
//! decision).

use crate::failopen::{self, Failure};
use crate::harness::{self, Harness};
use adage_store::{spool, Paths};
use serde_json::{json, Value};
use std::time::Instant;

/// Milliseconds since the Unix epoch: every record's `ts`.
pub(crate) fn now_ms() -> u64 {
    adage_core::time::now_ms()
}

/// The hook's own wall time so far, in microseconds: what every spool record carries as
/// `duration_us` (a lower bound on what the harness waited: the process's spawn is not in it).
pub(crate) fn duration_us(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// Name the harness on a spool record unless it is Claude Code (the records before there were
/// others): the one place that rule lives.
pub(crate) fn name_harness(rec: &mut Value, adapter: &dyn harness::Adapter) {
    if adapter.harness() != Harness::ClaudeCode {
        rec["harness"] = json!(adapter.name());
    }
}

/// Append one record to the session's spool, the failure returned for the caller to report.
pub(crate) fn append(paths: &Paths, session: &str, rec: &Value) -> Result<(), Failure> {
    spool::append(&paths.spool(session), rec).map_err(|e| Failure::io("spool", "appending to the session spool", &e))
}

/// Append one record to the session's spool; a failure is recorded in `fail-open.log` under this
/// harness and event, and the event carries on.
pub(crate) fn append_or_log(paths: &Paths, session: &str, harness: &str, event: &str, rec: &Value) {
    if let Err(f) = append(paths, session, rec) {
        failopen::record(&paths.root, harness, event, &f);
    }
}

#[cfg(test)]
mod tests {
    use crate::testkit::*;
    use crate::{handle, Options, StopGate};
    use adage_core::CommandTable;
    use adage_runtime::rebuild;
    use adage_store::spool;
    use adage_tracks::Index;
    use serde_json::{json, Value};
    use std::time::Instant;

    /// T-A2-01: every spool record a hook writes carries `duration_us`, the hook's own time, as an
    /// integer: a decision, a Stop gate's run, a session start, a session seen in a repository with
    /// no reflexes (the `pretool` records are checked above); a rebuild reads such records as
    /// before.
    #[test]
    fn every_record_carries_the_hooks_own_time() {
        let w = ObligationWorld::new();
        let ext = scripted(vec![StopGate::Notice("adage: ran `make test` at finish: passed (exit 0, 0.1 s).".into())]);
        let start = json!({ "session_id": "s", "cwd": w.root, "source": "startup" });
        let opts = Options { learner: None, extension: Some(ext) };
        handle("claude", "SessionStart", &start.to_string(), opts, &w.paths, Instant::now()).unwrap();
        w.edit(ext);
        w.stop(ext, false);
        let recs = spooled(&w);
        let kinds: Vec<&str> = recs.iter().map(|r| r["kind"].as_str().unwrap()).collect();
        for kind in ["session_start", "batch", "stop", "stop_gate"] {
            assert!(kinds.contains(&kind), "{kinds:?}");
        }
        for r in &recs {
            assert!(r["duration_us"].is_u64(), "{r}");
        }
        let table = CommandTable::load();
        let index = Index::try_load(&w.paths.index()).unwrap();
        let st = rebuild::rebuild(&recs, &index, "acme", table).expect("rebuilt");
        assert_eq!((st.batches, st.obligations.len()), (1, 1));
        // A repository with no reflexes yet: its `seen` record too.
        let plain = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(plain.path().join(".git")).unwrap();
        let p = json!({ "session_id": "u", "cwd": plain.path(), "prompt": "hello" });
        handle("claude", "UserPromptSubmit", &p.to_string(), opts, &w.paths, Instant::now()).unwrap();
        let seen = spool::read_all::<Value>(&w.paths.spool("u")).unwrap();
        assert_eq!((seen.len(), seen[0]["unrouted"].as_bool()), (1, Some(true)));
        assert!(seen[0]["duration_us"].is_u64());
    }
}
