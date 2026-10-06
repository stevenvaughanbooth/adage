//! Experiment lineage uses canonical production route types, including unreviewed candidates
//! and the learner's candidates file.
use adage_core::{spans::SpanKind, Effect};
use adage_eval::experiment::sources;
use adage_tracks::*;

fn fixture(name: &str) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: name.into(),
                version: "0.1.0".into(),
                description: "candidate lineage fixture".into(),
                provenance: "learned".into(),
            },
            scope: Scope { repo: "github.com/adage-evals/pilot".into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "lookup-ticket".into(),
            summary: "look up the issue".into(),
            trigger: Trigger::Prompt { span: SpanKind::Ticket },
            step: Step::Run { command: "git show {item}".into() },
            slots: vec![Slot {
                name: "item".into(),
                source: SlotSource::Prompt { span: SpanKind::Ticket },
                contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
            }],
            effect: Effect::Observe,
            strength: Strength::Prime,
            state: RouteState::Nascent,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence { support: 8, opportunities: 10, ..Default::default() },
            authored: false,
        }],
    }
}

#[test]
fn canonical_sources_preserve_nascent_slots_state_strength_and_reviewed_index() {
    let dir = tempfile::tempdir().unwrap();
    let mut nascent = fixture("pilot/zeta");
    let mut second = nascent.routes[0].clone();
    second.id = "another-ticket".into();
    nascent.routes.push(second);
    let mut reviewed = fixture("pilot/alpha");
    reviewed.routes[0].state = RouteState::Firm;
    reviewed.routes[0].strength = Strength::Fire;
    io::write(&dir.path().join("z-last"), &nascent).unwrap();
    io::write(&dir.path().join("a-first"), &reviewed).unwrap();
    let yaml = dir.path().join("z-last/routes/lookup-ticket.yaml");
    let original = std::fs::read(&yaml).unwrap();

    let output = sources(dir.path()).unwrap();
    assert_eq!(output.schema, 1);
    assert_eq!(output.tracks.len(), 2);
    assert_eq!(output.tracks[0].manifest, reviewed.manifest);
    assert_eq!(output.tracks[1].routes[1], nascent.routes[0]);
    assert_eq!(output.tracks[1].routes[0].id, "another-ticket");
    assert_eq!(output.index.routes.len(), 1);
    assert_eq!(output.index.routes[0].route, reviewed.routes[0]);
    let value = serde_json::to_value(&output).unwrap();
    assert_eq!(value["tracks"][1]["routes"][1]["state"], "nascent");
    assert_eq!(value["tracks"][1]["routes"][1]["strength"], "prime");
    assert!(!value["tracks"][1]["routes"][1]["slots"].as_array().unwrap().is_empty());
    assert_eq!(serde_json::to_vec(&output).unwrap(), serde_json::to_vec(&sources(dir.path()).unwrap()).unwrap());
    assert_eq!(std::fs::read(&yaml).unwrap(), original);
    assert!(!dir.path().join("index.json").exists());
}

#[test]
fn missing_or_corrupt_tracks_fail_instead_of_returning_partial_lineage() {
    let dir = tempfile::tempdir().unwrap();
    assert!(sources(&dir.path().join("missing")).is_err());
    assert!(sources(dir.path()).unwrap().tracks.is_empty());
    io::write(&dir.path().join("valid"), &fixture("pilot/valid")).unwrap();
    let broken = dir.path().join("broken");
    io::write(&broken, &fixture("pilot/broken")).unwrap();
    let route = broken.join("routes/lookup-ticket.yaml");
    std::fs::write(&route, "id: [broken\n").unwrap();
    assert!(sources(dir.path()).unwrap_err().to_string().contains("invalid experiment source track"));
    io::write(&broken, &fixture("pilot/broken")).unwrap();
    std::fs::remove_dir_all(broken.join("routes")).unwrap();
    assert!(sources(dir.path()).is_err());
    io::write(&broken, &fixture("pilot/broken")).unwrap();
    std::fs::remove_file(broken.join("track.toml")).unwrap();
    assert!(sources(dir.path()).unwrap_err().to_string().contains("no track.toml"));
    std::fs::write(broken.join("track.toml"), "[broken").unwrap();
    assert!(sources(dir.path()).is_err());
}

#[test]
fn duplicate_sources_and_invalid_execution_sidecars_are_not_silently_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let track = dir.path().join("track");
    io::write(&track, &fixture("pilot/source")).unwrap();
    let route = track.join("routes/lookup-ticket.yaml");
    std::fs::copy(&route, track.join("routes/duplicate.yaml")).unwrap();
    assert!(sources(dir.path()).unwrap_err().to_string().contains("duplicate route id"));
    std::fs::remove_file(track.join("routes/duplicate.yaml")).unwrap();
    std::fs::write(track.join("exec.json"), "not-json").unwrap();
    assert!(sources(dir.path()).unwrap_err().to_string().contains("invalid experiment exec bindings"));
    std::fs::remove_file(track.join("exec.json")).unwrap();
    std::fs::write(track.join("routes/orphan.json"), "{}").unwrap();
    assert!(sources(dir.path()).unwrap_err().to_string().contains("unexpected route source"));
    std::fs::remove_file(track.join("routes/orphan.json")).unwrap();
    io::write(&dir.path().join("copy"), &fixture("pilot/source")).unwrap();
    assert!(sources(dir.path()).unwrap_err().to_string().contains("duplicate repository/track identity"));
}

#[cfg(unix)]
#[test]
fn source_symlinks_are_rejected_without_following_them() {
    let dir = tempfile::tempdir().unwrap();
    let track = dir.path().join("track");
    io::write(&track, &fixture("pilot/source")).unwrap();
    let route = track.join("routes/lookup-ticket.yaml");
    let outside = dir.path().join("outside");
    std::fs::rename(&route, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &route).unwrap();
    assert!(sources(&track).unwrap_err().to_string().contains("not a regular file"));
    std::fs::remove_file(&route).unwrap();
    std::fs::rename(&outside, &route).unwrap();
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&track, &alias).unwrap();
    assert!(sources(&alias).is_err());
}

/// The learner writes its candidates (plan D1) to `<slug>/learned/candidates.json`, also when it
/// learned no route there. They are lineage, through the compiled index, never a stray file; a
/// corrupt one fails, and the same name anywhere else is still unexpected.
#[test]
fn the_learners_candidates_are_lineage_not_a_stray_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = "github.com/adage-evals/pilot";
    let mut file = candidates::CandidateFile::new(repo, 5);
    file.candidates.push(candidates::Entry {
        id: "lint-after-tests".into(),
        trigger: Trigger::Context {
            items: vec![
                CtxItem::Edit { glob: "acme/*.py".into() },
                CtxItem::Act { keys: vec!["exec make test".into()] },
            ],
            within: 8,
        },
        step: Step::Run { command: "make lint".into() },
        support: 3,
        days: 3,
        last_seen: Some("2026-09-09".into()),
        review: false,
        sessions: vec!["a".into(), "b".into(), "c".into()],
    });
    candidates::write(dir.path(), &file).unwrap();
    let alone = sources(dir.path()).unwrap();
    assert!(alone.tracks.is_empty(), "no route was learned");
    assert_eq!(alone.index.candidates(repo).len(), 1);

    let path = candidates::path(dir.path(), repo);
    io::write(path.parent().unwrap(), &fixture("pilot/learned")).unwrap();
    candidates::write(dir.path(), &file).unwrap();
    let beside = sources(dir.path()).unwrap();
    assert_eq!(beside.tracks.len(), 1);
    assert_eq!(beside.index.candidates(repo).len(), 1);

    std::fs::write(&path, "{broken").unwrap();
    assert!(sources(dir.path()).unwrap_err().to_string().contains("invalid experiment candidates"));
    std::fs::remove_file(&path).unwrap();
    std::fs::write(dir.path().join(candidates::FILE), "{}").unwrap();
    assert!(sources(dir.path()).unwrap_err().to_string().contains("unexpected file outside a source track"));
}
