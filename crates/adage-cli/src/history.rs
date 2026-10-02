//! Source selection shared by `adage condition`, its read-only preview `adage sense`, and
//! automatic learning: Claude Code history, Codex rollouts once Codex hooks are registered, and
//! configured OpenCode exports. Each caller then keeps one repository's traces (`autolearn`, and
//! the manual commands unless `--all`). No commands in history are executed.

use adage_store::config::Config;
use std::path::{Path, PathBuf};

pub fn claude_projects(cfg: &Config) -> Option<PathBuf> {
    cfg.learn_claude_projects.clone().or_else(|| crate::detect::claude_dir().map(|d| d.join("projects")))
}

/// Codex rollouts: the configured directory, or with `auto` (the default) `$CODEX_HOME/sessions`
/// (else `~/.codex/sessions`) once Codex hooks are registered, in user scope or for `root`.
/// Without them, Claude Code history alone (docs/design/everyday-ux.md §5.5).
pub fn codex_sessions(cfg: &Config, root: Option<&Path>) -> Option<PathBuf> {
    if let Some(d) = &cfg.learn_codex_sessions {
        return Some(d.clone());
    }
    let registered = cfg.learn_codex_auto && crate::detect::registrations(root).iter().any(|r| r.harness == "codex");
    registered.then(|| crate::detect::codex_dir().map(|d| d.join("sessions"))).flatten()
}

pub fn opencode_exports(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
}
