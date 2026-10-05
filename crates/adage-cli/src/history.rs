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

/// Hook-named history is part of the default repository source set, including relocated traces.
/// Scope before parsing, and never prune the learner's queue from a read-only preview.
pub fn remembered(paths: &adage_store::Paths, root: &Path) -> Vec<PathBuf> {
    let repo = adage_core::repo::repo_key(root);
    let q = adage_hook::autolearn::Queue::new(paths, &repo);
    let mut files: Vec<PathBuf> =
        std::fs::read(q.sources()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if let Some(req) =
        std::fs::read(q.request()).ok().and_then(|b| serde_json::from_slice::<adage_hook::autolearn::Request>(&b).ok())
    {
        if let Some(path) = req.transcript_path {
            files.push(path.into());
        }
    }
    files.sort();
    files.dedup();
    adage_learn::scope::recorded_inside(files, &adage_learn::scope::checkouts(root))
}

pub fn summary(paths: &adage_store::Paths, root: Option<&Path>) -> Vec<String> {
    let cfg = adage_store::config::load(paths).config;
    let describe = |name: &str, path: Option<PathBuf>, absent: &str| match path {
        Some(p) => {
            format!("{name}: {}{}", crate::detect::tilde(&p), if p.is_dir() { "" } else { " (folder not found)" })
        }
        None => format!("{name}: {absent}"),
    };
    vec![
        describe("Claude Code", claude_projects(&cfg), "no history folder detected"),
        describe("Codex", codex_sessions(&cfg, root), "connect hooks or choose a history folder"),
        describe("OpenCode", cfg.learn_opencode_exports, "import local history or choose an exports folder"),
        "Only history recorded in this repository's checkouts is used. History commands are never executed.".into(),
    ]
}

pub fn preview(paths: &adage_store::Paths, root: &Path) -> anyhow::Result<Vec<String>> {
    let root = adage_core::repo::find_root(root).ok_or_else(|| anyhow::anyhow!("Choose a Git repository first."))?;
    let (files, _) =
        crate::sense::discover(&crate::sense::Options { root: Some(root.clone()), ..Default::default() }, paths);
    let table = adage_core::CommandTable::load();
    let (mut usable, mut steered, mut unreadable) = (0, 0, 0);
    for file in &files {
        match adage_learn::read_session(file, table, None) {
            Some(s) if !s.steps.is_empty() || !s.prompts.is_empty() => {
                if s.steered {
                    steered += 1;
                } else {
                    usable += 1;
                }
            }
            _ => unreadable += 1,
        }
    }
    let mut lines = summary(paths, Some(&root));
    lines.push(format!(
        "{} files found: {usable} unsteered sessions, {steered} already steered, {unreadable} empty or unreadable.",
        files.len()
    ));
    if usable == 0 {
        if adage_store::config::load(paths).config.instincts_enabled {
            lines.push(
                "No unsteered history yet. Conditioning can still bind instincts from this repository's files.".into(),
            );
        } else {
            anyhow::bail!("No unsteered history found and instincts are disabled. Configure a history source or enable instincts in Settings.");
        }
    }
    lines.push("Conditioning reads these sources locally. Counts can change if a session is still running.".into());
    Ok(lines)
}

pub fn set_source(paths: &adage_store::Paths, harness: &str, directory: &Path) -> anyhow::Result<Vec<String>> {
    let person = adage_app::person::require("config set history")?;
    let directory = adage_app::history::set_source(paths, harness, directory, &person)?;
    Ok(vec![
        format!("{harness} history configured: {}", crate::detect::tilde(&directory)),
        "Choose Condition now to preview this repository's sessions before confirming.".into(),
    ])
}

pub fn import_opencode(paths: &adage_store::Paths, root: &Path) -> anyhow::Result<Vec<String>> {
    let person = adage_app::person::require("config set history")?;
    let root = adage_core::repo::find_root(root).ok_or_else(|| anyhow::anyhow!("Choose a Git repository first."))?;
    let bin = crate::detect::on_path("opencode")
        .ok_or_else(|| anyhow::anyhow!("OpenCode was not found. Install it or choose an existing exports folder."))?;
    let result = adage_app::history::import_opencode(paths, &root, &bin, &person)?;
    Ok(vec![
        format!(
            "Imported {} local OpenCode sessions into {}.",
            result.count,
            crate::detect::tilde(&result.destination)
        ),
        "Choose Condition now to preview these sources before confirming.".into(),
    ])
}
