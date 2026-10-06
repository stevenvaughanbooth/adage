//! Synthetic protocol/accounting fixtures. These do not demonstrate savings in real workloads.
use adage_eval::experiment::{self, Frozen, Objects, Protocol, Receipt, Side, Status, Submission};
use adage_learn::EvidenceIdentity;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct Memory(BTreeMap<String, Vec<u8>>);

impl Objects for Memory {
    fn read(&self, sha: &str) -> Result<Vec<u8>> {
        self.0.get(sha).cloned().context("absent fixture object")
    }
}

impl Memory {
    fn put(&mut self, value: &impl Serialize) -> String {
        let body = serde_json::to_vec(value).unwrap();
        let sha = experiment::digest(&body);
        self.0.insert(sha.clone(), body);
        sha
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/experiment.json")).unwrap()
}

fn protocol(n: usize) -> (Protocol, Memory) {
    let fixture = fixture();
    let mut objects = Memory::default();
    let mut p: Protocol = serde_json::from_value(fixture["protocol"].clone()).unwrap();
    let mut training = fixture["training"].clone();
    let identity = EvidenceIdentity { task_id: Some("TRAIN-0".into()), ..Default::default() };
    training["training_groups"] = json!([identity.group(&p.repo, "").unwrap().key]);
    p.training_audit_sha256 = objects.put(&training);
    p.baseline.configuration_sha256 = objects.put(&fixture["configuration"]);
    p.intervention.configuration_sha256 = p.baseline.configuration_sha256.clone();
    p.intervention.reflex_set_sha256 = Some(objects.put(&fixture["reflex_set"]));
    let task = p.tasks[0].clone();
    p.tasks = (0..n)
        .map(|i| {
            let mut task = task.clone();
            task.identity.task_id = Some(format!("EVAL-{i}"));
            task
        })
        .collect();
    (p, objects)
}

fn complete(n: usize) -> (Frozen, Submission, Memory) {
    let (p, mut objects) = protocol(n);
    let frozen = experiment::freeze(p, &objects).unwrap();
    let template: Receipt = serde_json::from_value(fixture()["receipt"].clone()).unwrap();
    let receipts = frozen
        .jobs
        .iter()
        .map(|job| {
            let mut r = template.clone();
            r.protocol_sha256 = frozen.protocol_sha256.clone();
            r.job_id = job.id.clone();
            r.run_id = format!("run-{}", job.id);
            r.calls[0].id = format!("provider/request-{}", job.id);
            let arm = frozen.protocol.arm(job.side);
            r.observed.configuration_sha256 = arm.configuration_sha256.clone();
            r.observed.reflex_set_sha256 = arm.reflex_set_sha256.clone();
            if job.side == Side::Intervention {
                let u = r.calls[0].usage.as_mut().unwrap();
                u.input_tokens = 600;
                u.output_tokens = 150;
                u.cached_input_tokens = 300;
                u.cost_microusd = Some(1500);
            }
            objects.put(&r)
        })
        .collect();
    let submitted = Submission { schema: 1, protocol_sha256: frozen.protocol_sha256.clone(), receipts };
    (frozen, submitted, objects)
}

fn change(submitted: &mut Submission, objects: &mut Memory, index: usize, mutate: impl FnOnce(&mut Receipt)) {
    let mut r: Receipt = serde_json::from_slice(&objects.read(&submitted.receipts[index]).unwrap()).unwrap();
    mutate(&mut r);
    submitted.receipts[index] = objects.put(&r);
}

#[test]
fn frozen_schedule_and_report_are_order_independent_and_balance_arm_order() {
    let (f, mut s, o) = complete(200);
    assert_eq!(f.jobs.len(), 400);
    assert_eq!(f.max_cost_microusd, 4_000_000);
    assert_eq!(f.jobs.chunks(2).filter(|pair| pair[0].side == Side::Baseline).count(), 100);
    for pair in f.jobs.chunks(2) {
        assert_eq!(pair[0].group, pair[1].group);
        assert_ne!(pair[0].side, pair[1].side);
    }
    let first = experiment::evaluate(&f, &s, &o).unwrap();
    assert!(first.evidence_complete && first.paired.token_target_accepted && first.paired.cost_target_accepted);
    assert_eq!(first.paired.pairs, 200);
    let tokens = first.paired.tokens.as_ref().unwrap();
    assert_eq!((tokens.baseline, tokens.intervention, tokens.lower_95), (200_000, 150_000, Some(0.25)));
    let mut reordered = f.protocol.clone();
    reordered.tasks.reverse();
    assert_eq!(
        serde_json::to_value(experiment::freeze(reordered, &o).unwrap()).unwrap(),
        serde_json::to_value(&f).unwrap()
    );
    s.receipts.reverse();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(experiment::evaluate(&f, &s, &o).unwrap()).unwrap()
    );
    let mut reseeded = f.protocol.clone();
    reseeded.seed += 1;
    assert_ne!(experiment::freeze(reseeded, &o).unwrap().protocol_sha256, f.protocol_sha256);
}

#[test]
fn dropping_one_bad_run_never_turns_a_200_task_protocol_into_199_good_pairs() {
    let (f, mut s, o) = complete(200);
    let missing = f.jobs[0].id.clone();
    s.receipts.remove(0);
    let report = experiment::evaluate(&f, &s, &o).unwrap();
    assert_eq!((report.scheduled_runs, report.submitted_receipts, report.paired.pairs), (400, 399, 200));
    assert_eq!(report.missing_jobs, [missing].into());
    assert_eq!((report.paired.unknown_outcomes, report.paired.missing_usage), (1, 1));
    assert!(!report.evidence_complete && !report.paired.token_target_accepted);
    assert!(report.paired.quality_delta_lower_95.is_none());
}

#[test]
fn timeouts_count_as_failures_and_all_retry_calls_are_charged() {
    let (f, mut s, mut o) = complete(200);
    let intervention: Vec<_> =
        f.jobs.iter().enumerate().filter(|(_, j)| j.side == Side::Intervention).map(|(i, _)| i).collect();
    for &i in &intervention[..10] {
        change(&mut s, &mut o, i, |r| {
            r.status = Status::Timeout;
            r.passed = None;
            let mut retry = r.calls[0].clone();
            retry.id.push_str("-retry");
            r.calls.push(retry);
        });
    }
    let report = experiment::evaluate(&f, &s, &o).unwrap();
    assert!(report.evidence_complete);
    assert_eq!((report.failed_runs, report.paired.regressions, report.paired.unknown_outcomes), (10, 10, 0));
    assert_eq!(report.paired.tokens.as_ref().unwrap().intervention, 157_500);
    assert_eq!(report.paired.model_calls.as_ref().unwrap().intervention, 210);
    assert!(!report.paired.quality_accepted && !report.paired.token_target_accepted);
}

#[test]
fn receipt_conflicts_and_unplanned_conditions_invalidate_every_confidence_bound() {
    for reason in [
        "receipt_protocol_mismatch",
        "missing_or_reused_run_id",
        "unscheduled_job",
        "repeated_scheduled_job",
        "observed_conditions_mismatch",
        "failed_run_claimed_success",
        "unplanned_model_call",
        "missing_or_reused_provider_request",
        "invalid_request_usage",
        "workflow_budget_exceeded",
    ] {
        let (f, mut s, mut o) = complete(2);
        let first: Receipt = serde_json::from_slice(&o.read(&s.receipts[0]).unwrap()).unwrap();
        change(&mut s, &mut o, 1, |r| match reason {
            "receipt_protocol_mismatch" => r.protocol_sha256 = "different-protocol".into(),
            "missing_or_reused_run_id" => r.run_id = first.run_id.clone(),
            "unscheduled_job" => r.job_id = "unscheduled".into(),
            "repeated_scheduled_job" => r.job_id = first.job_id.clone(),
            "observed_conditions_mismatch" => r.observed.reflex_set_sha256 = Some("other-reflex-set".into()),
            "failed_run_claimed_success" => r.status = Status::Failed,
            "unplanned_model_call" => r.calls[0].model = "unplanned-fallback".into(),
            "missing_or_reused_provider_request" => r.calls[0].id = first.calls[0].id.clone(),
            "invalid_request_usage" => r.calls[0].usage.as_mut().unwrap().model_calls = 2,
            _ => r.calls[0].usage.as_mut().unwrap().cost_microusd = Some(10001),
        });
        let report = experiment::evaluate(&f, &s, &o).unwrap();
        assert!(report.reasons.contains(reason), "{reason}: {:?}", report.reasons);
        assert!(!report.evidence_complete && !report.paired.quality_accepted);
        assert!(report.paired.quality_delta_lower_95.is_none());
        for metric in
            [&report.paired.tokens, &report.paired.cost_microusd, &report.paired.model_calls].into_iter().flatten()
        {
            assert!(metric.lower_95.is_none(), "{reason}");
        }
    }
}

#[test]
fn unknown_accounting_is_not_a_free_call_and_missing_cost_stays_separate() {
    for unknown in ["workflow", "request", "cost"] {
        let (f, mut s, mut o) = complete(200);
        change(&mut s, &mut o, 0, |r| match unknown {
            "workflow" => r.usage_complete = false,
            "request" => r.calls[0].usage = None,
            _ => r.calls[0].usage.as_mut().unwrap().cost_microusd = None,
        });
        let r = experiment::evaluate(&f, &s, &o).unwrap();
        assert!(!r.paired.cost_target_accepted && r.paired.cost_microusd.is_none());
        if unknown == "cost" {
            assert!(r.paired.token_target_accepted && r.evidence_complete);
        } else {
            assert!(!r.paired.token_target_accepted && !r.evidence_complete);
            assert!(r.paired.tokens.is_none());
        }
    }
}

#[test]
fn protocol_rejects_leakage_retries_unknown_identities_and_unusable_audits_before_execution() {
    for problem in ["overlap", "repeat", "unknown", "attempt", "audit", "few", "kind", "no-reflex", "budget"] {
        let (mut p, mut o) = protocol(2);
        match problem {
            "overlap" => p.tasks[0].identity.task_id = Some("TRAIN-0".into()),
            "repeat" => p.tasks[1].identity = p.tasks[0].identity.clone(),
            "unknown" => p.tasks[0].identity = Default::default(),
            "attempt" => p.tasks[0].identity.attempt_id = Some("retry-1".into()),
            "audit" => {
                let mut audit: Value = serde_json::from_slice(&o.read(&p.training_audit_sha256).unwrap()).unwrap();
                audit["training_inferred_identity"] = json!(1);
                p.training_audit_sha256 = o.put(&audit);
            }
            "few" => p.tasks.truncate(1),
            "kind" => p.intervention.model = "different-model".into(),
            "no-reflex" => p.intervention.reflex_set_sha256 = None,
            _ => p.budget.max_model_calls = 0,
        }
        assert!(experiment::freeze(p, &o).is_err(), "{problem}");
    }
}

#[test]
fn digests_bind_every_protocol_choice_schedule_artifact_and_receipt() {
    for problem in
        ["policy", "schedule", "budget-total", "task", "training", "configuration", "reflexes", "receipt", "path"]
    {
        let (mut f, s, mut o) = complete(2);
        match problem {
            "policy" => f.protocol.policy.max_quality_loss = 1.0,
            "schedule" => {
                f.jobs.pop();
            }
            "budget-total" => f.max_cost_microusd += 1,
            "task" => f.protocol.tasks[0].task_revision = "later-task".into(),
            "path" => f.protocol.training_audit_sha256 = "../escape".into(),
            _ => {
                let sha = match problem {
                    "training" => &f.protocol.training_audit_sha256,
                    "configuration" => &f.protocol.baseline.configuration_sha256,
                    "reflexes" => f.protocol.intervention.reflex_set_sha256.as_ref().unwrap(),
                    _ => &s.receipts[0],
                };
                o.0.insert(sha.clone(), b"{}".to_vec());
            }
        }
        assert!(experiment::evaluate(&f, &s, &o).is_err(), "{problem}");
    }
}

#[test]
fn repeated_delivery_is_visible_and_accounting_overflow_fails_closed() {
    let (f, mut s, mut o) = complete(2);
    s.receipts.push(s.receipts[0].clone());
    assert!(experiment::evaluate(&f, &s, &o).unwrap().reasons.contains("repeated_scheduled_job"));
    s.receipts.pop();
    change(&mut s, &mut o, 0, |r| {
        r.calls[0].usage.as_mut().unwrap().input_tokens = u64::MAX;
        let mut extra = r.calls[0].clone();
        extra.id.push_str("-extra");
        r.calls.push(extra);
    });
    assert!(experiment::evaluate(&f, &s, &o).unwrap_err().to_string().contains("accounting limit"));
}

#[test]
fn missing_cost_cannot_hide_a_known_budget_overrun() {
    let (f, mut s, mut o) = complete(2);
    change(&mut s, &mut o, 0, |r| {
        let mut retry = r.calls[0].clone();
        retry.id.push_str("-retry");
        retry.usage.as_mut().unwrap().cost_microusd = Some(10001);
        r.calls[0].usage.as_mut().unwrap().cost_microusd = None;
        r.calls.push(retry);
    });
    let report = experiment::evaluate(&f, &s, &o).unwrap();
    assert!(report.reasons.contains("workflow_budget_exceeded"));
    assert!(!report.evidence_complete && !report.paired.token_target_accepted);
    assert!(report.paired.cost_microusd.is_none());
}

#[test]
fn misspelled_protocol_and_policy_fields_are_rejected() {
    let mut value = fixture()["protocol"].clone();
    value["policy"]["max_quality_losses"] = json!(1.0);
    assert!(serde_json::from_value::<Protocol>(value).is_err());
    let mut value = fixture()["protocol"].clone();
    value["budgets"] = json!({});
    assert!(serde_json::from_value::<Protocol>(value).is_err());
}

#[test]
fn objects_are_private_idempotent_and_cannot_overwrite_corrupt_evidence() {
    let root = tempfile::tempdir().unwrap();
    let objects = experiment::Directory(root.path().join("objects"));
    let original = br#"{"independent_grade":false}"#;
    let sha = objects.put(original).unwrap();
    assert_eq!(objects.put(original).unwrap(), sha);
    assert_eq!(objects.read(&sha).unwrap(), original);
    let path = objects.0.join(format!("{sha}.json"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    std::fs::write(&path, b"{}").unwrap();
    assert!(objects.put(original).unwrap_err().to_string().contains("checksum mismatch"));
    assert_eq!(std::fs::read(&path).unwrap(), b"{}");
    assert!(objects.put(b"not json").is_err());
    assert!(objects.read("../escape").is_err());
}

#[test]
fn receipt_source_objects_must_exist_and_match_their_digests() {
    let (f, mut s, mut o) = complete(2);
    let sha = o.put(&serde_json::json!({"independent_grade": true}));
    change(&mut s, &mut o, 0, |r| {
        r.evidence.insert("grade".into(), sha.clone());
    });
    assert!(experiment::evaluate(&f, &s, &o).unwrap().evidence_complete);
    o.0.insert(sha.clone(), b"{}".to_vec());
    assert!(experiment::evaluate(&f, &s, &o).unwrap_err().to_string().contains("checksum mismatch"));
    o.0.remove(&sha);
    assert!(experiment::evaluate(&f, &s, &o).unwrap_err().to_string().contains("missing experiment object"));
}

#[test]
fn frozen_interventions_require_an_enabled_runtime_artifact_for_the_selected_repository() {
    let (mut protocol, mut objects) = protocol(2);
    protocol.intervention.reflex_set_sha256 = Some(objects.put(&json!({"reflexes": ["placeholder"]})));
    assert!(experiment::freeze(protocol.clone(), &objects).unwrap_err().to_string().contains("compiled Adage index"));
    protocol.intervention.reflex_set_sha256 = Some(objects.put(&adage_tracks::Index::compile(&[])));
    assert!(experiment::freeze(protocol, &objects).unwrap_err().to_string().contains("no enabled reflexes"));
    let body = serde_json::to_vec(&fixture()["reflex_set"]).unwrap();
    let report = experiment::artifact(&body, "acme/api", None).unwrap();
    assert_eq!((report.reflexes, report.directive_reflexes), (1, 1));
    assert!(report.runtime_repo.is_none());
    assert!(experiment::artifact(&body, "unrelated/repo", None)
        .unwrap_err()
        .to_string()
        .contains("no enabled reflexes"));
}

#[test]
fn artifact_reports_direct_and_context_embedding_requirements() {
    use adage_tracks::{CtxItem, Index, Trigger};
    let mut index: Index = serde_json::from_value(fixture()["reflex_set"].clone()).unwrap();
    let direct = Trigger::Intent {
        family: "fixture-intent".into(),
        model: "fixture@000000000000".into(),
        centroid: vec![5000, 5000, 5000, 5000, 0, 0, 0, 0],
        threshold: 0.8,
    };
    let context = Trigger::Context {
        items: vec![
            CtxItem::Intent {
                family: "fixture-intent".into(),
                model: "fixture@000000000000".into(),
                centroid: vec![5000, 5000, 5000, 5000, 0, 0, 0, 0],
                threshold: 0.8,
            },
            CtxItem::Edit { glob: "src/*.py".into() },
        ],
        within: 1,
    };
    for trigger in [direct, context] {
        index.routes[0].route.trigger = trigger;
        index.generation = experiment::digest(&serde_json::to_vec(&index.routes).unwrap())[..16].into();
        // Legacy indexes rebuild their derived caches when decoded.
        let mut value = serde_json::to_value(&index).unwrap();
        value.as_object_mut().unwrap().remove("triggers");
        value.as_object_mut().unwrap().remove("contexts");
        let report = experiment::artifact(&serde_json::to_vec(&value).unwrap(), "acme/api", None).unwrap();
        let requirements: Vec<_> = report.directive_embeddings.into_iter().collect();
        assert_eq!(
            requirements,
            vec![experiment::EmbeddingRequirement { model: "fixture@000000000000".into(), dimensions: 8 }]
        );
        assert_eq!(report.bundled_embedding.map(|m| m.model), adage_intent::model::bundled_id());
    }
}

#[test]
fn artifact_validation_rejects_stale_generation_and_suppressed_runtime_caches() {
    let index: adage_tracks::Index = serde_json::from_value(fixture()["reflex_set"].clone()).unwrap();
    let mut stale = index.clone();
    stale.routes[0].route.summary.push_str(" changed");
    assert!(experiment::artifact(&serde_json::to_vec(&stale).unwrap(), "acme/api", None)
        .unwrap_err()
        .to_string()
        .contains("generation is stale"));
    let mut suppressed = index.clone();
    suppressed.triggers.insert("acme/api".into(), Default::default());
    assert!(experiment::artifact(&serde_json::to_vec(&suppressed).unwrap(), "acme/api", None)
        .unwrap_err()
        .to_string()
        .contains("trigger/context caches"));
    // A legacy index without precomputed caches still gets the runtime's correct derived ones.
    let mut legacy = index;
    legacy.triggers.clear();
    experiment::artifact(&serde_json::to_vec(&legacy).unwrap(), "acme/api", None).unwrap();
}

#[test]
fn artifact_validation_preserves_review_and_shared_safety_boundaries() {
    let index: adage_tracks::Index = serde_json::from_value(fixture()["reflex_set"].clone()).unwrap();
    for (mut invalid, reason) in
        [(index.clone(), "unapproved"), (index.clone(), "duplicate"), (index, "invalid compiled reflex")]
    {
        match reason {
            "unapproved" => invalid.routes[0].route.state = adage_tracks::RouteState::Nascent,
            "duplicate" => invalid.routes.push(invalid.routes[0].clone()),
            _ => invalid.routes[0].route.step = adage_tracks::Step::Run { command: "make check; rm -rf .".into() },
        }
        invalid.attach_exec(&Default::default());
        assert!(experiment::artifact(&serde_json::to_vec(&invalid).unwrap(), "acme/api", None)
            .unwrap_err()
            .to_string()
            .contains(reason));
    }
}
