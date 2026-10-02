//! Parity with today's route kinds (Track G §8): on the committed datasets and on `evals/gen`
//! smoke bundles, the mined reflexes include a pattern for every ground-truth convention in
//! `conventions.json` (lookup: prompt =flow=> lookup; linked files: lookup =same_art=> read;
//! dead end and instinct violation: failed runner → working command; finish line:
//! last(edit glob) → regenerate), and direct no decoy. Recall and false patterns (reflexes that
//! match no convention) are reported per bundle.
//!
//! The smoke bundles are generated and rehearsed into a temporary directory with
//! `python3 evals/gen/gen.py --suite smoke` and `python3 evals/gen/rehearse.py --no-learn`; the
//! part is skipped when python3 (or the generator's tools) is missing. `ADAGE_GEN_SMOKE=<dir>`
//! reuses a rehearsed suite.
mod support;

use adage_graph::events::EventSession;
use adage_graph::select::{self, matches_convention, Params, Reflex};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Conv {
    id: String,
    kind: String,
    command: String,
    avoided: Vec<String>,
    glob: Option<String>,
}

const KINDS: [&str; 5] = ["lookup", "linked_files", "dead_end", "instinct_violation", "finish_line"];

fn conventions(path: &Path) -> Vec<Conv> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let list = v.get("conventions").cloned().unwrap_or(v);
    list.as_array()
        .unwrap()
        .iter()
        .map(|c| Conv {
            id: c["id"].as_str().unwrap_or_default().into(),
            kind: c["kind"].as_str().unwrap_or_default().into(),
            command: c["command"].as_str().unwrap_or_default().into(),
            avoided: match &c["avoided"] {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                _ => vec![],
            },
            glob: c["glob"].as_str().map(str::to_string),
        })
        .collect()
}

struct Result {
    recall: f64,
    missing: Vec<String>,
    decoys: Vec<String>,
    false_patterns: usize,
}

fn check(name: &str, sessions: &[EventSession], convs: &[Conv]) -> Result {
    let l = select::learn(sessions, &Params::demo());
    let hit = |r: &Reflex, c: &Conv| matches_convention(r, &c.kind, &c.command, &c.avoided, c.glob.as_deref());
    let truth: Vec<&Conv> = convs.iter().filter(|c| KINDS.contains(&c.kind.as_str())).collect();
    let missing: Vec<String> = truth
        .iter()
        .filter(|c| !l.reflexes.iter().any(|r| hit(r, c)))
        .map(|c| format!("{} ({})", c.id, c.kind))
        .collect();
    let decoys: Vec<String> = convs
        .iter()
        .filter(|c| c.kind == "decoy" && l.reflexes.iter().any(|r| hit(r, c)))
        .map(|c| c.id.clone())
        .collect();
    let false_patterns = l
        .reflexes
        .iter()
        .filter(|r| !convs.iter().any(|c| (KINDS.contains(&c.kind.as_str()) || c.kind == "decoy") && hit(r, c)))
        .count();
    let recall = 1.0 - missing.len() as f64 / truth.len().max(1) as f64;
    eprintln!(
        "{name}: {} sessions, {} reflexes; recall {recall:.2} ({} conventions){}; decoys {decoys:?}; false patterns {false_patterns}",
        sessions.len(),
        l.reflexes.len(),
        truth.len(),
        if missing.is_empty() { String::new() } else { format!(", missing {missing:?}") },
    );
    if !missing.is_empty() || !decoys.is_empty() {
        for r in &l.reflexes {
            eprintln!("    {} {}", r.shape, r.why);
        }
    }
    Result { recall, missing, decoys, false_patterns }
}

#[test]
fn datasets_parity() {
    for name in ["demo-haiku45", "synthetic"] {
        // The conventions label what agents did, steered by Adage or not, so recall is measured on
        // every step (the learner's discount of its own influence is tested in adage-learn).
        let Some(sessions) = support::dataset_with(name, support::Influence::Ignored) else { continue };
        let convs = conventions(&support::repo_root().join("evals/datasets").join(name).join("conventions.json"));
        let r = check(name, &sessions, &convs);
        assert!(r.missing.is_empty(), "{name}: missing {:?}", r.missing);
        assert!(r.decoys.is_empty(), "{name}: decoys {:?}", r.decoys);
        assert_eq!(r.recall, 1.0);
        let _ = r.false_patterns;
    }
}

fn python() -> bool {
    Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// A rehearsed smoke suite: `ADAGE_GEN_SMOKE`, or generated into a temporary directory.
fn smoke_suite() -> Option<(PathBuf, Option<tempfile_dir::Dir>)> {
    if let Some(d) = std::env::var_os("ADAGE_GEN_SMOKE") {
        return Some((PathBuf::from(d), None));
    }
    if !python() {
        eprintln!("python3 missing: gen smoke parity skipped");
        return None;
    }
    let tmp = tempfile_dir::Dir::new("adage-gen-smoke");
    let out = tmp.path.join("smoke");
    let root = support::repo_root();
    let gen = Command::new("python3")
        .arg(root.join("evals/gen/gen.py"))
        .args(["--suite", "smoke", "--out"])
        .arg(&out)
        .output()
        .ok()?;
    if !gen.status.success() {
        eprintln!("gen.py failed (skipped): {}", String::from_utf8_lossy(&gen.stderr));
        return None;
    }
    let reh =
        Command::new("python3").arg(root.join("evals/gen/rehearse.py")).arg(&out).arg("--no-learn").output().ok()?;
    if !reh.status.success() {
        eprintln!("rehearse.py failed (skipped): {}", String::from_utf8_lossy(&reh.stderr));
        return None;
    }
    Some((out, Some(tmp)))
}

/// A temporary directory removed on drop (no dependency).
mod tempfile_dir {
    pub struct Dir {
        pub path: std::path::PathBuf,
    }
    impl Dir {
        pub fn new(tag: &str) -> Dir {
            let path = std::env::temp_dir().join(format!("{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("temp dir");
            Dir { path }
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[test]
fn gen_smoke_parity() {
    let Some((suite, _keep)) = smoke_suite() else { return };
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&suite)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("spec.json").is_file() && p.join("rehearsal/transcripts").is_dir())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no rehearsed bundles in {}", suite.display());
    let mut failures = Vec::new();
    for d in &dirs {
        let name = d.file_name().unwrap().to_string_lossy().to_string();
        let sessions = support::bundle(d);
        let convs = conventions(&d.join("conventions.json"));
        let r = check(&name, &sessions, &convs);
        if !r.missing.is_empty() || !r.decoys.is_empty() {
            failures.push(format!("{name}: missing {:?}, decoys {:?}", r.missing, r.decoys));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
