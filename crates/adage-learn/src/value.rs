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
//! `examples/outcome_priors.rs`, κ by the method of moments). `p_follow` has a fixed prior until
//! follow judgements exist. The value is `A = p_follow · (Q(∅) − Q(Sat d)) − C(d) − p_misfire · m`
//! ([`adage_graph::stats::advantage_mix`], with `Q(∅)` the mixture of the agents who already take
//! `Sat(d)` and those who do something else).
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
                .filter(|a| !a.x.next.as_ref().is_some_and(|r| r.env == env))
                .map(|a| a.x.reward.calls as f64)
                .collect();
            let (m0, k0, _, _) = priors.prior(&key, "null");
            let (m1, k1, ma, mb) = priors.prior(&key, &role);
            let q_other = stats::gamma_poisson(other.iter().sum(), other.len() as f64, m0, k0);
            let q_sat =
                stats::gamma_poisson(sat.iter().map(|a| a.x.reward.calls as f64).sum(), sat.len() as f64, m1, k1);
            let mis = sat.iter().filter(|a| a.x.next_misfired).count() as f64;
            let misfire = (ma + mis, mb + sat.len() as f64 - mis);
            let share = (1.0 + sat.len() as f64, 1.0 + other.len() as f64);
            let adv = stats::advantage_mix(q_other, q_sat, share, FOLLOW_PRIOR, misfire, DIRECTIVE_COST, MISFIRE_COST);
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
            let admitted = verified || (adv.lower > 0.0 && effective >= MIN_EFFECTIVE);
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
                ..Default::default()
            },
            authored: false,
        };
        let why = Why {
            shape: "avoid".into(),
            pattern: format!(
                "[{} ✗ {}] =value=> [{}]: A {:.2} calls (sd {:.2}, P(A>0) {:.2}); Q(∅) {:.2}, Q(Sat) {:.2}; {} triggers, {} took it; {}",
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
        assert!(judged(2, 0, 8).mean < 0.0);
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
}
