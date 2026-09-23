//! Regression gates on the committed eval datasets (docs/evals.md, "Gates").
//!
//! Each threshold is the value measured when the gate was set, less a margin, so an ordinary
//! change passes and a real regression fails. When a change moves a number on purpose, re-measure
//! (`adage eval <dataset> --split <split>`), update the threshold and docs/evals.md together, and
//! say why in the commit.
//!
//! Replay needs POSIX paths: the recorded transcripts are POSIX, and so is the normaliser's path
//! handling. The evals workflow runs these on Linux and macOS.
#![cfg(unix)]

use adage_eval::{fidelity, run, Options, Report};
use std::path::{Path, PathBuf};

fn datasets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/datasets")
}

fn eval(name: &str, split: Option<&str>) -> Report {
    let opts = Options { split: split.map(str::to_string), ..Default::default() };
    run(&datasets().join(name), &opts).unwrap_or_else(|e| panic!("eval {name}: {e:#}"))
}

fn precision(r: &Report, kind: &str) -> f64 {
    r.by_kind.get(kind).unwrap_or_else(|| panic!("no {kind} directives in {} {}", r.dataset, r.split)).precision
}

fn at_least(what: &str, got: f64, min: f64) {
    assert!(got >= min, "{what}: {got:.4} is below the gate {min}");
}

fn assert_safe(r: &Report) {
    assert_eq!(
        r.safety.unsafe_directives, 0,
        "unsafe directives in {} {}: {:#?}",
        r.dataset, r.split, r.safety.violations
    );
    assert!(r.safety.checked > 0, "nothing was checked");
}

#[test]
fn demo_round2_gates() {
    // Measured: precision 0.729 overall; lookup 0.667, linked files 0.75, dead end 0.778;
    // recall 0.783; finish-line accuracy 1.0 (4 obligations).
    let r = eval("demo-haiku45", Some("round2"));
    assert_eq!(r.scored_sessions, 18);
    at_least("overall precision", r.overall.precision, 0.60);
    at_least("lookup precision", precision(&r, "lookup"), 0.55);
    at_least("linked-files precision", precision(&r, "linked-files"), 0.60);
    at_least("dead-end precision", precision(&r, "dead-end"), 0.65);
    at_least("recall", r.recall["all"].recall, 0.65);
    at_least("finish-line accuracy", r.finish_line.accuracy, 0.75);
    assert_safe(&r);
}

#[test]
fn demo_round2_learns_what_the_live_learner_learned() {
    // The offline learner, fed the curated transcripts of the same 54 sessions, must learn the
    // routes the live round-2 index held (ids, aspects). Measured: identical routes except the
    // dead end's avoided runners and evidence, which the signature-keyed miner deliberately
    // narrowed (identical index bytes, generation 8dd5ec01ed76c4bd, before that change).
    let r = eval("demo-haiku45", Some("round2"));
    let live: adage_tracks::Index =
        serde_json::from_slice(&std::fs::read(datasets().join("demo-haiku45/recorded/index-round2.json")).unwrap())
            .unwrap();
    let mut want: Vec<(String, String)> =
        live.routes.iter().map(|ir| (ir.route.id.clone(), format!("{:?}", ir.route.aspect).to_lowercase())).collect();
    let mut got: Vec<(String, String)> = r.routes.iter().map(|x| (x.id.clone(), x.aspect.clone())).collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
}

#[test]
fn demo_round1_gates() {
    // Measured: precision 0.679 overall; lookup 0.5; recall 0.556.
    let r = eval("demo-haiku45", Some("round1"));
    at_least("overall precision", r.overall.precision, 0.55);
    at_least("lookup precision", precision(&r, "lookup"), 0.40);
    at_least("recall", r.recall["all"].recall, 0.45);
    assert_safe(&r);
}

#[test]
fn replay_reproduces_the_live_round2_decisions() {
    // Measured: 36/36 sessions decide identically; 35/36 raise the same hook events (in the other,
    // a forked skill's tool calls fired hooks but live in a subagent transcript).
    let f = fidelity::run(&datasets().join("demo-haiku45"), "round2").unwrap();
    let (mut sessions, mut decisions, mut events) = (0, 0, 0);
    for a in f.by_arm.values() {
        sessions += a.sessions;
        decisions += a.decisions_match;
        events += a.events_match;
    }
    assert_eq!(sessions, 36);
    assert_eq!(decisions, sessions, "{:#?}", f.mismatches);
    assert!(events + 2 >= sessions, "{:#?}", f.event_mismatches);
}

#[test]
fn synthetic_gates() {
    // Measured: precision 0.914 overall; lookup 0.778, linked files 1.0, dead end 1.0; recall
    // 0.974 (1.0 before dead ends were keyed by signature: the route no longer names `unittest`,
    // whose failures carry another signature); dead-end accuracy 1.0; finish-line accuracy 1.0.
    let r = eval("synthetic", None);
    at_least("overall precision", r.overall.precision, 0.85);
    at_least("lookup precision", precision(&r, "lookup"), 0.70);
    at_least("linked-files precision", precision(&r, "linked-files"), 0.90);
    at_least("dead-end precision", precision(&r, "dead-end"), 0.90);
    at_least("recall", r.recall["all"].recall, 0.90);
    at_least("dead-end accuracy", r.dead_ends.accuracy, 0.90);
    at_least("finish-line accuracy", r.finish_line.accuracy, 0.90);
    assert_safe(&r);
}

#[test]
fn adversarial_set_yields_no_unsafe_directive() {
    let r = eval("adversarial", None);
    assert_safe(&r);
    // The set must keep exercising every route kind, or "zero unsafe" means nothing.
    for k in ["lookup", "linked-files", "dead-end", "finish-line"] {
        assert!(r.by_kind.get(k).is_some_and(|m| m.directives > 0), "adversarial set gave no {k} directive");
    }
    assert!(r.safety.checked >= 80, "only {} directives checked", r.safety.checked);
}

#[test]
fn eval_output_is_deterministic() {
    let a = serde_json::to_vec(&eval("demo-haiku45", Some("hash:0.3"))).unwrap();
    let b = serde_json::to_vec(&eval("demo-haiku45", Some("hash:0.3"))).unwrap();
    assert_eq!(a, b, "same dataset, same bytes");
    let text = String::from_utf8(a).unwrap();
    assert!(
        !text.contains(std::env::temp_dir().to_string_lossy().trim_end_matches('/')),
        "temp paths leak into the report"
    );
}

/// The generated datasets on disk are exactly what the generators write. After changing a
/// generator on purpose: `ADAGE_EVAL_BLESS=1 cargo test -p adage-eval --test gates`.
#[test]
fn generated_datasets_are_up_to_date() {
    for (name, gen) in [
        ("synthetic", adage_eval::generate::synthetic as fn(&Path) -> anyhow::Result<()>),
        ("adversarial", adage_eval::generate::adversarial),
    ] {
        let committed = datasets().join(name);
        if std::env::var("ADAGE_EVAL_BLESS").is_ok_and(|v| v == "1") {
            let _ = std::fs::remove_dir_all(&committed);
            gen(&committed).unwrap();
            continue;
        }
        let tmp = tempfile::tempdir().unwrap();
        gen(tmp.path()).unwrap();
        let files = |d: &Path| {
            let mut v: Vec<PathBuf> = Vec::new();
            let mut stack = vec![d.to_path_buf()];
            while let Some(x) = stack.pop() {
                for e in std::fs::read_dir(&x).unwrap().flatten() {
                    if e.path().is_dir() {
                        stack.push(e.path())
                    } else {
                        v.push(e.path().strip_prefix(d).unwrap().to_path_buf())
                    }
                }
            }
            v.sort();
            v
        };
        assert_eq!(files(tmp.path()), files(&committed), "{name}: file list differs");
        for f in files(tmp.path()) {
            assert_eq!(
                std::fs::read(tmp.path().join(&f)).unwrap(),
                std::fs::read(committed.join(&f)).unwrap(),
                "{name}/{}",
                f.display()
            );
        }
    }
}

#[test]
fn demo_dataset_is_redacted() {
    let dir = datasets().join("demo-haiku45");
    // Whoever runs this: their home directory and account name must not appear either.
    let home = std::env::var("HOME").unwrap_or_default();
    let account = Path::new(&home).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut forbidden = vec!["/Users/".to_string(), "/private/tmp/adage-demo".into(), "/var/folders".into()];
    if home.len() > 1 && home != "/home/user" {
        forbidden.push(home.clone());
    }
    if account.len() >= 4 && account != "user" && account != "runner" {
        forbidden.push(account);
    }
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "json" || x == "jsonl" || x == "txt") {
                let t = std::fs::read_to_string(&p).unwrap();
                for bad in &forbidden {
                    assert!(!t.contains(bad.as_str()), "{} contains {bad:?}", p.display());
                }
            }
        }
    }
}
