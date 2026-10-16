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
use adage_eval::{fidelity, run, InstinctMode, Options, Report};
use std::path::{Path, PathBuf};

fn datasets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/datasets")
}

fn eval(name: &str, split: Option<&str>) -> Report {
    let opts = Options { split: split.map(str::to_string), ..Default::default() };
    run(&datasets().join(name), &opts).unwrap_or_else(|e| panic!("eval {name}: {e:#}"))
}

/// Directives by delivery shape and trigger (`adage_eval::score::point`).
const LOOKUP: &str = "run@prompt";
const LINKED: &str = "read-batch@after-command";
const AVOID: &str = "avoid@session-start";
const OBLIGATION: &str = "obligation@after-edit";

fn precision(r: &Report, shape: &str) -> f64 {
    r.by_shape.get(shape).unwrap_or_else(|| panic!("no {shape} directives in {} {}", r.dataset, r.split)).precision
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
    // dead end 0.7778; recall 0.913 (0.7826); finish-line accuracy 1.0 (4 obligations). With the
    // graph engine as the only learner (Track G G5): precision 0.7568 overall (the sequence
    // `make gen then make test then make ci`, 3 of 4 directives followed), every shape's
    // precision unchanged, recall 0.9348. Round 2 is leaky by task (it trains on round-1 sessions of the tasks it tests), and the report
    // must say so.
    let r = eval("demo-haiku45", Some("round2"));
    assert_eq!(r.scored_sessions, 18);
    at_least("overall precision", r.overall.precision, 0.7568 - EPS);
    at_least("lookup precision", precision(&r, LOOKUP), 0.6667 - EPS);
    at_least("linked-files precision", precision(&r, LINKED), 0.8333 - EPS);
    at_least("dead-end precision", precision(&r, AVOID), 0.7778 - EPS);
    at_least("recall", r.recall["all"].recall, 0.9348 - EPS);
    at_least("finish-line accuracy", r.finish_line.accuracy, 1.0 - EPS);
    assert_safe(&r);
    assert!(r.leaky && r.task_overlap == 6, "round2 must be flagged leaky: {} tasks", r.task_overlap);
    assert!(r.split_note.as_deref().is_some_and(|n| n.contains("Leaky")));
}

#[test]
fn demo_round2_learns_what_the_live_learner_learned() {
    // The offline learner, fed the curated transcripts of the same 54 sessions, must learn the
    // routes the live round-2 index held (ids, strengths). Measured: identical routes except two
    // deliberate changes since: the dead end's avoided runners and evidence (keyed by
    // signature), and the exit condition dead ends and linked files now carry (Design W2). Before
    // them, identical index bytes (generation 8dd5ec01ed76c4bd).
    //
    // One route is added, deliberately: dead ends are now one per (working command, signature),
    // so the second signature before `make test` (`python -m pytest`, "command not found", 4 of
    // 5 sessions) is a route of its own, `dead-end-21cb2c`. The live route keeps its id and strength
    // (`dead-end-d41b1e`, the most common signature).
    //
    // And one since the graph engine became the only learner (Track G G5): the sessions that ran
    // `make gen` after a schema edit and then `make test` and `make ci` did so one right after
    // another (`adage_learn::engine`'s together rule), so beside the obligation to regenerate the
    // sequence `make gen then make test then make ci` is learned, prime.
    // Predictive execution also learns source-edit → test-edit → make test. Its full trigger
    // has an explicit execution contract in conventions.json; it must remain silent without
    // an execution approval, and is not evidence of advisory benefit.
    let r = eval("demo-haiku45", Some("round2"));
    let live: adage_tracks::Index =
        serde_json::from_slice(&std::fs::read(datasets().join("demo-haiku45/recorded/index-round2.json")).unwrap())
            .unwrap();
    let mut want: Vec<(String, String)> =
        live.routes.iter().map(|ir| (ir.route.id.clone(), format!("{:?}", ir.route.strength).to_lowercase())).collect();
    want.push(("dead-end-21cb2c".into(), "inhibit".into()));
    want.push(("sequence-a36be6".into(), "prime".into()));
    want.push(("run-c1ba29".into(), "prime".into()));
    let mut got: Vec<(String, String)> = r.routes.iter().map(|x| (x.id.clone(), x.strength.clone())).collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
    let second = r.routes.iter().find(|x| x.id == "dead-end-21cb2c").unwrap();
    assert_eq!((second.step.as_str(), second.support), ("avoid python -m pytest; instead make test", 4));
    assert!(!r.by_route.contains_key("run-c1ba29"), "an unapproved execution candidate must not be delivered");
    let prepared = adage_eval::prepare(
        &datasets().join("demo-haiku45"),
        &Options { split: Some("round2".into()), ..Default::default() },
    )
    .unwrap();
    let execution = prepared.index.routes.iter().find(|r| r.route.id == "run-c1ba29").unwrap();
    assert!(execution.exec_only());
    let evidence = execution.route.evidence.execution.as_ref().unwrap();
    assert_eq!((evidence.opportunities, evidence.successful, evidence.sessions), (6, 6, 6));
}

#[test]
fn demo_round1_gates() {
    // Measured: precision 0.6667 overall (0.6786 before the command-pattern algebra: two more
    // linked-files directives, one of them not followed); lookup 0.5; recall 0.5833 (0.5556).
    // No task on both sides.
    let r = eval("demo-haiku45", Some("round1"));
    at_least("overall precision", r.overall.precision, 0.6667 - EPS);
    at_least("lookup precision", precision(&r, LOOKUP), 0.5 - EPS);
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
    // matching (`--exact-keys`, `adage_runtime::Policy::exact_keys`). Under the current
    // matching (the command-pattern algebra), exactly four control sessions differ, each by one
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
fn replay_with_the_learned_index_gives_every_recorded_directive() {
    // Track G G5: the recorded decisions came from the removed miners' index. Replayed against
    // the index the graph engine learns from the same round's train sessions, every directive the
    // live hook gave must still arrive, at its event or earlier. Measured, both rounds: 36/36.
    // What differs, precisely:
    // - round 1: 34/36 identical; two control sessions get one more linked-files directive (the
    //   command-pattern algebra, as in the test above); the live `finish-line-1` is today's
    //   `finish-line-e7e809` (same trigger and step, an id from before content-derived ids).
    // - round 2: none identical, because every session also gets the second dead end
    //   (`dead-end-21cb2c`) at its first prompt; sessions that edit a schema get the sequence
    //   `make gen then make test then make ci` beside the obligation (10); four control sessions
    //   get one more linked-files directive; and one (`eval2-W-2045-r2-holdout`, a recorded run id) gets its lookup
    //   and dead end at session start, one event earlier than live.
    let dir = datasets().join("demo-haiku45");
    for (round, identical) in [("round1", 34), ("round2", 0)] {
        let f = fidelity::run_learned(&dir, round, false).unwrap();
        assert_eq!(f.sessions, 36, "{round}");
        assert_eq!(f.superset, 36, "{round}: {:#?}", f.missing);
        assert!(f.identical >= identical, "{round}: {} identical", f.identical);
        for a in &f.added {
            assert!(
                ["read-linked-files", "dead-end-21cb2c", "sequence-a36be6"].iter().any(|id| a.contains(id)),
                "{round}: unexpected {a}"
            );
        }
        assert!(f.earlier.len() <= 1, "{round}: {:#?}", f.earlier);
    }
}

#[test]
fn synthetic_gates() {
    // Measured: precision 0.934 overall; lookup 0.778, linked files 1.0, dead end 1.0; recall 1.0
    // (0.974 while there was one dead end per working command: `unittest` fails with another
    // signature, now a route of its own); dead-end accuracy 1.0; finish-line accuracy 1.0.
    let r = eval("synthetic", None);
    at_least("overall precision", r.overall.precision, 0.9342 - EPS);
    at_least("lookup precision", precision(&r, LOOKUP), 0.7778 - EPS);
    at_least("linked-files precision", precision(&r, LINKED), 1.0 - EPS);
    at_least("dead-end precision", precision(&r, AVOID), 1.0 - EPS);
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
    for k in [LOOKUP, LINKED, AVOID, OBLIGATION] {
        assert!(r.by_shape.get(k).is_some_and(|m| m.directives > 0), "adversarial set gave no {k} directive");
    }
    assert!(r.safety.checked >= 80, "only {} directives checked", r.safety.checked);
}

fn eval_instincts(name: &str, split: Option<&str>, mode: InstinctMode) -> Report {
    let opts = Options { split: split.map(str::to_string), instincts: mode, ..Default::default() };
    run(&datasets().join(name), &opts).unwrap_or_else(|e| panic!("eval {name} {mode:?}: {e:#}"))
}

#[test]
fn demo_instincts_only_gates() {
    // Day 0 (docs/instincts.md): nothing is learned from the train sessions; the index holds only
    // what binds from the sandbox's files (evals/sandboxes/acme-billing), and the test sessions
    // are replayed. Measured on the default split (hash:0.3): 11 directives, 10 correct; dead ends
    // 3 of 4 correct, answering 3 of 4 failed verify runs; finish lines 7 of 7; 0 unsafe. Over
    // every session (split all, steered included): dead ends 26/32, finish lines 22/23.
    let r = eval_instincts("demo-haiku45", None, InstinctMode::Only);
    let i = &r.instincts.as_ref().expect("an instincts report").stats;
    assert!(i.dead_end_correct >= 1, "no correct instinct dead end: {i:#?}");
    assert!(i.finish_line_correct >= 1, "no correct instinct finish line: {i:#?}");
    at_least("instinct precision", i.precision, 0.9091 - EPS);
    at_least("instinct dead-end recall", i.recall, 0.75 - EPS);
    assert_safe(&r);
    assert!(r.routes.iter().all(|x| x.support == 0 && x.opportunities == 0), "nothing may be learned");
    assert!(r.fast_path.mismatches.is_empty(), "{:#?}", r.fast_path.mismatches);
    // The learned run of the same split is untouched by instincts being off.
    let off = eval("demo-haiku45", None);
    assert!(off.instincts.is_none() && off.routes.iter().all(|x| !x.id.starts_with("instinct-")));
}

#[test]
fn instincts_after_learning_give_way_and_stay_safe() {
    // With learning (what `adage condition` does): a learned route with an instinct's id wins, the
    // rest bind, and every directive and wrapper is still one the oracle accepts.
    for (name, split) in [("demo-haiku45", Some("round2")), ("synthetic", None), ("adversarial", None)] {
        let r = eval_instincts(name, split, InstinctMode::With);
        assert_safe(&r);
        assert!(r.fast_path.mismatches.is_empty(), "{name}: {:#?}", r.fast_path.mismatches);
        let learned: std::collections::BTreeSet<&str> =
            r.routes.iter().filter(|x| x.support > 0).map(|x| x.id.as_str()).collect();
        assert!(learned.contains("dead-end-d41b1e") || name != "demo-haiku45", "{name}: {learned:?}");
        let ids: Vec<&str> = r.routes.iter().map(|x| x.id.as_str()).collect();
        let mut dedup = ids.clone();
        dedup.dedup();
        assert_eq!(ids, dedup, "{name}: one route per id");
    }
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
/// generator on purpose: `ADAGE_BLESS=1 cargo test -p adage-eval --test gates` (the variable
/// every golden in the workspace is re-blessed with; `ADAGE_EVAL_BLESS=1` still works).
#[test]
fn generated_datasets_are_up_to_date() {
    for (name, gen) in [
        ("synthetic", adage_eval::generate::synthetic as fn(&Path) -> anyhow::Result<()>),
        ("adversarial", adage_eval::generate::adversarial),
    ] {
        let committed = datasets().join(name);
        if ["ADAGE_BLESS", "ADAGE_EVAL_BLESS"].iter().any(|k| std::env::var(k).is_ok_and(|v| v == "1")) {
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
