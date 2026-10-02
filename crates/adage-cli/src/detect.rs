//! Read-only probing for `adage init` and `adage doctor`: which harnesses are installed, and where
//! Adage's hooks are registered for each. Nothing here writes, and nothing is executed — binaries
//! are looked up on `PATH`, never run.
//!
//! Locations follow each harness's documented configuration, honouring the same overrides as
//! `adage install`: `CLAUDE_CONFIG_DIR` (default `~/.claude`), `XDG_CONFIG_HOME` (default
//! `~/.config`) for OpenCode, and `CODEX_HOME` (default `~/.codex`).

use adage_trace::Harness;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Every harness, in the order registrations are listed.
pub const HARNESSES: [&str; 3] = [Harness::ClaudeCode.id(), Harness::OpenCode.id(), Harness::Codex.id()];

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn home() -> Option<PathBuf> {
    adage_store::paths::home_dir()
}

/// Claude Code's configuration directory.
pub fn claude_dir() -> Option<PathBuf> {
    env_path("CLAUDE_CONFIG_DIR").or_else(|| home().map(|h| h.join(".claude")))
}

/// OpenCode's configuration directory.
pub fn opencode_dir() -> Option<PathBuf> {
    env_path("XDG_CONFIG_HOME").or_else(|| home().map(|h| h.join(".config"))).map(|d| d.join("opencode"))
}

/// Codex's home directory.
pub fn codex_dir() -> Option<PathBuf> {
    env_path("CODEX_HOME").or_else(|| home().map(|h| h.join(".codex")))
}

/// An executable named `name` on `PATH`, if any.
pub fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| executable(p))
}

/// How the person runs this adage from a terminal, for the notices a session shows: `adage` when
/// it is on `PATH`; else, for a Claude Code plugin user (no `adage` on `PATH`), the plugin's
/// stable binary `<plugin data>/bin/adage`, which the plugin keeps pointing at its verified copy
/// (this process runs a digest-named copy beside it); else this binary's own path.
pub fn adage_command() -> String {
    if on_path("adage").is_some() {
        return "adage".into();
    }
    let Ok(exe) = std::env::current_exe() else { return "adage".into() };
    let dir = exe.parent().filter(|d| d.file_name().is_some_and(|n| n == "bin"));
    let stable = dir.map(|d| d.join("adage")).filter(|l| l.symlink_metadata().is_ok());
    command_for(&stable.unwrap_or(exe))
}

/// A binary's path as a person types it: `~` for the home directory, quoted if it has a space.
pub fn command_for(p: &Path) -> String {
    let t = tilde(p);
    if t.contains(' ') {
        format!("'{t}'")
    } else {
        t
    }
}

pub fn executable(p: &Path) -> bool {
    let Ok(m) = std::fs::metadata(p) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        m.is_file() && m.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        m.is_file()
    }
}

/// A path as people type it: without Windows' `\\?\` verbatim prefix, which `canonicalize` adds.
fn plain(p: &Path) -> String {
    let raw = p.display().to_string();
    match raw.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => raw.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(raw),
    }
}

/// Show a path with the home directory as `~`, both compared in their plain form.
pub fn tilde(p: &Path) -> String {
    let s = plain(p);
    match home().map(|h| plain(&h)) {
        Some(h) if h.len() > 1 && s.starts_with(&h) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

#[cfg(test)]
mod plain_tests {
    use super::plain;
    use std::path::Path;

    #[test]
    fn plain_drops_the_verbatim_prefix() {
        assert_eq!(plain(Path::new(r"\\?\C:\Users\a")), r"C:\Users\a");
        assert_eq!(plain(Path::new(r"\\?\UNC\srv\share\x")), r"\\srv\share\x");
        assert_eq!(plain(Path::new("/home/a/.claude")), "/home/a/.claude");
    }
}

#[derive(Debug, Clone)]
pub struct Detected {
    pub harness: &'static str,
    /// What showed it is installed: a binary on `PATH`, or its configuration directory.
    pub evidence: Vec<String>,
}

impl Detected {
    pub fn found(&self) -> bool {
        !self.evidence.is_empty()
    }
}

/// Which harnesses look installed on this machine.
pub fn harnesses() -> Vec<Detected> {
    HARNESSES
        .iter()
        .map(|&h| {
            let p = crate::placement::of(Harness::parse(h).expect("HARNESSES are harness ids"));
            let (bin, dir) = (p.command, (p.config_dir)());
            let mut evidence = Vec::new();
            if let Some(p) = on_path(bin) {
                evidence.push(format!("`{bin}` on PATH ({})", tilde(&p)));
            }
            // An empty Codex home is not Codex: `CODEX_HOME` pointed at a directory Codex never used
            // (Codex writes its config, auth and sessions there at its first run).
            let used = |d: &Path| h != "codex" || std::fs::read_dir(d).is_ok_and(|mut rd| rd.next().is_some());
            if let Some(d) = dir.filter(|d| d.is_dir() && used(d)) {
                evidence.push(format!("{} exists", tilde(&d)));
            }
            Detected { harness: h, evidence }
        })
        .collect()
}

/// One place Adage's hooks are registered, and the adage binary it runs.
#[derive(Debug, Clone)]
pub struct Registration {
    pub harness: &'static str,
    /// The settings file, plugin file, or plugin data directory.
    pub location: PathBuf,
    /// The adage binary it runs, if it names one.
    pub binary: Option<PathBuf>,
    /// `settings` (hooks in a settings file), `plugin` (the OpenCode plugin file), or
    /// `plugin-data` (the Claude Code plugin's staged binary).
    pub kind: &'static str,
}

fn read_json(p: &Path) -> Option<Value> {
    let bytes = std::fs::read(p).ok()?;
    if bytes.len() > 4 << 20 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// Binaries named by Adage's own hook entries in a Claude Code or Codex settings document.
fn hook_binaries(doc: &Value, harness: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entries in doc.get("hooks").and_then(|h| h.as_object()).into_iter().flat_map(|o| o.values()) {
        for h in entries
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|e| e.get("hooks").and_then(|h| h.as_array()).into_iter().flatten())
        {
            let cmd = h.get("command").and_then(|c| c.as_str()).unwrap_or("");
            let args: Vec<&str> = h
                .get("args")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            let bin = match harness {
                "claude-code" if args.len() >= 2 && args[0] == "hook" && args[1] == "claude" => Some(cmd.to_string()),
                "claude-code" => cmd.split_once(" hook claude ").map(|(b, _)| unquote(b)),
                _ => cmd.split_once(" hook codex ").map(|(b, _)| unquote(b)),
            };
            if let Some(b) = bin.filter(|b| !b.is_empty()) {
                let b = PathBuf::from(b);
                if !out.contains(&b) {
                    out.push(b);
                }
            }
        }
    }
    out
}

/// Undo `harness::codex::sh_quote`.
fn unquote(s: &str) -> String {
    let s = s.trim();
    match s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        Some(inner) => inner.replace(r"'\''", "'"),
        None => s.to_string(),
    }
}

/// The binary an installed OpenCode plugin file names (`const ADAGE_BIN = "..."`).
fn opencode_binary(p: &Path) -> Option<Option<PathBuf>> {
    let src = std::fs::read_to_string(p).ok()?;
    if !src.starts_with("// Adage for OpenCode") {
        return None;
    }
    let line = src.lines().find(|l| l.trim_start().starts_with("const ADAGE_BIN ="));
    let bin = line
        .and_then(|l| l.split_once('='))
        .and_then(|(_, v)| serde_json::from_str::<String>(v.trim().trim_end_matches(';')).ok());
    Some(bin.filter(|b| b != "__ADAGE_BIN__").map(PathBuf::from))
}

/// Every registration of Adage's hooks, in user scope and in `repo` (a repository root) if given.
pub fn registrations(repo: Option<&Path>) -> Vec<Registration> {
    let mut out = Vec::new();
    // Settings files: every harness's user scope, then the repository's (in harness order).
    let settings_harnesses: Vec<Harness> = Harness::ALL
        .into_iter()
        .filter(|h| crate::placement::of(*h).kind == crate::placement::Kind::Settings)
        .collect();
    let mut settings: Vec<(&'static str, PathBuf)> = Vec::new();
    for &h in &settings_harnesses {
        let p = crate::placement::of(h);
        if let Some(d) = (p.config_dir)() {
            settings.extend(p.user_files.iter().map(|f| (h.id(), d.join(f))));
        }
    }
    if let Some(r) = repo {
        for &h in &settings_harnesses {
            settings.extend(crate::placement::of(h).project_files.iter().map(|f| (h.id(), r.join(f))));
        }
    }
    for (h, p) in settings {
        if let Some(doc) = read_json(&p) {
            for b in hook_binaries(&doc, h) {
                out.push(Registration { harness: h, location: p.clone(), binary: Some(b), kind: "settings" });
            }
        }
    }
    for h in Harness::ALL.into_iter().filter(|h| crate::placement::of(*h).kind == crate::placement::Kind::Plugin) {
        let p = crate::placement::of(h);
        let mut plugins: Vec<PathBuf> =
            (p.config_dir)().map(|d| p.user_files.iter().map(|f| d.join(f)).collect()).unwrap_or_default();
        if let Some(r) = repo {
            plugins.extend(p.project_files.iter().map(|f| r.join(f)));
        }
        for path in plugins {
            if let Some(bin) = opencode_binary(&path) {
                out.push(Registration { harness: h.id(), location: path, binary: bin, kind: "plugin" });
            }
        }
    }
    // The Claude Code plugin stages its binary in its data directory (`${CLAUDE_PLUGIN_DATA}`,
    // under `plugins/data/<plugin id>/`); its hooks are registered by the plugin itself.
    if let Some(Ok(rd)) = claude_dir().map(|d| std::fs::read_dir(d.join("plugins/data"))) {
        let mut dirs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains("adage")))
            .collect();
        dirs.sort();
        for d in dirs {
            let bin = d.join("bin/adage");
            if bin.symlink_metadata().is_ok() {
                out.push(Registration { harness: "claude-code", location: d, binary: Some(bin), kind: "plugin-data" });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_adage_hook_entries_only() {
        let claude = crate::harness::claude::settings("/opt/adage/bin/adage");
        assert_eq!(hook_binaries(&claude, "claude-code"), vec![PathBuf::from("/opt/adage/bin/adage")]);
        let codex = crate::harness::codex::settings("/Users/a b/adage");
        assert_eq!(hook_binaries(&codex, "codex"), vec![PathBuf::from("/Users/a b/adage")]);
        let other =
            json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "notify-send done" }] }] } });
        assert!(hook_binaries(&other, "claude-code").is_empty());
        assert!(hook_binaries(&json!([1, 2]), "codex").is_empty());
    }

    #[test]
    fn reads_the_opencode_plugins_binary() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("adage.js");
        std::fs::write(&p, crate::harness::opencode::plugin_source("/usr/local/bin/adage")).unwrap();
        assert_eq!(opencode_binary(&p), Some(Some(PathBuf::from("/usr/local/bin/adage"))));
        std::fs::write(&p, "export default {}").unwrap();
        assert_eq!(opencode_binary(&p), None, "not Adage's plugin");
    }
}
