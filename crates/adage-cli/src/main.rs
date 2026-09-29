//! `adage` — agent traces become reflexes.
//!
//! The command-line grammar (every command, flag and help text) is `cli.rs`; `adage --help`
//! prints it, grouped. `adage hook <claude|opencode|codex> <Event>` is the hook client and is
//! dispatched here before anything else, on the fast path. Output goes through `adage_term`
//! (docs/design/style.md); words follow docs/lexicon.md.
//!
//! Bare `adage` prints the status card (`status_card.rs`); `adage ui` opens the app.
//!
//! Environment: `ADAGE_HOME` (data directory), `ADAGE_ARM` (`treated`|`control`, overrides
//! assignment), `ADAGE_CONTROL` (control share; overrides `runtime.control`, default 0.1),
//! `ADAGE_AUTO_LEARN` (`0`/`1`; overrides `learn.auto`), `ADAGE_DISABLE=1` (kill switch),
//! `ADAGE_ALLOW_NONINTERACTIVE=1` (let `share` and `add` run without a terminal, for CI);
//! output: `NO_COLOR`, `ADAGE_COLOR=never|16|256|truecolor`, `ADAGE_THEME=day|night`,
//! `ADAGE_ASCII=1` (or `--plain` for both).

mod add;
mod autolearn;
mod cli;
mod condition_out;
mod config;
mod detect;
mod doctor;
mod evidence;
mod exec;
use adage_hook::failopen;
mod graph;
use adage_hook::harness;
mod hook;
mod init;
mod install;
mod learnlog;
mod map;
mod models;
mod packs;
mod proposal;
mod redact;
mod reimport;
mod review;
mod savings;
mod sense;
mod share;
mod share_pack;
mod status_card;
mod train;
mod tty;
mod ui;
mod words;

// Commands run on the hook's stack size (`adage_hook::STACK`).
use adage_hook::on_big_stack;
use adage_learn::{Inputs, LearnConfig};
use adage_store::Paths;
use adage_tracks::io;
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("hook") {
        // Fail open, always: no panic, error, or timeout may block or disturb the agent.
        hook::main(args.get(1).cloned().unwrap_or_default(), args.get(2).cloned().unwrap_or_default());
    }
    match on_big_stack(move || run(&args)) {
        Some(Ok(())) => {}
        // The reader went away (`adage status | head`): nothing is wrong, and there is no one to tell.
        Some(Err(e))
            if e.chain().any(|c| {
                c.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
            }) =>
        {
            std::process::exit(0)
        }
        Some(Err(e)) => {
            // `error  what failed`, then each cause (adage_term::Out::error).
            let causes: Vec<String> = e.chain().skip(1).map(|c| c.to_string()).collect();
            let mut help = e.chain().find_map(|c| c.downcast_ref::<Hint>()).map(|h| h.help.clone());
            // A bare permission error names no path: say which directory to check.
            let denied = e.chain().any(|c| {
                c.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
            });
            if help.is_none() && denied {
                help = Some(format!(
                    "a file could not be written or read; Adage keeps its data in {} (ADAGE_HOME), and `adage doctor` checks it",
                    detect::tilde(&Paths::from_env().root)
                ));
            }
            let mut err = adage_term::Out::stderr();
            if err.error(&e.to_string(), &causes, help.as_deref()).is_err() {
                eprintln!("adage: {e:#}");
            }
            std::process::exit(1);
        }
        None => std::process::exit(1),
    }
}

/// An error that knows the next step: `main` prints `help` as its `╰─ help` line.
#[derive(Debug)]
pub(crate) struct Hint {
    what: String,
    help: String,
}

impl std::fmt::Display for Hint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.what)
    }
}

impl std::error::Error for Hint {}

/// `what` failed; `help` says what to run instead.
pub(crate) fn hint(what: impl Into<String>, help: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Hint { what: what.into(), help: help.into() })
}

/// `what` failed for each of `causes` (printed as `├─ cause` lines); `help` says what to do.
pub(crate) fn refusal(what: impl Into<String>, mut causes: Vec<String>, help: impl Into<String>) -> anyhow::Error {
    let Some(last) = causes.pop() else { return hint(what, help) };
    let mut e = hint(last, help);
    for c in causes.into_iter().rev() {
        e = e.context(c);
    }
    e.context(what.into())
}

/// Create the data directory if need be and check that it can be written, with one clear message
/// if not (rather than a bare "Permission denied" from whichever file came first).
pub(crate) fn writable_data_dir(paths: &Paths) -> Result<()> {
    let at = detect::tilde(&paths.root);
    let fix = "set ADAGE_HOME to a writable directory (`adage doctor` checks it)";
    if let Err(e) = std::fs::create_dir_all(&paths.root) {
        return Err(hint(format!("cannot create the data directory {at} ({})", io_reason(&e)), fix));
    }
    let probe = paths.root.join(format!(".adage-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => Err(hint(format!("cannot write in the data directory {at} ({})", io_reason(&e)), fix)),
    }
}

/// An I/O error in a few words, without the OS error number.
fn io_reason(e: &std::io::Error) -> String {
    let s = e.to_string();
    s.split(" (os error").next().unwrap_or(&s).to_lowercase()
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn run(args: &[String]) -> Result<()> {
    // `--plain` anywhere: no colour and ASCII glyphs, for screen readers and logs. It is read here
    // and removed, so command modules never see it.
    let plain = args.iter().any(|a| a == "--plain");
    let args: Vec<String> = args.iter().filter(|a| *a != "--plain").cloned().collect();
    if plain {
        std::env::set_var("ADAGE_COLOR", "never");
        std::env::set_var("ADAGE_ASCII", "1");
    }
    // clap validates the whole command line and prints help, versions and usage errors (with a
    // suggestion for a typo); each command then reads its own flags from the raw arguments
    // (`cli.rs` explains why, and tests that the two agree).
    let theme = adage_term::Theme::stdout();
    if let Some(now) = cli::renamed(&args) {
        let was = cli::renamed_from(&args);
        let mut e = adage_term::Out::stderr();
        e.error(&format!("`adage {was}` is now `{now}`"), &[], Some(&format!("see `{now} --help`")))?;
        std::process::exit(2);
    }
    let matches = match cli::command(&theme)
        .try_get_matches_from(std::iter::once("adage".to_string()).chain(args.clone()))
    {
        Ok(m) => m,
        // Help and version are not errors: clap prints them as they are.
        Err(e) if !e.use_stderr() || e.kind() == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            e.exit()
        }
        // Usage errors in the output grammar, with clap's suggestions; exit code 2 as clap's.
        Err(e) => {
            let (message, items) = cli::usage_error(&e, &args);
            let items: Vec<(&str, &str)> = items.iter().map(|(l, t)| (*l, t.as_str())).collect();
            if adage_term::Out::stderr().error_items(&message, &items).is_err() {
                e.exit()
            }
            std::process::exit(e.exit_code());
        }
    };
    let paths = Paths::from_env();
    // Root commits `condition` and `sense` find are left for hooks (repository keys).
    adage_core::repo::use_cache(paths.repo_keys());
    let rest = args.get(1..).unwrap_or(&[]);
    let sub = matches.subcommand();
    let has = |f: &str| rest.iter().any(|a| a == f);
    match sub.map(|(name, _)| name) {
        Some("condition") if has("--auto") => autolearn::run_auto(rest, &paths),
        Some("condition") if has("--instincts-only") => instincts_only(rest, &paths),
        Some("condition") => condition(rest, &paths),
        Some("train") => train::train(rest, &paths),
        Some("savings") => savings::run(rest, &paths),
        Some("config") => config::run(rest, &paths),
        Some("status") => status(&paths, has("--json")),
        Some("report") => report(&paths, has("--json")),
        Some("share") => share::share(rest, &paths),
        Some("add") => add::add(rest, &paths),
        Some("review") => {
            // `review.rs` takes the lexicon's verbs as they are; execution grants are `exec.rs`'s.
            match sub.and_then(|(_, m)| m.subcommand_name()) {
                Some(v @ ("delegate" | "revoke")) => {
                    let at = rest.iter().position(|a| a == v).map_or(1, |p| p + 1);
                    exec::review(v, &rest[at..], &paths)
                }
                _ => review::review(rest, &paths),
            }
        }
        Some("delegate") => exec::delegate(rest, &paths),
        Some("map") => map::run(rest, &paths),
        Some("graph") => graph::graph(rest, &paths),
        Some("eval") => adage_eval::cli::run(rest),
        Some("models") => models::run(rest, &paths),
        Some("sense") => sense::sense(rest, &paths),
        Some("init") => init::init(rest, &paths),
        Some("doctor") => doctor::doctor(rest, &paths),
        Some("watch") => ui::watch(rest, &paths),
        Some("ui") => ui::app(rest, &paths),
        Some("hooks-config") => {
            let bin = rest.first().cloned().unwrap_or_else(|| "adage".into());
            println!("{}", serde_json::to_string_pretty(&harness::claude::settings(&bin))?);
            Ok(())
        }
        Some("install") => install::install(rest),
        Some("completions") => {
            let shell = *sub.and_then(|(_, m)| m.get_one::<clap_complete::Shell>("shell")).expect("required");
            clap_complete::generate(
                shell,
                &mut cli::command(&adage_term::Theme::plain()),
                "adage",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Some("version") => {
            println!("adage {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        // Bare `adage` prints the status card, wherever it runs (a terminal, a pipe, `!adage` in
        // Claude Code); `adage ui` opens the app and `--help` shows the usage.
        None => status_card::run(&paths),
        Some(other) => bail!("unknown command `{other}`: `adage --help` lists them"),
    }
}

/// `adage condition [--transcripts <file>...] [--claude-projects <dir>] [--codex-sessions <dir>] [--cwd-prefix <path>]
///               [--outcomes <json>] [--repo <key>] [--demo] [--no-cache] [--no-instincts] [--json]`
///
/// Thresholds: `--demo`, else `learn.thresholds` (`adage config`). Parses are cached under
/// `$ADAGE_HOME/cache/sessions` unless `--no-cache` or `learn.parse_cache = false` (the result is
/// the same).
/// Source directories a person named must exist: an empty result would say "no traces" when the
/// path is simply wrong.
pub(crate) fn check_dirs(dirs: &[(&str, Option<&std::path::Path>)]) -> Result<()> {
    for (flag, dir) in dirs {
        if let Some(d) = dir.filter(|d| !d.is_dir()) {
            return Err(hint(
                format!("{} is not a directory ({flag})", d.display()),
                format!("check the path; {flag} takes a directory of traces"),
            ));
        }
    }
    Ok(())
}

fn condition(args: &[String], paths: &Paths) -> Result<()> {
    let started = std::time::Instant::now();
    let (cp, cs) =
        (flag(args, "--claude-projects").map(PathBuf::from), flag(args, "--codex-sessions").map(PathBuf::from));
    check_dirs(&[("--claude-projects", cp.as_deref()), ("--codex-sessions", cs.as_deref())])?;
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
    if let Some(missing) = transcripts.iter().find(|p| !p.is_file()) {
        return Err(hint(
            format!("{} is not a transcript file", missing.display()),
            "pass files that exist, or a directory with --claude-projects <dir> or --codex-sessions <dir>",
        ));
    }
    // Codex rollouts: `--codex-sessions <dir>` (`$CODEX_HOME/sessions`), read only when asked.
    if let Some(dir) = flag(args, "--codex-sessions") {
        let (found, compressed) = adage_learn::codex::discover(std::path::Path::new(&dir));
        if compressed > 0 {
            let n = words::count(compressed, "compressed Codex rollout");
            let _ = adage_term::Out::stderr()
                .warn(&format!("skipped {n} (.jsonl.zst); decompress them with `zstd -d` to include them"));
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
        let source = if let Some(p) = flag(args, "--cwd-prefix") {
            format!("none has a working directory under {p}")
        } else if explicit || args.iter().any(|a| a == "--claude-projects") {
            "the sources you named hold no traces".to_string()
        } else {
            "no Claude Code history in ~/.claude/projects".to_string()
        };
        return Err(hint(
            format!("no traces found: {source}"),
            "name a source: adage condition --transcripts <file>… (or --claude-projects <dir>, --codex-sessions <dir>), \
             or run `adage condition --instincts-only` for instincts alone",
        ));
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
        let mut ic = autolearn::instinct_config(&settings, paths);
        // Bind against the checkout this runs in, not whatever checkouts the traces name (per-task
        // worktrees, containers, a clone since removed or without its `.venv`): the live one is
        // the one the next session works in.
        // Only when the checkout is the repository being learned: `--repo <key>` names another
        // repository's traces from anywhere (an eval harness's working directory).
        if let Some(root) = std::env::current_dir().ok().and_then(|d| adage_core::repo::find_root(&d)) {
            let own = adage_core::repo::repo_key_cached(&root, &paths.repo_keys());
            if flag(args, "--repo").as_deref().is_none_or(|k| k.trim() == own) {
                ic.roots.insert(own, root);
            }
        }
        cfg.instincts = Some(ic);
    }
    cfg.cards = autolearn::card_params(&settings, paths);
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
    writable_data_dir(paths)?;
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
        let _ = adage_term::Out::stderr().warn(&format!("could not write {}: {e}", paths.learn_log().display()));
    }
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&rep)?);
    } else {
        let run = condition_out::Run {
            command: "condition",
            report: &rep,
            after: &after,
            changes: &changes,
            elapsed_ms: started.elapsed().as_millis() as u64,
            waiting: waiting(paths),
        };
        condition_out::render(&mut adage_term::Out::stdout(), &run)?;
    }
    Ok(())
}

/// `adage condition --instincts-only [--repo-path <dir>] [--repo <key>] [--json]`: bind a repository's
/// instincts from its declared files, with no sessions (day 0), and compile the index. The repository
/// is `--repo-path` (default: the current directory), resolved to its root; its key is `--repo <key>`
/// when given (as `adage condition --repo` keys reflexes), else derived from the root.
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
    writable_data_dir(paths)?;
    let tracks_lock = adage_store::lock::exclusive(&paths.tracks_lock())?;
    let before = adage_learn::diff::snapshot(&paths.tracks(), &repo);
    let result = adage_learn::instincts::learn_only(&repo, &root, &paths.tracks(), &paths.index(), &ic);
    let after = adage_learn::diff::snapshot(&paths.tracks(), &repo);
    drop(tracks_lock);
    let rep = result?;
    let changes: BTreeMap<&String, Vec<adage_learn::diff::Change>> =
        rep.repos.keys().map(|r| (r, adage_learn::diff::diff(&before, &after))).collect();
    if let Err(e) = log_learn(paths, &rep, &changes, started.elapsed().as_millis() as u64) {
        let _ = adage_term::Out::stderr().warn(&format!("could not write {}: {e}", paths.learn_log().display()));
    }
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&rep)?);
        return Ok(());
    }
    let after: BTreeMap<String, adage_learn::diff::Snapshot> = BTreeMap::from([(repo.clone(), after)]);
    let run = condition_out::Run {
        command: "condition --instincts-only",
        report: &rep,
        after: &after,
        changes: &changes,
        elapsed_ms: started.elapsed().as_millis() as u64,
        waiting: waiting(paths),
    };
    condition_out::render(&mut adage_term::Out::stdout(), &run)?;
    Ok(())
}

/// Reflexes that wait for a decision in `adage review`.
fn waiting(paths: &Paths) -> usize {
    review::standings(paths).map(|s| s.values().filter(|s| !s.reasons.is_empty()).count()).unwrap_or(0)
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

/// `adage status [--json]`: the reflexes and instincts in effect, with strength and evidence.
fn status(paths: &Paths, json: bool) -> Result<()> {
    use adage_term::{Glyph, Line, Out, Role};
    let dirs = io::read_all_dirs(&paths.tracks())?;
    // The evidence each strength was decided on (`evidence::describe`), with its reasons.
    let whys: Vec<_> = dirs.iter().map(|(dir, _)| adage_tracks::why::load(dir)).collect();
    let tracks: Vec<adage_tracks::Track> = dirs.into_iter().map(|(_, t)| t).collect();
    let has_intents =
        tracks.iter().any(|t| t.routes.iter().any(|r| matches!(r.trigger, adage_tracks::Trigger::Intent { .. })));
    // What hooks embed prompts with: the installed model, else the one bundled in this binary, else
    // the hashed fallback (`model::resolve`, as hooks, doctor and the status card choose it).
    let resolved = adage_intent::model::resolve(&paths.models(), false);
    let hashed = match &resolved.source {
        adage_intent::model::Source::Hashed { why } => Some(why.clone()),
        _ => None,
    };
    let model = hashed.is_none().then(|| resolved.embedder.id().to_string());
    let origin = |t: &adage_tracks::Track| {
        if adage_tracks::index::is_imported(t) {
            "imported"
        } else if adage_tracks::index::is_instinct_track(t) {
            "instincts"
        } else {
            "conditioned"
        }
    };
    let instinct =
        |t: &adage_tracks::Track, r: &adage_tracks::Route| r.is_instinct() || adage_tracks::index::is_instinct_track(t);
    if json {
        let tracks: Vec<serde_json::Value> = tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                serde_json::json!({
                    "track": t.manifest.track.name,
                    "version": t.manifest.track.version,
                    "repo": t.manifest.scope.repo,
                    "origin": origin(t),
                    "reflexes": t.routes.iter().map(|r| serde_json::json!({
                        "id": r.id,
                        "kind": if instinct(t, r) { "instinct" } else { "reflex" },
                        "strength": r.strength,
                        "lifecycle": words::lifecycle(r.state),
                        "support": r.evidence.support,
                        "opportunities": r.evidence.opportunities,
                        "rate": r.evidence.rate,
                        "lower_bound": r.evidence.lower_bound,
                        "effect": r.effect.as_str(),
                        "evidence": evidence::describe(r, whys[i].get(&r.id)),
                        "uplift": r.evidence.uplift.as_ref().map(|u| u.line()),
                        "obligation": evidence::obligation(r),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "intent_model": model, "tracks": tracks }))?);
        return Ok(());
    }
    let mut o = Out::stdout();
    let (mut n_reflex, mut n_instinct) = (0, 0);
    for t in &tracks {
        for r in &t.routes {
            if instinct(t, r) {
                n_instinct += 1;
            } else {
                n_reflex += 1;
            }
        }
    }
    o.header(
        "status",
        &[
            &words::count(tracks.len(), "track"),
            &words::count(n_reflex, "reflex"),
            &words::count(n_instinct, "instinct"),
        ],
    )?;
    o.blank()?;
    match &hashed {
        Some(why) if has_intents && resolved.selected != "hashed" => o.kv(
            "intent model",
            Line::new()
                .push(Role::Prime, "none")
                .sep()
                .quiet(format!("{why}; intent reflexes will not fire (`adage models status` says why)")),
        )?,
        _ => o.kv("intent model", Line::new().text(resolved.describe()))?,
    }
    if tracks.is_empty() {
        o.blank()?;
        o.note(&format!("No tracks yet in {}.", paths.tracks().display()))?;
        o.blank()?;
        o.next("adage condition", "form reflexes from your agents' past traces")?;
        return Ok(());
    }
    let id_w =
        tracks.iter().flat_map(|t| t.routes.iter().map(|r| r.id.chars().count())).max().unwrap_or(16).clamp(16, 34);
    for (t, whys) in tracks.iter().zip(&whys) {
        o.blank()?;
        let mut head = Line::new()
            .bold(Role::Ink, t.manifest.track.name.clone())
            .faint(format!("@{}", t.manifest.track.version))
            .sep()
            .quiet(t.manifest.scope.repo.clone());
        if origin(t) != "conditioned" {
            head = head.sep().quiet(origin(t));
        }
        o.line(&head)?;
        for r in &t.routes {
            let e = &r.evidence;
            let glyph = if instinct(t, r) { Glyph::Instinct } else { Glyph::Reflex };
            // The evidence its strength was decided on: counts, or (a valued reflex, often a
            // desire path no trace took yet) the value estimate on a line of its own.
            let evidence = evidence::describe(r, whys.get(&r.id));
            let valued = evidence.starts_with("value:");
            let mut l = Line::new()
                .text("  ")
                .strength(words::strength(r.strength))
                .pad_to(13)
                .glyph(Role::Accent, glyph)
                .text(" ")
                .text(r.id.clone())
                .pad_to(16 + id_w);
            l = if valued { l.faint("valued") } else { l.extend(words::evidence(e)) };
            if r.state != adage_tracks::RouteState::Firm {
                l = l.sep().quiet(words::lifecycle(r.state));
            }
            o.line(&l)?;
            let detail = |text: String| Line::new().text(" ".repeat(16)).quiet(text);
            if valued {
                o.line(&detail(evidence))?;
            }
            if let Some(u) = &e.uplift {
                o.line(&detail(format!("uplift: {}", u.line())))?;
            }
            if let Some(ob) = evidence::obligation(r) {
                o.line(&detail(ob))?;
            }
        }
    }
    // Nascent reflexes, and new or changed ones nobody has decided on yet.
    let waiting = waiting(paths);
    o.blank()?;
    if waiting > 0 {
        let why = if waiting == 1 {
            "1 reflex waits for a decision".to_string()
        } else {
            format!("{waiting} reflexes wait for a decision")
        };
        o.next("adage review", &why)?;
    } else {
        o.next("adage report", "what Adage told agents, and what they did")?;
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
        return Ok(());
    }
    use adage_term::{Line, Out, Role};
    let mut o = Out::stdout();
    o.header("report", &[&words::count(sessions, "trace")])?;
    o.blank()?;
    if totals.is_empty() {
        o.note("Nothing to report yet: hooks record a trace each time an agent works in a repository.")?;
        o.blank()?;
        return Ok(o.next("adage doctor", "check that hooks are installed")?);
    }
    let n = |k: &str| totals.get(k).copied().unwrap_or(0);
    o.kv(
        "directives",
        Line::new()
            .text(format!("{} sent", n("directives")))
            .sep()
            .quiet(format!("{} withheld (control)", n("would_have"))),
    )?;
    // Judgements, in the lexicon's words: what agents did with what Adage said.
    let known = ["directives", "would_have", "stop_requests"];
    for (k, v) in totals.iter().filter(|(k, _)| !known.contains(&k.as_str())) {
        let role = match k.as_str() {
            "followed" | "met" | "hit_avoided" => Role::Fire,
            "ignored" | "derailed" | "followed_failed" | "misfired" => Role::Inhibit,
            _ => Role::Ink,
        };
        // Older spools spell these `followed_failed` and `derailed`.
        let label = match k.as_str() {
            "followed_failed" => "misfired".to_string(),
            "derailed" => "ignored".to_string(),
            _ => words::prose(&k.replace('_', " ")),
        };
        o.kv(&label, Line::new().push(role, v.to_string()))?;
    }
    if n("stop_requests") > 0 {
        o.kv(
            "stop requests",
            Line::new().text(n("stop_requests").to_string()).sep().quiet("finish lines held at Stop"),
        )?;
    }
    o.blank()?;
    o.next("adage status", "the reflexes behind these numbers")?;
    Ok(())
}
