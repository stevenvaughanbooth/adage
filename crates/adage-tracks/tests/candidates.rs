//! T-D1-05: candidates in the index. `compile_root` reads each repository's `candidates.json`;
//! `compile(tracks)` is unchanged; the candidates' words and globs join the fast path's triggers;
//! their net is separate from the routes' and has its own digest; an index written before them
//! loads with none; the same inputs give the same bytes.

use adage_core::Effect;
use adage_tracks::candidates::{self, CandidateFile, Entry};
use adage_tracks::context::CtxNet;
use adage_tracks::*;

fn ctx(items: Vec<CtxItem>) -> Trigger {
    Trigger::Context { items, within: 8 }
}

fn act(k: &str) -> CtxItem {
    CtxItem::Act { keys: vec![k.into()] }
}

fn track() -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme-billing/learned".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme-billing".into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "run-aaaaaa".into(),
            summary: String::new(),
            trigger: ctx(vec![act("exec make gen"), act("exec make check")]),
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: Effect::Verify,
            strength: Strength::Prime,
            state: RouteState::Firm,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence::default(),
            authored: false,
        }],
    }
}

fn entry(id: &str, items: Vec<CtxItem>, command: &str) -> Entry {
    Entry {
        id: id.into(),
        trigger: ctx(items),
        step: Step::Run { command: command.into() },
        support: 2,
        days: 2,
        last_seen: Some("2026-09-09".into()),
        review: false,
        sessions: vec!["s1".into(), "s2".into()],
    }
}

fn setup() -> (tempfile::TempDir, Vec<Track>) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("tracks");
    let t = track();
    io::write(&root.join("acme-billing/learned"), &t).unwrap();
    let mut f = CandidateFile::new("acme-billing", 3);
    f.candidates = vec![
        entry("run-bbbbbb", vec![CtxItem::Edit { glob: "acme/*.py".into() }, act("exec ruff check")], "make lint"),
        // The same id as a route: never a candidate in the index.
        entry("run-aaaaaa", vec![act("exec make gen"), act("exec make check")], "make test"),
    ];
    candidates::write(&root, &f).unwrap();
    (d, vec![t])
}

#[test]
fn compile_root_reads_candidates_and_compile_is_unchanged() {
    let (d, tracks) = setup();
    let root = d.path().join("tracks");
    let (with, _) = Index::compile_root(&root, &tracks);
    let plain = Index::compile(&tracks);
    assert!(plain.candidates.is_empty() && plain.candidate_contexts.is_empty(), "compile(tracks) reads no file");
    assert_eq!(with.routes, plain.routes);
    assert_eq!(with.generation, plain.generation, "candidates are not part of the generation");
    assert_eq!(with.contexts, plain.contexts, "the routes' net is untouched");

    let cs = with.candidates("acme-billing");
    assert_eq!(cs.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["run-bbbbbb"], "never a route's id");
    assert_eq!((cs[0].support, cs[0].s_min, cs[0].review), (2, 3, false));
    assert!(with.candidate("acme-billing", "run-bbbbbb").is_some());
    assert!(with.candidate("acme-billing", "run-aaaaaa").is_none());
    assert!(with.candidates("elsewhere").is_empty() && with.candidate_contexts("elsewhere").is_empty());

    // Their own net and digest, apart from the routes'.
    let net = with.candidate_contexts("acme-billing");
    assert_eq!(*net, CtxNet::of_candidates(cs));
    assert_ne!(net.digest, with.contexts("acme-billing").digest);
    let ruff = net.alphabet.iter().position(|it| *it == act("exec ruff check")).unwrap() as u32;
    let edit = net.alphabet.iter().position(|it| *it == CtxItem::Edit { glob: "acme/*.py".into() }).unwrap() as u32;
    assert_eq!(net.walk(&[(edit, 1), (ruff, 0)]).into_iter().collect::<Vec<_>>(), ["run-bbbbbb"]);

    // The fast path: the candidate's words and globs, with the routes'.
    let t = with.triggers("acme-billing");
    assert!(t.words.contains(&"ruff".to_string()) && t.words.contains(&"make".to_string()), "{t:?}");
    assert!(t.edit_globs.contains(&"acme/*.py".to_string()), "{t:?}");
    assert!(!plain.triggers("acme-billing").words.contains(&"ruff".to_string()));
    assert!(t.call_might_match(r#"/r Bash {"command":"ruff check acme"}"#));
    assert!(!t.call_might_match(r#"/r Bash {"command":"ls -la"}"#), "an unrelated call is still skipped");
}

/// The same net over the same items, as routes or as candidates, never shares a digest: a
/// session's two context states can never be confused.
#[test]
fn a_candidate_net_never_shares_a_digest_with_a_route_net() {
    let t = track();
    let routes = CtxNet::of(t.routes.iter());
    let c = candidates::Candidate {
        id: "run-aaaaaa".into(),
        trigger: t.routes[0].trigger.clone(),
        step: t.routes[0].step.clone(),
        support: 2,
        s_min: 3,
        days: 1,
        last_seen: None,
        review: false,
    };
    let cands = CtxNet::of_candidates([&c]);
    assert_eq!(routes.alphabet, cands.alphabet);
    assert_ne!(routes.digest, cands.digest);
}

#[test]
fn the_index_is_deterministic_and_an_older_one_loads_with_none() {
    let (d, tracks) = setup();
    let root = d.path().join("tracks");
    let (a, _) = Index::compile_root(&root, &tracks);
    let (b, _) = Index::compile_root(&root, &tracks);
    let (pa, pb) = (d.path().join("a.json"), d.path().join("b.json"));
    a.write_atomic(&pa).unwrap();
    b.write_atomic(&pb).unwrap();
    assert_eq!(std::fs::read(&pa).unwrap(), std::fs::read(&pb).unwrap());
    assert_eq!(Index::load(&pa), a, "round-trips");

    // An index written before candidates: no field, empty maps.
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&pa).unwrap()).unwrap();
    let o = v.as_object_mut().unwrap();
    assert!(o.remove("candidates").is_some() && o.remove("candidate_contexts").is_some());
    std::fs::write(&pa, v.to_string()).unwrap();
    let old = Index::try_load(&pa).expect("an older index loads");
    assert!(old.candidates.is_empty() && old.candidate_contexts.is_empty());
    assert!(old.candidates("acme-billing").is_empty());
    assert_eq!(old.routes, a.routes);
}
