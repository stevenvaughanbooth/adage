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

use adage_eval::conventions::{self, RoutesReport};
use adage_eval::dataset::Dataset;
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

/// Offline output is deterministic, so a floor sits just under the measured value.
const EPS: f64 = 0.02;

#[test]
fn demo_round2_gates() {
    // Measured: precision 0.7571 overall (0.7424 before the command-pattern algebra, 0.729
    // before the second dead end); lookup 0.6667, linked files 0.8333 (0.75: the trigger now
    // fires when an agent runs the lookup script without its interpreter, 12 directives, not 8),
    // dead end 0.7778; recall 0.913 (0.7826); finish-line accuracy 1.0 (4 obligations). Round 2
    // is leaky by task (it trains on round-1 sessions of the tasks it tests), and the report
    // must say so.
    let r = eval("demo-haiku45", Some("round2"));
    assert_eq!(r.scored_sessions, 18);
    at_least("overall precision", r.overall.precision, 0.7571 - EPS);
    at_least("lookup precision", precision(&r, "lookup"), 0.6667 - EPS);
    at_least("linked-files precision", precision(&r, "linked-files"), 0.8333 - EPS);
    at_least("dead-end precision", precision(&r, "dead-end"), 0.7778 - EPS);
    at_least("recall", r.recall["all"].recall, 0.913 - EPS);
    at_least("finish-line accuracy", r.finish_line.accuracy, 1.0 - EPS);
    assert_safe(&r);
    assert!(r.leaky && r.task_overlap == 6, "round2 must be flagged leaky: {} tasks", r.task_overlap);
    assert!(r.split_note.as_deref().is_some_and(|n| n.contains("Leaky")));
}

#[test]
fn demo_round2_learns_what_the_live_learner_learned() {
    // The offline learner, fed the curated transcripts of the same 54 sessions, must learn the
    // routes the live round-2 index held (ids, aspects). Measured: identical routes except two
    // deliberate changes since: the dead end's avoided runners and evidence (the signature-keyed
    // miner), and the exit condition dead ends and linked files now carry (Design W2). Before
    // them, identical index bytes (generation 8dd5ec01ed76c4bd).
    //
    // One route is added, deliberately: dead ends are now one per (working command, signature),
    // so the second signature before `make test` (`python -m pytest`, "command not found", 4 of
    // 5 sessions) is a route of its own, `dead-end-21cb2c`. The live route keeps its id and aspect
    // (`dead-end-d41b1e`, the most common signature). Nothing else may differ.
    let r = eval("demo-haiku45", Some("round2"));
    let live: adage_tracks::Index =
        serde_json::from_slice(&std::fs::read(datasets().join("demo-haiku45/recorded/index-round2.json")).unwrap())
            .unwrap();
    let mut want: Vec<(String, String)> =
        live.routes.iter().map(|ir| (ir.route.id.clone(), format!("{:?}", ir.route.aspect).to_lowercase())).collect();
    want.push(("dead-end-21cb2c".into(), "red".into()));
    let mut got: Vec<(String, String)> = r.routes.iter().map(|x| (x.id.clone(), x.aspect.clone())).collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
    let second = r.routes.iter().find(|x| x.id == "dead-end-21cb2c").unwrap();
    assert_eq!((second.step.as_str(), second.support), ("avoid python -m pytest; instead make test", 4));
}

#[test]
fn demo_round1_gates() {
    // Measured: precision 0.6667 overall (0.6786 before the command-pattern algebra: two more
    // linked-files directives, one of them not followed); lookup 0.5; recall 0.5833 (0.5556).
    // No task on both sides.
    let r = eval("demo-haiku45", Some("round1"));
    at_least("overall precision", r.overall.precision, 0.6667 - EPS);
    at_least("lookup precision", precision(&r, "lookup"), 0.5 - EPS);
    at_least("recall", r.recall["all"].recall, 0.5833 - EPS);
    assert_safe(&r);
    assert!(!r.leaky && r.task_overlap == 0);
}

#[test]
fn replay_reproduces_the_live_round2_decisions() {
    // Measured: 36/36 sessions decide identically; 35/36 raise the same hook events (in the other,
    // a forked skill's tool calls fired hooks but live in a subagent transcript).
    //
    // The recorded hooks matched commands by exact template key; replay compares under that
    // matching (`--exact-keys`, `adage_runtime::with_exact_key_matching`). Under the current
    // matching (the command-pattern algebra), exactly four holdout sessions differ, each by one
    // would-have linked-files directive the live hook did not make: the trigger was mined from
    // `python3 scripts/wi-lookup {id}` and the agent ran `./scripts/wi-lookup W-…`, the same script.
    let dir = datasets().join("demo-haiku45");
    let f = fidelity::run_with(&dir, "round2", true).unwrap();
    let (mut sessions, mut decisions, mut events) = (0, 0, 0);
    for a in f.by_arm.values() {
        sessions += a.sessions;
        decisions += a.decisions_match;
        events += a.events_match;
    }
    assert_eq!(sessions, 36);
    assert_eq!(decisions, sessions, "{:#?}", f.mismatches);
    assert!(events + 2 >= sessions, "{:#?}", f.event_mismatches);

    let now = fidelity::run(&dir, "round2").unwrap();
    let mut differ: Vec<&str> = now.mismatches.iter().map(|m| m.split(':').next().unwrap()).collect();
    differ.sort();
    assert_eq!(
        differ,
        ["eval2-W-2042-r1-holdout", "eval2-W-2043-r2-holdout", "eval2-W-2046-r1-holdout", "eval2-W-2046-r3-holdout"],
        "{:#?}",
        now.mismatches
    );
    for m in &now.mismatches {
        let (replay, live) = m.split_once(" vs live ").unwrap();
        assert_eq!(replay.matches("read-linked-files").count(), 1 + live.matches("read-linked-files").count(), "{m}");
    }
}

#[test]
fn synthetic_gates() {
    // Measured: precision 0.934 overall; lookup 0.778, linked files 1.0, dead end 1.0; recall 1.0
    // (0.974 while there was one dead end per working command: `unittest` fails with another
    // signature, now a route of its own); dead-end accuracy 1.0; finish-line accuracy 1.0.
    let r = eval("synthetic", None);
    at_least("overall precision", r.overall.precision, 0.9342 - EPS);
    at_least("lookup precision", precision(&r, "lookup"), 0.7778 - EPS);
    at_least("linked-files precision", precision(&r, "linked-files"), 1.0 - EPS);
    at_least("dead-end precision", precision(&r, "dead-end"), 1.0 - EPS);
    at_least("recall", r.recall["all"].recall, 1.0 - EPS);
    at_least("dead-end accuracy", r.dead_ends.accuracy, 1.0 - EPS);
    at_least("finish-line accuracy", r.finish_line.accuracy, 1.0 - EPS);
    assert_safe(&r);
}

fn routes(name: &str, split: Option<&str>) -> RoutesReport {
    let opts = Options { split: split.map(str::to_string), ..Default::default() };
    conventions::run(&datasets().join(name), &opts).unwrap_or_else(|e| panic!("routes {name}: {e:#}"))
}

#[test]
fn learned_routes_match_the_sandbox_conventions() {
    // Route precision (learned routes that match a convention in conventions.json) and route
    // recall (conventions some learned route matches). Measured: 1.0 / 1.0 on synthetic, demo
    // round 2, demo group:task:3:0 and adversarial; demo round 1 learns no dead end from its 18
    // training sessions (nor did the live round-1 index), so recall is 0.75 there.
    for (ds, split, p, rc) in [
        ("synthetic", None, 1.0, 1.0),
        ("demo-haiku45", Some("round2"), 1.0, 1.0),
        ("demo-haiku45", Some("group:task:3:0"), 1.0, 1.0),
        ("demo-haiku45", Some("round1"), 1.0, 0.75),
        ("adversarial", None, 1.0, 1.0),
    ] {
        let r = routes(ds, split);
        let what = format!("{ds} {}", r.split);
        at_least(&format!("{what}: route precision"), r.precision, p - EPS);
        at_least(&format!("{what}: route recall"), r.recall, rc - EPS);
        assert!(r.false_routes.is_empty(), "{what}: false routes {:?}", r.false_routes);
    }
    let r1 = routes("demo-haiku45", Some("round1"));
    assert_eq!(r1.missed_conventions, ["tests-through-make"]);
}

#[test]
fn task_grouped_and_temporal_splits_do_not_leak() {
    // Every fold of a task-grouped split, and the temporal split, has no task on both sides;
    // the folds partition the sessions. The session-hash split does leak on the demo (11 tasks
    // on both sides of hash:0.3), and must say so.
    for ds in ["demo-haiku45", "synthetic"] {
        let d = Dataset::load(&datasets().join(ds)).unwrap();
        let mut tested = Vec::new();
        for fold in 0..3 {
            let s = d.split(&format!("group:task:3:{fold}")).unwrap();
            assert_eq!(d.task_overlap(&s), Vec::<String>::new(), "{ds} fold {fold}");
            assert_eq!(s.train.len() + s.test.len(), d.manifest.sessions.len());
            tested.extend(s.test);
        }
        tested.sort();
        let mut all: Vec<String> = d.manifest.sessions.iter().map(|s| s.id.clone()).collect();
        all.sort();
        assert_eq!(tested, all, "{ds}: the folds' test sets partition the sessions");
        let t = d.split("temporal").unwrap();
        assert!(d.task_overlap(&t).is_empty(), "{ds} temporal");
        assert!(!t.train.is_empty() && !t.test.is_empty());
    }
    // Temporal on the demo: train is strictly earlier. Round-2 sessions are never in it.
    let d = Dataset::load(&datasets().join("demo-haiku45")).unwrap();
    let t = d.split("temporal").unwrap();
    assert!(t.train.iter().all(|id| d.session(id).unwrap().round == Some(1)));
    let r = eval("demo-haiku45", Some("group:task:3:0"));
    assert_eq!((r.task_overlap, r.leaky), (0, false));
    assert!(adage_eval::render_text(&r).contains("task overlap between train and test: 0 tasks\n"));
    let h = eval("demo-haiku45", Some("hash:0.3"));
    assert!(h.leaky && h.task_overlap == 11, "hash:0.3 overlap {}", h.task_overlap);
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
fn fast_path_changes_no_decision() {
    // The hook's no-match fast path (adage_runtime::fast) skips events before any decision. On
    // every event of every dataset, a skipped event's full decision is empty, and deciding the
    // hook's way gives the same decisions and the same decision state as deciding every event.
    // Measured skips: round 2 131/294, round 1 292/405, hash:0.3 437/634, synthetic 365/492,
    // adversarial 95/210. The recorded live index predates stored triggers: they are derived.
    let recorded = Options {
        split: Some("round2".into()),
        index: Some(datasets().join("demo-haiku45/recorded/index-round2.json")),
        ..Default::default()
    };
    let runs = [
        eval("demo-haiku45", Some("round2")),
        eval("demo-haiku45", Some("round1")),
        eval("demo-haiku45", Some("hash:0.3")),
        eval("synthetic", None),
        eval("adversarial", None),
        run(&datasets().join("demo-haiku45"), &recorded).unwrap(),
    ];
    for r in &runs {
        let f = &r.fast_path;
        assert!(f.mismatches.is_empty(), "{} {}: {:#?}", r.dataset, r.split, f.mismatches);
        assert!(f.skipped > 0 && f.skipped < f.events, "{} {}: {} of {}", r.dataset, r.split, f.skipped, f.events);
    }
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
    // The same for the text report (with its intervals) and the routes report.
    let (a, b) = (eval("demo-haiku45", Some("temporal")), eval("demo-haiku45", Some("temporal")));
    assert_eq!(adage_eval::render_text(&a), adage_eval::render_text(&b));
    let (a, b) = (routes("demo-haiku45", Some("round2")), routes("demo-haiku45", Some("round2")));
    assert_eq!(serde_json::to_vec(&a).unwrap(), serde_json::to_vec(&b).unwrap());
    assert_eq!(conventions::render_text(&a), conventions::render_text(&b));
    let text = serde_json::to_string(&a).unwrap();
    assert!(!text.contains(&*datasets().to_string_lossy()), "dataset paths leak into the routes report");
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
