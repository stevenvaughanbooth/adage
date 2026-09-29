//! Directive value (outcome-optimisation §2.3–2.4, §4.2): which verified directive, or silence,
//! gets the agent to its next productive event fastest in a situation.
//!
//! For a situation key `s` ([`crate::outcome::SituationKey`]) and a candidate directive `d` (run
//! the test family in a given environment), the traces give
//!
//! - `Q(s, ∅)`: tool calls from the trigger to the first test run that ran tests, over every
//!   unexposed trace at `s` (what agents do with no directive);
//! - `Q(s, Sat d)`: the same over the traces whose next test run was in `Sat(d)` (the same
//!   environment and tool, any arguments);
//! - `p_misfire`: how often that next run failed for the environment again;
//!
//! each as a posterior whose prior is supplied from the level above (global → ecosystem →
//! environment kind → situation → this repository: [`Priors`], fitted across repositories by
//! `examples/outcome_priors.rs`, κ by the method of moments). The value is
//! `A = (p_follow − p_self) · (Q(other) − Q(Sat d)) − C(d) − p_misfire · m` ([`advantage_uplift`]).
//!
//! **Uplift** (evals/lab/research/uptake-delivery): agents follow about 96% of directives, but
//! half the time they would have taken the step anyway, so a directive is worth its uplift,
//! `p_follow − p_self`, not its follow rate. Both are Beta posteriors ([`uplift`]):
//!
//! - `p_self` starts from its kind's prior ([`kind_of`]): a cross-repository cell when one is fitted
//!   ([`Priors::uptake`], `examples/outcome_priors.rs`), else the research's measured constant
//!   ([`SELF_PRIORS`], κ = [`SELF_KAPPA`]). It is updated by the repository's unexposed training
//!   traces in the matching situation (the route's own count where that is the unaided rate, else
//!   judged with the learned follow judge, [`unaided_in_traces`]) and by the control arm's
//!   `would_have` judgements from the spools.
//! - `p_follow` starts from its kind's research prior ([`FOLLOW_PRIORS`]) and is updated by the
//!   treated arm's judgements.
//!
//! [`assess`] records every route's uplift and puts a route below [`UPLIFT_MIN`] (with at least
//! [`UPLIFT_MIN_EVIDENCE`] situations of evidence) out of service as nascent: `adage review`
//! and `adage sense` say why it is quiet, and a person's approval delivers it anyway. The value
//! model's own candidates (the dead end, the tests of the edit) are gated the same way at
//! admission.
//!
//! **Candidates** are safe by construction: the project's own environment running the tool that
//! failed (declared by the repository, [`Statics`], or seen running tests in a trace). An install
//! is never a candidate. **Admission** is verified (declared, and seen running tests in at least one
//! trace) or a positive one-sided 90% lower bound on A with enough effective samples. **Strength**
//! comes from A under the ceiling: fire on a positive lower bound, prime on a positive mean with
//! P(A > 0) ≥ 0.8, else nothing.

use crate::import::Session;
use crate::outcome::{self, Env, Runner, Situation, SituationKey};
use adage_core::Effect;
use adage_graph::stats::{self, Advantage};
use adage_tracks::why::Why;
use adage_tracks::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Sufficient statistics of one cell of the hierarchy: calls to the goal over `n` triggers, and
/// misfires among them; `kappa` is the concentration a child of this cell gets as its prior.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub n: f64,
    pub sum: f64,
    #[serde(default)]
    pub misfire: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kappa: Option<f64>,
}

/// Cross-repository priors: cells keyed `<ecosystem>|<env_kind>|<trigger>|<role>` with `*` for a
/// level pooled over (`*|*|*|null` is the global cell of `Q(∅)`). Roles: `null` (`Q(∅)`), and
/// the environment kind of a candidate's `Sat(d)` (`venv`, `managed`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Priors {
    #[serde(default)]
    pub cells: BTreeMap<String, Cell>,
    /// Cross-repository `p_self` per directive kind ([`kind_of`]): how often agents take the
    /// directed step unaided, pooled over other repositories' routes of the kind (fitted by
    /// `examples/outcome_priors.rs`). A kind with no cell has the research prior ([`SELF_PRIORS`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uptake: BTreeMap<String, SelfCell>,
}

/// One kind's pooled unaided rate: `took` of `n` situations, and the concentration a route of the
/// kind gets as its prior (fitted by the method of moments across repositories).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelfCell {
    pub n: f64,
    pub took: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kappa: Option<f64>,
}

/// The prior of a cell no data reaches: a weak prior at this many calls.
pub const DEFAULT_CALLS: f64 = 4.0;
/// The concentration of that prior, and of a cell with no fitted κ.
pub const DEFAULT_KAPPA: f64 = 1.0;
/// `p_follow`'s prior (Beta): follows of a verified command are common (U11 was followed 41 of 41
/// times once judged correctly), not certain.
pub const FOLLOW_PRIOR: (f64, f64) = (4.0, 1.0);
/// `p_misfire`'s prior (Beta).
pub const MISFIRE_PRIOR: (f64, f64) = (0.5, 4.5);
/// What a directive costs, in tool calls (its tokens carried in context).
pub const DIRECTIVE_COST: f64 = 0.1;
/// What a misfire costs, in tool calls.
pub const MISFIRE_COST: f64 = 2.0;
/// Effective samples (prior plus data) a value must rest on to admit a directive on value alone.
pub const MIN_EFFECTIVE: f64 = 3.0;

impl Priors {
    /// The levels of a key, general first.
    fn levels(k: &SituationKey, role: &str) -> [String; 4] {
        [
            format!("*|*|*|{role}"),
            format!("{}|*|*|{role}", k.ecosystem),
            format!("{}|{}|*|{role}", k.ecosystem, k.env_kind),
            format!("{}|{}|{}|{role}", k.ecosystem, k.env_kind, k.trigger),
        ]
    }

    /// The prior (mean, κ) for this repository's cell of `k` and `role`: the posterior down the
    /// levels, each level's κ the prior of the next.
    ///
    /// Levels with no data for `role` are passed over. When no level has any, the `null` chain is
    /// the prior: with no evidence that the directed class does differently anywhere, it is
    /// what agents do anyway, so a directive's value starts at zero rather than at the difference
    /// between two unrelated defaults.
    pub fn prior(&self, k: &SituationKey, role: &str) -> (f64, f64, f64, f64) {
        let (mut m, mut kappa) = (DEFAULT_CALLS, DEFAULT_KAPPA);
        let (mut mis_a, mut mis_b) = MISFIRE_PRIOR;
        let levels = Self::levels(k, role);
        let chain = if levels.iter().any(|l| self.cells.contains_key(l)) { levels } else { Self::levels(k, "null") };
        for l in chain {
            if let Some(c) = self.cells.get(&l) {
                let (a, b) = stats::gamma_poisson(c.sum, c.n, m, kappa);
                m = a / b;
                kappa = c.kappa.unwrap_or(DEFAULT_KAPPA);
                // The misfire rate pools the same way, at the same concentration.
                let p = (c.misfire + mis_a) / (c.n + mis_a + mis_b);
                mis_a = p * kappa.max(1.0);
                mis_b = (1.0 - p) * kappa.max(1.0);
            }
        }
        (m, kappa, mis_a, mis_b)
    }

    /// Add one repository's situations to the cells (for fitting across repositories).
    pub fn add(&mut self, rows: &[Row]) {
        for r in rows {
            for role in ["null".to_string()].into_iter().chain(r.role.clone()) {
                for l in Self::levels(&r.key, &role) {
                    let c = self.cells.entry(l).or_default();
                    c.n += 1.0;
                    c.sum += r.calls as f64;
                    if role != "null" && r.misfired {
                        c.misfire += 1.0;
                    }
                }
            }
        }
    }
}

/// One situation as the value model reads it: its key, the role of the agent's next test run
/// (the environment kind it ran in, when that is a candidate's), and the local reward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub key: SituationKey,
    pub role: Option<String>,
    pub calls: u32,
    pub misfired: bool,
    pub harm: bool,
}

/// What the repository declares about its environment (a static scan of its files, the same one
/// instincts bind from).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Statics {
    /// `venv`, `managed`, `node_modules` or `none`.
    pub env_kind: String,
    /// The project's test command in its own environment, per test family
    /// (`pytest` → `.venv/bin/python -m pytest`, or `uv run pytest`).
    pub declared: BTreeMap<String, (Env, String)>,
}

impl Statics {
    /// The statics of a repository root (none when it is not known or not readable).
    pub fn of(root: Option<&std::path::Path>) -> Statics {
        let Some(root) = root.filter(|r| r.is_dir()) else {
            return Statics { env_kind: "none".into(), ..Default::default() };
        };
        let f = adage_instincts::declared::extract(root);
        let py = &f.python;
        let mut s = Statics { env_kind: "none".into(), ..Default::default() };
        if let Some(v) = &py.venv {
            s.env_kind = "venv".into();
            if py.pytest_configured || py.pyproject {
                s.declared.insert("pytest".into(), (Env::Venv(v.clone()), format!("{v}/bin/python -m pytest")));
            }
        } else if py.uv {
            s.env_kind = "managed".into();
            if let Some(g) = &py.uv_pytest_group {
                let c = if g.is_empty() { "uv run pytest".to_string() } else { format!("uv run --group {g} pytest") };
                s.declared.insert("pytest".into(), (Env::Managed("uv".into()), c));
            }
        } else if root.join("node_modules").is_dir() {
            s.env_kind = "node_modules".into();
        }
        s
    }
}

/// The role a next test run plays: the kind of project environment it ran in (`venv`, `managed`,
/// `script`, …), or none (the system, the user site).
fn role_of(r: &Runner) -> Option<String> {
    (r.env.class() == adage_core::action::EnvClass::Project).then(|| r.env.kind().to_string())
}

/// The value model's rows of one trace.
pub fn rows(s: &Session, env_kind: &str) -> Vec<Row> {
    outcome::situations(s, env_kind)
        .into_iter()
        .map(|x: Situation| Row {
            role: x.next.as_ref().and_then(role_of),
            key: x.key,
            calls: x.reward.calls,
            misfired: x.next_misfired,
            harm: x.reward.harm,
        })
        .collect()
}

/// A candidate's value at one situation key, with what it rests on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Valued {
    pub key: SituationKey,
    /// The failing runner's command (`python -m pytest`), for an after-failure trigger.
    pub failed: Option<String>,
    /// The signature text it fails with.
    pub signature: String,
    /// The command to run instead.
    pub command: String,
    pub role: String,
    pub advantage: Advantage,
    /// Triggers at the key (unexposed), and those whose next run was in `Sat(d)`.
    pub n_null: u32,
    pub n_sat: u32,
    /// Traces in which a run in `Sat(d)` ran tests.
    pub ran_tests: u32,
    /// The repository declares the candidate's environment.
    pub declared: bool,
    pub effective: f64,
    pub harm_in_window: u32,
    /// What delivering it changes ([`uplift`]): gated, it is not delivered.
    pub uplift: Uplift,
    pub strength: Option<Strength>,
}

/// The failing spelling of a runner (`python -m pytest`, `pytest`): its argv up to the tool.
fn failing_spelling(s: &Session, at: usize) -> Option<String> {
    for av in outcome::argvs(&s.steps[at].norm) {
        let Some(family) = outcome::test_family(av) else { continue };
        let lead = av.iter().take_while(|t| t.contains('=') && !t.starts_with('-')).count();
        let prog = av[lead].as_str();
        // `python -m pytest` (the interpreter's own flags dropped), `pytest`, `go test`.
        if let Some(k) = av.iter().skip(lead + 1).position(|t| t == "-m") {
            let module = av.get(lead + 2 + k)?;
            return Some(format!("{prog} -m {module}"));
        }
        let words = family.split_whitespace().count();
        return Some(av[lead..(lead + words).min(av.len())].join(" "));
    }
    None
}

/// Value every safe candidate at the repository's failure situations (`no-module:*`,
/// `cmd-not-found:*` of a test runner), and decide admission and strength.
pub fn value_failures(ss: &[&Session], statics: &Statics, priors: &Priors) -> Vec<Valued> {
    // The situations, per key and failing spelling.
    struct At<'a> {
        s: &'a Session,
        x: Situation,
        spelling: String,
    }
    let mut at: Vec<At> = Vec::new();
    for s in ss {
        for x in outcome::situations(s, &statics.env_kind) {
            if x.failed.is_none() {
                continue;
            }
            let Some(spelling) = failing_spelling(s, x.at) else { continue };
            at.push(At { s, x, spelling });
        }
    }
    // Candidates per failing family: the declared environment, and every project or managed
    // environment a trace ran this family's tests in.
    let mut out = Vec::new();
    // The signature text of a key: the one its first trace printed (sorted traces: deterministic).
    let mut texts: BTreeMap<SituationKey, String> = BTreeMap::new();
    for a in &at {
        if let Some(t) = &a.x.signature {
            texts.entry(a.x.key.clone()).or_insert_with(|| t.clone());
        }
    }
    let keys: BTreeSet<(SituationKey, String, String)> = at
        .iter()
        .map(|a| {
            (a.x.key.clone(), a.x.failed.as_ref().map(|r| r.family.clone()).unwrap_or_default(), a.spelling.clone())
        })
        .collect();
    for (key, family, spelling) in keys {
        let Some(signature) = texts.get(&key).cloned() else { continue };
        let mut cands: BTreeMap<String, (Env, bool)> = BTreeMap::new();
        if let Some((env, cmd)) = statics.declared.get(&family) {
            cands.insert(cmd.clone(), (env.clone(), true));
        }
        for s in ss {
            for st in &s.steps {
                if let Some(r) = outcome::runner(st).filter(|r| r.family == family && spelled(r).is_some()) {
                    if outcome::executed_tests(&st.call.output) {
                        if let Some(c) = spelled(&r) {
                            cands.entry(c).or_insert((r.env.clone(), false));
                        }
                    }
                }
            }
        }
        let mine: Vec<&At> = at.iter().filter(|a| a.x.key == key && a.spelling == spelling).collect();
        for (command, (env, declared)) in cands {
            let role = env.kind().to_string();
            let unexposed: Vec<&&At> = mine.iter().filter(|a| !a.s.exposed_to(&[command.as_str()], false)).collect();
            let null: Vec<u32> = unexposed.iter().map(|a| a.x.reward.calls).collect();
            let sat: Vec<&&&At> =
                unexposed.iter().filter(|a| a.x.next.as_ref().is_some_and(|r| r.env == env)).collect();
            let other: Vec<f64> = unexposed
                .iter()
                .filter(|a| a.x.next.as_ref().is_none_or(|r| r.env != env))
                .map(|a| a.x.reward.calls as f64)
                .collect();
            let (m0, k0, _, _) = priors.prior(&key, "null");
            let (m1, k1, ma, mb) = priors.prior(&key, &role);
            let q_other = stats::gamma_poisson(other.iter().sum(), other.len() as f64, m0, k0);
            let q_sat =
                stats::gamma_poisson(sat.iter().map(|a| a.x.reward.calls as f64).sum(), sat.len() as f64, m1, k1);
            let mis = sat.iter().filter(|a| a.x.next_misfired).count() as f64;
            let misfire = (ma + mis, mb + sat.len() as f64 - mis);
            // What delivering changes: the unexposed triggers whose agent reached `Sat(d)` unaided
            // are `p_self`'s evidence (the research's prior for a dead end: 0.29).
            let unaided =
                Unaided { traces: unexposed.len() as u32, traces_took: sat.len() as u32, ..Default::default() };
            let up = uplift("avoid", &unaided, priors);
            let adv = advantage_uplift(q_other, q_sat, &up, misfire, DIRECTIVE_COST, MISFIRE_COST);
            let ran_tests = ss
                .iter()
                .filter(|s| {
                    s.steps.iter().any(|st| {
                        outcome::runner(st).is_some_and(|r| r.env == env && r.family == family)
                            && outcome::executed_tests(&st.call.output)
                    })
                })
                .count() as u32;
            let effective = sat.len() as f64 + k1;
            let verified = declared && ran_tests >= 1;
            // Verified or valued, and worth delivering: agents who already recover to it unaided
            // gain nothing from being told.
            let admitted = (verified || (adv.lower > 0.0 && effective >= MIN_EFFECTIVE)) && !up.summary.gated;
            let strength = admitted.then(|| strength_of(&adv, Effect::Verify, false)).flatten();
            out.push(Valued {
                key: key.clone(),
                failed: Some(spelling.clone()),
                signature: signature.clone(),
                command,
                role,
                advantage: adv,
                n_null: null.len() as u32,
                n_sat: sat.len() as u32,
                ran_tests,
                declared,
                effective,
                harm_in_window: unexposed.iter().filter(|a| a.x.reward.harm).count() as u32,
                uplift: up.summary,
                strength,
            });
        }
    }
    out
}

/// A runner's command in its environment (`.venv/bin/python -m pytest`, `uv run pytest`).
fn spelled(r: &Runner) -> Option<String> {
    let tool = match r.family.as_str() {
        "pytest" => "-m pytest",
        "unittest" => "-m unittest",
        _ => return None,
    };
    match &r.env {
        Env::Venv(d) => Some(format!("{d}/bin/python {tool}")),
        Env::Managed(m) if r.family == "pytest" && matches!(m.as_str(), "uv" | "poetry" | "pdm" | "hatch") => {
            Some(format!("{m} run pytest"))
        }
        _ => None,
    }
}

/// What a directive that works saves when followed, in tool calls, before any trace of the
/// repository says otherwise: U11's measured effect (1.85 calls from the dead end to a test run
/// that ran tests, RepoBench v2), with the width of its interval.
pub const PRIOR_SAVING: (f64, f64) = (1.85, 0.35);

/// The value of a directive from its runtime judgements alone (an instinct's, which no trace
/// valued): `p_follow` from follows and ignores (an ignore only lowers it), `p_misfire` from
/// misfires, and the saving when followed from [`PRIOR_SAVING`] (outcome-optimisation §2.7).
pub fn judged(followed: u32, ignored: u32, misfired: u32) -> Advantage {
    let n = (followed + ignored + misfired) as f64;
    let (m, sd) = PRIOR_SAVING;
    // Gamma posteriors whose difference has mean m and variance sd²: Q(∅) = m + 1, Q(Sat) = 1.
    let shape = |mean: f64, var: f64| (mean * mean / var, mean / var);
    let q_null = shape(m + 1.0, sd * sd / 2.0);
    let q_sat = shape(1.0, sd * sd / 2.0);
    let follow = (FOLLOW_PRIOR.0 + followed as f64, FOLLOW_PRIOR.1 + ignored as f64);
    let misfire = (MISFIRE_PRIOR.0 + misfired as f64, MISFIRE_PRIOR.1 + n - misfired as f64);
    stats::advantage(q_null, q_sat, follow, misfire, DIRECTIVE_COST, MISFIRE_COST)
}

/// Strength from the value of a directive under the effect's ceiling (outcome-optimisation §4.2):
/// fire on a positive one-sided 90% lower bound (and a fire ceiling), prime on a positive mean with
/// P(A > 0) ≥ 0.8, else none.
pub fn strength_of(a: &Advantage, effect: Effect, by_edit: bool) -> Option<Strength> {
    let max = ceiling(effect, by_edit)?;
    if a.lower > 0.0 && max == Strength::Fire {
        Some(Strength::Fire)
    } else if a.mean > 0.0 && a.p_positive >= 0.8 {
        Some(Strength::Prime)
    } else {
        None
    }
}

/// The routes of the valued failures that have a strength: after the failure, run the candidate
/// with the best value (one per failing spelling and signature; ties by command).
pub fn failure_routes(valued: &[Valued], ss: &[&Session], provenance: &str) -> Vec<(Route, Why)> {
    let table = adage_core::CommandTable::load();
    let mut best: BTreeMap<(String, String), &Valued> = BTreeMap::new();
    for v in valued.iter().filter(|v| v.strength.is_some()) {
        let k = (v.failed.clone().unwrap_or_default(), v.signature.clone());
        let better = |o: &&Valued| {
            (v.advantage.mean, std::cmp::Reverse(&v.command)) > (o.advantage.mean, std::cmp::Reverse(&o.command))
        };
        if best.get(&k).is_none_or(better) {
            best.insert(k, v);
        }
    }
    // The most common failure first.
    let mut best: Vec<((String, String), &Valued)> = best.into_iter().collect();
    best.sort_by(|a, b| b.1.n_null.cmp(&a.1.n_null).then_with(|| a.0.cmp(&b.0)));
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for ((failed, signature), v) in best {
        let (Some(rkey), Some(wkey)) = (
            adage_instincts::safety::command_key(&failed, &table),
            adage_instincts::safety::command_key(&v.command, &table),
        ) else {
            continue;
        };
        if rkey == wkey || !io::safe_command(&v.command) {
            continue;
        }
        // Named by all it says (working command, failing runner, signature): the dead-end ids an
        // instinct takes first name less, and a learned route must never take the id of an
        // instinct for another failure. The instinct for this very failure yields to it as the
        // same step at the same point.
        let id = format!("dead-end-{}", adage_instincts::safety::stable(&[&wkey, &rkey, &signature]));
        if !used.insert(id.clone()) {
            continue;
        }
        let a = &v.advantage;
        let support = v.n_sat;
        let opp = v.n_null.max(support);
        let route = Route {
            id,
            summary: "After this failure, what works instead in this repository".into(),
            trigger: Trigger::AfterFailure { command: rkey, signature: signature.clone() },
            step: Step::Run { command: v.command.clone() },
            slots: vec![],
            effect: Effect::Verify,
            strength: v.strength.expect("valued with a strength"),
            state: initial_state(Effect::Verify),
            exit: vec![Exit::IntentChanges],
            evidence: Evidence {
                opportunities: opp,
                support,
                days: crate::evidence::days(ss, &BTreeSet::new()),
                rate: if opp > 0 { support as f64 / opp as f64 } else { 0.0 },
                lower_bound: wilson_lower(support, opp),
                turns_saved: (a.mean * 100.0).round() / 100.0,
                provenance: provenance.into(),
                models: crate::evidence::models(ss),
                origin: Some(Origin::Learned),
                uplift: Some(v.uplift.clone()),
                ..Default::default()
            },
            authored: false,
        };
        let why = Why {
            shape: "avoid".into(),
            pattern: format!(
                "[{} ✗ {}] =value=> [{}]: A {:.2} calls (sd {:.2}, P(A>0) {:.2}); Q(∅) {:.2}, Q(Sat) {:.2}; {} triggers, {} took it unaided; uplift {:+.2}; {}",
                failed,
                v.key.trigger,
                v.command,
                a.mean,
                a.sd,
                a.p_positive,
                a.q_null,
                a.q_sat,
                v.n_null,
                v.n_sat,
                v.uplift.mean,
                if v.declared { "declared by the repository" } else { "seen in the traces" }
            ),
            backoff: 1,
            gain: (a.mean * 100.0).round() / 100.0,
            lift: 0.0,
            conf: (a.p_positive * 1e4).round() / 1e4,
            conf_lb: (a.lower * 1e4).round() / 1e4,
            support,
            days: 0,
            members: vec![],
            examples: vec![],
            prior: None,
        };
        out.push((route, why));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Uplift (evals/lab/research/uptake-delivery): A(s, d) = (p_follow − p_self)·ΔQ − C(d) − λ·p_misfire.
// ---------------------------------------------------------------------------------------------

/// A route whose posterior uplift is below this is not delivered: agents already take its step
/// unaided nearly as often as they follow it (the research's bar: 79% of deliveries lie under it,
/// with no measurable loss).
pub const UPLIFT_MIN: f64 = 0.1;
/// A prior alone never silences a route: the gate needs at least this many situations of the
/// repository's own evidence (training traces plus control-arm judgements).
pub const UPLIFT_MIN_EVIDENCE: u32 = 3;
/// The concentration of a kind's research prior on `p_self` (pseudo-situations): a handful of the
/// repository's own situations outweighs it.
pub const SELF_KAPPA: f64 = 4.0;
/// `p_self` of a kind the research did not measure: the pooled rate over every directive kind
/// (0.50 [0.41, 0.59], 80 withheld units).
pub const SELF_POOLED: f64 = 0.5;
/// `p_self` per directive kind, measured against the randomised control's `would_have`
/// situations (uptake-delivery, "Model of p_follow", learned judge). **Priors**: a kind with a
/// cross-repository cell ([`Priors::uptake`]) uses that instead, and every route's own evidence
/// updates it.
pub const SELF_PRIORS: [(&str, f64); 7] = [
    // The environment dead end (U11 `.venv`): 12 of 41 withheld agents recovered to it unaided.
    ("avoid", 0.29),
    // The after-edit test run.
    ("run-after-edit", 0.32),
    ("read-batch", 0.93),
    // A step at prompt time (the work-item lookup; prompt-time advice measured the same).
    ("prompt", 0.95),
    ("obligation", 0.88),
    // The finish sequence behaved as the obligation (uplift 0).
    ("sequence", 0.88),
    // Session-start advice: no clean control; the pooled rate.
    ("advise", SELF_POOLED),
];
/// `p_follow`'s empirical-Bayes concentration across kinds (κ by the method of moments).
pub const FOLLOW_KAPPA: f64 = 11.8;
/// `p_follow` of a kind the research did not measure: the pooled rate (183 of 191 delivered).
pub const FOLLOW_POOLED: f64 = 0.958;
/// `p_follow` per directive kind (the research's hierarchical Beta, learned follow judge). Model
/// and wording add nothing identifiable, so the kind is the only key. Treated judgements update it.
pub const FOLLOW_PRIORS: [(&str, f64); 7] = [
    ("avoid", 0.944),
    ("run-after-edit", 0.881),
    ("read-batch", 0.986),
    ("prompt", 0.987),
    ("obligation", 0.975),
    ("sequence", 0.975),
    ("advise", 0.99),
];
/// The calls after a post-failure trigger in which an unaided agent's recovery counts (the
/// research's window for a post-failure avoid). Other triggers look to the end of the trace.
pub const AFTER_FAILURE_WINDOW: usize = 5;

/// The directive kind of a route, which keys its priors: the delivery shape, with a plain `run`
/// split by when it is delivered (after an edit, at the prompt, or otherwise).
pub fn kind_of(r: &Route) -> &'static str {
    match adage_tracks::shape(r) {
        "avoid" => "avoid",
        "read-batch" => "read-batch",
        "obligation" => "obligation",
        "sequence" => "sequence",
        // Advice is kinded by when it comes: after a failure it is the dead end's (use the
        // project's interpreter), after an edit an obligation's (regenerate, do not hand-edit).
        "advise" => match r.trigger {
            Trigger::AfterFailure { .. } => "avoid",
            Trigger::AfterEdit { .. } => "obligation",
            _ => "advise",
        },
        _ => match r.trigger {
            Trigger::AfterEdit { .. } => "run-after-edit",
            Trigger::Prompt { .. } | Trigger::Intent { .. } => "prompt",
            _ => "run",
        },
    }
}

fn prior_of(table: &[(&str, f64)], kind: &str, pooled: f64) -> f64 {
    table.iter().find(|(k, _)| *k == kind).map_or(pooled, |(_, p)| *p)
}

impl Priors {
    /// `p_self`'s prior for a kind: (mean, κ, fitted across repositories).
    pub fn self_prior(&self, kind: &str) -> (f64, f64, bool) {
        match self.uptake.get(kind).filter(|c| c.n > 0.0) {
            Some(c) => ((c.took / c.n).clamp(0.01, 0.99), c.kappa.unwrap_or(SELF_KAPPA), true),
            None => (prior_of(&SELF_PRIORS, kind, SELF_POOLED), SELF_KAPPA, false),
        }
    }
}

/// What a route's `p_self` and `p_follow` rest on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Unaided {
    /// Unexposed training traces in the matching situation, and those that took the step.
    pub traces: u32,
    pub traces_took: u32,
    /// Control-arm situations the runtime judged (`would_have`), and those that took the step.
    pub control: u32,
    pub control_took: u32,
    /// Treated judgements, and the follows among them (a misfire is a follow that failed).
    pub delivered: u32,
    pub followed: u32,
}

/// The posteriors of one route's uplift: `p_follow` and `p_self` as Betas, and the summary.
#[derive(Debug, Clone, PartialEq)]
pub struct UpliftPosterior {
    pub follow: (f64, f64),
    pub unaided: (f64, f64),
    pub summary: Uplift,
}

/// The uplift of a directive of `kind` on this evidence: `p_follow − p_self`, each a Beta
/// posterior from its kind's prior, the two independent. Gated below [`UPLIFT_MIN`] when at least
/// [`UPLIFT_MIN_EVIDENCE`] situations of the repository's own say so.
///
/// The training traces and the control arm both count toward `p_self`. They can overlap (a
/// control session is also a training trace once learned from), which overstates the certainty
/// but not the rate: the control's judgement is the runtime's own at the exact trigger.
pub fn uplift(kind: &str, u: &Unaided, priors: &Priors) -> UpliftPosterior {
    let (m, k, fitted) = priors.self_prior(kind);
    let took = (u.traces_took.min(u.traces) + u.control_took.min(u.control)) as f64;
    let n = (u.traces + u.control) as f64;
    let unaided = (m * k + took, (1.0 - m) * k + n - took);
    let f = prior_of(&FOLLOW_PRIORS, kind, FOLLOW_POOLED);
    let followed = u.followed.min(u.delivered) as f64;
    let follow = (f * FOLLOW_KAPPA + followed, (1.0 - f) * FOLLOW_KAPPA + u.delivered as f64 - followed);
    let (pf, vf) = stats::beta_moments(follow.0, follow.1);
    let (ps, vs) = stats::beta_moments(unaided.0, unaided.1);
    let mean = pf - ps;
    let lower = mean - stats::ADVANTAGE_Z * (vf + vs).sqrt();
    let r = |x: f64| (x * 1e4).round() / 1e4;
    let gated = mean < UPLIFT_MIN && u.traces + u.control >= UPLIFT_MIN_EVIDENCE;
    UpliftPosterior {
        follow,
        unaided,
        summary: Uplift {
            kind: kind.to_string(),
            p_follow: r(pf),
            p_self: r(ps),
            mean: r(mean),
            lower: r(lower),
            prior_self: r(m),
            prior_fitted: fitted,
            traces: u.traces,
            traces_took: u.traces_took,
            control: u.control,
            control_took: u.control_took,
            delivered: u.delivered,
            followed: u.followed,
            gated,
        },
    }
}

/// [`stats::advantage_mix`] with the research's uplift in place of `p_follow · (1 − p_sat)`:
/// `A = (p_follow − p_self) · (Q(other) − Q(Sat d)) − C − p_misfire · m`, where `p_follow` is
/// the unconditional rate of taking `Sat(d)` once told and `p_self` the rate unaided.
pub fn advantage_uplift(
    q_other: (f64, f64),
    q_sat: (f64, f64),
    up: &UpliftPosterior,
    misfire: (f64, f64),
    cost: f64,
    misfire_cost: f64,
) -> Advantage {
    let (m0, s0) = stats::gamma_moments(q_other);
    let (m1, s1) = stats::gamma_moments(q_sat);
    let (pf, vf) = stats::beta_moments(up.follow.0, up.follow.1);
    let (ps, vs) = stats::beta_moments(up.unaided.0, up.unaided.1);
    let (pm, vm) = stats::beta_moments(misfire.0, misfire.1);
    let (u, vu) = (pf - ps, vf + vs);
    let d = m0 - m1;
    let vd = s0 * s0 + s1 * s1;
    let mean = u * d - cost - pm * misfire_cost;
    let var = u * u * vd + vu * d * d + vu * vd + misfire_cost * misfire_cost * vm;
    let sd = var.max(0.0).sqrt();
    let p_positive = if sd > 0.0 {
        stats::normal_cdf(mean / sd)
    } else if mean > 0.0 {
        1.0
    } else {
        0.0
    };
    Advantage {
        mean,
        sd,
        p_positive,
        lower: mean - stats::ADVANTAGE_Z * sd,
        q_null: ps * m1 + (1.0 - ps) * m0,
        q_sat: m1,
    }
}

/// The command a route directs, as a follow judgement reads it (slots dropped): its first step
/// command, or the command a session-start advice names.
fn directed_command(r: &Route) -> Option<String> {
    let c = match &r.step {
        Step::Advise { slots, .. } => ["command", "interpreter", "manager"].iter().find_map(|k| slots.get(*k).cloned()),
        s => adage_graph::compile::commands(s).first().map(|c| c.to_string()),
    }?;
    let c: Vec<&str> = c.split_whitespace().filter(|t| !t.contains('{')).collect();
    (!c.is_empty()).then(|| c.join(" "))
}

/// Whether a trace's step took the directed command: the learned follow judge where the
/// repository's weights are fitted, else the structural and action judges
/// ([`adage_core::pattern::follows`], which reads a test run by [`adage_core::action::satisfies`]).
fn took(directed: &str, st: &crate::import::StepObs, weights: &adage_core::judge::Weights) -> bool {
    if st.call.failed {
        return false;
    }
    let raw = st.call.input.get("command").and_then(|c| c.as_str());
    raw.is_some_and(|o| weights.follows(directed, o))
        || adage_core::pattern::follows(&format!("exec {directed}"), &st.key)
}

/// Whether a failed call (canonical key) fires an after-failure trigger on `command`, as the
/// runtime reads it for an instinct or a bare runner: the command with options added, or any run
/// of the same runner.
fn failure_matches(command: &str, observed: &str) -> bool {
    adage_core::pattern::meets(command, observed)
        || command.strip_prefix("exec ").is_some_and(|runner| adage_core::pattern::runs_avoided(runner, observed))
}

/// The unaided rate of a route over the training traces, judged at its trigger: each unexposed
/// trace where the trigger occurs is one situation, and it took the step when a later call (within
/// [`AFTER_FAILURE_WINDOW`] after a failure) follows the directed command. `None` for a trigger
/// read only at hook time (a context, an intent) or a step with no command.
pub fn unaided_in_traces(r: &Route, ss: &[&Session]) -> Option<(u32, u32)> {
    let directed = directed_command(r)?;
    let pairs: Vec<(&str, &str)> = ss
        .iter()
        .flat_map(|s| {
            s.steps
                .iter()
                .filter_map(|st| st.call.input.get("command").and_then(|c| c.as_str()).map(|c| (s.id.as_str(), c)))
        })
        .collect();
    let weights = adage_core::judge::Weights::fit(pairs);
    let (mut took_n, mut n) = (0u32, 0u32);
    for s in ss.iter().filter(|s| !crate::engine::exposed(s, r)) {
        let at: Option<(usize, usize)> = match &r.trigger {
            Trigger::AfterFailure { command, signature } => s
                .steps
                .iter()
                .position(|x| {
                    x.call.failed && x.call.output.contains(signature.as_str()) && failure_matches(command, &x.key)
                })
                .map(|i| (i + 1, (i + 1 + AFTER_FAILURE_WINDOW).min(s.steps.len()))),
            Trigger::AfterEdit { paths } => s
                .steps
                .iter()
                .position(|x| {
                    adage_core::keys::is_author(&x.norm.action)
                        && x.rel_path.as_deref().is_some_and(|p| crate::engine::glob_hit(paths, p))
                })
                .map(|i| (i + 1, s.steps.len())),
            Trigger::AfterCommand { command } => {
                s.steps.iter().position(|x| x.key == *command && !x.call.failed).map(|i| (i + 1, s.steps.len()))
            }
            Trigger::SessionStart => Some((0, s.steps.len())),
            Trigger::Prompt { .. } => {
                let mut emb = crate::dormant::Embeddings::new(None);
                crate::dormant::occurs(r, s, &mut emb).then_some((0, s.steps.len()))
            }
            Trigger::Intent { .. } | Trigger::Context { .. } => return None,
        };
        let Some((from, to)) = at else { continue };
        n += 1;
        took_n += u32::from(s.steps[from..to].iter().any(|st| took(&directed, st, &weights)));
    }
    Some((took_n, n))
}

/// The training traces' unaided counts of a route, `(took, situations)`: what its own judgement
/// counted where that is the unaided rate (a mined route's support among the unexposed traces
/// where its trigger occurred; a valued route's traces whose next run was in `Sat(d)`), else
/// judged on the traces ([`unaided_in_traces`]). The session-start test card counts the sessions
/// it would have helped, so its unaided count is the rest.
pub fn trace_counts(r: &Route, ss: &[&Session]) -> Option<(u32, u32)> {
    if let Some(u) = &r.evidence.uplift {
        return Some((u.traces_took, u.traces));
    }
    let e = &r.evidence;
    if r.is_instinct() || e.opportunities == 0 {
        return unaided_in_traces(r, ss);
    }
    let (k, n) = (e.support.min(e.opportunities), e.opportunities);
    Some(match &r.step {
        Step::Advise { template, .. } if template == "test-command" => (n - k, n),
        _ => (k, n),
    })
}

/// Estimate every route's uplift ([`uplift`]) from the training traces and the runtime's
/// judgements of it (`judged`, keyed by repository and route), record it on the route, and put a
/// gated route in service no longer: a live route becomes nascent, so it waits for a person's
/// approval and shows in `adage review` with why. Returns the gated ids.
pub fn assess(
    routes: &mut [Route],
    ss: &[&Session],
    repo: &str,
    judged: &BTreeMap<(String, String), crate::instincts::Judged>,
    priors: &Priors,
) -> Vec<String> {
    let mut gated = Vec::new();
    for r in routes.iter_mut() {
        if r.authored {
            continue;
        }
        let (traces_took, traces) = trace_counts(r, ss).unwrap_or_default();
        let j = judged.get(&(repo.to_string(), r.id.clone())).copied().unwrap_or_default();
        let u = Unaided {
            traces,
            traces_took,
            control: j.control_total,
            control_took: j.control_followed,
            delivered: j.total(),
            followed: j.followed + j.misfired,
        };
        let up = uplift(kind_of(r), &u, priors).summary;
        if up.gated && r.state.is_live() {
            r.state = RouteState::Nascent;
            gated.push(r.id.clone());
        }
        r.evidence.uplift = Some(up);
    }
    gated
}

/// Per kind, the unaided counts of these routes (for fitting [`Priors::uptake`] across
/// repositories): `(kind, took, situations)`.
pub fn uptake_units(routes: &[Route], ss: &[&Session]) -> Vec<(String, f64, f64)> {
    routes
        .iter()
        .filter_map(|r| {
            let (k, n) = trace_counts(r, ss)?;
            (n > 0).then(|| (kind_of(r).to_string(), k as f64, n as f64))
        })
        .collect()
}

/// A Beta prior's concentration fitted by the method of moments across units `(took, n)` (one per
/// repository, say): the between-unit variance of the rates beyond binomial sampling, τ², gives
/// κ = p(1 − p)/τ² − 1. `max_kappa` when the units vary no more than sampling explains; `None`
/// with fewer than two units that have data.
pub fn fit_beta_kappa(units: &[(f64, f64)], max_kappa: f64) -> Option<f64> {
    let units: Vec<(f64, f64)> = units.iter().copied().filter(|&(_, n)| n > 0.0).collect();
    if units.len() < 2 {
        return None;
    }
    let (t, n): (f64, f64) = units.iter().fold((0.0, 0.0), |(a, b), &(x, y)| (a + x, b + y));
    let p = t / n;
    let k = units.len() as f64;
    let between = units.iter().map(|&(x, y)| (x / y - p).powi(2)).sum::<f64>() / (k - 1.0);
    let sampling = units.iter().map(|&(_, y)| p * (1.0 - p) / y).sum::<f64>() / k;
    let tau2 = between - sampling;
    if tau2 <= 0.0 || p <= 0.0 || p >= 1.0 {
        return Some(max_kappa);
    }
    Some((p * (1.0 - p) / tau2 - 1.0).clamp(0.5, max_kappa))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priors_pool_down_the_levels() {
        let k = SituationKey {
            ecosystem: "py".into(),
            env_kind: "venv".into(),
            trigger: "no-module:pytest".into(),
            phase: "pre-goal".into(),
        };
        let mut p = Priors::default();
        assert_eq!(p.prior(&k, "null").0, DEFAULT_CALLS);
        p.cells.insert("*|*|*|venv".into(), Cell { n: 10.0, sum: 15.0, misfire: 0.0, kappa: Some(4.0) });
        let (m, kappa, _, _) = p.prior(&k, "venv");
        assert!((m - (15.0 + 4.0) / 11.0).abs() < 1e-12, "{m}");
        assert_eq!(kappa, 4.0);
    }

    #[test]
    fn judgements_value_an_instinct() {
        // Followed: plainly worth it.
        assert!(judged(20, 0, 0).p_positive > 0.99);
        // Ignored a few times: its value is lower, not gone (an ignore is no harm).
        assert!(judged(0, 5, 0).p_positive > 0.5);
        // Ignored almost always, over many deliveries: not worth its tokens.
        assert!(judged(0, 200, 0).p_positive < 0.1, "{:?}", judged(0, 200, 0));
        // Misfires cost.
        assert!(judged(1, 0, 20).mean < 0.0, "{:?}", judged(1, 0, 20));
    }

    #[test]
    fn strength_follows_the_value() {
        let a = |mean: f64, sd: f64| Advantage {
            mean,
            sd,
            p_positive: stats::normal_cdf(mean / sd),
            lower: mean - stats::ADVANTAGE_Z * sd,
            q_null: 0.0,
            q_sat: 0.0,
        };
        assert_eq!(strength_of(&a(2.0, 0.5), Effect::Verify, false), Some(Strength::Fire));
        assert_eq!(strength_of(&a(1.0, 0.9), Effect::Verify, false), Some(Strength::Prime));
        assert_eq!(strength_of(&a(0.2, 1.0), Effect::Verify, false), None);
        assert_eq!(strength_of(&a(2.0, 0.5), Effect::Environment, false), Some(Strength::Prime));
        assert_eq!(strength_of(&a(2.0, 0.5), Effect::Author, false), None);
    }

    fn route(trigger: Trigger, step: Step, effect: Effect, support: u32, opportunities: u32) -> Route {
        Route {
            id: "r".into(),
            summary: String::new(),
            trigger,
            step,
            slots: vec![],
            effect,
            strength: Strength::Prime,
            state: RouteState::Firm,
            exit: vec![],
            evidence: Evidence { support, opportunities, provenance: "desire-path".into(), ..Default::default() },
            authored: false,
        }
    }

    fn traces(took: u32, of: u32) -> Unaided {
        Unaided { traces: of, traces_took: took, ..Default::default() }
    }

    #[test]
    fn uplift_is_follow_minus_unaided() {
        let p = Priors::default();
        // The dead end (research: +0.69): agents rarely recover to the project's environment.
        let u = uplift("avoid", &traces(1, 8), &p).summary;
        assert!(u.mean > 0.6 && !u.gated, "{u:?}");
        // A read batch agents already read (research: p_self 0.93, uplift ≈ 0).
        let u = uplift("read-batch", &traces(9, 10), &p).summary;
        assert!(u.mean < UPLIFT_MIN && u.gated, "{u:?}");
        // A lookup every agent already runs.
        assert!(uplift("prompt", &traces(12, 12), &p).summary.gated);
        // A prior alone never silences a route: an obligation's research prior is below the bar,
        // but with no situation of this repository's it is delivered.
        let u = uplift("obligation", &Unaided::default(), &p).summary;
        assert!(u.mean < UPLIFT_MIN && !u.gated, "{u:?}");
        // The repository's own evidence outweighs the kind's prior: a read batch agents here
        // mostly miss has uplift.
        assert!(uplift("read-batch", &traces(3, 12), &p).summary.mean > 0.3);
    }

    #[test]
    fn control_judgements_and_follows_count() {
        let p = Priors::default();
        // The control arm's would-have judgements: agents did it unaided 19 of 20 times.
        let u = Unaided { control: 20, control_took: 19, ..Default::default() };
        assert!(uplift("run-after-edit", &u, &p).summary.gated);
        // Treated judgements move p_follow: a route agents ignore has little uplift.
        let told = Unaided { delivered: 20, followed: 6, ..traces(2, 10) };
        let base = uplift("avoid", &traces(2, 10), &p).summary;
        let ignored = uplift("avoid", &told, &p).summary;
        assert!(ignored.p_follow < base.p_follow - 0.3 && ignored.mean < base.mean, "{ignored:?}");
    }

    #[test]
    fn a_fitted_cell_replaces_the_research_constant() {
        let mut p = Priors::default();
        assert_eq!(p.self_prior("read-batch"), (0.93, SELF_KAPPA, false));
        p.uptake.insert("read-batch".into(), SelfCell { n: 40.0, took: 10.0, kappa: Some(6.0) });
        assert_eq!(p.self_prior("read-batch"), (0.25, 6.0, true));
        let u = uplift("read-batch", &Unaided::default(), &p).summary;
        assert!(u.prior_fitted && u.mean > 0.5, "{u:?}");
    }

    #[test]
    fn value_scales_with_uplift() {
        let q_other = stats::gamma_poisson(40.0, 10.0, DEFAULT_CALLS, DEFAULT_KAPPA);
        let q_sat = stats::gamma_poisson(10.0, 10.0, DEFAULT_CALLS, DEFAULT_KAPPA);
        let p = Priors::default();
        let a = |u: &Unaided| advantage_uplift(q_other, q_sat, &uplift("avoid", u, &p), (0.5, 20.0), 0.1, 2.0);
        let rare = a(&traces(1, 20));
        let usual = a(&traces(19, 20));
        assert!(rare.mean > 2.0, "{rare:?}");
        // What agents already do is worth (about) nothing to tell them.
        assert!(usual.mean < 0.3 && usual.lower < 0.0, "{usual:?}");
    }

    #[test]
    fn kinds_follow_the_shape_and_the_moment() {
        let run = |t: Trigger| route(t, Step::Run { command: "make test".into() }, Effect::Verify, 0, 0);
        assert_eq!(kind_of(&run(Trigger::AfterEdit { paths: "src/**".into() })), "run-after-edit");
        assert_eq!(kind_of(&run(Trigger::Prompt { span: adage_core::spans::SpanKind::Path })), "prompt");
        assert_eq!(
            kind_of(&run(Trigger::AfterFailure { command: "exec pytest".into(), signature: "x".into() })),
            "avoid"
        );
        assert_eq!(kind_of(&run(Trigger::AfterCommand { command: "exec make".into() })), "run");
        let rb = route(
            Trigger::AfterCommand { command: "exec x".into() },
            Step::ReadBatch { slot: "files".into(), max: 5 },
            Effect::Observe,
            0,
            0,
        );
        assert_eq!(kind_of(&rb), "read-batch");
    }

    #[test]
    fn assess_quiets_what_agents_already_do() {
        let rb = route(
            Trigger::AfterCommand { command: "exec x".into() },
            Step::ReadBatch { slot: "files".into(), max: 5 },
            Effect::Observe,
            9,
            10,
        );
        let fix = Route {
            id: "d".into(),
            ..route(
                Trigger::AfterFailure { command: "exec pytest".into(), signature: "No module".into() },
                Step::Run { command: ".venv/bin/python -m pytest".into() },
                Effect::Verify,
                1,
                8,
            )
        };
        let mut card_slots = BTreeMap::new();
        card_slots.insert("command".to_string(), ".venv/bin/python -m pytest".to_string());
        // The session-start card counts the sessions it would have helped: 8 of 10 here, so 2 ran
        // tests the right way unaided.
        let card = Route {
            id: "c".into(),
            ..route(
                Trigger::SessionStart,
                Step::Advise { template: "test-command".into(), slots: card_slots },
                Effect::Observe,
                8,
                10,
            )
        };
        let mut routes = vec![rb, fix, card];
        let gated = assess(&mut routes, &[], "repo", &BTreeMap::new(), &Priors::default());
        assert_eq!(gated, vec!["r".to_string()]);
        assert_eq!(routes[0].state, RouteState::Nascent);
        let u = routes[0].evidence.uplift.as_ref().unwrap();
        assert!(u.gated && (u.traces, u.traces_took) == (10, 9), "{u:?}");
        assert!(u.line().contains("9 of 10 traces did it unaided") && u.line().contains("Quiet"), "{}", u.line());
        assert_eq!(routes[1].state, RouteState::Firm);
        assert!(routes[1].evidence.uplift.as_ref().unwrap().mean > 0.6);
        let c = routes[2].evidence.uplift.as_ref().unwrap();
        assert_eq!((c.traces, c.traces_took, c.gated), (10, 2, false));
        // Control-arm judgements from the spools quiet a route the traces could not.
        let mut judged = BTreeMap::new();
        judged.insert(
            ("repo".to_string(), "d".to_string()),
            crate::instincts::Judged { control_followed: 60, control_total: 60, ..Default::default() },
        );
        let mut routes = vec![routes[1].clone()];
        routes[0].evidence.uplift = None;
        assert_eq!(assess(&mut routes, &[], "repo", &judged, &Priors::default()), vec!["d".to_string()]);
    }

    #[test]
    fn beta_kappa_by_moments() {
        // Units that agree beyond sampling: full pooling.
        assert_eq!(fit_beta_kappa(&[(5.0, 10.0), (5.0, 10.0), (6.0, 10.0)], 20.0), Some(20.0));
        // Units that disagree: a weak prior.
        let k = fit_beta_kappa(&[(1.0, 20.0), (19.0, 20.0), (10.0, 20.0)], 20.0).unwrap();
        assert!(k < 3.0, "{k}");
        assert_eq!(fit_beta_kappa(&[(1.0, 2.0)], 20.0), None);
    }
}
