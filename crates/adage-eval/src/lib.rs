//! Offline evaluation of Adage (Design §12, §14, §16).
//!
//! `adage eval` is the credibility test run over recorded history: learn tracks from a train
//! split into a temporary home, replay each held-out session event by event through the real
//! runtime (`adage_runtime::decide`, holdout arm), and score every directive Adage would have
//! given against what the agent actually did later in that session. Nothing runs: no agent, no
//! command, no model. The same dataset gives the same bytes on every run.
//!
//! Modules: [`dataset`] (manifests, splits, sandboxes), [`transcript`] (transcripts as hook
//! events), [`replay`] (the runtime, holdout arm), [`score`] (metrics), [`safety`] (the
//! directive oracle), [`fidelity`] (replay versus what the live hooks recorded), [`generate`]
//! (the synthetic and adversarial datasets), [`cli`].

pub mod cli;
pub mod dataset;
pub mod fidelity;
pub mod generate;
pub mod replay;
pub mod safety;
pub mod score;
pub mod transcript;

use adage_core::CommandTable;
use adage_learn::{Inputs, LearnConfig};
use adage_tracks::Index;
use anyhow::{Context, Result};
use dataset::{Dataset, Sandbox};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// A path inside a dataset directory: relative, no `..`.
pub(crate) fn safe_rel(p: &str) -> bool {
    !p.is_empty() && Path::new(p).components().all(|c| matches!(c, Component::Normal(_)))
}

pub(crate) fn lexical(p: &Path) -> PathBuf {
    adage_core::repo::lexical_normalise(p)
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Split name (see [`Dataset::split`]); `None` uses the dataset's default.
    pub split: Option<String>,
    /// `demo` or `default` learn thresholds; `None` uses the dataset's.
    pub thresholds: Option<String>,
    /// Model turns within which a followed directive counts as prompt.
    pub window: u32,
    /// Score test sessions that received live directives too (their behaviour was influenced).
    pub include_steered: bool,
    /// Replay with this index instead of learning one from the train split.
    pub index: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Options { split: None, thresholds: None, window: 3, include_steered: false, index: None }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LearnedRoute {
    pub id: String,
    pub kind: String,
    pub aspect: String,
    pub step: String,
    pub support: u32,
    pub opportunities: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub dataset: String,
    pub split: String,
    pub thresholds: String,
    pub window: u32,
    pub train_sessions: usize,
    pub test_sessions: usize,
    pub scored_sessions: usize,
    /// Test sessions not scored because live directives influenced them.
    pub skipped_steered: usize,
    /// Test sessions also in the train split (non-zero only for in-sample splits such as `all`).
    pub in_sample: usize,
    pub index_generation: String,
    pub routes: Vec<LearnedRoute>,
    pub overall: score::Metrics,
    pub by_kind: BTreeMap<String, score::Metrics>,
    pub by_route: BTreeMap<String, score::Metrics>,
    pub recall: BTreeMap<String, score::Recall>,
    pub dead_ends: score::DeadEnds,
    pub finish_line: score::FinishLine,
    pub safety: score::Safety,
    pub sessions: Vec<score::SessionScore>,
}

fn step_text(s: &adage_tracks::Step) -> String {
    match s {
        adage_tracks::Step::Run { command } => command.clone(),
        adage_tracks::Step::ReadBatch { slot, max } => format!("read {{{slot}}} (max {max})"),
        adage_tracks::Step::Avoid { avoid, instead, .. } => format!("avoid {avoid}; instead {instead}"),
    }
}

/// Learn tracks from `train` transcripts into a fresh temporary home; return the compiled index.
pub fn learn_index(ds: &Dataset, train: &[String], cfg: &LearnConfig) -> Result<Index> {
    let home = tempfile::tempdir()?;
    let mut transcripts = Vec::new();
    let mut outcomes = BTreeMap::new();
    for id in train {
        let s = ds.session(id).with_context(|| format!("split names unknown session {id}"))?;
        transcripts.push(ds.path(s));
        if let Some(p) = s.passed {
            outcomes.insert(s.id.clone(), p);
        }
    }
    let inputs = Inputs { transcripts, outcomes, repo_override: Some(ds.manifest.repo.clone()), models: None };
    let index_path = home.path().join("index.json");
    adage_learn::learn(&inputs, &home.path().join("tracks"), &index_path, cfg)?;
    Ok(Index::load(&index_path))
}

pub fn config(name: &str) -> Result<LearnConfig> {
    match name {
        "demo" => Ok(LearnConfig::demo()),
        "default" => Ok(LearnConfig::default()),
        other => anyhow::bail!("unknown thresholds {other:?} (demo or default)"),
    }
}

/// Run the offline eval over a dataset directory.
pub fn run(dir: &Path, opts: &Options) -> Result<Report> {
    let ds = Dataset::load(dir)?;
    let split_name = opts.split.clone().unwrap_or_else(|| ds.default_split());
    let split = ds.split(&split_name)?;
    let thresholds = opts.thresholds.clone().unwrap_or_else(|| ds.manifest.learn.clone());
    let cfg = config(&thresholds)?;
    let index = match &opts.index {
        Some(p) => {
            let p = if p.is_relative() && !p.exists() { dir.join(p) } else { p.clone() };
            serde_json::from_slice::<Index>(&std::fs::read(&p).with_context(|| p.display().to_string())?)?
        }
        None => learn_index(&ds, &split.train, &cfg)?,
    };
    let table = CommandTable::load();
    let sandbox = Sandbox::create(&ds.sandbox()?)?;
    let repo_key = ds.manifest.repo.clone();

    let mut tally = score::Tally::default();
    let (mut scored, mut skipped) = (0, 0);
    let mut test = split.test.clone();
    test.sort();
    for id in &test {
        let meta = ds.session(id).with_context(|| format!("split names unknown session {id}"))?;
        let t = transcript::read(&ds.path(meta))?;
        if t.steered && !opts.include_steered {
            skipped += 1;
            continue;
        }
        let rep = replay::replay(&t, &index, &repo_key, &sandbox.root, &table, meta.stopped());
        score::score_session(
            &mut tally,
            &rep,
            Some(meta),
            &index,
            &repo_key,
            &sandbox.root,
            &table,
            opts.window,
            &ds.manifest.canaries,
        );
        scored += 1;
    }
    tally.finish();
    let in_sample = test.iter().filter(|id| split.train.contains(id)).count();
    let routes = index
        .for_repo(&repo_key)
        .map(|ir| LearnedRoute {
            id: ir.route.id.clone(),
            kind: score::kind(&ir.route).into(),
            aspect: format!("{:?}", ir.route.aspect).to_lowercase(),
            step: step_text(&ir.route.step),
            support: ir.route.evidence.support,
            opportunities: ir.route.evidence.opportunities,
        })
        .collect();
    Ok(Report {
        dataset: ds.manifest.name.clone(),
        split: split_name,
        thresholds,
        window: opts.window,
        train_sessions: split.train.len(),
        test_sessions: split.test.len(),
        scored_sessions: scored,
        skipped_steered: skipped,
        in_sample,
        index_generation: index.generation.clone(),
        routes,
        overall: tally.overall,
        by_kind: tally.by_kind,
        by_route: tally.by_route,
        recall: tally.recall,
        dead_ends: tally.dead_ends,
        finish_line: tally.finish_line,
        safety: tally.safety,
        sessions: tally.sessions,
    })
}

fn pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

/// Human-readable report. Deterministic, like the JSON.
pub fn render_text(r: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "adage eval: {} (split {}, thresholds {}, window {} turns)",
        r.dataset, r.split, r.thresholds, r.window
    );
    let _ = writeln!(
        s,
        "sessions: {} train, {} test, {} scored, {} skipped (steered){}",
        r.train_sessions,
        r.test_sessions,
        r.scored_sessions,
        r.skipped_steered,
        if r.in_sample > 0 { format!(", {} IN-SAMPLE", r.in_sample) } else { String::new() }
    );
    let _ = writeln!(s, "index {} with {} routes", r.index_generation, r.routes.len());
    for x in &r.routes {
        let _ = writeln!(
            s,
            "  {:<22} {:<12} {:<6} {:>3}/{:<3} {}",
            x.id, x.kind, x.aspect, x.support, x.opportunities, x.step
        );
    }
    let _ = writeln!(s, "\ndirectives        given  correct  precision  @window  false-rate  turns-saved (median)");
    let mut row = |name: &str, m: &score::Metrics| {
        let _ = writeln!(
            s,
            "  {:<15} {:>5}  {:>7}  {:>9}  {:>7}  {:>10}  {:>6} ({})",
            name,
            m.directives,
            m.correct,
            pct(m.precision),
            pct(m.precision_in_window),
            pct(m.false_directive_rate),
            m.turns_saved,
            m.turns_saved_median
        );
    };
    row("all", &r.overall);
    for (k, m) in &r.by_kind {
        row(&format!("kind:{k}"), m);
    }
    for (k, m) in &r.by_route {
        row(k, m);
    }
    let _ = writeln!(s, "\nrecall (key steps with a directive first)");
    for (k, x) in &r.recall {
        let _ = writeln!(s, "  {:<15} {:>3}/{:<3} {}", k, x.covered, x.key_steps, pct(x.recall));
    }
    let d = &r.dead_ends;
    let _ = writeln!(
        s,
        "\ndead ends: {} warnings; avoided command run after {} ({} failed as predicted, {} passed): accuracy {}; {}/{} failed verify runs came after a warning",
        d.warnings, d.tested, d.confirmed, d.refuted, pct(d.accuracy), d.failed_runs_after_warning, d.failed_verify_runs
    );
    let f = &r.finish_line;
    let _ = writeln!(
        s,
        "finish line: {} obligations — {} met by the agent, {} needed but skipped, {} false alarms, {} unlabelled: accuracy {}; {} stale sessions missed; stop requests {} ({} correct)",
        f.obligations, f.met, f.caught, f.false_alarms, f.unknown, pct(f.accuracy), f.missed, f.stop_requests, f.stop_requests_correct
    );
    let _ = writeln!(s, "safety: {} checked, {} unsafe", r.safety.checked, r.safety.unsafe_directives);
    for v in &r.safety.violations {
        let _ = writeln!(s, "  UNSAFE {}: {}", v.route, v.reason);
    }
    s
}
