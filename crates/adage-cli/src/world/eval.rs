//! `adage world eval`: the held-out prompt set the person labels, and the resolver's score on it.
//! A labelled prompt is the one text the world keeps verbatim, because the person typed it here
//! on purpose; the activity timeline and the session spools hold no prompt text.

use adage_intent::normalize::normalize;
use adage_store::{state, Paths};
use adage_world::state::{ArtifactsState, CalendarState, EvalCase};
use adage_world::{Decision, Params, WorldCtx};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::io::Write;

const USAGE: &str = "usage: adage world eval record \"<prompt>\" --meant <artifact id|none> | adage world eval report [--json] | adage world eval list";

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("record") => record(&args[1..], paths),
        Some("report") => report(&args[1..], paths),
        Some("list") => list(paths),
        _ => bail!("{USAGE}"),
    }
}

fn cases(paths: &Paths) -> Vec<EvalCase> {
    std::fs::read_to_string(paths.world_eval())
        .map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

fn record(args: &[String], paths: &Paths) -> Result<()> {
    let prompt = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let meant = crate::helpers::flag(args, "--meant").unwrap_or_default();
    if prompt.trim().is_empty() || meant.is_empty() {
        bail!("{USAGE}");
    }
    let meant = if meant == "none" { String::new() } else { meant };
    let case = EvalCase { ts: adage_core::time::now_ms(), prompt, meant };
    std::fs::create_dir_all(paths.world()).with_context(|| format!("creating {}", paths.world().display()))?;
    let mut f =
        std::fs::OpenOptions::new().create(true).append(true).open(paths.world_eval()).context("the eval set")?;
    writeln!(f, "{}", serde_json::to_string(&case)?)?;
    let n = cases(paths).len();
    let mut o = adage_term::Out::stdout();
    o.header("world eval", &[&format!("{n} labelled")])?;
    o.blank()?;
    o.done("recorded", if case.meant.is_empty() { "the prompt refers to nothing" } else { &case.meant })?;
    if n < 50 {
        o.next("adage world eval record …", &format!("{} more make the resolver measurable", 50 - n))?;
    } else {
        o.next("adage world eval report", "hit@1 and false attaches")?;
    }
    o.flush()?;
    Ok(())
}

fn list(paths: &Paths) -> Result<()> {
    for c in cases(paths) {
        println!("{}\t{}\t{}", c.ts, if c.meant.is_empty() { "none" } else { &c.meant }, c.prompt);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub cases: usize,
    pub hits: usize,
    pub false_attaches: usize,
    pub disambiguated: usize,
    pub silent_right: usize,
    pub misses: Vec<Miss>,
    pub hit_at_1: f32,
    pub false_attach_rate: f32,
}

#[derive(Debug, Serialize)]
pub struct Miss {
    pub prompt: String,
    pub meant: String,
    pub got: String,
}

/// Replay every labelled prompt against the artifacts as they stood at its time: an artifact is
/// in the window when it was first seen by then, its last touch clipped to that time (an
/// approximation of the timeline then; the activity spool holds the exact touches, which a later
/// chunk replays).
pub fn replay(paths: &Paths) -> Report {
    let all = cases(paths);
    let arts = state::try_read::<ArtifactsState>(&paths.world_artifacts()).ok().flatten().unwrap_or_default();
    let cal = state::try_read::<CalendarState>(&paths.world_calendar()).ok().flatten().unwrap_or_default();
    let embed = adage_hook::world::embedder(paths);
    let params = Params::default();
    let (mut hits, mut false_attaches, mut disambiguated, mut silent_right) = (0, 0, 0, 0);
    let mut misses = Vec::new();
    for c in &all {
        let then: Vec<_> = arts
            .artifacts
            .values()
            .filter(|a| a.first_seen <= c.ts)
            .map(|a| {
                let mut a = a.clone();
                a.last_touched = a.last_touched.min(c.ts);
                a
            })
            .collect();
        let ctx = WorldCtx {
            now: c.ts,
            tz_offset_min: 0,
            artifacts: &then,
            events: &[],
            blocks: &cal.blocks,
            embed: &|t| embed(&normalize(t)),
            params: &params,
        };
        let d = adage_world::resolve(&ctx, &c.prompt);
        let got = match &d {
            Decision::Attach(h) => h.artifacts[0].clone(),
            Decision::Disambiguate(h) => format!("one of {}", h.artifacts.join(", ")),
            Decision::None { reason } => format!("nothing ({reason})"),
        };
        match (&d, c.meant.is_empty()) {
            (Decision::Attach(h), false) if h.artifacts[0] == c.meant => hits += 1,
            (Decision::Attach(_), _) => {
                false_attaches += 1;
                misses.push(Miss { prompt: c.prompt.clone(), meant: c.meant.clone(), got });
            }
            (Decision::Disambiguate(h), false) if h.artifacts.contains(&c.meant) => disambiguated += 1,
            (Decision::None { .. }, true) => silent_right += 1,
            _ => misses.push(Miss { prompt: c.prompt.clone(), meant: c.meant.clone(), got }),
        }
    }
    let n = all.len().max(1) as f32;
    Report {
        cases: all.len(),
        hits,
        false_attaches,
        disambiguated,
        silent_right,
        misses,
        hit_at_1: hits as f32 / n,
        false_attach_rate: false_attaches as f32 / n,
    }
}

fn report(args: &[String], paths: &Paths) -> Result<()> {
    let r = replay(paths);
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string(&r)?);
        return Ok(());
    }
    let mut o = adage_term::Out::stdout();
    o.header("world eval", &[&format!("{} prompts", r.cases)])?;
    o.blank()?;
    o.kv(
        "hit@1",
        adage_term::Line::new()
            .text(format!("{:.0}%", r.hit_at_1 * 100.0))
            .sep()
            .quiet(format!("{} of {}; target 80%", r.hits, r.cases)),
    )?;
    o.kv(
        "false attach",
        adage_term::Line::new()
            .text(format!("{:.0}%", r.false_attach_rate * 100.0))
            .sep()
            .quiet(format!("{}; target 5% or under", r.false_attaches)),
    )?;
    o.kv(
        "asked",
        adage_term::Line::new().text(format!("{} disambiguated with the right one listed", r.disambiguated)),
    )?;
    o.kv("silent", adage_term::Line::new().text(format!("{} rightly said nothing", r.silent_right)))?;
    for m in r.misses.iter().take(10) {
        o.kv(
            "miss",
            adage_term::Line::new().text(format!("{:?}", m.prompt)).sep().quiet(format!(
                "meant {}, got {}",
                if m.meant.is_empty() { "nothing" } else { &m.meant },
                m.got
            )),
        )?;
    }
    o.blank()?;
    if r.cases < 50 {
        o.next("adage world eval record …", &format!("{} more prompts make this measurable", 50 - r.cases))?;
    }
    o.flush()?;
    Ok(())
}
