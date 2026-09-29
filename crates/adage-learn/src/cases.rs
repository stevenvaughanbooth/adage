//! The case store: "roughly the same situation" by embedding, and what worked from there
//! (architecture review §3.2 option (a), §5.4 row 6). Embeddings generate candidates; the value
//! model admits them.
//!
//! - **Situations** ([`points`], [`situation`]). A decision point follows a prompt, a model turn's
//!   batch of tool calls, or a failed call. Its situation is two texts: the prompt it serves, and a
//!   compact account of where the session is, in the action grammar (`adage_core::action`): the
//!   phase (exploring, editing, tests ran), the last [`SIT_ACTIONS`] actions (`test-run|narrow
//!   pytest system failed`, `edit src click`), the last failure's signature (`no-module:pytest`, or
//!   its first line with identifiers replaced by placeholders), and the directories edited so far.
//!   Both texts are embedded with the process's [`Embedder`] (a static model, or the explicit
//!   hashed fallback) and stored as `i16` unit vectors.
//! - **Cases** ([`Case`]). Every decision point of a training trace, labelled with the action that
//!   reached a good local outcome from it: the first test run within [`crate::outcome::HORIZON`]
//!   calls that executed tests and **passed** ([`tests_passed`]), in the project's own environment.
//!   Its proposals are the runner spelled in that environment (`.venv/bin/python -m pytest`,
//!   `npm test`) and, in a monorepo, the area it tested (`npm test -- packages/runtime-core`). A
//!   point with no good outcome is still a case: it votes for silence.
//! - **Retrieval** ([`CaseStore::propose`]). Exact kNN over the cases of the same kind (prompt,
//!   batch, failure) by integer dot products of the `i16` vectors (deterministic: ties go to the
//!   earlier case), [`K`] neighbours; each votes for its proposals with weight sim² × its
//!   outcome weight (a quicker good outcome weighs more), and for silence when it had none. A
//!   proposal needs a weighted share ≥ [`THETA`] from ≥ [`MIN_SESSIONS`] sessions.
//! - **Admission** ([`build`]). Leave one session out: every training decision point is queried
//!   against the other sessions' cases, and where a proposal would have been given, the session's
//!   own continuation values it, exactly as `value.rs` values a failure route: `Q(Sat d)` and
//!   `Q(other)` are Gamma–Poisson posteriors of the calls to the first test run that ran tests
//!   (priors from [`Priors`], the cross-repository hierarchy), `p_sat` the share that already did
//!   it, and `A = p_follow · (1 − p_sat) · (Q(other) − Q(Sat d)) − C − p_misfire · m`
//!   ([`adage_graph::stats::advantage_mix`]). A proposal is admitted only on a positive one-sided
//!   90% lower bound with at least [`MIN_EFFECTIVE`] effective samples: SPIBB's rule, so where the
//!   data are thin the baseline (silence) stands.
//!
//! Candidates are safe by construction: a test run that passed in the project's own environment;
//! never an install, a system interpreter, or anything destructive.
//!
//! Also here: cross-session **task identity** by prompt similarity ([`task_groups`]) and **prompt →
//! area routing** ([`area_of`], and the area proposals above).

use crate::import::{Session, StepObs};
use crate::outcome::{self, Goal, HORIZON};
use crate::value::{Priors, DIRECTIVE_COST, FOLLOW_PRIOR, MIN_EFFECTIVE, MISFIRE_COST, MISFIRE_PRIOR};
use adage_core::action::{Env, EnvClass, Role, Target};
use adage_graph::stats::{self, Advantage};
use adage_intent::model::Embedder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// The store's format.
pub const VERSION: u32 = 1;
/// Neighbours that vote.
pub const K: usize = 10;
/// The weighted share of the neighbours' votes a proposal needs.
pub const THETA: f64 = 0.5;
/// Distinct sessions among the neighbours that voted for it.
pub const MIN_SESSIONS: usize = 2;
/// Actions a situation describes.
pub const SIT_ACTIONS: usize = 3;
/// Share of the prompt's similarity in a batch or failure situation (the rest is the situation
/// text's).
pub const PROMPT_SHARE: f64 = 0.5;

/// What a decision point follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Prompt,
    Batch,
    Failure,
}

/// A decision point of a session: the steps before it are `steps[..after]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub kind: Kind,
    pub after: usize,
    /// The prompt it serves (index into `Session::prompts`).
    pub prompt: usize,
}

/// The decision points of a session, in order: each prompt, the end of each model turn, and each
/// failed step.
pub fn points(s: &Session) -> Vec<Point> {
    let mut out = Vec::new();
    for (i, p) in s.prompts.iter().enumerate() {
        out.push(Point { kind: Kind::Prompt, after: (p.at_step as usize).min(s.steps.len()), prompt: i });
    }
    for (j, st) in s.steps.iter().enumerate() {
        let prompt = st.prompt as usize;
        if st.call.failed {
            out.push(Point { kind: Kind::Failure, after: j + 1, prompt });
        }
        if s.steps.get(j + 1).is_none_or(|n| n.turn != st.turn) {
            out.push(Point { kind: Kind::Batch, after: j + 1, prompt });
        }
    }
    out.sort_by_key(|p| (p.after, p.kind != Kind::Prompt, p.kind));
    out
}

fn words(p: &str) -> String {
    p.split(['/', '.', '_', '-']).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

fn dir_of(p: &str) -> &str {
    p.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// One step, in the action grammar.
fn describe(st: &StepObs) -> String {
    let failed = if st.call.failed || outcome::env_failure(&st.call.output).is_some() { " failed" } else { "" };
    if let Some(a) = outcome::action_of(st) {
        if a.role != Role::Other || !a.runner.is_empty() {
            return format!("{} {} {}{failed}", a.intent(), a.runner, a.env.kind());
        }
    }
    let verb = if adage_core::keys::is_author(&st.norm.action) {
        "edit"
    } else if st.rel_path.is_some() {
        "read"
    } else {
        st.key.split_whitespace().next().unwrap_or("act")
    };
    let path = st.rel_path.as_deref().map(|p| words(dir_of(p))).unwrap_or_default();
    format!("{verb} {path}{failed}").trim().to_string()
}

/// A failure's signature: its environment family, else its first meaningful line with
/// identifiers replaced by placeholders, at most 12 words.
fn signature(output: &str) -> String {
    if let Some((family, _)) = outcome::env_signature(output) {
        return family.replace([':', '-'], " ");
    }
    static NOISE: OnceLock<Regex> = OnceLock::new();
    let noise = NOISE.get_or_init(|| Regex::new(r"^\s*(\$|>|Exit code|\s*$)").expect("valid"));
    let line = adage_core::tokens::bounded(output, 4096).lines().find(|l| !noise.is_match(l)).unwrap_or("");
    let n = adage_intent::normalize::normalize(line);
    n.split_whitespace().take(12).collect::<Vec<_>>().join(" ")
}

/// The situation text after `steps` (see the module documentation).
pub fn situation(steps: &[StepObs]) -> String {
    let edited: BTreeSet<String> =
        steps.iter().filter_map(outcome::source_edit).map(|p| words(dir_of(p))).collect();
    let tested = steps.iter().any(outcome::test_goal);
    let mut parts = vec![if tested {
        "tests ran".to_string()
    } else if !edited.is_empty() {
        "editing, no tests yet".to_string()
    } else {
        "exploring".to_string()
    }];
    let from = steps.len().saturating_sub(SIT_ACTIONS);
    parts.extend(steps[from..].iter().map(describe));
    if let Some(st) = steps.iter().rev().take(5).find(|s| s.call.failed) {
        parts.push(format!("error {}", signature(&st.call.output)));
    }
    if !edited.is_empty() {
        parts.push(format!("edited {}", edited.into_iter().collect::<Vec<_>>().join(" ")));
    }
    parts.join(" ; ")
}

/// Whether a test run's output shows tests that ran and none that failed.
pub fn tests_passed(output: &str) -> bool {
    static FAIL: OnceLock<Regex> = OnceLock::new();
    static PASS: OnceLock<Regex> = OnceLock::new();
    let out = adage_core::tokens::bounded(output, 64 * 1024);
    if !outcome::executed_tests(out) {
        return false;
    }
    let fail = FAIL.get_or_init(|| {
        Regex::new(r"(?m)\b[1-9]\d* (failed|errors?)\b|^FAIL\b|^--- FAIL|test result: FAILED|^FAILED|Tests:.*\b[1-9]\d* failed")
            .expect("valid")
    });
    let pass = PASS.get_or_init(|| {
        Regex::new(r"(?m)\b[1-9]\d* passed\b|^OK\b|^ok\s|test result: ok|^--- PASS").expect("valid")
    });
    pass.is_match(out) && !fail.is_match(out)
}

/// What a case proposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// Run the tests with this runner, in the project's environment.
    Runner,
    /// Run the tests of this area (a monorepo package) with this runner.
    Area,
}

/// A candidate directive, and the value model's verdict on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub key: String,
    pub level: Level,
    /// What to run: `.venv/bin/python -m pytest`, `npm test -- packages/runtime-core`.
    pub command: String,
    /// The test family (`pytest`, `npm test`), for `Sat(d)`.
    pub family: String,
    /// The environment kind it runs in (`venv`, `script`).
    pub env: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    /// Training decision points (leave one session out) where it would have been given.
    pub exposed: u32,
    /// Of those, where the session's next test run was already in `Sat(d)`.
    pub sat: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advantage: Option<Advantage>,
    pub effective: f64,
    pub admitted: bool,
}

/// One decision point of a training trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub session: String,
    pub kind: Kind,
    pub after: u32,
    /// Prompt and situation vectors (units of 1/`adage_intent::SCALE`); empty when the embedder
    /// knew no token.
    pub p: Vec<i16>,
    pub s: Vec<i16>,
    /// Indices into [`CaseStore::proposals`]: the runner, then the area (if any).
    #[serde(default)]
    pub proposals: Vec<u32>,
    /// Calls to the good outcome (0 when there was none).
    #[serde(default)]
    pub calls: u32,
}

impl Case {
    /// A quicker good outcome weighs more: 1 at the next call, ½ at the horizon.
    fn outcome_weight(&self) -> f64 {
        if self.proposals.is_empty() {
            1.0
        } else {
            1.0 - 0.5 * (self.calls.saturating_sub(1) as f64 / HORIZON as f64).min(1.0)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CaseStore {
    pub version: u32,
    pub repo: String,
    /// The embedder's id (`potion-base-8m@…+i8`, `hashed-256@1`): a store is only queried with
    /// the embedder it was built with.
    pub embedder: String,
    pub cases: Vec<Case>,
    pub proposals: Vec<Proposal>,
}

/// A proposal the store would give now.
#[derive(Debug, Clone, PartialEq)]
pub struct Given<'a> {
    pub proposal: &'a Proposal,
    pub index: usize,
    pub confidence: f64,
    pub sessions: usize,
}

fn dot(a: &[i16], b: &[i16]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let d: i64 = a.iter().zip(b).map(|(x, y)| *x as i64 * *y as i64).sum();
    d as f64 / (adage_intent::SCALE as f64 * adage_intent::SCALE as f64)
}

/// Embed a text as a stored vector (empty when the embedder knows no token of it).
pub fn vector(e: &Embedder, text: &str) -> Vec<i16> {
    e.embed(text).map(|v| adage_intent::quantize(&v)).unwrap_or_default()
}

/// The two vectors of a decision point: the normalised prompt, and the situation text.
pub fn vectors(e: &Embedder, prompt: &str, steps: &[StepObs]) -> (Vec<i16>, Vec<i16>) {
    (vector(e, &adage_intent::normalize::normalize(prompt)), vector(e, &situation(steps)))
}

impl CaseStore {
    /// The `K` nearest cases of `kind`, most similar first, leaving out `skip`'s cases.
    pub fn neighbours(&self, kind: Kind, p: &[i16], s: &[i16], skip: Option<&str>) -> Vec<(f64, usize)> {
        let mut out: Vec<(f64, usize)> = self
            .cases
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == kind && skip.is_none_or(|x| c.session != x))
            .map(|(i, c)| {
                let sim = match kind {
                    Kind::Prompt => dot(p, &c.p),
                    _ => PROMPT_SHARE * dot(p, &c.p) + (1.0 - PROMPT_SHARE) * dot(s, &c.s),
                };
                (sim, i)
            })
            .collect();
        out.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        out.truncate(K);
        out
    }

    /// Every proposal the neighbours vote for, with its weighted share and distinct sessions.
    pub fn votes(&self, nb: &[(f64, usize)]) -> Vec<(usize, f64, usize)> {
        let mut total = 0.0;
        let mut by: BTreeMap<usize, (f64, BTreeSet<&str>)> = BTreeMap::new();
        for &(sim, i) in nb {
            let c = &self.cases[i];
            let w = sim.max(0.0).powi(2) * c.outcome_weight();
            total += w;
            for &p in &c.proposals {
                let e = by.entry(p as usize).or_default();
                e.0 += w;
                e.1.insert(c.session.as_str());
            }
        }
        by.into_iter().map(|(p, (w, ss))| (p, if total > 0.0 { w / total } else { 0.0 }, ss.len())).collect()
    }

    /// The proposal to give at a decision point, if any: the best-supported one clearing
    /// [`THETA`] and [`MIN_SESSIONS`] (admitted by the value model, unless `ungated`), areas
    /// before runners at equal share.
    pub fn propose(&self, kind: Kind, p: &[i16], s: &[i16], skip: Option<&str>, ungated: bool) -> Option<Given<'_>> {
        let nb = self.neighbours(kind, p, s, skip);
        self.votes(&nb)
            .into_iter()
            .filter(|&(i, conf, n)| conf >= THETA && n >= MIN_SESSIONS && (ungated || self.proposals[i].admitted))
            .max_by(|a, b| {
                a.1.total_cmp(&b.1)
                    .then((self.proposals[a.0].level == Level::Area).cmp(&(self.proposals[b.0].level == Level::Area)))
                    .then(b.0.cmp(&a.0))
            })
            .map(|(i, conf, n)| Given { proposal: &self.proposals[i], index: i, confidence: conf, sessions: n })
    }
}

/// The directory a test run's targets share, cut to two segments (`packages/runtime-core`), when
/// it has two: a monorepo area. `root` is stripped from absolute targets.
pub fn area_of(targets: &[Target], root: &str) -> Option<String> {
    let dirs: Vec<Vec<&str>> = targets
        .iter()
        .map(|t| {
            let p = t.path();
            let p = p.strip_prefix(root).map(|x| x.trim_start_matches('/')).unwrap_or(p);
            let p = p.trim_start_matches("./");
            let d = match t {
                Target::Dir(_) => p,
                _ => dir_of(p),
            };
            d.split('/').filter(|x| !x.is_empty()).collect()
        })
        .collect();
    let first = dirs.first()?;
    let common = (0..first.len()).take_while(|&i| dirs.iter().all(|d| d.get(i) == first.get(i))).count();
    (common >= 2 && !first[0].starts_with('/') && first[0] != "..").then(|| first[..2].join("/"))
}

/// The proposals a passing test run makes: its runner spelled in the project's environment, and
/// its area. `None` for a run outside the project's environment.
pub fn proposals_of(st: &StepObs, root: &str) -> Option<(Proposal, Option<Proposal>)> {
    let a = outcome::action_of(st)?;
    if a.role != Role::TestRun || a.env.class() != EnvClass::Project {
        return None;
    }
    let prefix = match (a.runner.as_str(), &a.env) {
        ("pytest" | "unittest", Env::Venv(d)) => format!("{d}/bin/python -m {}", a.runner),
        ("pytest", Env::Managed(m)) if matches!(m.as_str(), "uv" | "poetry" | "pdm" | "hatch") => format!("{m} run pytest"),
        ("pytest" | "unittest", _) => return None,
        _ => {
            // The argv up to its first option or target (`npm test`, `npx vitest run`, `go test`).
            let t: Vec<&str> = a
                .argv
                .iter()
                .map(String::as_str)
                .skip_while(|t| t.contains('=') && !t.starts_with('-'))
                .take_while(|t| !t.starts_with('-') && !t.contains('/') && !t.contains('.'))
                .collect();
            if t.is_empty() {
                return None;
            }
            t.join(" ")
        }
    };
    let dashes = a.argv.iter().any(|t| t == "--");
    let base = |level, key: String, command: String, area: Option<String>| Proposal {
        key,
        level,
        command,
        family: a.runner.clone(),
        env: a.env.kind().to_string(),
        area,
        exposed: 0,
        sat: 0,
        advantage: None,
        effective: 0.0,
        admitted: false,
    };
    let runner = base(Level::Runner, format!("run:{prefix}"), prefix.clone(), None);
    let area = area_of(&a.targets, root).map(|ar| {
        let cmd = if dashes { format!("{prefix} -- {ar}") } else { format!("{prefix} {ar}") };
        base(Level::Area, format!("area:{prefix}|{ar}"), cmd, Some(ar))
    });
    Some((runner, area))
}

/// Whether a step is in `Sat(p)`: a test run of p's family that executed tests (in whatever
/// environment: the goal class the replay metric is calibrated with), under p's area for an area
/// proposal.
pub fn satisfies(p: &Proposal, st: &StepObs, root: &str) -> bool {
    let Some(a) = outcome::action_of(st) else { return false };
    if a.role != Role::TestRun || a.runner != p.family || !outcome::executed_tests(&st.call.output) {
        return false;
    }
    match &p.area {
        None => true,
        Some(ar) => area_of(&a.targets, root).is_some_and(|x| &x == ar),
    }
}

/// The first good outcome at or after `from`: (step, calls to it).
fn good_outcome(s: &Session, from: usize) -> Option<(usize, u32)> {
    let end = (from + HORIZON as usize).min(s.steps.len());
    (from..end)
        .find(|&j| outcome::test_goal(&s.steps[j]) && tests_passed(&s.steps[j].call.output))
        .map(|j| (j, (j + 1 - from) as u32))
}

/// Calls from a decision point to the first test run that ran tests (censored at the horizon), and
/// the first test run itself.
fn to_goal(s: &Session, from: usize) -> (u32, Option<usize>) {
    let end = (from + HORIZON as usize).min(s.steps.len());
    let calls = (from..end).find(|&j| outcome::is_goal(&s.steps[j], Goal::Test)).map(|j| (j + 1 - from) as u32);
    let next = (from..end).find(|&j| outcome::is_test_run(&s.steps[j]));
    (calls.unwrap_or(HORIZON), next)
}

/// The repository root a session's paths are relative to, as a string.
fn root_of(s: &Session) -> String {
    s.root.to_string_lossy().to_string()
}

/// Build a repository's case store from its training sessions, and value every proposal
/// (leave one session out). `env_kind` is the repository's declared environment (`value::Statics`).
pub fn build(repo: &str, ss: &[&Session], e: &Embedder, priors: &Priors, env_kind: &str) -> CaseStore {
    let mut store = CaseStore { version: VERSION, repo: repo.to_string(), embedder: e.id(), ..Default::default() };
    let mut index: BTreeMap<String, u32> = BTreeMap::new();
    let mut pts: Vec<(usize, Point)> = Vec::new();
    let mut add = |p: Proposal, store: &mut CaseStore| -> u32 {
        *index.entry(p.key.clone()).or_insert_with(|| {
            store.proposals.push(p);
            (store.proposals.len() - 1) as u32
        })
    };
    for (si, s) in ss.iter().enumerate() {
        if s.steered {
            continue;
        }
        let root = root_of(s);
        for pt in points(s) {
            let prompt = s.prompts.get(pt.prompt).map(|p| p.text.as_str()).unwrap_or(&s.prompt);
            let (p, sv) = vectors(e, prompt, &s.steps[..pt.after]);
            let mut case = Case { session: s.id.clone(), kind: pt.kind, after: pt.after as u32, p, s: sv, proposals: vec![], calls: 0 };
            if let Some((j, calls)) = good_outcome(s, pt.after) {
                if let Some((runner, area)) = proposals_of(&s.steps[j], &root) {
                    case.proposals.push(add(runner, &mut store));
                    if let Some(a) = area {
                        case.proposals.push(add(a, &mut store));
                    }
                    case.calls = calls;
                }
            }
            store.cases.push(case);
            pts.push((si, pt));
        }
    }
    // Leave one session out: where would each proposal have been given, and what happened there.
    #[derive(Default)]
    struct Obs {
        sat: Vec<f64>,
        other: Vec<f64>,
        misfire: f64,
        exposed: BTreeSet<String>,
    }
    let mut obs: BTreeMap<usize, Obs> = BTreeMap::new();
    for (ci, (si, pt)) in pts.iter().enumerate() {
        let s = ss[*si];
        let c = &store.cases[ci];
        let Some(g) = store.propose(pt.kind, &c.p, &c.s, Some(&s.id), true) else { continue };
        let o = obs.entry(g.index).or_default();
        // Once per session: the first time it would have been given.
        if !o.exposed.insert(s.id.clone()) {
            continue;
        }
        let (calls, next) = to_goal(s, pt.after);
        let root = root_of(s);
        let in_sat = next.is_some_and(|j| satisfies(g.proposal, &s.steps[j], &root));
        if in_sat {
            o.sat.push(calls as f64);
            if next.is_some_and(|j| outcome::env_failure(&s.steps[j].call.output).is_some()) {
                o.misfire += 1.0;
            }
        } else {
            o.other.push(calls as f64);
        }
    }
    for (i, p) in store.proposals.iter_mut().enumerate() {
        let Some(o) = obs.get(&i) else { continue };
        let key = outcome::SituationKey {
            ecosystem: outcome::ecosystem(&p.family).to_string(),
            env_kind: env_kind.to_string(),
            trigger: "case".into(),
            phase: "pre-goal".into(),
        };
        let (m0, k0, _, _) = priors.prior(&key, "null");
        let (m1, k1, ma, mb) = priors.prior(&key, &p.env);
        let q_other = stats::gamma_poisson(o.other.iter().sum(), o.other.len() as f64, m0, k0);
        let q_sat = stats::gamma_poisson(o.sat.iter().sum(), o.sat.len() as f64, m1, k1);
        let n_sat = o.sat.len() as f64;
        let misfire = (ma.max(MISFIRE_PRIOR.0) + o.misfire, mb.max(MISFIRE_PRIOR.1) + n_sat - o.misfire);
        let share = (1.0 + n_sat, 1.0 + o.other.len() as f64);
        let adv = stats::advantage_mix(q_other, q_sat, share, FOLLOW_PRIOR, misfire, DIRECTIVE_COST, MISFIRE_COST);
        p.exposed = o.exposed.len() as u32;
        p.sat = o.sat.len() as u32;
        p.effective = n_sat + k1;
        p.admitted = adv.lower > 0.0 && p.effective >= MIN_EFFECTIVE;
        p.advantage = Some(adv);
    }
    store
}

/// Task identity by prompt similarity: sessions whose prompts' cosine is ≥ `tau` (transitively)
/// share a task. Returns a group id per vector (the smallest member index), deterministic. An
/// empty vector is its own task.
pub fn task_groups(prompts: &[Vec<i16>], tau: f64) -> Vec<usize> {
    let n = prompts.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut y = x;
        while p[y] != r {
            let next = p[y];
            p[y] = r;
            y = next;
        }
        r
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if !prompts[i].is_empty() && dot(&prompts[i], &prompts[j]) >= tau {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                let (lo, hi) = (a.min(b), a.max(b));
                parent[hi] = lo;
            }
        }
    }
    (0..n).map(|i| find(&mut parent, i)).collect()
}

/// A tool call's steps, exactly as the importer makes them (for replay: one situation builder for
/// learning and for replay).
pub fn steps_of_call(
    turn: u32,
    prompt: u16,
    call: &adage_runtime::Call,
    root: &std::path::Path,
    table: &adage_core::CommandTable,
) -> Vec<StepObs> {
    let actions = adage_core::tools::normalize_tool(&call.tool, &call.input, &root.to_string_lossy(), table);
    crate::import::call_steps(turn, prompt, call, actions, root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passing_runs_are_read_from_their_output() {
        assert!(tests_passed("============ 47 passed in 0.32s ============"));
        assert!(!tests_passed("==== 1 failed, 46 passed in 0.4s ===="));
        assert!(tests_passed(" Test Files  1 passed (1)\n      Tests  3 passed (3)"));
        assert!(!tests_passed(" Test Files  1 failed (1)\n      Tests  1 failed | 2 passed (3)"));
        assert!(tests_passed("ok  \tgithub.com/urfave/cli/v3\t0.412s"));
        assert!(!tests_passed("--- FAIL: TestX (0.00s)\nFAIL\tgithub.com/urfave/cli/v3\t0.4s"));
        assert!(tests_passed("Ran 4 tests in 0.01s\n\nOK"));
        assert!(!tests_passed("/usr/bin/python: No module named pytest"));
    }

    #[test]
    fn areas_are_monorepo_packages() {
        let f = |p: &str| Target::File(p.to_string());
        assert_eq!(
            area_of(&[f("packages/runtime-core/__tests__/a.spec.ts"), f("packages/runtime-core/__tests__/b.spec.ts")], "/r")
                .as_deref(),
            Some("packages/runtime-core")
        );
        assert_eq!(area_of(&[f("/r/packages/x/__tests__/a.spec.ts")], "/r").as_deref(), Some("packages/x"));
        assert_eq!(area_of(&[f("tests/test_core.py")], "/r"), None, "one segment: no area");
        assert_eq!(area_of(&[f("packages/a/x.spec.ts"), f("packages/b/y.spec.ts")], "/r"), None);
        assert_eq!(area_of(&[], "/r"), None);
    }

    #[test]
    fn task_groups_are_transitive_and_deterministic() {
        let v = |x: [f32; 2]| adage_intent::quantize(&adage_intent::unit(x.to_vec()));
        let ps = vec![v([1.0, 0.0]), v([0.0, 1.0]), v([1.0, 0.05]), vec![], v([0.02, 1.0])];
        assert_eq!(task_groups(&ps, 0.95), vec![0, 1, 0, 3, 1]);
    }
}
