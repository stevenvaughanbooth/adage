//! Converting local tracks to packs: the tracks `adage condition --demo` learns from the committed simulator
//! fixtures (`sim/fixtures/claude`, copied to `tests/fixtures/tracks`), and the instinct catalogue
//! bound on the acme fixture repository.

use adage_pack::{from_track_dir, from_track_dir_with, ClosureLevel, Pack, PackOrigin, Shape, TriggerV2};
use adage_tracks::{io, Gauge, Manifest, Scope, Track, TrackInfo};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn track(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tracks/acme-billing").join(name)
}

/// A repository holding the script the learned lookup runs.
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("scripts")).unwrap();
    std::fs::write(d.path().join("scripts/wi-lookup"), "#!/usr/bin/env python3\nprint('{}')\n").unwrap();
    d
}

fn round_trip(p: &Pack) -> Pack {
    let d = tempfile::tempdir().unwrap();
    p.write(d.path()).unwrap();
    let back = Pack::load(d.path()).unwrap();
    assert_eq!(&back, p);
    back
}

#[test]
fn learned_work_items_become_an_l_ref_pack_with_the_implicit_chain() {
    let root = repo();
    let u = from_track_dir_with(&track("work-items"), Some(root.path())).unwrap();
    assert!(u.skipped.is_empty(), "{:?}", u.skipped);
    let p = round_trip(&u.pack);
    assert_eq!(p.manifest.pack.name, "acme-billing/work-items");
    assert_eq!(p.manifest.pack.origin, PackOrigin::Reflex);
    let lookup = p.routes.iter().find(|r| r.id == "lookup-work-item").unwrap();
    assert_eq!(lookup.shape, Shape::Run);
    let sc = lookup.steps[0].script.as_ref().unwrap();
    assert_eq!((sc.path.as_str(), sc.closure), ("scripts/wi-lookup", ClosureLevel::Ref));
    assert_eq!(p.manifest.requires.repo_files[0].sha256, sc.sha256);
    let read = p.routes.iter().find(|r| r.id == "read-linked-files").unwrap();
    assert_eq!(read.shape, Shape::Read);
    assert_eq!(read.inputs["files"].ty, "repo-path-list");
    let closure = p.closure().unwrap().render();
    assert!(closure.contains("edge:after route:lookup-work-item->route:read-linked-files -"), "{closure}");
    // The learned contract became a user type named by its pattern's hash.
    assert_eq!(p.types.len(), 1);
    assert!(p.types.keys().all(|k| k.starts_with("c-")));
}

#[test]
fn without_a_repository_the_script_route_is_skipped_not_left_unpinned() {
    let u = from_track_dir(&track("work-items")).unwrap();
    assert_eq!(u.skipped.len(), 1);
    assert_eq!(u.skipped[0].0, "lookup-work-item");
    assert!(u.skipped[0].1.contains("scripts/wi-lookup"), "{:?}", u.skipped);
    assert_eq!(u.pack.routes.len(), 1);
    round_trip(&u.pack);
}

#[test]
fn learned_dead_ends_and_finish_line_become_l_none_packs() {
    let dead = from_track_dir(&track("dead-ends")).unwrap();
    assert!(dead.skipped.is_empty());
    let p = round_trip(&dead.pack);
    assert!(p.routes.iter().all(|r| r.shape == Shape::Avoid));
    assert!(p.manifest.requires.repo_files.is_empty() && p.scripts.is_empty());

    let finish = from_track_dir(&track("finish-line")).unwrap();
    let p = round_trip(&finish.pack);
    assert_eq!(p.routes[0].shape, Shape::Obligation);
    assert_eq!(p.manifest.requires.tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["make"]);
}

#[test]
fn converting_is_deterministic() {
    let root = repo();
    let a = from_track_dir_with(&track("work-items"), Some(root.path())).unwrap().pack;
    let b = from_track_dir_with(&track("work-items"), Some(root.path())).unwrap().pack;
    assert_eq!(a.digest().unwrap(), b.digest().unwrap());
    for r in &a.routes {
        assert_eq!(a.route_exec_digest(&r.id), b.route_exec_digest(&r.id));
    }
}

#[test]
fn the_instinct_catalogue_bound_on_acme_becomes_an_instinct_pack() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../adage-instincts/tests/fixtures/acme");
    let bound = adage_instincts::bind(&fixture, &[], &BTreeSet::new());
    assert!(bound.routes.len() >= 4, "{:?}", bound.routes.iter().map(|r| &r.id).collect::<Vec<_>>());
    let track = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme/instincts".into(),
                version: "0.1.0".into(),
                description: "Instincts bound on acme".into(),
                provenance: "instinct:core@0.1.0".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: bound.routes.clone(),
    };
    let d = tempfile::tempdir().unwrap();
    io::write(d.path(), &track).unwrap();
    let u = from_track_dir(d.path()).unwrap();
    assert!(u.skipped.is_empty(), "{:?}", u.skipped);
    let p = round_trip(&u.pack);
    assert_eq!(p.manifest.pack.origin, PackOrigin::Instinct);
    assert_eq!(p.routes.len(), bound.routes.len());
    let shapes: BTreeSet<Shape> = p.routes.iter().map(|r| r.shape).collect();
    assert!(
        shapes.contains(&Shape::After) && shapes.contains(&Shape::Obligation) && shapes.contains(&Shape::Advise),
        "{shapes:?}"
    );
    assert!(p.routes.iter().all(|r| r.strength == adage_pack::Strength::Prime));
    assert!(p.manifest.requires.templates.iter().any(|t| t == "advise:generated-file@1"));
    // The dead end the demo learned keeps its id and its failing-command trigger.
    let de = p.routes.iter().find(|r| r.id == "dead-end-d41b1e").unwrap();
    assert!(matches!(&de.trigger, TriggerV2::AfterFailure { signature, .. } if signature.contains("ACME_ENV")));
}

#[test]
fn routine_execution_evidence_survives_sharing_without_a_grant() {
    let d = tempfile::tempdir().unwrap();
    let mut t = io::read(&track("finish-line")).unwrap();
    let evidence = adage_tracks::ExecutionEvidence { opportunities: 5, successful: 5, sessions: 5 };
    t.routes[0].evidence.execution = Some(evidence.clone());
    io::write(d.path(), &t).unwrap();
    let p = round_trip(&from_track_dir(d.path()).unwrap().pack);
    assert_eq!(p.routes[0].evidence.as_ref().unwrap().execution.as_ref(), Some(&evidence));
    let r = adage_pack::bind::to_runtime(&p.routes[0], &p, &adage_core::CommandTable::load()).unwrap();
    let ir = adage_tracks::IndexedRoute {
        repo: "another-repository".into(),
        track: "shared/routine".into(),
        version: "0.1.0".into(),
        route: r,
        exec: None,
    };
    assert!(ir.exec_only(), "import must not turn a silent CPU candidate into unsolicited advice");
    assert_eq!(ir.route.evidence.execution, Some(evidence));
    assert!(p.routes[0].exec.is_none(), "observational evidence does not create execution authority");
}
