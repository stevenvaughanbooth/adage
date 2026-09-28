//! `adage-train run`: the dry run (the default), and with `--execute` the exploration sessions,
//! mining into staged reflexes, and the optional validation arms.
//!
//! Layout under `$ADAGE_HOME/train/<run>/`:
//!
//! ```text
//! plan.json          the plan, as run
//! state.json         every session: arm, task, status, session id, turns, cost; spend; staging
//! settings/          the harness settings files (sandbox; hooks for the reflex arm)
//! sessions/<job>/    each session's raw harness output (stream.jsonl)
//! transcripts/<phase>/<job>.jsonl
//!                    transcripts copied out of each session's own config directory; never
//!                    anywhere `adage condition` or live learning reads history
//! staged/tracks/     reflexes mined from the exploration transcripts, under the repository's
//!                    key; NOT installed, NOT in any index hooks read
//! staged/index.json  their compiled index (what the reflex arm's hooks read, re-keyed)
//! staged/provenance.json
//!                    synthetic:skill:<sha256>@<run>, the source, the sessions, per route
//! reflex-home/       the reflex arm's ADAGE_HOME (validation only)
//! wt/, homes/        per-session clones and homes, removed after each session unless --keep
//! report.json        written by `report` (and at the end of a run)
//! ```

use crate::harness::{find_transcript, Arm, Harness, Outcome, SessionSpec};
use crate::isolate;
use crate::plan::Plan;
use crate::tasks::{self, Task};
use adage_learn::{Inputs, LearnConfig};
use adage_tracks::{io, Index};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub harness: String,
    pub model: String,
    pub budget_usd: f64,
    pub max_turns: u32,
    pub concurrency: usize,
    /// Held-out tasks for the validation phase (each runs in three arms); 0 skips validation.
    pub validate: usize,
    pub oracle: String,
    pub container: String,
    pub execute: bool,
    pub run_id: Option<String>,
    pub adage_home: PathBuf,
    /// The `adage` binary whose hooks the reflex arm runs (validation with a real harness).
    pub adage_bin: Option<PathBuf>,
    pub keep: bool,
    /// `default` or `demo` learning thresholds.
    pub thresholds: String,
    pub timeout: Duration,
}

impl RunOptions {
    pub fn defaults(adage_home: PathBuf) -> RunOptions {
        RunOptions {
            harness: "claude-code".into(),
            model: crate::plan::DEFAULT_MODEL.into(),
            budget_usd: 5.0,
            max_turns: 30,
            concurrency: 2,
            validate: 0,
            oracle: "trajectory".into(),
            container: "none".into(),
            execute: false,
            run_id: None,
            adage_home,
            adage_bin: None,
            keep: false,
            thresholds: "default".into(),
            timeout: Duration::from_secs(1800),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub job: String,
    /// `explore` or `validate`.
    pub phase: String,
    pub arm: Arm,
    pub task: Task,
    /// `done`, `skipped` (budget), `failed` (harness or infrastructure error).
    pub status: String,
    #[serde(default)]
    pub outcome: Outcome,
    /// Relative to the run directory.
    pub transcript: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Staged {
    pub tracks: String,
    pub index: String,
    pub provenance: String,
    pub routes: Vec<String>,
    pub sessions_used: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run: String,
    pub harness: String,
    pub model: String,
    pub plan_sha256: String,
    pub source_sha256: String,
    pub repo_key: String,
    /// The key hooks compute inside a clone (no origin): the reflex arm's index is keyed by it.
    pub clone_key: Option<String>,
    pub budget_usd: f64,
    pub spent_usd: f64,
    pub max_turns: u32,
    pub concurrency: usize,
    pub thresholds: String,
    pub provenance: String,
    pub validate_seed: Option<u64>,
    pub jobs: Vec<Job>,
    pub staged: Option<Staged>,
}

pub fn safe_run_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// `20260927-101500-<first 8 hex of the source sha256>`.
pub fn default_run_id(source_sha: &str) -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil(days);
    let t = secs % 86_400;
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{}",
        t / 3600,
        (t / 60) % 60,
        t % 60,
        &source_sha[..8.min(source_sha.len())]
    )
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn run_dir(adage_home: &Path, run: &str) -> PathBuf {
    adage_home.join("train").join(run)
}

/// The provenance staged reflexes carry.
pub fn provenance(source_sha: &str, run: &str) -> String {
    format!("synthetic:skill:{source_sha}@{run}")
}

/// Held-out validation tasks: a different seed, no prompt of the exploration set.
pub fn validation_tasks(plan: &Plan, n: usize) -> (u64, Vec<Task>) {
    let seed = plan.seed.wrapping_add(0x5eed_0001);
    let files = adage_instincts::files::RepoFiles::open(&plan.repo.root).map(|f| f.walk()).unwrap_or_default();
    let seen: BTreeSet<String> = plan.tasks.iter().map(|t| t.prompt.clone()).collect();
    (seed, tasks::generate(&plan.claims, &files, &plan.source.name, seed, n, &seen, "v"))
}

/// The arms of one held-out task, in an order drawn from the seed.
fn arm_order(seed: u64, task: &str) -> Vec<Arm> {
    let mut arms = vec![Arm::Baseline, Arm::Skill, Arm::Reflex];
    let mut h: u64 = seed;
    for b in task.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    tasks::Rng::new(h).shuffle(&mut arms);
    arms
}

/// Every session a run would start, in order.
pub fn jobs(plan: &Plan, validate: usize) -> (Option<u64>, Vec<Job>) {
    let mut out: Vec<Job> = plan
        .tasks
        .iter()
        .map(|t| Job {
            job: format!("explore-{}", t.id),
            phase: "explore".into(),
            arm: Arm::Explore,
            task: t.clone(),
            status: "planned".into(),
            outcome: Outcome::default(),
            transcript: None,
        })
        .collect();
    if validate == 0 {
        return (None, out);
    }
    let (seed, held) = validation_tasks(plan, validate);
    for t in held {
        for arm in arm_order(seed, &t.id) {
            out.push(Job {
                job: format!("validate-{}-{}", t.id, arm.as_str()),
                phase: "validate".into(),
                arm,
                task: t.clone(),
                status: "planned".into(),
                outcome: Outcome::default(),
                transcript: None,
            });
        }
    }
    (Some(seed), out)
}

/// The directory name clones get: the repository's own, so a clone's key is predictable.
fn repo_dir_name(plan: &Plan) -> String {
    let n = adage_core::repo::dir_name(&plan.repo.root);
    if n.is_empty() || !safe_run_id(&n) {
        "repo".into()
    } else {
        n
    }
}

/// Adage's environment for one arm, on top of the isolation environment.
fn arm_env(arm: Arm, run: &str, job: &str, reflex_home: &Path) -> BTreeMap<String, String> {
    let mut e = BTreeMap::new();
    e.insert("ADAGE_TRAIN_SESSION".to_string(), format!("{run}/{job}"));
    e.insert("ADAGE_AUTO_LEARN".to_string(), "0".to_string());
    if arm.hooks() {
        e.insert("ADAGE_HOME".to_string(), reflex_home.to_string_lossy().to_string());
        e.insert("ADAGE_ARM".to_string(), "treated".to_string());
        e.insert("ADAGE_HOLDOUT".to_string(), "0".to_string());
    } else {
        e.insert("ADAGE_DISABLE".to_string(), "1".to_string());
    }
    e
}

struct Ctx<'a> {
    plan: &'a Plan,
    o: &'a RunOptions,
    h: &'a dyn Harness,
    base: &'a BTreeMap<String, String>,
    run: String,
    dir: PathBuf,
    spent: Mutex<f64>,
    settings: BTreeMap<bool, PathBuf>,
}

fn spec_for(c: &Ctx, j: &Job, max_budget: f64) -> SessionSpec {
    let home = c.dir.join("homes").join(&j.job);
    let mut env = isolate::isolation_env(&home, c.base, c.h.auth_env());
    env.extend(arm_env(j.arm, &c.run, &j.job, &c.dir.join("reflex-home")));
    SessionSpec {
        job: j.job.clone(),
        arm: j.arm,
        task: j.task.clone(),
        prompt: j.task.prompt.clone(),
        cwd: c.dir.join("wt").join(&j.job).join(repo_dir_name(c.plan)),
        config_dir: home.join(".claude"),
        env,
        session_dir: c.dir.join("sessions").join(&j.job),
        model: c.o.model.clone(),
        max_turns: c.o.max_turns,
        append_system_prompt: j.arm.skill_in_context().then(|| c.plan.source.text.clone()),
        settings: c.settings.get(&j.arm.hooks()).cloned().unwrap_or_default(),
        max_budget_usd: max_budget,
        timeout: c.o.timeout,
    }
}

fn run_job(c: &Ctx, mut j: Job) -> Job {
    let remaining = {
        let spent = c.spent.lock().unwrap();
        if *spent >= c.o.budget_usd {
            j.status = "skipped".into();
            j.outcome.error = Some(format!("budget ${:.2} reached", c.o.budget_usd));
            return j;
        }
        c.o.budget_usd - *spent
    };
    let spec = spec_for(c, &j, remaining.max(0.01));
    let home = c.dir.join("homes").join(&j.job);
    let result = (|| -> Result<Outcome> {
        isolate::clone_worktree(&c.plan.repo.root, &spec.cwd)?;
        isolate::prepare_home(&home)?;
        c.h.prepare(&spec.config_dir, c.base)?;
        let out = c.h.run(&spec);
        c.h.cleanup(&spec.config_dir);
        out
    })();
    match result {
        Ok(out) => {
            *c.spent.lock().unwrap() += out.cost_usd;
            j.outcome = out;
            if let Some(src) = j.outcome.session_id.as_deref().and_then(|sid| find_transcript(&spec.config_dir, sid)) {
                let rel = format!("transcripts/{}/{}.jsonl", j.phase, j.job);
                let dest = c.dir.join(&rel);
                if dest.parent().is_some_and(|p| std::fs::create_dir_all(p).is_ok())
                    && std::fs::copy(&src, &dest).is_ok()
                {
                    j.transcript = Some(rel);
                }
            }
            if j.transcript.is_some() {
                j.status = "done".into();
            } else {
                j.status = "failed".into();
                j.outcome.error.get_or_insert_with(|| "no transcript".into());
            }
        }
        Err(e) => {
            j.status = "failed".into();
            j.outcome.error = Some(format!("{e:#}"));
        }
    }
    if !c.o.keep {
        // The clone goes, but an empty `<name>/.git` stays: adage-learn finds a transcript's
        // repository root by walking up from its cwd to a `.git`, and must stop at the clone's
        // root, not at some repository above $ADAGE_HOME.
        isolate::remove(&c.dir.join("wt").join(&j.job));
        let _ = std::fs::create_dir_all(spec.cwd.join(".git"));
        isolate::remove(&home);
    }
    j
}

/// Run jobs on at most `concurrency` workers; stop starting them once the budget is spent.
fn run_all(c: &Ctx, jobs: Vec<Job>, progress: &(dyn Fn(&Job) + Sync)) -> Vec<Job> {
    let n = jobs.len();
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Job>>> = jobs.into_iter().map(|j| Mutex::new(Some(j))).collect();
    let workers = c.o.concurrency.clamp(1, 32).min(n.max(1));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= n {
                    break;
                }
                let j = slots[i].lock().unwrap().take().unwrap();
                let done = run_job(c, j);
                progress(&done);
                *slots[i].lock().unwrap() = Some(done);
            });
        }
    });
    slots.into_iter().map(|m| m.into_inner().unwrap().unwrap()).collect()
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Mine the exploration transcripts into staged tracks under the person's repository key.
pub fn mine(dir: &Path, plan: &Plan, state: &RunState) -> Result<Staged> {
    let transcripts: Vec<PathBuf> = state
        .jobs
        .iter()
        .filter(|j| j.phase == "explore")
        .filter_map(|j| j.transcript.as_ref().map(|t| dir.join(t)))
        .collect();
    let tracks = dir.join("staged/tracks");
    let index = dir.join("staged/index.json");
    let _ = std::fs::remove_dir_all(&tracks);
    let mut cfg = if state.thresholds == "demo" { LearnConfig::demo() } else { LearnConfig::default() };
    cfg.provenance = state.provenance.clone();
    let inputs = Inputs {
        transcripts: transcripts.clone(),
        outcomes: BTreeMap::new(),
        repo_override: Some(plan.repo.key.clone()),
        models: None,
        cache: None,
        only_repo: None,
    };
    std::fs::create_dir_all(&tracks)?;
    let report = adage_learn::learn(&inputs, &tracks, &index, &cfg).context("mining the exploration transcripts")?;
    // The synthetic-origin cap (docs/training.md): never green, never more than probation. A
    // dead end stays red (the model requires it of an avoid step); review decides.
    let mut all = io::read_all(&tracks)?;
    for t in all.iter_mut() {
        for r in t.routes.iter_mut() {
            if r.aspect == adage_tracks::Aspect::Green {
                r.aspect = adage_tracks::Aspect::Yellow;
            }
            if r.state == adage_tracks::RouteState::Active {
                r.state = adage_tracks::RouteState::Probation;
            }
        }
        io::validate(t)?;
        io::write(
            &tracks
                .join(adage_learn::slug(&t.manifest.scope.repo))
                .join(t.manifest.track.name.rsplit('/').next().unwrap_or("track")),
            t,
        )?;
    }
    Index::compile(&all).write_atomic(&index)?;
    let routes: Vec<String> =
        all.iter().flat_map(|t| t.routes.iter().map(move |r| format!("{}:{}", t.manifest.track.name, r.id))).collect();
    let sidecar = serde_json::json!({
        "provenance": state.provenance,
        "origin": "synthetic",
        "source": { "path": plan.source.path, "kind": plan.source.kind, "name": plan.source.name, "sha256": plan.source.sha256 },
        "run": state.run,
        "harness": state.harness,
        "model": state.model,
        "repo_key": plan.repo.key,
        "sessions_used": report.sessions_used,
        "transcripts": transcripts.len(),
        "thresholds": report.thresholds,
        "routes": routes,
        "status": "staged: not installed and not in any index hooks read; review before use (docs/training.md)",
        "model_change_needed": "Evidence.by_origin and a TrackInfo origin field, so synthetic evidence is counted \
                                apart from organic evidence and capped (docs/training.md, Follow-up model changes)",
    });
    write_json(&dir.join("staged/provenance.json"), &sidecar)?;
    Ok(Staged {
        tracks: "staged/tracks".into(),
        index: "staged/index.json".into(),
        provenance: "staged/provenance.json".into(),
        routes,
        sessions_used: report.sessions_used,
    })
}

/// The reflex arm's ADAGE_HOME: the staged tracks re-keyed to the clone's repository key (a
/// clone has no origin, so hooks inside it compute `<dir>@<root commit>`), and their index.
fn reflex_home(dir: &Path, clone_key: &str) -> Result<()> {
    let home = dir.join("reflex-home");
    let _ = std::fs::remove_dir_all(&home);
    let slug = adage_learn::slug(clone_key);
    let mut tracks = Vec::new();
    for mut t in io::read_all(&dir.join("staged/tracks"))? {
        let suffix = t.manifest.track.name.rsplit('/').next().unwrap_or("track").to_string();
        t.manifest.scope.repo = clone_key.to_string();
        t.manifest.track.name = format!("{slug}/{suffix}");
        io::write(&home.join("tracks").join(&slug).join(&suffix), &t)?;
        tracks.push(t);
    }
    Index::compile(&tracks).write_atomic(&home.join("index.json"))?;
    Ok(())
}

/// The key a hook computes inside a clone, from a probe clone.
fn clone_key(dir: &Path, plan: &Plan) -> Result<String> {
    let probe = dir.join("wt/probe").join(repo_dir_name(plan));
    isolate::clone_worktree(&plan.repo.root, &probe)?;
    let key = adage_core::repo::repo_key(&probe);
    isolate::remove(&dir.join("wt/probe"));
    Ok(key)
}

/// Checks before anything runs.
pub fn preflight(plan: &Plan, o: &RunOptions, h: &dyn Harness, base: &BTreeMap<String, String>) -> Result<()> {
    if o.oracle != "trajectory" {
        bail!("--oracle {}: only `trajectory` is implemented", o.oracle);
    }
    if o.container != "none" {
        bail!("--container {}: only `none` is implemented (the harness's own sandbox is used)", o.container);
    }
    if o.budget_usd.is_nan() || o.budget_usd <= 0.0 {
        bail!("--budget must be more than 0");
    }
    if !plan.repo.root.join(".git").exists() {
        bail!("{} is not a git repository (training clones its HEAD)", plan.repo.root.display());
    }
    if h.spawns_agents() {
        if o.validate > 0 && o.adage_bin.is_none() {
            bail!("--validate runs a reflex arm with Adage's hooks: pass --adage <path to the adage binary>");
        }
        if h.name() == "claude-code" && crate::harness::ClaudeCode::auth_source(base).is_none() {
            bail!(
                "isolated sessions cannot find your Claude Code login: set ANTHROPIC_API_KEY or \
                 CLAUDE_CODE_OAUTH_TOKEN (from `claude setup-token`), or have ~/.claude/.credentials.json"
            );
        }
    }
    // Transcripts must never land where learning reads history.
    let dir = run_dir(&o.adage_home, "x");
    let projects = base
        .get("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| base.get("HOME").map(|h| Path::new(h).join(".claude")))
        .map(|d| d.join("projects"));
    if projects.is_some_and(|p| dir.starts_with(p)) {
        bail!("ADAGE_HOME is inside your Claude Code projects directory; training transcripts must not be");
    }
    Ok(())
}

/// Execute a plan: exploration, mining, and validation if asked. The caller has already checked
/// that a person asked for it (`isolate::require_person`) when the harness spawns agents.
pub fn execute(
    plan: &Plan,
    o: &RunOptions,
    h: &dyn Harness,
    base: &BTreeMap<String, String>,
    progress: &(dyn Fn(&Job) + Sync),
) -> Result<(PathBuf, RunState)> {
    preflight(plan, o, h, base)?;
    let run = o.run_id.clone().unwrap_or_else(|| default_run_id(&plan.source.sha256));
    if !safe_run_id(&run) {
        bail!("run id {run:?}: letters, digits, `.`, `_` and `-` only");
    }
    let dir = run_dir(&o.adage_home, &run);
    if dir.join("state.json").exists() {
        bail!("{} already holds a run; pick another --run-id", dir.display());
    }
    std::fs::create_dir_all(&dir)?;
    let plan_json = serde_json::to_string_pretty(plan)?;
    std::fs::write(dir.join("plan.json"), &plan_json)?;
    let (validate_seed, all) = jobs(plan, o.validate);
    let hooks = match (&o.adage_bin, o.validate > 0 && h.spawns_agents()) {
        (Some(bin), true) => {
            let out = std::process::Command::new(bin)
                .arg("hooks-config")
                .arg(bin)
                .output()
                .with_context(|| format!("running {} hooks-config", bin.display()))?;
            Some(serde_json::from_slice::<serde_json::Value>(&out.stdout).context("adage hooks-config output")?)
        }
        _ => None,
    };
    let mut settings = BTreeMap::new();
    for with_hooks in [false, true] {
        let p = dir.join("settings").join(if with_hooks { "reflex.json" } else { "plain.json" });
        write_json(&p, &h.settings(if with_hooks { hooks.as_ref() } else { None }))?;
        settings.insert(with_hooks, p);
    }
    let mut state = RunState {
        run: run.clone(),
        harness: h.name().into(),
        model: o.model.clone(),
        plan_sha256: crate::plan::sha256_hex(plan_json.as_bytes()),
        source_sha256: plan.source.sha256.clone(),
        repo_key: plan.repo.key.clone(),
        clone_key: None,
        budget_usd: o.budget_usd,
        spent_usd: 0.0,
        max_turns: o.max_turns,
        concurrency: o.concurrency,
        thresholds: o.thresholds.clone(),
        provenance: provenance(&plan.source.sha256, &run),
        validate_seed,
        jobs: all.clone(),
        staged: None,
    };
    write_json(&dir.join("state.json"), &state)?;
    let ctx = Ctx { plan, o, h, base, run: run.clone(), dir: dir.clone(), spent: Mutex::new(0.0), settings };

    // 1. Exploration.
    let explore: Vec<Job> = all.iter().filter(|j| j.phase == "explore").cloned().collect();
    let mut done = run_all(&ctx, explore, progress);
    state.spent_usd = *ctx.spent.lock().unwrap();
    state.jobs = done.clone();
    write_json(&dir.join("state.json"), &state)?;

    // 2. Mining into staging.
    state.staged = Some(mine(&dir, plan, &state)?);
    write_json(&dir.join("state.json"), &state)?;

    // 3. Validation.
    let validate: Vec<Job> = all.iter().filter(|j| j.phase == "validate").cloned().collect();
    if !validate.is_empty() {
        let key = clone_key(&dir, plan)?;
        reflex_home(&dir, &key)?;
        state.clone_key = Some(key);
        done.extend(run_all(&ctx, validate, progress));
        state.spent_usd = *ctx.spent.lock().unwrap();
        state.jobs = done;
        write_json(&dir.join("state.json"), &state)?;
    }
    Ok((dir, state))
}

pub fn load_state(dir: &Path) -> Result<RunState> {
    let p = dir.join("state.json");
    serde_json::from_str(&std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?)
        .with_context(|| format!("{} is not an adage-train run state", p.display()))
}

/// The dry run: everything `--execute` would do, with nothing written or started.
pub fn describe(plan: &Plan, o: &RunOptions, h: &dyn Harness, base: &BTreeMap<String, String>) -> serde_json::Value {
    let run = o.run_id.clone().unwrap_or_else(|| "<run>".into());
    let dir = run_dir(&o.adage_home, &run);
    let (_, all) = jobs(plan, o.validate);
    let ctx = Ctx {
        plan,
        o,
        h,
        base,
        run: run.clone(),
        dir: dir.clone(),
        spent: Mutex::new(0.0),
        settings: [(false, dir.join("settings/plain.json")), (true, dir.join("settings/reflex.json"))].into(),
    };
    let mut examples = BTreeMap::new();
    for j in &all {
        examples.entry(j.arm.as_str()).or_insert_with(|| {
            let s = spec_for(&ctx, j, o.budget_usd);
            let placeholder = serde_json::json!({"hooks": "<adage hooks-config>"});
            let adage_env: BTreeMap<&String, &String> = s.env.iter().filter(|(k, _)| k.starts_with("ADAGE_")).collect();
            serde_json::json!({
                "job": s.job, "cwd": s.cwd, "command": h.command(&s), "adage_env": adage_env,
                "settings": h.settings(if j.arm.hooks() { Some(&placeholder) } else { None }),
            })
        });
    }
    let by_arm: BTreeMap<&str, usize> = all.iter().fold(BTreeMap::new(), |mut m, j| {
        *m.entry(j.arm.as_str()).or_default() += 1;
        m
    });
    let per = crate::plan::usd_per_session(&o.model);
    serde_json::json!({
        "dry_run": true,
        "run_dir": dir,
        "harness": h.name(),
        "model": o.model,
        "sessions": all.len(),
        "by_arm": by_arm,
        "jobs": all.iter().map(|j| serde_json::json!({"job": j.job, "arm": j.arm, "claim": j.task.claim, "prompt": j.task.prompt})).collect::<Vec<_>>(),
        "budget_usd": o.budget_usd,
        "estimate": {
            "usd": per * all.len() as f64,
            "line": format!("{} sessions × ${per:.2}/session ({}) = ${:.2} (ESTIMATE)", all.len(), o.model, per * all.len() as f64),
        },
        "max_turns": o.max_turns,
        "concurrency": o.concurrency,
        "isolation": {
            "clone": "git clone --local --no-hardlinks <repo> <run>/wt/<job>/<name>; git remote remove origin",
            "per_session": isolate::home_dirs(Path::new("<run>/homes/<job>")).keys().collect::<Vec<_>>(),
            "passed_through": isolate::PASS_ENV.iter().chain(h.auth_env()).collect::<Vec<_>>(),
            "auth": if h.name() == "claude-code" { crate::harness::ClaudeCode::auth_source(base) } else { None },
            "hooks": "exploration, baseline and skill arms: ADAGE_DISABLE=1; reflex arm: hooks on, ADAGE_HOME=<run>/reflex-home (staged reflexes only), ADAGE_HOLDOUT=0",
            "transcripts": format!("{}/transcripts (never where learning reads history)", dir.display()),
        },
        "examples": examples,
        "staging": {
            "tracks": format!("{}/staged/tracks", dir.display()),
            "provenance": provenance(&plan.source.sha256, &run),
            "activation": "none: staged reflexes are reviewed before anything uses them",
        },
    })
}

pub fn render_describe(d: &serde_json::Value) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "DRY RUN: nothing is executed. Add --execute to start {} {} sessions.",
        d["sessions"],
        d["harness"].as_str().unwrap_or("")
    );
    let _ = writeln!(s, "run dir   {}", d["run_dir"].as_str().unwrap_or(""));
    let _ = writeln!(
        s,
        "model     {}   max turns {}   concurrency {}",
        d["model"].as_str().unwrap_or(""),
        d["max_turns"],
        d["concurrency"]
    );
    let _ = writeln!(s, "sessions  {} by arm {}", d["sessions"], d["by_arm"]);
    let _ = writeln!(
        s,
        "budget    cap ${:.2}; {}",
        d["budget_usd"].as_f64().unwrap_or(0.0),
        d["estimate"]["line"].as_str().unwrap_or("")
    );
    let iso = &d["isolation"];
    let _ = writeln!(s, "isolation {}", iso["clone"].as_str().unwrap_or(""));
    let _ = writeln!(s, "          per session: {}", iso["per_session"]);
    let _ = writeln!(s, "          environment: only {}", iso["passed_through"]);
    if let Some(a) = iso["auth"].as_str() {
        let _ = writeln!(s, "          auth: {a}");
    }
    let _ = writeln!(s, "          {}", iso["hooks"].as_str().unwrap_or(""));
    let _ = writeln!(s, "          transcripts: {}", iso["transcripts"].as_str().unwrap_or(""));
    let _ = writeln!(
        s,
        "staging   {} (provenance {}); {}",
        d["staging"]["tracks"].as_str().unwrap_or(""),
        d["staging"]["provenance"].as_str().unwrap_or(""),
        d["staging"]["activation"].as_str().unwrap_or("")
    );
    let _ = writeln!(s, "\nsessions:");
    for j in d["jobs"].as_array().into_iter().flatten() {
        let _ = writeln!(
            s,
            "  {:<24} {:<8} {:<6} {}",
            j["job"].as_str().unwrap_or(""),
            j["arm"].as_str().unwrap_or(""),
            j["claim"].as_str().unwrap_or("-"),
            j["prompt"].as_str().unwrap_or("")
        );
    }
    let _ = writeln!(s, "\nexact commands (one per arm):");
    for (arm, e) in d["examples"].as_object().into_iter().flatten() {
        let args: Vec<&str> = e["command"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
        let mut cmd: Vec<String> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if i > 0 && args[i - 1] == "--append-system-prompt" {
                // The whole source text: shown by size here, in full with --json.
                cmd.push(format!("'<the skill text, {} bytes>'", a.len()));
            } else if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "'\"$`\\;&|<>()*?!#~".contains(c)) {
                cmd.push(format!("'{}'", a.replace('\'', "'\\''")));
            } else {
                cmd.push(a.to_string());
            }
        }
        let shown = cmd.join(" ");
        let _ = writeln!(s, "  [{arm}] cwd {}", e["cwd"].as_str().unwrap_or(""));
        let _ = writeln!(s, "    env {}", e["adage_env"]);
        let _ = writeln!(s, "    {shown}");
        let _ = writeln!(s, "    settings {}", e["settings"]);
    }
    s
}
