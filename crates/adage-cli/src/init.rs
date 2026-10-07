//! `adage init [--repo <path>] [--yes [--hooks user|project|none] | --print]` (onboarding plan
//! O4): first run, in one command, and safe to run again.
//!
//! 1. Harnesses: which are installed (read-only probing; see [`crate::detect`]).
//! 2. Binaries: `adage-exec` (execution) and `adage-train` (`adage train`) beside this `adage`.
//!    With `--yes` a missing one is fetched from the release of this version, each archive
//!    verified against the `.sha256` the release publishes beside it ([`crate::net`]), and
//!    `$ADAGE_HOME/channel.json` records the release; a checkout build is pointed at `cargo
//!    build`; the Claude Code plugin's own adage needs nothing (the plugin bundles all three).
//!    This comes before the hooks so the plugin is staged with every binary.
//! 3. Hooks, per harness found, written only with `--yes --hooks user|project`:
//!    - Claude Code: the plugin first. [`crate::claude_plugin`] stages it from this binary as a
//!      local marketplace under `$ADAGE_HOME/plugin`, runs `claude plugin marketplace add` and
//!      `claude plugin install adage@adage-local` (user scope; `--hooks project` installs it for
//!      this repository alone), and checks `claude plugin list`. Adage's entries in a settings
//!      file are then taken out (the diff shown, `<file>.adage-bak` kept), so every event is wired
//!      once. When `claude` is not on `PATH` or refuses, the settings file is the route
//!      (`adage install claude-code --write`), and the typed `/plugin` commands are named.
//!    - Codex and OpenCode: their hook file (`adage install <harness> --write`), diffed and backed
//!      up; Codex then asks the person to approve the hooks in `/hooks`.
//!
//!    A harness registered anywhere that applies here already is never registered a second time.
//!    `--hooks none`, or no `--hooks`, connects nothing and says so.
//! 4. The embedding model: the one bundled in this binary serves; a selected model that is not
//!    installed is fetched with `--yes` (`adage models fetch <key>`).
//! 5. The repositories Claude Code has sessions in (from its project directories; no transcript
//!    is parsed), with the current one ticked. With `--yes`, the ticked ones are the ones Adage
//!    conditions on (`learn.repos`, [`crate::repos`]).
//! 6. `adage sense` over the current repository's history (read-only).
//! 7. Which instincts would bind here, from the repository's files; with `--yes`, binds them, and
//!    starts the first `adage condition` in the background when the repository has history.
//!
//! It ends with one screen: what is connected, where the binaries are, what Adage learns from,
//! the line the next Claude Code session here opens with, and at most two `next` lines.
//!
//! At a terminal (stdin and stdout both terminals, outside any agent session, and without
//! `--print` or `--yes`) `init` is a guided wizard instead (`crate::ui::wizard`): the same steps,
//! each asked before it runs. `--yes` writes, so it needs a person at a terminal and refuses
//! inside an agent session; printing is allowed anywhere. `--restage` (hidden; what `adage
//! update` runs with the new binary) stages the installed local plugin again from this binary.

use crate::claude_plugin;
use crate::detect;
use adage_core::repo;
use adage_store::config::Source;
use adage_store::Paths;
use adage_term::{Line, Out, Role};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: adage init [--repo <path>] [--yes [--hooks user|project|none] | --print]";

fn agent_session() -> Option<&'static str> {
    crate::install::AGENT_ENV.iter().copied().find(|v| std::env::var_os(v).is_some())
}

/// Where `--hooks` registers: every repository (`user`), this one (`project`), or nowhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hooks {
    User,
    Project,
    None,
}

impl Hooks {
    fn parse(s: &str) -> Option<Hooks> {
        match s {
            "user" => Some(Hooks::User),
            "project" => Some(Hooks::Project),
            "none" => Some(Hooks::None),
            _ => None,
        }
    }
    /// The scope `adage install` writes.
    pub(crate) fn scope(self) -> &'static str {
        if self == Hooks::Project {
            "project"
        } else {
            "user"
        }
    }
    /// The scope `claude plugin install` takes: a project's own install is `local` (its
    /// `.claude/settings.local.json`, never committed), as `adage install`'s project scope is.
    pub(crate) fn plugin_scope(self) -> &'static str {
        if self == Hooks::Project {
            "local"
        } else {
            "user"
        }
    }
}

/// This binary, resolved.
fn this_exe() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot locate the running adage binary")?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

pub fn init(args: &[String], paths: &Paths) -> Result<()> {
    let yes = args.iter().any(|a| a == "--yes");
    let print = args.iter().any(|a| a == "--print");
    if let Some(bad) = args.iter().find(|a| {
        a.starts_with("--") && !matches!(a.as_str(), "--yes" | "--repo" | "--print" | "--hooks" | "--restage")
    }) {
        bail!("unknown option {bad}\n{USAGE}");
    }
    if args.iter().any(|a| a == "--restage") {
        return restage(paths);
    }
    let hooks = match crate::helpers::flag(args, "--hooks") {
        None => None,
        Some(h) => Some(Hooks::parse(&h).with_context(|| format!("--hooks must be user, project or none\n{USAGE}"))?),
    };
    let agent = agent_session();
    // A person at a terminal gets the guided wizard; `--print`, `--yes`, a pipe, or an agent's
    // shell get the plain report below.
    if !yes && !print && agent.is_none() && crate::ui::interactive() {
        let repo = crate::helpers::flag(args, "--repo").map(PathBuf::from);
        return crate::ui::wizard(repo, paths);
    }
    if yes {
        if let Some(v) = agent {
            return Err(crate::hint(
                format!("`adage init --yes` will not run inside an agent session ({v} is set)"),
                "it installs hooks and downloads into Adage's data directory: run it yourself, in a terminal, or run `adage init` to print only",
            ));
        }
        crate::share::require_person("init --yes")?;
    }
    // What `--hooks` does only happens with `--yes`; without it, the report says what it would.
    let connect = hooks.filter(|h| yes && *h != Hooks::None);
    let repo_arg = crate::helpers::flag(args, "--repo").map(PathBuf::from);
    let start = match &repo_arg {
        Some(p) => p.canonicalize().with_context(|| format!("--repo {} does not exist", p.display()))?,
        None => std::env::current_dir()?,
    };
    let root = repo::find_root(&start);
    if repo_arg.is_some() && root.is_none() {
        bail!("{} is not inside a git repository", start.display());
    }
    if hooks == Some(Hooks::Project) && root.is_none() {
        bail!("--hooks project registers for a repository: run it inside one, pass --repo <path>, or use --hooks user");
    }
    let repo_key = root.as_deref().map(repo::repo_key);
    let exe = this_exe()?;

    let mut out = Out::stdout();
    let label = repo_key.clone().unwrap_or_else(|| "this machine".into());
    out.header("init", &[&label, if yes { "setting up" } else { "printing only" }])?;
    out.note(if yes {
        "Adage ships with instincts and forms reflexes from your traces. Each step says what it does as it runs."
    } else {
        "Adage ships with instincts and forms reflexes from your traces. This shows each step of setting it up; nothing is written unless you pass --yes."
    })?;
    out.blank()?;
    out.line(&Line::new().faint("It will:"))?;
    out.line(&Line::new().faint("  1. detect the agent harnesses installed here (read-only);"))?;
    out.line(&Line::new().faint(format!(
        "  2. check that adage-exec and adage-train sit beside adage{};",
        if yes { ", and fetch any that are missing from this version's release" } else { "" }
    )))?;
    out.line(&Line::new().faint(match (yes, connect) {
        (true, Some(h)) => format!(
            "  3. connect each harness for {}: Claude Code through the Adage plugin, Codex and OpenCode through their hook files;",
            if h == Hooks::Project { "this repository" } else { "every repository" }
        ),
        (true, None) => "  3. show how to connect each harness (--yes does not register them: add --hooks user, or project);".into(),
        _ => "  3. show how to connect each harness: Claude Code through the Adage plugin, the others through their hook files (nothing is written);".into(),
    }))?;
    out.line(&Line::new().faint(format!(
        "  4. {} the optional embedding model for intent families (`adage models fetch`);",
        if yes { "download" } else { "offer" }
    )))?;
    out.line(&Line::new().faint(format!(
        "  5. list the repositories your Claude Code sessions were in{};",
        if yes && root.is_some() { ", and condition on this one" } else { " (nothing is read yet)" }
    )))?;
    out.line(&Line::new().faint(format!(
        "  6. sense {} existing traces: which reflexes they would form (read-only);",
        if root.is_some() { "this repository's" } else { "a repository's" }
    )))?;
    out.line(&Line::new().faint(format!(
        "  7. show which instincts (Adage's built-in reflexes) would bind in this repository, from its files alone{}.",
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
    for d in &found {
        if d.found() {
            o.line(&row(d.harness).push(Role::Fire, "found").quiet(format!(": {}", d.evidence.join("; "))))?;
        } else {
            o.line(&row(d.harness).faint("not found"))?;
        }
    }
    o.flush()?;

    // 2. Binaries.
    step(2, "Binaries")?;
    let bins = binaries(&exe, yes, paths)?;

    // 3. Hooks.
    step(3, "Hooks")?;
    let repo_opt = repo_arg.as_ref().map(|p| format!(" --repo {}", p.display())).unwrap_or_default();
    let mut codex_written = false;
    match connect {
        Some(h) => {
            let cwd = root.clone().unwrap_or_else(|| start.clone());
            for d in found.iter().filter(|d| d.found()) {
                let wrote = if d.harness == "claude-code" {
                    connect_claude(paths, &exe, h, root.as_deref(), &cwd)?;
                    false
                } else {
                    connect_file(d.harness, h, root.as_deref())?
                };
                codex_written |= wrote && d.harness == "codex";
            }
            if !found.iter().any(|d| d.found()) {
                say(Line::new()
                    .quiet("No harness found to connect. When you install one, run ")
                    .cmd("adage init --yes --hooks user")
                    .quiet(" again."))?;
            }
        }
        None => describe_hooks(&found, root.as_deref(), &repo_opt, paths)?,
    }

    // 4. The embedding model: hooks embed prompts with the installed one, else the one bundled in
    // this binary (`model::resolve`).
    step(4, "Embedding model")?;
    let mut o = Out::stdout();
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

    // 5. Repositories: which ones Adage conditions on.
    step(5, "Repositories to condition on")?;
    repositories(root.as_deref(), repo_key.as_deref(), yes, paths)?;

    // 6. Sense this repository's traces.
    step(6, "Sense this repository's traces")?;
    let mut history = 0usize;
    match &root {
        Some(r) => {
            o.blank()?;
            o.flush()?;
            let opts = crate::sense::Options { root: Some(r.clone()), ..Default::default() };
            let s = crate::sense::run(&opts, paths)?;
            history = s.sessions;
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

    // 7. Instincts: what Adage's built-in defaults would bind here, from this repository's files.
    step(7, "Instincts (day 0)")?;
    instincts(root.as_deref(), yes, paths)?;
    // The first conditioning, in the background, so the first session here already has what the
    // history teaches (the learner the hooks start at a turn's end, started now).
    let learning = match (&root, yes) {
        (Some(r), true) if history > 0 => match crate::autolearn::on_turn_end(paths, r, "claude", None, &exe, true) {
            Ok(crate::autolearn::Triggered::Started(_) | crate::autolearn::Triggered::Running) => Some(history),
            _ => None,
        },
        _ => None,
    };

    // One screen: what is set up, and what to do next.
    finish(paths, &found, root.as_deref(), repo_key.as_deref(), &bins, learning, yes, connect, agent, codex_written)
}

/// `  claude-code  `: a harness's row, then its details under the same column.
fn row(h: &str) -> Line {
    Line::new().text("  ").text(h).pad_to(15)
}

fn say(l: Line) -> Result<()> {
    let mut o = Out::stdout();
    o.line(&Line::new().text("  ").extend(l))?;
    o.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Binaries

/// How this `adage` came, as far as its companions are concerned.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Origin {
    /// The Claude Code plugin's adage: the plugin bundles its companions.
    Plugin,
    /// Built in a checkout of this workspace (`<root>/target/<profile>/adage`).
    Checkout { root: PathBuf, profile: String },
    /// Anywhere else: a release's companions can be fetched beside it.
    Installed,
}

/// What the Binaries step found (and fetched).
#[derive(Debug, Clone)]
pub(crate) struct Binaries {
    /// Where `adage` lives.
    pub(crate) dir: PathBuf,
    pub(crate) origin: Origin,
    /// Companions not beside `adage` (after any fetch).
    pub(crate) missing: Vec<&'static str>,
    /// What was fetched, and what could not be, as lines.
    pub(crate) fetched: Vec<&'static str>,
    pub(crate) problems: Vec<String>,
}

/// `exe` is `<root>/target/<profile>/adage` and `<root>` a git checkout with a `Cargo.toml`.
pub(crate) fn origin(exe: &Path) -> Origin {
    if detect::in_plugin_path(exe) {
        return Origin::Plugin;
    }
    match crate::update::channel::checkout(exe) {
        Some(c) => Origin::Checkout { root: c.root, profile: c.profile },
        None => Origin::Installed,
    }
}

/// The companions not beside `exe`.
pub(crate) fn missing_companions(exe: &Path) -> Vec<&'static str> {
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    claude_plugin::COMPANIONS
        .into_iter()
        .filter(|n| !detect::executable(&dir.join(format!("{n}{}", std::env::consts::EXE_SUFFIX))))
        .collect()
}

/// `cargo build` of the missing companions in a checkout, with the binary's profile.
pub(crate) fn cargo_build(root: &Path, profile: &str, missing: &[&str]) -> String {
    let profile = match profile {
        "debug" => String::new(),
        "release" => " --release".into(),
        p => format!(" --profile {p}"),
    };
    let crates: String = missing.iter().map(|m| format!(" -p {m}")).collect();
    format!("cargo build --locked --manifest-path {}{profile}{crates}", detect::command_for(&root.join("Cargo.toml")))
}

/// The one line that installs the companions from source, when no release has them.
pub(crate) const CARGO_INSTALL: &str =
    "cargo install --locked --git https://github.com/stevenvaughanbooth/adage adage-exec adage-train";

/// Fetch `missing` from the release of this version into `dir`, verified; records
/// `$ADAGE_HOME/channel.json` once one is placed.
pub(crate) fn fetch_companions(
    paths: &Paths,
    dir: &Path,
    missing: &[&'static str],
) -> (Vec<&'static str>, Vec<String>) {
    let version = env!("CARGO_PKG_VERSION");
    let scratch = paths.cache().join(format!("download-{}", std::process::id()));
    let mut fetched = Vec::new();
    let mut problems = Vec::new();
    for name in missing {
        let into = scratch.join(name);
        let got = crate::net::fetch_companion(name, version, dir, &into);
        let _ = std::fs::remove_dir_all(&into);
        match got {
            Ok(crate::net::Companion::Placed(p)) if p.parent() == Some(dir) => fetched.push(*name),
            Ok(crate::net::Companion::Placed(p)) => {
                problems.push(format!("{name}: placed at {}, not beside adage", p.display()))
            }
            Ok(crate::net::Companion::NoTarget) => problems.push(format!(
                "{name}: no release is built for {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )),
            Ok(crate::net::Companion::NotPublished(url)) => {
                problems.push(format!("{name}: release {version} has no {name} for this machine yet ({url})"))
            }
            Ok(crate::net::Companion::Failed(f)) => {
                problems.push(format!("{name}: cannot fetch it now: {} ({})", f.class, f.host))
            }
            Ok(crate::net::Companion::Mismatch { archive, want, got }) => problems.push(format!(
                "{name}: mismatch: {archive} has sha256 {got}, the release lists {}; nothing written",
                if want.is_empty() { "none".to_string() } else { want }
            )),
            Err(e) => problems.push(format!("{name}: {e:#}")),
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    if !fetched.is_empty() {
        let record = serde_json::json!({
            "channel": "release",
            "version": version,
            "at": crate::learnlog::now_ms(),
            "bin_dir": dir,
        });
        let _ = adage_store::config::write_atomic(
            &paths.channel_file(),
            &serde_json::to_vec_pretty(&record).unwrap_or_default(),
        );
    }
    (fetched, problems)
}

/// The Binaries step: say where the companions are, and fetch the missing ones with `--yes`.
fn binaries(exe: &Path, yes: bool, paths: &Paths) -> Result<Binaries> {
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let origin = origin(exe);
    let mut b = Binaries {
        dir: dir.clone(),
        origin: origin.clone(),
        missing: missing_companions(exe),
        fetched: vec![],
        problems: vec![],
    };
    let at = detect::tilde(&dir);
    if origin == Origin::Plugin {
        // `<data>/bin/adage-<sha256>`: the plugin's shim links its companions beside it.
        let data = dir.parent().map(Path::to_path_buf).unwrap_or_default();
        let staged = detect::plugin_companions(&data);
        b.missing = claude_plugin::COMPANIONS.into_iter().filter(|c| !staged.contains(c)).collect();
        if b.missing.is_empty() {
            say(Line::new().quiet("This adage is the Claude Code plugin's; the plugin staged adage-exec and adage-train beside it. Nothing to do."))?;
        } else {
            say(Line::new().quiet(format!(
                "This adage is the Claude Code plugin's; it staged no {} for this machine. Updating the plugin (/plugin in Claude Code) brings what the release bundles.",
                b.missing.join(" or ")
            )))?;
        }
        return Ok(b);
    }
    if b.missing.is_empty() {
        say(Line::new()
            .text("adage, adage-exec and adage-train")
            .quiet(format!(" are in {at}"))
            .sep()
            .quiet("nothing to do"))?;
        return Ok(b);
    }
    let names = b.missing.join(" and ");
    let what = if b.missing.len() == 1 { "is" } else { "are" };
    match &origin {
        Origin::Checkout { root, profile } => {
            say(Line::new()
                .text(format!("{names} {what} not built beside adage"))
                .quiet(format!(" (this adage is the checkout build in {at})")))?;
            say(Line::new().cmd(cargo_build(root, profile, &b.missing)).quiet(" builds them there"))?;
        }
        _ if yes => {
            say(Line::new().quiet(format!(
                "{names} {what} not beside adage in {at}: fetching from release {} (each archive checked against the release's .sha256)…",
                env!("CARGO_PKG_VERSION")
            )))?;
            let (fetched, problems) = fetch_companions(paths, &dir, &b.missing);
            for f in &fetched {
                say(Line::new()
                    .glyph(Role::Fire, adage_term::Glyph::Ok)
                    .text(format!(" {f}"))
                    .quiet(format!(" fetched into {at}, verified")))?;
            }
            for p in &problems {
                say(Line::new().push(Role::Prime, p.clone()))?;
            }
            b.missing.retain(|m| !fetched.contains(m));
            if !b.missing.is_empty() {
                say(Line::new()
                    .quiet("Everything but those works now; ")
                    .cmd(CARGO_INSTALL)
                    .quiet(" builds them from source."))?;
            }
            b.fetched = fetched;
            b.problems = problems;
        }
        _ => {
            say(Line::new().text(format!("{names} {what} not beside adage")).quiet(format!(" in {at}")))?;
            say(Line::new()
                .cmd("adage init --yes")
                .quiet(format!(
                    " fetches {} from release {} (verified against the release's checksums); or ",
                    if b.missing.len() == 1 { "it" } else { "them" },
                    env!("CARGO_PKG_VERSION")
                ))
                .cmd(CARGO_INSTALL))?;
        }
    }
    Ok(b)
}

// ---------------------------------------------------------------------------------------------
// Hooks

/// Describe each harness's route without writing anything (`--print`, or `--yes` without
/// `--hooks`).
fn describe_hooks(found: &[detect::Detected], root: Option<&Path>, repo_opt: &str, paths: &Paths) -> Result<()> {
    let mut o = Out::stdout();
    let regs = detect::registrations(root);
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
    let claude_on_path = claude_plugin::System::on_path().is_some();
    for d in installed {
        let h = d.harness;
        o.blank()?;
        for r in regs.iter().filter(|r| r.harness == h) {
            let at = if r.kind == "plugin-data" {
                "by the Adage plugin".to_string()
            } else {
                let bin = r.binary.as_ref().map(|b| detect::tilde(b)).unwrap_or_else(|| "?".into());
                format!("in {} (runs {bin})", detect::tilde(&r.location))
            };
            o.line(
                &row(h)
                    .push(Role::Fire, "registered")
                    .quiet(format!(" {at}"))
                    .sep()
                    .cmd("adage doctor")
                    .quiet(" checks it"),
            )?;
        }
        let here =
            if root.is_some() { "this repository" } else { "a repository (run it inside one, or pass --repo <path>)" };
        if h == "claude-code" {
            let plugin = regs.iter().any(|r| r.harness == h && r.kind == "plugin-data");
            if !plugin {
                if claude_on_path {
                    o.line(
                        &row(h)
                            .text("the Adage plugin")
                            .quiet(": ")
                            .cmd("adage init --yes --hooks user")
                            .quiet(format!(
                                " stages it from this adage in {} and installs it with ",
                                detect::tilde(&paths.plugin())
                            ))
                            .cmd("claude plugin install"),
                    )?;
                    o.line(
                        &row("")
                            .quiet("or, in Claude Code: ")
                            .cmd(claude_plugin::SLASH_ADD)
                            .quiet(", then ")
                            .cmd(claude_plugin::SLASH_INSTALL),
                    )?;
                } else {
                    o.line(
                        &row(h)
                            .text("the Adage plugin")
                            .quiet(", in Claude Code: ")
                            .cmd(claude_plugin::SLASH_ADD)
                            .quiet(", then ")
                            .cmd(claude_plugin::SLASH_INSTALL),
                    )?;
                }
            }
            o.line(
                &row(if plugin { h } else { "" })
                    .quiet("or the settings file: ")
                    .cmd(format!("adage install {h} --write{repo_opt}"))
                    .quiet(format!(" registers Adage for {here}; add "))
                    .cmd("--scope user")
                    .quiet(format!(" for every repository; `adage install {h}` prints what it would write")),
            )?;
            continue;
        }
        o.line(
            &row(h)
                .cmd(format!("adage install {h} --write{repo_opt}"))
                .quiet(format!(" registers Adage for {here}; add "))
                .cmd("--scope user")
                .quiet(" for every repository"),
        )?;
        match h {
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
    o.blank()?;
    o.line(
        &Line::new()
            .text("  ")
            .cmd("adage init --yes --hooks user")
            .quiet(" connects every harness found, for every repository (")
            .cmd("--hooks project")
            .quiet(": this one)."),
    )?;
    o.flush()?;
    Ok(())
}

/// Write one harness's hook file for `hooks`' scope, showing the diff first; nothing when it is
/// registered anywhere that applies here already. Returns whether it wrote.
fn connect_file(h: &str, hooks: Hooks, root: Option<&Path>) -> Result<bool> {
    let mut o = Out::stdout();
    let p = match crate::install::plan(h, hooks.scope(), root, None) {
        Ok(p) => p,
        Err(e) => {
            o.line(&row(h).push(Role::Inhibit, "not connected").quiet(format!(": {e:#}")))?;
            return Ok(false);
        }
    };
    let target = detect::tilde(&p.target);
    if p.before.as_deref() == Some(p.after.as_str()) {
        o.line(&row(h).push(Role::Fire, "registered").quiet(format!(" in {target} · nothing to write")))?;
        return Ok(false);
    }
    let elsewhere = crate::install::registered_elsewhere(h, &p.target, root);
    if let Some(r) = elsewhere.first() {
        o.line(&row(h).push(Role::Fire, "registered").quiet(format!(
            " in {} · nothing written: a second registration in {target} would run every hook twice",
            crate::install::place(r)
        )))?;
        return Ok(false);
    }
    if h == "opencode" {
        o.line(
            &row(h)
                .text(format!("{target} {}", if p.before.is_some() { "would change" } else { "would be created" }))
                .quiet(" (Adage's plugin file; ")
                .cmd("adage install opencode")
                .quiet(" prints it)"),
        )?;
    } else {
        o.line(
            &row(h).text(format!("{target} {}:", if p.before.is_some() { "would change" } else { "would be created" })),
        )?;
        crate::install::print_diff(&mut o, p.before.as_deref().unwrap_or(""), &p.after)?;
    }
    match crate::install::apply_backed_up(&p) {
        Ok(backup) => {
            o.line(&row("").push(Role::Fire, "wrote").quiet(format!(" {target}")))?;
            if let Some(b) = backup {
                o.line(&row("").quiet("the previous version is in ").text(detect::tilde(&b)))?;
            }
            if h == "codex" {
                o.line(&row("").quiet(
                    "Codex asks you to review new hooks before they run: open /hooks in Codex to approve them.",
                ))?;
            }
            Ok(true)
        }
        Err(e) => {
            o.line(&row("").push(Role::Inhibit, "not written").quiet(format!(": {e:#}")))?;
            Ok(false)
        }
    }
}

/// Claude Code: the plugin first (staged from this binary and installed with `claude plugin`),
/// then every settings-file registration it makes redundant taken out; the settings file when the
/// plugin route cannot run.
fn connect_claude(paths: &Paths, exe: &Path, hooks: Hooks, root: Option<&Path>, cwd: &Path) -> Result<()> {
    let h = "claude-code";
    let mut o = Out::stdout();
    let cli = claude_plugin::System::on_path();
    let listed = cli.as_ref().map(|c| claude_plugin::list(c, cwd));
    let present: Vec<claude_plugin::Installed> = match &listed {
        Some(Ok(l)) => l.iter().filter(|p| p.is_adage()).cloned().collect(),
        // Without `claude` to ask: what its files say, and a plugin data directory that a session
        // has staged a binary in (`plugins/data/adage-<marketplace>/bin/adage`).
        _ => {
            let mut v: Vec<claude_plugin::Installed> = detect::claude_plugins(root)
                .into_iter()
                .map(|p| claude_plugin::Installed {
                    id: p.id,
                    enabled: p.enabled,
                    scope: p.scope,
                    version: p.version,
                    read_from: None,
                })
                .collect();
            for r in detect::registrations(root).into_iter().filter(|r| r.kind == "plugin-data" && r.binary.is_some()) {
                let dir = r.location.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let id = dir.strip_prefix("adage-").map(|m| format!("adage@{m}")).unwrap_or(dir);
                if !v.iter().any(|p| p.id == id) {
                    v.push(claude_plugin::Installed {
                        id,
                        enabled: true,
                        scope: "user".into(),
                        version: String::new(),
                        read_from: None,
                    });
                }
            }
            v
        }
    };
    let plugin_scope: String;
    if let Some(p) = present.iter().find(|p| p.enabled) {
        if p.id == claude_plugin::PLUGIN_ID {
            // Ours: stage it again from this binary (it is read in place).
            let refreshed = match &cli {
                Some(c) => claude_plugin::refresh(c, paths, exe, cwd).map(|s| s.map(|s| s.changed)),
                None => claude_plugin::stage(paths, exe).map(|s| Some(s.changed)),
            };
            match refreshed {
                Ok(Some(0)) | Ok(None) => o.line(
                    &row(h)
                        .push(Role::Fire, "the Adage plugin")
                        .quiet(format!(" ({}) is installed and up to date with this adage", p.id)),
                )?,
                Ok(Some(_)) => {
                    o.line(&row(h).push(Role::Fire, "the Adage plugin").quiet(format!(
                        " ({}) is installed; staged again from this adage, for your next session",
                        p.id
                    )))?
                }
                Err(e) => o.line(
                    &row(h)
                        .push(Role::Fire, "the Adage plugin")
                        .quiet(format!(" ({}) is installed; staging it again failed: {e:#}", p.id)),
                )?,
            }
        } else {
            o.line(&row(h).push(Role::Fire, "the Adage plugin").quiet(format!(
                " ({}) is installed; Claude Code updates it from the {} marketplace",
                p.id,
                p.marketplace()
            )))?;
        }
        plugin_scope = p.scope.clone();
    } else if let Some(p) = present.first() {
        o.line(
            &row(h)
                .push(Role::Prime, "the Adage plugin is turned off")
                .quiet(format!(" ({}): turn it on in Claude Code with ", p.id))
                .cmd("/plugin"),
        )?;
        return Ok(());
    } else if let Some(c) = &cli {
        o.line(
            &row(h)
                .text("the Adage plugin")
                .quiet(format!(", staged from this adage in {}", detect::tilde(&paths.plugin()))),
        )?;
        o.flush()?;
        match claude_plugin::connect(c, paths, exe, hooks.plugin_scope(), cwd) {
            Ok(done) => {
                let root_at = detect::tilde(&done.staged.root);
                o.line(&row("").quiet("ran ").cmd(format!("claude plugin marketplace add {root_at}")))?;
                o.line(&row("").quiet("ran ").cmd(format!(
                    "claude plugin install {} --scope {}",
                    claude_plugin::PLUGIN_ID,
                    hooks.plugin_scope()
                )))?;
                let from = done
                    .installed
                    .read_from
                    .as_deref()
                    .map(detect::tilde)
                    .unwrap_or_else(|| detect::tilde(&done.staged.plugin));
                let state = if done.already { " installed already, and enabled" } else { " installed and enabled" };
                o.line(
                    &row("")
                        .glyph(Role::Fire, adage_term::Glyph::Ok)
                        .text(state)
                        .quiet(format!(" · Claude Code reads it from {from}")),
                )?;
                if !done.staged.missing.is_empty() {
                    o.line(
                        &row("")
                            .push(Role::Prime, format!("{} not staged", done.staged.missing.join(" and ")))
                            .quiet(": not beside this adage (step 2)"),
                    )?;
                }
                plugin_scope = done.installed.scope.clone();
            }
            Err(e) => {
                o.line(&row("").push(Role::Prime, "the plugin route did not finish").quiet(format!(": {e:#}")))?;
                o.line(
                    &row("")
                        .quiet("in Claude Code instead: ")
                        .cmd(claude_plugin::SLASH_ADD)
                        .quiet(", then ")
                        .cmd(claude_plugin::SLASH_INSTALL),
                )?;
                o.flush()?;
                return settings_route(hooks, root);
            }
        }
    } else {
        let settings = detect::registrations(root).into_iter().find(|r| r.harness == h && r.kind == "settings");
        match settings {
            // Connected through a settings file already; the plugin adds the band, the pane and `/adage`.
            Some(r) => {
                o.line(
                    &row(h)
                        .push(Role::Fire, "registered")
                        .quiet(format!(" in {} · nothing to write", detect::tilde(&r.location))),
                )?;
                o.line(
                    &row("")
                        .quiet("the Adage plugin adds the band, the pane and /adage; in Claude Code: ")
                        .cmd(claude_plugin::SLASH_ADD)
                        .quiet(", then ")
                        .cmd(claude_plugin::SLASH_INSTALL),
                )?;
            }
            None => {
                o.line(
                    &row(h)
                        .quiet("`claude` is not on PATH, so Adage cannot install its plugin for you; in Claude Code: ")
                        .cmd(claude_plugin::SLASH_ADD)
                        .quiet(", then ")
                        .cmd(claude_plugin::SLASH_INSTALL),
                )?;
                o.line(&row("").quiet("meanwhile, the settings file:"))?;
                o.flush()?;
                return settings_route(hooks, root);
            }
        }
        o.flush()?;
        return Ok(());
    }
    o.flush()?;
    // The plugin wires every event; a settings-file registration would run each a second time.
    remove_duplicates(&plugin_scope, root)
}

/// Register Claude Code in its settings file (`adage install claude-code --write`).
fn settings_route(hooks: Hooks, root: Option<&Path>) -> Result<()> {
    connect_file("claude-code", hooks, root).map(drop)
}

/// Take Adage's hooks out of every Claude Code settings file the plugin makes redundant here:
/// with the plugin at user scope, every one `adage install` wrote; at a repository's own scope,
/// that repository's. Each removal shows its diff and keeps `<file>.adage-bak`; a file `adage
/// install` did not write is named for the person to edit.
pub(crate) fn remove_duplicates(plugin_scope: &str, root: Option<&Path>) -> Result<()> {
    let mut o = Out::stdout();
    let regs = detect::registrations(root);
    let mut seen: Vec<PathBuf> = Vec::new();
    for r in regs.iter().filter(|r| r.harness == "claude-code" && r.kind == "settings") {
        if seen.contains(&r.location) {
            continue;
        }
        seen.push(r.location.clone());
        let at = detect::tilde(&r.location);
        match crate::install::scope_of("claude-code", &r.location, root) {
            Some(s) if plugin_scope == "user" || s == "project" => {
                let Some(p) = crate::install::plan_removal("claude-code", s, root)? else { continue };
                o.line(&row("").text(format!("{at} registers Adage's hooks too, so each would run twice; taking them out:")))?;
                crate::install::print_diff(&mut o, p.before.as_deref().unwrap_or(""), &p.after)?;
                match crate::install::remove_backed_up(&p) {
                    Ok(b) => o.line(&row("").push(Role::Fire, "removed").quiet(format!(" Adage's hooks from {at}; the previous version is in {}", detect::tilde(&b))))?,
                    Err(e) => o.line(&row("").push(Role::Inhibit, "not removed").quiet(format!(": {e:#}")))?,
                }
            }
            Some(s) => o.line(&row("").quiet(format!(
                "{at} registers Adage's hooks for every repository; here they run beside the plugin: `adage install claude-code --remove --scope {s}` takes them out"
            )))?,
            None => o.line(&row("").push(Role::Prime, format!("{at} registers Adage's hooks too")).quiet(": remove Adage's entries there by hand so each hook runs once"))?,
        }
    }
    o.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The closing screen

/// The line the next Claude Code session in `repo` opens with, as the session-start hook words
/// it for a repository set up from a terminal: what is in effect, or, before anything is, how
/// many past sessions it is learning from (`crates/adage-hook/src/session_start.rs`).
pub(crate) fn opening_line(paths: &Paths, root: &Path, repo: &str) -> Option<String> {
    let cfg = adage_store::config::load(paths).config;
    if !cfg.ui_notices || !cfg.learn_auto || !cfg.learns(repo) {
        return None;
    }
    let index = adage_tracks::Index::load(&paths.index());
    let (mut learned, mut instincts) = (0usize, 0usize);
    for r in index.for_repo(repo).filter(|r| !r.exec_only()) {
        if r.route.is_instinct() {
            instincts += 1;
        } else {
            learned += 1;
        }
    }
    let n = |k: usize, one: &str, many: &str| if k == 1 { format!("1 {one}") } else { format!("{k} {many}") };
    let what = match (learned, instincts) {
        (0, 0) => {
            let projects = cfg
                .learn_claude_projects
                .clone()
                .or_else(|| adage_store::home_dir().map(|h| h.join(".claude").join("projects")));
            let k = projects
                .map(|p| adage_learn::scope::count_claude_sessions(&p, &adage_learn::scope::checkouts(root)))
                .unwrap_or(0);
            return Some(match k {
                0 => "adage: learning from this repo as you work; its first reflexes form in a few sessions. Nothing runs without you.".into(),
                1 => "adage: learning from 1 past session in this repo; reflexes arrive within a minute. Nothing runs without you.".into(),
                k => format!("adage: learning from {k} past sessions in this repo; reflexes arrive within a minute. Nothing runs without you."),
            });
        }
        (0, i) => n(i, "instinct", "instincts"),
        (l, 0) => n(l, "reflex", "reflexes"),
        (l, i) => format!("{} and {}", n(l, "reflex", "reflexes"), n(i, "instinct", "instincts")),
    };
    Some(format!("adage: {what} active in this repo; it keeps learning as you work. Nothing runs without you."))
}

#[allow(clippy::too_many_arguments)]
fn finish(
    paths: &Paths,
    found: &[detect::Detected],
    root: Option<&Path>,
    repo_key: Option<&str>,
    bins: &Binaries,
    learning: Option<usize>,
    yes: bool,
    connect: Option<Hooks>,
    agent: Option<&str>,
    codex_written: bool,
) -> Result<()> {
    let mut out = Out::stdout();
    out.blank()?;
    // Where hooks stand: a scripted `--yes` without `--hooks` must not read as finished setup
    // when agents will not hear from Adage.
    let regs = detect::registrations(root);
    let unhooked = installed_without_hooks(found, &regs);
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
            if let Some(h) = unhooked.first() {
                out.kv("", Line::new().push(Role::Prime, "not registered").quiet(format!(": {h}")))?;
            }
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
    let have: Vec<&str> = std::iter::once("adage")
        .chain(claude_plugin::COMPANIONS.into_iter().filter(|c| !bins.missing.contains(c)))
        .collect();
    let mut b = Line::new().text(have.join(", "));
    b = if bins.origin == Origin::Plugin {
        b.quiet(" from the Claude Code plugin")
    } else {
        b.quiet(format!(" in {}", detect::tilde(&bins.dir)))
    };
    if !bins.missing.is_empty() {
        b = b.sep().push(Role::Prime, format!("{} missing", bins.missing.join(", ")));
    }
    out.kv("binaries", b)?;
    if let (Some(root), Some(key)) = (root, repo_key) {
        let cfg = adage_store::config::load(paths).config;
        let l = if !cfg.learn_auto {
            Line::new().push(Role::Prime, "off").sep().cmd("adage config set learn.auto true").quiet(" turns it on")
        } else if !cfg.learns(key) {
            Line::new().push(Role::Prime, "not here").quiet(" (learn.repos leaves this repository out)")
        } else {
            match learning {
                Some(n) => Line::new().text(key).sep().quiet(format!(
                    "conditioning {} in the background",
                    crate::helpers::plural(n, "past session", "past sessions")
                )),
                None => Line::new().text(key).sep().quiet("after each session"),
            }
        };
        out.kv("learning", l)?;
        if yes && !regs.is_empty() {
            if let Some(first) = opening_line(paths, root, key) {
                out.kv("first line", Line::new().quiet("your next Claude Code session here opens with"))?;
                out.kv("", Line::new().text(first))?;
            }
        }
    }
    out.blank()?;
    let connected = !regs.is_empty();
    if agent.is_some() {
        out.next("adage doctor", "checks the whole setup at any time")?;
        out.also("adage watch", "live pane for a split terminal")?;
    } else if !yes {
        out.next("adage init", "in your own terminal, to set this up step by step")?;
        out.also("adage doctor", "checks the whole setup at any time")?;
    } else if !connected || (connect.is_none() && !unhooked.is_empty()) {
        out.next(
            "adage init --yes --hooks user",
            "connects every harness found (Claude Code through the Adage plugin)",
        )?;
        out.also("adage doctor", "checks the whole setup at any time")?;
    } else {
        out.note("Later: `adage update` updates Adage, its hooks and the plugin together; `adage doctor` if anything looks wrong.")?;
        out.blank()?;
        let claude = regs.iter().any(|r| r.harness == "claude-code");
        if claude {
            out.next("claude", "start a session; Adage says what it is doing on its first line")?;
        } else {
            out.next("adage doctor", "checks the whole setup at any time")?;
        }
        if codex_written {
            out.also("codex", "open /hooks in Codex to approve the new hooks")?;
        }
    }
    let (needs, look) = if yes { crate::waiting(paths) } else { (0, 0) };
    if let Some(why) = crate::review::waiting_words(needs, look) {
        out.also("adage review", &why)?;
    }
    out.flush()?;
    Ok(())
}

/// `adage init --restage` (what `adage update` runs with the binary it installed): stage the
/// local Claude Code plugin again from this binary, refresh its marketplace and check it, so the
/// next session runs this version's binaries and modules. Nothing when no local plugin was staged.
fn restage(paths: &Paths) -> Result<()> {
    let mut o = Out::stdout();
    if !paths.plugin().join(".claude-plugin/marketplace.json").is_file() {
        o.header("init --restage", &["no local plugin staged here"])?;
        o.note("Claude Code's Adage plugin was not installed by `adage init` on this machine; nothing to do.")?;
        return Ok(());
    }
    let exe = this_exe()?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| paths.root.clone());
    let staged = match claude_plugin::System::on_path() {
        Some(c) => match claude_plugin::refresh(&c, paths, &exe, &cwd)? {
            Some(s) => s,
            None => {
                o.header("init --restage", &["the local plugin is not installed"])?;
                o.note(&format!(
                    "{} is staged, but Claude Code does not list {}; `adage init` installs it.",
                    detect::tilde(&paths.plugin()),
                    claude_plugin::PLUGIN_ID
                ))?;
                return Ok(());
            }
        },
        // Claude Code reads the plugin in place: staging it is enough.
        None => claude_plugin::stage(paths, &exe)?,
    };
    o.header("init --restage", &[&format!("adage {}", staged.version)])?;
    o.blank()?;
    o.kv(
        "staged",
        Line::new().text(detect::tilde(&staged.plugin)).sep().quiet(if staged.changed == 0 {
            "already up to date".to_string()
        } else {
            format!("{} files changed", staged.changed)
        }),
    )?;
    let names: Vec<&str> = staged.binaries.iter().map(|(n, _)| *n).collect();
    o.kv("binaries", Line::new().text(names.join(", ")))?;
    o.note("Takes effect in your next Claude Code session.")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_map_to_each_route_s_scope() {
        assert_eq!(Hooks::parse("user").map(|h| (h.scope(), h.plugin_scope())), Some(("user", "user")));
        assert_eq!(Hooks::parse("project").map(|h| (h.scope(), h.plugin_scope())), Some(("project", "local")));
        assert_eq!(Hooks::parse("none"), Some(Hooks::None));
        assert_eq!(Hooks::parse("all"), None);
    }

    #[test]
    fn where_the_companions_come_from() {
        let d = tempfile::tempdir().unwrap();
        let co = d.path().join("co");
        std::fs::create_dir_all(co.join("target/release")).unwrap();
        std::fs::create_dir_all(co.join(".git")).unwrap();
        std::fs::write(co.join("Cargo.toml"), "").unwrap();
        assert_eq!(
            origin(&co.join("target/release/adage")),
            Origin::Checkout { root: co.clone(), profile: "release".into() }
        );
        assert_eq!(origin(Path::new("/home/a/.claude/plugins/data/adage-adage/bin/adage-0123")), Origin::Plugin);
        assert_eq!(origin(Path::new("/home/a/.cargo/bin/adage")), Origin::Installed);
        let line = cargo_build(&co, "release", &["adage-exec", "adage-train"]);
        assert!(line.starts_with("cargo build --locked --manifest-path "), "{line}");
        assert!(line.ends_with("/co/Cargo.toml --release -p adage-exec -p adage-train"), "{line}");
        assert!(cargo_build(&co, "debug", &["adage-train"]).ends_with("Cargo.toml -p adage-train"));
    }

    #[test]
    #[cfg(unix)]
    fn the_binaries_step_is_skipped_with_both_companions_beside() {
        // T-O4-07.
        let d = tempfile::tempdir().unwrap();
        for n in ["adage", "adage-exec", "adage-train"] {
            std::fs::write(d.path().join(n), "#!/bin/sh\n").unwrap();
            crate::net::make_executable(&d.path().join(n)).unwrap();
        }
        assert!(missing_companions(&d.path().join("adage")).is_empty());
        std::fs::remove_file(d.path().join("adage-train")).unwrap();
        assert_eq!(missing_companions(&d.path().join("adage")), vec!["adage-train"]);
    }
}
