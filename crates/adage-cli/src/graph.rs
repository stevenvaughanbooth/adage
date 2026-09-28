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
//! The JSON is what the signal box (`crates/adage-cli/assets/brain.html`) draws; `adage graph
//! --serve` serves both from the binary (see `serve.rs`).

use crate::review::{ceiling_str, discover, preview};
use adage_core::{pattern, CommandTable};
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

pub(crate) fn trigger(t: &Trigger) -> Value {
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
        Step::Advise { template, slots } => {
            json!({ "kind": "advise", "label": adage_tracks::advice::text(template, slots).unwrap_or_default(), "template": template })
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
        Step::ReadBatch { .. } | Step::Advise { .. } => vec![],
    }
}

/// `adage graph --learned --dump <repo> [--transcripts <file>...] [--claude-projects <dir>] [--json]`:
/// the graph engine's session facts for one repository (Track G G0), as counts: events, prompts,
/// acts, artifacts, edges by kind, token occurrences, embeddings, truncations, and the value slots
/// learned from them. Sessions are read as `adage learn` reads them (Claude Code history by
/// default); facts go through the facts cache (`$ADAGE_HOME/cache/facts`). Read-only apart from
/// that cache.
fn dump_learned(args: &[String], paths: &Paths) -> Result<()> {
    let at = args.iter().position(|a| a == "--dump").unwrap_or(0);
    let repo = args.get(at + 1).filter(|r| !r.starts_with("--")).cloned();
    let Some(repo) = repo else {
        anyhow::bail!("usage: adage graph --learned --dump <repo> [--transcripts <file>...] [--json]")
    };
    let mut files: Vec<PathBuf> = Vec::new();
    if let Some(i) = args.iter().position(|a| a == "--transcripts") {
        files.extend(args[i + 1..].iter().take_while(|a| !a.starts_with("--")).map(PathBuf::from));
    } else {
        let dir = crate::flag(args, "--claude-projects")
            .map(PathBuf::from)
            .or_else(|| adage_store::home_dir().map(|h| h.join(".claude").join("projects")));
        if let Some(d) = dir {
            files.extend(adage_learn::import::discover(&d));
        }
    }
    let table = CommandTable::load();
    let mut sessions: Vec<adage_learn::import::Session> = files
        .iter()
        .filter_map(|p| adage_learn::read_session(p, &table, None))
        .filter(|s| s.repo == repo && !s.steps.is_empty())
        .collect();
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    let refs: Vec<&adage_learn::import::Session> = sessions.iter().collect();
    let model = adage_intent::model::StaticModel::load(&adage_intent::model::dir_in(&paths.models())).ok();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
    let facts_dir = paths.cache().join("facts");
    let (facts, hits, misses) = adage_learn::graph::compute_facts(&refs, model.as_ref(), Some(&facts_dir), threads);
    let stats = adage_graph::facts::Stats::of(&facts);
    let inputs: Vec<adage_graph::input::Session> = refs.iter().map(|s| adage_learn::bridge::to_input(s)).collect();
    let learned = adage_graph::learn(&inputs, &adage_graph::Params { threads, ..Default::default() });
    let slots: Vec<Value> = learned
        .slots
        .iter()
        .map(|s| {
            let contract = match &s.contract {
                adage_graph::Contract::Pattern(i) => i.vtype.regex.clone(),
                adage_graph::Contract::RepoPath => "repo_path".into(),
                adage_graph::Contract::None(r) => format!("none: {r:?}"),
            };
            json!({ "sink": s.slot.sink.name(), "sessions": s.slot.sessions, "distinct": s.slot.distinct,
                    "flow_rate": s.slot.flow_rate, "sources": s.slot.sources, "contract": contract })
        })
        .collect();
    // Every sink that saw a flow, with the value slot test's numbers, for the ones that failed it.
    let all = adage_graph::flow::fold(&adage_graph::flow::observe_all(&inputs, threads), &Default::default());
    let rejected: Vec<Value> = all
        .iter()
        .filter(|s| !s.is_slot)
        .map(|s| {
            json!({ "sink": s.sink.name(), "sessions": s.sessions, "distinct": s.distinct,
                    "flow_rate": s.flow_rate, "top_share": s.top_share, "sources": s.sources })
        })
        .collect();
    let out = json!({
        "repo": repo, "sessions": sessions.len(), "model": model.as_ref().map(|m| m.id.clone()),
        "facts": stats, "cache": { "dir": facts_dir, "hits": hits, "misses": misses },
        "sinks": learned.sinks, "slots": slots, "not_slots": rejected,
    });
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    println!(
        "adage graph --learned: {repo}: {} sessions (facts cache: {hits} hits, {misses} computed)",
        sessions.len()
    );
    println!(
        "  events {} (prompts {}, acts {}, failed {}), max per session {}",
        stats.events, stats.prompts, stats.acts, stats.failed_acts, stats.max_events
    );
    println!(
        "  artifacts {} ({} distinct), distinct act keys {}, observations {}, failures {}",
        stats.arts, stats.distinct_arts, stats.distinct_act_keys, stats.obs, stats.fails
    );
    let edges: Vec<String> = stats.edges.iter().map(|(k, v)| format!("{k} {v}")).collect();
    println!("  edges: {}", if edges.is_empty() { "none".into() } else { edges.join(", ") });
    println!(
        "  token occurrences {}, embeddings {} (model: {}), truncated sessions {}",
        stats.tokens,
        stats.embeddings,
        model.as_ref().map(|m| m.id.as_str()).unwrap_or("none; prompts not embedded"),
        stats.truncated_sessions
    );
    println!("  value flow: {} sinks, {} value slots", learned.sinks, learned.slots.len());
    for s in &slots {
        println!(
            "    {}  {}  ({} sessions)",
            s["sink"].as_str().unwrap_or(""),
            s["contract"].as_str().unwrap_or(""),
            s["sessions"]
        );
    }
    for s in &rejected {
        println!(
            "    not a slot: {}  ({} sessions, {} distinct, flow rate {:.2}, top share {:.2})",
            s["sink"].as_str().unwrap_or(""),
            s["sessions"],
            s["distinct"],
            s["flow_rate"].as_f64().unwrap_or(0.0),
            s["top_share"].as_f64().unwrap_or(0.0)
        );
    }
    Ok(())
}

pub fn graph(args: &[String], paths: &Paths) -> Result<()> {
    if args.iter().any(|a| a == "--learned") && args.iter().any(|a| a == "--dump") {
        return dump_learned(args, paths);
    }
    let max_events: usize = crate::flag(args, "--events").and_then(|n| n.parse().ok()).unwrap_or(80).min(1000);
    let out = build(paths, max_events)?;
    if args.iter().any(|a| a == "--json") {
        println!("{out}");
    } else {
        let s = &out["sessions"];
        println!(
            "adage graph: {} routes, {} sessions ({} treated, {} holdout)",
            routes_len(&out),
            s["total"],
            s["treated"],
            s["holdout"]
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

/// The graph as JSON, with at most `max_events` recent events.
pub(crate) fn build(paths: &Paths, max_events: usize) -> Result<Value> {
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
            i.track.routes.iter().filter_map(move |r| match &r.step {
                Step::Avoid { avoid, .. } => {
                    Some((repo.clone(), r.id.clone(), avoid.split(" / ").map(|a| a.trim().to_string()).collect()))
                }
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
        // The whole (sanitised) id: OpenCode ids and Codex UUIDv7s begin with the creation time,
        // so sessions started together share a long prefix. Displays shorten it themselves.
        let sid: String = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
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
                    (Some("followed"), Some(id)) => {
                        sets(&mut arms, &repo, id, &arm).followed.insert(sid.clone());
                    }
                    (Some("obligation_met"), Some(id)) => {
                        sets(&mut arms, &repo, id, &arm).met.insert(sid.clone());
                    }
                    _ => {}
                }
            }
            for k in d["observed"].as_array().into_iter().flatten().filter_map(|k| k.as_str()) {
                let k = k.trim_end_matches(" !failed");
                for (rrepo, id, keys) in &avoided {
                    if *rrepo == repo && keys.iter().any(|runner| pattern::runs_avoided(runner, k)) {
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
            let r = railed.get(k).or_else(|| railed.iter().find(|(rk, v)| speaks_for(rk, v.1, k)).map(|(_, v)| v));
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
    Ok(out)
}

/// Whether a route's command key covers an observed key, as the runtime judges it
/// (`adage_core::pattern`): a directed command is met by the same command with options added
/// (`wi-lookup {item}` covers `wi-lookup {id} --json`, slots matching slots); an avoided runner
/// covers any run of its program and subcommand (`python3 -m pytest` covers
/// `python3 -m pytest {path} -v`).
fn speaks_for(route_key: &str, how: &str, observed: &str) -> bool {
    if how == "avoids" {
        pattern::CommandPattern::exact(route_key)
            .map(|p| p.at_least(pattern::Level::HeadSub))
            .is_some_and(|p| p.matches_key(observed))
    } else {
        pattern::meets(route_key, observed)
    }
}

fn routes_len(v: &Value) -> usize {
    v["routes"].as_array().map_or(0, |a| a.len())
}

#[cfg(test)]
mod tests {
    use super::speaks_for;

    #[test]
    fn route_keys_cover_observed_variants() {
        let d = |r: &str, o: &str| speaks_for(r, "directs", o);
        assert!(d("exec python3 scripts/wi-lookup {item}", "exec python3 scripts/wi-lookup {id}"));
        assert!(d("exec python3 scripts/wi-lookup {item}", "exec scripts/wi-lookup {id} --json"));
        assert!(speaks_for("exec python3 -m pytest", "avoids", "exec python3 -m pytest {path} -v"));
        assert!(d("exec make test", "exec make test"));
        assert!(d("exec make gen", "exec make gen -j4"));
        assert!(!d("exec make test", "exec make gen"));
        assert!(!d("exec npm run check", "exec npm run build"));
        assert!(!d("exec make", "exec"));
        assert!(!d("exec python3 -m pytest {path}", "exec python3 -m pytest"));
        assert!(!d("exec python3 -m pytest", "exec python3 -m pytest {path} -v"), "a directed command is not a runner");
    }
}
