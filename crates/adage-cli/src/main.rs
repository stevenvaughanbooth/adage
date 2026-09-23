//! `adage` — put your agent on rails.
//!
//! ```text
//! adage hook <claude|opencode|codex> <Event>   hook client (reads the harness payload on stdin)
//! adage init [--repo <path>] [--yes]  first run: detect harnesses, show hook snippets, offer the model, survey
//! adage survey [--repo <key>] [--since <days>] [--json]   what your agents actually do (read-only)
//! adage doctor [--json]              check the binary, data directory, index, tracks, model, hooks
//! adage learn [options]              mine tracks from agent sessions and compile the index
//! adage status                       installed tracks, routes, aspects, and evidence
//! adage report [--json]              directives emitted, followed, and derailed, from spools
//! adage graph [--json] [--events <n>] situations → routes → steps → agent responses, with live counts
//! adage graph --serve [--port <n>]   serve the signal box at http://127.0.0.1:<n>/ (default 8765)
//! adage hooks-config <adage-path>    print a Claude Code hooks settings block (exec form)
//! adage install <claude-code|opencode|codex> [--print|--write] [--scope project|user]
//!                                    print (default) or write a harness's hook registration
//! adage share <track> --out <dir> [--key <path>]   redaction-check, copy, and sign a track
//! adage add <dir> [--trust]          verify a shared track and install it at probation
//! adage review [--json]              routes awaiting a person's decision, with evidence and directive text
//! adage review <approve|reject|demote|promote|pin|dismiss> <track> <route> [--by <name>]
//! adage models fetch|install <dir>|status   the embedding model for intent families (optional)
//! adage eval <dataset> [--split ..] [--json]  offline replay eval of what Adage would have said
//! adage version
//! ```
//! Environment: `ADAGE_HOME` (data directory), `ADAGE_ARM` (`treated`|`holdout`, overrides
//! assignment), `ADAGE_HOLDOUT` (holdout share, default 0.1), `ADAGE_DISABLE=1` (kill switch),
//! `ADAGE_ALLOW_NONINTERACTIVE=1` (let `share` and `add` run without a terminal, for CI).

mod add;
mod detect;
mod doctor;
mod failopen;
mod graph;
mod harness;
mod hook;
mod init;
mod install;
mod models;
mod proposal;
mod redact;
mod reimport;
mod review;
mod serve;
mod share;
mod survey;
mod tty;

use adage_learn::{Inputs, LearnConfig};
use adage_store::Paths;
use adage_tracks::{io, Aspect};
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("hook") {
        // Fail open, always: no panic, error, or timeout may block or disturb the agent.
        let harness = args.get(1).cloned().unwrap_or_default();
        let event = args.get(2).cloned().unwrap_or_default();
        // Nothing may reach the harness but our JSON: silence panic messages entirely.
        failopen::install_panic_hook();
        let (h, e) = (harness.clone(), event.clone());
        let out = match on_big_stack(move || std::panic::catch_unwind(|| hook::run(&h, &e))) {
            Some(Ok(Ok(out))) => out,
            Some(Ok(Err(f))) => {
                failopen::record(&Paths::from_env().root, &harness, &event, &f);
                None
            }
            Some(Err(_)) => {
                failopen::record(&Paths::from_env().root, &harness, &event, &failopen::panic_failure());
                None
            }
            None => {
                let f = failopen::Failure::new("thread", "the hook thread could not start");
                failopen::record(&Paths::from_env().root, &harness, &event, &f);
                None
            }
        };
        if let Some(o) = out {
            use std::io::Write;
            let _ = writeln!(std::io::stdout(), "{o}");
        }
        std::process::exit(0);
    }
    match on_big_stack(move || run(&args)) {
        Some(Ok(())) => {}
        Some(Err(e)) => {
            eprintln!("adage: {e:#}");
            std::process::exit(1);
        }
        None => std::process::exit(1),
    }
}

/// The stack every command runs on. Main-thread stacks differ by platform (8 MiB on Linux and
/// macOS, 1 MiB on Windows); normalising hostile commands recurses to a bounded depth that fits
/// easily in release builds, but should not depend on which platform's default it got.
const STACK: usize = 8 << 20;

/// Run `f` on a thread with [`STACK`] bytes of stack. `None` if the thread could not be started
/// or `f` panicked.
fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::Builder::new().stack_size(STACK).spawn(f).ok()?.join().ok()
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn run(args: &[String]) -> Result<()> {
    let paths = Paths::from_env();
    // Root commits `learn` and `survey` find are left for hooks (repository keys).
    adage_core::repo::use_cache(paths.repo_keys());
    match args.first().map(String::as_str) {
        Some("learn") => learn(&args[1..], &paths),
        Some("status") => status(&paths),
        Some("report") => report(&paths, args.iter().any(|a| a == "--json")),
        Some("share") => share::share(&args[1..], &paths),
        Some("add") => add::add(&args[1..], &paths),
        Some("review") => review::review(&args[1..], &paths),
        Some("graph") if args.iter().any(|a| a == "--serve") => serve::serve(&args[1..], &paths),
        Some("graph") => graph::graph(&args[1..], &paths),
        Some("eval") => adage_eval::cli::run(&args[1..]),
        Some("models") => models::run(&args[1..], &paths),
        Some("survey") => survey::survey(&args[1..], &paths),
        Some("init") => init::init(&args[1..], &paths),
        Some("doctor") => doctor::doctor(&args[1..], &paths),
        Some("hooks-config") => {
            let bin = args.get(1).cloned().unwrap_or_else(|| "adage".into());
            println!("{}", serde_json::to_string_pretty(&harness::claude::settings(&bin))?);
            Ok(())
        }
        Some("install") => install::install(&args[1..]),
        Some("version") | Some("--version") => {
            println!("adage {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            println!("usage: adage <init|survey|learn|status|report|graph|review|share|add|install|models|doctor|hooks-config|version|hook>");
            Ok(())
        }
    }
}

/// `adage learn [--transcripts <file>...] [--claude-projects <dir>] [--codex-sessions <dir>] [--cwd-prefix <path>]
///               [--outcomes <json>] [--repo <key>] [--demo] [--json]`
fn learn(args: &[String], paths: &Paths) -> Result<()> {
    let mut transcripts: Vec<PathBuf> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--transcripts" {
            i += 1;
            while i < args.len() && !args[i].starts_with("--") {
                transcripts.push(PathBuf::from(&args[i]));
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    // Codex rollouts: `--codex-sessions <dir>` (`$CODEX_HOME/sessions`), read only when asked.
    if let Some(dir) = flag(args, "--codex-sessions") {
        let (found, compressed) = adage_learn::codex::discover(std::path::Path::new(&dir));
        if compressed > 0 {
            eprintln!("adage learn: skipped {compressed} compressed Codex rollouts (.jsonl.zst); decompress them with `zstd -d` to include them");
        }
        let prefix = flag(args, "--cwd-prefix");
        for p in found {
            if let Some(pre) = &prefix {
                // The first line (session metadata) records the cwd.
                let mut head = String::new();
                if let Ok(f) = std::fs::File::open(&p) {
                    use std::io::{BufRead, Read};
                    let _ = std::io::BufReader::new(f).take(1 << 20).read_line(&mut head);
                }
                if !head.contains(&format!("\"cwd\":\"{pre}")) {
                    continue;
                }
            }
            transcripts.push(p);
        }
    }
    // Default to Claude Code history only when no source was named: an explicit source that finds
    // nothing is an error, never a silent fallback to other history.
    let explicit = args.iter().any(|a| a == "--transcripts" || a == "--codex-sessions");
    if let Some(dir) = flag(args, "--claude-projects").or_else(|| {
        (!explicit).then(|| {
            adage_store::home_dir()
                .map(|h| h.join(".claude").join("projects").to_string_lossy().into_owned())
                .unwrap_or_default()
        })
    }) {
        let prefix = flag(args, "--cwd-prefix");
        for p in adage_learn::import::discover(std::path::Path::new(&dir)) {
            if let Some(pre) = &prefix {
                // Cheap pre-filter on the transcript's recorded cwd.
                let head = std::fs::read_to_string(&p).unwrap_or_default();
                if !head.contains(&format!("\"cwd\":\"{pre}")) {
                    continue;
                }
            }
            transcripts.push(p);
        }
    }
    if transcripts.is_empty() {
        bail!("no transcripts found");
    }
    let outcomes: BTreeMap<String, bool> = match flag(args, "--outcomes") {
        Some(p) => serde_json::from_str(&std::fs::read_to_string(p)?)?,
        None => BTreeMap::new(),
    };
    let cfg = if args.iter().any(|a| a == "--demo") { LearnConfig::demo() } else { LearnConfig::default() };
    let inputs = Inputs { transcripts, outcomes, repo_override: flag(args, "--repo"), models: Some(paths.models()) };
    let rep = adage_learn::learn(&inputs, &paths.tracks(), &paths.index(), &cfg)?;
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&rep)?);
    } else {
        println!(
            "adage learn: {} sessions read, {} used, {} steered (excluded per route kind from evidence they influenced)",
            rep.sessions_read, rep.sessions_used, rep.sessions_steered
        );
        println!("thresholds: {}", rep.thresholds);
        match &rep.intent_model {
            Some(m) => println!("intent families: {m}"),
            None => println!("intent families: skipped (no embedding model; `adage models fetch` enables them)"),
        }
        for (repo, rr) in &rep.repos {
            println!("\n{repo} — {} sessions, {} usable intent families", rr.sessions, rr.intent_families);
            for t in &rr.tracks {
                println!("  {} @ {} ({})", t.name, t.version, t.change);
                for r in &t.routes {
                    println!("    · {r}");
                }
            }
        }
        println!("\nindex generation {}", rep.index_generation);
    }
    Ok(())
}

fn status(paths: &Paths) -> Result<()> {
    let tracks = io::read_all(&paths.tracks())?;
    let has_intents =
        tracks.iter().any(|t| t.routes.iter().any(|r| matches!(r.trigger, adage_tracks::Trigger::Intent { .. })));
    match adage_intent::model::StaticModel::open(&adage_intent::model::dir_in(&paths.models())) {
        Ok(m) => println!("intent model: {}", m.id),
        Err(_) if has_intents => {
            println!("intent model: not installed, so intent routes will not fire (`adage models fetch`)")
        }
        Err(_) => {}
    }
    if tracks.is_empty() {
        println!("no tracks installed in {}", paths.tracks().display());
        return Ok(());
    }
    for t in tracks {
        let origin = if adage_tracks::index::is_imported(&t) { "  (imported)" } else { "" };
        println!("{} @ {}  [{}]{origin}", t.manifest.track.name, t.manifest.track.version, t.manifest.scope.repo);
        for r in &t.routes {
            let dot = match r.aspect {
                Aspect::Green => "🟢",
                Aspect::Yellow => "🟡",
                Aspect::Red => "🔴",
            };
            let e = &r.evidence;
            println!(
                "  {dot} {:<22} {:>3}/{:<3} sessions  rate {:.2}  lb {:.2}  {:?}",
                r.id, e.support, e.opportunities, e.rate, e.lower_bound, r.effect
            );
        }
    }
    Ok(())
}

fn report(paths: &Paths, json: bool) -> Result<()> {
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    let dir = paths.sessions();
    let mut sessions = 0;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x == "spool") {
                sessions += 1;
                let recs: Vec<serde_json::Value> = adage_store::spool::read_all(&e.path())?;
                for r in recs {
                    for k in ["directives", "would_have"] {
                        let n = r["decision"][k].as_array().map(|a| a.len()).unwrap_or(0) as u64;
                        *totals.entry(k.to_string()).or_default() += n;
                    }
                    for j in r["decision"]["judgements"].as_array().into_iter().flatten() {
                        *totals.entry(j["kind"].as_str().unwrap_or("?").to_string()).or_default() += 1;
                    }
                    if r["decision"]["continue_reason"].is_string() {
                        *totals.entry("stop_requests".into()).or_default() += 1;
                    }
                }
            }
        }
    }
    if json {
        println!("{}", serde_json::json!({ "sessions": sessions, "totals": totals }));
    } else {
        println!("{sessions} sessions");
        for (k, v) in totals {
            println!("  {k:<16} {v}");
        }
    }
    Ok(())
}
