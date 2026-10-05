//! Read-only probing for `adage init` and `adage doctor`: which harnesses are installed, and where
//! Adage's hooks are registered for each. Nothing here writes, and nothing is executed — binaries
//! are looked up on `PATH`, never run.
//!
//! Locations follow each harness's documented configuration, honouring the same overrides as
//! `adage install`: `CLAUDE_CONFIG_DIR` (default `~/.claude`), `XDG_CONFIG_HOME` (default
//! `~/.config`) for OpenCode, and `CODEX_HOME` (default `~/.codex`).

use serde_json::Value;
use std::path::{Path, PathBuf};

pub const HARNESSES: [&str; 3] = ["claude-code", "opencode", "codex"];

pub fn harness_name(harness: &str) -> &str {
    match harness {
        "claude" | "claude-code" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        other => other,
    }
}

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

/// One resolved history source. Configuration wins over harness home detection, including an
/// explicitly configured folder which is missing. Never silently fall back from a bad setting.
#[derive(Debug, Clone)]
pub struct HistorySource {
    pub harness: &'static str,
    pub key: &'static str,
    pub path: PathBuf,
    pub configured: bool,
    pub files: Vec<PathBuf>,
    pub compressed: usize,
}

pub fn history_sources(cfg: &adage_store::config::Config) -> Vec<HistorySource> {
    let dirs = [
        (
            "claude-code",
            "learn.claude_projects",
            cfg.learn_claude_projects.clone(),
            claude_dir().map(|p| p.join("projects")),
        ),
        ("codex", "learn.codex_sessions", cfg.learn_codex_sessions.clone(), codex_dir().map(|p| p.join("sessions"))),
        ("opencode", "learn.opencode_exports", cfg.learn_opencode_exports.clone(), None),
    ];
    dirs.into_iter()
        .filter_map(|(harness, key, configured, detected)| {
            let explicitly_set = configured.is_some();
            let path = configured.or(detected)?;
            let (mut files, compressed) = match harness {
                "claude-code" => (adage_learn::import::discover(&path), 0),
                "codex" => adage_learn::codex::discover(&path),
                _ => (
                    std::fs::read_dir(&path)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json"))
                        .collect(),
                    0,
                ),
            };
            files.sort();
            Some(HistorySource { harness, key, path, configured: explicitly_set, files, compressed })
        })
        .collect()
}

pub fn history_summary(cfg: &adage_store::config::Config) -> Vec<String> {
    let mut lines = vec!["Survey and Learn use these same history sources:".into()];
    for source in history_sources(cfg) {
        lines.push(format!(
            "{}: {} history file(s) · {} ({}){}",
            source.harness,
            source.files.len(),
            tilde(&source.path),
            if source.configured { "configured" } else { "detected" },
            if source.path.is_dir() { "" } else { " — folder not found" }
        ));
        if source.compressed > 0 {
            lines.push(format!(
                "{} compressed Codex file(s) cannot yet be read; only uncompressed rollouts are included.",
                source.compressed
            ));
        }
    }
    if cfg.learn_opencode_exports.is_none() {
        lines.push("OpenCode: Import OpenCode history, or choose an exports folder in Configure OpenCode history. Hook registration alone does not capture its session history.".into());
    }
    lines.push(
        "File counts are before parsing and repository filtering. Survey shows readable sessions for this repository."
            .into(),
    );
    lines
}

#[derive(serde::Serialize, serde::Deserialize)]
struct HookReceipt {
    root_hash: String,
    ts: u64,
}

fn receipt_path(paths: &adage_store::Paths, harness: &str) -> Option<PathBuf> {
    let name = match harness {
        "claude" | "claude-code" => "claude-code",
        "codex" => "codex",
        "opencode" => "opencode",
        _ => return None,
    };
    Some(paths.root.join("hook-receipts").join(format!("{name}.json")))
}

fn root_hash(root: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).to_string_lossy().as_bytes())
    )
}

fn receipts(path: &Path) -> Vec<HookReceipt> {
    if !std::fs::metadata(path).is_ok_and(|m| m.len() <= 8192) {
        return vec![];
    }
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// A tiny, bounded receipt independent of installed reflexes. No prompt, tool, session id or
/// repository path is kept. Never wait for another hook: health telemetry cannot delay guidance.
pub fn record_hook_receipt(paths: &adage_store::Paths, harness: &str, root: &Path) {
    let Some(path) = receipt_path(paths, harness) else { return };
    let Ok(Some(_lock)) = adage_store::lock::try_exclusive(&path.with_extension("lock")) else { return };
    let mut entries = receipts(&path);
    let hash = root_hash(root);
    entries.retain(|entry| entry.root_hash != hash);
    entries.push(HookReceipt { root_hash: hash, ts: crate::learnlog::now_ms() });
    if entries.len() > 16 {
        entries.drain(..entries.len() - 16);
    }
    if let Ok(bytes) = serde_json::to_vec(&entries) {
        let _ = adage_store::config::write_atomic(&path, &bytes);
    }
}

pub fn last_hook_receipt(paths: &adage_store::Paths, harness: &str, root: Option<&Path>) -> Option<u64> {
    let path = receipt_path(paths, harness)?;
    let hash = root.map(root_hash);
    receipts(&path).into_iter().filter(|e| hash.as_ref().is_none_or(|h| *h == e.root_hash)).map(|e| e.ts).max()
}

/// An executable named `name` on `PATH`, if any.
pub fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| executable(p))
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
            let (bin, dir) = match h {
                "claude-code" => ("claude", claude_dir()),
                "opencode" => ("opencode", opencode_dir()),
                _ => ("codex", codex_dir()),
            };
            let mut evidence = Vec::new();
            if let Some(p) = on_path(bin) {
                evidence.push(format!("`{bin}` on PATH ({})", tilde(&p)));
            }
            if let Some(d) = dir.filter(|d| d.is_dir()) {
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
    let mut settings: Vec<(&'static str, PathBuf)> = Vec::new();
    if let Some(d) = claude_dir() {
        settings.push(("claude-code", d.join("settings.json")));
        settings.push(("claude-code", d.join("settings.local.json")));
    }
    if let Some(d) = codex_dir() {
        settings.push(("codex", d.join("hooks.json")));
    }
    if let Some(r) = repo {
        settings.push(("claude-code", r.join(".claude/settings.json")));
        settings.push(("claude-code", r.join(".claude/settings.local.json")));
        settings.push(("codex", r.join(".codex/hooks.json")));
    }
    for (h, p) in settings {
        if let Some(doc) = read_json(&p) {
            for b in hook_binaries(&doc, h) {
                out.push(Registration { harness: h, location: p.clone(), binary: Some(b), kind: "settings" });
            }
        }
    }
    let mut plugins: Vec<PathBuf> = opencode_dir().map(|d| d.join("plugins/adage.js")).into_iter().collect();
    if let Some(r) = repo {
        plugins.push(r.join(".opencode/plugins/adage.js"));
    }
    for p in plugins {
        if let Some(bin) = opencode_binary(&p) {
            out.push(Registration { harness: "opencode", location: p, binary: bin, kind: "plugin" });
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
    fn hook_receipts_are_private_bounded_and_repository_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let paths = adage_store::Paths::at(temp.path());
        let root = temp.path().join("private-repository");
        assert_eq!(last_hook_receipt(&paths, "codex", Some(&root)), None);
        record_hook_receipt(&paths, "codex", &root);
        assert!(last_hook_receipt(&paths, "codex", Some(&root)).is_some());
        assert_eq!(last_hook_receipt(&paths, "codex", Some(&temp.path().join("other"))), None);
        assert_eq!(last_hook_receipt(&paths, "claude-code", Some(&root)), None);
        let path = receipt_path(&paths, "codex").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("private-repository"));
        for i in 0..30 {
            record_hook_receipt(&paths, "codex", &temp.path().join(i.to_string()));
        }
        assert_eq!(receipts(&path).len(), 16);
        assert_eq!(last_hook_receipt(&paths, "codex", Some(&root)), None);
        assert!(last_hook_receipt(&paths, "codex", None).is_some());
    }

    #[test]
    fn configured_history_never_falls_back_to_detected_homes() {
        let temp = tempfile::tempdir().unwrap();
        let paths = adage_store::Paths::at(temp.path());
        let cfg = adage_store::config::Config {
            learn_claude_projects: Some(temp.path().join("missing-claude")),
            learn_codex_sessions: Some(temp.path().join("missing-codex")),
            learn_opencode_exports: Some(temp.path().join("missing-opencode")),
            ..adage_store::config::load(&paths).config
        };
        let sources = history_sources(&cfg);
        assert_eq!(sources.len(), 3);
        assert!(sources.iter().all(|s| s.configured && s.files.is_empty() && s.path.starts_with(temp.path())));
    }

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
