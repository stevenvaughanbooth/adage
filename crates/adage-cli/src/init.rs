//! `adage init [--repo <path>] [--yes | --print]` (plan §6.3): first run, in one command.
//!
//! 1. Detects the installed harnesses (read-only probing; see [`crate::detect`]).
//! 2. Shows how to register Adage's hooks with each: `adage install <harness>`'s printed snippet,
//!    and the `--write` command that would install it. `init` itself never writes a harness's
//!    configuration.
//! 3. Offers the optional embedding model: prints `adage models fetch`, and runs it only with
//!    `--yes`.
//! 4. Runs `adage sense` over existing history (read-only), for the repository at `--repo` if
//!    given, else for every repository.
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
            bail!("`adage init --yes` downloads into Adage's data directory, so it will not run inside an agent session ({v} is set). Run it yourself, in a terminal, or run `adage init` to print only.");
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
    out.line(
        &Line::new().faint(
            "  2. show how to register Adage's hooks with each (`adage install <harness>`; nothing is written);",
        ),
    )?;
    out.line(&Line::new().faint(format!(
        "  3. {} the optional embedding model for intent families (`adage models fetch`);",
        if yes { "download" } else { "offer" }
    )))?;
    out.line(&Line::new().faint(format!(
        "  4. sense your existing agent traces{} — read-only;",
        match (&repo_arg, &repo_key) {
            (Some(_), Some(k)) => format!(" for {k}"),
            _ => String::new(),
        }
    )))?;
    out.line(&Line::new().faint(format!(
        "  5. show which instincts would bind in this repository, from its files alone{}.",
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
    for d in &found {
        if d.found() {
            println!("  {:<12} found: {}", d.harness, d.evidence.join("; "));
        } else {
            println!("  {:<12} not found", d.harness);
        }
    }

    // 2. Hook registration.
    step(2, "Hooks")?;
    let regs = detect::registrations(root.as_deref());
    let repo_opt = repo_arg.as_ref().map(|p| format!(" --repo {}", p.display())).unwrap_or_default();
    let installed: Vec<&detect::Detected> = found.iter().filter(|d| d.found()).collect();
    if installed.is_empty() {
        println!("  No harness found. When you install one: `adage install <claude-code|opencode|codex>` prints its registration.");
    }
    for d in installed {
        let h = d.harness;
        println!();
        for r in regs.iter().filter(|r| r.harness == h) {
            let bin = r.binary.as_ref().map(|b| detect::tilde(b)).unwrap_or_else(|| "?".into());
            println!(
                "  {h}: already registered in {} (runs {bin}); `adage doctor` checks it",
                detect::tilde(&r.location)
            );
        }
        let here =
            if root.is_some() { "this repository" } else { "a repository (run it inside one, or pass --repo <path>)" };
        println!("  {h}: `adage install {h} --write{repo_opt}` registers Adage for {here}; add `--scope user` for every repository.");
        match h {
            "claude-code" => println!("  {h}: or use the plugin: `claude --plugin-dir plugin/claude-code` (see README). The settings it would write:"),
            "opencode" => {
                println!("  {h}: it writes Adage's plugin file (.opencode/plugins/adage.js); `adage install opencode` prints it in full.");
                continue;
            }
            _ => println!("  {h}: the hooks.json entries it would merge (Codex then asks you to approve them in /hooks):"),
        }
        crate::install::install(&[h.to_string(), "--print".to_string()])?;
    }

    // 3. The embedding model.
    step(3, "Embedding model (optional)")?;
    match adage_intent::model::status(&paths.models()) {
        Ok(m) => println!("  installed: {}", m.id),
        Err(_) if yes => {
            println!("  fetching (31 MB, SHA-256 verified)…");
            if let Err(e) = crate::models::run(&["fetch".to_string()], paths) {
                println!("  could not fetch it: {e:#}");
                println!("  Everything else works without it; retry with `adage models fetch`, or `adage models install <dir>` offline.");
            }
        }
        Err(_) => println!(
            "  not installed. `adage models fetch` downloads it (31 MB, SHA-256 verified) to enable intent families; `adage init --yes` does it for you. Everything else works without it."
        ),
    }

    // 4. Sense.
    step(4, "Sense your traces")?;
    println!();
    let opts = crate::survey::Options { repo: repo_arg.as_ref().and(repo_key.clone()), ..Default::default() };
    let s = crate::survey::run(&opts, paths)?;
    crate::survey::print_text(&s)?;

    // 5. Instincts: what Adage's built-in defaults would bind here, from this repository's files.
    step(5, "Instincts (day 0)")?;
    instincts(root.as_deref(), yes, paths)?;
    let mut out = Out::stdout();
    out.blank()?;
    if agent.is_none() && !yes {
        out.next("adage init", "in your own terminal, to set this up step by step")?;
        out.also("adage doctor", "checks the whole setup at any time")?;
    } else {
        out.next("adage doctor", "checks the whole setup at any time")?;
        out.also("adage watch", "live pane for a split terminal")?;
    }
    out.flush()?;
    Ok(())
}

/// Show what would bind for the repository (a dry bind, nothing written), and offer
/// `adage condition --instincts-only`; with `--yes`, run it.
fn instincts(root: Option<&std::path::Path>, yes: bool, paths: &Paths) -> Result<()> {
    let Some(root) = root else {
        println!("  Not inside a repository: instincts bind per repository (run `adage init` inside one).");
        return Ok(());
    };
    let settings = adage_store::config::load(paths).config;
    if !settings.instincts_enabled {
        println!("  Off (`adage config set instincts.enabled true` turns them on).");
        return Ok(());
    }
    let opts = adage_instincts::BindOptions { disabled: settings.instincts_disabled.iter().cloned().collect() };
    let dry = adage_instincts::bind_with(root, &[], &Default::default(), &opts);
    if dry.routes.is_empty() {
        println!("  Nothing in this repository's files matches a built-in default.");
        return Ok(());
    }
    println!("  From this repository's files alone, with no session history, Adage would bind:");
    for r in &dry.routes {
        let instinct = dry.origins.get(&r.id).map(String::as_str).unwrap_or("?");
        let draft =
            if r.state == adage_tracks::RouteState::Draft { "  (needs your approval in `adage review`)" } else { "" };
        println!("    {instinct:<3} {}{draft}", crate::share::describe_step(&r.step));
    }
    println!("  Each primes at most, never blocks the agent from stopping, and gives way to anything Adage learns.");
    let drafts = dry.routes.iter().any(|r| r.state == adage_tracks::RouteState::Draft);
    if drafts {
        println!("  Environment steps (installs) stay drafts until you approve one in `adage review`; approving one approves the pack's environment steps for this repository.");
    }
    let cmd = format!("adage condition --instincts-only --repo-path {}", detect::tilde(root));
    if yes {
        println!("  binding: {cmd}");
        crate::instincts_only(&["--repo-path".to_string(), root.to_string_lossy().into_owned()], paths)?;
    } else {
        println!("  `{cmd}` binds them now (`adage init --yes` does it for you); otherwise the first `adage condition` does.");
    }
    Ok(())
}
