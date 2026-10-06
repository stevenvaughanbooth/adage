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
use adage_runtime::command_key;
use adage_store::Paths;
use adage_tracks::{Index, Route, Step};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

mod measurements;
pub(crate) use measurements::current_judgements;
pub use measurements::trace;

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
        // A skill's use is keyed as the trace keys it (`control Skill <name>`).
        Step::Skill { skill, .. } => vec![(adage_tracks::skill_key(skill), "directs")],
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

    let scan = measurements::read(paths, &installed, max_events, warn);
    let observed = &scan.observed;
    // What a person could be asked to grant (`offers`): every reflex's grants, and the policy a
    // first grant would run under (a machine at E1 is raised with its first action).
    let grants = adage_exec::approvals::Store::load(&adage_exec::approvals::path(&paths.root)).unwrap_or_default();
    let trial = adage_exec::policy::trial_e2(&paths.root).ok().flatten();
    let mut railed: BTreeMap<String, (String, &'static str)> = BTreeMap::new();
    let mut routes = Vec::new();
    for inst in &installed {
        let t = &inst.track;
        let repo = &t.manifest.scope.repo;
        // Why each learned reflex exists (`.why.json`): what its strength was decided on.
        let whys = adage_tracks::why::load(&inst.dir);
        for r in &t.routes {
            let indexed = index.routes.iter().any(|x| {
                &x.repo == repo
                    && x.route.id == r.id
                    && x.track == t.manifest.track.name
                    && x.version == t.manifest.track.version
            });
            for (k, how) in route_keys(r, table) {
                railed.entry(k).or_insert((r.id.clone(), how));
            }
            let standing = standings.get(&(inst.reference.clone(), r.id.clone()));
            let (verbs, reasons) = standing.map(|s| (s.verbs.clone(), s.reasons.as_slice())).unwrap_or_default();
            let mut route = serde_json::to_value(Reflex::of(inst, r, whys.get(&r.id))).unwrap_or_default();
            let mut decisions = Decisions::of(&inst.reference, r, verbs, reasons);
            let label = format!("{}/{}", t.manifest.track.name, r.id);
            let granted = grants.approvals.iter().any(|a| a.route == label && (repo.is_empty() || a.key.repo == *repo));
            let permitted = trial.as_ref().is_some_and(|p| !p.effects_deny.contains(&r.effect));
            decisions.offers.extend(crate::reflex::Offer::of(&inst.reference, r, granted, permitted));
            route["review"] = json!(decisions);
            route["in_index"] = json!(indexed);
            let measurements = scan.measurement(inst, r);
            for (key, value) in measurements.as_object().into_iter().flatten() {
                route[key] = value.clone();
            }
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
    repos.extend(scan.pretool.keys().cloned());
    repos.extend(scan.arm_sessions.keys().map(|(r, _)| r.clone()));
    repos.remove("");
    let savings: Vec<Value> = repos.iter().map(|r| savings_json(paths, r)).collect();
    let marker = paths.rewrite_off().exists();
    let pretool: Vec<Value> = scan
        .pretool
        .iter()
        .map(
            |(repo, p)| json!({ "repo": repo, "rewritten": p.rewritten, "advised": p.advised, "withheld": p.withheld }),
        )
        .collect();

    let out = json!({
        "generation": index.generation,
        "home": paths.root.display().to_string(),
        "sessions": { "total": scan.spools.len(), "treated": scan.treated, "control": scan.control },
        "stop_requests": scan.stops,
        "routes": routes,
        "observed": observed,
        "events": scan.events,
        "totals": scan.totals.read(),
        "attribution": { "exposures": scan.attribution_counts, "judgements": scan.judgement_attribution,
            "description": "Raw totals count each recorded decision once. Reflex measurements require an unambiguous collection and version; unidentified legacy records and duplicate identities are not assigned to a current reflex." },
        "trace_events": scan.trace_events,
        "trace_sessions": scan.trace_sessions,
        "trace_info": { "source": "recorded_hook_decisions", "events_total": scan.trace_events_total,
            "events_visible": scan.trace_events.len(), "truncated": scan.trace_events_total > scan.trace_events.len(),
            "limit": max_events.min(1000), "complete_session_inventory": true, "session_endpoint": "/api/v1/trace",
            "description": "Recorded hook decisions and normalized actions, not raw prompts, tool outputs or private reasoning. Hooks that cannot change a decision may be skipped; this is not a complete agent transcript." },
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

/// Current attributable outcomes, keyed by `(installed reference, route id)`, exactly as the map.
pub fn live(paths: &Paths) -> BTreeMap<(String, String), Live> {
    let Ok((installed, _)) = discover(&paths.tracks()) else {
        return BTreeMap::new();
    };
    let scan = measurements::read(paths, &installed, 0, &mut |_| {});
    let mut result = BTreeMap::new();
    for inst in &installed {
        for route in &inst.track.routes {
            result.insert((inst.reference.clone(), route.id.clone()), scan.live(inst, route));
        }
    }
    result
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

/// The fields the Claude Code mod's records add to a timeline event, each only when its record
/// says it: the step Adage made in the agent's place or would have (`predicted`,
/// `would_predict`), the dead end replaced or that would have been (`substituted`,
/// `would_substitute`), the candidates a batch noticed (`noticed`, learning's count and never a
/// directive), and the Bash call the mod answered with a fresh receipt instead of running it
/// (`answered`). A map without the mod reads as before.
pub(crate) const MOD_FIELDS: [&str; 6] =
    ["predicted", "would_predict", "substituted", "would_substitute", "noticed", "answered"];

/// [`MOD_FIELDS`] of one spool record, as route ids (`noticed`: candidate ids).
pub(crate) fn mod_fields(r: &Value) -> serde_json::Map<String, Value> {
    let d = &r["decision"];
    let substituted = d["substituted"]["route"].as_str();
    let routes = [
        ("predicted", d["predicted"]["route"].as_str()),
        ("would_predict", d["would_predict"]["route"].as_str()),
        ("substituted", substituted),
        // In the treated arm a substitution records both; the event names what happened.
        ("would_substitute", d["would_substitute"]["route"].as_str().filter(|_| substituted.is_none())),
        ("answered", r["answered"].as_str().filter(|_| r["kind"] == "pretool")),
    ];
    let mut out: serde_json::Map<String, Value> =
        routes.into_iter().filter_map(|(k, v)| Some((k.to_string(), json!(v?)))).collect();
    let noticed: Vec<&str> =
        d["noticed"].as_array().into_iter().flatten().filter_map(|n| n["candidate"].as_str()).collect();
    if !noticed.is_empty() {
        out.insert("noticed".into(), json!(noticed));
    }
    out
}

/// One spool record as a timeline event (`None` when nothing happened in it): the routes it
/// directed, would have directed (control, or over a limit), the judgements it made as
/// `[kind, route]`, the command keys it observed, whether it asked the agent to continue, and the
/// mod's [`MOD_FIELDS`]. The map's live stream sends it; the graph's `events` are the fuller
/// recorded form (`measurements`), with the same fields.
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
    let extra = mod_fields(r);
    if directed.is_empty() && would.is_empty() && judged.is_empty() && keys.is_empty() && !cont && extra.is_empty() {
        return None;
    }
    let mut e = json!({
        "ts": r["ts"], "session": sid, "arm": r["arm"].as_str().map(arm_word), "event": r["event"], "repo": repo,
        "directed": directed, "would": would, "judged": judged, "observed": keys, "continue": cont,
        // A streaming event lacks prior session exposures, so preserve the actual identity
        // and text without inventing an installed route key or a causal outcome link.
        "directives": d["directives"].as_array().cloned().unwrap_or_default(),
        "would_have": d["would_have"].as_array().cloned().unwrap_or_default(),
    });
    if let Some(e) = e.as_object_mut() {
        e.extend(extra);
    }
    Some(e)
}

/// The most recent live-learning runs from `learn.log` (JSON lines), oldest first: when each ran,
/// for which repository, and what changed.
fn learning(paths: &Paths) -> Vec<Value> {
    use std::io::{Read, Seek, SeekFrom};
    const TAIL: u64 = 64 << 10;
    let Ok(mut f) = std::fs::File::open(paths.learn_log()) else {
        return vec![];
    };
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
    fn streamed_directives_keep_their_exact_recorded_identity_and_response() {
        let directive = serde_json::json!({ "route":"test", "track":"acme/work", "version":"0.2.0", "strength":"prime", "text":"The recorded response" });
        let record = serde_json::json!({ "arm":"treated", "decision":{"directives":[directive.clone()], "would_have":[directive.clone()]} });
        let event = super::event("acme", "s1", &record).unwrap();
        assert_eq!(event["directives"][0], directive);
        assert_eq!(event["would_have"][0], directive);
        assert!(event["directives"][0].get("route_key").is_none());
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
