//! `adage init [--repo <path>] [--yes | --print]` (plan §6.3): first run, in one command.
//!
//! 1. Detects the installed harnesses (read-only probing; see [`crate::detect`]).
//! 2. Shows how to register Adage's hooks with each: one line per harness with the `--write`
//!    command that would install it (`adage install <harness>` prints the snippet). `init` itself
//!    never writes a harness's configuration.
//! 3. The embedding model: the one bundled in this binary serves; a selected model that is not
//!    installed is fetched with `--yes` (`adage models fetch <key>`).
//! 4. Lists the repositories Claude Code has sessions in (from its project directories; no
//!    transcript is parsed), with the current one ticked. With `--yes`, the current one is added
//!    to the ones Adage conditions on (`learn.repos`, [`crate::repos`]).
//! 5. Runs `adage sense` over the current repository's history (read-only).
//! 6. Shows which instincts would bind here, from the repository's files; with `--yes`, binds them.
//!
//! At a terminal (stdin and stdout both terminals, outside any agent session, and without
//! `--print` or `--yes`) `init` is a guided wizard instead (`crate::ui::wizard`): it asks
//! before each step, shows the exact file and diff before installing hooks, and can fetch the
//! model, run a first `adage condition`, and turn auto-learning on.
//!
//! Printing is allowed anywhere, including inside an agent session. `--yes` downloads into the
//! data directory, so it needs a person at a terminal and refuses inside an agent session.

use crate::detect;
use adage_core::repo;
use adage_store::config::Source;
use adage_store::Paths;
use adage_term::{Line, Out, Role};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

fn agent_session() -> Option<&'static str> {
    crate::install::AGENT_ENV.iter().copied().find(|v| std::env::var_os(v).is_some())
}

pub fn init(args: &[String], paths: &Paths) -> Result<()> {
    let yes = args.iter().any(|a| a == "--yes");
    let print = args.iter().any(|a| a == "--print");
    if let Some(bad) =
        args.iter().find(|a| a.starts_with("--") && !matches!(a.as_str(), "--yes" | "--repo" | "--print"))
    {
        bail!("unknown option {bad}\nusage: adage init [--repo <path>] [--yes | --print]");
    }
    let agent = agent_session();
    // A person at a terminal gets the guided wizard; `--print`, `--yes`, a pipe, or an agent's
    // shell get the plain report below.
    if !yes && !print && agent.is_none() && crate::ui::interactive() {
        let repo = crate::flag(args, "--repo").map(PathBuf::from);
        return crate::ui::wizard(repo, paths);
    }
    if yes {
        if let Some(v) = agent {
            return Err(crate::hint(
                format!("`adage init --yes` will not run inside an agent session ({v} is set)"),
                "it downloads into Adage's data directory: run it yourself, in a terminal, or run `adage init` to print only",
            ));
        }
        crate::share::require_person("init --yes")?;
    }
    let repo_arg = crate::flag(args, "--repo").map(PathBuf::from);
    let start = match &repo_arg {
        Some(p) => p.canonicalize().with_context(|| format!("--repo {} does not exist", p.display()))?,
        None => std::env::current_dir()?,
    };
    let root = repo::find_root(&start);
    if repo_arg.is_some() && root.is_none() {
        bail!("{} is not inside a git repository", start.display());
    }
    let repo_key = root.as_deref().map(repo::repo_key);

    let mut out = Out::stdout();
    let label = repo_key.clone().unwrap_or_else(|| "this machine".into());
    out.header("init", &[&label, if yes { "setting up" } else { "printing only" }])?;
    out.note("Adage ships with instincts and forms reflexes from your traces. This shows each step of setting it up; nothing is written unless you pass --yes.")?;
    out.blank()?;
    out.line(&Line::new().faint("It will:"))?;
    out.line(&Line::new().faint("  1. detect the agent harnesses installed here (read-only);"))?;
    out.line(&Line::new().faint(if yes {
        "  2. show how to register Adage's hooks with each (--yes does not register them: `adage install <harness> --write` does);"
    } else {
        "  2. show how to register Adage's hooks with each (`adage install <harness>`; nothing is written);"
    }))?;
    out.line(&Line::new().faint(format!(
        "  3. {} the optional embedding model for intent families (`adage models fetch`);",
        if yes { "download" } else { "offer" }
    )))?;
    out.line(&Line::new().faint(format!(
        "  4. list the repositories your Claude Code sessions were in{};",
        if yes && root.is_some() { ", and condition on this one" } else { " (nothing is read yet)" }
    )))?;
    out.line(&Line::new().faint(format!(
        "  5. sense {} existing traces: which reflexes they would form (read-only);",
        if root.is_some() { "this repository's" } else { "a repository's" }
    )))?;
    out.line(&Line::new().faint(format!(
        "  6. show which instincts (Adage's built-in reflexes) would bind in this repository, from its files alone{}.",
        if yes { ", and bind them" } else { "" }
    )))?;
    if let Some(v) = agent {
        out.note(&format!("Inside an agent session ({v} is set): printing only."))?;
    }
    out.flush()?;
    let step = |n: u8, title: &str| -> Result<()> {
        let mut o = Out::stdout();
        o.blank()?;
        o.line(&Line::new().bold(Role::Accent, format!("{n}")).pad_to(3).bold(Role::Ink, title))?;
        o.flush()?;
        Ok(())
    };

    // 1. Harnesses.
    step(1, "Harnesses")?;
    let found = detect::harnesses();
    let mut o = Out::stdout();
    // `  claude-code  found: …`: one row per harness, then its details under the same column.
    let row = |h: &str| Line::new().text("  ").text(h).pad_to(15);
    for d in &found {
        if d.found() {
            o.line(&row(d.harness).push(Role::Fire, "found").quiet(format!(": {}", d.evidence.join("; "))))?;
        } else {
            o.line(&row(d.harness).faint("not found"))?;
        }
    }

    // 2. Hook registration.
    step(2, "Hooks")?;
    let regs = detect::registrations(root.as_deref());
    let repo_opt = repo_arg.as_ref().map(|p| format!(" --repo {}", p.display())).unwrap_or_default();
    let installed: Vec<&detect::Detected> = found.iter().filter(|d| d.found()).collect();
    if installed.is_empty() {
        o.line(
            &Line::new()
                .text("  ")
                .quiet("No harness found. When you install one, ")
                .cmd("adage install <claude-code|opencode|codex>")
                .quiet(" prints its registration."),
        )?;
    }
    for d in installed {
        let h = d.harness;
        o.blank()?;
        for r in regs.iter().filter(|r| r.harness == h) {
            let bin = r.binary.as_ref().map(|b| detect::tilde(b)).unwrap_or_else(|| "?".into());
            o.line(
                &row(h)
                    .push(Role::Fire, "registered")
                    .quiet(format!(" in {} (runs {bin})", detect::tilde(&r.location)))
                    .sep()
                    .cmd("adage doctor")
                    .quiet(" checks it"),
            )?;
        }
        let here =
            if root.is_some() { "this repository" } else { "a repository (run it inside one, or pass --repo <path>)" };
        o.line(
            &row(h)
                .cmd(format!("adage install {h} --write{repo_opt}"))
                .quiet(format!(" registers Adage for {here}; add "))
                .cmd("--scope user")
                .quiet(" for every repository"),
        )?;
        match h {
            "claude-code" => o.line(
                &row("")
                    .quiet("or install the plugin: ")
                    .cmd("/plugin marketplace add stevenvaughanbooth/adage")
                    .quiet(", then ")
                    .cmd("/plugin install adage@adage")
                    .quiet("; ")
                    .cmd(format!("adage install {h}"))
                    .quiet(" prints the settings it would write"),
            )?,
            "opencode" => o.line(
                &row("")
                    .quiet("it writes Adage's plugin file (.opencode/plugins/adage.js); ")
                    .cmd("adage install opencode")
                    .quiet(" prints it in full"),
            )?,
            _ => o.line(
                &row("")
                    .quiet(
                        "it merges Adage's entries into hooks.json (Codex then asks you to approve them in /hooks); ",
                    )
                    .cmd("adage install codex")
                    .quiet(" prints them"),
            )?,
        }
    }

    // 3. The embedding model: hooks embed prompts with the installed one, else the one bundled in
    // this binary (`model::resolve`).
    step(3, "Embedding model")?;
    let r = adage_intent::model::resolve(&paths.models(), false);
    match r.source {
        adage_intent::model::Source::Hashed { .. } if yes && r.selected != "hashed" => {
            // The selected model (`ADAGE_EMBED_MODEL`, `adage models use`), else the default.
            let spec = adage_intent::model::spec(&r.selected).unwrap_or(&adage_intent::model::BASE_8M);
            o.line(&row("model").quiet(format!(
                "fetching {} ({:.0} MB, SHA-256 verified)…",
                spec.key,
                spec.bytes() as f64 / 1e6
            )))?;
            o.flush()?;
            if let Err(e) = crate::models::run(&["fetch".to_string(), spec.key.to_string()], paths) {
                o.warn(&format!("could not fetch it: {e:#}"))?;
                o.line(
                    &Line::new()
                        .quiet("Everything else works without it; retry with ")
                        .cmd("adage models fetch")
                        .quiet(", or ")
                        .cmd("adage models install <dir>")
                        .quiet(" offline."),
                )?;
            }
        }
        adage_intent::model::Source::Hashed { .. } => {
            o.line(&row("model").push(Role::Prime, r.describe()))?;
            o.line(
                &row("")
                    .cmd("adage models fetch")
                    .quiet(" downloads potion-base-8M (31 MB, SHA-256 verified) for intent families; ")
                    .cmd("adage init --yes")
                    .quiet(" does it for you"),
            )?;
        }
        _ => {
            o.line(&row("model").push(Role::Fire, r.describe()).sep().quiet("nothing to do"))?;
            o.line(
                &row("")
                    .cmd("adage models fetch")
                    .quiet(" installs it at full precision; ")
                    .cmd("adage models fetch potion-code-16m-v2")
                    .quiet(" adds the code model"),
            )?;
        }
    }
    o.flush()?;

    // 4. Repositories: which ones Adage conditions on.
    step(4, "Repositories to condition on")?;
    repositories(root.as_deref(), repo_key.as_deref(), yes, paths)?;

    // 5. Sense this repository's traces.
    step(5, "Sense this repository's traces")?;
    match &root {
        Some(r) => {
            o.blank()?;
            o.flush()?;
            let opts = crate::sense::Options { root: Some(r.clone()), ..Default::default() };
            let s = crate::sense::run(&opts, paths)?;
            crate::sense::print_text(&s)?;
        }
        None => o.line(
            &Line::new()
                .text("  ")
                .quiet("Not inside a repository: run it inside one, or ")
                .cmd("adage sense --all")
                .quiet(" for every repository's traces."),
        )?,
    }

    // 6. Instincts: what Adage's built-in defaults would bind here, from this repository's files.
    step(6, "Instincts (day 0)")?;
    instincts(root.as_deref(), yes, paths)?;
    let mut out = Out::stdout();
    out.blank()?;
    // Where hooks stand: `init` never registers them, so a scripted `--yes` must not read as
    // finished setup when agents will not hear from Adage.
    let regs = detect::registrations(root.as_deref());
    let unhooked = installed_without_hooks(&found, &regs);
    match regs.first() {
        Some(_) => {
            let mut at: Vec<String> = Vec::new();
            for r in &regs {
                let place = if r.kind == "plugin-data" {
                    format!("{} (the Adage plugin)", r.harness)
                } else {
                    format!("{} ({})", r.harness, detect::tilde(&r.location))
                };
                if !at.contains(&place) {
                    at.push(place);
                }
            }
            out.kv("hooks", Line::new().push(Role::Fire, "registered").quiet(format!(": {}", at.join(", "))))?;
        }
        None => match unhooked.first() {
            Some(h) => out.kv(
                "hooks",
                Line::new()
                    .push(Role::Prime, "not registered")
                    .quiet(format!(": {h} sessions will not hear from Adage until they are")),
            )?,
            None => out.kv("hooks", Line::new().quiet("no harness found to register them with"))?,
        },
    }
    out.blank()?;
    let register = unhooked.first().filter(|_| regs.is_empty()).map(|h| format!("adage install {h} --write{repo_opt}"));
    if agent.is_none() && !yes {
        out.next("adage init", "in your own terminal, to set this up step by step")?;
        out.also("adage doctor", "checks the whole setup at any time")?;
    } else if let Some(cmd) = register.filter(|_| agent.is_none()) {
        out.next(&cmd, "registers the hooks for this repository (add --scope user for every repository)")?;
        out.also("adage doctor", "checks the whole setup at any time")?;
    } else {
        out.next("adage doctor", "checks the whole setup at any time")?;
        out.also("adage watch", "live pane for a split terminal")?;
    }
    let (needs, look) = if yes { crate::waiting(paths) } else { (0, 0) };
    if let Some(why) = crate::review::waiting_words(needs, look) {
        out.also("adage review", &why)?;
    }
    out.flush()?;
    Ok(())
}

/// The most repositories listed; the rest are counted.
const REPOS_SHOWN: usize = 12;

/// The repositories Claude Code has sessions in, ticked when Adage conditions on them
/// (`learn.repos`, with the current one ticked). With `--yes` inside a repository, the ticked ones
/// are stored, unless every repository was chosen on purpose (`learn.repos = "all"`).
fn repositories(root: Option<&std::path::Path>, here: Option<&str>, yes: bool, paths: &Paths) -> Result<()> {
    let mut o = Out::stdout();
    let found = crate::repos::found(paths);
    let chosen = crate::repos::preselected(paths, &found, here);
    let rows = crate::repos::rows(found, &chosen, here.zip(root));
    let now = crate::learnlog::now_ms();
    let w = rows.iter().take(REPOS_SHOWN).map(|f| f.label().chars().count()).max().unwrap_or(0) + 2;
    for f in rows.iter().take(REPOS_SHOWN) {
        let on = chosen.contains(&f.key);
        let mut l = Line::new().text("  ").text(if on { "[x] " } else { "[ ] " });
        l = if on { l.bold(Role::Ink, f.label()) } else { l.text(f.label()) };
        l = l.pad_to(6 + w).quiet(crate::repos::usage(f, now));
        if Some(f.key.as_str()) == here {
            l = l.sep().faint("this repository");
        }
        o.line(&l)?;
    }
    if rows.len() > REPOS_SHOWN {
        o.line(&Line::new().text("  ").faint(format!("… and {} more", rows.len() - REPOS_SHOWN)))?;
    }
    if rows.is_empty() {
        o.line(&Line::new().text("  ").quiet("No Claude Code sessions found yet."))?;
    }
    let loaded = adage_store::config::load(paths);
    let all_on_purpose =
        loaded.config.learn_repos.is_none() && loaded.get("learn.repos").is_some_and(|(_, s)| *s != Source::Default);
    let say = |o: &mut Out<_>, l: Line| o.line(&Line::new().text("  ").extend(l));
    if all_on_purpose {
        say(&mut o, Line::new().quiet("learn.repos is all: Adage conditions on every repository you work in."))?;
        o.flush()?;
        return Ok(());
    }
    if yes && root.is_some() {
        crate::repos::choose(paths, &chosen)?;
        say(
            &mut o,
            Line::new()
                .quiet(format!(
                    "Adage conditions on the {} ticked (learn.repos); ",
                    if chosen.len() == 1 { "one" } else { "ones" }
                ))
                .cmd("adage init")
                .quiet(" at a terminal picks others."),
        )?;
    } else {
        say(
            &mut o,
            Line::new()
                .quiet("Adage reads only the repositories ticked, and only their Claude Code sessions (Codex's once its hooks are registered). ")
                .cmd("adage init")
                .quiet(" at a terminal lets you pick; ")
                .cmd("--yes")
                .quiet(" takes these."),
        )?;
    }
    say(&mut o, Line::new().faint("Sessions in other repositories are recorded, not read."))?;
    o.flush()?;
    Ok(())
}

/// Harnesses found on this machine with no registration of Adage's hooks.
fn installed_without_hooks(found: &[detect::Detected], regs: &[detect::Registration]) -> Vec<&'static str> {
    found.iter().filter(|d| d.found() && !regs.iter().any(|r| r.harness == d.harness)).map(|d| d.harness).collect()
}

/// Show what would bind for the repository (a dry bind, nothing written), and offer
/// `adage condition --instincts-only`; with `--yes`, run it.
fn instincts(root: Option<&std::path::Path>, yes: bool, paths: &Paths) -> Result<()> {
    let mut o = Out::stdout();
    let say = |o: &mut Out<_>, l: Line| o.line(&Line::new().text("  ").extend(l));
    let Some(root) = root else {
        say(
            &mut o,
            Line::new()
                .quiet("Not inside a repository: instincts bind per repository (run ")
                .cmd("adage init")
                .quiet(" inside one)."),
        )?;
        return Ok(());
    };
    let settings = adage_store::config::load(paths).config;
    if !settings.instincts_enabled {
        say(
            &mut o,
            Line::new().faint("off").sep().cmd("adage config set instincts.enabled true").quiet(" turns them on"),
        )?;
        return Ok(());
    }
    // A dry bind through the instinct packs turned on: the base packs of the marketplace built into
    // Adage (installed by the bind itself) and any added with `adage add`.
    let ic = crate::autolearn::instinct_config(&settings, paths);
    let (packs, _) = adage_learn::instincts::packs(&ic, &paths.tracks());
    let refs: Vec<&adage_pack::Pack> = packs.iter().collect();
    let bound = adage_pack::instincts::bind(&refs, root, &[], &Default::default(), &ic.disabled_ids());
    let here: Vec<&str> =
        packs.iter().map(|p| p.manifest.pack.name.as_str()).filter(|n| bound.of_pack(n).next().is_some()).collect();
    say(
        &mut o,
        Line::new()
            .quiet(format!(
                "{} on, from the adage marketplace built into Adage; ",
                crate::words::count(packs.len(), "instinct pack")
            ))
            .text(if here.is_empty() {
                "none applies here".to_string()
            } else {
                format!("{} here: {}", here.len(), here.join(", "))
            })
            .quiet(" (")
            .cmd("adage packs")
            .quiet(" to see or turn them off)"),
    )?;
    let dry = bound.into_result();
    if dry.routes.is_empty() {
        say(&mut o, Line::new().quiet("Nothing in this repository's files matches an instinct."))?;
        return Ok(());
    }
    say(
        &mut o,
        Line::new().quiet(format!(
            "From this repository's files alone, with no traces, Adage would bind {}:",
            crate::words::count(dry.routes.len(), "reflex")
        )),
    )?;
    let w = dry.origins.values().map(|s| s.chars().count()).max().unwrap_or(1);
    let origin = |r: &adage_tracks::Route| dry.origins.get(&r.id).cloned().unwrap_or_else(|| "?".into());
    // One line per instinct and step; what sets the reflexes of a line apart is their trigger.
    let groups = crate::words::group(dry.routes.iter(), |r| {
        format!("{}\n{}\n{:?}", origin(r), crate::share::describe_step(&r.step), r.state)
    });
    for g in &groups {
        let r = g[0];
        let when = crate::words::trigger(&r.trigger);
        let mut l = Line::new()
            .text("  ")
            .glyph(Role::Accent, adage_term::Glyph::Instinct)
            .text(" ")
            .text(origin(r))
            .pad_to(8 + w)
            .text(crate::share::describe_step(&r.step))
            .sep()
            .quiet(if g.len() == 1 { format!("when {when}") } else { format!("{} triggers, e.g. {when}", g.len()) });
        if r.state == adage_tracks::RouteState::Nascent {
            l = l.sep().faint("nascent").quiet(": needs your approval in ").cmd("adage review");
        }
        say(&mut o, l)?;
    }
    say(
        &mut o,
        Line::new().quiet(
            "Each is prime at most, never blocks the agent from stopping, and gives way to any reflex Adage conditions.",
        ),
    )?;
    let nascent = dry.routes.iter().any(|r| r.state == adage_tracks::RouteState::Nascent);
    if nascent {
        say(
            &mut o,
            Line::new()
                .quiet("Environment steps (installs) stay nascent until you approve one in ")
                .cmd("adage review")
                .quiet("; approving one approves the pack's environment steps for this repository."),
        )?;
    }
    let cmd = format!("adage condition --instincts-only --repo-path {}", detect::tilde(root));
    if yes {
        say(&mut o, Line::new().quiet("binding").sep().cmd(cmd))?;
        o.flush()?;
        // Its own `next` would come before init's: init ends with the one that applies.
        crate::instincts_only_then(&["--repo-path".to_string(), root.to_string_lossy().into_owned()], paths, false)?;
    } else {
        say(
            &mut o,
            Line::new()
                .cmd(cmd)
                .quiet(" binds them now (")
                .cmd("adage init --yes")
                .quiet(" does it for you); otherwise the first ")
                .cmd("adage condition")
                .quiet(" does."),
        )?;
    }
    o.flush()?;
    Ok(())
}
