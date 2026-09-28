//! The compiler (Track G §6, G3): selected reflexes → delivery shapes → routes.
//!
//! A pure, deterministic function of the selected reflexes ([`crate::select::Reflex`]), the traces
//! they were mined from (summarised in a [`Corpus`]: command literals, effects, failure
//! signatures, the files the traces touched), the learned value types ([`crate::Learned`], G1)
//! and the parameters. Nothing here knows a route *kind*: a reflex's structure decides its
//! delivery shape ([`shapes`]), from a closed set with fixed templates:
//!
//! | Continuation | Context | Shape | Route |
//! | --- | --- | --- | --- |
//! | one act, a value flowing from a prompt | prompt | `run` | `prompt` → `run`, slot `prompt_value` |
//! | reads of paths an act's output listed | act | `read-batch` | `after_command` → `read_batch` |
//! | a working act after a failed one | a failed act | `avoid` | `session_start` → `avoid`, or `after_failure` → `run` |
//! | one regenerate act | the last edit of a file kind | `obligation` | `after_edit` → `run` |
//! | one act with a value from the edited path | an edit | `run` | `after_edit` → `run`, slot `edit_segment` |
//! | one environment act | an edit | `run` (nascent until reviewed) | `after_edit` → `run` |
//! | 2–3 regenerate/verify acts | an edit, or a longer context | `sequence` | `after_edit` / `context` → `sequence` |
//! | one fixed act | two or three events | `run` | `context` → `run` |
//! | one observe or verify act before the first edit | a prompt-intent prototype | `run` | `intent` → `run` ([`intent`]) |
//!
//! Ceilings are unchanged (`adage_tracks::ceiling`): environment steps are nascent until a person
//! approves them, author and publish steps are never directed. Every compiled route gets a stable,
//! content-derived id: `<shape>-<hash>` ([`ids`]), or the fixed form its shape has always had
//! ([`stable`]: `lookup-work-item`, `read-linked-files`, `dead-end-…`, `finish-line-…`,
//! `intent-…`), so review decisions survive re-learning; and a [`adage_tracks::why::Why`] for
//! people.

pub mod glob;
pub mod ids;
pub mod intent;
pub mod shapes;
pub mod stable;

use crate::events;
use crate::input;
use crate::select::Reflex;
use adage_core::Effect;
use adage_tracks::why::Why;
use adage_tracks::{Route, Step};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Compiler parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompileParams {
    /// Fire needs the posterior's lower bound at least this (`learn.fire_lower_bound`).
    pub fire_lower_bound: f64,
    /// Prime needs the posterior mean at least this (`learn.prime_rate`).
    pub prime_rate: f64,
    /// A context trigger's gap, in alphabet events (the miner's G).
    pub within: u8,
    /// The glob lattice's precision bar.
    pub glob_precision: f64,
    /// An avoid is announced at session start when at least this share of traces run the
    /// failing runner; otherwise after its failure. 0 (the default) announces every avoid at
    /// session start, before the first wasted run, as dead ends always were: an after-failure
    /// avoid names one runner, and fires only once the run was wasted.
    pub announce_at_start: f64,
    /// A directed command must be the dominant variant of its item (this share of its matches).
    pub dominant: f64,
    /// Instinct priors: pseudo-traces and mean.
    pub prior_kappa: f64,
    pub prior_mean: f64,
    /// Compile `after_command` → `run` reflexes (a single successful act as the context). Off by
    /// default in G3: on the committed datasets such a reflex directs the agent's own next check
    /// (`make test` after `make gen`), which no convention asks for, so it is a false route; the
    /// same next steps are delivered as sequences after the edit that owes them.
    pub after_command_runs: bool,
    /// The outcome-lift gate ([`outcome_gate`], `learn.outcome_lift_lower_bound` and
    /// `learn.outcome_min_support`).
    pub outcome_lift_lower_bound: f64,
    pub outcome_min_support: u32,
    /// At most this many avoids (failure signatures) per working command
    /// (`learn.dead_end_signatures`).
    pub max_signatures: usize,
    /// An avoid names a runner only if it failed in at least this share of the traces that ran
    /// it (`learn.dead_end_consistency`): a runner that passes once the environment is fixed is
    /// not a dead end.
    pub consistency: f64,
    pub provenance: String,
}

impl Default for CompileParams {
    fn default() -> Self {
        CompileParams {
            fire_lower_bound: 0.8,
            prime_rate: 0.5,
            within: 8,
            glob_precision: 0.9,
            announce_at_start: 0.0,
            dominant: 0.8,
            prior_kappa: 3.0,
            prior_mean: 0.7,
            after_command_runs: false,
            outcome_lift_lower_bound: 0.2,
            outcome_min_support: 5,
            max_signatures: 3,
            consistency: 0.9,
            provenance: "desire-path".into(),
        }
    }
}

/// What the traces say about one command key (`exec <template>`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActStats {
    /// Literal commands (`shell_join` of the argv) with their argv and run counts.
    pub literals: BTreeMap<String, (Vec<String>, u32)>,
    /// Traces that ran it at all.
    pub sessions: u32,
    /// The highest effect class of its runs.
    pub effect: Option<Effect>,
    /// Failure signatures of its failed runs.
    pub signatures: BTreeMap<String, u32>,
    /// The output field that lists files, per successful run.
    pub fields: BTreeMap<String, u32>,
}

/// The traces, summarised for the compiler.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Corpus {
    pub sessions: u32,
    pub acts: BTreeMap<String, ActStats>,
    /// Traces that ran each runner (`stable::runner`), whatever the outcome.
    pub runners: BTreeMap<String, u32>,
    /// Traces in which a run of each runner passed.
    #[serde(default)]
    pub runners_passed: BTreeMap<String, u32>,
    /// The repository tree the glob lattice measures precision on: every path read or edited,
    /// and any repository file given.
    pub tree: BTreeSet<String>,
    /// Paths some trace read or edited.
    pub touched: BTreeSet<String>,
}

fn effect_of(s: &str) -> Option<Effect> {
    Some(match s {
        "observe" => Effect::Observe,
        "verify" => Effect::Verify,
        "regenerate" => Effect::Regenerate,
        "environment" => Effect::Environment,
        "author" => Effect::Author,
        "publish" => Effect::Publish,
        _ => return None,
    })
}

fn rank(e: Effect) -> u8 {
    match e {
        Effect::Observe => 0,
        Effect::Verify => 1,
        Effect::Regenerate => 2,
        Effect::Environment => 3,
        Effect::Author => 4,
        Effect::Publish => 5,
    }
}

/// The stronger of two effects.
pub fn max_effect(a: Effect, b: Effect) -> Effect {
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

impl Corpus {
    /// Summarise traces; `tree` adds repository files the lattice should see.
    pub fn of(sessions: &[input::Annotated], tree: &[String]) -> Corpus {
        let table = adage_core::CommandTable::load();
        let mut c = Corpus { sessions: sessions.len() as u32, ..Default::default() };
        let mut order: Vec<&input::Annotated> = sessions.iter().collect();
        order.sort_by(|a, b| a.session.id.cmp(&b.session.id));
        for a in order {
            let mut keys: BTreeSet<String> = BTreeSet::new();
            let mut runners: BTreeSet<String> = BTreeSet::new();
            let mut passed: BTreeSet<String> = BTreeSet::new();
            for (i, st) in a.session.steps.iter().enumerate().take(input::MAX_STEPS) {
                match st {
                    input::Step::Exec { argv, output, failed } => {
                        if argv.is_empty() {
                            continue;
                        }
                        let key = events::act_key(argv);
                        let e = c.acts.entry(key.clone()).or_default();
                        let lit = stable::shell_join(argv);
                        e.literals.entry(lit).or_insert_with(|| (argv.clone(), 0)).1 += 1;
                        let eff = a
                            .meta
                            .get(i)
                            .and_then(|m| effect_of(&m.effect))
                            .unwrap_or_else(|| events::classify(argv, &table));
                        e.effect = Some(e.effect.map_or(eff, |x| max_effect(x, eff)));
                        let out = adage_core::tokens::bounded(output, input::MAX_OUTPUT);
                        if *failed {
                            if let Some(sig) = stable::failure_signature(out) {
                                *e.signatures.entry(sig).or_default() += 1;
                            }
                        } else if let Some(f) = stable::listed_field(out) {
                            *e.fields.entry(f).or_default() += 1;
                        }
                        keys.insert(key);
                        runners.insert(stable::runner(argv));
                        if !*failed {
                            passed.insert(stable::runner(argv));
                        }
                    }
                    input::Step::Read { path } | input::Step::Edit { path } => {
                        if !path.is_empty() && !path.starts_with('/') {
                            c.tree.insert(path.clone());
                            c.touched.insert(path.clone());
                        }
                    }
                }
            }
            for k in keys {
                c.acts.get_mut(&k).expect("seen").sessions += 1;
            }
            for r in runners {
                *c.runners.entry(r).or_default() += 1;
            }
            for r in passed {
                *c.runners_passed.entry(r).or_default() += 1;
            }
        }
        c.tree.extend(tree.iter().filter(|p| !p.is_empty() && !p.starts_with('/')).cloned());
        c
    }

    /// The literal most traces ran for `key` (ties: the smallest), and its argv.
    pub fn literal(&self, key: &str) -> Option<(&str, &[String])> {
        let a = self.acts.get(key)?;
        a.literals
            .iter()
            .max_by(|x, y| x.1 .1.cmp(&y.1 .1).then_with(|| y.0.cmp(x.0)))
            .map(|(l, (argv, _))| (l.as_str(), argv.as_slice()))
    }

    /// The effect class of `key` as the traces ran it, never below what Adage assigns the literal.
    pub fn effect(&self, key: &str) -> Option<Effect> {
        let (lit, _) = self.literal(key)?;
        let assigned = adage_tracks::io::classify(lit, &adage_core::CommandTable::load());
        Some(self.acts.get(key).and_then(|a| a.effect).map_or(assigned, |e| max_effect(e, assigned)))
    }
}

/// An instinct route that is a prior for reflexes at its decision point directing its command
/// (Track G §6.3): `prior_kappa` pseudo-traces at `prior_mean`.
#[derive(Debug, Clone, PartialEq)]
pub struct Prior {
    pub route: Route,
}

/// Everything the compiler reads.
pub struct Input<'a> {
    pub reflexes: &'a [Reflex],
    pub corpus: &'a Corpus,
    /// G1's learned value slots (the lookup's typed slot), when learned.
    pub values: Option<&'a crate::Learned>,
    pub priors: &'a [Prior],
    pub params: &'a CompileParams,
}

/// One compiled route.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// The delivery shape (`run`, `obligation`, `avoid`, `read-batch`, `sequence`).
    pub shape: &'static str,
    pub route: Route,
    pub why: Why,
    /// Its id has its shape's fixed form ([`stable`]).
    pub fixed_id: bool,
}

/// A reflex that compiled to nothing, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dropped {
    pub pattern: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Output {
    pub routes: Vec<Compiled>,
    pub dropped: Vec<Dropped>,
}

/// At most this many compiled routes per repository (Track G §1.3).
pub const MAX_ROUTES: usize = 256;

/// Compile selected reflexes into routes (see the module documentation).
pub fn compile(input: &Input) -> Output {
    shapes::compile(input)
}

/// The compiler's second gate (Track G §6, outcome lift): a reflex that fails the rate gate,
/// counted on its own trigger and step, still compiles at prime (prime) when the traces that
/// took its step succeeded significantly more often than those where its trigger occurred and
/// they did not (`adage_tracks::lift`): at least `outcome_min_support` traces took it and the 95%
/// lower bound on the difference is at least `outcome_lift_lower_bound`. Never fire, never above
/// the effect's ceiling, never for a step that may not be directed at all.
pub fn outcome_gate(
    effect: Effect,
    by_edit: bool,
    lift: Option<&adage_tracks::lift::OutcomeLift>,
    p: &CompileParams,
) -> Option<adage_tracks::Strength> {
    let l = lift?;
    if !l.qualifies(p.outcome_min_support, p.outcome_lift_lower_bound) {
        return None;
    }
    adage_tracks::lift::strength(effect, by_edit)
}

/// The commands a step directs.
pub fn commands(s: &Step) -> Vec<&str> {
    match s {
        Step::Run { command } => vec![command],
        Step::Sequence { commands } => commands.iter().map(String::as_str).collect(),
        Step::Avoid { instead, .. } => vec![instead],
        _ => vec![],
    }
}

#[cfg(test)]
mod outcome_gate_tests {
    use super::*;
    use adage_tracks::lift::OutcomeLift;
    use adage_tracks::Strength;

    fn lift(with: (usize, usize), without: (usize, usize)) -> OutcomeLift {
        let v = |(p, f): (usize, usize)| {
            std::iter::repeat_n(true, p).chain(std::iter::repeat_n(false, f)).collect::<Vec<_>>()
        };
        OutcomeLift::of(&v(with), &v(without), "outcomes").unwrap()
    }

    #[test]
    fn a_significant_lift_compiles_at_prime_and_never_above_the_ceiling() {
        let p = CompileParams::default();
        let strong = lift((5, 0), (1, 9));
        assert_eq!(outcome_gate(Effect::Observe, false, Some(&strong), &p), Some(Strength::Prime));
        assert_eq!(outcome_gate(Effect::Regenerate, true, Some(&strong), &p), Some(Strength::Prime));
        // Never for a step that may not be directed.
        assert_eq!(outcome_gate(Effect::Author, false, Some(&strong), &p), None);
        // Too few takers, or no significant difference, or no outcomes at all.
        assert_eq!(outcome_gate(Effect::Observe, false, Some(&lift((3, 0), (0, 9))), &p), None);
        assert_eq!(outcome_gate(Effect::Observe, false, Some(&lift((4, 1), (3, 3))), &p), None);
        assert_eq!(outcome_gate(Effect::Observe, false, None, &p), None);
    }
}
