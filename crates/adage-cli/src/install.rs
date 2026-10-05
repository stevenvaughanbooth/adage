//! `adage install <claude-code|opencode|codex> [--print | --write | --remove] [--scope project|user]
//!                [--bin <path>] [--repo <dir>]`
//!
//! Prints (the default), writes, or removes one harness's hook registration for this adage
//! binary. Writing needs `--write`, touches exactly one file, and only at that harness's
//! documented location:
//!
//! | Harness | `--scope project` (default) | `--scope user` |
//! | --- | --- | --- |
//! | Claude Code | `<repo>/.claude/settings.local.json` | `~/.claude/settings.json` |
//! | OpenCode | `<repo>/.opencode/plugins/adage.js` | `$XDG_CONFIG_HOME/opencode/plugins/adage.js` (default `~/.config`) |
//! | Codex | `<repo>/.codex/hooks.json` | `$CODEX_HOME/hooks.json` (default `~/.codex`) |
//!
//! JSON files are merged: Adage's own entries are replaced, everything else is kept as the person
//! wrote it (key order and formatting included, `crate::jsonedit`), and a file that is not a JSON
//! object is refused. The OpenCode plugin file is replaced only if it is Adage's.
//!
//! A harness already registered in another place that applies to the repository (the other
//! scope's file, or the Claude Code plugin) is not registered again: Claude Code and Codex run
//! every registration, so a second one would run every hook twice. `--write` then says where it
//! is registered and writes nothing; `adage doctor` and `adage delegate status` flag duplicates
//! made otherwise, with the `--remove` that leaves one.
//!
//! `--remove` takes Adage's entries out of the scope's file (an event left with none is removed,
//! and `hooks` when nothing is left in it; the person's own hooks and settings stay as they are),
//! or removes the OpenCode plugin file if it is Adage's.
//!
//! `--write` and `--remove` first show the change as a line diff. At a terminal they then ask
//! before writing, unless `--yes` was given; without one (`ADAGE_ALLOW_NONINTERACTIVE=1`) they
//! write. A file they change is copied to `<file>.adage-bak` first, so the previous version is
//! one rename away.

use crate::harness::{claude, codex, opencode};
use adage_core::repo;
use adage_trace::Harness;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: adage install <claude-code|opencode|codex> [--print | --write [--yes] | --remove [--yes]] [--scope project|user] [--bin <path>] [--repo <dir>]";

/// The suffix of the copy kept of a file before `adage install` changes it.
pub(crate) const BACKUP_SUFFIX: &str = ".adage-bak";

/// Environment variables that mark a shell run by an agent. Only `CLAUDECODE` is documented; the
/// others are observed conventions (docs/harnesses.md). A guard against steering, not a sandbox.
pub(crate) use adage_app::person::AGENT_ENV;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// A harness's registration for one binary: what to write, and where each scope puts it
/// ([`crate::placement`]).
pub(crate) struct Registration {
    content: String,
    project_rel: &'static str,
    user_path: Box<dyn Fn() -> Result<PathBuf>>,
}

fn registration(harness: &str, bin: &str) -> Result<Registration> {
    let Some(h) = Harness::parse(harness) else { bail!("{USAGE}") };
    let content = match h {
        Harness::ClaudeCode => serde_json::to_string_pretty(&claude::settings(bin))?,
        Harness::Codex => serde_json::to_string_pretty(&codex::settings(bin))?,
        Harness::OpenCode => opencode::plugin_source(bin),
    };
    let p = crate::placement::of(h);
    let (dir, file) = (p.config_dir, p.user_install);
    let user_path = Box::new(move || dir().map(|d| d.join(file)).context("HOME is not set"));
    Ok(Registration { content, project_rel: p.project_install, user_path })
}

/// The adage binary hooks should run: `--bin`, else this one; absolute, in plain form.
fn hook_binary(bin: Option<PathBuf>) -> Result<String> {
    let bin = match bin {
        Some(b) => b,
        None => std::env::current_exe().context("cannot locate the adage binary; pass --bin")?,
    };
    let bin = bin.canonicalize().unwrap_or(bin);
    if !bin.is_absolute() {
        bail!("--bin must be an absolute path");
    }
    let bin = bin.to_string_lossy().to_string();
    // Windows' canonical form is a verbatim path (`\\?\C:\...`); hooks want the plain one.
    Ok(match bin.strip_prefix(r"\\?\") {
        Some(plain) if cfg!(windows) && plain.as_bytes().get(1) == Some(&b':') => plain.to_string(),
        _ => bin,
    })
}

/// One write `adage install --write` would make: the file, what it holds now, and what it will.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub(crate) harness: String,
    pub(crate) target: PathBuf,
    pub(crate) before: Option<String>,
    pub(crate) after: String,
}

/// Plan the write for `harness` in `scope`: `project` is the repository containing `repo` (else
/// the current directory), `user` the harness's user location. Reads, never writes.
pub(crate) fn plan(harness: &str, scope: &str, repo: Option<&Path>, bin: Option<PathBuf>) -> Result<Plan> {
    if scope != "project" && scope != "user" {
        bail!("--scope must be project or user\n{USAGE}");
    }
    let reg = registration(harness, &hook_binary(bin)?)?;
    let target = if scope == "user" {
        (reg.user_path)()?
    } else {
        let start = match repo {
            Some(r) => r.to_path_buf(),
            None => std::env::current_dir()?,
        };
        let start = start.canonicalize().with_context(|| format!("{} does not exist", start.display()))?;
        let root = repo::find_root(&start).with_context(|| {
            format!("{} is not inside a git repository; pass --repo or --scope user", start.display())
        })?;
        let target = root.join(reg.project_rel);
        // A repository must not redirect the write elsewhere through a symlinked directory or file.
        for p in target.ancestors().take_while(|p| *p != root) {
            if p.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
                bail!("{} is a symlink; not writing through it", p.display());
            }
        }
        target
    };
    let after = if harness == "opencode" {
        own_file(&target, &reg.content)?
    } else {
        merge_hooks(&target, &reg.content, harness)?
    };
    let before = std::fs::read_to_string(&target).ok();
    Ok(Plan { harness: harness.to_string(), target, before, after })
}

/// Make a planned write. Needs a person at a terminal, outside any agent session.
pub(crate) fn apply(p: &Plan) -> Result<()> {
    require_person()?;
    write_atomic(&p.target, &p.after)
}

/// Print one harness's registration to stdout, as it would be written; returns it.
pub(crate) fn print_only(harness: &str, bin: Option<PathBuf>) -> Result<Registration> {
    let reg = registration(harness, &hook_binary(bin)?)?;
    print!("{}", reg.content);
    if !reg.content.ends_with('\n') {
        println!();
    }
    Ok(reg)
}

/// [`apply`], first keeping the file's previous version in `<file>.adage-bak` (`adage init` and
/// `adage ui`, which show the diff first). Returns the backup's path, when one was made.
pub(crate) fn apply_backed_up(p: &Plan) -> Result<Option<PathBuf>> {
    require_person()?;
    write_with_backup(p)
}

/// Write the plan's file, first copying what it holds now (if anything, and if it changes) to
/// `<file>.adage-bak`. Returns the backup's path, when one was made.
fn write_with_backup(p: &Plan) -> Result<Option<PathBuf>> {
    let backup = match &p.before {
        Some(before) if *before != p.after => {
            let mut name = p.target.clone().into_os_string();
            name.push(BACKUP_SUFFIX);
            let b = PathBuf::from(name);
            std::fs::write(&b, before)
                .with_context(|| format!("backing up {} to {}", p.target.display(), b.display()))?;
            Some(b)
        }
        _ => None,
    };
    write_atomic(&p.target, &p.after)?;
    Ok(backup)
}

/// The person's answer to "Write it? [y/N]": yes only for `y` or `yes`.
fn confirmed(answer: &mut impl std::io::BufRead) -> bool {
    let mut line = String::new();
    answer.read_line(&mut line).is_ok() && matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

/// The lines that differ, `+` added and `-` removed, under the file they belong to.
fn print_diff<W: std::io::Write>(o: &mut adage_term::Out<W>, before: &str, after: &str) -> Result<()> {
    use adage_term::{Line, Role};
    for l in line_diff(before, after).lines() {
        let role = if l.starts_with('+') {
            Role::Fire
        } else if l.starts_with('-') {
            Role::Inhibit
        } else {
            Role::Ink3
        };
        o.line(&Line::new().text("  ").push(role, l.to_string()))?;
    }
    o.flush()?;
    Ok(())
}

/// At a terminal without `--yes`, ask `question`; `false` (and "nothing written") on anything
/// but yes.
fn confirm<W: std::io::Write>(o: &mut adage_term::Out<W>, args: &[String], question: &str) -> Result<bool> {
    let yes = args.iter().any(|a| a == "--yes" || a == "-y");
    if !yes && std::io::stdin().is_terminal() {
        use std::io::Write;
        print!("{question}");
        let _ = std::io::stdout().flush();
        if !confirmed(&mut std::io::stdin().lock()) {
            o.blank()?;
            o.note("nothing written")?;
            return Ok(false);
        }
    }
    Ok(true)
}

/// The repository `repo` (else the current directory) is in, if any.
fn repo_root(repo: Option<&Path>) -> Option<PathBuf> {
    let start = match repo {
        Some(r) => r.to_path_buf(),
        None => std::env::current_dir().ok()?,
    };
    repo::find_root(&start.canonicalize().unwrap_or(start))
}

/// `claude` is `claude-code` (the CLI's alias); other names as they are.
fn canonical(harness: &str) -> &'static str {
    adage_trace::Harness::parse(harness).unwrap_or(adage_trace::Harness::OpenCode).id()
}

/// Is `a` the file `b` (compared as they are, then resolved)?
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().is_some_and(|x| b.canonicalize().ok().is_some_and(|y| x == y))
}

/// Which scope's file `at` is for `harness`: `user` or `project` when it is the file `adage
/// install` writes for that scope (in the repository `repo`), else `None` (a file the person or a
/// plugin wrote).
pub(crate) fn scope_of(harness: &str, at: &Path, repo: Option<&Path>) -> Option<&'static str> {
    let reg = registration(harness, "adage").ok()?;
    if (reg.user_path)().ok().is_some_and(|u| same_file(&u, at)) {
        return Some("user");
    }
    repo.filter(|r| same_file(&r.join(reg.project_rel), at)).map(|_| "project")
}

/// Every place `harness`'s hooks are registered that applies to `repo`, other than `target`.
fn registered_elsewhere(harness: &str, target: &Path, repo: Option<&Path>) -> Vec<crate::detect::Registration> {
    let h = canonical(harness);
    let mut out: Vec<crate::detect::Registration> = Vec::new();
    for r in crate::detect::registrations(repo_root(repo).as_deref()) {
        if r.harness == h && !same_file(&r.location, target) && !out.iter().any(|o| o.location == r.location) {
            out.push(r);
        }
    }
    out
}

/// A registration's place as people read it.
fn place(r: &crate::detect::Registration) -> String {
    if r.kind == "plugin-data" {
        "the Adage plugin".into()
    } else {
        crate::detect::tilde(&r.location)
    }
}

fn runs(r: &crate::detect::Registration) -> String {
    r.binary.as_ref().map(|b| format!(" runs {}", crate::detect::tilde(b))).unwrap_or_default()
}

/// One harness registered in more than one place that applies to a repository: each place runs
/// every hook, so every directive and notice arrives twice.
pub(crate) struct Duplicate {
    pub(crate) harness: &'static str,
    /// Each place, as people read it (`~/.claude/settings.json`, `the Adage plugin`).
    pub(crate) places: Vec<String>,
    /// One of them is the Claude Code plugin.
    pub(crate) plugin: bool,
    /// What leaves one: `adage install … --remove` for each copy `adage install` wrote (the
    /// first command is the one to run), and the files to edit by hand.
    pub(crate) fix: String,
}

/// The harnesses registered more than once among `regs` (`detect::registrations(repo)`). The one
/// kept is the plugin if it is one of them, else the user-scope registration, else the first.
pub(crate) fn duplicates(regs: &[crate::detect::Registration], repo: Option<&Path>) -> Vec<Duplicate> {
    let mut out = Vec::new();
    for h in crate::detect::HARNESSES {
        let mut at: Vec<&crate::detect::Registration> = Vec::new();
        for r in regs.iter().filter(|r| r.harness == h) {
            if !at.iter().any(|a| a.location == r.location) {
                at.push(r);
            }
        }
        if at.len() < 2 {
            continue;
        }
        let plugin = at.iter().any(|r| r.kind == "plugin-data");
        let keep = at
            .iter()
            .position(|r| r.kind == "plugin-data")
            .or_else(|| at.iter().position(|r| scope_of(h, &r.location, repo) == Some("user")))
            .unwrap_or(0);
        let mut steps = Vec::new();
        for r in at.iter().enumerate().filter(|(i, _)| *i != keep).map(|(_, r)| r) {
            match scope_of(h, &r.location, repo) {
                Some(s) => steps.push(format!("`adage install {h} --remove --scope {s}` removes {}", place(r))),
                None => steps.push(format!("remove Adage's hook entries from {} by hand", place(r))),
            }
        }
        out.push(Duplicate {
            harness: h,
            places: at.iter().map(|r| place(r)).collect(),
            plugin,
            fix: format!("keep one, {}: {}", place(at[keep]), steps.join("; ")),
        });
    }
    out
}

/// `adage install <harness> --remove`: take Adage's hook entries out of the scope's file (the
/// OpenCode plugin file is removed), showing the diff and keeping `<file>.adage-bak`.
fn uninstall(harness: &str, scope: &str, repo: Option<&Path>, args: &[String]) -> Result<()> {
    use adage_term::Line;
    // Only the location matters; the binary is never written.
    let p = plan(harness, scope, repo, Some(PathBuf::from("/adage-remove/adage")))?;
    let target = p.target.display().to_string();
    let mut o = adage_term::Out::stdout();
    let after: Option<String> = match (harness, &p.before) {
        ("opencode", Some(b)) if b.starts_with("// Adage for OpenCode") => Some(String::new()),
        ("opencode", _) => None,
        _ => unmerge_hooks(&p.target, harness)?,
    };
    let Some(after) = after else {
        o.header("install --remove", &[harness, scope, "not registered"])?;
        o.blank()?;
        o.kv("checked", Line::new().text(target.clone()))?;
        o.note(&format!("{target} holds no Adage hooks; nothing to remove."))?;
        let other = registered_elsewhere(harness, &p.target, repo);
        for r in &other {
            o.kv("registered", Line::new().text(place(r)).quiet(runs(r)))?;
        }
        o.blank()?;
        match other.iter().find_map(|r| scope_of(harness, &r.location, repo_root(repo).as_deref())) {
            Some(s) => o.next(&format!("adage install {harness} --remove --scope {s}"), "removes that one")?,
            None if other.iter().any(|r| r.kind == "plugin-data") => {
                o.next("/plugin uninstall adage@adage", "in Claude Code, removes the plugin's hooks")?
            }
            None => o.next("adage doctor", "lists every registration")?,
        }
        return Ok(());
    };
    o.header("install --remove", &[harness, scope])?;
    o.blank()?;
    let before = p.before.clone().unwrap_or_default();
    if after.is_empty() {
        o.line(&Line::new().text(format!("{target} would be removed (it is Adage's plugin file).")))?;
    } else {
        o.line(&Line::new().text(format!("{target} would change:")))?;
        print_diff(&mut o, &before, &after)?;
    }
    if !confirm(&mut o, args, "Remove them? [y/N] ")? {
        return Ok(());
    }
    let mut name = p.target.clone().into_os_string();
    name.push(BACKUP_SUFFIX);
    let backup = PathBuf::from(name);
    std::fs::write(&backup, &before)
        .with_context(|| format!("backing up {} to {}", p.target.display(), backup.display()))?;
    if after.is_empty() {
        std::fs::remove_file(&p.target).with_context(|| format!("cannot remove {target}"))?;
    } else {
        write_atomic(&p.target, &after)?;
    }
    o.blank()?;
    o.kv("removed", Line::new().text(format!("Adage's hooks from {target}")))?;
    o.kv("backup", Line::new().quiet("the previous version is in ").text(backup.display().to_string()))?;
    o.note("Agent sessions started from now on run without Adage's hooks. Your data in ADAGE_HOME is kept.")?;
    o.blank()?;
    o.next("adage doctor", "shows any registration left")?;
    Ok(())
}

pub fn install(args: &[String]) -> Result<()> {
    let harness = args.first().map(String::as_str).unwrap_or("");
    let write = args.iter().any(|a| a == "--write");
    let remove = args.iter().any(|a| a == "--remove");
    if write && args.iter().any(|a| a == "--print") {
        bail!("--print and --write are exclusive\n{USAGE}");
    }
    if remove && (write || args.iter().any(|a| a == "--print")) {
        bail!("--remove takes neither --print nor --write\n{USAGE}");
    }
    let scope = flag(args, "--scope").unwrap_or_else(|| "project".into());
    if scope != "project" && scope != "user" {
        bail!("--scope must be project or user\n{USAGE}");
    }
    let bin = flag(args, "--bin").map(PathBuf::from);
    if !matches!(harness, "claude-code" | "claude" | "codex" | "opencode") {
        bail!("{USAGE}");
    }

    if !write && !remove {
        let reg = print_only(harness, bin)?;
        let target = if scope == "user" {
            (reg.user_path)()?.display().to_string()
        } else {
            format!("<repo>/{}", reg.project_rel)
        };
        // The registration goes to stdout, so it can be piped; this goes to stderr.
        let mut e = adage_term::Out::stderr();
        e.line(
            &adage_term::Line::new().faint(format!("adage install: printed only; nothing was written to {target}")),
        )?;
        e.next(&format!("adage install {harness} --write --scope {scope}"), "writes it")?;
        return Ok(());
    }

    require_person()?;
    let repo = flag(args, "--repo").map(PathBuf::from);
    if remove {
        return uninstall(harness, &scope, repo.as_deref(), args);
    }
    let p = plan(harness, &scope, repo.as_deref(), bin)?;
    use adage_term::Line;
    let mut o = adage_term::Out::stdout();
    let target = p.target.display().to_string();
    if p.before.as_deref() == Some(p.after.as_str()) {
        o.header("install", &[harness, &scope, "already registered"])?;
        o.blank()?;
        o.kv("checked", Line::new().text(target.clone()))?;
        o.note(&format!("{target} already registers these hooks; nothing to write."))?;
        o.blank()?;
        o.next("adage doctor", "checks that the hooks are registered and healthy")?;
        return Ok(());
    }
    // Registered somewhere else that applies here already (the other scope, the Claude Code
    // plugin): a second registration would run every hook twice, so nothing is written.
    let elsewhere = registered_elsewhere(harness, &p.target, repo.as_deref());
    if !elsewhere.is_empty() {
        o.header("install", &[harness, &scope, "already registered"])?;
        o.blank()?;
        for r in &elsewhere {
            o.kv("registered", Line::new().text(place(r)).quiet(runs(r)))?;
        }
        o.note(&format!(
            "Nothing written: a second registration in {} would run every hook twice.",
            crate::detect::tilde(&p.target)
        ))?;
        o.blank()?;
        o.next("adage doctor", "checks that the hooks are registered and healthy")?;
        if let Some(s) =
            elsewhere.iter().find_map(|r| scope_of(harness, &r.location, repo_root(repo.as_deref()).as_deref()))
        {
            o.also(
                &format!("adage install {harness} --remove --scope {s}"),
                &format!(
                    "first, to register it for {} instead",
                    if scope == "user" { "every repository" } else { "this repository only" }
                ),
            )?;
        }
        return Ok(());
    }
    // What changes, before anything is written.
    o.header("install", &[harness, &scope])?;
    o.blank()?;
    let verb = if p.before.is_some() { "would change" } else { "would be created" };
    o.line(&Line::new().text(format!("{target} {verb}:")))?;
    print_diff(&mut o, p.before.as_deref().unwrap_or(""), &p.after)?;
    if !confirm(&mut o, args, "Write it? [y/N] ")? {
        return Ok(());
    }
    let backup = write_with_backup(&p)?;
    o.blank()?;
    o.kv("wrote", Line::new().text(target))?;
    if let Some(b) = backup {
        o.kv("backup", Line::new().quiet("the previous version is in ").text(b.display().to_string()))?;
    }
    if harness == "codex" {
        o.blank()?;
        o.note("Codex asks you to review new hooks before they run: open /hooks in Codex to approve them.")?;
    }
    o.blank()?;
    o.next("adage doctor", "checks that the hooks are registered and healthy")?;
    Ok(())
}

fn require_person() -> Result<()> {
    if let Some(v) = AGENT_ENV.iter().find(|v| std::env::var_os(v).is_some()) {
        return Err(crate::hint(
            format!("`adage install --write` and `--remove` change what runs in your agent's sessions, so they will not run inside one ({v} is set)"),
            "run it yourself, in a terminal",
        ));
    }
    if std::env::var("ADAGE_ALLOW_NONINTERACTIVE").as_deref() != Ok("1") && !std::io::stdin().is_terminal() {
        return Err(crate::hint(
            "`adage install --write` and `--remove` need a person at a terminal (stdin is not a terminal)",
            "use the printed snippet instead, or set ADAGE_ALLOW_NONINTERACTIVE=1",
        ));
    }
    Ok(())
}

/// Is this hook entry one of Adage's own, for this harness?
pub(crate) fn is_ours(entry: &Value, harness: &str) -> bool {
    entry.get("hooks").and_then(|h| h.as_array()).is_some_and(|hs| {
        hs.iter().any(|h| {
            let args: Vec<&str> = h
                .get("args")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            let cmd = h.get("command").and_then(|c| c.as_str()).unwrap_or("");
            match harness {
                "codex" => cmd.contains(" hook codex "),
                _ => args.len() >= 2 && args[0] == "hook" && args[1] == "claude",
            }
        })
    })
}

/// A settings file's text, if it has any, checked to be a JSON object whose `hooks` (if any) is
/// an object; `Ok(None)` when the file is missing or blank.
fn settings_text(target: &Path) -> Result<Option<String>> {
    let text = match std::fs::read_to_string(target) {
        Ok(s) if s.trim().is_empty() => return Ok(None),
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", target.display())),
    };
    let doc: Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON; not touching it", target.display()))?;
    let obj = doc.as_object().with_context(|| format!("{} is not a JSON object; not touching it", target.display()))?;
    if let Some(hooks) = obj.get("hooks") {
        hooks
            .as_object()
            .with_context(|| format!("\"hooks\" in {} is not an object; not touching it", target.display()))?;
    }
    Ok(Some(text))
}

/// Merge Adage's hook registrations into an existing settings file, replacing only its own
/// entries. Everything else keeps its text: the person's key order, formatting and own hooks
/// (`crate::jsonedit`), so the diff shows only what Adage changes.
pub fn merge_hooks(target: &Path, ours: &str, harness: &str) -> Result<String> {
    let ours: Value = serde_json::from_str(ours)?;
    let ours_hooks = ours["hooks"].as_object().cloned().unwrap_or_default();
    let Some(text) = settings_text(target)? else {
        return Ok(serde_json::to_string_pretty(&json!({ "hooks": ours_hooks }))? + "\n");
    };
    let doc: Value = serde_json::from_str(&text)?;
    for event in ours_hooks.keys() {
        if doc["hooks"].get(event).is_some_and(|l| !l.is_array()) {
            bail!("hooks.{event} in {} is not an array; not touching it", target.display());
        }
    }
    crate::jsonedit::merge_hooks(&text, &ours_hooks, &|e| is_ours(e, harness))
}

/// The settings file without Adage's hook entries (an event left with none is removed, and
/// `hooks` when nothing is left in it); `None` when it holds none.
pub fn unmerge_hooks(target: &Path, harness: &str) -> Result<Option<String>> {
    let Some(text) = settings_text(target)? else { return Ok(None) };
    crate::jsonedit::remove_hooks(&text, &|e| is_ours(e, harness))
}

/// The OpenCode plugin file: replaced only if absent or already Adage's.
fn own_file(target: &Path, content: &str) -> Result<String> {
    match std::fs::read_to_string(target) {
        Ok(s) if !s.starts_with("// Adage for OpenCode") => {
            bail!("{} exists and is not Adage's plugin; not touching it", target.display())
        }
        _ => Ok(content.to_string()),
    }
}

fn write_atomic(target: &Path, body: &str) -> Result<()> {
    let dir = target.parent().context("no parent directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let tmp = dir.join(format!(".adage-install-{}.tmp", std::process::id()));
    std::fs::write(&tmp, body).with_context(|| format!("cannot write in {}", dir.display()))?;
    std::fs::rename(&tmp, target).with_context(|| format!("cannot replace {}", target.display()))?;
    Ok(())
}

/// The lines that differ between two texts, `-` removed and `+` added, in order (a longest
/// common subsequence diff; the files involved are small).
pub(crate) fn line_diff(before: &str, after: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    if a.len().saturating_mul(b.len()) > 4_000_000 {
        return format!("({} lines before, {} after)\n", a.len(), b.len());
    }
    let mut l = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            l[i][j] = if a[i] == b[j] { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, String::new());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || l[i][j + 1] >= l[i + 1][j]) {
            out.push_str(&format!("+ {}\n", b[j]));
            j += 1;
        } else {
            out.push_str(&format!("- {}\n", a[i]));
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod diff_tests {
    use super::*;

    #[test]
    fn only_yes_confirms() {
        for (a, want) in [("y\n", true), ("YES\n", true), (" yes ", true), ("\n", false), ("n\n", false), ("", false)] {
            assert_eq!(confirmed(&mut a.as_bytes()), want, "{a:?}");
        }
    }

    #[test]
    fn diffs_show_only_changed_lines() {
        assert_eq!(line_diff("a\nb\nc\n", "a\nc\nd\n"), "- b\n+ d\n");
        assert_eq!(line_diff("", "x\n"), "+ x\n");
    }
}
