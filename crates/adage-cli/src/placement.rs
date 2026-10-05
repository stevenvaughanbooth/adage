//! Where each harness keeps Adage's registration, on this machine and in a repository: the one
//! table `adage install` writes by and detection (`adage doctor`, `adage status`, the learner)
//! reads by, so the two can never disagree about a location.

use crate::detect::{claude_dir, codex_dir, opencode_dir};
use adage_trace::Harness;
use std::path::PathBuf;

/// How a harness registers Adage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A JSON settings file with a `hooks` table (Claude Code, Codex).
    Settings,
    /// A plugin source file (OpenCode).
    Plugin,
}

/// A harness's registration locations.
pub(crate) struct Placement {
    /// The harness's command, as found on `PATH`.
    pub command: &'static str,
    /// Its configuration directory on this machine (honouring its own environment variable).
    pub config_dir: fn() -> Option<PathBuf>,
    pub kind: Kind,
    /// The file `adage install` writes in user scope, relative to [`Placement::config_dir`].
    pub user_install: &'static str,
    /// Every user-scope file a registration may be in, relative to the configuration directory.
    pub user_files: &'static [&'static str],
    /// The file `adage install --project` writes, relative to the repository.
    pub project_install: &'static str,
    /// Every repository file a registration may be in.
    pub project_files: &'static [&'static str],
}

/// Every harness's placement: one arm per harness.
pub(crate) fn of(h: Harness) -> Placement {
    match h {
        Harness::ClaudeCode => Placement {
            command: "claude",
            config_dir: claude_dir,
            kind: Kind::Settings,
            user_install: "settings.json",
            user_files: &["settings.json", "settings.local.json"],
            project_install: ".claude/settings.local.json",
            project_files: &[".claude/settings.json", ".claude/settings.local.json"],
        },
        Harness::Codex => Placement {
            command: "codex",
            config_dir: codex_dir,
            kind: Kind::Settings,
            user_install: "hooks.json",
            user_files: &["hooks.json"],
            project_install: ".codex/hooks.json",
            project_files: &[".codex/hooks.json"],
        },
        Harness::OpenCode => Placement {
            command: "opencode",
            config_dir: opencode_dir,
            kind: Kind::Plugin,
            user_install: "plugins/adage.js",
            user_files: &["plugins/adage.js"],
            project_install: ".opencode/plugins/adage.js",
            project_files: &[".opencode/plugins/adage.js"],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What install writes is always among what detection reads.
    #[test]
    fn detection_reads_every_file_install_writes() {
        for h in Harness::ALL {
            let p = of(h);
            assert!(p.user_files.contains(&p.user_install), "{h}");
            assert!(p.project_files.contains(&p.project_install), "{h}");
        }
    }
}
