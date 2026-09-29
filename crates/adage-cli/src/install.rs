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
//!
//! `--write` first shows the change as a line diff. At a terminal it then asks before writing,
//! unless `--yes` was given; without one (`ADAGE_ALLOW_NONINTERACTIVE=1`) it writes. A file it
//! changes is copied to `<file>.adage-bak` first, so the previous version is one rename away.

use crate::harness::{claude, codex, opencode};
use adage_core::repo;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: adage install <claude-code|opencode|codex> [--print | --write [--yes]] [--scope project|user] [--bin <path>] [--repo <dir>]";

/// The suffix of the copy kept of a file before `adage install` changes it.
pub(crate) const BACKUP_SUFFIX: &str = ".adage-bak";

/// Environment variables that mark a shell run by an agent. Only `CLAUDECODE` is documented; the
/// others are observed conventions (docs/harnesses.md). A guard against steering, not a sandbox.
pub(crate) const AGENT_ENV: [&str; 4] = ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED"];

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn home(rel: &str) -> Result<PathBuf> {
    let home = adage_store::home_dir().context("HOME is not set")?;
    Ok(home.join(rel))
}

/// `$var` if set and non-empty, else `~/<home_rel>`.
fn env_dir(var: &str, home_rel: &str) -> Result<PathBuf> {
    match std::env::var_os(var).filter(|v| !v.is_empty()) {
        Some(v) => Ok(PathBuf::from(v)),
        None => home(home_rel),
    }
}

/// A harness's registration for one binary: what to write, and where each scope puts it.
struct Registration {
    content: String,
    project_rel: &'static str,
    user_path: fn() -> Result<PathBuf>,
}

fn registration(harness: &str, bin: &str) -> Result<Registration> {
    let (content, project_rel, user_path): (String, &'static str, fn() -> Result<PathBuf>) = match harness {
        "claude-code" | "claude" => {
            (serde_json::to_string_pretty(&claude::settings(bin))?, ".claude/settings.local.json", || {
                home(".claude/settings.json")
            })
        }
        "codex" => (serde_json::to_string_pretty(&codex::settings(bin))?, ".codex/hooks.json", || {
            env_dir("CODEX_HOME", ".codex").map(|d| d.join("hooks.json"))
        }),
        "opencode" => (opencode::plugin_source(bin), ".opencode/plugins/adage.js", || {
            env_dir("XDG_CONFIG_HOME", ".config").map(|d| d.join("opencode/plugins/adage.js"))
        }),
        _ => bail!("{USAGE}"),
    };
    Ok(Registration { content, project_rel, user_path })
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
    write_with_backup(p).map(drop)
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
    let bin = flag(args, "--bin").map(PathBuf::from);
    if !matches!(harness, "claude-code" | "claude" | "codex" | "opencode") {
        bail!("{USAGE}");
    }

    if !write {
        let reg = registration(harness, &hook_binary(bin)?)?;
        let content = reg.content;
        print!("{content}");
        if !content.ends_with('\n') {
            println!();
        }
        let target = if scope == "user" {
            (reg.user_path)()?.display().to_string()
        } else {
            format!("<repo>/{}", reg.project_rel)
        };
        eprintln!(
            "adage install: printed only. `adage install {harness} --write --scope {scope}` would write {target}"
        );
        return Ok(());
    }

    require_person()?;
    let repo = flag(args, "--repo").map(PathBuf::from);
    let p = plan(harness, &scope, repo.as_deref(), bin)?;
    if p.before.as_deref() == Some(p.after.as_str()) {
        println!("adage install: {} already registers these hooks; nothing to write", p.target.display());
        return Ok(());
    }
    // What changes, before anything is written.
    let verb = if p.before.is_some() { "would change" } else { "would be created" };
    println!("adage install: {} {verb}:", p.target.display());
    for l in line_diff(p.before.as_deref().unwrap_or(""), &p.after).lines() {
        println!("  {l}");
    }
    let yes = args.iter().any(|a| a == "--yes" || a == "-y");
    if !yes && std::io::stdin().is_terminal() {
        use std::io::Write;
        print!("Write it? [y/N] ");
        let _ = std::io::stdout().flush();
        if !confirmed(&mut std::io::stdin().lock()) {
            println!("adage install: nothing written");
            return Ok(());
        }
    }
    let backup = write_with_backup(&p)?;
    println!("adage install: wrote {}", p.target.display());
    if let Some(b) = backup {
        println!("  the previous version is in {}", b.display());
    }
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

/// Merge Adage's hook registrations into an existing settings file, replacing only its own entries.
pub fn merge_hooks(target: &Path, ours: &str, harness: &str) -> Result<String> {
    let ours: Value = serde_json::from_str(ours)?;
    let mut doc: Value = match std::fs::read_to_string(target) {
        Ok(s) if s.trim().is_empty() => json!({}),
        Ok(s) => serde_json::from_str(&s)
            .with_context(|| format!("{} is not valid JSON; not touching it", target.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e).with_context(|| format!("reading {}", target.display())),
    };
    let obj =
        doc.as_object_mut().with_context(|| format!("{} is not a JSON object; not touching it", target.display()))?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .with_context(|| format!("\"hooks\" in {} is not an object; not touching it", target.display()))?;
    for (event, entries) in ours["hooks"].as_object().into_iter().flatten() {
        let list = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        let list = list
            .as_array_mut()
            .with_context(|| format!("hooks.{event} in {} is not an array; not touching it", target.display()))?;
        list.retain(|e| !is_ours(e, harness));
        list.extend(entries.as_array().into_iter().flatten().cloned());
    }
    Ok(serde_json::to_string_pretty(&doc)? + "\n")
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
