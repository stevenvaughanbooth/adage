//! The spool record seam: every record a hook writes to a session's spool is a [`Record`] of one
//! [`RecordKind`], built here. The builder writes what every kind shares once: the wall-clock stamp
//! (`ts`), the native event's name (`event`), the `kind`, the harness's name on records from
//! harnesses other than Claude Code ([`Record::harness`]), the call's harness id
//! ([`Record::tool_use_id`], [`Record::calls`]) and the hook's own time ([`Record::timed`],
//! `duration_us`); each kind's own fields are added with [`Record::with`]. The append's failure is
//! either returned to a caller that must report it (a decision's record) or recorded in
//! `fail-open.log` (a lost record loses measurement, never the decision).
//!
//! A record is a JSON object whose keys serialise sorted (serde_json keeps object keys in a
//! `BTreeMap` in this workspace), so the bytes do not depend on the order the fields are added.
//! `record::snapshot_tests` pins every kind's bytes; `docs/spool.md` lists every kind, its fields
//! and who reads them.

use crate::failopen::{self, Failure};
use crate::harness;
use adage_runtime::Event;
use adage_store::{spool, Paths};
use serde::Serialize;
use serde_json::{Map, Value};
use std::time::Instant;

/// Every kind of record a hook writes to a session's spool. [`RecordKind::as_str`] is the record's
/// `kind`: never renamed, because `adage_runtime::rebuild`, the feed, the status card, adage-app's
/// readers and the mod's feed all match on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordKind {
    /// A session start: decided in a repository with reflexes, or seen (`unrouted`) without.
    SessionStart,
    /// A prompt, decided or seen.
    Prompt,
    /// A tool batch (or one tool call), decided or seen.
    Batch,
    /// A failed tool call, decided or seen.
    Failure,
    /// A `Stop`, decided or seen.
    Stop,
    /// A session end, seen (the fast path never decides one).
    SessionEnd,
    /// A step boundary the mod asked about that predicted or routed a step.
    TurnStep,
    /// adage-exec's finish-line gate ran at a `Stop`.
    StopGate,
    /// A Bash call about to run was decided (`PreToolUse`).
    Pretool,
    /// A skill's text was annotated, or would have been.
    SkillPrompt,
    /// A press token was issued.
    PressIssued,
    /// A press token was spent by a person-only verb.
    PressSpent,
    /// A Bash call named the press.
    Tripwire,
}

impl RecordKind {
    /// The record's `kind`.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            RecordKind::SessionStart => "session_start",
            RecordKind::Prompt => "prompt",
            RecordKind::Batch => "batch",
            RecordKind::Failure => "failure",
            RecordKind::Stop => "stop",
            RecordKind::SessionEnd => "session_end",
            RecordKind::TurnStep => "turn_step",
            RecordKind::StopGate => "stop_gate",
            RecordKind::Pretool => "pretool",
            RecordKind::SkillPrompt => "skill_prompt",
            RecordKind::PressIssued => "press_issued",
            RecordKind::PressSpent => "press_spent",
            RecordKind::Tripwire => "tripwire",
        }
    }

    /// The kind of a decided (or seen) event's record: the runtime event's own name
    /// ([`Event::name`]).
    pub(crate) fn of(ev: &Event) -> RecordKind {
        match ev {
            Event::SessionStart => RecordKind::SessionStart,
            Event::Prompt { .. } => RecordKind::Prompt,
            Event::Batch { .. } => RecordKind::Batch,
            Event::Failure { .. } => RecordKind::Failure,
            Event::Stop { .. } => RecordKind::Stop,
            Event::SessionEnd => RecordKind::SessionEnd,
            Event::TurnStep { .. } => RecordKind::TurnStep,
        }
    }
}

/// Milliseconds since the Unix epoch: every record's `ts`.
pub(crate) fn now_ms() -> u64 {
    adage_core::time::now_ms()
}

/// The hook's own wall time so far, in microseconds: what every spool record carries as
/// `duration_us` (a lower bound on what the harness waited: the process's spawn is not in it).
pub(crate) fn duration_us(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// One spool record, being built.
#[derive(Debug, Clone)]
pub(crate) struct Record(Map<String, Value>);

impl Record {
    /// A record of `kind` for the native `event`, stamped now.
    pub(crate) fn new(kind: RecordKind, event: &str) -> Self {
        Self::at(kind, event, now_ms())
    }

    /// A record of `kind` for the native `event`, stamped `ts` (milliseconds since the epoch): for
    /// a record whose time is the moment its state changed.
    pub(crate) fn at(kind: RecordKind, event: &str, ts: u64) -> Self {
        let mut m = Map::new();
        m.insert("ts".into(), Value::from(ts));
        m.insert("event".into(), Value::from(event));
        m.insert("kind".into(), Value::from(kind.as_str()));
        Record(m)
    }

    /// One of the kind's own fields, serialised as `json!` would (`null` for `None`).
    pub(crate) fn with(mut self, key: &str, value: impl Serialize) -> Self {
        self.set(key, value);
        self
    }

    /// [`Record::with`], in place.
    pub(crate) fn set(&mut self, key: &str, value: impl Serialize) {
        self.0.insert(key.into(), serde_json::to_value(value).unwrap_or(Value::Null));
    }

    /// `harness`: the adapter's name, unless it records none ([`harness::Adapter::recorded_name`]:
    /// Claude Code, as the records before there were others).
    pub(crate) fn harness(mut self, adapter: &dyn harness::Adapter) -> Self {
        if let Some(name) = adapter.recorded_name() {
            self.set("harness", name);
        }
        self
    }

    /// `tool_use_id`: the payload's harness id for the one call this record is about, when it
    /// carries a non-empty one (the feed draws the line under that very call).
    pub(crate) fn tool_use_id(mut self, p: &Value) -> Self {
        if let Some(id) = p["tool_use_id"].as_str().filter(|id| !id.is_empty()) {
            self.set("tool_use_id", id);
        }
        self
    }

    /// `tool_use_ids`: the harness's ids of the calls an event judged, in order, when its payload
    /// carries any (a judgement is then drawn under its call, not merely near it).
    pub(crate) fn calls(mut self, ev: &Event) -> Self {
        let ids: Vec<&str> = match ev {
            Event::Batch { calls } => calls.iter().filter_map(|c| c.tool_use_id.as_deref()).collect(),
            Event::Failure { call } => call.tool_use_id.as_deref().into_iter().collect(),
            _ => Vec::new(),
        };
        if !ids.is_empty() {
            self.set("tool_use_ids", ids);
        }
        self
    }

    /// `duration_us`: the hook's own time since `started` ([`duration_us`]). The last field set
    /// before the append, so it covers the work the record reports.
    pub(crate) fn timed(mut self, started: Instant) -> Self {
        self.set("duration_us", duration_us(started));
        self
    }

    /// Append to the session's spool, the failure returned for the caller to report.
    pub(crate) fn append(self, paths: &Paths, session: &str) -> Result<(), Failure> {
        spool::append(&paths.spool(session), &Value::Object(self.0))
            .map_err(|e| Failure::io("spool", "appending to the session spool", &e))
    }

    /// Append to the session's spool; a failure is recorded in `fail-open.log` under this harness
    /// and event, and the event carries on.
    pub(crate) fn append_or_log(self, paths: &Paths, session: &str, harness: &str, event: &str) {
        if let Err(f) = self.append(paths, session) {
            failopen::record(&paths.root, harness, event, &f);
        }
    }
}

#[cfg(test)]
mod snapshot_tests;

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

    /// A decided event's record is of its runtime event's kind (`Event::name`, which `rebuild`
    /// replays), and no two kinds share a name.
    #[test]
    fn a_decided_records_kind_is_its_events_name() {
        use super::RecordKind;
        use adage_runtime::{By, Call, Event};
        let call = Call {
            tool: "Bash".into(),
            input: Value::Null,
            output: String::new(),
            failed: false,
            by: By::Agent,
            tool_use_id: None,
            agent: None,
        };
        let step = Event::TurnStep { turn_id: "t".into(), index: 0, model: None, agent: None, ending: false };
        for ev in [
            Event::SessionStart,
            Event::Prompt { text: String::new() },
            Event::Batch { calls: vec![] },
            Event::Failure { call },
            Event::Stop { stop_hook_active: false },
            Event::SessionEnd,
            step,
        ] {
            assert_eq!(RecordKind::of(&ev).as_str(), ev.name());
        }
        let all = [
            RecordKind::SessionStart,
            RecordKind::Prompt,
            RecordKind::Batch,
            RecordKind::Failure,
            RecordKind::Stop,
            RecordKind::SessionEnd,
            RecordKind::TurnStep,
            RecordKind::StopGate,
            RecordKind::Pretool,
            RecordKind::SkillPrompt,
            RecordKind::PressIssued,
            RecordKind::PressSpent,
            RecordKind::Tripwire,
        ];
        let mut names: Vec<&str> = all.iter().map(|k| k.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }

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
