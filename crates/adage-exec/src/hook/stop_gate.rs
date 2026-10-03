//! The finish-line gate: at Stop, a learned verification this treated session already ran is run
//! again when the files changed since its last pass ([`stop_gate`]), and its outcome is told to the
//! agent or only to the person ([`gate_answer`]).

use adage_hook::{StopCall, StopGate};
use adage_runtime::render;
use adage_tracks::Trigger;
use std::time::Instant;

use super::grants::verifications;
use super::pytest::failure_record;
use super::{label, run_context};
use crate::approvals::{self, Store};
use crate::request::Level;
use crate::{policy, route, sandbox};
use adage_core::CommandTable;

/// At most this many Stop-gate runs per session; only the first may block.
const MAX_STOP_GATES: u32 = 2;

/// Time a gate run needs beyond its command's own wall limit: the worktree copy, the change set,
/// the input snapshots and the apply (bounded in entries and bytes, not in time).
const GATE_OVERHEAD_MS: u64 = 6_000;

/// The finish-line gate. After a learned context verification ran in this treated session, a
/// Stop whose current inputs have no successful run of it runs it (same grant, policy, sandbox,
/// journal). Passed: the turn ends with a line for the person. Failed: the first time (and not
/// while another stop hook is already continuing), the agent continues with the failing tests;
/// otherwise the turn ends with a line for the person. Never more than [`MAX_STOP_GATES`] runs.
pub(super) fn stop_gate(call: &StopCall<'_>) -> Option<StopGate> {
    if !call.harness.capabilities().actions {
        return None;
    }
    let state = adage_store::state::try_read::<adage_runtime::SessionState>(&call.paths.state(call.session)).ok()??;
    if state.arm != Some(adage_runtime::Arm::Treated) || state.stop_gates >= MAX_STOP_GATES {
        return None;
    }
    let home = &call.paths.root;
    let pol = policy::load(home).ok()?;
    if pol.max_level < Level::E2 {
        return None;
    }
    let store = Store::load(&approvals::path(home)).ok()?;
    let rows = crate::journal::list(home).ok()?;
    if crate::journal::pending(home, call.root).ok()?.is_some() {
        return None; // recovery comes first and is reported by the ordinary pipeline
    }
    let mine: Vec<&crate::journal::Receipt> =
        rows.iter().filter(|r| r.repo == call.repo_key && r.session == call.session).collect();
    // Only a verification this session's learned context already ran: the gate follows prediction.
    let (ir, command, a) =
        verifications(call.repo_key, call.root, call.index, call.paths, &store).into_iter().find(|(ir, _, _)| {
            matches!(ir.route.trigger, Trigger::Context { .. })
                && mine.iter().any(|r| r.route == label(&ir.track, &ir.route.id))
        })?;
    let name = label(&ir.track, &ir.route.id);
    let table = CommandTable::load();
    let mut req =
        route::plan(ir, call.index, call.root, &call.paths.tracks(), Level::E2, a.terms.as_ref(), table).ok()?.request;
    req.inputs = route::bound_from_command(&req.argv, &command)?;
    let action = req.action.clone()?;
    let now =
        crate::stage::input_snapshot_until(&req.repo_root.canonicalize().ok()?, &action, Some(call.deadline)).ok()?;
    let digest = crate::sha256_hex(crate::canonical_json(&now).as_bytes());
    let fresh = mine.iter().any(|r| {
        r.route == name && r.input_digest == digest && matches!(r.outcome, crate::stage::Outcome::Applied { .. })
    });
    if fresh {
        return None;
    }
    // Start only a run that, staged, checked and applied, ends before the hook's deadline: a hook
    // killed between the journal's prepared and completed receipts leaves the worktree in
    // recovery (review F4). Not started, it waits for the next Stop, uncounted.
    let needed = std::time::Duration::from_millis(req.profile.limits.wall_ms.saturating_add(GATE_OVERHEAD_MS));
    if call.deadline.saturating_duration_since(Instant::now()) < needed {
        return None;
    }
    let cap = sandbox::detect();
    let ctx = run_context(&pol, &store, &cap, table, call.repo_key, home);
    let gates = adage_store::state::update::<adage_runtime::SessionState, _>(&call.paths.state(call.session), |st| {
        st.stop_gates += 1;
        st.stop_gates
    })
    .ok()?;
    let may_block = gates == 1 && !call.stop_hook_active;
    let staged = match crate::stage::stage(&req, &ctx) {
        Ok(s) => s,
        Err(r) => {
            return Some(StopGate::Notice(format!(
                "adage: could not run `{}` at finish: {}.",
                render::sanitise(&command),
                crate::reason_text(&r.reason)
            )))
        }
    };
    let completed = crate::journal::finish(
        &staged,
        &crate::journal::Commit {
            home,
            repo: call.repo_key,
            session: call.session,
            occurrence: &format!("stop-gate:{gates}:{digest}"),
            approved_on: &a.approved_on,
            request: &req,
            origin: crate::journal::RunOrigin::StopGate,
        },
    );
    Some(gate_answer(&completed.outcome, &command, &staged, &state.new_tests, may_block, call.stop_hook_active))
}

/// What a gate run's outcome tells the agent (a `Block`) or only the person (a `Notice`).
pub(super) fn gate_answer(
    outcome: &crate::stage::Outcome,
    command: &str,
    staged: &crate::stage::Staged,
    new_tests: &[String],
    may_block: bool,
    stop_hook_active: bool,
) -> StopGate {
    match outcome {
        crate::stage::Outcome::Applied { .. } => {
            StopGate::Notice(render::stop_gate_notice(command, true, staged.exit, staged.duration_ms))
        }
        crate::stage::Outcome::Failed { exit, .. } if may_block => StopGate::Block(render::stop_gate_failed(
            command,
            *exit,
            failure_record(staged, command, new_tests).as_ref(),
        )),
        crate::stage::Outcome::Failed { exit, .. } => {
            StopGate::Notice(render::stop_gate_notice(command, false, *exit, staged.duration_ms))
        }
        // As for an action (`act`): completion unconfirmed, so the agent is told not to retry it
        // and to have a person inspect it (the journal's recovery block pauses later runs).
        crate::stage::Outcome::Unknown { reason } if !stop_hook_active => {
            StopGate::Block(render::stop_gate_unknown(command, &crate::reason_text(reason)))
        }
        crate::stage::Outcome::Refused { reason, .. } | crate::stage::Outcome::Unknown { reason } => {
            StopGate::Notice(format!(
                "adage: `{}` at finish did not complete: {}.",
                render::sanitise(command),
                crate::reason_text(reason)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::super::PREFETCH;
    use super::*;
    use crate::limits;
    use adage_hook::Extension;
    use adage_runtime::{Arm, SessionState};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn stop_gate_needs_a_grant_a_treated_session_and_this_sessions_context_run() {
        let f = Fixture::new();
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "stale");
        assert!(stop(&f, false).is_none(), "no grant: nothing runs");
        f.approve();
        treated(&f, Arm::Control);
        assert!(stop(&f, false).is_none(), "control arm: nothing runs");
        let g = Fixture::new();
        g.approve();
        treated(&g, Arm::Treated);
        assert!(stop(&g, false).is_none(), "the learned context never ran this session");
        record_receipt(&g, "another-session", "stale");
        assert!(stop(&g, false).is_none(), "another session's run does not arm this one");
        let digest = current_digest(&g);
        record_receipt(&g, "fixture-session", &digest);
        assert!(stop(&g, false).is_none(), "a passing run on the current inputs needs no rerun");
        assert_eq!(crate::journal::list(&g.paths.root).unwrap().len(), 2, "nothing ran");
    }

    #[test]
    fn stop_gate_runs_a_stale_check_and_blocks_once_on_failure() {
        if !staging() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        match stop(&f, false) {
            Some(StopGate::Notice(n)) => {
                assert!(n.contains("ran `sh tools/check.sh` at finish: passed (exit 0"), "{n}")
            }
            other => panic!("stale passing check: {other:?}"),
        }
        assert!(stop(&f, false).is_none(), "now current: no second run");
        // A later edit breaks the check: the first gate continues the agent, the next only reports.
        let f = Fixture::new();
        f.approve();
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        std::fs::remove_file(f.root.join("src/feature.rs")).unwrap();
        match stop(&f, false) {
            Some(StopGate::Block(r)) => {
                assert!(r.contains("as you finished: it failed (exit 1)") && r.contains("runs it again"), "{r}")
            }
            other => panic!("failing check: {other:?}"),
        }
        match stop(&f, true) {
            Some(StopGate::Notice(n)) => assert!(n.contains("failed"), "{n}"),
            other => panic!("second gate never blocks: {other:?}"),
        }
        assert!(stop(&f, true).is_none(), "at most two gate runs per session");
        let runs = crate::journal::list(&f.paths.root).unwrap().len();
        assert_eq!(runs, 3, "the context run plus two gate runs, all journaled");
        // A first gate while another stop hook is already continuing reports instead of blocking.
        let g = Fixture::new();
        g.approve();
        treated(&g, Arm::Treated);
        record_receipt(&g, "fixture-session", "an-earlier-input-state");
        std::fs::remove_file(g.root.join("src/feature.rs")).unwrap();
        assert!(matches!(stop(&g, true), Some(StopGate::Notice(_))));
    }

    /// Review F1, through the hook pipeline: the session armed `make gen` and its learned check
    /// ran earlier. At the first Stop the check fails and the obligation is due: both reach the
    /// agent in one continuation, recognisable as Adage's own text. The agent fixes the failure;
    /// at the next Stop the check passes, and the obligation was requested (once).
    #[test]
    fn a_failing_finish_check_keeps_the_learned_obligation_request() {
        if !staging() {
            return;
        }
        let mut f = Fixture::new();
        f.approve();
        with_obligation(&mut f);
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        std::fs::remove_file(f.root.join("src/feature.rs")).unwrap();
        let edit = edit(f.root.join("registry/currencies.csv"));
        let armed = hook(&f, "PostToolBatch", json!({ "tool_calls": [edit] })).expect("the obligation's directive");
        assert!(armed.to_string().contains("make gen"), "{armed}");
        let first = hook(&f, "Stop", json!({ "stop_hook_active": false })).expect("a continuation");
        assert_eq!(first["decision"], "block");
        let reason = first["reason"].as_str().unwrap();
        assert!(
            reason.contains("Adage ran `sh tools/check.sh` (person-approved) as you finished: it failed (exit 1)"),
            "{reason}"
        );
        assert!(reason.contains("requires running `make gen` after such changes"), "the obligation too: {reason}");
        assert!(adage_hook::harness::is_own_text(reason), "{reason}");
        std::fs::write(f.root.join("src/feature.rs"), "fixture\n").unwrap();
        let second = hook(&f, "Stop", json!({ "stop_hook_active": true })).expect("the person's line");
        let line = second["systemMessage"].as_str().unwrap();
        assert!(line.contains("ran `sh tools/check.sh` at finish: passed (exit 0"), "{second}");
        let st: SessionState = adage_store::state::try_read(&f.paths.state("fixture-session")).unwrap().unwrap();
        let o = st.obligations.iter().find(|o| o.route == "regenerate-registry").unwrap();
        assert!(o.stop_requested && st.stop_requests == 1, "{st:?}");
        assert_eq!(st.stop_gates, 2);
        let origins: Vec<_> = crate::journal::list(&f.paths.root).unwrap().iter().map(|r| r.origin).collect();
        // dev-v9: a gate run's pass must never be read as the context run's (it made a failed
        // control look unrealized), so each receipt records which one it was.
        use crate::journal::RunOrigin::{Context, StopGate};
        assert_eq!(origins, [Some(Context), Some(StopGate), Some(StopGate)], "the context run and two gate runs");
    }

    /// Review F4: the gate starts a run only when, staged and applied, it ends before the hook's
    /// deadline (the registered timeout less what the event already spent). Otherwise nothing is
    /// staged, journaled or counted, and it waits for the next Stop.
    #[test]
    fn stop_gate_never_starts_a_run_it_cannot_finish_in_time() {
        let f = Fixture::new();
        f.approve();
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        let soon = Instant::now() + std::time::Duration::from_millis(limits::E2_MAX_WALL_MS + GATE_OVERHEAD_MS - 1_000);
        assert!(stop_by(&f, false, soon).is_none(), "20 s of wall time and the overhead do not fit");
        assert!(stop_by(&f, false, Instant::now()).is_none(), "nothing left at all");
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1, "nothing staged or journaled");
        let st: SessionState = adage_store::state::try_read(&f.paths.state("fixture-session")).unwrap().unwrap();
        assert_eq!(st.stop_gates, 0, "a run that never started is not counted");
        // The whole budget of a hook that has just started: it runs.
        if staging() {
            assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        }
    }

    /// Review F4: a hook killed between the journal's prepared and completed receipts (the only
    /// window in which a kill matters) leaves its worktree in recovery. The next session runs
    /// nothing there, gate included, and tells the agent not to retry; once a person has
    /// inspected and acknowledged the operation, actions run again.
    #[test]
    fn an_interrupted_run_pauses_the_worktree_until_a_person_acknowledges_it() {
        let f = Fixture::new();
        f.approve();
        let id = "0123456789abcdef".repeat(4);
        let prepared = crate::journal::Receipt {
            origin: None,
            id: id.clone(),
            ts: "fixture".into(),
            session: "the-killed-session".into(),
            repo: "fixture".into(),
            worktree: f.root.canonicalize().unwrap().to_string_lossy().into_owned(),
            route: CHECK.into(),
            approved_on: "fixture-only".into(),
            level: "e2".into(),
            phase: crate::journal::Phase::Prepared,
            outcome: crate::stage::Outcome::Unknown { reason: "prepared without a durable completion".into() },
            input_digest: "d".into(),
            outputs: BTreeMap::new(),
        };
        journal(&f, &prepared);
        // The next session: its learned context is selected, and its check ran earlier.
        let (event, dirs) = f.selected("src/feature.rs");
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        let d = PREFETCH.deliver(&f.turn(&event), &dirs);
        let warning = d.recovery.expect("recovery comes first");
        assert!(warning.contains(&id) && warning.contains("do not retry automatically"), "{warning}");
        assert!(stop(&f, false).is_none(), "no gate run while the worktree is in recovery");
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 2, "nothing ran");
        // A person inspects the worktree and acknowledges it (`adage-exec recover --id <id> --acknowledge`).
        crate::journal::acknowledge(&f.paths.root, &id).unwrap();
        let d = PREFETCH.deliver(&f.turn(&event), &dirs);
        assert!(d.recovery.is_none(), "recovered: no warning");
        if staging() {
            assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))), "the finish-line check runs again");
            assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 3);
        }
    }

    /// Review F8: a gate run whose completion cannot be confirmed tells the agent, like an action,
    /// not to retry it (once, never while another Stop hook is continuing).
    #[test]
    fn an_unconfirmed_gate_run_tells_the_agent_not_to_retry() {
        let f = pytest_fixture();
        let staged = failed_run(&f, None, None, false);
        let unknown = crate::stage::Outcome::Unknown { reason: "the completion could not be recorded".into() };
        match gate_answer(&unknown, "make test", &staged, &[], false, false) {
            StopGate::Block(r) => {
                assert!(r.contains("Do not retry it automatically") && r.contains("acknowledge recovery"), "{r}");
                assert!(adage_hook::harness::is_own_text(&r), "{r}");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(gate_answer(&unknown, "make test", &staged, &[], false, true), StopGate::Notice(_)));
    }
}
