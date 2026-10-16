use adage_ingest::corpus::{Limits, Record};
use adage_learn::{
    corpus::{self, Mode},
    store::{Files, Store},
    LearnConfig,
};
use adage_trace::Session;
use std::io::{self, BufReader, Read};

const REPO: &str = "canonical-routine";

fn fixture(i: usize) -> Session {
    let mut s: Session = serde_json::from_str(include_str!("fixtures/canonical-routine.json")).unwrap();
    s.id = format!("routine-{i}");
    s.identity.task_id = Some(format!("FEATURE-{i}"));
    s
}

fn stream(sessions: &[Session]) -> Vec<u8> {
    sessions
        .iter()
        .map(|s| format!("{}\n", serde_json::to_string(&Record::new(s.clone())).unwrap()))
        .collect::<String>()
        .into_bytes()
}

fn store(root: &std::path::Path) -> Files {
    Files::new(&root.join("tracks"), &root.join("index.json"))
}

#[test]
fn imports_are_idempotent_order_independent_and_repository_scoped() {
    let d = tempfile::tempdir().unwrap();
    let a = store(&d.path().join("a"));
    let b = store(&d.path().join("b"));
    let mut other = fixture(0);
    other.repo = "other".into();
    let mut sessions = vec![fixture(1), fixture(0), fixture(1), other];
    let first = corpus::import(stream(&sessions).as_slice(), REPO, Mode::Append, Limits::default(), &a).unwrap();
    assert!(first.committed);
    assert_eq!((first.added, first.duplicates, first.elsewhere, first.sessions), (2, 1, 1, 2));
    sessions.reverse();
    let reverse = corpus::import(stream(&sessions).as_slice(), REPO, Mode::Append, Limits::default(), &b).unwrap();
    assert_eq!(first.generation, reverse.generation);
    assert_eq!(a.corpus_manifest(REPO).unwrap(), b.corpus_manifest(REPO).unwrap());
    let replay = corpus::import(stream(&sessions).as_slice(), REPO, Mode::Append, Limits::default(), &a).unwrap();
    assert_eq!((replay.added, replay.duplicates, replay.sessions), (0, 3, 2));
    assert_eq!(replay.generation, first.generation);
    assert!(a.corpus_manifest("other").unwrap().is_none());
}

#[test]
fn rejected_and_interrupted_imports_leave_the_previous_manifest_committed() {
    let d = tempfile::tempdir().unwrap();
    let store = store(d.path());
    corpus::import(stream(&[fixture(0)]).as_slice(), REPO, Mode::Append, Limits::default(), &store).unwrap();
    let before = store.corpus_manifest(REPO).unwrap();
    let mut conflict = fixture(0);
    conflict.outcome = Some(false);
    for data in [stream(&[fixture(1), conflict]), [stream(&[fixture(1)]), b"broken\n".to_vec()].concat()] {
        let rejected = corpus::import(data.as_slice(), REPO, Mode::Append, Limits::default(), &store).unwrap();
        assert!(!rejected.committed);
        assert_eq!(store.corpus_manifest(REPO).unwrap(), before);
    }
    struct FailedSource;
    impl Read for FailedSource {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("disconnected"))
        }
    }
    let source = io::Cursor::new(stream(&[fixture(1)])).chain(FailedSource);
    assert!(corpus::import(BufReader::new(source), REPO, Mode::Replace, Limits::default(), &store).is_err());
    assert_eq!(store.corpus_manifest(REPO).unwrap(), before);
}

#[test]
fn explicit_replacement_can_correct_and_remove_evidence_but_not_choose_between_conflicts() {
    let d = tempfile::tempdir().unwrap();
    let store = store(d.path());
    corpus::import(stream(&[fixture(0), fixture(1)]).as_slice(), REPO, Mode::Append, Limits::default(), &store)
        .unwrap();
    let mut corrected = fixture(0);
    corrected.outcome = Some(false);
    let rejected = corpus::import(
        stream(&[fixture(0), corrected.clone()]).as_slice(),
        REPO,
        Mode::Replace,
        Limits::default(),
        &store,
    )
    .unwrap();
    assert!(!rejected.committed);
    assert_eq!(rejected.conflicts, 1);
    let replaced =
        corpus::import(stream(&[corrected.clone()]).as_slice(), REPO, Mode::Replace, Limits::default(), &store)
            .unwrap();
    assert_eq!((replaced.replaced, replaced.removed, replaced.sessions), (1, 1, 1));
    assert_eq!(corpus::load(REPO, &store).unwrap().sessions, vec![corrected]);
}

#[test]
fn canonical_fixture_learns_the_expected_candidate_and_empty_replacement_retires_it() {
    let d = tempfile::tempdir().unwrap();
    let store = store(d.path());
    let sessions: Vec<_> = (0..5).map(fixture).collect();
    corpus::import(stream(&sessions).as_slice(), REPO, Mode::Append, Limits::default(), &store).unwrap();
    let report = corpus::learn(REPO, None, &store, &LearnConfig::default()).unwrap();
    assert_eq!(report.sessions_used, 5);
    let index = adage_tracks::Index::load(&store.index_path);
    let actions: Vec<_> = index.routes.iter().filter(|r| r.route.evidence.execution.is_some()).collect();
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].route.step,
        adage_tracks::Step::Run { command: "python -m pytest tests/test_feature.py".into() }
    );
    let evidence = actions[0].route.evidence.execution.as_ref().unwrap();
    assert_eq!((evidence.sessions, evidence.successful, evidence.opportunities), (5, 5, 5));
    corpus::import(b"".as_slice(), REPO, Mode::Replace, Limits::default(), &store).unwrap();
    corpus::learn(REPO, None, &store, &LearnConfig::default()).unwrap();
    assert!(adage_tracks::Index::load(&store.index_path).routes.is_empty());
}

#[test]
fn corrupt_evidence_is_an_error_and_object_names_cannot_escape_the_store() {
    let d = tempfile::tempdir().unwrap();
    let store = store(d.path());
    corpus::import(stream(&[fixture(0)]).as_slice(), REPO, Mode::Append, Limits::default(), &store).unwrap();
    let manifest = store.corpus_manifest(REPO).unwrap().unwrap();
    let hash = &manifest.sessions["routine-0"];
    let object = d.path().join("corpus/objects").join(&hash[..2]).join(format!("{hash}.json"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&object).unwrap().permissions().mode() & 0o077, 0);
    }
    std::fs::write(object, b"{}").unwrap();
    assert!(corpus::load(REPO, &store).unwrap_err().to_string().contains("digest mismatch"));
    assert!(store.corpus_object("../../outside").is_err());
}

#[test]
fn native_export_and_learning_share_child_lineage_without_overriding_declared_identity() {
    use adage_ingest::batch::{self, Options};
    use serde_json::json;
    use std::collections::BTreeMap;

    let d = tempfile::tempdir().unwrap();
    let record = |ordinal, kind: &str, payload| json!({"timestamp":"2026-10-01T00:00:00Z", "ordinal":ordinal, "type":kind, "payload":payload});
    let meta = |id: &str| json!({"id":id,"cwd":"/fixture/corpus-lineage","source":"cli"});
    let user = || json!({"type":"message","role":"user","content":[{"type":"input_text","text":"check this module"}]});
    let call = |id: &str| json!({"type":"function_call","name":"exec_command","call_id":id,"arguments":"{\"cmd\":\"cargo test\"}"});
    let output = |id: &str| json!({"type":"function_call_output","call_id":id,"output":"Process exited with code 0\nOutput:\nok"});
    let parent = vec![
        record(0, "session_meta", meta("parent")),
        record(1, "response_item", user()),
        record(2, "response_item", call("parent-call")),
        record(3, "response_item", output("parent-call")),
    ];
    let mut child_meta = meta("child");
    child_meta["source"] =
        json!({"subagent":{"thread_spawn":{"parent_thread_id":"parent","agent_path":"/root/child"}}});
    child_meta["parent_thread_id"] = json!("parent");
    child_meta["subagent_history_start_ordinal"] = json!(4);
    let mut child = parent.clone();
    child[0] = record(0, "session_meta", child_meta);
    child.extend([
        record(
            4,
            "response_item",
            json!({"type":"agent_message","recipient":"/root/child","encrypted_content":"opaque"}),
        ),
        record(5, "response_item", call("child-call")),
        record(6, "response_item", output("child-call")),
    ]);
    let paths: Vec<_> = [("child", child), ("parent", parent)]
        .into_iter()
        .map(|(name, rows)| {
            let path = d.path().join(format!("{name}.jsonl"));
            std::fs::write(&path, rows.iter().map(|r| format!("{r}\n")).collect::<String>()).unwrap();
            path
        })
        .collect();
    let options =
        Options { repo_override: Some(REPO.into()), locator: &adage_ingest::AsRecorded, ..Default::default() };
    let batch = batch::read(&paths, &options);
    assert_eq!(batch.sessions.len(), 2);
    let mut exported = Vec::new();
    corpus::export(&paths, Some(REPO), &BTreeMap::new(), &BTreeMap::new(), &mut exported).unwrap();
    let sessions: Vec<_> = String::from_utf8(exported)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Record>(l).unwrap().session)
        .collect();
    assert_eq!(sessions, batch.sessions);
    assert_eq!(sessions[0].evidence_group(), sessions[1].evidence_group());
    assert!(sessions[0].prompt.is_empty(), "an encrypted assignment never inherits the parent's task");
    assert_eq!(sessions[0].steps.len(), 1, "only the child's own action survives");

    let declared = adage_trace::EvidenceIdentity { task_id: Some("explicit-task".into()), ..Default::default() };
    let identities = BTreeMap::from([("child".into(), declared.clone())]);
    let mut exported = Vec::new();
    corpus::export(&paths, Some(REPO), &identities, &BTreeMap::new(), &mut exported).unwrap();
    let first: Record = serde_json::from_slice(exported.split(|b| *b == b'\n').next().unwrap()).unwrap();
    assert_eq!(first.session.identity, declared);
}

#[test]
fn separately_exported_nested_workers_rejoin_when_the_corpus_is_loaded() {
    use serde_json::json;
    use std::collections::BTreeMap;

    let d = tempfile::tempdir().unwrap();
    let trace = |id: &str, parent: Option<&str>| {
        let mut meta = json!({"id":id,"cwd":"/fixture/corpus-lineage","source":"cli"});
        if let Some(parent) = parent {
            meta["source"] = json!({"subagent":{"thread_spawn":{
                "parent_thread_id":parent,"agent_path":format!("/root/{id}")}}});
            meta["parent_thread_id"] = json!(parent);
            meta["subagent_history_start_ordinal"] = json!(1);
        }
        let prompt = if parent.is_some() {
            json!({"type":"agent_message","recipient":format!("/root/{id}"),"encrypted_content":"opaque"})
        } else {
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"verify module"}]})
        };
        let rows = [
            ("session_meta", meta),
            ("response_item", prompt),
            (
                "response_item",
                json!({"type":"function_call","name":"exec_command","call_id":id,
                "arguments":"{\"cmd\":\"cargo test\"}"}),
            ),
            (
                "response_item",
                json!({"type":"function_call_output","call_id":id,
                "output":"Process exited with code 0\nOutput:\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s"}),
            ),
        ];
        let path = d.path().join(format!("{id}.jsonl"));
        let bytes = rows
            .into_iter()
            .enumerate()
            .map(|(ordinal, (kind, payload))| {
                format!(
                    "{}\n",
                    json!({"timestamp":"2026-10-01T00:00:00Z","ordinal":ordinal,"type":kind,"payload":payload})
                )
            })
            .collect::<String>();
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let root = trace("root", None);
    let child = trace("child", Some("root"));
    let grandchild = trace("grandchild", Some("child"));
    let export = |paths: &[std::path::PathBuf]| {
        let mut bytes = Vec::new();
        corpus::export(paths, Some(REPO), &BTreeMap::new(), &BTreeMap::new(), &mut bytes).unwrap();
        bytes
    };
    let a = export(&[root, child]);
    let b = export(&[grandchild]);
    let before: Record = serde_json::from_slice(b.split(|x| *x == b'\n').next().unwrap()).unwrap();
    assert_eq!(before.session.identity.group_id.as_deref(), Some("codex-lineage:child"));
    let files = store(d.path());
    for bytes in [&a, &b] {
        assert!(corpus::import(bytes.as_slice(), REPO, Mode::Append, Limits::default(), &files).unwrap().committed);
    }
    let manifest = files.corpus_manifest(REPO).unwrap().unwrap();
    let loaded = corpus::load(REPO, &files).unwrap();
    assert_eq!(loaded.sessions.len(), 3);
    assert!(loaded.sessions.iter().all(|s| s.identity.group_id.as_deref() == Some("codex-lineage:root")));
    assert_eq!(corpus::audit(REPO, &files).unwrap().training_groups.len(), 1);
    // Regrouping affects the evidence view, not the pinned source object or its committed digest.
    let original = files.corpus_object(&manifest.sessions["grandchild"]).unwrap();
    assert_eq!(serde_json::from_slice::<Session>(&original).unwrap(), before.session);
    assert_eq!(files.corpus_manifest(REPO).unwrap().unwrap(), manifest);
}
