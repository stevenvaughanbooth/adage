//! `adage world [status]`: what the world holds, through the output grammar (header, key column,
//! `next`), or as JSON for the mod's pane and Health tab.

use adage_store::{config, spool, state, Paths};
use adage_term::{Line, Out};
use adage_trace::{ActivityEvent, ArtifactKind};
use adage_world::state::{ArtifactsState, CalendarState, HealthState};
use adage_world::text::{hhmm, until};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct Status {
    pub enabled: bool,
    pub paused: bool,
    pub desktop: bool,
    pub retention_days: u32,
    pub sources: Vec<adage_world::SourceHealth>,
    pub artifacts: BTreeMap<String, usize>,
    pub artifacts_total: usize,
    pub blocks: usize,
    pub next_block: Option<adage_trace::CalendarBlock>,
    pub events_24h: usize,
    pub eval_cases: usize,
    pub now: u64,
}

pub fn read(paths: &Paths) -> Status {
    let cfg = config::load(paths).config;
    let now = adage_core::time::now_ms();
    let health = state::try_read::<HealthState>(&paths.world_health()).ok().flatten().unwrap_or_default();
    let arts = state::try_read::<ArtifactsState>(&paths.world_artifacts()).ok().flatten().unwrap_or_default();
    let cal = state::try_read::<CalendarState>(&paths.world_calendar()).ok().flatten().unwrap_or_default();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for a in arts.artifacts.values() {
        *by_kind.entry(a.kind.as_str().to_string()).or_default() += 1;
    }
    let events: Vec<ActivityEvent> = spool::read_all(&paths.world_activity()).unwrap_or_default();
    let since = now.saturating_sub(86_400_000);
    let next_block = cal.blocks.iter().filter(|b| b.end > now && b.busy && b.accepted).min_by_key(|b| b.start).cloned();
    let eval_cases = std::fs::read_to_string(paths.world_eval())
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    Status {
        enabled: cfg.world_enabled,
        paused: paths.world_paused().exists(),
        desktop: cfg.world_desktop,
        retention_days: cfg.world_retention_days,
        sources: health.sources.into_values().collect(),
        artifacts_total: arts.artifacts.len(),
        artifacts: by_kind,
        blocks: cal.blocks.len(),
        next_block,
        events_24h: events.iter().filter(|e| e.ts >= since).count(),
        eval_cases,
        now,
    }
}

pub fn status(args: &[String], paths: &Paths) -> Result<()> {
    let s = read(paths);
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string(&s)?);
        return Ok(());
    }
    let mut o = Out::stdout();
    let state = match (s.enabled, s.paused) {
        (false, _) => "off",
        (true, true) => "paused",
        (true, false) => "on",
    };
    o.header("world", &[state, &format!("{} sources", s.sources.len())])?;
    o.blank()?;
    if s.sources.is_empty() {
        o.kv("sources", Line::new().quiet("none have reported yet: the Claude Code mod polls connectors in a session, `adage world follow` the local ones"))?;
    }
    for (i, src) in s.sources.iter().enumerate() {
        let mut l = Line::new().text(&src.name);
        if !src.server.is_empty() {
            l = l.quiet(format!(" ({})", src.server));
        } else if !src.via.is_empty() {
            l = l.quiet(format!(" ({})", src.via));
        }
        l = l.sep().quiet(if src.last_poll == 0 {
            "never".to_string()
        } else {
            format!("polled {}", crate::helpers::ago(s.now, src.last_poll))
        });
        l = l.sep().quiet(format!("{} events", src.events));
        if !src.ok {
            l = l.sep().text(format!("failed: {}", src.error));
        }
        o.kv(if i == 0 { "sources" } else { "" }, l)?;
    }
    let kinds: Vec<String> = s
        .artifacts
        .iter()
        .map(|(k, n)| format!("{n} {}", ArtifactKind::parse(k).map_or(k.as_str(), |k| k.label())))
        .collect();
    o.kv(
        "artifacts",
        Line::new().text(s.artifacts_total.to_string()).quiet(if kinds.is_empty() {
            String::new()
        } else {
            format!(": {}", kinds.join(", "))
        }),
    )?;
    let mut cal = Line::new().text(format!("{} blocks in the window", s.blocks));
    if let Some(b) = &s.next_block {
        cal = cal.sep().quiet(format!("next: {} \"{}\" ({})", hhmm(b.start, 0), b.title, until(s.now, b.start)));
    }
    o.kv("calendar", cal)?;
    o.kv("activity", Line::new().text(format!("{} events in the last 24 h", s.events_24h)))?;
    o.kv(
        "retention",
        Line::new().text(format!("{} days", s.retention_days)).sep().quiet(if s.desktop {
            "desktop titles on"
        } else {
            "desktop titles off"
        }),
    )?;
    o.kv("eval", Line::new().text(format!("{} labelled prompts", s.eval_cases)))?;
    o.blank()?;
    if !s.enabled {
        o.next("adage world on", "record what you do and resolve references (a person's decision)")?;
    } else if s.paused {
        o.next("adage world resume", "start the sources and hints again")?;
    } else if s.eval_cases < 50 {
        o.next(
            "adage world eval record \"<prompt>\" --meant <artifact>",
            "label prompts until fifty make the resolver measurable",
        )?;
    } else {
        o.next("adage world eval report", "hit@1 and false attaches on your prompts")?;
    }
    o.flush()?;
    Ok(())
}
