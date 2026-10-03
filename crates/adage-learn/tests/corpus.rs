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
