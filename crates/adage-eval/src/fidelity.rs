//! Replay fidelity: does replaying a transcript reproduce what the live hooks decided?
//!
//! The demo dataset keeps, for every eval session, the decisions its live hooks recorded in their
//! spools (`recorded/decisions.json`) and the index each round used (`recorded/index-<round>.json`).
//! Replaying those sessions with that index must give the same would-have directives at the same
//! events; otherwise the offline numbers describe a different system from the one that ran.
//!
//! [`run_learned`] asks the second question: replayed with the index *today's learner* learns from
//! the round's train sessions (the recorded index came from the removed miners), does every
//! directive the live hook gave still arrive, at that event or earlier? The learner may add
//! directives (a superset), and may deliver one earlier: a dead end is now announced at session
//! start, not at the first prompt. Each recorded directive that arrives late or never is listed.
//! A recorded route whose id predates content-derived ids (round 1's `finish-line-1`) is matched
//! by its trigger and step to the learned route with the same ones.

use crate::dataset::Dataset;
use crate::{replay, transcript};
use adage_core::CommandTable;
use adage_tracks::Index;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Recorded {
    event: String,
    #[serde(default)]
    would_have: Vec<String>,
    #[serde(default)]
    would_continue: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ArmFidelity {
    pub sessions: u32,
    /// Same hook events, in the same order: every event (hooks from before the no-match fast
    /// path), or every event the fast path does not skip.
    pub events_match: u32,
    /// Same would-have directives (and stop requests) at the same events.
    pub decisions_match: u32,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct FidelityReport {
    pub round: String,
    /// `current`, or `exact-keys`: the matching the recorded hooks used ([`run_with`]).
    pub matching: String,
    pub index_generation: String,
    pub by_arm: BTreeMap<String, ArmFidelity>,
    pub mismatches: Vec<String>,
    pub event_mismatches: Vec<String>,
}

type Sig = Vec<(String, Vec<String>, bool)>;

fn decisions(seq: &[(String, Vec<String>, bool)]) -> Sig {
    seq.iter().filter(|(_, w, c)| !w.is_empty() || *c).cloned().collect()
}

/// Compare replay against the live record for every eval session of `round` (`round1`, `round2`),
/// deciding with the current matching.
pub fn run(dir: &Path, round: &str) -> Result<FidelityReport> {
    run_with(dir, round, false)
}

/// As [`run`]; with `exact_keys`, decide with the matching the recorded hooks used, before the
/// command-pattern algebra (`adage_runtime::with_exact_key_matching`). The demo's round-2 hooks
/// recorded that a linked-files trigger mined from `python3 scripts/wi-lookup {id}` did not fire
/// when an agent ran `./scripts/wi-lookup W-2042`; the current matching fires it (the same
/// script), so four control sessions replay one more would-have directive than was recorded.
pub fn run_with(dir: &Path, round: &str, exact_keys: bool) -> Result<FidelityReport> {
    let ds = Dataset::load(dir)?;
    let n: u32 = round.strip_prefix("round").and_then(|x| x.parse().ok()).context("round must be round1 or round2")?;
    let idx_path = dir.join("recorded").join(format!("index-{round}.json"));
    let index: Index =
        serde_json::from_slice(&std::fs::read(&idx_path).with_context(|| idx_path.display().to_string())?)?;
    let rec_path = dir.join("recorded/decisions.json");
    let recorded: BTreeMap<String, Vec<Recorded>> = serde_json::from_slice(&std::fs::read(&rec_path)?)?;
    let table = CommandTable::load();
    let sandbox = crate::dataset::Sandbox::create(&ds.sandbox()?)?;
    let mut rep = FidelityReport {
        round: round.into(),
        matching: if exact_keys { "exact-keys" } else { "current" }.into(),
        index_generation: index.generation.clone(),
        ..Default::default()
    };
    for s in ds.manifest.sessions.iter().filter(|s| s.phase == "eval" && s.round == Some(n)) {
        let Some(live) = recorded.get(&s.run) else { continue };
        let t = transcript::read(&ds.path(s))?;
        let go =
            || replay::replay(&t, &index, &ds.manifest.repo, &sandbox.root, &table, s.stopped(), Default::default());
        let r = if exact_keys { adage_runtime::with_exact_key_matching(go) } else { go() };
        let replayed: Sig = r.trace.iter().map(|(e, w, c)| (e.to_string(), w.clone(), *c)).collect();
        let live: Sig = live.iter().map(|x| (x.event.clone(), x.would_have.clone(), x.would_continue)).collect();
        let a = rep.by_arm.entry(s.arm.clone()).or_default();
        a.sessions += 1;
        let names = |v: &Sig| v.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
        let (rn, ln) = (names(&replayed), names(&live));
        // Hooks with the no-match fast path record no event it skips.
        let recorded: Vec<String> =
            rn.iter().zip(&r.skipped).filter(|(_, skipped)| !**skipped).map(|(n, _)| n.clone()).collect();
        if rn == ln || recorded == ln {
            a.events_match += 1;
        } else {
            let i = rn.iter().zip(&ln).take_while(|(x, y)| x == y).count();
            rep.event_mismatches.push(format!(
                "{}: event {i} replay {:?} vs live {:?} ({} vs {} events)",
                s.run,
                rn.get(i),
                ln.get(i),
                rn.len(),
                ln.len()
            ));
        }
        if decisions(&replayed) == decisions(&live) {
            a.decisions_match += 1;
        } else {
            rep.mismatches.push(format!("{}: replay {:?} vs live {:?}", s.run, decisions(&replayed), decisions(&live)));
        }
    }
    Ok(rep)
}

/// Replay against the index today's learner learns from `round`'s train sessions (see the module
/// documentation), with the recorded hooks' matching when `exact_keys`.
#[derive(Debug, Default, Clone, Serialize)]
pub struct LearnedFidelity {
    pub round: String,
    pub index_generation: String,
    pub sessions: u32,
    /// Sessions where every recorded directive arrived at its event or earlier.
    pub superset: u32,
    /// Sessions whose decisions are identical to the recorded ones.
    pub identical: u32,
    /// `<run>: event <i> (<name>) missing <route ids>`, for every recorded directive not given at
    /// that event or before it.
    pub missing: Vec<String>,
    /// `<run>: event <i> (<name>) gives <route ids> earlier`: directives the live hook gave at a
    /// later event.
    pub earlier: Vec<String>,
    /// `<run>: event <i> (<name>) adds <route ids>`: directives the learned index adds.
    pub added: Vec<String>,
}

/// See [`LearnedFidelity`].
pub fn run_learned(dir: &Path, round: &str, exact_keys: bool) -> Result<LearnedFidelity> {
    let ds = Dataset::load(dir)?;
    let n: u32 = round.strip_prefix("round").and_then(|x| x.parse().ok()).context("round must be round1 or round2")?;
    let prepared = crate::prepare(dir, &crate::Options { split: Some(round.to_string()), ..Default::default() })?;
    let index = &prepared.index;
    let rec_path = dir.join("recorded/decisions.json");
    let recorded: BTreeMap<String, Vec<Recorded>> = serde_json::from_slice(&std::fs::read(&rec_path)?)?;
    // Recorded id → the learned id of the route with the same trigger and step.
    let live_index: Index = serde_json::from_slice(&std::fs::read(dir.join(format!("recorded/index-{round}.json")))?)?;
    let alias: BTreeMap<String, String> = live_index
        .routes
        .iter()
        .filter_map(|l| {
            index
                .routes
                .iter()
                .find(|x| x.route.trigger == l.route.trigger && x.route.step == l.route.step)
                .filter(|x| x.route.id != l.route.id)
                .map(|x| (l.route.id.clone(), x.route.id.clone()))
        })
        .collect();
    let recorded: BTreeMap<String, Vec<Recorded>> = recorded
        .into_iter()
        .map(|(run, events)| {
            let events = events
                .into_iter()
                .map(|mut e| {
                    for id in &mut e.would_have {
                        if let Some(a) = alias.get(id) {
                            *id = a.clone();
                        }
                    }
                    e
                })
                .collect();
            (run, events)
        })
        .collect();
    // A finish line a sequence carries (the sequence begins with its command, after the same
    // edits) is delivered as that sequence: one edit, one directive (`adage_runtime::decide`).
    let mut carried_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in &index.routes {
        let (adage_tracks::Trigger::AfterEdit { .. }, adage_tracks::Step::Sequence { commands }) =
            (&s.route.trigger, &s.route.step)
        else {
            continue;
        };
        for o in index.routes.iter().filter(|o| o.repo == s.repo && !o.route.is_instinct()) {
            if let (adage_tracks::Trigger::AfterEdit { .. }, adage_tracks::Step::Run { command }) =
                (&o.route.trigger, &o.route.step)
            {
                let same = commands.first().is_some_and(|f| adage_core::pattern::same_command(f, command));
                if same && adage_tracks::interlock::same_point(&s.route.trigger, &o.route.trigger) {
                    carried_by.entry(o.route.id.clone()).or_default().push(s.route.id.clone());
                }
            }
        }
    }
    let table = CommandTable::load();
    let sandbox = crate::dataset::Sandbox::create(&ds.sandbox()?)?;
    let mut rep =
        LearnedFidelity { round: round.into(), index_generation: index.generation.clone(), ..Default::default() };
    for s in ds.manifest.sessions.iter().filter(|s| s.phase == "eval" && s.round == Some(n)) {
        let Some(live) = recorded.get(&s.run) else { continue };
        let t = transcript::read(&ds.path(s))?;
        let go =
            || replay::replay(&t, index, &ds.manifest.repo, &sandbox.root, &table, s.stopped(), Default::default());
        let r = if exact_keys { adage_runtime::with_exact_key_matching(go) } else { go() };
        rep.sessions += 1;
        // Events the live hooks recorded, in order; the replay's events the fast path would record.
        let replayed: Vec<(String, Vec<String>)> = r
            .trace
            .iter()
            .zip(&r.skipped)
            .filter(|(_, skipped)| !**skipped)
            .map(|((e, w, _), _)| (e.to_string(), w.clone()))
            .collect();
        let all: Vec<(String, Vec<String>)> = r.trace.iter().map(|(e, w, _)| (e.to_string(), w.clone())).collect();
        let events = if all.len() == live.len() { &all } else { &replayed };
        let (mut ok, mut same) = (true, true);
        for (i, l) in live.iter().enumerate() {
            let got: Vec<String> = events.get(i).map(|x| x.1.clone()).unwrap_or_default();
            let given_by = |id: &String| {
                let carriers = carried_by.get(id).map(Vec::as_slice).unwrap_or_default();
                events.iter().take(i + 1).any(|e| e.1.contains(id) || carriers.iter().any(|c| e.1.contains(c)))
            };
            let missing: Vec<&String> = l.would_have.iter().filter(|x| !given_by(x)).collect();
            let later = |id: &String| live.iter().skip(i + 1).any(|e| e.would_have.contains(id));
            let (early, added): (Vec<&String>, Vec<&String>) =
                got.iter().filter(|x| !l.would_have.contains(x)).partition(|x| later(x));
            if !missing.is_empty() {
                ok = false;
                rep.missing.push(format!("{}: event {i} ({}) missing {missing:?}", s.run, l.event));
            }
            if !early.is_empty() {
                same = false;
                rep.earlier.push(format!("{}: event {i} ({}) gives {early:?} earlier", s.run, l.event));
            }
            if !added.is_empty() {
                same = false;
                rep.added.push(format!("{}: event {i} ({}) adds {added:?}", s.run, l.event));
            }
        }
        rep.superset += u32::from(ok);
        rep.identical += u32::from(ok && same);
    }
    Ok(rep)
}

pub fn render_text(r: &FidelityReport) -> String {
    let mut s = format!("replay fidelity, {} (index {}, {} matching)\n", r.round, r.index_generation, r.matching);
    for (arm, a) in &r.by_arm {
        s.push_str(&format!(
            "  {:<8} {} traces: decisions match {}/{}, hook event sequence matches {}/{}\n",
            if arm == "holdout" { "control" } else { arm.as_str() },
            a.sessions,
            a.decisions_match,
            a.sessions,
            a.events_match,
            a.sessions
        ));
    }
    for m in &r.mismatches {
        s.push_str(&format!("  decisions differ: {m}\n"));
    }
    for m in &r.event_mismatches {
        s.push_str(&format!("  events differ: {m}\n"));
    }
    s
}

pub fn render_learned(r: &LearnedFidelity) -> String {
    let mut s = format!(
        "replay fidelity against the learned index, {} (index {}): {}/{} sessions get every recorded directive in time, {} identically\n",
        r.round, r.index_generation, r.superset, r.sessions, r.identical
    );
    for m in &r.missing {
        s.push_str(&format!("  missing: {m}\n"));
    }
    for m in &r.earlier {
        s.push_str(&format!("  earlier: {m}\n"));
    }
    for m in &r.added {
        s.push_str(&format!("  added: {m}\n"));
    }
    s
}
