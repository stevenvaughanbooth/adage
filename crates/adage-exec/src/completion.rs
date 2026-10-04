//! Explicit completion requirements and their durable attempt budget.
//!
//! Requirements refer to existing, person-approved verification routes. Learning can propose
//! routes; it cannot make a check mandatory. Execution facts stay in the action journal. This
//! small log records only admission and completion of automatic attempts, so a crashed hook
//! cannot silently reset its budget or reuse a previous pass.

use crate::approvals::{ApprovalKey, Store};
use crate::request::{ActionKind, ExecRequest, Level};
use crate::{policy, route, verification};
use adage_core::{CommandTable, Effect};
use adage_store::{lock, spool, Paths};
use adage_tracks::{Index, Step};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const MAX_REQUIREMENTS: usize = 8;
pub const MAX_ATTEMPTS: usize = 2;
const MAX_REQUIREMENTS_BYTES: u64 = 1 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirements {
    pub schema: u32,
    pub repositories: BTreeMap<String, Vec<String>>,
}

impl Default for Requirements {
    fn default() -> Self {
        Self { schema: 1, repositories: BTreeMap::new() }
    }
}

impl Requirements {
    pub fn load(home: &Path) -> Result<Self> {
        let path = home.join("completion.json");
        regular_within(&path, MAX_REQUIREMENTS_BYTES)?;
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(MAX_REQUIREMENTS_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_REQUIREMENTS_BYTES {
            bail!("completion requirements exceed their size limit");
        }
        let value: Self = serde_json::from_slice(&bytes).context("invalid completion requirements")?;
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<()> {
        if self.schema != 1 || self.repositories.len() > 1024 {
            bail!("unsupported or oversized completion requirements");
        }
        for (repo, routes) in &self.repositories {
            if repo.is_empty() || repo.len() > 1024 || routes.len() > MAX_REQUIREMENTS {
                bail!("invalid repository or too many required checks");
            }
            for (i, route) in routes.iter().enumerate() {
                if route.len() > 512
                    || route.rsplit_once('/').is_none_or(|(track, id)| track.is_empty() || id.is_empty())
                    || route.chars().any(|c| c.is_whitespace() || c.is_control())
                    || routes[..i].contains(route)
                {
                    bail!("invalid or duplicate completion route {route:?}");
                }
            }
        }
        Ok(())
    }

    /// Caller holds the configuration lock and has passed `person::require_person`.
    pub fn save(&self, home: &Path) -> Result<()> {
        use std::io::Write;
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)?;
        if bytes.len() as u64 > MAX_REQUIREMENTS_BYTES {
            bail!("completion requirements exceed their size limit");
        }
        std::fs::create_dir_all(home)?;
        let path = home.join("completion.json");
        regular_within(&path, MAX_REQUIREMENTS_BYTES)?;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        let tmp = home.join(format!(".completion.json.{}.{nonce}.tmp", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        let result = (|| -> Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&tmp, &path)?;
            #[cfg(unix)]
            std::fs::File::open(home)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }

    pub fn routes(&self, repo: &str) -> &[String] {
        self.repositories.get(repo).map(Vec::as_slice).unwrap_or_default()
    }
}

fn regular_within(path: &Path, limit: u64) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() || m.len() > limit => {
            bail!("{} is not a regular file within its size limit", path.display())
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    Ok(())
}

pub struct Check {
    pub label: String,
    pub command: String,
    pub approved_on: String,
    pub request: ExecRequest,
}

/// Resolve a fixed verification command through exactly the existing route, approval and policy
/// gates. A requirement is never authority to execute by itself.
pub fn prepare(
    label: &str,
    repo: &str,
    root: &Path,
    index: &Index,
    paths: &Paths,
    pol: &policy::Policy,
    store: &Store,
) -> Result<Check> {
    let (track, id) = label.rsplit_once('/').context("expected track/route")?;
    let ir = route::find(index, repo, track, id).context("required route is absent from the compiled index")?;
    let Step::Run { command } = &ir.route.step else { bail!("required route is not a command") };
    if command.contains('{') {
        bail!("completion requires a fixed verification command");
    }
    let table = CommandTable::load();
    for approval in
        store.approvals.iter().filter(|a| a.key.repo == repo && a.route == label && a.key.level == Level::E2)
    {
        let Ok(plan) = route::plan(ir, index, root, &paths.tracks(), Level::E2, approval.terms.as_ref(), table) else {
            continue;
        };
        let mut request = plan.request;
        request.inputs =
            route::bound_from_command(&request.argv, command).context("required command cannot be bound")?;
        if request.effect != Effect::Verify || request.action.as_ref().is_none_or(|a| a.kind != ActionKind::Verify) {
            continue;
        }
        let key = ApprovalKey::for_request(&request, repo);
        if key != approval.key {
            continue;
        }
        let classified = crate::classify::effect(&request.bind()?, table);
        pol.permits(&request, classified).map_err(anyhow::Error::msg)?;
        return Ok(Check {
            label: label.into(),
            command: command.clone(),
            approved_on: approval.approved_on.clone(),
            request,
        });
    }
    bail!("required verification has no current E2 approval; review and approve its exact command first")
}

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub route: String,
    pub command: Option<String>,
    pub readiness: verification::Readiness,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub repo: String,
    pub worktree: String,
    pub configured: bool,
    pub ready: bool,
    pub evidence_scope: &'static str,
    pub checks: Vec<CheckReport>,
}

pub fn unknown(reason: impl Into<String>) -> verification::Readiness {
    verification::Readiness { status: verification::Status::Unknown, reason: reason.into(), receipt: None }
}

pub fn inspect(
    repo: &str,
    root: &Path,
    index: &Index,
    paths: &Paths,
    session: Option<&str>,
    deadline: Option<Instant>,
) -> Result<Report> {
    // Route planning fingerprints an approved project-Python environment. Share the hook's
    // remaining budget through the existing checkpoint mechanism; CLI inspection has no hook
    // deadline and remains an explicit read-only operation.
    let _planning_budget = deadline
        .map(|until| crate::project_python::budget(until.saturating_duration_since(Instant::now()).as_millis() as u64));
    let requirements = Requirements::load(&paths.root)?;
    let worktree = root.canonicalize()?.to_string_lossy().into_owned();
    let labels = requirements.routes(repo);
    let mut report = Report {
        schema: 1, repo: repo.into(), worktree: worktree.clone(), configured: !labels.is_empty(), ready: false,
        evidence_scope: "approved verification process exited zero on matching inputs; test coverage and external state are not established",
        checks: Vec::new(),
    };
    if labels.is_empty() {
        return Ok(report);
    }
    let pol = policy::load(&paths.root)?;
    let store = Store::load(&crate::approvals::path(&paths.root))?;
    let rows = crate::journal::list(&paths.root)?;
    let attempts = Attempts::read(&paths.root, &worktree)?;
    attempts.validate_receipts(&rows, &worktree)?;
    for label in labels {
        if deadline.is_some_and(|until| Instant::now() >= until) {
            report.checks.push(CheckReport {
                route: label.clone(),
                command: None,
                readiness: unknown("completion readiness could not be checked within the hook deadline"),
            });
            continue;
        }
        let prepared = prepare(label, repo, root, index, paths, &pol, &store);
        let (command, readiness) = match prepared {
            Err(e) => (None, unknown(crate::reason_text(&format!("{e:#}")))),
            Ok(check) => {
                let mut readiness = if attempts.pending() {
                    unknown("an automatic completion attempt was interrupted or is still running; inspect before acknowledging it")
                } else {
                    match crate::stage::input_snapshot_until(
                        Path::new(&worktree),
                        check.request.action.as_ref().unwrap(),
                        deadline,
                    ) {
                        Err(e) => unknown(crate::reason_text(&e.reason)),
                        Ok(inputs) => verification::evaluate(
                            &rows,
                            &verification::Expectation {
                                repo,
                                worktree: &worktree,
                                route: label,
                                session,
                                approved_on: &check.approved_on,
                                request: &check.request,
                                input_digest: &crate::sha256_hex(crate::canonical_json(&inputs).as_bytes()),
                            },
                        ),
                    }
                };
                if readiness.status == verification::Status::Passed
                    && attempts.invalidates(label, readiness.receipt.as_deref(), &rows)
                {
                    readiness = unknown(
                        "an acknowledged attempt has no newer verification receipt; acknowledgement is not a pass",
                    );
                }
                (Some(check.command), readiness)
            }
        };
        report.checks.push(CheckReport { route: label.clone(), command, readiness });
    }
    report.ready = report.checks.iter().all(|c| c.readiness.status == verification::Status::Passed);
    Ok(report)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum AttemptEvent {
    Started { id: usize, session: String, route: String, prior_receipt: Option<String> },
    Finished { id: usize, receipt: String },
    Acknowledged { id: usize },
    Continued { session: String },
}

#[derive(Default)]
pub struct Attempts {
    events: Vec<AttemptEvent>,
    unfinished: BTreeMap<usize, (String, String)>,
}

impl Attempts {
    fn location(home: &Path, worktree: &str) -> PathBuf {
        home.join("exec/completion").join(crate::sha256_hex(worktree.as_bytes()))
    }

    fn read(home: &Path, worktree: &str) -> Result<Self> {
        let path = Self::location(home, worktree).with_extension("spool");
        regular_within(&path, 4 << 20)?;
        let events: Vec<AttemptEvent> = spool::read_all_strict(&path)?;
        let mut result = Self::default();
        for event in events {
            result.accept(event)?;
        }
        Ok(result)
    }

    fn accept(&mut self, event: AttemptEvent) -> Result<()> {
        match &event {
            AttemptEvent::Started { id, session, route, .. } => {
                if *id != self.events.len() || self.pending() || self.count(session, route) >= MAX_ATTEMPTS {
                    bail!("invalid completion attempt reservation");
                }
                self.unfinished.insert(*id, (session.clone(), route.clone()));
            }
            AttemptEvent::Finished { id, receipt } => {
                if receipt.is_empty() || self.unfinished.remove(id).is_none() {
                    bail!("invalid completed attempt");
                }
            }
            AttemptEvent::Acknowledged { id } => {
                if self.unfinished.remove(id).is_none() {
                    bail!("invalid acknowledged attempt");
                }
            }
            AttemptEvent::Continued { session } => {
                if self.continued(session) {
                    bail!("duplicate completion continuation");
                }
            }
        }
        self.events.push(event);
        Ok(())
    }

    pub fn pending(&self) -> bool {
        !self.unfinished.is_empty()
    }

    /// A finished attempt must name the exact durable operation it completed. Missing journal
    /// rows or cross-session/worktree references are inconsistent evidence, not an older pass.
    fn validate_receipts(&self, rows: &[crate::journal::Receipt], worktree: &str) -> Result<()> {
        let by_id: BTreeMap<_, _> = rows.iter().enumerate().map(|(i, r)| (r.id.as_str(), (i, r))).collect();
        let mut used = std::collections::BTreeSet::new();
        for event in &self.events {
            let AttemptEvent::Finished { id, receipt } = event else { continue };
            let Some(AttemptEvent::Started { session, route, prior_receipt, .. }) = self.events.get(*id) else {
                bail!("completion receipt has no original reservation");
            };
            let (position, row) =
                by_id.get(receipt.as_str()).context("completion receipt is missing from the execution journal")?;
            if !used.insert(receipt)
                || row.phase == crate::journal::Phase::Prepared
                || row.session != *session
                || row.route != *route
                || row.worktree != worktree
            {
                bail!("completion receipt does not match its reserved attempt");
            }
            if let Some(prior) = prior_receipt {
                let (prior_position, _) =
                    by_id.get(prior.as_str()).context("completion's prior execution receipt is missing")?;
                if position <= prior_position {
                    bail!("completion reused a receipt that predates its attempt");
                }
            }
        }
        Ok(())
    }

    fn invalidates(&self, route: &str, receipt: Option<&str>, rows: &[crate::journal::Receipt]) -> bool {
        let Some((id, prior)) = self.events.iter().rev().find_map(|e| match e {
            AttemptEvent::Started { id, route: r, prior_receipt, .. } if r == route => {
                Some((*id, prior_receipt.as_deref()))
            }
            _ => None,
        }) else {
            return false;
        };
        let Some(position) = receipt.and_then(|id| rows.iter().position(|r| r.id == id)) else { return true };
        let finished = self.events.iter().find_map(|e| match e {
            AttemptEvent::Finished { id: i, receipt } if *i == id => Some(receipt.as_str()),
            _ => None,
        });
        if let Some(finished) = finished {
            return rows.iter().position(|r| r.id == finished).is_none_or(|completed| position < completed);
        }
        // Comparing unequal ids is insufficient: session-scoped inspection might select a
        // receipt older than the prior one. Only strictly newer journal evidence can recover
        // readiness after an acknowledged attempt.
        prior.is_some_and(|prior| rows.iter().position(|r| r.id == prior).is_none_or(|before| position <= before))
    }

    pub fn count(&self, session: &str, route: &str) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, AttemptEvent::Started { session: s, route: r, .. } if s == session && r == route))
            .count()
    }

    pub fn continued(&self, session: &str) -> bool {
        self.events.iter().any(|e| matches!(e, AttemptEvent::Continued { session: s } if s == session))
    }
}

pub struct AttemptGuard {
    _lock: lock::Lock,
    path: PathBuf,
    pub attempts: Attempts,
}

impl AttemptGuard {
    pub fn open(home: &Path, root: &Path) -> Result<Self> {
        let worktree = root.canonicalize()?.to_string_lossy().into_owned();
        let base = Attempts::location(home, &worktree);
        // A parallel sandbox probe can briefly inherit the parent's lock descriptor between
        // fork and exit/exec. Match the action journal's bounded patience without allowing a
        // second active completion check to acquire the lock.
        let lock = lock::exclusive_within(&base.with_extension("lock"), std::time::Duration::from_millis(250))?
            .context("another completion check is running")?;
        Ok(Self { _lock: lock, path: base.with_extension("spool"), attempts: Attempts::read(home, &worktree)? })
    }

    fn append(&mut self, event: AttemptEvent) -> Result<()> {
        regular_within(&self.path, 4 << 20)?;
        // Validate before persisting. On an I/O failure the caller stops; strict replay will
        // reject a torn record, and a complete reservation remains unknown until inspected.
        self.attempts.accept(event.clone())?;
        spool::append_durable(&self.path, &event)?;
        Ok(())
    }

    pub fn begin(&mut self, session: &str, route: &str, prior_receipt: Option<&str>) -> Result<usize> {
        let id = self.attempts.events.len();
        self.append(AttemptEvent::Started {
            id,
            session: session.into(),
            route: route.into(),
            prior_receipt: prior_receipt.map(str::to_string),
        })?;
        Ok(id)
    }

    pub fn finish(&mut self, id: usize, receipt: &str) -> Result<()> {
        self.append(AttemptEvent::Finished { id, receipt: receipt.into() })
    }

    pub fn continue_once(&mut self, session: &str) -> Result<()> {
        self.append(AttemptEvent::Continued { session: session.into() })
    }

    /// Acknowledgement permits another bounded attempt; it never creates passing evidence.
    /// Caller must require a person and must also resolve any pending action journal receipt.
    pub fn acknowledge(&mut self) -> Result<usize> {
        let ids: Vec<usize> = self.attempts.unfinished.keys().copied().collect();
        for id in &ids {
            self.append(AttemptEvent::Acknowledged { id: *id })?;
        }
        Ok(ids.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(id: &str, session: &str) -> crate::journal::Receipt {
        crate::journal::Receipt {
            id: id.into(),
            ts: "1".into(),
            session: session.into(),
            repo: "repo".into(),
            worktree: "/project".into(),
            route: "checks/test".into(),
            approved_on: "approved".into(),
            level: "e2".into(),
            phase: crate::journal::Phase::Completed,
            outcome: crate::stage::Outcome::Applied { changed: vec![], discarded: 0 },
            input_digest: "inputs".into(),
            outputs: BTreeMap::new(),
            origin: None,
            verification: None,
        }
    }

    #[test]
    fn requirements_are_explicit_versioned_and_strict() {
        let home = tempfile::tempdir().unwrap();
        let mut r = Requirements::load(home.path()).unwrap();
        assert!(r.routes("repo").is_empty());
        r.repositories.insert("repo".into(), vec!["checks/test".into()]);
        r.save(home.path()).unwrap();
        assert_eq!(Requirements::load(home.path()).unwrap().routes("repo"), ["checks/test"]);
        r.repositories.get_mut("repo").unwrap().push("checks/test".into());
        assert!(r.save(home.path()).is_err());
        std::fs::write(home.path().join("completion.json"), r#"{"schema":2,"repositories":{}}"#).unwrap();
        assert!(Requirements::load(home.path()).is_err());
    }

    #[test]
    fn interrupted_attempt_is_durable_and_acknowledgement_is_not_a_pass_or_budget_reset() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut guard = AttemptGuard::open(home.path(), root.path()).unwrap();
        guard.begin("session", "checks/test", Some("old-pass")).unwrap();
        assert!(AttemptGuard::open(home.path(), root.path()).is_err(), "single flight");
        drop(guard);
        let mut guard = AttemptGuard::open(home.path(), root.path()).unwrap();
        assert!(guard.attempts.pending());
        assert!(guard.begin("session", "checks/test", None).is_err());
        assert_eq!(guard.acknowledge().unwrap(), 1);
        let rows = [receipt("old-pass", "session"), receipt("new-pass", "session")];
        assert!(guard.attempts.invalidates("checks/test", Some("old-pass"), &rows));
        assert!(!guard.attempts.invalidates("checks/test", Some("new-pass"), &rows));
        let id = guard.begin("session", "checks/test", Some("old-pass")).unwrap();
        guard.finish(id, "durable-action-receipt").unwrap();
        assert!(!guard.attempts.pending());
        assert_eq!(guard.attempts.count("session", "checks/test"), 2);
        assert!(guard.begin("session", "checks/test", None).is_err());
        guard.continue_once("session").unwrap();
        drop(guard);
        let guard = AttemptGuard::open(home.path(), root.path()).unwrap();
        assert!(guard.attempts.continued("session"));
        assert_eq!(guard.attempts.count("session", "checks/test"), 2);
    }

    #[test]
    fn a_transient_holder_does_not_leave_completion_busy() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let guard = AttemptGuard::open(home.path(), root.path()).unwrap();
        // Model the brief remaining lifetime of an inherited descriptor. Using a holder thread
        // avoids forking this parallel test process and inheriting other tests' unrelated locks.
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            drop(guard);
        });
        let reopened = AttemptGuard::open(home.path(), root.path());
        release.join().unwrap();
        assert!(reopened.is_ok(), "a released holder cannot leave completion busy: {:?}", reopened.err());
    }

    #[test]
    fn a_short_lived_sandbox_child_may_outlive_the_parent_lock_guard() {
        use std::io::Write;
        use std::os::fd::FromRawFd;

        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let guard = AttemptGuard::open(home.path(), root.path()).unwrap();
        let mut pipe = [0; 2];
        // SAFETY: pipe points to two valid integers; the child below uses only syscalls on
        // inherited descriptors and exits without touching Rust's multithreaded runtime.
        assert_eq!(unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            unsafe {
                libc::close(pipe[1]);
                let mut byte = 0u8;
                while libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1) < 0 {}
                libc::_exit(0);
            }
        }
        unsafe {
            libc::close(pipe[0]);
        }
        // The inherited descriptor keeps the lock held until this child exits. Release it
        // shortly after acquisition starts, reproducing the sandbox probe's transient hold.
        drop(guard);
        let release = std::thread::spawn(move || {
            let mut writer = unsafe { std::fs::File::from_raw_fd(pipe[1]) };
            std::thread::sleep(std::time::Duration::from_millis(75));
            writer.write_all(&[1]).unwrap();
        });
        let reopened = AttemptGuard::open(home.path(), root.path());
        release.join().unwrap();
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status));
        assert!(reopened.is_ok(), "an exited probe cannot leave completion busy: {:?}", reopened.err());
    }

    #[test]
    fn truncated_attempt_history_cannot_reset_the_budget() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut guard = AttemptGuard::open(home.path(), root.path()).unwrap();
        guard.begin("s", "r/check", None).unwrap();
        let path = guard.path.clone();
        drop(guard);
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_len(5).unwrap();
        assert!(AttemptGuard::open(home.path(), root.path()).is_err());
    }

    #[test]
    fn acknowledged_failure_cannot_resurrect_an_older_session_receipt() {
        let mut attempts = Attempts::default();
        attempts
            .accept(AttemptEvent::Started {
                id: 0,
                session: "interrupted".into(),
                route: "checks/test".into(),
                prior_receipt: Some("latest-before".into()),
            })
            .unwrap();
        attempts.accept(AttemptEvent::Acknowledged { id: 0 }).unwrap();
        let rows =
            [receipt("older-session-pass", "older"), receipt("latest-before", "other"), receipt("fresh", "later")];
        assert!(attempts.invalidates("checks/test", Some("older-session-pass"), &rows));
        assert!(attempts.invalidates("checks/test", Some("latest-before"), &rows));
        assert!(!attempts.invalidates("checks/test", Some("fresh"), &rows));
        assert!(
            attempts.invalidates("checks/test", Some("fresh"), &rows[2..]),
            "missing history is not proof of freshness"
        );
    }

    #[test]
    fn completed_attempt_requires_its_own_present_and_newer_journal_receipt() {
        let mut attempts = Attempts::default();
        attempts
            .accept(AttemptEvent::Started {
                id: 0,
                session: "session".into(),
                route: "checks/test".into(),
                prior_receipt: Some("old-pass".into()),
            })
            .unwrap();
        attempts.accept(AttemptEvent::Finished { id: 0, receipt: "new-result".into() }).unwrap();
        let rows = [receipt("old-pass", "older-session"), receipt("new-result", "session")];
        attempts.validate_receipts(&rows, "/project").unwrap();
        assert!(
            attempts.validate_receipts(&rows[..1], "/project").is_err(),
            "rolled-back journal cannot revive old evidence"
        );
        assert!(attempts.validate_receipts(&rows, "/other-checkout").is_err());
        assert!(attempts.invalidates("checks/test", Some("old-pass"), &rows));
        assert!(!attempts.invalidates("checks/test", Some("new-result"), &rows));
        let mut foreign = rows.clone();
        foreign[1].session = "another-session".into();
        assert!(attempts.validate_receipts(&foreign, "/project").is_err());
        let reversed = [rows[1].clone(), rows[0].clone()];
        assert!(attempts.validate_receipts(&reversed, "/project").is_err());
    }

    #[test]
    fn configuration_never_saves_an_unreadable_oversized_replacement() {
        let home = tempfile::tempdir().unwrap();
        let mut old = Requirements::default();
        old.repositories.insert("repo".into(), vec!["checks/test".into()]);
        old.save(home.path()).unwrap();
        let mut oversized = Requirements::default();
        for repo in 0..1024 {
            oversized
                .repositories
                .insert(format!("repo-{repo}"), (0..8).map(|n| format!("{}/check-{n}", "a".repeat(490))).collect());
        }
        assert!(oversized.save(home.path()).is_err());
        assert_eq!(Requirements::load(home.path()).unwrap().routes("repo"), ["checks/test"]);
        std::fs::write(
            home.path().join(format!(".completion.json.{}.tmp", std::process::id())),
            "stale temporary file",
        )
        .unwrap();
        old.save(home.path()).unwrap();
    }

    #[test]
    fn expired_hook_deadline_reports_unknown_without_planning_or_execution() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::at(home.path());
        let mut config = Requirements::default();
        config.repositories.insert("repo".into(), vec!["checks/test".into()]);
        config.save(home.path()).unwrap();
        let report = inspect("repo", root.path(), &Index::default(), &paths, None, Some(Instant::now())).unwrap();
        assert!(report.configured && !report.ready);
        assert_eq!(report.checks[0].readiness.status, verification::Status::Unknown);
        assert!(report.checks[0].readiness.reason.contains("deadline"));
        assert!(!crate::stage::spool_path(home.path()).exists(), "readiness never creates an execution receipt");
    }
}
