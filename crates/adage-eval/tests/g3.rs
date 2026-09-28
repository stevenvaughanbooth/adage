//! Track G G3 gates (docs/tracks/track-g-graph-engine.md §10): the graph engine against the
//! miners, and its new capability.
//!
//! - **Parity.** On every committed dataset (its default split) and every `evals/gen` smoke
//!   bundle, `learn.engine = graph` recovers at least as many conventions as the miners and learns
//!   no more false routes (`adage eval routes --engine graph`).
//! - **New capability.** On the `newkinds` suite (`evals/gen/specs/newkinds.json`), the graph
//!   engine learns the scope, env_sync and sequence conventions — the environment step as a draft
//!   (nascent until a person approves it, as its ceiling requires) — and the miners learn none of
//!   them. No learner code names these kinds: they are delivery shapes the compiler reaches from
//!   the mined patterns' structure.
//! - **Fast path.** The hook's no-match fast path changes no decision with the graph engine's
//!   index either (since G4 the triggers hold every context item's words; `g4.rs` adds a context route).
//!
//! The generated suites are built and rehearsed into a temporary directory with
//! `python3 evals/gen/gen.py --suite <name>` and `python3 evals/gen/rehearse.py --no-learn`; that
//! part is skipped when python3 (or the generator's tools) is missing, and node-npm bundles are
//! skipped without node. `ADAGE_GEN_SMOKE=<dir>` and `ADAGE_GEN_NEWKINDS=<dir>` reuse rehearsed
//! suites.
#![cfg(unix)]

use adage_eval::conventions::{self, RoutesReport};
use adage_eval::{run, Options};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn opts(engine: &str, split: Option<&str>) -> Options {
    Options {
        split: split.map(str::to_string),
        params: Some(serde_json::json!({ "learn": { "engine": engine } })),
        ..Default::default()
    }
}

fn line(what: &str, m: &RoutesReport, g: &RoutesReport) -> String {
    format!(
        "{what}: miners recall {}/{} false {:?} | graph recall {}/{} false {:?} (unscored {})",
        m.conventions_found,
        m.conventions_evaluable,
        m.false_routes,
        g.conventions_found,
        g.conventions_evaluable,
        g.false_routes,
        g.inert_routes.len()
    )
}

fn parity(what: &str, m: &RoutesReport, g: &RoutesReport) -> Option<String> {
    let l = line(what, m, g);
    eprintln!("{l}");
    (g.conventions_found < m.conventions_found || g.false_routes.len() > m.false_routes.len()).then_some(l)
}

#[test]
fn datasets_parity() {
    let mut bad = Vec::new();
    for ds in ["demo-haiku45", "synthetic", "adversarial"] {
        let dir = root().join("evals/datasets").join(ds);
        let m = conventions::run(&dir, &opts("miners", None)).unwrap_or_else(|e| panic!("{ds}: {e:#}"));
        let g = conventions::run(&dir, &opts("graph", None)).unwrap_or_else(|e| panic!("{ds}: {e:#}"));
        assert_eq!(g.engine, "graph");
        bad.extend(parity(ds, &m, &g));
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn fast_path_changes_no_decision_with_the_graph_engine() {
    for (ds, split) in [("demo-haiku45", Some("hash:0.3")), ("synthetic", None), ("adversarial", None)] {
        let r = run(&root().join("evals/datasets").join(ds), &opts("graph", split)).unwrap();
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
fn gen_bundles_parity_and_new_capability() {
    let tmp = Tmp(std::env::temp_dir().join(format!("adage-g3-gen-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&tmp.0);
    std::fs::create_dir_all(&tmp.0).unwrap();
    let mut bad = Vec::new();
    if let Some(s) = suite("smoke", "ADAGE_GEN_SMOKE", &tmp.0) {
        let found = bundles(&s);
        assert!(!found.is_empty(), "no rehearsed smoke bundle in {}", s.display());
        for b in found {
            let name = b.file_name().unwrap().to_string_lossy().to_string();
            let m = conventions::run_bundle(&b, "miners").unwrap_or_else(|e| panic!("{name}: {e:#}"));
            let g = conventions::run_bundle(&b, "graph").unwrap_or_else(|e| panic!("{name}: {e:#}"));
            bad.extend(parity(&name, &m, &g));
        }
    }
    let new_kinds = ["scope", "env_sync", "sequence"];
    if let Some(s) = suite("newkinds", "ADAGE_GEN_NEWKINDS", &tmp.0) {
        let found = bundles(&s);
        assert!(!found.is_empty(), "no rehearsed newkinds bundle in {}", s.display());
        for b in found {
            let name = b.file_name().unwrap().to_string_lossy().to_string();
            let m = conventions::run_bundle(&b, "miners").unwrap_or_else(|e| panic!("{name}: {e:#}"));
            let g = conventions::run_bundle(&b, "graph").unwrap_or_else(|e| panic!("{name}: {e:#}"));
            bad.extend(parity(&name, &m, &g));
            for kind in new_kinds {
                let c = |r: &RoutesReport| r.conventions.iter().find(|c| c.kind == kind).cloned();
                let (cm, cg) = (c(&m).expect(kind), c(&g).expect(kind));
                eprintln!("{name}: {kind}: miners {:?}, graph {:?}", cm.matched_by, cg.matched_by);
                if !cm.matched_by.is_empty() {
                    bad.push(format!("{name}: the miners learned {kind} ({:?})", cm.matched_by));
                }
                if cg.matched_by.is_empty() {
                    bad.push(format!("{name}: the graph engine did not learn {kind}"));
                }
                // The environment step is nascent (a draft) until a person approves it.
                if kind == "env_sync" {
                    for id in &cg.matched_by {
                        let r = g.routes.iter().find(|r| &r.id == id).unwrap();
                        if r.state != "draft" {
                            bad.push(format!("{name}: env_sync route {id} is {}, not a draft", r.state));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
