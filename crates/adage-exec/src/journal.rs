//! The authoritative E2 apply journal, using the existing actions.spool framing.
//!
//! A durable prepared receipt precedes worktree effects; a durable completed receipt follows.
//! A prepared receipt without completion is Unknown, never evidence that nothing happened.
//! Recovery is inspection and explicit acknowledgement, not automatic execution or rollback.
//! Worktree locks cover only this commit boundary, never a sandboxed subprocess. These locks
//! coordinate Adage writers, not editors or other programs running as the same user.

use crate::approvals::{self, ApprovalKey, Store};
use crate::request::ExecRequest;
use crate::stage::{self, Outcome, Staged};
use adage_store::{lock, spool};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::time::Duration;

const MAX_JOURNAL_BYTES: u64 = 64 << 20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Prepared,
    #[default]
    Completed,
    Acknowledged,
}

/// What started an operation. A learned context's prediction and the finish-line gate run the
/// same approved command, so a reader (an evaluation, `adage review`) cannot tell them apart by
/// the command or the event: the journal says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunOrigin {
    /// A learned context's prediction (a reflex that fired on the session's events).
    Context,
    /// A learned finish-line obligation, at an edit or at Stop.
    Obligation,
    /// The finish-line gate: a stale or never-passed verification run when the agent stops.
    StopGate,
    /// A person's `adage-exec run`.
    Manual,
}

/// Latest durable state of one operation. Old outcome-only telemetry rows are readable as
/// completed receipts with an empty id/worktree; they never act as replay reservations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    #[serde(default)]
    pub id: String,
    pub ts: String,
    pub session: String,
    pub repo: String,
    #[serde(default)]
    pub worktree: String,
    pub route: String,
    pub approved_on: String,
    pub level: String,
    #[serde(default)]
    pub phase: Phase,
    pub outcome: Outcome,
    #[serde(default)]
    pub input_digest: String,
    #[serde(default)]
    pub outputs: BTreeMap<String, stage::FileState>,
    /// What started it; `None` in a journal written before origins were recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<RunOrigin>,
    /// Facts from a staged verification, immutable through preparation/completion/recovery.
    /// Legacy receipts and non-verification actions have no authoritative verification facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<crate::verification::Evidence>,
}

impl Receipt {
    pub fn needs_recovery(&self) -> bool {
        self.phase != Phase::Acknowledged
            && (self.phase == Phase::Prepared || matches!(self.outcome, Outcome::Unknown { .. }))
    }
}

fn journal_lock(home: &Path) -> io::Result<lock::Lock> {
    lock::exclusive_within(&home.join("exec/actions.lock"), Duration::from_millis(250))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "execution journal is busy"))
}

fn worktree_lock(home: &Path, worktree: &str) -> io::Result<lock::Lock> {
    let name = crate::sha256_hex(worktree.as_bytes());
    lock::try_exclusive(&home.join("exec/worktrees").join(format!("{name}.lock")))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "another action is applying in this worktree"))
}

fn read_unlocked(home: &Path) -> io::Result<Vec<Receipt>> {
    let path = stage::spool_path(home);
    match std::fs::symlink_metadata(&path) {
        Ok(m) if !m.file_type().is_file() || m.len() > MAX_JOURNAL_BYTES => {
            return Err(io::Error::other("execution journal is not a regular file within 64 MiB"));
        }
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let all: Vec<Receipt> = spool::read_all_strict(&path)?;
    let mut index = BTreeMap::<String, usize>::new();
    let mut out: Vec<Receipt> = Vec::new();
    for r in all {
        if r.id.is_empty() {
            if r.phase != Phase::Completed {
                return Err(io::Error::other("legacy journal row cannot reserve an action"));
            }
            out.push(r);
        } else if let Some(&i) = index.get(&r.id) {
            let before = &out[i];
            let same = before.repo == r.repo
                && before.worktree == r.worktree
                && before.session == r.session
                && before.route == r.route
                && before.approved_on == r.approved_on
                && before.level == r.level
                && before.input_digest == r.input_digest
                && before.verification == r.verification;
            let transition = (before.phase == Phase::Prepared && r.phase == Phase::Completed)
                || (before.needs_recovery() && r.phase == Phase::Acknowledged && before.outcome == r.outcome);
            if !same || !transition {
                return Err(io::Error::other("invalid execution journal transition"));
            }
            out[i] = r;
        } else {
            if r.phase != Phase::Prepared || !matches!(r.outcome, Outcome::Unknown { .. }) || r.worktree.is_empty() {
                return Err(io::Error::other("execution journal operation has no valid preparation"));
            }
            index.insert(r.id.clone(), out.len());
            out.push(r);
        }
    }
    Ok(out)
}

/// Strictly read the canonical receipts. Corruption is an error, never an empty history.
pub fn list(home: &Path) -> io::Result<Vec<Receipt>> {
    let _lock = journal_lock(home)?;
    read_unlocked(home)
}

pub fn latest(home: &Path, repo: &str, route: &str) -> io::Result<Option<Receipt>> {
    Ok(list(home)?.into_iter().rev().find(|r| r.repo == repo && r.route == route))
}

/// Worktree-wide recovery block, also checked before staging so another route does not run
/// past an unresolved application. `finish` repeats this check under the apply lock.
pub fn pending(home: &Path, worktree: &Path) -> io::Result<Option<Receipt>> {
    let key = worktree.canonicalize()?.to_string_lossy().into_owned();
    Ok(list(home)?.into_iter().find(|r| r.worktree == key && r.needs_recovery()))
}

pub struct Commit<'a> {
    pub home: &'a Path,
    pub repo: &'a str,
    pub session: &'a str,
    /// Caller occurrence identity, separate from command identity: a later edit/obligation
    /// must remain eligible even when its command has not changed.
    pub occurrence: &'a str,
    pub approved_on: &'a str,
    pub request: &'a ExecRequest,
    /// What started it, recorded on the receipt (never part of its identity).
    pub origin: RunOrigin,
}

#[derive(Debug)]
pub struct Completion {
    pub id: Option<String>,
    pub outcome: Outcome,
    pub revoked: bool,
    pub replayed: bool,
}

fn refused(reason: String) -> Completion {
    Completion {
        id: None,
        outcome: Outcome::Refused { stage: "journal".into(), reason, counts: false },
        revoked: false,
        replayed: false,
    }
}

fn operation_id(c: &Commit<'_>, worktree: &str, key: &ApprovalKey, route: &str) -> String {
    crate::sha256_hex(
        crate::canonical_json(&serde_json::json!({
            "worktree": worktree, "repo": c.repo, "session": c.session, "route": route,
            "key": key, "inputs": c.request.inputs, "occurrence": c.occurrence,
        }))
        .as_bytes(),
    )
}

/// Durably retain an opt-in verification attempt that staging refused, including a timeout.
/// There is no staged exit or duration to assert. The caller reserves its attempt before
/// staging; this terminal negative record prevents an earlier pass from becoming current again.
/// Failure to write either transition leaves the caller's attempt unresolved, never ready.
pub fn withheld_verification(c: &Commit<'_>, input_digest: &str, refusal: &crate::run::Refusal) -> io::Result<Receipt> {
    if c.request.level != crate::request::Level::E2
        || c.request.effect != adage_core::Effect::Verify
        || !c.request.action.as_ref().is_some_and(|a| a.kind == crate::request::ActionKind::Verify)
    {
        return Err(io::Error::other("only an E2 verification refusal can be recorded"));
    }
    let worktree = c.request.repo_root.canonicalize()?.to_string_lossy().into_owned();
    let _worktree_lock = worktree_lock(c.home, &worktree)?;
    let _journal_lock = journal_lock(c.home)?;
    let key = ApprovalKey::for_request(c.request, c.repo);
    let route = approvals::route_label(c.request);
    let id = operation_id(c, &worktree, &key, &route);
    let rows = read_unlocked(c.home)?;
    if rows.iter().any(|r| r.worktree == worktree && r.needs_recovery()) {
        return Err(io::Error::other("worktree has unresolved execution; inspect recovery first"));
    }
    if let Some(row) = rows.into_iter().find(|r| r.id == id) {
        if row.input_digest != input_digest {
            return Err(io::Error::other("this occurrence has another input identity"));
        }
        if row.verification.is_some()
            || !matches!(row.outcome, Outcome::Refused { ref stage, ref reason, counts: false }
                if stage == refusal.stage && *reason == crate::reason_text(&refusal.reason))
        {
            return Err(io::Error::other("this occurrence already records a different verification result"));
        }
        return Ok(row);
    }
    let mut row = Receipt {
        id,
        ts: crate::time::now(),
        session: c.session.into(),
        repo: c.repo.into(),
        worktree,
        route,
        approved_on: c.approved_on.into(),
        level: "e2".into(),
        phase: Phase::Prepared,
        outcome: Outcome::Unknown { reason: "verification refusal has no durable completion".into() },
        input_digest: input_digest.into(),
        outputs: BTreeMap::new(),
        origin: Some(c.origin),
        verification: None,
    };
    spool::append_durable(&stage::spool_path(c.home), &row)?;
    row.phase = Phase::Completed;
    row.outcome =
        Outcome::Refused { stage: refusal.stage.into(), reason: crate::reason_text(&refusal.reason), counts: false };
    row.ts = crate::time::now();
    spool::append_durable(&stage::spool_path(c.home), &row)?;
    Ok(row)
}

/// Apply only after a durable reservation and a current matching grant. A lost final receipt
/// returns Unknown and leaves the durable reservation for recovery; it never claims rollback.
pub fn finish(staged: &Staged, c: &Commit<'_>) -> Completion {
    finish_inner(staged, c, || Ok(()))
}

fn finish_inner(staged: &Staged, c: &Commit<'_>, after_apply: impl FnOnce() -> io::Result<()>) -> Completion {
    let worktree = staged.repo.to_string_lossy().into_owned();
    let _worktree_lock = match worktree_lock(c.home, &worktree) {
        Ok(l) => l,
        Err(e) => return refused(format!("cannot reserve this worktree: {e}; nothing was applied")),
    };
    let key = ApprovalKey::for_request(c.request, c.repo);
    let route = approvals::route_label(c.request);
    let id = operation_id(c, &worktree, &key, &route);
    let mut receipt = Receipt {
        id: id.clone(), ts: crate::time::now(), session: c.session.into(), repo: c.repo.into(),
        worktree, route, approved_on: c.approved_on.into(), level: "e2".into(), phase: Phase::Prepared,
        outcome: Outcome::Unknown { reason: format!("operation {id} was prepared without a durable completion; inspect the worktree before acknowledging it") },
        input_digest: crate::sha256_hex(crate::canonical_json(&staged.inputs).as_bytes()),
        outputs: BTreeMap::new(),
        origin: Some(c.origin),
        verification: crate::verification::Evidence::from_staged(staged, c.request, c.repo),
    };
    {
        let _journal_lock = match journal_lock(c.home) {
            Ok(l) => l,
            Err(e) => return refused(format!("cannot open the execution journal: {e}; nothing was applied")),
        };
        let previous = match read_unlocked(c.home) {
            Ok(r) => r,
            Err(e) => return Completion {
                id: None,
                outcome: Outcome::Unknown { reason: format!("cannot verify the execution journal: {e}; this attempt did not apply changes, but earlier effects cannot be ruled out; inspect before retrying") },
                revoked: false,
                replayed: false,
            },
        };
        if let Some(r) = previous.iter().find(|r| r.worktree == receipt.worktree && r.needs_recovery()) {
            return Completion { id: Some(r.id.clone()), outcome: r.outcome.clone(), revoked: false, replayed: true };
        }
        if let Some(r) = previous.iter().find(|r| r.id == id) {
            if matches!(r.outcome, Outcome::Applied { .. })
                && (r.input_digest != receipt.input_digest
                    || r.outputs.iter().any(|(p, before)| stage::state_of(&staged.repo, p) != *before))
            {
                return refused("this occurrence already completed, but its inputs or outputs have since changed; a new occurrence is required".into());
            }
            return Completion { id: Some(id), outcome: r.outcome.clone(), revoked: false, replayed: true };
        }
        // Re-read at the commit boundary, after the subprocess has finished. Grants are local
        // authorization; a pack signature or an old snapshot cannot substitute for this check.
        let store = match Store::load(&approvals::path(c.home)) {
            Ok(s) => s,
            Err(e) => return refused(format!("cannot recheck approval: {e}; nothing was applied")),
        };
        if !store.find(&key).is_some_and(|a| a.approved_on == c.approved_on) {
            return refused("approval was revoked or changed while the action ran; nothing was applied".into());
        }
        if let Err(e) = spool::append_durable(&stage::spool_path(c.home), &receipt) {
            return refused(format!("cannot durably prepare the action: {e}; nothing was applied"));
        }
    }
    let outcome = stage::finish(staged);
    if matches!(outcome, Outcome::Applied { .. }) {
        receipt.outputs =
            staged.plan.apply.iter().map(|c| (c.path.clone(), stage::state_of(&staged.repo, &c.path))).collect();
    }
    let written = (|| {
        after_apply()?;
        let _lock = journal_lock(c.home)?;
        // Refuse to append through an obstruction or damaged tail introduced since preparation.
        read_unlocked(c.home)?;
        receipt.phase = Phase::Completed;
        receipt.outcome = outcome.clone();
        receipt.ts = crate::time::now();
        spool::append_durable(&stage::spool_path(c.home), &receipt)
    })();
    if let Err(e) = written {
        return Completion {
            id: Some(id.clone()),
            outcome: Outcome::Unknown { reason: format!("operation {id} reached apply but its completion could not be durably recorded: {e}; changes may already be present; inspect before retrying") },
            revoked: false, replayed: false,
        };
    }
    let revoked = stage::revoke_if_needed(c.home, c.repo, &receipt.route, c.approved_on, &outcome);
    Completion { id: Some(id), outcome, revoked, replayed: false }
}

/// Record a person's acknowledgement after inspection. This neither asserts success nor
/// rolls anything back, and never retries the interrupted operation. The CLI requires a person.
pub fn acknowledge(home: &Path, id: &str) -> io::Result<Receipt> {
    let first = list(home)?
        .into_iter()
        .find(|r| r.id == id && !id.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "operation not found"))?;
    let _worktree_lock = worktree_lock(home, &first.worktree)?;
    let _journal_lock = journal_lock(home)?;
    let mut r = read_unlocked(home)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "operation disappeared during recovery"))?;
    if !r.needs_recovery() {
        return Err(io::Error::other("operation does not need recovery"));
    }
    r.phase = Phase::Acknowledged;
    r.ts = crate::time::now();
    spool::append_durable(&stage::spool_path(home), &r)?;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approvals::Approval;
    use crate::request::{ActionKind, ActionSpec, Labels, Level, Origin, OutputSpec};
    use crate::sandbox::Profile;
    use crate::stage::{After, Change, ChangeKind, PreState, StageBackend};
    use adage_core::Effect;
    use std::io::Write;
    use std::path::PathBuf;

    struct Fixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
        req: ExecRequest,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let repo = dir.path().join("repo");
            std::fs::create_dir_all(repo.join("gen")).unwrap();
            std::fs::write(repo.join("gen/out"), "before").unwrap();
            std::fs::write(repo.join("source"), "source").unwrap();
            let req = ExecRequest {
                repo_root: repo.canonicalize().unwrap(),
                project_python: None,
                argv: vec!["make".into(), "gen".into()],
                inputs: BTreeMap::new(),
                contracts: BTreeMap::new(),
                effect: Effect::Regenerate,
                level: Level::E2,
                output: OutputSpec::default(),
                scripts: vec![],
                profile: Profile::for_level(Level::E2),
                labels: Labels::default(),
                origin: Origin::Local,
                route_exec_digest: None,
                action: Some(ActionSpec {
                    kind: ActionKind::Regenerate,
                    outputs: vec!["gen/**".into()],
                    cache: vec![],
                }),
            };
            let mut store = Store::default();
            store.approve(Approval {
                key: ApprovalKey::for_request(&req, "repo"),
                route: approvals::route_label(&req),
                approved_on: "approved".into(),
                by: None,
                basis: serde_json::Value::Null,
                auto: false,
                terms: None,
            });
            store.save(&approvals::path(&home)).unwrap();
            Self { _dir: dir, home, req }
        }

        fn staged(&self) -> Staged {
            let action = self.req.action.clone().unwrap();
            let pre = PreState::take(&self.req.repo_root, &action).unwrap();
            let inputs = stage::input_snapshot(&self.req.repo_root, &action).unwrap();
            let changes = if action.kind == ActionKind::Verify {
                vec![]
            } else {
                vec![Change {
                    path: "gen/out".into(),
                    kind: ChangeKind::Modified,
                    after: After::File { bytes: b"after".to_vec(), exec: false },
                    before_file: true,
                }]
            };
            let plan = stage::judge(changes, &action, 1024);
            Staged {
                repo: self.req.repo_root.clone(),
                action,
                exit: Some(0),
                signal: None,
                duration_ms: 1,
                plan,
                pre,
                inputs,
                fields: None,
                backend: StageBackend::Copy,
                stderr: vec![],
                rewritten: vec![],
            }
        }

        fn commit<'a>(&'a self, occurrence: &'a str) -> Commit<'a> {
            Commit {
                home: &self.home,
                repo: "repo",
                session: "session",
                occurrence,
                approved_on: "approved",
                request: &self.req,
                origin: RunOrigin::Obligation,
            }
        }

        fn output(&self) -> String {
            std::fs::read_to_string(self.req.repo_root.join("gen/out")).unwrap()
        }

        fn verification() -> Self {
            let mut fixture = Self::new();
            fixture.req.argv = vec!["make".into(), "check".into()];
            fixture.req.effect = Effect::Verify;
            fixture.req.action = Some(ActionSpec { kind: ActionKind::Verify, outputs: vec![], cache: vec![] });
            let mut store = Store::default();
            store.approve(Approval {
                key: ApprovalKey::for_request(&fixture.req, "repo"),
                route: approvals::route_label(&fixture.req),
                approved_on: "approved".into(),
                by: None,
                basis: serde_json::Value::Null,
                auto: false,
                terms: None,
            });
            store.save(&approvals::path(&fixture.home)).unwrap();
            fixture
        }
    }

    #[test]
    fn obstructed_journal_reports_uncertainty_without_applying_changes() {
        let f = Fixture::new();
        std::fs::create_dir_all(stage::spool_path(&f.home)).unwrap();
        let done = finish(&f.staged(), &f.commit("one"));
        assert!(matches!(done.outcome, Outcome::Unknown { .. }), "{done:?}");
        assert_eq!(f.output(), "before");
    }

    #[test]
    fn durable_completion_deduplicates_occurrence_but_allows_later_obligations() {
        let f = Fixture::new();
        let staged = f.staged();
        let first = finish(&staged, &f.commit("one"));
        assert!(matches!(first.outcome, Outcome::Applied { .. }), "{first:?}");
        let duplicate = finish(&f.staged(), &f.commit("one"));
        assert!(duplicate.replayed);
        assert_eq!(duplicate.id, first.id);
        assert_eq!(list(&f.home).unwrap().len(), 1);
        let later = finish(&f.staged(), &f.commit("two"));
        assert!(matches!(later.outcome, Outcome::Applied { .. }));
        assert!(!later.replayed);
        assert_ne!(later.id, first.id);
        assert_eq!(list(&f.home).unwrap().len(), 2);
    }

    #[test]
    fn a_receipt_records_its_origin_without_it_entering_the_identity() {
        let f = Fixture::new();
        let gate = Commit { origin: RunOrigin::StopGate, ..f.commit("one") };
        let done = finish(&f.staged(), &gate);
        let row = list(&f.home).unwrap().pop().unwrap();
        assert_eq!(row.origin, Some(RunOrigin::StopGate));
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["origin"], "stop-gate");
        // The same occurrence from another origin is the same operation: identity is unchanged.
        let again = finish(&f.staged(), &f.commit("one"));
        assert!(again.replayed && again.id == done.id);
        // A journal written before origins were recorded still reads.
        let mut old = json.clone();
        old.as_object_mut().unwrap().remove("origin");
        assert_eq!(serde_json::from_value::<Receipt>(old).unwrap().origin, None);
    }

    #[test]
    fn lost_completion_is_unknown_and_blocks_until_person_acknowledges() {
        let f = Fixture::new();
        let done =
            finish_inner(&f.staged(), &f.commit("one"), || Err(io::Error::other("injected completion write failure")));
        assert!(matches!(done.outcome, Outcome::Unknown { .. }));
        assert_eq!(f.output(), "after");
        let interrupted = pending(&f.home, &f.req.repo_root).unwrap().unwrap();
        assert_eq!(interrupted.phase, Phase::Prepared);
        assert!(interrupted.needs_recovery());
        // Read anew, as a restarted hook does; do not infer success from matching output bytes.
        let blocked = finish(&f.staged(), &f.commit("two"));
        assert!(matches!(blocked.outcome, Outcome::Unknown { .. }));
        assert_eq!(list(&f.home).unwrap().len(), 1);
        let ack = acknowledge(&f.home, &interrupted.id).unwrap();
        assert_eq!(ack.phase, Phase::Acknowledged);
        assert!(matches!(ack.outcome, Outcome::Unknown { .. }), "acknowledgement is not success");
        assert!(pending(&f.home, &f.req.repo_root).unwrap().is_none());
        assert!(matches!(finish(&f.staged(), &f.commit("two")).outcome, Outcome::Applied { .. }));
    }

    #[test]
    fn revoked_grant_and_changed_source_refuse_at_commit() {
        let f = Fixture::new();
        let staged = f.staged();
        Store::default().save(&approvals::path(&f.home)).unwrap();
        assert!(matches!(finish(&staged, &f.commit("one")).outcome, Outcome::Refused { .. }));
        assert_eq!(f.output(), "before");
        let f = Fixture::new();
        let staged = f.staged();
        std::fs::write(f.req.repo_root.join("source"), "new source").unwrap();
        assert!(
            matches!(finish(&staged, &f.commit("one")).outcome, Outcome::Refused { stage, .. } if stage == "inputs")
        );
        assert_eq!(f.output(), "before");
    }

    #[test]
    fn changed_output_prevents_apply_and_cannot_be_hidden_by_receipt_replay() {
        let f = Fixture::new();
        let staged = f.staged();
        std::fs::write(f.req.repo_root.join("gen/out"), "person's edit").unwrap();
        assert!(
            matches!(finish(&staged, &f.commit("one")).outcome, Outcome::Refused { stage, .. } if stage == "conflict")
        );
        assert_eq!(f.output(), "person's edit");
        let f = Fixture::new();
        assert!(matches!(finish(&f.staged(), &f.commit("one")).outcome, Outcome::Applied { .. }));
        std::fs::write(f.req.repo_root.join("gen/out"), "person's edit").unwrap();
        assert!(matches!(finish(&f.staged(), &f.commit("one")).outcome, Outcome::Refused { .. }));
        assert_eq!(f.output(), "person's edit");
    }

    #[test]
    fn corrupt_journal_is_not_empty_history() {
        let f = Fixture::new();
        finish(&f.staged(), &f.commit("one"));
        std::fs::OpenOptions::new().append(true).open(stage::spool_path(&f.home)).unwrap().write_all(b"torn").unwrap();
        assert!(list(&f.home).is_err());
        assert!(matches!(finish(&f.staged(), &f.commit("two")).outcome, Outcome::Unknown { .. }));
    }

    #[test]
    fn competing_apply_is_refused_while_the_first_commit_holds_the_worktree() {
        let f = Fixture::new();
        let staged = f.staged();
        std::thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let (staged_ref, fixture_ref) = (&staged, &f);
            let first = scope.spawn(move || {
                finish_inner(staged_ref, &fixture_ref.commit("one"), || {
                    tx.send(()).unwrap();
                    wait.recv().unwrap();
                    Ok(())
                })
            });
            rx.recv().unwrap();
            let other = finish(&staged, &f.commit("one"));
            assert!(matches!(other.outcome, Outcome::Refused { .. }));
            release.send(()).unwrap();
            assert!(matches!(first.join().unwrap().outcome, Outcome::Applied { .. }));
        });
        assert_eq!(list(&f.home).unwrap().len(), 1);
    }

    #[test]
    fn partial_apply_reports_unknown_and_does_not_claim_rollback() {
        let f = Fixture::new();
        let mut staged = f.staged();
        // A deterministic I/O failure after the first rename: removing an absent path. This
        // exercises apply's partial-failure boundary without special permissions or races.
        staged.plan.apply.push(Change {
            path: "gen/missing".into(),
            kind: ChangeKind::Deleted,
            after: After::Absent,
            before_file: true,
        });
        let done = finish(&staged, &f.commit("one"));
        assert!(matches!(done.outcome, Outcome::Unknown { .. }));
        assert_eq!(f.output(), "after");
        assert!(pending(&f.home, &f.req.repo_root).unwrap().is_some());
    }

    #[test]
    fn a_well_framed_but_impossible_transition_cannot_erase_recovery() {
        let f = Fixture::new();
        finish_inner(&f.staged(), &f.commit("one"), || Err(io::Error::other("interrupted")));
        let mut r = list(&f.home).unwrap().pop().unwrap();
        // A mismatched worktree on the same id must not hide the original reservation.
        r.worktree.push_str("-different");
        r.phase = Phase::Completed;
        r.outcome = Outcome::Applied { changed: vec![], discarded: 0 };
        spool::append_durable(&stage::spool_path(&f.home), &r).unwrap();
        assert!(list(&f.home).is_err());
        assert!(matches!(finish(&f.staged(), &f.commit("two")).outcome, Outcome::Unknown { .. }));
    }

    #[test]
    fn input_capture_refuses_missing_directories_and_oversized_files() {
        let f = Fixture::new();
        let action = f.req.action.as_ref().unwrap();
        assert!(stage::input_snapshot(&f.req.repo_root.join("missing"), action).is_err());
        let file = std::fs::File::create(f.req.repo_root.join("oversized")).unwrap();
        file.set_len(stage::MAX_APPLY_BYTES + 1).unwrap();
        assert!(stage::input_snapshot(&f.req.repo_root, action).is_err());
    }

    #[test]
    fn durable_verification_binds_working_inputs_without_relying_on_head() {
        use crate::verification::{evaluate, Expectation, Status};
        let f = Fixture::verification();
        let staged = f.staged();
        assert!(matches!(finish(&staged, &f.commit("one")).outcome, Outcome::Applied { .. }));
        let rows = list(&f.home).unwrap();
        let evidence = rows[0].verification.as_ref().unwrap();
        assert_eq!(evidence.exit, Some(0));
        assert_eq!(evidence.duration_ms, staged.duration_ms);
        let worktree = f.req.repo_root.to_string_lossy();
        let label = approvals::route_label(&f.req);
        let mut expected = Expectation {
            repo: "repo",
            worktree: &worktree,
            route: &label,
            session: Some("session"),
            approved_on: "approved",
            request: &f.req,
            input_digest: &rows[0].input_digest,
        };
        assert_eq!(evaluate(&rows, &expected).status, Status::Passed);
        std::fs::write(f.req.repo_root.join("source"), "uncommitted change").unwrap();
        let now = stage::input_snapshot(&f.req.repo_root, f.req.action.as_ref().unwrap()).unwrap();
        let changed = crate::sha256_hex(crate::canonical_json(&now).as_bytes());
        expected.input_digest = &changed;
        assert_eq!(evaluate(&rows, &expected).status, Status::Stale);
        // Receipts from before this field existed remain readable, but cannot be passed off as
        // verification facts. A regeneration still receives no verification evidence.
        let mut old = serde_json::to_value(&rows[0]).unwrap();
        old.as_object_mut().unwrap().remove("verification");
        assert!(serde_json::from_value::<Receipt>(old).unwrap().verification.is_none());
        let regeneration = Fixture::new();
        finish(&regeneration.staged(), &regeneration.commit("one"));
        assert!(list(&regeneration.home).unwrap()[0].verification.is_none());
    }

    #[test]
    fn verification_facts_cannot_change_during_journal_completion() {
        let f = Fixture::verification();
        finish_inner(&f.staged(), &f.commit("one"), || Err(io::Error::other("lost completion")));
        let mut row = list(&f.home).unwrap().pop().unwrap();
        assert_eq!(row.phase, Phase::Prepared);
        row.verification.as_mut().unwrap().binding_digest = "forged binding".into();
        row.phase = Phase::Completed;
        row.outcome = Outcome::Applied { changed: vec![], discarded: 0 };
        spool::append_durable(&stage::spool_path(&f.home), &row).unwrap();
        assert!(list(&f.home).is_err());
    }

    #[test]
    fn staging_timeout_supersedes_prior_pass_without_inventing_a_process_result() {
        use crate::verification::{evaluate, Expectation, Status};
        let f = Fixture::verification();
        finish(&f.staged(), &f.commit("one"));
        let before = list(&f.home).unwrap().pop().unwrap();
        let refusal = crate::run::Refusal { stage: "limits", reason: "timed out".into() };
        let row = withheld_verification(&f.commit("two"), &before.input_digest, &refusal).unwrap();
        assert_eq!(row.phase, Phase::Completed);
        assert!(row.verification.is_none());
        assert!(matches!(row.outcome, Outcome::Refused { counts: false, .. }));
        let rows = list(&f.home).unwrap();
        assert_eq!(rows.len(), 2);
        let label = approvals::route_label(&f.req);
        let worktree = f.req.repo_root.to_string_lossy();
        let expected = Expectation {
            repo: "repo",
            worktree: &worktree,
            route: &label,
            session: None,
            approved_on: "approved",
            request: &f.req,
            input_digest: &before.input_digest,
        };
        assert_eq!(evaluate(&rows, &expected).status, Status::Failed);
        let replay = withheld_verification(&f.commit("two"), &before.input_digest, &refusal).unwrap();
        assert_eq!(replay.id, row.id);
        assert_eq!(list(&f.home).unwrap().len(), 2);
        assert!(withheld_verification(&f.commit("two"), "different inputs", &refusal).is_err());
        assert!(withheld_verification(&f.commit("one"), &before.input_digest, &refusal).is_err());
    }
}
