//! `adage graph [--json] [--events <n>] [--demo]`: Adage's live state as a graph, for people and tools.
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
//! `--demo` creates and removes an isolated scripted example through the real hook engine;
//! it never reads or changes the person's agent history.

use crate::review::{ceiling_str, discover, preview};
use adage_core::CommandTable;
use adage_runtime::{command_key, span_kind_label, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::{Index, Route, Step, Trigger};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MAX_OBSERVED: usize = 18;

/// Keep recent rows bounded while scanning every spool for aggregate counts. The sequence is
/// part of the ordering so simultaneous hook timestamps never reorder a session's steps.
fn recent(rows: &mut BTreeMap<(u64, String, usize), Value>, event: Value, limit: usize) {
    if limit == 0 {
        return;
    }
    rows.insert(
        (
            event["ts"].as_u64().unwrap_or(0),
            event["session"].as_str().unwrap_or("").into(),
            event["sequence"].as_u64().unwrap_or(0) as usize,
        ),
        event,
    );
    if rows.len() > limit {
        rows.pop_first();
    }
}

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

#[derive(Default)]
struct TraceSummary {
    arm: Option<String>,
    harnesses: BTreeSet<String>,
    start_ts: Option<u64>,
    end_ts: Option<u64>,
    events: usize,
    observed: usize,
    directed: usize,
    would: usize,
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

/// The installed directory distinguishes imported and locally learned copies. Spools name
/// track/version, not that directory: ambiguous copies must never both inherit one delivery.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Identity {
    repo: String,
    track: String,
    version: String,
    route: String,
}

type Catalog = BTreeMap<Identity, Vec<String>>;
type MetricKey = (String, String); // installed route key, recorded version

fn route_key(repo: &str, reference: &str, id: &str) -> String {
    json!([repo, reference, id]).to_string()
}

fn catalog(installed: &[crate::review::Installed]) -> Catalog {
    let mut out = Catalog::new();
    for installed in installed {
        for route in &installed.track.routes {
            let manifest = &installed.track.manifest;
            out.entry(Identity {
                repo: manifest.scope.repo.clone(),
                track: manifest.track.name.clone(),
                version: manifest.track.version.clone(),
                route: route.id.clone(),
            })
            .or_default()
            .push(route_key(&manifest.scope.repo, &installed.reference, &route.id));
        }
    }
    out
}

fn identity(repo: &str, directive: &Value) -> Option<Identity> {
    let field = |name| directive[name].as_str().filter(|s| !s.is_empty()).map(String::from);
    Some(Identity { repo: repo.into(), track: field("track")?, version: field("version")?, route: field("route")? })
}

fn attribution(repo: &str, directive: &Value, catalog: &Catalog) -> (&'static str, Option<String>) {
    let Some(id) = identity(repo, directive) else { return ("legacy", None) };
    if let Some(keys) = catalog.get(&id) {
        return if keys.len() == 1 { ("current", keys.first().cloned()) } else { ("ambiguous", None) };
    }
    let keys: Vec<&String> = catalog
        .iter()
        .filter(|(key, _)| key.repo == id.repo && key.track == id.track && key.route == id.route)
        .flat_map(|(_, keys)| keys)
        .collect();
    if keys.len() == 1 {
        ("historical", keys.first().map(|key| (*key).clone()))
    } else if keys.len() > 1 {
        ("ambiguous", None)
    } else {
        ("historical", None)
    }
}

fn attach_attribution(value: &mut Value, repo: &str, catalog: &Catalog) {
    let (status, key) = attribution(repo, value, catalog);
    value["attribution"] = json!(status);
    value["route_key"] = json!(key);
}

/// Normalize recorded decisions once for both the recent graph and complete selected traces.
/// Outcome records contain only a route id in older/current spool formats. Correlate them with
/// the uniquely identified exposure in this session, never whichever installation exists now.
fn recorded_events(path: &Path, catalog: &Catalog) -> Result<Vec<Value>> {
    let st: SessionState = state::read(&path.with_extension("state")).unwrap_or_default();
    let sid = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let records: Vec<Value> = spool::read_all(path)?;
    let fallback_repo = st
        .repo
        .filter(|repo| !repo.is_empty())
        .or_else(|| {
            records.iter().find_map(|record| record["repo"].as_str().filter(|repo| !repo.is_empty()).map(String::from))
        })
        .unwrap_or_default();
    let mut exposures = BTreeMap::<(String, String, String), Vec<(Value, usize)>>::new();
    let mut events = Vec::with_capacity(records.len());
    for (offset, record) in records.iter().enumerate() {
        let sequence = offset + 1;
        let repo = record["repo"].as_str().filter(|repo| !repo.is_empty()).unwrap_or(&fallback_repo);
        let arm = record["arm"].as_str().unwrap_or("unknown");
        let d = &record["decision"];
        let mut directives = d["directives"].as_array().cloned().unwrap_or_default();
        let mut would = d["would_have"].as_array().cloned().unwrap_or_default();
        for directive in directives.iter_mut().chain(&mut would) {
            attach_attribution(directive, repo, catalog);
        }
        // A batch may both announce an obligation and observe it later in that same batch.
        // Keep all candidate identities; disagreement makes a route-only outcome ambiguous.
        let exposed = if arm == "holdout" { &would } else { &directives };
        for directive in exposed {
            let Some(id) = directive["route"].as_str() else { continue };
            let candidates = exposures.entry((repo.into(), id.into(), arm.into())).or_default();
            if !candidates.iter().any(|(prior, _)| identity(repo, prior) == identity(repo, directive)) {
                candidates.push((directive.clone(), sequence));
            }
        }
        let mut judgements = d["judgements"].as_array().cloned().unwrap_or_default();
        for judgement in &mut judgements {
            let id = judgement["route"].as_str().unwrap_or("");
            let candidates = exposures.get(&(repo.into(), id.into(), arm.into()));
            match candidates.map(Vec::as_slice) {
                Some([(directive, exposure_sequence)]) => {
                    for key in ["track", "version", "route_key", "attribution"] {
                        judgement[key] = directive[key].clone();
                    }
                    judgement["exposure_sequence"] = json!(exposure_sequence);
                }
                Some(_) => {
                    judgement["attribution"] = json!("ambiguous");
                }
                None => {
                    judgement["attribution"] = json!("legacy");
                }
            }
        }
        let ids = |items: &[Value]| {
            items.iter().filter_map(|value| value["route"].as_str().map(String::from)).collect::<Vec<_>>()
        };
        let judged: Vec<Value> = judgements
            .iter()
            .filter(|j| j["kind"].is_string() && j["route"].is_string())
            .map(|j| json!([j["kind"], j["route"]]))
            .collect();
        events.push(json!({
            "ts": record["ts"], "session": sid, "sequence": sequence,
            "arm": arm, "event": record["event"], "kind": record["kind"], "repo": repo,
            "harness": record["harness"].as_str().unwrap_or("claude"), "generation": record["generation"],
            "directed": ids(&directives), "would": ids(&would), "judged": judged,
            "observed": d["observed"].as_array().cloned().unwrap_or_default(),
            "continue": d["continue_reason"].is_string(), "directives": directives,
            "would_have": would, "judgements": judgements,
            "continue_reason": d["continue_reason"], "would_continue": d["would_continue"],
        }));
    }
    Ok(events)
}

/// Review and display must use the same attributable outcomes. A route-only legacy
/// judgement or a judgement about another version cannot weaken an installed reflex.
pub(crate) fn current_judgements(
    paths: &Paths,
    installed: &[crate::review::Installed],
) -> BTreeMap<(String, String), (u32, u32)> {
    let catalog = catalog(installed);
    let keys: BTreeMap<String, (String, String)> = installed
        .iter()
        .flat_map(|installed| {
            installed.track.routes.iter().map(move |route| {
                (
                    route_key(&installed.track.manifest.scope.repo, &installed.reference, &route.id),
                    (installed.reference.clone(), route.id.clone()),
                )
            })
        })
        .collect();
    let mut counts = BTreeMap::<(String, String), (u32, u32)>::new();
    let Ok(entries) = std::fs::read_dir(paths.sessions()) else { return counts };
    for entry in entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file())) {
        let path = entry.path();
        if !path.extension().is_some_and(|extension| extension == "spool") {
            continue;
        }
        let Ok(events) = recorded_events(&path, &catalog) else { continue };
        for event in events.iter().filter(|event| event["arm"] == "treated") {
            for judgement in event["judgements"].as_array().into_iter().flatten() {
                if judgement["attribution"] != "current" {
                    continue;
                }
                let Some(key) = judgement["route_key"].as_str().and_then(|key| keys.get(key)) else { continue };
                let count = counts.entry(key.clone()).or_default();
                match judgement["kind"].as_str() {
                    Some("followed") => count.0 = count.0.saturating_add(1),
                    Some("derailed") => count.1 = count.1.saturating_add(1),
                    _ => {}
                }
            }
        }
    }
    counts
}

/// IDs are already the sanitized file stems from the inventory. Reject rather than sanitize
/// again (which could select a different session), and never follow a requested symlink.
pub(crate) fn valid_session_id(session: &str) -> bool {
    !session.is_empty()
        && session.len() <= 113
        && session.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(crate) fn trace(paths: &Paths, session: &str, repo: &str) -> Result<Option<Value>> {
    if !valid_session_id(session) || repo.len() > 2048 || repo.chars().any(char::is_control) {
        bail!("Invalid session or repository identifier");
    }
    let path = paths.sessions().join(format!("{session}.spool"));
    if !std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) {
        return Ok(None);
    }
    let catalog = catalog(&discover(&paths.tracks())?);
    let events: Vec<Value> =
        recorded_events(&path, &catalog)?.into_iter().filter(|event| event["repo"] == repo).collect();
    if events.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({ "session": session, "repo": repo, "events_total": events.len(), "events": events,
        "source": "recorded_hook_decisions", "complete": true })))
}

#[derive(Default)]
struct Measurement {
    applied: Vec<usize>,
    followed: Vec<usize>,
    met: Vec<usize>,
    hit_avoided: Vec<usize>,
}

#[derive(Default)]
struct Metrics {
    live: Live,
    arms: BTreeMap<String, ArmSets>,
    sessions: BTreeMap<(String, String), Measurement>, // session, arm
}

fn metric_key(value: &Value) -> Option<MetricKey> {
    Some((value["route_key"].as_str()?.into(), value["version"].as_str()?.into()))
}

fn judgement_count(live: &mut Live, kind: &str, holdout: bool) {
    match (kind, holdout) {
        ("followed", false) => live.followed += 1,
        ("derailed", false) => live.derailed += 1,
        ("obligation_met", false) => live.obligation_met += 1,
        ("followed", true) => live.unaided += 1,
        ("derailed", true) => live.not_reached += 1,
        ("obligation_met", true) => live.obligation_met_unaided += 1,
        _ => {}
    }
}

fn live_json(live: Option<&Live>) -> Value {
    let n = |f: fn(&Live) -> u64| live.map_or(0, f);
    json!({ "emitted": n(|l| l.emitted), "would_have": n(|l| l.would_have),
        "followed": n(|l| l.followed), "derailed": n(|l| l.derailed), "obligation_met": n(|l| l.obligation_met),
        "holdout": { "unaided": n(|l| l.unaided), "not_reached": n(|l| l.not_reached), "obligation_met": n(|l| l.obligation_met_unaided) },
        "sessions": live.map_or(0, |l| l.sessions.len()) })
}

fn measurement_json(
    metrics: Option<&Metrics>,
    repo: &str,
    arm_sessions: &BTreeMap<(String, String), BTreeSet<String>>,
) -> Value {
    let arms: serde_json::Map<String, Value> = ["treated", "holdout"]
        .iter()
        .map(|arm| {
            let sets = metrics.and_then(|metrics| metrics.arms.get(*arm));
            let n = |f: fn(&ArmSets) -> &BTreeSet<String>| sets.map_or(0, |sets| f(sets).len());
            (
                (*arm).into(),
                json!({
                    "sessions": arm_sessions.get(&(repo.into(), (*arm).into())).map_or(0, BTreeSet::len),
                    "applied": n(|s| &s.applied), "followed": n(|s| &s.followed), "met": n(|s| &s.met),
                    "hit_avoided": sets.map_or(0, |s| s.hit_avoided.intersection(&s.applied).count()),
                }),
            )
        })
        .collect();
    let sessions: Vec<Value> = metrics
        .into_iter()
        .flat_map(|metrics| &metrics.sessions)
        .map(|((session, arm), m)| {
            json!({ "session": session, "repo": repo, "arm": arm, "applied": m.applied,
            "followed": m.followed, "met": m.met, "hit_avoided": m.hit_avoided })
        })
        .collect();
    json!({ "live": live_json(metrics.map(|metrics| &metrics.live)), "arms": arms, "measurement_sessions": sessions })
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
    let demo = args.iter().any(|arg| arg == "--demo").then(crate::ui::demo::seeded).transpose()?;
    let paths = demo.as_ref().map(|home| &home.paths).unwrap_or(paths);
    if let Some(session) = crate::flag(args, "--session") {
        let repo = crate::flag(args, "--repo").ok_or_else(|| anyhow::anyhow!("--session requires --repo"))?;
        let value = trace(paths, &session, &repo)?.ok_or_else(|| anyhow::anyhow!("Recorded trace not found"))?;
        println!("{value}");
        return Ok(());
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
                "  [{}] {} → {} → {}   directed {} · followed {} · not observed within follow window {} │ control: withheld {} · unaided {} · not observed within follow window {}",
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

/// The graph as JSON, with a bounded recent event window and the complete session inventory.
pub(crate) fn build(paths: &Paths, max_events: usize) -> Result<Value> {
    let table = CommandTable::load();
    let installed = discover(&paths.tracks())?;
    let catalog = catalog(&installed);
    let index = Index::load(&paths.index());
    let mut metrics = BTreeMap::<MetricKey, Metrics>::new();
    let mut totals = Live::default();
    let mut attribution_counts = BTreeMap::<String, u64>::new();
    let mut judgement_attribution = BTreeMap::<String, u64>::new();
    let mut observed: BTreeMap<String, (u64, BTreeSet<String>)> = BTreeMap::new();
    let max_events = max_events.min(1000);
    let mut events = BTreeMap::new();
    let mut trace_events = BTreeMap::new();
    let mut trace_sessions = Vec::new();
    let mut trace_events_total = 0usize;
    let (mut treated, mut holdout, mut stops) = (0u64, 0u64, 0u64);
    let mut arm_sessions: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    // Avoided-action matching uses only the current version's rule. Applying a newer rule to
    // an old exposure would invent history: older versions retain their recorded outcomes.
    let avoided: BTreeMap<MetricKey, (String, Vec<String>)> = installed
        .iter()
        .flat_map(|i| {
            let table = &table;
            i.track.routes.iter().filter_map(move |r| match &r.step {
                Step::Avoid { avoid, .. } => Some((
                    (
                        route_key(&i.track.manifest.scope.repo, &i.reference, &r.id),
                        i.track.manifest.track.version.clone(),
                    ),
                    (
                        i.track.manifest.scope.repo.clone(),
                        avoid.split(" / ").map(|a| command_key(a.trim(), table)).collect(),
                    ),
                )),
                _ => None,
            })
        })
        .collect();
    let mut spools: Vec<PathBuf> = std::fs::read_dir(paths.sessions())
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "spool"))
                .collect()
        })
        .unwrap_or_default();
    spools.sort();
    for path in &spools {
        let records = recorded_events(path, &catalog)?;
        match records.first().and_then(|r| r["arm"].as_str()) {
            Some("treated") => treated += 1,
            Some("holdout") => holdout += 1,
            _ => {}
        }
        let mut session_traces = BTreeMap::<String, TraceSummary>::new();
        for event in records {
            let repo = event["repo"].as_str().unwrap_or("").to_string();
            let sid = event["session"].as_str().unwrap_or("").to_string();
            let arm = event["arm"].as_str().unwrap_or("unknown").to_string();
            let is_holdout = arm == "holdout";
            let sequence = event["sequence"].as_u64().unwrap_or(0) as usize;
            arm_sessions.entry((repo.clone(), arm.clone())).or_default().insert(sid.clone());
            let delivered = event["directives"].as_array().unwrap();
            let would = event["would_have"].as_array().unwrap();
            totals.emitted += delivered.len() as u64;
            totals.would_have += if is_holdout { would.len() as u64 } else { 0 };
            if !delivered.is_empty() {
                totals.sessions.insert(sid.clone());
            }
            let exposures = if is_holdout { would } else { delivered };
            for exposure in exposures {
                *attribution_counts.entry(exposure["attribution"].as_str().unwrap_or("legacy").into()).or_default() +=
                    1;
                let Some(key) = metric_key(exposure) else { continue };
                let m = metrics.entry(key).or_default();
                if is_holdout {
                    m.live.would_have += 1;
                } else {
                    m.live.emitted += 1;
                    m.live.sessions.insert(sid.clone());
                }
                m.arms.entry(arm.clone()).or_default().applied.insert(sid.clone());
                m.sessions.entry((sid.clone(), arm.clone())).or_default().applied.push(sequence);
            }
            for judgement in event["judgements"].as_array().unwrap() {
                let kind = judgement["kind"].as_str().unwrap_or("");
                judgement_count(&mut totals, kind, is_holdout);
                *judgement_attribution
                    .entry(judgement["attribution"].as_str().unwrap_or("legacy").into())
                    .or_default() += 1;
                let Some(key) = metric_key(judgement) else { continue };
                let m = metrics.entry(key).or_default();
                judgement_count(&mut m.live, kind, is_holdout);
                let sets = m.arms.entry(arm.clone()).or_default();
                let session = m.sessions.entry((sid.clone(), arm.clone())).or_default();
                match kind {
                    "followed" => {
                        sets.followed.insert(sid.clone());
                        session.followed.push(sequence);
                    }
                    "obligation_met" => {
                        sets.met.insert(sid.clone());
                        session.met.push(sequence);
                    }
                    _ => {}
                }
            }
            let keys = event["observed"].as_array().unwrap();
            for key in keys.iter().filter_map(Value::as_str) {
                let obs = observed.entry(key.into()).or_default();
                obs.0 += 1;
                obs.1.insert(sid.clone());
                let normalized = key.trim_end_matches(" !failed");
                for (metric_key, (avoided_repo, avoided_keys)) in &avoided {
                    if avoided_repo != &repo {
                        continue;
                    }
                    let Some(m) = metrics.get_mut(metric_key) else { continue };
                    let Some(session) = m.sessions.get_mut(&(sid.clone(), arm.clone())) else { continue };
                    if avoided_keys.iter().any(|key| key == normalized || speaks_for(key, normalized)) {
                        m.arms.entry(arm.clone()).or_default().hit_avoided.insert(sid.clone());
                        if session.hit_avoided.last() != Some(&sequence) {
                            session.hit_avoided.push(sequence);
                        }
                    }
                }
            }
            let cont = event["continue"] == true;
            stops += cont as u64;
            let summary = session_traces.entry(repo).or_default();
            summary.arm.get_or_insert(arm);
            summary.harnesses.insert(event["harness"].as_str().unwrap_or("claude").into());
            if let Some(ts) = event["ts"].as_u64() {
                summary.start_ts = Some(summary.start_ts.map_or(ts, |start| start.min(ts)));
                summary.end_ts = Some(summary.end_ts.map_or(ts, |end| end.max(ts)));
            }
            summary.events += 1;
            summary.observed += keys.len();
            summary.directed += delivered.len();
            summary.would += if is_holdout { would.len() } else { 0 };
            trace_events_total += 1;
            recent(&mut trace_events, event.clone(), max_events);
            if !delivered.is_empty()
                || !would.is_empty()
                || !event["judged"].as_array().unwrap().is_empty()
                || !keys.is_empty()
                || cont
            {
                recent(&mut events, event, max_events);
            }
        }
        let sid = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        for (repo, summary) in session_traces {
            trace_sessions.push(json!({ "session": sid, "repo": repo, "arm": summary.arm.unwrap_or_else(|| "unknown".into()),
                "harnesses": summary.harnesses, "start_ts": summary.start_ts, "end_ts": summary.end_ts,
                "events_total": summary.events, "observed_count": summary.observed, "directed_count": summary.directed, "would_count": summary.would }));
        }
    }
    let events: Vec<Value> = events.into_values().collect();
    let trace_events: Vec<Value> = trace_events.into_values().collect();
    let mut visible = BTreeMap::<(&str, &str), usize>::new();
    for event in &trace_events {
        *visible.entry((event["repo"].as_str().unwrap_or(""), event["session"].as_str().unwrap_or(""))).or_default() +=
            1;
    }
    for session in &mut trace_sessions {
        let count = visible
            .get(&(session["repo"].as_str().unwrap_or(""), session["session"].as_str().unwrap_or("")))
            .copied()
            .unwrap_or(0);
        session["events_visible"] = json!(count);
        session["truncated"] = json!(count < session["events_total"].as_u64().unwrap_or(0) as usize);
    }
    trace_sessions.sort_by_key(|session| std::cmp::Reverse(session["end_ts"].as_u64().unwrap_or(0)));
    let mut railed: BTreeMap<String, (String, &'static str)> = BTreeMap::new();
    let mut routes = Vec::new();
    for inst in &installed {
        let t = &inst.track;
        let repo = &t.manifest.scope.repo;
        for r in &t.routes {
            let key = route_key(repo, &inst.reference, &r.id);
            let id = Identity {
                repo: repo.clone(),
                track: t.manifest.track.name.clone(),
                version: t.manifest.track.version.clone(),
                route: r.id.clone(),
            };
            let ambiguous = catalog.get(&id).is_some_and(|keys| keys.len() > 1);
            let indexed = index.routes.iter().any(|x| {
                &x.repo == repo
                    && x.route.id == r.id
                    && x.track == t.manifest.track.name
                    && x.version == t.manifest.track.version
            });
            for (k, how) in route_keys(r, &table) {
                railed.entry(k).or_insert((r.id.clone(), how));
            }
            let current =
                measurement_json(metrics.get(&(key.clone(), t.manifest.track.version.clone())), repo, &arm_sessions);
            let info = |status| json!({ "status": status, "description": "Counts are tied to the recorded collection and version. Outcomes link to the session's recorded exposure; ambiguous or unidentified legacy records are excluded." });
            let history: Vec<Value> = metrics
                .iter()
                .filter(|((route_key, version), _)| route_key == &key && version != &t.manifest.track.version)
                .map(|((_, version), m)| {
                    let mut json = measurement_json(Some(m), repo, &arm_sessions);
                    json["version"] = json!(version);
                    json["measurement_info"] = info("historical");
                    json["arms"]["treated"]["hit_avoided"] = Value::Null;
                    json["arms"]["holdout"]["hit_avoided"] = Value::Null;
                    json
                })
                .collect();
            let e = &r.evidence;
            routes.push(json!({ "id": r.id, "track": t.manifest.track.name, "version": t.manifest.track.version,
                "reference": inst.reference, "repo": repo, "imported": inst.imported, "in_index": indexed,
                "aspect": lower(r.aspect), "ceiling": ceiling_str(r), "effect": lower(r.effect), "state": lower(r.state), "authored": r.authored,
                "trigger": trigger(&r.trigger), "step": step(&r.step), "directive": preview(t, r).0,
                "evidence": { "support": e.support, "opportunities": e.opportunities, "rate": e.rate, "lower_bound": e.lower_bound, "turns_saved": e.turns_saved },
                "live": current["live"], "arms": current["arms"], "measurement_sessions": current["measurement_sessions"],
                "measurement_info": info(if ambiguous { "ambiguous" } else { "current" }), "measurement_history": history,
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
    Ok(json!({ "generation": index.generation, "home": paths.root.display().to_string(),
        "sessions": { "total": spools.len(), "treated": treated, "holdout": holdout }, "stop_requests": stops,
        "totals": live_json(Some(&totals)), "attribution": { "exposures": attribution_counts, "judgements": judgement_attribution,
            "description": "Raw totals count each recorded decision once. Reflex measurements require an unambiguous recorded collection/version; legacy and duplicate identities are not assigned to a current reflex." },
        "routes": routes, "observed": observed, "events": events, "trace_events": trace_events, "trace_sessions": trace_sessions,
        "trace_info": { "source": "recorded_hook_decisions", "events_total": trace_events_total, "events_visible": trace_events.len(),
            "truncated": trace_events_total > trace_events.len(), "limit": max_events, "complete_session_inventory": true, "session_endpoint": "trace.json",
            "description": "Recorded hook decisions and normalized action templates. Raw prompts, tool outputs, and private agent reasoning are not recorded in these traces. Hooks that cannot change a decision may be skipped; this is not a complete agent transcript." },
        "source": if paths.root.join("demo.json").is_file() { json!({ "kind": "demo", "label": "Scripted demo", "description": "Scripted sessions processed by the real hook engine; learning updates and evidence are simulated." }) }
        else { json!({ "kind": "live", "label": "Local workspace" }) },
    }))
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
