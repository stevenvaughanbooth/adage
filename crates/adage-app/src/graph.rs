//! The map's read model: Adage's live state as one graph, for people and tools (`adage graph
//! --json`, `adage map`'s `/api/v1/graph`, the TUI's reflex list).
//!
//! Judgements are split by arm. In treated sessions they say whether the agent followed what it
//! was told; in control sessions nothing was delivered, so they say whether the agent found the
//! same path unaided — the baseline every effect is measured against.
//!
//! Nodes are the situations Adage watches for (route triggers), the routes, the steps they
//! direct, and how agents responded. Every count comes from the installed tracks and the session
//! spools under `$ADAGE_HOME`; nothing is estimated. Read-only. The JSON is what the web map draws
//! (`web/map/`); a contract test pins it to the web map's fixtures.
//!
//! Spools written before the lexicon's rename spell the control arm `holdout` and an ignored
//! directive `derailed` (`followed_failed` for misfired); they are read as the new words.

pub use crate::reflex::trigger;
use crate::reflex::{Control, Decisions, Live, Reflex};
use crate::review::{discover, standings};
use adage_core::{pattern, CommandTable};
use adage_runtime::{command_key, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::{Index, Route, Step};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MAX_OBSERVED: usize = 18;
const MAX_LEARNING: usize = 60;

/// What a reflex did in sessions, while the spools are read ([`crate::reflex::Live`] once read).
#[derive(Default)]
struct Tally {
    emitted: u64,
    would_have: u64,
    followed: u64,
    ignored: u64,
    misfired: u64,
    obligation_met: u64,
    /// Control sessions: nothing was delivered, so a "followed" judgement means the agent took the
    /// route's path unaided, and a "ignored" one means it did not.
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
    /// Sessions where the dead end was replaced before it ran (control: would have been).
    substituted: BTreeSet<String>,
}

type ArmMap = BTreeMap<(String, String, String), ArmSets>;

fn sets<'a>(m: &'a mut ArmMap, repo: &str, id: &str, arm: &str) -> &'a mut ArmSets {
    m.entry((repo.to_string(), id.to_string(), arm.to_string())).or_default()
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
        Step::Sequence { commands } => commands.iter().map(|c| (command_key(c, table), "directs")).collect(),
        Step::Card { card } => card.commands.iter().map(|c| (command_key(&c.command, table), "directs")).collect(),
        Step::ReadBatch { .. } | Step::Advise { .. } => vec![],
    }
}

/// The graph as JSON, with at most `max_events` recent events. `here` is the checkout the person
/// is in (execution grants are per checkout); each installed track that cannot be read is skipped
/// and named to `warn`.
pub fn build(paths: &Paths, here: Option<&Path>, max_events: usize, warn: &mut dyn FnMut(String)) -> Result<Value> {
    let table = CommandTable::load();
    let (installed, warnings) = discover(&paths.tracks())?;
    warnings.into_iter().for_each(&mut *warn);
    let index = Index::load(&paths.index());
    // The decisions a person can make about each route; the map offers them as commands to copy.
    let standings = standings(paths, here).unwrap_or_default();

    // Routes, keyed by (repo, route id) as the runtime identifies them.
    let mut live: BTreeMap<(String, String), Tally> = BTreeMap::new();
    let mut observed: BTreeMap<String, (u64, BTreeSet<String>)> = BTreeMap::new();
    let mut events: Vec<Value> = Vec::new();
    let (mut treated, mut control, mut stops) = (0u64, 0u64, 0u64);

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
    // PreToolUse records per repository: commands rewritten to the project's environment, advice
    // given instead (a permission rule, `pip install`), and either withheld in the control arm.
    let mut pretool: BTreeMap<String, Pretool> = BTreeMap::new();

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
        match recs.iter().find_map(|r| r["arm"].as_str()).map(arm_word) {
            Some("treated") => treated += 1,
            Some("control") => control += 1,
            _ => {}
        }
        for r in &recs {
            if r["kind"] == "pretool" {
                let p = pretool.entry(repo.clone()).or_default();
                let what = &r["pretool"];
                if r["withheld"] == true {
                    p.withheld += 1;
                } else if what["rewrite"].is_string() {
                    p.rewritten += 1;
                } else if what["advise"].is_string() {
                    p.advised += 1;
                }
            }
            let d = &r["decision"];
            let ids = |k: &str| -> Vec<String> {
                d[k].as_array().into_iter().flatten().filter_map(|x| x["route"].as_str().map(String::from)).collect()
            };
            let (directed, would) = (ids("directives"), ids("would_have"));
            tally(&mut live, &repo, &sid, r);
            let arm = r["arm"].as_str().map(arm_word).unwrap_or("unknown").to_string();
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
                    (Some("substituted"), Some(id)) => {
                        sets(&mut arms, &repo, id, &arm).substituted.insert(sid.clone());
                    }
                    _ => {}
                }
            }
            if let (Some(id), None) = (d["would_substitute"]["route"].as_str(), d["substituted"].as_object()) {
                sets(&mut arms, &repo, id, &arm).substituted.insert(sid.clone());
            }
            for k in d["observed"].as_array().into_iter().flatten().filter_map(|k| k.as_str()) {
                let k = k.trim_end_matches(" !failed");
                for (rrepo, id, keys) in &avoided {
                    if *rrepo == repo && keys.iter().any(|runner| pattern::runs_avoided(runner, k)) {
                        sets(&mut arms, &repo, id, &arm).hit_avoided.insert(sid.clone());
                    }
                }
            }
            for k in d["observed"].as_array().into_iter().flatten().filter_map(|k| k.as_str()) {
                let e = observed.entry(k.to_string()).or_default();
                e.0 += 1;
                e.1.insert(sid.clone());
            }
            stops += d["continue_reason"].is_string() as u64;
            events.extend(event(&repo, &sid, r));
        }
    }
    events.sort_by_key(|e| e["ts"].as_u64().unwrap_or(0));
    let events: Vec<Value> = events.split_off(events.len().saturating_sub(max_events));

    let mut railed: BTreeMap<String, (String, &'static str)> = BTreeMap::new();
    let mut routes = Vec::new();
    for inst in &installed {
        let t = &inst.track;
        let repo = &t.manifest.scope.repo;
        // Why each learned reflex exists (`.why.json`): what its strength was decided on.
        let whys = adage_tracks::why::load(&inst.dir);
        for r in &t.routes {
            let indexed =
                index.routes.iter().any(|x| &x.repo == repo && x.route.id == r.id && x.track == t.manifest.track.name);
            for (k, how) in route_keys(r, &table) {
                railed.entry(k).or_insert((r.id.clone(), how));
            }
            let arm_stats: serde_json::Map<String, Value> = ["treated", "control"]
                .iter()
                .map(|a| {
                    let s = arms.get(&(repo.clone(), r.id.clone(), a.to_string()));
                    let n = |f: fn(&ArmSets) -> &BTreeSet<String>| s.map_or(0, |s| f(s).len());
                    let mut json = json!({
                        "sessions": arm_sessions.get(&(repo.clone(), a.to_string())).map_or(0, |s| s.len()),
                        "applied": n(|s| &s.applied),
                        "followed": n(|s| &s.followed),
                        "met": n(|s| &s.met),
                        // Only sessions the route applied to: before it was learned, it could not help.
                        "hit_avoided": s.map_or(0, |s| s.hit_avoided.intersection(&s.applied).count()),
                    });
                    // A dead end replaced before it ran is avoided, not hit, and not a follow. Said
                    // only where it happened, so a map without the mod reads as before.
                    if n(|s| &s.substituted) > 0 {
                        json["substituted"] = json!(n(|s| &s.substituted));
                    }
                    (a.to_string(), json)
                })
                .collect();
            let standing = standings.get(&(inst.reference.clone(), r.id.clone()));
            let (verbs, reasons) = standing.map(|s| (s.verbs.clone(), s.reasons.as_slice())).unwrap_or_default();
            let mut route = serde_json::to_value(Reflex::of(inst, r, whys.get(&r.id))).unwrap_or_default();
            route["review"] = json!(Decisions::of(&inst.reference, r, verbs, reasons));
            route["in_index"] = json!(indexed);
            route["live"] = json!(live.get(&(repo.clone(), r.id.clone())).map(Tally::read).unwrap_or_default());
            route["arms"] = Value::Object(arm_stats);
            routes.push(route);
        }
    }

    // The bars this home conditions with (`learn.thresholds`), so the map measures evidence
    // against the bar that actually set each strength.
    let config = adage_store::config::load(paths).config;
    let bar = adage_learn::LearnConfig::for_thresholds(config.learn_thresholds);
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

    // The savings estimate per repository, as `adage savings` and the status card state it.
    let mut repos: BTreeSet<String> = installed.iter().map(|i| i.track.manifest.scope.repo.clone()).collect();
    repos.extend(pretool.keys().cloned());
    repos.extend(arm_sessions.keys().map(|(r, _)| r.clone()));
    repos.remove("");
    let savings: Vec<Value> = repos.iter().map(|r| savings_json(paths, r)).collect();
    let marker = adage_hook::rewrite_off(paths).exists();
    let pretool: Vec<Value> = pretool
        .iter()
        .map(
            |(repo, p)| json!({ "repo": repo, "rewritten": p.rewritten, "advised": p.advised, "withheld": p.withheld }),
        )
        .collect();

    let out = json!({
        "generation": index.generation,
        "home": paths.root.display().to_string(),
        "sessions": { "total": spools.len(), "treated": treated, "control": control },
        "stop_requests": stops,
        "routes": routes,
        "observed": observed,
        "events": events,
        "learning": learning(paths),
        "bars": { "min_support": bar.min_support, "fire_lower_bound": bar.fire_lower_bound, "uplift_min": bar.uplift_min },
        "savings": savings,
        "pretool": pretool,
        "runtime": {
            // Rewrites to the project's environment: on unless `runtime.rewrite` is false or the
            // marker `delegate off` wrote before the switch existed is there.
            "rewrite": { "on": config.runtime_rewrite && !marker, "config": config.runtime_rewrite, "marker": marker },
            "control": config.runtime_control,
            "card_threshold": crate::reflex::round(config.learn_card_threshold),
        },
    });
    Ok(out)
}

impl Tally {
    fn read(&self) -> Live {
        Live {
            emitted: self.emitted,
            would_have: self.would_have,
            followed: self.followed,
            ignored: self.ignored,
            misfired: self.misfired,
            obligation_met: self.obligation_met,
            control: Control {
                unaided: self.unaided,
                not_reached: self.not_reached,
                obligation_met: self.obligation_met_unaided,
            },
            sessions: self.sessions.len(),
        }
    }
}

/// Count one spool record of session `sid` in `repo` into the reflexes it directed, would have
/// directed, and was judged on.
fn tally(live: &mut BTreeMap<(String, String), Tally>, repo: &str, sid: &str, r: &Value) {
    let d = &r["decision"];
    let ids = |k: &str| -> Vec<String> {
        d[k].as_array().into_iter().flatten().filter_map(|x| x["route"].as_str().map(String::from)).collect()
    };
    for id in ids("directives") {
        let l = live.entry((repo.to_string(), id)).or_default();
        l.emitted += 1;
        l.sessions.insert(sid.to_string());
    }
    for id in ids("would_have") {
        live.entry((repo.to_string(), id)).or_default().would_have += 1;
    }
    let control = r["arm"].as_str().map(arm_word) == Some("control");
    for j in d["judgements"].as_array().into_iter().flatten() {
        let (Some(kind), Some(id)) = (j["kind"].as_str(), j["route"].as_str()) else { continue };
        let l = live.entry((repo.to_string(), id.to_string())).or_default();
        match (judgement_word(kind), control) {
            ("followed", false) => l.followed += 1,
            ("ignored", false) => l.ignored += 1,
            ("misfired", false) => l.misfired += 1,
            ("obligation_met", false) => l.obligation_met += 1,
            ("followed", true) => l.unaided += 1,
            ("ignored", true) => l.not_reached += 1,
            ("obligation_met", true) => l.obligation_met_unaided += 1,
            _ => {}
        }
    }
}

/// What each reflex did in sessions, by `(repo, route id)`, counted as the map counts it.
pub fn live(paths: &Paths) -> BTreeMap<(String, String), Live> {
    let mut live: BTreeMap<(String, String), Tally> = BTreeMap::new();
    let mut spools: Vec<PathBuf> = std::fs::read_dir(paths.sessions())
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "spool")).collect())
        .unwrap_or_default();
    spools.sort();
    for p in &spools {
        let st: SessionState = state::read(&p.with_extension("state")).unwrap_or_default();
        let repo = st.repo.clone().unwrap_or_default();
        let sid: String = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        for r in spool::read_all(p).unwrap_or_default().iter() {
            tally(&mut live, &repo, &sid, r);
        }
    }
    live.into_iter().map(|(k, t)| (k, t.read())).collect()
}

#[derive(Default)]
struct Pretool {
    rewritten: u64,
    advised: u64,
    withheld: u64,
}

/// One repository's savings summary (`crate::savings`), with the lines every surface prints. The
/// summary `adage savings` last wrote when it is newer than the ledger, else computed here and
/// not written (the graph is read-only). Dollars only past the gate (10 control, 30 treated
/// sessions), always with the 95% interval.
fn savings_json(paths: &Paths, repo: &str) -> Value {
    use crate::savings;
    let l = adage_learn::ledger::Ledger::new(&paths.ledger(), repo);
    let mtime = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let fresh = match (mtime(&l.summary_path()), mtime(&l.path())) {
        (Some(s), Some(g)) => s >= g,
        _ => false,
    };
    let cached: Option<savings::Summary> =
        if fresh { std::fs::read(l.summary_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()) } else { None };
    let s = cached.unwrap_or_else(|| {
        let ledger = l.read();
        let now = crate::savings::now_ms();
        let (units, counts) = savings::gather(paths, repo, &ledger, now);
        let mut s = savings::summarise(repo, &units, counts, now);
        s.unpriced = ledger.values().filter(|x| x.cost.usd.is_none()).count();
        s
    });
    let mut v = serde_json::to_value(&s).unwrap_or(Value::Null);
    v["need"] = json!({ "control": savings::NEED_CONTROL, "treated": savings::NEED_TREATED });
    v["lines"] = json!(savings::lines(&s));
    v["guardrail"] = json!(savings::guardrail(&s));
    v
}

/// One spool record as a timeline event (`None` when nothing happened in it): the routes it
/// directed, would have directed (control, or over a limit), the judgements it made as
/// `[kind, route]`, the command keys it observed, and whether it asked the agent to continue. The
/// same shape in the graph's `events` and in the map's live stream.
pub fn event(repo: &str, sid: &str, r: &Value) -> Option<Value> {
    // A PreToolUse record (`kind: "pretool"`) has no decision: it says what the hook rewrote or
    // advised before a command ran (`pretool`), or would have in a control session (`withheld`).
    // A substitution's record (the mod's verdict) carries a decision instead, read below.
    if r["kind"] == "pretool" && r.get("decision").is_none() {
        let pretool = r.get("pretool").filter(|p| p.is_object())?;
        return Some(json!({
            "ts": r["ts"], "session": sid, "arm": r["arm"].as_str().map(arm_word), "event": r["event"],
            "repo": repo, "kind": "pretool", "pretool": pretool, "withheld": r["withheld"] == true,
            "directed": [], "would": [], "judged": [], "observed": [], "continue": false,
        }));
    }
    let d = &r["decision"];
    let ids = |k: &str| -> Vec<String> {
        d[k].as_array().into_iter().flatten().filter_map(|x| x["route"].as_str().map(String::from)).collect()
    };
    let (directed, would) = (ids("directives"), ids("would_have"));
    let judged: Vec<Value> = d["judgements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|j| Some(json!([judgement_word(j["kind"].as_str()?), j["route"].as_str()?])))
        .collect();
    let keys: Vec<String> =
        d["observed"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect();
    let cont = d["continue_reason"].is_string();
    // A `turn_step` record: the step Adage made in the agent's place, or would have.
    let predicted = d["predicted"]["route"].as_str();
    let would_predict = d["would_predict"]["route"].as_str();
    // A `pretool` record of the mod's verdict: the dead end replaced, or that would have been.
    let substituted = d["substituted"]["route"].as_str();
    let would_substitute = d["would_substitute"]["route"].as_str().filter(|_| substituted.is_none());
    if directed.is_empty()
        && would.is_empty()
        && judged.is_empty()
        && keys.is_empty()
        && !cont
        && predicted.is_none()
        && would_predict.is_none()
        && would_substitute.is_none()
    {
        return None;
    }
    let mut e = json!({
        "ts": r["ts"], "session": sid, "arm": r["arm"].as_str().map(arm_word), "event": r["event"], "repo": repo,
        "directed": directed, "would": would, "judged": judged, "observed": keys, "continue": cont,
    });
    if let Some(p) = predicted {
        e["predicted"] = json!(p);
    }
    if let Some(p) = would_predict {
        e["would_predict"] = json!(p);
    }
    if let Some(p) = substituted {
        e["substituted"] = json!(p);
    }
    if let Some(p) = would_substitute {
        e["would_substitute"] = json!(p);
    }
    Some(e)
}

/// The most recent live-learning runs from `learn.log` (JSON lines), oldest first: when each ran,
/// for which repository, and what changed.
fn learning(paths: &Paths) -> Vec<Value> {
    use std::io::{Read, Seek, SeekFrom};
    const TAIL: u64 = 64 << 10;
    let Ok(mut f) = std::fs::File::open(paths.learn_log()) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(TAIL);
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(start)).is_err() || f.take(TAIL).read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // mid-line
    }
    let parsed: Vec<Value> = lines.iter().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect();
    parsed[parsed.len().saturating_sub(MAX_LEARNING)..].to_vec()
}

/// Whether a route's command key covers an observed key, as the runtime judges it
/// (`adage_core::pattern::follows`): a directed command is followed by the same command with
/// options and operands added (`wi-lookup {item}` covers `wi-lookup {id} --json`, slots matching
/// slots; `.venv/bin/python -m pytest` covers `.venv/bin/python -m pytest {path} -x`); an avoided runner
/// covers any run of its program and subcommand (`python3 -m pytest` covers
/// `python3 -m pytest {path} -v`).
fn speaks_for(route_key: &str, how: &str, observed: &str) -> bool {
    if how == "avoids" {
        pattern::CommandPattern::exact(route_key)
            .map(|p| p.at_least(pattern::Level::HeadSub))
            .is_some_and(|p| p.matches_key(observed))
    } else {
        pattern::follows(route_key, observed)
    }
}

/// A spool's arm in the lexicon's words: older spools say `holdout` for `control`.
fn arm_word(arm: &str) -> &str {
    if arm == "holdout" {
        "control"
    } else {
        arm
    }
}

/// A judgement kind in the lexicon's words: older spools say `derailed` and `followed_failed`.
fn judgement_word(kind: &str) -> &str {
    match kind {
        "derailed" => "ignored",
        "followed_failed" => "misfired",
        k => k,
    }
}
#[cfg(test)]
mod tests {
    use super::{arm_word, judgement_word, speaks_for};

    #[test]
    fn pretool_records_reach_the_stream() {
        let r = serde_json::json!({ "ts": 5, "event": "PreToolUse", "kind": "pretool", "arm": "holdout",
            "withheld": true, "pretool": { "rewrite": "test", "interpreter": ".venv/bin/python" } });
        let e = super::event("acme", "s1", &r).expect("an event");
        assert_eq!(e["kind"], "pretool");
        assert_eq!(e["arm"], "control");
        assert_eq!(e["withheld"], true);
        assert_eq!(e["pretool"]["rewrite"], "test");
        assert!(super::event("acme", "s1", &serde_json::json!({ "kind": "pretool" })).is_none());
        // The mod's verdict: a substitution is a judgement of its own, never a follow.
        let sub = serde_json::json!({ "route": "dead-end", "track": "t", "command": "make test", "context": "c" });
        let r = serde_json::json!({ "ts": 6, "event": "PreToolUse", "kind": "pretool", "arm": "treated", "substituted": true,
            "decision": { "judgements": [{ "kind": "substituted", "route": "dead-end" }], "substituted": sub, "would_substitute": sub } });
        let e = super::event("acme", "s1", &r).unwrap();
        assert_eq!(
            (e["judged"].clone(), e["substituted"].clone()),
            (serde_json::json!([["substituted", "dead-end"]]), serde_json::json!("dead-end"))
        );
        assert!(e.get("would_substitute").is_none());
        let r = serde_json::json!({ "ts": 7, "event": "PreToolUse", "kind": "pretool", "arm": "control", "withheld": true,
            "would_substitute": true, "decision": { "judgements": [], "would_substitute": sub } });
        assert_eq!(super::event("acme", "s2", &r).unwrap()["would_substitute"], "dead-end");
    }

    #[test]
    fn old_spool_words_read_as_new_ones() {
        assert_eq!(arm_word("holdout"), "control");
        assert_eq!(arm_word("treated"), "treated");
        assert_eq!(judgement_word("derailed"), "ignored");
        assert_eq!(judgement_word("followed_failed"), "misfired");
        assert_eq!(judgement_word("followed"), "followed");
    }

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
        // The whole suite covers a narrowed run (the action classifier: `adage_core::action::satisfies`).
        assert!(d("exec python3 -m pytest {path}", "exec python3 -m pytest"));
        // Followed: the directed runner narrowed to a path, as agents run it.
        assert!(d("exec .venv/bin/python -m pytest", "exec .venv/bin/python -m pytest {path} -xvs"));
        assert!(!d("exec .venv/bin/python -m pytest", "exec python -m pytest {path}"));
    }

    /// The `/api/v1/graph` contract: what `adage map` serves has the fields the web map's
    /// fixtures (`web/map/e2e/fixtures/busy.json`, written by `make.mjs` to the served shape) give
    /// it, at the top level and on every reflex. A field added, renamed or removed on either side
    /// fails here, so the server and the UI cannot drift apart unnoticed.
    #[test]
    fn the_served_graph_has_the_fields_the_web_map_is_tested_with() {
        use adage_tracks::*;
        let d = tempfile::tempdir().unwrap();
        let paths = adage_store::Paths::at(d.path());
        let route = Route {
            id: "verify".into(),
            summary: "run the tests".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: adage_core::Effect::Verify,
            strength: Strength::Prime,
            state: RouteState::Firm,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence { support: 5, opportunities: 6, days: 2, ..Default::default() },
            authored: false,
        };
        let track = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "acme/learned".into(),
                    version: "0.1.0".into(),
                    description: "t".into(),
                    provenance: "learned".into(),
                },
                scope: Scope { repo: "acme".into() },
                gauge: Gauge::default(),
            },
            routes: vec![route],
        };
        io::write(&paths.tracks().join("acme/learned"), &track).unwrap();
        Index::compile(&[track]).write_atomic(&paths.index()).unwrap();
        let served = super::build(&paths, None, 10, &mut |w| panic!("{w}")).unwrap();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../web/map/e2e/fixtures/busy.json")).unwrap();
        let keys = |v: &serde_json::Value| -> std::collections::BTreeSet<String> {
            v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
        };
        assert_eq!(keys(&served), keys(&fixture), "top-level fields of /api/v1/graph");
        let (ours, theirs) = (&served["routes"][0], &fixture["routes"][0]);
        assert!(ours.is_object() && theirs.is_object(), "a reflex on each side");
        assert_eq!(keys(ours), keys(theirs), "fields of a reflex in /api/v1/graph");
    }
}
