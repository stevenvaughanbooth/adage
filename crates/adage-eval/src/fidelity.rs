//! Replay fidelity: does replaying a transcript reproduce what the live hooks decided?
//!
//! The demo dataset keeps, for every eval session, the decisions its live hooks recorded in their
//! spools (`recorded/decisions.json`) and the index each round used (`recorded/index-<round>.json`).
//! Replaying those sessions with that index must give the same would-have directives at the same
//! events; otherwise the offline numbers describe a different system from the one that ran.

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
    pub index_generation: String,
    pub by_arm: BTreeMap<String, ArmFidelity>,
    pub mismatches: Vec<String>,
    pub event_mismatches: Vec<String>,
}

type Sig = Vec<(String, Vec<String>, bool)>;

fn decisions(seq: &[(String, Vec<String>, bool)]) -> Sig {
    seq.iter().filter(|(_, w, c)| !w.is_empty() || *c).cloned().collect()
}

/// Compare replay against the live record for every eval session of `round` (`round1`, `round2`).
pub fn run(dir: &Path, round: &str) -> Result<FidelityReport> {
    let ds = Dataset::load(dir)?;
    let n: u32 = round.strip_prefix("round").and_then(|x| x.parse().ok()).context("round must be round1 or round2")?;
    let idx_path = dir.join("recorded").join(format!("index-{round}.json"));
    let index: Index =
        serde_json::from_slice(&std::fs::read(&idx_path).with_context(|| idx_path.display().to_string())?)?;
    let rec_path = dir.join("recorded/decisions.json");
    let recorded: BTreeMap<String, Vec<Recorded>> = serde_json::from_slice(&std::fs::read(&rec_path)?)?;
    let table = CommandTable::load();
    let sandbox = crate::dataset::Sandbox::create(&ds.sandbox()?)?;
    let mut rep =
        FidelityReport { round: round.into(), index_generation: index.generation.clone(), ..Default::default() };
    for s in ds.manifest.sessions.iter().filter(|s| s.phase == "eval" && s.round == Some(n)) {
        let Some(live) = recorded.get(&s.run) else { continue };
        let t = transcript::read(&ds.path(s))?;
        let r = replay::replay(&t, &index, &ds.manifest.repo, &sandbox.root, &table, s.stopped());
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

pub fn render_text(r: &FidelityReport) -> String {
    let mut s = format!("replay fidelity, {} (index {})\n", r.round, r.index_generation);
    for (arm, a) in &r.by_arm {
        s.push_str(&format!(
            "  {arm:<8} {} sessions: decisions match {}/{}, hook event sequence matches {}/{}\n",
            a.sessions, a.decisions_match, a.sessions, a.events_match, a.sessions
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
