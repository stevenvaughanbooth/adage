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
//! `adage exec enable` / `disable` also register / remove the `adage-exec mcp` server (Track C
//! §3.5) in the user scope of the harnesses that have Adage hooks ([`mcp_plan`]): Codex's
//! `$CODEX_HOME/config.toml` (`[mcp_servers.adage]`, in a block between marker comments) and
//! OpenCode's `$XDG_CONFIG_HOME/opencode/opencode.json` (`mcp.adage`). Claude Code gets the server
//! from the `adage-exec` plugin's `.mcp.json`. Those writes show a diff, keep a backup of the
//! previous file (`<file>.adage-backup`), are idempotent, and never touch another server's entry.

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
    write_atomic(&p.target, &p.after)
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
    write_atomic(&p.target, &p.after)?;
    println!("adage install: wrote {}", p.target.display());
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
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".adage-install-{}.tmp", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, target)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// MCP server registrations (Track C §3.5, `adage exec enable | disable`).

/// The server's name in every harness's configuration.
pub(crate) const MCP_NAME: &str = "adage";
const CODEX_BEGIN: &str = "# >>> adage mcp (written by `adage exec enable`; removed by `adage exec disable`)";
const CODEX_END: &str = "# <<< adage mcp";

/// Harnesses whose MCP registration `adage exec enable` writes (Claude Code's is the plugin's).
pub(crate) const MCP_HARNESSES: [&str; 2] = ["codex", "opencode"];

/// Where `harness` reads user-scope MCP servers from.
pub(crate) fn mcp_target(harness: &str) -> Result<PathBuf> {
    match harness {
        "codex" => Ok(env_dir("CODEX_HOME", ".codex")?.join("config.toml")),
        "opencode" => {
            let dir = env_dir("XDG_CONFIG_HOME", ".config")?.join("opencode");
            let (json, jsonc) = (dir.join("opencode.json"), dir.join("opencode.jsonc"));
            Ok(if !json.exists() && jsonc.exists() { jsonc } else { json })
        }
        _ => bail!("no MCP registration for {harness}"),
    }
}

/// The environment the server needs beyond what harnesses pass: `ADAGE_HOME` when it is set
/// (Codex starts MCP servers with a minimal environment).
fn mcp_env() -> Option<String> {
    std::env::var("ADAGE_HOME").ok().filter(|v| !v.is_empty())
}

/// Plan registering `adage-exec mcp` (`bin`: the adage-exec binary) for `harness`, or removing
/// Adage's registration (`bin` is `None`). Reads, never writes.
pub(crate) fn mcp_plan(harness: &str, bin: Option<&Path>) -> Result<Plan> {
    let target = mcp_target(harness)?;
    if target.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("{} is a symlink; not writing through it", target.display());
    }
    let before = match std::fs::read_to_string(&target) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", target.display())),
    };
    let bin = bin.map(|b| hook_binary(Some(b.to_path_buf()))).transpose()?;
    let env = mcp_env();
    let after = match harness {
        "codex" => codex_mcp(before.as_deref().unwrap_or(""), bin.as_deref(), env.as_deref(), &target)?,
        _ => opencode_mcp(before.as_deref(), bin.as_deref(), env.as_deref(), &target)?,
    };
    Ok(Plan { harness: harness.to_string(), target, before, after })
}

fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// Codex's `config.toml` with Adage's block removed, then (with `bin`) appended again.
pub(crate) fn codex_mcp(text: &str, bin: Option<&str>, env: Option<&str>, target: &Path) -> Result<String> {
    let lines: Vec<&str> = text.lines().collect();
    let begin = lines.iter().position(|l| l.trim_end() == CODEX_BEGIN);
    let end = lines.iter().position(|l| l.trim_end() == CODEX_END);
    let kept: Vec<&str> = match (begin, end) {
        (None, None) => lines,
        (Some(b), Some(e)) if b < e => lines[..b].iter().chain(&lines[e + 1..]).copied().collect(),
        _ => bail!("{}: Adage's marker comments are unbalanced; fix the file by hand", target.display()),
    };
    let mut out = kept.join("\n");
    while out.ends_with('\n') {
        out.pop();
    }
    let doc: toml::Table =
        toml::from_str(&out).with_context(|| format!("{} is not valid TOML; not touching it", target.display()))?;
    if doc.get("mcp_servers").and_then(|m| m.get(MCP_NAME)).is_some() {
        bail!(
            "{} already defines mcp_servers.{MCP_NAME} outside Adage's block; remove or rename it first",
            target.display()
        );
    }
    if let Some(bin) = bin {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(CODEX_BEGIN);
        out.push_str(&format!("\n[mcp_servers.{MCP_NAME}]\ncommand = {}\nargs = [\"mcp\"]\n", toml_str(bin)));
        if let Some(h) = env {
            out.push_str(&format!("env = {{ ADAGE_HOME = {} }}\n", toml_str(h)));
        }
        out.push_str(CODEX_END);
    }
    if !out.is_empty() {
        out.push('\n');
    }
    toml::from_str::<toml::Table>(&out)
        .with_context(|| format!("the result for {} would not parse", target.display()))?;
    Ok(out)
}

/// Is this OpenCode `mcp` entry Adage's (a local server running `adage-exec mcp`)?
fn opencode_ours(v: &Value) -> bool {
    let cmd: Vec<&str> = v
        .get("command")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    cmd.len() == 2
        && cmd[1] == "mcp"
        && Path::new(cmd[0]).file_name().is_some_and(|n| n.to_string_lossy().starts_with("adage-exec"))
}

/// OpenCode's config with `mcp.adage` set (with `bin`) or removed.
pub(crate) fn opencode_mcp(text: Option<&str>, bin: Option<&str>, env: Option<&str>, target: &Path) -> Result<String> {
    let fresh = || json!({ "$schema": "https://opencode.ai/config.json" });
    let mut doc: Value = match text {
        None => fresh(),
        Some(s) if s.trim().is_empty() => fresh(),
        Some(s) => serde_json::from_str(s)
            .with_context(|| format!("{} is not plain JSON (comments?); not touching it", target.display()))?,
    };
    let obj =
        doc.as_object_mut().with_context(|| format!("{} is not a JSON object; not touching it", target.display()))?;
    if let Some(existing) = obj.get("mcp").and_then(|m| m.get(MCP_NAME)) {
        if !opencode_ours(existing) {
            bail!("{} already has an mcp.{MCP_NAME} entry that is not Adage's; not touching it", target.display());
        }
    }
    match bin {
        Some(bin) => {
            let mcp = obj.entry("mcp").or_insert_with(|| json!({}));
            let mcp = mcp
                .as_object_mut()
                .with_context(|| format!("\"mcp\" in {} is not an object; not touching it", target.display()))?;
            let mut entry = json!({ "type": "local", "command": [bin, "mcp"], "enabled": true });
            if let Some(h) = env {
                entry["environment"] = json!({ "ADAGE_HOME": h });
            }
            mcp.insert(MCP_NAME.to_string(), entry);
        }
        None => {
            if text.is_none() {
                return Ok(String::new());
            }
            if let Some(mcp) = obj.get_mut("mcp").and_then(Value::as_object_mut) {
                mcp.remove(MCP_NAME);
                if mcp.is_empty() {
                    obj.remove("mcp");
                }
            }
        }
    }
    Ok(serde_json::to_string_pretty(&doc)? + "\n")
}

/// Is Adage's server registered in this harness's file, and with which binary?
pub(crate) fn mcp_registered(harness: &str) -> Option<(PathBuf, String)> {
    let target = mcp_target(harness).ok()?;
    let text = std::fs::read_to_string(&target).ok()?;
    let bin = match harness {
        "codex" => {
            if !text.lines().any(|l| l.trim_end() == CODEX_BEGIN) {
                return None;
            }
            let doc: toml::Table = toml::from_str(&text).ok()?;
            doc.get("mcp_servers")?.get(MCP_NAME)?.get("command")?.as_str()?.to_string()
        }
        _ => {
            let v: Value = serde_json::from_str(&text).ok()?;
            let e = v.get("mcp")?.get(MCP_NAME)?;
            if !opencode_ours(e) {
                return None;
            }
            e["command"][0].as_str()?.to_string()
        }
    };
    Some((target, bin))
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

/// Make a planned MCP write: nothing when unchanged; otherwise keep the previous file as
/// `<file>.adage-backup` and write atomically. Returns whether anything was written. Needs a
/// person at a terminal, outside any agent session.
pub(crate) fn apply_with_backup(p: &Plan) -> Result<bool> {
    if p.before.as_deref() == Some(p.after.as_str()) || (p.before.is_none() && p.after.is_empty()) {
        return Ok(false);
    }
    require_person()?;
    if let Some(before) = &p.before {
        let mut name = p.target.file_name().context("no file name")?.to_os_string();
        name.push(".adage-backup");
        write_atomic(&p.target.with_file_name(name), before)?;
    }
    write_atomic(&p.target, &p.after)?;
    Ok(true)
}

#[cfg(test)]
mod mcp_tests {
    use super::*;

    #[test]
    fn codex_block_is_added_replaced_and_removed_without_touching_the_rest() {
        let t = Path::new("/x/config.toml");
        let user = "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        let on = codex_mcp(user, Some("/opt/adage-exec"), None, t).unwrap();
        assert!(on.starts_with(user.trim_end()), "{on}");
        assert!(on.contains("[mcp_servers.adage]\ncommand = \"/opt/adage-exec\"\nargs = [\"mcp\"]\n"), "{on}");
        assert_eq!(codex_mcp(&on, Some("/opt/adage-exec"), None, t).unwrap(), on, "idempotent");
        let moved = codex_mcp(&on, Some("/usr/bin/adage-exec"), Some("/h"), t).unwrap();
        assert!(moved.contains("/usr/bin/adage-exec") && !moved.contains("/opt/adage-exec"));
        assert!(moved.contains("env = { ADAGE_HOME = \"/h\" }"));
        assert_eq!(codex_mcp(&moved, None, None, t).unwrap(), user);
        // Someone else's adage server, a broken file, or unbalanced markers: refused.
        assert!(codex_mcp("[mcp_servers.adage]\ncommand = \"x\"\n", Some("/b"), None, t).is_err());
        assert!(codex_mcp("not = [toml", Some("/b"), None, t).is_err());
        assert!(codex_mcp(&format!("{CODEX_BEGIN}\n"), Some("/b"), None, t).is_err());
        assert_eq!(codex_mcp("", None, None, t).unwrap(), "");
    }

    #[test]
    fn opencode_entry_is_merged_and_only_ours_is_removed() {
        let t = Path::new("/x/opencode.json");
        let user = r#"{"theme":"x","mcp":{"other":{"type":"remote","url":"https://e.example"}}}"#;
        let on = opencode_mcp(Some(user), Some("/opt/adage-exec"), None, t).unwrap();
        let v: Value = serde_json::from_str(&on).unwrap();
        assert_eq!(v["mcp"]["adage"]["command"], json!(["/opt/adage-exec", "mcp"]));
        assert_eq!(v["mcp"]["other"]["url"], "https://e.example");
        assert_eq!(opencode_mcp(Some(&on), Some("/opt/adage-exec"), None, t).unwrap(), on);
        let off: Value = serde_json::from_str(&opencode_mcp(Some(&on), None, None, t).unwrap()).unwrap();
        assert!(off["mcp"].get("adage").is_none() && off["mcp"]["other"].is_object());
        let theirs = r#"{"mcp":{"adage":{"type":"local","command":["something-else"]}}}"#;
        assert!(opencode_mcp(Some(theirs), Some("/b/adage-exec"), None, t).is_err());
        assert!(opencode_mcp(Some("// comment\n{}"), Some("/b/adage-exec"), None, t).is_err());
        assert_eq!(opencode_mcp(None, None, None, t).unwrap(), "");
    }

    #[test]
    fn diffs_show_only_changed_lines() {
        assert_eq!(line_diff("a\nb\nc\n", "a\nc\nd\n"), "- b\n+ d\n");
        assert_eq!(line_diff("", "x\n"), "+ x\n");
    }
}
