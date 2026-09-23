//! `adage install <claude-code|opencode|codex> [--print | --write] [--scope project|user]
//!                [--bin <path>] [--repo <dir>]`
//!
//! Prints (the default) or writes one harness's hook registration for this adage binary. Writing
//! needs `--write`, touches exactly one file, and only at that harness's documented location:
//!
//! | Harness | `--scope project` (default) | `--scope user` |
//! | --- | --- | --- |
//! | Claude Code | `<repo>/.claude/settings.local.json` | `~/.claude/settings.json` |
//! | OpenCode | `<repo>/.opencode/plugins/adage.js` | `$XDG_CONFIG_HOME/opencode/plugins/adage.js` (default `~/.config`) |
//! | Codex | `<repo>/.codex/hooks.json` | `$CODEX_HOME/hooks.json` (default `~/.codex`) |
//!
//! JSON files are merged: Adage's own entries are replaced, everything else is kept, and a file
//! that is not a JSON object is refused. The OpenCode plugin file is replaced only if it is Adage's.

use crate::harness::{claude, codex, opencode};
use adage_core::repo;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: adage install <claude-code|opencode|codex> [--print | --write] [--scope project|user] [--bin <path>] [--repo <dir>]";

/// Environment variables that mark a shell run by an agent. Only `CLAUDECODE` is documented; the
/// others are observed conventions (docs/harnesses.md). A guard against steering, not a sandbox.
pub(crate) const AGENT_ENV: [&str; 4] = ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED"];

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn home(rel: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).context("HOME is not set")?;
    Ok(PathBuf::from(home).join(rel))
}

/// `$var` if set and non-empty, else `~/<home_rel>`.
fn env_dir(var: &str, home_rel: &str) -> Result<PathBuf> {
    match std::env::var_os(var).filter(|v| !v.is_empty()) {
        Some(v) => Ok(PathBuf::from(v)),
        None => home(home_rel),
    }
}

pub fn install(args: &[String]) -> Result<()> {
    let harness = args.first().map(String::as_str).unwrap_or("");
    let write = args.iter().any(|a| a == "--write");
    if write && args.iter().any(|a| a == "--print") {
        bail!("--print and --write are exclusive\n{USAGE}");
    }
    let scope = flag(args, "--scope").unwrap_or_else(|| "project".into());
    if scope != "project" && scope != "user" {
        bail!("--scope must be project or user\n{USAGE}");
    }
    let bin = match flag(args, "--bin") {
        Some(b) => PathBuf::from(b),
        None => std::env::current_exe().context("cannot locate the adage binary; pass --bin")?,
    };
    let bin = bin.canonicalize().unwrap_or(bin);
    if !bin.is_absolute() {
        bail!("--bin must be an absolute path");
    }
    let bin = bin.to_string_lossy().to_string();

    let (content, project_rel, user_path): (String, &str, fn() -> Result<PathBuf>) = match harness {
        "claude-code" | "claude" => (
            serde_json::to_string_pretty(&claude::settings(&bin))?,
            ".claude/settings.local.json",
            || home(".claude/settings.json"),
        ),
        "codex" => (
            serde_json::to_string_pretty(&codex::settings(&bin))?,
            ".codex/hooks.json",
            || env_dir("CODEX_HOME", ".codex").map(|d| d.join("hooks.json")),
        ),
        "opencode" => (
            opencode::plugin_source(&bin),
            ".opencode/plugins/adage.js",
            || env_dir("XDG_CONFIG_HOME", ".config").map(|d| d.join("opencode/plugins/adage.js")),
        ),
        _ => bail!("{USAGE}"),
    };

    if !write {
        print!("{content}");
        if !content.ends_with('\n') {
            println!();
        }
        let target = if scope == "user" { user_path()?.display().to_string() } else { format!("<repo>/{project_rel}") };
        eprintln!("adage install: printed only. `adage install {harness} --write --scope {scope}` would write {target}");
        return Ok(());
    }

    require_person()?;
    let target = if scope == "user" {
        user_path()?
    } else {
        let start = match flag(args, "--repo") {
            Some(r) => PathBuf::from(r),
            None => std::env::current_dir()?,
        };
        let start = start.canonicalize().with_context(|| format!("{} does not exist", start.display()))?;
        let root = repo::find_root(&start).with_context(|| format!("{} is not inside a git repository; pass --repo or --scope user", start.display()))?;
        let target = root.join(project_rel);
        // A repository must not redirect the write elsewhere through a symlinked directory or file.
        for p in target.ancestors().take_while(|p| *p != root) {
            if p.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
                bail!("{} is a symlink; not writing through it", p.display());
            }
        }
        target
    };
    let body = if harness == "opencode" { own_file(&target, &content)? } else { merge_hooks(&target, &content, harness)? };
    write_atomic(&target, &body)?;
    println!("adage install: wrote {}", target.display());
    if harness == "codex" {
        println!("Codex asks you to review new hooks before they run: open /hooks in Codex to approve them.");
    }
    Ok(())
}

fn require_person() -> Result<()> {
    if let Some(v) = AGENT_ENV.iter().find(|v| std::env::var_os(v).is_some()) {
        bail!("`adage install --write` changes what runs in your agent's sessions, so it will not run inside one ({v} is set). Run it yourself, in a terminal.");
    }
    if std::env::var("ADAGE_ALLOW_NONINTERACTIVE").as_deref() != Ok("1") && !std::io::stdin().is_terminal() {
        bail!("`adage install --write` needs a person at a terminal (stdin is not a terminal); use the printed snippet instead, or set ADAGE_ALLOW_NONINTERACTIVE=1");
    }
    Ok(())
}

/// Is this hook entry one of Adage's own, for this harness?
fn is_ours(entry: &Value, harness: &str) -> bool {
    entry.get("hooks").and_then(|h| h.as_array()).is_some_and(|hs| {
        hs.iter().any(|h| {
            let args: Vec<&str> = h.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            let cmd = h.get("command").and_then(|c| c.as_str()).unwrap_or("");
            match harness {
                "codex" => cmd.contains(" hook codex "),
                _ => args.len() >= 2 && args[0] == "hook" && args[1] == "claude",
            }
        })
    })
}

/// Merge Adage's hook registrations into an existing settings file, replacing only its own entries.
pub fn merge_hooks(target: &Path, ours: &str, harness: &str) -> Result<String> {
    let ours: Value = serde_json::from_str(ours)?;
    let mut doc: Value = match std::fs::read_to_string(target) {
        Ok(s) if s.trim().is_empty() => json!({}),
        Ok(s) => serde_json::from_str(&s).with_context(|| format!("{} is not valid JSON; not touching it", target.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e).with_context(|| format!("reading {}", target.display())),
    };
    let obj = doc.as_object_mut().with_context(|| format!("{} is not a JSON object; not touching it", target.display()))?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().with_context(|| format!("\"hooks\" in {} is not an object; not touching it", target.display()))?;
    for (event, entries) in ours["hooks"].as_object().into_iter().flatten() {
        let list = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        let list = list.as_array_mut().with_context(|| format!("hooks.{event} in {} is not an array; not touching it", target.display()))?;
        list.retain(|e| !is_ours(e, harness));
        list.extend(entries.as_array().into_iter().flatten().cloned());
    }
    Ok(serde_json::to_string_pretty(&doc)? + "\n")
}

/// The OpenCode plugin file: replaced only if absent or already Adage's.
fn own_file(target: &Path, content: &str) -> Result<String> {
    match std::fs::read_to_string(target) {
        Ok(s) if !s.starts_with("// Adage for OpenCode") => bail!("{} exists and is not Adage's plugin; not touching it", target.display()),
        _ => Ok(content.to_string()),
    }
}

fn write_atomic(target: &Path, body: &str) -> Result<()> {
    let dir = target.parent().context("no parent directory")?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".adage-install-{}.tmp", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, target)?;
    Ok(())
}
