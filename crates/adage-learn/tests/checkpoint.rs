//! Warm computation must produce exactly the same artifacts as full computation across updates.
use adage_ingest::corpus::{Limits, Record};
use adage_learn::{
    corpus::{self, Mode},
    store::{Files, Store},
    LearnConfig,
};
use adage_trace::Session;
use std::collections::BTreeMap;
use std::path::Path;

const REPO: &str = "canonical-routine";

fn sessions(n: usize) -> Vec<Session> {
    (0..n)
        .map(|i| {
            let mut s: Session = serde_json::from_str(include_str!("fixtures/canonical-routine.json")).unwrap();
            s.id = format!("s{i}");
            s.identity.task_id = Some(format!("TASK-{i}"));
            s
        })
        .collect()
}

fn replace(store: &Files, sessions: &[Session]) {
    let body: String =
        sessions.iter().map(|s| format!("{}\n", serde_json::to_string(&Record::new(s.clone())).unwrap())).collect();
    assert!(corpus::import(body.as_bytes(), REPO, Mode::Replace, Limits::default(), store).unwrap().committed);
}

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut todo = vec![root.to_path_buf()];
    let mut result = BTreeMap::new();
    while let Some(path) = todo.pop() {
        for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                result.insert(p.strip_prefix(root).unwrap().to_string_lossy().into(), std::fs::read(&p).unwrap());
            }
        }
    }
    result
}

#[test]
fn full_and_warm_artifacts_agree_after_reordering_corrections_additions_removals_and_tuning() {
    let dir = tempfile::tempdir().unwrap();
    let cached = Files::new(&dir.path().join("cached/tracks"), &dir.path().join("cached/index.json"));
    let full = Files::new(&dir.path().join("full/tracks"), &dir.path().join("full/index.json"));
    let mut evidence = sessions(5);
    let mut cfg = LearnConfig::default();
    for (case, hit) in [
        ("initial", false),
        ("unchanged", true),
        ("reordered", true),
        ("corrected", false),
        ("added", false),
        ("removed", false),
        ("parameters", false),
        ("judgment", true),
    ] {
        match case {
            "reordered" => evidence.reverse(),
            "corrected" => evidence[0].outcome = Some(false),
            "added" => evidence.push(sessions(6).pop().unwrap()),
            "removed" => {
                evidence.pop();
            }
            "parameters" => cfg.min_support = 6,
            "judgment" => cfg.fire_lower_bound = 0.999,
            _ => {}
        }
        replace(&cached, &evidence);
        replace(&full, &evidence);
        let warm = corpus::learn(REPO, None, &cached, &cfg).unwrap();
        let cold_cfg = LearnConfig { computation_cache: false, ..cfg.clone() };
        let cold = corpus::learn(REPO, None, &full, &cold_cfg).unwrap();
        assert_eq!(warm.repos[REPO].engine.mining_cache_hit, hit, "{case}");
        assert!(!cold.repos[REPO].engine.mining_cache_hit);
        assert_eq!(
            std::fs::read(&cached.index_path).unwrap(),
            std::fs::read(&full.index_path).unwrap(),
            "index: {case}"
        );
        assert_eq!(files(&cached.tracks_root), files(&full.tracks_root), "tracks/why/reviews: {case}");
    }
    let before = std::fs::read(&cached.index_path).unwrap();
    cached.write_mining_checkpoint(REPO, b"{broken");
    let repaired = corpus::learn(REPO, None, &cached, &cfg).unwrap();
    assert!(!repaired.repos[REPO].engine.mining_cache_hit);
    assert_eq!(std::fs::read(&cached.index_path).unwrap(), before);
    assert!(corpus::learn(REPO, None, &cached, &cfg).unwrap().repos[REPO].engine.mining_cache_hit);
}

#[test]
fn warm_mining_still_reconciles_human_decisions_and_dry_plans_write_no_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let store = Files::new(&dir.path().join("tracks"), &dir.path().join("index.json"));
    let evidence = sessions(5);
    let cfg = LearnConfig::default();
    adage_learn::plan(evidence.clone(), None, &store, &cfg).unwrap();
    assert!(store.mining_checkpoint(REPO).is_none());
    replace(&store, &evidence);
    corpus::learn(REPO, None, &store, &cfg).unwrap();
    let name = format!("{REPO}/learned");
    let track = store.track(&name).unwrap();
    let route = track.routes.iter().find(|r| r.evidence.execution.is_some()).unwrap();
    let reviews = adage_tracks::review::Reviews {
        routes: BTreeMap::from([(
            route.id.clone(),
            adage_tracks::review::Review {
                decision: adage_tracks::review::Decision::Extinguished,
                on: "2026-10-03".into(),
                by: None,
                basis: adage_tracks::review::basis(route, 0, 0),
                via: None,
            },
        )]),
        ..Default::default()
    };
    adage_tracks::review::save(&store.tracks_root.join(&name), &reviews).unwrap();
    let report = corpus::learn(REPO, None, &store, &cfg).unwrap();
    assert!(report.repos[REPO].engine.mining_cache_hit);
    assert_eq!(
        store.track(&name).unwrap().routes.iter().find(|r| r.id == route.id).unwrap().state,
        adage_tracks::RouteState::Extinguished
    );
}
