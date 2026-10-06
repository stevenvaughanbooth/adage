//! Attribution and complete recorded traces shared by the map, review and terminal surfaces.
use super::{arm_word, judgement_word, mod_fields, ArmSets, Pretool, Tally, MOD_FIELDS};
use crate::review::{discover, Installed};
use adage_core::pattern;
use adage_runtime::SessionState;
use adage_store::{spool, state, Paths};
use adage_tracks::{Route, Step};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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

fn catalog(installed: &[Installed]) -> Catalog {
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
    let Some(id) = identity(repo, directive) else {
        return ("legacy", None);
    };
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
        let arm = record["arm"].as_str().map(arm_word).unwrap_or("unknown");
        let d = &record["decision"];
        // Truncated or malformed local records must not crash a read-only surface.
        let objects = |field: &str| -> Vec<Value> {
            d[field].as_array().into_iter().flatten().filter(|value| value.is_object()).cloned().collect()
        };
        let mut directives = objects("directives");
        let mut would = objects("would_have");
        for directive in directives.iter_mut().chain(&mut would) {
            attach_attribution(directive, repo, catalog);
        }
        // A batch may both announce an obligation and observe it later in that same batch.
        // Keep all candidate identities; disagreement makes a route-only outcome ambiguous.
        let exposed = if arm == "control" { &would } else { &directives };
        for directive in exposed {
            let Some(id) = directive["route"].as_str() else {
                continue;
            };
            let candidates = exposures.entry((repo.into(), id.into(), arm.into())).or_default();
            candidates.push((directive.clone(), sequence));
        }
        let mut judgements = objects("judgements");
        for judgement in &mut judgements {
            if let Some(kind) = judgement["kind"].as_str() {
                judgement["kind"] = json!(judgement_word(kind));
            }
            let id = judgement["route"].as_str().unwrap_or("");
            let candidates = exposures.get(&(repo.into(), id.into(), arm.into()));
            match candidates.map(Vec::as_slice) {
                Some([(directive, exposure_sequence)]) => {
                    for key in ["track", "version", "route_key", "attribution"] {
                        judgement[key] = directive[key].clone();
                    }
                    judgement["exposure_sequence"] = json!(exposure_sequence);
                }
                Some(candidates)
                    if candidates
                        .iter()
                        .all(|(value, _)| identity(repo, value) == identity(repo, &candidates[0].0)) =>
                {
                    // Repeated exposure still measures one identity, but a route-only outcome
                    // does not establish which of its deliveries was responsible.
                    for key in ["track", "version", "route_key", "attribution"] {
                        judgement[key] = candidates[0].0[key].clone();
                    }
                    judgement["exposure_sequence"] = Value::Null;
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
            "harness": record["harness"].as_str().unwrap_or("unknown"), "generation": record["generation"],
            "pretool": record["pretool"], "withheld": record["withheld"] == true,
            "unrouted": record["unrouted"] == true, "calls": record["calls"],
            "directed": ids(&directives), "would": ids(&would), "judged": judged,
            "observed": d["observed"].as_array().cloned().unwrap_or_default(),
            "continue": d["continue_reason"].is_string(), "directives": directives,
            "would_have": would, "judgements": judgements,
            "continue_reason": d["continue_reason"], "would_continue": d["would_continue"],
        }));
        if let Some(event) = events.last_mut().and_then(Value::as_object_mut) {
            event.extend(mod_fields(record));
        }
    }
    Ok(events)
}

/// Review and display must use the same attributable outcomes. A route-only legacy
/// judgement or a judgement about another version cannot weaken an installed reflex.
pub fn current_judgements(paths: &Paths, installed: &[Installed]) -> BTreeMap<(String, String), (u32, u32)> {
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
    let Ok(entries) = std::fs::read_dir(paths.sessions()) else {
        return counts;
    };
    for entry in entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file())) {
        let path = entry.path();
        if !path.extension().is_some_and(|extension| extension == "spool") {
            continue;
        }
        let Ok(events) = recorded_events(&path, &catalog) else {
            continue;
        };
        for event in events.iter().filter(|event| event["arm"] == "treated") {
            for judgement in event["judgements"].as_array().into_iter().flatten() {
                if judgement["attribution"] != "current" {
                    continue;
                }
                let Some(key) = judgement["route_key"].as_str().and_then(|key| keys.get(key)) else {
                    continue;
                };
                let count = counts.entry(key.clone()).or_default();
                match judgement["kind"].as_str() {
                    Some("followed" | "misfired") => count.0 = count.0.saturating_add(1),
                    Some("ignored") => count.1 = count.1.saturating_add(1),
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

pub fn trace(paths: &Paths, session: &str, repo: &str) -> Result<Option<Value>> {
    if !valid_session_id(session) || repo.len() > 2048 || repo.chars().any(char::is_control) {
        bail!("Invalid session or repository identifier");
    }
    let path = paths.sessions().join(format!("{session}.spool"));
    if !std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) {
        return Ok(None);
    }
    let catalog = catalog(&discover(&paths.tracks())?.0);
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
    live: Tally,
    arms: BTreeMap<String, ArmSets>,
    sessions: BTreeMap<(String, String), Measurement>, // session, arm
}

fn metric_key(value: &Value) -> Option<MetricKey> {
    Some((value["route_key"].as_str()?.into(), value["version"].as_str()?.into()))
}

fn judgement_count(live: &mut Tally, kind: &str, control: bool) {
    match (kind, control) {
        ("followed", false) => live.followed += 1,
        ("ignored", false) => live.ignored += 1,
        ("misfired", false) => live.misfired += 1,
        ("obligation_met", false) => live.obligation_met += 1,
        ("followed", true) => live.unaided += 1,
        ("ignored", true) => live.not_reached += 1,
        ("obligation_met", true) => live.obligation_met_unaided += 1,
        _ => {}
    }
}

fn live_json(live: Option<&Tally>) -> Value {
    json!(live.map(Tally::read).unwrap_or_default())
}

fn measurement_json(
    metrics: Option<&Metrics>,
    repo: &str,
    arm_sessions: &BTreeMap<(String, String), BTreeSet<String>>,
) -> Value {
    let arms: serde_json::Map<String, Value> = ["treated", "control"]
        .iter()
        .map(|arm| {
            let sets = metrics.and_then(|metrics| metrics.arms.get(*arm));
            let n = |f: fn(&ArmSets) -> &BTreeSet<String>| sets.map_or(0, |sets| f(sets).len());
            let mut json = json!({
                "sessions": arm_sessions.get(&(repo.into(), (*arm).into())).map_or(0, BTreeSet::len),
                "applied": n(|s| &s.applied), "followed": n(|s| &s.followed), "met": n(|s| &s.met),
                "hit_avoided": sets.map_or(0, |s| s.hit_avoided.intersection(&s.applied).count()),
            });
            // A dead end replaced before it ran is avoided, not hit, and not a follow. Said only
            // where it happened, so a map without the mod reads as before.
            if n(|s| &s.substituted) > 0 {
                json["substituted"] = json!(n(|s| &s.substituted));
            }
            ((*arm).into(), json)
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

pub(super) struct Scan {
    catalog: Catalog,
    metrics: BTreeMap<MetricKey, Metrics>,
    pub totals: Tally,
    pub attribution_counts: BTreeMap<String, u64>,
    pub judgement_attribution: BTreeMap<String, u64>,
    pub observed: BTreeMap<String, (u64, BTreeSet<String>)>,
    pub events: Vec<Value>,
    pub trace_events: Vec<Value>,
    pub trace_sessions: Vec<Value>,
    pub trace_events_total: usize,
    pub treated: u64,
    pub control: u64,
    pub stops: u64,
    pub spools: Vec<PathBuf>,
    pub arm_sessions: BTreeMap<(String, String), BTreeSet<String>>,
    pub pretool: BTreeMap<String, Pretool>,
}

pub(super) fn read(paths: &Paths, installed: &[Installed], max_events: usize, warn: &mut dyn FnMut(String)) -> Scan {
    let catalog = catalog(installed);
    let mut metrics = BTreeMap::<MetricKey, Metrics>::new();
    let mut totals = Tally::default();
    let mut attribution_counts = BTreeMap::<String, u64>::new();
    let mut judgement_attribution = BTreeMap::<String, u64>::new();
    let mut observed: BTreeMap<String, (u64, BTreeSet<String>)> = BTreeMap::new();
    let max_events = max_events.min(1000);
    let mut events = BTreeMap::new();
    let mut trace_events = BTreeMap::new();
    let mut trace_sessions = Vec::new();
    let mut trace_events_total = 0usize;
    let (mut treated, mut control, mut stops) = (0u64, 0u64, 0u64);
    let mut pretool = BTreeMap::<String, Pretool>::new();
    let mut arm_sessions: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    // Avoided-action matching uses only the current version's rule. Applying a newer rule to
    // an old exposure would invent history: older versions retain their recorded outcomes.
    let avoided: BTreeMap<MetricKey, (String, Vec<String>)> = installed
        .iter()
        .flat_map(|i| {
            i.track.routes.iter().filter_map(move |r| match &r.step {
                Step::Avoid { avoid, .. } => Some((
                    (
                        route_key(&i.track.manifest.scope.repo, &i.reference, &r.id),
                        i.track.manifest.track.version.clone(),
                    ),
                    (i.track.manifest.scope.repo.clone(), avoid.split(" / ").map(|a| a.trim().to_string()).collect()),
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
        let records = match recorded_events(path, &catalog) {
            Ok(records) => records,
            Err(error) => {
                warn(format!("adage: skipping unreadable trace {}: {error:#}", path.display()));
                continue;
            }
        };
        match records.iter().filter_map(|r| r["arm"].as_str()).find(|arm| matches!(*arm, "treated" | "control")) {
            Some("treated") => treated += 1,
            Some("control") => control += 1,
            _ => {}
        }
        let mut session_traces = BTreeMap::<String, TraceSummary>::new();
        // The reflexes each route id was exposed as in this session, by repository and arm: a
        // would-be substitution names only its route, so it is counted as a judgement is, against
        // one unambiguous exposure.
        let mut exposed = BTreeMap::<(String, String, String), BTreeSet<Option<MetricKey>>>::new();
        for event in records {
            let repo = event["repo"].as_str().unwrap_or("").to_string();
            let sid = event["session"].as_str().unwrap_or("").to_string();
            let arm = event["arm"].as_str().unwrap_or("unknown").to_string();
            let is_control = arm == "control";
            if event["kind"] == "pretool" {
                let p = pretool.entry(repo.clone()).or_default();
                if event["withheld"] == true {
                    p.withheld += 1;
                } else if event["pretool"]["rewrite"].is_string() {
                    p.rewritten += 1;
                } else if event["pretool"]["advise"].is_string() {
                    p.advised += 1;
                }
            }
            let sequence = event["sequence"].as_u64().unwrap_or(0) as usize;
            arm_sessions.entry((repo.clone(), arm.clone())).or_default().insert(sid.clone());
            let delivered = event["directives"].as_array().unwrap();
            let would = event["would_have"].as_array().unwrap();
            totals.emitted += delivered.len() as u64;
            totals.would_have += if is_control { would.len() as u64 } else { 0 };
            if !delivered.is_empty() {
                totals.sessions.insert(sid.clone());
            }
            let exposures = if is_control { would } else { delivered };
            for exposure in exposures {
                *attribution_counts.entry(exposure["attribution"].as_str().unwrap_or("legacy").into()).or_default() +=
                    1;
                if let Some(id) = exposure["route"].as_str() {
                    exposed.entry((repo.clone(), id.into(), arm.clone())).or_default().insert(metric_key(exposure));
                }
                let Some(key) = metric_key(exposure) else {
                    continue;
                };
                let m = metrics.entry(key).or_default();
                if is_control {
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
                judgement_count(&mut totals, kind, is_control);
                *judgement_attribution
                    .entry(judgement["attribution"].as_str().unwrap_or("legacy").into())
                    .or_default() += 1;
                let Some(key) = metric_key(judgement) else {
                    continue;
                };
                let m = metrics.entry(key).or_default();
                judgement_count(&mut m.live, kind, is_control);
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
                    "substituted" => {
                        sets.substituted.insert(sid.clone());
                    }
                    _ => {}
                }
            }
            // The control arm (and observe mode) records only the replacement it would make.
            let replaced = event["would_substitute"].as_str();
            let keys = replaced.and_then(|id| exposed.get(&(repo.clone(), id.into(), arm.clone())));
            if let Some(Some(key)) = keys.filter(|keys| keys.len() == 1).and_then(|keys| keys.first()) {
                metrics
                    .entry(key.clone())
                    .or_default()
                    .arms
                    .entry(arm.clone())
                    .or_default()
                    .substituted
                    .insert(sid.clone());
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
                    let Some(m) = metrics.get_mut(metric_key) else {
                        continue;
                    };
                    let Some(session) = m.sessions.get_mut(&(sid.clone(), arm.clone())) else {
                        continue;
                    };
                    if avoided_keys.iter().any(|key| pattern::runs_avoided(key, normalized)) {
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
            if summary.arm.as_deref().is_none_or(|previous| previous == "unknown") {
                summary.arm = Some(arm);
            }
            summary.harnesses.insert(event["harness"].as_str().unwrap_or("unknown").into());
            if let Some(ts) = event["ts"].as_u64() {
                summary.start_ts = Some(summary.start_ts.map_or(ts, |start| start.min(ts)));
                summary.end_ts = Some(summary.end_ts.map_or(ts, |end| end.max(ts)));
            }
            summary.events += 1;
            summary.observed += keys.len();
            summary.directed += delivered.len();
            summary.would += if is_control { would.len() } else { 0 };
            trace_events_total += 1;
            recent(&mut trace_events, event.clone(), max_events);
            if event["kind"] == "pretool"
                || !delivered.is_empty()
                || !would.is_empty()
                || !event["judged"].as_array().unwrap().is_empty()
                || !keys.is_empty()
                || cont
                || MOD_FIELDS.iter().any(|field| event.get(*field).is_some())
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
    Scan {
        catalog,
        metrics,
        totals,
        attribution_counts,
        judgement_attribution,
        observed,
        events,
        trace_events,
        trace_sessions,
        trace_events_total,
        treated,
        control,
        stops,
        spools,
        arm_sessions,
        pretool,
    }
}

impl Scan {
    pub fn measurement(&self, inst: &Installed, route: &Route) -> Value {
        let manifest = &inst.track.manifest;
        let repo = &manifest.scope.repo;
        let key = route_key(repo, &inst.reference, &route.id);
        let version = &manifest.track.version;
        let identity = Identity {
            repo: repo.clone(),
            track: manifest.track.name.clone(),
            version: version.clone(),
            route: route.id.clone(),
        };
        let ambiguous = self.catalog.get(&identity).is_some_and(|keys| keys.len() > 1);
        let mut current = measurement_json(self.metrics.get(&(key.clone(), version.clone())), repo, &self.arm_sessions);
        let info = |status| json!({ "status": status, "description": "Counts are tied to the recorded collection and version. Outcomes require an unambiguous exposure in the same session. An ignored outcome means the directed action was not observed within the next three action batches; it does not establish intent. Harnesses observe different batches." });
        current["measurement_info"] = info(if ambiguous { "ambiguous" } else { "current" });
        current["measurement_history"] = self
            .metrics
            .iter()
            .filter(|((k, v), _)| k == &key && v != version)
            .map(|((_, v), metrics)| {
                let mut history = measurement_json(Some(metrics), repo, &self.arm_sessions);
                history["version"] = json!(v);
                history["measurement_info"] = info("historical");
                // Never judge old observations against the current version's avoidance rule.
                history["arms"]["treated"]["hit_avoided"] = Value::Null;
                history["arms"]["control"]["hit_avoided"] = Value::Null;
                history
            })
            .collect::<Vec<_>>()
            .into();
        current
    }

    pub fn live(&self, inst: &Installed, route: &Route) -> crate::reflex::Live {
        self.metrics
            .get(&(
                route_key(&inst.track.manifest.scope.repo, &inst.reference, &route.id),
                inst.track.manifest.track.version.clone(),
            ))
            .map(|m| m.live.read())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_tracks::*;

    fn track(version: &str) -> Track {
        Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "acme/work".into(),
                    version: version.into(),
                    description: String::new(),
                    provenance: "learned".into(),
                },
                scope: Scope { repo: "acme".into() },
                gauge: Gauge::default(),
            },
            routes: vec![Route {
                id: "verify".into(),
                summary: "Run tests".into(),
                trigger: Trigger::SessionStart,
                step: Step::Run { command: "make test".into() },
                slots: vec![],
                effect: adage_core::Effect::Verify,
                strength: Strength::Prime,
                state: RouteState::Firm,
                exit: vec![],
                evidence: Evidence::default(),
                authored: false,
            }],
        }
    }
    fn home() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        io::write(&paths.tracks().join("acme/work"), &track("0.2.0")).unwrap();
        (dir, paths)
    }
    fn directive(version: &str) -> Value {
        json!({ "route": "verify", "track": "acme/work", "version": version, "strength": "prime", "text": "Recorded response" })
    }
    fn append(paths: &Paths, sid: &str, ts: u64, decision: Value) {
        spool::append(&paths.sessions().join(format!("{sid}.spool")),
            &json!({ "ts": ts, "repo": "acme", "arm": "treated", "event": "PostToolBatch", "harness": "codex", "decision": decision })).unwrap();
    }
    fn graph(paths: &Paths, limit: usize) -> Value {
        crate::graph::build(paths, None, limit, &mut |w| panic!("{w}")).unwrap()
    }

    #[test]
    fn current_and_historical_versions_keep_separate_outcomes_and_links() {
        let (_dir, paths) = home();
        append(&paths, "old", 1, json!({ "directives": [directive("0.1.0")], "would_have": [directive("0.1.0")] }));
        append(&paths, "old", 2, json!({ "judgements": [{"kind":"followed", "route":"verify"}] }));
        append(&paths, "new", 3, json!({ "directives": [directive("0.2.0")] }));
        let g = graph(&paths, 400);
        let r = &g["routes"][0];
        assert_eq!(g["totals"]["emitted"], 2);
        assert_eq!(g["totals"]["would_have"], 0, "treated mirrors are not control exposures");
        assert_eq!(r["live"]["emitted"], 1);
        assert_eq!(r["live"]["followed"], 0);
        let h = &r["measurement_history"][0];
        assert_eq!(h["version"], "0.1.0");
        assert_eq!(h["live"]["followed"], 1);
        assert_eq!(h["measurement_sessions"][0]["applied"], json!([1]));
        assert_eq!(h["measurement_sessions"][0]["followed"], json!([2]));
        assert!(h["arms"]["treated"]["hit_avoided"].is_null());
        let t = trace(&paths, "old", "acme").unwrap().unwrap();
        assert_eq!(t["events"][0]["directives"][0]["text"], "Recorded response");
        assert_eq!(t["events"][1]["judgements"][0]["version"], "0.1.0");
        assert_eq!(t["events"][1]["judgements"][0]["exposure_sequence"], 1);
        assert!(crate::review::judgements(&paths).is_empty(), "historical outcomes cannot weaken the new version");
    }

    #[test]
    fn duplicate_installations_and_legacy_outcomes_are_never_copied_to_reflexes() {
        let (_dir, paths) = home();
        io::write(&paths.tracks().join("@imported/acme/work"), &track("0.2.0")).unwrap();
        append(&paths, "ambiguous", 1, json!({ "directives": [directive("0.2.0")] }));
        append(&paths, "ambiguous", 2, json!({ "judgements": [{"kind":"followed", "route":"verify"}] }));
        append(&paths, "legacy", 3, json!({ "directives": [{"route":"verify", "text":"Old response"}] }));
        append(&paths, "legacy", 4, json!({ "judgements": [{"kind":"derailed", "route":"verify"}] }));
        let g = graph(&paths, 400);
        for r in g["routes"].as_array().unwrap() {
            assert_eq!(r["live"]["emitted"], 0);
            assert_eq!(r["live"]["followed"], 0);
            assert_eq!(r["live"]["ignored"], 0);
            assert_eq!(r["measurement_info"]["status"], "ambiguous");
        }
        assert_eq!(g["totals"]["emitted"], 2);
        assert_eq!(g["totals"]["followed"], 1);
        assert_eq!(g["totals"]["ignored"], 1);
        assert_eq!(g["attribution"]["exposures"]["ambiguous"], 1);
        assert_eq!(g["attribution"]["exposures"]["legacy"], 1);
        assert!(crate::review::judgements(&paths).is_empty());
    }

    #[test]
    fn full_inventory_and_trace_outlive_the_recent_window() {
        let (_dir, paths) = home();
        for ts in 0..450 {
            append(&paths, "old", ts, json!({}));
        }
        for ts in 450..1050 {
            append(&paths, "new", ts, json!({ "observed": ["exec pwd"] }));
        }
        let g = graph(&paths, 400);
        let old = g["trace_sessions"].as_array().unwrap().iter().find(|s| s["session"] == "old").unwrap();
        assert_eq!(old["events_visible"], 0);
        assert_eq!(old["events_total"], 450);
        assert_eq!(g["trace_info"]["events_total"], 1050);
        let t = trace(&paths, "old", "acme").unwrap().unwrap();
        assert_eq!(t["complete"], true);
        assert_eq!(t["events_total"], 450);
        assert_eq!(t["events"][449]["sequence"], 450);
        assert!(trace(&paths, "old", "other").unwrap().is_none());
        assert!(trace(&paths, "../old", "acme").is_err());
    }

    #[test]
    fn repository_switches_and_tied_timestamps_keep_recorded_sequence() {
        let (_dir, paths) = home();
        append(&paths, "switch", 1, json!({ "directives": [directive("0.2.0")] }));
        spool::append(
            &paths.sessions().join("switch.spool"),
            &json!({ "ts": 1, "repo": "other", "arm": "treated", "decision": {"observed":["exec pwd"]} }),
        )
        .unwrap();
        append(&paths, "switch", 1, json!({ "judgements": [{"kind":"followed", "route":"verify"}] }));
        let g = graph(&paths, 400);
        assert_eq!(g["trace_sessions"].as_array().unwrap().len(), 2);
        let t = trace(&paths, "switch", "acme").unwrap().unwrap();
        assert_eq!(t["events_total"], 2);
        assert_eq!(t["events"][1]["sequence"], 3);
        assert_eq!(t["events"][1]["judgements"][0]["exposure_sequence"], 1);
    }

    #[test]
    fn an_unrouted_bootstrap_keeps_its_later_assignment_and_call_count() {
        let (_dir, paths) = home();
        spool::append(
            &paths.sessions().join("bootstrap.spool"),
            &json!({
                "ts": 1, "repo": "acme", "unrouted": true, "kind": "batch", "calls": 2,
            }),
        )
        .unwrap();
        append(&paths, "bootstrap", 2, json!({ "directives": [directive("0.2.0")] }));
        let g = graph(&paths, 400);
        assert_eq!(g["sessions"]["treated"], 1);
        assert_eq!(g["trace_sessions"][0]["arm"], "treated");
        assert_eq!(g["trace_events"][0]["unrouted"], true);
        assert_eq!(g["trace_events"][0]["calls"], 2);
        assert_eq!(g["trace_events"][0]["arm"], "unknown");
    }

    #[test]
    fn repeated_or_different_version_exposures_do_not_invent_causal_links() {
        let (_dir, paths) = home();
        for ts in [1, 2] {
            append(&paths, "repeated", ts, json!({ "directives": [directive("0.2.0")] }));
        }
        append(&paths, "repeated", 3, json!({ "judgements": [{"kind":"followed_failed", "route":"verify"}] }));
        let t = trace(&paths, "repeated", "acme").unwrap().unwrap();
        assert_eq!(t["events"][2]["judgements"][0]["kind"], "misfired");
        assert_eq!(t["events"][2]["judgements"][0]["attribution"], "current");
        assert!(t["events"][2]["judgements"][0]["exposure_sequence"].is_null());
        append(&paths, "repeated", 4, json!({ "directives": [directive("0.1.0")] }));
        append(&paths, "repeated", 5, json!({ "judgements": [{"kind":"ignored", "route":"verify"}] }));
        let g = graph(&paths, 400);
        assert_eq!(g["routes"][0]["live"]["misfired"], 1);
        assert_eq!(g["routes"][0]["live"]["ignored"], 0);
        assert_eq!(g["attribution"]["judgements"]["ambiguous"], 1);
    }

    #[test]
    fn pretool_and_legacy_control_records_keep_the_newer_runtime_contract() {
        let (_dir, paths) = home();
        let record = json!({ "ts":1, "repo":"acme", "arm":"holdout", "kind":"pretool", "withheld":true, "pretool":{"rewrite":"make test"} });
        spool::append(&paths.sessions().join("control.spool"), &record).unwrap();
        let g = graph(&paths, 400);
        assert_eq!(g["sessions"]["control"], 1);
        assert_eq!(g["events"][0]["kind"], "pretool");
        assert_eq!(g["events"][0]["arm"], "control");
        assert_eq!(g["pretool"][0]["withheld"], 1);
        assert_eq!(g["totals"]["emitted"], 0);
    }

    #[test]
    fn a_substitution_counts_for_the_reflex_the_session_was_exposed_to() {
        let (_dir, paths) = home();
        let sub = json!({ "route": "verify", "track": "acme/work", "command": "make check", "context": "c" });
        append(&paths, "treated", 1, json!({ "directives": [directive("0.2.0")] }));
        append(
            &paths,
            "treated",
            2,
            json!({ "judgements": [{ "kind": "substituted", "route": "verify" }], "substituted": sub, "would_substitute": sub }),
        );
        let control = |sid: &str, ts: u64, decision: Value| {
            let record = json!({ "ts": ts, "repo": "acme", "arm": "control", "event": "PreToolUse", "kind": "pretool", "decision": decision });
            spool::append(&paths.sessions().join(format!("{sid}.spool")), &record).unwrap();
        };
        control("control", 3, json!({ "would_have": [directive("0.2.0")] }));
        control("control", 4, json!({ "would_substitute": sub }));
        // Never exposed in its session: nothing ties it to the installed reflex.
        control("unexposed", 5, json!({ "would_substitute": sub }));
        let g = graph(&paths, 400);
        let r = &g["routes"][0];
        assert_eq!(
            (r["arms"]["treated"]["substituted"].clone(), r["arms"]["control"]["substituted"].clone()),
            (json!(1), json!(1))
        );
        assert_eq!(r["arms"]["treated"]["followed"], 0, "a substitution is not a follow");
        let events = g["events"].as_array().unwrap();
        let named = |field: &str, arm: &str| events.iter().filter(|e| e[field] == "verify" && e["arm"] == arm).count();
        assert_eq!((named("substituted", "treated"), named("would_substitute", "treated")), (1, 0));
        assert_eq!(named("would_substitute", "control"), 2, "both would-be substitutions stay in the trace");
    }

    #[cfg(unix)]
    #[test]
    fn selected_traces_never_follow_a_spool_symlink() {
        let (_dir, paths) = home();
        append(&paths, "real", 1, json!({}));
        std::os::unix::fs::symlink(paths.sessions().join("real.spool"), paths.sessions().join("alias.spool")).unwrap();
        assert!(trace(&paths, "alias", "acme").unwrap().is_none());
        assert_eq!(graph(&paths, 400)["sessions"]["total"], 1);
    }
}
