//! Source selection shared by explicit learning and its read-only preview. Automatic learning
//! additionally limits Codex auto-discovery to a registered/requesting harness and scopes every
//! source to the current repository (see `autolearn`). No commands in history are executed.

use adage_store::config::Config;
use std::path::PathBuf;

pub fn claude_projects(cfg: &Config) -> Option<PathBuf> {
    cfg.learn_claude_projects.clone().or_else(|| crate::detect::claude_dir().map(|d| d.join("projects")))
}

pub fn codex_sessions(cfg: &Config) -> Option<PathBuf> {
    cfg.learn_codex_sessions
        .clone()
        .or_else(|| cfg.learn_codex_auto.then(|| crate::detect::codex_dir().map(|d| d.join("sessions"))).flatten())
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
