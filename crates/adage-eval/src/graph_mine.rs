//! `adage eval mine`: the Track G graph engine's mined reflexes (G2), on a dataset or on
//! generated sandboxes.
//!
//! ```text
//! adage eval mine <dataset-dir | gen-bundle | gen-suite> [--split <split>] [--top <n>] [--json]
//! ```
//!
//! For each repository it learns reflexes from every session (the reflexes and the parity), and
//! from the train side of **task-grouped** folds for the held-out measures, pooled over folds.
//! It reports:
//!
//! - the top reflexes (pattern, support, days, conf and its lower bound, lift, contrast, value,
//!   compression gain);
//! - on the held-out side: next-action top-1 accuracy of the engine (the
//!   variable-order backoff model, `adage_graph::score::NextModel`) against the order-1 and
//!   order-2 baselines with the oracle ceilings, conformance (mean −ln p of the next act), and
//!   the calibration ECE of every candidate's calibrated posterior against held-out outcomes
//!   (all (session, candidate) pairs, and the selected reflexes' with a bootstrap interval;
//!   Track G §5, Design §8.5);
//! - parity with `conventions.json`: each convention found as a mined pattern (Track G §8,
//!   [`adage_graph::select::matches_convention`]), decoys selected, and reflexes matching no
//!   convention.
//!
//! Folds: a dataset uses `group:task:3:{0,1,2}` (or the one `--split` named, see
//! [`crate::dataset`]); a generated bundle uses its own task split (`tasks.json`: train tasks
//! then eval tasks held out, and the reverse), from the rehearsal's transcripts
//! (`python3 evals/gen/rehearse.py <dir> --no-learn`).
//!
//! Sessions come through adage-learn's importer; each import step becomes an event
//! (`adage_graph::events::from_input`), with its model turn as the batch and its effect class.

use adage_core::types::CanonicalAction;
use adage_core::CommandTable;
use adage_graph::events::{self, EventSession};
use adage_graph::input;
use adage_graph::score::Holdout;
use adage_graph::select::{self, Learned, Params, Reflex};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Convert an imported session into the graph engine's events.
pub fn to_events(s: &adage_learn::import::Session, task: Option<String>, table: &CommandTable) -> EventSession {
    let mut steps = Vec::new();
    let mut turns = Vec::new();
    let mut effects = Vec::new();
    for st in &s.steps {
        let mut push = |step: input::Step, eff| {
            steps.push(step);
            turns.push(st.turn);
            effects.push(eff);
        };
        match &st.norm.action {
            CanonicalAction::Exec { argv, .. } => push(
                input::Step::Exec { argv: argv.clone(), output: st.call.output.clone(), failed: st.call.failed },
                Some(st.norm.effect),
            ),
            CanonicalAction::Read { .. } | CanonicalAction::List { .. } | CanonicalAction::Search { .. } => {
                if let Some(p) = &st.rel_path {
                    push(input::Step::Read { path: p.clone() }, None);
                }
            }
            CanonicalAction::Edit { .. } | CanonicalAction::Write { .. } => {
                if let Some(p) = &st.rel_path {
                    push(input::Step::Edit { path: p.clone() }, None);
                }
            }
            _ => {}
        }
    }
    let inp =
        input::Session { id: s.id.clone(), prompts: vec![input::Prompt { text: s.prompt.clone(), at_step: 0 }], steps };
    let meta = events::Meta {
        day: Some(s.day.clone()),
        task,
        outcome: s.outcome,
        turns: Some(turns),
        effects: Some(effects),
        influence: vec![],
    };
    events::from_input(&inp, &meta, table)
}

/// A ground-truth convention, from a dataset's or a bundle's `conventions.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conv {
    pub id: String,
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub avoided: Vec<String>,
    #[serde(default)]
    pub glob: Option<String>,
}

/// Read `conventions.json` (`adage-conventions/1`, a dataset's or a generated bundle's): its
/// conventions, then its decoys (kind `decoy`). A dead end planted as the retired universal
/// default (`mode: instinct_violation`) is kind `instinct_violation` here, as the G2 parity counts it.
pub fn read_conventions(path: &Path) -> Result<Vec<Conv>> {
    let c = crate::conventions::Conventions::load(path)?;
    let mut out: Vec<Conv> = c
        .conventions
        .iter()
        .map(|x| Conv {
            id: x.id.clone(),
            kind: if x.mode.as_deref() == Some("instinct_violation") {
                "instinct_violation".to_string()
            } else {
                x.kind.name().to_string()
            },
            command: x.command.clone(),
            avoided: x.avoided.clone(),
            glob: x.glob.clone(),
        })
        .collect();
    out.extend(c.decoys.iter().map(|d| Conv {
        id: d.id.clone(),
        kind: "decoy".into(),
        command: d.command.clone(),
        avoided: vec![],
        glob: None,
    }));
    Ok(out)
}

/// Parity of mined reflexes with the conventions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Parity {
    /// (convention id, kind, the first matching reflex's pattern) per non-decoy convention.
    pub found: Vec<(String, String, Option<String>)>,
    pub recall: f64,
    /// Decoy conventions some reflex directs.
    pub decoys_selected: Vec<String>,
    pub decoys: usize,
    /// Reflexes matching no convention.
    pub unmatched: usize,
}

/// Kinds the G2 engine is measured on (intents need the embedding model).
pub const KINDS: [&str; 5] = ["lookup", "linked_files", "dead_end", "instinct_violation", "finish_line"];

pub fn parity(reflexes: &[Reflex], convs: &[Conv]) -> Parity {
    let mut p = Parity::default();
    let hit = |r: &Reflex, c: &Conv| select::matches_convention(r, &c.kind, &c.command, &c.avoided, c.glob.as_deref());
    for c in convs.iter().filter(|c| KINDS.contains(&c.kind.as_str())) {
        let m = reflexes.iter().find(|r| hit(r, c)).map(|r| r.pattern.clone());
        p.found.push((c.id.clone(), c.kind.clone(), m));
    }
    let n = p.found.len();
    p.recall = if n == 0 { 0.0 } else { p.found.iter().filter(|f| f.2.is_some()).count() as f64 / n as f64 };
    for c in convs.iter().filter(|c| c.kind == "decoy") {
        p.decoys += 1;
        if reflexes.iter().any(|r| hit(r, c)) {
            p.decoys_selected.push(c.id.clone());
        }
    }
    p.unmatched = reflexes
        .iter()
        .filter(|r| !convs.iter().any(|c| (KINDS.contains(&c.kind.as_str()) || c.kind == "decoy") && hit(r, c)))
        .count();
    p
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleReport {
    pub name: String,
    pub split: String,
    pub sessions: usize,
    pub folds: usize,
    pub learned: Learned,
    pub holdout: Option<Holdout>,
    pub parity: Parity,
    /// Milliseconds: prototypes, mining, scoring, selection.
    pub timings_ms: [u64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub bundles: Vec<BundleReport>,
    /// Means over bundles with a holdout.
    pub top1_engine: f64,
    pub top1_baseline: f64,
    pub top1_order2: f64,
    pub ece: f64,
    pub ece_raw: f64,
    pub recall: f64,
    pub decoys_selected: usize,
}

fn read_all(files: &[PathBuf], repo: &str, table: &CommandTable) -> Vec<adage_learn::import::Session> {
    let mut out: Vec<_> = files.iter().filter_map(|f| adage_learn::read_session(f, table, Some(repo))).collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Learn from every session (reflexes, parity), and per fold (train, test) for the pooled
/// held-out measures.
fn evaluate(
    name: &str,
    split: &str,
    all: &[EventSession],
    folds: &[(Vec<EventSession>, Vec<EventSession>)],
    convs: &[Conv],
    p: &Params,
) -> BundleReport {
    let learned = select::learn(all, p);
    let parity = parity(&learned.reflexes, convs);
    let parts: Vec<Holdout> = folds
        .iter()
        .filter(|(tr, te)| !tr.is_empty() && !te.is_empty())
        .filter_map(|(tr, te)| select::learn_split(tr, te, p).1)
        .collect();
    let holdout = (!parts.is_empty()).then(|| Holdout::merge(&parts));
    let t = &learned.timings;
    BundleReport {
        name: name.to_string(),
        split: split.to_string(),
        sessions: all.len(),
        folds: parts.len(),
        timings_ms: [t.proto_ms, t.mine_ms, t.score_ms, t.select_ms],
        learned,
        holdout,
        parity,
    }
}

fn params(demo: bool) -> Params {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
    let p = if demo { Params::demo() } else { Params::default() };
    p.with_threads(threads)
}

/// The run directory's phase and task (`runs/<phase>-<task>-r<n>/…`), from a session root.
pub fn run_task(root: &Path) -> Option<(String, String)> {
    for c in root.components().rev() {
        let s = c.as_os_str().to_string_lossy();
        let Some((phase, rest)) = s.split_once('-') else { continue };
        if phase != "train" && phase != "eval" {
            continue;
        }
        if let Some((task, rep)) = rest.rsplit_once("-r") {
            if rep.chars().all(|c| c.is_ascii_digit()) && !rep.is_empty() {
                return Some((phase.to_string(), task.to_string()));
            }
        }
    }
    None
}

fn gen_bundle(dir: &Path, table: &CommandTable) -> Result<BundleReport> {
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let convs = read_conventions(&dir.join("conventions.json"))?;
    let tasks: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("tasks.json"))?)?;
    let repo = tasks["repo"].as_str().unwrap_or_default().to_string();
    let tdir = dir.join("rehearsal").join("transcripts");
    if !tdir.is_dir() {
        bail!(
            "{name}: no rehearsal transcripts: run `python3 evals/gen/rehearse.py {} --no-learn` first",
            dir.display()
        );
    }
    let mut files = Vec::new();
    for p in std::fs::read_dir(&tdir)?.flatten() {
        if p.path().is_dir() {
            for f in std::fs::read_dir(p.path())?.flatten() {
                if f.path().extension().is_some_and(|e| e == "jsonl") {
                    files.push(f.path());
                }
            }
        }
    }
    files.sort();
    let split_of: std::collections::BTreeMap<String, String> = tasks["tasks"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| {
                    (
                        t["id"].as_str().unwrap_or_default().to_string(),
                        t["split"].as_str().unwrap_or("train").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let (mut all, mut a, mut b) = (Vec::new(), Vec::new(), Vec::new());
    for s in read_all(&files, &repo, table) {
        let task = run_task(&s.root).map(|x| x.1).unwrap_or_else(|| s.id.clone());
        let e = to_events(&s, Some(task.clone()), table);
        if split_of.get(&task).map(String::as_str) == Some("eval") {
            b.push(e.clone())
        } else {
            a.push(e.clone())
        }
        all.push(e);
    }
    let folds = vec![(a.clone(), b.clone()), (b, a)];
    Ok(evaluate(&name, "tasks.json train/eval, both ways", &all, &folds, &convs, &params(true)))
}

fn dataset(dir: &Path, split: Option<&str>, table: &CommandTable) -> Result<BundleReport> {
    let ds = crate::dataset::Dataset::load(dir)?;
    let names: Vec<String> = match split {
        Some(s) => vec![s.to_string()],
        None => (0..3).map(|f| format!("group:task:3:{f}")).collect(),
    };
    let convs = match ds.conventions_file()? {
        Some(f) => read_conventions(&f)?,
        None => vec![],
    };
    let files: Vec<PathBuf> = ds.manifest.sessions.iter().map(|s| ds.path(s)).collect();
    let sessions = read_all(&files, &ds.manifest.repo, table);
    // Sessions are matched to the manifest by id (the transcript's session id) or file stem.
    let mut all: Vec<(String, EventSession)> = Vec::new();
    for s in &sessions {
        let m = ds
            .manifest
            .sessions
            .iter()
            .find(|m| m.id == s.id || ds.path(m).file_stem().is_some_and(|f| f == s.id.as_str()));
        if let Some(m) = m {
            all.push((m.id.clone(), to_events(s, Some(m.task_key().to_string()), table)));
        }
    }
    let mut folds = Vec::new();
    for n in &names {
        let sp = ds.split(n)?;
        let test_ids: BTreeSet<&str> = sp.test.iter().map(String::as_str).collect();
        let train_ids: BTreeSet<&str> = sp.train.iter().map(String::as_str).collect();
        let pick = |ids: &BTreeSet<&str>| {
            all.iter().filter(|(m, _)| ids.contains(m.as_str())).map(|(_, e)| e.clone()).collect()
        };
        folds.push((pick(&train_ids), pick(&test_ids)));
    }
    let every: Vec<EventSession> = all.into_iter().map(|(_, e)| e).collect();
    let label = names.join(", ");
    Ok(evaluate(&ds.manifest.name, &label, &every, &folds, &convs, &params(ds.manifest.learn == "demo")))
}

/// Run on a dataset, a generated bundle, or a suite of bundles.
pub fn run(dir: &Path, split: Option<&str>) -> Result<Report> {
    let table = CommandTable::load();
    let mut bundles = Vec::new();
    if dir.join("manifest.json").is_file() {
        bundles.push(dataset(dir, split, &table)?);
    } else if dir.join("spec.json").is_file() {
        bundles.push(gen_bundle(dir, &table)?);
    } else {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| dir.display().to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("spec.json").is_file())
            .collect();
        dirs.sort();
        if dirs.is_empty() {
            bail!("{}: not a dataset, a generated bundle or a suite", dir.display());
        }
        for d in dirs {
            bundles.push(gen_bundle(&d, &table)?);
        }
    }
    let with: Vec<&Holdout> = bundles.iter().filter_map(|b| b.holdout.as_ref()).collect();
    let mean = |f: &dyn Fn(&Holdout) -> f64| {
        if with.is_empty() {
            0.0
        } else {
            with.iter().map(|h| f(h)).sum::<f64>() / with.len() as f64
        }
    };
    let n = bundles.len().max(1) as f64;
    Ok(Report {
        top1_engine: mean(&|h| h.top1_engine),
        top1_baseline: mean(&|h| h.top1_baseline),
        top1_order2: mean(&|h| h.top1_order2),
        ece: mean(&|h| h.ece),
        ece_raw: mean(&|h| h.ece_raw),
        recall: bundles.iter().map(|b| b.parity.recall).sum::<f64>() / n,
        decoys_selected: bundles.iter().map(|b| b.parity.decoys_selected.len()).sum(),
        bundles,
    })
}

pub fn render_text(r: &Report, top: usize) -> String {
    let mut s = String::new();
    for b in &r.bundles {
        let l = &b.learned;
        s.push_str(&format!(
            "== {} ({} sessions; held out: {}, {} folds)\n   labels {} (alphabet {}), s_min {}, targets {}, expansions {}{}, frequent {}, closed {}, gated {}, selected {}; τ {:?}; singletons {:.0}%\n   time: prototypes {} ms, mining {} ms, scoring {} ms, selection {} ms\n",
            b.name,
            b.sessions,
            b.split,
            b.folds,
            l.labels,
            l.alphabet,
            l.s_min,
            l.targets,
            l.expansions,
            if l.truncated { " (truncated)" } else { "" },
            l.frequent,
            l.closed,
            l.gated,
            l.reflexes.len(),
            l.tau,
            100.0 * l.singleton_share,
            b.timings_ms[0],
            b.timings_ms[1],
            b.timings_ms[2],
            b.timings_ms[3],
        ));
        for x in l.reflexes.iter().take(top) {
            s.push_str(&format!(
                "   {:<10} {:>3} sess {:>2} d  conf {:.2} (lb {:.2})  lift {:>5.2}  contrast {:>5.2}  V {:>5.2}  gain {:>6.1}  {}\n",
                x.shape, x.support, x.days, x.conf, x.conf_lb, x.lift, x.contrast, x.value, x.gain, x.pattern
            ));
        }
        if let Some(h) = &b.holdout {
            s.push_str(&format!(
                "   held out: next-action top-1 engine {:.3} vs order-1 {:.3} vs order-2 {:.3} ({} positions; chains chosen {:?}); oracle ceilings (order 1/2/3) {}; conformance {:.3} vs {:.3} nats\n   calibration: ECE {:.3} over {} (session, candidate) pairs (raw {:.3}); selected reflexes {:.3} over {} pairs (90% CI {:.3}–{:.3})\n",
                h.top1_engine,
                h.top1_baseline,
                h.top1_order2,
                h.positions,
                h.chosen,
                h.oracle.iter().map(|o| format!("{:.3} ({:.2} bits)", o.ceiling, o.entropy_bits)).collect::<Vec<_>>().join(" / "),
                h.conformance_engine,
                h.conformance_baseline,
                h.ece,
                h.ece_pairs,
                h.ece_raw,
                h.ece_selected,
                h.ece_selected_pairs,
                h.ece_selected_ci.0,
                h.ece_selected_ci.1,
            ));
        }
        let p = &b.parity;
        for (id, kind, m) in &p.found {
            s.push_str(&format!(
                "   {:<9} {:<20} {:<18} {}\n",
                if m.is_some() { "FOUND" } else { "MISSING" },
                id,
                kind,
                m.as_deref().unwrap_or("")
            ));
        }
        s.push_str(&format!(
            "   parity: recall {:.2}; decoys selected {}/{} {:?}; reflexes matching no convention {}\n\n",
            p.recall,
            p.decoys_selected.len(),
            p.decoys,
            p.decoys_selected,
            p.unmatched
        ));
    }
    s.push_str(&format!(
        "mean over bundles: next-action top-1 engine {:.3} vs order-1 {:.3} vs order-2 {:.3} (target: order-1 + 0.05); ECE {:.3} (raw {:.3}; target ≤ 0.05); convention recall {:.2}; decoys selected {}\n",
        r.top1_engine, r.top1_baseline, r.top1_order2, r.ece, r.ece_raw, r.recall, r.decoys_selected
    ));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_tasks_from_roots() {
        let p = Path::new("/x/rehearsal/runs/train-BIL-1872-r1/tallyhouse-billing");
        assert_eq!(run_task(p), Some(("train".into(), "BIL-1872".into())));
        let p = Path::new("/x/runs/eval-ctl-2-r2/repo");
        assert_eq!(run_task(p), Some(("eval".into(), "ctl-2".into())));
        assert_eq!(run_task(Path::new("/tmp/repo")), None);
    }

    #[test]
    fn datasets_mine() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/datasets/synthetic");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let r = run(&dir, None).unwrap();
        let b = &r.bundles[0];
        assert!(b.sessions > 0 && b.folds > 0);
        assert!(!b.learned.reflexes.is_empty());
        assert!(b.holdout.is_some());
    }
}
