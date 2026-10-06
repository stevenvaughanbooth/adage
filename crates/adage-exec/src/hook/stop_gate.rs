//! The finish-line gate: at Stop, a learned verification this treated session already ran is run
//! again when the files changed since its last pass ([`stop_gate`]), and its outcome is told to the
//! agent or only to the person ([`gate_answer`]). Where a person requires completion checks for
//! the repository (`crate::completion`), those come first instead ([`required_completion`]): each
//! missing or stale one is attempted within its retry budget, and the turn ends verified or says
//! it is unverified.

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
    let state = match adage_store::state::try_read::<adage_runtime::SessionState>(&call.paths.state(call.session)) {
        Ok(Some(state)) => state,
        _ => return match crate::completion::Requirements::load(&call.paths.root) {
            Ok(requirements) if requirements.routes(call.repo_key).is_empty() => None,
            _ => Some(StopGate::Notice("adage: completion is unverified because session state is unavailable; no required check was automatically run.".into())),
        },
    };
    if state.arm != Some(adage_runtime::Arm::Treated) {
        return None;
    }
    match crate::completion::Requirements::load(&call.paths.root) {
        Ok(requirements) if !requirements.routes(call.repo_key).is_empty() => {
            return Some(required_completion(call).unwrap_or_else(|e| StopGate::Notice(format!(
                "adage: completion is unverified: {}. Inspect `adage-exec readiness --repo <dir>` before treating this work as checked.",
                crate::reason_text(&format!("{e:#}"))
            ))));
        }
        Err(e) => {
            return Some(StopGate::Notice(format!(
                "adage: completion requirements are unreadable; completion is unverified: {}",
                crate::reason_text(&e.to_string())
            )))
        }
        _ => {}
    }
    if state.stop_gates >= MAX_STOP_GATES {
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

/// Explicit completion requirements also cover a check the model never started. This remains a
/// bounded hook continuation, not a universal publication or harness termination interlock.
fn required_completion(call: &StopCall<'_>) -> anyhow::Result<StopGate> {
    use crate::completion::{self, AttemptGuard};
    use crate::verification::Status;
    let _planning_budget = crate::project_python::budget(
        call.deadline.saturating_duration_since(Instant::now()).as_millis().min(u128::from(u64::MAX)) as u64,
    );
    let home = &call.paths.root;
    let mut guard = AttemptGuard::open(home, call.root)?;
    let before =
        completion::inspect(call.repo_key, call.root, call.index, call.paths, Some(call.session), Some(call.deadline))?;
    let pol = policy::load(home)?;
    let store = Store::load(&approvals::path(home))?;
    let cap = sandbox::detect();
    let table = CommandTable::load();
    let ctx = run_context(&pol, &store, &cap, table, call.repo_key, home);
    if !guard.attempts.pending() && crate::journal::pending(home, call.root)?.is_none() {
        for check in &before.checks {
            if check.readiness.status == Status::Passed
                || guard.attempts.count(call.session, &check.route) >= completion::MAX_ATTEMPTS
            {
                continue;
            }
            let Ok(prepared) =
                completion::prepare(&check.route, call.repo_key, call.root, call.index, call.paths, &pol, &store)
            else {
                continue;
            };
            let req = &prepared.request;
            let needed = std::time::Duration::from_millis(req.profile.limits.wall_ms.saturating_add(GATE_OVERHEAD_MS));
            if call.deadline.saturating_duration_since(Instant::now()) < needed {
                break;
            }
            let inputs = crate::stage::input_snapshot_until(
                &call.root.canonicalize()?,
                req.action.as_ref().unwrap(),
                Some(call.deadline),
            )
            .map_err(|e| anyhow::anyhow!(e.reason))?;
            let digest = crate::sha256_hex(crate::canonical_json(&inputs).as_bytes());
            // Bind invalidation to the latest receipt across sessions, even though this Stop
            // requires a result from its own session. An acknowledgement cannot revive it.
            let rows = crate::journal::list(home)?;
            let worktree = call.root.canonicalize()?.to_string_lossy().into_owned();
            let prior =
                rows.iter().rev().find(|r| r.repo == call.repo_key && r.worktree == worktree && r.route == check.route);
            let id = guard.begin(call.session, &check.route, prior.map(|r| r.id.as_str()))?;
            let occurrence = format!("completion:{id}:{digest}");
            let commit = crate::journal::Commit {
                home,
                repo: call.repo_key,
                session: call.session,
                occurrence: &occurrence,
                approved_on: &prepared.approved_on,
                request: req,
                origin: crate::journal::RunOrigin::StopGate,
            };
            match crate::stage::stage(req, &ctx) {
                Err(refusal) => {
                    let receipt = crate::journal::withheld_verification(&commit, &digest, &refusal)?;
                    guard.finish(id, &receipt.id)?;
                }
                Ok(staged) => {
                    let finished = crate::journal::finish(&staged, &commit);
                    // A completion whose persistence failed is still an unfinished attempt.
                    // Do not reset this reservation just because a process returned.
                    let durable = crate::journal::list(home)?.into_iter().find(|r| {
                        finished.id.as_deref() == Some(r.id.as_str()) && r.phase == crate::journal::Phase::Completed
                    });
                    let Some(receipt) = durable else {
                        anyhow::bail!("completion result was not durably recorded; inspect the worktree before acknowledging recovery")
                    };
                    guard.finish(id, &receipt.id)?;
                }
            }
            if crate::journal::pending(home, call.root)?.is_some() {
                break;
            }
        }
    }
    let after =
        completion::inspect(call.repo_key, call.root, call.index, call.paths, Some(call.session), Some(call.deadline))?;
    if after.ready {
        return Ok(StopGate::Notice(format!("adage: all {} required verification command(s) passed on current inputs. This establishes command completion; test coverage and external state require separate validation.", after.checks.len())));
    }
    let unmet: Vec<_> = after.checks.iter().filter(|c| c.readiness.status != Status::Passed).collect();
    let details = unmet
        .iter()
        .take(2)
        .map(|c| format!("{}: {}", render::sanitise(&c.route), crate::reason_text(&c.readiness.reason)))
        .collect::<Vec<_>>()
        .join("; ");
    let recovery = guard.attempts.pending() || crate::journal::pending(home, call.root)?.is_some();
    let body = if recovery {
        format!("Completion is unverified ({details}). Do not retry automatically. Ask the person to inspect the worktree and acknowledge interrupted execution.")
    } else {
        format!("Completion is unverified for {} required check(s): {details}. Fix any failing checks before finishing; do not describe these checks as passed without current evidence. Automatic checks have a bounded retry budget.", unmet.len())
    };
    if !call.stop_hook_active && !guard.attempts.continued(call.session) {
        guard.continue_once(call.session)?;
        Ok(StopGate::Block(render::finish_line(&body)))
    } else {
        Ok(StopGate::Notice(format!("adage: {body}")))
    }
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

    const REQUIRED_CHECK: &str = CHECK;

    fn require_completion(f: &Fixture) {
        let mut requirements = crate::completion::Requirements::default();
        requirements.repositories.insert("fixture".into(), vec![REQUIRED_CHECK.into()]);
        requirements.save(&f.paths.root).unwrap();
        f.index.write_atomic(&f.paths.index()).unwrap();
        std::fs::write(f.paths.root.join("config.toml"), "[learn]\nauto = false\n").unwrap();
        treated(f, Arm::Treated);
    }

    fn completion_status(f: &Fixture) -> crate::completion::Report {
        crate::completion::inspect("fixture", &f.root, &f.index, &f.paths, Some("fixture-session"), None).unwrap()
    }

    fn completion_staging() -> bool {
        let available = staging();
        assert!(
            available || std::env::var_os("ADAGE_REQUIRE_COMPLETION_STAGING").is_none(),
            "completion integration proof requires a working OS staging sandbox"
        );
        if !available {
            eprintln!("completion execution not exercised: no staging sandbox; ADAGE_REQUIRE_COMPLETION_STAGING=1 makes this a failure");
        }
        available
    }

    #[cfg(unix)]
    #[test]
    fn required_completion_runs_a_never_started_check_and_records_current_evidence() {
        if !completion_staging() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        require_completion(&f);
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty());
        let before = completion_status(&f);
        assert!(!before.ready);
        assert_eq!(before.checks[0].readiness.status, crate::verification::Status::Missing);
        match stop(&f, false) {
            Some(StopGate::Notice(n)) => {
                assert!(n.contains("required verification command(s) passed on current inputs"), "{n}")
            }
            other => panic!("never-started check must really run: {other:?}"),
        }
        let rows = crate::journal::list(&f.paths.root).unwrap();
        assert_eq!(rows.len(), 1);
        let receipt = &rows[0];
        assert_eq!(receipt.phase, crate::journal::Phase::Completed);
        assert_eq!(receipt.origin, Some(crate::journal::RunOrigin::StopGate));
        assert!(matches!(receipt.outcome, crate::stage::Outcome::Applied { .. }));
        assert_eq!(receipt.verification.as_ref().unwrap().exit, Some(0));
        let after = completion_status(&f);
        assert!(after.ready);
        assert_eq!(after.checks[0].readiness.receipt.as_deref(), Some(receipt.id.as_str()));
        assert!(after.evidence_scope.contains("test coverage") && after.evidence_scope.contains("not established"));
        assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1, "fresh evidence avoids a duplicate run");
        std::fs::write(f.root.join("src/feature.rs"), "an uncommitted later edit\n").unwrap();
        assert_eq!(completion_status(&f).checks[0].readiness.status, crate::verification::Status::Stale);
        assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        assert!(completion_status(&f).ready, "the changed inputs were actually checked again");
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn required_completion_does_not_resurrect_a_pass_after_a_newer_failure() {
        if !completion_staging() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        require_completion(&f);
        assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        let passing = crate::journal::list(&f.paths.root).unwrap()[0].id.clone();
        std::fs::remove_file(f.root.join("src/feature.rs")).unwrap();
        assert!(matches!(stop(&f, false), Some(StopGate::Block(_))));
        let failed = completion_status(&f);
        assert!(!failed.ready);
        assert_eq!(failed.checks[0].readiness.status, crate::verification::Status::Failed);
        let latest = failed.checks[0].readiness.receipt.clone();
        assert_ne!(latest.as_deref(), Some(passing.as_str()));
        // Restoring the old passing input bytes must not select the older passing operation.
        std::fs::write(f.root.join("src/feature.rs"), "fixture\n").unwrap();
        let restored = completion_status(&f);
        assert!(!restored.ready);
        assert_eq!(restored.checks[0].readiness.status, crate::verification::Status::Stale);
        assert_eq!(restored.checks[0].readiness.receipt, latest);
        match stop(&f, false) {
            Some(StopGate::Notice(n)) => assert!(n.contains("unverified"), "{n}"),
            other => panic!("exhausted budget must not report a pass or repeat its continuation: {other:?}"),
        }
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn required_completion_retry_budget_and_continuation_survive_corrupt_session_state() {
        if !completion_staging() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        require_completion(&f);
        // A real pipeline event records the treated assignment for session-state reconstruction.
        hook(
            &f,
            "PostToolUse",
            json!({"tool_name":"Bash","tool_input":{"command":"git status --short"},"tool_response":""}),
        );
        std::fs::remove_file(f.root.join("src/feature.rs")).unwrap();
        let first = hook(&f, "Stop", json!({"stop_hook_active":false})).expect("first failed required check");
        assert_eq!(first["decision"], "block");
        assert!(first["reason"].as_str().unwrap().contains("unverified"));
        assert!(adage_hook::harness::is_own_text(first["reason"].as_str().unwrap()));
        for attempt in 0..3 {
            std::fs::write(f.paths.state("fixture-session"), b"both state slots are corrupt").unwrap();
            let next = hook(&f, "Stop", json!({"stop_hook_active":false})).expect("unverified status remains visible");
            assert!(next.get("decision").is_none(), "a lost state file must not reset the continuation: {next}");
            assert!(next["systemMessage"].as_str().unwrap().contains("unverified"), "{next}");
            assert_eq!(
                crate::journal::list(&f.paths.root).unwrap().len(),
                2,
                "iteration {attempt}: bounded actual execution"
            );
        }
        let guard = crate::completion::AttemptGuard::open(&f.paths.root, &f.root).unwrap();
        assert_eq!(guard.attempts.count("fixture-session", REQUIRED_CHECK), 2);
        assert!(guard.attempts.continued("fixture-session"));
        assert!(!completion_status(&f).ready);
    }

    #[cfg(unix)]
    #[test]
    fn required_completion_needs_current_approval_and_never_changes_default_or_control_behavior() {
        let default = Fixture::new();
        default.approve();
        treated(&default, Arm::Treated);
        assert!(stop(&default, false).is_none(), "an approved prediction alone does not make a check mandatory");
        assert!(crate::journal::list(&default.paths.root).unwrap().is_empty());
        let missing = Fixture::new();
        require_completion(&missing);
        assert!(matches!(stop(&missing, false), Some(StopGate::Block(_))));
        assert!(!completion_status(&missing).ready);
        assert!(
            crate::journal::list(&missing.paths.root).unwrap().is_empty(),
            "requirements never authorize execution"
        );
        let lapsed = Fixture::new();
        lapsed.approve();
        require_completion(&lapsed);
        std::fs::write(lapsed.root.join("tools/check.sh"), "#!/bin/sh\necho changed approval\n").unwrap();
        assert!(matches!(stop(&lapsed, false), Some(StopGate::Block(_))));
        assert!(!completion_status(&lapsed).ready);
        assert!(crate::journal::list(&lapsed.paths.root).unwrap().is_empty(), "changed script must be approved again");
        let control = Fixture::new();
        control.approve();
        require_completion(&control);
        treated(&control, Arm::Control);
        assert!(stop(&control, false).is_none());
        assert!(crate::journal::list(&control.paths.root).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn required_completion_interruption_acknowledgement_is_not_a_pass() {
        if !completion_staging() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        require_completion(&f);
        assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        let prior = crate::journal::list(&f.paths.root).unwrap()[0].id.clone();
        // Simulate a reserved second attempt whose hook died before a durable action receipt.
        let mut guard = crate::completion::AttemptGuard::open(&f.paths.root, &f.root).unwrap();
        guard.begin("fixture-session", REQUIRED_CHECK, Some(&prior)).unwrap();
        drop(guard);
        assert_eq!(completion_status(&f).checks[0].readiness.status, crate::verification::Status::Unknown);
        match stop(&f, false) {
            Some(StopGate::Block(reason)) => assert!(reason.contains("Do not retry automatically"), "{reason}"),
            other => panic!("interruption must remain unverified: {other:?}"),
        }
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1, "no interrupted work is blindly retried");
        let mut guard = crate::completion::AttemptGuard::open(&f.paths.root, &f.root).unwrap();
        assert_eq!(guard.acknowledge().unwrap(), 1);
        assert_eq!(guard.attempts.count("fixture-session", REQUIRED_CHECK), 2, "acknowledgement never resets budget");
        drop(guard);
        let acknowledged = completion_status(&f);
        assert!(!acknowledged.ready);
        assert_eq!(acknowledged.checks[0].readiness.status, crate::verification::Status::Unknown);
        assert!(acknowledged.checks[0].readiness.reason.contains("acknowledgement is not a pass"));
        assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        assert!(!completion_status(&f).ready);
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn required_completion_acknowledged_prepared_receipt_is_not_completed_verification() {
        if !completion_staging() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        require_completion(&f);
        assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        let mut interrupted = crate::journal::list(&f.paths.root).unwrap()[0].clone();
        let mut guard = crate::completion::AttemptGuard::open(&f.paths.root, &f.root).unwrap();
        guard.begin("fixture-session", REQUIRED_CHECK, Some(&interrupted.id)).unwrap();
        drop(guard);
        interrupted.id = "abcdef0123456789".repeat(4);
        interrupted.phase = crate::journal::Phase::Prepared;
        interrupted.outcome = crate::stage::Outcome::Unknown { reason: "hook killed before durable completion".into() };
        // Even a process result captured before apply is not a durably completed verification.
        assert_eq!(interrupted.verification.as_ref().unwrap().exit, Some(0));
        adage_store::spool::append_durable(&crate::stage::spool_path(&f.paths.root), &interrupted).unwrap();
        assert_eq!(completion_status(&f).checks[0].readiness.status, crate::verification::Status::Unknown);
        assert!(matches!(stop(&f, false), Some(StopGate::Block(_))));
        crate::journal::acknowledge(&f.paths.root, &interrupted.id).unwrap();
        assert!(!completion_status(&f).ready, "journal recovery alone does not clear an unfinished reservation");
        let mut guard = crate::completion::AttemptGuard::open(&f.paths.root, &f.root).unwrap();
        assert_eq!(guard.acknowledge().unwrap(), 1);
        drop(guard);
        let report = completion_status(&f);
        assert!(!report.ready);
        assert_eq!(report.checks[0].readiness.status, crate::verification::Status::Unknown);
        assert_eq!(report.checks[0].readiness.receipt.as_deref(), Some(interrupted.id.as_str()));
        assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        assert_eq!(
            crate::journal::list(&f.paths.root).unwrap().len(),
            2,
            "acknowledgement neither retries nor creates a pass"
        );
    }

    #[test]
    fn required_completion_deadline_exhaustion_does_not_spend_an_attempt() {
        let f = Fixture::new();
        f.approve();
        require_completion(&f);
        let short = Instant::now() + std::time::Duration::from_secs(1);
        assert!(matches!(stop_by(&f, false, short), Some(StopGate::Block(_))));
        assert!(!completion_status(&f).ready);
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty());
        let guard = crate::completion::AttemptGuard::open(&f.paths.root, &f.root).unwrap();
        assert_eq!(guard.attempts.count("fixture-session", REQUIRED_CHECK), 0);
    }

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
            verification: None,
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
