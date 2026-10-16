use adage_graph::procedures::ProcedureKind;
use adage_ingest::format::Decode;
use adage_ingest::locate::AsRecorded;
use adage_learn::store::{Files, Store};
use adage_learn::{learn_sessions, LearnConfig};
use serde_json::json;

fn batch() -> adage_ingest::batch::Batch {
    let records = [
        json!({"type":"user","sessionId":"parent","agentId":"auditor","cwd":"/missing/repo",
            "isSidechain":true,"message":{"content":"Audit parser"}}),
        json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"run1",
            "name":"Bash","input":{"command":"pytest tests/test_parser.py","description":"Run mutated implementation"}}]}}),
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"run1",
            "content":"27 passed in 0.2s","is_error":false}]}}),
    ];
    let text = records.iter().map(serde_json::Value::to_string).collect::<Vec<_>>().join("\n");
    let cx = Decode {
        table: adage_core::CommandTable::load(),
        locator: &AsRecorded,
        repo_override: Some("github.com/acme/parser"),
        subagent: true,
    };
    let s = adage_ingest::claude::decode(text.as_bytes(), &cx).unwrap();
    assert_eq!(s.outcome, None);
    let mut batch = adage_ingest::batch::Batch { sessions: vec![s], read: 1, ..Default::default() };
    adage_ingest::batch::group_native_lineages(&mut batch.sessions);
    batch
}

#[test]
fn normal_learning_persists_private_proposals_without_activating_or_grading_them() {
    let batch = batch();
    let d = tempfile::tempdir().unwrap();
    let tracks = d.path().join("tracks");
    let index = d.path().join("index.json");
    let cfg = LearnConfig { instincts: None, ..Default::default() };
    let again = adage_ingest::batch::Batch { sessions: batch.sessions.clone(), read: batch.read, ..Default::default() };
    let first = learn_sessions(batch, None, None, &tracks, &index, &cfg).unwrap();
    let learned = &first.repos["github.com/acme/parser"].engine.procedures;
    assert_eq!(learned.proposals.len(), 1);
    assert_eq!(learned.proposals[0].kind, ProcedureKind::NegativeControl);
    assert_eq!(learned.proposals[0].evidence_groups, 1);
    let path = tracks.join("github-com-acme-parser/.procedures.json");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(serde_json::from_slice::<adage_graph::procedures::Learning>(&bytes).unwrap(), *learned);
    let idx = adage_tracks::Index::load(&index);
    assert!(idx.routes.is_empty(), "a useful observed method is not automatically a qualified route");
    let second = learn_sessions(again, None, None, &tracks, &index, &cfg).unwrap();
    assert_eq!(second.repos["github.com/acme/parser"].engine.procedures, *learned);
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "stable repeated conditioning");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn procedure_persistence_alone_does_not_establish_a_learned_track() {
    let d = tempfile::tempdir().unwrap();
    let tracks = d.path().join("tracks");
    let index = d.path().join("index.json");
    let files = Files::new(&tracks, &index);
    let batch = batch();
    let learning = adage_graph::procedures::learn(&batch.sessions.iter().collect::<Vec<_>>());
    assert_eq!(learning.proposals.len(), 1);
    files.write_procedures("github.com/acme/parser", &learning).unwrap();
    let bytes = std::fs::read(tracks.join("github-com-acme-parser/.procedures.json")).unwrap();
    assert_eq!(serde_json::from_slice::<adage_graph::procedures::Learning>(&bytes).unwrap(), learning);
    assert!(!files.holds("github-com-acme-parser/learned"));
    assert!(!index.exists(), "persisting evidence alone does not compile an index");
}

#[test]
fn procedure_evidence_does_not_block_retrying_a_legacy_review_migration() {
    use adage_tracks::review::{self, Decision, Review, Reviews};
    use adage_tracks::{
        io, Evidence, Gauge, Manifest, Route, RouteState, Scope, Step, Strength, Track, TrackInfo, Trigger,
    };

    let d = tempfile::tempdir().unwrap();
    let tracks = d.path().join("tracks");
    let index = d.path().join("index.json");
    let files = Files::new(&tracks, &index);
    let cfg = LearnConfig { instincts: None, ..Default::default() };
    let legacy_name = "github-com-acme-parser/finish-line";
    let learned_name = "github-com-acme-parser/learned";
    let legacy_dir = tracks.join(legacy_name);
    let route = Route {
        id: "reviewed-check".into(),
        summary: "Run the project check".into(),
        trigger: Trigger::SessionStart,
        step: Step::Run { command: "make test".into() },
        slots: vec![],
        effect: adage_core::Effect::Verify,
        strength: Strength::Prime,
        state: RouteState::Extinguished,
        exit: vec![],
        evidence: Evidence::default(),
        authored: false,
    };
    let track = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: legacy_name.into(),
                version: "0.1.0".into(),
                description: "Legacy checks".into(),
                provenance: String::new(),
            },
            scope: Scope { repo: "github.com/acme/parser".into() },
            gauge: Gauge::default(),
        },
        routes: vec![route.clone()],
    };
    io::validate(&track).unwrap();
    io::write(&legacy_dir, &track).unwrap();
    std::fs::write(legacy_dir.join(review::FILE), "routes: [not a review map]").unwrap();

    let first = learn_sessions(batch(), None, None, &tracks, &index, &cfg).unwrap();
    assert!(first.repos["github.com/acme/parser"].tracks.is_empty());
    assert!(files.holds(legacy_name), "failed review reads leave the old track in place");
    assert!(tracks.join("github-com-acme-parser/.procedures.json").is_file());
    assert!(!files.holds(learned_name), "private evidence is not a completed track migration");

    let mut reviews = Reviews::default();
    reviews.routes.insert(
        route.id.clone(),
        Review {
            decision: Decision::Extinguished,
            on: "2026-10-15".into(),
            by: None,
            basis: review::basis(&route, 0, 0),
            via: None,
        },
    );
    review::save(&legacy_dir, &reviews).unwrap();
    let (_, planned) = adage_learn::plan(batch().sessions, None, &files, &cfg).unwrap();
    let planned = planned.iter().find(|p| p.name == learned_name).expect("migration is retried");
    assert_eq!(planned.replaces, [legacy_name]);
    assert_eq!(
        planned.track.routes.as_slice(),
        std::slice::from_ref(&route),
        "the repaired review keeps the extinguished route"
    );

    learn_sessions(batch(), None, None, &tracks, &index, &cfg).unwrap();
    assert!(!files.holds(legacy_name), "the legacy directory is retired only after migration");
    assert_eq!(files.track(learned_name).unwrap().routes, [route]);
    assert!(
        adage_tracks::Index::load(&index).routes.iter().all(|r| !r.route.state.is_live()),
        "the person's rejection remains effective"
    );
}

#[test]
fn unexplained_recovery_is_reported_privately_without_an_active_reflex_or_task_grade() {
    let mut batch = batch();
    let s = &mut batch.sessions[0];
    let mut failed = s.steps[0].clone();
    failed.call.input["description"] = json!("Run tests");
    failed.call.output = "1 failed, 26 passed in 0.2s".into();
    failed.call.tool_use_id = Some("failed-check".into());
    let mut passed = failed.clone();
    passed.turn += 1;
    passed.call.tool_use_id = Some("passed-check".into());
    passed.call.output = "27 passed in 0.2s".into();
    s.steps = vec![failed, passed];
    assert_eq!(s.outcome, None);
    let d = tempfile::tempdir().unwrap();
    let tracks = d.path().join("tracks");
    let index = d.path().join("index.json");
    let cfg = LearnConfig { instincts: None, ..Default::default() };
    let report = learn_sessions(batch, None, None, &tracks, &index, &cfg).unwrap();
    let learning = &report.repos["github.com/acme/parser"].engine.procedures;
    let proposal = learning.proposals.iter().find(|p| p.kind == ProcedureKind::UnexplainedRecovery).unwrap();
    assert_eq!(proposal.status, "needs_evidence_review");
    assert_eq!(proposal.evidence_groups, 1);
    assert_eq!(proposal.episodes[0].checks[0].call_id.as_deref(), Some("failed-check"));
    assert_eq!(proposal.episodes[0].checks[1].call_id.as_deref(), Some("passed-check"));
    let bytes = std::fs::read(tracks.join("github-com-acme-parser/.procedures.json")).unwrap();
    assert_eq!(serde_json::from_slice::<adage_graph::procedures::Learning>(&bytes).unwrap(), *learning);
    assert!(adage_tracks::Index::load(&index).routes.is_empty(), "a retry proposal creates no route");
}

#[test]
fn persistence_method_leads_use_normal_reporting_without_becoming_a_route() {
    let mut batch = batch();
    batch.sessions[0].steps[0].call.input["description"] = json!("Run serialized index round-trip regression");
    let d = tempfile::tempdir().unwrap();
    let tracks = d.path().join("tracks");
    let index = d.path().join("index.json");
    let cfg = LearnConfig { instincts: None, ..Default::default() };
    let report = learn_sessions(batch, None, None, &tracks, &index, &cfg).unwrap();
    let learning = &report.repos["github.com/acme/parser"].engine.procedures;
    assert_eq!(learning.proposals.len(), 1);
    assert_eq!(learning.proposals[0].kind, ProcedureKind::PersistenceEquivalence);
    assert_eq!(learning.proposals[0].template, None);
    assert_eq!(learning.proposals[0].status, "needs_evidence_review");
    assert!(adage_tracks::Index::load(&index).routes.is_empty());
    let bytes = std::fs::read(tracks.join("github-com-acme-parser/.procedures.json")).unwrap();
    assert_eq!(serde_json::from_slice::<adage_graph::procedures::Learning>(&bytes).unwrap(), *learning);
}

#[test]
fn normal_conditioning_discovers_declared_custom_checks_without_inventing_a_test_result() {
    let records = [
        json!({"type":"user","sessionId":"parent","agentId":"worker","isSidechain":true,
            "cwd":"/missing/project","message":{"content":"Check the package layout"}}),
        json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"run",
            "name":"Bash","input":{"command":"node tools/check-layout.mjs"}}]}}),
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"run",
            "content":"layout ok","is_error":false}]}}),
        json!({"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"declare",
            "name":"Write","input":{"file_path":"/missing/project/.github/workflows/checks.yml",
                "content":"name: Checks\njobs:\n  layout:\n    steps:\n      - name: Repository layout\n        run: node tools/check-layout.mjs\n"}}]}}),
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"declare",
            "content":"File created successfully at: /missing/project/.github/workflows/checks.yml","is_error":false}]}}),
    ];
    let text = records.iter().map(serde_json::Value::to_string).collect::<Vec<_>>().join("\n");
    let cx = Decode {
        table: adage_core::CommandTable::load(),
        locator: &AsRecorded,
        repo_override: Some("github.com/acme/layout"),
        subagent: true,
    };
    let session = adage_ingest::claude::decode(text.as_bytes(), &cx).unwrap();
    assert_eq!(session.steps[0].norm.effect, adage_core::Effect::Environment);
    assert!(session.steps[0].verification().is_none());
    let original = session.clone();
    let mut batch = adage_ingest::batch::Batch { sessions: vec![session], read: 1, ..Default::default() };
    adage_ingest::batch::group_native_lineages(&mut batch.sessions);
    let again = adage_ingest::batch::Batch { sessions: batch.sessions.clone(), read: 1, ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let tracks = d.path().join("tracks");
    let index = d.path().join("index.json");
    let cfg = LearnConfig { instincts: None, ..Default::default() };
    let first = learn_sessions(batch, None, None, &tracks, &index, &cfg).unwrap();
    let learning = &first.repos["github.com/acme/layout"].engine.procedures;
    assert_eq!(learning.declared_validators.len(), 1);
    assert_eq!(learning.checks, 0, "a declared script is not a recognized test runner");
    assert!(learning.proposals.is_empty());
    let candidate = &learning.declared_validators[0];
    assert_eq!((candidate.sessions, candidate.evidence_groups), (1, 1));
    assert_eq!(candidate.observations[0].outcome, "unknown");
    assert_eq!(candidate.observations[0].declared_role_basis, "script_filename");
    assert_eq!(candidate.observations[0].declared_role, "check-layout.mjs");
    assert_eq!(
        candidate.observations[0].declaration_timing,
        adage_graph::procedures::DeclarationTiming::AfterInvocation
    );
    assert!(adage_tracks::Index::load(&index).routes.is_empty());
    let path = tracks.join("github-com-acme-layout/.procedures.json");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(serde_json::from_slice::<adage_graph::procedures::Learning>(&bytes).unwrap(), *learning);
    let second = learn_sessions(again, None, None, &tracks, &index, &cfg).unwrap();
    assert_eq!(second.repos["github.com/acme/layout"].engine.procedures, *learning);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(original.outcome, None);
    assert_eq!(original.steps[0].norm.effect, adage_core::Effect::Environment);
}
