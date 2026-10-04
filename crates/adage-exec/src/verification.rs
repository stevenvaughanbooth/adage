//! Verification facts from approved, staged execution, and pure readiness evaluation.
//!
//! A passing result establishes that the exact approved verification command exited zero for
//! the captured repository inputs. It does not establish that tests were discovered, that the
//! check covers the task, or that the user-visible behavior is correct. Those are separate
//! policy and grading decisions. Ambient toolchains are not fingerprinted: only the sandbox
//! profile and an explicitly approved project-Python descriptor have recorded identities.
//! These local records inherit the approval store's trust boundary: a process able to rewrite
//! Adage's home can fabricate both. Durable receipts are not independent evaluation evidence
//! when the evaluated agent can write that home.

use crate::approvals::ApprovalKey;
use crate::journal::{Phase, Receipt};
use crate::request::{ActionKind, ExecRequest, Level};
use crate::stage::{Outcome, Staged};
use adage_core::Effect;
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

/// Immutable facts captured after staging and kept unchanged through journal transitions.
/// Their presence in a prepared receipt never proves durable completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub schema: u32,
    pub approval_digest: String,
    /// The exact bound slot values; a narrower test selection cannot satisfy a wider one.
    pub binding_digest: String,
    pub input_digest: String,
    pub action_kind: ActionKind,
    pub profile_digest: String,
    /// Identity of the approved descriptor, not an inference about an ambient interpreter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_python_digest: Option<String>,
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    pub duration_ms: u64,
}

fn digest(value: &impl Serialize) -> String {
    crate::sha256_hex(crate::canonical_json(value).as_bytes())
}

impl Evidence {
    pub(crate) fn from_staged(staged: &Staged, request: &ExecRequest, repo: &str) -> Option<Self> {
        if request.level != Level::E2
            || request.effect != Effect::Verify
            || staged.action.kind != ActionKind::Verify
            || request.action.as_ref() != Some(&staged.action)
        {
            return None;
        }
        Some(Self {
            schema: SCHEMA,
            approval_digest: digest(&ApprovalKey::for_request(request, repo)),
            binding_digest: digest(&request.inputs),
            input_digest: digest(&staged.inputs),
            action_kind: ActionKind::Verify,
            profile_digest: request.profile.id(),
            project_python_digest: request.project_python.as_ref().map(digest),
            exit: staged.exit,
            signal: staged.signal,
            duration_ms: staged.duration_ms,
        })
    }
}

/// Current independently observed requirements. The caller must resolve the canonical worktree,
/// recapture inputs, and verify that the exact current grant and policy still authorize this
/// request. This pure evaluator does not load a grant or infer one from a historical receipt.
pub struct Expectation<'a> {
    pub repo: &'a str,
    pub worktree: &'a str,
    pub route: &'a str,
    /// `None` permits reuse across sessions; `Some` requires the latest operation to belong to
    /// that session. It never selects an older pass behind a newer operation in another session.
    pub session: Option<&'a str>,
    pub approved_on: &'a str,
    pub request: &'a ExecRequest,
    pub input_digest: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Passed,
    Missing,
    Stale,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Readiness {
    pub status: Status,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

fn answer(status: Status, reason: &str, receipt: Option<&Receipt>) -> Readiness {
    Readiness { status, reason: reason.into(), receipt: receipt.map(|r| r.id.clone()) }
}

/// Evaluate canonical journal rows in their durable operation order, as returned by
/// [`crate::journal::list`]. Select the latest matching operation *before* checking freshness
/// or success: a newer failure, unknown result, legacy receipt, or changed approval never
/// resurrects an older pass. Any unresolved operation in the worktree takes precedence.
pub fn evaluate(rows: &[Receipt], expected: &Expectation<'_>) -> Readiness {
    if let Some(row) = rows.iter().rev().find(|r| r.worktree == expected.worktree && r.needs_recovery()) {
        return answer(Status::Unknown, "worktree has an unresolved execution; inspect recovery first", Some(row));
    }
    let request = expected.request;
    if request.level != Level::E2
        || request.effect != Effect::Verify
        || !request.action.as_ref().is_some_and(|a| a.kind == ActionKind::Verify)
    {
        return answer(Status::Unknown, "requirement is not an approved E2 verification action", None);
    }
    let Some(row) = rows
        .iter()
        .rev()
        .find(|r| r.repo == expected.repo && r.worktree == expected.worktree && r.route == expected.route)
    else {
        return answer(Status::Missing, "no verification receipt for this requirement", None);
    };
    if expected.session.is_some_and(|session| row.session != session) {
        return answer(Status::Missing, "latest verification receipt belongs to another session", Some(row));
    }
    if row.id.is_empty() || row.phase != Phase::Completed || row.level != "e2" {
        return answer(Status::Unknown, "receipt does not establish completed E2 verification", Some(row));
    }
    // A staging refusal has no process result to put in Evidence. It still supersedes a pass.
    if matches!(row.outcome, Outcome::Refused { .. }) {
        return answer(Status::Failed, "latest verification could not be accepted", Some(row));
    }
    let Some(evidence) = &row.verification else {
        return answer(Status::Unknown, "legacy or non-verification receipt has no verification evidence", Some(row));
    };
    if evidence.schema != SCHEMA || evidence.action_kind != ActionKind::Verify {
        return answer(Status::Unknown, "unsupported verification evidence", Some(row));
    }
    if evidence.input_digest != row.input_digest {
        return answer(Status::Unknown, "verification input identity disagrees with its receipt", Some(row));
    }
    if row.approved_on != expected.approved_on
        || evidence.approval_digest != digest(&ApprovalKey::for_request(request, expected.repo))
        || evidence.profile_digest != request.profile.id()
        || evidence.project_python_digest != request.project_python.as_ref().map(digest)
    {
        return answer(Status::Stale, "verification approval, command, profile or pinned runtime changed", Some(row));
    }
    if evidence.binding_digest != digest(&request.inputs) {
        return answer(Status::Stale, "verification command bindings changed", Some(row));
    }
    if expected.input_digest.is_empty() || row.input_digest != expected.input_digest {
        return answer(Status::Stale, "repository inputs changed or their current identity is unavailable", Some(row));
    }
    match &row.outcome {
        Outcome::Applied { .. } if evidence.exit == Some(0) && evidence.signal.is_none() => answer(
            Status::Passed,
            "approved verification command exited zero for current inputs; test coverage is not established",
            Some(row),
        ),
        Outcome::Applied { .. } => {
            answer(Status::Unknown, "applied receipt does not establish a zero exit without a signal", Some(row))
        }
        Outcome::Failed { .. } => answer(Status::Failed, "latest verification command failed", Some(row)),
        Outcome::Refused { .. } => answer(Status::Failed, "latest verification could not be accepted", Some(row)),
        Outcome::Unknown { .. } => answer(Status::Unknown, "latest verification result is unknown", Some(row)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{ActionSpec, Labels, Origin, OutputSpec};
    use crate::sandbox::Profile;
    use std::collections::BTreeMap;

    fn request() -> ExecRequest {
        ExecRequest {
            repo_root: "/project".into(),
            argv: vec!["make".into(), "check".into()],
            inputs: BTreeMap::new(),
            contracts: BTreeMap::new(),
            effect: Effect::Verify,
            level: Level::E2,
            output: OutputSpec::default(),
            scripts: vec![],
            project_python: None,
            profile: Profile::for_level(Level::E2),
            labels: Labels::default(),
            origin: Origin::Local,
            route_exec_digest: None,
            action: Some(ActionSpec { kind: ActionKind::Verify, outputs: vec![], cache: vec![] }),
        }
    }

    fn expected(request: &ExecRequest) -> Expectation<'_> {
        Expectation {
            repo: "repo",
            worktree: "/project",
            route: "checks/verify",
            session: None,
            approved_on: "approval-1",
            request,
            input_digest: "inputs-1",
        }
    }

    fn passed(request: &ExecRequest) -> Receipt {
        Receipt {
            id: "operation-1".into(),
            ts: "1".into(),
            session: "session-1".into(),
            repo: "repo".into(),
            worktree: "/project".into(),
            route: "checks/verify".into(),
            approved_on: "approval-1".into(),
            level: "e2".into(),
            phase: Phase::Completed,
            outcome: Outcome::Applied { changed: vec![], discarded: 0 },
            input_digest: "inputs-1".into(),
            outputs: BTreeMap::new(),
            origin: None,
            verification: Some(Evidence {
                schema: SCHEMA,
                approval_digest: digest(&ApprovalKey::for_request(request, "repo")),
                binding_digest: digest(&request.inputs),
                input_digest: "inputs-1".into(),
                action_kind: ActionKind::Verify,
                profile_digest: request.profile.id(),
                project_python_digest: None,
                exit: Some(0),
                signal: None,
                duration_ms: 7,
            }),
        }
    }

    #[test]
    fn passing_evidence_is_scoped_and_does_not_claim_test_discovery() {
        let request = request();
        let rows = [passed(&request)];
        let current = expected(&request);
        let result = evaluate(&rows, &current);
        assert_eq!(result.status, Status::Passed);
        assert!(result.reason.contains("test coverage is not established"));
        let mut scoped = current;
        scoped.session = Some("session-2");
        assert_eq!(evaluate(&rows, &scoped).status, Status::Missing);
        scoped.session = Some("session-1");
        assert_eq!(evaluate(&rows, &scoped).status, Status::Passed);
        scoped.worktree = "/another-checkout";
        assert_eq!(evaluate(&rows, &scoped).status, Status::Missing);
        scoped.worktree = "/project";
        scoped.repo = "another-repo";
        assert_eq!(evaluate(&rows, &scoped).status, Status::Missing);
        scoped.repo = "repo";
        scoped.route = "checks/other";
        assert_eq!(evaluate(&rows, &scoped).status, Status::Missing);
    }

    #[test]
    fn later_failure_refusal_or_unknown_never_resurrects_a_pass() {
        let request = request();
        let pass = passed(&request);
        for outcome in [
            Outcome::Failed { exit: Some(1), signal: None },
            Outcome::Refused { stage: "limits".into(), reason: "timed out".into(), counts: false },
            Outcome::Unknown { reason: "completion lost".into() },
        ] {
            let mut later = pass.clone();
            later.id = "later".into();
            later.outcome = outcome;
            let wanted =
                if matches!(later.outcome, Outcome::Unknown { .. }) { Status::Unknown } else { Status::Failed };
            if matches!(later.outcome, Outcome::Refused { .. }) {
                later.verification = None; // Timeout has no completed process observation.
            }
            let result = evaluate(&[pass.clone(), later], &expected(&request));
            assert_eq!(result.status, wanted);
            assert_eq!(result.receipt.as_deref(), Some("later"));
        }
    }

    #[test]
    fn a_session_filter_cannot_hide_a_newer_failure_in_another_session() {
        let request = request();
        let pass = passed(&request);
        let mut later = pass.clone();
        later.id = "later".into();
        later.session = "session-2".into();
        later.outcome = Outcome::Failed { exit: Some(1), signal: None };
        later.verification.as_mut().unwrap().exit = Some(1);
        let rows = [pass, later];
        let mut current = expected(&request);
        assert_eq!(evaluate(&rows, &current).status, Status::Failed);
        current.session = Some("session-1");
        let scoped = evaluate(&rows, &current);
        assert_eq!(scoped.status, Status::Missing);
        assert_eq!(scoped.receipt.as_deref(), Some("later"));
        current.session = Some("session-2");
        assert_eq!(evaluate(&rows, &current).status, Status::Failed);
    }

    #[test]
    fn changed_inputs_approval_bindings_recipe_and_profile_are_stale() {
        let request = request();
        let rows = [passed(&request)];
        let mut now = expected(&request);
        now.input_digest = "inputs-after-uncommitted-edit";
        assert_eq!(evaluate(&rows, &now).status, Status::Stale);
        now.input_digest = "inputs-1";
        now.approved_on = "approval-2";
        assert_eq!(evaluate(&rows, &now).status, Status::Stale);
        let mut bound = request.clone();
        bound.inputs.insert("scope".into(), "all".into());
        assert_eq!(evaluate(&rows, &expected(&bound)).status, Status::Stale);
        let mut recipe = request.clone();
        recipe.argv.push("--all".into());
        assert_eq!(evaluate(&rows, &expected(&recipe)).status, Status::Stale);
        let mut profile = request.clone();
        profile.profile.limits.wall_ms = 15_000;
        assert_eq!(evaluate(&rows, &expected(&profile)).status, Status::Stale);
        // An old pinned environment cannot stand in for a currently different descriptor;
        // the caller obtains the current identity while resolving the approved request.
        let mut old_runtime = rows[0].clone();
        old_runtime.verification.as_mut().unwrap().project_python_digest = Some("old-runtime".into());
        assert_eq!(evaluate(&[old_runtime], &expected(&request)).status, Status::Stale);
    }

    #[test]
    fn latest_legacy_or_stale_receipt_cannot_be_skipped() {
        let request = request();
        let pass = passed(&request);
        let mut later = pass.clone();
        later.id = "later".into();
        later.verification = None;
        assert_eq!(evaluate(&[pass.clone(), later.clone()], &expected(&request)).status, Status::Unknown);
        later.verification = pass.verification.clone();
        later.approved_on = "an-older-grant".into();
        assert_eq!(evaluate(&[pass, later], &expected(&request)).status, Status::Stale);
    }

    #[test]
    fn unresolved_other_route_blocks_the_worktree_but_not_another_checkout() {
        let request = request();
        let pass = passed(&request);
        let mut interrupted = pass.clone();
        interrupted.id = "interrupted".into();
        interrupted.route = "generation/other".into();
        interrupted.session = "another-session".into();
        interrupted.phase = Phase::Prepared;
        interrupted.outcome = Outcome::Unknown { reason: "lost completion".into() };
        assert_eq!(evaluate(&[pass.clone(), interrupted.clone()], &expected(&request)).status, Status::Unknown);
        interrupted.worktree = "/another-checkout".into();
        assert_eq!(evaluate(&[pass, interrupted], &expected(&request)).status, Status::Passed);
    }

    #[test]
    fn acknowledgement_missing_status_and_inconsistent_facts_are_not_success() {
        let request = request();
        let current = expected(&request);
        let pass = passed(&request);
        let mut acknowledged = pass.clone();
        acknowledged.phase = Phase::Acknowledged;
        acknowledged.outcome = Outcome::Unknown { reason: "person inspected".into() };
        assert_eq!(evaluate(&[acknowledged], &current).status, Status::Unknown);
        for (exit, signal) in [(None, None), (Some(1), None), (Some(0), Some(9))] {
            let mut row = pass.clone();
            let evidence = row.verification.as_mut().unwrap();
            evidence.exit = exit;
            evidence.signal = signal;
            assert_eq!(evaluate(&[row], &current).status, Status::Unknown);
        }
        let mut mismatch = pass.clone();
        mismatch.verification.as_mut().unwrap().input_digest = "another-digest".into();
        assert_eq!(evaluate(&[mismatch], &current).status, Status::Unknown);
        let mut future = pass;
        future.verification.as_mut().unwrap().schema = SCHEMA + 1;
        assert_eq!(evaluate(&[future], &current).status, Status::Unknown);
    }

    #[test]
    fn regeneration_and_observation_cannot_be_required_verification() {
        let mut request = request();
        let rows = [passed(&request)];
        request.action.as_mut().unwrap().kind = ActionKind::Regenerate;
        assert_eq!(evaluate(&rows, &expected(&request)).status, Status::Unknown);
        request.action.as_mut().unwrap().kind = ActionKind::Verify;
        request.effect = Effect::Observe;
        assert_eq!(evaluate(&rows, &expected(&request)).status, Status::Unknown);
    }
}
