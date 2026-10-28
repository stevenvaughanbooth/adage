//! `adage world ingest`: events, artifacts and calendar blocks on stdin, written to the world's
//! store after every check `adage_world::ingest` makes. The Claude Code mod pipes a connector
//! source's poll here; `adage world follow` writes its local sources through the same function.
//! Exits 0 always and says what it wrote and what it refused, as JSON.
//!
//! ```json
//! { "source": "calendar", "events": [ActivityEvent…], "artifacts": [Artifact…], "blocks": [CalendarBlock…], "error": "…" }
//! ```
//!
//! A bare array is taken as `events`. An artifact arrives without an embedding; it is embedded
//! here with the same model the hook resolves with (`adage_hook::world::embedder`).

use super::descriptors_in_effect;
use adage_intent::normalize::normalize;
use adage_store::{config, lock, spool, state, Paths};
use adage_trace::{ActivityEvent, Artifact, CalendarBlock};
use adage_world::state::{ArtifactsState, CalendarState, HealthState, SourceHealth};
use adage_world::{ingest as check, Descriptor};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::time::Duration;

/// What one ingest carries.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Batch {
    pub source: String,
    pub events: Vec<ActivityEvent>,
    pub artifacts: Vec<Artifact>,
    pub blocks: Option<Vec<CalendarBlock>>,
    /// The source's failure, when the poll failed: health records it and nothing is written.
    pub error: String,
    /// Where the source ran (`mod`, `follow`) and the server it read, for `adage world status`.
    pub via: String,
    pub server: String,
}

/// What was written.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Written {
    pub source: String,
    pub events: usize,
    pub artifacts: usize,
    pub blocks: usize,
    pub skipped: usize,
    pub problems: Vec<String>,
}

/// `adage world ingest [--source <name>] [--error <text>]`.
pub fn ingest(args: &[String], paths: &Paths) -> Result<()> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).context("reading stdin")?;
    let mut batch: Batch = if text.trim().is_empty() {
        Batch::default()
    } else if text.trim_start().starts_with('[') {
        Batch { events: serde_json::from_str(&text).context("the events on stdin")?, ..Batch::default() }
    } else {
        serde_json::from_str(&text).context("the batch on stdin")?
    };
    if let Some(s) = crate::helpers::flag(args, "--source") {
        batch.source = s;
    }
    if let Some(e) = crate::helpers::flag(args, "--error") {
        batch.error = e;
    }
    if batch.source.is_empty() {
        batch.source = batch.events.first().map(|e| e.source.clone()).unwrap_or_default();
    }
    let written = write(paths, batch)?;
    println!("{}", serde_json::to_string(&written)?);
    Ok(())
}

/// How long a writer waits for another's lock before giving up on this batch.
const PATIENCE: Duration = Duration::from_secs(2);

/// Write one batch: every event checked against its descriptor and the deny list, duplicates
/// dropped, artifacts embedded and merged, the calendar replaced when blocks are given, the
/// source's health updated. Under `world/lock`'s writer lock, so two sources never interleave.
pub fn write(paths: &Paths, mut batch: Batch) -> Result<Written> {
    let cfg = config::load(paths).config;
    let now = adage_core::time::now_ms();
    let (descriptors, mut problems) = descriptors_in_effect(paths);
    let desc: Option<&Descriptor> = descriptors.iter().find(|d| d.name() == batch.source);
    let mut out = Written { source: batch.source.clone(), ..Written::default() };
    std::fs::create_dir_all(paths.world()).with_context(|| format!("creating {}", paths.world().display()))?;
    let _lock = lock::exclusive_within(&paths.world_lock().with_extension("write"), PATIENCE)
        .context("the world's writer lock")?
        .ok_or_else(|| anyhow::anyhow!("another writer holds the world's lock"))?;
    if !batch.error.is_empty() {
        health(paths, &batch, false, 0)?;
        out.problems.push(batch.error.clone());
        return Ok(out);
    }
    if batch.events.len() > check::MAX_BATCH {
        problems.push(format!("{} events: only the first {} are taken", batch.events.len(), check::MAX_BATCH));
        batch.events.truncate(check::MAX_BATCH);
    }
    let embed = adage_hook::world::embedder(paths);
    let artifacts_path = paths.world_artifacts();
    let activity = paths.world_activity();
    let deny = &cfg.world_deny;
    let mut kept: Vec<ActivityEvent> = Vec::new();
    let (events, arts, skipped) = state::update::<ArtifactsState, _>(&artifacts_path, |s| {
        let mut skipped = 0usize;
        let mut arts = 0usize;
        for mut a in std::mem::take(&mut batch.artifacts) {
            if let Err(e) = check::check_artifact(desc, &a) {
                problems.push(e);
                skipped += 1;
                continue;
            }
            if check::artifact_denied(deny, &a) {
                skipped += 1;
                continue;
            }
            if a.embedding.is_none() {
                a.embedding = embed(&normalize(&a.text())).map(|v| adage_intent::quantize(&v));
            }
            if a.first_seen == 0 {
                a.first_seen = now;
            }
            if a.last_touched == 0 {
                a.last_touched = a.first_seen;
            }
            s.upsert(a);
            arts += 1;
        }
        for e in std::mem::take(&mut batch.events) {
            if let Err(err) = check::check_event(desc, &e) {
                problems.push(err);
                skipped += 1;
                continue;
            }
            if !s.artifacts.contains_key(&e.artifact) {
                skipped += 1;
                continue;
            }
            if s.is_duplicate(&e) {
                skipped += 1;
                continue;
            }
            s.touched(&e);
            kept.push(e);
        }
        s.prune(now, u64::from(cfg.world_retention_days) * 6 * 86_400_000);
        (kept.len(), arts, skipped)
    })
    .context("updating the artifacts")?;
    for e in &kept {
        spool::append(&activity, e).context("appending to the activity timeline")?;
    }
    out.events = events;
    out.artifacts = arts;
    out.skipped = skipped;
    if let Some(blocks) = batch.blocks.take() {
        let n = blocks.len();
        state::update::<CalendarState, _>(&paths.world_calendar(), |c| {
            c.ts = now;
            c.blocks = blocks;
        })
        .context("updating the calendar")?;
        out.blocks = n;
    }
    health(paths, &batch, true, events)?;
    out.problems.append(&mut problems);
    Ok(out)
}

fn health(paths: &Paths, batch: &Batch, ok: bool, events: usize) -> Result<()> {
    if batch.source.is_empty() {
        return Ok(());
    }
    let now = adage_core::time::now_ms();
    state::update::<HealthState, _>(&paths.world_health(), |h| {
        let s = h
            .sources
            .entry(batch.source.clone())
            .or_insert_with(|| SourceHealth { name: batch.source.clone(), ..SourceHealth::default() });
        s.last_poll = now;
        s.ok = ok;
        s.error = if ok { String::new() } else { batch.error.clone() };
        s.events += events as u64;
        if !batch.via.is_empty() {
            s.via = batch.via.clone();
        }
        if !batch.server.is_empty() {
            s.server = batch.server.clone();
        }
    })
    .context("updating the sources' health")?;
    Ok(())
}
