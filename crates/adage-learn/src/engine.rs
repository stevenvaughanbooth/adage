//! The graph engine's routes inside `adage learn` (Track G G3): reflexes mined from the traces,
//! compiled to delivery shapes, judged on the traces, and grouped into track directories.
//!
//! 1. **Mine** (G2): every trace no directive steered becomes an event sequence
//!    ([`crate::bridge::events`]); `adage_graph::select::learn` mines, scores and selects reflexes.
//! 2. **Compile** (G3, `adage_graph::compile`): each reflex becomes zero or more routes by its
//!    structure, with a content-derived id, or a miner's id for a shape a miner emits today
//!    (so `review.yaml` decisions survive), and a `why` block for people.
//! 3. **Judge.** A compiled route's evidence is counted on the traces exactly as its own trigger
//!    and step read them, the way the miners count a route of that shape: the lookup through G1's
//!    value types ([`crate::graph::lookup_routes`]), linked files through the read-batch counter,
//!    an after-edit route over the traces that edited a file of its glob (run after the last such
//!    edit, contrast against the traces that did not), an after-command route over the traces
//!    that ran the command. Its aspect is then the miners' rule over those counts (Wilson lower
//!    bound for green, rate for yellow, capped by the ceiling): the parity switch of Track G §5,
//!    so an engine change does not move an aspect by itself. Avoids (red) and context routes keep
//!    the reflex's own evidence. Traces steered by a route of the same shape
//!    (`adage_tracks::route_kind`) are left out of its count.
//! 4. **Group** by `adage_graph::compile::legacy_track`: the legacy track of the route's shape
//!    (`work-items`, `finish-line`, `dead-ends`), else `graph`. Intent families still come from
//!    `crate::intent` (a prompt prototype needs the pinned embedding model; G5 moves it).
//!
//! **Instincts as priors.** The repository's installed instinct routes are pseudo-evidence on the
//! reflex at the same decision point directing the same command: 3 traces at 0.7 in the yellow
//! test only; green reads the organic lower bound alone, so no reflex is delivered green from
//! prior mass, and a reflex that says otherwise always wins (interlocking puts instincts last).

use crate::bridge;
use crate::import::Session;
use crate::mine::{aspect_for, days, models, shell_join, LearnConfig};
use adage_core::Effect;
use adage_graph::compile::{self, CompileParams, Corpus, Dropped, Prior};
use adage_graph::select::{self, Counts};
use adage_tracks::why::Why;
use adage_tracks::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// What the graph engine did for one repository.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EngineReport {
    /// Traces mined.
    pub sessions: usize,
    pub reflexes: usize,
    pub compiled: usize,
    /// Routes that passed judgement.
    pub routes: usize,
    /// Routes compiled with a context trigger (matched at hook time since G4).
    pub contexts: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<Dropped>,
}

/// One judged route, where it goes, and why it exists.
#[derive(Debug, Clone)]
pub struct Judged {
    pub track: &'static str,
    pub route: Route,
    pub why: Why,
}

/// The result of a graph learn for one repository.
#[derive(Debug, Clone, Default)]
pub struct EngineRun {
    pub routes: Vec<Judged>,
    pub report: EngineReport,
    /// Every scored candidate's counts by pattern (for stale routes).
    pub counts: BTreeMap<String, Counts>,
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8)
}

fn glob_hit(glob: &str, rel: &str) -> bool {
    adage_graph::select::glob_match(glob, rel) || glob::Pattern::new(glob).is_ok_and(|g| g.matches(rel))
}

/// The repository-relative path an edit step touched.
fn edited(st: &crate::import::StepObs) -> Option<&str> {
    adage_core::keys::is_author(&st.norm.action).then_some(st.rel_path.as_deref()).flatten()
}

fn ok_literal(st: &crate::import::StepObs) -> Option<String> {
    if st.call.failed {
        return None;
    }
    crate::mine::argv(st).map(|av| shell_join(av))
}

/// Whether `steps` ran what `step` directs after position `from` (a sequence in order), with
/// `{part}` bound to `part`.
fn ran_after(steps: &[crate::import::StepObs], from: usize, step: &Step, part: Option<&str>) -> bool {
    let bind = |c: &str| part.map_or(c.to_string(), |p| c.replace("{part}", p));
    let want: Vec<String> = compile::commands(step).into_iter().map(bind).collect();
    let mut i = 0;
    for st in steps.iter().skip(from + 1) {
        if i < want.len() && ok_literal(st).as_deref() == Some(want[i].as_str()) {
            i += 1;
        }
    }
    !want.is_empty() && i == want.len()
}

struct Count {
    opportunities: u32,
    /// The traces where the trigger occurred (for the outcome lift).
    open: BTreeSet<String>,
    support: BTreeSet<String>,
    rate_without: f64,
    deviations_failed: u32,
}

/// An after-edit route over the traces (see the module documentation).
fn count_after_edit(r: &Route, sessions: &[&Session]) -> Option<Count> {
    let Trigger::AfterEdit { paths } = &r.trigger else { return None };
    let depth = r.slots.iter().find_map(|s| match &s.source {
        SlotSource::EditSegment { depth, .. } => Some(*depth as usize),
        _ => None,
    });
    let first = compile::commands(&r.step).first().map(|c| c.to_string())?;
    let (mut opp, mut support, mut ran_without, mut not_editing, mut failed) = (0, BTreeSet::new(), 0u32, 0u32, 0u32);
    let mut open = BTreeSet::new();
    for s in sessions {
        let last = s
            .steps
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(i, st)| edited(st).map(|p| (i, p)))
            .find(|(_, p)| glob_hit(paths, p));
        match last {
            Some((i, p)) => {
                opp += 1;
                open.insert(s.id.clone());
                let part = depth.and_then(|d| p.split('/').filter(|x| !x.is_empty()).nth(d));
                if depth.is_some() && part.is_none() {
                    continue;
                }
                if ran_after(&s.steps, i, &r.step, part) {
                    support.insert(s.id.clone());
                } else if s.outcome == Some(false) {
                    failed += 1;
                }
            }
            None => {
                not_editing += 1;
                let head = first.split('{').next().unwrap_or_default().to_string();
                let ran = s.steps.iter().filter_map(ok_literal).any(|l| {
                    if depth.is_some() {
                        l.starts_with(&head)
                    } else {
                        l == first
                    }
                });
                ran_without += u32::from(ran);
            }
        }
    }
    let rate_without = if not_editing == 0 { 0.0 } else { ran_without as f64 / not_editing as f64 };
    Some(Count { opportunities: opp, open, support, rate_without, deviations_failed: failed })
}

/// An after-command run over the traces: those that ran the command, and those that then ran the
/// directed one within the next eight steps.
fn count_after_command(r: &Route, sessions: &[&Session]) -> Option<Count> {
    let Trigger::AfterCommand { command: key } = &r.trigger else { return None };
    let (mut opp, mut support, mut open) = (0, BTreeSet::new(), BTreeSet::new());
    for s in sessions {
        let Some(i) = s.steps.iter().position(|st| &st.key == key && !st.call.failed) else { continue };
        opp += 1;
        open.insert(s.id.clone());
        if ran_after(&s.steps[..(i + 9).min(s.steps.len())], i, &r.step, None) {
            support.insert(s.id.clone());
        }
    }
    Some(Count { opportunities: opp, open, support, rate_without: 0.0, deviations_failed: 0 })
}

/// An avoid over the traces: those that ran an avoided runner (the opportunities), and those
/// where a run of it failed and the working command then succeeded within five steps (support).
fn count_avoid(runners: &[&str], instead: &str, sessions: &[&Session]) -> Count {
    let (mut opp, mut support) = (0, BTreeSet::new());
    for s in sessions {
        let runs: Vec<usize> = s
            .steps
            .iter()
            .enumerate()
            .filter(|(_, st)| {
                crate::mine::argv(st).is_some_and(|av| runners.contains(&crate::mine::runner(av).as_str()))
            })
            .map(|(i, _)| i)
            .collect();
        if runs.is_empty() {
            continue;
        }
        opp += 1;
        // The recovery is the first check that then passed (within five steps), as the dead-end
        // miner pairs them: a later check that also passed is not what replaced the runner.
        let recovered = runs.iter().any(|&i| {
            let failed = &s.steps[i];
            failed.call.failed
                && s.steps
                    .iter()
                    .skip(i + 1)
                    .take(5)
                    .find(|b| {
                        crate::mine::argv(b).is_some()
                            && !b.call.failed
                            && b.norm.effect == Effect::Verify
                            && b.key != failed.key
                    })
                    .and_then(ok_literal)
                    .as_deref()
                    == Some(instead)
        });
        if recovered {
            support.insert(s.id.clone());
        }
    }
    Count { opportunities: opp, open: BTreeSet::new(), support, rate_without: 0.0, deviations_failed: 0 }
}

/// Judge one compiled route (see the module documentation); `None` drops it.
fn judge(mut r: Route, ss: &[&Session], cfg: &LearnConfig, lookup: &[Route], prior: bool) -> Result<Route, String> {
    let kind = route_kind(&r);
    let seen: Vec<&Session> = ss.iter().copied().filter(|s| !s.steered_by(kind)).collect();
    match (&r.trigger, &r.step) {
        (Trigger::Prompt { .. }, Step::Run { command }) => {
            let id = |c: &str| adage_graph::select::command_identity(c);
            return lookup
                .iter()
                .find(|l| matches!(&l.step, Step::Run { command: c } if id(c) == id(command)))
                .cloned()
                .ok_or_else(|| "the value-type lookup does not judge it".to_string());
        }
        (Trigger::AfterCommand { command }, Step::ReadBatch { .. }) => {
            return crate::mine::read_batch_routes(&seen, command, cfg)
                .into_iter()
                .next()
                .map(|mut x| {
                    x.evidence.origin = r.evidence.origin.clone();
                    x
                })
                .ok_or_else(|| "too few traces read the files it listed together".to_string());
        }
        (Trigger::SessionStart, Step::Avoid { avoid, instead, .. }) => {
            let runners: Vec<&str> = avoid.split(" / ").collect();
            let c = count_avoid(&runners, instead, &seen);
            let n = c.support.len() as u32;
            if n < cfg.min_support {
                return Err(format!("{n} unsteered traces recovered, fewer than {}", cfg.min_support));
            }
            let opp = c.opportunities.max(n);
            r.evidence = Evidence {
                opportunities: opp,
                support: n,
                days: days(&seen, &c.support),
                rate: n as f64 / opp as f64,
                lower_bound: wilson_lower(n, opp),
                turns_saved: 1.0,
                provenance: cfg.provenance.clone(),
                models: models(&seen),
                origin: r.evidence.origin.clone(),
                ..Default::default()
            };
            return Ok(r);
        }
        (Trigger::AfterFailure { command: key, .. }, Step::Run { command }) => {
            let runner = crate::mine::runner(
                &key.trim_start_matches("exec ").split(' ').map(str::to_string).collect::<Vec<_>>(),
            );
            let c = count_avoid(&[runner.as_str()], command, &seen);
            let n = c.support.len() as u32;
            if n < cfg.min_support {
                return Err(format!("{n} unsteered traces recovered, fewer than {}", cfg.min_support));
            }
            let opp = c.opportunities.max(n);
            let (rate, lb) = (n as f64 / opp as f64, wilson_lower(n, opp));
            r.aspect = aspect_for(Effect::Verify, false, lb, rate, cfg, &r.id)
                .ok_or_else(|| format!("rate {rate:.2} below the bar"))?;
            r.evidence = Evidence {
                opportunities: opp,
                support: n,
                days: days(&seen, &c.support),
                rate,
                lower_bound: lb,
                turns_saved: 1.0,
                provenance: cfg.provenance.clone(),
                models: models(&seen),
                origin: r.evidence.origin.clone(),
                ..Default::default()
            };
            return Ok(r);
        }
        (Trigger::Context { .. }, _) => {
            // Nothing counts a context again on the traces (G4 matches it at hook time only): it
            // keeps the reflex's own evidence.
            r.evidence.models = models(&seen);
            return Ok(r);
        }
        _ => {}
    }
    let by_edit = matches!(r.trigger, Trigger::AfterEdit { .. });
    let c = match &r.trigger {
        Trigger::AfterEdit { .. } => count_after_edit(&r, &seen),
        Trigger::AfterCommand { .. } => count_after_command(&r, &seen),
        _ => None,
    }
    .ok_or_else(|| "no counter for this shape".to_string())?;
    let n = c.support.len() as u32;
    let rate = n as f64 / c.opportunities.max(1) as f64;
    // The outcome lift over the traces where the trigger occurred: the compiler's second gate
    // (`adage_graph::compile::outcome_gate`), for a reflex too few traces show for the rate bar.
    let lift = crate::mine::outcome_lift(&seen, &c.open, &c.support);
    if n < cfg.min_support && outcome_gate(r.effect, by_edit, lift.as_ref(), cfg).is_none() {
        return Err(format!("{n} supporting traces, fewer than {}", cfg.min_support));
    }
    if by_edit && rate - c.rate_without < cfg.finish_line_contrast {
        return Err(format!("contrast {:.2} below {}", rate - c.rate_without, cfg.finish_line_contrast));
    }
    let lb = wilson_lower(n, c.opportunities);
    // An instinct's prior enters the yellow test only (3 traces at 0.7).
    let yellow_rate = if prior { (n as f64 + 3.0 * 0.7) / (c.opportunities as f64 + 3.0) } else { rate };
    // A sequence is capped by its lowest ceiling.
    let effect = match &r.step {
        Step::Sequence { commands } => commands
            .iter()
            .map(|x| io::classify(x, &adage_core::CommandTable::load()))
            .fold(Effect::Observe, |a, e| if ceiling(e, by_edit) == Some(Aspect::Green) { a } else { e }),
        _ => r.effect,
    };
    let aspect =
        (if n >= cfg.min_support { aspect_for(effect, by_edit, lb, yellow_rate.max(rate), cfg, &r.id) } else { None })
            .or_else(|| outcome_gate(effect, by_edit, lift.as_ref(), cfg))
            .ok_or_else(|| format!("rate {rate:.2} below the bar and no outcome lift, or may never be directed"))?;
    r.aspect = aspect;
    r.evidence = Evidence {
        opportunities: c.opportunities,
        support: n,
        days: days(&seen, &c.support),
        deviations_failed: c.deviations_failed,
        rate,
        lower_bound: lb,
        provenance: cfg.provenance.clone(),
        models: models(&seen),
        origin: r.evidence.origin.clone(),
        outcome_lift: lift,
        ..Default::default()
    };
    Ok(r)
}

/// The compiler's outcome-lift gate with the learn thresholds.
fn outcome_gate(
    effect: Effect,
    by_edit: bool,
    lift: Option<&adage_tracks::lift::OutcomeLift>,
    cfg: &LearnConfig,
) -> Option<Aspect> {
    let p = CompileParams {
        outcome_lift_lower_bound: cfg.outcome_lift_lower_bound,
        outcome_min_support: cfg.outcome_min_support,
        ..Default::default()
    };
    compile::outcome_gate(effect, by_edit, lift, &p)
}

/// Learn, compile and judge the graph engine's routes for one repository's traces.
pub fn run(ss: &[&Session], cfg: &LearnConfig, priors: &[Route]) -> EngineRun {
    // Every trace is mined: patterns are candidates only. Evidence for what is delivered is
    // counted again below on the traces no directive of the route's shape steered.
    let mined: Vec<&Session> = ss.to_vec();
    let annotated: Vec<adage_graph::input::Annotated> = mined.iter().map(|s| bridge::annotate(s)).collect();
    let events: Vec<adage_graph::events::EventSession> =
        mined.iter().zip(&annotated).map(|(s, a)| bridge::events(a, s)).collect();
    let mut p = select::Params::default().with_threads(threads());
    p.mine.min_support = cfg.min_support;
    p.score.min_support = cfg.min_support;
    let learned = select::learn(&events, &p);
    // G1's value slots, learned in the discovery window as the lookup is judged there.
    let prefix: Vec<adage_graph::input::Session> =
        mined.iter().map(|s| bridge::annotate_prefix(s, s.discovery().len()).session).collect();
    let values = adage_graph::learn(&prefix, &adage_graph::Params { threads: threads(), ..Default::default() });
    // The glob lattice's tree is what the traces touched: a pure function of the traces, whatever
    // is on this machine (see `adage_graph::compile::glob`).
    let corpus = Corpus::of(&annotated, &[]);
    let params = CompileParams {
        green_lower_bound: cfg.green_lower_bound,
        yellow_rate: cfg.yellow_rate,
        outcome_lift_lower_bound: cfg.outcome_lift_lower_bound,
        outcome_min_support: cfg.outcome_min_support,
        provenance: cfg.provenance.clone(),
        ..Default::default()
    };
    let priors: Vec<Prior> = priors.iter().map(|r| Prior { route: r.clone() }).collect();
    // The selected reflexes first, then every candidate that passed its gates (their cores are
    // deduplicated by route id, so a selected reflex explains a route whenever one does).
    let pool: Vec<select::Reflex> = learned.reflexes.iter().chain(learned.unselected.iter()).cloned().collect();
    let out = compile::compile(&compile::Input {
        reflexes: &pool,
        corpus: &corpus,
        values: Some(&values),
        priors: &priors,
        params: &params,
    });
    let ws: Vec<&Session> = ss.iter().copied().filter(|s| !s.steered_by(KIND_WORK_ITEMS)).collect();
    // The lookup is a value-flow reflex (Track G §8, `[prompt] =flow=> [act …]`): G1's value slot
    // test over every trace finds it, and judges it.
    let lookup = crate::graph::lookup_routes(&ws, cfg);
    let mut report = EngineReport {
        sessions: mined.len(),
        reflexes: learned.reflexes.len(),
        compiled: out.routes.len(),
        dropped: out.dropped.clone(),
        ..Default::default()
    };
    let mut routes = Vec::new();
    for c in out.routes {
        let prior = c.why.prior.is_some();
        match judge(c.route.clone(), ss, cfg, &lookup, prior) {
            Ok(r) => {
                report.contexts += usize::from(matches!(r.trigger, Trigger::Context { .. }));
                routes.push(Judged { track: compile::legacy_track(&r), route: r, why: c.why });
            }
            Err(reason) => report
                .dropped
                .push(Dropped { pattern: c.why.pattern.clone(), reason: format!("{}: {reason}", c.route.id) }),
        }
    }
    // A value-flow reflex MDL did not select on its own (a longer pattern holding it was chosen,
    // or none): its route is the value slot's, with the slot's own evidence.
    if !routes.iter().any(|j| j.route.id == compile::legacy::LOOKUP_ID) {
        if let Some(l) = lookup.first() {
            let Step::Run { command } = &l.step else { unreachable!("a lookup runs a command") };
            let why = Why {
                shape: "run".into(),
                pattern: format!("[prompt] =flow=> [act {command}]"),
                backoff: 1,
                gain: 0.0,
                lift: 0.0,
                conf: (l.evidence.rate * 1e4).round() / 1e4,
                conf_lb: (l.evidence.lower_bound * 1e4).round() / 1e4,
                support: l.evidence.support,
                days: l.evidence.days,
                members: vec![],
                examples: vec![],
                prior: None,
            };
            routes.push(Judged { track: KIND_WORK_ITEMS, route: l.clone(), why });
        }
    }
    report.routes = routes.len();
    EngineRun { routes, report, counts: learned.counts }
}

/// The description of a graph-engine track directory.
pub fn description(suffix: &str) -> &'static str {
    match suffix {
        KIND_WORK_ITEMS => "Context gathering for prompts that name a work item",
        KIND_FINISH_LINE => "Steps this repository requires after certain edits",
        KIND_DEAD_ENDS => "Commands that fail in this repository, and what works instead",
        _ => "Reflexes the graph engine learned whose shapes no miner produces",
    }
}

/// For a graph route that was in service and was not selected again: `Some(exposed)` to keep it
/// stale, from the scorer's counts for its own pattern (its `why`), `None` to drop it.
pub fn verdict(why: Option<&Why>, counts: &BTreeMap<String, Counts>, cfg: &LearnConfig) -> Option<u32> {
    let c = counts.get(&why?.pattern)?;
    crate::stale::verdict_counts(c.exposed, c.opportunities, c.support, cfg)
}
