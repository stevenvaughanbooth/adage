use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output};

fn run(home: &Path, args: &[&str]) -> Output {
    run_with_model(home, args, "none")
}

fn run_with_model(home: &Path, args: &[&str], model: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_adage"))
        .env("ADAGE_HOME", home)
        .env("ADAGE_COLOR", "never")
        .env("ADAGE_EMBED_MODEL", model)
        .args(args)
        .output()
        .unwrap()
}

fn ok_json(output: Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn export_import_inspect_and_learn_share_the_canonical_contract() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let raw = d.path().join("trace.jsonl");
    let lines = [
        json!({"type":"user","sessionId":"s1","cwd":"/recorded/repo","message":{"role":"user","content":"Run the tests"}}),
        json!({"type":"assistant","sessionId":"s1","cwd":"/recorded/repo","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"make test"}}]}}),
        json!({"type":"user","sessionId":"s1","cwd":"/recorded/repo","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"1 passed","is_error":false}]}}),
    ];
    std::fs::write(&raw, lines.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
    let identity = d.path().join("identities.json");
    std::fs::write(&identity, br#"{"s1":{"task_id":"TASK-7","attempt_id":"1"}}"#).unwrap();
    let outcomes = d.path().join("outcomes.json");
    // An independent failure must override the passing check in the raw trace.
    std::fs::write(&outcomes, br#"{"s1":false}"#).unwrap();
    let output = run(
        &home,
        &[
            "corpus",
            "export",
            raw.to_str().unwrap(),
            "--repo",
            "acme/repo",
            "--identities",
            identity.to_str().unwrap(),
            "--outcomes",
            outcomes.to_str().unwrap(),
        ],
    );
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let canonical: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(canonical["schema"], 1);
    assert_eq!(canonical["session"]["identity"]["task_id"], "TASK-7");
    assert_eq!(canonical["session"]["outcome"], false);
    let input = d.path().join("corpus.jsonl");
    std::fs::write(&input, output.stdout).unwrap();
    let args = ["corpus", "import", input.to_str().unwrap(), "--repo", "acme/repo"];
    let imported = ok_json(run(&home, &args));
    assert_eq!(imported["added"], 1);
    assert_eq!(ok_json(run(&home, &args))["duplicates"], 1);
    let inspected = ok_json(run(&home, &["corpus", "inspect", "--repo", "acme/repo"]));
    assert_eq!(inspected["generation"], imported["generation"]);
    let learned = ok_json(run(&home, &["corpus", "learn", "--repo", "acme/repo"]));
    assert_eq!(learned["sessions_used"], 1);
    assert_eq!(learned["corpus_generation"], imported["generation"]);
    let audit = ok_json(run(&home, &["corpus", "inspect", "--repo", "acme/repo", "--evidence"]));
    assert_eq!(audit["training_declared_identity"], 1);
    assert_eq!(audit["training_inferred_identity"], 0);
    assert_eq!(audit["recorded_outcomes"], 1);
    assert_eq!(audit["proxy_outcomes"], 0);
    assert_eq!(audit["training_generation"], imported["generation"]);
    let logged: Value =
        serde_json::from_str(std::fs::read_to_string(home.join("learn.log")).unwrap().lines().last().unwrap()).unwrap();
    assert_eq!(logged["corpus_generation"], imported["generation"]);
    assert!(home.join("index.json").is_file());
    std::fs::write(&input, b"{broken\n").unwrap();
    let rejected = run(&home, &args);
    assert!(!rejected.status.success());
    let report: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(report["committed"], false);
    assert_eq!(ok_json(run(&home, &["corpus", "inspect", "--repo", "acme/repo"])), inspected);
}

/// Canonical import must feed the same production card compiler as `condition`: no demo
/// thresholds, authored routes, or review decisions are supplied by this fixture.
#[test]
fn canonical_learning_respects_production_card_settings() {
    use adage_intent::model;
    use adage_tracks::{io, RouteState, Step, Strength, Trigger};

    let d = tempfile::tempdir().unwrap();
    let mut transcripts = Vec::new();
    let mut outcomes = serde_json::Map::new();
    let mut identities = serde_json::Map::new();
    for name in ["Even", "Odd", "Prime"] {
        let sid = format!("validator-{name}");
        let raw = d.path().join(format!("{sid}.jsonl"));
        let lines = [
            json!({"type":"user","sessionId":sid,"cwd":"/recorded/validators","message":{"role":"user","content":format!("Add a new {name} validator")}}),
            json!({"type":"assistant","sessionId":sid,"cwd":"/recorded/validators","message":{"id":"edit","content":[
                {"type":"tool_use","id":"source","name":"Edit","input":{"file_path":"/recorded/validators/src/validate.py","old_string":"","new_string":format!("class {name}(Validator):\n    pass")}},
                {"type":"tool_use","id":"test","name":"Edit","input":{"file_path":"/recorded/validators/tests/test_validate.py","old_string":"","new_string":format!("def test_{}():\n    assert True", name.to_lowercase())}}
            ]}}),
            json!({"type":"user","sessionId":sid,"cwd":"/recorded/validators","message":{"content":[
                {"type":"tool_result","tool_use_id":"source","content":"updated","is_error":false},
                {"type":"tool_result","tool_use_id":"test","content":"updated","is_error":false}
            ]}}),
            json!({"type":"assistant","sessionId":sid,"cwd":"/recorded/validators","message":{"id":"check","content":[{"type":"tool_use","id":"check","name":"Bash","input":{"command":"make check"}}]}}),
            json!({"type":"user","sessionId":sid,"cwd":"/recorded/validators","message":{"content":[{"type":"tool_result","tool_use_id":"check","content":"passed","is_error":false}]}}),
        ];
        std::fs::write(&raw, lines.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
        transcripts.push(raw);
        outcomes.insert(sid.clone(), json!(true));
        identities.insert(sid, json!({"task_id":name,"group_id":format!("fixture/{name}")}));
    }
    let outcomes_path = d.path().join("outcomes.json");
    let identities_path = d.path().join("identities.json");
    std::fs::write(&outcomes_path, serde_json::to_vec(&outcomes).unwrap()).unwrap();
    std::fs::write(&identities_path, serde_json::to_vec(&identities).unwrap()).unwrap();
    let mut args = vec!["corpus", "export"];
    args.extend(transcripts.iter().map(|p| p.to_str().unwrap()));
    args.extend([
        "--repo",
        "fixture/validators",
        "--outcomes",
        outcomes_path.to_str().unwrap(),
        "--identities",
        identities_path.to_str().unwrap(),
    ]);
    let exported = run(&d.path().join("export-home"), &args);
    assert!(exported.status.success(), "{}", String::from_utf8_lossy(&exported.stderr));
    let corpus = d.path().join("corpus.jsonl");
    std::fs::write(&corpus, exported.stdout).unwrap();

    for (name, config, expected) in [("defaults", "", 1), ("disabled", "[learn]\ncards = false\n", 0)] {
        let home = d.path().join(name);
        let words = ["[UNK]", "add", "a", "new", "validator", "even", "odd", "prime"];
        let rows: Vec<_> = words
            .iter()
            .map(|w| if *w == "[UNK]" { vec![0.0; 8] } else { vec![1.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] })
            .collect();
        model::write_installed(
            &model::dir_in(&home.join("models")),
            "fixture@000000000000",
            8,
            0,
            100,
            &words.join("\n"),
            &model::safetensors_bytes(&rows),
        )
        .unwrap();
        std::fs::write(home.join("config.toml"), config).unwrap();
        let imported =
            ok_json(run(&home, &["corpus", "import", corpus.to_str().unwrap(), "--repo", "fixture/validators"]));
        assert_eq!(imported["added"], 3);
        let learned = ok_json(run_with_model(&home, &["corpus", "learn", "--repo", "fixture/validators"], "auto"));
        assert_eq!(learned["intent_model"], "fixture@000000000000");
        assert_eq!(learned["corpus_generation"], imported["generation"]);
        assert_eq!(learned["thresholds"], "min_support=5 fire_lower_bound=0.8 prime_rate=0.5");
        assert_eq!(
            learned["repos"]["fixture/validators"]["engine"]["cards"].as_u64().unwrap_or_default(),
            expected as u64,
            "{learned}"
        );
        let cards: Vec<_> = io::read_all(&home.join("tracks"))
            .unwrap()
            .into_iter()
            .flat_map(|t| t.routes)
            .filter(|r| matches!(r.step, Step::Card { .. }))
            .collect();
        assert_eq!(cards.len(), expected);
        for route in cards {
            let Step::Card { card } = route.step else { unreachable!() };
            assert_eq!((card.sessions, card.passed), (3, 3));
            assert_eq!(route.state, RouteState::Forming, "normal learned advice state; no approval fabricated");
            assert_eq!(route.strength, Strength::Prime);
            assert!(!route.authored);
            assert!(route.evidence.execution.is_none());
            let Trigger::Intent { threshold, .. } = route.trigger else { panic!("card needs its learned intent") };
            assert_eq!(threshold, adage_learn::cards::THRESHOLD);
        }
        assert!(!home.join("approvals.json").exists());
    }
}

#[test]
fn paired_evaluation_prints_its_reasons_and_fails_a_required_target() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("study.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema":1, "experiment":"empty-fixture", "kind":"same_model", "training_generation":"fixture",
            "training_groups":[], "training_inferred_identity":0, "training_unknown_identity":0, "pairs":[]
        }))
        .unwrap(),
    )
    .unwrap();
    let output = run(d.path(), &["eval", "paired", path.to_str().unwrap(), "--require-token-target"]);
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["token_target_accepted"], false);
    assert!(report["reasons"].as_array().unwrap().contains(&json!("insufficient_independent_tasks")));
}
