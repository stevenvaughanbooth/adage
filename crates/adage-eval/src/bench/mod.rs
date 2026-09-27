//! The L0 optimisation bench: `adage eval bench`.
//!
//! L0 is the offline tier: learn from a train split, replay held-out sessions through the
//! runtime, score what Adage would have said (the rest of this crate). The bench runs a fixed
//! **suite** of such evaluations for one setting of the engine's parameters
//! ([`adage_tracks::params::EngineParams`]), reduces them to one **objective** and a few
//! **guardrails**, and writes every number as JSON Lines in one schema
//! (`evals/bench/result.schema.json`), so settings can be compared, swept and blessed.
//!
//! - [`run`]: one setting, one suite → `run`, `trial`, `directive`, `aggregate` and `scorecard`
//!   records.
//! - [`space`] / [`sweep`]: many settings, sampled deterministically from a parameter space.
//! - [`report`]: rank a results file, its Pareto front, deltas against a baseline, and
//!   [`report::bless`] a run as the committed baseline (`evals/baselines/l0-core.jsonl`).
//!
//! **Objective.** With λ_fp and λ_gt from the suite (2.0 and 1.0 by default), per evaluation unit
//! (dataset × split):
//!
//! ```text
//! J = turns_saved_at_window_per_session − λ_fp · false_directives_per_session + λ_gt · route_F1
//! ```
//!
//! The ranking objective is the mean of J over the suite's cross-validation units (the
//! task-grouped folds: no task on both sides of any split). The temporal split is a final check,
//! reported separately (`scorecard.final`) and never used for ranking.
//!
//! **Guardrails** (a setting that fails one is infeasible, whatever its objective): no unsafe
//! directive on any unit (the adversarial set included); no fast-path mismatch; cross-validated
//! route precision at least the baseline's less `route_precision_margin` (0.05).
//!
//! Everything is deterministic: the same suite, datasets and parameters give the same bytes. No
//! wall clock is recorded; `git_sha` only when given (`--git-sha` or `ADAGE_GIT_SHA`) and never
//! with `--deterministic`.

pub mod cli;
pub mod report;
pub mod space;
pub mod sweep;

use crate::score::{self, round, wilson};
use crate::{conventions, Options};
use adage_tracks::params::{canonical_json, EngineParams};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The result schema's version (`schema_version` in every record).
pub const SCHEMA_VERSION: u32 = 1;
pub const TIER: &str = "l0";
/// The built-in `core` suite.
pub const CORE_SUITE: &str = include_str!("../../../../evals/bench/suites/core.toml");

/// What a unit is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Cross-validation: its J is averaged into the ranking objective.
    Cv,
    /// The final check (the temporal split): reported, never ranked on.
    Final,
    /// Guardrails only (the adversarial set).
    Safety,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Cv => "cv",
            Role::Final => "final",
            Role::Safety => "safety",
        }
    }
}

/// One evaluation: a dataset and a split.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub dataset: String,
    pub split: String,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Objective {
    pub lambda_fp: f64,
    pub lambda_gt: f64,
    /// The route-precision guardrail: at least the baseline's less this.
    pub route_precision_margin: f64,
}

impl Default for Objective {
    fn default() -> Self {
        Objective { lambda_fp: 2.0, lambda_gt: 1.0, route_precision_margin: 0.05 }
    }
}

/// A suite: which evaluations make up one bench run, and how they are scored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The parameter preset a run starts from (`demo` or `default`); `--params` overrides it.
    #[serde(default = "default_preset")]
    pub preset: String,
    /// Model turns within which a followed directive counts as prompt (scoring, not a parameter).
    #[serde(default = "default_window")]
    pub window: u32,
    #[serde(default)]
    pub objective: Objective,
    pub units: Vec<Unit>,
}

fn default_preset() -> String {
    "demo".into()
}

fn default_window() -> u32 {
    3
}

impl Suite {
    /// `core` (built in) or a suite file (`.toml`).
    pub fn load(name: &str) -> Result<Suite> {
        let (text, what) = if name == "core" {
            (CORE_SUITE.to_string(), "core".to_string())
        } else {
            (std::fs::read_to_string(name).with_context(|| name.to_string())?, name.to_string())
        };
        let s: Suite = toml::from_str(&text).with_context(|| format!("suite {what}"))?;
        if s.units.is_empty() {
            bail!("suite {what}: no units");
        }
        if !s.units.iter().any(|u| u.role == Role::Cv) {
            bail!("suite {what}: no cross-validation unit to rank on");
        }
        EngineParams::preset(&s.preset).with_context(|| format!("suite {what}"))?;
        Ok(s)
    }

    /// The suite's definition as canonical JSON (it is part of the run id).
    pub fn canonical(&self) -> String {
        canonical_json(&serde_json::to_value(self).unwrap_or(Value::Null))
    }
}

/// Where and how a bench runs.
#[derive(Debug, Clone)]
pub struct Env {
    pub suite: Suite,
    /// The directory holding the datasets the suite names.
    pub datasets: PathBuf,
    /// Recorded in every record unless `deterministic`.
    pub git_sha: Option<String>,
    pub deterministic: bool,
    /// SHA-256 of each dataset's files, computed once.
    pub dataset_sha: BTreeMap<String, String>,
    /// Bind instincts after learning (`--instincts`); off by default, so the committed baseline is
    /// learned routes alone.
    pub instincts: crate::InstinctMode,
}

impl Env {
    pub fn new(suite: Suite, datasets: &Path, git_sha: Option<String>, deterministic: bool) -> Result<Env> {
        let mut dataset_sha = BTreeMap::new();
        for u in &suite.units {
            if !dataset_sha.contains_key(&u.dataset) {
                let dir = datasets.join(&u.dataset);
                dataset_sha.insert(u.dataset.clone(), content_sha256(&dir)?);
            }
        }
        Ok(Env {
            suite,
            datasets: datasets.to_path_buf(),
            git_sha,
            deterministic,
            dataset_sha,
            instincts: crate::InstinctMode::Off,
        })
    }

    /// The deterministic id of a run: the hash of its inputs (schema, suite, datasets, params).
    pub fn run_id(&self, params: &EngineParams) -> String {
        let mut inputs = json!({
            "schema_version": SCHEMA_VERSION,
            "tier": TIER,
            "suite": serde_json::to_value(&self.suite).unwrap_or(Value::Null),
            "datasets": self.dataset_sha,
            "params_hash": params.params_hash(),
        });
        // Only a run with instincts says so: a run without them keeps the id it always had.
        if self.instincts != crate::InstinctMode::Off {
            inputs["instincts"] = json!(self.instincts.as_str());
        }
        adage_tracks::io::hex(&Sha256::digest(canonical_json(&inputs).as_bytes()))[..16].to_string()
    }
}

/// SHA-256 over a dataset directory: every file's relative path and content hash, in path order.
pub fn content_sha256(dir: &Path) -> Result<String> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).with_context(|| d.display().to_string())? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                files.push((rel, p));
            }
        }
    }
    files.sort();
    let mut h = Sha256::new();
    for (rel, p) in files {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(Sha256::digest(std::fs::read(&p)?));
    }
    Ok(adage_tracks::io::hex(&h.finalize()))
}

/// Counts from one or more units, from which every metric is computed.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub sessions: u32,
    pub directives: u32,
    pub correct: u32,
    pub correct_in_window: u32,
    pub key_steps: u32,
    pub covered: u32,
    pub dead_ends_tested: u32,
    pub dead_ends_confirmed: u32,
    pub finish_line_right: u32,
    pub finish_line_labelled: u32,
    pub unsafe_directives: u32,
    pub fast_path_mismatches: u32,
    pub routes_learned: u32,
    pub routes_true: u32,
    pub conventions: u32,
    pub conventions_found: u32,
    /// Per scored session: turns saved by directives followed within the window.
    pub turns_saved_at_window: Vec<f64>,
    /// Per scored session: directives the agent did not follow.
    pub false_directives: Vec<f64>,
    /// Per directive: its route's Wilson lower bound, and whether it was correct.
    pub calibration: Vec<(f64, bool)>,
}

impl Stats {
    fn add(&mut self, o: &Stats) {
        self.sessions += o.sessions;
        self.directives += o.directives;
        self.correct += o.correct;
        self.correct_in_window += o.correct_in_window;
        self.key_steps += o.key_steps;
        self.covered += o.covered;
        self.dead_ends_tested += o.dead_ends_tested;
        self.dead_ends_confirmed += o.dead_ends_confirmed;
        self.finish_line_right += o.finish_line_right;
        self.finish_line_labelled += o.finish_line_labelled;
        self.unsafe_directives += o.unsafe_directives;
        self.fast_path_mismatches += o.fast_path_mismatches;
        self.routes_learned += o.routes_learned;
        self.routes_true += o.routes_true;
        self.conventions += o.conventions;
        self.conventions_found += o.conventions_found;
        self.turns_saved_at_window.extend(&o.turns_saved_at_window);
        self.false_directives.extend(&o.false_directives);
        self.calibration.extend(&o.calibration);
    }

    pub fn route_precision(&self) -> f64 {
        score::ratio(self.routes_true, self.routes_learned)
    }

    pub fn route_recall(&self) -> f64 {
        score::ratio(self.conventions_found, self.conventions)
    }

    pub fn route_f1(&self) -> f64 {
        f1(self.route_precision(), self.route_recall())
    }

    pub fn turns_saved_per_session(&self) -> f64 {
        mean(&self.turns_saved_at_window)
    }

    pub fn false_directives_per_session(&self) -> f64 {
        mean(&self.false_directives)
    }

    /// J for these counts.
    pub fn objective(&self, o: &Objective) -> f64 {
        round(
            self.turns_saved_per_session() - o.lambda_fp * self.false_directives_per_session()
                + o.lambda_gt * self.route_f1(),
        )
    }

    /// Expected calibration error of route lower bounds against the offline correct rate, over
    /// five equal-width bins (weighted by directives).
    pub fn ece(&self) -> f64 {
        let n = self.calibration.len();
        if n == 0 {
            return 0.0;
        }
        let mut bins = [(0.0f64, 0u32, 0u32); 5];
        for (lb, ok) in &self.calibration {
            let b = ((lb * 5.0).floor() as usize).min(4);
            bins[b].0 += lb;
            bins[b].1 += u32::from(*ok);
            bins[b].2 += 1;
        }
        let e: f64 = bins
            .iter()
            .filter(|b| b.2 > 0)
            .map(|(sum, ok, k)| (*k as f64 / n as f64) * (sum / *k as f64 - *ok as f64 / *k as f64).abs())
            .sum();
        round(e)
    }
}

fn f1(p: f64, r: f64) -> f64 {
    if p + r > 0.0 {
        round(2.0 * p * r / (p + r))
    } else {
        0.0
    }
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        round(v.iter().sum::<f64>() / v.len() as f64)
    }
}

/// A normal-approximation 95% interval for a mean; `None` with fewer than two values.
fn mean_ci(v: &[f64]) -> Option<[f64; 2]> {
    let n = v.len();
    if n < 2 {
        return None;
    }
    let m = v.iter().sum::<f64>() / n as f64;
    let var = v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1) as f64;
    let half = 1.959_963_984_540_054 * (var / n as f64).sqrt();
    Some([round(m - half), round(m + half)])
}

/// One unit's measurement.
#[derive(Debug, Clone)]
pub struct UnitResult {
    pub unit: Unit,
    pub report: crate::Report,
    pub routes: conventions::RoutesReport,
    /// Session id → task.
    pub tasks: BTreeMap<String, String>,
    pub stats: Stats,
}

/// Every unit of a suite, measured with one setting.
#[derive(Debug, Clone)]
pub struct Measured {
    pub params: EngineParams,
    pub units: Vec<UnitResult>,
}

/// The engine parameters a bench run uses: the suite's preset with `overrides` laid over it.
pub fn params_for(suite: &Suite, overrides: Option<&Value>) -> Result<EngineParams> {
    crate::engine_params(&suite.preset, overrides)
}

/// Run every unit of the suite with `params`.
pub fn measure(env: &Env, params: &EngineParams) -> Result<Measured> {
    let mut units = Vec::new();
    for u in &env.suite.units {
        let dir = env.datasets.join(&u.dataset);
        // `params` is complete: the preset only fills what the overrides leave out.
        let opts = Options {
            split: Some(u.split.clone()),
            thresholds: Some(env.suite.preset.clone()),
            window: env.suite.window,
            params: Some(serde_json::to_value(params)?),
            instincts: env.instincts,
            ..Default::default()
        };
        let prepared = crate::prepare(&dir, &opts).with_context(|| format!("bench unit {} {}", u.dataset, u.split))?;
        let report = crate::evaluate(&prepared, &opts)?;
        let routes = conventions::score_prepared(&prepared)?;
        let lower_bounds: BTreeMap<String, f64> = prepared
            .index
            .for_repo(&prepared.ds.manifest.repo)
            .map(|ir| (ir.route.id.clone(), ir.route.evidence.lower_bound))
            .collect();
        let tasks: BTreeMap<String, String> =
            prepared.ds.manifest.sessions.iter().map(|s| (s.id.clone(), s.task_key().to_string())).collect();
        let stats = unit_stats(&report, &routes, &lower_bounds, env.suite.window);
        units.push(UnitResult { unit: u.clone(), report, routes, tasks, stats });
    }
    Ok(Measured { params: params.clone(), units })
}

/// Whether a scored directive was followed within the window (as `score` counts it).
fn in_window(d: &score::Scored, window: u32) -> bool {
    d.correct && d.matched_turn.is_some_and(|t| t.saturating_sub(d.turn) <= window)
}

fn unit_stats(
    r: &crate::Report,
    routes: &conventions::RoutesReport,
    lower_bounds: &BTreeMap<String, f64>,
    window: u32,
) -> Stats {
    let mut s = Stats {
        sessions: r.scored_sessions as u32,
        directives: r.overall.directives,
        correct: r.overall.correct,
        correct_in_window: r.overall.correct_in_window,
        key_steps: r.recall.get("all").map_or(0, |x| x.key_steps),
        covered: r.recall.get("all").map_or(0, |x| x.covered),
        dead_ends_tested: r.dead_ends.tested,
        dead_ends_confirmed: r.dead_ends.confirmed,
        finish_line_right: r.finish_line.met + r.finish_line.caught,
        finish_line_labelled: r.finish_line.met + r.finish_line.caught + r.finish_line.false_alarms,
        unsafe_directives: r.safety.unsafe_directives,
        fast_path_mismatches: r.fast_path.mismatches.len() as u32,
        routes_learned: routes.learned_routes as u32,
        routes_true: routes.true_routes as u32,
        conventions: routes.conventions_evaluable as u32,
        conventions_found: routes.conventions_found as u32,
        ..Default::default()
    };
    for sess in &r.sessions {
        let saved: u32 = sess.directives.iter().filter(|d| in_window(d, window)).map(|d| d.turns_saved).sum();
        s.turns_saved_at_window.push(saved as f64);
        s.false_directives.push(sess.directives.iter().filter(|d| !d.correct).count() as f64);
        for d in &sess.directives {
            if let Some(lb) = lower_bounds.get(&d.route) {
                s.calibration.push((*lb, d.correct));
            }
        }
    }
    s
}

/// One `aggregate` metric: (metric, estimate, [low, high], interval method, n).
pub type Metric = (&'static str, f64, Option<[f64; 2]>, &'static str, u32);

/// The `aggregate` metrics of a set of counts.
pub fn metrics(s: &Stats, o: &Objective) -> Vec<Metric> {
    let w = |k: u32, n: u32| Some(wilson(k, n));
    vec![
        ("precision", score::ratio(s.correct, s.directives), w(s.correct, s.directives), "wilson", s.directives),
        (
            "precision_at_window",
            score::ratio(s.correct_in_window, s.directives),
            w(s.correct_in_window, s.directives),
            "wilson",
            s.directives,
        ),
        (
            "false_directive_rate",
            if s.directives == 0 { 0.0 } else { round(1.0 - score::ratio(s.correct, s.directives)) },
            w(s.directives - s.correct, s.directives),
            "wilson",
            s.directives,
        ),
        ("recall", score::ratio(s.covered, s.key_steps), w(s.covered, s.key_steps), "wilson", s.key_steps),
        (
            "turns_saved_at_window_per_session",
            s.turns_saved_per_session(),
            mean_ci(&s.turns_saved_at_window),
            "normal",
            s.sessions,
        ),
        (
            "false_directives_per_session",
            s.false_directives_per_session(),
            mean_ci(&s.false_directives),
            "normal",
            s.sessions,
        ),
        (
            "dead_end_accuracy",
            score::ratio(s.dead_ends_confirmed, s.dead_ends_tested),
            w(s.dead_ends_confirmed, s.dead_ends_tested),
            "wilson",
            s.dead_ends_tested,
        ),
        (
            "finish_line_accuracy",
            score::ratio(s.finish_line_right, s.finish_line_labelled),
            w(s.finish_line_right, s.finish_line_labelled),
            "wilson",
            s.finish_line_labelled,
        ),
        ("unsafe_directives", s.unsafe_directives as f64, None, "count", s.directives),
        ("fast_path_mismatches", s.fast_path_mismatches as f64, None, "count", s.sessions),
        ("route_precision", s.route_precision(), w(s.routes_true, s.routes_learned), "wilson", s.routes_learned),
        ("route_recall", s.route_recall(), w(s.conventions_found, s.conventions), "wilson", s.conventions),
        ("route_f1", s.route_f1(), None, "none", s.routes_learned),
        ("calibration_ece", s.ece(), None, "none", s.calibration.len() as u32),
        ("objective", s.objective(o), None, "none", s.sessions),
    ]
}

/// The kind of a split name: `group` (`group:task:k:fold`), `hash`, `temporal`, `all` or `named`;
/// and its seed, where it has one.
pub fn split_meta(name: &str) -> Value {
    let (kind, seed) = if name.starts_with("group:") {
        ("group", None)
    } else if let Some(rest) = name.strip_prefix("hash:") {
        ("hash", Some(rest.split_once(':').map_or("adage-eval", |(_, s)| s).to_string()))
    } else if name == "temporal" || name.starts_with("temporal:") {
        ("temporal", None)
    } else if name == "all" {
        ("all", None)
    } else {
        ("named", None)
    };
    json!({"kind": kind, "name": name, "seed": seed})
}

/// The records of one bench run, and its scorecard.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub run_id: String,
    pub records: Vec<Value>,
    pub scorecard: Scorecard,
}

/// A guardrail's verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guardrail {
    pub name: String,
    pub value: f64,
    /// `<= limit` for counts, `>= limit` for route precision (`op` says which).
    pub limit: f64,
    pub op: String,
    pub pass: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scorecard {
    pub label: String,
    /// The mean of J over the cross-validation units.
    pub objective: f64,
    pub feasible: bool,
    /// The objective's terms, pooled over the cross-validation units, and J per unit.
    pub terms: BTreeMap<String, Value>,
    pub guardrails: Vec<Guardrail>,
    /// The final check (the `final` units, the temporal split): reported, never ranked on.
    #[serde(rename = "final")]
    pub final_check: BTreeMap<String, Value>,
}

/// Records of a measured run. `baseline_route_precision`: the route-precision guardrail's
/// reference (`None`: this run's own). `detail`: include `trial` and `directive` records
/// (aggregates, per unit and pooled, are always included).
pub fn records(env: &Env, m: &Measured, label: &str, baseline_route_precision: Option<f64>, detail: bool) -> RunOutput {
    let o = &env.suite.objective;
    let run_id = env.run_id(&m.params);
    let params_value = serde_json::to_value(&m.params).unwrap_or(Value::Null);
    let envelope = |record: &str, unit: Option<&Unit>, split: Value| -> Map<String, Value> {
        let mut e = Map::new();
        e.insert("schema_version".into(), json!(SCHEMA_VERSION));
        e.insert("record".into(), json!(record));
        e.insert("run_id".into(), json!(run_id));
        e.insert("tier".into(), json!(TIER));
        e.insert("suite".into(), json!(env.suite.name));
        if let (Some(sha), false) = (&env.git_sha, env.deterministic) {
            e.insert("git_sha".into(), json!(sha));
        }
        e.insert("adage_version".into(), json!(env!("CARGO_PKG_VERSION")));
        e.insert("params".into(), params_value.clone());
        e.insert("params_hash".into(), json!(m.params.params_hash()));
        if let Some(u) = unit {
            e.insert(
                "dataset".into(),
                json!({"id": u.dataset, "content_sha256": env.dataset_sha.get(&u.dataset).cloned().unwrap_or_default()}),
            );
        }
        e.insert("split".into(), split);
        e
    };
    let with = |mut e: Map<String, Value>, body: Value| -> Value {
        if let Value::Object(b) = body {
            e.extend(b);
        }
        Value::Object(e)
    };
    let pooled_split = |kind: &str| json!({"kind": kind, "name": kind, "seed": null});
    let mut out = Vec::new();

    // run
    let units: Vec<Value> = m
        .units
        .iter()
        .map(|u| {
            json!({
                "dataset": u.unit.dataset, "split": u.unit.split, "role": u.unit.role.as_str(),
                "train_sessions": u.report.train_sessions, "test_sessions": u.report.test_sessions,
                "scored_sessions": u.report.scored_sessions, "leaky": u.report.leaky,
                "index_generation": u.report.index_generation, "learned_routes": u.routes.learned_routes,
            })
        })
        .collect();
    out.push(with(
        envelope("run", None, pooled_split("suite")),
        json!({"label": label, "window": env.suite.window, "preset": env.suite.preset,
               "objective": env.suite.objective, "units": units}),
    ));

    let aggregate = |out: &mut Vec<Value>, unit: Option<&Unit>, split: Value, role: &str, s: &Stats| {
        for (metric, est, ci, method, n) in metrics(s, o) {
            out.push(with(
                envelope("aggregate", unit, split.clone()),
                json!({"role": role, "metric": metric, "estimate": est,
                       "ci_low": ci.map(|c| c[0]), "ci_high": ci.map(|c| c[1]), "ci_method": method, "n": n}),
            ));
        }
    };

    // per unit: trials, directives, aggregates
    for u in &m.units {
        let split = split_meta(&u.unit.split);
        if detail {
            for sess in &u.report.sessions {
                let tw: u32 =
                    sess.directives.iter().filter(|d| in_window(d, env.suite.window)).map(|d| d.turns_saved).sum();
                let correct = sess.directives.iter().filter(|d| d.correct).count();
                out.push(with(
                    envelope("trial", Some(&u.unit), split.clone()),
                    json!({
                        "role": u.unit.role.as_str(), "session": sess.id,
                        "task": u.tasks.get(&sess.id).cloned().unwrap_or_default(),
                        "turns": sess.turns, "directives": sess.directives.len(), "correct": correct,
                        "correct_in_window": sess.directives.iter().filter(|d| in_window(d, env.suite.window)).count(),
                        "false_directives": sess.directives.len() - correct,
                        "turns_saved": sess.turns_saved, "turns_saved_at_window": tw,
                        "key_steps": sess.key_steps, "covered": sess.covered,
                    }),
                ));
                for d in &sess.directives {
                    out.push(with(
                        envelope("directive", Some(&u.unit), split.clone()),
                        json!({
                            "role": u.unit.role.as_str(), "session": sess.id, "route": d.route, "kind": d.kind,
                            "event": d.event, "turn": d.turn, "correct": d.correct,
                            "in_window": in_window(d, env.suite.window), "matched_turn": d.matched_turn,
                            "turns_saved": d.turns_saved,
                        }),
                    ));
                }
            }
        }
        aggregate(&mut out, Some(&u.unit), split, u.unit.role.as_str(), &u.stats);
    }

    // pooled
    let pool = |role: Role| {
        let mut s = Stats::default();
        for u in m.units.iter().filter(|u| u.unit.role == role) {
            s.add(&u.stats);
        }
        s
    };
    let (cv, fin) = (pool(Role::Cv), pool(Role::Final));
    aggregate(&mut out, None, pooled_split("cv"), "cv", &cv);
    let folds: Vec<f64> = m.units.iter().filter(|u| u.unit.role == Role::Cv).map(|u| u.stats.objective(o)).collect();
    let objective = mean(&folds);
    let ci = mean_ci(&folds);
    out.push(with(
        envelope("aggregate", None, pooled_split("cv")),
        json!({"role": "cv", "metric": "objective_cv_mean", "estimate": objective,
               "ci_low": ci.map(|c| c[0]), "ci_high": ci.map(|c| c[1]), "ci_method": "normal", "n": folds.len()}),
    ));
    let has_final = m.units.iter().any(|u| u.unit.role == Role::Final);
    if has_final {
        aggregate(&mut out, None, pooled_split("final"), "final", &fin);
    }

    // scorecard
    let mut all = Stats::default();
    for u in &m.units {
        all.add(&u.stats);
    }
    let rp = cv.route_precision();
    let base = baseline_route_precision.unwrap_or(rp);
    let rp_limit = round(base - o.route_precision_margin);
    let guardrails = vec![
        Guardrail {
            name: "unsafe_directives".into(),
            value: all.unsafe_directives as f64,
            limit: 0.0,
            op: "<=".into(),
            pass: all.unsafe_directives == 0,
        },
        Guardrail {
            name: "fast_path_mismatches".into(),
            value: all.fast_path_mismatches as f64,
            limit: 0.0,
            op: "<=".into(),
            pass: all.fast_path_mismatches == 0,
        },
        Guardrail {
            name: "route_precision".into(),
            value: rp,
            limit: rp_limit,
            op: ">=".into(),
            pass: rp >= rp_limit - 1e-9,
        },
    ];
    let mut terms: BTreeMap<String, Value> = BTreeMap::new();
    terms.insert("turns_saved_at_window_per_session".into(), json!(cv.turns_saved_per_session()));
    terms.insert("false_directives_per_session".into(), json!(cv.false_directives_per_session()));
    terms.insert("route_f1".into(), json!(cv.route_f1()));
    terms.insert("route_precision".into(), json!(rp));
    terms.insert("route_recall".into(), json!(cv.route_recall()));
    terms.insert("precision_at_window".into(), json!(score::ratio(cv.correct_in_window, cv.directives)));
    terms.insert("recall".into(), json!(score::ratio(cv.covered, cv.key_steps)));
    terms.insert("calibration_ece".into(), json!(cv.ece()));
    terms.insert("lambda_fp".into(), json!(o.lambda_fp));
    terms.insert("lambda_gt".into(), json!(o.lambda_gt));
    let per_unit: Map<String, Value> = m
        .units
        .iter()
        .filter(|u| u.unit.role == Role::Cv)
        .map(|u| (format!("{} {}", u.unit.dataset, u.unit.split), json!(u.stats.objective(o))))
        .collect();
    terms.insert("objective_per_unit".into(), Value::Object(per_unit));
    let mut final_check: BTreeMap<String, Value> = BTreeMap::new();
    if has_final {
        let fin_units: Vec<f64> =
            m.units.iter().filter(|u| u.unit.role == Role::Final).map(|u| u.stats.objective(o)).collect();
        final_check.insert("objective".into(), json!(mean(&fin_units)));
        final_check.insert("turns_saved_at_window_per_session".into(), json!(fin.turns_saved_per_session()));
        final_check.insert("false_directives_per_session".into(), json!(fin.false_directives_per_session()));
        final_check.insert("route_f1".into(), json!(fin.route_f1()));
        final_check.insert("route_precision".into(), json!(fin.route_precision()));
    }
    let scorecard = Scorecard {
        label: label.to_string(),
        objective,
        feasible: guardrails.iter().all(|g| g.pass),
        terms,
        guardrails,
        final_check,
    };
    out.push(with(envelope("scorecard", None, pooled_split("suite")), serde_json::to_value(&scorecard).unwrap()));
    RunOutput { run_id, records: out, scorecard }
}

/// One record per line, keys sorted: the same records give the same bytes.
pub fn to_jsonl(records: &[Value]) -> String {
    let mut s = String::new();
    for r in records {
        s.push_str(&canonical_json(r));
        s.push('\n');
    }
    s
}

/// Read a results file (JSON Lines).
pub fn read_jsonl(path: &Path) -> Result<Vec<Value>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| serde_json::from_str(l).with_context(|| format!("{}:{}", path.display(), i + 1)))
        .collect()
}

/// The route precision recorded in a baseline file's scorecard.
pub fn baseline_route_precision(records: &[Value]) -> Option<f64> {
    records.iter().find(|r| r["record"] == "scorecard").and_then(|r| r["terms"]["route_precision"].as_f64())
}

/// A one-screen summary of a run's scorecard.
pub fn render_scorecard(s: &Scorecard, run_id: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "bench run {run_id} ({}): objective J = {:.4} (mean over cross-validation units){}",
        s.label,
        s.objective,
        if s.feasible { "" } else { " — INFEASIBLE" }
    );
    for (k, v) in &s.terms {
        if k != "objective_per_unit" {
            let _ = writeln!(out, "  {k:<36} {v}");
        }
    }
    if let Some(Value::Object(m)) = s.terms.get("objective_per_unit") {
        for (k, v) in m {
            let _ = writeln!(out, "  J {k:<34} {v}");
        }
    }
    for g in &s.guardrails {
        let _ = writeln!(
            out,
            "  guardrail {:<26} {} {} {} {}",
            g.name,
            g.value,
            g.op,
            g.limit,
            if g.pass { "pass" } else { "FAIL" }
        );
    }
    if !s.final_check.is_empty() {
        let _ = writeln!(out, "  final check (temporal; not ranked on):");
        for (k, v) in &s.final_check {
            let _ = writeln!(out, "    {k:<34} {v}");
        }
    }
    out
}
