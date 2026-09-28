//! Rebuilding a session's state from its spool (spike S3), for when the two-slot state file can
//! no longer be read.
//!
//! Every hook event that reaches [`crate::decide`] appends one spool record after the state is
//! stored: `{ ts, event, kind, arm, propensity, repo, generation, decision }` (`propensity` since
//! E1; `kind` and `repo` since this
//! module; older records carry only the harness's event name, which [`kind_of`] maps). Replaying
//! the recorded decisions in order restores what the state is for:
//!
//! | Field | From |
//! | --- | --- |
//! | `arm`, `repo`, `propensity` | the first record carrying them (`repo` falls back to the caller's key) |
//! | `prompts`, `batches` | the count of prompt and batch events |
//! | `emitted` | every would-have directive, in order (both arms record them) |
//! | `history`, `first_edit_at_batch` | the observed call keys, in order |
//! | `obligations` | each finish-line route emitted, from the index; met by `obligation_met` |
//! | `stop_requests`, `stop_requested` | each recorded stop request, applied as the runtime does |
//! | `intent` | the family of the last intent route emitted |
//! | `ctx` | the last context state recorded (a decision records it when its event changed it), if it belongs to the index's alphabet for the repository |
//!
//! Every event that changes the context state reaches the runtime and is recorded (the no-match
//! fast path skips only events outside the alphabet), so the last recorded state is the state. A
//! state recorded under another alphabet (the index was relearned since) is dropped: its symbol
//! ids mean other items now.
//!
//! What a record does not show is left out, on the quiet side: an in-flight follow judgement
//! (`pending`, at most three batches long) is dropped, and an obligation re-armed by a later edit
//! after it was met stays met (the spool keeps call keys, not the edited paths), so a rebuilt
//! session may miss a stop request but never makes an extra one.

use crate::{command_key, push_history, Arm, Decision, Judgement, Obligation, SessionState};
use adage_core::CommandTable;
use adage_tracks::{Index, Step, Trigger};
use serde_json::Value;

/// The most spool a rebuild reads (about four thousand events). A longer spool is not replayed,
/// and the session's state starts over, as it did before rebuilds.
pub const MAX_SPOOL_BYTES: u64 = 4 << 20;
/// The most records a rebuild replays.
pub const MAX_RECORDS: usize = 10_000;

/// The runtime event a record is for: its `kind`, or, in records written before `kind`, the
/// harness event name (Claude Code, Codex, OpenCode) mapped to one.
pub fn kind_of(record: &Value) -> &str {
    if let Some(k) = record.get("kind").and_then(Value::as_str) {
        return k;
    }
    match record.get("event").and_then(Value::as_str).unwrap_or("") {
        "UserPromptSubmit" | "chat.message" => "prompt",
        "PostToolBatch" | "PostToolUse" | "tool.execute.after" => "batch",
        "PostToolUseFailure" | "tool.error" => "failure",
        "Stop" | "session.idle" => "stop",
        "SessionStart" | "session.created" => "session_start",
        "SessionEnd" | "session.deleted" => "session_end",
        _ => "",
    }
}

/// The session state the recorded decisions imply, or `None` if there is nothing to replay.
/// `repo` is the key the hook resolved; finish-line routes are looked up in `index` under it.
pub fn rebuild(records: &[Value], index: &Index, repo: &str, table: &CommandTable) -> Option<SessionState> {
    if records.is_empty() {
        return None;
    }
    let mut st = SessionState::default();
    for r in records.iter().take(MAX_RECORDS) {
        if st.arm.is_none() {
            st.arm = r.get("arm").and_then(|a| serde_json::from_value::<Arm>(a.clone()).ok());
        }
        if st.propensity.is_none() {
            st.propensity = r.get("propensity").and_then(Value::as_f64).filter(|p| (0.0..=1.0).contains(p));
        }
        if st.repo.is_none() {
            st.repo = r.get("repo").and_then(Value::as_str).map(str::to_string);
        }
        let kind = kind_of(r);
        match kind {
            "prompt" => st.prompts += 1,
            "batch" => st.batches += 1,
            _ => {}
        }
        let Some(d) = r.get("decision").and_then(|d| serde_json::from_value::<Decision>(d.clone()).ok()) else {
            continue;
        };
        if let Some(c) = d.ctx {
            st.ctx = c;
        }
        for key in d.observed {
            if kind == "batch" && (key.starts_with("edit ") || key.starts_with("write ")) {
                st.first_edit_at_batch.get_or_insert(st.batches);
            }
            push_history(&mut st, key);
        }
        for dir in &d.would_have {
            st.emitted.push(dir.route.clone());
            let Some(ir) = index.for_repo(repo).find(|ir| ir.route.id == dir.route) else { continue };
            match (&ir.route.trigger, &ir.route.step) {
                (Trigger::AfterEdit { paths }, Step::Run { command })
                    if !ir.route.is_instinct() && !st.obligations.iter().any(|o| o.route == dir.route) =>
                {
                    st.obligations.push(Obligation {
                        route: dir.route.clone(),
                        expect_key: command_key(command, table),
                        command: command.clone(),
                        paths: paths.clone(),
                        met: false,
                        stop_requested: false,
                        edited: String::new(),
                        fresh: false,
                    });
                }
                (Trigger::Intent { family, .. }, _) => st.intent = Some(family.clone()),
                _ => {}
            }
        }
        // Judgements after emissions: an edit and the step that meets it can share a batch.
        for j in &d.judgements {
            if let Judgement::ObligationMet { route } = j {
                for o in st.obligations.iter_mut().filter(|o| &o.route == route) {
                    o.met = true;
                }
            }
        }
        if d.would_continue.is_some() {
            // As the runtime chose: the first obligation neither met nor already requested.
            st.stop_requests += 1;
            if let Some(o) = st.obligations.iter_mut().find(|o| !o.met && !o.stop_requested) {
                o.stop_requested = true;
            }
        }
    }
    if st.ctx.alphabet != index.contexts(repo).digest {
        st.ctx = Default::default();
    }
    st.repo.get_or_insert_with(|| repo.to_string());
    Some(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn old_records_are_mapped_by_harness_event_name() {
        for (event, kind) in [
            ("UserPromptSubmit", "prompt"),
            ("chat.message", "prompt"),
            ("PostToolBatch", "batch"),
            ("tool.execute.after", "batch"),
            ("PostToolUseFailure", "failure"),
            ("session.idle", "stop"),
            ("Mystery", ""),
        ] {
            assert_eq!(kind_of(&json!({ "event": event })), kind, "{event}");
        }
        assert_eq!(kind_of(&json!({ "event": "Stop", "kind": "stop" })), "stop");
    }

    #[test]
    fn nothing_to_replay_is_no_state() {
        assert!(rebuild(&[], &Index::default(), "r", &CommandTable::load()).is_none());
    }
}
