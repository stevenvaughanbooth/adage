//! The learner's drop ledger (evals/launch/learner-diagnosis.md): every candidate the miner scored
//! for one repository's traces, and the gate that stopped it (the scorer's gate, the compiler's
//! reason, or the judgement's), with the routes that survived.
//!
//! ```text
//! cargo run --release -p adage-learn --example ledger -- [--demo] [--repo <key>] [--root <dir>]
//!     [--priors <json>] <transcript>... > ledger.json
//! ```
//!
//! Read-only: nothing is written but the JSON on stdout. Thresholds are the `default` preset
//! unless `--demo`. Instincts are not bound (reflexes alone).

use adage_core::CommandTable;
use adage_learn::{bridge, engine, LearnConfig};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|a| a == "--demo");
    let value = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let repo = value("--repo");
    let takes = ["--repo", "--root", "--priors"];
    let files: Vec<PathBuf> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && takes.contains(&args[i - 1].as_str())))
        .map(|(_, a)| PathBuf::from(a))
        .collect();
    let table = CommandTable::load();
    let mut sessions: Vec<adage_learn::import::Session> = files
        .iter()
        .filter_map(|p| adage_learn::read_session(p, &table, repo.as_deref()))
        .filter(|s| !s.steps.is_empty())
        .collect();
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    let ss: Vec<&adage_learn::import::Session> = sessions.iter().collect();
    let mut cfg = if demo { LearnConfig::demo() } else { LearnConfig::default() };
    if let (Some(root), Some(repo)) = (value("--root"), &repo) {
        cfg.roots.insert(repo.clone(), PathBuf::from(root));
    }
    if let Some(p) = value("--priors") {
        cfg.priors = serde_json::from_str(&std::fs::read_to_string(p).expect("priors file")).expect("priors json");
    }

    // The scorer's view: every closed pattern it scored, with its gate.
    let annotated: Vec<adage_graph::input::Annotated> = ss.iter().map(|s| bridge::annotate(s)).collect();
    let events: Vec<adage_graph::events::EventSession> =
        ss.iter().zip(&annotated).map(|(s, a)| bridge::events(a, s)).collect();
    let mut p = adage_graph::select::Params::default().with_threads(4);
    p.mine.min_support = cfg.min_support;
    p.score.min_support = cfg.min_support;
    let (num, den) = cfg.dead_end_consistency;
    p.score.consistency = num as f64 / den.max(1) as f64;
    let scored = adage_graph::select::candidates_shaped(&events, &p);

    // The engine's view: what compiled, what the compiler and the judgement dropped, what stayed.
    let run = engine::run(&ss, &cfg, &[], None);
    let mut dropped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for d in &run.report.dropped {
        dropped.entry(d.pattern.clone()).or_default().push(d.reason.clone());
    }
    let mut kept: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for j in &run.routes {
        kept.entry(j.why.pattern.clone()).or_default().push(j.route.id.clone());
    }
    let candidates: Vec<serde_json::Value> = scored
        .iter()
        .map(|(text, shape, s)| {
            let stage = if !s.passes {
                format!("score: {}", s.gate)
            } else if let Some(ids) = kept.get(text) {
                format!("kept: {}", ids.join(", "))
            } else if let Some(r) = dropped.get(text) {
                format!("compile/judge: {}", r.join(" | "))
            } else {
                "passed the gates; no route of its own (deduplicated, or beyond the selection bound)".into()
            };
            json!({
                "pattern": text, "shape": shape, "support": s.support, "opportunities": s.opp_sessions,
                "days": s.days, "conf": s.conf, "conf_lb": s.conf_lb, "contrast": s.contrast,
                "p_soon": s.p_soon, "dturns": s.dturns, "value": s.value, "variant_share": s.variant_share,
                "consistency": s.consistency, "stage": stage,
            })
        })
        .collect();
    let routes: Vec<serde_json::Value> = run
        .routes
        .iter()
        .map(|j| {
            json!({
                "id": j.route.id, "trigger": j.route.trigger, "step": j.route.step, "slots": j.route.slots,
                "strength": j.route.strength, "effect": j.route.effect, "evidence": j.route.evidence,
                "pattern": j.why.pattern,
            })
        })
        .collect();
    let out = json!({
        "sessions": ss.len(),
        "thresholds": if demo { "demo" } else { "default" },
        "candidates": candidates,
        "engine": run.report,
        "routes": routes,
    });
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}
