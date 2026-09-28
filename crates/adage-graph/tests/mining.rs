//! G2 mining: planted conventions are found, the result does not depend on thread count or input
//! order, and the complexity bound (Track G §9) holds at scale.
//!
//! The scale test is ignored by default (it is a release-mode measurement):
//! `cargo test -p adage-graph --release --test mining -- --ignored --nocapture`.
mod support;

use adage_graph::select::{self, matches_convention, Params};
use std::time::Instant;

fn params() -> Params {
    Params::demo()
}

#[test]
fn planted_conventions_are_found() {
    let sessions = support::synthetic_sessions(120, 40, 7);
    let l = select::learn(&sessions, &params());
    let has = |kind: &str, cmd: &str, avoided: &[&str], glob: Option<&str>| {
        let avoided: Vec<String> = avoided.iter().map(|s| s.to_string()).collect();
        l.reflexes.iter().any(|r| matches_convention(r, kind, cmd, &avoided, glob))
    };
    let why: Vec<&str> = l.reflexes.iter().map(|r| r.why.as_str()).collect();
    assert!(has("lookup", "./scripts/wi-lookup {item}", &[], None), "{why:#?}");
    assert!(has("linked_files", "./scripts/wi-lookup {item}", &[], None), "{why:#?}");
    assert!(has("dead_end", "make test", &["python3 -m pytest"], None), "{why:#?}");
    assert!(has("finish_line", "make gen", &[], Some("registry/*.csv")), "{why:#?}");
    // Noise commands are never directed.
    for noise in ["ls", "git status", "pwd", "make bench", "echo done"] {
        assert!(!has("decoy", noise, &[], None), "{noise} directed: {why:#?}");
    }
    for r in &l.reflexes {
        assert!(r.gain > 0.0 && r.examples.len() <= 3 && !r.why.is_empty());
    }
}

#[test]
fn thread_count_and_input_order_change_nothing() {
    let sessions = support::synthetic_sessions(80, 30, 11);
    let one = select::learn(&sessions, &params());
    let mut rev = sessions.clone();
    rev.reverse();
    let four = select::learn(&rev, &params().with_threads(4));
    assert_eq!(serde_json::to_string(&one).unwrap(), serde_json::to_string(&four).unwrap());
    // A dataset, when present.
    if let Some(ds) = support::dataset("synthetic") {
        let a = select::learn(&ds, &params());
        let mut r = ds.clone();
        r.rotate_left(ds.len() / 3);
        let b = select::learn(&r, &params().with_threads(3));
        assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
    }
}

#[test]
fn the_budget_truncates_deterministically() {
    let sessions = support::synthetic_sessions(60, 30, 3);
    let mut p = params();
    p.mine.budget = 50;
    let a = select::learn(&sessions, &p);
    let b = select::learn(&sessions, &p.clone().with_threads(4));
    assert!(a.truncated && a.expansions <= 50);
    assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
}

/// 2,000 sessions × 200 events: the full pipeline (prototypes, mining, scoring, selection).
/// Target (Track G §9): a full mine ≤ 10 s in release mode.
#[test]
#[ignore]
fn scale_2000_sessions_of_200_events() {
    let t = Instant::now();
    let n_s = std::env::var("SCALE_SESSIONS").ok().and_then(|x| x.parse().ok()).unwrap_or(2000);
    let n_e = std::env::var("SCALE_EVENTS").ok().and_then(|x| x.parse().ok()).unwrap_or(200);
    let sessions = support::synthetic_sessions(n_s, n_e, 42);
    let n: usize = sessions.iter().map(|s| s.events.len()).sum();
    let gen = t.elapsed();
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(1).min(8);
    let ths: Vec<usize> = match std::env::var("SCALE_THREADS") {
        Ok(x) => vec![x.parse().unwrap()],
        Err(_) => vec![1, threads],
    };
    for th in ths {
        let t = Instant::now();
        let l = select::learn(&sessions, &Params::default().with_threads(th));
        eprintln!(
            "scale: {} sessions, {n} events (generated in {gen:?}); threads {th}: total {:?} ({:?}); targets {}, expansions {}{}, frequent {}, closed {}, gated {}, selected {}",
            sessions.len(),
            t.elapsed(),
            l.timings,
            l.targets,
            l.expansions,
            if l.truncated { " (truncated)" } else { "" },
            l.frequent,
            l.closed,
            l.gated,
            l.reflexes.len()
        );
        assert!(!l.reflexes.is_empty());
    }
}

/// The largest committed dataset, timed.
#[test]
fn largest_dataset_is_mined() {
    let Some(ds) = support::dataset("demo-haiku45") else { return };
    let t = Instant::now();
    let l = select::learn(&ds, &params());
    eprintln!(
        "demo-haiku45: {} sessions in {:?} ({:?}), selected {}",
        ds.len(),
        t.elapsed(),
        l.timings,
        l.reflexes.len()
    );
    assert!(!l.truncated);
    assert!(!l.reflexes.is_empty());
}
