//! Frozen, fixed-sample experiments over independently graded runs. No agents are executed here.
//! The schedule fixes the denominator; content-addressed objects bind the inputs and receipts.
//! Hashes establish integrity, not when a protocol was registered or whether a provider was honest.

use crate::paired::{self, Pair, Policy, Run, Study, StudyKind, Usage};
use adage_learn::{corpus::Audit, EvidenceIdentity};
use adage_tracks::{context::CtxNet, triggers::Triggers, Gauge, Index, Manifest, Scope, Track, TrackInfo};
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;
const MAX_TASKS: usize = 10_000;

/// An immutable object store. Each read is bounded and checksum-verified by the evaluator.
pub trait Objects {
    fn read(&self, sha256: &str) -> Result<Vec<u8>>;
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn bytes(objects: &dyn Objects, sha: &str) -> Result<Vec<u8>> {
    ensure!(valid_digest(sha), "invalid SHA-256 object id");
    let body = objects.read(sha).with_context(|| format!("missing experiment object {sha}"))?;
    ensure!(body.len() <= MAX_OBJECT_BYTES, "experiment object exceeds 64 MiB");
    ensure!(digest(&body) == sha, "experiment object checksum mismatch: {sha}");
    Ok(body)
}

fn object<T: DeserializeOwned>(objects: &dyn Objects, sha: &str) -> Result<T> {
    serde_json::from_slice(&bytes(objects, sha)?).with_context(|| format!("invalid experiment object {sha}"))
}

/// Canonical source records retain every production route field, including review state.
/// They are separate from the compiled index, which intentionally omits nascent candidates.
#[derive(Debug, Serialize)]
pub struct SourceTrack {
    pub manifest: Manifest,
    pub routes: Vec<adage_tracks::Route>,
}

#[derive(Debug, Serialize)]
pub struct Sources {
    pub schema: u32,
    pub tracks: Vec<SourceTrack>,
    pub index: Index,
}

/// Read source tracks for experiment lineage without the runtime's fail-open loading policy.
/// No review, compilation artifact or other file is written by this inspection.
pub fn sources(root: &Path) -> Result<Sources> {
    fn regular(path: &Path) -> Result<()> {
        ensure!(
            std::fs::symlink_metadata(path)?.file_type().is_file(),
            "experiment source is not a regular file: {}",
            path.display()
        );
        read_bounded(path)?;
        Ok(())
    }
    let mut pending = vec![root.to_path_buf()];
    let mut tracks = Vec::new();
    while let Some(dir) = pending.pop() {
        ensure!(
            std::fs::symlink_metadata(&dir)?.file_type().is_dir(),
            "experiment tracks path is not a directory: {}",
            dir.display()
        );
        let manifest = dir.join("track.toml");
        if manifest.try_exists()? {
            regular(&manifest)?;
            let routes = dir.join("routes");
            ensure!(
                std::fs::symlink_metadata(&routes)?.file_type().is_dir(),
                "experiment routes path is not a directory: {}",
                routes.display()
            );
            // Enumerate strictly before io::read: the service loader tolerates directory-entry
            // errors, while a lineage snapshot must not silently lose one candidate.
            for entry in std::fs::read_dir(&routes)? {
                let path = entry?.path();
                ensure!(
                    path.extension().is_some_and(|ext| ext == "yaml"),
                    "unexpected route source: {}",
                    path.display()
                );
                regular(&path)?;
            }
            let mut track = adage_tracks::io::read(&dir)
                .with_context(|| format!("invalid experiment source track {}", dir.display()))?;
            track.routes.sort_by(|a, b| a.id.cmp(&b.id));
            ensure!(
                track.routes.windows(2).all(|pair| pair[0].id != pair[1].id),
                "duplicate route id in experiment source track {}",
                dir.display()
            );
            let exec = dir.join(adage_tracks::index::EXEC_FILE);
            if exec.try_exists()? {
                regular(&exec)?;
                serde_json::from_slice::<BTreeMap<String, adage_tracks::index::ExecBinding>>(&read_bounded(&exec)?)
                    .with_context(|| format!("invalid experiment exec bindings {}", exec.display()))?;
            }
            tracks.push(track);
        } else {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                ensure!(
                    entry.file_type()?.is_dir(),
                    "unexpected file outside a source track: {}",
                    entry.path().display()
                );
                ensure!(entry.file_name() != "routes", "routes directory has no track.toml: {}", dir.display());
                pending.push(entry.path());
            }
        }
    }
    tracks.sort_by(|a, b| {
        (&a.manifest.scope.repo, &a.manifest.track.name).cmp(&(&b.manifest.scope.repo, &b.manifest.track.name))
    });
    ensure!(
        tracks.windows(2).all(|pair| (&pair[0].manifest.scope.repo, &pair[0].manifest.track.name)
            != (&pair[1].manifest.scope.repo, &pair[1].manifest.track.name)),
        "duplicate repository/track identity in experiment sources"
    );
    let index = Index::compile_root(root, &tracks).0;
    let tracks =
        tracks.into_iter().map(|track| SourceTrack { manifest: track.manifest, routes: track.routes }).collect();
    Ok(Sources { schema: 1, tracks, index })
}

#[derive(Debug, Serialize)]
pub struct Artifact {
    pub sha256: String,
    pub generation: String,
    pub repo: String,
    pub reflexes: usize,
    /// Reflexes the ordinary directive hook can deliver, without an execution extension.
    pub directive_reflexes: usize,
    pub runtime_repo: Option<String>,
}

/// Inspect the actual runtime format before an experiment can spend on an inert intervention.
/// This establishes a loadable, internally consistent artifact, not learned benefit or coverage.
pub fn artifact(body: &[u8], repo: &str, checkout: Option<&Path>) -> Result<Artifact> {
    ensure!(body.len() <= MAX_OBJECT_BYTES, "reflex set exceeds 64 MiB");
    let index: Index = serde_json::from_slice(body).context("reflex set is not a compiled Adage index")?;
    ensure!(
        index.generation == digest(&serde_json::to_vec(&index.routes)?)[..16],
        "reflex set generation is stale; recompile with the pinned Adage build"
    );
    let mut ids = BTreeSet::new();
    for entry in &index.routes {
        ensure!(ids.insert((&entry.repo, &entry.route.id)), "duplicate reflex id in compiled index");
        ensure!(entry.route.state.is_live(), "compiled index contains an unapproved or disabled reflex");
        // Use the same validation boundary as learning/import, rather than maintaining a second
        // set of rules for slot binding, command effects and directive ceilings.
        adage_tracks::io::validate(&Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: entry.track.clone(),
                    version: entry.version.clone(),
                    description: String::new(),
                    provenance: String::new(),
                },
                scope: Scope { repo: entry.repo.clone() },
                gauge: Gauge::default(),
            },
            routes: vec![entry.route.clone()],
        })
        .context("invalid compiled reflex")?;
    }
    let keys: BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    for key in &keys {
        let routes = || index.for_repo(key).map(|r| &r.route);
        ensure!(
            *index.triggers(key) == Triggers::of(routes()) && *index.contexts(key) == CtxNet::of(routes()),
            "compiled trigger/context caches differ from the reflexes; recompile the index"
        );
    }
    let reflexes = index.for_repo(repo).count();
    ensure!(reflexes > 0, "reflex set has no enabled reflexes for repository {repo}; review and compile first");
    let runtime_repo = if let Some(checkout) = checkout {
        let root = adage_core::repo::find_root(checkout).context("experiment checkout is not a Git repository")?;
        let resolved = adage_core::repo::lookup_key(&root, None, &keys);
        ensure!(
            resolved.as_deref() == Some(repo),
            "checkout does not resolve to reflex repository {repo}; check its origin and training repository key"
        );
        resolved
    } else {
        None
    };
    Ok(Artifact {
        sha256: digest(body),
        generation: index.generation.clone(),
        repo: repo.to_owned(),
        reflexes,
        directive_reflexes: index.for_repo(repo).filter(|r| !r.exec_only()).count(),
        runtime_repo,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Whole task workflow, including retries, planning and intervention overhead.
    pub max_model_calls: u64,
    pub max_cost_microusd: u64,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arm {
    pub model: String,
    /// JSON settings: sampling, harness switches and all other treatment settings.
    pub configuration_sha256: String,
    /// Exact compiled reflex-set bytes. None means no reflexes delivered.
    pub reflex_set_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub identity: EvidenceIdentity,
    pub task_revision: String,
    /// Pins the checkout, harness, tools and input context common to both arms.
    pub environment_revision: String,
    pub evaluator_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    pub schema: u32,
    pub experiment: String,
    pub repo: String,
    pub seed: u64,
    pub kind: StudyKind,
    /// Exact output of `corpus inspect --evidence`, stored alongside the other objects.
    pub training_audit_sha256: String,
    pub learner_revision: String,
    pub policy: Policy,
    pub budget: Budget,
    pub baseline: Arm,
    pub intervention: Arm,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Baseline,
    Intervention,
}

impl Protocol {
    pub fn arm(&self, side: Side) -> &Arm {
        match side {
            Side::Baseline => &self.baseline,
            Side::Intervention => &self.intervention,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: String,
    pub group: String,
    pub side: Side,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frozen {
    pub protocol_sha256: String,
    pub protocol: Protocol,
    /// Paired blocks, with seeded task order and balanced first-arm order. One run per arm/task.
    pub jobs: Vec<Job>,
    pub max_cost_microusd: u128,
}

fn group(repo: &str, task: &Task) -> String {
    task.identity.group(repo, "").map_or_else(String::new, |g| g.key)
}

fn training(p: &Protocol, objects: &dyn Objects) -> Result<Audit> {
    let audit: Audit = object(objects, &p.training_audit_sha256)?;
    ensure!(audit.repo == p.repo && valid_digest(&audit.training_generation), "training snapshot mismatch");
    ensure!(
        audit.sessions > 0
            && audit.training_declared_identity == audit.sessions
            && audit.training_inferred_identity == 0
            && audit.training_unknown_identity == 0
            && !audit.training_groups.is_empty()
            && audit.training_groups.len() <= audit.sessions
            && audit.training_groups.iter().all(|g| g.strip_prefix("group-v1:").is_some_and(valid_digest)),
        "training audit must contain declared task identities for every session"
    );
    ensure!(
        audit.recorded_outcomes.checked_add(audit.proxy_outcomes).and_then(|n| n.checked_add(audit.unknown_outcomes))
            == Some(audit.sessions),
        "inconsistent training outcome counts"
    );
    Ok(audit)
}

/// Freeze before collecting any outcomes. Publish the digest outside the run directory before
/// executing the jobs; a local content hash alone cannot prove prospective registration.
pub fn freeze(mut protocol: Protocol, objects: &dyn Objects) -> Result<Frozen> {
    let p = &mut protocol;
    ensure!(p.schema == 1, "unsupported experiment protocol schema");
    ensure!(
        [&p.experiment, &p.repo, &p.learner_revision].iter().all(|s| !s.trim().is_empty()),
        "missing experiment provenance"
    );
    paired::validate_policy(&p.policy)?;
    ensure!((p.policy.min_groups..=MAX_TASKS).contains(&p.tasks.len()), "task count outside fixed-sample limits");
    ensure!(
        p.budget.max_model_calls > 0 && p.budget.max_cost_microusd > 0 && p.budget.timeout_seconds > 0,
        "experiment budgets must be positive"
    );
    ensure!(p.baseline.reflex_set_sha256.is_none(), "baseline must have reflexes disabled");
    ensure!(p.intervention.reflex_set_sha256.is_some(), "intervention must pin a reflex set");
    for arm in [&p.baseline, &p.intervention] {
        ensure!(!arm.model.trim().is_empty(), "missing model revision");
        bytes(objects, &arm.configuration_sha256)?;
        if let Some(sha) = &arm.reflex_set_sha256 {
            artifact(&bytes(objects, sha)?, &p.repo, None)?;
        }
    }
    ensure!(
        (p.kind == StudyKind::SameModel) == (p.baseline.model == p.intervention.model),
        "models do not match study kind"
    );
    let audit = training(p, objects)?;
    let mut groups = BTreeSet::new();
    for task in &p.tasks {
        let key = group(&p.repo, task);
        ensure!(!key.is_empty(), "unknown evaluation task identity");
        ensure!(groups.insert(key.clone()), "repeated evaluation task group");
        ensure!(!audit.training_groups.contains(&key), "training/evaluation task overlap");
        ensure!(task.identity.attempt_id.is_none(), "attempt ids do not define evaluation tasks");
        ensure!(
            [&task.task_revision, &task.environment_revision, &task.evaluator_revision]
                .iter()
                .all(|s| !s.trim().is_empty()),
            "missing task, environment or evaluator revision"
        );
    }
    p.tasks.sort_by_key(|t| group(&p.repo, t));
    let protocol_sha256 = digest(&serde_json::to_vec(&protocol)?);
    let mut ordered: Vec<_> = groups.into_iter().collect();
    ordered.sort_by_cached_key(|g| digest(format!("adage-experiment-order-v1:{}:{g}", protocol.seed).as_bytes()));
    let mut jobs = Vec::with_capacity(ordered.len() * 2);
    for (i, group) in ordered.into_iter().enumerate() {
        let sides = if (i as u64 + protocol.seed % 2).is_multiple_of(2) {
            [Side::Baseline, Side::Intervention]
        } else {
            [Side::Intervention, Side::Baseline]
        };
        for side in sides {
            let id = digest(&serde_json::to_vec(&("adage-experiment-job-v1", &protocol_sha256, &group, side))?);
            jobs.push(Job { id, group: group.clone(), side });
        }
    }
    let max_cost_microusd = jobs.len() as u128 * u128::from(protocol.budget.max_cost_microusd);
    Ok(Frozen { protocol_sha256, protocol, jobs, max_cost_microusd })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observed {
    pub model: String,
    pub configuration_sha256: String,
    pub reflex_set_sha256: Option<String>,
    pub task_revision: String,
    pub environment_revision: String,
    pub evaluator_revision: String,
    pub budget: Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Completed,
    Failed,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCall {
    /// Provider-scoped request id. Shared/replayed receipts cannot pay for multiple jobs.
    pub id: String,
    pub model: String,
    /// A single request, including any cached tokens. Missing accounting remains unknown.
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u32,
    pub protocol_sha256: String,
    pub job_id: String,
    pub run_id: String,
    pub observed: Observed,
    pub status: Status,
    /// Independent task grade, never inferred from token usage or a passing trace check.
    pub passed: Option<bool>,
    /// Runner assertion that the complete workflow's calls (including retries) are present.
    pub usage_complete: bool,
    pub calls: Vec<ModelCall>,
    /// Optional immutable source objects (native accounting, independent grade, execution
    /// observations). Their integrity is checked; their authenticity remains the runner's job.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub evidence: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub schema: u32,
    pub protocol_sha256: String,
    pub receipts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub schema: u32,
    pub protocol_sha256: String,
    pub submission_sha256: String,
    pub scheduled_runs: usize,
    pub submitted_receipts: usize,
    pub missing_jobs: BTreeSet<String>,
    pub failed_runs: usize,
    pub reasons: BTreeSet<String>,
    pub evidence_complete: bool,
    pub paired: paired::Report,
}

fn usage(receipt: &Receipt, calls: &mut BTreeSet<String>, reasons: &mut BTreeSet<String>) -> Result<Option<Usage>> {
    let mut total = Usage {
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        model_calls: receipt.calls.len() as u64,
        cost_microusd: Some(0),
    };
    let mut complete = receipt.usage_complete;
    let mut known_cost = 0u64;
    for call in &receipt.calls {
        if call.id.trim().is_empty() || !calls.insert(call.id.clone()) {
            reasons.insert("missing_or_reused_provider_request".into());
        }
        if call.model != receipt.observed.model {
            reasons.insert("unplanned_model_call".into());
        }
        let Some(u) = &call.usage else {
            complete = false;
            continue;
        };
        if u.model_calls != 1 || u.cached_input_tokens > u.input_tokens {
            reasons.insert("invalid_request_usage".into());
        }
        let add = |a: u64, b: u64| a.checked_add(b).context("workflow usage exceeds u64 accounting limit");
        total.input_tokens = add(total.input_tokens, u.input_tokens)?;
        total.output_tokens = add(total.output_tokens, u.output_tokens)?;
        total.cached_input_tokens = add(total.cached_input_tokens, u.cached_input_tokens)?;
        if let Some(cost) = u.cost_microusd {
            known_cost = add(known_cost, cost)?;
        }
        total.cost_microusd = match (total.cost_microusd, u.cost_microusd) {
            (Some(a), Some(b)) => Some(add(a, b)?),
            _ => None,
        };
    }
    if total.model_calls > receipt.observed.budget.max_model_calls
        || known_cost > receipt.observed.budget.max_cost_microusd
    {
        reasons.insert("workflow_budget_exceeded".into());
    }
    Ok(complete.then_some(total))
}

fn run(p: &Protocol, task: &Task, side: Side, job: &Job) -> Run {
    Run {
        id: job.id.clone(),
        model: p.arm(side).model.clone(),
        task_revision: task.task_revision.clone(),
        environment_revision: task.environment_revision.clone(),
        evaluator_revision: task.evaluator_revision.clone(),
        passed: None,
        usage: None,
    }
}

/// Verify the immutable protocol, scheduled denominator and referenced training/artifact objects.
pub fn validate(frozen: &Frozen, objects: &dyn Objects) -> Result<()> {
    let expected = freeze(frozen.protocol.clone(), objects)?;
    ensure!(
        expected.protocol_sha256 == frozen.protocol_sha256
            && expected.jobs == frozen.jobs
            && expected.max_cost_microusd == frozen.max_cost_microusd,
        "frozen protocol or schedule was changed"
    );
    Ok(())
}

/// Count every scheduled task. Missing jobs become unknown outcomes/usage; no complete-case
/// filtering or best-of-retry selection is permitted. Protocol violations invalidate all bounds.
pub fn evaluate(frozen: &Frozen, submitted: &Submission, objects: &dyn Objects) -> Result<Report> {
    validate(frozen, objects)?;
    ensure!(
        submitted.schema == 1 && submitted.protocol_sha256 == frozen.protocol_sha256,
        "submission protocol mismatch"
    );
    // Bound work even for a submission containing a flood of duplicate/foreign receipts.
    ensure!(submitted.receipts.len() <= frozen.jobs.len() * 2, "too many receipts for the frozen schedule");
    let p = &frozen.protocol;
    let audit = training(p, objects)?;
    let tasks: BTreeMap<_, _> = p.tasks.iter().map(|t| (group(&p.repo, t), t)).collect();
    let jobs: BTreeMap<_, _> = frozen.jobs.iter().map(|j| (j.id.clone(), j)).collect();
    let mut receipts = submitted.receipts.clone();
    receipts.sort();
    let submission_sha256 = digest(&serde_json::to_vec(&(submitted.schema, &submitted.protocol_sha256, &receipts))?);
    let (mut reasons, mut received, mut run_ids, mut call_ids) =
        (BTreeSet::new(), BTreeMap::new(), BTreeSet::new(), BTreeSet::new());
    let mut failed_runs = 0;
    for sha in receipts {
        let r: Receipt = object(objects, &sha)?;
        ensure!(r.schema == 1, "unsupported experiment receipt schema");
        ensure!(r.evidence.len() <= 16, "too many source objects in one receipt");
        for source in r.evidence.values() {
            bytes(objects, source)?;
        }
        if r.protocol_sha256 != frozen.protocol_sha256 {
            reasons.insert("receipt_protocol_mismatch".into());
        }
        if r.run_id.trim().is_empty() || !run_ids.insert(r.run_id.clone()) {
            reasons.insert("missing_or_reused_run_id".into());
        }
        let accounting = usage(&r, &mut call_ids, &mut reasons)?;
        let Some(job) = jobs.get(&r.job_id) else {
            reasons.insert("unscheduled_job".into());
            continue;
        };
        let task = tasks[&job.group];
        let arm = p.arm(job.side);
        let o = &r.observed;
        if o.model != arm.model
            || o.configuration_sha256 != arm.configuration_sha256
            || o.reflex_set_sha256 != arm.reflex_set_sha256
            || o.task_revision != task.task_revision
            || o.environment_revision != task.environment_revision
            || o.evaluator_revision != task.evaluator_revision
            || o.budget != p.budget
        {
            reasons.insert("observed_conditions_mismatch".into());
        }
        let passed = if r.status == Status::Completed {
            r.passed
        } else {
            failed_runs += 1;
            if r.passed == Some(true) {
                reasons.insert("failed_run_claimed_success".into());
            }
            Some(false)
        };
        let mut result = run(p, task, job.side, job);
        result.id = r.run_id;
        result.passed = passed;
        result.usage = accounting;
        if received.insert(job.id.clone(), result).is_some() {
            reasons.insert("repeated_scheduled_job".into());
        }
    }
    let missing_jobs: BTreeSet<_> = jobs.keys().filter(|id| !received.contains_key(*id)).cloned().collect();
    if !missing_jobs.is_empty() {
        reasons.insert("missing_scheduled_jobs".into());
    }
    let by_group: BTreeMap<_, _> = frozen.jobs.iter().map(|j| ((j.group.clone(), j.side), j)).collect();
    let mut pairs = Vec::with_capacity(tasks.len());
    for (group, task) in tasks {
        let mut take = |side| {
            let job = by_group[&(group.clone(), side)];
            received.remove(&job.id).unwrap_or_else(|| run(p, task, side, job))
        };
        pairs.push(Pair {
            repo: p.repo.clone(),
            identity: task.identity.clone(),
            baseline: take(Side::Baseline),
            intervention: take(Side::Intervention),
        });
    }
    let study = Study {
        schema: 1,
        experiment: p.experiment.clone(),
        kind: p.kind,
        training_generation: audit.training_generation,
        training_groups: audit.training_groups,
        training_inferred_identity: audit.training_inferred_identity,
        training_unknown_identity: audit.training_unknown_identity,
        policy: p.policy.clone(),
        pairs,
    };
    let paired = paired::evaluate_with_reasons(&study, reasons.clone())?;
    let evidence_complete = reasons.is_empty() && paired.unknown_outcomes == 0 && paired.missing_usage == 0;
    Ok(Report {
        schema: 1,
        protocol_sha256: frozen.protocol_sha256.clone(),
        submission_sha256,
        scheduled_runs: frozen.jobs.len(),
        submitted_receipts: submitted.receipts.len(),
        missing_jobs,
        failed_runs,
        reasons,
        evidence_complete,
        paired,
    })
}

/// `<sha256>.json` objects; digest validation prevents the input from choosing arbitrary paths.
pub struct Directory(pub PathBuf);

impl Directory {
    /// Publish exact JSON bytes once, privately and without replacing an existing object.
    pub fn put(&self, body: &[u8]) -> Result<String> {
        use std::io::Write;
        ensure!(body.len() <= MAX_OBJECT_BYTES, "experiment object exceeds 64 MiB");
        let _: serde_json::Value = serde_json::from_slice(body).context("experiment objects must be JSON")?;
        let sha = digest(body);
        std::fs::create_dir_all(&self.0)?;
        let path = self.0.join(format!("{sha}.json"));
        let mut temp = tempfile::NamedTempFile::new_in(&self.0)?;
        temp.write_all(body)?;
        temp.as_file().sync_all()?;
        match temp.persist_noclobber(path) {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                bytes(self, &sha)?;
            }
            Err(e) => return Err(e.error.into()),
        }
        Ok(sha)
    }
}

impl Objects for Directory {
    fn read(&self, sha256: &str) -> Result<Vec<u8>> {
        ensure!(valid_digest(sha256), "invalid SHA-256 object id");
        read_bounded(self.0.join(format!("{sha256}.json")))
    }
}

fn read_bounded(path: impl AsRef<std::path::Path>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(MAX_OBJECT_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_OBJECT_BYTES, "experiment input exceeds 64 MiB");
    Ok(bytes)
}

pub fn cli(args: &[String]) -> Result<()> {
    const USAGE: &str = "usage: adage eval experiment sources <tracks-dir>\n       adage eval experiment object <input.json> --objects <dir>\n       adage eval experiment artifact <index.json> --repo <repo> [--checkout <dir>]\n       adage eval experiment freeze <protocol.json> --objects <dir>\n       adage eval experiment validate <frozen.json> --objects <dir>\n       adage eval experiment report <frozen.json> <submission.json> --objects <dir> [--require-token-target | --require-cost-target]";
    let required = args.last().map(String::as_str).filter(|s| s.starts_with("--require-"));
    match args.first().map(String::as_str) {
        Some("sources") if args.len() == 2 => {
            println!("{}", serde_json::to_string_pretty(&sources(Path::new(&args[1]))?)?);
        }
        Some("artifact") if matches!(args.len(), 4 | 6) && args[2] == "--repo" => {
            ensure!(args.len() == 4 || args[4] == "--checkout", USAGE);
            let checkout = args.get(5).map(Path::new);
            println!("{}", serde_json::to_string_pretty(&artifact(&read_bounded(&args[1])?, &args[3], checkout)?)?);
        }
        Some("object") if args.len() == 4 && args[2] == "--objects" => {
            let sha = Directory(PathBuf::from(&args[3])).put(&read_bounded(&args[1])?)?;
            println!("{}", serde_json::json!({"sha256": sha}));
        }
        Some("--help" | "-h") if args.len() == 1 => println!("{USAGE}"),
        Some("freeze") if args.len() == 4 && args[2] == "--objects" => {
            let protocol = serde_json::from_slice(&read_bounded(&args[1])?)?;
            let frozen = freeze(protocol, &Directory(PathBuf::from(&args[3])))?;
            println!("{}", serde_json::to_string_pretty(&frozen)?);
        }
        Some("validate") if args.len() == 4 && args[2] == "--objects" => {
            let frozen = serde_json::from_slice(&read_bounded(&args[1])?)?;
            validate(&frozen, &Directory(PathBuf::from(&args[3])))?;
            println!("{}", serde_json::to_string_pretty(&frozen)?);
        }
        Some("report") if (args.len() == 5 || args.len() == 6) && args[3] == "--objects" => {
            ensure!(
                args.len() == 5 || matches!(required, Some("--require-token-target" | "--require-cost-target")),
                USAGE
            );
            let frozen = serde_json::from_slice(&read_bounded(&args[1])?)?;
            let submitted = serde_json::from_slice(&read_bounded(&args[2])?)?;
            let report = evaluate(&frozen, &submitted, &Directory(PathBuf::from(&args[4])))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            match required {
                Some("--require-token-target") => {
                    ensure!(report.paired.token_target_accepted, "token target is not qualified by this experiment")
                }
                Some("--require-cost-target") => {
                    ensure!(report.paired.cost_target_accepted, "cost target is not qualified by this experiment")
                }
                _ => {}
            }
        }
        _ => anyhow::bail!(USAGE),
    }
    Ok(())
}
