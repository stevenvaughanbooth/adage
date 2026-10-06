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
//! | `obligations` | exact `obligation_state` snapshots; older records use emitted routes and `obligation_met` |
//! | `stop_requests`, `stop_requested` | each recorded stop request, applied as the runtime does |
//! | `stop_gates` | each `stop_gate` record (`adage-exec`'s finish-line gate ran), so the cap survives |
//! | `intent` | the family of the last intent route emitted |
//! | `predictions`, `last_step_predicted`, `predicted_routes` | each `turn_step` decision whose prediction counted (`predicted_at`) |
//! | `ctx` | the last context state recorded (a decision records it when its event changed it), if it belongs to the index's alphabet for the repository |
//! | `cand_ctx`, `noticed` | the same for the candidates' own alphabet (`Index::candidate_contexts`); every candidate any decision noticed, in order |
//!
//! Every event that changes the context state reaches the runtime and is recorded (the no-match
//! fast path skips only events outside the alphabet), so the last recorded state is the state. A
//! state recorded under another alphabet (the index was relearned since) is dropped: its symbol
//! ids mean other items now.
//!
//! An in-flight follow judgement (`pending`, at most three batches long) and a bound read batch
//! (`reads`) are not reconstructed (so nothing is predicted from them). Current records preserve
//! every change to advisory obligations, including a later edit that re-arms one after its
//! command was observed. Older records lack exact edited paths: a recorded edit conservatively
//! invalidates earlier met obligations rather than inventing freshness. Advisory `met` means the
//! requested command was observed, never that a check passed. Stop requests remain bounded by the
//! recorded session count and ordinary runtime policy.

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
/// harness event name (Claude Code, Codex, OpenCode) mapped to one by the event table
/// ([`adage_trace::Harness::events`]; a name means the same in every harness that sends it). An
/// event the runtime does not decide (`PreToolUse`, the mod's asks) or an unknown name is `""`.
pub fn kind_of(record: &Value) -> &str {
    if let Some(k) = record.get("kind").and_then(Value::as_str) {
        return k;
    }
    let event = record.get("event").and_then(Value::as_str).unwrap_or("");
    adage_trace::Harness::ALL
        .into_iter()
        .find_map(|h| h.event_kind(event))
        .and_then(adage_trace::EventKind::decided)
        .unwrap_or("")
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
            // The finish-line gate's run (adage-exec): its per-session cap must survive a rebuild.
            "stop_gate" => st.stop_gates = st.stop_gates.saturating_add(1),
            _ => {}
        }
        let Some(mut raw) = r.get("decision").cloned() else { continue };
        // An incomplete new snapshot must not hide an otherwise readable legacy edit record.
        // Decode it independently and use the conservative legacy path when it is malformed.
        let snapshot = raw.as_object_mut().and_then(|d| d.remove("obligation_state"));
        let Ok(mut d) = serde_json::from_value::<Decision>(raw) else { continue };
        d.obligation_state = snapshot.and_then(|s| serde_json::from_value(s).ok());
        if let Some(c) = d.ctx {
            st.ctx = c;
        }
        if let Some(c) = d.cand_ctx {
            st.cand_ctx = c;
        }
        for n in &d.noticed {
            if st.noticed.len() < crate::MAX_NOTICED && !st.noticed.contains(&n.candidate) {
                st.noticed.push(n.candidate.clone());
            }
        }
        // A step whose prediction counted against the bounds (both arms record it).
        if let Some(at) = d.predicted_at {
            st.predictions = st.predictions.saturating_add(1);
            st.last_step_predicted = Some(at);
            if let Some(p) = &d.would_predict {
                if !st.predicted_routes.contains(&p.route) {
                    st.predicted_routes.push(p.route.clone());
                }
            }
        }
        let legacy_edit = d.obligation_state.is_none()
            && kind == "batch"
            && d.observed.iter().any(|key| key.starts_with("edit ") || key.starts_with("write "));
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
                        via: String::new(),
                    });
                }
                // A sequence after an edit carries the finish lines it subsumes (`decide`).
                (Trigger::AfterEdit { .. }, Step::Sequence { commands }) => {
                    let Some(first) = commands.first() else { continue };
                    for o in index.for_repo(repo) {
                        let (Trigger::AfterEdit { paths }, Step::Run { command }) = (&o.route.trigger, &o.route.step)
                        else {
                            continue;
                        };
                        let carried = !o.route.is_instinct()
                            && o.route.slots.is_empty()
                            && adage_tracks::interlock::same_point(&ir.route.trigger, &o.route.trigger)
                            && adage_core::pattern::same_command(first, command);
                        if carried && !st.obligations.iter().any(|x| x.route == o.route.id) {
                            st.obligations.push(Obligation {
                                route: o.route.id.clone(),
                                expect_key: command_key(command, table),
                                command: command.clone(),
                                paths: paths.clone(),
                                met: false,
                                stop_requested: false,
                                edited: String::new(),
                                fresh: false,
                                via: dir.route.clone(),
                            });
                        }
                    }
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
        if let Some(snapshot) = d.obligation_state {
            // A snapshot contains only existing finish-line routes, at most one entry per route;
            // rebuilding cannot introduce an instruction absent from the current index. Its
            // bound command/path are the ones the live runtime already recorded for the agent.
            let mut seen = std::collections::BTreeSet::new();
            st.obligations = snapshot
                .obligations
                .into_iter()
                .filter(|o| {
                    seen.insert(o.route.clone())
                        && index.for_repo(repo).any(|ir| {
                            ir.route.id == o.route
                                && !ir.route.is_instinct()
                                && matches!(
                                    (&ir.route.trigger, &ir.route.step),
                                    (Trigger::AfterEdit { .. }, Step::Run { .. })
                                )
                        })
                })
                .collect();
            st.stop_requests = snapshot.stop_requests;
        } else if legacy_edit {
            // Legacy judgement lists do not preserve where an edit fell relative to a call in
            // the same batch. Requiring another advisory check is safer than treating an earlier
            // attempt as current. No synthetic success or outcome is inferred from the edit key.
            for o in &mut st.obligations {
                if o.met {
                    o.met = false;
                    o.stop_requested = false;
                }
                o.edited.clear();
                o.fresh = false;
            }
        }
    }
    if st.ctx.alphabet != index.contexts(repo).digest {
        st.ctx = Default::default();
    }
    if st.cand_ctx.alphabet != index.candidate_contexts(repo).digest {
        st.cand_ctx = Default::default();
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
            ("TurnStep", "turn_step"),
            ("PreToolUse", ""),
            ("Standing", ""),
            ("Mystery", ""),
        ] {
            assert_eq!(kind_of(&json!({ "event": event })), kind, "{event}");
        }
        assert_eq!(kind_of(&json!({ "event": "Stop", "kind": "stop" })), "stop");
    }

    #[test]
    fn nothing_to_replay_is_no_state() {
        assert!(rebuild(&[], &Index::default(), "r", CommandTable::load()).is_none());
    }

    /// F8: the finish-line gate's per-session cap is restored from its spool records.
    #[test]
    fn stop_gate_runs_are_restored() {
        let records = [
            json!({ "event": "SessionStart", "kind": "session_start", "arm": "treated", "repo": "r" }),
            json!({ "event": "Stop", "kind": "stop_gate", "blocked": true }),
            json!({ "event": "Stop", "kind": "stop", "arm": "treated", "repo": "r" }),
            json!({ "event": "Stop", "kind": "stop_gate", "blocked": false }),
        ];
        let st = rebuild(&records, &Index::default(), "r", CommandTable::load()).unwrap();
        assert_eq!(st.stop_gates, 2);
    }
}
