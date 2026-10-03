use adage_eval::experiment::{Directory, Frozen, Receipt, Submission};
use adage_learn::EvidenceIdentity;
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_adage"))
        .env("ADAGE_HOME", home.join("home"))
        .env("ADAGE_COLOR", "never")
        .env("ADAGE_EMBED_MODEL", "none")
        .args(["eval", "experiment"])
        .args(args)
        .output()
        .unwrap()
}

fn ok(output: Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn freeze_and_report_require_every_scheduled_run_and_preserve_the_registered_protocol() {
    let d = tempfile::tempdir().unwrap();
    let objects = Directory(d.path().join("objects"));
    let object_dir = objects.0.to_str().unwrap();
    let input = d.path().join("configuration.json");
    std::fs::write(&input, br#"{"temperature":0,"autolearn":false}"#).unwrap();
    let output = ok(run(d.path(), &["object", input.to_str().unwrap(), "--objects", object_dir]));
    let config = output["sha256"].as_str().unwrap();
    let reflexes = objects.put(br#"{"synthetic_fixture":true,"reflexes":["test"]}"#).unwrap();
    let group = EvidenceIdentity { task_id: Some("TRAIN-0".into()), ..Default::default() }
        .group("fixture/repo", "")
        .unwrap()
        .key;
    let audit = json!({
        "repo":"fixture/repo", "training_generation":"0".repeat(64), "sessions":1,
        "training_groups":[group], "training_declared_identity":1,
        "training_inferred_identity":0, "training_unknown_identity":0,
        "recorded_outcomes":1, "proxy_outcomes":0, "unknown_outcomes":0
    });
    let audit = objects.put(&serde_json::to_vec(&audit).unwrap()).unwrap();
    // A deliberately relaxed synthetic policy exercises successful/nonzero CLI exits in two tasks.
    // Production quality noninferiority is covered with 200 tasks in adage-eval's fixtures.
    let protocol = json!({
        "schema":1, "experiment":"cli-fixture", "repo":"fixture/repo", "seed":7,
        "kind":"same_model", "training_audit_sha256":audit, "learner_revision":"fixture-v1",
        "policy":{"min_groups":2,"max_quality_loss":1.0,"min_savings":0.20,"min_baseline_success":0.8},
        "budget":{"max_model_calls":4,"max_cost_microusd":10000,"timeout_seconds":60},
        "baseline":{"model":"fixture-v1","configuration_sha256":config,"reflex_set_sha256":null},
        "intervention":{"model":"fixture-v1","configuration_sha256":config,"reflex_set_sha256":reflexes},
        "tasks":[
            {"identity":{"task_id":"TEST-0"},"task_revision":"task-v1","environment_revision":"env-v1","evaluator_revision":"grader-v1"},
            {"identity":{"task_id":"TEST-1"},"task_revision":"task-v1","environment_revision":"env-v1","evaluator_revision":"grader-v1"}
        ]
    });
    let protocol_path = d.path().join("protocol.json");
    std::fs::write(&protocol_path, serde_json::to_vec(&protocol).unwrap()).unwrap();
    let frozen = ok(run(d.path(), &["freeze", protocol_path.to_str().unwrap(), "--objects", object_dir]));
    assert_eq!(frozen["jobs"].as_array().unwrap().len(), 4);
    assert_eq!(frozen["max_cost_microusd"], 40000);
    let frozen_path = d.path().join("frozen.json");
    std::fs::write(&frozen_path, serde_json::to_vec(&frozen).unwrap()).unwrap();
    let f: Frozen = serde_json::from_value(frozen).unwrap();
    let receipts = f
        .jobs
        .iter()
        .map(|job| {
            let arm = f.protocol.arm(job.side);
            let r: Receipt = serde_json::from_value(json!({
                "schema":1, "protocol_sha256":f.protocol_sha256, "job_id":job.id, "run_id":format!("run-{}",job.id),
                "observed":{
                    "model":arm.model, "configuration_sha256":arm.configuration_sha256,
                    "reflex_set_sha256":arm.reflex_set_sha256, "task_revision":"task-v1",
                    "environment_revision":"env-v1", "evaluator_revision":"grader-v1", "budget":f.protocol.budget
                },
                "status":"completed", "passed":true, "usage_complete":true,
                "calls":[{"id":format!("provider/request-{}",job.id), "model":arm.model,
                    "usage":{"input_tokens":if arm.reflex_set_sha256.is_some() {750} else {1000},
                    "output_tokens":0,"cached_input_tokens":0,"model_calls":1,"cost_microusd":null}}]
            }))
            .unwrap();
            objects.put(&serde_json::to_vec(&r).unwrap()).unwrap()
        })
        .collect();
    let mut submitted = Submission { schema: 1, protocol_sha256: f.protocol_sha256.clone(), receipts };
    let submission_path = d.path().join("submission.json");
    std::fs::write(&submission_path, serde_json::to_vec(&submitted).unwrap()).unwrap();
    let args = [
        "report",
        frozen_path.to_str().unwrap(),
        submission_path.to_str().unwrap(),
        "--objects",
        object_dir,
        "--require-token-target",
    ];
    let report = ok(run(d.path(), &args));
    assert_eq!(report["evidence_complete"], true);
    assert_eq!(report["paired"]["token_target_accepted"], true);
    assert_eq!(report["paired"]["cost_target_accepted"], false);
    submitted.receipts.pop();
    std::fs::write(&submission_path, serde_json::to_vec(&submitted).unwrap()).unwrap();
    let output = run(d.path(), &args);
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["paired"]["pairs"], 2);
    assert_eq!(report["missing_jobs"].as_array().unwrap().len(), 1);
    assert_eq!(report["paired"]["token_target_accepted"], false);
    let mut edited = f;
    edited.protocol.policy.min_savings = 0.0;
    std::fs::write(&frozen_path, serde_json::to_vec(&edited).unwrap()).unwrap();
    let output = run(d.path(), &args);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("was changed"));
}
