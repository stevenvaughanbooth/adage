//! `adage` — agent traces become reflexes.
//!
//! ```text
//! adage hook <claude|opencode|codex> <Event>   hook client (reads the harness payload on stdin)
//! adage init [--repo <path>] [--yes | --print]  first run: a guided wizard at a terminal; else detect
//!                                    harnesses, show hook snippets, offer the model, sense
//! adage sense [--repo <key>] [--since <days>] [--json]   read-only: which reflexes your traces would form
//! adage doctor [--json]              check the binary, data directory, index, tracks, model, hooks
//! adage watch [--repo <path>] [--demo]  a live pane: health lights, hook activity, what Adage conditions
//! adage ui                           the full-screen app: live feed, reflexes and review, health fixes, settings
//! adage condition [options]          mine traces into reflexes, bind instincts, compile the index
//! adage condition --auto --repo-root <path>   condition one repository quietly (what hooks start)
//! adage condition --instincts-only [--repo-path <dir>] [--repo <key>]  bind Adage's instincts for
//!                                    a repository from its files alone, with no traces (day 0)
//! adage train <plan|--plan|report> …  grow reflexes from a skill (runs the adage-train binary)
//! adage config [get <key> | set <key> <value>] [--json]   settings in $ADAGE_HOME/config.toml
//! adage status                       installed tracks, reflexes, strengths, and evidence
//! adage report [--json]              directives emitted, followed, and ignored, from spools
//! adage graph [--json] [--events <n>] situations → reflexes → steps → agent responses, with live counts
//! adage map [--port <n>]             serve the map at http://127.0.0.1:<n>/ (default 8765)
//! adage graph --learned --dump <repo> [--transcripts <file>...] [--json]   the graph engine's trace facts
//! adage hooks-config <adage-path>    print a Claude Code hooks settings block (exec form)
//! adage install <claude-code|opencode|codex> [--print|--write] [--scope project|user]
//!                                    print (default) or write a harness's hook registration
//! adage share <track-or-pack> --out <dir> [--key <path>] [--vendor] [--attest <results.jsonl>] [--ack-scripts] [--repo-root <dir>]
//!                                    redaction-check, build and sign a pack (namespace adage-pack)
//! adage add <dir> [--trust]          verify a signed pack, install it,
//!                                    and bind it to this repository at forming
//! adage review [--json]              reflexes awaiting a person's decision, with evidence and directive text
//! adage review delegate <track> <route> --level e1|e2 [--raw] [--fields ..] [--outputs <glob>]   approve an E1 prefetch or an E2 action (a person)
//! adage review revoke <track> <route>   withdraw that grant
//! adage delegate on|off|status       hooks call adage-exec (E1 prefetch, E2 actions) or adage; grants per repository
//! adage review <approve|extinguish|weaken|strengthen|pin|dismiss> <track> <route> [--by <name>]
//! adage models fetch|install <dir>|status   the embedding model for intent families (optional)
//! adage eval <dataset> [--split ..] [--json]  offline replay eval of what Adage would have said
//! adage version
//! ```
//! Environment: `ADAGE_HOME` (data directory), `ADAGE_ARM` (`treated`|`control`, overrides
//! assignment), `ADAGE_CONTROL` (control share; overrides `runtime.control`, default 0.1),
//! `ADAGE_AUTO_LEARN` (`0`/`1`; overrides `learn.auto`), `ADAGE_DISABLE=1` (kill switch),
//! `ADAGE_ALLOW_NONINTERACTIVE=1` (let `share` and `add` run without a terminal, for CI).

mod add;
mod autolearn;
mod config;
mod detect;
mod doctor;
mod exec;
use adage_hook::failopen;
mod graph;
mod help;
use adage_hook::harness;
mod hook;
mod init;
mod install;
mod learnlog;
mod models;
mod packs;
mod proposal;
mod redact;
mod reimport;
mod review;
mod sense;
mod serve;
mod share;
mod share_pack;
mod train;
mod tty;
mod ui;

// Commands run on the hook's stack size (`adage_hook::STACK`).
use adage_hook::on_big_stack;
use adage_learn::{Inputs, LearnConfig};
use adage_store::Paths;
use adage_tracks::{io, Strength};
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--help` is never an argument: answered before anything runs (the hook included).
    if let Some(cmd) = args.first().filter(|c| help::wants(c, &args[1..])) {
        match help::command(cmd) {
            Some(h) => print!("{h}"),
            None => print!("{}", help::all()),
        }
        return;
    }
    if args.first().map(String::as_str) == Some("hook") {
        // Fail open, always: no panic, error, or timeout may block or disturb the agent.
        hook::main(args.get(1).cloned().unwrap_or_default(), args.get(2).cloned().unwrap_or_default());
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

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn run(args: &[String]) -> Result<()> {
    let paths = Paths::from_env();
    // Root commits `condition` and `sense` find are left for hooks (repository keys).
    adage_core::repo::use_cache(paths.repo_keys());
    match args.first().map(String::as_str) {
        Some("condition") if args.iter().any(|a| a == "--auto") => autolearn::run_auto(&args[1..], &paths),
        Some("condition") if args.iter().any(|a| a == "--instincts-only") => instincts_only(&args[1..], &paths),
        Some("condition") => condition(&args[1..], &paths),
        Some("train") => train::train(&args[1..]),
        Some("config") => config::run(&args[1..], &paths),
        Some("status") => status(&paths),
        Some("report") => report(&paths, args.iter().any(|a| a == "--json")),
        Some("share") => share::share(&args[1..], &paths),
        Some("add") => add::add(&args[1..], &paths),
        Some("review") if matches!(args.get(1).map(String::as_str), Some("delegate" | "revoke")) => {
            exec::review(&args[1], &args[2..], &paths)
        }
        Some("review") => review::review(&args[1..], &paths),
        Some("delegate") => exec::delegate(&args[1..], &paths),
        Some("map") => serve::serve(&args[1..], &paths),
        Some("graph") => graph::graph(&args[1..], &paths),
        Some("eval") => adage_eval::cli::run(&args[1..]),
        Some("models") => models::run(&args[1..], &paths),
        Some("sense") => sense::sense(&args[1..], &paths),
        Some("init") => init::init(&args[1..], &paths),
        Some("doctor") => doctor::doctor(&args[1..], &paths),
        Some("watch") => ui::watch(&args[1..], &paths),
        Some("ui") => ui::app(&args[1..], &paths),
        // Bare `adage` at a terminal opens the app; anywhere else (a pipe, an agent's shell) it
        // prints the usage, as `--help` always does.
        None if ui::interactive() => ui::app(&[], &paths),
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
        Some("help") if args.len() > 1 => match help::command(&args[1]) {
            Some(h) => {
                print!("{h}");
                Ok(())
            }
            None => bail!("unknown command `{}`: `adage --help` lists them", args[1]),
        },
        None | Some("help" | "--help" | "-h") => {
            print!("{}", help::all());
            Ok(())
        }
        Some(cmd) => bail!("unknown command `{cmd}`: `adage --help` lists them"),
    }
}

/// `adage condition [--transcripts <file>...] [--claude-projects <dir>] [--codex-sessions <dir>] [--cwd-prefix <path>]
///               [--outcomes <json>] [--repo <key>] [--demo] [--no-cache] [--json]`
///
/// Thresholds: `--demo`, else `learn.thresholds` (`adage config`). Parses are cached under
/// `$ADAGE_HOME/cache/sessions` unless `--no-cache` or `learn.parse_cache = false` (the result is
/// the same).
fn condition(args: &[String], paths: &Paths) -> Result<()> {
    let started = std::time::Instant::now();
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
            eprintln!("adage condition: skipped {compressed} compressed Codex rollouts (.jsonl.zst); decompress them with `zstd -d` to include them");
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
        bail!(
            "no traces found{}. Point it at your history with --claude-projects <dir>, --codex-sessions <dir> \
             or --transcripts <file>..., or run `adage condition --instincts-only` for instincts alone",
            if explicit || flag(args, "--claude-projects").is_some() {
                " in the sources given"
            } else {
                " in ~/.claude/projects"
            }
        );
    }
    let outcomes: BTreeMap<String, bool> = match flag(args, "--outcomes") {
        Some(p) => serde_json::from_str(&std::fs::read_to_string(p)?)?,
        None => BTreeMap::new(),
    };
    let settings = adage_store::config::load(paths).config;
    let cfg = if args.iter().any(|a| a == "--demo") {
        LearnConfig::demo()
    } else {
        autolearn::thresholds(settings.learn_thresholds)
    };
    let mut cfg = cfg;
    // Instincts are bound after learning, per repository, unless turned off (config, or --no-instincts).
    if !args.iter().any(|a| a == "--no-instincts") {
        cfg.instincts = Some(autolearn::instinct_config(&settings, paths));
    }
    let cache =
        (settings.learn_parse_cache && !args.iter().any(|a| a == "--no-cache")).then(|| paths.cache().join("sessions"));
    transcripts.sort();
    transcripts.dedup();
    let inputs = Inputs {
        transcripts,
        outcomes,
        repo_override: flag(args, "--repo"),
        models: Some(paths.models()),
        cache,
        only_repo: None,
    };
    // One writer of the tracks and index at a time (automatic learners, review decisions).
    std::fs::create_dir_all(&paths.root)?;
    let tracks_lock = adage_store::lock::exclusive(&paths.tracks_lock())?;
    let before = adage_learn::diff::snapshot_all(&paths.tracks());
    let result = adage_learn::learn(&inputs, &paths.tracks(), &paths.index(), &cfg);
    let after = adage_learn::diff::snapshot_all(&paths.tracks());
    drop(tracks_lock);
    let rep = match result {
        Ok(r) => r,
        Err(e) => {
            let line = learnlog::error_line(learnlog::now_ms(), "manual", None, "condition", &learnlog::describe(&e));
            let _ = learnlog::append(paths, &line);
            return Err(e);
        }
    };
    let changes: BTreeMap<&String, Vec<adage_learn::diff::Change>> = rep
        .repos
        .keys()
        .map(|r| {
            let empty = Default::default();
            (r, adage_learn::diff::diff(before.get(r).unwrap_or(&empty), after.get(r).unwrap_or(&empty)))
        })
        .collect();
    if let Err(e) = log_learn(paths, &rep, &changes, started.elapsed().as_millis() as u64) {
        eprintln!("adage condition: could not write {}: {e}", paths.learn_log().display());
    }
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&rep)?);
    } else {
        println!(
            "adage condition: {} traces read, {} used, {} steered (what their directives named is not evidence)",
            rep.sessions_read, rep.sessions_used, rep.sessions_steered
        );
        println!("thresholds: {}", rep.thresholds);
        match &rep.intent_model {
            Some(m) => println!("prompt intents: {m}"),
            None => println!("prompt intents: skipped (no embedding model; `adage models fetch` enables them)"),
        }
        for (repo, rr) in &rep.repos {
            println!("\n{repo} — {} traces, {} usable prompt intents", rr.sessions, rr.intent_families);
            for t in &rr.tracks {
                println!("  {} @ {} ({})", t.name, t.version, t.change);
                for r in &t.routes {
                    println!("    · {r}");
                }
            }
            if let Some(ir) = &rr.instincts {
                print_instincts(ir);
            }
            let e = &rr.engine;
            println!(
                "  conditioned: {} traces, {} patterns selected, {} reflexes compiled, {} kept ({} with a context trigger)",
                e.sessions, e.reflexes, e.compiled, e.routes, e.contexts
            );
            for c in changes.get(repo).into_iter().flatten() {
                println!("  changed: {}: {}", c.route, c.summary);
            }
        }
        let nascent = rep
            .repos
            .values()
            .flat_map(|rr| rr.tracks.iter())
            .flat_map(|t| t.routes.iter())
            .filter(|l| l.contains(" NASCENT: "))
            .count();
        if nascent > 0 {
            println!(
                "\n{nascent} nascent reflex(es) formed but not delivered: each waits for a person. Review them with `adage review`."
            );
        }
        if !rep.conflicts.is_empty() {
            println!("\ninterlocking: {} conflict(s) resolved", rep.conflicts.len());
            for c in &rep.conflicts {
                println!("  {}", c.describe());
            }
        }
        println!("\nindex generation {}", rep.index_generation);
    }
    Ok(())
}

/// What binding instincts did for one repository, for the text output of `adage condition`.
fn print_instincts(ir: &adage_learn::instincts::InstinctReport) {
    match &ir.not_bound {
        Some(why) => println!("  instincts: none bound ({why})"),
        None => println!(
            "  instincts: {} bound, {} advice, {} awaiting approval in `adage review`{}{}",
            ir.bound,
            ir.advice,
            ir.nascent,
            if ir.extinguished.is_empty() {
                String::new()
            } else {
                format!(", {} extinguished automatically", ir.extinguished.len())
            },
            if ir.confirmed.is_empty() {
                String::new()
            } else {
                format!(", {} confirmed by traces", ir.confirmed.len())
            }
        ),
    }
}

/// `adage condition --instincts-only [--repo-path <dir>] [--repo <key>] [--json]`: bind a repository's instincts
/// from its declared files, with no sessions (day 0), and compile the index. The repository is
/// `--repo-path` (default: the current directory), resolved to its root; its key is `--repo <key>` when
/// given (as `adage condition --repo` keys reflexes), else derived from the root.
pub(crate) fn instincts_only(args: &[String], paths: &Paths) -> Result<()> {
    let started = std::time::Instant::now();
    let dir = match flag(args, "--repo-path").or_else(|| flag(args, "--repo-root")) {
        Some(d) => PathBuf::from(d),
        None => std::env::current_dir()?,
    };
    let dir = dir.canonicalize().map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    let root = adage_core::repo::find_root(&dir).unwrap_or(dir);
    // The same key `adage condition --repo <key>` stores reflexes under, so both indexes of one
    // repository agree (`adage_core::repo::key_with`).
    let explicit = flag(args, "--repo");
    let repo = adage_core::repo::key_with(explicit.as_deref(), || {
        adage_core::repo::repo_key_cached(&root, &paths.repo_keys())
    });
    let settings = adage_store::config::load(paths).config;
    let ic = autolearn::instinct_config(&settings, paths);
    std::fs::create_dir_all(&paths.root)?;
    let tracks_lock = adage_store::lock::exclusive(&paths.tracks_lock())?;
    let before = adage_learn::diff::snapshot(&paths.tracks(), &repo);
    let result = adage_learn::instincts::learn_only(&repo, &root, &paths.tracks(), &paths.index(), &ic);
    let after = adage_learn::diff::snapshot(&paths.tracks(), &repo);
    drop(tracks_lock);
    let rep = result?;
    let changes: BTreeMap<&String, Vec<adage_learn::diff::Change>> =
        rep.repos.keys().map(|r| (r, adage_learn::diff::diff(&before, &after))).collect();
    if let Err(e) = log_learn(paths, &rep, &changes, started.elapsed().as_millis() as u64) {
        eprintln!("adage condition: could not write {}: {e}", paths.learn_log().display());
    }
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&rep)?);
        return Ok(());
    }
    for (repo, rr) in &rep.repos {
        println!("adage condition --instincts-only: {repo}");
        for t in &rr.tracks {
            println!("  {} @ {} ({})", t.name, t.version, t.change);
            for r in &t.routes {
                println!("    · {r}");
            }
        }
        if let Some(ir) = &rr.instincts {
            print_instincts(ir);
            for s in ir.skipped.iter().take(12) {
                println!("    not bound: {s}");
            }
        }
        for c in changes.get(repo).into_iter().flatten() {
            println!("  changed: {}: {}", c.route, c.summary);
        }
    }
    println!("\nindex generation {}", rep.index_generation);
    Ok(())
}

/// Append one `learn.log` line per repository this `adage condition` learned (`crate::learnlog`), or
/// one with no repository if it found no usable session. No prompt, command or path.
fn log_learn(
    paths: &Paths,
    rep: &adage_learn::Report,
    changes: &BTreeMap<&String, Vec<adage_learn::diff::Change>>,
    duration_ms: u64,
) -> Result<()> {
    let ts = learnlog::now_ms();
    let line = |repo: Option<&str>, changes: &[adage_learn::diff::Change]| {
        learnlog::learn_line(
            ts,
            &learnlog::Learned {
                trigger: "manual",
                repo,
                sessions_read: rep.sessions_read,
                duration_ms,
                changes,
                report: rep,
            },
        )
    };
    if rep.repos.is_empty() {
        learnlog::append(paths, &line(None, &[]))?;
    }
    for repo in rep.repos.keys() {
        learnlog::append(paths, &line(Some(repo), changes.get(repo).map(Vec::as_slice).unwrap_or(&[])))?;
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
            println!("intent model: not installed, so intent reflexes will not fire (`adage models fetch`)")
        }
        Err(_) => {}
    }
    if tracks.is_empty() {
        println!("no tracks installed in {}", paths.tracks().display());
        return Ok(());
    }
    for t in tracks {
        let origin = if adage_tracks::index::is_imported(&t) {
            "  (imported)"
        } else if adage_tracks::index::is_instinct_track(&t) {
            "  (instincts: Adage's built-in defaults, from the repository's files)"
        } else {
            ""
        };
        println!("{} @ {}  [{}]{origin}", t.manifest.track.name, t.manifest.track.version, t.manifest.scope.repo);
        for r in &t.routes {
            let (dot, word) = match r.strength {
                Strength::Fire => ("🟢", "fire"),
                Strength::Prime => ("🟡", "prime"),
                Strength::Inhibit => ("🔴", "inhibit"),
            };
            let e = &r.evidence;
            println!(
                "  {dot} {word:<7} {:<22} {:>3}/{:<3} traces  rate {:.2}  lb {:.2}  {:?}",
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
