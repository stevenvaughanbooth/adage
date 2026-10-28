//! `adage world`: the situational awareness layer's verbs (the design's "world"; `adage-world`
//! decides, this module does the I/O).
//!
//! ```text
//! adage world [status] [--json]            what the world holds: sources, artifacts, the calendar, activity
//! adage world descriptors [--json]         the source descriptors in effect (built in, plus $ADAGE_HOME/packs/world/)
//! adage world ingest [--source <name>]     events, artifacts and calendar blocks on stdin, from the mod's connector sources
//! adage world follow [--repo <dir>] [--session <id>] [--stdin ignore|eof] [--since <cursor>]
//!                                          the local sources (transcripts, files, the desktop) and the timeline as JSON lines
//! adage world eval record <prompt> --meant <artifact id|none>
//! adage world eval report [--json]         the resolver against the prompts you labelled
//! adage world pause | resume               stop every source and every hint, or start them again
//! adage world on | off                     world.enabled (on is a person's decision)
//! ```
//!
//! Everything lives under `$ADAGE_HOME/world/` (`adage_store::Paths::world_*`). No verb here
//! reaches the network: connector sources run in the Claude Code mod and arrive through
//! `ingest`; the follow child reads this machine only.

mod eval;
mod follow;
mod ingest;
mod status;

use adage_store::Paths;
use adage_term::{Line, Out};
use anyhow::{bail, Context, Result};

pub const USAGE: &str =
    "usage: adage world [status|descriptors|ingest|follow|eval|pause|resume|on|off] (adage world --help)";

/// `adage world …`.
pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        None | Some("status") => status::status(if args.is_empty() { args } else { rest }, paths),
        Some(a) if a.starts_with("--") => status::status(args, paths),
        Some("descriptors") => descriptors(rest, paths),
        Some("ingest") => ingest::ingest(rest, paths),
        Some("follow") => follow::follow(rest, paths),
        Some("eval") => eval::run(rest, paths),
        Some("pause") => pause(true, paths),
        Some("resume") => pause(false, paths),
        Some("on") => switch(true, paths),
        Some("off") => switch(false, paths),
        _ => bail!("{USAGE}"),
    }
}

/// The descriptors in effect: the built-in ones, then any under `$ADAGE_HOME/packs/world/`, a
/// person's file replacing a built-in of the same name.
pub fn descriptors_in_effect(paths: &Paths) -> (Vec<adage_world::Descriptor>, Vec<String>) {
    let mut all = adage_world::descriptor::builtin();
    let mut problems = Vec::new();
    let dir = paths.root.join("packs").join("world");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect())
        .unwrap_or_default();
    files.sort();
    for f in files {
        match std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|t| adage_world::descriptor::parse(&t)) {
            Ok(d) => {
                all.retain(|b| b.name() != d.name());
                all.push(d);
            }
            Err(e) => problems.push(format!("{}: {e}", f.display())),
        }
    }
    all.sort_by(|a, b| a.name().cmp(b.name()));
    (all, problems)
}

fn descriptors(args: &[String], paths: &Paths) -> Result<()> {
    let (all, problems) = descriptors_in_effect(paths);
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string(&serde_json::json!({ "descriptors": all, "problems": problems }))?);
        return Ok(());
    }
    let mut o = Out::stdout();
    o.header("world descriptors", &[&format!("{} sources", all.len())])?;
    o.blank()?;
    for d in &all {
        let how = match d.source.needs {
            adage_world::Needs::Connector => {
                format!(
                    "connector · {} · every {}s",
                    d.provider.iter().map(|p| p.server.as_str()).collect::<Vec<_>>().join(", "),
                    d.every_secs()
                )
            }
            adage_world::Needs::Local => format!("local · every {}s", d.every_secs()),
        };
        o.kv(d.name(), Line::new().text(d.kind().label()).sep().quiet(how))?;
    }
    for p in &problems {
        o.warn(p)?;
    }
    o.blank()?;
    o.note(&format!("a file under {} adds or replaces one", crate::detect::tilde(&paths.root.join("packs/world"))))?;
    o.flush()?;
    Ok(())
}

/// `adage world pause` writes the marker every source and the hook read; `resume` removes it.
/// Pausing only turns something down, so it runs anywhere, an agent session included.
fn pause(on: bool, paths: &Paths) -> Result<()> {
    let m = paths.world_paused();
    if on {
        std::fs::create_dir_all(paths.world()).with_context(|| format!("creating {}", paths.world().display()))?;
        std::fs::write(
            &m,
            "The world is paused: no source records and no hint is delivered. `adage world resume` starts it again.\n",
        )
        .with_context(|| format!("writing {}", m.display()))?;
    } else if m.exists() {
        std::fs::remove_file(&m).with_context(|| format!("removing {}", m.display()))?;
    }
    let mut o = Out::stdout();
    o.header("world", &[if on { "paused" } else { "resumed" }])?;
    o.blank()?;
    if on {
        o.note("every source stopped and no hint is delivered until `adage world resume`")?;
    } else {
        o.note("sources record again from the next poll, and hints are delivered")?;
    }
    o.flush()?;
    Ok(())
}

/// `adage world on | off`: `world.enabled`. On records what you do, so it is a person's decision;
/// off turns it down and runs anywhere.
fn switch(on: bool, paths: &Paths) -> Result<()> {
    if on {
        crate::share::require_person("world on")?;
    }
    adage_store::config::set(paths, "world.enabled", if on { "true" } else { "false" })
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut o = Out::stdout();
    o.header("world", &[if on { "on" } else { "off" }])?;
    o.blank()?;
    if on {
        o.note("the world records calendar, meetings, files and pull requests from the next session on (titles, paths and summaries made locally; never pixels or keystrokes)")?;
        o.next("adage world", "what it holds, and which sources are live")?;
        o.also("adage config set world.transcripts ~/Meetings", "where your meeting tool writes transcripts")?;
    } else {
        o.note("nothing is recorded and no hint is delivered; what was recorded stays until its retention runs out")?;
    }
    o.flush()?;
    Ok(())
}
