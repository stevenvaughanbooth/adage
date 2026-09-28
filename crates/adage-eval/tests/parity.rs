//! Track G G5 gates (docs/tracks/track-g-graph-engine.md §10): the graph engine is the only
//! learner, and it learns at least what the hand-written miners it replaced learned.
//!
//! - **The miners' floor.** On every committed dataset and every split the gates and the L0 bench
//!   use, the conventions the miners recovered when they were retired (measured with their last
//!   version, the real-use fixes included) must all be recovered, and no learned route may be
//!   false beyond the one the miners learned too (`MINERS_FALSE`). The table is that measurement: a convention may be added to a row when learning
//!   improves, never removed.
//! - **Generated bundles.** On the `evals/gen` smoke, idshapes and newkinds suites, every
//!   convention is recovered with no false route and no decoy: the miners recovered all of the
//!   smoke suite's, and the graph engine alone the id shapes and the new kinds (scope, env_sync,
//!   sequence), with the environment step a draft (nascent until a person approves it). No
//!   learner code names these kinds.
//! - **Fast path.** The hook's no-match fast path changes no decision with the learned index.
//!
//! The generated suites are built and rehearsed into a temporary directory with
//! `python3 evals/gen/gen.py --suite <name>` and `python3 evals/gen/rehearse.py --no-learn`; that
//! part is skipped when python3 (or the generator's tools) is missing, and node-npm bundles are
//! skipped without node. `ADAGE_GEN_SMOKE=<dir>`, `ADAGE_GEN_IDSHAPES=<dir>` and
//! `ADAGE_GEN_NEWKINDS=<dir>` reuse rehearsed suites.
#![cfg(unix)]

use adage_eval::conventions::{self, RoutesReport};
use adage_eval::{run, Options};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn opts(split: Option<&str>) -> Options {
    Options { split: split.map(str::to_string), ..Default::default() }
}

/// (dataset, split, conventions the miners recovered there when they were retired).
const FLOOR: &[(&str, Option<&str>, &[&str])] = &[
    ("adversarial", None, &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"]),
    ("adversarial", Some("group:task:3:0"), &["tests-through-make"]),
    (
        "adversarial",
        Some("group:task:3:1"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    ("adversarial", Some("group:task:3:2"), &["work-item-lookup", "tests-through-make"]),
    (
        "adversarial",
        Some("temporal"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    ("demo-haiku45", None, &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"]),
    ("demo-haiku45", Some("round1"), &["work-item-lookup", "work-item-linked-files", "regenerate-models"]),
    (
        "demo-haiku45",
        Some("round2"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    (
        "demo-haiku45",
        Some("group:task:3:0"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    (
        "demo-haiku45",
        Some("group:task:3:1"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    (
        "demo-haiku45",
        Some("group:task:3:2"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    (
        "demo-haiku45",
        Some("temporal"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    (
        "demo-haiku45",
        Some("hash:0.3"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    ("live-20260928-acme-billing-sonnet5", None, &["work-item-lookup", "work-item-linked-files", "regenerate-models"]),
    ("live-20260928-acme-billing", None, &["work-item-lookup", "work-item-linked-files"]),
    ("live-20260928-harbor-dispatch-sonnet5", None, &["incident-lookup", "incident-linked-files"]),
    ("live-20260928-harbor-dispatch", None, &["incident-lookup", "incident-linked-files", "registry-codegen"]),
    ("live-20260928-node-billing-s22", None, &["lookup"]),
    ("live-20260928-node-billing-uuid-s42", None, &[]),
    ("live-20260928-node-clinic-s23", None, &["lookup", "linked-files"]),
    ("live-20260928-node-logistics-s21", None, &["lookup", "linked-files"]),
    ("live-20260928-py-billing-hash-s32", None, &[]),
    ("live-20260928-py-billing-s11", None, &["lookup"]),
    ("live-20260928-py-clinic-s12", None, &["lookup", "linked-files"]),
    ("live-20260928-py-logistics-s13", None, &["lookup", "linked-files"]),
    ("synthetic", None, &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"]),
    (
        "synthetic",
        Some("group:task:3:0"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    (
        "synthetic",
        Some("group:task:3:1"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    (
        "synthetic",
        Some("group:task:3:2"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
    (
        "synthetic",
        Some("temporal"),
        &["work-item-lookup", "work-item-linked-files", "tests-through-make", "regenerate-models"],
    ),
];

/// False routes the miners learned too, where the scorer is stricter than the sandbox: on
/// py-clinic-s12 every generic runner fails (`tests/__init__.py` aborts unless the fixtures are
/// loaded), so `avoid python -m pytest / python3 -m pytest; instead make check` is true there, but
/// the convention names only `python3 -m unittest`. No other false route is allowed.
const MINERS_FALSE: &[(&str, &str)] = &[("live-20260928-py-clinic-s12", "dead-end-60822e")];

fn found(r: &RoutesReport) -> Vec<&str> {
    r.conventions.iter().filter(|c| !c.matched_by.is_empty()).map(|c| c.id.as_str()).collect()
}

#[test]
fn every_dataset_keeps_the_miners_floor() {
    let mut bad = Vec::new();
    for (ds, split, floor) in FLOOR {
        let dir = root().join("evals/datasets").join(ds);
        let r = conventions::run(&dir, &opts(*split)).unwrap_or_else(|e| panic!("{ds}: {e:#}"));
        let got = found(&r);
        eprintln!(
            "{ds} {}: {}/{} {got:?}, false {:?}",
            r.split, r.conventions_found, r.conventions_evaluable, r.false_routes
        );
        let missing: Vec<&&str> = floor.iter().filter(|c| !got.contains(c)).collect();
        let false_routes: Vec<&String> = r
            .false_routes
            .iter()
            .filter(|f| !MINERS_FALSE.iter().any(|(d, id)| d == ds && f.starts_with(id)))
            .collect();
        if !missing.is_empty() || !false_routes.is_empty() {
            bad.push(format!("{ds} {}: missing {missing:?}, false {false_routes:?}", r.split));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn fast_path_changes_no_decision_with_the_learned_index() {
    for (ds, split) in [("demo-haiku45", Some("hash:0.3")), ("synthetic", None), ("adversarial", None)] {
        let r = run(&root().join("evals/datasets").join(ds), &opts(split)).unwrap();
        let f = &r.fast_path;
        assert!(f.mismatches.is_empty(), "{ds}: {:#?}", f.mismatches);
        assert!(f.skipped > 0 && f.skipped < f.events, "{ds}: {} of {}", f.skipped, f.events);
        assert_eq!(r.safety.unsafe_directives, 0, "{ds}: {:#?}", r.safety.violations);
    }
}

fn python() -> bool {
    Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// A temporary directory removed on drop.
struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A rehearsed suite: `$var`, or generated and rehearsed (without learning) into `tmp`.
fn suite(name: &str, var: &str, tmp: &Path) -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(var) {
        return Some(PathBuf::from(d));
    }
    if !python() {
        eprintln!("python3 missing: the {name} suite is skipped");
        return None;
    }
    let out = tmp.join(name);
    let gen = Command::new("python3")
        .arg(root().join("evals/gen/gen.py"))
        .args(["--suite", name, "--out"])
        .arg(&out)
        .output()
        .ok()?;
    if !gen.status.success() {
        eprintln!("gen.py --suite {name} failed (skipped): {}", String::from_utf8_lossy(&gen.stderr));
        return None;
    }
    let reh =
        Command::new("python3").arg(root().join("evals/gen/rehearse.py")).arg(&out).arg("--no-learn").output().ok()?;
    if !reh.status.success() {
        eprintln!("rehearse.py {name} failed (skipped): {}", String::from_utf8_lossy(&reh.stderr));
        return None;
    }
    Some(out)
}

fn bundles(suite: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(suite)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("spec.json").is_file() && p.join("rehearsal/transcripts").is_dir())
        .collect();
    v.sort();
    v
}

#[test]
fn every_generated_convention_is_learned() {
    let tmp = Tmp(std::env::temp_dir().join(format!("adage-g5-gen-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&tmp.0);
    std::fs::create_dir_all(&tmp.0).unwrap();
    let mut bad = Vec::new();
    for (name, var) in
        [("smoke", "ADAGE_GEN_SMOKE"), ("idshapes", "ADAGE_GEN_IDSHAPES"), ("newkinds", "ADAGE_GEN_NEWKINDS")]
    {
        let Some(s) = suite(name, var, &tmp.0) else { continue };
        let found_bundles = bundles(&s);
        assert!(!found_bundles.is_empty(), "no rehearsed {name} bundle in {}", s.display());
        for b in found_bundles {
            let bundle = b.file_name().unwrap().to_string_lossy().to_string();
            let r = conventions::run_bundle(&b).unwrap_or_else(|e| panic!("{bundle}: {e:#}"));
            eprintln!(
                "{name}/{bundle}: {}/{} false {:?} decoys {:?}",
                r.conventions_found, r.conventions_evaluable, r.false_routes, r.decoy_hits
            );
            if r.conventions_found < r.conventions_evaluable || !r.false_routes.is_empty() || !r.decoy_hits.is_empty() {
                bad.push(format!(
                    "{bundle}: missed {:?}, false {:?}, decoys {:?}",
                    r.missed_conventions, r.false_routes, r.decoy_hits
                ));
            }
            // The environment step is nascent (a draft) until a person approves it.
            for c in r.conventions.iter().filter(|c| c.kind == "env_sync") {
                for id in &c.matched_by {
                    let x = r.routes.iter().find(|x| &x.id == id).unwrap();
                    if x.state != "draft" {
                        bad.push(format!("{bundle}: env_sync route {id} is {}, not a draft", x.state));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
