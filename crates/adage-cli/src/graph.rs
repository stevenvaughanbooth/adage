//! `adage graph [--json] [--events <n>]`: Adage's live state as a graph, for people and tools.
//!
//! Judgements are split by arm. In treated sessions they say whether the agent followed what it
//! was told; in control sessions nothing was delivered, so they say whether the agent found the
//! same path unaided — the baseline every effect is measured against.
//!
//! Nodes are the situations Adage watches for (route triggers), the routes, the steps they
//! direct, and how agents responded. Every count comes from the installed tracks and the session
//! spools under `$ADAGE_HOME`; nothing is estimated. Read-only, so agents may run it.
//!
//! The JSON is what the map draws (`web/map/`); `adage map` serves both from the binary (see
//! `map/`), at `/api/v1/graph`, and streams new decisions shaped as [`event`] records. Any other
//! option is refused: `--serve` is gone (no aliases).
//!
//! Spools written before the lexicon's rename spell the control arm `holdout` and an ignored
//! directive `derailed` (`followed_failed` for misfired); they are read as the new words.

use crate::review::{ceiling_str, discover, origin_label, preview, standings};
use adage_core::{pattern, CommandTable};
use adage_runtime::{command_key, span_kind_label, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::{index, Index, Route, Step, Trigger};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const MAX_OBSERVED: usize = 18;
const MAX_LEARNING: usize = 60;

#[derive(Default)]
struct Live {
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
        Trigger::Context { items, .. } => ("context", format!("a context of {} events", items.len())),
    };
    let mut v = json!({ "kind": kind, "label": label });
    match t {
        // How close a prompt must be to the family (cosine), what a card's grey zone is measured from.
        Trigger::Intent { family, threshold, .. } => {
            v["family"] = json!(family);
            v["threshold"] = json!(round(f64::from(*threshold)));
        }
        // The events, in time order, each within `within` of the one before.
        Trigger::Context { items, within } => {
            v["within"] = json!(within);
            v["items"] = items.iter().map(ctx_item).collect();
        }
        _ => {}
    }
    v
}

/// One event of a context trigger, as `{kind, label}`.
fn ctx_item(i: &adage_tracks::CtxItem) -> Value {
    use adage_tracks::CtxItem;
    let first =
        |keys: &[String]| keys.first().map(|k| k.strip_prefix("exec ").unwrap_or(k).to_string()).unwrap_or_default();
    let (kind, label) = match i {
        CtxItem::Act { keys } => ("act", format!("`{}` succeeds", first(keys))),
        CtxItem::Fail { keys, signature } => ("fail", format!("`{}` fails: {signature}", first(keys))),
        CtxItem::Edit { glob } => ("edit", format!("an edit touches {glob}")),
        CtxItem::PromptValue { .. } => ("prompt_value", "a prompt names a known value".to_string()),
        CtxItem::Intent { family, .. } => ("intent", format!("a prompt means: {family}")),
    };
    json!({ "kind": kind, "label": label })
}

/// Three decimals: thresholds and posteriors as people compare them.
fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// A procedure card's contents (`adage_tracks::card`): files in order, the commands that worked
/// (with their replay verdict: `verified` true, false, or null when not replayed), the commands
/// that mostly failed, and the finishing check. Delivered once a session at a prompt at least
/// `threshold` close to the family, or, from `grey_from` up, at the first edit of one of its
/// files. It never travels in a pack.
fn card_json(card: &adage_tracks::card::Card, threshold: Option<f32>) -> Value {
    let cmd = |c: &adage_tracks::card::CardCommand| {
        json!({ "command": c.command, "sessions": c.sessions, "runs": c.runs, "failed": c.failed,
                "error": (!c.error.is_empty()).then_some(&c.error), "verified": c.verified })
    };
    json!({
        "sessions": card.sessions,
        "passed": card.passed,
        "files": card.files.iter().map(|f| json!({ "path": f.path, "sessions": f.sessions })).collect::<Vec<_>>(),
        "exemplars": card.exemplars.iter().map(|x| json!({ "path": x.path, "lines": x.lines })).collect::<Vec<_>>(),
        "commands": card.commands.iter().map(cmd).collect::<Vec<_>>(),
        "avoid": card.avoid.iter().map(cmd).collect::<Vec<_>>(),
        "finish": card.finish,
        "threshold": threshold.map(|t| round(f64::from(t))),
        "grey_from": threshold.map(|t| round(f64::from(t - adage_tracks::card::GREY_MARGIN))),
        "shared": false,
    })
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
        Step::Sequence { commands } => {
            json!({ "kind": "sequence", "label": commands.join(" → "), "commands": commands })
        }
        Step::Card { card } => json!({
            "kind": "card",
            "label": format!(
                "procedure card: {} · from {}",
                crate::words::count(card.files.len(), "file"),
                crate::words::count(card.sessions as usize, "trace")
            ),
        }),
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
        Step::Sequence { commands } => commands.iter().map(|c| (command_key(c, table), "directs")).collect(),
        Step::Card { card } => card.commands.iter().map(|c| (command_key(&c.command, table), "directs")).collect(),
        Step::ReadBatch { .. } | Step::Advise { .. } => vec![],
    }
}

/// `adage graph --learned --dump <repo> [--transcripts <file>...] [--claude-projects <dir>] [--json]`:
/// the graph engine's session facts for one repository (Track G G0), as counts: events, prompts,
/// acts, artifacts, edges by kind, token occurrences, embeddings, truncations, and the value slots
/// learned from them. Sessions are read as `adage condition` reads them (Claude Code history by
/// default); facts go through the facts cache (`$ADAGE_HOME/cache/facts`). Read-only apart from
/// that cache.
fn dump_learned(args: &[String], paths: &Paths) -> Result<()> {
    let at = args.iter().position(|a| a == "--dump").unwrap_or(0);
    let repo = args.get(at + 1).filter(|r| !r.starts_with("--")).cloned();
    let Some(repo) = repo else {
        return Err(crate::hint(
            "--dump needs a repository key",
            "adage graph --learned --dump <repo> [--transcripts <file>…] [--json]",
        ));
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
    use crate::words::count;
    use adage_term::{Glyph, Line, Out, Role};
    let mut o = Out::stdout();
    o.header(
        "graph --learned",
        &[&repo, &count(sessions.len(), "trace"), &format!("facts cache {hits} hits, {misses} computed")],
    )?;
    o.blank()?;
    o.kv(
        "events",
        Line::new()
            .text(stats.events.to_string())
            .sep()
            .quiet(format!("{} prompts, {} acts, {} failed", stats.prompts, stats.acts, stats.failed_acts))
            .sep()
            .quiet(format!("at most {} per trace", stats.max_events)),
    )?;
    o.kv(
        "artifacts",
        Line::new()
            .text(stats.arts.to_string())
            .sep()
            .quiet(format!("{} distinct", stats.distinct_arts))
            .sep()
            .quiet(format!("{} distinct act keys", stats.distinct_act_keys)),
    )?;
    o.kv(
        "observed",
        Line::new().text(format!("{} observations", stats.obs)).sep().quiet(format!("{} failures", stats.fails)),
    )?;
    let edges: Vec<String> = stats.edges.iter().map(|(k, v)| format!("{k} {v}")).collect();
    o.kv("edges", Line::new().quiet(if edges.is_empty() { "none".into() } else { edges.join(", ") }))?;
    o.kv(
        "tokens",
        Line::new()
            .text(format!("{} occurrences", stats.tokens))
            .sep()
            .quiet(format!("{} truncated traces", stats.truncated_sessions)),
    )?;
    o.kv(
        "embeddings",
        Line::new().text(stats.embeddings.to_string()).sep().quiet(match &model {
            Some(m) => format!("model {}", m.id),
            None => "no intent model; prompts not embedded".into(),
        }),
    )?;
    o.kv(
        "value flow",
        Line::new().text(count(learned.sinks, "sink")).sep().text(count(learned.slots.len(), "value slot")),
    )?;
    let w = slots.iter().chain(&rejected).map(|s| s["sink"].as_str().unwrap_or("").chars().count()).max();
    let col = 6 + w.unwrap_or(0).max(16);
    for s in &slots {
        o.line(
            &Line::new()
                .text("  ")
                .glyph(Role::Fire, Glyph::Ok)
                .text(" ")
                .text(s["sink"].as_str().unwrap_or(""))
                .pad_to(col)
                .quiet(s["contract"].as_str().unwrap_or(""))
                .sep()
                .faint(count(s["sessions"].as_u64().unwrap_or(0) as usize, "trace")),
        )?;
    }
    for s in &rejected {
        o.line(
            &Line::new()
                .text("  ")
                .glyph(Role::Ink3, Glyph::Off)
                .text(" ")
                .quiet(s["sink"].as_str().unwrap_or(""))
                .pad_to(col)
                .faint("not a slot")
                .sep()
                .faint(format!(
                    "{}, {} distinct, flow rate {:.2}, top share {:.2}",
                    count(s["sessions"].as_u64().unwrap_or(0) as usize, "trace"),
                    s["distinct"],
                    s["flow_rate"].as_f64().unwrap_or(0.0),
                    s["top_share"].as_f64().unwrap_or(0.0)
                )),
        )?;
    }
    o.blank()?;
    o.next("adage condition", "condition reflexes from these traces")?;
    Ok(())
}

pub fn graph(args: &[String], paths: &Paths) -> Result<()> {
    if args.iter().any(|a| a == "--learned") && args.iter().any(|a| a == "--dump") {
        return dump_learned(args, paths);
    }
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => i += 1,
            "--events" if args.get(i + 1).is_some_and(|n| n.parse::<usize>().is_ok()) => i += 2,
            "--events" => bail!("--events takes a number of events"),
            other => bail!(
                "unknown option `{other}` for `adage graph` (it takes --json and --events <n>; the map is `adage map`)"
            ),
        }
    }
    let max_events: usize = crate::flag(args, "--events").and_then(|n| n.parse().ok()).unwrap_or(80).min(1000);
    let out = build(paths, max_events)?;
    if args.iter().any(|a| a == "--json") {
        println!("{out}");
    } else {
        use adage_term::{Glyph, Line, Out, Role, Strength};
        let s = &out["sessions"];
        let mut o = Out::stdout();
        o.header(
            "graph",
            &[
                &crate::words::count(routes_len(&out), "reflex"),
                &format!(
                    "{} ({} treated, {} control)",
                    crate::words::count(s["total"].as_u64().unwrap_or(0) as usize, "trace"),
                    s["treated"],
                    s["control"]
                ),
            ],
        )?;
        let routes: Vec<&Value> = out["routes"].as_array().into_iter().flatten().collect();
        if !routes.is_empty() {
            o.blank()?;
        }
        for r in routes {
            let l = &r["live"];
            let strength = match r["strength"].as_str().unwrap_or("") {
                "green" | "fire" => Strength::Fire,
                "red" | "inhibit" => Strength::Inhibit,
                _ => Strength::Prime,
            };
            o.line(
                &Line::new()
                    .text("  ")
                    .strength(strength)
                    .pad_to(13)
                    .quiet(r["trigger"]["label"].as_str().unwrap_or(""))
                    .text(" ")
                    .glyph(Role::Ink3, Glyph::Arrow)
                    .text(" ")
                    .text(r["id"].as_str().unwrap_or(""))
                    .text(" ")
                    .glyph(Role::Ink3, Glyph::Arrow)
                    .text(" ")
                    .cmd(r["step"]["label"].as_str().unwrap_or("")),
            )?;
            o.line(
                &Line::new()
                    .pad_to(13)
                    .faint(format!("directed {}", l["emitted"]))
                    .sep()
                    .faint(format!("followed {}", l["followed"]))
                    .sep()
                    .faint(format!("ignored {}", l["ignored"]))
                    .faint("   control: ")
                    .faint(format!("would have {}", l["would_have"]))
                    .sep()
                    .faint(format!("unaided {}", l["control"]["unaided"]))
                    .sep()
                    .faint(format!("not reached {}", l["control"]["not_reached"])),
            )?;
        }
        o.blank()?;
        if routes_len(&out) == 0 {
            o.next("adage condition", "form reflexes from your agents' past traces")?;
        } else {
            o.next("adage map", "the same graph, drawn in your browser")?;
            o.also("adage graph --json", "all of it: reflexes, observed commands, recent events")?;
        }
    }
    Ok(())
}

/// The graph as JSON, with at most `max_events` recent events.
pub(crate) fn build(paths: &Paths, max_events: usize) -> Result<Value> {
    let table = CommandTable::load();
    let installed = discover(&paths.tracks())?;
    let index = Index::load(&paths.index());
    // The decisions a person can make about each route; the map offers them as commands to copy.
    let standings = standings(paths).unwrap_or_default();

    // Routes, keyed by (repo, route id) as the runtime identifies them.
    let mut live: BTreeMap<(String, String), Live> = BTreeMap::new();
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
            for id in &directed {
                let l = live.entry((repo.clone(), id.clone())).or_default();
                l.emitted += 1;
                l.sessions.insert(sid.clone());
            }
            for id in &would {
                live.entry((repo.clone(), id.clone())).or_default().would_have += 1;
            }
            let arm = r["arm"].as_str().map(arm_word).unwrap_or("unknown").to_string();
            let control = arm == "control";
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
            for j in d["judgements"].as_array().into_iter().flatten() {
                let (Some(kind), Some(id)) = (j["kind"].as_str(), j["route"].as_str()) else { continue };
                let l = live.entry((repo.clone(), id.to_string())).or_default();
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
            let standing = standings.get(&(inst.reference.clone(), r.id.clone()));
            let reasons: Vec<Value> = standing
                .map(|s| s.reasons.iter().map(|(k, why)| json!({ "kind": k, "why": why })).collect())
                .unwrap_or_default();
            let why = whys.get(&r.id);
            let described = crate::evidence::describe(r, why);
            // What the strength was decided on: shipped facts, a value estimate, or a follow count.
            let basis = if r.is_instinct() {
                "instinct"
            } else if crate::evidence::value_line(r, why).is_some() {
                "value"
            } else if matches!(r.step, Step::Card { .. }) {
                "family"
            } else {
                "count"
            };
            let mut step_json = step(&r.step);
            if let Step::Card { card } = &r.step {
                let threshold = match &r.trigger {
                    Trigger::Intent { threshold, .. } => Some(*threshold),
                    _ => None,
                };
                step_json["card"] = card_json(card, threshold);
            }
            let uplift = e.uplift.as_ref().map(|u| {
                let mut v = serde_json::to_value(u).unwrap_or(Value::Null);
                v["line"] = json!(u.line());
                v
            });
            let evidence = json!({
                "support": e.support, "opportunities": e.opportunities, "rate": e.rate, "lower_bound": e.lower_bound,
                "turns_saved": e.turns_saved, "days": e.days, "exposed": e.exposed, "dormant_since": e.dormant_since,
                "provenance": e.provenance,
                "describe": described,
                "basis": basis,
                "why": why.map(|w| json!({
                    "shape": w.shape, "pattern": w.pattern, "gain": w.gain, "lift": w.lift, "conf": w.conf,
                    "conf_lb": w.conf_lb, "support": w.support, "days": w.days, "backoff": w.backoff,
                })),
                "uplift": uplift,
                "outcome_lift": e.outcome_lift,
                "effect_evidence": e.effect_evidence,
                "execution": e.execution,
            });
            // Kept quiet on its uplift: agents already take the step unaided, so it waits for a person.
            let quiet = r.state == adage_tracks::RouteState::Nascent
                && !r.authored
                && e.uplift.as_ref().is_some_and(|u| u.gated);
            routes.push(json!({
                "id": r.id,
                "summary": r.summary,
                "instinct": r.is_instinct() || index::is_instinct_track(t),
                "origin": origin_label(r),
                "review": {
                    "verbs": standing.map(|s| s.verbs.clone()).unwrap_or_default(),
                    "reasons": reasons,
                    "execution_command": e.execution.as_ref().map(|_| format!("adage review delegate {} {} --level e2", inst.reference, r.id)),
                },
                "track": t.manifest.track.name,
                "version": t.manifest.track.version,
                "reference": inst.reference,
                "repo": repo,
                "imported": inst.imported,
                "in_index": indexed,
                "strength": lower(r.strength),
                "ceiling": ceiling_str(r),
                "effect": lower(r.effect),
                "state": lower(r.state),
                "authored": r.authored,
                "shape": adage_tracks::shape(r),
                "quiet": quiet,
                "trigger": trigger(&r.trigger),
                "step": step_json,
                "execution_only": e.execution.is_some(),
                "directive": if e.execution.is_some() { String::new() } else { preview(t, r).0 },
                "obligation": crate::evidence::obligation(r),
                "evidence": evidence,
                "live": {
                    "emitted": l.map_or(0, |l| l.emitted),
                    "would_have": l.map_or(0, |l| l.would_have),
                    "followed": l.map_or(0, |l| l.followed),
                    "ignored": l.map_or(0, |l| l.ignored),
                    "misfired": l.map_or(0, |l| l.misfired),
                    "obligation_met": l.map_or(0, |l| l.obligation_met),
                    "control": {
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

    // The bars this home conditions with (`learn.thresholds`), so the map measures evidence
    // against the bar that actually set each strength.
    let config = adage_store::config::load(paths).config;
    let bar = crate::autolearn::thresholds(config.learn_thresholds);
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
            "card_threshold": round(config.learn_card_threshold),
        },
    });
    Ok(out)
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
        let now = crate::learnlog::now_ms();
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
pub(crate) fn event(repo: &str, sid: &str, r: &Value) -> Option<Value> {
    // A PreToolUse record (`kind: "pretool"`) has no decision: it says what the hook rewrote or
    // advised before a command ran (`pretool`), or would have in a control session (`withheld`).
    if r["kind"] == "pretool" {
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
    if directed.is_empty() && would.is_empty() && judged.is_empty() && keys.is_empty() && !cont {
        return None;
    }
    Some(json!({
        "ts": r["ts"], "session": sid, "arm": r["arm"].as_str().map(arm_word), "event": r["event"], "repo": repo,
        "directed": directed, "would": would, "judged": judged, "observed": keys, "continue": cont,
    }))
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

fn routes_len(v: &Value) -> usize {
    v["routes"].as_array().map_or(0, |a| a.len())
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
}
