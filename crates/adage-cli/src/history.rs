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

/// One history source, as the conditioning preview names it.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub name: &'static str,
    /// The folder, with `~` for the home folder; `None` when none is set or detected.
    pub path: Option<String>,
    /// The folder exists.
    pub found: bool,
    /// What to do when there is no folder.
    pub hint: &'static str,
}

impl Source {
    /// `Claude Code: ~/.claude/projects`, as `adage ui`'s History sources lists it.
    pub fn line(&self) -> String {
        match &self.path {
            Some(p) => format!("{}: {p}{}", self.name, if self.found { "" } else { " (folder not found)" }),
            None => format!("{}: {}", self.name, self.hint),
        }
    }
}

/// The history sources conditioning reads for `root`'s repository.
pub fn sources(paths: &adage_store::Paths, root: Option<&Path>) -> Vec<Source> {
    let cfg = adage_store::config::load(paths).config;
    let source = |name: &'static str, path: Option<PathBuf>, hint: &'static str| Source {
        name,
        found: path.as_ref().is_some_and(|p| p.is_dir()),
        path: path.map(|p| crate::detect::tilde(&p)),
        hint,
    };
    vec![
        source("Claude Code", claude_projects(&cfg), "no history folder detected"),
        source("Codex", codex_sessions(&cfg, root), "connect hooks or choose a history folder"),
        source("OpenCode", cfg.learn_opencode_exports, "import local history or choose an exports folder"),
    ]
}

/// What only reading history ever does, said wherever sources are listed.
pub const LOCAL_ONLY: &str =
    "Only history recorded in this repository's checkouts is used. History commands are never executed.";

pub fn summary(paths: &adage_store::Paths, root: Option<&Path>) -> Vec<String> {
    let mut lines: Vec<String> = sources(paths, root).iter().map(Source::line).collect();
    lines.push(LOCAL_ONLY.into());
    lines
}

/// A discovery inventory before conditioning. Files are not deduplicated evidence or a promise
/// that the learner can use every decoded session.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub sources: Vec<Source>,
    /// Trace files found in the sources for this repository.
    pub files: usize,
    /// Files with a non-empty decoded session and no recorded Adage advice.
    pub usable: usize,
    /// Files with a non-empty decoded session that received Adage advice. Other work in the
    /// same session can still contribute evidence; the learner decides that per candidate.
    pub steered: usize,
    /// Files with no session in them, or that could not be read.
    pub unreadable: usize,
    /// No non-empty sessions were decoded and instincts are enabled.
    pub instincts_only: bool,
}

pub const COVERAGE_NOTE: &str = "Discovery includes supported agent and subagent histories.";
pub const INVENTORY_NOTE: &str = "File counts are not sessions used for learning.";
pub const ELIGIBILITY_NOTE: &str = "Conditioning filters evidence and removes duplicate sessions.";
pub const STEERED_NOTE: &str = "Steered traces can still contribute unsteered work.";

impl Preview {
    /// The preview as lines of text.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.sources.iter().map(Source::line).collect();
        lines.push(LOCAL_ONLY.into());
        lines.push(format!(
            "{} trace files found: {} unsteered, {} steered, {} empty or unreadable.",
            self.files, self.usable, self.steered, self.unreadable
        ));
        lines.extend([COVERAGE_NOTE.into(), INVENTORY_NOTE.into(), ELIGIBILITY_NOTE.into()]);
        if self.steered > 0 {
            lines.push(STEERED_NOTE.into());
        }
        if self.instincts_only {
            lines.push(
                "No non-empty history found. Conditioning can bind instincts from this repository's files.".into(),
            );
        }
        lines.push("Conditioning reads these sources locally. Counts can change if a session is still running.".into());
        lines
    }
}

pub fn preview(paths: &adage_store::Paths, root: &Path) -> anyhow::Result<Preview> {
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
    // Discovery is not an eligibility gate: a steered session may contain unrelated unsteered
    // work, and only the learner deduplicates and evaluates that evidence.
    let instincts_only = usable + steered == 0 && adage_store::config::load(paths).config.instincts_enabled;
    Ok(Preview {
        sources: sources(paths, Some(&root)),
        files: files.len(),
        usable,
        steered,
        unreadable,
        instincts_only,
    })
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

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
