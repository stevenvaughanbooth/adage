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

use adage_core::CommandTable;
use adage_store::Paths;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

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
fn routes_len(v: &Value) -> usize {
    v["routes"].as_array().map_or(0, |a| a.len())
}

/// The graph as JSON (`adage_app::graph::build`) for the checkout the person is in, warning about
/// each installed track skipped.
pub(crate) fn build(paths: &Paths, max_events: usize) -> Result<Value> {
    let here = std::env::current_dir().ok().and_then(|p| adage_core::repo::find_root(&p));
    adage_app::graph::build(paths, here.as_deref(), max_events, &mut |w| crate::ui::warn(w))
}
