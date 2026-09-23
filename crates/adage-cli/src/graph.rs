//! `adage graph [--json] [--events <n>]`: Adage's live state as a graph, for people and tools.
//!
//! Judgements are split by arm. In treated sessions they say whether the agent followed what it
//! was told; in holdout sessions nothing was delivered, so they say whether the agent found the
//! same path unaided — the baseline every effect is measured against.
//!
//! Nodes are the situations Adage watches for (route triggers), the routes, the steps they
//! direct, and how agents responded. Every count comes from the installed tracks and the session
//! spools under `$ADAGE_HOME`; nothing is estimated. Read-only, so agents may run it.
//!
//! The JSON is what `demo/dashboard/brain.html` draws.

use crate::review::{ceiling_str, discover, preview};
use adage_core::CommandTable;
use adage_runtime::{command_key, span_kind_label, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::{Index, Route, Step, Trigger};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const MAX_OBSERVED: usize = 18;

#[derive(Default)]
struct Live {
    emitted: u64,
    would_have: u64,
    followed: u64,
    derailed: u64,
    obligation_met: u64,
    /// Holdout sessions: nothing was delivered, so a "followed" judgement means the agent took the
    /// route's path unaided, and a "derailed" one means it did not.
    unaided: u64,
    not_reached: u64,
    obligation_met_unaided: u64,
    sessions: BTreeSet<String>,
}

/// Per route and arm, in distinct sessions: where the route applied (delivered, or would have
/// been), where the agent took its step, met its obligation, or ran a command it warns against.
#[derive(Default)]
struct ArmSets {
    applied: BTreeSet<String>,
    followed: BTreeSet<String>,
    met: BTreeSet<String>,
    hit_avoided: BTreeSet<String>,
}

type ArmMap = BTreeMap<(String, String, String), ArmSets>;

fn sets<'a>(m: &'a mut ArmMap, repo: &str, id: &str, arm: &str) -> &'a mut ArmSets {
    m.entry((repo.to_string(), id.to_string(), arm.to_string())).or_default()
}

fn lower(x: impl std::fmt::Debug) -> String {
    format!("{x:?}").to_lowercase()
}

fn trigger(t: &Trigger) -> Value {
    let (kind, label) = match t {
        Trigger::Prompt { span } => ("prompt", format!("a prompt names a {}", span_kind_label(*span))),
        Trigger::AfterCommand { command } => ("after_command", format!("`{command}` succeeds")),
        Trigger::AfterFailure { command, signature } => ("after_failure", format!("`{command}` fails: {signature}")),
        Trigger::AfterEdit { paths } => ("after_edit", format!("an edit touches {paths}")),
        Trigger::SessionStart => ("session_start", "the first prompt of a session".to_string()),
        Trigger::Intent { family, .. } => ("intent", format!("a prompt means: {family}")),
    };
    json!({ "kind": kind, "label": label })
}

fn step(s: &Step) -> Value {
    match s {
        Step::Run { command } => json!({ "kind": "run", "label": command }),
        Step::ReadBatch { slot, max } => {
            json!({ "kind": "read_batch", "label": format!("read ≤{max} listed files at once"), "slot": slot })
        }
        Step::Avoid { avoid, instead, signature } => {
            json!({ "kind": "avoid", "label": instead, "avoid": avoid, "signature": signature })
        }
    }
}

/// The observed-command keys a route speaks for, to mark which of the agents' paths are railed.
fn route_keys(r: &Route, table: &CommandTable) -> Vec<(String, &'static str)> {
    match &r.step {
        Step::Run { command } => vec![(command_key(command, table), "directs")],
        Step::Avoid { avoid, instead, .. } => {
            let mut v: Vec<(String, &'static str)> =
                avoid.split(" / ").map(|a| (command_key(a.trim(), table), "avoids")).collect();
            v.push((command_key(instead, table), "directs"));
            v
        }
        Step::ReadBatch { .. } => vec![],
    }
}

pub fn graph(args: &[String], paths: &Paths) -> Result<()> {
    let max_events: usize = crate::flag(args, "--events").and_then(|n| n.parse().ok()).unwrap_or(80).min(1000);
    let table = CommandTable::load();
    let installed = discover(&paths.tracks())?;
    let index = Index::load(&paths.index());

    // Routes, keyed by (repo, route id) as the runtime identifies them.
    let mut live: BTreeMap<(String, String), Live> = BTreeMap::new();
    let mut observed: BTreeMap<String, (u64, BTreeSet<String>)> = BTreeMap::new();
    let mut events: Vec<Value> = Vec::new();
    let (mut treated, mut holdout, mut stops) = (0u64, 0u64, 0u64);

    // The commands each dead-end route warns against, to count sessions that ran one anyway.
    let avoided: Vec<(String, String, Vec<String>)> = installed
        .iter()
        .flat_map(|i| {
            let repo = i.track.manifest.scope.repo.clone();
            let table = &table;
            i.track.routes.iter().filter_map(move |r| match &r.step {
                Step::Avoid { avoid, .. } => Some((
                    repo.clone(),
                    r.id.clone(),
                    avoid.split(" / ").map(|a| command_key(a.trim(), table)).collect(),
                )),
                _ => None,
            })
        })
        .collect();
    let mut arms: ArmMap = BTreeMap::new();
    let mut arm_sessions: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();

    let mut spools: Vec<PathBuf> = std::fs::read_dir(paths.sessions())
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "spool")).collect())
        .unwrap_or_default();
    spools.sort();
    for p in &spools {
        let st: SessionState = state::read(&p.with_extension("state")).unwrap_or_default();
        let repo = st.repo.clone().unwrap_or_default();
        let sid: String = p.file_stem().map(|s| s.to_string_lossy().chars().take(8).collect()).unwrap_or_default();
        let recs: Vec<Value> = spool::read_all(p).unwrap_or_default();
        match recs.iter().find_map(|r| r["arm"].as_str()) {
            Some("treated") => treated += 1,
            Some("holdout") => holdout += 1,
            _ => {}
        }
        for r in &recs {
            let d = &r["decision"];
            let ids = |k: &str| -> Vec<String> {
                d[k].as_array().into_iter().flatten().filter_map(|x| x["route"].as_str().map(String::from)).collect()
            };
            let (directed, would) = (ids("directives"), ids("would_have"));
            for id in &directed {
                let l = live.entry((repo.clone(), id.clone())).or_default();
                l.emitted += 1;
                l.sessions.insert(sid.clone());
            }
            for id in &would {
                live.entry((repo.clone(), id.clone())).or_default().would_have += 1;
            }
            let holdout = r["arm"] == "holdout";
            let arm = r["arm"].as_str().unwrap_or("unknown").to_string();
            arm_sessions.entry((repo.clone(), arm.clone())).or_default().insert(sid.clone());
            for id in directed.iter().chain(&would) {
                sets(&mut arms, &repo, id, &arm).applied.insert(sid.clone());
            }
            for j in d["judgements"].as_array().into_iter().flatten() {
                match (j["kind"].as_str(), j["route"].as_str()) {
                    (Some("followed"), Some(id)) => drop(sets(&mut arms, &repo, id, &arm).followed.insert(sid.clone())),
                    (Some("obligation_met"), Some(id)) => {
                        drop(sets(&mut arms, &repo, id, &arm).met.insert(sid.clone()))
                    }
                    _ => {}
                }
            }
            for k in d["observed"].as_array().into_iter().flatten().filter_map(|k| k.as_str()) {
                let k = k.trim_end_matches(" !failed");
                for (rrepo, id, keys) in &avoided {
                    if *rrepo == repo && keys.iter().any(|ak| ak == k || speaks_for(ak, k)) {
                        sets(&mut arms, &repo, id, &arm).hit_avoided.insert(sid.clone());
                    }
                }
            }
            let mut judged = Vec::new();
            for j in d["judgements"].as_array().into_iter().flatten() {
                let (Some(kind), Some(id)) = (j["kind"].as_str(), j["route"].as_str()) else { continue };
                let l = live.entry((repo.clone(), id.to_string())).or_default();
                match (kind, holdout) {
                    ("followed", false) => l.followed += 1,
                    ("derailed", false) => l.derailed += 1,
                    ("obligation_met", false) => l.obligation_met += 1,
                    ("followed", true) => l.unaided += 1,
                    ("derailed", true) => l.not_reached += 1,
                    ("obligation_met", true) => l.obligation_met_unaided += 1,
                    _ => {}
                }
                judged.push(json!([kind, id]));
            }
            let keys: Vec<String> =
                d["observed"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect();
            for k in &keys {
                let e = observed.entry(k.clone()).or_default();
                e.0 += 1;
                e.1.insert(sid.clone());
            }
            let cont = d["continue_reason"].is_string();
            stops += cont as u64;
            if !directed.is_empty() || !would.is_empty() || !judged.is_empty() || !keys.is_empty() || cont {
                events.push(json!({
                    "ts": r["ts"], "session": sid, "arm": r["arm"], "event": r["event"], "repo": repo,
                    "directed": directed, "would": would, "judged": judged, "observed": keys, "continue": cont,
                }));
            }
        }
    }
    events.sort_by_key(|e| e["ts"].as_u64().unwrap_or(0));
    let events: Vec<Value> = events.split_off(events.len().saturating_sub(max_events));

    let mut railed: BTreeMap<String, (String, &'static str)> = BTreeMap::new();
    let mut routes = Vec::new();
    for inst in &installed {
        let t = &inst.track;
        let repo = &t.manifest.scope.repo;
        for r in &t.routes {
            let indexed =
                index.routes.iter().any(|x| &x.repo == repo && x.route.id == r.id && x.track == t.manifest.track.name);
            for (k, how) in route_keys(r, &table) {
                railed.entry(k).or_insert((r.id.clone(), how));
            }
            let arm_stats: serde_json::Map<String, Value> = ["treated", "holdout"]
                .iter()
                .map(|a| {
                    let s = arms.get(&(repo.clone(), r.id.clone(), a.to_string()));
                    let n = |f: fn(&ArmSets) -> &BTreeSet<String>| s.map_or(0, |s| f(s).len());
                    let json = json!({
                        "sessions": arm_sessions.get(&(repo.clone(), a.to_string())).map_or(0, |s| s.len()),
                        "applied": n(|s| &s.applied),
                        "followed": n(|s| &s.followed),
                        "met": n(|s| &s.met),
                        // Only sessions the route applied to: before it was learned, it could not help.
                        "hit_avoided": s.map_or(0, |s| s.hit_avoided.intersection(&s.applied).count()),
                    });
                    (a.to_string(), json)
                })
                .collect();
            let l = live.get(&(repo.clone(), r.id.clone()));
            let e = &r.evidence;
            routes.push(json!({
                "id": r.id,
                "track": t.manifest.track.name,
                "version": t.manifest.track.version,
                "reference": inst.reference,
                "repo": repo,
                "imported": inst.imported,
                "in_index": indexed,
                "aspect": lower(r.aspect),
                "ceiling": ceiling_str(r),
                "effect": lower(r.effect),
                "state": lower(r.state),
                "authored": r.authored,
                "trigger": trigger(&r.trigger),
                "step": step(&r.step),
                "directive": preview(t, r).0,
                "evidence": { "support": e.support, "opportunities": e.opportunities, "rate": e.rate, "lower_bound": e.lower_bound, "turns_saved": e.turns_saved },
                "live": {
                    "emitted": l.map_or(0, |l| l.emitted),
                    "would_have": l.map_or(0, |l| l.would_have),
                    "followed": l.map_or(0, |l| l.followed),
                    "derailed": l.map_or(0, |l| l.derailed),
                    "obligation_met": l.map_or(0, |l| l.obligation_met),
                    "holdout": {
                        "unaided": l.map_or(0, |l| l.unaided),
                        "not_reached": l.map_or(0, |l| l.not_reached),
                        "obligation_met": l.map_or(0, |l| l.obligation_met_unaided),
                    },
                    "sessions": l.map_or(0, |l| l.sessions.len()),
                },
                "arms": arm_stats,
            }));
        }
    }

    let mut obs: Vec<(&String, &(u64, BTreeSet<String>))> = observed.iter().collect();
    obs.sort_by(|a, b| b.1 .1.len().cmp(&a.1 .1.len()).then(b.1 .0.cmp(&a.1 .0)).then(a.0.cmp(b.0)));
    let observed: Vec<Value> = obs
        .into_iter()
        .take(MAX_OBSERVED)
        .map(|(k, (n, s))| {
            let r = railed.get(k).or_else(|| railed.iter().find(|(rk, _)| speaks_for(rk, k)).map(|(_, v)| v));
            json!({ "key": k, "count": n, "sessions": s.len(), "route": r.map(|r| &r.0), "how": r.map(|r| r.1) })
        })
        .collect();

    let out = json!({
        "generation": index.generation,
        "home": paths.root.display().to_string(),
        "sessions": { "total": spools.len(), "treated": treated, "holdout": holdout },
        "stop_requests": stops,
        "routes": routes,
        "observed": observed,
        "events": events,
    });
    if args.iter().any(|a| a == "--json") {
        println!("{out}");
    } else {
        println!(
            "adage graph: {} routes, {} sessions ({} treated, {} holdout)",
            routes_len(&out),
            spools.len(),
            treated,
            holdout
        );
        for r in out["routes"].as_array().into_iter().flatten() {
            let l = &r["live"];
            println!(
                "  [{}] {} → {} → {}   directed {} · followed {} · derailed {} │ holdout: would-have {} · unaided {} · not reached {}",
                r["aspect"].as_str().unwrap_or(""),
                r["trigger"]["label"].as_str().unwrap_or(""),
                r["id"].as_str().unwrap_or(""),
                r["step"]["label"].as_str().unwrap_or(""),
                l["emitted"], l["followed"], l["derailed"], l["would_have"], l["holdout"]["unaided"], l["holdout"]["not_reached"]
            );
        }
        println!("Use --json for the full graph (routes, observed commands, recent events).");
    }
    Ok(())
}

/// Whether a route's command key covers an observed key: the route's tokens are a prefix of the
/// observed ones, with any slot matching any slot (`wi-lookup {item}` covers `wi-lookup {id}`;
/// `python3 -m pytest` covers `python3 -m pytest {path} -v`).
fn speaks_for(route_key: &str, observed: &str) -> bool {
    let slot = |t: &str| t.starts_with('{') && t.ends_with('}');
    let (r, o): (Vec<&str>, Vec<&str>) =
        (route_key.split_whitespace().collect(), observed.split_whitespace().collect());
    r.len() >= 2 && r.len() <= o.len() && r.iter().zip(&o).all(|(a, b)| a == b || (slot(a) && slot(b)))
}

fn routes_len(v: &Value) -> usize {
    v["routes"].as_array().map_or(0, |a| a.len())
}

#[cfg(test)]
mod tests {
    use super::speaks_for;

    #[test]
    fn route_keys_cover_observed_variants() {
        assert!(speaks_for("exec python3 scripts/wi-lookup {item}", "exec python3 scripts/wi-lookup {id}"));
        assert!(speaks_for("exec python3 -m pytest", "exec python3 -m pytest {path} -v"));
        assert!(speaks_for("exec make test", "exec make test"));
        assert!(!speaks_for("exec make test", "exec make gen"));
        assert!(!speaks_for("exec make", "exec"));
        assert!(!speaks_for("exec python3 -m pytest {path}", "exec python3 -m pytest"));
    }
}
