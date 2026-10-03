//! Offline evaluation of Adage (Design §12, §14, §16).
//!
//! `adage eval` is the credibility test run over recorded history: learn tracks from a train
//! split into a temporary home, replay each held-out session event by event through the real
//! runtime (`adage_runtime::decide`, control arm), and score every directive Adage would have
//! given against what the agent actually did later in that session. Nothing runs: no agent, no
//! command, no model. The same dataset gives the same bytes on every run.
//!
//! Modules: [`dataset`] (manifests, splits, sandboxes), [`transcript`] (transcripts as hook
//! events), [`replay`] (the runtime, control arm), [`score`] (metrics), [`safety`] (the
//! directive oracle), [`fidelity`] (replay versus what the live hooks recorded), [`conventions`]
//! (learned routes versus a sandbox's known conventions), [`generate`] (the synthetic and
//! adversarial datasets), [`cli`].

pub mod bench;
pub mod cli;
pub mod conventions;
pub mod dataset;
pub mod embed_eval;
pub mod exec_e1;
pub mod fidelity;
pub mod generate;
pub mod graph_mine;
pub mod replay;
/// The safety oracle, re-exported where the eval and its tests have always named it.
pub use adage_safety as safety;
pub mod savings_replay;
pub mod score;
pub mod transcript;
pub mod value_types;
pub mod value_types_e2e;

use adage_core::CommandTable;
use adage_learn::{Inputs, LearnConfig};
use adage_tracks::params::EngineParams;
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
    /// Engine parameters laid over the thresholds preset (a partial `EngineParams` object, as
    /// `--params <file>` reads it); `None` is the preset exactly.
    pub params: Option<serde_json::Value>,
    /// Instincts (prebuilt routes bound from the repository's files): off by default, so the
    /// eval's numbers are those of learned routes alone.
    pub instincts: InstinctMode,
    /// The repository's real files, for binding instincts (a dataset's `sandbox.txt` lists names
    /// only). Default: `evals/sandboxes/<repo>` beside the datasets directory.
    pub sandbox_dir: Option<PathBuf>,
    /// Estimate E1 prefetch on the scored sessions (`--exec-e1`, [`exec_e1`]).
    pub exec_e1: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            split: None,
            thresholds: None,
            window: 3,
            include_steered: false,
            index: None,
            params: None,
            instincts: InstinctMode::Off,
            sandbox_dir: None,
            exec_e1: false,
        }
    }
}

/// Whether the eval binds instincts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstinctMode {
    /// Learned routes only (the default; the committed numbers).
    #[default]
    Off,
    /// Learned routes, then instincts bound from the sandbox (`--instincts`): what `adage condition`
    /// does by default.
    With,
    /// Instincts only (`--instincts-only`): nothing is learned from the train sessions; the index
    /// holds only what binds from the sandbox's files. Day-0 value.
    Only,
}

impl InstinctMode {
    pub fn as_str(self) -> &'static str {
        match self {
            InstinctMode::Off => "off",
            InstinctMode::With => "with",
            InstinctMode::Only => "only",
        }
    }
}

/// The directory of a dataset's repository files for binding instincts: `--sandbox-dir`, else
/// `<datasets>/../sandboxes/<repo>`.
pub fn sandbox_files(dir: &Path, ds: &Dataset, opts: &Options) -> Result<PathBuf> {
    let p = match &opts.sandbox_dir {
        Some(p) => p.clone(),
        None => dir.join("../../sandboxes").join(&ds.manifest.repo),
    };
    if !p.is_dir() {
        anyhow::bail!(
            "binding instincts needs the repository's files; {} does not exist (pass --sandbox-dir <dir>)",
            p.display()
        );
    }
    Ok(p)
}

/// The instinct configuration for an eval: the default packs, the sandbox as the repository's root,
/// and no spools (nothing to extinguish).
fn instinct_config(repo: &str, root: &Path) -> adage_learn::instincts::Config {
    let mut c = adage_learn::instincts::Config::default();
    c.roots.insert(repo.to_string(), root.to_path_buf());
    c
}

/// An index of instincts alone, bound from `root` for `repo`: nothing learned from any session.
pub fn instincts_index(repo: &str, root: &Path) -> Result<Index> {
    let home = tempfile::tempdir()?;
    let index_path = home.path().join("index.json");
    adage_learn::instincts::learn_only(
        repo,
        root,
        &home.path().join("tracks"),
        &index_path,
        &instinct_config(repo, root),
    )?;
    Ok(Index::load(&index_path))
}

#[derive(Debug, Clone, Serialize)]
pub struct LearnedRoute {
    pub id: String,
    pub shape: String,
    pub strength: String,
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
    /// Tasks with sessions in both train and test (a session without a `task` is its own task).
    pub task_overlap: usize,
    pub task_overlap_ids: Vec<String>,
    /// Train and test share a task or a session: the numbers are not held-out-task numbers.
    pub leaky: bool,
    /// The split's note (the manifest's, or what a computed split does).
    pub split_note: Option<String>,
    pub index_generation: String,
    /// SHA-256 of the engine parameters' canonical JSON (`EngineParams::params_hash`).
    pub params_hash: String,
    pub routes: Vec<LearnedRoute>,
    pub overall: score::Metrics,
    pub by_shape: BTreeMap<String, score::Metrics>,
    pub by_route: BTreeMap<String, score::Metrics>,
    pub recall: BTreeMap<String, score::Recall>,
    pub dead_ends: score::DeadEnds,
    pub finish_line: score::FinishLine,
    pub safety: score::Safety,
    /// The hook's no-match fast path over every replayed event (`replay::FastCheck`).
    pub fast_path: replay::FastCheck,
    pub sessions: Vec<score::SessionScore>,
    /// Instinct directives against what the agents did; `None` when instincts were off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instincts: Option<InstinctsReport>,
    /// The E1 prefetch estimate, with `--exec-e1`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_e1: Option<exec_e1::ExecE1>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstinctsReport {
    /// `with` (after learning) or `only` (nothing learned: day 0).
    pub mode: InstinctMode,
    #[serde(flatten)]
    pub stats: score::Instincts,
}

fn step_text(s: &adage_tracks::Step) -> String {
    match s {
        adage_tracks::Step::Run { command } => command.clone(),
        adage_tracks::Step::ReadBatch { slot, max } => format!("read {{{slot}}} (max {max})"),
        adage_tracks::Step::Avoid { avoid, instead, .. } => format!("avoid {avoid}; instead {instead}"),
        adage_tracks::Step::Advise { template, slots } => {
            let s: Vec<String> = slots.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("advise {template} ({})", s.join(", "))
        }
        adage_tracks::Step::Sequence { commands } => commands.join(" then "),
        adage_tracks::Step::Card { card } => format!("card of {} file(s)", card.files.len()),
    }
}

/// Learn tracks from `train` transcripts into a fresh temporary home; return the compiled index.
pub fn learn_index(ds: &Dataset, train: &[String], cfg: &LearnConfig) -> Result<Index> {
    let home = tempfile::tempdir()?;
    let mut transcripts = Vec::new();
    let mut outcomes = BTreeMap::new();
    let mut identities = BTreeMap::new();
    for id in train {
        let s = ds.session(id).with_context(|| format!("split names unknown session {id}"))?;
        transcripts.push(ds.path(s));
        identities
            .insert(s.id.clone(), adage_learn::EvidenceIdentity { task_id: s.task.clone(), ..Default::default() });
        if let Some(p) = s.passed {
            outcomes.insert(s.id.clone(), p);
        }
    }
    let inputs = Inputs {
        identities,
        transcripts,
        outcomes,
        repo_override: Some(ds.manifest.repo.clone()),
        models: None,
        cache: None,
        only_repo: None,
    };
    let index_path = home.path().join("index.json");
    adage_learn::learn(&inputs, &home.path().join("tracks"), &index_path, cfg)?;
    Ok(Index::load(&index_path))
}

pub fn config(name: &str) -> Result<LearnConfig> {
    Ok(LearnConfig::from_params(&EngineParams::preset(name)?.learn))
}

/// The engine parameters for a thresholds preset (`demo` or `default`) with `overrides` laid
/// over it.
pub fn engine_params(thresholds: &str, overrides: Option<&serde_json::Value>) -> Result<EngineParams> {
    let base = EngineParams::preset(thresholds)?;
    match overrides {
        Some(o) => base.with_overrides(o),
        None => Ok(base),
    }
}

/// A dataset with its split resolved and an index learned from (or loaded for) the train side.
pub struct Prepared {
    pub ds: Dataset,
    pub split_name: String,
    pub split: dataset::Split,
    pub thresholds: String,
    pub index: Index,
    /// Tasks on both sides of the split.
    pub overlap: Vec<String>,
    /// The engine parameters: the index was learned with `learn`; replay decides with `runtime`.
    pub params: EngineParams,
    /// The index file given with `--index`: nothing was learned from the dataset's sessions.
    pub index_file: Option<PathBuf>,
}

/// Load a dataset, resolve the split, and learn the index from the train split, exactly as
/// `adage eval` does (shared by [`run`] and [`conventions::run`]).
pub fn prepare(dir: &Path, opts: &Options) -> Result<Prepared> {
    let ds = Dataset::load(dir)?;
    let split_name = opts.split.clone().unwrap_or_else(|| ds.default_split());
    let split = ds.split(&split_name)?;
    let thresholds = opts.thresholds.clone().unwrap_or_else(|| ds.manifest.learn.clone());
    let params = engine_params(&thresholds, opts.params.as_ref())?;
    let mut cfg = LearnConfig::from_params(&params.learn);
    let index = match (&opts.index, opts.instincts) {
        (Some(p), _) => {
            let p = if p.is_relative() && !p.exists() { dir.join(p) } else { p.clone() };
            serde_json::from_slice::<Index>(&std::fs::read(&p).with_context(|| p.display().to_string())?)?
        }
        (None, InstinctMode::Off) => {
            // The repository's files, when given, are read for the value model's static scan
            // (`adage_learn::value::Statics`), with no instinct bound.
            if let Some(d) = &opts.sandbox_dir {
                cfg.roots.insert(ds.manifest.repo.clone(), d.clone());
            }
            learn_index(&ds, &split.train, &cfg)?
        }
        (None, InstinctMode::With) => {
            cfg.instincts = Some(instinct_config(&ds.manifest.repo, &sandbox_files(dir, &ds, opts)?));
            learn_index(&ds, &split.train, &cfg)?
        }
        (None, InstinctMode::Only) => instincts_index(&ds.manifest.repo, &sandbox_files(dir, &ds, opts)?)?,
    };
    let overlap = ds.task_overlap(&split);
    let index_file = opts.index.clone();
    Ok(Prepared { ds, split_name, split, thresholds, index, overlap, params, index_file })
}

/// Run the offline eval over a dataset directory.
pub fn run(dir: &Path, opts: &Options) -> Result<Report> {
    evaluate(&prepare(dir, opts)?, opts)
}

/// Replay and score a prepared dataset (the second half of [`run`]).
pub fn evaluate(p: &Prepared, opts: &Options) -> Result<Report> {
    let Prepared { ds, split_name, split, thresholds, index, overlap, params, .. } = p;
    let table = CommandTable::load();
    let sandbox = Sandbox::create(&ds.sandbox()?)?;
    let repo_key = ds.manifest.repo.clone();

    let mut tally = score::Tally::default();
    let mut fast = replay::FastCheck::default();
    let mut e1 = opts.exec_e1.then(exec_e1::ExecE1::default);
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
        let rep = replay::replay(
            &t,
            &adage_runtime::Ctx {
                checkout: &adage_runtime::checkout::Disk,
                index,
                repo: repo_key.to_string(),
                root: &sandbox.root,
                table,
                embed: None,
                params: params.runtime,
                policy: Default::default(),
            },
            meta.stopped(),
        );
        fast.events += rep.fast.events;
        fast.skipped += rep.fast.skipped;
        fast.mismatches.extend(rep.fast.mismatches.iter().cloned());
        if let Some(e) = e1.as_mut() {
            e.add(&exec_e1::measure(&rep, index, &repo_key, table));
        }
        score::score_session(
            &mut tally,
            &rep,
            Some(meta),
            index,
            &repo_key,
            &sandbox.root,
            table,
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
            shape: score::point(&ir.route),
            strength: format!("{:?}", ir.route.strength).to_lowercase(),
            step: step_text(&ir.route.step),
            support: ir.route.evidence.support,
            opportunities: ir.route.evidence.opportunities,
        })
        .collect();
    Ok(Report {
        dataset: ds.manifest.name.clone(),
        split: split_name.clone(),
        thresholds: thresholds.clone(),
        window: opts.window,
        train_sessions: split.train.len(),
        test_sessions: split.test.len(),
        scored_sessions: scored,
        skipped_steered: skipped,
        in_sample,
        task_overlap: overlap.len(),
        leaky: !overlap.is_empty() || in_sample > 0,
        task_overlap_ids: overlap.clone(),
        split_note: split.note.clone(),
        index_generation: index.generation.clone(),
        params_hash: params.params_hash(),
        routes,
        overall: tally.overall,
        by_shape: tally.by_shape,
        by_route: tally.by_route,
        recall: tally.recall,
        dead_ends: tally.dead_ends,
        finish_line: tally.finish_line,
        safety: tally.safety,
        fast_path: fast,
        sessions: tally.sessions,
        instincts: (opts.instincts != InstinctMode::Off)
            .then(|| InstinctsReport { mode: opts.instincts, stats: tally.instincts.clone() }),
        exec_e1: e1,
    })
}

/// `n` and a noun, pluralised (`1 reflex`, `3 reflexes`).
pub(crate) fn count(n: usize, noun: &str) -> String {
    match (n, noun.ends_with('x')) {
        (1, _) => format!("1 {noun}"),
        (_, true) => format!("{n} {noun}es"),
        _ => format!("{n} {noun}s"),
    }
}

fn pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

/// The split's note and its leakage line, for both text reports.
pub(crate) fn leakage_text(note: Option<&str>, overlap: &[String], leaky: bool) -> String {
    let mut s = String::new();
    if let Some(n) = note {
        s.push_str(&format!("split note: {n}\n"));
    }
    s.push_str(&format!("task overlap between train and test: {} tasks", overlap.len()));
    if leaky {
        const SHOWN: usize = 8;
        let mut ids = overlap.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
        if overlap.len() > SHOWN {
            ids.push_str(", …");
        }
        if ids.is_empty() {
            s.push_str(" — LEAKY: traces are in both train and test");
        } else {
            s.push_str(&format!(" — LEAKY: test tasks were trained on ({ids})"));
        }
    }
    s.push('\n');
    s
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
        "traces: {} train, {} test, {} scored, {} skipped (steered){}",
        r.train_sessions,
        r.test_sessions,
        r.scored_sessions,
        r.skipped_steered,
        if r.in_sample > 0 { format!(", {} IN-SAMPLE", r.in_sample) } else { String::new() }
    );
    s.push_str(&leakage_text(r.split_note.as_deref(), &r.task_overlap_ids, r.leaky));
    let _ = writeln!(s, "index {} with {}", r.index_generation, count(r.routes.len(), "reflex"));
    for x in &r.routes {
        let _ = writeln!(
            s,
            "  {:<22} {:<12} {:<7} {:>3}/{:<3} {}",
            x.id, x.shape, x.strength, x.support, x.opportunities, x.step
        );
    }
    let _ = writeln!(
        s,
        "\ndirectives        given  correct  precision  95% CI          @window  false-rate  turns-saved (median)"
    );
    let mut row = |name: &str, m: &score::Metrics| {
        let _ = writeln!(
            s,
            "  {:<15} {:>5}  {:>7}  {:>9}  {:<14}  {:>7}  {:>10}  {:>6} ({})",
            name,
            m.directives,
            m.correct,
            pct(m.precision),
            score::ci_text(m.precision_ci),
            pct(m.precision_in_window),
            pct(m.false_directive_rate),
            m.turns_saved,
            m.turns_saved_median
        );
    };
    row("all", &r.overall);
    for (k, m) in &r.by_shape {
        row(&format!("shape:{k}"), m);
    }
    for (k, m) in &r.by_route {
        row(k, m);
    }
    let _ = writeln!(s, "\nrecall (key steps with a directive first)");
    for (k, x) in &r.recall {
        let _ = writeln!(
            s,
            "  {:<15} {:>3}/{:<3} {:>6}  95% CI {}",
            k,
            x.covered,
            x.key_steps,
            pct(x.recall),
            score::ci_text(x.recall_ci)
        );
    }
    let d = &r.dead_ends;
    let _ = writeln!(
        s,
        "\ndead ends: {} warnings; avoided command run after {} ({} failed as predicted, {} passed): accuracy {} (95% CI {}); {}/{} failed verify runs came after a warning",
        d.warnings, d.tested, d.confirmed, d.refuted, pct(d.accuracy), score::ci_text(d.accuracy_ci), d.failed_runs_after_warning, d.failed_verify_runs
    );
    let f = &r.finish_line;
    let _ = writeln!(
        s,
        "finish line: {} obligations — {} met by the agent, {} needed but skipped, {} false alarms, {} unlabelled: accuracy {} (95% CI {}); {} stale traces missed; stop requests {} ({} correct)",
        f.obligations, f.met, f.caught, f.false_alarms, f.unknown, pct(f.accuracy), score::ci_text(f.accuracy_ci), f.missed, f.stop_requests, f.stop_requests_correct
    );
    if let Some(i) = &r.instincts {
        let x = &i.stats;
        let _ = writeln!(
            s,
            "instincts ({}): {} directives, {} correct: precision {} (95% CI {}); dead ends {}/{}, finish lines {}/{}; {} advice; failed verify runs answered {}/{}: recall {} (95% CI {})",
            i.mode.as_str(),
            x.directives,
            x.correct,
            pct(x.precision),
            score::ci_text(x.precision_ci),
            x.dead_end_correct,
            x.dead_end_directives,
            x.finish_line_correct,
            x.finish_line_directives,
            x.advice,
            x.failures_answered,
            x.failed_verify_runs,
            pct(x.recall),
            score::ci_text(x.recall_ci)
        );
    }
    let _ = writeln!(s, "safety: {} checked, {} unsafe", r.safety.checked, r.safety.unsafe_directives);
    let f = &r.fast_path;
    let _ = writeln!(
        s,
        "fast path: {} of {} events skipped with no match; {} decisions differ",
        f.skipped,
        f.events,
        f.mismatches.len()
    );
    for m in &f.mismatches {
        let _ = writeln!(s, "  DIFFERS {m}");
    }
    for v in &r.safety.violations {
        let _ = writeln!(s, "  UNSAFE {}: {}", v.route, v.reason);
    }
    if let Some(e) = &r.exec_e1 {
        s.push_str(&e.text());
    }
    s
}
