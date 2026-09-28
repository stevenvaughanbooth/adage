//! `adage` — agent traces become reflexes.
//!
//! The command-line grammar (every command, flag and help text) is `cli.rs`; `adage --help`
//! prints it, grouped. `adage hook <claude|opencode|codex> <Event>` is the hook client and is
//! dispatched here before anything else, on the fast path. Output goes through `adage_term`
//! (docs/design/style.md); words follow docs/lexicon.md.
//!
//! Environment: `ADAGE_HOME` (data directory), `ADAGE_ARM` (`treated`|`holdout`, overrides
//! assignment), `ADAGE_HOLDOUT` (holdout share; overrides `runtime.holdout`, default 0.1),
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
mod share;
mod share_pack;
mod survey;
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
        Some(Err(e)) => {
            // `error  what failed`, then each cause (adage_term::Out::error).
            let causes: Vec<String> = e.chain().skip(1).map(|c| c.to_string()).collect();
            let mut err = adage_term::Out::stderr();
            if err.error(&e.to_string(), &causes, None).is_err() {
                eprintln!("adage: {e:#}");
            }
            std::process::exit(1);
        }
        None => std::process::exit(1),
    }
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
        let was = match (args[0].as_str(), args.get(1)) {
            ("graph", _) => "graph --serve".to_string(),
            (v @ ("review" | "exec"), Some(s)) => format!("{v} {s}"),
            (v, _) => v.to_string(),
        };
        let mut e = adage_term::Out::stderr();
        e.error(&format!("`adage {was}` is now `{now}`"), &[], Some(&format!("see `{now} --help`")))?;
        std::process::exit(2);
    }
    let matches =
        match cli::command(&theme).try_get_matches_from(std::iter::once("adage".to_string()).chain(args.clone())) {
            Ok(m) => m,
            Err(e) => e.exit(),
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
        Some("condition") => learn(rest, &paths),
        Some("config") => config::run(rest, &paths),
        Some("status") => status(&paths, has("--json")),
        Some("report") => report(&paths, has("--json")),
        Some("share") => share::share(rest, &paths),
        Some("add") => add::add(rest, &paths),
        Some("review") => {
            // The lexicon's verbs, onto the decisions `review.rs` records.
            let verb = sub.and_then(|(_, m)| m.subcommand_name());
            match verb {
                Some("delegate") => exec::review("allow-exec", &rest[1..], &paths),
                Some("undelegate") => exec::review("revoke-exec", &rest[1..], &paths),
                Some(v) => {
                    let internal = match v {
                        "strengthen" => "promote",
                        "weaken" => "demote",
                        "extinguish" => "reject",
                        other => other,
                    };
                    let mut a = rest.to_vec();
                    if let Some(p) = a.iter().position(|x| x == v) {
                        a[p] = internal.to_string();
                    }
                    review::review(&a, &paths)
                }
                None => review::review(rest, &paths),
            }
        }
        Some("delegate") => {
            let action = if has("--status") {
                "status"
            } else if has("--off") {
                "disable"
            } else {
                "enable"
            };
            let a: Vec<String> = std::iter::once(action.to_string())
                .chain(rest.iter().filter(|x| *x != "--status" && *x != "--off").cloned())
                .collect();
            exec::exec(&a, &paths)
        }
        Some("map") => map::run(rest, &paths),
        Some("graph") => graph::graph(rest, &paths),
        Some("eval") => adage_eval::cli::run(rest),
        Some("models") => models::run(rest, &paths),
        Some("sense") => survey::survey(rest, &paths),
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
        // Bare `adage` at a terminal opens the app; anywhere else (a pipe, an agent's shell) it
        // prints the overview, as `--help` always does.
        None if ui::interactive() => ui::app(&[], &paths),
        _ => {
            cli::command(&theme).print_help()?;
            Ok(())
        }
    }
}

/// `adage condition [--transcripts <file>...] [--claude-projects <dir>] [--codex-sessions <dir>] [--cwd-prefix <path>]
///               [--outcomes <json>] [--repo <key>] [--demo] [--no-cache] [--engine <miners|shadow|graph>] [--json]`
///
/// Thresholds: `--demo`, else `learn.thresholds` (`adage config`). Parses are cached under
/// `$ADAGE_HOME/cache/sessions` unless `--no-cache` or `learn.parse_cache = false` (the result is
/// the same).
fn learn(args: &[String], paths: &Paths) -> Result<()> {
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
        bail!("no transcripts found");
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
    // The learning engine (Track G): `miners` (the default), `shadow` or `graph`.
    if let Some(e) = flag(args, "--engine") {
        if !adage_tracks::params::ENGINES.contains(&e.as_str()) {
            bail!("--engine must be one of {:?}", adage_tracks::params::ENGINES);
        }
        cfg.engine = e;
    }
    // Instincts are bound after mining, per repository, unless turned off (config, or --no-instincts).
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
            let line = learnlog::error_line(learnlog::now_ms(), "manual", None, "learn", &learnlog::describe(&e));
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
        let run = condition_out::Run {
            command: "condition",
            report: &rep,
            after: &after,
            changes: &changes,
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        condition_out::render(&mut adage_term::Out::stdout(), &run)?;
    }
    Ok(())
}

/// `adage condition --instincts-only [--repo-path <dir>] [--json]`: bind a repository's instincts
/// from its declared files, with no sessions (day 0), and compile the index. The repository is
/// `--repo-path` (default: the current directory), resolved to its root.
pub(crate) fn instincts_only(args: &[String], paths: &Paths) -> Result<()> {
    let started = std::time::Instant::now();
    let dir = match flag(args, "--repo-path").or_else(|| flag(args, "--repo-root")) {
        Some(d) => PathBuf::from(d),
        None => std::env::current_dir()?,
    };
    let dir = dir.canonicalize().map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    let root = adage_core::repo::find_root(&dir).unwrap_or(dir);
    let repo = adage_core::repo::repo_key_cached(&root, &paths.repo_keys());
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
    let after: BTreeMap<String, adage_learn::diff::Snapshot> = BTreeMap::from([(repo.clone(), after)]);
    let run = condition_out::Run {
        command: "condition --instincts-only",
        report: &rep,
        after: &after,
        changes: &changes,
        elapsed_ms: started.elapsed().as_millis() as u64,
    };
    condition_out::render(&mut adage_term::Out::stdout(), &run)?;
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

/// `adage status [--json]`: the reflexes and instincts in effect, with strength and evidence.
fn status(paths: &Paths, json: bool) -> Result<()> {
    use adage_term::{Glyph, Line, Out, Role};
    let tracks = io::read_all(&paths.tracks())?;
    let has_intents =
        tracks.iter().any(|t| t.routes.iter().any(|r| matches!(r.trigger, adage_tracks::Trigger::Intent { .. })));
    let model =
        adage_intent::model::StaticModel::open(&adage_intent::model::dir_in(&paths.models())).ok().map(|m| m.id);
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
            .map(|t| {
                serde_json::json!({
                    "track": t.manifest.track.name,
                    "version": t.manifest.track.version,
                    "repo": t.manifest.scope.repo,
                    "origin": origin(t),
                    "reflexes": t.routes.iter().map(|r| serde_json::json!({
                        "id": r.id,
                        "kind": if instinct(t, r) { "instinct" } else { "reflex" },
                        "strength": words::strength(r.aspect).word(),
                        "lifecycle": words::lifecycle(r.state),
                        "support": r.evidence.support,
                        "opportunities": r.evidence.opportunities,
                        "rate": r.evidence.rate,
                        "lower_bound": r.evidence.lower_bound,
                        "effect": format!("{:?}", r.effect).to_lowercase(),
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
    match &model {
        Some(id) => o.kv("intent model", Line::new().text(id))?,
        None if has_intents => o.kv(
            "intent model",
            Line::new().push(Role::Prime, "not installed").sep().quiet("intent reflexes cannot fire"),
        )?,
        None => {}
    }
    if tracks.is_empty() {
        o.note(&format!("No tracks yet in {}.", paths.tracks().display()))?;
        o.blank()?;
        o.next("adage condition", "form reflexes from your agents' past traces")?;
        return Ok(());
    }
    let id_w =
        tracks.iter().flat_map(|t| t.routes.iter().map(|r| r.id.chars().count())).max().unwrap_or(16).clamp(16, 34);
    let mut nascent = 0;
    for t in &tracks {
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
            let mut l = Line::new()
                .text("  ")
                .strength(words::strength(r.aspect))
                .pad_to(13)
                .glyph(Role::Accent, glyph)
                .text(" ")
                .text(r.id.clone())
                .pad_to(16 + id_w)
                .extend(words::evidence(e));
            if r.state != adage_tracks::RouteState::Active {
                l = l.sep().quiet(words::lifecycle(r.state));
            }
            if r.state == adage_tracks::RouteState::Draft {
                nascent += 1;
            }
            o.line(&l)?;
        }
    }
    if nascent > 0 {
        o.blank()?;
        let why = if nascent == 1 {
            "1 nascent reflex waits for you".to_string()
        } else {
            format!("{nascent} nascent reflexes wait for you")
        };
        o.next("adage review", &why)?;
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
            "derailed" | "followed_failed" | "misfired" => Role::Inhibit,
            _ => Role::Ink,
        };
        o.kv(&words::prose(&k.replace('_', " ")), Line::new().push(role, v.to_string()))?;
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
